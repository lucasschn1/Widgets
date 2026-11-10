//! Widget Bateria: a bateria do notebook (com a saúde, os ciclos e as últimas 24 horas) e a dos
//! dispositivos conectados (fones, mouses, teclados, controles). No pequeno, um círculo por
//! aparelho; no médio, o anel do notebook e a lista; no grande, tudo. Num PC sem bateria, só os
//! dispositivos.
//!
//! As animações tocam nos momentos que importam e param depois: ligar na tomada (uma onda sai do
//! anel e o raio entra com um "pop"; o arco fica onde está), a carga mudando (os números rolam
//! como num hodômetro), a carga completa (um reflexo atravessa o cartão e o raio vira um ✓), um
//! aparelho chegando ou saindo, a bateria baixa (a ponta do arco respira uma vez por minuto) e o
//! gráfico se desenhando ao abrir o grande. Carregando, um brilho percorre o arco a cada 3,5 s.
//! Parado, nada se mexe. (O reflexo do mouse na borda de vidro é do cartão, em todos os widgets.)

use std::cell::Cell;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{Ctx, Env, Hot, HoverStyle, Kind, Layout, MenuItem, Mode, Status, SMALL, WIDE};
use crate::battery::{self, DevKind, Device, Power};
use crate::gfx::{Anchor, Canvas, Face, Fill};
use crate::theme::{ease_in_out, Rgba, FG, MUTED, WHITE};
use crate::win;

// ---------- cores e fontes ----------
const OK: Rgba = crate::theme::GREEN;
const MID: Rgba = crate::theme::AMBER;
const LOW: Rgba = crate::theme::RED;
const TRACK: Rgba = Rgba(255, 255, 255, 30);
const LINE: Rgba = Rgba(255, 255, 255, 26);
const NUM: Face = Face::Custom("Outfit", 300);
const TEXT: Face = Face::Custom("Outfit", 400);
const BOLD: Face = Face::Custom("Outfit", 600);

/// A cor da carga: verde, âmbar abaixo de 30% e vermelho abaixo de 15%.
fn level_color(v: f32) -> Rgba {
    if v < 15.0 {
        LOW
    } else if v < 30.0 {
        MID
    } else {
        OK
    }
}

// ---------- os anéis (3/4 de volta, abertos embaixo, como nos outros widgets) ----------
const START: f32 = 135.0;
const SWEEP: f32 = 270.0;

/// Um anel: centro, raio e espessura.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ring {
    cx: f32,
    cy: f32,
    r: f32,
    w: f32,
}

impl Ring {
    /// O ponto do anel a `v` (0 a 100) do caminho.
    fn at(self, v: f32) -> (f32, f32) {
        let a = (START + SWEEP * v.clamp(0.0, 100.0) / 100.0).to_radians();
        (self.cx + self.r * a.cos(), self.cy + self.r * a.sin())
    }
}

const RING_P: Ring = Ring { cx: SMALL / 2.0, cy: 79.0, r: 55.0, w: 10.0 };
const RING_M: Ring = Ring { cx: 64.0, cy: 70.0, r: 44.0, w: 9.0 };
const RING_G: Ring = Ring { cx: 80.0, cy: 132.0, r: 54.0, w: 10.0 };
/// a margem de dentro do cartão (a mesma de todos os widgets)
const PAD: f32 = super::CARD_PAD;

// ---------- as animações ----------
const FRAME: Duration = Duration::from_millis(16);
/// o arco andando até a carga nova
const GROW_MS: f32 = 600.0;
/// os números rolando
const ROLL_MS: f32 = 650.0;
/// a onda e o "pop" do raio ao ligar na tomada
const RIPPLE_MS: f32 = 950.0;
const POP_MS: f32 = 560.0;
/// o ✓ no lugar do raio e o reflexo atravessando o cartão, na carga completa
const CHECK_MS: f32 = 3000.0;
const SHEEN_MS: f32 = 1100.0;
/// um aparelho chegando (desliza e a barra enche) ou saindo (esmaece e a barra esvazia)
const DEVICE_MS: f32 = 800.0;
/// o gráfico se desenhando
const REVEAL_MS: f32 = 1300.0;
/// o brilho correndo no arco, carregando: um a cada `SHIMMER_EVERY`, visível nesta parte dele
const SHIMMER_EVERY: f32 = 3500.0;
const SHIMMER_ON: (f32, f32) = (0.06, 0.44);
/// a ponta do arco respirando com a bateria baixa: um pulso deste tamanho a cada minuto
const LOW_EVERY: f32 = 60_000.0;
const LOW_MS: f32 = 2400.0;

