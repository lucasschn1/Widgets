//! Widget "Uso do Claude": quanto do limite diário (a sessão de 5 horas) e do semanal do Claude
//! já foi usado, em medidores de 3/4 de volta com a porcentagem no meio. O pequeno mostra só o
//! diário; o médio, os dois; o grande, os dois e os dias da semana em que mais se usa. Os números
//! vêm do Claude Code (ver `claude_usage`).
//!
//! No vidro, como os outros widgets, na fonte Outfit e com os medidores em laranja. O arco cresce
//! aos poucos quando o uso sobe; ao passar de 90%, fica vermelho e a bolinha da ponta dá um pulo.

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime};

use super::clawd::{self, Anim, Crab, Mood};
use super::{Ctx, Env, Hot, HoverStyle, Kind, MenuItem, Mode, Status, SMALL, WIDE};
use crate::claude_history::{self as history, HISTORY_DAYS};
use crate::claude_usage::{self as usage, Link, Usage, Window, FIVE_HOURS};
use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::{Rgba, FG, MUTED, WHITE};
use crate::win;

// ---------- cores ----------
/// o símbolo do Claude e o botão de conectar
pub const ACCENT: Rgba = Rgba::rgb(0xD4, 0x95, 0x5A);
/// os medidores, os ícones e o gráfico dos dias
pub const ORANGE: Rgba = Rgba::rgb(0xF2, 0x8C, 0x28);
/// avisos em texto ("Acaba às..."): claro o bastante para ler no vidro
const ALERT: Rgba = Rgba::rgb(255, 122, 92);
const TRACK: Rgba = Rgba(255, 255, 255, 30);

// ---------- ações ----------
pub const CONNECT: u32 = 1;
pub const DISCONNECT: u32 = 2;
const OPEN_USAGE: u32 = 3;
/// clique no Clawd: ele pula
const POKE_CRAB: u32 = 4;
/// sem resposta do Claude Code por este tempo (segundos), o Clawd tira um cochilo
const NAP_AFTER: f64 = 30.0 * 60.0;
const USAGE_URL: &str = "https://claude.ai/settings/usage";

/// Os medidores são 3/4 de volta, abertos embaixo: de 135° (embaixo à esquerda) a 405°, pelo alto.
const START: f32 = 135.0;
const SWEEP: f32 = 270.0;
/// A partir de quanto o medidor fica vermelho.
const HOT: f64 = 0.9;
/// Quanto dura o arco crescendo até o número novo, e a passagem do laranja para o vermelho.
const GROW_MS: f32 = 900.0;
const TURN_MS: f32 = 450.0;
/// O arco acima de 90%.
const RED: Rgba = Rgba::rgb(0xFF, 0x45, 0x3A);

// ---------- fontes: a Outfit, embutida no app ----------
/// a porcentagem
const NUM: Face = Face::Custom("Outfit", 300);
/// os textos
const TEXT: Face = Face::Custom("Outfit", 400);
/// os títulos e o botão
const BOLD: Face = Face::Custom("Outfit", 600);

/// De quanto em quanto tempo ler de novo o histórico de conversas (para os dias da semana).
const HISTORY_EVERY: Duration = Duration::from_secs(30 * 60);
/// Intervalo entre os quadros enquanto anima.
const FRAME: Duration = Duration::from_millis(16);
/// O tamanho dos quadradinhos do Clawd embaixo dos medidores, e a altura do centro dele (na
/// abertura de baixo dos anéis, onde ficava o ícone do diário).
const CRAB_UNIT: f32 = 1.2;
const CRAB_Y: f32 = 127.0;
/// sozinho embaixo de um medidor (no lugar do sol), há espaço para ele maior
const CRAB_UNIT_SOLO: f32 = 2.0;

/// O arco indo de `from` a `to` (uso de 0 a 1), desde `start`.
#[derive(Clone, Copy, Debug)]
struct Grow {
    from: f64,
    to: f64,
    start: Instant,
}

impl Grow {
    fn elapsed_ms(&self) -> f32 {
        self.start.elapsed().as_secs_f32() * 1000.0
    }

    /// O valor agora: sai rápido e chega devagar (curva cúbica).
    fn value(&self) -> f64 {
        let t = (self.elapsed_ms() / GROW_MS).clamp(0.0, 1.0) as f64;
        self.from + (self.to - self.from) * ease(t)
    }

    /// Há quantos milissegundos o arco passou de 90%, se ele subiu passando por aí e já passou.
    fn since_hot(&self) -> Option<f32> {
        if !(self.from < HOT && self.to >= HOT) {
            return None;
        }
        // quando a curva passa de 90%: o inverso de `ease`
        let p = (HOT - self.from) / (self.to - self.from);
        let cross = (1.0 - (1.0 - p).cbrt()) as f32 * GROW_MS;
        let ms = self.elapsed_ms() - cross;
        (ms >= 0.0).then_some(ms)
    }

    fn done(&self) -> bool {
        self.elapsed_ms() >= GROW_MS + TURN_MS
    }
}

/// Sai rápido e chega devagar.
fn ease(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// Os dois limites.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Limit {
    /// a sessão de 5 horas: o limite do dia a dia, com o sol
    Daily,
    /// a semana, com o calendário
    Weekly,
}

