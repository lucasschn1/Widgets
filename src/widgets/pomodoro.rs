//! Widget de relógio com timer Pomodoro.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{Ctx, Env, Hot, Kind, MenuItem, Mode, NotifyAction, LARGE_H, MEDIUM_H, SMALL, WIDE};
use crate::config::Get;
use crate::drawing;
use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::*;
use crate::util;
use crate::win;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Foco,
    Pausa,
    Longa,
}

impl Phase {
    pub fn code(self) -> &'static str {
        match self {
            Phase::Foco => "foco",
            Phase::Pausa => "pausa",
            Phase::Longa => "longa",
        }
    }
    fn from_code(s: &str) -> Phase {
        match s {
            "pausa" => Phase::Pausa,
            "longa" => Phase::Longa,
            _ => Phase::Foco,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Phase::Foco => "Foco",
            Phase::Pausa => "Pausa",
            Phase::Longa => "Pausa longa",
        }
    }
    pub fn color(self) -> Rgba {
        match self {
            Phase::Foco => RED,
            Phase::Pausa => GREEN,
            Phase::Longa => BLUE,
        }
    }
    fn default_minutes(self) -> i64 {
        match self {
            Phase::Foco => 25,
            Phase::Pausa => 5,
            Phase::Longa => 15,
        }
    }
    fn choices(self) -> &'static [i64] {
        match self {
            Phase::Foco => &[15, 20, 25, 30, 40, 45, 50, 60, 90],
            Phase::Pausa => &[3, 5, 10, 15],
            Phase::Longa => &[10, 15, 20, 30],
        }
    }
}

const PHASES: [Phase; 3] = [Phase::Foco, Phase::Pausa, Phase::Longa];

// ações (cliques, menu e notificações)
pub const TOGGLE: u32 = 1;
pub const RESET: u32 = 2;
pub const SKIP: u32 = 3;
const OPEN_CLOCK: u32 = 4;
pub const RESET_DAY: u32 = 5;
pub const START_IF_IDLE: u32 = 6;
const TOGGLE_AUTO: u32 = 7;
const DURATION: u32 = 1000; // + fase * 1000 + minutos
const EVERY: u32 = 900; // + n

const W: f32 = WIDE;
const PAD: f32 = super::CARD_PAD;
const C_RING: (f32, f32, f32, f32) = (SMALL / 2.0, 62.0, 104.0, 9.0); // cx, cy, diâmetro, espessura
const M_RING: (f32, f32, f32, f32) = (77.0, MEDIUM_H / 2.0, 110.0, 8.0);
const M_COL_X: f32 = 150.0; // coluna da direita (relógio e botões)
const M_CTRL_Y: f32 = 120.0;
const M_DOTS_Y: f32 = 86.0;
const M_PLAY_D: f32 = 34.0;
// grande: cabeçalho, o anel grande no meio, os ciclos, os botões e, no pé, a próxima fase
const G_HEAD_LINE: f32 = 92.0;
const G_RING: (f32, f32, f32, f32) = (W / 2.0, 206.0, 176.0, 12.0);
const G_DOTS_Y: f32 = 320.0;
const G_CTRL_Y: f32 = 362.0;
const G_PLAY_D: f32 = 44.0;
const G_FOOT_Y: f32 = LARGE_H - 24.0;
const MIN_S: f64 = 60.0;
const MAX_S: f64 = 180.0 * 60.0;

pub fn fmt_timer(seconds: f64) -> String {
    let s = seconds.max(0.0).ceil() as i64;
    let (h, m, sec) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m:02}:{sec:02}")
    }
}

pub fn fmt_minutes(minutes: f64) -> String {
    let total = minutes.round() as i64;
    let (h, m) = (total / 60, total % 60);
    match (h, m) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h{m:02}"),
    }
}

#[derive(Clone, Copy)]
enum Control {
    Restart,
    Toggle(bool), // true = está correndo (mostra pausa)
    Skip,
}

pub struct Pomodoro {
    pub phase: Phase,
    pub cycle: i64,
    pub total: f64,
    pub remaining: f64,
    /// horário (segundos desde 1970) em que a fase acaba, enquanto está correndo
    pub end: Option<f64>,
    /// próximo `tick` (guardado: calcular na hora da pergunta empurraria o horário para sempre)
    next: Instant,
}

