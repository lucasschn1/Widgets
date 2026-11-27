//! Widget Bateria: a bateria do notebook (com a saúde, os ciclos e as últimas 24 horas) e a dos
//! dispositivos conectados (fones, mouses, teclados, controles). No pequeno, um anel por aparelho
//! (os lugares que sobram ficam com o anel vazio); no médio, o anel do notebook e a lista; no
//! grande, tudo. Num PC sem bateria, só os dispositivos. Os ícones são desenhados (`vector`).
//!
//! As animações tocam nos momentos que importam e param depois: ligar na tomada (uma onda sai do
//! anel e o raio entra com um "pop"; o arco fica onde está), a carga mudando (os números rolam
//! como num hodômetro), a carga completa (um reflexo atravessa o cartão e o raio vira um ✓), um
//! aparelho chegando ou saindo (no pequeno, o ícone entra com um "pop", uma onda sai do anel e
//! o arco enche do zero), a bateria baixa (a ponta do arco respira uma vez por minuto) e o
//! gráfico se desenhando ao abrir o grande. Carregando, um brilho percorre o arco a cada 3,5 s.
//! Parado, nada se mexe. (O reflexo do mouse na borda de vidro é do cartão, em todos os widgets.)

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::island_ui::{self as iu, SOFT};
use super::{alert_hash, Ctx, Env, Hot, HoverStyle, IslandForm, IslandItem, Kind, Layout, MenuItem, Mode, Rect4, Status, SMALL, WIDE};
use crate::platform::battery::{self, DevKind, Device, Power};
use super::odometer::draw_rolling;
use super::vector;
use crate::gfx::{Anchor, Canvas, Face, Fill};
use crate::ui::theme::{ease_in_out, Rgba, FG, MUTED, WHITE};
use crate::platform::win;

// ---------- cores e fontes ----------
const OK: Rgba = crate::ui::theme::GREEN;
/// Com a cor do papel de parede, os matizes que não mudam (graus no OKLab): do vermelho da
/// bateria fraca (uns 27°) ao âmbar da média (uns 84°); o verde (uns 153°) muda.
const ALERT_HUES: (f32, f32) = (10.0, 100.0);
const MID: Rgba = crate::ui::theme::AMBER;
const LOW: Rgba = crate::ui::theme::RED;
/// o trilho dos anéis e das barras (9%), o dos anéis do pequeno (10%) e o do anel vazio (8%)
const TRACK: Rgba = Rgba(255, 255, 255, 23);
const MINI_TRACK: Rgba = Rgba(255, 255, 255, 26);
const EMPTY_TRACK: Rgba = Rgba(255, 255, 255, 20);
/// os fios entre as linhas da saúde e o que separa o anel da lista (7%) e as guias do gráfico (7%)
const RULE: Rgba = Rgba(255, 255, 255, 18);
const GRID: Rgba = Rgba(255, 255, 255, 18);
/// os textos de apoio (64%), o "Notebook" (58%), a legenda (60%) e os ícones (82% e 86%)
const SUB: Rgba = Rgba(255, 255, 255, 163);
const LEGEND: Rgba = Rgba(255, 255, 255, 153);
const ICON: Rgba = Rgba(255, 255, 255, 209);
const MINI_ICON: Rgba = Rgba(255, 255, 255, 219);
/// o quadradinho de vidro dos ícones da lista (6%, contorno de 4%) e o painel do gráfico (3,5%,
/// contorno de 5%)
const TILE: Rgba = Rgba(255, 255, 255, 15);
const TILE_LINE: Rgba = Rgba(255, 255, 255, 10);
const PANEL: Rgba = Rgba(255, 255, 255, 9);
const PANEL_LINE: Rgba = Rgba(255, 255, 255, 13);
/// a linha embaixo do anel: na bateria, na tomada e com a bateria baixa
const STATUS: Rgba = Rgba(255, 255, 255, 184);
const CHARGE_TEXT: Rgba = Rgba::rgb(125, 219, 160);
const LOW_TEXT: Rgba = Rgba::rgb(255, 154, 144);
/// os números grandes (os mesmos dos outros widgets)
const NUM: Face = super::PERCENT;
/// os valores menores (a saúde, os ciclos, a carga de cada aparelho)
const VALUE: Face = super::VALUE;
const TEXT: Face = Face::Regular;
const MEDIUM: Face = Face::Medium;
const BOLD: Face = Face::Semibold;
/// a altura dos dígitos da fonte dos números (em relação ao tamanho da fonte): para centrar o número no anel
const DIGIT_H: f32 = 0.70;
/// o "%" em relação ao número
const PCT: f32 = 0.37;
/// o tamanho da porcentagem no anel do médio e no do grande (o "100%" diminui para caber)
const PERCENT_M: f32 = 30.0;
const PERCENT_G: f32 = 40.0;
/// o espaço entre a porcentagem e o "%"
const SIGN_GAP: f32 = 1.5;

/// O degradê do arco de cada cor: do tom mais fundo (no começo, embaixo à esquerda) ao mais claro.
fn gradient_of(c: Rgba) -> (Rgba, Rgba) {
    if c == LOW {
        (Rgba::rgb(232, 78, 66), Rgba::rgb(255, 150, 139))
    } else if c == MID {
        (Rgba::rgb(228, 158, 54), Rgba::rgb(246, 210, 138))
    } else {
        (Rgba::rgb(61, 190, 111), Rgba::rgb(143, 227, 174))
    }
}

/// A cor com a opacidade multiplicada por `a`.
fn fade(c: Rgba, a: f32) -> Rgba {
    c.with_a((c.3 as f32 * a.clamp(0.0, 1.0)).round() as u8)
}

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
    fn at(self, level: f32) -> (f32, f32) {
        let angle = (START + SWEEP * level.clamp(0.0, 100.0) / 100.0).to_radians();
        (self.cx + self.r * angle.cos(), self.cy + self.r * angle.sin())
    }
}

const RING_M: Ring = Ring { cx: 66.0, cy: 58.0, r: 40.0, w: 8.0 };
/// grande: onde começa a lista dos aparelhos, embaixo do painel do gráfico
const G_ROWS_Y: f32 = 314.0;
const RING_G: Ring = Ring { cx: 80.0, cy: 126.0, r: 52.0, w: 10.0 };
/// a margem de dentro do cartão médio e do grande (a de todos os widgets)
const INSET: f32 = super::CARD_PAD;
/// no grande, a linha embaixo do rótulo (quanto falta ou de onde vem a energia)
const TOP_LINE_Y: f32 = 30.0;
/// o pequeno: a grade 2 × 2 começa em `CELL_X` (nos dois eixos), com lugares de `CELL` (para
/// saber em qual o mouse está); cada anel tem raio `MINI_R` e traço `MINI_W`, e os quatro vão até
/// as margens do cartão, com a carga embaixo de cada um (como no Baterias do macOS)
const CELL_X: f32 = super::CARD_PAD;
const CELL: f32 = (SMALL - 2.0 * CELL_X) / 2.0;
const MINI_R: f32 = 19.0;
const MINI_W: f32 = 4.5;
/// o raio de fora do anel do pequeno (até a borda do traço)
const MINI_OUT: f32 = MINI_R + MINI_W / 2.0;
/// a carga embaixo de cada anel do pequeno: o tamanho e o espaço entre o anel e o alto dos
/// algarismos
const MINI_LABEL_SIZE: f32 = 12.5;
const MINI_LABEL_GAP: f32 = 4.0;
/// o ícone no meio do anel do pequeno
const MINI_ICON_SIZE: f32 = 19.0;

/// O meio da abertura de baixo do anel (entre as duas pontas do arco): onde fica o ícone.
fn g_opening(ring: Ring) -> (f32, f32) {
    (ring.cx, ring.cy + ring.r * std::f32::consts::FRAC_1_SQRT_2)
}

/// Como desenhar uma linha da lista: com o ícone num quadradinho de vidro deste tamanho, ou solto.
#[derive(Clone, Copy)]
enum Row {
    Tile(f32),
    Inline,
}

/// Um lugar do pequeno: o notebook, um aparelho ou vazio.
#[derive(Clone, Copy)]
enum Spot<'a> {
    Laptop,
    Dev(&'a Device),
    Empty,
}

/// O arco de `g` até `v` (0 a 100) num degradê na diagonal do anel (de baixo à esquerda para
/// cima à direita), em pedaços de até 8°.
fn gradient_arc(c: &mut Canvas, ring: Ring, level: f32, color: Rgba) {
    let (from, to) = gradient_of(color);
    let end = START + SWEEP * level.clamp(0.0, 100.0) / 100.0;
    let pieces = ((end - START) / 8.0).ceil().max(1.0) as usize;
    for i in 0..pieces {
        let a0 = START + (end - START) * i as f32 / pieces as f32;
        let a1 = START + (end - START) * (i + 1) as f32 / pieces as f32;
        // a cor do pedaço: onde o meio dele cai na diagonal (0 embaixo à esquerda, 1 em cima à
        // direita)
        let mid = ((a0 + a1) / 2.0).to_radians();
        let (px, py) = (ring.r * mid.cos(), ring.r * mid.sin());
        let along = ((px + ring.r) + (ring.r - py)) / (4.0 * ring.r);
        c.arc(ring.cx, ring.cy, ring.r, a0, a1, from.mix(to, along.clamp(0.0, 1.0)), ring.w);
    }
}

/// O ícone do aparelho chegando no pequeno: (escala, opacidade) aos `ms` do começo. Entra pequeno,
/// passa do tamanho (112%) e volta.
fn connect_icon(ms: f32) -> (f32, f32) {
    if ms < 120.0 {
        // cresce e acende
        let step = ease_out(ms / 120.0);
        (0.5 + 0.35 * step, 0.8 * step)
    } else if ms < 250.0 {
        // passa do tamanho
        let step = ease_out((ms - 120.0) / 130.0);
        (0.85 + 0.27 * step, 0.8 + 0.2 * step)
    } else {
        // e volta aos 100%
        (1.12 - 0.12 * ease_in_out(((ms - 250.0) / 130.0).min(1.0)), 1.0)
    }
}

/// A linha suave (Catmull-Rom) pelos pontos: duas paradas a mais entre cada par.
fn catmull_rom(pts: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(pts.len() * 3);
    for i in 0..pts.len() - 1 {
        let (p0, p1, p2, p3) = (pts[i.saturating_sub(1)], pts[i], pts[i + 1], pts[(i + 2).min(pts.len() - 1)]);
        let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
        let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
        out.push(p1);
        // dois pontos da curva de Bézier entre p1 e p2 (c1 e c2 são os pontos de controle)
        for t in [1.0 / 3.0, 2.0 / 3.0] {
            let rest = 1.0 - t;
            let weight = [rest * rest * rest, 3.0 * rest * rest * t, 3.0 * rest * t * t, t * t * t];
            let mix = |a: f32, b: f32, c: f32, d: f32| weight[0] * a + weight[1] * b + weight[2] * c + weight[3] * d;
            out.push((mix(p1.0, c1.0, c2.0, p2.0), mix(p1.1, c1.1, c2.1, p2.1)));
        }
    }
    out.push(pts[pts.len() - 1]);
    out
}

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
/// um aparelho chegando ou saindo: a animação inteira (no pequeno) e a da linha da lista (desliza
/// e a barra enche, ou esmaece e a barra esvazia)
const DEVICE_MS: f32 = 900.0;
const ROW_MS: f32 = 800.0;
/// a onda saindo do anel do aparelho que chegou, no pequeno
const RIPPLE_CELL_MS: f32 = 500.0;
/// o gráfico se desenhando
const REVEAL_MS: f32 = 1300.0;
/// o brilho correndo no arco, carregando: um a cada `SHIMMER_EVERY`, visível nesta parte dele
const SHIMMER_EVERY: f32 = 3500.0;
const SHIMMER_ON: (f32, f32) = (0.06, 0.44);
/// a ponta do arco respirando com a bateria baixa: um pulso deste tamanho a cada minuto
const LOW_EVERY: f32 = 60_000.0;
const LOW_MS: f32 = 2400.0;

/// Quantos milésimos se passaram desde `t`.
fn ms_since(t: Instant) -> f32 {
    t.elapsed().as_secs_f32() * 1000.0
}

/// Começa rápido e chega devagar (0 a 1).
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
const SAMPLE_EVERY: f64 = 5.0 * 60.0;
const DAY: f64 = 24.0 * 3600.0;
/// avisa quando um aparelho conectado passa disto; avisa de novo só depois de carregar até `REARM`
const LOW_AT: u8 = crate::data::config::LOW_BATTERY;
const REARM: u8 = LOW_AT + 5;