impl Limit {
    fn name(self) -> &'static str {
        match self {
            Limit::Daily => "Uso diário",
            Limit::Weekly => "Uso semanal",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Geometria dos medidores em cada tamanho: raio, espessura, tamanho do número, altura do
/// centro e onde fica cada medidor (x do centro).
struct Geom {
    r: f32,
    w: f32,
    num: f32,
    cy: f32,
    gauges: &'static [(Limit, f32)],
}

const WIDE_Q: f32 = WIDE / 4.0;
/// O título do grande (o Clawd e "Uso do Claude"): canto e tamanho da letra.
const BRAND_X: f32 = 22.0;
const BRAND_Y: f32 = 20.0;
const BRAND_SIZE: f32 = 14.0;

fn geom(mode: Mode) -> Geom {
    match mode {
        Mode::P => Geom { r: 55.0, w: 10.0, num: 38.0, cy: 79.0, gauges: &[(Limit::Daily, SMALL / 2.0)] },
        Mode::M => Geom { r: 55.0, w: 10.0, num: 38.0, cy: 79.0, gauges: &[(Limit::Daily, WIDE_Q), (Limit::Weekly, 3.0 * WIDE_Q)] },
        Mode::G => Geom { r: 57.0, w: 11.0, num: 40.0, cy: 132.0, gauges: &[(Limit::Daily, WIDE_Q), (Limit::Weekly, 3.0 * WIDE_Q)] },
    }
}

/// Botão "Conectar ao Claude Code" em cada tamanho: (x0, y0, x1, y1).
fn connect_rect(mode: Mode) -> (f32, f32, f32, f32) {
    match mode {
        Mode::P => (14.0, SMALL - 38.0, SMALL - 14.0, SMALL - 12.0),
        Mode::M => (172.0, 96.0, WIDE - 18.0, 124.0),
        Mode::G => (60.0, 360.0, WIDE - 60.0, 394.0),
    }
}

/// O que o widget mostra.
#[derive(Clone, Debug, PartialEq)]
struct View {
    usage: Option<Usage>,
    link: Link,
}

pub struct Claude {
    view: View,
    /// hora em que o arquivo de números mudou pela última vez (para ler de novo só se mudar)
    seen: Option<SystemTime>,
    /// mensagem de um conectar que não deu certo
    note: Option<String>,
    due: Instant,
    /// "agora" fixo nas imagens de exemplo
    fixed_now: Option<f64>,
    /// minuto do último desenho (os textos de tempo mudam com o relógio)
    minute: RefCell<i64>,
    /// o arco de cada limite (diário, semanal) crescendo até o número novo
    grow: [Option<Grow>; 2],
    /// o uso de cada dia da semana (segunda a domingo) nas últimas semanas; None: ainda lendo
    weekdays: Option<[f64; 7]>,
    /// a leitura do histórico em andamento, e quando foi a última
    reading: Option<Receiver<[f64; 7]>>,
    read_at: Option<Instant>,
    /// o Clawd, o caranguejo do Claude Code, no lugar do símbolo (com as animações dele)
    crab: Crab,
    /// o mouse sobre o widget (em pontos), para onde o Clawd olha, e se está em cima dele
    pointer: Option<(f32, f32)>,
    gaze: Option<(i32, i32)>,
    near_crab: bool,
    /// o humor do último desenho (mudou: desenha de novo)
    shown_mood: Mood,
}

impl Claude {
    pub fn new() -> Claude {
        let mut c = Claude {
            view: View { usage: None, link: Link::Off },
            seen: None,
            note: None,
            due: Instant::now(),
            fixed_now: None,
            minute: RefCell::new(0),
            grow: [None; 2],
            weekdays: None,
            reading: None,
            read_at: None,
            crab: Crab::new(),
            pointer: None,
            gaze: None,
            near_crab: false,
            shown_mood: Mood::Normal,
        };
        c.reload(true);
        c.read_history();
        c
    }

    /// Com números de exemplo (galeria e imagens): diário em 46% (reinicia em 2 h 15), semanal
    /// em 31% (reinicia daqui a 1 dia), e mais uso no meio da semana.
    pub fn sample() -> Claude {
        Claude::sample_with(0.46, 0.31, 2.25 * 3600.0)
    }

    /// Perto do limite: diário em 93% (já com o brilho dos 90%) com 2 h pela frente, semanal em 74%.
    #[cfg(test)]
    pub fn sample_limit() -> Claude {
        Claude::sample_with(0.93, 0.74, 2.0 * 3600.0)
    }

    /// No meio da animação: os dois arcos subindo de `from` para 92% (diário) e 64% (semanal),
    /// `ms` milissegundos depois de começar (para as imagens da animação).
    #[cfg(test)]
    pub fn sample_growing(from: f64, ms: u64) -> Claude {
        Claude::sample_growing_to(from, (0.92, 0.64), ms)
    }

    /// O mesmo, subindo até `to` (diário, semanal).
    #[cfg(test)]
    pub fn sample_growing_to(from: f64, to: (f64, f64), ms: u64) -> Claude {
        let mut c = Claude::sample_with(to.0, to.1, 2.0 * 3600.0);
        let start = Instant::now() - Duration::from_millis(ms);
        c.grow = [Some(Grow { from, to: to.0, start }), Some(Grow { from: from * 0.7, to: to.1, start })];
        c
    }

    /// Com os números, mas ainda lendo o histórico de conversas (o aviso no lugar das barras).
    #[cfg(test)]
    pub fn sample_reading() -> Claude {
        let mut c = Claude::sample();
        c.weekdays = None;
        c
    }

    /// O mesmo exemplo, com o mouse em (x, y) (o Clawd olhando para ele).
    #[cfg(test)]
    pub fn with_pointer(mut self, x: f32, y: f32, mode: Mode) -> Claude {
        Kind::pointer_moved(&mut self, x, y, mode);
        self
    }

    /// O mesmo exemplo, com a última resposta do Claude Code há `secs` segundos.
    #[cfg(test)]
    pub fn idle_for(mut self, secs: f64) -> Claude {
        if let Some(u) = &mut self.view.usage {
            u.updated = self.fixed_now.unwrap_or_else(win::now) - secs;
        }
        self
    }

    /// O mesmo exemplo, com o Clawd no meio de uma animação (`t` de 0 a 1).
    #[cfg(test)]
    pub fn with_crab(mut self, anim: clawd::Anim, t: f32) -> Claude {
        self.crab = Crab::playing(anim, t);
        self
    }

    /// Ainda sem ligação com o Claude Code (o botão de conectar).
    #[cfg(test)]
    pub fn sample_unlinked() -> Claude {
        let mut c = Claude::sample();
        c.view = View { usage: None, link: Link::Off };
        c
    }

    fn sample_with(session: f64, week: f64, session_left: f64) -> Claude {
        let now = win::now();
        let usage = Usage {
            five_hour: Some(Window { used: session, resets_at: now + session_left }),
            seven_day: Some(Window { used: week, resets_at: now + 0.98 * 86400.0 }),
            updated: now,
        };
        Claude {
            view: View { usage: Some(usage), link: Link::Connected },
            seen: None,
            note: None,
            due: Instant::now(),
            fixed_now: Some(now),
            minute: RefCell::new(0),
            grow: [None; 2],
            // mais uso de terça a quinta e quase nada no domingo
            weekdays: Some([4.4, 6.4, 7.6, 5.6, 3.6, 1.4, 0.6]),
            reading: None,
            read_at: None,
            crab: Crab::new(),
            pointer: None,
            gaze: None,
            near_crab: false,
            shown_mood: Mood::Normal,
        }
    }

    /// Começa a ler o histórico de conversas do Claude Code, num fio à parte (são centenas de
    /// megabytes); o resultado chega em `tick`.
    fn read_history(&mut self) {
        let Some(dir) = history::projects_dir() else {
            self.weekdays = Some([0.0; 7]);
            return;
        };
        let (tx, rx) = mpsc::channel();
        let (today, offset) = (history::today(), history::utc_offset());
        let started = std::thread::Builder::new().name("claude-history".into()).spawn(move || {
            let _ = tx.send(history::by_weekday(&dir, today, offset));
        });
        if started.is_ok() {
            self.reading = Some(rx);
            self.read_at = Some(Instant::now());
        }
    }

    /// Pega o histórico, se a leitura terminou; true se mudou.
    fn take_history(&mut self) -> bool {
        let Some(rx) = &self.reading else { return false };
        match rx.try_recv() {
            Ok(days) => {
                self.reading = None;
                let changed = self.weekdays != Some(days);
                self.weekdays = Some(days);
                changed
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.reading = None;
                false
            }
        }
    }

    fn now(&self) -> f64 {
        self.fixed_now.unwrap_or_else(win::now)
    }

    /// Lê de novo o arquivo de números (se mudou) e a ligação; true se algo mudou.
    fn reload(&mut self, force: bool) -> bool {
        let modified = std::fs::metadata(usage::data_file()).and_then(|m| m.modified()).ok();
        let mut view = self.view.clone();
        if force || modified != self.seen {
            self.seen = modified;
            view.usage = usage::load();
        }
        view.link = usage::link();
        if view.link == Link::Connected {
            self.note = None;
        }
        let changed = view != self.view;
        // o arco sai de onde está agora e cresce (ou desce) até o número novo
        let now = self.now();
        let before = [Limit::Daily, Limit::Weekly].map(|l| self.shown(l, now).unwrap_or(0.0));
        self.view = view;
        if changed && self.fixed_now.is_none() && win::animations_enabled() {
            for (l, from) in [Limit::Daily, Limit::Weekly].into_iter().zip(before) {
                let to = self.window(l).map_or(0.0, |w| w.used_at(now));
                if (to - from).abs() > 0.001 {
                    self.grow[l.index()] = Some(Grow { from, to, start: Instant::now() });
                }
            }
        }
        changed
    }

    /// O uso que o medidor mostra agora: o número, ou onde o arco está no caminho até ele.
    fn shown(&self, limit: Limit, now: f64) -> Option<f64> {
        let target = self.window(limit)?.used_at(now);
        Some(match self.grow[limit.index()] {
            Some(g) if g.elapsed_ms() < GROW_MS => g.value(),
            _ => target,
        })
    }

    fn animating(&self) -> bool {
        self.grow.iter().any(Option::is_some)
    }

    /// Como o Clawd está: dormindo sem números ou depois de um bom tempo sem o Claude Code
    /// responder (acorda com o mouse em cima), suando com o diário perto do limite.
    fn mood(&self) -> Mood {
        let now = self.now();
        let napping = self.view.usage.as_ref().is_none_or(|u| now - u.updated > NAP_AFTER);
        if self.pointer.is_none() && (!self.has_data() || napping) {
            Mood::Sleepy
        } else if self.session().is_some_and(|w| w.used_at(now) >= 0.85) {
            Mood::Worried
        } else {
            Mood::Normal
        }
    }

    /// Onde o Clawd fica em cada tamanho: (centro x, centro y, tamanho dos quadradinhos).
    /// No grande, no título; sem números no pequeno, grande no meio; com um medidor só, na
    /// abertura de baixo do anel (no lugar do ícone do diário); com dois, entre eles.
    fn crab_spot(&self, mode: Mode) -> (f32, f32, f32) {
        match mode {
            Mode::G => {
                let unit = BRAND_SIZE / 10.0;
                (BRAND_X + clawd::COLS * unit / 2.0, BRAND_Y + BRAND_SIZE * 0.55, unit)
            }
            Mode::P if self.needs_connect() && self.view.link == Link::Off => (SMALL / 2.0, 50.0, 3.0),
            Mode::P => (SMALL / 2.0, CRAB_Y, CRAB_UNIT_SOLO),
            Mode::M if !self.has_data() => (WIDE_Q, CRAB_Y, CRAB_UNIT_SOLO),
            Mode::M => (WIDE / 2.0, CRAB_Y, CRAB_UNIT),
        }
    }

    /// O Clawd está no lugar do ícone do diário (o sol) neste tamanho?
    fn crab_on_daily(&self, mode: Mode) -> bool {
        mode == Mode::P || (mode == Mode::M && !self.has_data())
    }

    /// O ponto (x, y) está em cima do Clawd (com uma folga em volta)?
    fn on_crab(&self, x: f32, y: f32, mode: Mode) -> bool {
        let (cx, cy, unit) = self.crab_spot(mode);
        (x - cx).abs() <= clawd::COLS * unit / 2.0 + 4.0 && (y - cy).abs() <= clawd::ROWS * unit / 2.0 + 4.0
    }

    /// Desenha o Clawd no lugar dele; devolve a largura.
    fn crab(&self, c: &mut Canvas, mode: Mode) -> f32 {
        let (cx, cy, unit) = self.crab_spot(mode);
        clawd::draw(c, cx, cy, unit, &self.crab.pose(self.mood(), self.gaze))
    }

    fn session(&self) -> Option<Window> {
        self.view.usage.as_ref().and_then(|u| u.five_hour)
    }

    fn week(&self) -> Option<Window> {
        self.view.usage.as_ref().and_then(|u| u.seven_day)
    }

    fn window(&self, limit: Limit) -> Option<Window> {
        match limit {
            Limit::Daily => self.session(),
            Limit::Weekly => self.week(),
        }
    }

    fn has_data(&self) -> bool {
        self.session().is_some() || self.week().is_some()
    }

    /// Mostra o botão de conectar (ainda não ligado e sem números).
    fn needs_connect(&self) -> bool {
        !self.has_data() && self.view.link != Link::Connected
    }

    /// Nota do diário: quando acaba (se acabar antes de reiniciar) ou quando reinicia.
    fn session_note(&self, now: f64) -> (String, bool) {
        let Some(w) = self.session() else { return ("Sem dados".into(), false) };
        if now >= w.resets_at {
            return ("Livre até o próximo uso".into(), false);
        }
        match w.runs_out_at(now, FIVE_HOURS) {
            Some(t) if t <= now => (format!("No limite · volta às {}", win::clock_at(w.resets_at)), true),
            Some(t) => (format!("Acaba às {}", win::clock_at(t)), true),
            None => (format!("Reinicia às {}", win::clock_at(w.resets_at)), false),
        }
    }

    fn week_note(&self, now: f64) -> String {
        match self.week() {
            None => "Sem dados".into(),
            Some(w) if now >= w.resets_at => "Renovado".into(),
            Some(w) => format!("Reinicia {}", day_and_time(w.resets_at, now)),
        }
    }

    /// A frase do ritmo do diário (e se é um aviso).
    fn forecast(&self, now: f64) -> Option<(String, bool)> {
        let w = self.session()?;
        if now >= w.resets_at {
            return None;
        }
        Some(match w.runs_out_at(now, FIVE_HOURS) {
            Some(t) if t <= now => (format!("Limite diário atingido · volta às {}", win::clock_at(w.resets_at)), true),
            Some(t) => (format!("No ritmo atual, o diário acaba às {}", win::clock_at(t)), true),
            None => (format!("No ritmo atual, o diário dura até reiniciar, às {}", win::clock_at(w.resets_at)), false),
        })
    }

    /// No alto à direita do grande: de onde vêm os números, ou há quanto tempo não chegam.
    fn source_line(&self, now: f64) -> String {
        match &self.view.usage {
            Some(u) if now - u.updated > 15.0 * 60.0 => format!("atualizado às {}", win::clock_at(u.updated)),
            _ => "Claude Code".into(),
        }
    }

    /// O uso em cada dia da semana (segunda a domingo), nas últimas semanas (zeros enquanto lê).
    fn weekdays(&self) -> [f64; 7] {
        self.weekdays.unwrap_or([0.0; 7])
    }

    // ---------- desenho ----------

    /// Um medidor: o trilho de 3/4 de volta, o arco até o uso com a bolinha na ponta, a
    /// porcentagem no meio, o nome embaixo dela e o ícone na abertura de baixo. A partir de 90%
    /// o arco fica vermelho; `since` é há quantos milissegundos ele acabou de passar de 90% (o
    /// laranja vira vermelho e a bolinha cresce e volta); None: já estava assim.
    fn gauge(c: &mut Canvas, g: &Geom, cx: f32, limit: Limit, v: Option<f64>, since: Option<f32>, icon: bool) {
        let (cy, r, w) = (g.cy, g.r, g.w);
        c.arc(cx, cy, r, START, START + SWEEP, TRACK, w);
        if let Some(v) = v {
            let frac = v.clamp(0.0, 1.0) as f32;
            let end = START + SWEEP * frac;
            let a = end.to_radians();
            let (dx, dy) = (cx + r * a.cos(), cy + r * a.sin());
            let mut color = ORANGE;
            let mut dot = w * 0.36;
            if v >= HOT {
                let k = since.map_or(1.0, |t| (t / TURN_MS).clamp(0.0, 1.0));
                color = ORANGE.mix(RED, k);
                c.fill_circle(dx, dy, w * 1.15, color.alpha(0.28));
                dot *= 1.0 + 0.55 * (k * std::f32::consts::PI).sin();
            }
            if frac > 0.0 {
                c.arc(cx, cy, r, START, end, color, w);
            }
            c.fill_circle(dx, dy + 0.8, dot * 1.17, Rgba(0, 0, 0, 70));
            c.fill_circle(dx, dy, dot, FG);
        }
        // o número com o nome embaixo, os dois juntos no meio do círculo
        let label = (g.num * 0.29).clamp(10.5, 12.0);
        let cap = g.num * 0.7;
        let base = cy + (cap - label - 8.0) / 2.0;
        match v {
            Some(v) => percent(c, cx, base, v, g.num),
            None => {
                c.text(cx, base, "–", NUM, g.num, FG.alpha(0.4), Anchor::MS, None);
            }
        }
        c.text(cx, base + 6.0, limit.name(), TEXT, label, MUTED, Anchor::MA, None);
        let (iy, ir) = (cy + r * 0.86, w * 0.9);
        match limit {
            _ if !icon => {} // o Clawd está no lugar do ícone
            Limit::Daily => sun(c, cx, iy, ir, ORANGE),
            Limit::Weekly => calendar(c, cx, iy, ir, ORANGE),
        }
    }

    fn draw_gauges(&self, c: &mut Canvas, mode: Mode, now: f64, only: Option<Limit>) {
        let g = geom(mode);
        for &(limit, cx) in g.gauges {
            if only.is_none_or(|o| o == limit) {
                let since = self.grow[limit.index()].and_then(|g| g.since_hot());
                let icon = !(limit == Limit::Daily && self.crab_on_daily(mode));
                Self::gauge(c, &g, cx, limit, self.shown(limit, now), since, icon);
            }
        }
    }

    /// O Clawd e o nome, no alto à esquerda (no grande).
    fn brand(&self, c: &mut Canvas, text: &str) {
        let w = self.crab(c, Mode::G);
        c.text(BRAND_X + w + 7.0, BRAND_Y, text, BOLD, BRAND_SIZE, FG, Anchor::LA, None);
    }

    fn draw_connect(&self, c: &mut Canvas, mode: Mode) {
        let (x0, y0, x1, y1) = connect_rect(mode);
        c.fill_round_rect(x0, y0, x1, y1, (y1 - y0) / 2.0, ACCENT);
        let label = if mode == Mode::G { "Conectar ao Claude Code" } else { "Conectar" };
        c.text((x0 + x1) / 2.0, (y0 + y1) / 2.0, label, BOLD, 11.5, WHITE, Anchor::MM, None);
    }

    /// O que dizer sem números: o que falta para eles aparecerem.
    fn waiting_text(&self) -> (&'static str, String) {
        if let Some(note) = &self.note {
            return ("Não deu para conectar", note.clone());
        }
        match self.view.link {
            Link::Connected => ("Aguardando o Claude Code", "Os números aparecem depois da próxima resposta dele.".into()),
            Link::Missing => ("Claude Code não encontrado", "Instale o Claude Code para ver o uso do seu plano.".into()),
            Link::Other => ("Já há outra linha de status", "O Claude Code já usa outra linha de status; tire-a para conectar.".into()),
            Link::Off => ("Uso do seu plano", "Limite diário e semanal, direto do Claude Code.".into()),
        }
    }

    fn draw_small(&self, c: &mut Canvas, now: f64) {
        if self.needs_connect() && self.view.link == Link::Off {
            // sem números: o símbolo, o nome e o botão
            self.crab(c, Mode::P);
            c.text(SMALL / 2.0, 82.0, "Uso do Claude", BOLD, 12.5, FG, Anchor::MA, None);
            self.draw_connect(c, Mode::P);
            return;
        }
        self.draw_gauges(c, Mode::P, now, Some(Limit::Daily));
        self.crab(c, Mode::P);
    }

    fn draw_medium(&self, c: &mut Canvas, now: f64) {
        self.crab(c, Mode::M);
        if !self.has_data() {
            self.draw_gauges(c, Mode::M, now, Some(Limit::Daily));
            let (x, w) = (172.0, WIDE - 18.0 - 172.0);
            let (title, text) = self.waiting_text();
            c.text(x, 26.0, title, BOLD, 12.5, FG, Anchor::LA, Some(w));
            let lines = c.wrap(&text, TEXT, 10.5, w, 3);
            for (i, l) in lines.iter().enumerate() {
                c.text(x, 46.0 + i as f32 * 14.0, l, TEXT, 10.5, MUTED, Anchor::LA, None);
            }
            if self.needs_connect() && self.view.link == Link::Off {
                self.draw_connect(c, Mode::M);
            }
            return;
        }
        self.draw_gauges(c, Mode::M, now, None);
    }

    fn draw_large(&self, c: &mut Canvas, now: f64) {
        let pad = 22.0;
        self.brand(c, "Uso do Claude");
        self.draw_gauges(c, Mode::G, now, None);
        let g = geom(Mode::G);
        if !self.has_data() {
            let (title, text) = self.waiting_text();
            let top = 262.0;
            c.text(WIDE / 2.0, top, title, BOLD, 13.0, FG, Anchor::MA, Some(WIDE - 2.0 * pad));
            for (i, l) in c.wrap(&text, TEXT, 11.0, WIDE - 2.0 * pad, 3).iter().enumerate() {
                c.text(WIDE / 2.0, top + 22.0 + i as f32 * 15.0, l, TEXT, 11.0, MUTED, Anchor::MA, None);
            }
            if self.needs_connect() && self.view.link == Link::Off {
                self.draw_connect(c, Mode::G);
            }
            return;
        }
        c.text(WIDE - pad, 22.0, &self.source_line(now), TEXT, 10.5, MUTED, Anchor::RA, Some(WIDE / 2.0));
        // embaixo de cada medidor, quando ele reinicia
        let (s_note, s_warn) = self.session_note(now);
        for (&(_, cx), (note, warn)) in g.gauges.iter().zip([(s_note, s_warn), (self.week_note(now), false)]) {
            c.text(cx, 202.0, &note, TEXT, 10.5, if warn { ALERT } else { MUTED }, Anchor::MA, Some(WIDE / 2.0 - 16.0));
        }
        c.fill_round_rect(pad, 232.0, WIDE - pad, 233.0, 0.5, WHITE.alpha(0.1));
        self.draw_weekdays(c, pad, 248.0, WIDE - pad, 394.0);
        if let Some((text, warn)) = self.forecast(now) {
            c.text(WIDE / 2.0, 416.0, &text, TEXT, 10.5, if warn { ALERT } else { MUTED }, Anchor::MA, Some(WIDE - 2.0 * pad));
        }
    }

    /// Os dias da semana em que mais se usa: uma barra por dia (o de mais uso em laranja cheio,
    /// com a parte dele no total em cima) e o dia de hoje com o nome em branco.
    fn draw_weekdays(&self, c: &mut Canvas, x0: f32, y0: f32, x1: f32, y1: f32) {
        const NAMES: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
        let days = self.weekdays();
        let total: f64 = days.iter().sum();
        let top = (0..7).max_by(|&a, &b| days[a].total_cmp(&days[b])).filter(|_| total > 0.0);
        let caption = format!("últimas {} semanas", HISTORY_DAYS / 7);
        let cw = c.text_width(&caption, TEXT, 10.0);
        c.text(x1, y0 + 1.5, &caption, TEXT, 10.0, MUTED, Anchor::RA, None);
        c.text(x0, y0, "Dias em que você mais usa", BOLD, 12.0, FG, Anchor::LA, Some(x1 - x0 - cw - 10.0));
        let today = win::local_time().weekday as usize;
        let max = days.iter().cloned().fold(0.0, f64::max);
        let (bars_top, base) = (y0 + 40.0, y1 - 20.0);
        let slot = (x1 - x0) / 7.0;
        let bw = 20.0;
        for (i, name) in NAMES.iter().enumerate() {
            let cx = x0 + slot * (i as f32 + 0.5);
            let (bx0, bx1) = (cx - bw / 2.0, cx + bw / 2.0);
            c.fill_round_rect(bx0, bars_top, bx1, base, 6.0, WHITE.alpha(0.06));
            if max > 0.0 && days[i] > 0.0 {
                let h = ((base - bars_top) * (days[i] / max) as f32).max(12.0);
                let color = if Some(i) == top { ORANGE } else { ORANGE.mix(WHITE, 0.2).alpha(0.5) };
                c.fill_round_rect(bx0, base - h, bx1, base, 6.0, color);
                if Some(i) == top {
                    let share = format!("{}%", (days[i] / total * 100.0).round());
                    c.text(cx, base - h - 6.0, &share, TEXT, 10.5, FG, Anchor::MS, None);
                }
            }
            c.text(cx, base + 7.0, name, TEXT, 10.0, if i == today { FG } else { MUTED }, Anchor::MA, None);
        }
        if top.is_none() {
            let note = if self.weekdays.is_none() { "Lendo as conversas do Claude Code…" } else { "Sem uso do Claude Code nestas semanas" };
            let mid = (bars_top + base) / 2.0;
            let nw = c.text_width(note, TEXT, 10.5) + 24.0;
            c.fill_round_rect((x0 + x1 - nw) / 2.0, mid - 12.0, (x0 + x1 + nw) / 2.0, mid + 12.0, 12.0, Rgba(0, 0, 0, 60));
            c.text((x0 + x1) / 2.0, mid, note, TEXT, 10.5, FG.alpha(0.85), Anchor::MM, None);
        }
    }
}

impl Default for Claude {
    fn default() -> Self {
        Claude::new()
    }
}

/// A porcentagem no meio do medidor: o número (na fonte dos números grandes dos outros widgets)
/// centrado em `x` e apoiado na linha de base `y`, e o "%" menor logo depois dele.
fn percent(c: &mut Canvas, x: f32, y: f32, v: f64, size: f32) {
    let num = format!("{}", (v * 100.0).round());
    let nw = c.text_width(&num, NUM, size);
    c.text(x - nw / 2.0, y, &num, NUM, size, FG, Anchor::LS, None);
    c.text(x + nw / 2.0 + 1.0, y, "%", NUM, size * 0.42, FG.alpha(0.7), Anchor::LS, None);
}

/// Sol: um círculo e oito raios, cabendo num círculo de raio `r`.
fn sun(c: &mut Canvas, cx: f32, cy: f32, r: f32, color: Rgba) {
    c.fill_circle(cx, cy, r * 0.42, color);
    for i in 0..8 {
        let a = (i as f32 * 45.0).to_radians();
        let (dx, dy) = (a.cos(), a.sin());
        c.line(&[(cx + dx * r * 0.68, cy + dy * r * 0.68), (cx + dx * r, cy + dy * r)], color, r * 0.17);
    }
}

/// Calendário: a folha com a faixa de cima cheia, as duas argolas e os dias em pontos, cabendo
/// num quadrado de lado `2r`.
fn calendar(c: &mut Canvas, cx: f32, cy: f32, r: f32, color: Rgba) {
    let (x0, y0, x1, y1) = (cx - r * 0.95, cy - r * 0.78, cx + r * 0.95, cy + r * 0.95);
    let lw = r * 0.16;
    c.stroke_round_rect(x0, y0, x1, y1, r * 0.28, color, lw);
    c.fill_round_rect(x0, y0, x1, y0 + r * 0.5, r * 0.28, color);
    c.fill_round_rect(x0 + lw / 2.0, y0 + r * 0.3, x1 - lw / 2.0, y0 + r * 0.5, 0.0, color);
    for x in [cx - r * 0.45, cx + r * 0.45] {
        c.line(&[(x, cy - r), (x, cy - r * 0.62)], color, r * 0.2);
    }
    let d = r * 0.11;
    for row in 0..2 {
        for col in 0..3 {
            let (px, py) = (cx + (col as f32 - 1.0) * r * 0.5, cy + r * 0.12 + row as f32 * r * 0.45);
            c.fill_circle(px, py, d, color);
        }
    }
}

/// O centro (x) do medidor no ponto (x, y), se houver um ali. O pequeno só mostra o diário.
fn gauge_at(x: f32, y: f32, mode: Mode) -> Option<f32> {
    let g = geom(mode);
    g.gauges.iter().map(|&(_, cx)| cx).find(|&cx| (x - cx).hypot(y - g.cy) <= g.r + g.w)
}

/// "seg., 09:00", "amanhã, 09:00" ou "hoje, 18:40" para um horário (segundos desde 1970).
fn day_and_time(epoch: f64, now: f64) -> String {
    let t = win::local_time();
    let today = t.hour as f64 * 3600.0 + t.minute as f64 * 60.0 + t.second as f64;
    let days = ((today + (epoch - now)) / 86400.0).floor() as i64;
    let day = match days {
        0 => "hoje".to_string(),
        1 => "amanhã".to_string(),
        d => ["seg.", "ter.", "qua.", "qui.", "sex.", "sáb.", "dom."][(t.weekday as i64 + d).rem_euclid(7) as usize].to_string(),
    };
    format!("{day}, {}", win::clock_at(epoch))
}

impl Kind for Claude {
    fn key(&self) -> &'static str {
        "claude"
    }
    fn title(&self) -> &'static str {
        "Uso do Claude"
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, _env: &Env) {
        let now = self.now();
        *self.minute.borrow_mut() = (now / 60.0) as i64;
        match mode {
            Mode::P => self.draw_small(c, now),
            Mode::M => self.draw_medium(c, now),
            Mode::G => self.draw_large(c, now),
        }
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        if self.on_crab(x, y, mode) {
            return Some(POKE_CRAB);
        }
        if self.needs_connect() && self.view.link == Link::Off {
            let (x0, y0, x1, y1) = connect_rect(mode);
            if (x0..=x1).contains(&x) && (y0..=y1).contains(&y) {
                return Some(CONNECT);
            }
            return None;
        }
        (self.has_data() && gauge_at(x, y, mode).is_some() && env.shortcuts()).then_some(OPEN_USAGE)
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<Hot> {
        match self.hit(x, y, mode, env)? {
            CONNECT => {
                let (x0, y0, x1, y1) = connect_rect(mode);
                Some((x0, y0, x1, y1, (y1 - y0) / 2.0))
            }
            POKE_CRAB => {
                let (cx, cy, unit) = self.crab_spot(mode);
                let (hw, hh) = (clawd::COLS * unit / 2.0 + 4.0, clawd::ROWS * unit / 2.0 + 4.0);
                Some((cx - hw, cy - hh, cx + hw, cy + hh, 6.0))
            }
            _ => {
                let g = geom(mode);
                let cx = gauge_at(x, y, mode)?;
                let r = g.r + g.w;
                Some((cx - r, g.cy - r, cx + r, g.cy + r, r))
            }
        }
    }

    fn hover_style(&self, x: f32, y: f32, mode: Mode, env: &Env) -> HoverStyle {
        if matches!(self.hit(x, y, mode, env), Some(OPEN_USAGE | POKE_CRAB)) { HoverStyle::Quiet } else { HoverStyle::Surface }
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            CONNECT => {
                self.note = usage::connect().err();
                self.reload(true);
            }
            DISCONNECT => {
                self.note = usage::disconnect().err();
                self.reload(true);
            }
            OPEN_USAGE => ctx.open(USAGE_URL),
            POKE_CRAB => self.crab.play(Anim::Hop),
            _ => {}
        }
    }

