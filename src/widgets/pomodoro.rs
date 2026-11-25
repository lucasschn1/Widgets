//! Widget de relógio com timer Pomodoro.

use std::cell::Cell;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::odometer::Odometer;
use super::island_ui::{self as iu, SOFT};
use super::{Ctx, Env, Hot, IslandForm, IslandItem, Kind, MenuItem, Mode, NotifyAction, Rect4, LARGE_H, MEDIUM_H, SMALL, WIDE};
use crate::data::config::Get;
use crate::gfx::drawing;
use crate::gfx::{Anchor, Canvas, Face};
use crate::ui::theme::*;
use crate::data::util;
use crate::platform::win;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Foco,
    Pausa,
    Longa,
}

impl Phase {
    /// Como a fase fica guardada nas opções ("foco", "pausa", "longa").
    pub fn code(self) -> &'static str {
        match self {
            Phase::Foco => "foco",
            Phase::Pausa => "pausa",
            Phase::Longa => "longa",
        }
    }
    fn from_code(code: &str) -> Phase {
        match code {
            "pausa" => Phase::Pausa,
            "longa" => Phase::Longa,
            _ => Phase::Foco,
        }
    }
    /// O nome da fase na tela.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Foco => "Foco",
            Phase::Pausa => "Pausa",
            Phase::Longa => "Pausa longa",
        }
    }
    /// A cor do anel e dos pontos da fase.
    pub fn color(self) -> Rgba {
        match self {
            Phase::Foco => RED,
            Phase::Pausa => GREEN,
            Phase::Longa => BLUE,
        }
    }
    /// A duração de fábrica, em minutos.
    fn default_minutes(self) -> i64 {
        match self {
            Phase::Foco => 25,
            Phase::Pausa => 5,
            Phase::Longa => 15,
        }
    }
    /// As durações oferecidas no menu, em minutos.
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
pub const RESET_DAY: u32 = 5;
pub const START_IF_IDLE: u32 = 6;
const TOGGLE_AUTO: u32 = 7;
/// um minuto a mais (o "+1 min" da ilha)
pub const ADD_MINUTE: u32 = 8;
pub(crate) const DURATION: u32 = 1000; // + fase * 1000 + minutos
pub(crate) const EVERY: u32 = 900; // + quantos focos até a pausa longa

const W: f32 = WIDE;
const PAD: f32 = super::CARD_PAD;
/// Onde fica o anel do tempo em cada tamanho.
#[derive(Clone, Copy)]
struct RingGeom {
    cx: f32,
    cy: f32,
    diameter: f32,
    thickness: f32,
}

const C_RING: RingGeom = RingGeom { cx: SMALL / 2.0, cy: 62.0, diameter: 104.0, thickness: 9.0 };
const M_RING: RingGeom = RingGeom { cx: 77.0, cy: MEDIUM_H / 2.0, diameter: 110.0, thickness: 8.0 };
const M_COL_X: f32 = 150.0; // coluna da direita (o que está acontecendo e os botões)
const M_CTRL_Y: f32 = 120.0;
const M_DOTS_Y: f32 = 86.0;
/// médio: embaixo do rótulo "POMODORO", o que está acontecendo (`headline`) e o total do dia
const M_HEAD_Y: f32 = 30.0;
const M_SUB_Y: f32 = 54.0;
/// grande: o que está acontecendo, embaixo do rótulo
const G_HEAD_Y: f32 = 34.0;
const M_PLAY_D: f32 = 34.0;
// grande: cabeçalho, o anel grande no meio, os ciclos, os botões e, no pé, a próxima fase
const G_HEAD_LINE: f32 = 92.0;
const G_RING: RingGeom = RingGeom { cx: W / 2.0, cy: 206.0, diameter: 176.0, thickness: 12.0 };
const G_DOTS_Y: f32 = 320.0;
const G_CTRL_Y: f32 = 362.0;
const G_PLAY_D: f32 = 44.0;
const G_FOOT_Y: f32 = LARGE_H - 24.0;
/// a roda do mouse não deixa a fase com menos de 1 minuto nem mais de 3 horas
const MIN_S: f64 = 60.0;
const MAX_S: f64 = 180.0 * 60.0;
/// a fase acaba no último minuto: a ilha passa o Pomodoro na frente da música
const HURRY_S: f64 = 60.0;
/// a fase acabou há mais que isso (o PC dormiu ou o app estava fechado): termina sem avisar
const LATE_S: f64 = 60.0;
/// o `tick` vem um pouco depois da virada do segundo, para não cair antes dela
const AFTER_TURN_MS: u64 = 15;