// ---------- ações ----------
const OPEN_POWER: u32 = 1;
const OPEN_BLUETOOTH: u32 = 2;
/// na ilha: "OK" (não avisar mais deste aparelho até ele carregar)
const DISMISS: u32 = 3;
/// na ilha abaixo disto (o mesmo limite da notificação); abaixo de `ISLAND_URGENT`, fica na pílula
/// até alguém ver
const ISLAND_LOW: f32 = LOW_AT as f32;
const ISLAND_URGENT: f32 = 10.0;
/// ao entrar ou sair da tomada, a ilha mostra a carga por este tempo
const PLUG_SHOW: Duration = Duration::from_secs(8);
/// um aparelho que acabou de conectar fica na ilha por este tempo
const ARRIVAL_SHOW: Duration = Duration::from_secs(8);
/// só avisa de novo quando o aparelho ficou desconectado ao menos isto: fones e controles que
/// piscam a conexão não esticam a ilha a cada vez
const AWAY_FOR: Duration = Duration::from_secs(2 * 60);
/// teclado, mouse e caneta cochilam sozinhos (e reconectam quando mexidos): para eles, só depois
/// de muito tempo longe, como na volta ao PC
const AWAY_FOR_INPUT: Duration = Duration::from_secs(30 * 60);
/// logo depois de abrir o app, as primeiras listas trazem todo mundo "chegando": não avisa
const ARRIVAL_QUIET_START: Duration = Duration::from_secs(20);
/// de quanto em quanto tempo conferir se entrou ou saiu da tomada e se chegou uma lista nova de
/// dispositivos (só isso: barato)
const CHECK_EVERY: Duration = Duration::from_millis(500);

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
    /// a de baixo do anel do grande
    under: String,
    at: Instant,
}