fn ms_since(t: Instant) -> f32 {
    t.elapsed().as_secs_f32() * 1000.0
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// O "pop": cresce até 118% e volta a 100% (0 a 1 do tempo).
fn pop(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.6 { 0.3 + 0.88 * ease_out(t / 0.6) } else { 1.18 - 0.18 * ease_in_out((t - 0.6) / 0.4) }
}

// ---------- a leitura ----------
/// de quanto em quanto tempo olhar a bateria do notebook, a dos dispositivos e guardar no histórico
const POWER_EVERY: Duration = Duration::from_secs(5);
const DEVICES_EVERY: Duration = Duration::from_secs(30);
const SAMPLE_EVERY: f64 = 5.0 * 60.0;
const DAY: f64 = 24.0 * 3600.0;
/// avisa quando um aparelho conectado passa disto; avisa de novo só depois de carregar até `REARM`
const LOW_AT: u8 = 15;
const REARM: u8 = 20;

// ---------- ações ----------
const OPEN_POWER: u32 = 1;
const OPEN_BLUETOOTH: u32 = 2;

/// o consumo na bateria é a média dos últimos minutos (o do momento pula de 7 W para 10 W com
/// qualquer coisa que o PC faz, e o tempo restante pulava de 5 h 50 para 4 h 10)
const DRAIN_TAU: f32 = 180.0;

/// O tempo restante mostrado só muda quando a conta nova se afasta dele mais que isto (4%, no
/// mínimo 2 minutos); fora isso, ele desce 1 minuto por minuto, como um relógio. (Recalculado a
/// cada 5 s, ele ia e voltava um minuto toda hora, e os dígitos rolavam sem parar.)
fn keeps_left(shown: f64, estimate: f64) -> bool {
    (estimate - shown).abs() <= (estimate * 0.04).max(120.0)
}

/// Uma leitura que pula pelo menos isto da carga de antes (sem entrar nem sair da tomada) é
/// estranha, e espera até `DOUBT_READINGS` leituras para ser aceita.
const JUMP: f32 = 10.0;
const DOUBT_READINGS: u8 = 2;

/// O consumo médio novo, com a leitura `mw` feita `dt` segundos depois da anterior.
fn smooth_drain(avg: Option<f32>, mw: f32, dt: f32) -> f32 {
    match avg {
        Some(a) => a + (mw - a) * (1.0 - (-dt.max(0.0) / DRAIN_TAU).exp()),
        None => mw,
    }
}

/// Os textos de antes de uma mudança, de onde os dígitos rolam (como um hodômetro).
#[derive(Clone, Debug)]
struct Rolled {
    percent: String,
    status: String,
    header: String,
    at: Instant,
}

/// Um aparelho chegando (true) ou saindo (false) da lista conectada, e desde quando.
#[derive(Clone, Debug)]
struct Change {
    name: String,
    arriving: bool,
    since: Instant,
}

pub struct Battery {
    power: Option<Power>,
    devices: Vec<Device>,
    /// as últimas 24 horas: (segundos desde 1970, carga, na tomada)
    hist: Vec<(f64, f32, bool)>,
    last_sample: f64,
    /// a leitura dos dispositivos em andamento (num fio à parte)
    reading: Option<Receiver<Vec<Device>>>,
    next_power: Instant,
    next_devices: Instant,
    /// "agora" fixo nos exemplos (sem ler o sistema)
    fixed_now: Option<f64>,
    // animações
    grow: Option<(f32, f32, Instant, f32)>,
    roll: Option<Rolled>,
    /// o consumo médio na bateria (mW) e quando foi lido por último
    drain: Option<(f32, Instant)>,
    /// o tempo restante mostrado (segundos) e desde quando: dali ele desce como um relógio
    left: Option<(f64, Instant)>,
    /// os textos (porcentagem, linha de baixo, linha de cima) do último desenho pedido
    texts: Option<(String, String, String)>,
    /// alguma animação rodava no último tick (o quadro final precisa ser desenhado)
    was_animating: bool,
    plug_at: Option<Instant>,
    full_at: Option<Instant>,
    charge_since: Instant,
    changes: Vec<Change>,
    reveal: Cell<Option<Instant>>,
    mode_seen: Cell<Option<Mode>>,
    shimmering: bool,
    /// os aparelhos já avisados de pouca bateria (de novo só depois de carregar)
    warned: Vec<String>,
    /// leituras estranhas seguidas que ainda esperam confirmação (ver `steady`)
    doubt: u8,
}

impl Battery {
    pub fn new(cache: &Value) -> Battery {
        let mut b = Battery::empty();
        b.hist = cache["bateria"]["hist"]
            .as_array()
            .map(|a| a.iter().filter_map(|s| Some((s[0].as_f64()?, s[1].as_f64()? as f32, s[2].as_bool().unwrap_or(false)))).collect())
            .unwrap_or_default();
        b.last_sample = b.hist.last().map_or(0.0, |s| s.0);
        b.apply_power(battery::power());
        b.read_devices();
        b
    }

    fn empty() -> Battery {
        Battery {
            power: None,
            devices: Vec::new(),
            hist: Vec::new(),
            last_sample: 0.0,
            reading: None,
            next_power: Instant::now() + POWER_EVERY,
            next_devices: Instant::now() + DEVICES_EVERY,
            fixed_now: None,
            grow: None,
            roll: None,
            drain: None,
            left: None,
            texts: None,
            was_animating: false,
            plug_at: None,
            full_at: None,
            charge_since: Instant::now(),
            changes: Vec::new(),
            reveal: Cell::new(None),
            mode_seen: Cell::new(None),
            shimmering: false,
            warned: Vec::new(),
            doubt: 0,
        }
    }

    /// Exemplo da galeria e do README: notebook em 73% na bateria (3 h 20 pela frente) e quatro
    /// aparelhos, um quase sem bateria e um desconectado.
    pub fn sample() -> Battery {
        let mut b = Battery::empty();
        let now = win::now();
        b.fixed_now = Some(now);
        b.power = Some(Power { percent: 73.0, plugged: false, charging: false, secs_left: Some(3 * 3600 + 20 * 60), secs_to_full: None, health: Some(0.87), cycles: Some(312), ..Default::default() });
        let d = |name: &str, kind, level: u8, connected| Device { name: name.into(), kind, level: Some(level), connected };
        b.devices = vec![
            d("Teclado K380", DevKind::Keyboard, 12, true),
            d("Mouse MX Master 3S", DevKind::Mouse, 64, true),
            d("Fone WH-1000XM5", DevKind::Headphones, 82, true),
            d("Controle Xbox", DevKind::Gamepad, 47, false),
        ];
        battery::sort(&mut b.devices);
        b.hist = sample_history(now, 73.0);
        b.reveal.set(Some(Instant::now() - Duration::from_secs(10)));
        b
    }

    /// Carregando (41%, cheia daqui a 1 h).
    #[cfg(test)]
    pub fn sample_charging() -> Battery {
        let mut b = Battery::sample();
        b.power = Some(Power { percent: 41.0, plugged: true, charging: true, secs_left: None, secs_to_full: Some(3600), health: Some(0.87), cycles: Some(312), ..Default::default() });
        b
    }

    /// Quase acabando (11%, 25 minutos).
    #[cfg(test)]
    pub fn sample_low() -> Battery {
        let mut b = Battery::sample();
        b.power = Some(Power { percent: 11.0, plugged: false, charging: false, secs_left: Some(25 * 60), secs_to_full: None, health: Some(0.87), cycles: Some(312), ..Default::default() });
        b
    }

    /// Um PC de mesa: sem bateria, só os dispositivos.
    #[cfg(test)]
    pub fn sample_desktop() -> Battery {
        let mut b = Battery::sample();
        b.power = None;
        b.hist.clear();
        b
    }

    /// Só o notebook, cheio na tomada, sem nenhum aparelho.
    #[cfg(test)]
    pub fn sample_alone() -> Battery {
        let mut b = Battery::sample();
        b.devices.clear();
        b.power = Some(Power { percent: 100.0, plugged: true, charging: false, secs_left: None, secs_to_full: None, health: None, cycles: None, ..Default::default() });
        b
    }

    /// Nem bateria nem aparelhos.
    #[cfg(test)]
    pub fn sample_nothing() -> Battery {
        let mut b = Battery::sample_desktop();
        b.devices.clear();
        b
    }

    /// Ligado na tomada há `ms` milissegundos (a onda, o "pop" e o arco acendendo).
    #[cfg(test)]
    pub fn sample_plugging(ms: u64) -> Battery {
        let mut b = Battery::sample_charging();
        let at = Instant::now() - Duration::from_millis(ms);
        b.plug_at = Some(at);
        b.charge_since = at;
        b
    }

    /// A carga chegou a 100% há `ms` milissegundos (o reflexo e o ✓).
    #[cfg(test)]
    pub fn sample_full(ms: u64) -> Battery {
        let mut b = Battery::sample_charging();
        b.power = Some(Power { percent: 100.0, plugged: true, charging: false, secs_left: None, secs_to_full: None, health: Some(0.87), cycles: Some(312), ..Default::default() });
        b.full_at = Some(Instant::now() - Duration::from_millis(ms));
        b.roll = Some(Rolled { percent: "99".into(), status: "cheia às 15:03".into(), header: "Carregando · cheia às 15:03".into(), at: Instant::now() - Duration::from_millis(ms) });
        b
    }

    /// A carga caindo de 73% para 68% (e o tempo de 3 h 20 para 3 h 05) há `ms` milissegundos:
    /// os dígitos no meio do caminho.
    #[cfg(test)]
    pub fn sample_rolling(ms: u64) -> Battery {
        let mut b = Battery::sample();
        let before = (b.percent_text(), b.status_line(), b.header_line());
        let mut p = b.power.clone().unwrap();
        p.percent = 68.0;
        p.secs_left = Some(3 * 3600 + 5 * 60);
        b.apply_power(Some(p));
        let r = b.roll.as_mut().expect("rola");
        (r.percent, r.status, r.header) = before;
        r.at = Instant::now() - Duration::from_millis(ms);
        if let Some(g) = b.grow.as_mut() {
            g.2 = r.at;
        }
        b
    }

    /// Um aparelho chegando e outro saindo, há `ms` milissegundos.
    #[cfg(test)]
    pub fn sample_changing(ms: u64) -> Battery {
        let mut b = Battery::sample();
        let since = Instant::now() - Duration::from_millis(ms);
        b.devices[3].connected = true;
        b.devices[0].connected = false;
        b.changes = vec![Change { name: b.devices[3].name.clone(), arriving: true, since }, Change { name: b.devices[0].name.clone(), arriving: false, since }];
        b
    }

    fn now(&self) -> f64 {
        self.fixed_now.unwrap_or_else(win::now)
    }

    /// Começa a ler os dispositivos num fio à parte; o resultado chega em `tick`.
    fn read_devices(&mut self) {
        let (tx, rx) = mpsc::channel();
        let started = std::thread::Builder::new().name("bateria-dispositivos".into()).spawn(move || {
            unsafe {
                let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
            }
            let _ = tx.send(battery::devices());
        });
        if started.is_ok() {
            self.reading = Some(rx);
        }
    }

    // ---------- o que mostrar ----------

    /// A carga desenhada no arco agora (no caminho até a nova, se está animando).
    fn shown(&self) -> f32 {
        let target = self.power.as_ref().map_or(0.0, |p| p.percent);
        match self.grow {
            Some((from, to, at, dur)) if ms_since(at) < dur => from + (to - from) * ease_out(ms_since(at) / dur),
            _ => target,
        }
    }

    fn charging(&self) -> bool {
        self.power.as_ref().is_some_and(|p| p.charging)
    }

    fn plugged(&self) -> bool {
        self.power.as_ref().is_some_and(|p| p.plugged)
    }

    /// A carga completa, há menos de 3 s: o ✓ no lugar do raio.
    fn showing_check(&self) -> bool {
        self.full_at.is_some_and(|t| ms_since(t) < CHECK_MS)
    }

    /// A porcentagem, sem o "%".
    fn percent_text(&self) -> String {
        self.power.as_ref().map_or(String::new(), |p| format!("{}", p.percent.round() as i32))
    }

    /// Quanto falta para acabar, como mostrado: a última conta guardada, descendo com o tempo.
    fn secs_left(&self) -> Option<f64> {
        self.power.as_ref().filter(|p| !p.plugged)?;
        match self.left {
            Some((s, at)) => Some((s - at.elapsed().as_secs_f64()).max(0.0)),
            None => self.estimate_left(),
        }
    }

    /// A conta de quanto falta, agora: a energia que resta sobre o consumo médio dos últimos
    /// minutos; sem os dados do driver, a do Windows (que segue o consumo do momento e pula
    /// muito).
    fn estimate_left(&self) -> Option<f64> {
        let p = self.power.as_ref().filter(|p| !p.plugged)?;
        match (p.left_mwh, self.drain) {
            (Some(left), Some((mw, _))) if mw >= 100.0 => Some(left as f64 / mw as f64 * 3600.0),
            _ => p.secs_left.map(|s| s as f64),
        }
    }

    /// O que dizer embaixo do anel: o tempo que falta, a hora em que enche ou o estado.
    fn status_line(&self) -> String {
        let Some(p) = &self.power else { return String::new() };
        if !p.plugged {
            return self.secs_left().map_or("na bateria".into(), |s| format!("{} restantes", duration(s)));
        }
        if p.charging {
            return p.secs_to_full.map_or("carregando".into(), |s| format!("cheia às {}", win::clock_at(self.now() + s as f64)));
        }
        if p.percent >= 99.0 { "carga completa".into() } else { "na tomada".into() }
    }

    /// A linha de cima do grande: "Na bateria · acaba às 17:40", "Carregando · cheia às 15:03".
    fn header_line(&self) -> String {
        let Some(p) = &self.power else {
            let n = self.devices.iter().filter(|d| d.connected).count();
            return match n {
                0 => "Nenhum aparelho conectado".into(),
                1 => "PC na tomada · 1 conectado".into(),
                n => format!("PC na tomada · {n} conectados"),
            };
        };
        if !p.plugged {
            return match self.secs_left() {
                Some(s) => format!("Na bateria · acaba às {}", win::clock_at(self.now() + s)),
                None => "Na bateria".into(),
            };
        }
        if p.charging {
            return match p.secs_to_full {
                Some(s) => format!("Carregando · cheia às {}", win::clock_at(self.now() + s as f64)),
                None => "Carregando".into(),
            };
        }
        if p.percent >= 99.0 { "Na tomada · carga completa".into() } else { "Na tomada · sem carregar".into() }
    }

    /// Quanto tempo esteve na tomada hoje e o maior tempo seguido fora dela nas últimas 24 h.
    fn day_stats(&self) -> (Option<f64>, Option<f64>) {
        if self.hist.len() < 2 {
            return (None, None);
        }
        let now = self.now();
        let t = win::local_time();
        let midnight = now - (t.hour as f64 * 3600.0 + t.minute as f64 * 60.0 + t.second as f64);
        let (mut plugged_today, mut longest, mut run) = (0.0, 0.0f64, 0.0);
        for w in self.hist.windows(2) {
            let (a, b) = (w[0], w[1]);
            let span = (b.0 - a.0).clamp(0.0, 2.0 * SAMPLE_EVERY);
            if a.2 && b.0 > midnight {
                plugged_today += (b.0 - a.0.max(midnight)).clamp(0.0, span);
            }
            if !a.2 && b.0 > now - DAY {
                run += span;
                longest = longest.max(run);
            } else {
                run = 0.0;
            }
        }
        (Some(plugged_today), (longest > 0.0).then_some(longest))
    }

    // ---------- desenho ----------

    /// O anel do notebook: o trilho, o arco até a carga, a bolinha da ponta, a onda de quando
    /// liga na tomada e o brilho vermelho que respira com a bateria baixa.
    fn draw_ring(&self, c: &mut Canvas, g: Ring) {
        let v = self.shown();
        c.arc(g.cx, g.cy, g.r, START, START + SWEEP, TRACK, g.w);
        if let Some(at) = self.plug_at.filter(|&t| ms_since(t) < RIPPLE_MS) {
            let t = ease_out(ms_since(at) / RIPPLE_MS);
            // (cresce até 6 pt da borda do cartão, no máximo: com o anel mais perto da borda, na
            // margem de 16, a onda de 50% a mais encostava nela)
            let room = (g.cx.min(g.cy) - 6.0 - g.r - g.w).clamp(4.0, 0.5 * g.r);
            c.stroke_circle(g.cx, g.cy, g.r + room * t, OK.alpha(0.7 * (1.0 - t)), g.w * 0.6 * (1.0 - t) + 1.0);
        }
        let color = level_color(self.power.as_ref().map_or(v, |p| p.percent));
        if v > 0.05 {
            c.arc(g.cx, g.cy, g.r, START, START + SWEEP * v / 100.0, color, g.w);
        }
        let (dx, dy) = g.at(v);
        if let Some(a) = self.low_pulse() {
            c.fill_circle(dx, dy, g.w * 1.3, LOW.alpha(0.55 * a));
        }
        c.fill_circle(dx, dy + 0.8, g.w * 0.36 * 1.17, Rgba(0, 0, 0, 70));
        c.fill_circle(dx, dy, g.w * 0.36, FG);
    }

    /// A ponta respirando com a bateria baixa: a força do brilho agora (None: fora do pulso).
    fn low_pulse(&self) -> Option<f32> {
        let p = self.power.as_ref()?;
        if p.plugged || p.percent >= 15.0 {
            return None;
        }
        let t = ms_since(self.charge_since) % LOW_EVERY;
        (t < LOW_MS).then(|| (t / LOW_MS * std::f32::consts::PI).sin())
    }

    /// O raio (ou o ✓ da carga completa) dentro do anel, com o "pop" de quando liga na tomada.
    fn draw_badge(&self, c: &mut Canvas, x: f32, y: f32, size: f32) {
        if !self.plugged() {
            return;
        }
        let check = self.showing_check();
        if !self.charging() && !check {
            return;
        }
        let since = if check { self.full_at } else { self.plug_at };
        let scale = since.map_or(1.0, |t| pop(ms_since(t) / POP_MS));
        let alpha = since.map_or(1.0, |t| (ms_since(t) / (POP_MS * 0.4)).min(1.0));
        let glyph = if check { "\u{e73e}" } else { "\u{e945}" };
        c.text(x, y, glyph, Face::Icons, size * scale, OK.alpha(alpha), Anchor::MM, None);
    }

    /// A porcentagem grande, com os dígitos que mudaram rolando (o de antes sobe e some, o novo
    /// vem de baixo), e o "%" menor depois dela; centrada em `x`, na linha de base `y`. O tamanho
    /// diminui até tudo caber em `max_w` (o "100%", de três dígitos, não encosta no anel).
    fn draw_percent(&self, c: &mut Canvas, x: f32, y: f32, size: f32, max_w: f32) {
        if self.power.is_none() {
            return;
        }
        let text = self.percent_text();
        let size = fit_size(c, &text, size, max_w);
        let nw = c.text_width(&text, NUM, size);
        // o número e o "%" juntos no centro (só o número no centro deixava tudo para a direita)
        let x0 = x - (nw + 1.0 + c.text_width("%", NUM, size * 0.42)) / 2.0;
        let (before, t) = self.rolled(|r| &r.percent);
        draw_rolling(c, x0, y, &text, before, t, NUM, size, FG, Anchor::LS, None);
        c.text(x0 + nw + 1.0, y, "%", NUM, size * 0.42, FG.alpha(0.7), Anchor::LS, None);
    }

    /// Um texto de antes da última mudança (`pick` escolhe qual) e quanto do rolar já foi
    /// (0 a 1); (None, 1) fora da animação.
    fn rolled(&self, pick: impl Fn(&Rolled) -> &String) -> (Option<&str>, f32) {
        match self.roll.as_ref().filter(|r| ms_since(r.at) < ROLL_MS) {
            Some(r) => (Some(pick(r).as_str()), ease_out(ms_since(r.at) / ROLL_MS)),
            None => (None, 1.0),
        }
    }

    /// A linha de baixo do anel (o tempo que falta, ou quando enche), rolando.
    fn draw_status(&self, c: &mut Canvas, x: f32, y: f32, max_w: f32) {
        let (before, t) = self.rolled(|r| &r.status);
        draw_rolling(c, x, y, &self.status_line(), before, t, TEXT, 10.5, if self.charging() { OK } else { MUTED }, Anchor::MA, Some(max_w));
    }

    /// Quanto da chegada (ou da saída) de um aparelho já foi: (chegando, 0 a 1).
    fn change_of(&self, name: &str) -> Option<(bool, f32)> {
        self.changes.iter().find(|c| c.name == name && ms_since(c.since) < DEVICE_MS).map(|c| (c.arriving, ease_out(ms_since(c.since) / DEVICE_MS)))
    }

    /// Uma linha da lista: o ícone, o nome, a barra de carga e a porcentagem, de `x` a `x + w`.
    fn draw_row(&self, c: &mut Canvas, d: &Device, x: f32, y: f32, w: f32) {
        let change = self.change_of(&d.name);
        let (mut alpha, mut fill, mut dx) = (if d.connected { 1.0 } else { 0.45 }, 1.0, 0.0);
        match change {
            Some((true, t)) => {
                alpha = t;
                fill = t;
                dx = 10.0 * (1.0 - t);
            }
            Some((false, t)) => {
                alpha = 1.0 - 0.55 * t;
                fill = 1.0 - t;
            }
            None if !d.connected => fill = 0.0,
            None => {}
        }
        let x = x + dx;
        let shows = d.connected || matches!(change, Some((false, _)));
        let level = d.level.filter(|_| shows);
        c.push_opacity(alpha);
        c.text(x + 8.0, y + 9.0, glyph(d.kind), Face::Icons, 14.0, FG, Anchor::MM, None);
        let (nx, vx) = (x + 26.0, x + w);
        c.text(nx, y, &d.name, TEXT, 11.5, FG, Anchor::LA, Some(vx - 42.0 - nx));
        let (bx0, bx1, by) = (nx, vx - 42.0, y + 16.0);
        c.fill_round_rect(bx0, by, bx1, by + 4.0, 2.0, TRACK);
        if let Some(l) = level {
            let frac = l as f32 / 100.0 * fill;
            if frac > 0.0 {
                c.fill_round_rect(bx0, by, bx0 + ((bx1 - bx0) * frac).max(4.0), by + 4.0, 2.0, level_color(l as f32));
            }
        }
        let value = match (d.connected, d.level) {
            (true, Some(l)) => format!("{l}%"),
            _ => "–".into(),
        };
        let vcol = if d.connected && d.level.is_some_and(|l| l < LOW_AT) { LOW } else { FG };
        c.text(vx, y + 1.0, &value, TEXT, 11.5, vcol, Anchor::RA, None);
        c.pop_clip();
    }

    /// A lista de aparelhos, uma linha a cada `gap`, até caber `max`.
    fn draw_rows(&self, c: &mut Canvas, x: f32, y: f32, w: f32, gap: f32, max: usize) {
        for (i, d) in self.devices.iter().take(max).enumerate() {
            self.draw_row(c, d, x, y + i as f32 * gap, w);
        }
    }

    /// Nenhum aparelho para mostrar: o ícone e duas linhas, centradas em `x`.
    fn draw_no_devices(&self, c: &mut Canvas, x: f32, y: f32, w: f32) {
        c.text(x, y, "\u{e702}", Face::Icons, 18.0, MUTED, Anchor::MM, None);
        c.text(x, y + 20.0, "Nenhum dispositivo", BOLD, 12.0, FG, Anchor::MA, Some(w));
        for (i, line) in c.wrap("Fones, mouses e teclados Bluetooth aparecem aqui", TEXT, 10.0, w, 2).iter().enumerate() {
            c.text(x, y + 37.0 + 13.0 * i as f32, line, TEXT, 10.0, MUTED, Anchor::MA, None);
        }
    }

    fn draw_small(&self, c: &mut Canvas) {
        let laptop = self.power.is_some();
        if laptop && self.devices.is_empty() {
            // só o notebook: o anel grande
            self.draw_ring(c, RING_P);
            self.draw_badge(c, RING_P.cx, 45.0, 12.0);
            self.draw_percent(c, RING_P.cx, 88.0, 38.0, inner_width(RING_P));
            c.text(RING_P.cx, 95.0, "Notebook", TEXT, 10.5, MUTED, Anchor::MA, None);
            self.draw_status(c, RING_P.cx, 131.0, SMALL - 28.0);
            return;
        }
        if !laptop && self.devices.is_empty() {
            self.draw_no_devices(c, SMALL / 2.0, 52.0, SMALL - 24.0);
            return;
        }
        // um círculo para o notebook e um para cada aparelho conectado (os quatro primeiros)
        // (o arco do notebook anda com a animação; a cor é a da carga de verdade)
        let mut items: Vec<(Option<&Device>, f32)> = Vec::new();
        if laptop {
            items.push((None, self.shown()));
        }
        let pc_level = self.power.as_ref().map_or(0.0, |p| p.percent);
        for d in self.devices.iter().filter(|d| d.connected || matches!(self.change_of(&d.name), Some((false, _)))) {
            items.push((Some(d), d.level.map_or(0.0, |l| l as f32)));
        }
        for (i, (dev, v)) in items.into_iter().take(4).enumerate() {
            let (x, y) = (14.0 + (i % 2) as f32 * 63.0, 14.0 + (i / 2) as f32 * 64.0);
            let (cx, cy) = (x + 31.5, y + 22.0);
            let (pop_scale, alpha) = match dev.and_then(|d| self.change_of(&d.name)) {
                Some((true, t)) => (pop(t), t),
                Some((false, t)) => (1.0, 1.0 - 0.55 * t),
                None => (1.0, 1.0),
            };
            let r = 18.5 * pop_scale;
            c.push_opacity(alpha);
            c.arc(cx, cy, r, -90.0, 270.0, TRACK, 4.5);
            let known = dev.map_or(true, |d| d.level.is_some());
            if known && v > 0.05 {
                let color = level_color(if dev.is_none() { pc_level } else { v });
                c.arc(cx, cy, r, -90.0, -90.0 + 360.0 * v / 100.0, color, 4.5);
            }
            let (icon, col) = match dev {
                Some(d) => (glyph(d.kind), FG),
                None if self.showing_check() => ("\u{e73e}", OK),
                None if self.charging() => ("\u{e945}", OK),
                None => ("\u{e7f8}", FG),
            };
            c.text(cx, cy, icon, Face::Icons, 14.0, col, Anchor::MM, None);
            let label = match dev {
                None => format!("{}%", self.power.as_ref().map_or(0, |p| p.percent.round() as i32)),
                Some(d) => d.level.map_or("–".into(), |l| format!("{l}%")),
            };
            let low = (if dev.is_none() { pc_level } else { v }) < LOW_AT as f32 && known;
            // a do notebook rola como a porcentagem grande
            let (before, t) = if dev.is_none() { self.rolled(|r| &r.percent) } else { (None, 1.0) };
            let before = before.map(|b| format!("{b}%"));
            draw_rolling(c, cx, y + 49.0, &label, before.as_deref(), t, TEXT, 11.0, if low { LOW } else { FG }, Anchor::MA, None);
            c.pop_clip();
        }
    }

    fn draw_medium(&self, c: &mut Canvas) {
        if self.power.is_none() {
            // um PC de mesa: só os aparelhos
            c.text(PAD, 16.0, "Dispositivos", BOLD, 12.5, FG, Anchor::LA, None);
            c.text(PAD, 33.0, &self.header_line(), TEXT, 10.5, MUTED, Anchor::LA, None);
            if self.devices.is_empty() {
                self.draw_no_devices(c, WIDE / 2.0, 82.0, WIDE - 2.0 * PAD);
            } else {
                self.draw_rows(c, PAD, 56.0, WIDE - 2.0 * PAD, 23.0, 4);
            }
            return;
        }
        self.draw_ring(c, RING_M);
        self.draw_badge(c, RING_M.cx, 41.0, 11.0);
        self.draw_percent(c, RING_M.cx, 72.0, 30.0, inner_width(RING_M));
        c.text(RING_M.cx, 78.0, "Notebook", TEXT, 10.5, MUTED, Anchor::MA, None);
        self.draw_status(c, RING_M.cx, 128.0, 130.0);
        if self.devices.is_empty() {
            self.draw_no_devices(c, 226.0, 56.0, 150.0);
        } else {
            self.draw_rows(c, 146.0, 22.0, WIDE - PAD - 146.0, 29.0, 4);
        }
    }

    fn draw_large(&self, c: &mut Canvas) {
        c.text(PAD, 20.0, if self.power.is_some() { "Bateria" } else { "Dispositivos" }, BOLD, 14.0, FG, Anchor::LA, None);
        let (before, t) = self.rolled(|r| &r.header);
        draw_rolling(c, PAD, 41.0, &self.header_line(), before, t, TEXT, 10.5, MUTED, Anchor::LA, Some(WIDE - 2.0 * PAD));
        let Some(p) = self.power.clone() else {
            if self.devices.is_empty() {
                self.draw_no_devices(c, WIDE / 2.0, 200.0, WIDE - 2.0 * PAD);
            } else {
                self.draw_rows(c, PAD, 70.0, WIDE - 2.0 * PAD, 26.0, 14);
            }
            return;
        };
        self.draw_ring(c, RING_G);
        self.draw_badge(c, RING_G.cx, 99.0, 14.0);
        self.draw_percent(c, RING_G.cx, 143.0, 40.0, inner_width(RING_G));
        let under = if p.plugged { "na tomada".to_string() } else { self.status_line() };
        let (before, t) = self.rolled(|r| &r.status);
        draw_rolling(c, RING_G.cx, 186.0, &under, before, t, TEXT, 10.5, MUTED, Anchor::MA, Some(160.0));
        // ao lado do anel: saúde, ciclos e o dia
        let (today, longest) = self.day_stats();
        let stats = [
            ("Saúde", p.health.map_or("—".into(), |h| format!("{}%", (h * 100.0).round()))),
            ("Ciclos", p.cycles.map_or("—".into(), |n| n.to_string())),
            ("Na tomada hoje", today.map_or("—".into(), duration)),
            ("Maior tempo fora", longest.map_or("—".into(), duration)),
        ];
        let (sx0, sx1) = (170.0, WIDE - PAD);
        for (i, (k, v)) in stats.iter().enumerate() {
            let y = 92.0 + i as f32 * 25.0;
            c.text(sx0, y, k, TEXT, 10.5, MUTED, Anchor::LA, None);
            c.text(sx1, y, v, TEXT, 11.0, FG, Anchor::RA, None);
            c.fill_round_rect(sx0, y + 18.0, sx1, y + 19.0, 0.0, LINE);
        }
        c.fill_round_rect(PAD, 212.0, WIDE - PAD, 213.0, 0.0, LINE);
        c.text(PAD, 224.0, "Últimas 24 horas", BOLD, 12.0, FG, Anchor::LA, None);
        c.text(WIDE - PAD, 226.0, "verde: na tomada", TEXT, 10.0, MUTED, Anchor::RA, None);
        self.draw_chart(c, PAD, 246.0, WIDE - PAD, 300.0);
        c.fill_round_rect(PAD, 328.0, WIDE - PAD, 329.0, 0.0, LINE);
        if self.devices.is_empty() {
            self.draw_no_devices(c, WIDE / 2.0, 360.0, WIDE - 2.0 * PAD);
        } else {
            self.draw_rows(c, PAD, 342.0, WIDE - 2.0 * PAD, 25.0, 4);
        }
    }

    /// O gráfico das últimas 24 horas: a carga em linha, com a área embaixo, as faixas em que
    /// esteve na tomada e o ponto de agora; ao abrir, a linha se desenha da esquerda para a
    /// direita.
    fn draw_chart(&self, c: &mut Canvas, x0: f32, y0: f32, x1: f32, y1: f32) {
        let now = self.now();
        let (w, h) = (x1 - x0, y1 - y0);
        for g in [0.0, 0.5, 1.0] {
            c.fill_round_rect(x0, y1 - h * g, x1, y1 - h * g + 1.0, 0.0, Rgba(255, 255, 255, 16));
        }
        let t = win::local_time();
        let hour_now = t.hour as f64 + t.minute as f64 / 60.0;
        for k in 0..5 {
            let label = if k == 4 { "agora".to_string() } else { format!("{}h", ((hour_now - 24.0 + 6.0 * k as f64).rem_euclid(24.0)).floor() as i32) };
            let anchor = match k {
                0 => Anchor::LA,
                4 => Anchor::RA,
                _ => Anchor::MA,
            };
            c.text(x0 + w * k as f32 / 4.0, y1 + 6.0, &label, TEXT, 9.0, FAINT_TEXT, anchor, None);
        }
        let pts: Vec<(f32, f32, bool)> = self
            .hist
            .iter()
            .filter(|s| s.0 >= now - DAY)
            .map(|s| (x0 + w * (1.0 - ((now - s.0) / DAY) as f32), y1 - h * s.1.clamp(0.0, 100.0) / 100.0, s.2))
            .collect();
        if pts.len() < 2 {
            c.text((x0 + x1) / 2.0, (y0 + y1) / 2.0, "O gráfico enche com o uso, de 5 em 5 minutos", TEXT, 10.0, MUTED, Anchor::MM, Some(w));
            return;
        }
        // as faixas na tomada
        let mut i = 0;
        while i < pts.len() {
            if pts[i].2 {
                let start = pts[i].0;
                while i < pts.len() && pts[i].2 {
                    i += 1;
                }
                let end = pts.get(i).map_or(pts[pts.len() - 1].0, |p| p.0);
                c.fill_round_rect(start, y0, end.max(start + 2.0), y1, 3.0, OK.alpha(0.10));
            }
            i += 1;
        }
        // a linha se desenhando (ao abrir o grande)
        let reveal = self.reveal.get().map_or(1.0, |t| ease_in_out((ms_since(t) / REVEAL_MS).min(1.0)));
        let cut = x0 + (pts[pts.len() - 1].0 - x0) * reveal;
        let mut line: Vec<(f32, f32)> = Vec::new();
        for win in pts.windows(2) {
            let (a, b) = (win[0], win[1]);
            if a.0 > cut {
                break;
            }
            line.push((a.0, a.1));
            if b.0 > cut {
                let k = (cut - a.0) / (b.0 - a.0).max(0.001);
                line.push((cut, a.1 + (b.1 - a.1) * k));
            }
        }
        if reveal >= 1.0 {
            let last = pts[pts.len() - 1];
            line.push((last.0, last.1));
        }
        if line.len() >= 2 {
            let area_alpha = ((reveal - 0.35) / 0.65).clamp(0.0, 1.0);
            let mut area = line.clone();
            area.push((line[line.len() - 1].0, y1));
            area.push((line[0].0, y1));
            c.polygon_with(&area, Fill::VGrad(y0, y1, OK.alpha(0.35 * area_alpha), OK.alpha(0.0)));
            c.line(&line, OK, 1.6);
        }
        if reveal >= 0.85 {
            let a = ((reveal - 0.85) / 0.15).min(1.0);
            let (nx, ny, _) = pts[pts.len() - 1];
            c.fill_circle(nx, ny, 4.5, OK.alpha(0.25 * a));
            c.fill_circle(nx, ny, 2.2, FG.alpha(a));
        }
    }

    /// O brilho correndo no arco enquanto carrega: onde está (0 a 1 do caminho até a carga) e
    /// a força dele; None: entre um e outro.
    fn shimmer(&self) -> Option<(f32, f32)> {
        if !self.charging() || self.plug_at.is_some_and(|t| ms_since(t) < RIPPLE_MS) {
            return None;
        }
        let t = ((ms_since(self.charge_since) - 700.0).rem_euclid(SHIMMER_EVERY)) / SHIMMER_EVERY;
        let (a, b) = SHIMMER_ON;
        if !(a..=b).contains(&t) {
            return None;
        }
        let u = (t - a) / (b - a);
        let fade = (u / 0.12).min((1.0 - u) / 0.15).clamp(0.0, 1.0);
        Some((ease_in_out(u), fade))
    }

    /// O anel deste tamanho (None: sem o anel do notebook).
    fn ring_of(&self, mode: Mode) -> Option<Ring> {
        self.power.as_ref()?;
        match mode {
            Mode::P if self.devices.is_empty() => Some(RING_P),
            Mode::P => None,
            Mode::M => Some(RING_M),
            Mode::G => Some(RING_G),
        }
    }

    /// Algo animando agora (precisa de quadros)?
    fn animating(&self) -> bool {
        let live = |t: Option<Instant>, ms: f32| t.is_some_and(|t| ms_since(t) < ms);
        self.grow.is_some_and(|(_, _, at, dur)| ms_since(at) < dur)
            || live(self.roll.as_ref().map(|r| r.at), ROLL_MS)
            || live(self.plug_at, RIPPLE_MS.max(POP_MS))
            || live(self.full_at, CHECK_MS.max(SHEEN_MS))
            || self.changes.iter().any(|c| ms_since(c.since) < DEVICE_MS)
            || live(self.reveal.get(), REVEAL_MS)
            || self.low_pulse().is_some()
    }

    // ---------- atualização ----------

    /// A leitura a usar: uma estranha de um instante fica esperando a próxima confirmar. O
    /// Windows às vezes devolve, por uma leitura, "sem bateria" ou uma carga muito diferente
    /// (voltando da suspensão, por exemplo); acreditando nela, o anel sumia (ou descia até perto
    /// de zero) e o tempo restante pulava, até a leitura seguinte, 5 s depois.
    fn steady(&mut self, new: Option<Power>) -> Option<Power> {
        let odd = match (&self.power, &new) {
            (Some(_), None) => true,
            (Some(o), Some(n)) => o.plugged == n.plugged && (o.percent - n.percent).abs() >= JUMP,
            _ => false,
        };
        if odd && self.doubt < DOUBT_READINGS {
            self.doubt += 1;
            return self.power.clone();
        }
        self.doubt = 0;
        new
    }

    /// Os textos de agora: a porcentagem, a linha de baixo do anel e a de cima.
    fn current_texts(&self) -> (String, String, String) {
        (self.percent_text(), self.status_line(), self.header_line())
    }

    /// Uma leitura nova da bateria do notebook: as animações de cada mudança. Devolve true se
    /// algo na tela mudou (o consumo muda a cada leitura, mas só ele não redesenha).
    fn apply_power(&mut self, new: Option<Power>) -> bool {
        // onde o arco está agora, antes de trocar a leitura (medido depois, com a carga já fora,
        // dava 0: o arco zerava e subia de novo a cada mudança, mesmo com 99%)
        let shown = self.shown();
        // o que está escrito na tela (o do último desenho; o tempo restante desce sozinho)
        let before = self.texts.take().unwrap_or_else(|| self.current_texts());
        let seen = |p: &Option<Power>| p.as_ref().map(|p| (p.percent, p.plugged, p.charging, p.health, p.cycles));
        let looks_same = seen(&new) == seen(&self.power);
        let old = self.power.take();
        // o consumo médio na bateria (some na tomada: começa de novo ao tirar)
        self.drain = match new.as_ref().filter(|n| !n.plugged).and_then(|n| n.drain_mw) {
            Some(mw) => {
                let now = Instant::now();
                let avg = smooth_drain(self.drain.map(|d| d.0), mw as f32, self.drain.map_or(0.0, |d| (now - d.1).as_secs_f32()));
                Some((avg, now))
            }
            None => self.drain.filter(|_| new.as_ref().is_some_and(|n| !n.plugged)),
        };
        if let (Some(o), Some(n)) = (&old, &new) {
            if !o.plugged && n.plugged {
                // ligou na tomada: a onda e o "pop" do raio (o arco fica onde está: acender do
                // zero dava uma volta inteira com a bateria quase cheia)
                self.plug_at = Some(Instant::now());
                self.charge_since = Instant::now();
            }
            if (o.percent - n.percent).abs() >= 0.5 {
                self.grow = Some((shown, n.percent, Instant::now(), GROW_MS));
            }
            if n.plugged && n.percent >= 99.5 && o.percent < 99.5 {
                self.full_at = Some(Instant::now());
            }
            if o.plugged && !n.plugged {
                self.charge_since = Instant::now();
            }
        }
        let had = old.is_some();
        self.power = new;
        // o tempo restante: fica o mostrado (descendo) enquanto a conta nova não se afasta dele
        self.left = match (self.estimate_left(), self.left) {
            (None, _) => None,
            (Some(e), Some((s, at))) if keeps_left(s - at.elapsed().as_secs_f64(), e) => Some((s, at)),
            (Some(e), _) => Some((e, Instant::now())),
        };
        // a porcentagem ou as linhas de tempo mudaram: os dígitos rolam desde o que estava escrito
        let now = self.current_texts();
        let changed = now != before;
        let (percent, status, header) = before;
        if had && changed {
            self.roll = Some(Rolled { percent, status, header, at: Instant::now() });
        }
        self.texts = Some(now);
        changed || !looks_same
    }

    /// A lista nova de aparelhos: quem chegou e quem saiu (as animações) e os avisos de pouca
    /// bateria.
    fn apply_devices(&mut self, list: Vec<Device>, ctx: &mut Ctx) -> bool {
        if list == self.devices {
            return false;
        }
        let was = |name: &str| self.devices.iter().find(|d| d.name == name).is_some_and(|d| d.connected);
        let now = Instant::now();
        let mut changes: Vec<Change> = self.changes.iter().filter(|c| ms_since(c.since) < DEVICE_MS).cloned().collect();
        for d in &list {
            if d.connected && !was(&d.name) {
                changes.push(Change { name: d.name.clone(), arriving: true, since: now });
            }
            if !d.connected && was(&d.name) {
                changes.push(Change { name: d.name.clone(), arriving: false, since: now });
            }
            // pouca bateria: avisa uma vez, e de novo só depois de carregar
            match d.level {
                Some(l) if d.connected && l < LOW_AT && !self.warned.contains(&d.name) => {
                    self.warned.push(d.name.clone());
                    ctx.notify("bateria", format!("{} com pouca bateria", d.name), format!("Está com {l}%. Carregue logo."), None);
                }
                Some(l) if l >= REARM => self.warned.retain(|n| n != &d.name),
                _ => {}
            }
        }
        self.changes = changes;
        self.devices = list;
        true
    }

    /// Guarda a carga no histórico (de 5 em 5 minutos, e quando entra ou sai da tomada).
    fn sample_now(&mut self, ctx: &mut Ctx) {
        let Some(p) = &self.power else { return };
        let now = self.now();
        let changed_plug = self.hist.last().is_some_and(|s| s.2 != p.plugged);
        if now - self.last_sample < SAMPLE_EVERY && !changed_plug {
            return;
        }
        self.last_sample = now;
        self.hist.push((now, p.percent, p.plugged));
        self.hist.retain(|s| s.0 >= now - DAY - SAMPLE_EVERY);
        let rows: Vec<Value> = self.hist.iter().map(|s| json!([s.0.round(), (s.1 * 10.0).round() / 10.0, s.2])).collect();
        ctx.cache["bateria"] = json!({ "hist": rows });
        ctx.out.push(super::Request::SaveCache);
    }
}

/// A largura que a porcentagem pode ocupar dentro de um anel: 68% do miolo (a borda de dentro do
/// traço), para sobrar um respiro dos dois lados.
fn inner_width(g: Ring) -> f32 {
    2.0 * (g.r - g.w / 2.0) * 0.68
}

/// Os dois textos têm o mesmo formato (os mesmos caracteres, com dígitos nos mesmos lugares)?
/// Só aí os dígitos rolam: "3 h 20 restantes" → "3 h 05 restantes" sim; "99" → "100" não.
fn same_shape(a: &str, b: &str) -> bool {
    let shape = |s: &str| s.chars().map(|c| if c.is_ascii_digit() { '0' } else { c }).collect::<String>();
    shape(a) == shape(b)
}

/// Escreve `text` como `Canvas::text`, mas com os dígitos que mudaram desde `before` rolando como
/// num hodômetro (`t`, de 0 a 1, do caminho): cada dígito é uma coluna de 0 a 9 que sobe quando
/// ele aumenta e desce quando diminui, passando pelos do meio, como no mockup. Sem `before`, ou
/// de outro formato, escreve parado. Devolve a largura escrita.
#[allow(clippy::too_many_arguments)]
fn draw_rolling(c: &mut Canvas, x: f32, y: f32, text: &str, before: Option<&str>, t: f32, face: Face, size: f32, color: Rgba, anchor: Anchor, max_w: Option<f32>) -> f32 {
    let total = c.text_width(text, face, size);
    let Some(before) = before.filter(|b| t < 1.0 && b != &text && same_shape(b, text) && max_w.is_none_or(|m| total <= m)) else {
        return c.text(x, y, text, face, size, color, anchor, max_w);
    };
    let (h, v) = anchor.parts();
    let x0 = match h {
        'm' => x - total / 2.0,
        'r' => x - total,
        _ => x,
    };
    // os dígitos que rolam são escritos pela linha de base; os parados, como o resto do texto
    let (base, left_anchor) = match v {
        'a' => (y + c.baseline(face, size), Anchor::LA),
        'm' => (y + size * 0.36, Anchor::LM),
        _ => (y, Anchor::LS),
    };
    // como no mockup: uma coluna com um dígito a cada 1 em, vista por uma janela de 1 em em volta
    // do dígito (o de antes e o novo aparecem só em parte, no meio do caminho)
    let step = size;
    // (dígitos não descem da linha de base: embaixo, a janela termina logo nela, senão o topo
    // do dígito seguinte aparece embaixo do número no fim do caminho)
    let (top, bottom) = (base - size * 0.9, base + size * 0.05);
    let digit_w = (0..10).map(|d| c.text_width(&d.to_string(), face, size)).fold(0.0, f32::max);
    let rising = digits_of(text) > digits_of(before);
    for ((i, ch), old) in text.char_indices().zip(before.chars()) {
        let left = x0 + c.text_width(&text[..i], face, size);
        let digits = if ch != old { ch.to_digit(10).zip(old.to_digit(10)) } else { None };
        let Some((to, from)) = digits else {
            c.text(left, y, ch.encode_utf8(&mut [0; 4]), face, size, color, left_anchor, None);
            continue;
        };
        let cx = left + c.text_width(ch.encode_utf8(&mut [0; 4]), face, size) / 2.0;
        let at = from as f32 + odometer_steps(from, to, rising) as f32 * t;
        c.push_round_clip(cx - digit_w / 2.0 - 1.0, top, cx + digit_w / 2.0 + 1.0, bottom, 0.0);
        let lo = at.floor() as i32;
        for d in [lo, lo + 1] {
            c.text(cx, base + (d as f32 - at) * step, &d.rem_euclid(10).to_string(), face, size, color, Anchor::MS, None);
        }
        c.pop_clip();
    }
    total
}

/// Só os dígitos de um texto, para saber se o número subiu ou desceu.
fn digits_of(s: &str) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}