impl Pomodoro {
    pub fn new(cfg: &Value) -> Pomodoro {
        let wcfg = &cfg["pomodoro"];
        let state = &wcfg["estado"];
        let phase = Phase::from_code(state.str_of("fase").unwrap_or("foco"));
        let total = state.f64_of("total").filter(|t| *t > 0.0).unwrap_or(duration(wcfg, phase) as f64 * 60.0);
        let mut p = Pomodoro {
            phase,
            cycle: state.i64_of("ciclo").unwrap_or(0),
            total,
            remaining: state.f64_of("restante").unwrap_or(total),
            end: state.f64_of("fim"),
            next: Instant::now(),
        };
        p.schedule();
        p
    }

    /// Correndo: na próxima virada de segundo; parado: na próxima virada de minuto (só o relógio muda).
    fn schedule(&mut self) {
        let now = win::now();
        let delay = match self.end {
            Some(end) => {
                let d = (end - now).rem_euclid(1.0);
                if d < 0.001 { 1.0 } else { d }
            }
            None => 60.0 - now.rem_euclid(60.0),
        };
        self.next = Instant::now() + Duration::from_millis((delay * 1000.0) as u64 + 15);
    }

    pub fn running(&self) -> bool {
        self.end.is_some()
    }

    pub fn left(&self) -> f64 {
        match self.end {
            Some(end) => (end - win::now()).max(0.0),
            None => self.remaining,
        }
    }

    fn started(&self) -> bool {
        self.running() || self.remaining < self.total - 0.5
    }

    fn every(wcfg: &Value) -> i64 {
        wcfg.i64_of("longa_a_cada").unwrap_or(4)
    }

    fn next_phase(&self, wcfg: &Value) -> Phase {
        if self.phase == Phase::Foco {
            if self.cycle >= Self::every(wcfg) {
                Phase::Longa
            } else {
                Phase::Pausa
            }
        } else {
            Phase::Foco
        }
    }

    /// A fase que vem quando esta acabar sozinha (a pausa longa depois do último foco do ciclo).
    fn upcoming(&self, wcfg: &Value) -> Phase {
        match self.phase {
            Phase::Foco if self.cycle + 1 >= Self::every(wcfg) => Phase::Longa,
            Phase::Foco => Phase::Pausa,
            _ => Phase::Foco,
        }
    }

    fn set_phase(&mut self, phase: Phase, wcfg: &Value) {
        self.phase = phase;
        self.total = duration(wcfg, phase) as f64 * 60.0;
        self.remaining = self.total;
        self.end = None;
    }

    fn store(&mut self, ctx: &mut Ctx) {
        self.schedule();
        let left = self.left();
        ctx.wcfg().insert(
            "estado".into(),
            json!({"fase": self.phase.code(), "ciclo": self.cycle, "total": self.total, "restante": left, "fim": self.end}),
        );
        ctx.save();
    }

    pub fn toggle(&mut self, ctx: &mut Ctx) {
        if self.running() {
            self.remaining = self.left();
            self.end = None;
        } else {
            self.end = Some(win::now() + self.remaining);
        }
        self.store(ctx);
    }

    fn reset(&mut self, ctx: &mut Ctx) {
        let wcfg = ctx.cfg[ctx.key].clone();
        self.set_phase(self.phase, &wcfg);
        self.store(ctx);
    }

    /// Vai para a próxima fase sem contar a atual como concluída.
    fn skip(&mut self, ctx: &mut Ctx) {
        let wcfg = ctx.cfg[ctx.key].clone();
        if self.phase == Phase::Longa {
            self.cycle = 0;
        }
        let next = self.next_phase(&wcfg);
        self.set_phase(next, &wcfg);
        self.store(ctx);
    }

    fn reset_day(&mut self, ctx: &mut Ctx) {
        ctx.wcfg().remove("hoje");
        self.cycle = 0;
        self.store(ctx);
    }

