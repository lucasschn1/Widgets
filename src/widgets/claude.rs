//! Widget "Uso do Claude": quanto da sessão de 5 horas e da semana do Claude já foi usado, em
//! dois medidores de 270° (a sessão por fora, a semana por dentro) com o símbolo do Claude na
//! abertura de baixo. Os números vêm do Claude Code (ver `claude_usage`).
//!
//! Cores próprias, claras: fundo areia, a sessão em caramelo, a semana em quase preto e o texto
//! em marrom.

use std::cell::RefCell;
use std::time::{Duration, Instant, SystemTime};

use super::{Ctx, Env, Hot, HoverStyle, Kind, MenuItem, Mode, Status, SMALL, WIDE};
use crate::claude_usage::{self as usage, Link, Usage, Window, FIVE_HOURS, SEVEN_DAYS};
use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::Rgba;
use crate::win;

// ---------- cores ----------
/// fundo
pub const BG: Rgba = Rgba::rgb(0xE8, 0xD5, 0xB0);
/// destaque: a sessão e o símbolo
pub const ACCENT: Rgba = Rgba::rgb(0xD4, 0x95, 0x5A);
/// a semana e os números
pub const SECONDARY: Rgba = Rgba::rgb(0x1A, 0x1A, 0x1A);
/// textos
pub const BODY: Rgba = Rgba::rgb(0x5A, 0x45, 0x35);
/// perto do limite (85%) e no limite (95%)
const ALERT: Rgba = Rgba::rgb(0xB8, 0x4A, 0x2A);
const ALERT_STRONG: Rgba = Rgba::rgb(0x9E, 0x2A, 0x1E);
const TRACK: Rgba = Rgba(0x5A, 0x45, 0x35, 36);

// ---------- ações ----------
pub const CONNECT: u32 = 1;
pub const DISCONNECT: u32 = 2;
const OPEN_USAGE: u32 = 3;
const USAGE_URL: &str = "https://claude.ai/settings/usage";

/// Os medidores vão de 135° (embaixo à esquerda) até 405° (embaixo à direita), pelo alto.
const START: f32 = 135.0;
const SWEEP: f32 = 270.0;

/// Geometria dos medidores em cada tamanho: centro, raio de fora, raio de dentro, espessura,
/// tamanho do número e raio do símbolo.
struct Geom {
    cx: f32,
    cy: f32,
    ro: f32,
    ri: f32,
    w: f32,
    num: f32,
    mark: f32,
}

fn geom(mode: Mode) -> Geom {
    match mode {
        Mode::P => Geom { cx: SMALL / 2.0, cy: 78.0, ro: 54.0, ri: 40.0, w: 8.0, num: 26.0, mark: 9.0 },
        Mode::M => Geom { cx: 80.0, cy: 74.0, ro: 57.0, ri: 42.0, w: 9.0, num: 24.0, mark: 10.0 },
        Mode::G => Geom { cx: WIDE / 2.0, cy: 150.0, ro: 110.0, ri: 88.0, w: 13.0, num: 58.0, mark: 15.0 },
    }
}