    fn menu(&self, _env: &Env) -> Vec<MenuItem> {
        let mut items = vec![MenuItem::Cmd("Ver uso no claude.ai".into(), OPEN_USAGE)];
        match self.view.link {
            Link::Connected => items.push(MenuItem::Cmd("Desconectar do Claude Code".into(), DISCONNECT)),
            Link::Off => items.push(MenuItem::Cmd("Conectar ao Claude Code".into(), CONNECT)),
            _ => {}
        }
        items
    }

    fn next_tick(&self) -> Option<Instant> {
        // enquanto o arco anima, um quadro a cada 16 ms; o Clawd, nos quadros dele
        let next = if self.animating() { self.due.min(Instant::now() + FRAME) } else { self.due };
        Some(next.min(self.crab.next_tick()))
    }

    fn pointer_entered(&mut self) -> bool {
        // o Clawd reage ao mouse (os quadros vêm pelo tick)
        let mood = self.mood();
        self.crab.poke(mood);
        false
    }

    fn pointer_moved(&mut self, x: f32, y: f32, mode: Mode) -> bool {
        // acabou de chegar: ele acorda (se cochilava) e passa a olhar para o mouse
        let mut redraw = self.pointer.is_none();
        self.pointer = Some((x, y));
        let (cx, cy, _) = self.crab_spot(mode);
        let gaze = clawd::gaze(cx, cy, x, y);
        if self.gaze != Some(gaze) {
            self.gaze = Some(gaze);
            redraw = true;
        }
        // o mouse chegou em cima dele: acena
        let near = self.on_crab(x, y, mode);
        if near && !self.near_crab && !self.crab.animating() {
            self.crab.play(Anim::Wave);
            redraw = true;
        }
        self.near_crab = near;
        redraw
    }