/// "24:59" ou, com hora, "1:05:00" (o tempo que falta, arredondado para cima).
pub fn fmt_timer(seconds: f64) -> String {
    let secs = seconds.max(0.0).ceil() as i64;
    let (hours, minutes, rest) = (secs / 3600, secs / 60 % 60, secs % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{minutes:02}:{rest:02}")
    }
}

/// "25 min", "2h" ou "1h30".
pub fn fmt_minutes(minutes: f64) -> String {
    let total = minutes.round() as i64;
    match (total / 60, total % 60) {
        (0, mins) => format!("{mins} min"),
        (hours, 0) => format!("{hours}h"),
        (hours, mins) => format!("{hours}h{mins:02}"),
    }
}

/// Um botão do médio e do grande (reiniciar, começar/pausar, pular), pelo centro.
#[derive(Clone, Copy)]
struct Button {
    control: Control,
    cx: f32,
    cy: f32,
    size: f32,
    /// o botão principal, num círculo cheio
    circle: bool,
}

/// Os focos de hoje: a data, quantos pomodoros e quantos minutos.
struct DayStats {
    date: String,
    pomodoros: i64,
    minutes: i64,
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
    /// o tempo que falta, rolando como num hodômetro a cada segundo
    timer_roll: Odometer,
    /// próximo quadro enquanto os dígitos rolam (guardado, pelo mesmo motivo)
    frame_at: Cell<Instant>,
}

/// Quanto o tempo que falta leva para rolar (curto: ele muda a cada segundo).
const TIMER_ROLL_MS: f32 = 320.0;
/// Um quadro enquanto os dígitos rolam.
const FRAME: Duration = Duration::from_millis(16);

impl Pomodoro {
    pub fn new(cfg: &Value) -> Pomodoro {
        let wcfg = &cfg["pomodoro"];
        let state = &wcfg["estado"];
        let phase = Phase::from_code(state.str_of("fase").unwrap_or("foco"));
        let total = state.f64_of("total").filter(|total| *total > 0.0).unwrap_or(duration(wcfg, phase) as f64 * 60.0);
        let mut pomodoro = Pomodoro {
            phase,
            cycle: state.i64_of("ciclo").unwrap_or(0),
            total,
            remaining: state.f64_of("restante").unwrap_or(total),
            end: state.f64_of("fim"),
            next: Instant::now(),
            timer_roll: Odometer::new(TIMER_ROLL_MS),
            frame_at: Cell::new(Instant::now()),
        };
        pomodoro.schedule();
        pomodoro
    }

    /// Correndo: na próxima virada de segundo; parado: na próxima virada de minuto (o dia pode
    /// virar, e o total de hoje volta a zero).
    fn schedule(&mut self) {
        let now = win::now();
        let delay = match self.end {
            Some(end) => {
                let to_turn = (end - now).rem_euclid(1.0);
                if to_turn < 0.001 { 1.0 } else { to_turn }
            }
            None => 60.0 - now.rem_euclid(60.0),
        };
        self.next = Instant::now() + Duration::from_millis((delay * 1000.0) as u64 + AFTER_TURN_MS);
    }

    /// Os dígitos estão rolando (precisa de quadros)?
    fn rolling(&self) -> bool {
        self.timer_roll.animating()
    }

    /// O tempo que falta acabou de mudar: os dígitos parados a `at` (0 a 1) do caminho.
    #[cfg(test)]
    pub fn sample_rolling(cfg: &Value, at: f32) -> Pomodoro {
        let mut pomodoro = Pomodoro::new(cfg);
        let (timer_before, timer_now) = pomodoro.last_change();
        pomodoro.timer_roll = Odometer::frozen(&timer_before, &timer_now, at);
        pomodoro
    }

    /// O tempo que falta agora e um segundo antes (para os exemplos rolando).
    #[cfg(test)]
    fn last_change(&self) -> (String, String) {
        (fmt_timer(self.left() + 1.0), fmt_timer(self.left()))
    }

    /// O que está acontecendo, em poucas palavras (no médio e no grande, no lugar da hora, que o
    /// relógio do Windows já mostra): até quando vai a fase, se está pausado, ou o que vem.
    fn headline(&self) -> String {
        match self.end {
            Some(end) => format!("Até {}", win::clock_at(end)),
            None if self.started() => "Pausado".into(),
            None if self.phase == Phase::Foco => "Pronto para focar".into(),
            None => "Hora da pausa".into(),
        }
    }

    /// O tempo está correndo?
    pub fn running(&self) -> bool {
        self.end.is_some()
    }

    /// Quantos segundos faltam para a fase acabar.
    pub fn left(&self) -> f64 {
        match self.end {
            Some(end) => (end - win::now()).max(0.0),
            None => self.remaining,
        }
    }