/// Botão "Conectar ao Claude Code" em cada tamanho: (x0, y0, x1, y1).
fn connect_rect(mode: Mode) -> (f32, f32, f32, f32) {
    match mode {
        Mode::P => (14.0, SMALL - 40.0, SMALL - 14.0, SMALL - 14.0),
        Mode::M => (164.0, 96.0, WIDE - 18.0, 124.0),
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
}

impl Claude {
    pub fn new() -> Claude {
        let mut c = Claude { view: View { usage: None, link: Link::Off }, seen: None, note: None, due: Instant::now(), fixed_now: None, minute: RefCell::new(0) };
        c.reload(true);
        c
    }

    /// Com números de exemplo (galeria e imagens): sessão em 46% (reinicia em 2 h 15), semana
    /// em 31% (reinicia daqui a 1 dia).
    pub fn sample() -> Claude {
        Claude::sample_with(0.46, 0.31, 2.25 * 3600.0)
    }

    /// Perto do limite: sessão em 88% com 2 h pela frente, semana em 74%.
    #[cfg(test)]
    pub fn sample_limit() -> Claude {
        Claude::sample_with(0.88, 0.74, 2.0 * 3600.0)
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
        Claude { view: View { usage: Some(usage), link: Link::Connected }, seen: None, note: None, due: Instant::now(), fixed_now: Some(now), minute: RefCell::new(0) }
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
        self.view = view;
        changed
    }

    fn session(&self) -> Option<Window> {
        self.view.usage.as_ref().and_then(|u| u.five_hour)
    }

    fn week(&self) -> Option<Window> {
        self.view.usage.as_ref().and_then(|u| u.seven_day)
    }

    fn has_data(&self) -> bool {
        self.session().is_some() || self.week().is_some()
    }

    /// Mostra o botão de conectar (ainda não ligado e sem números).
    fn needs_connect(&self) -> bool {
        !self.has_data() && self.view.link != Link::Connected
    }

    fn tone(v: f64, base: Rgba) -> Rgba {
        if v >= 0.95 {
            ALERT_STRONG
        } else if v >= 0.85 {
            ALERT
        } else {
            base
        }
    }

    /// Começo do degradê de cada medidor (mais claro, puxado para o fundo).
    fn light(color: Rgba) -> Rgba {
        color.mix(BG, 0.42)
    }

    /// O limite mais perto de acabar: (nome, uso).
    fn critical(&self, now: f64) -> Option<(&'static str, f64)> {
        let s = self.session().map(|w| w.used_at(now));
        let w = self.week().map(|w| w.used_at(now));
        match (s, w) {
            (Some(s), Some(w)) if w > s => Some(("semana", w)),
            (Some(s), _) => Some(("sessão", s)),
            (None, Some(w)) => Some(("semana", w)),
            _ => None,
        }
    }

    /// Nota da sessão: quando acaba (se acabar antes de reiniciar) ou quando reinicia.
    fn session_note(&self, now: f64) -> (String, bool) {
        let Some(w) = self.session() else { return ("Sem dados".into(), false) };
        if now >= w.resets_at {
            return ("Livre: começa no próximo uso".into(), false);
        }
        match w.runs_out_at(now, FIVE_HOURS) {
            Some(t) if t <= now => (format!("No limite · reinicia às {}", win::clock_at(w.resets_at)), true),
            Some(t) => (format!("Acaba às {}", win::clock_at(t)), true),
            None => (format!("Reinicia às {}", win::clock_at(w.resets_at)), false),
        }
    }

    fn week_note(&self, now: f64) -> String {
        match self.week() {
            None => "Sem dados".into(),
            Some(w) if now >= w.resets_at => "Renovada".into(),
            Some(w) => format!("Reinicia {}", day_and_time(w.resets_at, now)),
        }
    }

    /// A frase do ritmo da sessão (e se é um aviso).
    fn forecast(&self, now: f64) -> Option<(String, bool)> {
        let w = self.session()?;
        if now >= w.resets_at {
            return None;
        }
        Some(match w.runs_out_at(now, FIVE_HOURS) {
            Some(t) if t <= now => (format!("Limite da sessão atingido · reinicia às {}", win::clock_at(w.resets_at)), true),
            Some(t) => (format!("No ritmo atual, acaba às {}", win::clock_at(t)), true),
            None => (format!("No ritmo atual, dura até reiniciar, às {}", win::clock_at(w.resets_at)), false),
        })
    }

    /// Linha de baixo: de onde vêm os números, ou há quanto tempo não chegam.
    fn source_line(&self, now: f64) -> String {
        match &self.view.usage {
            Some(u) if now - u.updated > 15.0 * 60.0 => format!("Claude Code · atualizado às {}", win::clock_at(u.updated)),
            _ => "Claude Code".into(),
        }
    }

    // ---------- desenho ----------

    fn draw_background(c: &mut Canvas, mode: Mode) {
        let l = super::Layout::of(mode);
        c.fill_round_rect(0.0, 0.0, l.w, l.h, l.radius, BG.alpha(0.96));
        // um pouco de luz no alto
        c.gradient_round_rect(0.0, 0.0, l.w, l.h, l.radius, &[(0.0, Rgba(255, 255, 255, 46)), (0.5, Rgba(255, 255, 255, 0))], 0.0, l.h);
    }

    /// Um medidor: trilho, o arco até `frac` em degradê e o tracinho de quanto da janela passou.
    #[allow(clippy::too_many_arguments)]
    fn gauge(c: &mut Canvas, cx: f32, cy: f32, r: f32, w: f32, frac: f32, elapsed: Option<f32>, color: Rgba) {
        c.arc(cx, cy, r, START, START + SWEEP, TRACK, w);
        let frac = frac.clamp(0.0, 1.0);
        if frac > 0.0 {
            let from = Self::light(color);
            let steps = ((SWEEP * frac / 4.0).ceil() as usize).max(2);
            for k in 0..steps {
                let a0 = START + SWEEP * frac * k as f32 / steps as f32;
                let a1 = START + SWEEP * frac * (k + 1) as f32 / steps as f32;
                c.arc(cx, cy, r, a0, a1, from.mix(color, k as f32 / (steps - 1) as f32), w);
            }
        }
        if let Some(e) = elapsed {
            let a = (START + SWEEP * e).to_radians();
            let (dx, dy) = (a.cos(), a.sin());
            let (r0, r1) = (r + w / 2.0 + 2.0, r + w / 2.0 + 6.0);
            c.line(&[(cx + dx * r0, cy + dy * r0), (cx + dx * r1, cy + dy * r1)], SECONDARY.alpha(0.55), 1.5);
        }
    }

    fn draw_gauges(&self, c: &mut Canvas, mode: Mode, now: f64) {
        let g = geom(mode);
        let s = self.session();
        let wk = self.week();
        let sv = s.map_or(0.0, |w| w.used_at(now));
        let wv = wk.map_or(0.0, |w| w.used_at(now));
        let se = s.and_then(|w| w.elapsed_at(now, FIVE_HOURS)).map(|e| e as f32);
        let we = wk.and_then(|w| w.elapsed_at(now, SEVEN_DAYS)).map(|e| e as f32);
        Self::gauge(c, g.cx, g.cy, g.ro, g.w, sv as f32, se, Self::tone(sv, ACCENT));
        Self::gauge(c, g.cx, g.cy, g.ri, g.w, wv as f32, we, Self::tone(wv, SECONDARY));
        mark(c, g.cx, g.cy + g.ro * 0.92, g.mark, ACCENT);
        match self.critical(now) {
            Some((name, v)) => {
                big(c, g.cx, g.cy - g.num * 0.66, v, g.num, 0.5);
                c.text(g.cx, g.cy + g.num * 0.48, name, Face::Regular, (g.num * 0.38).clamp(10.0, 12.5), BODY, Anchor::MA, None);
            }
            None => {
                c.text(g.cx, g.cy - g.num * 0.6, "–", Face::Light, g.num, BODY.alpha(0.6), Anchor::MA, None);
            }
        }
    }

    /// Linha de um limite: ponto da cor, nome, o número à direita e a nota embaixo.
    #[allow(clippy::too_many_arguments)]
    fn line(c: &mut Canvas, x: f32, y: f32, w: f32, name: &str, v: Option<f64>, note: &str, warn: bool, dot: Rgba) {
        c.fill_circle(x + 3.5, y + 8.0, 3.2, dot);
        c.text(x + 13.0, y, name, Face::Semibold, 12.5, SECONDARY, Anchor::LA, None);
        match v {
            Some(v) => {
                big(c, x + w, y - 4.0, v, 20.0, 1.0);
            }
            None => {
                c.text(x + w, y - 2.0, "–", Face::Light, 18.0, BODY.alpha(0.6), Anchor::RA, None);
            }
        }
        c.text(x + 13.0, y + 18.0, note, Face::Regular, 10.5, if warn { ALERT } else { BODY }, Anchor::LA, Some(w - 13.0));
    }

    fn draw_connect(&self, c: &mut Canvas, mode: Mode) {
        let (x0, y0, x1, y1) = connect_rect(mode);
        c.fill_round_rect(x0, y0, x1, y1, (y1 - y0) / 2.0, SECONDARY);
        let label = if mode == Mode::P { "Conectar" } else { "Conectar ao Claude Code" };
        c.text((x0 + x1) / 2.0, (y0 + y1) / 2.0, label, Face::Semibold, 11.5, BG, Anchor::MM, None);
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
            Link::Off => ("Uso do seu plano", "Sessão de 5 horas e semana, direto do Claude Code.".into()),
        }
    }

    fn draw_small(&self, c: &mut Canvas, now: f64) {
        self.draw_gauges(c, Mode::P, now);
        if self.needs_connect() && self.view.link != Link::Missing {
            self.draw_connect(c, Mode::P);
        }
    }

    fn draw_medium(&self, c: &mut Canvas, now: f64) {
        self.draw_gauges(c, Mode::M, now);
        let (x, w) = (164.0, WIDE - 18.0 - 164.0);
        if !self.has_data() {
            let (title, text) = self.waiting_text();
            c.text(x, 26.0, title, Face::Semibold, 12.5, SECONDARY, Anchor::LA, Some(w));
            let lines = c.wrap(&text, Face::Regular, 10.5, w, 3);
            for (i, l) in lines.iter().enumerate() {
                c.text(x, 46.0 + i as f32 * 14.0, l, Face::Regular, 10.5, BODY, Anchor::LA, None);
            }
            if self.needs_connect() && self.view.link == Link::Off {
                self.draw_connect(c, Mode::M);
            }
            return;
        }
        let (s_note, s_warn) = self.session_note(now);
        let sv = self.session().map(|w| w.used_at(now));
        let wv = self.week().map(|w| w.used_at(now));
        Self::line(c, x, 24.0, w, "Sessão", sv, &s_note, s_warn, Self::tone(sv.unwrap_or(0.0), ACCENT));
        Self::line(c, x, 72.0, w, "Semana", wv, &self.week_note(now), false, Self::tone(wv.unwrap_or(0.0), SECONDARY));
        c.text(x, 122.0, &self.source_line(now), Face::Regular, 10.5, BODY.alpha(0.8), Anchor::LA, Some(w));
    }

    fn draw_large(&self, c: &mut Canvas, now: f64) {
        self.draw_gauges(c, Mode::G, now);
        let g = geom(Mode::G);
        let pad = 18.0;
        if !self.has_data() {
            let (title, text) = self.waiting_text();
            let top = 292.0;
            c.text(g.cx, top, title, Face::Semibold, 13.0, SECONDARY, Anchor::MA, Some(WIDE - 2.0 * pad));
            for (i, l) in c.wrap(&text, Face::Regular, 11.0, WIDE - 2.0 * pad - 20.0, 3).iter().enumerate() {
                c.text(g.cx, top + 22.0 + i as f32 * 15.0, l, Face::Regular, 11.0, BODY, Anchor::MA, None);
            }
            if self.needs_connect() && self.view.link == Link::Off {
                self.draw_connect(c, Mode::G);
            }
            return;
        }
        c.text(g.cx, g.cy + 48.0, &self.source_line(now), Face::Regular, 10.5, BODY.alpha(0.75), Anchor::MA, None);
        let mid = WIDE / 2.0;
        let (s_note, s_warn) = self.session_note(now);
        let sv = self.session().map(|w| w.used_at(now));
        let wv = self.week().map(|w| w.used_at(now));
        for (x0, x1, name, v, note, warn, base) in [
            (pad, mid - 5.0, "Sessão", sv, s_note, s_warn, ACCENT),
            (mid + 5.0, WIDE - pad, "Semana", wv, self.week_note(now), false, SECONDARY),
        ] {
            c.fill_round_rect(x0, 290.0, x1, 378.0, 16.0, BODY.alpha(0.07));
            c.stroke_round_rect(x0 + 0.5, 290.5, x1 - 0.5, 377.5, 16.0, BODY.alpha(0.14), 1.0);
            c.fill_circle(x0 + 17.0, 307.0, 3.2, Self::tone(v.unwrap_or(0.0), base));
            c.text(x0 + 26.0, 299.0, name, Face::Semibold, 11.5, SECONDARY, Anchor::LA, None);
            match v {
                Some(v) => {
                    big(c, x0 + 13.0, 318.0, v, 30.0, 0.0);
                }
                None => {
                    c.text(x0 + 14.0, 318.0, "–", Face::Light, 28.0, BODY.alpha(0.6), Anchor::LA, None);
                }
            }
            c.text(x0 + 14.0, 356.0, &note, Face::Regular, 10.0, if warn { ALERT } else { BODY }, Anchor::LA, Some(x1 - x0 - 24.0));
        }
        if let Some((text, warn)) = self.forecast(now) {
            c.text(g.cx, 398.0, &text, Face::Regular, 11.0, if warn { ALERT } else { BODY }, Anchor::MA, Some(WIDE - 2.0 * pad));
        }
    }
}