    fn pointer_left(&mut self) -> bool {
        self.pointer = None;
        self.gaze = None;
        self.near_crab = false;
        true
    }

    fn tick(&mut self, _ctx: &mut Ctx) -> bool {
        // mais um quadro da animação (e um último, já parado, depois que ela acaba)
        let mut redraw = self.animating();
        for g in &mut self.grow {
            if g.is_some_and(|g| g.done()) {
                *g = None;
            }
        }
        redraw |= self.take_history();
        let mood = self.mood();
        redraw |= self.crab.tick(mood);
        // dormiu ou acordou (cochilo depois de um tempo sem o Claude Code, ou o mouse)
        if mood != self.shown_mood {
            self.shown_mood = mood;
            redraw = true;
        }
        if self.reading.is_none() && self.read_at.is_some_and(|t| t.elapsed() >= HISTORY_EVERY) {
            self.read_history();
        }
        if Instant::now() >= self.due {
            self.due = Instant::now() + Duration::from_secs(5);
            redraw |= self.reload(false);
            // os textos de tempo ("acaba às", "atualizado às") mudam com o relógio
            redraw |= (self.now() / 60.0) as i64 != *self.minute.borrow();
        }
        redraw
    }

    fn status(&self) -> Status {
        Status { loading: false, error: None, updated: self.view.usage.as_ref().map(|u| u.updated) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> serde_json::Value {
        serde_json::json!({"claude": {}, "app": {}})
    }

    #[test]
    fn crab_has_room_in_every_size() {
        // o Clawd (com folga para pular e para os "z") cabe no cartão e não encosta nos anéis,
        // nos ícones nem no botão de conectar
        for (name, w) in [("com números", Claude::sample()), ("sem conectar", Claude::sample_unlinked())] {
            for mode in Mode::ALL {
                let l = super::super::Layout::of(mode);
                let (cx, cy, unit) = w.crab_spot(mode);
                let (hw, hh) = (clawd::COLS * unit / 2.0, clawd::ROWS * unit / 2.0);
                let (x0, y0, x1, y1) = (cx - hw, cy - hh - 3.0 * unit, cx + hw, cy + hh);
                assert!(x0 >= 6.0 && y0 >= 6.0 && x1 <= l.w - 6.0 && y1 <= l.h - 6.0, "{name} {mode:?}: fora do cartão");
                if mode == Mode::G || (mode == Mode::P && w.needs_connect()) {
                    continue; // no título do grande e sozinho no meio do pequeno
                }
                let g = geom(mode);
                for &(limit, gx) in g.gauges {
                    if !w.has_data() && mode == Mode::M && limit == Limit::Weekly {
                        continue; // sem números, o médio mostra só o diário
                    }
                    // nenhum canto do Clawd em cima do trilho do anel (fora do vão de baixo)
                    for (px, py) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1), (cx, y0)] {
                        let d = (px - gx).hypot(py - g.cy);
                        let angle = (py - g.cy).atan2(px - gx).to_degrees().rem_euclid(360.0);
                        let in_gap = (45.0..=135.0).contains(&angle);
                        assert!(in_gap || d < g.r - g.w || d > g.r + g.w, "{name} {mode:?}: em cima do anel em ({px}, {py})");
                    }
                    // nem em cima do ícone que ficou (sol ou calendário)
                    let icon_here = !(limit == Limit::Daily && w.crab_on_daily(mode));
                    if icon_here {
                        let (ix, iy, ir) = (gx, g.cy + g.r * 0.86, g.w * 0.9 + 3.0);
                        assert!(x1 < ix - ir || x0 > ix + ir || y1 < iy - ir || y0 > iy + ir, "{name} {mode:?}: em cima do ícone");
                    }
                }
                if w.needs_connect() {
                    let (bx0, by0, bx1, by1) = connect_rect(mode);
                    assert!(x1 < bx0 || x0 > bx1 || y1 < by0 || y0 > by1, "{name} {mode:?}: em cima do botão");
                }
            }
        }
    }