    /// A fase já começou (correndo, ou pausada no meio)?
    fn started(&self) -> bool {
        self.running() || self.remaining < self.total - 0.5
    }

    /// Quantos focos até a pausa longa.
    fn every(wcfg: &Value) -> i64 {
        wcfg.i64_of("longa_a_cada").unwrap_or(4)
    }

    /// A fase seguinte ao concluir ou pular esta (o ciclo atual já contado).
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

    /// Começa `phase` do zero, parada, com a duração escolhida.
    fn set_phase(&mut self, phase: Phase, wcfg: &Value) {
        self.phase = phase;
        self.total = duration(wcfg, phase) as f64 * 60.0;
        self.remaining = self.total;
        self.end = None;
    }

    /// Remarca o próximo `tick` e guarda o estado nas opções (para continuar depois de fechar).
    fn store(&mut self, ctx: &mut Ctx) {
        self.schedule();
        let left = self.left();
        ctx.wcfg().insert(
            "estado".into(),
            json!({"fase": self.phase.code(), "ciclo": self.cycle, "total": self.total, "restante": left, "fim": self.end}),
        );
        ctx.save();
    }

    /// Começa ou pausa.
    pub fn toggle(&mut self, ctx: &mut Ctx) {
        if self.running() {
            self.remaining = self.left();
            self.end = None;
        } else {
            self.end = Some(win::now() + self.remaining);
        }
        self.store(ctx);
    }

    /// Volta esta fase ao começo, parada.
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

    /// Zera os pomodoros de hoje e o ciclo.
    fn reset_day(&mut self, ctx: &mut Ctx) {
        ctx.wcfg().remove("hoje");
        self.cycle = 0;
        self.store(ctx);
    }

