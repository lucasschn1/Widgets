//! Widget "Uso do Claude": quanto do limite diário (a sessão de 5 horas) e do semanal do Claude
//! já foi usado, em medidores de 3/4 de volta com a porcentagem no meio. O pequeno mostra só o
//! diário; o médio, os dois; o grande, os dois e os dias da semana em que mais se usa. Os números
//! vêm do Claude Code (ver `claude_usage`).
//!
//! No vidro, como os outros widgets, na fonte Outfit e com os medidores em laranja. O arco cresce
//! aos poucos quando o uso sobe; ao passar de 90%, fica vermelho e a bolinha da ponta dá um pulo.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime};

use super::clawd::{self, Anim, Crab, Mood};
use super::{Ctx, Env, Hot, HoverStyle, Kind, MenuItem, Mode, Request, Status, SMALL, WIDE};
use crate::claude_history::{self as history, Tool, Turn, HISTORY_DAYS};
use crate::claude_usage::{self as usage, Link, Usage, Window, FIVE_HOURS};
use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::{ease_in_out, Rgba, FG, MUTED, WHITE};
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
/// clique no Clawd: ele vai para a barra de tarefas (ou volta de lá)
const POKE_CRAB: u32 = 4;
/// pelo clique direito: mandar o Clawd para a barra de tarefas, ou trazer de volta
const PERCH_TOGGLE: u32 = 5;
/// sem resposta do Claude Code por este tempo (segundos), o Clawd tira um cochilo
const NAP_AFTER: f64 = 30.0 * 60.0;
/// sem saber pelas conversas: a linha de status foi chamada há menos que isto (segundos), o
/// Claude Code está trabalhando, e o Clawd escreve no caderno (ela é chamada a cada mensagem nova)
const BUSY_FOR: f64 = 60.0;
/// uma conversa no meio de um trabalho, mas parada há tanto tempo (segundos): fechou, travou ou
/// espera uma permissão; não conta como trabalhando
const STALE_AFTER: f64 = 10.0 * 60.0;
/// de quanto em quanto tempo olhar os números e as conversas (mais vezes trabalhando, para
/// comemorar logo que o Claude Code termina)
const CHECK_EVERY: Duration = Duration::from_secs(5);
const CHECK_BUSY: Duration = Duration::from_secs(2);

/// O que as conversas abertas do Claude Code estão fazendo.
#[derive(Clone, Debug, Default, PartialEq)]
struct Activity {
    /// alguma está trabalhando (None: não deu para saber pelas conversas)
    busy: Option<bool>,
    /// passos por minuto, somando todas
    steps: u32,
    /// a última ferramenta chamada na conversa trabalhando que mudou por último
    tool: Option<Tool>,
}
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
const RED: Rgba = crate::theme::RED;

// ---------- fontes: a Outfit, embutida no app ----------
/// a porcentagem
const NUM: Face = Face::Custom("Outfit", 300);
/// os textos
const TEXT: Face = Face::Custom("Outfit", 400);
/// os títulos e o botão
const BOLD: Face = Face::Custom("Outfit", 600);

/// No médio, quantos quadradinhos o Clawd fica à esquerda do meio com o notebook de lado (ou a
/// plaquinha) à direita, e quanto tempo leva andando até lá (ou de volta).
const MID_SHIFT: f32 = 5.0;
const SLIDE_MS: f32 = 600.0;

/// A partir de quanto do diário o limite foi atingido (o Clawd põe a plaquinha e vai ler).
const LIMIT_AT: f64 = 0.995;

/// A metade da largura e da altura da grade do Clawd, num lugar de `unit` pontos.
fn crab_half(unit: f32) -> (f32, f32) {
    (clawd::COLS * clawd::CELL * unit / 2.0, clawd::ROWS * clawd::CELL * unit / 2.0)
}

/// De onde o Clawd é visto trabalhando em cada tamanho: no pequeno, o notebook de frente, no
/// colo (de lado não caberia entre as pontas do anel); no médio e no grande, de lado, com a tela
/// à mostra.
fn view_of(mode: Mode) -> clawd::View {
    if mode == Mode::P { clawd::View::Front } else { clawd::View::Side }
}

/// De quanto em quanto tempo ler de novo o histórico de conversas (para os dias da semana).
const HISTORY_EVERY: Duration = Duration::from_secs(30 * 60);
/// Intervalo entre os quadros enquanto anima.
const FRAME: Duration = Duration::from_millis(16);
/// A altura do centro do Clawd sozinho embaixo de um medidor (na abertura de baixo do anel, no
/// lugar do sol) e o tamanho dos quadradinhos dele aí (o `unit` de `clawd::draw`). No pequeno,
/// um pouco mais embaixo: o gorro, a lâmpada, o chapéu e os "z" não chegam ao "Uso diário".
const CRAB_Y: f32 = 127.0;
const CRAB_Y_SMALL: f32 = 129.0;
const CRAB_UNIT_SOLO: f32 = 2.0;
/// No médio com números, o Clawd fica no meio, entre os dois anéis: o centro dele e o tamanho
/// dos quadradinhos (2,4 pontos: 3 pixels a 125%, como no pequeno). Embaixo dele, o que está
/// fazendo; embaixo de cada anel, quando ele reinicia.
const CRAB_MID_Y: f32 = 76.0;
const CRAB_UNIT_MID: f32 = 2.4;
const DOING_Y: f32 = 101.0;
const NOTE_Y: f32 = 124.0;
/// A largura que cabe a nota embaixo de cada anel (centrada nele, sem chegar à borda).
const NOTE_W: f32 = 124.0;
/// No médio, o centro do anel do diário (o do semanal fica à mesma distância da borda direita).
const MID_GAUGE_X: f32 = 68.0;

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

/// Quanto aparece de cada parte do grande (1: tudo; na troca de tamanho, elas entram e saem aos
/// poucos): o nome e de onde vêm os números, a linha, os dias da semana (e quanto cada barra já
/// subiu) e o ritmo do diário.
struct Reveal {
    head: f32,
    line: f32,
    days: f32,
    bars: [f32; 7],
    forecast: f32,
}

impl Reveal {
    const ALL: Reveal = Reveal { head: 1.0, line: 1.0, days: 1.0, bars: [1.0; 7], forecast: 1.0 };
}

/// Desenha o que `f` desenha com a opacidade `alpha` (nada, se for 0).
fn faded(c: &mut Canvas, alpha: f32, f: impl FnOnce(&mut Canvas)) {
    if alpha <= 0.004 {
        return;
    }
    if alpha >= 0.996 {
        f(c);
        return;
    }
    c.push_opacity(alpha);
    f(c);
    c.pop_clip();
}

// ---------- a troca de tamanho ----------
/// O pulo do Clawd de um lugar para o outro: agacha até `JUMP.0` do caminho, voa até `JUMP.1`,
/// pousa agachado até `LANDED` e volta ao que fazia. A altura do pulo, em pontos.
const JUMP: (f32, f32) = (0.15, 0.8);
const LANDED: f32 = 0.92;
const JUMP_H: f32 = 30.0;

/// Sai devagar, passa um pouco do ponto e volta (o que nasce na troca de tamanho).
fn ease_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let (c1, c3) = (1.3, 2.3);
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

/// Quanto aparece, a `p` do caminho, algo que existe só antes (`was`) ou só depois (`will`) da
/// troca de tamanho: o que é novo entra depois de `delay`, em `dur`; o que sai some logo no
/// começo, em `out`.
fn presence(was: bool, will: bool, p: f32, delay: f32, dur: f32, out: f32) -> f32 {
    match (was, will) {
        (true, true) => 1.0,
        (false, false) => 0.0,
        (false, true) => 1.0 - (1.0 - ((p - delay) / dur).clamp(0.0, 1.0)).powi(3),
        (true, false) => 1.0 - (p / out).clamp(0.0, 1.0),
    }
}