    #[test]
    fn crab_naps_when_claude_code_is_quiet() {
        assert_eq!(Claude::sample().mood(), Mood::Normal);
        assert_eq!(Claude::sample().idle_for(10.0 * 60.0).mood(), Mood::Normal, "10 min sem usar: acordado");
        assert_eq!(Claude::sample().idle_for(NAP_AFTER + 60.0).mood(), Mood::Sleepy, "muito tempo sem usar: cochila");
        assert_eq!(Claude::sample_unlinked().mood(), Mood::Sleepy, "sem números: dorme");
        assert_eq!(Claude::sample_limit().mood(), Mood::Worried);
        // o mouse em cima acorda
        let awake = Claude::sample().idle_for(NAP_AFTER + 60.0).with_pointer(40.0, 40.0, Mode::P);
        assert_eq!(awake.mood(), Mood::Normal);
        // e o widget se redesenha quando ele dorme (o tick nota a mudança)
        let mut w = Claude::sample().idle_for(NAP_AFTER + 60.0);
        w.shown_mood = Mood::Normal;
        let mut req = Vec::new();
        let (mut c, mut cache) = (cfg(), serde_json::json!({}));
        let redraw = w.tick(&mut Ctx { cfg: &mut c, cache: &mut cache, key: "claude", out: &mut req });
        assert!(redraw && w.shown_mood == Mood::Sleepy);
    }