    /// A fase acabou: conta o foco no dia, passa para a próxima (começando sozinha, se pedido)
    /// e avisa. Com `late` (acabou faz tempo), não começa nada nem avisa.
    fn complete(&mut self, late: bool, ctx: &mut Ctx) {
        let finished = self.phase;
        if finished == Phase::Foco {
            let minutes = (self.total / 60.0).round() as i64;
            let stats = today(ctx.cfg, true);
            ctx.wcfg().insert("hoje".into(), json!({"data": stats.date, "pomodoros": stats.pomodoros + 1, "minutos": stats.minutes + minutes}));
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
            let done = today(ctx.cfg, false).pomodoros;
            (
                "Pomodoro concluído 🍅".to_string(),
                format!("{done}º do dia. Hora de uma {} de {}.", next.label().to_lowercase(), fmt_minutes(self.total / 60.0)),
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

    /// Muda a duração de uma fase (e a de agora, se ela ainda não começou).
    fn set_duration(&mut self, phase: Phase, minutes: i64, ctx: &mut Ctx) {
        ctx.wcfg().obj_mut_or("duracoes").insert(phase.code().into(), json!(minutes));
        if phase == self.phase && !self.started() {
            let wcfg = ctx.cfg[ctx.key].clone();
            self.set_phase(phase, &wcfg);
        }
        self.store(ctx);
    }

    /// Muda quantos focos vêm antes da pausa longa.
    fn set_every(&mut self, count: i64, ctx: &mut Ctx) {
        ctx.wcfg().insert("longa_a_cada".into(), json!(count));
        self.cycle = self.cycle.min(count);
        self.store(ctx);
    }

    // --- geometria ---

    /// O anel do tempo neste tamanho.
    fn ring(mode: Mode) -> RingGeom {
        match mode {
            Mode::P => C_RING,
            Mode::M => M_RING,
            Mode::G => G_RING,
        }
    }

    /// Os três botões do médio e do grande (o pequeno não tem: o anel é o botão).
    fn controls(&self, mode: Mode) -> Vec<Button> {
        let (cx, cy, play, gap) = match mode {
            Mode::M => ((M_COL_X + W - 16.0) / 2.0, M_CTRL_Y, M_PLAY_D, 52.0),
            Mode::G => (W / 2.0, G_CTRL_Y, G_PLAY_D, 70.0),
            Mode::P => return Vec::new(),
        };
        vec![
            Button { control: Control::Restart, cx: cx - gap, cy, size: play * 0.5, circle: false },
            Button { control: Control::Toggle(self.running()), cx, cy, size: play, circle: true },
            Button { control: Control::Skip, cx: cx + gap, cy, size: play * 0.42, circle: false },
        ]
    }

    /// O botão embaixo do ponto (x, y).
    fn control_at(&self, x: f32, y: f32, mode: Mode) -> Option<Button> {
        self.controls(mode).into_iter().find(|button| {
            let reach = if button.circle { button.size / 2.0 + 2.0 } else { button.size.max(14.0) * 0.9 };
            ((x - button.cx).powi(2) + (y - button.cy).powi(2)).sqrt() <= reach
        })
    }

    /// O ponto está em cima do anel (o botão de começar e pausar)?
    fn on_ring(x: f32, y: f32, mode: Mode) -> bool {
        let ring = Self::ring(mode);
        ((x - ring.cx).powi(2) + (y - ring.cy).powi(2)).sqrt() <= ring.diameter / 2.0 + 2.0
    }

}

/// A duração escolhida para uma fase, em minutos.
fn duration(wcfg: &Value, phase: Phase) -> i64 {
    wcfg.get("duracoes").and_then(|durations| durations.i64_of(phase.code())).unwrap_or(phase.default_minutes())
}

/// Os focos de hoje: zera sozinho quando muda o dia.
fn today(cfg: &Value, _for_write: bool) -> DayStats {
    let date = win::local_time().iso_date();
    let stats = &cfg["pomodoro"]["hoje"];
    if stats.str_of("data") == Some(date.as_str()) {
        DayStats { date, pomodoros: stats.i64_of("pomodoros").unwrap_or(0), minutes: stats.i64_of("minutos").unwrap_or(0) }
    } else {
        DayStats { date, pomodoros: 0, minutes: 0 }
    }
}

/// O total do dia numa linha: "2 pomodoros · 50 min" (ou "Nenhum pomodoro hoje").
fn day_total(stats: &DayStats) -> String {
    match stats.pomodoros {
        0 => "Nenhum pomodoro hoje".into(),
        done => format!("{done} pomodoro{} · {}", if done != 1 { "s" } else { "" }, fmt_minutes(stats.minutes as f64)),
    }
}

/// O objeto `key` de dentro de um objeto JSON, criado vazio se ainda não existe.
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
    /// Na ilha desde que a fase começou (correndo ou pausada no meio); no último minuto passa
    /// na frente da música.
    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        if !self.started() {
            return None;
        }
        let mut item = IslandItem::live(self.phase.color());
        item.idle = !self.running();
        item.hurry = self.running() && self.left() <= HURRY_S;
        Some(item)
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, env: &Env) {
        self.draw_island(c, form, r, hot, env);
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        island_buttons(form, w, h).into_iter().find(|(_, rect)| iu::inside(*rect, x, y)).map(|(action, _)| action)
    }

    fn key(&self) -> &'static str {
        "pomodoro"
    }
    fn title(&self) -> &'static str {
        "Pomodoro"
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, env: &Env) {
        let wcfg = env.wcfg();
        let color = self.phase.color();
        let RingGeom { cx, cy, diameter, thickness } = Self::ring(mode);
        let frac = if self.total > 0.0 { (self.left() / self.total) as f32 } else { 0.0 };
        drawing::ring(c, cx, cy, diameter, thickness, frac, color);
        let local = win::local_time();
        let timer = fmt_timer(self.left());
        let every = Self::every(wcfg);

        if mode == Mode::P {
            // quadrado: o anel (o botão de começar e pausar, com o símbolo do que o clique faz)
            // com o tempo no meio; embaixo, até quando vai a fase (rodando) ou quantos pomodoros
            // hoje (a hora o relógio do Windows já mostra) e a fase
            let paused = self.started() && !self.running();
            if self.running() {
                drawing::pause(c, cx, cy - 22.0, 8.0, MUTED);
            } else {
                drawing::play(c, cx, cy - 22.0, 8.0, MUTED);
            }
            self.timer_roll.draw(c, cx, cy + 4.0, &timer, super::HERO, 24.0, if paused { MUTED } else { FG }, Anchor::MM, None);
            let foot = match self.end.filter(|_| self.running()) {
                Some(end) => format!("Até {}", win::clock_at(end)),
                None => format!("hoje: {}", today(env.cfg, false).pomodoros),
            };
            c.text(14.0, 136.0, &foot, Face::Semibold, 12.0, FG, Anchor::LM, Some(62.0));
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
        let (x0, dots_y) = if mode == Mode::M { (M_COL_X + r, M_DOTS_Y) } else { (W / 2.0 - (every - 1) as f32 * spacing / 2.0, G_DOTS_Y) };
        let focus = Phase::Foco.color();
        for i in 0..every {
            let x = x0 + i as f32 * spacing;
            if i < self.cycle {
                c.fill_circle(x, dots_y, r, focus);
            } else if i == self.cycle && self.phase == Phase::Foco {
                c.stroke_circle(x, dots_y, r - 0.75, focus, 1.5);
            } else {
                c.fill_circle(x, dots_y, r, WHITE.alpha(0.22));
            }
        }
        for Button { control, cx: bx, cy: by, size, circle } in self.controls(mode) {
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
            self.timer_roll.draw(c, cx, cy - 4.0, &timer, super::HERO, size, digits, Anchor::MM, None);
            c.text(cx, cy + 20.0, label, Face::Semibold, 10.5, color, Anchor::MM, Some(diameter - 24.0));
            // a coluna da direita: o rótulo, o que está acontecendo e o total do dia (o tempo no
            // anel é o único número grande)
            let col_w = W - PAD - M_COL_X;
            super::draw_label(c, M_COL_X, super::LABEL_Y, "Pomodoro");
            c.text(M_COL_X, M_HEAD_Y, &self.headline(), Face::Semibold, 15.0, FG, Anchor::LA, Some(col_w));
            c.text(M_COL_X, M_SUB_Y, &day_total(&stats), Face::Regular, 11.0, MUTED, Anchor::LA, Some(col_w));
            return;
        }

        super::draw_label(c, PAD, super::LABEL_Y, "Pomodoro");
        c.text(W - PAD, super::LABEL_Y, &util::long_date(&local), Face::Regular, 10.5, MUTED, Anchor::RA, Some(190.0));
        c.text(PAD, G_HEAD_Y, &self.headline(), Face::Semibold, 20.0, FG, Anchor::LA, Some(W / 2.0 - PAD));
        let done = stats.pomodoros;
        c.text(W - PAD, 46.0, &format!("{done} pomodoro{} hoje", if done != 1 { "s" } else { "" }), Face::Semibold, 13.0, FG, Anchor::RA, None);
        c.text(W - PAD, 66.0, &format!("{} de foco", fmt_minutes(stats.minutes as f64)), Face::Regular, 11.0, MUTED, Anchor::RA, None);
        c.hline_px(PAD, W - PAD, G_HEAD_LINE, DIVIDER);
        if paused {
            c.text(cx, cy - 40.0, "PAUSADO", Face::Semibold, 9.5, MUTED, Anchor::MM, None);
        }
        let size = if timer.len() <= 5 { 42.0 } else { 32.0 };
        self.timer_roll.draw(c, cx, cy - 6.0, &timer, super::HERO, size, digits, Anchor::MM, None);
        c.text(cx, cy + 30.0, label, Face::Semibold, 12.5, color, Anchor::MM, Some(diameter - 40.0));
        // no pé: o que vem depois desta fase
        let next = self.upcoming(wcfg);
        c.hline_px(PAD, W - PAD, G_FOOT_Y - 20.0, DIVIDER);
        let text = format!("A seguir: {} de {} min", next.label().to_lowercase(), duration(wcfg, next));
        let tw = c.text_width(&text, Face::Regular, 11.5);
        c.fill_circle(W / 2.0 - tw / 2.0 - 9.0, G_FOOT_Y, 3.5, next.color());
        c.text(W / 2.0 + 5.0, G_FOOT_Y, &text, Face::Regular, 11.5, MUTED, Anchor::MM, None);
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> Option<u32> {
        if let Some(Button { control, .. }) = self.control_at(x, y, mode) {
            return Some(match control {
                Control::Restart => RESET,
                Control::Toggle(_) => TOGGLE,
                Control::Skip => SKIP,
            });
        }
        if Self::on_ring(x, y, mode) {
            return Some(TOGGLE);
        }

        None
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            TOGGLE => self.toggle(ctx),
            RESET => self.reset(ctx),
            SKIP => self.skip(ctx),
            RESET_DAY => self.reset_day(ctx),
            START_IF_IDLE => {
                if !self.running() {
                    self.toggle(ctx);
                }
            }
            ADD_MINUTE => self.wheel(0.0, 0.0, 1.0, Mode::M, ctx),
            TOGGLE_AUTO => {
                let auto = ctx.cfg[ctx.key].bool_of("auto").unwrap_or(false);
                ctx.wcfg().insert("auto".into(), json!(!auto));
                ctx.save();
            }
            every if (EVERY + 2..=EVERY + 6).contains(&every) => self.set_every((every - EVERY) as i64, ctx),
            duration if duration >= DURATION => {
                let phase = PHASES[((duration - DURATION) / 1000) as usize % 3];
                self.set_duration(phase, ((duration - DURATION) % 1000) as i64, ctx);
            }
            _ => {}
        }
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> Option<Hot> {
        if let Some(Button { cx, cy, size, circle, .. }) = self.control_at(x, y, mode) {
            let r = if circle { size / 2.0 + 4.0 } else { size.max(14.0) * 0.9 };
            return Some((cx - r, cy - r, cx + r, cy + r, r));
        }
        if Self::on_ring(x, y, mode) {
            let RingGeom { cx, cy, diameter, .. } = Self::ring(mode);
            let r = diameter / 2.0 + 5.0;
            return Some((cx - r, cy - r, cx + r, cy + r, r));
        }

        None
    }

    fn hover_style(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> super::HoverStyle {
        if self.control_at(x, y, mode).is_none() { super::HoverStyle::Quiet } else { super::HoverStyle::Surface }
    }

    fn tooltip(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<String> {
        let wcfg = env.wcfg();
        if let Some(Button { control, .. }) = self.control_at(x, y, mode) {
            return match control {
                Control::Skip => Some(format!("Pular para: {}", self.next_phase(wcfg).label().to_lowercase())),
                _ => None,
            };
        }
        // o anel é o botão de começar e pausar: a dica diz o que o clique faz
        if Self::on_ring(x, y, mode) {
            return Some(match (self.running(), self.end) {
                (true, Some(end)) => format!("Pausar · termina às {}", win::clock_at(end)),
                (true, None) => "Pausar".into(),
                (false, _) if self.started() => "Continuar".into(),
                _ => "Começar".into(),
            });
        }
        None
    }

    /// Roda do mouse: um minuto a mais ou a menos (parado no início, muda a duração da fase).
    fn uses_wheel(&self, x: f32, y: f32, mode: Mode) -> bool {
        // numa pilha, o tempo pela roda só em cima do anel
        let ring = Self::ring(mode);
        (x - ring.cx).hypot(y - ring.cy) <= ring.diameter / 2.0 + 6.0
    }

    fn wheel(&mut self, _x: f32, _y: f32, steps: f32, _mode: Mode, ctx: &mut Ctx) {
        // um minuto por passo da roda (um toque de touchpad, menor que um passo, ainda conta um)
        let whole = steps.round();
        let delta = 60.0 * if whole != 0.0 { whole as f64 } else if steps > 0.0 { 1.0 } else { -1.0 };
        let left = self.left();
        if !self.started() {
            self.total = (left + delta).clamp(MIN_S, MAX_S);
            self.remaining = self.total;
            let minutes = (self.total / 60.0).round() as i64;
            ctx.wcfg().obj_mut_or("duracoes").insert(self.phase.code().into(), json!(minutes));
        } else {
            let new_left = (left + delta).max(left.min(MIN_S)).min(MAX_S);
            let change = new_left - left;
            self.total = (self.total + change).max(new_left).max(MIN_S);
            if let Some(end) = self.end.as_mut() {
                *end += change;
            } else {
                self.remaining = new_left;
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
                    .map(|minutes| MenuItem::Radio(fmt_minutes(minutes as f64), minutes == current, DURATION + i as u32 * 1000 + minutes as u32))
                    .collect();
                MenuItem::Sub(phase.label().into(), items)
            })
            .collect();
        let every = Self::every(wcfg);
        durations.push(MenuItem::Sub(
            "Pausa longa a cada".into(),
            (2..=6).map(|count| MenuItem::Radio(format!("{count} pomodoros"), count == every, EVERY + count as u32)).collect(),
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
        Some(if self.rolling() { self.next.min(self.frame_at.get()) } else { self.next })
    }

    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        if self.rolling() {
            self.frame_at.set(Instant::now() + FRAME);
        }
        if let Some(end) = self.end {
            let now = win::now();
            if now >= end {
                // muito atrasado: o PC dormiu ou o app estava fechado
                self.complete(now - end > LATE_S, ctx);
            }
        }
        self.schedule();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O tempo que falta rola a cada segundo: o widget pede quadros enquanto os dígitos andam e
    /// volta ao ritmo de um segundo quando eles param.
    #[test]
    fn timer_digits_roll_every_second() {
        let cfg = json!({"pomodoro": {"estado": {"fase": "foco", "fim": win::now() + 600.0, "total": 1500.0}}, "app": {}});
        let rolled = |ms: f32| {
            let mut p = Pomodoro::new(&cfg);
            let (t0, t1) = p.last_change();
            p.timer_roll = Odometer::rolled(&t0, &t1, ms);
            p
        };
        let p = rolled(100.0);
        assert!(p.rolling());
        assert!(p.next_tick().unwrap() <= Instant::now() + FRAME);
        let p = rolled(2000.0);
        assert!(!p.rolling());
        assert_eq!(p.next_tick(), Some(p.next));
    }

    /// O pequeno não repete a hora do Windows: rodando, mostra até quando vai a fase; parado,
    /// quantos pomodoros hoje. E a dica do anel diz o que o clique faz.
    #[test]
    fn small_shows_the_end_instead_of_the_clock() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let texts = |p: &Pomodoro, cfg: &Value| -> Vec<String> {
            let env = Env { cfg, key: "pomodoro" };
            crate::card::morph::record(p, Mode::P, 1.0, 1.0, &env)
                .ops
                .into_iter()
                .filter_map(|r| match r.op {
                    crate::gfx::Op::Text { text, .. } => Some(text),
                    _ => None,
                })
                .collect()
        };
        let RingGeom { cx, cy, .. } = Pomodoro::ring(Mode::P);
        let running = serde_json::json!({"pomodoro": {"estado": {"fase": "foco", "ciclo": 0, "total": 1500.0, "fim": win::now() + 600.0}}});
        let p = Pomodoro::new(&running);
        let end = format!("Até {}", win::clock_at(p.end.unwrap()));
        assert!(texts(&p, &running).contains(&end), "{:?}", texts(&p, &running));
        let env = Env { cfg: &running, key: "pomodoro" };
        assert!(p.tooltip(cx, cy, Mode::P, &env).unwrap().starts_with("Pausar · termina às"));
        let stopped = serde_json::json!({"pomodoro": {}});
        let p = Pomodoro::new(&stopped);
        assert!(texts(&p, &stopped).iter().any(|t| t.starts_with("hoje: ")), "{:?}", texts(&p, &stopped));
        assert_eq!(p.tooltip(cx, cy, Mode::P, &Env { cfg: &stopped, key: "pomodoro" }).as_deref(), Some("Começar"));
    }

    /// O médio e o grande também não repetem a hora do Windows: o tempo no anel é o único número
    /// grande; ao lado, o que está acontecendo e o total do dia.
    #[test]
    fn medium_and_large_say_what_is_happening_instead_of_the_clock() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let texts = |p: &Pomodoro, cfg: &Value, mode: Mode| -> Vec<String> {
            let env = Env { cfg, key: "pomodoro" };
            crate::card::morph::record(p, mode, 1.0, 1.0, &env)
                .ops
                .into_iter()
                .filter_map(|r| match r.op {
                    crate::gfx::Op::Text { text, .. } => Some(text),
                    _ => None,
                })
                .collect()
        };
        let local = win::local_time();
        let clock = format!("{:02}:{:02}", local.hour, local.minute);
        let today = win::now();
        let date = win::local_time().iso_date();
        let running = json!({"pomodoro": {"estado": {"fase": "foco", "ciclo": 0, "total": 1500.0, "fim": today + 600.0},
            "hoje": {"data": date, "pomodoros": 2, "minutos": 50}}});
        let p = Pomodoro::new(&running);
        let end = format!("Até {}", win::clock_at(p.end.unwrap()));
        for mode in [Mode::M, Mode::G] {
            let shown = texts(&p, &running, mode);
            assert!(shown.contains(&end), "{mode:?}: {shown:?}");
            assert!(!shown.contains(&clock), "{mode:?}: a hora do Windows de novo ({shown:?})");
        }
        assert!(texts(&p, &running, Mode::M).iter().any(|t| t.starts_with("2 pomodoros")), "o total do dia no médio");
        let stopped = json!({"pomodoro": {}});
        assert_eq!(Pomodoro::new(&stopped).headline(), "Pronto para focar");
        let paused = json!({"pomodoro": {"estado": {"fase": "foco", "total": 1500.0, "restante": 900.0}}});
        assert_eq!(Pomodoro::new(&paused).headline(), "Pausado");
    }
}

// ---------- na ilha ----------

/// Os botões da ilha em cada forma: (ação, retângulo a partir do canto da pílula).
fn island_buttons(form: IslandForm, w: f32, h: f32) -> Vec<(u32, (f32, f32, f32, f32))> {
    match form {
        IslandForm::Medium => vec![(TOGGLE, iu::circle(w - 74.0, h / 2.0, 18.0)), (SKIP, iu::circle(w - 34.0, h / 2.0, 16.0))],
        IslandForm::Large => {
            let y = 140.0;
            vec![(TOGGLE, (22.0, y, 130.0, y + 34.0)), (SKIP, (138.0, y, 236.0, y + 34.0)), (ADD_MINUTE, (244.0, y, 316.0, y + 34.0))]
        }
        _ => Vec::new(),
    }
}

impl Pomodoro {
    /// Quanto da fase já passou (0 a 1): o anel.
    fn done_frac(&self) -> f32 {
        if self.total <= 0.0 { 0.0 } else { (1.0 - self.left() / self.total).clamp(0.0, 1.0) as f32 }
    }

    /// "2 de 4": o foco de agora no ciclo até a pausa longa.
    fn cycle_text(&self, wcfg: &Value) -> (i64, i64) {
        let every = Self::every(wcfg);
        let current = if self.phase == Phase::Foco { self.cycle + 1 } else { self.cycle.max(1) };
        (current.clamp(1, every), every)
    }

    /// O Pomodoro em cada forma da ilha: o anel do que já passou, o tempo, o ciclo e, aberta,
    /// os botões e os focos do ciclo.
    fn draw_island(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, env: &Env) {
        let (x, y, w, h) = r;
        let color = self.phase.color();
        let running = self.running();
        let time = fmt_timer(self.left());
        let (current, every) = self.cycle_text(env.wcfg());
        let is_hot = |action: u32| hot == Some(action);
        match form {
            IslandForm::Bubble => {
                drawing::ring(c, x + w / 2.0, y + h / 2.0, 30.0, 3.5, self.done_frac(), color);
                if !running {
                    drawing::pause(c, x + w / 2.0, y + h / 2.0, 9.0, FG);
                }
            }
            IslandForm::Compact | IslandForm::Banner => {
                drawing::ring(c, x + 21.0, y + h / 2.0, 24.0, 3.5, self.done_frac(), color);
                let label = if running { format!("{} · {current} de {every}", self.phase.label()) } else { "Pausado".to_string() };
                iu::compact_text(c, x + 42.0, r, &label, &time, if running { FG } else { SOFT });
            }
            IslandForm::Medium => {
                let (cx, cy) = (x + 39.0, y + h / 2.0);
                drawing::ring(c, cx, cy, 50.0, 4.5, self.done_frac(), color);
                c.text(cx, cy, &format!("{current}/{every}"), Face::Semibold, 11.0, SOFT, Anchor::MM, None);
                let label = if running { self.phase.label().to_string() } else { format!("{} · pausado", self.phase.label()) };
                c.text(x + 78.0, cy - 13.0, &label, Face::Semibold, 11.5, color, Anchor::LM, None);
                c.text(x + 78.0, cy + 9.0, &time, Face::Semibold, 24.0, FG, Anchor::LM, None);
                for (action, button) in island_buttons(form, w, h) {
                    let (bx, by, radius) = (x + (button.0 + button.2) / 2.0, y + (button.1 + button.3) / 2.0, (button.2 - button.0) / 2.0);
                    let glyph = if action == TOGGLE { if running { "pause" } else { "play" } } else { "next" };
                    iu::round_button(c, bx, by, radius, glyph, is_hot(action));
                }
            }
            IslandForm::Large => {
                let upcoming = self.upcoming(env.wcfg()).label().to_lowercase();
                iu::header(c, r, &self.phase.label().to_uppercase(), color, &format!("Ciclo {current} de {every}"), SOFT);
                c.text(x + 22.0, y + 72.0, &time, Face::Semibold, 42.0, FG, Anchor::LM, None);
                let today = today(env.cfg, false).pomodoros;
                let sub = match (running, self.end) {
                    (true, Some(end)) => format!("Termina às {} · depois, {upcoming}", win::clock_at(end)),
                    _ => format!("Pausado · {today} hoje"),
                };
                c.text(x + 22.0, y + 108.0, &sub, Face::Regular, 12.0, SOFT, Anchor::LM, Some(w - 120.0));
                drawing::ring(c, x + w - 54.0, y + 82.0, 60.0, 6.0, self.done_frac(), color);
                for (action, button) in island_buttons(form, w, h) {
                    let rect = (x + button.0, y + button.1, x + button.2, y + button.3);
                    let label = match action {
                        TOGGLE if running => "Pausar",
                        TOGGLE => "Continuar",
                        SKIP => "Pular fase",
                        _ => "+1 min",
                    };
                    iu::pill_button(c, rect, label, action == TOGGLE, is_hot(action));
                }
                // os focos do ciclo, em pontinhos
                for focus in 0..every {
                    let done = focus < current - i64::from(self.phase == Phase::Foco);
                    let now = focus == current - 1 && self.phase == Phase::Foco;
                    let col = if done || now { color } else { Rgba(255, 255, 255, 46) };
                    c.fill_circle(x + 26.0 + focus as f32 * 13.0, y + h - 24.0, if now { 4.0 } else { 3.2 }, col);
                }
                c.text(x + w - 22.0, y + h - 24.0, &format!("{today} pomodoros hoje"), Face::Regular, 11.5, SOFT, Anchor::RM, None);
            }
        }
    }
}