    fn complete(&mut self, late: bool, ctx: &mut Ctx) {
        let finished = self.phase;
        if finished == Phase::Foco {
            let minutes = (self.total / 60.0).round() as i64;
            let stats = today(ctx.cfg, true);
            let w = ctx.wcfg();
            w.insert("hoje".into(), json!({"data": stats.0, "pomodoros": stats.1 + 1, "minutos": stats.2 + minutes}));
            self.cycle += 1;
        }
        let wcfg = ctx.cfg[ctx.key].clone();
        let next = self.next_phase(&wcfg);
        if finished == Phase::Longa {
            self.cycle = 0;
        }
        self.set_phase(next, &wcfg);
        let auto = wcfg.bool_of("auto").unwrap_or(false) && !late;
        if auto {
            self.end = Some(win::now() + self.total);
        }
        self.store(ctx);
        if late {
            return;
        }
        let (title, mut text) = if finished == Phase::Foco {
            let n = today(ctx.cfg, false).1;
            (
                "Pomodoro concluído 🍅".to_string(),
                format!("{n}º do dia. Hora de uma {} de {}.", next.label().to_lowercase(), fmt_minutes(self.total / 60.0)),
            )
        } else {
            ("Fim da pausa".to_string(), format!("Bora focar por {}.", fmt_minutes(self.total / 60.0)))
        };
        if !auto {
            text.push_str(" Clique para começar.");
        }
        let action = if auto { None } else { Some(NotifyAction::Widget("pomodoro", START_IF_IDLE)) };
        ctx.notify("pomodoro", title, text, action);
    }

    fn set_duration(&mut self, phase: Phase, minutes: i64, ctx: &mut Ctx) {
        ctx.wcfg().obj_mut_or("duracoes").insert(phase.code().into(), json!(minutes));
        if phase == self.phase && !self.started() {
            let wcfg = ctx.cfg[ctx.key].clone();
            self.set_phase(phase, &wcfg);
        }
        self.store(ctx);
    }

    fn set_every(&mut self, n: i64, ctx: &mut Ctx) {
        ctx.wcfg().insert("longa_a_cada".into(), json!(n));
        self.cycle = self.cycle.min(n);
        self.store(ctx);
    }

    // --- geometria ---

    fn ring(mode: Mode) -> (f32, f32, f32, f32) {
        match mode {
            Mode::P => C_RING,
            Mode::M => M_RING,
            Mode::G => G_RING,
        }
    }

    /// (controle, x, y, tamanho, em círculo) de cada botão.
    fn controls(&self, mode: Mode) -> Vec<(Control, f32, f32, f32, bool)> {
        let (cx, y, d, gap) = match mode {
            Mode::M => ((M_COL_X + W - 16.0) / 2.0, M_CTRL_Y, M_PLAY_D, 52.0),
            Mode::G => (W / 2.0, G_CTRL_Y, G_PLAY_D, 70.0),
            Mode::P => return Vec::new(),
        };
        vec![
            (Control::Restart, cx - gap, y, d * 0.5, false),
            (Control::Toggle(self.running()), cx, y, d, true),
            (Control::Skip, cx + gap, y, d * 0.42, false),
        ]
    }

    fn control_at(&self, x: f32, y: f32, mode: Mode) -> Option<(Control, f32, f32, f32, bool)> {
        self.controls(mode).into_iter().find(|&(_, cx, cy, size, circle)| {
            let reach = if circle { size / 2.0 + 2.0 } else { size.max(14.0) * 0.9 };
            ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() <= reach
        })
    }

    fn on_ring(x: f32, y: f32, mode: Mode) -> bool {
        let (cx, cy, d, _) = Self::ring(mode);
        ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() <= d / 2.0 + 2.0
    }

    fn clock_rect(mode: Mode) -> (f32, f32, f32, f32) {
        match mode {
            Mode::P => (6.0, 120.0, SMALL - 6.0, SMALL - 6.0),
            Mode::M => (M_COL_X - 6.0, 8.0, W - 8.0, 72.0),
            Mode::G => (PAD - 6.0, 30.0, 150.0, 86.0),
        }
    }