    #[test]
    fn crab_follows_the_mouse() {
        for mode in Mode::ALL {
            let mut w = Claude::sample();
            let (cx, cy, _) = w.crab_spot(mode);
            // chegou: olha para o mouse (à esquerda e acima)
            assert!(Kind::pointer_moved(&mut w, 2.0, 2.0, mode), "{mode:?}");
            assert_eq!(w.gaze, Some((-1, -1)));
            // mexer sem mudar a direção do olhar não redesenha
            assert!(!Kind::pointer_moved(&mut w, 3.0, 3.0, mode), "{mode:?}: redesenhou à toa");
            // do outro lado: olha para lá
            assert!(Kind::pointer_moved(&mut w, cx + 60.0, cy, mode));
            assert_eq!(w.gaze, Some((1, 0)));
            // em cima dele: acena
            assert!(Kind::pointer_moved(&mut w, cx, cy, mode));
            assert!(w.crab.animating(), "{mode:?}: não acenou");
            // saiu: volta a olhar para a frente
            assert!(Kind::pointer_left(&mut w));
            assert_eq!(w.gaze, None);
        }
    }

    #[test]
    fn clicking_the_crab_makes_it_hop() {
        let c = cfg();
        let env = Env { cfg: &c, key: "claude" };
        for (name, sample) in [("com números", Claude::sample as fn() -> Claude), ("sem conectar", Claude::sample_unlinked)] {
            for mode in Mode::ALL {
                let mut w = sample();
                let (cx, cy, _) = w.crab_spot(mode);
                // o clique nele vale mais que o do medidor embaixo
                assert_eq!(Kind::hit(&w, cx, cy, mode, &env), Some(POKE_CRAB), "{name} {mode:?}");
                let mut req = Vec::new();
                let (mut c2, mut cache) = (cfg(), serde_json::json!({}));
                Kind::act(&mut w, POKE_CRAB, &mut Ctx { cfg: &mut c2, cache: &mut cache, key: "claude", out: &mut req });
                assert!(w.crab.animating(), "{name} {mode:?}: não pulou");
            }
        }
    }