/// Um aparelho que acabou de conectar (o aviso na ilha).
#[derive(Clone, Debug, PartialEq)]
struct Arrival {
    name: String,
    kind: DevKind,
    /// a bateria dele, se informa
    level: Option<u8>,
    at: Instant,
    /// quantos já chegaram (cada um é um aviso novo na ilha)
    count: u64,
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
    /// as listas de dispositivos, lidas num fio à parte (de tempos em tempos e logo depois de um
    /// aparelho conectar ou desconectar)
    feed: Option<Receiver<Vec<Device>>>,
    /// os aparelhos nos lugares do pequeno, na ordem em que entraram (None: o lugar de um que saiu)
    slots: Vec<Option<String>>,
    next_power: Instant,
    /// "agora" fixo nos exemplos (sem ler o sistema)
    fixed_now: Option<f64>,
    // animações
    grow: Option<(f32, f32, Instant, f32)>,
    roll: Option<Rolled>,
    /// o consumo médio na bateria (mW) e quando foi lido por último
    drain: Option<(f32, Instant)>,
    /// o tempo restante mostrado (segundos) e desde quando: dali ele desce como um relógio
    left: Option<(f64, Instant)>,
    /// os textos (porcentagem, linha de baixo, linha de cima, linha de baixo do grande) do último
    /// desenho pedido
    texts: Option<Texts>,
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
    /// os avisos da ilha já dispensados ("nome|nível"), até o aparelho carregar
    island_ok: Vec<String>,
    /// entrou (true) ou saiu (false) da tomada, e quando: a ilha mostra por `PLUG_SHOW`
    plug_event: Option<(bool, Instant)>,
    /// quantas vezes entrou ou saiu (cada uma é um aviso novo na ilha)
    plug_count: u64,
    /// o último aparelho que conectou: a ilha mostra por `ARRIVAL_SHOW`
    arrival: Option<Arrival>,
    /// quantos aparelhos já conectaram com aviso (não volta a zero com o OK: o mesmo aparelho
    /// chegando de novo é outro aviso)
    arrivals: u64,
    /// quando cada aparelho desconectou (para não avisar quem só piscou a conexão)
    away_since: HashMap<String, Instant>,
    /// quando o widget nasceu (as primeiras listas não avisam)
    born: Instant,
    /// próxima conferida rápida da tomada e dos dispositivos
    next_check: Instant,
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
        b.feed = battery::watch_devices();
        b
    }

    fn empty() -> Battery {
        Battery {
            power: None,
            devices: Vec::new(),
            hist: Vec::new(),
            last_sample: 0.0,
            feed: None,
            slots: Vec::new(),
            next_power: Instant::now() + POWER_EVERY,
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
            island_ok: Vec::new(),
            doubt: 0,
            plug_event: None,
            plug_count: 0,
            arrival: None,
            arrivals: 0,
            away_since: HashMap::new(),
            born: Instant::now(),
            next_check: Instant::now() + CHECK_EVERY,
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

    /// Acabou de entrar (`plugged`) ou sair da tomada: o aviso na ilha.
    #[cfg(test)]
    pub fn sample_plug_event(plugged: bool) -> Battery {
        let mut b = if plugged { Battery::sample_charging() } else { Battery::sample() };
        // nenhum aparelho fraco (o aviso de pouca bateria passaria na frente)
        for d in &mut b.devices {
            d.level = d.level.map(|l| l.max(60));
        }
        b.plug_event = Some((plugged, Instant::now()));
        b.plug_count = 1;
        b
    }

    /// O fone acabou de conectar: o aviso na ilha.
    #[cfg(test)]
    pub fn sample_arrival() -> Battery {
        let mut b = Battery::sample_plug_event(false);
        b.plug_event = None;
        b.arrival = Some(Arrival { name: "Fone WH-1000XM5".into(), kind: DevKind::Headphones, level: Some(82), at: Instant::now(), count: 1 });
        b
    }

    /// A carga chegou a 100% há `ms` milissegundos (o reflexo e o ✓).
    #[cfg(test)]
    pub fn sample_full(ms: u64) -> Battery {
        let mut b = Battery::sample_charging();
        b.power = Some(Power { percent: 100.0, plugged: true, charging: false, secs_left: None, secs_to_full: None, health: Some(0.87), cycles: Some(312), ..Default::default() });
        b.full_at = Some(Instant::now() - Duration::from_millis(ms));
        b.roll = Some(Rolled { percent: "99".into(), status: "cheia às 15:03".into(), header: "Carregando · cheia às 15:03".into(), under: "cheia em 2 min".into(), at: Instant::now() - Duration::from_millis(ms) });
        b
    }

    /// A carga caindo de 73% para 68% (e o tempo de 3 h 20 para 3 h 05) há `ms` milissegundos:
    /// os dígitos no meio do caminho.
    #[cfg(test)]
    pub fn sample_rolling(ms: u64) -> Battery {
        let mut b = Battery::sample();
        let before = b.current_texts();
        let mut p = b.power.clone().unwrap();
        p.percent = 68.0;
        p.secs_left = Some(3 * 3600 + 5 * 60);
        b.apply_power(Some(p));
        let r = b.roll.as_mut().expect("rola");
        (r.percent, r.status, r.header, r.under) = before;
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

    /// Só o notebook, com o fone conectando há `ms` milissegundos (no pequeno, ele entra no
    /// primeiro anel vazio).
    #[cfg(test)]
    pub fn sample_connecting(ms: u64) -> Battery {
        let mut b = Battery::sample();
        b.devices.retain(|d| d.kind == DevKind::Headphones);
        b.changes = vec![Change { name: b.devices[0].name.clone(), arriving: true, since: Instant::now() - Duration::from_millis(ms) }];
        b
    }

    fn now(&self) -> f64 {
        self.fixed_now.unwrap_or_else(win::now)
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
        let power = self.power.as_ref().filter(|p| !p.plugged)?;
        match (power.left_mwh, self.drain) {
            (Some(left), Some((mw, _))) if mw >= 100.0 => Some(left as f64 / mw as f64 * 3600.0),
            _ => power.secs_left.map(|s| s as f64),
        }
    }

    /// O que dizer embaixo do anel: o tempo que falta, a hora em que enche ou o estado.
    fn status_line(&self) -> String {
        let Some(power) = &self.power else { return String::new() };
        if !power.plugged {
            return self.secs_left().map_or("na bateria".into(), |s| format!("{} restantes", duration(s)));
        }
        if power.charging {
            return power.secs_to_full.map_or("carregando".into(), |s| format!("cheia às {}", win::clock_at(self.now() + s as f64)));
        }
        if power.percent >= 99.0 { "carga completa".into() } else { "na tomada".into() }
    }

    /// A linha de cima do grande: "Na bateria · acaba às 17:40", "Carregando · cheia às 15:03".
    fn header_line(&self) -> String {
        let Some(power) = &self.power else {
            let connected = self.devices.iter().filter(|d| d.connected).count();
            return match connected {
                0 => "Nenhum aparelho conectado".into(),
                1 => "PC na tomada · 1 conectado".into(),
                n => format!("PC na tomada · {n} conectados"),
            };
        };
        if !power.plugged {
            return match self.secs_left() {
                Some(s) => format!("Na bateria · acaba às {}", win::clock_at(self.now() + s)),
                None => "Na bateria".into(),
            };
        }
        if power.charging {
            return match power.secs_to_full {
                Some(s) => format!("Carregando · cheia às {}", win::clock_at(self.now() + s as f64)),
                None => "Carregando".into(),
            };
        }
        if power.percent >= 99.0 { "Na tomada · carga completa".into() } else { "Na tomada · sem carregar".into() }
    }

    /// Quanto tempo esteve na tomada hoje e o maior tempo seguido fora dela nas últimas 24 h.
    fn day_stats(&self) -> (Option<f64>, Option<f64>) {
        if self.hist.len() < 2 {
            return (None, None);
        }
        let now = self.now();
        let local = win::local_time();
        let midnight = now - (local.hour as f64 * 3600.0 + local.minute as f64 * 60.0 + local.second as f64);
        let (mut plugged_today, mut longest, mut run) = (0.0, 0.0f64, 0.0);
        // de uma amostra do histórico (horário, carga, na tomada) para a seguinte
        for pair in self.hist.windows(2) {
            let (sample, next) = (pair[0], pair[1]);
            let span = (next.0 - sample.0).clamp(0.0, 2.0 * SAMPLE_EVERY);
            if sample.2 && next.0 > midnight {
                plugged_today += (next.0 - sample.0.max(midnight)).clamp(0.0, span);
            }
            if !sample.2 && next.0 > now - DAY {
                run += span;
                longest = longest.max(run);
            } else {
                run = 0.0;
            }
        }
        (Some(plugged_today), (longest > 0.0).then_some(longest))
    }

    // ---------- desenho ----------

    /// O anel do notebook: o trilho, o arco até a carga (num degradê da cor dela), a ponta com
    /// o brilho, a onda de quando liga na tomada e o brilho vermelho que respira com a bateria
    /// baixa.
    fn draw_ring(&self, c: &mut Canvas, ring: Ring) {
        let level = self.shown();
        c.arc(ring.cx, ring.cy, ring.r, START, START + SWEEP, TRACK, ring.w);
        // ligou na tomada: uma onda sai do anel
        if let Some(at) = self.plug_at.filter(|&t| ms_since(t) < RIPPLE_MS) {
            let wave = ease_out(ms_since(at) / RIPPLE_MS);
            // (cresce até 6 pt da borda do cartão, no máximo: com o anel mais perto da borda, na
            // margem de 16, a onda de 50% a mais encostava nela)
            let room = (ring.cx.min(ring.cy) - 6.0 - ring.r - ring.w).clamp(4.0, 0.5 * ring.r);
            c.stroke_circle(ring.cx, ring.cy, ring.r + room * wave, OK.alpha(0.7 * (1.0 - wave)), ring.w * 0.6 * (1.0 - wave) + 1.0);
        }
        let color = level_color(self.power.as_ref().map_or(level, |p| p.percent));
        if level > 0.05 {
            gradient_arc(c, ring, level, color);
        }
        // a ponta do arco (com o brilho vermelho respirando, na bateria baixa)
        let (tip_x, tip_y) = ring.at(level);
        if let Some(strength) = self.low_pulse() {
            c.fill_circle(tip_x, tip_y, ring.w * 1.3, LOW.alpha(0.55 * strength));
        }
        c.fill_circle(tip_x, tip_y, ring.w * 0.75, color.alpha(0.30));
        c.fill_circle(tip_x, tip_y, ring.w * 0.3, WHITE);
    }

    /// A ponta respirando com a bateria baixa: a força do brilho agora (None: fora do pulso).
    fn low_pulse(&self) -> Option<f32> {
        let power = self.power.as_ref()?;
        if power.plugged || power.percent >= 15.0 {
            return None;
        }
        let into_minute = ms_since(self.charge_since) % LOW_EVERY;
        (into_minute < LOW_MS).then(|| (into_minute / LOW_MS * std::f32::consts::PI).sin())
    }

    /// O raio (ou o ✓ da carga completa) do notebook agora, com o "pop" de quando liga na tomada:
    /// (o desenho, a escala, a opacidade); None: sem raio.
    fn badge(&self) -> Option<(&'static str, f32, f32)> {
        if !self.plugged() {
            return None;
        }
        let check = self.showing_check();
        if !self.charging() && !check {
            return None;
        }
        let since = if check { self.full_at } else { self.plug_at };
        let scale = since.map_or(1.0, |t| pop(ms_since(t) / POP_MS));
        let alpha = since.map_or(1.0, |t| (ms_since(t) / (POP_MS * 0.4)).min(1.0));
        Some((if check { vector::CHECK } else { vector::BOLT }, scale, alpha))
    }

    /// A porcentagem grande, com os dígitos que mudaram rolando (o de antes sobe e some, o novo
    /// vem de baixo), e o "%" menor e um pouco acima depois dela, com o centro óptico em x
    /// (`super::optical_start`) e os dígitos no meio na altura de cy. O tamanho diminui até tudo caber em `max_w` (o "100%", de três dígitos, não
    /// encosta no anel).
    fn draw_percent(&self, c: &mut Canvas, x: f32, cy: f32, size: f32, max_w: f32) {
        if self.power.is_none() {
            return;
        }
        let text = self.percent_text();
        let size = fit_size(c, &text, size, max_w);
        let nw = c.text_width(&text, NUM, size);
        let x0 = super::optical_start(x, nw, SIGN_GAP + c.text_width("%", NUM, size * PCT));
        let base = cy + DIGIT_H * size / 2.0;
        let (before, t) = self.rolled(|r| &r.percent);
        draw_rolling(c, x0, base, &text, before, t, NUM, size, FG, Anchor::LS, None);
        c.text(x0 + nw + SIGN_GAP, base - 0.1 * size, "%", NUM, size * PCT, FG.alpha(0.7), Anchor::LS, None);
    }

    /// Um texto de antes da última mudança (`pick` escolhe qual) e quanto do rolar já foi
    /// (0 a 1); (None, 1) fora da animação.
    fn rolled(&self, pick: impl Fn(&Rolled) -> &String) -> (Option<&str>, f32) {
        match self.roll.as_ref().filter(|r| ms_since(r.at) < ROLL_MS) {
            Some(r) => (Some(pick(r).as_str()), ease_out(ms_since(r.at) / ROLL_MS)),
            None => (None, 1.0),
        }
    }

    /// A cor da linha embaixo do anel: verde na tomada, vermelha com a bateria baixa.
    fn status_color(&self) -> Rgba {
        match &self.power {
            Some(p) if p.plugged => CHARGE_TEXT,
            Some(p) if p.percent < 15.0 => LOW_TEXT,
            _ => STATUS,
        }
    }

    /// O que dizer embaixo do anel do grande: quanto falta para encher ("cheia em 1 h 10") ou
    /// para acabar.
    fn under_line(&self) -> String {
        match self.plug_line() {
            s if s.is_empty() => self.status_line(),
            s => s,
        }
    }

    /// A linha de baixo do anel, rolando: a do médio (`status_line`) ou a do grande (`under_line`).
    fn draw_status(&self, c: &mut Canvas, x: f32, y: f32, size: f32, max_w: f32, large: bool) {
        let (text, (before, t)) = if large { (self.under_line(), self.rolled(|r| &r.under)) } else { (self.status_line(), self.rolled(|r| &r.status)) };
        draw_rolling(c, x, y, &text, before, t, MEDIUM, size, self.status_color(), Anchor::MA, Some(max_w));
    }

    /// Quanto tempo desde que um aparelho chegou ou saiu da lista: (chegando, milissegundos).
    fn change_of(&self, name: &str) -> Option<(bool, f32)> {
        self.changes.iter().find(|c| c.name == name && ms_since(c.since) < DEVICE_MS).map(|c| (c.arriving, ms_since(c.since)))
    }

    /// Uma linha da lista: o ícone, o nome, a porcentagem e a barra de carga, de `x` a `x + w`,
    /// a partir de `top`. `Row::Tile`: o ícone num quadradinho de vidro e a barra embaixo do nome
    /// e da porcentagem, de ponta a ponta; `Row::Inline`: o ícone solto, e a porcentagem numa
    /// coluna à direita.
    fn draw_row(&self, c: &mut Canvas, d: &Device, x: f32, top: f32, w: f32, style: Row) {
        let change = self.change_of(&d.name).map(|(a, ms)| (a, ease_out(ms / ROW_MS)));
        let (mut alpha, mut fill, mut dx) = (if d.connected { 1.0 } else { 0.4 }, 1.0, 0.0);
        match change {
            Some((true, t)) => {
                alpha = t;
                fill = t;
                dx = 10.0 * (1.0 - t);
            }
            Some((false, t)) => {
                alpha = 1.0 - 0.6 * t;
                fill = 1.0 - t;
            }
            None if !d.connected => fill = 0.0,
            None => {}
        }
        let x = x + dx;
        let shows = d.connected || matches!(change, Some((false, _)));
        let level = d.level.filter(|_| shows);
        let low = d.connected && d.level.is_some_and(|l| l < LOW_AT);
        let icon_col = if low { LOW } else { ICON };
        // (desconectado escrito: um traço sozinho parecia um erro)
        let value = match (d.connected, d.level) {
            (true, Some(l)) => format!("{l}%"),
            (false, _) => "desconectado".into(),
            _ => "—".into(),
        };
        c.push_opacity(alpha);
        // (onde ficam o nome e a barra, e o tamanho do texto)
        let (nx, name_y, bar_y, size, bar_end) = match style {
            Row::Tile(t) => {
                let (radius, icon, stroke) = if t >= 24.0 { (8.0, 15.0, 1.7) } else { (7.0, 13.0, 1.8) };
                c.fill_round_rect(x, top, x + t, top + t, radius, TILE);
                c.stroke_round_rect(x, top, x + t, top + t, radius, TILE_LINE, 1.0);
                vector::icon(c, icon_of(d.kind), x + t / 2.0, top + t / 2.0, icon, icon_col, stroke * 24.0 / icon);
                let big = t >= 24.0;
                (x + t + 10.0, top + if big { 1.0 } else { 0.0 }, top + if big { 20.0 } else { 18.0 }, if big { 11.5 } else { 12.0 }, x + w)
            }
            Row::Inline => {
                vector::icon(c, icon_of(d.kind), x + 8.0, top + 7.0, 16.0, icon_col, 1.6);
                (x + 26.0, top, top + 18.0, 12.0, x + w - 50.0)
            }
        };
        let vw = if d.connected {
            c.text(x + w, name_y, &value, VALUE, size, if low { LOW } else { FG }, Anchor::RA, None)
        } else if c.text_width(&d.name, TEXT, size) + 8.0 + c.text_width(&value, TEXT, size - 1.0) <= x + w - nx {
            c.text(x + w, name_y + 1.0, &value, TEXT, size - 1.0, SUB, Anchor::RA, None)
        } else {
            0.0 // sem espaço (no médio): a linha apagada já diz, e o nome fica inteiro
        };
        let name_end = if matches!(style, Row::Tile(_)) { x + w - vw - 8.0 } else { x + w - 50.0 };
        let name = short_name(c, &d.name, size, name_end - nx);
        c.text(nx, name_y, &name, TEXT, size, FG, Anchor::LA, Some(name_end - nx));
        c.fill_round_rect(nx, bar_y, bar_end, bar_y + 3.0, 1.5, TRACK);
        if let Some(l) = level {
            let frac = (l as f32).max(3.0) / 100.0 * fill;
            if frac > 0.0 {
                c.fill_round_rect(nx, bar_y, nx + ((bar_end - nx) * frac).max(3.0), bar_y + 3.0, 1.5, level_color(l as f32));
            }
        }
        c.pop_clip();
    }

    /// A lista de aparelhos, uma linha a cada `gap`, até caber `max`.
    fn draw_rows(&self, c: &mut Canvas, x: f32, y: f32, w: f32, gap: f32, max: usize, style: Row) {
        for (i, d) in self.devices.iter().take(max).enumerate() {
            self.draw_row(c, d, x, y + i as f32 * gap, w, style);
        }
    }
    /// Nenhum aparelho para mostrar: o ícone e duas linhas, centradas em `x`.
    fn draw_no_devices(&self, c: &mut Canvas, x: f32, y: f32, w: f32) {
        vector::icon(c, vector::DEVICE, x, y, 20.0, MUTED, 1.6);
        c.text(x, y + 18.0, "Nenhum dispositivo", BOLD, 12.0, FG, Anchor::MA, Some(w));
        for (i, line) in c.wrap("Fones, mouses e teclados Bluetooth aparecem aqui", TEXT, 10.5, w, 2).iter().enumerate() {
            c.text(x, y + 36.0 + 14.0 * i as f32, line, TEXT, 10.5, SUB, Anchor::MA, None);
        }
    }

    /// Os quatro lugares do pequeno: o notebook primeiro, os aparelhos conectados no lugar onde
    /// entraram (um que chega ocupa o primeiro lugar vazio; um que sai deixa o lugar vazio, e os
    /// outros não mudam de lugar) e anéis vazios no resto.
    fn small_cells(&self) -> Vec<Spot<'_>> {
        let shows = |d: &Device| d.connected || matches!(self.change_of(&d.name), Some((false, _)));
        let mut cells: Vec<Spot> = Vec::new();
        if self.power.is_some() {
            cells.push(Spot::Laptop);
        }
        // cada aparelho no lugar que ele já tinha (para ninguém trocar de lugar quando outro sai)
        for slot in &self.slots {
            let device = slot.as_ref().and_then(|name| self.devices.iter().find(|d| &d.name == name && shows(d)));
            cells.push(device.map_or(Spot::Empty, Spot::Dev));
        }
        for d in self.devices.iter().filter(|d| shows(d) && !self.slots.iter().flatten().any(|n| n == &d.name)) {
            match cells.iter().position(|c| matches!(c, Spot::Empty)) {
                Some(i) => cells[i] = Spot::Dev(d),
                None => cells.push(Spot::Dev(d)),
            }
        }
        cells.truncate(4);
        cells.resize(4, Spot::Empty);
        cells
    }

    /// O centro do anel do lugar `i` (0 a 3) do pequeno: os da esquerda e os de cima encostam
    /// nas margens; os de baixo ficam tão no pé que a carga deles termina na margem de baixo.
    fn cell_center(i: usize) -> (f32, f32) {
        let x = if i % 2 == 0 { CELL_X + MINI_OUT } else { SMALL - CELL_X - MINI_OUT };
        let y = if i < 2 { CELL_X + MINI_OUT } else { SMALL - CELL_X - DIGIT_H * MINI_LABEL_SIZE - MINI_LABEL_GAP - MINI_OUT };
        (x, y)
    }

    /// A linha de base da carga embaixo do anel com centro em `cy`, no pequeno.
    fn cell_label_base(cy: f32) -> f32 {
        cy + MINI_OUT + MINI_LABEL_GAP + DIGIT_H * MINI_LABEL_SIZE
    }

    fn draw_small(&self, c: &mut Canvas) {
        for (i, cell) in self.small_cells().into_iter().enumerate() {
            let (cx, cy) = Battery::cell_center(i);
            let label_y = Battery::cell_label_base(cy);
            match cell {
                Spot::Empty => c.arc(cx, cy, MINI_R, START, START + SWEEP, EMPTY_TRACK, MINI_W),
                Spot::Laptop => {
                    let percent = self.power.as_ref().map_or(0.0, |p| p.percent);
                    let low = percent < LOW_AT as f32;
                    c.arc(cx, cy, MINI_R, START, START + SWEEP, MINI_TRACK, MINI_W);
                    let level = self.shown();
                    if level > 0.05 {
                        c.arc(cx, cy, MINI_R, START, START + SWEEP * level / 100.0, level_color(percent), MINI_W);
                    }
                    match self.badge() {
                        Some((shape, scale, alpha)) => vector::icon(c, shape, cx, cy, MINI_ICON_SIZE * scale, OK.alpha(alpha), if shape == vector::CHECK { 2.6 } else { 0.0 }),
                        None => vector::icon(c, vector::LAPTOP, cx, cy, MINI_ICON_SIZE, if low { LOW } else { MINI_ICON }, 2.4),
                    }
                    // a do notebook rola como a porcentagem grande
                    let (before, roll) = self.rolled(|r| &r.percent);
                    let before = before.map(|b| format!("{b}%"));
                    draw_rolling(c, cx, label_y, &format!("{}%", percent.round() as i32), before.as_deref(), roll, VALUE, MINI_LABEL_SIZE, if low { LOW } else { FG }, Anchor::MS, None);
                }
                Spot::Dev(d) => self.draw_cell(c, d, cx, cy, label_y),
            }
        }
    }

    /// O anel de um aparelho no pequeno, com a chegada (o ícone entra com um "pop", uma onda sai
    /// do anel, o arco enche do zero e o número sobe) ou a saída (o caminho de volta, até sobrar o
    /// anel vazio).
    fn draw_cell(&self, c: &mut Canvas, d: &Device, cx: f32, cy: f32, label_y: f32) {
        let level = d.level.map_or(0.0, |l| l as f32);
        let color = level_color(level);
        let low = d.level.is_some_and(|l| l < LOW_AT);
        // (o ícone: escala e opacidade; o arco: quanto da carga; o número: opacidade e deslocamento)
        let (mut icon_k, mut icon_a, mut arc, mut num_a, mut num_dy) = (1.0, 1.0, 1.0, 1.0, 0.0);
        match self.change_of(&d.name) {
            Some((true, ms)) => {
                (icon_k, icon_a) = connect_icon(ms);
                arc = ease_out((ms - 140.0) / 760.0);
                // o número sobe e acende depois do ícone
                let number_in = ease_out((ms - 300.0) / 600.0);
                (num_a, num_dy) = (number_in, 4.0 * (1.0 - number_in));
                // uma onda sai do anel
                if ms < RIPPLE_CELL_MS {
                    let wave = ease_out(ms / RIPPLE_CELL_MS);
                    // (cresce até um terço a mais: com os anéis nas margens, não passa da borda)
                    c.stroke_circle(cx, cy, MINI_R * (1.0 + 0.3 * wave), color.alpha(0.45 * (1.0 - wave)), 2.0 * (1.0 + 0.5 * wave));
                }
            }
            Some((false, ms)) => {
                // saindo: o número desce e apaga, o arco esvazia, o ícone encolhe e some
                let number_out = (ms / 160.0).clamp(0.0, 1.0);
                (num_a, num_dy) = (1.0 - number_out, 2.0 * number_out);
                arc = 1.0 - ease_in_out((ms / 320.0).clamp(0.0, 1.0));
                let icon_out = (ms / 240.0).clamp(0.0, 1.0);
                (icon_k, icon_a) = (1.0 - 0.3 * icon_out, 1.0 - icon_out);
            }
            None => {}
        }
        c.arc(cx, cy, MINI_R, START, START + SWEEP, MINI_TRACK, MINI_W);
        if d.level.is_some() && level * arc > 0.05 {
            c.arc(cx, cy, MINI_R, START, START + SWEEP * level * arc / 100.0, color, MINI_W);
        }
        if icon_a > 0.01 {
            vector::icon(c, icon_of(d.kind), cx, cy, MINI_ICON_SIZE * icon_k, fade(if low { LOW } else { MINI_ICON }, icon_a), 2.4);
        }
        if num_a > 0.01 {
            let label = d.level.map_or("—".into(), |l| format!("{l}%"));
            c.text(cx, label_y + num_dy, &label, VALUE, MINI_LABEL_SIZE, fade(if low { LOW } else { FG }, num_a), Anchor::MS, None);
        }
    }

    /// O fundo, o contorno e a cor do texto das pílulas do estado: cinza na bateria, verde na
    /// tomada e vermelha com a bateria baixa (`chip`: a do título do grande, um pouco mais clara).
    fn tint(&self, chip: bool) -> (Rgba, Rgba, Rgba) {
        match &self.power {
            Some(p) if p.plugged => (OK.alpha(0.14), OK.alpha(0.22), CHARGE_TEXT),
            Some(p) if p.percent < 15.0 => (LOW.alpha(0.14), LOW.alpha(0.24), LOW_TEXT),
            _ => (Rgba(255, 255, 255, 18), Rgba(255, 255, 255, 15), if chip { Rgba(255, 255, 255, 209) } else { STATUS }),
        }
    }

    /// O estado, na pílula do título do grande.
    fn chip_text(&self) -> &'static str {
        match &self.power {
            Some(p) if p.charging => "Carregando",
            Some(p) if p.plugged && p.percent >= 99.0 => "Carga completa",
            Some(p) if p.plugged => "Na tomada",
            Some(p) if p.percent < 15.0 => "Bateria baixa",
            _ => "Na bateria",
        }
    }

    /// A linha embaixo do título do grande: desde quando o notebook está fora da tomada ou nela
    /// ("tirado da tomada às 14:10", "na tomada desde 13:05"). Quando acaba ou enche já está
    /// embaixo do anel; repetir aqui ("acaba às 17:40" e "3 h 20 restantes") dizia a mesma coisa
    /// duas vezes. Sem o histórico para saber: quando acaba ou enche (sem bateria, o que está
    /// conectado).
    fn top_line(&self) -> String {
        let Some(p) = &self.power else { return self.header_line() };
        if let Some(since) = self.since_plug_change(p.plugged) {
            let at = win::clock_at(since);
            return if p.plugged { format!("na tomada desde {at}") } else { format!("tirado da tomada às {at}") };
        }
        if !p.plugged {
            return self.secs_left().map_or("na bateria".into(), |s| format!("acaba às {}", win::clock_at(self.now() + s)));
        }
        match (p.charging, p.secs_to_full) {
            (true, Some(s)) => format!("cheia às {}", win::clock_at(self.now() + s as f64)),
            (true, None) => "carregando".into(),
            _ if p.percent >= 99.0 => "na tomada".into(),
            _ => "sem carregar".into(),
        }
    }

    /// Quando o notebook entrou na tomada (`plugged`) ou saiu dela, pelo histórico: o começo da
    /// última sequência de amostras nesse estado. None: o histórico não sabe (está vazio, mudou
    /// agora há pouco ou ficou assim o tempo todo).
    fn since_plug_change(&self, plugged: bool) -> Option<f64> {
        let last = self.hist.last()?;
        if last.2 != plugged {
            return None;
        }
        let changed = self.hist.iter().rposition(|sample| sample.2 != plugged)?;
        Some(self.hist[changed + 1].0)
    }

    /// Uma pílula de altura `h` centrada em `cx` (ou terminando em `right`), com o texto (e, se
    /// houver, o raio na frente): devolve a largura.
    #[allow(clippy::too_many_arguments)]
    fn draw_pill(&self, c: &mut Canvas, cx: Option<f32>, right: f32, cy: f32, h: f32, text: &str, before: Option<&str>, t: f32, chip: bool) -> f32 {
        let (bg, line, col) = self.tint(chip);
        let size = 11.0;
        let pad = if chip { 9.0 } else { 10.0 };
        let badge = if chip { None } else { self.badge() };
        // (no título, um pontinho na cor da carga na frente)
        let lead = if chip || badge.is_some() { if chip { 12.0 } else { 15.0 } } else { 0.0 };
        let w = pad * 2.0 + lead + c.text_width(text, MEDIUM, size);
        let x0 = cx.map_or(right - w, |cx| cx - w / 2.0);
        c.fill_round_rect(x0, cy - h / 2.0, x0 + w, cy + h / 2.0, h / 2.0, bg);
        c.stroke_round_rect(x0, cy - h / 2.0, x0 + w, cy + h / 2.0, h / 2.0, line, 1.0);
        if chip {
            c.fill_circle(x0 + pad + 3.0, cy, 3.0, level_color(self.power.as_ref().map_or(100.0, |p| p.percent)));
        } else if let Some((d, scale, alpha)) = badge {
            let stroke = if d == vector::CHECK { 3.0 } else { 0.0 };
            vector::icon(c, d, x0 + pad + 5.0, cy, 10.0 * scale, fade(CHARGE_TEXT, alpha), stroke);
        }
        draw_rolling(c, x0 + pad + lead, cy, text, before, t, MEDIUM, size, col, Anchor::LM, None);
        w
    }

    fn draw_medium(&self, c: &mut Canvas) {
        if self.power.is_none() {
            // um PC de mesa: só os aparelhos
            c.text(INSET, 16.0, "Dispositivos", BOLD, 12.5, FG, Anchor::LA, None);
            let n = self.devices.iter().filter(|d| d.connected).count();
            let count = match n {
                0 => String::new(),
                1 => "1 conectado".into(),
                n => format!("{n} conectados"),
            };
            c.text(WIDE - INSET, 17.5, &count, TEXT, 10.5, SUB, Anchor::RA, None);
            if self.devices.is_empty() {
                self.draw_no_devices(c, WIDE / 2.0, 66.0, WIDE - 2.0 * INSET);
            } else {
                self.draw_rows(c, INSET, 44.0, WIDE - 2.0 * INSET, 25.0, 4, Row::Inline);
            }
            return;
        }
        // o anel só com o número; o tempo numa pílula embaixo (com o raio, carregando)
        self.draw_ring(c, RING_M);
        self.draw_percent(c, RING_M.cx, RING_M.cy, PERCENT_M, inner_width(RING_M));
        let (before, t) = self.rolled(|r| &r.status);
        self.draw_pill(c, Some(RING_M.cx), 0.0, 122.0, 24.0, &self.status_line(), before, t, false);
        c.fill_round_rect(134.0, 22.0, 135.0, 132.0, 0.0, RULE);
        if self.devices.is_empty() {
            self.draw_no_devices(c, 225.0, 44.0, 150.0);
        } else {
            self.draw_rows(c, 148.0, 15.0, WIDE - INSET - 148.0, 32.0, 4, Row::Tile(26.0));
        }
    }

    fn draw_large(&self, c: &mut Canvas) {
        // o rótulo no mesmo lugar e na mesma letra dos outros widgets, na cor da bateria
        super::draw_label(c, INSET, super::LABEL_Y, if self.power.is_some() { "Bateria" } else { "Dispositivos" });
        let (before, t) = self.rolled(|r| &r.header);
        let Some(p) = self.power.clone() else {
            draw_rolling(c, INSET, TOP_LINE_Y, &self.top_line(), before, t, TEXT, 11.5, SUB, Anchor::LA, Some(WIDE - 2.0 * INSET));
            if self.devices.is_empty() {
                self.draw_no_devices(c, WIDE / 2.0, 200.0, WIDE - 2.0 * INSET);
            } else {
                self.draw_rows(c, INSET, 70.0, WIDE - 2.0 * INSET, 26.0, 14, Row::Tile(22.0));
            }
            return;
        };
        // o estado numa pílula à direita do título; embaixo, quando acaba ou enche
        let chip_w = self.draw_pill(c, None, WIDE - INSET, 27.0, 22.0, self.chip_text(), None, 1.0, true);
        draw_rolling(c, INSET, TOP_LINE_Y, &self.top_line(), before, t, TEXT, 11.5, SUB, Anchor::LA, Some(WIDE - 2.0 * INSET - chip_w));
        // o anel: o número no centro e o ícone na abertura de baixo
        self.draw_ring(c, RING_G);
        self.draw_percent(c, RING_G.cx, RING_G.cy, PERCENT_G, inner_width(RING_G));
        let (ox, oy) = g_opening(RING_G);
        match self.badge() {
            Some((d, scale, alpha)) => vector::icon(c, d, ox, oy, 16.0 * scale, OK.alpha(alpha), if d == vector::CHECK { 2.6 } else { 0.0 }),
            None => vector::icon(c, vector::LAPTOP, ox, oy, 16.0, if p.percent < 15.0 { LOW_TEXT } else { Rgba(255, 255, 255, 204) }, 2.4),
        }
        self.draw_status(c, RING_G.cx, 176.0, 11.5, 140.0, true);
        // ao lado do anel: saúde, ciclos e o dia
        let (today, longest) = self.day_stats();
        let stats = [
            ("Saúde", p.health.map_or("—".into(), |h| format!("{}%", (h * 100.0).round()))),
            ("Ciclos", p.cycles.map_or("—".into(), |n| n.to_string())),
            ("Na tomada hoje", today.map_or("—".into(), duration)),
            ("Maior tempo fora", longest.map_or("—".into(), duration)),
        ];
        let (sx0, sx1) = (160.0, WIDE - INSET);
        for (i, (k, v)) in stats.iter().enumerate() {
            let top = 68.0 + i as f32 * 30.0;
            c.text(sx0, top + 15.0, k, TEXT, 11.5, SUB, Anchor::LM, None);
            c.text(sx1, top + 15.0, v, VALUE, 13.0, FG, Anchor::RM, None);
            if i < 3 {
                c.fill_round_rect(sx0, top + 29.0, sx1, top + 30.0, 0.0, RULE);
            }
        }
        // o gráfico num painel de vidro mais claro
        c.fill_round_rect(10.0, 196.0, WIDE - 10.0, 300.0, 18.0, PANEL);
        c.stroke_round_rect(10.0, 196.0, WIDE - 10.0, 300.0, 18.0, PANEL_LINE, 1.0);
        c.text(20.0, 214.0, "Últimas 24 horas", BOLD, 12.0, FG, Anchor::LM, None);
        let lw = c.text(WIDE - 20.0, 214.0, "na tomada", TEXT, 10.5, LEGEND, Anchor::RM, None);
        let lx = WIDE - 20.0 - lw - 6.0 - 9.0;
        c.fill_round_rect(lx, 209.5, lx + 9.0, 218.5, 3.0, OK.alpha(0.30));
        c.stroke_round_rect(lx, 209.5, lx + 9.0, 218.5, 3.0, OK.alpha(0.55), 1.0);
        self.draw_chart(c, 20.0, 230.0, WIDE - 20.0, 270.0);
        if self.devices.is_empty() {
            self.draw_no_devices(c, WIDE / 2.0, G_ROWS_Y + 22.0, WIDE - 2.0 * INSET);
        } else {
            self.draw_rows(c, INSET, G_ROWS_Y, WIDE - 2.0 * INSET, 26.0, 4, Row::Tile(22.0));
        }
    }

    /// O gráfico das últimas 24 horas: a carga numa curva suave, com a área embaixo, as faixas em
    /// que esteve na tomada e o ponto de agora; ao abrir, a linha se desenha da esquerda para a
    /// direita.
    fn draw_chart(&self, c: &mut Canvas, x0: f32, y0: f32, x1: f32, y1: f32) {
        let now = self.now();
        let (w, h) = (x1 - x0, y1 - y0);
        for y in [y0, (y0 + y1) / 2.0 - 0.5, y1 - 1.0] {
            c.fill_round_rect(x0, y, x1, y + 1.0, 0.0, GRID);
        }
        // as horas embaixo, de 6 em 6 (a última é "agora")
        let local = win::local_time();
        let hour_now = local.hour as f64 + local.minute as f64 / 60.0;
        for mark in 0..5 {
            let label = if mark == 4 { "agora".to_string() } else { format!("{}h", ((hour_now - 24.0 + 6.0 * mark as f64).rem_euclid(24.0)).floor() as i32) };
            let anchor = match mark {
                0 => Anchor::LA,
                4 => Anchor::RA,
                _ => Anchor::MA,
            };
            c.text(x0 + w * mark as f32 / 4.0, y1 + 6.0, &label, TEXT, 9.5, FAINT_TEXT, anchor, None);
        }
        // (3 pt de folga em cima e embaixo: o ponto de 100% não encosta na borda)
        let pts: Vec<(f32, f32, bool)> = self
            .hist
            .iter()
            .filter(|s| s.0 >= now - DAY)
            .map(|s| (x0 + w * (1.0 - ((now - s.0) / DAY) as f32), y0 + 3.0 + (h - 6.0) * (1.0 - s.1.clamp(0.0, 100.0) / 100.0), s.2))
            .collect();
        if pts.len() < 2 {
            c.text((x0 + x1) / 2.0, (y0 + y1) / 2.0, "O gráfico enche com o uso, de 5 em 5 minutos", TEXT, 10.5, SUB, Anchor::MM, Some(w));
            return;
        }
        // as faixas na tomada: cada sequência de pontos na tomada vira uma faixa verde
        let mut index = 0;
        while index < pts.len() {
            if pts[index].2 {
                let start = pts[index].0;
                while index < pts.len() && pts[index].2 {
                    index += 1;
                }
                let end = pts.get(index).map_or(pts[pts.len() - 1].0, |p| p.0).max(start + 2.0);
                c.fill_round_rect(start, y0, end, y1, 5.0f32.min((end - start) / 2.0), OK.alpha(0.11));
            }
            index += 1;
        }
        // a linha se desenhando (ao abrir o grande)
        let smooth = catmull_rom(&pts.iter().map(|p| (p.0, p.1)).collect::<Vec<_>>());
        let reveal = self.reveal.get().map_or(1.0, |t| ease_in_out((ms_since(t) / REVEAL_MS).min(1.0)));
        let last = smooth[smooth.len() - 1];
        let cut = x0 + (last.0 - x0) * reveal;
        let mut line: Vec<(f32, f32)> = Vec::new();
        // a linha até o ponto do corte (o último pedaço, cortado no meio)
        for pair in smooth.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            if from.0 > cut {
                break;
            }
            line.push(from);
            if to.0 > cut {
                let frac = (cut - from.0) / (to.0 - from.0).max(0.001);
                line.push((cut, from.1 + (to.1 - from.1) * frac));
            }
        }
        if reveal >= 1.0 {
            line.push(last);
        }
        if line.len() >= 2 {
            let area_alpha = ((reveal - 0.35) / 0.65).clamp(0.0, 1.0);
            let mut area = line.clone();
            area.push((line[line.len() - 1].0, y1));
            area.push((line[0].0, y1));
            c.polygon_with(&area, Fill::VGrad(y0, y1, OK.alpha(0.30 * area_alpha), OK.alpha(0.0)));
            c.line(&line, OK, 1.8);
        }
        // no fim do desenho, o ponto de agora acende
        if reveal >= 0.85 {
            let dot = ((reveal - 0.85) / 0.15).min(1.0);
            let col = level_color(self.power.as_ref().map_or(100.0, |p| p.percent));
            c.fill_circle(last.0, last.1, 5.5, col.alpha(0.30 * dot));
            c.fill_circle(last.0, last.1, 2.6, WHITE.alpha(dot));
        }
    }

    /// O brilho correndo no arco enquanto carrega: onde está (0 a 1 do caminho até a carga) e
    /// a força dele; None: entre um e outro.
    fn shimmer(&self) -> Option<(f32, f32)> {
        if !self.charging() || self.plug_at.is_some_and(|t| ms_since(t) < RIPPLE_MS) {
            return None;
        }
        // em que parte do ciclo estamos (0 a 1); o brilho só passa num trecho dele
        let cycle = ((ms_since(self.charge_since) - 700.0).rem_euclid(SHIMMER_EVERY)) / SHIMMER_EVERY;
        let (on, off) = SHIMMER_ON;
        if !(on..=off).contains(&cycle) {
            return None;
        }
        let progress = (cycle - on) / (off - on);
        // acende no começo e apaga no fim
        let fade = (progress / 0.12).min((1.0 - progress) / 0.15).clamp(0.0, 1.0);
        Some((ease_in_out(progress), fade))
    }

    /// O anel deste tamanho (None: sem o anel do notebook).
    fn ring_of(&self, mode: Mode) -> Option<Ring> {
        self.power.as_ref()?;
        match mode {
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

    /// Os textos de agora: a porcentagem, a linha de baixo do anel, a de cima e a de baixo do
    /// anel do grande.
    fn current_texts(&self) -> Texts {
        (self.percent_text(), self.status_line(), self.top_line(), self.under_line())
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
            if o.plugged != n.plugged {
                // na ilha por alguns segundos: "Carregando · 64%" ou "Na bateria · 64%"
                self.plug_event = Some((n.plugged, Instant::now()));
                self.plug_count += 1;
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
        let (percent, status, header, under) = before;
        if had && changed {
            self.roll = Some(Rolled { percent, status, header, under, at: Instant::now() });
        }
        self.texts = Some(now);
        changed || !looks_same
    }

    /// O aparelho `d`, que acabou de conectar, ganha o aviso na ilha? Não logo depois de abrir o
    /// app (aí todos "chegam"), nem quando ele só piscou a conexão: precisa ter ficado longe por
    /// `AWAY_FOR` (`AWAY_FOR_INPUT` para teclado, mouse e caneta, que cochilam sozinhos). Um que
    /// nunca esteve conectado desde que o app abriu conta como longe.
    fn announces(&self, d: &Device, now: Instant) -> bool {
        if now.saturating_duration_since(self.born) < ARRIVAL_QUIET_START {
            return false;
        }
        let away_for = if matches!(d.kind, DevKind::Keyboard | DevKind::Mouse | DevKind::Pen) { AWAY_FOR_INPUT } else { AWAY_FOR };
        self.away_since.get(&d.name).is_none_or(|since| now.saturating_duration_since(*since) >= away_for)
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
        let mut arrived = None;
        for d in &list {
            if d.connected && !was(&d.name) {
                changes.push(Change { name: d.name.clone(), arriving: true, since: now });
                if self.announces(d, now) {
                    arrived = Some(d.clone());
                }
                self.away_since.remove(&d.name);
            }
            if !d.connected && was(&d.name) {
                changes.push(Change { name: d.name.clone(), arriving: false, since: now });
                self.away_since.insert(d.name.clone(), now);
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
        if let Some(d) = arrived {
            self.arrivals += 1;
            self.arrival = Some(Arrival { name: d.name, kind: d.kind, level: d.level, at: now, count: self.arrivals });
        }
        self.changes = changes;
        self.devices = list;
        // os lugares do pequeno ficam como estão (quem chegou, no primeiro vazio)
        let mut slots: Vec<Option<String>> = self
            .small_cells()
            .into_iter()
            .filter(|c| !matches!(c, Spot::Laptop))
            .map(|c| match c {
                Spot::Dev(d) => Some(d.name.clone()),
                _ => None,
            })
            .collect();
        // (um lugar vazio guarda o nome de quem estava lá, para ele voltar ao mesmo lugar)
        for (i, s) in slots.iter_mut().enumerate() {
            if s.is_none() {
                *s = self.slots.get(i).cloned().flatten();
            }
        }
        while slots.last().is_some_and(|s| s.as_ref().is_none_or(|n| !self.devices.iter().any(|d| &d.name == n && d.connected))) {
            slots.pop();
        }
        self.slots = slots;
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
fn inner_width(ring: Ring) -> f32 {
    2.0 * (ring.r - ring.w / 2.0) * 0.68
}

/// O nome de um aparelho que cabe em `max_w`: sem o código do modelo no fim quando não cabe
/// ("Mouse MX Master 3S" vira "Mouse MX Master"), em vez de cortado no meio ("Mouse MX Mast…").
/// Só tira palavras com números, e deixa pelo menos duas ("Fone WH-1000XM5" fica e é cortado).
fn short_name(c: &Canvas, name: &str, size: f32, max_w: f32) -> String {
    let mut words: Vec<&str> = name.split_whitespace().collect();
    while c.text_width(&words.join(" "), TEXT, size) > max_w && words.len() > 2 && words.last().is_some_and(|w| w.chars().any(|ch| ch.is_ascii_digit())) {
        words.pop();
    }
    words.join(" ")
}

/// O tamanho da porcentagem (`text` e o "%" menor) para caber em `max_w`, até `size`.
fn fit_size(c: &Canvas, text: &str, size: f32, max_w: f32) -> f32 {
    // (a largura do texto não diminui na mesma proporção da fonte: algumas rodadas até caber)
    let mut fitted = size;
    for _ in 0..6 {
        let width = c.text_width(text, NUM, fitted) + SIGN_GAP + c.text_width("%", NUM, fitted * PCT);
        if width <= max_w {
            break;
        }
        fitted *= max_w / width * 0.995;
    }
    fitted
}

/// O tom apagado dos rótulos do gráfico.
const FAINT_TEXT: Rgba = Rgba(255, 255, 255, 115);

/// O ícone de cada tipo de aparelho.
fn icon_of(kind: DevKind) -> &'static str {
    match kind {
        DevKind::Headphones => vector::HEADPHONES,
        DevKind::Speaker => vector::SPEAKER,
        DevKind::Mouse => vector::MOUSE,
        DevKind::Keyboard => vector::KEYBOARD,
        DevKind::Gamepad => vector::GAMEPAD,
        DevKind::Phone => vector::PHONE,
        DevKind::Pen => vector::PEN,
        DevKind::Other => vector::DEVICE,
    }
}

/// Os textos que rolam: a porcentagem, a linha de baixo do anel, a de cima e a de baixo do anel
/// do grande.
type Texts = (String, String, String, String);

/// "3 h 20", "45 min".
fn duration(secs: f64) -> String {
    let minutes = (secs / 60.0).round() as i64;
    if minutes < 60 { format!("{minutes} min") } else { format!("{} h {:02}", minutes / 60, minutes % 60) }
}

/// Um dia de exemplo para o gráfico: descarregando, duas idas à tomada, terminando em `end`.
fn sample_history(now: f64, end: f32) -> Vec<(f64, f32, bool)> {
    // uma amostra a cada 5 minutos: 288 no dia
    let samples = 288;
    let mut out = Vec::with_capacity(samples + 1);
    let mut level: f32 = 96.0;
    let plugged = |day_frac: f32| (0.12..0.24).contains(&day_frac) || (0.58..0.70).contains(&day_frac);
    for i in 0..=samples {
        let day_frac = i as f32 / samples as f32;
        let on = plugged(day_frac);
        level += if on { 1.15 } else { -(0.38 + 0.27 * (i as f32 * 0.09).sin().powi(2)) };
        level = level.clamp(8.0, 100.0);
        out.push((now - DAY + DAY * day_frac as f64, level, on));
    }
    // termina na carga de agora, sem salto: a diferença entra aos poucos ao longo do dia
    let diff = end - out[samples].1;
    for (i, sample) in out.iter_mut().enumerate() {
        sample.1 = (sample.1 + diff * i as f32 / samples as f32).clamp(5.0, 100.0);
    }
    out
}

impl Kind for Battery {
    /// Os vermelhos e âmbares da carga baixa (o verde de sempre segue o papel de parede).
    fn alert_hues(&self) -> Option<(f32, f32)> {
        Some(ALERT_HUES)
    }
    /// Na ilha quando o notebook (fora da tomada) ou um aparelho conectado fica abaixo de 20%;
    /// por alguns segundos, quando um aparelho conecta ou o notebook entra ou sai da tomada.
    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        if let Some(low) = self.island_low() {
            let mut item = IslandItem::alert(level_color(low.level), alert_hash(&low.key()));
            item.urgent = low.level < ISLAND_URGENT;
            return Some(item);
        }
        if let Some(arrival) = self.arrival_shown() {
            return Some(IslandItem::alert(arrival_color(arrival), alert_hash(&format!("chegou|{}|{}", arrival.name, arrival.count))));
        }
        let plugged = self.plug_shown()?;
        let col = if plugged { OK } else { level_color(self.power.as_ref()?.percent) };
        Some(IslandItem::alert(col, alert_hash(&format!("tomada|{}", self.plug_count))))
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, _env: &Env) {
        self.draw_island(c, form, r, hot);
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        island_buttons(form, w, h).into_iter().find(|(_, b)| iu::inside(*b, x, y)).map(|(a, _)| a)
    }

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
        if let Some(at) = self.full_at.filter(|&t| ms_since(t) < SHEEN_MS) {
            let layout = Layout::of(mode);
            let progress = ease_in_out(ms_since(at) / SHEEN_MS);
            // uma faixa clara (45% da largura) que passa da esquerda para a direita
            let band_w = layout.w * 0.45;
            let x = -band_w + (layout.w + band_w) * progress;
            c.push_round_clip(0.0, 0.0, layout.w, layout.h, layout.radius);
            c.gradient_h(x, 0.0, x + band_w, layout.h, 0.0, &[(0.0, WHITE.alpha(0.0)), (0.5, WHITE.alpha(0.22)), (1.0, WHITE.alpha(0.0))], x, x + band_w);
            c.pop_clip();
        }
    }

    /// O brilho correndo no arco, carregando.
    fn overlay(&self, c: &mut Canvas, mode: Mode, ox: f32, oy: f32) {
        if let (Some(ring), Some((progress, fade))) = (self.ring_of(mode), self.shimmer()) {
            let percent = self.power.as_ref().map_or(0.0, |p| p.percent);
            // um pedaço de 6% do arco, andando do começo até a carga
            let head = (percent * progress).max(2.0);
            let tail = (head - 6.0).max(0.0);
            c.arc(ring.cx + ox, ring.cy + oy, ring.r, START + SWEEP * tail / 100.0, START + SWEEP * head / 100.0, WHITE.alpha(0.6 * fade), ring.w * 0.55);
        }
    }

    fn overlay_fps(&self) -> u32 {
        if self.shimmer().is_some() { 30 } else { 0 }
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        if !env.shortcuts() {
            return None;
        }
        if let Some(ring) = self.ring_of(mode) {
            if (x - ring.cx).hypot(y - ring.cy) <= ring.r + ring.w {
                return Some(OPEN_POWER);
            }
        }
        if mode == Mode::P {
            // o anel do notebook abre a energia; os outros (e os vazios), o Bluetooth
            let col = ((x - CELL_X) / CELL).floor().clamp(0.0, 1.0) as usize;
            let row = ((y - CELL_X) / CELL).floor().clamp(0.0, 1.0) as usize;
            return Some(if self.power.is_some() && col + row == 0 { OPEN_POWER } else { OPEN_BLUETOOTH });
        }
        if self.devices.is_empty() {
            return None;
        }
        let in_list = match (mode, self.power.is_some()) {
            (Mode::M, true) => x >= 130.0,
            (Mode::M, false) => y >= 40.0,
            (Mode::G, true) => y >= G_ROWS_Y - 8.0,
            _ => y >= 64.0,
        };
        in_list.then_some(OPEN_BLUETOOTH)
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<Hot> {
        match self.hit(x, y, mode, env)? {
            OPEN_POWER if mode == Mode::P => {
                let (cx, cy) = Battery::cell_center(0);
                let reach = MINI_R + MINI_W + 2.0;
                Some((cx - reach, cy - reach, cx + reach, cy + reach, reach))
            }
            OPEN_POWER => {
                let ring = self.ring_of(mode)?;
                let reach = ring.r + ring.w;
                Some((ring.cx - reach, ring.cy - reach, ring.cx + reach, ring.cy + reach, reach))
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
            DISMISS => match (self.island_low(), self.arrival_shown()) {
                (Some(low), _) => self.island_ok.push(low.key()),
                (None, Some(_)) => self.arrival = None,
                (None, None) => self.plug_event = None,
            },
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
        let mut next = self.next_power;
        if self.animating() || self.was_animating {
            next = next.min(Instant::now() + FRAME);
        }
        if self.charging() {
            // o começo e o fim de cada brilho (para ligar e desligar os quadros da camada)
            next = next.min(Instant::now() + Duration::from_millis(100));
        }
        if self.power.is_some() || self.feed.is_some() {
            next = next.min(self.next_check);
        }
        // o aviso da tomada (e o do aparelho que conectou) sai da ilha na hora certa
        if let Some((_, at)) = self.plug_event.filter(|e| e.1.elapsed() < PLUG_SHOW) {
            next = next.min(at + PLUG_SHOW);
        }
        if let Some(arrival) = self.arrival_shown() {
            next = next.min(arrival.at + ARRIVAL_SHOW);
        }
        Some(next)
    }

    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        self.prune_island_ok();
        // durante uma animação, um quadro por tick; e mais um depois que ela acaba: o último
        // desenhado podia ser do meio do caminho (com o tick atrasado, comum na bateria) e ficar
        // na tela, com o arco ou os dígitos pela metade, até o mouse passar e redesenhar
        let animating = self.animating();
        let mut redraw = animating || self.was_animating;
        self.was_animating = animating;
        if self.fixed_now.is_none() {
            let now = Instant::now();
            // entrou ou saiu da tomada: lê tudo já (sem esperar a leitura de 5 em 5 s), para a
            // ilha e a onda do anel aparecerem na hora
            if now >= self.next_check {
                self.next_check = now + CHECK_EVERY;
                if let (Some(p), Some(ac)) = (&self.power, battery::on_ac()) {
                    if p.plugged != ac {
                        self.next_power = now;
                    }
                }
            }
            if self.plug_event.is_some_and(|e| e.1.elapsed() >= PLUG_SHOW) {
                self.plug_event = None;
            }
            if self.arrival.as_ref().is_some_and(|a| a.at.elapsed() >= ARRIVAL_SHOW) {
                self.arrival = None;
            }
            if now >= self.next_power {
                self.next_power = now + POWER_EVERY;
                let reading = self.steady(battery::power());
                redraw |= self.apply_power(reading);
                self.sample_now(ctx);
            }
            // as listas novas de dispositivos (fica a última, se chegaram várias)
            let mut latest = None;
            let mut gone = false;
            if let Some(rx) = &self.feed {
                loop {
                    match rx.try_recv() {
                        Ok(list) => latest = Some(list),
                        Err(mpsc::TryRecvError::Disconnected) => {
                            gone = true;
                            break;
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                    }
                }
            }
            if gone {
                self.feed = None;
            }
            if let Some(list) = latest {
                redraw |= self.apply_devices(list, ctx);
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

    /// Entrar ou sair da tomada põe a carga na ilha por alguns segundos, cada vez como um aviso
    /// novo; um aparelho com pouca bateria passa na frente.
    #[test]
    fn plugging_shows_on_the_island_for_a_while() {
        let env_cfg = json!({"bateria": {}, "app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        let mut b = Battery::sample_alone();
        b.power = Some(Power { percent: 64.0, plugged: false, ..Default::default() });
        assert!(b.island_item(&env).is_none(), "parado na bateria: nada na ilha");
        b.apply_power(Some(Power { percent: 64.0, plugged: true, charging: true, secs_to_full: Some(70 * 60), ..Default::default() }));
        let first = b.island_item(&env).expect("entrou na tomada");
        assert!(first.alert && !first.urgent);
        assert_eq!(b.plug_line(), "cheia em 1 h 10");
        b.apply_power(Some(Power { percent: 64.0, plugged: false, ..Default::default() }));
        let second = b.island_item(&env).expect("saiu da tomada");
        assert_ne!(first.alert_id, second.alert_id, "cada vez é um aviso novo");
        assert_eq!(b.plug_shown(), Some(false));
        // passou o tempo: sai da ilha
        b.plug_event = Some((false, Instant::now() - PLUG_SHOW));
        assert!(b.island_item(&env).is_none());
        // um aparelho quase sem bateria vem antes do carregador
        let mut low = Battery::sample();
        low.plug_event = Some((true, Instant::now()));
        assert_eq!(low.island_item(&env).map(|i| i.alert_id), low.island_low().map(|l| alert_hash(&l.key())));
    }

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

    /// O aparelho que conecta aparece na ilha, menos logo depois de abrir o app e quando ele só
    /// piscou a conexão (teclado e mouse, que cochilam, precisam ficar bem mais tempo longe).
    #[test]
    fn a_device_connecting_shows_on_the_island() {
        let env_cfg = json!({"app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        let mut b = Battery::sample();
        for d in &mut b.devices {
            d.level = d.level.map(|l| l.max(60));
        }
        b.born = Instant::now() - Duration::from_secs(60);
        let with = |b: &Battery, name: &str, connected: bool| -> Vec<Device> {
            let mut list = b.devices.clone();
            list.iter_mut().find(|d| d.name.starts_with(name)).unwrap().connected = connected;
            list
        };
        // o fone sai e volta logo: só piscou, nada na ilha
        let list = with(&b, "Fone", false);
        ctx_run(|ctx| b.apply_devices(list, ctx));
        let list = with(&b, "Fone", true);
        ctx_run(|ctx| b.apply_devices(list, ctx));
        assert!(b.arrival.is_none());
        // longe por 3 min: volta com o aviso
        let list = with(&b, "Fone", false);
        ctx_run(|ctx| b.apply_devices(list, ctx));
        *b.away_since.get_mut("Fone WH-1000XM5").unwrap() -= Duration::from_secs(3 * 60);
        let list = with(&b, "Fone", true);
        ctx_run(|ctx| b.apply_devices(list, ctx));
        let arrival = b.arrival.clone().expect("o fone conectou");
        assert_eq!((arrival.name.as_str(), arrival.level), ("Fone WH-1000XM5", Some(82)));
        let item = b.island_item(&env).expect("na ilha");
        assert!(item.alert && !item.urgent);
        // OK tira o aviso
        ctx_run(|ctx| b.act(DISMISS, ctx));
        assert!(b.arrival.is_none() && b.island_item(&env).is_none());
        // o mouse longe 3 min (cochilou): nada; 31 min (a pessoa voltou): avisa
        let list = with(&b, "Mouse", false);
        ctx_run(|ctx| b.apply_devices(list, ctx));
        *b.away_since.get_mut("Mouse MX Master 3S").unwrap() -= Duration::from_secs(3 * 60);
        let list = with(&b, "Mouse", true);
        ctx_run(|ctx| b.apply_devices(list, ctx));
        assert!(b.arrival.is_none(), "o mouse só cochilou");
        let list = with(&b, "Mouse", false);
        ctx_run(|ctx| b.apply_devices(list, ctx));
        *b.away_since.get_mut("Mouse MX Master 3S").unwrap() -= Duration::from_secs(31 * 60);
        let list = with(&b, "Mouse", true);
        ctx_run(|ctx| b.apply_devices(list, ctx));
        assert_eq!(b.arrival.as_ref().map(|a| a.count), Some(2), "cada chegada é um aviso novo");
        // passado o tempo, sai da ilha sozinho
        b.arrival.as_mut().unwrap().at -= ARRIVAL_SHOW;
        assert!(b.island_item(&env).is_none());
        // logo depois de abrir o app, todos "chegam" de uma vez: nada
        let mut fresh = Battery::empty();
        let list = b.devices.clone();
        ctx_run(|ctx| fresh.apply_devices(list, ctx));
        assert!(fresh.arrival.is_none());
    }

    /// A bateria fraca passa na frente do aparelho que conectou.
    #[test]
    fn low_battery_beats_a_device_connecting() {
        let env_cfg = json!({"app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        let mut b = Battery::sample();
        b.arrival = Some(Arrival { name: "Fone WH-1000XM5".into(), kind: DevKind::Headphones, level: Some(82), at: Instant::now(), count: 1 });
        let item = b.island_item(&env).unwrap();
        assert!(item.alert);
        assert_eq!(b.island_low().map(|l| l.name), Some("Teclado K380".to_string()));
        ctx_run(|ctx| b.act(DISMISS, ctx));
        assert!(b.arrival.is_some(), "o OK foi para a bateria fraca");
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

    /// Os dígitos passando pela virada (2 h 59 → 3 h 00) desenham em todos os tamanhos (a conta
    /// de cada dígito fica nos testes de `odometer`).
    #[test]
    fn odometer_digits_wrap_around() {
        // desenha passando pela virada, em todos os tamanhos
        com();
        let env_cfg = json!({"bateria": {}, "app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        let mut b = Battery::sample();
        b.roll = Some(Rolled { percent: "73".into(), status: "2 h 59 restantes".into(), header: b.header_line(), under: "2 h 59 restantes".into(), at: Instant::now() - Duration::from_millis(300) });
        let mut p = b.power.clone().unwrap();
        p.secs_left = Some(3 * 3600);
        b.power = Some(p);
        for mode in Mode::ALL {
            let l = Layout::of(mode);
            let mut c = Canvas::new(l.w as u32, l.h as u32, 1.0);
            b.draw(&mut c, mode, &env);
        }
    }

    /// Todos os estados nos três tamanhos e o aparelho chegando e saindo no pequeno, quadro a
    /// quadro, num PNG (para conferir de olho com o mockup):
    ///
    ///   cargo test --release widgets::battery::tests::sheet -- --ignored --nocapture
    #[test]
    #[ignore]
    fn sheet() {
        com();
        let env_cfg = json!({"bateria": {}, "app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        let s = 2.0;
        let rows: Vec<Battery> = vec![Battery::sample(), Battery::sample_charging(), Battery::sample_low(), Battery::sample_desktop(), Battery::sample_alone(), Battery::sample_nothing()];
        let (cw, ch) = (WIDE + SMALL + WIDE + 48.0, super::super::LARGE_H + 16.0);
        let frames = [0u64, 120, 250, 500, 900];
        let leaving = [0u64, 120, 240, 400];
        let total_h = ch * rows.len() as f32 + 2.0 * (SMALL + 16.0);
        let mut sheet = Canvas::new((cw * s) as u32, (total_h * s) as u32, s);
        sheet.fill_round_rect(0.0, 0.0, cw, total_h, 0.0, Rgba::rgb(46, 40, 80));
        let card = |b: &Battery, mode: Mode| {
            let l = Layout::of(mode);
            let mut c = Canvas::new((l.w * s) as u32, (l.h * s) as u32, s);
            c.fill_round_rect(0.0, 0.0, l.w, l.h, l.radius, Rgba(28, 24, 48, 235));
            b.draw(&mut c, mode, &env);
            c.finish()
        };
        for (i, b) in rows.iter().enumerate() {
            let y = i as f32 * ch + 8.0;
            sheet.image(&card(b, Mode::G), 8.0, y);
            sheet.image(&card(b, Mode::M), WIDE + 24.0, y);
            sheet.image(&card(b, Mode::P), WIDE + 24.0, y + SMALL + 16.0);
            sheet.image(&card(b, Mode::P), WIDE + 24.0 + SMALL + 16.0, y + SMALL + 16.0);
        }
        let y = rows.len() as f32 * ch + 8.0;
        for (k, ms) in frames.into_iter().enumerate() {
            sheet.image(&card(&Battery::sample_connecting(ms), Mode::P), 8.0 + k as f32 * (SMALL + 12.0), y);
        }
        for (k, ms) in leaving.into_iter().enumerate() {
            let mut b = Battery::sample_connecting(0);
            b.devices[0].connected = false;
            b.changes[0].arriving = false;
            b.changes[0].since = Instant::now() - Duration::from_millis(ms);
            sheet.image(&card(&b, Mode::P), 8.0 + k as f32 * (SMALL + 12.0), y + SMALL + 16.0);
        }
        let path = std::env::temp_dir().join("bateria_folha.png");
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

    /// Os nomes nos quatro lugares do pequeno (None: o notebook; "" : vazio).
    fn cells(b: &Battery) -> Vec<Option<String>> {
        b.small_cells()
            .into_iter()
            .map(|c| match c {
                Spot::Laptop => None,
                Spot::Dev(d) => Some(d.name.clone()),
                Spot::Empty => Some(String::new()),
            })
            .collect()
    }

    /// No pequeno, os lugares que sobram ficam com o anel vazio; um aparelho que sai deixa o lugar
    /// vazio (os outros não andam) e um que chega ocupa o primeiro vazio.
    #[test]
    fn small_spots_stay_put_and_fill_the_first_empty_one() {
        let s = |v: &str| Some(v.to_string());
        let e = || Some(String::new());
        assert_eq!(cells(&Battery::sample_alone()), [None, e(), e(), e()], "só o notebook: três vazios");
        assert_eq!(cells(&Battery::sample_nothing()), [e(), e(), e(), e()], "sem nada: os quatro vazios");
        let mut b = Battery::sample();
        let list = b.devices.clone();
        let _ = ctx_run(|ctx| b.apply_devices(list.clone(), ctx));
        assert_eq!(cells(&b), [None, s("Teclado K380"), s("Mouse MX Master 3S"), s("Fone WH-1000XM5")]);
        // o mouse desconecta: no meio da saída ainda está lá; depois, o lugar fica vazio
        let mut gone = list.clone();
        gone.iter_mut().find(|d| d.name.starts_with("Mouse")).unwrap().connected = false;
        let _ = ctx_run(|ctx| b.apply_devices(gone.clone(), ctx));
        assert_eq!(cells(&b)[2], s("Mouse MX Master 3S"));
        b.changes.clear();
        assert_eq!(cells(&b), [None, s("Teclado K380"), e(), s("Fone WH-1000XM5")], "o fone não andou");
        // o controle conecta: no lugar vazio do mouse
        let mut back = gone.clone();
        back.iter_mut().find(|d| d.name == "Controle Xbox").unwrap().connected = true;
        let _ = ctx_run(|ctx| b.apply_devices(back, ctx));
        assert_eq!(cells(&b), [None, s("Teclado K380"), s("Controle Xbox"), s("Fone WH-1000XM5")]);
        assert_eq!(b.change_of("Controle Xbox").map(|c| c.0), Some(true), "e entra com a animação");
    }

    /// Conectando, o anel enche do zero até a carga, o ícone passa do tamanho e volta e o número
    /// aparece por último.
    #[test]
    fn connecting_fills_the_ring_from_zero() {
        assert_eq!(connect_icon(0.0), (0.5, 0.0));
        assert!(connect_icon(250.0).0 > 1.1, "o \"pop\"");
        assert!((connect_icon(900.0).0 - 1.0).abs() < 1e-4 && connect_icon(900.0).1 == 1.0);
        // desenha em cada ponto do caminho, e no começo o arco do fone não aparece
        com();
        let env_cfg = json!({"bateria": {}, "app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        let green_at = |ms: u64| {
            let b = Battery::sample_connecting(ms);
            let mut c = Canvas::new(SMALL as u32, SMALL as u32, 1.0);
            b.draw(&mut c, Mode::P, &env);
            let img = c.finish();
            // um ponto no meio do arco do fone (o segundo lugar), a 40% do caminho
            let (cx, cy) = Battery::cell_center(1);
            let a = (START + SWEEP * 0.4).to_radians();
            let (x, y) = ((cx + MINI_R * a.cos()).round() as u32, (cy + MINI_R * a.sin()).round() as u32);
            let i = ((y * img.w + x) * 4) as usize;
            img.px[i + 1] as i32 - img.px[i + 2] as i32
        };
        assert!(green_at(50) < 30, "o arco já estava lá");
        assert!(green_at(1500) > 60, "o arco não encheu");
    }

    /// No pequeno, o anel do notebook abre a energia e os outros (mesmo vazios) o Bluetooth.
    #[test]
    fn small_rings_open_power_or_bluetooth() {
        let env_cfg = json!({"bateria": {}, "app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        let b = Battery::sample_alone();
        let (x, y) = Battery::cell_center(0);
        assert_eq!(b.hit(x, y, Mode::P, &env), Some(OPEN_POWER));
        let (x, y) = Battery::cell_center(3);
        assert_eq!(b.hit(x, y, Mode::P, &env), Some(OPEN_BLUETOOTH));
        let (x, y) = Battery::cell_center(0);
        assert_eq!(Battery::sample_nothing().hit(x, y, Mode::P, &env), Some(OPEN_BLUETOOTH));
    }

    /// Uma lista nova de dispositivos que chega do fio entra no próximo tick (sem esperar meio
    /// minuto), e o widget confere o canal a cada meio segundo.
    #[test]
    fn device_lists_from_the_watcher_apply_on_the_next_tick() {
        let mut b = Battery::sample();
        b.fixed_now = None;
        let (tx, rx) = mpsc::channel();
        b.feed = Some(rx);
        assert!(b.next_tick().is_some_and(|t| t <= Instant::now() + CHECK_EVERY));
        let mut list = b.devices.clone();
        list.iter_mut().find(|d| d.name == "Controle Xbox").unwrap().connected = true;
        tx.send(b.devices.clone()).unwrap();
        tx.send(list).unwrap();
        let (redraw, _) = ctx_run(|ctx| b.tick(ctx));
        assert!(redraw);
        assert!(b.devices.iter().any(|d| d.name == "Controle Xbox" && d.connected), "ficou a última lista");
        // o fio acabou: o widget larga o canal
        drop(tx);
        let _ = ctx_run(|ctx| b.tick(ctx));
        assert!(b.feed.is_none());
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

    /// A porcentagem fica no meio do anel, medida na tinta das letras: na horizontal, no centro
    /// óptico (o número com o "%" um quarto do "%" à direita do centro) e, na vertical, os
    /// algarismos no meio (o grande desenhava 2 pt acima).
    #[test]
    fn the_percent_sits_in_the_middle_of_the_ring() {
        com();
        let env_cfg = json!({"bateria": {}, "app": {}});
        let env = Env { cfg: &env_cfg, key: "bateria" };
        let s = 1.25;
        for (name, b) in [("73%", Battery::sample()), ("carregando", Battery::sample_charging()), ("100%", Battery::sample_full(60_000))] {
            for (mode, ring) in [(Mode::M, RING_M), (Mode::G, RING_G)] {
                let l = super::super::Layout::of(mode);
                let mut c = Canvas::new((l.w * s) as u32, (l.h * s) as u32, s);
                b.draw(&mut c, mode, &env);
                let size = fit_size(&c, &b.percent_text(), if mode == Mode::M { PERCENT_M } else { PERCENT_G }, inner_width(ring));
                let sign = SIGN_GAP + c.text_width("%", NUM, size * PCT);
                let optical = (1.0 - super::super::PERCENT_SIGN_WEIGHT) * sign / 2.0;
                let bands = crate::testes::ink_bands(&c.finish(), s, (ring.cx, ring.cy), ring.r - ring.w / 2.0 - 1.0, ring.cy + 0.6 * ring.r);
                let number = bands[0];
                let (dx, dy) = ((number[0] + number[2]) / 2.0 - ring.cx - optical, (number[1] + number[3]) / 2.0 - ring.cy);
                // (um pixel a 125% é 0,8 pt, e os algarismos têm um respiro diferente de cada lado:
                // o "1", mais à esquerda, empurra o "100%" uns 1,6 pt para a direita)
                assert!(dx.abs() <= 2.0 && dy.abs() <= 1.0, "{name} {mode:?}: o número está {dx:+.1}, {dy:+.1} pt fora do meio");
            }
        }
    }

    /// O nome que não cabe perde o código do modelo do fim (e não é cortado no meio); o que cabe,
    /// ou que ficaria com uma palavra só, fica como está.
    #[test]
    fn long_device_names_lose_the_model_code() {
        com();
        let c = Canvas::new(10, 10, 1.0);
        let wide = c.text_width("Mouse MX Master 3S", TEXT, 11.5) + 1.0;
        assert_eq!(short_name(&c, "Mouse MX Master 3S", 11.5, wide), "Mouse MX Master 3S");
        let fits = c.text_width("Mouse MX Master", TEXT, 11.5) + 1.0;
        assert_eq!(short_name(&c, "Mouse MX Master 3S", 11.5, fits), "Mouse MX Master");
        assert_eq!(short_name(&c, "Fone WH-1000XM5", 11.5, 30.0), "Fone WH-1000XM5");
        assert_eq!(short_name(&c, "Controle Xbox Series", 11.5, 30.0), "Controle Xbox Series");
    }

    /// A linha do alto do grande diz desde quando o notebook está fora da tomada (quando acaba
    /// já está embaixo do anel); sem histórico, quando acaba.
    #[test]
    fn the_top_line_says_since_when() {
        let mut b = Battery::sample();
        let since = b.since_plug_change(false).expect("o exemplo saiu da tomada no fim da tarde");
        assert!(since < b.now() && since > b.now() - DAY);
        assert_eq!(b.top_line(), format!("tirado da tomada às {}", win::clock_at(since)));
        assert!(b.since_plug_change(true).is_none(), "na tomada, mas o histórico diz que não");
        b.hist.clear();
        assert!(b.top_line().starts_with("acaba às "), "{}", b.top_line());
    }

    #[test]
    fn a_full_charge_shrinks_to_fit_inside_the_ring() {
        com();
        let c = Canvas::new(10, 10, 1.0);
        for (g, size) in [(RING_M, PERCENT_M), (RING_G, PERCENT_G)] {
            let max = inner_width(g);
            // dois dígitos: o tamanho de sempre; o "100%": menor, e cabendo com o respiro
            let two = fit_size(&c, "73", size, max);
            let three = fit_size(&c, "100", size, max);
            assert_eq!(two, size, "{g:?}: o 73% diminuiu");
            assert!(three < size, "{g:?}: o 100% não diminuiu");
            let w = c.text_width("100", NUM, three) + SIGN_GAP + c.text_width("%", NUM, three * PCT);
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
        let connecting = [0, 120, 250, 500, 900].map(Battery::sample_connecting);
        for b in [Battery::sample(), Battery::sample_charging(), Battery::sample_desktop(), Battery::sample_alone(), Battery::sample_nothing(), Battery::sample_full(400), Battery::sample_changing(300), Battery::sample_rolling(300)].into_iter().chain(waves).chain(connecting) {
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
        assert!(G_ROWS_Y + 4.0 + 3.0 * 25.0 + 21.0 <= super::super::LARGE_H - 16.0);
    }
}

// ---------- na ilha ----------

/// Um aparelho (ou o notebook) com a carga e o ícone, para a ilha.
#[derive(Clone, Debug, PartialEq)]
struct Low {
    name: String,
    level: f32,
    icon: &'static str,
}

impl Low {
    /// "Fones|15": o aviso de cada aparelho em cada nível (abaixo de `ISLAND_LOW` e de `ISLAND_URGENT`).
    fn key(&self) -> String {
        format!("{}|{}", self.name, if self.level < ISLAND_URGENT { ISLAND_URGENT } else { ISLAND_LOW })
    }
}

/// A cor do aparelho que conectou: a da carga dele (verde sem a carga, só "conectou").
fn arrival_color(arrival: &Arrival) -> Rgba {
    arrival.level.map_or(OK, |l| level_color(l as f32))
}

fn kind_icon(kind: DevKind) -> &'static str {
    match kind {
        DevKind::Headphones => "fones",
        DevKind::Speaker => "caixa",
        DevKind::Mouse => "mouse",
        DevKind::Keyboard => "teclado",
        DevKind::Gamepad => "controle",
        DevKind::Phone => "celular",
        DevKind::Pen => "caneta",
        DevKind::Other => "bateria",
    }
}

fn island_buttons(form: IslandForm, w: f32, h: f32) -> Vec<(u32, (f32, f32, f32, f32))> {
    match form {
        IslandForm::Medium => vec![(DISMISS, (w - 74.0, h / 2.0 - 16.0, w - 18.0, h / 2.0 + 16.0))],
        IslandForm::Large => vec![(DISMISS, (w - 82.0, h - 46.0, w - 22.0, h - 14.0))],
        _ => Vec::new(),
    }
}

impl Battery {
    /// O notebook e os aparelhos conectados que dizem a carga, o notebook primeiro.
    fn island_list(&self) -> Vec<Low> {
        let mut out = Vec::new();
        if let Some(p) = &self.power {
            out.push(Low { name: "Notebook".into(), level: p.percent, icon: "notebook" });
        }
        for d in self.devices.iter().filter(|d| d.connected) {
            if let Some(l) = d.level {
                out.push(Low { name: d.name.clone(), level: l as f32, icon: kind_icon(d.kind) });
            }
        }
        out
    }

    /// O mais fraco abaixo de `ISLAND_LOW` (o notebook só fora da tomada), se ninguém dispensou o aviso.
    fn island_low(&self) -> Option<Low> {
        let plugged = self.power.as_ref().is_some_and(|p| p.plugged);
        self.island_list()
            .into_iter()
            .filter(|l| l.level < ISLAND_LOW && !(l.icon == "notebook" && plugged))
            .filter(|l| !self.island_ok.contains(&l.key()))
            .min_by(|a, b| a.level.total_cmp(&b.level))
    }

    /// O aparelho que acabou de conectar, enquanto a ilha o mostra.
    fn arrival_shown(&self) -> Option<&Arrival> {
        self.arrival.as_ref().filter(|a| a.at.elapsed() < ARRIVAL_SHOW)
    }

    /// A ilha logo depois de um aparelho conectar: o ícone dele, o nome e a carga, "Fone
    /// WH-1000XM5 conectou · 82%" (o verbo não depende do nome ser masculino ou feminino).
    fn draw_arrival(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, arrival: &Arrival) {
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let col = arrival_color(arrival);
        let glyph = kind_icon(arrival.kind);
        let pct = arrival.level.map(|l| format!("{l}%")).unwrap_or_default();
        let icon = |c: &mut Canvas, cx: f32, size: f32| {
            c.text(cx, cy, crate::ui::theme::icon(glyph), Face::Icons, size, col, Anchor::MM, None);
        };
        match form {
            IslandForm::Bubble => icon(c, x + w / 2.0, 17.0),
            IslandForm::Compact => {
                icon(c, x + 26.0, 15.0);
                iu::compact_text(c, x + 46.0, r, &arrival.name, &pct, col);
            }
            IslandForm::Banner => {
                icon(c, x + 26.0, 15.0);
                iu::compact_text(c, x + 46.0, r, &format!("{} conectou", arrival.name), &pct, col);
            }
            IslandForm::Medium => {
                iu::badge_icon(c, x + 39.0, cy, 22.0, glyph, col);
                let sub = if pct.is_empty() { "Acabou de conectar".to_string() } else { format!("Acabou de conectar · {pct}") };
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, &arrival.name, &sub, col);
                for (a, b) in island_buttons(form, w, h) {
                    iu::pill_button(c, (x + b.0, y + b.1, x + b.2, y + b.3), "OK", false, hot == Some(a));
                }
            }
            // aberta: a lista de sempre, com o que chegou em destaque
            IslandForm::Large => self.draw_island_list(c, r, hot, &arrival.name),
        }
    }

    /// Acabou de entrar (true) ou sair (false) da tomada: a ilha mostra a carga por alguns segundos.
    fn plug_shown(&self) -> Option<bool> {
        self.power.as_ref()?;
        self.plug_event.filter(|e| e.1.elapsed() < PLUG_SHOW).map(|e| e.0)
    }

    /// A linha da tomada na ilha: "cheia em 1 h 10", "3 h 20 restantes" (vazia sem a conta).
    fn plug_line(&self) -> String {
        let Some(p) = &self.power else { return String::new() };
        if p.plugged {
            match (p.charging, p.secs_to_full) {
                (true, Some(s)) => format!("cheia em {}", duration(s as f64)),
                (true, None) => String::new(),
                (false, _) if p.percent >= 99.0 => "carga completa".into(),
                (false, _) => "sem carregar".into(),
            }
        } else {
            self.secs_left().map_or(String::new(), |s| format!("{} restantes", duration(s)))
        }
    }

    /// A ilha logo depois de entrar ou sair da tomada: o raio e a carga, "⚡ 64% · cheia em 1 h 10".
    fn draw_plug(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, plugged: bool) {
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let level = self.power.as_ref().map_or(0.0, |p| p.percent);
        let col = if plugged { OK } else { level_color(level) };
        let pct = format!("{level:.0}%");
        let (glyph, title) = if plugged { ("carregando", "Carregando") } else { ("bateria", "Na bateria") };
        let line = self.plug_line();
        let icon = |c: &mut Canvas, cx: f32, size: f32| {
            c.text(cx, cy, crate::ui::theme::icon(glyph), Face::Icons, size, col, Anchor::MM, None);
        };
        match form {
            IslandForm::Bubble => icon(c, x + w / 2.0, 17.0),
            IslandForm::Compact => {
                icon(c, x + 26.0, 15.0);
                iu::compact_text(c, x + 46.0, r, title, &pct, col);
            }
            IslandForm::Banner => {
                icon(c, x + 26.0, 15.0);
                let label = if line.is_empty() { title.to_string() } else { format!("{title} · {line}") };
                iu::compact_text(c, x + 46.0, r, &label, &pct, col);
            }
            IslandForm::Medium => {
                iu::badge_icon(c, x + 39.0, cy, 22.0, glyph, col);
                let sub = if line.is_empty() { pct } else { format!("{pct} · {line}") };
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, title, &sub, col);
                for (a, b) in island_buttons(form, w, h) {
                    iu::pill_button(c, (x + b.0, y + b.1, x + b.2, y + b.3), "OK", false, hot == Some(a));
                }
            }
            // aberta: a lista de sempre (o notebook com a carga e os aparelhos)
            IslandForm::Large => self.draw_island_list(c, r, hot, "Notebook"),
        }
    }

    /// Os avisos dispensados voltam a valer quando o aparelho carrega (ou some).
    fn prune_island_ok(&mut self) {
        let list = self.island_list();
        self.island_ok.retain(|k| list.iter().any(|l| l.key() == *k || (k.starts_with(&format!("{}|", l.name)) && l.level < ISLAND_LOW + 5.0)));
    }

    fn draw_island(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>) {
        if self.island_low().is_none() {
            if let Some(arrival) = self.arrival_shown() {
                return self.draw_arrival(c, form, r, hot, arrival);
            }
            if let Some(plugged) = self.plug_shown() {
                return self.draw_plug(c, form, r, hot, plugged);
            }
        }
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let low = self.island_low().unwrap_or(Low { name: "Bateria".into(), level: 100.0, icon: "bateria" });
        let col = level_color(low.level);
        let pct = format!("{:.0}%", low.level);
        let glyph = |c: &mut Canvas, cx: f32, cy: f32, size: f32, color: Rgba| {
            c.text(cx, cy, crate::ui::theme::icon(low.icon), Face::Icons, size, color, Anchor::MM, None);
        };
        match form {
            IslandForm::Bubble => glyph(c, x + w / 2.0, cy, 17.0, col),
            IslandForm::Compact | IslandForm::Banner => {
                glyph(c, x + 26.0, cy, 15.0, col);
                let label = if form == IslandForm::Banner { format!("{} com pouca bateria", low.name) } else { low.name.clone() };
                iu::compact_text(c, x + 46.0, r, &label, &pct, col);
            }
            IslandForm::Medium => {
                c.fill_circle(x + 39.0, cy, 22.0, col.alpha(0.18));
                glyph(c, x + 39.0, cy, 19.0, col);
                let sub = if low.level < ISLAND_URGENT { format!("{pct} · quase no fim") } else { format!("{pct} · carregue logo") };
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, &low.name, &sub, col);
                for (a, b) in island_buttons(form, w, h) {
                    iu::pill_button(c, (x + b.0, y + b.1, x + b.2, y + b.3), "OK", false, hot == Some(a));
                }
            }
            IslandForm::Large => self.draw_island_list(c, r, hot, &low.name),
        }
    }

    /// A ilha aberta: o notebook e os aparelhos, cada um com a carga (`strong`: o do aviso), e a
    /// linha do notebook embaixo.
    fn draw_island_list(&self, c: &mut Canvas, r: Rect4, hot: Option<u32>, strong: &str) {
        let (x, y, w, h) = r;
        let list = self.island_list();
        let count = if list.len() == 1 { "1 aparelho".to_string() } else { format!("{} aparelhos", list.len()) };
        iu::header(c, r, "BATERIA", OK, &count, SOFT);
        for (n, d) in list.iter().take(4).enumerate() {
            let ry = y + 56.0 + n as f32 * 28.0;
            let dc = level_color(d.level);
            let strong = d.name == strong;
            c.text(x + 30.0, ry, crate::ui::theme::icon(d.icon), Face::Icons, 14.0, if strong { dc } else { SOFT }, Anchor::MM, None);
            c.text(x + 48.0, ry, &d.name, if strong { Face::Semibold } else { Face::Regular }, 12.0, FG, Anchor::LM, Some(104.0));
            iu::bar(c, x + 160.0, x + w - 70.0, ry, d.level / 100.0, dc);
            c.text(x + w - 22.0, ry, &format!("{:.0}%", d.level), Face::Semibold, 12.0, if strong { dc } else { SOFT }, Anchor::RM, None);
        }
        let arrived = self.arrival_shown().filter(|_| self.island_low().is_none());
        let foot = match &self.power {
            // logo depois de um aparelho conectar: "Fone WH-1000XM5 conectou"
            _ if arrived.is_some() => arrived.map_or(String::new(), |a| format!("{} conectou", a.name)),
            // logo depois de entrar ou sair da tomada: "Carregando · cheia às 15:03"
            Some(_) if self.plug_shown().is_some() => self.header_line(),
            Some(p) if p.charging => "Notebook carregando".to_string(),
            Some(p) if p.plugged => "Notebook na tomada".to_string(),
            _ => "Carregue para o aviso sumir".to_string(),
        };
        c.text(x + 22.0, y + h - 30.0, &foot, Face::Regular, 12.0, SOFT, Anchor::LM, Some(w - 130.0));
        for (a, b) in island_buttons(IslandForm::Large, w, h) {
            iu::pill_button(c, (x + b.0, y + b.1, x + b.2, y + b.3), "OK", true, hot == Some(a));
        }
    }
}