/// Quantas casas um dígito anda de `from` até `to`, como num hodômetro: todos para o mesmo lado
/// (para cima quando o número sobe, para baixo quando desce), dando a volta no 9/0. (Antes, de
/// 2 h 59 para 3 h 00 o 9 voltava até o 0 passando por todos, em vez de andar uma casa.)
fn odometer_steps(from: u32, to: u32, rising: bool) -> i32 {
    let up = (to as i32 - from as i32).rem_euclid(10);
    if up == 0 {
        0
    } else if rising {
        up
    } else {
        up - 10
    }
}

/// O tamanho da porcentagem (`text` e o "%" menor) para caber em `max_w`, até `size`.
fn fit_size(c: &Canvas, text: &str, size: f32, max_w: f32) -> f32 {
    // (a largura do texto não diminui na mesma proporção da fonte: algumas rodadas até caber)
    let mut s = size;
    for _ in 0..6 {
        let w = c.text_width(text, NUM, s) + 1.0 + c.text_width("%", NUM, s * 0.42);
        if w <= max_w {
            break;
        }
        s *= max_w / w * 0.995;
    }
    s
}

/// O tom apagado dos rótulos do gráfico.
const FAINT_TEXT: Rgba = Rgba(255, 255, 255, 115);

/// O ícone de cada tipo de aparelho (Segoe Fluent Icons).
fn glyph(kind: DevKind) -> &'static str {
    match kind {
        DevKind::Headphones => "\u{e7f6}",
        DevKind::Speaker => "\u{e7f5}",
        DevKind::Mouse => "\u{e962}",
        DevKind::Keyboard => "\u{e765}",
        DevKind::Gamepad => "\u{e7fc}",
        DevKind::Phone => "\u{e8ea}",
        DevKind::Pen => "\u{edc6}",
        DevKind::Other => "\u{e702}",
    }
}