    #[test]
    fn critical_limit_and_notes() {
        let c = Claude::sample();
        let now = c.now();
        assert_eq!(c.window(Limit::Daily).map(|w| w.used_at(now)), Some(0.46));
        assert_eq!(c.window(Limit::Weekly).map(|w| w.used_at(now)), Some(0.31));
        let (note, warn) = c.session_note(now);
        assert!(note.starts_with("Reinicia às "), "{note}");
        assert!(!warn);
        assert!(c.week_note(now).starts_with("Reinicia amanhã, ") || c.week_note(now).starts_with("Reinicia hoje, "), "{}", c.week_note(now));
        let (f, warn) = c.forecast(now).unwrap();
        assert!(f.starts_with("No ritmo atual, o diário dura até reiniciar"), "{f}");
        assert!(!warn);
        // perto do limite: o ritmo acaba antes do reinício
        let c = Claude::sample_limit();
        let (note, warn) = c.session_note(now);
        assert!(note.starts_with("Acaba às ") && warn, "{note}");
        // o exemplo usa mais no meio da semana: quarta é o dia de mais uso
        let days = c.weekdays();
        let top = (0..7).max_by(|&a, &b| days[a].total_cmp(&days[b])).unwrap();
        assert_eq!(top, 2, "{days:?}");
    }