/// O medidor `g` com o tamanho multiplicado por `s`, no mesmo centro.
fn scaled(g: &Geom, s: f32) -> Geom {
    Geom { r: g.r * s, w: g.w * s, num: g.num * s, cy: g.cy, gauges: &[] }
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
/// Embaixo do nome: a que distância dele e até onde vai (sem chegar ao Clawd, com folga para a
/// caneca do lado dele e para ele andar até a plaquinha, 4 quadradinhos à esquerda).
const SUBTITLE_DY: f32 = 21.0;
const SUBTITLE_W: f32 = LARGE_CRAB_X - (clawd::COLS / 2.0 + 4.0) * clawd::CELL * CRAB_UNIT_MID - 10.0 - BRAND_X;
/// O Clawd no grande: no canto de cima à direita, com o notebook de lado chegando à margem.
const LARGE_CRAB_X: f32 = WIDE - 22.0 - (clawd::SIDE_COLS - clawd::COLS / 2.0) * clawd::CELL * CRAB_UNIT_MID;
const LARGE_CRAB_Y: f32 = 36.0;

fn geom(mode: Mode) -> Geom {
    match mode {
        Mode::P => Geom { r: 55.0, w: 10.0, num: 38.0, cy: 79.0, gauges: &[(Limit::Daily, SMALL / 2.0)] },
        // anéis menores, para o Clawd caber maior no meio
        Mode::M => Geom { r: 44.0, w: 9.0, num: 30.0, cy: 70.0, gauges: &[(Limit::Daily, MID_GAUGE_X), (Limit::Weekly, WIDE - MID_GAUGE_X)] },
        Mode::G => Geom { r: 57.0, w: 11.0, num: 40.0, cy: 132.0, gauges: &[(Limit::Daily, WIDE_Q), (Limit::Weekly, 3.0 * WIDE_Q)] },
    }
}

/// O centro (x) dos dois medidores, no médio e no grande.
fn gauge_xs(mode: Mode) -> [f32; 2] {
    let g = geom(mode);
    [g.gauges[0].1, g.gauges.get(1).map_or(g.gauges[0].1, |x| x.1)]
}

/// A nota de quando reinicia, embaixo de cada anel: (altura, largura máxima). O pequeno não tem.
const LARGE_NOTE_Y: f32 = 202.0;
const LARGE_NOTE_W: f32 = WIDE / 2.0 - 16.0;
fn note_spot(mode: Mode) -> Option<(f32, f32)> {
    match mode {
        Mode::P => None,
        Mode::M => Some((NOTE_Y, NOTE_W)),
        Mode::G => Some((LARGE_NOTE_Y, LARGE_NOTE_W)),
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
    /// o que o Claude Code está fazendo, o fim lido de cada conversa (para ler de novo só quando
    /// o arquivo muda) e desde quando está trabalhando (para o café)
    activity: Activity,
    turns: HashMap<String, (SystemTime, Turn)>,
    busy_since: Option<Instant>,
    /// o Clawd foi para a barra de tarefas (o lugar dele no widget fica vazio)
    perched: bool,
    /// no médio: se o Clawd está à esquerda do meio (com o notebook ou a plaquinha à direita;
    /// None: ainda não sabe, vale o de agora) e ele andando de um lugar para o outro (de quantos
    /// quadradinhos para quantos, desde quando)
    shifted: Option<bool>,
    slide: Option<(f32, f32, Instant)>,
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
            activity: Activity::default(),
            turns: HashMap::new(),
            busy_since: None,
            perched: false,
            shifted: None,
            slide: None,
        };
        c.reload(true);
        c.read_activity();
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
        let dressed = self.crab.scene(clawd::View::Side).hat;
        self.crab = Crab::playing(anim, t);
        if dressed {
            self.crab = std::mem::take(&mut self.crab).dressed();
        }
        self
    }

    /// Com o Claude Code trabalhando agora (o Clawd no notebook).
    #[cfg(test)]
    pub fn sample_busy() -> Claude {
        Claude::sample().idle_for(5.0)
    }

    /// Trabalhando, numa tarefa de Sherlock (de chapéu de detetive).
    #[cfg(test)]
    pub fn sample_sherlock() -> Claude {
        let mut c = Claude::sample_busy();
        c.crab = Crab::new().dressed();
        c
    }

    /// O diário no limite (100%, volta daqui a 1 h 40): o Clawd lendo do lado da plaquinha.
    #[cfg(test)]
    pub fn sample_ko() -> Claude {
        Claude::sample_with(1.0, 0.62, 1.67 * 3600.0)
    }

    /// Conectado, mas ainda esperando os primeiros números do Claude Code.
    #[cfg(test)]
    pub fn sample_waiting() -> Claude {
        let mut c = Claude::sample();
        c.view = View { usage: None, link: Link::Connected };
        c
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
            // a última resposta do Claude Code há pouco (já parado: o Clawd sem o caderno)
            updated: now - 5.0 * 60.0,
            sessions: Vec::new(),
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
            activity: Activity::default(),
            turns: HashMap::new(),
            busy_since: None,
            perched: false,
            shifted: None,
            slide: None,
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
    /// responder (acorda com o mouse em cima), lendo do lado da plaquinha "VOLTO LOGO" com o
    /// diário no limite (até ele reiniciar), suando perto do limite, no notebook enquanto o
    /// Claude Code trabalha.
    fn mood(&self) -> Mood {
        let now = self.now();
        let daily = self.session().map(|w| w.used_at(now));
        if !self.has_data() && self.pointer.is_none() {
            Mood::Sleepy
        } else if daily.is_some_and(|u| u >= LIMIT_AT) {
            Mood::Limit
        } else if self.pointer.is_none() && self.quiet() > NAP_AFTER {
            Mood::Sleepy
        } else if daily.is_some_and(|u| u >= 0.85) {
            Mood::Worried
        } else if self.has_data() && self.busy() {
            Mood::Busy
        } else {
            Mood::Normal
        }
    }

    /// Segundos desde a última chamada da linha de status.
    fn quiet(&self) -> f64 {
        self.view.usage.as_ref().map_or(f64::INFINITY, |u| self.now() - u.updated)
    }

    /// O Claude Code está trabalhando agora? Pelas conversas, se dá para saber; senão, pela
    /// última chamada da linha de status.
    fn busy(&self) -> bool {
        self.activity.busy.unwrap_or_else(|| self.quiet() < BUSY_FOR)
    }

    /// Olha o fim das conversas que chamaram a linha de status na última hora (só as que mudaram
    /// desde a última vez): se alguma está trabalhando e quantos passos por minuto.
    fn read_activity(&mut self) {
        let now = self.now();
        let sessions = self.view.usage.as_ref().map(|u| u.sessions.clone()).unwrap_or_default();
        let mut act = Activity::default();
        let mut seen = Vec::new();
        // a conversa trabalhando que mudou por último (a ferramenta dela vai para o balão)
        let mut newest: Option<f64> = None;
        for (path, at) in sessions {
            if now - at > 3600.0 {
                continue;
            }
            let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) else { continue };
            let turn = match self.turns.get(&path) {
                Some((m, t)) if *m == modified => t.clone(),
                _ => {
                    let t = history::read_tail(std::path::Path::new(&path)).map_or(Turn::default(), |tail| history::turn(&tail, now));
                    self.turns.insert(path.clone(), (modified, t.clone()));
                    t
                }
            };
            let still = modified.elapsed().map_or(0.0, |d| d.as_secs_f64());
            let busy = turn.busy.map(|b| b && still < STALE_AFTER);
            if busy == Some(true) && newest.is_none_or(|s| still < s) {
                newest = Some(still);
                act.tool = turn.tool.clone();
            }
            act.busy = match (act.busy, busy) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (None, None) => None,
                _ => Some(false),
            };
            // os passos contados quando o arquivo mudou; parado há um minuto, já não valem
            if still < 60.0 {
                act.steps += turn.steps;
            }
            seen.push(path);
        }
        self.turns.retain(|p, _| seen.contains(p));
        self.activity = act;
    }

    /// Uma conversa do Claude Code, no arquivo `path`, chamou a linha de status agora (testes).
    #[cfg(test)]
    pub fn with_session(mut self, path: &std::path::Path) -> Claude {
        let now = self.now();
        if let Some(u) = &mut self.view.usage {
            u.sessions = vec![(path.to_string_lossy().into_owned(), now)];
            u.updated = now;
        }
        self.read_activity();
        self
    }

    /// Onde o Clawd fica em cada tamanho: (centro x, centro y, tamanho dos quadradinhos; o
    /// centro é o do corpo, o notebook de lado fica à direita). No grande, no canto de cima à
    /// direita; sem números no pequeno, grande no meio; com um medidor só, na abertura de baixo
    /// do anel (no lugar do ícone do diário); com dois, no meio, entre eles.
    fn crab_spot(&self, mode: Mode) -> (f32, f32, f32) {
        self.crab_spot_at(mode, self.mid_shift())
    }

    /// O mesmo, com o Clawd do médio `shift` quadradinhos à esquerda do meio.
    fn crab_spot_at(&self, mode: Mode, shift: f32) -> (f32, f32, f32) {
        match mode {
            Mode::G => (LARGE_CRAB_X, LARGE_CRAB_Y, CRAB_UNIT_MID),
            Mode::P if self.needs_connect() && self.view.link == Link::Off => (SMALL / 2.0, 50.0, 3.0),
            Mode::P => (SMALL / 2.0, CRAB_Y_SMALL, CRAB_UNIT_SOLO),
            Mode::M if !self.has_data() => (MID_GAUGE_X, CRAB_Y, CRAB_UNIT_SOLO),
            Mode::M => (WIDE / 2.0 - shift * clawd::CELL * CRAB_UNIT_MID, CRAB_MID_Y, CRAB_UNIT_MID),
        }
    }

    /// No médio, o Clawd vai para a esquerda quando há algo à direita dele (o notebook de lado,
    /// ou a plaquinha no limite): o desenho todo fica no meio. Sozinho, ele fica no meio.
    fn wants_shift(&self) -> bool {
        let playing = |a| self.crab.is_playing(a);
        matches!(self.mood(), Mood::Busy | Mood::Limit) || playing(Anim::Done) || playing(Anim::SignUp) || playing(Anim::SignDown)
    }

    /// Quantos quadradinhos à esquerda do meio o Clawd do médio está agora (andando, no meio do
    /// caminho).
    fn mid_shift(&self) -> f32 {
        let at = |on: bool| if on { MID_SHIFT } else { 0.0 };
        match self.slide {
            Some((from, to, start)) if start.elapsed().as_secs_f32() * 1000.0 < SLIDE_MS => {
                from + (to - from) * ease_in_out(start.elapsed().as_secs_f32() * 1000.0 / SLIDE_MS)
            }
            _ => at(self.shifted.unwrap_or_else(|| self.wants_shift())),
        }
    }

    /// Está andando de um lugar para o outro no médio?
    fn sliding(&self) -> bool {
        self.slide.is_some_and(|(_, _, start)| start.elapsed().as_secs_f32() * 1000.0 < SLIDE_MS)
    }

    /// O Clawd está no lugar do ícone do diário (o sol) neste tamanho?
    fn crab_on_daily(&self, mode: Mode) -> bool {
        mode == Mode::P || (mode == Mode::M && !self.has_data())
    }

    /// O medidor mostra o ícone (sol ou calendário) na abertura de baixo? No médio com números,
    /// esse lugar é de quando ele reinicia; e não onde o Clawd está.
    fn shows_icon(&self, limit: Limit, mode: Mode) -> bool {
        !(mode == Mode::M && self.has_data()) && !(limit == Limit::Daily && self.crab_on_daily(mode))
    }

    /// O que escrever embaixo do lugar do Clawd: o que ele faz, ou que foi para a barra.
    fn doing_here(&self) -> &'static str {
        if self.perched { "na barra de tarefas" } else { self.doing() }
    }

    /// O que o Clawd está fazendo, escrito embaixo dele no médio ("" parado, sem nada a dizer).
    fn doing(&self) -> &'static str {
        let playing = |a| self.crab.is_playing(a);
        if playing(Anim::Done) {
            return "pronto";
        }
        if playing(Anim::Party) || playing(Anim::SignDown) {
            return "limite reiniciou";
        }
        match self.mood() {
            Mood::Busy if playing(Anim::Think) => "pensando",
            Mood::Busy if playing(Anim::Investigate) => "investigando",
            Mood::Busy if playing(Anim::Idea) => "teve uma ideia",
            Mood::Busy if playing(Anim::Coffee) => "tomando café",
            Mood::Busy => "programando",
            Mood::Sleepy => "cochilando",
            Mood::Worried => "perto do limite",
            Mood::Limit if playing(Anim::SignUp) => "volto logo",
            Mood::Limit if playing(Anim::Nod) => "cochilando",
            Mood::Limit => "lendo",
            Mood::Normal if playing(Anim::Music) => "ouvindo música",
            Mood::Normal => "",
        }
    }

    /// O ponto (x, y) está em cima do Clawd (com uma folga em volta)?
    fn on_crab(&self, x: f32, y: f32, mode: Mode) -> bool {
        let (cx, cy, unit) = self.crab_spot(mode);
        let (hw, hh) = crab_half(unit);
        (x - cx).abs() <= hw + 4.0 && (y - cy).abs() <= hh + 4.0
    }

    /// O Clawd agora neste tamanho, trabalhando no notebook: no pequeno, de frente, no colo
    /// (de lado não caberia entre as pontas do anel); no médio e no grande, de lado, com a tela à
    /// mostra.
    fn crab_pose(&self, mode: Mode) -> clawd::Pose {
        let mut p = self.crab.pose(self.mood(), self.gaze, view_of(mode));
        // andando de lado no médio: as patas se revezam
        if let (Mode::M, Some((_, _, start))) = (mode, self.slide.filter(|_| self.sliding())) {
            if p.legs == clawd::Legs::Stand {
                p.legs = if (start.elapsed().as_millis() / 100) % 2 == 0 { clawd::Legs::StepA } else { clawd::Legs::StepB };
            }
        }
        p
    }

    /// Desenha o Clawd no lugar dele (na barra de tarefas, o lugar fica vazio, com um aviso).
    fn crab(&self, c: &mut Canvas, mode: Mode) {
        let (cx, cy, unit) = self.crab_spot(mode);
        if self.perched {
            // no pequeno, no lugar dele; no médio, onde ficaria o que ele faz; no grande, embaixo
            // do nome
            if mode == Mode::P {
                c.text(cx, cy - 4.0, "na barra", TEXT, 10.0, MUTED, Anchor::MM, None);
            }
            return;
        }
        clawd::draw(c, cx, cy, unit, &self.crab_pose(mode));
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
        // (sem o ícone, o Clawd ou a nota de quando reinicia está no lugar dele)
        if icon {
            gauge_icon(c, g, cx, limit);
        }
    }

    fn draw_gauges(&self, c: &mut Canvas, mode: Mode, now: f64, only: Option<Limit>) {
        let g = geom(mode);
        for &(limit, cx) in g.gauges {
            if only.is_none_or(|o| o == limit) {
                let since = self.grow[limit.index()].and_then(|g| g.since_hot());
                Self::gauge(c, &g, cx, limit, self.shown(limit, now), since, self.shows_icon(limit, mode));
            }
        }
    }

    /// Embaixo do nome, no grande: de onde vêm os números e o que o Clawd está fazendo.
    fn subtitle(&self, now: f64) -> String {
        match self.doing_here() {
            "" => self.source_line(now),
            doing => format!("{} · {doing}", self.source_line(now)),
        }
    }

    /// As notas de quando cada anel reinicia (diário, semanal), e se são um aviso.
    fn notes(&self, now: f64) -> [(String, bool); 2] {
        [self.session_note(now), (self.week_note(now), false)]
    }

    /// As notas embaixo dos anéis com o centro em `xs`, na altura `y`.
    fn draw_notes(&self, c: &mut Canvas, now: f64, xs: [f32; 2], y: f32, max_w: f32) {
        for (cx, (note, warn)) in xs.into_iter().zip(self.notes(now)) {
            c.text(cx, y, &note, TEXT, 10.5, if warn { ALERT } else { MUTED }, Anchor::MA, Some(max_w));
        }
    }

    /// O que só o grande tem, além dos anéis e das notas: o nome no alto (o Clawd vai à parte),
    /// de onde vêm os números, a linha, os dias da semana e o ritmo do diário.
    fn draw_large_parts(&self, c: &mut Canvas, now: f64, r: &Reveal) {
        let pad = 22.0;
        faded(c, r.head, |c| {
            // o nome desce um pouco enquanto aparece
            let y = BRAND_Y - (1.0 - r.head) * 8.0;
            c.text(BRAND_X, y, "Uso do Claude", BOLD, BRAND_SIZE, FG, Anchor::LA, None);
            c.text(BRAND_X, y + SUBTITLE_DY, &self.subtitle(now), TEXT, 10.5, MUTED, Anchor::LA, Some(SUBTITLE_W));
        });
        // a linha abre do meio
        if r.line > 0.0 {
            let half = (WIDE / 2.0 - pad) * r.line.min(1.0);
            c.fill_round_rect(WIDE / 2.0 - half, 232.0, WIDE / 2.0 + half, 233.0, 0.5, WHITE.alpha(0.1 * r.line.min(1.0)));
        }
        faded(c, r.days, |c| self.draw_weekdays(c, pad, 248.0, WIDE - pad, 394.0, &r.bars));
        if let Some((text, warn)) = self.forecast(now) {
            faded(c, r.forecast, |c| {
                c.text(WIDE / 2.0, 416.0, &text, TEXT, 10.5, if warn { ALERT } else { MUTED }, Anchor::MA, Some(WIDE - 2.0 * pad));
            });
        }
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
        let doing = self.doing_here();
        if !doing.is_empty() {
            c.text(WIDE / 2.0, DOING_Y, doing, TEXT, 10.5, MUTED, Anchor::MA, None);
        }
        // embaixo de cada anel, quando ele reinicia (ou o aviso de que acaba antes)
        self.draw_notes(c, now, gauge_xs(Mode::M), NOTE_Y, NOTE_W);
    }

    fn draw_large(&self, c: &mut Canvas, now: f64) {
        let pad = 22.0;
        self.crab(c, Mode::G);
        self.draw_gauges(c, Mode::G, now, None);
        if !self.has_data() {
            c.text(BRAND_X, BRAND_Y, "Uso do Claude", BOLD, BRAND_SIZE, FG, Anchor::LA, None);
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
        // embaixo de cada medidor, quando ele reinicia
        self.draw_notes(c, now, gauge_xs(Mode::G), LARGE_NOTE_Y, LARGE_NOTE_W);
        self.draw_large_parts(c, now, &Reveal::ALL);
    }

    /// Os dias da semana em que mais se usa: uma barra por dia (o de mais uso em laranja cheio,
    /// com a parte dele no total em cima) e o dia de hoje com o nome em branco. `bars`: quanto
    /// cada barra já subiu (1: inteira; na troca de tamanho, sobem uma depois da outra).
    fn draw_weekdays(&self, c: &mut Canvas, x0: f32, y0: f32, x1: f32, y1: f32, bars: &[f32; 7]) {
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
            let grow = bars[i];
            if grow <= 0.01 {
                continue;
            }
            let cx = x0 + slot * (i as f32 + 0.5);
            let (bx0, bx1) = (cx - bw / 2.0, cx + bw / 2.0);
            faded(c, (grow * 1.5).min(1.0), |c| {
                c.fill_round_rect(bx0, bars_top, bx1, base, 6.0, WHITE.alpha(0.06));
                if max > 0.0 && days[i] > 0.0 {
                    let h = ((base - bars_top) * (days[i] / max) as f32).max(12.0) * grow;
                    let color = if Some(i) == top { ORANGE } else { ORANGE.mix(WHITE, 0.2).alpha(0.5) };
                    c.fill_round_rect(bx0, base - h, bx1, base, 6.0_f32.min(h / 2.0), color);
                    if Some(i) == top {
                        let share = format!("{}%", (days[i] / total * 100.0).round());
                        c.text(cx, base - h - 6.0, &share, TEXT, 10.5, FG, Anchor::MS, None);
                    }
                }
                c.text(cx, base + 7.0, name, TEXT, 10.0, if i == today { FG } else { MUTED }, Anchor::MA, None);
            });
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

impl Claude {
    /// Onde e como o Clawd está a `p` (0 a 1) da troca de `from` para `to`: (centro x, centro y,
    /// tamanho dos quadradinhos, quadro). Agacha, pula num arco até o lugar novo (os
    /// quadradinhos trocam de tamanho no alto do pulo), pousa agachado e volta ao que fazia.
    fn crab_between(&self, from: Mode, to: Mode, p: f32) -> (f32, f32, f32, clawd::Pose) {
        let ((ax, ay, au), (bx, by, bu)) = (self.crab_spot(from), self.crab_spot(to));
        if p < JUMP.0 {
            return (ax, ay, au, clawd::jump_pose(true));
        }
        if p >= LANDED {
            return (bx, by, bu, self.crab_pose(to));
        }
        if p >= JUMP.1 {
            return (bx, by, bu, clawd::jump_pose(true));
        }
        let s = ease_in_out((p - JUMP.0) / (JUMP.1 - JUMP.0));
        let (x, y) = (ax + (bx - ax) * s, ay + (by - ay) * s - JUMP_H * 4.0 * s * (1.0 - s));
        (x, y, if s < 0.5 { au } else { bu }, clawd::jump_pose(false))
    }

    /// Um quadro da troca de tamanho, a `p` (0 a 1) do caminho de `from` para `to`, com tudo no
    /// meio do caminho: o que existe nos dois tamanhos anda (e muda de tamanho) de um lugar para
    /// o outro; o que é novo entra depois (o semanal crescendo do centro, as barras dos dias
    /// subindo uma depois da outra); o que sai some logo no começo; e o Clawd pula.
    fn draw_morph(&self, c: &mut Canvas, from: Mode, to: Mode, p: f32) {
        let now = self.now();
        let e = ease_in_out(p);
        let mix = |a: f32, b: f32| a + (b - a) * e;
        let (ga, gb) = (geom(from), geom(to));
        for limit in [Limit::Daily, Limit::Weekly] {
            let at = |g: &Geom| g.gauges.iter().find(|x| x.0 == limit).map(|x| x.1);
            let (g, cx, alpha) = match (at(&ga), at(&gb)) {
                (Some(x0), Some(x1)) => {
                    let g = Geom { r: mix(ga.r, gb.r), w: mix(ga.w, gb.w), num: mix(ga.num, gb.num), cy: mix(ga.cy, gb.cy), gauges: &[] };
                    (g, mix(x0, x1), 1.0)
                }
                // o semanal nasce crescendo do centro dele (passa um pouco e volta), ou some
                // encolhendo
                (None, Some(x1)) => (scaled(&gb, ease_back((p - 0.2) / 0.6)), x1, ((p - 0.2) / 0.35).clamp(0.0, 1.0)),
                (Some(x0), None) => (scaled(&ga, 1.0 - ease_in_out((p / 0.45).min(1.0))), x0, 1.0 - (p / 0.4).min(1.0)),
                (None, None) => continue,
            };
            if g.r < 2.0 {
                continue;
            }
            faded(c, alpha, |c| Self::gauge(c, &g, cx, limit, self.shown(limit, now), None, false));
            // o sol e o calendário (só no grande)
            let icon = |m: Mode| self.shows_icon(limit, m) && at(&geom(m)).is_some();
            faded(c, alpha * presence(icon(from), icon(to), p, 0.45, 0.4, 0.3), |c| gauge_icon(c, &g, cx, limit));
        }
        // as notas de quando reinicia andam junto com os anéis
        let (na, nb) = (note_spot(from), note_spot(to));
        if let (Some(n0), Some(n1)) = (na.or(nb), nb.or(na)) {
            let (x0, x1) = (gauge_xs(if na.is_some() { from } else { to }), gauge_xs(if nb.is_some() { to } else { from }));
            let xs = [mix(x0[0], x1[0]), mix(x0[1], x1[1])];
            faded(c, presence(na.is_some(), nb.is_some(), p, 0.5, 0.35, 0.25), |c| self.draw_notes(c, now, xs, mix(n0.0, n1.0), mix(n0.1, n1.1)));
        }
        // o que ele está fazendo, embaixo dele no médio
        let doing = self.doing_here();
        if !doing.is_empty() {
            faded(c, presence(from == Mode::M, to == Mode::M, p, 0.7, 0.3, 0.2), |c| {
                c.text(WIDE / 2.0, DOING_Y, doing, TEXT, 10.5, MUTED, Anchor::MA, None);
            });
        }
        // o grande: o nome desce, a linha abre do meio e as barras sobem uma depois da outra; na
        // volta, tudo sai antes, de uma vez
        let (was, will) = (from == Mode::G, to == Mode::G);
        if was || will {
            let bars = std::array::from_fn(|i| {
                let i = i as f32;
                // (a última termina de subir junto com a troca)
                if will { ease_back((p - 0.4 - i * 0.04) / 0.36) } else { 1.0 - ease_in_out(((p - i * 0.025) / 0.3).clamp(0.0, 1.0)) }
            });
            let head = presence(was, will, p, 0.4, 0.4, 0.25);
            let r = Reveal { head, line: head, days: presence(was, will, p, 0.5, 0.35, 0.2), bars, forecast: presence(was, will, p, 0.75, 0.25, 0.2) };
            self.draw_large_parts(c, now, &r);
        }
        // e o Clawd, por cima de tudo (se não foi para a barra de tarefas)
        if !self.perched {
            let (x, y, unit, pose) = self.crab_between(from, to, p);
            clawd::draw(c, x, y, unit, &pose);
        }
    }
}

// ---------- na barra de tarefas ----------

/// A janela do Clawd na barra de tarefas, em pontos: o balão em cima e o Clawd embaixo à direita,
/// com as patas na borda de baixo (em cima da barra). O centro do corpo dele e o tamanho dos
/// quadradinhos.
pub const PERCH_W: f32 = 240.0;
pub const PERCH_H: f32 = 74.0;
pub const PERCH_UNIT: f32 = 2.4;
pub const PERCH_CRAB: (f32, f32) = (PERCH_W - 6.0 - (clawd::SIDE_COLS - clawd::COLS / 2.0) * clawd::CELL * PERCH_UNIT, PERCH_H - clawd::ROWS / 2.0 * clawd::CELL * PERCH_UNIT);
/// O balão: altura e onde fica a borda de baixo (a ponta desce até perto da cabeça dele).
const BALLOON_H: f32 = 26.0;
const BALLOON_BOTTOM: f32 = 32.0;

/// As cores do pontinho do balão: editando, lendo, rodando, procurando, outras e pronto.
const EDITING: Rgba = ORANGE;
const READING: Rgba = Rgba::rgb(120, 196, 255);
const RUNNING: Rgba = Rgba::rgb(112, 214, 128);
const SEARCHING: Rgba = Rgba::rgb(242, 132, 164);
const OTHER: Rgba = Rgba::rgb(255, 226, 92);

/// O balão mostra os nomes de arquivos e comandos? (Desligado, só a ação: para compartilhar a
/// tela sem mostrar no que se está trabalhando.)
pub fn show_names(cfg: &serde_json::Value) -> bool {
    cfg["claude"]["balao_nomes"].as_bool().unwrap_or(true)
}

/// O que dizer no balão sobre uma ferramenta do Claude Code: a frase e a cor do pontinho.
fn tool_line(tool: &Tool, names: bool) -> (String, Rgba) {
    let input = tool.input.trim();
    let file = input.rsplit(['/', '\\']).next().unwrap_or("").to_string();
    let short = |t: &str, max: usize| if t.chars().count() > max { format!("{}…", t.chars().take(max - 1).collect::<String>()) } else { t.to_string() };
    let named = |doing: &str, what: String, plain: &str| if names && !what.is_empty() { format!("{doing} {what}") } else { plain.to_string() };
    match tool.name.as_str() {
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => (named("Editando", short(&file, 26), "Editando um arquivo"), EDITING),
        "Read" => (named("Lendo", short(&file, 26), "Lendo um arquivo"), READING),
        "Bash" | "PowerShell" => {
            let cmd = input.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
            (named("Rodando", short(&cmd, 24), "Rodando um comando"), RUNNING)
        }
        "Grep" | "Glob" => (named("Procurando", if input.is_empty() { String::new() } else { format!("\"{}\"", short(input, 20)) }, "Procurando no código"), SEARCHING),
        "WebFetch" | "WebSearch" => ("Pesquisando na web".into(), READING),
        "Task" | "Agent" => ("Chamando um ajudante".into(), OTHER),
        "TodoWrite" => ("Organizando as tarefas".into(), OTHER),
        other => {
            // ferramentas de MCP: "mcp__servidor__ferramenta"
            let name = other.rsplit("__").next().unwrap_or(other);
            (format!("Usando {}", short(name, 24)), OTHER)
        }
    }
}

impl Claude {
    /// O Clawd na barra: sempre com o notebook de lado, sem ninguém com o mouse em cima.
    fn perch_pose(&self) -> clawd::Pose {
        self.crab.pose(self.mood(), None, clawd::View::Side)
    }

    /// O que o balão diz agora (a frase e a cor do pontinho); None: sem balão (parado ou
    /// cochilando).
    fn balloon(&self, names: bool) -> Option<(String, Rgba)> {
        if self.crab.is_playing(Anim::Done) {
            return Some(("✓ Pronto".into(), RUNNING));
        }
        match self.mood() {
            Mood::Busy if self.crab.is_playing(Anim::Coffee) => Some(("Hora do café".into(), Rgba::rgb(196, 140, 98))),
            Mood::Busy => Some(match &self.activity.tool {
                Some(tool) => tool_line(tool, names),
                None => ("Pensando…".into(), Rgba::rgb(190, 170, 255)),
            }),
            Mood::Worried => Some(("Perto do limite diário".into(), ALERT)),
            Mood::Limit => Some(("Limite diário atingido · volto logo".into(), ALERT)),
            _ => None,
        }
    }

    /// A janela da barra de tarefas: o balão (se houver o que dizer) e o Clawd.
    fn draw_perch_window(&self, c: &mut Canvas, names: bool) {
        let (cx, cy) = PERCH_CRAB;
        if let Some((text, dot)) = self.balloon(names) {
            let size = 11.0;
            let w = (c.text_width(&text, TEXT, size) + 32.0).min(PERCH_W - 8.0);
            // à esquerda do Clawd, com a ponta apontando para a cabeça dele
            let x1 = (cx + 34.0).min(PERCH_W - 4.0);
            let (x0, y0, y1) = (x1 - w, BALLOON_BOTTOM - BALLOON_H, BALLOON_BOTTOM);
            let glass = Rgba(28, 22, 44, 235);
            c.fill_round_rect(x0, y0, x1, y1, BALLOON_H / 2.0, glass);
            c.stroke_round_rect(x0, y0, x1, y1, BALLOON_H / 2.0, Rgba(255, 255, 255, 40), 1.0);
            c.polygon(&[(cx - 6.0, y1 - 0.5), (cx + 4.0, y1 - 0.5), (cx - 2.0, y1 + 6.0)], glass);
            c.fill_circle(x0 + 13.0, (y0 + y1) / 2.0, 3.2, dot);
            c.text(x0 + 22.0, (y0 + y1) / 2.0, &text, TEXT, size, FG, Anchor::LM, Some(w - 30.0));
        }
        clawd::draw(c, cx, cy, PERCH_UNIT, &self.perch_pose());
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

/// O ícone na abertura de baixo do medidor `g` com o centro em `cx`: o sol (diário) ou o
/// calendário (semanal).
fn gauge_icon(c: &mut Canvas, g: &Geom, cx: f32, limit: Limit) {
    let (iy, ir) = (g.cy + g.r * 0.86, g.w * 0.9);
    match limit {
        Limit::Daily => sun(c, cx, iy, ir, ORANGE),
        Limit::Weekly => calendar(c, cx, iy, ir, ORANGE),
    }
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

    fn perch_from(&self, mode: Mode) -> Option<(f32, f32)> {
        let (cx, cy, _) = self.crab_spot(mode);
        Some((cx, cy))
    }

    fn set_perched(&mut self, on: bool) {
        self.perched = on;
        // na barra, ninguém está com o mouse em cima dele
        self.pointer = None;
        self.gaze = None;
    }

    fn draw_perch(&self, c: &mut Canvas, env: &Env) {
        self.draw_perch_window(c, show_names(env.cfg));
    }

    fn perch_key(&self, env: &Env) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}", clawd::cells(&self.perch_pose())).hash(&mut h);
        self.balloon(show_names(env.cfg)).map(|b| b.0).hash(&mut h);
        h.finish()
    }

    fn morphs(&self, from: Mode, to: Mode) -> bool {
        // sem números, cada tamanho tem um texto diferente: fica a troca de sempre
        from != to && self.has_data()
    }

    fn draw_between(&self, c: &mut Canvas, from: Mode, to: Mode, p: f32, _env: &Env) {
        self.draw_morph(c, from, to, p);
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
                let (hw, hh) = crab_half(unit);
                let (hw, hh) = (hw + 4.0, hh + 4.0);
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
            // (já pulou ao apertar) e vai para a barra de tarefas; lá, outro clique o traz
            POKE_CRAB | PERCH_TOGGLE => ctx.out.push(Request::Perch(!self.perched)),
            _ => {}
        }
    }

    fn pressed(&mut self, action: u32) -> bool {
        // clique no Clawd: agacha para o pulo na hora, ao apertar
        if action == POKE_CRAB && !self.perched {
            self.crab.play(Anim::Hop);
            return true;
        }
        false
    }

    fn menu(&self, _env: &Env) -> Vec<MenuItem> {
        let mut items = vec![MenuItem::Cmd("Ver uso no claude.ai".into(), OPEN_USAGE)];
        let perch = if self.perched { "Trazer o Clawd de volta" } else { "Mandar o Clawd para a barra de tarefas" };
        items.push(MenuItem::Cmd(perch.into(), PERCH_TOGGLE));
        match self.view.link {
            Link::Connected => items.push(MenuItem::Cmd("Desconectar do Claude Code".into(), DISCONNECT)),
            Link::Off => items.push(MenuItem::Cmd("Conectar ao Claude Code".into(), CONNECT)),
            _ => {}
        }
        items
    }

    fn next_tick(&self) -> Option<Instant> {
        // enquanto o arco anima, um quadro a cada 16 ms; o Clawd, nos quadros dele
        let next = if self.animating() || self.sliding() { self.due.min(Instant::now() + FRAME) } else { self.due };
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
            self.crab.greet(self.mood());
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
        if self.reading.is_none() && self.read_at.is_some_and(|t| t.elapsed() >= HISTORY_EVERY) {
            self.read_history();
        }
        if Instant::now() >= self.due {
            self.due = Instant::now() + if self.busy() { CHECK_BUSY } else { CHECK_EVERY };
            // (os exemplos, com a hora fixa, não leem o arquivo de verdade)
            if self.fixed_now.is_none() {
                redraw |= self.reload(false);
            }
            self.read_activity();
            // os textos de tempo ("acaba às", "atualizado às") mudam com o relógio
            redraw |= (self.now() / 60.0) as i64 != *self.minute.borrow();
        }
        let mood = self.mood();
        // o Claude Code terminou: o Clawd fecha o caderno, se espreguiça e mostra um ✓
        if self.shown_mood == Mood::Busy && mood == Mood::Normal {
            self.crab.play(Anim::Done);
        }
        // o ritmo da escrita e, depois de muito tempo trabalhando, o café
        let busy = self.has_data() && self.busy();
        self.busy_since = if busy { Some(self.busy_since.unwrap_or_else(Instant::now)) } else { None };
        self.crab.tired = self.busy_since.is_some_and(|t| t.elapsed() >= clawd::COFFEE_AFTER);
        self.crab.pace = if self.activity.busy.is_some() { clawd::pace_for(self.activity.steps as f32) } else { clawd::PACE };
        self.crab.music = self.has_data();
        redraw |= self.crab.tick(mood);
        // no médio, anda para o lado quando o notebook (ou a plaquinha) aparece ou sai
        let want = self.wants_shift();
        if self.shifted != Some(want) {
            if self.shifted.is_some() {
                let target = if want { MID_SHIFT } else { 0.0 };
                self.slide = Some((self.mid_shift(), target, Instant::now()));
            }
            self.shifted = Some(want);
            redraw = true;
        }
        redraw |= self.sliding();
        // dormiu ou acordou (cochilo depois de um tempo sem o Claude Code, ou o mouse)
        if mood != self.shown_mood {
            self.shown_mood = mood;
            redraw = true;
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

    /// A que distância (pontos) o retângulo `r` está do traço do anel com centro (gx, gy), raio
    /// `rad` e espessura `w` (negativo: em cima dele). O anel vai de 135° a 405° (o vão é embaixo,
    /// de 45° a 135°), com pontas redondas.
    fn ring_clearance(r: [f32; 4], gx: f32, gy: f32, rad: f32, w: f32) -> f32 {
        let point = |px: f32, py: f32| {
            let (dx, dy) = (px - gx, py - gy);
            let angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
            let d = if !(45.0..=135.0).contains(&angle) {
                ((dx * dx + dy * dy).sqrt() - rad).abs()
            } else {
                let end = |a: f32| (gx + rad * a.to_radians().cos(), gy + rad * a.to_radians().sin());
                let ((ax, ay), (bx, by)) = (end(45.0), end(135.0));
                (px - ax).hypot(py - ay).min((px - bx).hypot(py - by))
            };
            d - w / 2.0
        };
        // longe do anel (todo por dentro ou todo por fora): a conta direta
        let (nx, ny) = (gx.clamp(r[0], r[2]), gy.clamp(r[1], r[3]));
        let near = (nx - gx).hypot(ny - gy);
        let far = [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])].iter().map(|&(x, y)| (x - gx).hypot(y - gy)).fold(0.0, f32::max);
        if near > rad + w / 2.0 + 6.0 {
            return near - rad - w / 2.0;
        }
        if far < rad - w / 2.0 - 6.0 {
            return rad - w / 2.0 - far;
        }
        // perto: em pontos de 0,4 em 0,4 pt pelo retângulo
        let (nx, ny) = (((r[2] - r[0]) / 0.4).ceil().max(1.0) as i32, ((r[3] - r[1]) / 0.4).ceil().max(1.0) as i32);
        let mut best = f32::INFINITY;
        for i in 0..=nx {
            for j in 0..=ny {
                best = best.min(point(r[0] + (r[2] - r[0]) * i as f32 / nx as f32, r[1] + (r[3] - r[1]) * j as f32 / ny as f32));
            }
        }
        best
    }

    /// Os textos e botões de cada tamanho que o Clawd não pode tocar: (nome, x0, y0, x1, y1).
    fn obstacles(w: &Claude, mode: Mode) -> Vec<(&'static str, [f32; 4])> {
        let mut out = Vec::new();
        let g = geom(mode);
        let drawn: Vec<(Limit, f32)> = if w.needs_connect() && w.view.link == Link::Off && mode == Mode::P {
            Vec::new()
        } else if mode == Mode::P || (mode == Mode::M && !w.has_data()) {
            vec![g.gauges[0]]
        } else {
            g.gauges.to_vec()
        };
        // o número e o nome no meio de cada anel
        for (_, x) in drawn {
            let cap = g.num * 0.7;
            let label = (g.num * 0.29).clamp(10.5, 12.0);
            let base = g.cy + (cap - label - 8.0) / 2.0;
            out.push(("número", [x - g.num * 0.85, base - cap - 1.0, x + g.num * 0.85, base + 1.0]));
            out.push(("nome do anel", [x - label * 2.4, base + 5.0, x + label * 2.4, base + 6.0 + label * 1.2]));
        }
        match mode {
            Mode::P if w.needs_connect() && w.view.link == Link::Off => {
                out.push(("Uso do Claude", [32.0, 82.0, 122.0, 98.0]));
            }
            Mode::M if w.has_data() => {
                out.push(("o que ele faz", [WIDE / 2.0 - 40.0, DOING_Y - 1.0, WIDE / 2.0 + 40.0, DOING_Y + 14.0]));
                for x in gauge_xs(Mode::M) {
                    out.push(("nota", [x - NOTE_W / 2.0, NOTE_Y - 1.0, x + NOTE_W / 2.0, NOTE_Y + 14.0]));
                }
            }
            Mode::M => {
                out.push(("título", [170.0, 24.0, WIDE - 16.0, 42.0]));
                out.push(("texto", [170.0, 44.0, WIDE - 16.0, 88.0]));
            }
            Mode::G => {
                out.push(("Uso do Claude", [BRAND_X, BRAND_Y - 2.0, 124.0, BRAND_Y + 16.0]));
                out.push(("subtítulo", [BRAND_X, BRAND_Y + SUBTITLE_DY - 1.0, BRAND_X + SUBTITLE_W, BRAND_Y + SUBTITLE_DY + 14.0]));
            }
            _ => {}
        }
        if w.needs_connect() && w.view.link == Link::Off {
            let (x0, y0, x1, y1) = connect_rect(mode);
            out.push(("botão", [x0, y0, x1, y1]));
        }
        out
    }

    #[test]
    fn crab_fits_every_frame_in_every_size() {
        // todos os quadros de todas as animações que podem tocar em cada estado, em cada tamanho e
        // em cinco escalas de tela (com os quadradinhos em pixels inteiros, como no desenho): o
        // Clawd fica a 6 pt da borda e a 2 pt dos anéis, dos textos e do botão
        const MARGIN: f32 = 6.0;
        const PAD: f32 = 2.0;
        let states = [
            ("com números", Claude::sample()),
            ("trabalhando", Claude::sample_busy()),
            ("perto do limite", Claude::sample_limit()),
            ("no limite", Claude::sample_ko()),
            ("cochilando", Claude::sample().idle_for(NAP_AFTER + 60.0)),
            ("sem conectar", Claude::sample_unlinked()),
            ("esperando os números", Claude::sample_waiting()),
        ];
        let mut cache: HashMap<(String, bool, bool), Vec<(Option<Anim>, Vec<clawd::Cell>)>> = HashMap::new();
        let mut checked = 0;
        for (name, w) in &states {
            let data = w.has_data();
            // o humor de agora e, com o mouse em cima, o de acordado
            let mut moods = vec![w.mood()];
            if moods[0] == Mood::Sleepy {
                moods.push(Mood::Normal);
            }
            for mode in Mode::ALL {
                let l = super::super::Layout::of(mode);
                let view = view_of(mode);
                let obstacles = obstacles(w, mode);
                let rings: Vec<(f32, f32)> = obstacles.iter().filter(|o| o.0 == "número").map(|o| ((o.1[0] + o.1[2]) / 2.0, geom(mode).cy)).collect();
                let g = geom(mode);
                for &mood in &moods {
                    let key = (format!("{mood:?}"), view == clawd::View::Side, data);
                    let frames = cache.entry(key).or_insert_with(|| clawd::frames_for(mood, view, data));
                    for s in [1.0, 1.25, 1.5, 1.75, 2.0] {
                        for (anim, cells) in frames.iter() {
                            // no médio, à esquerda com o notebook ou a plaquinha à direita; no
                            // meio sozinho; o que toca nos dois, nos dois lugares (e no caminho)
                            let right = matches!(mood, Mood::Busy | Mood::Limit) || matches!(anim, Some(Anim::Done | Anim::SignUp | Anim::SignDown));
                            let shifts: &[f32] = if right { &[MID_SHIFT] } else { &[0.0, MID_SHIFT] };
                            for &shift in shifts {
                            let (cx, cy, unit) = w.crab_spot_at(mode, shift);
                            let (x0, y0, c) = clawd::origin(s, cx, cy, unit);
                            checked += 1;
                            for &(x, y, cw, ch, col) in cells {
                                if col.3 < 10 {
                                    continue;
                                }
                                let r = [x0 + x as f32 * c, y0 + y as f32 * c, x0 + (x + cw) as f32 * c, y0 + (y + ch) as f32 * c];
                                let at = || format!("{name}, {mode:?}, {mood:?}, a {}%: {r:?}", s * 100.0);
                                assert!(r[0] >= MARGIN - 0.01 && r[1] >= MARGIN - 0.01 && r[2] <= l.w - MARGIN + 0.01 && r[3] <= l.h - MARGIN + 0.01, "sai do cartão ({})", at());
                                for &(gx, gy) in &rings {
                                    assert!(ring_clearance(r, gx, gy, g.r, g.w) >= PAD - 0.05, "encosta no anel ({})", at());
                                }
                                for (what, b) in &obstacles {
                                    let apart = (b[0] - r[2]).max(r[0] - b[2]).max(b[1] - r[3]).max(r[1] - b[3]);
                                    assert!(apart >= PAD - 0.05, "encosta em \"{what}\" ({})", at());
                                }
                            }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 10_000, "{checked}");
        // no grande, o que está escrito embaixo do nome não chega ao Clawd
        let (cx, _, unit) = Claude::sample().crab_spot(Mode::G);
        assert!(BRAND_X + SUBTITLE_W <= cx - crab_half(unit).0 - 10.0 + 0.01);
    }

    #[test]
    fn medium_has_a_bigger_crab_between_smaller_rings() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let w = Claude::sample();
        let (cx, cy, unit) = w.crab_spot(Mode::M);
        // do tamanho do pequeno: a 125%, 3 pixels por quadradinho (antes eram 2)
        let px = |u: f32| (u * 1.25).round();
        assert_eq!(px(unit), 3.0);
        assert_eq!(px(unit), px(w.crab_spot(Mode::P).2), "do mesmo tamanho que no pequeno");
        // sozinho, no meio; trabalhando, o corpo um pouco à esquerda: com o notebook de lado, o
        // desenho todo fica no meio
        assert!((cx - WIDE / 2.0).abs() < 0.01, "fora do meio");
        let (bx, _, _) = Claude::sample_busy().crab_spot(Mode::M);
        assert!((bx + (clawd::SIDE_COLS / 2.0 - clawd::COLS / 2.0) * clawd::CELL * unit - WIDE / 2.0).abs() < 0.01, "trabalhando, fora do meio");
        // no médio com números, nenhum anel tem ícone: o lugar é da nota de quando reinicia
        assert!(!w.shows_icon(Limit::Daily, Mode::M) && !w.shows_icon(Limit::Weekly, Mode::M));
        assert!(w.shows_icon(Limit::Weekly, Mode::G) && !w.shows_icon(Limit::Daily, Mode::P));
        // o que ele faz fica embaixo dele (mesmo de patas esticadas), sem encostar nos anéis
        let g = geom(Mode::M);
        assert!(cy + (clawd::ROWS / 2.0 + 3.0) * clawd::CELL * unit < DOING_Y, "a palavra encosta no Clawd");
        // na altura do alto da palavra, até onde vai o anel da esquerda (por fora)
        let (outer, dy) = (g.r + g.w / 2.0, DOING_Y - g.cy);
        let ring_right = g.gauges[0].1 + (outer * outer - dy * dy).max(0.0).sqrt();
        let c = Canvas::new(10, 10, 1.0);
        for word in ["programando", "pensando", "investigando", "teve uma ideia", "tomando café", "pronto", "cochilando", "perto do limite", "volto logo", "lendo", "limite reiniciou", "ouvindo música", "na barra de tarefas"] {
            let half = c.text_width(word, TEXT, 10.5) / 2.0;
            assert!(WIDE / 2.0 - half > ring_right + 4.0, "\"{word}\" encosta no anel");
        }
        // as notas: embaixo dos anéis e dentro do cartão
        let ring_bottom = g.cy + g.r * 45f32.to_radians().sin() + g.w / 2.0;
        assert!(NOTE_Y > ring_bottom + 8.0 && NOTE_Y + 14.0 <= SMALL - 6.0);
        for &(_, gx) in g.gauges {
            assert!(gx - NOTE_W / 2.0 >= 4.0 && gx + NOTE_W / 2.0 <= WIDE - 4.0);
        }
    }

    #[test]
    fn small_crab_works_on_the_laptop_on_his_lap() {
        // trabalhando, o pequeno tem o notebook de frente, no colo; o médio e o grande, de lado,
        // com a tela à mostra
        let w = Claude::sample_busy().with_crab(Anim::Type, 0.3);
        let small = w.crab_pose(Mode::P);
        assert!(small.front.is_some_and(|l| l.open) && small.side.is_none());
        for mode in [Mode::M, Mode::G] {
            let p = w.crab_pose(mode);
            assert!(p.side.is_some_and(|s| s.open) && p.front.is_none(), "{mode:?}");
        }
        // parado, sem notebook em nenhum tamanho
        assert!(Mode::ALL.iter().all(|&m| {
            let p = Claude::sample().crab_pose(m);
            p.front.is_none() && p.side.is_none()
        }));
        // de Sherlock, de chapéu nos três tamanhos (o cachimbo só de lado)
        let w = Claude::sample_sherlock();
        for mode in Mode::ALL {
            let p = w.crab_pose(mode);
            assert!(p.hat.is_some() && p.pipe.is_some() == (mode != Mode::P), "{mode:?}");
        }
    }

    #[test]
    fn in_the_medium_size_the_crab_walks_aside_for_the_laptop() {
        let tick = |w: &mut Claude| {
            let mut req = Vec::new();
            let (mut c, mut cache) = (cfg(), serde_json::json!({}));
            w.tick(&mut Ctx { cfg: &mut c, cache: &mut cache, key: "claude", out: &mut req })
        };
        // parado, no meio
        let mut w = Claude::sample();
        tick(&mut w);
        assert!((w.crab_spot(Mode::M).0 - WIDE / 2.0).abs() < 0.01);
        // começou a trabalhar: anda para a esquerda, sem pular, com as patas se revezando
        w.view.usage.as_mut().unwrap().updated = w.now() - 5.0;
        assert!(tick(&mut w) && w.sliding());
        assert!((w.crab_spot(Mode::M).0 - WIDE / 2.0).abs() < 1.0, "pulou para o lado");
        assert!(matches!(w.crab_pose(Mode::M).legs, clawd::Legs::StepA | clawd::Legs::StepB));
        assert!(Kind::next_tick(&w).unwrap() <= Instant::now() + FRAME);
        // e chega lá: o notebook à direita, o desenho todo no meio
        w.slide = w.slide.map(|(a, b, _)| (a, b, Instant::now() - Duration::from_secs(1)));
        let shifted = WIDE / 2.0 - MID_SHIFT * clawd::CELL * CRAB_UNIT_MID;
        assert!((w.crab_spot(Mode::M).0 - shifted).abs() < 0.01);
        // nos outros tamanhos, nada muda
        assert_eq!(w.crab_spot(Mode::P), Claude::sample().crab_spot(Mode::P));
        assert_eq!(w.crab_spot(Mode::G), Claude::sample().crab_spot(Mode::G));
    }

    #[test]
    fn at_the_limit_the_crab_reads_beside_the_sign() {
        let w = Claude::sample_ko();
        assert_eq!(w.mood(), Mood::Limit);
        // nem o mouse nem o tempo sem o Claude Code mudam isso: ele lê até o limite reiniciar
        assert_eq!(Claude::sample_ko().idle_for(NAP_AFTER + 60.0).mood(), Mood::Limit);
        assert_eq!(Claude::sample_ko().with_pointer(40.0, 40.0, Mode::P).mood(), Mood::Limit);
        for mode in Mode::ALL {
            let p = w.crab_pose(mode);
            assert!(p.sign.is_some() && p.book.is_some(), "{mode:?}");
        }
        assert_eq!(w.doing(), "lendo");
        assert_eq!(w.balloon(true).unwrap().0, "Limite diário atingido · volto logo");
        // no tick, o humor novo toca a placa subindo; quando reinicia, recolhe e comemora
        let mut w = Claude::sample_ko();
        let mut req = Vec::new();
        let (mut c, mut cache) = (cfg(), serde_json::json!({}));
        w.tick(&mut Ctx { cfg: &mut c, cache: &mut cache, key: "claude", out: &mut req });
        assert!(w.crab.is_playing(Anim::SignUp));
        assert_eq!(w.doing(), "volto logo");
    }

    /// O widget na tela de `w` × `h` pontos, a `p` da troca de `from` para `to` (`p` None: só
    /// `to`, desenhado do jeito normal).
    fn frame(w: &Claude, from: Mode, to: Mode, p: Option<f32>, size: (f32, f32)) -> crate::gfx::Image {
        let s = 1.25;
        let mut c = Canvas::new((size.0 * s).round() as u32, (size.1 * s).round() as u32, s);
        let cfg = cfg();
        let env = Env { cfg: &cfg, key: "claude" };
        match p {
            Some(p) => w.draw_between(&mut c, from, to, p, &env),
            None => w.draw(&mut c, to, &env),
        }
        c.finish()
    }

    #[test]
    fn changing_size_moves_everything_to_its_new_place() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let size = |m: Mode| {
            let l = super::super::Layout::of(m);
            (l.w, l.h)
        };
        for w in [Claude::sample(), Claude::sample_busy(), Claude::sample_limit()] {
            for (from, to) in [(Mode::P, Mode::M), (Mode::M, Mode::G), (Mode::G, Mode::P), (Mode::M, Mode::P), (Mode::G, Mode::M), (Mode::P, Mode::G)] {
                assert!(w.morphs(from, to));
                // no fim, igual ao tamanho novo desenhado do jeito normal: não pula ao terminar
                let (end, real) = (frame(&w, from, to, Some(1.0), size(to)), frame(&w, from, to, None, size(to)));
                let differ = end.px.chunks(4).zip(real.px.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 24)).count();
                assert!(differ as f32 / (end.w * end.h) as f32 <= 0.002, "{from:?} → {to:?}: o último quadro é outro ({differ} pixels)");
                // no meio do caminho, num cartão de tamanho intermediário, sempre desenha algo
                for k in 0..=10 {
                    let p = k as f32 / 10.0;
                    let (a, b) = (size(from), size(to));
                    let img = frame(&w, from, to, Some(p), (a.0 + (b.0 - a.0) * p, a.1 + (b.1 - a.1) * p));
                    let ink = img.px.chunks(4).filter(|px| px[3] > 8).count();
                    assert!(ink as f32 / (img.w * img.h) as f32 > 0.01, "{from:?} → {to:?} em {p}: em branco");
                }
                // o Clawd agacha onde estava, sobe num arco acima dos dois lugares e pousa no novo
                let ((ax, ay, au), (bx, by, bu)) = (w.crab_spot(from), w.crab_spot(to));
                let (x, y, u, pose) = w.crab_between(from, to, 0.05);
                assert!((x, y, u) == (ax, ay, au) && pose == clawd::jump_pose(true));
                let (x, y, _, pose) = w.crab_between(from, to, (JUMP.0 + JUMP.1) / 2.0);
                assert!(y < (ay + by) / 2.0 - JUMP_H * 0.9 && pose == clawd::jump_pose(false), "{from:?} → {to:?}: não pula");
                assert!((x - (ax + bx) / 2.0).abs() < 1.0, "{from:?} → {to:?}: não vai em linha");
                let (x, y, u, _) = w.crab_between(from, to, 0.85);
                assert_eq!((x, y, u), (bx, by, bu));
                assert_eq!(w.crab_between(from, to, 1.0).3, w.crab_pose(to), "não volta ao que fazia");
                // a altura do pulo, mesmo saindo do título do grande, não passa muito do cartão
                assert!(ay.min(by) - JUMP_H >= -10.0);
            }
        }
        // sem números, cada tamanho tem um texto: fica a troca de sempre
        assert!(!Claude::sample_unlinked().morphs(Mode::P, Mode::M));
    }

    /// Folha com os quadros da troca de tamanho (pequeno → médio → grande → pequeno), com o
    /// Clawd trabalhando (%TEMP%\claude_troca.png):
    ///   cargo test --release claude_resize_sheet -- --ignored --nocapture
    #[test]
    #[ignore]
    fn claude_resize_sheet() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let w = Claude::sample_busy().with_crab(Anim::Type, 0.4);
        let steps = 9;
        let (cell_w, cell_h, s) = (340.0, 470.0, 1.25);
        let pairs = [(Mode::P, Mode::M), (Mode::M, Mode::G), (Mode::G, Mode::P)];
        let perch_h = PERCH_H + 20.0;
        let mut sheet = Canvas::new((cell_w * steps as f32 * s) as u32, ((cell_h * pairs.len() as f32 + 2.0 * perch_h) * s) as u32, s);
        sheet.fill_round_rect(0.0, 0.0, cell_w * steps as f32, cell_h * pairs.len() as f32 + 2.0 * perch_h, 0.0, Rgba::rgb(60, 50, 110));
        // embaixo: a janela da barra de tarefas em vários momentos (com a barra embaixo)
        let c = cfg();
        let env = Env { cfg: &c, key: "claude" };
        let mut editing = Claude::sample_busy().with_crab(Anim::Type, 0.5);
        editing.activity.tool = Some(Tool { name: "Edit".into(), input: r"C:\dev\src\claude.rs".into() });
        let mut running = Claude::sample_sherlock().with_crab(Anim::Investigate, 0.3);
        running.activity.tool = Some(Tool { name: "Bash".into(), input: "cargo test --release".into() });
        let states = [
            editing,
            running,
            Claude::sample_busy().with_crab(Anim::Think, 0.8),
            Claude::sample_busy().with_crab(Anim::Coffee, 0.5),
            Claude::sample_busy().with_crab(Anim::Coffee, 0.15),
            Claude::sample().with_crab(Anim::Done, 0.1),
            Claude::sample().with_crab(Anim::Done, 0.8),
            Claude::sample(),
            Claude::sample().idle_for(NAP_AFTER + 60.0).with_crab(Anim::Snooze, 0.3),
            Claude::sample_ko(),
        ];
        let base = cell_h * pairs.len() as f32;
        for (k, st) in states.iter().enumerate() {
            let (x, y) = ((k % 5) as f32 * (PERCH_W + 30.0) + 10.0, base + (k / 5) as f32 * perch_h + 4.0);
            sheet.fill_round_rect(x, y + PERCH_H, x + PERCH_W, y + PERCH_H + 14.0, 0.0, Rgba::rgb(28, 28, 34));
            let mut pc = Canvas::new((PERCH_W * s) as u32, (PERCH_H * s) as u32, s);
            Kind::draw_perch(st, &mut pc, &env);
            sheet.image(&pc.finish(), x, y);
        }
        for (row, &(from, to)) in pairs.iter().enumerate() {
            let (a, b) = (super::super::Layout::of(from), super::super::Layout::of(to));
            for k in 0..steps {
                let p = k as f32 / (steps - 1) as f32;
                let e = crate::theme::ease_spring(p);
                let (cw, ch) = (a.w + (b.w - a.w) * e, a.h + (b.h - a.h) * e);
                let img = frame(&w, from, to, Some(p), (cw, ch));
                let (x, y) = (k as f32 * cell_w + 10.0, row as f32 * cell_h + 10.0);
                sheet.fill_round_rect(x, y, x + cw, y + ch, 22.0, Rgba(32, 22, 72, 200));
                sheet.image(&img, x, y);
            }
        }
        let path = std::env::temp_dir().join("claude_troca.png");
        crate::gfx::save_png(&sheet.finish(), &path).unwrap();
        println!("{}", path.display());
    }

    #[test]
    fn new_parts_come_in_after_and_old_ones_leave_first() {
        // o que é novo entra depois de começar e chega inteiro no fim; o que sai some no começo
        assert_eq!(presence(false, true, 0.3, 0.45, 0.4, 0.3), 0.0);
        assert_eq!(presence(false, true, 1.0, 0.45, 0.4, 0.3), 1.0);
        assert_eq!(presence(true, false, 0.3, 0.45, 0.4, 0.3), 0.0);
        assert!(presence(true, false, 0.1, 0.45, 0.4, 0.3) > 0.5);
        assert_eq!(presence(true, true, 0.5, 0.45, 0.4, 0.3), 1.0);
        // o semanal nasce do zero, passa um pouco do tamanho e acaba no tamanho certo
        assert_eq!(ease_back(0.0), 0.0);
        assert!((ease_back(1.0) - 1.0).abs() < 1e-6);
        assert!((0..100).any(|k| ease_back(k as f32 / 100.0) > 1.02), "não passa do ponto");
    }

    #[test]
    fn says_what_the_crab_is_doing() {
        assert_eq!(Claude::sample().doing(), "", "parado: nada a dizer");
        assert_eq!(Claude::sample_busy().doing(), "programando");
        assert_eq!(Claude::sample_busy().with_crab(Anim::Think, 0.5).doing(), "pensando");
        assert_eq!(Claude::sample_busy().with_crab(Anim::Idea, 0.5).doing(), "teve uma ideia");
        assert_eq!(Claude::sample_busy().with_crab(Anim::Coffee, 0.5).doing(), "tomando café");
        assert_eq!(Claude::sample().with_crab(Anim::Done, 0.5).doing(), "pronto");
        assert_eq!(Claude::sample_sherlock().with_crab(Anim::Investigate, 0.5).doing(), "investigando");
        assert_eq!(Claude::sample().with_crab(Anim::Music, 0.5).doing(), "ouvindo música");
        assert_eq!(Claude::sample().with_crab(Anim::Party, 0.5).doing(), "limite reiniciou");
        assert_eq!(Claude::sample_ko().doing(), "lendo");
        assert_eq!(Claude::sample().idle_for(NAP_AFTER + 60.0).doing(), "cochilando");
        assert_eq!(Claude::sample_limit().doing(), "perto do limite");
    }

    #[test]
    fn crab_works_while_the_conversation_works_and_celebrates_the_end() {
        let dir = std::env::temp_dir().join(format!("widgets-clawd-turn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("conversa.jsonl");
        let prompt = r#"{"type":"user","message":{"role":"user","content":"faça o caderno"}}"#;
        let tool = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"tool_use","content":[]}}"#;
        let done = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"end_turn","content":[]}}"#;
        let write = |lines: &[&str], ago: u64| {
            std::fs::write(&path, lines.join("\n")).unwrap();
            let f = std::fs::File::options().write(true).open(&path).unwrap();
            f.set_modified(SystemTime::now() - Duration::from_secs(ago)).unwrap();
        };
        let tick = |w: &mut Claude| {
            w.due = Instant::now();
            let mut req = Vec::new();
            let (mut c, mut cache) = (cfg(), serde_json::json!({}));
            w.tick(&mut Ctx { cfg: &mut c, cache: &mut cache, key: "claude", out: &mut req })
        };
        // pediu e ele está usando ferramentas: trabalhando
        write(&[prompt, tool], 2);
        let mut w = Claude::sample().with_session(&path);
        assert_eq!(w.mood(), Mood::Busy);
        tick(&mut w);
        assert_eq!(w.shown_mood, Mood::Busy);
        assert!(w.due <= Instant::now() + CHECK_BUSY, "trabalhando, olha mais vezes");
        // um comando demorado (a conversa parada uns minutos) continua sendo trabalho, mesmo sem
        // a linha de status ser chamada nesse tempo
        let mut slow = Claude::sample().idle_for(5.0 * 60.0);
        write(&[prompt, tool], 4 * 60);
        slow = slow.with_session(&path).idle_for(4.0 * 60.0);
        assert_eq!(slow.mood(), Mood::Busy);
        // respondeu de vez: no próximo olhar ele para de escrever e comemora
        write(&[prompt, tool, done], 1);
        assert!(tick(&mut w));
        assert_eq!(w.shown_mood, Mood::Normal);
        assert!(w.crab.is_playing(Anim::Done), "não comemorou");
        // parada no meio há muito tempo (fechou, travou ou espera uma permissão): não trabalha
        write(&[prompt, tool], 11 * 60);
        assert_eq!(Claude::sample().with_session(&path).mood(), Mood::Normal);
        // sem as conversas (versão antiga da linha de status), vale a última chamada
        assert_eq!(Claude::sample_busy().mood(), Mood::Busy);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crab_hops_as_soon_as_the_button_goes_down() {
        for w in [Claude::sample(), Claude::sample_busy(), Claude::sample_unlinked()] {
            let mut w = w;
            let still = clawd::cells(&w.crab.pose(w.mood(), None, clawd::View::Side));
            // apertou em cima do Clawd: redesenha já, com ele saindo do lugar
            assert!(Kind::pressed(&mut w, POKE_CRAB));
            assert_ne!(clawd::cells(&w.crab.pose(w.mood(), None, clawd::View::Side)), still);
            // soltou: o clique não recomeça o pulo que já está acontecendo
            w.crab = clawd::Crab::playing(Anim::Hop, 0.4);
            let pose = w.crab.pose(w.mood(), None, clawd::View::Side);
            let mut req = Vec::new();
            let (mut c, mut cache) = (cfg(), serde_json::json!({}));
            w.act(POKE_CRAB, &mut Ctx { cfg: &mut c, cache: &mut cache, key: "claude", out: &mut req });
            assert_eq!(w.crab.pose(w.mood(), None, clawd::View::Side), pose);
            // outras áreas não reagem ao apertar
            assert!(!Kind::pressed(&mut w, OPEN_USAGE));
        }
    }

    #[test]
    fn crab_naps_when_claude_code_is_quiet() {
        assert_eq!(Claude::sample().mood(), Mood::Normal);
        assert_eq!(Claude::sample().idle_for(10.0 * 60.0).mood(), Mood::Normal, "10 min sem usar: acordado");
        assert_eq!(Claude::sample().idle_for(NAP_AFTER + 60.0).mood(), Mood::Sleepy, "muito tempo sem usar: cochila");
        assert_eq!(Claude::sample_unlinked().mood(), Mood::Sleepy, "sem números: dorme");
        assert_eq!(Claude::sample_limit().mood(), Mood::Worried);
        // o Claude Code respondeu agora há pouco: trabalhando (com o mouse em cima também)
        assert_eq!(Claude::sample_busy().mood(), Mood::Busy);
        assert_eq!(Claude::sample().idle_for(BUSY_FOR - 1.0).with_pointer(40.0, 40.0, Mode::P).mood(), Mood::Busy);
        assert_eq!(Claude::sample().idle_for(BUSY_FOR + 1.0).mood(), Mood::Normal, "parou: guarda o caderno");
        // perto do limite, suar vem antes
        assert_eq!(Claude::sample_limit().idle_for(5.0).mood(), Mood::Worried);
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
    fn clicking_the_crab_sends_it_to_the_taskbar_and_back() {
        let c = cfg();
        let env = Env { cfg: &c, key: "claude" };
        let act = |w: &mut Claude, action: u32| {
            let mut req = Vec::new();
            let (mut c2, mut cache) = (cfg(), serde_json::json!({}));
            Kind::act(w, action, &mut Ctx { cfg: &mut c2, cache: &mut cache, key: "claude", out: &mut req });
            req
        };
        for (name, sample) in [("com números", Claude::sample as fn() -> Claude), ("sem conectar", Claude::sample_unlinked)] {
            for mode in Mode::ALL {
                let mut w = sample();
                let (cx, cy, _) = w.crab_spot(mode);
                // o clique nele vale mais que o do medidor embaixo; ao apertar, agacha para o pulo
                assert_eq!(Kind::hit(&w, cx, cy, mode, &env), Some(POKE_CRAB), "{name} {mode:?}");
                assert!(Kind::pressed(&mut w, POKE_CRAB) && w.crab.animating(), "{name} {mode:?}: não pulou");
                // ao soltar, pede para ir para a barra
                assert!(matches!(act(&mut w, POKE_CRAB)[..], [Request::Perch(true)]), "{name} {mode:?}");
                // lá: o lugar dele fica vazio, mas clicável; o clique o traz de volta
                Kind::set_perched(&mut w, true);
                assert_eq!(Kind::hit(&w, cx, cy, mode, &env), Some(POKE_CRAB), "{name} {mode:?}: sem como trazer de volta");
                assert!(!Kind::pressed(&mut w, POKE_CRAB));
                assert!(matches!(act(&mut w, POKE_CRAB)[..], [Request::Perch(false)]), "{name} {mode:?}");
            }
        }
        // pelo clique direito também
        let mut w = Claude::sample();
        assert!(Kind::menu(&w, &env).iter().any(|m| matches!(m, MenuItem::Cmd(t, PERCH_TOGGLE) if t.contains("barra de tarefas"))));
        assert!(matches!(act(&mut w, PERCH_TOGGLE)[..], [Request::Perch(true)]));
    }

    #[test]
    fn the_spot_stays_empty_while_the_crab_is_on_the_taskbar() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let c = cfg();
        let env = Env { cfg: &c, key: "claude" };
        for mode in Mode::ALL {
            let l = super::super::Layout::of(mode);
            let (cx, cy, unit) = Claude::sample_busy().crab_spot(mode);
            // a área do Clawd, sem os textos em volta
            let (x0, y0) = (((cx - 6.0 * unit) * 1.25) as u32, ((cy - 3.0 * unit) * 1.25) as u32);
            let orange = |w: &Claude| {
                let mut canvas = Canvas::new((l.w * 1.25) as u32, (l.h * 1.25) as u32, 1.25);
                w.draw(&mut canvas, mode, &env);
                let img = canvas.finish();
                let mut n = 0;
                for y in y0..y0 + (6.0 * unit * 1.25) as u32 {
                    for x in x0..x0 + (12.0 * unit * 1.25) as u32 {
                        let px = &img.px[((y * img.w + x) * 4) as usize..][..4];
                        // (BGRA) o laranja do corpo
                        n += (px[2] > 180 && px[1] > 90 && px[1] < 140 && px[0] < 110) as u32;
                    }
                }
                n
            };
            let mut w = Claude::sample_busy();
            assert!(orange(&w) > 20, "{mode:?}: o Clawd não aparece");
            Kind::set_perched(&mut w, true);
            assert_eq!(orange(&w), 0, "{mode:?}: o Clawd continua no widget");
        }
        // e o lugar diz onde ele está
        let mut w = Claude::sample();
        Kind::set_perched(&mut w, true);
        assert_eq!(w.doing_here(), "na barra de tarefas");
        assert!(w.subtitle(w.now()).ends_with("na barra de tarefas"));
    }

    #[test]
    fn the_balloon_says_what_claude_code_is_doing() {
        let tool = |name: &str, input: &str| Tool { name: name.into(), input: input.into() };
        let says = |t: Tool, names: bool| tool_line(&t, names).0;
        assert_eq!(says(tool("Edit", r"C:\dev\src\widgets\claude.rs"), true), "Editando claude.rs");
        assert_eq!(says(tool("Write", "/home/eu/notas.md"), true), "Editando notas.md");
        assert_eq!(says(tool("Read", "README.md"), true), "Lendo README.md");
        assert_eq!(says(tool("Bash", "cargo test --release"), true), "Rodando cargo test");
        assert_eq!(says(tool("Grep", "crab_spot"), true), "Procurando \"crab_spot\"");
        assert_eq!(says(tool("WebFetch", "https://x"), true), "Pesquisando na web");
        assert_eq!(says(tool("mcp__ai-memory__memory_query", ""), true), "Usando memory_query");
        // sem nomes: só a ação (para compartilhar a tela)
        assert_eq!(says(tool("Edit", "claude.rs"), false), "Editando um arquivo");
        assert_eq!(says(tool("Bash", "rm -rf segredo"), false), "Rodando um comando");
        assert_eq!(says(tool("Grep", "senha"), false), "Procurando no código");
        // nomes compridos são cortados
        assert!(says(tool("Read", &"x".repeat(80)), true).chars().count() < 40);
        assert!(show_names(&serde_json::json!({})) && !show_names(&serde_json::json!({"claude": {"balao_nomes": false}})));
        // o balão: pensando sem ferramenta, a ferramenta quando há, pronto no fim, nada parado
        let mut w = Claude::sample_busy();
        assert_eq!(w.balloon(true).unwrap().0, "Pensando…");
        w.activity.tool = Some(tool("Edit", "clawd.rs"));
        assert_eq!(w.balloon(true).unwrap().0, "Editando clawd.rs");
        assert_eq!(w.with_crab(Anim::Coffee, 0.5).balloon(true).unwrap().0, "Hora do café");
        assert_eq!(Claude::sample().with_crab(Anim::Done, 0.5).balloon(true).unwrap().0, "✓ Pronto");
        assert!(Claude::sample().balloon(true).is_none());
    }

    #[test]
    fn the_taskbar_window_has_the_crab_and_the_balloon() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let c = cfg();
        let env = Env { cfg: &c, key: "claude" };
        let draw = |w: &Claude| {
            let mut canvas = Canvas::new((PERCH_W * 1.25) as u32, (PERCH_H * 1.25) as u32, 1.25);
            Kind::draw_perch(w, &mut canvas, &env);
            canvas.finish()
        };
        let ink = |img: &crate::gfx::Image, y0: f32, y1: f32| {
            let rows = (y0 * 1.25) as u32..(y1 * 1.25) as u32;
            rows.flat_map(|y| (0..img.w).map(move |x| (x, y))).filter(|&(x, y)| img.px[((y * img.w + x) * 4 + 3) as usize] > 8).count()
        };
        // trabalhando: o balão em cima e o Clawd com o notebook embaixo
        let busy = draw(&Claude::sample_busy());
        assert!(ink(&busy, 0.0, BALLOON_BOTTOM) > 500, "sem balão");
        assert!(ink(&busy, BALLOON_BOTTOM + 8.0, PERCH_H) > 500, "sem o Clawd");
        // parado, só o Clawd; o desenho muda de chave quando algo muda
        let idle = Claude::sample();
        assert_eq!(ink(&draw(&idle), 0.0, BALLOON_BOTTOM - 4.0), 0, "balão sem nada a dizer");
        assert_ne!(Kind::perch_key(&idle, &env), Kind::perch_key(&Claude::sample_busy(), &env));
        assert_eq!(Kind::perch_key(&idle, &env), Kind::perch_key(&idle, &env));
        // tudo dentro da janela: o notebook chega perto da borda direita e as patas na de baixo
        let (cx, cy) = PERCH_CRAB;
        let c = clawd::CELL * PERCH_UNIT;
        assert!(cx + (clawd::SIDE_COLS - clawd::COLS / 2.0) * c <= PERCH_W - 4.0);
        assert!((cy + clawd::ROWS / 2.0 * c - PERCH_H).abs() < 0.01);
        // (no limite, a placa passa 2 colunas do notebook)
        assert!(cx + (clawd::SIDE_COLS + 2.0 - clawd::COLS / 2.0) * c <= PERCH_W);
        // o chapéu e a lâmpada vão até 6 quadradinhos acima da grade
        assert!(cy - (clawd::ROWS / 2.0 + 6.0) * c > BALLOON_BOTTOM + 6.0, "o balão encosta no Clawd (ou na lâmpada)");
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