impl Default for Claude {
    fn default() -> Self {
        Claude::new()
    }
}

/// "46" grande e fino com o "%" pequeno; `ax`: 0 alinha pela esquerda, 0.5 centraliza, 1 pela
/// direita.
fn big(c: &mut Canvas, x: f32, y: f32, v: f64, size: f32, ax: f32) -> f32 {
    let num = format!("{}", (v * 100.0).round());
    let small = size * 0.42;
    let nw = c.text_width(&num, Face::Light, size);
    let pw = c.text_width("%", Face::Light, small);
    let total = nw + 1.0 + pw;
    let x0 = x - total * ax;
    c.text(x0, y, &num, Face::Light, size, SECONDARY, Anchor::LA, None);
    c.text(x0 + nw + 1.0, y + size * 0.2, "%", Face::Light, small, BODY, Anchor::LA, None);
    total
}

/// O símbolo do Claude: raios que afinam nas pontas, de comprimentos um pouco diferentes,
/// saindo do centro.
pub fn mark(c: &mut Canvas, cx: f32, cy: f32, r: f32, color: Rgba) {
    const RAYS: [(f32, f32); 12] = [
        (0.0, 1.0), (31.0, 0.74), (58.0, 0.93), (92.0, 0.7), (118.0, 0.97), (151.0, 0.78),
        (180.0, 0.9), (208.0, 0.72), (242.0, 1.0), (268.0, 0.76), (299.0, 0.92), (329.0, 0.7),
    ];
    for (deg, len) in RAYS {
        let a = (deg - 90.0f32).to_radians();
        let (dx, dy) = (a.cos(), a.sin());
        let (px, py) = (-dy, dx);
        let base = r * 0.15;
        let tip_w = r * 0.05;
        let tip = (cx + dx * r * len, cy + dy * r * len);
        let mid = (cx + dx * r * len * 0.5, cy + dy * r * len * 0.5);
        c.polygon(
            &[
                (cx + px * base, cy + py * base),
                (mid.0 + px * base * 0.6, mid.1 + py * base * 0.6),
                (tip.0 + px * tip_w, tip.1 + py * tip_w),
                (tip.0 - px * tip_w, tip.1 - py * tip_w),
                (mid.0 - px * base * 0.6, mid.1 - py * base * 0.6),
                (cx - px * base, cy - py * base),
            ],
            color,
        );
        c.fill_circle(tip.0, tip.1, tip_w, color);
    }
    c.fill_circle(cx, cy, r * 0.2, color);
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
        Self::draw_background(c, mode);
        match mode {
            Mode::P => self.draw_small(c, now),
            Mode::M => self.draw_medium(c, now),
            Mode::G => self.draw_large(c, now),
        }
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        if self.needs_connect() && self.view.link == Link::Off {
            let (x0, y0, x1, y1) = connect_rect(mode);
            if (x0..=x1).contains(&x) && (y0..=y1).contains(&y) {
                return Some(CONNECT);
            }
            return None;
        }
        let g = geom(mode);
        let r = g.ro + g.w;
        let on_gauges = (x - g.cx).hypot(y - g.cy) <= r;
        (self.has_data() && on_gauges && env.shortcuts()).then_some(OPEN_USAGE)
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<Hot> {
        match self.hit(x, y, mode, env)? {
            CONNECT => {
                let (x0, y0, x1, y1) = connect_rect(mode);
                Some((x0, y0, x1, y1, (y1 - y0) / 2.0))
            }
            _ => {
                let g = geom(mode);
                let r = g.ro + g.w;
                Some((g.cx - r, g.cy - r, g.cx + r, g.cy + r, r))
            }
        }
    }

    fn hover_style(&self, x: f32, y: f32, mode: Mode, env: &Env) -> HoverStyle {
        if self.hit(x, y, mode, env) == Some(OPEN_USAGE) { HoverStyle::Quiet } else { HoverStyle::Surface }
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
        Some(self.due)
    }

    fn tick(&mut self, _ctx: &mut Ctx) -> bool {
        self.due = Instant::now() + Duration::from_secs(5);
        let changed = self.reload(false);
        // os textos de tempo ("acaba às", "atualizado às") mudam com o relógio
        let minute = (self.now() / 60.0) as i64;
        changed || minute != *self.minute.borrow()
    }

    fn status(&self) -> Status {
        Status { loading: false, error: None, updated: self.view.usage.as_ref().map(|u| u.updated) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_limit_and_notes() {
        let c = Claude::sample();
        let now = c.now();
        assert_eq!(c.critical(now), Some(("sessão", 0.46)));
        let (note, warn) = c.session_note(now);
        assert!(note.starts_with("Reinicia às "), "{note}");
        assert!(!warn);
        assert!(c.week_note(now).starts_with("Reinicia amanhã, ") || c.week_note(now).starts_with("Reinicia hoje, "), "{}", c.week_note(now));
        let (f, warn) = c.forecast(now).unwrap();
        assert!(f.starts_with("No ritmo atual, dura até reiniciar"), "{f}");
        assert!(!warn);
        // perto do limite: o ritmo acaba antes do reinício
        let c = Claude::sample_limit();
        let (note, warn) = c.session_note(now);
        assert!(note.starts_with("Acaba às ") && warn, "{note}");
        assert_eq!(Claude::tone(0.88, ACCENT), ALERT);
        assert_eq!(Claude::tone(0.5, SECONDARY), SECONDARY);
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
}