    #[test]
    fn day_names() {
        let now = win::now();
        assert!(day_and_time(now + 60.0, now).starts_with("hoje") || day_and_time(now + 60.0, now).starts_with("amanhã"));
        let in3 = day_and_time(now + 3.0 * 86400.0, now);
        assert!(["seg.", "ter.", "qua.", "qui.", "sex.", "sáb.", "dom."].iter().any(|d| in3.starts_with(d)), "{in3}");
    }

    #[test]
    fn connect_button_only_without_data() {
        let mut c = Claude::sample();
        assert!(!c.needs_connect());
        c.view = View { usage: None, link: Link::Off };
        assert!(c.needs_connect());
        let cfg = serde_json::json!({"claude": {}, "app": {}});
        let env = Env { cfg: &cfg, key: "claude" };
        for mode in Mode::ALL {
            let (x0, y0, x1, y1) = connect_rect(mode);
            assert_eq!(c.hit((x0 + x1) / 2.0, (y0 + y1) / 2.0, mode, &env), Some(CONNECT), "{mode:?}");
            let l = super::super::Layout::of(mode);
            assert!(x1 <= l.w && y1 <= l.h, "{mode:?}");
        }
        c.view.link = Link::Connected;
        assert!(!c.needs_connect(), "conectado: só espera os números");
    }

    #[test]
    fn the_arc_grows_and_pulses_past_90() {
        let start = Instant::now();
        let g = Grow { from: 0.5, to: 0.95, start };
        assert!((g.value() - 0.5).abs() < 0.01, "começa de onde estava");
        assert_eq!(g.since_hot(), None, "o vermelho só vem quando passa de 90%");
        let g = Grow { from: 0.5, to: 0.95, start: start - Duration::from_millis(GROW_MS as u64) };
        assert!((g.value() - 0.95).abs() < 1e-9 && !g.done());
        assert!(g.since_hot().is_some());
        let g = Grow { from: 0.5, to: 0.95, start: start - Duration::from_millis((GROW_MS + TURN_MS) as u64 + 10) };
        assert!(g.done());
        // subir sem chegar a 90%, ou descer: sem pulso
        let mid = start - Duration::from_millis(GROW_MS as u64);
        assert_eq!(Grow { from: 0.2, to: 0.6, start: mid }.since_hot(), None);
        assert_eq!(Grow { from: 0.95, to: 0.3, start: mid }.since_hot(), None);
        // a curva passa de 90% no momento calculado
        let p = (0.9 - 0.5) / (0.95 - 0.5);
        let cross = 1.0 - (1.0f64 - p).cbrt();
        assert!((0.5 + 0.45 * ease(cross) - 0.9).abs() < 1e-9);
    }

    #[test]
    fn gauges_fit_and_answer_clicks() {
        for mode in Mode::ALL {
            let g = geom(mode);
            let l = super::super::Layout::of(mode);
            for &(_, cx) in g.gauges {
                let r = g.r + g.w / 2.0;
                assert!(cx - r >= 0.0 && cx + r <= l.w && g.cy - r >= 0.0 && g.cy + r <= l.h, "{mode:?}");
                assert_eq!(gauge_at(cx, g.cy - g.r, mode), Some(cx));
            }
        }
        assert_eq!(geom(Mode::P).gauges.len(), 1, "o pequeno só tem o diário");
        assert_eq!(gauge_at(2.0, 2.0, Mode::M), None);
    }
}