    fn on_clock(x: f32, y: f32, mode: Mode) -> bool {
        let (x0, y0, x1, y1) = Self::clock_rect(mode);
        (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
    }
}

fn duration(wcfg: &Value, phase: Phase) -> i64 {
    wcfg.get("duracoes").and_then(|d| d.i64_of(phase.code())).unwrap_or(phase.default_minutes())
}

/// (data de hoje, pomodoros, minutos) — zera sozinho quando muda o dia.
fn today(cfg: &Value, _for_write: bool) -> (String, i64, i64) {
    let date = win::local_time().iso_date();
    let stats = &cfg["pomodoro"]["hoje"];
    if stats.str_of("data") == Some(date.as_str()) {
        (date, stats.i64_of("pomodoros").unwrap_or(0), stats.i64_of("minutos").unwrap_or(0))
    } else {
        (date, 0, 0)
    }
}

trait ObjMutOr {
    fn obj_mut_or(&mut self, key: &str) -> &mut serde_json::Map<String, Value>;
}

impl ObjMutOr for serde_json::Map<String, Value> {
    fn obj_mut_or(&mut self, key: &str) -> &mut serde_json::Map<String, Value> {
        if !self.get(key).is_some_and(Value::is_object) {
            self.insert(key.to_string(), json!({}));
        }
        self.get_mut(key).unwrap().as_object_mut().unwrap()
    }
}

impl Kind for Pomodoro {
    fn key(&self) -> &'static str {
        "pomodoro"
    }
    fn title(&self) -> &'static str {
        "Pomodoro"
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, env: &Env) {
        let wcfg = env.wcfg();
        let color = self.phase.color();
        let (cx, cy, diameter, width) = Self::ring(mode);
        let frac = if self.total > 0.0 { (self.left() / self.total) as f32 } else { 0.0 };
        drawing::ring(c, cx, cy, diameter, width, frac, color);
        let t = win::local_time();
        let clock = format!("{:02}:{:02}", t.hour, t.minute);
        let timer = fmt_timer(self.left());
        let every = Self::every(wcfg);

        if mode == Mode::P {
            // quadrado: o anel com o tempo no meio; embaixo, o relógio e a fase
            let paused = self.started() && !self.running();
            if self.running() {
                drawing::pause(c, cx, cy - 22.0, 8.0, MUTED);
            } else {
                drawing::play(c, cx, cy - 22.0, 8.0, MUTED);
            }
            c.text(cx, cy + 4.0, &timer, Face::Light, 24.0, if paused { MUTED } else { FG }, Anchor::MM, None);
            c.text(14.0, 136.0, &clock, Face::Semibold, 12.5, FG, Anchor::LM, None);
            let mut status = self.phase.label().to_string();
            if self.phase == Phase::Foco {
                status += &format!(" · {}/{}", (self.cycle + 1).min(every), every);
            }
            if paused {
                status = "Pausado".into();
            }
            c.text(SMALL - 14.0, 136.0, &status, Face::Regular, 11.0, MUTED, Anchor::RM, Some(80.0));
            return;
        }

        // um ponto por pomodoro do ciclo até a pausa longa
        let (spacing, r) = (14.0, 3.5);
        let (x0, dy) = if mode == Mode::M { (M_COL_X + r, M_DOTS_Y) } else { (W / 2.0 - (every - 1) as f32 * spacing / 2.0, G_DOTS_Y) };
        let focus = Phase::Foco.color();
        for i in 0..every {
            let x = x0 + i as f32 * spacing;
            if i < self.cycle {
                c.fill_circle(x, dy, r, focus);
            } else if i == self.cycle && self.phase == Phase::Foco {
                c.stroke_circle(x, dy, r - 0.75, focus, 1.5);
            } else {
                c.fill_circle(x, dy, r, WHITE.alpha(0.22));
            }
        }
        for (control, bx, by, size, circle) in self.controls(mode) {
            let (glyph, glyph_color) = if circle {
                c.fill_circle(bx, by, size / 2.0, FG);
                (size * 0.38, DARK_GLYPH)
            } else {
                (size, FG)
            };
            match control {
                Control::Toggle(true) => drawing::pause(c, bx, by, glyph, glyph_color),
                Control::Toggle(false) => drawing::play(c, bx, by, glyph, glyph_color),
                Control::Restart => drawing::restart(c, bx, by, glyph, glyph_color),
                Control::Skip => drawing::skip(c, bx, by, glyph, glyph_color, true),
            }
        }

        let paused = self.started() && !self.running();
        let digits = if paused { MUTED } else { FG };
        let label = self.phase.label();
        let stats = today(env.cfg, false);
        if mode == Mode::M {
            if paused {
                c.text(cx, cy - 27.0, "PAUSADO", Face::Semibold, 8.5, MUTED, Anchor::MM, None);
            }
            let size = if timer.len() <= 5 { 25.0 } else { 19.0 };
            c.text(cx, cy - 4.0, &timer, Face::Light, size, digits, Anchor::MM, None);
            c.text(cx, cy + 20.0, label, Face::Semibold, 10.5, color, Anchor::MM, Some(diameter - 24.0));
            c.text(M_COL_X - 2.0, 10.0, &clock, Face::Light, 32.0, FG, Anchor::LA, None);
            c.text(M_COL_X, 52.0, &util::short_date(&t), Face::Regular, 11.0, MUTED, Anchor::LA, None);
            c.text(W - 16.0, M_DOTS_Y, &format!("hoje: {}", stats.1), Face::Regular, 10.5, MUTED, Anchor::RM, None);
            return;
        }

        c.text(PAD, 19.0, "POMODORO", Face::Semibold, 10.0, focus, Anchor::LA, None);
        c.text(W - PAD, 19.0, &util::long_date(&t), Face::Regular, 10.5, MUTED, Anchor::RA, Some(190.0));
        c.text(PAD - 2.0, 32.0, &clock, Face::Light, 40.0, FG, Anchor::LA, None);
        let n = stats.1;
        c.text(W - PAD, 46.0, &format!("{n} pomodoro{} hoje", if n != 1 { "s" } else { "" }), Face::Semibold, 13.0, FG, Anchor::RA, None);
        c.text(W - PAD, 66.0, &format!("{} de foco", fmt_minutes(stats.2 as f64)), Face::Regular, 11.0, MUTED, Anchor::RA, None);
        c.hline_px(PAD, W - PAD, G_HEAD_LINE, DIVIDER);
        if paused {
            c.text(cx, cy - 40.0, "PAUSADO", Face::Semibold, 9.5, MUTED, Anchor::MM, None);
        }
        let size = if timer.len() <= 5 { 42.0 } else { 32.0 };
        c.text(cx, cy - 6.0, &timer, Face::Light, size, digits, Anchor::MM, None);
        c.text(cx, cy + 30.0, label, Face::Semibold, 12.5, color, Anchor::MM, Some(diameter - 40.0));
        // no pé: o que vem depois desta fase
        let next = self.upcoming(wcfg);
        c.hline_px(PAD, W - PAD, G_FOOT_Y - 20.0, DIVIDER);
        let text = format!("A seguir: {} de {} min", next.label().to_lowercase(), duration(wcfg, next));
        let tw = c.text_width(&text, Face::Regular, 11.5);
        c.fill_circle(W / 2.0 - tw / 2.0 - 9.0, G_FOOT_Y, 3.5, next.color());
        c.text(W / 2.0 + 5.0, G_FOOT_Y, &text, Face::Regular, 11.5, MUTED, Anchor::MM, None);
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        if let Some((control, ..)) = self.control_at(x, y, mode) {
            return Some(match control {
                Control::Restart => RESET,
                Control::Toggle(_) => TOGGLE,
                Control::Skip => SKIP,
            });
        }
        if Self::on_ring(x, y, mode) {
            return Some(TOGGLE);
        }
        if Self::on_clock(x, y, mode) && env.shortcuts() {
            return Some(OPEN_CLOCK);
        }
        None
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            TOGGLE => self.toggle(ctx),
            RESET => self.reset(ctx),
            SKIP => self.skip(ctx),
            OPEN_CLOCK => ctx.open("ms-clock:"),
            RESET_DAY => self.reset_day(ctx),
            START_IF_IDLE => {
                if !self.running() {
                    self.toggle(ctx);
                }
            }
            TOGGLE_AUTO => {
                let auto = ctx.cfg[ctx.key].bool_of("auto").unwrap_or(false);
                ctx.wcfg().insert("auto".into(), json!(!auto));
                ctx.save();
            }
            a if (EVERY + 2..=EVERY + 6).contains(&a) => self.set_every((a - EVERY) as i64, ctx),
            a if a >= DURATION => {
                let phase = PHASES[((a - DURATION) / 1000) as usize % 3];
                self.set_duration(phase, ((a - DURATION) % 1000) as i64, ctx);
            }
            _ => {}
        }
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> Option<Hot> {
        if let Some((_, cx, cy, size, circle)) = self.control_at(x, y, mode) {
            let r = if circle { size / 2.0 + 4.0 } else { size.max(14.0) * 0.9 };
            return Some((cx - r, cy - r, cx + r, cy + r, r));
        }
        if Self::on_ring(x, y, mode) {
            let (cx, cy, d, _) = Self::ring(mode);
            let r = d / 2.0 + 5.0;
            return Some((cx - r, cy - r, cx + r, cy + r, r));
        }
        if Self::on_clock(x, y, mode) {
            let (x0, y0, x1, y1) = Self::clock_rect(mode);
            return Some((x0, y0, x1, y1, 12.0));
        }
        None
    }

    fn hover_style(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> super::HoverStyle {
        if self.control_at(x, y, mode).is_none() { super::HoverStyle::Quiet } else { super::HoverStyle::Surface }
    }

    fn tooltip(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<String> {
        let wcfg = env.wcfg();
        if let Some((control, ..)) = self.control_at(x, y, mode) {
            return match control {
                Control::Skip => Some(format!("Pular para: {}", self.next_phase(wcfg).label().to_lowercase())),
                _ => None,
            };
        }
        if Self::on_ring(x, y, mode) {
            return self.end.map(|end| format!("Termina às {}", win::clock_at(end)));
        }
        None
    }

    /// Roda do mouse: um minuto a mais ou a menos (parado no início, muda a duração da fase).
    fn uses_wheel(&self, x: f32, y: f32, mode: Mode) -> bool {
        // numa pilha, o tempo pela roda só em cima do anel
        let (cx, cy, d, _) = Self::ring(mode);
        (x - cx).hypot(y - cy) <= d / 2.0 + 6.0
    }

    fn wheel(&mut self, _x: f32, _y: f32, steps: f32, _mode: Mode, ctx: &mut Ctx) {
        let n = steps.round();
        let delta = 60.0 * if n != 0.0 { n as f64 } else if steps > 0.0 { 1.0 } else { -1.0 };
        let rem = self.left();
        if !self.started() {
            self.total = (rem + delta).clamp(MIN_S, MAX_S);
            self.remaining = self.total;
            let minutes = (self.total / 60.0).round() as i64;
            ctx.wcfg().obj_mut_or("duracoes").insert(self.phase.code().into(), json!(minutes));
        } else {
            let new = (rem + delta).max(rem.min(MIN_S)).min(MAX_S);
            let change = new - rem;
            self.total = (self.total + change).max(new).max(MIN_S);
            if let Some(end) = self.end.as_mut() {
                *end += change;
            } else {
                self.remaining = new;
            }
        }
        self.store(ctx);
    }

    fn menu(&self, env: &Env) -> Vec<MenuItem> {
        let wcfg = env.wcfg();
        let mut durations: Vec<MenuItem> = PHASES
            .iter()
            .enumerate()
            .map(|(i, &phase)| {
                let current = duration(wcfg, phase);
                let mut options: Vec<i64> = phase.choices().to_vec();
                if !options.contains(&current) {
                    options.push(current);
                    options.sort();
                }
                let items = options
                    .into_iter()
                    .map(|m| MenuItem::Radio(fmt_minutes(m as f64), m == current, DURATION + i as u32 * 1000 + m as u32))
                    .collect();
                MenuItem::Sub(phase.label().into(), items)
            })
            .collect();
        let every = Self::every(wcfg);
        durations.push(MenuItem::Sub(
            "Pausa longa a cada".into(),
            (2..=6).map(|n| MenuItem::Radio(format!("{n} pomodoros"), n == every, EVERY + n as u32)).collect(),
        ));
        let next = self.next_phase(wcfg);
        let article = if next == Phase::Foco { "o" } else { "a" };
        vec![
            MenuItem::Cmd(
                (if self.running() { "Pausar" } else if self.started() { "Continuar" } else { "Começar" }).into(),
                TOGGLE,
            ),
            MenuItem::Cmd("Reiniciar esta fase".into(), RESET),
            MenuItem::Cmd(format!("Pular para {article} {}", next.label().to_lowercase()), SKIP),
            MenuItem::Sub("Durações".into(), durations),
            MenuItem::Check("Começar a próxima fase sozinho".into(), wcfg.bool_of("auto").unwrap_or(false), TOGGLE_AUTO),
            MenuItem::Cmd("Zerar o dia".into(), RESET_DAY),
        ]
    }

    fn next_tick(&self) -> Option<Instant> {
        Some(self.next)
    }

    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        if let Some(end) = self.end {
            let now = win::now();
            if now >= end {
                // muito atrasado: o PC dormiu ou o app estava fechado
                self.complete(now - end > 60.0, ctx);
            }
        }
        self.schedule();
        true
    }
}