/// "3 h 20", "45 min".
fn duration(secs: f64) -> String {
    let m = (secs / 60.0).round() as i64;
    if m < 60 { format!("{m} min") } else { format!("{} h {:02}", m / 60, m % 60) }
}

/// Um dia de exemplo para o gráfico: descarregando, duas idas à tomada, terminando em `end`.
fn sample_history(now: f64, end: f32) -> Vec<(f64, f32, bool)> {
    let n = 288;
    let mut out = Vec::with_capacity(n + 1);
    let mut v: f32 = 96.0;
    let plugged = |t: f32| (0.12..0.24).contains(&t) || (0.58..0.70).contains(&t);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let on = plugged(t);
        v += if on { 1.15 } else { -(0.38 + 0.27 * (i as f32 * 0.09).sin().powi(2)) };
        v = v.clamp(8.0, 100.0);
        out.push((now - DAY + DAY * t as f64, v, on));
    }
    // termina na carga de agora, sem salto: a diferença entra aos poucos ao longo do dia
    let diff = end - out[n].1;
    for (i, s) in out.iter_mut().enumerate() {
        s.1 = (s.1 + diff * i as f32 / n as f32).clamp(5.0, 100.0);
    }
    out
}

impl Kind for Battery {
    fn key(&self) -> &'static str {
        "bateria"
    }

    fn title(&self) -> &'static str {
        "Bateria"
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, _env: &Env) {
        // ao abrir o grande (ou mudar para ele), o gráfico se desenha
        if mode == Mode::G && self.mode_seen.get() != Some(Mode::G) && self.fixed_now.is_none() {
            self.reveal.set(Some(Instant::now()));
        }
        self.mode_seen.set(Some(mode));
        match mode {
            Mode::P => self.draw_small(c),
            Mode::M => self.draw_medium(c),
            Mode::G => self.draw_large(c),
        }
        // a carga completa: um reflexo atravessa o cartão uma vez
        if let Some(t) = self.full_at.filter(|&t| ms_since(t) < SHEEN_MS) {
            let l = Layout::of(mode);
            let u = ease_in_out(ms_since(t) / SHEEN_MS);
            let bw = l.w * 0.45;
            let x = -bw + (l.w + bw) * u;
            c.push_round_clip(0.0, 0.0, l.w, l.h, l.radius);
            c.gradient_h(x, 0.0, x + bw, l.h, 0.0, &[(0.0, WHITE.alpha(0.0)), (0.5, WHITE.alpha(0.22)), (1.0, WHITE.alpha(0.0))], x, x + bw);
            c.pop_clip();
        }
    }

    /// O brilho correndo no arco, carregando.
    fn overlay(&self, c: &mut Canvas, mode: Mode, ox: f32, oy: f32) {
        if let (Some(g), Some((u, fade))) = (self.ring_of(mode), self.shimmer()) {
            let v = self.power.as_ref().map_or(0.0, |p| p.percent);
            let head = (v * u).max(2.0);
            let tail = (head - 6.0).max(0.0);
            c.arc(g.cx + ox, g.cy + oy, g.r, START + SWEEP * tail / 100.0, START + SWEEP * head / 100.0, WHITE.alpha(0.6 * fade), g.w * 0.55);
        }
    }

    fn overlay_fps(&self) -> u32 {
        if self.shimmer().is_some() { 30 } else { 0 }
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        if !env.shortcuts() {
            return None;
        }
        if let Some(g) = self.ring_of(mode) {
            if (x - g.cx).hypot(y - g.cy) <= g.r + g.w {
                return Some(OPEN_POWER);
            }
        }
        if self.devices.is_empty() {
            return None;
        }
        let in_list = match (mode, self.power.is_some()) {
            (Mode::P, _) => true,
            (Mode::M, true) => x >= 140.0,
            (Mode::M, false) => y >= 50.0,
            (Mode::G, true) => y >= 334.0,
            (Mode::G, false) => y >= 64.0,
        };
        in_list.then_some(OPEN_BLUETOOTH)
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<Hot> {
        match self.hit(x, y, mode, env)? {
            OPEN_POWER => {
                let g = self.ring_of(mode)?;
                let r = g.r + g.w;
                Some((g.cx - r, g.cy - r, g.cx + r, g.cy + r, r))
            }
            _ => None,
        }
    }

    fn hover_style(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> HoverStyle {
        HoverStyle::Quiet
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            OPEN_POWER => ctx.open("ms-settings:batterysaver"),
            OPEN_BLUETOOTH => ctx.open("ms-settings:bluetooth"),
            _ => {}
        }
    }

    fn menu(&self, _env: &Env) -> Vec<MenuItem> {
        let mut items = Vec::new();
        if self.power.is_some() {
            items.push(MenuItem::Cmd("Configurações de energia".into(), OPEN_POWER));
        }
        items.push(MenuItem::Cmd("Dispositivos Bluetooth".into(), OPEN_BLUETOOTH));
        items
    }

    fn next_tick(&self) -> Option<Instant> {
        if self.fixed_now.is_some() {
            return self.animating().then(|| Instant::now() + FRAME);
        }
        let mut next = self.next_power.min(self.next_devices);
        if self.animating() || self.was_animating {
            next = next.min(Instant::now() + FRAME);
        }
        if self.charging() {
            // o começo e o fim de cada brilho (para ligar e desligar os quadros da camada)
            next = next.min(Instant::now() + Duration::from_millis(100));
        }
        if self.reading.is_some() {
            next = next.min(Instant::now() + Duration::from_millis(250));
        }
        Some(next)
    }

    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        // durante uma animação, um quadro por tick; e mais um depois que ela acaba: o último
        // desenhado podia ser do meio do caminho (com o tick atrasado, comum na bateria) e ficar
        // na tela, com o arco ou os dígitos pela metade, até o mouse passar e redesenhar
        let animating = self.animating();
        let mut redraw = animating || self.was_animating;
        self.was_animating = animating;
        if self.fixed_now.is_none() {
            let now = Instant::now();
            if now >= self.next_power {
                self.next_power = now + POWER_EVERY;
                let reading = self.steady(battery::power());
                redraw |= self.apply_power(reading);
                self.sample_now(ctx);
            }
            if now >= self.next_devices && self.reading.is_none() {
                self.next_devices = now + DEVICES_EVERY;
                self.read_devices();
            }
            if let Some(rx) = &self.reading {
                match rx.try_recv() {
                    Ok(list) => {
                        self.reading = None;
                        redraw |= self.apply_devices(list, ctx);
                    }
                    Err(mpsc::TryRecvError::Disconnected) => self.reading = None,
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
        }
        // o brilho começou ou acabou: redesenha para a camada ganhar (ou perder) os quadros
        let shimmering = self.shimmer().is_some();
        if shimmering != self.shimmering {
            self.shimmering = shimmering;
            redraw = true;
        }
        redraw
    }

    fn status(&self) -> Status {
        Status::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn com() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
    }

    fn ctx_run<T>(f: impl FnOnce(&mut Ctx) -> T) -> (T, Vec<super::super::Request>) {
        let (mut cfg, mut cache) = (json!({"bateria": {}, "app": {}}), json!({}));
        let mut out = Vec::new();
        let r = f(&mut Ctx { cfg: &mut cfg, cache: &mut cache, key: "bateria", out: &mut out });
        (r, out)
    }

    #[test]
    fn says_how_long_is_left_or_when_it_is_full() {
        let b = Battery::sample();
        assert_eq!(b.status_line(), "3 h 20 restantes");
        assert!(b.header_line().starts_with("Na bateria · acaba às "));
        let c = Battery::sample_charging();
        assert!(c.status_line().starts_with("cheia às "), "{}", c.status_line());
        assert!(c.header_line().starts_with("Carregando · cheia às "));
        assert_eq!(Battery::sample_alone().status_line(), "carga completa");
        assert_eq!(Battery::sample_low().status_line(), "25 min restantes");
        assert_eq!(Battery::sample_desktop().header_line(), "PC na tomada · 3 conectados");
        assert_eq!(duration(3.0 * 3600.0 + 20.0 * 60.0), "3 h 20");
        assert_eq!(duration(45.0 * 60.0), "45 min");
    }

    #[test]
    fn plugging_in_ripples_and_pops_without_spinning_the_arc() {
        let mut b = Battery::sample();
        let mut p = b.power.clone().unwrap();
        p.plugged = true;
        p.charging = true;
        assert!(b.apply_power(Some(p.clone())));
        assert!(b.plug_at.is_some() && b.animating());
        // o arco fica onde estava, do começo ao fim da animação (nada de volta inteira)
        assert!(b.grow.is_none());
        for _ in 0..5 {
            assert_eq!(b.shown(), 73.0);
        }
        // o raio cresce além do tamanho e volta
        assert!(pop(0.0) < 0.4 && pop(0.6) > 1.15 && (pop(1.0) - 1.0).abs() < 1e-4);
        // a mesma leitura de novo não anima nada
        assert!(!b.apply_power(Some(p)));
    }

    #[test]
    fn numbers_roll_and_full_charge_shows_a_check() {
        let mut b = Battery::sample_charging();
        let mut p = b.power.clone().unwrap();
        p.percent = 42.0;
        b.apply_power(Some(p.clone()));
        assert_eq!(b.roll.as_ref().unwrap().percent, "41");
        assert!(b.full_at.is_none());
        p.percent = 100.0;
        p.charging = false;
        b.apply_power(Some(p));
        assert!(b.full_at.is_some() && b.showing_check());
        b.full_at = Some(Instant::now() - Duration::from_secs(4));
        assert!(!b.showing_check(), "o ✓ volta a ser raio depois de 3 s");
    }

    /// O arco anda da carga de antes até a nova; antes, com a leitura trocada cedo demais, ele
    /// voltava a 0 e subia de novo a cada mudança (mesmo com 99%).
    #[test]
    fn the_arc_moves_from_where_it_was_not_from_zero() {
        let mut b = Battery::sample();
        let mut p = b.power.clone().unwrap();
        p.percent = 99.0;
        b.power = Some(p.clone());
        p.percent = 98.0;
        assert!(b.apply_power(Some(p)));
        let (from, to, _, _) = b.grow.expect("anda");
        assert_eq!((from, to), (99.0, 98.0));
        assert!(b.shown() > 97.9, "o arco caiu para {}", b.shown());
    }

    /// Os dígitos rolam como um hodômetro: na porcentagem e nas linhas de tempo, só quando o
    /// formato é o mesmo, passando pelos dígitos do meio.
    #[test]
    fn digits_roll_like_an_odometer() {
        assert!(same_shape("73", "68") && same_shape("3 h 20 restantes", "3 h 05 restantes"));
        assert!(!same_shape("99", "100") && !same_shape("45 min restantes", "1 h 05 restantes"));
        let b = Battery::sample_rolling(200);
        let roll = b.roll.as_ref().unwrap();
        assert_eq!((roll.percent.as_str(), roll.status.as_str()), ("73", "3 h 20 restantes"));
        assert_eq!((b.percent_text().as_str(), b.status_line().as_str()), ("68", "3 h 05 restantes"));
        assert!(b.animating());
        // a mesma porcentagem, só o tempo mudando de minuto: rola também
        let mut b = Battery::sample();
        let mut p = b.power.clone().unwrap();
        p.secs_left = Some(3 * 3600 + 19 * 60);
        b.apply_power(Some(p.clone()));
        assert_eq!(b.roll.as_ref().map(|r| r.status.as_str()), Some("3 h 20 restantes"));
        // uma leitura nova com os mesmos textos (só a saúde mudou): nada rola
        b.roll = None;
        p.health = Some(0.86);
        assert!(b.apply_power(Some(p)));
        assert!(b.roll.is_none());
        // desenha no meio do caminho, em todos os tamanhos
        com();
        let env_cfg = json!({"bateria": {}, "app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        for ms in [0, 150, 320, 640] {
            let b = Battery::sample_rolling(ms);
            for mode in Mode::ALL {
                let l = Layout::of(mode);
                let mut c = Canvas::new(l.w as u32, l.h as u32, 1.0);
                b.draw(&mut c, mode, &env);
            }
        }
    }

    /// O tempo restante sai da energia que resta e do consumo médio dos últimos minutos, e não
    /// pula a cada leitura como o do Windows (que segue o consumo do momento).
    #[test]
    fn time_left_follows_the_average_drain() {
        let mut b = Battery::sample();
        let mut p = b.power.clone().unwrap();
        p.left_mwh = Some(39_938);
        p.drain_mw = Some(9_793);
        b.apply_power(Some(p.clone()));
        // a primeira leitura: 39,9 Wh a 9,8 W, umas 4 h 05
        assert_eq!(b.status_line(), "4 h 05 restantes");
        // o consumo pulando entre 7 W e 10 W a cada 5 s, por um minuto: o tempo quase não mexe
        let mut seen = Vec::new();
        for k in 0..12 {
            p.drain_mw = Some(if k % 2 == 0 { 6_800 } else { 9_800 });
            p.secs_left = Some(if k % 2 == 0 { 21_211 } else { 15_043 });
            b.drain = b.drain.map(|(mw, at)| (mw, at - Duration::from_secs(5)));
            b.apply_power(Some(p.clone()));
            seen.push(b.secs_left().unwrap());
        }
        let (lo, hi) = seen.iter().fold((f64::MAX, 0.0f64), |(lo, hi), &s| (lo.min(s), hi.max(s)));
        assert!(hi - lo < 15.0 * 60.0, "pulou de {lo} a {hi}");
        // e vai para a média com o tempo
        assert!((smooth_drain(Some(10_000.0), 7_000.0, 3_600.0) - 7_000.0).abs() < 1.0);
        assert_eq!(smooth_drain(None, 8_000.0, 5.0), 8_000.0);
        // na tomada, a média some (e começa de novo ao tirar)
        p.plugged = true;
        p.drain_mw = None;
        b.apply_power(Some(p));
        assert!(b.drain.is_none());
        // sem os dados do driver, a conta do Windows
        assert_eq!(Battery::sample().status_line(), "3 h 20 restantes");
    }

    /// O tempo restante desce como um relógio e só pula quando a conta nova se afasta de verdade;
    /// uma leitura que só muda o consumo não redesenha nada.
    #[test]
    fn time_left_counts_down_and_only_jumps_for_real_changes() {
        let mut b = Battery::sample();
        let mut p = b.power.clone().unwrap();
        p.secs_left = Some(4 * 3600);
        b.apply_power(Some(p.clone()));
        assert_eq!(b.status_line(), "4 h 00 restantes");
        // a conta ia e voltava um minuto: fica o mostrado, e nada é redesenhado
        b.roll = None;
        for s in [4 * 3600 - 60, 4 * 3600 + 60, 4 * 3600 - 90] {
            p.secs_left = Some(s);
            assert!(!b.apply_power(Some(p.clone())), "{s}");
        }
        assert_eq!(b.status_line(), "4 h 00 restantes");
        assert!(b.roll.is_none());
        // passou um minuto e meio: desce sozinho, e os dígitos rolam desde o que estava escrito
        b.left = b.left.map(|(s, at)| (s, at - Duration::from_secs(90)));
        assert!(b.apply_power(Some(p.clone())));
        assert_eq!(b.status_line(), "3 h 58 restantes");
        assert_eq!(b.roll.as_ref().map(|r| r.status.as_str()), Some("4 h 00 restantes"));
        // a conta mudou de verdade (abriu um jogo): pula para a nova
        p.secs_left = Some(2 * 3600);
        assert!(b.apply_power(Some(p)));
        assert_eq!(b.status_line(), "2 h 00 restantes");
        assert!(keeps_left(3600.0, 3600.0 + 120.0) && !keeps_left(3600.0, 3600.0 + 180.0));
        assert!(keeps_left(5.0 * 3600.0, 5.0 * 3600.0 + 700.0) && !keeps_left(5.0 * 3600.0, 5.0 * 3600.0 + 800.0));
    }

    /// Os dígitos andam todos para o mesmo lado, dando a volta no 9/0, como num hodômetro.
    #[test]
    fn odometer_digits_wrap_around() {
        // 2 h 59 → 3 h 00 (subindo): 2→3, 5→0 e 9→0 andam uma, cinco e uma casa para cima
        assert_eq!((odometer_steps(2, 3, true), odometer_steps(5, 0, true), odometer_steps(9, 0, true)), (1, 5, 1));
        // 3 h 00 → 2 h 59 (descendo): tudo para baixo
        assert_eq!((odometer_steps(3, 2, false), odometer_steps(0, 5, false), odometer_steps(0, 9, false)), (-1, -5, -1));
        assert_eq!(odometer_steps(4, 4, true), 0);
        assert!(digits_of("3 h 00 restantes") > digits_of("2 h 59 restantes"));
        // desenha passando pela virada, em todos os tamanhos
        com();
        let env_cfg = json!({"bateria": {}, "app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        let mut b = Battery::sample();
        b.roll = Some(Rolled { percent: "73".into(), status: "2 h 59 restantes".into(), header: b.header_line(), at: Instant::now() - Duration::from_millis(300) });
        let mut p = b.power.clone().unwrap();
        p.secs_left = Some(3 * 3600);
        b.power = Some(p);
        for mode in Mode::ALL {
            let l = Layout::of(mode);
            let mut c = Canvas::new(l.w as u32, l.h as u32, 1.0);
            b.draw(&mut c, mode, &env);
        }
    }

    /// Os anéis do médio e do grande, com 9%, 73% e 100%, num PNG (para conferir de olho).
    #[test]
    #[ignore]
    fn ring_sheet() {
        com();
        let env_cfg = json!({"bateria": {}, "app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        let s = 2.0;
        let mut sheet = Canvas::new((3.0 * WIDE * s) as u32, (520.0 * s) as u32, s);
        sheet.fill_round_rect(0.0, 0.0, 3.0 * WIDE, 520.0, 0.0, Rgba::rgb(40, 40, 52));
        // 9% na bateria; 73% e 100% carregando (com o raio dentro do anel)
        for (i, v) in [9.0, 73.0, 100.0].into_iter().enumerate() {
            let mut b = Battery::sample();
            let p = b.power.as_mut().unwrap();
            p.percent = v;
            (p.plugged, p.charging) = (i > 0, i > 0);
            for (mode, y) in [(Mode::M, 0.0), (Mode::G, 160.0)] {
                let l = Layout::of(mode);
                let mut c = Canvas::new((l.w * s) as u32, (l.h * s) as u32, s);
                b.draw(&mut c, mode, &env);
                let g = b.ring_of(mode).unwrap();
                // as guias: o centro do anel
                c.fill_round_rect(g.cx - 0.25, g.cy - g.r, g.cx + 0.25, g.cy + g.r, 0.0, Rgba(255, 0, 0, 160));
                c.fill_round_rect(g.cx - g.r, g.cy - 0.25, g.cx + g.r, g.cy + 0.25, 0.0, Rgba(255, 0, 0, 160));
                sheet.image(&c.finish(), i as f32 * WIDE, y);
            }
        }
        let path = std::env::temp_dir().join("bateria_aneis.png");
        crate::gfx::save_png(&sheet.finish(), &path).unwrap();
        println!("{}", path.display());
    }

    /// Uma leitura estranha de um instante ("sem bateria", ou a carga pulando de 65% para 0%)
    /// não apaga o anel: espera a próxima. Se ela se confirmar, aí vale.
    #[test]
    fn odd_readings_wait_for_the_next_one() {
        let mut b = Battery::sample();
        let ok = b.power.clone().unwrap();
        // "sem bateria" por uma leitura: fica a de antes
        assert_eq!(b.steady(None), Some(ok.clone()));
        assert_eq!(b.steady(Some(ok.clone())), Some(ok.clone()));
        // a carga pulando para 0% e voltando: o 0% nunca aparece
        let mut zero = ok.clone();
        zero.percent = 0.0;
        assert_eq!(b.steady(Some(zero.clone())), Some(ok.clone()));
        assert_eq!(b.steady(Some(ok.clone())), Some(ok.clone()));
        // mudanças normais passam na hora (1% a menos, ligar na tomada mesmo com salto)
        let mut less = ok.clone();
        less.percent -= 1.0;
        assert_eq!(b.steady(Some(less.clone())), Some(less.clone()));
        let mut plugged = less.clone();
        (plugged.plugged, plugged.percent) = (true, 40.0);
        assert_eq!(b.steady(Some(plugged.clone())), Some(plugged.clone()));
        // uma mudança de verdade (a bateria saiu): vale depois de se repetir
        b.power = Some(ok.clone());
        assert_eq!(b.steady(None), Some(ok.clone()));
        assert_eq!(b.steady(None), Some(ok.clone()));
        assert_eq!(b.steady(None), None);
    }

    /// Passando por leituras estranhas e animações, o anel termina desenhado com a cor da
    /// carga (o verde da bateria não some).
    #[test]
    fn the_ring_keeps_its_color_through_odd_readings() {
        com();
        let env_cfg = json!({"bateria": {}, "app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        let mut b = Battery::sample();
        let mut p = b.power.clone().unwrap();
        p.percent = 65.0;
        let mut zero = p.clone();
        zero.percent = 0.0;
        for reading in [Some(p.clone()), None, Some(p.clone()), Some(zero), Some(p.clone()), Some(p.clone())] {
            let r = b.steady(reading);
            b.apply_power(r);
            // a animação de cada mudança terminou
            b.grow = b.grow.map(|(f, t, at, d)| (f, t, at - Duration::from_secs(5), d));
            b.roll = None;
        }
        assert_eq!(b.shown(), 65.0);
        let l = Layout::of(Mode::M);
        let mut c = Canvas::new(l.w as u32, l.h as u32, 1.0);
        b.draw(&mut c, Mode::M, &env);
        let img = c.finish();
        // um ponto no meio do arco (a 30% do caminho): verde
        let (x, y) = RING_M.at(30.0);
        let i = ((y.round() as u32 * img.w + x.round() as u32) * 4) as usize;
        let (bl, g, r) = (img.px[i] as i32, img.px[i + 1] as i32, img.px[i + 2] as i32);
        assert!(g > 150 && g > r + 40 && g > bl + 20, "o arco não está verde: {:?}", (r, g, bl));
    }

    /// Depois que uma animação acaba, mais um quadro é desenhado: o último podia ficar no meio do
    /// caminho (o arco ou os dígitos pela metade) até o mouse passar em cima.
    #[test]
    fn draws_one_more_frame_after_an_animation_ends() {
        let mut b = Battery::sample_rolling(100);
        b.fixed_now = None;
        let (redraw, _) = ctx_run(|ctx| b.tick(ctx));
        assert!(redraw);
        // o tick seguinte atrasou e a animação já acabou
        b.roll.as_mut().unwrap().at -= Duration::from_secs(5);
        b.grow = b.grow.map(|(f, t, at, d)| (f, t, at - Duration::from_secs(5), d));
        assert!(!b.animating());
        assert!(b.next_tick().is_some_and(|t| t <= Instant::now() + FRAME));
        let (redraw, _) = ctx_run(|ctx| b.tick(ctx));
        assert!(redraw, "o quadro final");
        let (redraw, _) = ctx_run(|ctx| b.tick(ctx));
        assert!(!redraw, "e só ele");
    }

    #[test]
    fn the_shimmer_runs_only_while_charging_and_now_and_then() {
        let b = Battery::sample();
        assert!(b.shimmer().is_none() && b.overlay_fps() == 0, "parado, nada se mexe");
        let mut c = Battery::sample_charging();
        let mut seen = (0, 0);
        for k in 0..70 {
            c.charge_since = Instant::now() - Duration::from_millis(700 + k * 50);
            if c.shimmer().is_some() { seen.0 += 1 } else { seen.1 += 1 }
        }
        assert!(seen.0 > 10 && seen.1 > 20, "{seen:?}");
    }

    #[test]
    fn devices_arriving_and_leaving_animate_and_low_ones_warn_once() {
        let mut b = Battery::sample();
        let mut list = b.devices.clone();
        // o controle conecta; o mouse desconecta
        for d in &mut list {
            if d.name == "Controle Xbox" {
                d.connected = true;
            }
            if d.name.starts_with("Mouse") {
                d.connected = false;
            }
        }
        let (changed, out) = ctx_run(|ctx| b.apply_devices(list.clone(), ctx));
        assert!(changed);
        assert_eq!(b.change_of("Controle Xbox").map(|c| c.0), Some(true));
        assert_eq!(b.change_of("Mouse MX Master 3S").map(|c| c.0), Some(false));
        // o teclado com 12%: um aviso só
        let warns = out.iter().filter(|r| matches!(r, super::super::Request::Notify("bateria", ..))).count();
        assert_eq!(warns, 1);
        list[0].name.clone_from(&b.devices[0].name);
        let (_, out) = ctx_run(|ctx| {
            let mut again = b.devices.clone();
            again[0].level = again[0].level.map(|l| l.saturating_sub(1));
            b.apply_devices(again, ctx)
        });
        assert!(!out.iter().any(|r| matches!(r, super::super::Request::Notify(..))), "avisou de novo");
    }

    #[test]
    fn the_history_keeps_a_day_and_fills_the_stats() {
        let b = Battery::sample();
        let (today, longest) = b.day_stats();
        assert!(longest.is_some_and(|l| l > 3600.0));
        assert!(today.is_some());
        // grava de 5 em 5 minutos no cache, sem passar de um dia
        let mut b = Battery::sample();
        b.last_sample = 0.0;
        let (_, out) = ctx_run(|ctx| {
            b.sample_now(ctx);
            assert!(ctx.cache["bateria"]["hist"].as_array().is_some_and(|a| a.len() <= 290));
        });
        assert!(out.iter().any(|r| matches!(r, super::super::Request::SaveCache)));
        let first = b.hist.first().unwrap().0;
        assert!(b.now() - first <= DAY + SAMPLE_EVERY);
        // lê o histórico guardado
        let cache = json!({"bateria": {"hist": [[1.0, 50.0, true], [301.0, 49.0, false]]}});
        assert_eq!(Battery::new(&cache).hist.len(), 2);
    }

    #[test]
    fn a_full_charge_shrinks_to_fit_inside_the_ring() {
        com();
        let c = Canvas::new(10, 10, 1.0);
        for (g, size) in [(RING_P, 38.0), (RING_M, 30.0), (RING_G, 40.0)] {
            let max = inner_width(g);
            // dois dígitos: o tamanho de sempre; o "100%": menor, e cabendo com o respiro
            let two = fit_size(&c, "73", size, max);
            let three = fit_size(&c, "100", size, max);
            assert_eq!(two, size, "{g:?}: o 73% diminuiu");
            assert!(three < size, "{g:?}: o 100% não diminuiu");
            let w = c.text_width("100", NUM, three) + 1.0 + c.text_width("%", NUM, three * 0.42);
            assert!(w <= max + 0.01 && w < 2.0 * (g.r - g.w / 2.0) - 20.0, "{g:?}: {w} de {max}");
        }
    }

    #[test]
    fn everything_fits_inside_the_card() {
        com();
        let env_cfg = json!({"bateria": {}, "app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        // (a onda de ligar na tomada em vários pontos do caminho: dependendo de quanto demorava o
        // teste, ela passava ou não perto da borda)
        let waves = [100, 300, 450, 600].map(Battery::sample_plugging);
        for b in [Battery::sample(), Battery::sample_charging(), Battery::sample_desktop(), Battery::sample_alone(), Battery::sample_nothing(), Battery::sample_full(400), Battery::sample_changing(300), Battery::sample_rolling(300)].into_iter().chain(waves) {
            for mode in Mode::ALL {
                let l = Layout::of(mode);
                let mut c = Canvas::new((l.w * 1.25) as u32, (l.h * 1.25) as u32, 1.25);
                b.draw(&mut c, mode, &env);
                b.overlay(&mut c, mode, 0.0, 0.0);
                let img = c.finish();
                // nada encosta na borda (a 4 pt dela, só o reflexo da carga completa)
                let edge = (4.0 * 1.25) as u32;
                let painted = |x: u32, y: u32| img.px[((y * img.w + x) * 4 + 3) as usize] > 40;
                let touches = (0..img.w).any(|x| (0..edge).any(|y| painted(x, y) || painted(x, img.h - 1 - y)))
                    || (0..img.h).any(|y| (0..edge).any(|x| painted(x, y) || painted(img.w - 1 - x, y)));
                assert!(!touches || b.full_at.is_some(), "{mode:?}: encosta na borda");
            }
        }
        // as linhas do grande cabem embaixo do gráfico
        assert!(342.0 + 3.0 * 25.0 + 22.0 <= super::super::LARGE_H - 6.0);
    }
}
