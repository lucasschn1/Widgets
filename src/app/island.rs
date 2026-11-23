//! Ilha flutuante: o Tocando agora vira uma pílula preta por cima de todas as janelas, que dá
//! para arrastar para qualquer lugar da tela. Parada, mostra a capa e ondas no ritmo do som; com
//! o mouse em cima, estica e mostra a música, tocar e avançar; com um clique, abre inteira, com a
//! barra de progresso. "Voltar para a área de trabalho" (ou o clique direito) devolve o widget
//! ao lugar dele, que fica guardado enquanto isso. O lugar padrão é a barra de tarefas, à esquerda
//! do botão Iniciar (acompanhando o botão quando ele anda); arrastada para outro lugar, fica lá.
//!
//! Os outros widgets também entram na ilha, como na ilha dinâmica do iPhone: o Pomodoro
//! correndo, a CPU alta, um pedido de revisão, um fone acabando. Quem está vivo e foi mexido por
//! último fica na pílula; o segundo vira uma bolha ao lado (com o número dos outros que esperam).
//! Avisos não tomam a pílula: esticam a ilha por 4 s e esperam na bolha. Sem a música, a ilha
//! aparece sozinha quando algum widget tem o que mostrar e some quando ninguém tem.
//! (Módulo filho de `app`: mexe direto no estado do App.)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::*;
use crate::gfx::drawing;
use crate::gfx::{Anchor, Canvas, Face, Image};
use crate::widgets::media::{self, fmt_time};
use crate::widgets::{island_act, island_ui, Env, IslandForm, IslandInfo, IslandItem};

const KEY: &str = "midia";
/// a ilha inteira (pílula, textos e botões) neste tamanho, em relação à escala da tela
const SIZE: f32 = 0.86;

/// A escala da ilha no ponto (x, y) da tela: a do monitor, um pouco menor.
fn island_scale(x: i32, y: i32) -> f32 {
    win::scale_at(x, y) * SIZE
}
/// margem transparente em volta da pílula (a sombra curta)
const PAD: f32 = 8.0;
const STATE_MS: f32 = 480.0;
/// cartão virando ilha e ilha voltando a ser o cartão
const MORPH_MS: f32 = 420.0;
/// o mouse precisa ficar um instante em cima para a ilha esticar (e um pouco fora para recolher)
const HOVER_IN: Duration = Duration::from_millis(140);
const HOVER_OUT: Duration = Duration::from_millis(320);
/// aberta inteira e o mouse fora dela por esse tempo: volta ao tamanho normal
const LARGE_IDLE: Duration = Duration::from_secs(1);
/// nada tocando por esse tempo: volta para a área de trabalho
const QUIET: Duration = Duration::from_secs(5 * 60);
/// a que distância do alto e do meio da tela a ilha "puxa" para lá ao arrastar
const SNAP: f32 = 18.0;
/// distância do alto da tela, no "alto e no meio"
const TOP_GAP: f32 = 10.0;
/// na barra de tarefas: espaço até o botão Iniciar
const DOCK_GAP: f32 = 10.0;
/// arrastando, a que distância do lugar na barra a ilha encaixa lá
const DOCK_SNAP: f32 = 36.0;
/// na barra, de quanto em quanto tempo ver se o botão Iniciar andou (abrir e fechar apps mexe nele)
const DOCK_CHECK: Duration = Duration::from_secs(1);
/// a ilha aparecendo do nada (uma gota que acende e estica até a pílula) e sumindo (o conteúdo
/// apaga, a pílula encolhe numa gota e a gota some)
const APPEAR_MS: f32 = 620.0;
const VANISH_MS: f32 = 440.0;
/// um aviso estica a ilha por este tempo
const BANNER_FOR: Duration = Duration::from_secs(4);
/// espaço entre a pílula e as bolhas (e entre as bolhas)
const BUBBLE_GAP: f32 = 7.0;
/// de quanto em quanto tempo perguntar aos widgets o que eles têm para a ilha
const SYNC_EVERY: Duration = Duration::from_millis(400);
const SYNC_IDLE: Duration = Duration::from_millis(1000);

pub const M_ISLAND_BACK: u32 = 300_040;
pub const M_ISLAND_OPEN: u32 = 300_041;
pub const M_ISLAND_CENTER: u32 = 300_042;
pub const M_ISLAND_DOCK: u32 = 300_043;
pub const M_ISLAND_HIDE: u32 = 300_044;
pub const M_ISLAND_FOLLOW: u32 = 300_045;
/// a opção "Seguir a tela em uso" (em `cfg[KEY]`)
const FOLLOW_FIELD: &str = "ilha_segue_tela";
/// de quanto em quanto tempo ver em qual monitor está a janela em uso
const SCREEN_CHECK: Duration = Duration::from_millis(500);
/// a janela em uso precisa ficar este tempo no outro monitor para a ilha ir atrás: um clique
/// rápido lá (e a volta) não a faz atravessar a tela à toa
const SCREEN_SETTLE: Duration = Duration::from_millis(1500);
/// + o índice em `rest::Look::ALL`: liga ou desliga o que a ilha em repouso mostra
pub const M_ISLAND_REST: u32 = 303_010;
pub const M_ISLAND_REST_DOT: u32 = 303_020;
/// + a ação da fonte da ilha (o "não mostrar as notificações deste app")
pub const M_ISLAND_FEED: u32 = 303_100;
/// quantas ações cada fonte pode pôr no menu da ilha
const FEED_ACTIONS: u32 = 100;

const BLACK: Rgba = Rgba::rgb(0, 0, 0);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    /// fechada e encolhida numa gota (a ilha em repouso com o "só uma gota", ou o PC parado)
    Dot,
    Compact,
    /// esticada com um aviso, por alguns segundos
    Banner,
    Medium,
    Large,
}

impl Shape {
    /// O tamanho da forma, em pixels lógicos.
    fn size(self) -> island_ui::FormSize {
        match self {
            // a gota é menor que a bolha
            Shape::Dot => island_ui::FormSize { w: DOT, h: DOT, radius: DOT / 2.0 },
            _ => island_ui::size(self.form()),
        }
    }

    fn form(self) -> IslandForm {
        match self {
            Shape::Dot => IslandForm::Bubble,
            Shape::Compact => IslandForm::Compact,
            Shape::Banner => IslandForm::Banner,
            Shape::Medium => IslandForm::Medium,
            Shape::Large => IslandForm::Large,
        }
    }

    /// Fechada (a pílula ou a gota)?
    fn closed(self) -> bool {
        matches!(self, Shape::Dot | Shape::Compact)
    }
}

/// A ilha em repouso (só entra quando ninguém mais tem o que mostrar).
const REST: &str = crate::widgets::outside::REST;
/// apagada com o PC parado
const DIM: f32 = 0.5;
/// quanto a opacidade anda por quadro: apaga devagar, acende rápido (no primeiro movimento)
const DIM_OUT_STEP: f32 = 0.02;
const DIM_IN_STEP: f32 = 0.08;
/// a troca do conteúdo ao mudar de forma ou de widget dura isto
const LAYER_FADE_MS: f32 = 420.0;
/// perto da ilha (para responder rápido quando o mouse chegar): esta margem em volta, em pontos
const NEAR_MARGIN: f32 = 120.0;
/// o tamanho da gota (pixels lógicos)
const DOT: f32 = 26.0;
/// a gota aberta com um clique volta a ser gota com o mouse longe este tanto
const DOT_BACK: Duration = Duration::from_secs(3);
/// quanto a roda do mouse precisa girar para trocar (um "clique" da roda; no touchpad, a soma
/// dos pedacinhos)
const WHEEL_STEP: f32 = 1.0;

/// O que a roda do mouse faz na ilha.
#[derive(Clone, Copy, PartialEq, Debug)]
enum WheelDoes {
    /// o volume da música (com ela na pílula)
    Volume,
    /// troca o que a ilha em repouso mostra (o relógio, o tempo, as cotações...)
    Looks,
    /// troca o widget da pílula (com mais de um na ilha)
    Widgets,
}

/// O que a roda faz com o mouse nas bolhas (`on_bubbles`) ou na pílula que mostra `shown`: nas
/// bolhas, sempre troca o widget; na pílula, cada um usa a roda do seu jeito.
fn wheel_does(on_bubbles: bool, shown: &str) -> WheelDoes {
    if on_bubbles {
        WheelDoes::Widgets
    } else if shown == KEY {
        WheelDoes::Volume
    } else if shown == REST {
        WheelDoes::Looks
    } else {
        WheelDoes::Widgets
    }
}

/// Um widget com algo na ilha agora.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Entry {
    key: &'static str,
    item: IslandItem,
}

/// Põe os itens na ordem da ilha: o primeiro fica na pílula, o segundo na bolha. Na frente, o
/// que a pessoa escolheu (clicou na bolha, passou o mouse no aviso) e o aviso urgente ainda não
/// visto; depois quem tem pressa (o último minuto do Pomodoro), quem está andando, quem está
/// pausado e, por fim, os avisos. Empatados, o mexido por último.
fn rank(entries: &mut [Entry], touched: &HashMap<&'static str, Instant>, pinned: Option<&str>, unseen: &[&'static str]) {
    let tier = |e: &Entry| -> u8 {
        if e.item.urgent && unseen.contains(&e.key) {
            0
        } else if Some(e.key) == pinned {
            1
        } else if e.item.hurry {
            2
        } else if e.item.alert {
            5
        } else if e.item.idle {
            4
        } else {
            3
        }
    };
    entries.sort_by(|a, b| tier(a).cmp(&tier(b)).then_with(|| touched.get(b.key).cmp(&touched.get(a.key))));
}

/// O que a ilha lê dos widgets para se desenhar (emprestado do App a cada quadro).
pub(super) struct Scene<'a> {
    /// a música, se o Tocando agora está na ilha
    info: Option<&'a IslandInfo>,
    kinds: Vec<&'a dyn Kind>,
    cfg: &'a Value,
}

impl<'a> Scene<'a> {
    fn of(info: Option<&'a IslandInfo>, cards: &'a [Card], feeds: &'a [Box<dyn Kind>], cfg: &'a Value) -> Scene<'a> {
        let kinds = cards.iter().map(|c| c.kind.as_ref()).chain(feeds.iter().map(|f| f.as_ref())).collect();
        Scene { info, kinds, cfg }
    }
    /// Só a música (os quadros do README e os testes).
    #[cfg(test)]
    fn media(info: Option<&'a IslandInfo>) -> Scene<'a> {
        Scene { info, kinds: Vec::new(), cfg: &Value::Null }
    }
    fn kind(&self, key: &str) -> Option<&'a dyn Kind> {
        self.kinds.iter().copied().find(|k| k.key() == key)
    }
    fn env(&self, key: &'static str) -> Env<'a> {
        Env { cfg: self.cfg, key }
    }
}

/// A ilha nascendo ou sumindo (sem a música: porque algum widget tem, ou deixou de ter, o que
/// mostrar).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pop {
    Appear,
    Vanish,
}

/// Rampa suave de 0 a 1 entre `a` e `b`.
fn ramp(p: f32, a: f32, b: f32) -> f32 {
    ease_in_out(((p - a) / (b - a)).clamp(0.0, 1.0))
}

/// O estado da ilha com vários widgets.
struct Multi {
    /// quem está na ilha, na ordem (o primeiro na pílula)
    entries: Vec<Entry>,
    /// o widget desenhado na pílula agora e o de antes (para trocar com um esmaecer)
    key: &'static str,
    prev_key: &'static str,
    /// quando cada um entrou ou foi mexido por último
    touched: HashMap<&'static str, Instant>,
    /// o último aviso que esticou a ilha, de cada widget
    bannered: HashMap<&'static str, u64>,
    /// o aviso esticando a ilha agora, e desde quando
    banner: Option<(&'static str, Instant)>,
    /// avisos que ninguém viu ainda (o pontinho na bolha)
    unseen: Vec<&'static str>,
    /// o escolhido pela pessoa: (widget, fica até ela escolher outro)
    pinned: Option<(&'static str, bool)>,
    /// as bolhas todas abertas (mouse em cima da bolha com mais gente esperando)
    strip: bool,
    strip_out: Option<Instant>,
    /// quando a primeira bolha apareceu (ela entra esmaecendo)
    bubbles_at: Instant,
    /// nascendo ou sumindo, e desde quando
    pop: Option<(Pop, Instant)>,
    /// o widget do clique direito
    menu_key: &'static str,
    /// a capa pequena (na bolha), já reduzida
    bubble_art: Option<(usize, u32, Image)>,
    /// os widgets na ordem em que o App os acha, que não muda quando a ilha reordena: a roda do
    /// mouse anda por ela (pela ordem da ilha, o escolhido iria para a frente e a roda voltaria
    /// sempre aos mesmos dois)
    order: Vec<&'static str>,
    /// a roda do mouse: quanto já girou sem trocar, e quando foi a última troca
    wheel: f32,
    wheel_at: Instant,
}

impl Multi {
    fn new(now: Instant) -> Multi {
        Multi {
            entries: Vec::new(),
            key: KEY,
            prev_key: KEY,
            touched: HashMap::new(),
            bannered: HashMap::new(),
            banner: None,
            unseen: Vec::new(),
            pinned: None,
            strip: false,
            strip_out: None,
            bubbles_at: now,
            pop: None,
            menu_key: KEY,
            bubble_art: None,
            order: Vec::new(),
            wheel: 0.0,
            wheel_at: now - Duration::from_secs(1),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Morph {
    None,
    /// o cartão virando ilha
    In,
    /// a ilha voltando a ser o cartão
    Out,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Btn {
    Prev,
    Toggle,
    Next,
    /// milésimos da música
    Seek(u32),
    Open,
    Back,
    /// um botão do widget na pílula (a ação dele)
    Widget(u32),
    /// a bolha `n` (0 = a primeira, ao lado da pílula)
    Bubble(usize),
}

/// Retângulo (x, y, largura, altura) e raio, em pixels da tela.
#[derive(Clone, Copy, Debug)]
struct Geo {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
}

impl Geo {
    fn lerp(a: Geo, b: Geo, t: f32) -> Geo {
        let mix = |from: f32, to: f32| from + (to - from) * t;
        Geo { x: mix(a.x, b.x), y: mix(a.y, b.y), w: mix(a.w, b.w).max(1.0), h: mix(a.h, b.h).max(1.0), r: mix(a.r, b.r).max(0.0) }
    }

    fn contains(&self, px: i32, py: i32) -> bool {
        let (px, py) = (px as f32, py as f32);
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

type Rect4 = (f32, f32, f32, f32);

/// os botões do mouse (para "clicou fora")
const VK_LBUTTON: i32 = 0x01;
const VK_RBUTTON: i32 = 0x02;

/// A cor com a opacidade multiplicada por `alpha`.
fn faded(color: Rgba, alpha: f32) -> Rgba {
    color.with_a((color.3 as f32 * alpha) as u8)
}

/// A janela da ilha na tela: a pílula e as bolhas, com `pad` de margem em volta (a sombra).
/// Devolve (x, y, largura, altura), em pixels.
fn window_bounds(pill: Geo, bubbles: &[(f32, f32, f32)], pad: f32) -> (f32, f32, u32, u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (pill.x, pill.y, pill.x + pill.w, pill.y + pill.h);
    for &(cx, cy, r) in bubbles {
        x0 = x0.min(cx - r);
        y0 = y0.min(cy - r);
        x1 = x1.max(cx + r);
        y1 = y1.max(cy + r);
    }
    let (wx, wy) = ((x0 - pad).round(), (y0 - pad).round());
    (wx, wy, (x1 + pad - wx).round().max(1.0) as u32, (y1 + pad - wy).round().max(1.0) as u32)
}

/// Quanto esperar até o próximo quadro: animando, 60 por segundo; o widget pedindo quadros, no
/// ritmo dele; a música tocando (as ondas), 30; o mouse perto, 16 (para responder rápido); parada
/// e o mouse longe, 4 por segundo (só para ver se o mouse chegou: sem nada mudando, nem
/// redesenha).
fn next_frame_wait(busy: bool, fps: u32, playing: bool, near: bool) -> Duration {
    let ms = if busy {
        16
    } else if fps > 0 {
        (1000 / fps.min(60)) as u64
    } else if playing {
        33
    } else if near {
        60
    } else {
        250
    };
    Duration::from_millis(ms)
}

fn inside(r: Rect4, x: f32, y: f32) -> bool {
    x >= r.0 && x <= r.2 && y >= r.1 && y <= r.3
}

/// O lugar da pílula fechada na barra de tarefas (centro x e topo y, em pixels da tela): à esquerda
/// do botão Iniciar, no meio da altura da barra. Sem espaço ali (barra alinhada à esquerda), fica
/// antes da área de notificação.
fn taskbar_slot(t: &win::Taskbar, s: f32) -> (f32, f32) {
    let island_ui::FormSize { w, h, .. } = Shape::Compact.size();
    let (w, h, gap) = (w * s, h * s, DOCK_GAP * s);
    let bar = t.bar;
    let right = match (t.start, t.tray) {
        (Some(start), _) if (start.0 - bar.0) as f32 >= w + 2.0 * gap => start.0 as f32 - gap,
        (_, Some(tray)) => tray.0 as f32 - gap,
        _ => bar.2 as f32 - gap,
    };
    ((right - w / 2.0).round(), (bar.1 as f32 + ((bar.3 - bar.1) as f32 - h) / 2.0).round())
}

/// O lugar na barra de tarefas do monitor do ponto e a escala da ilha lá, se há barra à vista.
fn dock_slot(x: i32, y: i32) -> Option<((f32, f32), f32)> {
    let taskbar = win::taskbar_at(x, y)?;
    let s = island_scale((taskbar.bar.0 + taskbar.bar.2) / 2, (taskbar.bar.1 + taskbar.bar.3) / 2);
    Some((taskbar_slot(&taskbar, s), s))
}

/// A âncora da ilha levada da área de trabalho `from` (onde a escala da ilha é `s_from`) para
/// `to` (escala `s_to`), no mesmo lugar relativo: na mesma fração da largura e na mesma distância
/// (em pontos) da borda mais perto, a de cima ou a de baixo. No alto e no meio continua no alto e
/// no meio; perto de baixo continua perto de baixo.
fn anchor_on(anchor: (f32, f32), s_from: f32, from: Rect, to: Rect, s_to: f32) -> (f32, f32) {
    let (fw, tw) = ((from.2 - from.0).max(1) as f32, (to.2 - to.0) as f32);
    let x = to.0 as f32 + (anchor.0 - from.0 as f32) / fw * tw;
    let from_top = anchor.1 - from.1 as f32;
    let from_bottom = from.3 as f32 - anchor.1;
    let y = if from_top <= from_bottom { to.1 as f32 + from_top / s_from * s_to } else { to.3 as f32 - from_bottom / s_from * s_to };
    (x, y.clamp(to.1 as f32, to.3 as f32))
}

/// "Seguir a tela em uso" está ligado?
fn follows_screen(cfg: &Value) -> bool {
    cfg[KEY][FOLLOW_FIELD].as_bool().unwrap_or(false)
}

/// Botões de cada forma, em pixels lógicos a partir do canto da pílula.
fn buttons(shape: Shape, lw: f32, lh: f32) -> Vec<(Btn, Rect4)> {
    let circle = |cx: f32, cy: f32, r: f32| (cx - r, cy - r, cx + r, cy + r);
    match shape {
        Shape::Dot | Shape::Compact | Shape::Banner => Vec::new(),
        Shape::Medium => vec![(Btn::Toggle, circle(lw - 74.0, lh / 2.0, 18.0)), (Btn::Next, circle(lw - 34.0, lh / 2.0, 16.0))],
        Shape::Large => vec![
            (Btn::Seek(0), (20.0, 88.0, lw - 20.0, 104.0)),
            (Btn::Prev, circle(lw / 2.0 - 62.0, 146.0, 18.0)),
            (Btn::Toggle, circle(lw / 2.0, 146.0, 23.0)),
            (Btn::Next, circle(lw / 2.0 + 62.0, 146.0, 18.0)),
            (Btn::Open, (14.0, 182.0, 140.0, 208.0)),
            (Btn::Back, (lw - 20.0 - 214.0, 182.0, lw - 20.0, 208.0)),
        ],
    }
}

pub struct Island {
    pub hwnd: HWND,
    shape: Shape,
    prev_shape: Shape,
    shape_at: Instant,
    from: Geo,
    to: Geo,
    anim_at: Instant,
    anim_ms: f32,
    morph: Morph,
    /// centro (x) e topo (y) da pílula fechada, em pixels da tela
    anchor: (f32, f32),
    s: f32,
    /// na barra de tarefas, ao lado do botão Iniciar (cresce para a esquerda, sem cobrir o botão)
    docked: bool,
    /// quando ver de novo se o botão Iniciar andou
    dock_check: Instant,
    /// botão apertado: (x, y na tela, botão, âncora na hora)
    press: Option<(i32, i32, Option<Btn>, (f32, f32))>,
    dragging: bool,
    hot: Option<Btn>,
    inside_since: Option<Instant>,
    outside_since: Option<Instant>,
    hidden: bool,
    quiet_since: Option<Instant>,
    levels: [f32; 5],
    started: Instant,
    next_frame: Instant,
    /// cor do cartão: a transformação parte dela (e volta para ela)
    card_color: Rgba,
    /// capa já reduzida: (endereço da imagem, pixels, imagem)
    art_cache: Option<(usize, u32, Image)>,
    /// o cartão como aparece na tela, para a transformação (de ida e de volta)
    card_face: Option<Image>,
    /// o Tocando agora está na ilha (o cartão dele fica escondido)
    media: bool,
    multi: Multi,
    /// a opacidade da janela agora (1, ou apagando até `DIM` com o PC parado)
    dim: f32,
    /// o que foi para a tela por último, parada (sem mudar nada, não redesenha)
    drawn: Option<u64>,
    /// a gota aberta com um clique (fica pílula até o mouse ir embora)
    opened_dot: bool,
    /// quando ver de novo em qual monitor está a janela em uso, e o outro monitor onde ela está
    /// (e desde quando), esperando `SCREEN_SETTLE`
    screen_check: Instant,
    screen_pending: Option<(Rect, Instant)>,
}

impl Island {
    /// Quanto já andou da animação de forma ou de lugar (0 a 1).
    fn progress(&self) -> f32 {
        if self.anim_ms <= 0.0 { 1.0 } else { (self.anim_at.elapsed().as_secs_f32() * 1000.0 / self.anim_ms).min(1.0) }
    }

    /// Animando agora (precisa de quadros seguidos)?
    fn animating(&self) -> bool {
        self.progress() < 1.0 || self.shape_at.elapsed().as_secs_f32() < 0.5 || self.multi.pop.is_some()
    }

    /// O item da ilha em repouso, se é ela que está na pílula.
    fn rest_item(&self) -> Option<IslandItem> {
        self.multi.entries.first().filter(|e| e.key == REST && self.multi.key == REST).map(|e| e.item)
    }

    /// O que está na tela, para saber se precisa redesenhar: o carimbo do widget na pílula (None:
    /// ele não sabe dizer, redesenha sempre), a forma, o lugar, o botão sob o mouse e a opacidade.
    fn signature(&self, scene: &Scene, shown: &'static str) -> Option<u64> {
        let stamp = scene.kind(shown)?.island_stamp(&scene.env(shown))?;
        let g = self.geo();
        let text = format!("{stamp}|{shown}|{:?}|{:?}|{:?}|{}|{}|{}|{}|{}|{}|{}", self.shape, self.prev_shape, self.hot, g.x, g.y, g.w, g.h, self.dim, self.multi.entries.len(), self.s);
        Some(crate::widgets::alert_hash(&text))
    }

    /// A forma fechada agora: a gota (a ilha em repouso pedindo) ou a pílula.
    fn closed_shape(&self) -> Shape {
        if self.rest_item().is_some_and(|i| i.dot) && !self.opened_dot { Shape::Dot } else { Shape::Compact }
    }

    /// A opacidade que a janela quer agora (apagada com o PC parado).
    fn dim_target(&self) -> f32 {
        if self.rest_item().is_some_and(|i| i.dim) && self.shape.closed() { DIM } else { 1.0 }
    }

    /// Sumindo (ninguém mais tem o que mostrar)?
    fn vanishing(&self) -> bool {
        matches!(self.multi.pop, Some((Pop::Vanish, _)))
    }

    /// Nascendo (ainda sem as bolhas)?
    fn appearing(&self) -> bool {
        matches!(self.multi.pop, Some((Pop::Appear, _)))
    }

    /// Quanto já andou do nascer ou do sumir (0 a 1).
    fn pop_progress(&self) -> f32 {
        let progress = match self.multi.pop {
            Some((Pop::Appear, at)) => at.elapsed().as_secs_f32() * 1000.0 / APPEAR_MS,
            Some((Pop::Vanish, at)) => at.elapsed().as_secs_f32() * 1000.0 / VANISH_MS,
            None => 1.0,
        };
        progress.min(1.0)
    }

    /// Opacidade (da janela, do conteúdo) nascendo ou sumindo. Nascendo, a gota acende primeiro
    /// e o conteúdo aparece quando a pílula já está quase no tamanho; sumindo, o conteúdo apaga
    /// primeiro e a gota some no fim.
    fn pop_alpha(&self) -> (f32, f32) {
        let progress = self.pop_progress();
        match self.multi.pop {
            Some((Pop::Appear, _)) => (ramp(progress, 0.0, 0.25), ramp(progress, 0.4, 0.85)),
            Some((Pop::Vanish, _)) => (1.0 - ramp(progress, 0.55, 1.0), 1.0 - ramp(progress, 0.0, 0.3)),
            None => (1.0, 1.0),
        }
    }

    /// A gota de onde a ilha nasce (e onde ela some): um círculo de `size` vezes a altura da
    /// pílula `pill`, do lado do botão Iniciar na barra de tarefas (fora dela, no meio).
    fn drop_of(&self, pill: Geo, size: f32) -> Geo {
        let diameter = pill.h * size;
        let cx = if self.docked { pill.x + pill.w - pill.h / 2.0 } else { pill.x + pill.w / 2.0 };
        let cy = pill.y + pill.h / 2.0;
        Geo { x: cx - diameter / 2.0, y: cy - diameter / 2.0, w: diameter, h: diameter, r: diameter / 2.0 }
    }

    /// Começa a sumir: o conteúdo apaga e a pílula encolhe numa gota (`animate`: sem animação,
    /// some no próximo quadro).
    fn start_vanish(&mut self, animate: bool, now: Instant) {
        let done = if animate { Duration::ZERO } else { Duration::from_millis(VANISH_MS as u64) };
        self.multi.pop = Some((Pop::Vanish, now - done));
        self.multi.strip = false;
        self.from = self.geo();
        self.to = self.drop_of(self.target(Shape::Compact), 0.4);
        self.anim_at = now;
        self.anim_ms = if animate { VANISH_MS } else { 0.0 };
        self.next_frame = now;
    }

    /// Voltou alguém enquanto ela sumia: a pílula estica de novo, de onde estava.
    fn cancel_vanish(&mut self, now: Instant) {
        let (window_alpha, _) = self.pop_alpha();
        // a janela ainda inteira: o conteúdo volta a acender; já apagando, a gota acende de onde
        // estava (o passo do nascer com a mesma opacidade: o inverso da rampa)
        let unease = |y: f32| if y < 0.5 { (y / 4.0).cbrt() } else { 1.0 - (2.0 * (1.0 - y)).cbrt() / 2.0 };
        let back = if window_alpha >= 0.999 { 0.4 } else { 0.25 * unease(window_alpha) };
        self.multi.pop = Some((Pop::Appear, now - Duration::from_millis((back * APPEAR_MS) as u64)));
        self.from = self.geo();
        self.to = self.target(self.shape);
        self.anim_at = now;
        self.anim_ms = STATE_MS;
        self.next_frame = now;
    }

    /// Onde a pílula está agora (no meio da animação, entre `from` e `to`).
    fn geo(&self) -> Geo {
        let progress = self.progress();
        if progress >= 1.0 {
            return self.to;
        }
        // entre as formas da ilha, mola (estica e volta, como uma ilha de verdade); voando entre
        // a ilha e o cartão, sem passar do ponto
        let eased = if self.vanishing() {
            // sumindo: sai devagar e acelera para dentro da gota, sem mola
            ease_in_out(progress)
        } else if self.morph == Morph::None {
            ease_spring(progress)
        } else {
            ease_settle(progress)
        };
        Geo::lerp(self.from, self.to, eased)
    }

    /// Opacidade do cartão desenhado por cima da pílula, na transformação.
    fn face_alpha(&self) -> f32 {
        let progress = self.progress();
        match self.morph {
            // indo para a ilha: o cartão some enquanto a ilha aparece
            Morph::In => 1.0 - ease_in_out(((progress - 0.1) / 0.45).clamp(0.0, 1.0)),
            // voltando: o cartão aparece enquanto a ilha some (sem caixa vazia no meio) e
            // termina inteiro, no lugar dele
            Morph::Out => ease_in_out(((progress - 0.08) / 0.42).clamp(0.0, 1.0)),
            Morph::None => 0.0,
        }
    }

    /// Onde fica a pílula na forma dada: centrada na âncora; na metade de baixo da tela cresce
    /// para cima; na barra de tarefas, para a esquerda. Nunca sai da tela.
    fn target(&self, shape: Shape) -> Geo {
        let s = self.s;
        let island_ui::FormSize { w, h, radius: r } = shape.size();
        let (w, h, r) = (w * s, h * s, r * s);
        let compact_h = Shape::Compact.size().h * s;
        let (scr, _) = win::monitor_at(self.anchor.0 as i32, (self.anchor.1 + compact_h / 2.0) as i32);
        let lower = self.anchor.1 + compact_h / 2.0 > (scr.1 + scr.3) as f32 / 2.0;
        let y = if shape == Shape::Dot {
            // a gota fica no meio da altura da pílula
            self.anchor.1 + (compact_h - h) / 2.0
        } else if lower {
            self.anchor.1 + compact_h - h
        } else {
            self.anchor.1
        };
        let compact_w = Shape::Compact.size().w * s;
        let x = if self.docked { self.anchor.0 + compact_w / 2.0 - w } else { self.anchor.0 - w / 2.0 };
        // nunca encosta na borda da tela
        let (margin_x, margin_y) = (6.0 * s, 4.0 * s);
        let x = x.clamp(scr.0 as f32 + margin_x, (scr.2 as f32 - margin_x - w).max(scr.0 as f32 + margin_x));
        let y = y.clamp(scr.1 as f32 + margin_y, (scr.3 as f32 - margin_y - h).max(scr.1 as f32 + margin_y));
        Geo { x, y, w, h, r }
    }

    /// Muda de forma, animando (com a mola) de onde estiver.
    fn set_shape(&mut self, shape: Shape) {
        if shape == self.shape {
            return;
        }
        self.from = self.geo();
        self.multi.prev_key = self.key_for(self.shape);
        self.prev_shape = self.shape;
        self.shape = shape;
        self.to = self.target(shape);
        self.shape_at = Instant::now();
        self.anim_at = Instant::now();
        self.anim_ms = STATE_MS;
        self.next_frame = Instant::now();
    }

    /// Vai direto para o lugar certo (arrastando, ou depois de mudar de monitor).
    fn snap_to_target(&mut self) {
        self.to = self.target(self.shape);
        self.from = self.to;
        self.anim_ms = 0.0;
        self.next_frame = Instant::now();
    }

    /// Opacidade (forma anterior, forma atual) do conteúdo: uma some rápido e a outra aparece
    /// quando a pílula já está quase no tamanho novo.
    fn layer_alpha(&self) -> (f32, f32) {
        match self.morph {
            Morph::In => (0.0, ((self.progress() - 0.4) / 0.45).clamp(0.0, 1.0)),
            Morph::Out => (0.0, 1.0 - ease_in_out((self.progress() / 0.38).clamp(0.0, 1.0))),
            Morph::None => {
                let progress = self.shape_at.elapsed().as_secs_f32() * 1000.0 / LAYER_FADE_MS;
                ((1.0 - progress / 0.3).clamp(0.0, 1.0), ((progress - 0.25) / 0.5).clamp(0.0, 1.0))
            }
        }
    }

    /// O widget desenhado na pílula numa forma (esticada, o do aviso).
    fn key_for(&self, shape: Shape) -> &'static str {
        match (shape, self.multi.banner) {
            (Shape::Banner, Some((k, _))) => k,
            _ => self.multi.key,
        }
    }

    /// Troca o widget da pílula (o de antes some enquanto o novo aparece).
    fn switch_key(&mut self, key: &'static str) {
        if key == self.multi.key {
            return;
        }
        self.multi.prev_key = self.key_for(self.shape);
        self.multi.key = key;
        self.prev_shape = self.shape;
        self.shape_at = Instant::now();
        self.next_frame = Instant::now();
    }

    /// Na metade de baixo da tela (cresce para cima)?
    fn lower(&self) -> bool {
        let compact_h = Shape::Compact.size().h * self.s;
        let (scr, _) = win::monitor_at(self.anchor.0 as i32, (self.anchor.1 + compact_h / 2.0) as i32);
        self.anchor.1 + compact_h / 2.0 > (scr.1 + scr.3) as f32 / 2.0
    }

    /// Quem vai nas bolhas: todos menos o da pílula (esticada com um aviso, a pílula mostra o
    /// aviso e o de sempre vai para a primeira bolha).
    fn bubble_entries(&self) -> Vec<Entry> {
        let shown = self.key_for(self.shape);
        let mut list: Vec<Entry> = self.multi.entries.iter().copied().filter(|e| e.key != shown).collect();
        if shown != self.multi.key {
            if let Some(pos) = list.iter().position(|e| e.key == self.multi.key) {
                let usual = list.remove(pos);
                list.insert(0, usual);
            }
        }
        list
    }

    /// Quantas bolhas aparecem: a primeira; com o mouse em cima, todas.
    fn bubble_count(&self) -> usize {
        let extra = self.bubble_entries().len();
        if self.morph != Morph::None || self.multi.pop.is_some() {
            0
        } else if self.multi.strip {
            extra
        } else {
            extra.min(1)
        }
    }

    /// As bolhas na tela: (centro x, centro y, raio), em pixels, da pílula para a esquerda, na
    /// altura da pílula fechada.
    fn bubbles(&self) -> Vec<(f32, f32, f32)> {
        let pill = self.geo();
        let s = self.s;
        let diameter = island_ui::size(IslandForm::Bubble).w * s;
        let gap = BUBBLE_GAP * s;
        let compact_h = Shape::Compact.size().h * s;
        let cy = if self.lower() { pill.y + pill.h - compact_h / 2.0 } else { pill.y + compact_h / 2.0 };
        let center_x = |n: usize| pill.x - gap - diameter / 2.0 - n as f32 * (diameter + gap);
        (0..self.bubble_count()).map(|n| (center_x(n), cy, diameter / 2.0)).collect()
    }

    /// A bolha sob o ponto da tela.
    fn bubble_at(&self, sx: i32, sy: i32) -> Option<usize> {
        self.bubbles().iter().position(|&(cx, cy, r)| (sx as f32 - cx).hypot(sy as f32 - cy) <= r)
    }

    /// O mouse está perto das bolhas abertas (no espaço entre elas também)?
    fn near_bubbles(&self, sx: i32, sy: i32) -> bool {
        let bubbles = self.bubbles();
        let (Some(first), Some(last)) = (bubbles.first(), bubbles.last()) else { return false };
        let reach = first.2 + BUBBLE_GAP * self.s;
        (sx as f32) >= last.0 - reach && (sx as f32) <= first.0 + reach && (sy as f32 - first.1).abs() <= reach
    }

    /// O mouse está perto da ilha (para responder rápido quando ele chegar)?
    fn near(&self, sx: i32, sy: i32) -> bool {
        let pill = self.geo();
        let margin = NEAR_MARGIN * self.s;
        let left = self.bubbles().last().map_or(pill.x, |b| b.0 - b.2);
        (sx as f32) >= left - margin && (sx as f32) <= pill.x + pill.w + margin && (sy as f32) >= pill.y - margin && (sy as f32) <= pill.y + pill.h + margin
    }

    /// Botão sob o ponto (sx, sy) da tela, na forma atual.
    fn btn_at(&self, sx: i32, sy: i32, scene: &Scene) -> Option<Btn> {
        if self.morph != Morph::None {
            return None;
        }
        if let Some(k) = self.bubble_at(sx, sy) {
            return Some(Btn::Bubble(k));
        }
        // o ponto e o tamanho da pílula em pontos (lx, ly a partir do canto dela)
        let pill = self.geo();
        let (lx, ly) = ((sx as f32 - pill.x) / self.s, (sy as f32 - pill.y) / self.s);
        let (lw, lh) = (pill.w / self.s, pill.h / self.s);
        if !(0.0..=lw).contains(&lx) || !(0.0..=lh).contains(&ly) {
            return None;
        }
        let key = self.key_for(self.shape);
        if key != KEY {
            let kind = scene.kind(key)?;
            return kind.island_hit(self.shape.form(), lx, ly, lw, lh, &scene.env(key)).map(Btn::Widget);
        }
        let info = scene.info?;
        buttons(self.shape, lw, lh).into_iter().find(|(_, r)| inside(*r, lx, ly)).and_then(|(b, r)| match b {
            Btn::Seek(_) if !info.can_seek || info.duration <= 0.0 => None,
            Btn::Seek(_) => Some(Btn::Seek((((lx - r.0) / (r.2 - r.0)).clamp(0.0, 1.0) * 1000.0) as u32)),
            Btn::Prev if !info.can_prev => None,
            Btn::Next if !info.can_next => None,
            Btn::Toggle if !info.can_toggle => None,
            b => Some(b),
        })
    }

    /// Os widgets que o App achou para a ilha agora: avisos novos esticam a ilha; a ordem decide
    /// quem fica na pílula.
    fn update_entries(&mut self, found: Vec<Entry>, now: Instant) {
        let had_bubble = self.multi.entries.len() > 1;
        let multi = &mut self.multi;
        // quem saiu da ilha leva junto o que se sabia dele
        let present = |key: &str| found.iter().any(|e| e.key == key);
        multi.touched.retain(|key, _| present(key));
        multi.unseen.retain(|key| present(key));
        multi.bannered.retain(|key, _| present(key));
        if multi.pinned.is_some_and(|(key, _)| !present(key)) {
            multi.pinned = None;
        }
        if multi.banner.is_some_and(|(key, _)| !present(key)) {
            multi.banner = None;
        }
        // um aviso novo (ou outro do mesmo widget) estica a ilha e fica marcado como não visto
        for entry in &found {
            multi.touched.entry(entry.key).or_insert(now);
            let new_alert = entry.item.alert && multi.bannered.get(entry.key) != Some(&entry.item.alert_id);
            if new_alert {
                multi.bannered.insert(entry.key, entry.item.alert_id);
                if !multi.unseen.contains(&entry.key) {
                    multi.unseen.push(entry.key);
                }
                if multi.banner.is_none() {
                    multi.banner = Some((entry.key, now));
                }
            }
        }
        multi.order = found.iter().map(|e| e.key).collect();
        multi.entries = found;
        rank(&mut multi.entries, &multi.touched, multi.pinned.map(|(key, _)| key), &multi.unseen);
        if !had_bubble && multi.entries.len() > 1 {
            multi.bubbles_at = now;
        }
        if multi.entries.len() <= 2 {
            multi.strip = false;
        }
        let first = multi.entries.first().map_or(multi.key, |e| e.key);
        self.switch_key(first);
    }

    /// A pessoa escolheu este widget (clicou na bolha ou passou o mouse no aviso): ele vai para
    /// a pílula. `sticky`: fica lá até ela escolher outro.
    fn choose(&mut self, key: &'static str, sticky: bool) {
        let multi = &mut self.multi;
        multi.pinned = Some((key, sticky));
        multi.touched.insert(key, Instant::now());
        multi.unseen.retain(|k| *k != key);
        if multi.banner.is_some_and(|(k, _)| k == key) {
            multi.banner = None;
        }
        rank(&mut multi.entries, &multi.touched, multi.pinned.map(|(k, _)| k), &multi.unseen);
        multi.strip = false;
        self.switch_key(key);
    }

    /// A roda girou `steps` passos (positivo: para cima). Quando ela completa um passo, devolve a
    /// direção da troca: 1 (o próximo, rolando para baixo, como na pilha) ou -1 (o anterior). Uma
    /// troca por vez: a roda que continua girando durante o esmaecer não pula outro.
    fn wheel_turn(&mut self, steps: f32, now: Instant) -> Option<i32> {
        let multi = &mut self.multi;
        if now.saturating_duration_since(multi.wheel_at).as_secs_f32() * 1000.0 < LAYER_FADE_MS * 0.8 {
            multi.wheel = 0.0;
            return None;
        }
        multi.wheel += steps;
        if multi.wheel.abs() < WHEEL_STEP {
            return None;
        }
        let dir = if multi.wheel < 0.0 { 1 } else { -1 };
        multi.wheel = 0.0;
        multi.wheel_at = now;
        Some(dir)
    }

    /// Põe na pílula o widget seguinte (`dir` = 1) ou o anterior (-1), na ordem fixa e dando a
    /// volta. Fica lá até a pessoa escolher outro, como no clique na bolha; o aviso esticando a
    /// ilha recolhe (ela já está mexendo). Com um só na ilha, não faz nada (devolve false).
    fn cycle(&mut self, dir: i32) -> bool {
        let order = &self.multi.order;
        if order.len() < 2 {
            return false;
        }
        let current = self.key_for(self.shape);
        let at = order.iter().position(|k| *k == current).unwrap_or(0) as i32;
        let next = order[(at + dir).rem_euclid(order.len() as i32) as usize];
        self.multi.banner = None;
        self.choose(next, true);
        true
    }

    /// Desliza (animando) para a âncora dada, com a escala do monitor de lá.
    fn glide_to(&mut self, anchor: (f32, f32), docked: bool) {
        self.from = self.geo();
        self.anchor = anchor;
        self.docked = docked;
        self.s = island_scale(anchor.0 as i32, anchor.1 as i32 + 10);
        self.to = self.target(self.shape);
        self.anim_at = Instant::now();
        self.anim_ms = STATE_MS;
        self.next_frame = Instant::now();
    }

    /// A cor do widget `key` na ilha (o contorno do aviso, o brilho ao nascer).
    fn accent_of(&self, key: &str) -> Rgba {
        self.multi.entries.iter().find(|e| e.key == key).map_or(WHITE, |e| e.item.accent)
    }

    /// A capa reduzida para `px` pixels (guardada: reduzir a cada quadro custaria caro).
    fn art(&mut self, art: &Arc<Image>, px: u32) -> Image {
        let key = Arc::as_ptr(art) as usize;
        if self.art_cache.as_ref().map(|c| (c.0, c.1)) != Some((key, px)) {
            self.art_cache = Some((key, px, media::cover_image(art, px, px)));
        }
        self.art_cache.as_ref().unwrap().2.clone()
    }

    /// A pílula (e as bolhas) desenhada, e onde fica a janela na tela. A janela é a pílula e as
    /// bolhas com uma margem em volta (a sombra), e nada mais: fora dela, os cliques passam.
    ///
    /// Nas funções de desenho, (ox, oy) é o canto da pílula na tela de desenho e (lw, lh) o
    /// tamanho dela, em pontos; `lr` é o raio dos cantos.
    fn render(&mut self, scene: &Scene) -> (Image, (i32, i32)) {
        let pill = self.geo();
        let s = self.s;
        let bubbles = self.bubbles();
        let (wx, wy, ww, wh) = window_bounds(pill, &bubbles, (PAD * s).round());
        let mut c = Canvas::new(ww, wh, s);
        let (ox, oy, lw, lh, lr) = ((pill.x - wx) / s, (pill.y - wy) / s, pill.w / s, pill.h / s, pill.r / s);
        self.draw_pill_body(&mut c, ox, oy, lw, lh, lr);
        c.push_round_clip(ox, oy, ox + lw, oy + lh, lr);
        // o conteúdo: na troca de forma ou de widget, o de antes apaga enquanto o novo acende
        let (prev_alpha, cur_alpha) = self.content_alphas();
        let (prev_key, cur_key) = (self.multi.prev_key, self.key_for(self.shape));
        if prev_alpha > 0.01 && (self.prev_shape != self.shape || prev_key != cur_key) {
            self.draw_layer(&mut c, self.prev_shape, prev_key, scene, ox, oy, lw, lh, prev_alpha);
        }
        if cur_alpha > 0.01 {
            self.draw_layer(&mut c, self.shape, cur_key, scene, ox, oy, lw, lh, cur_alpha);
        }
        if let (Some(info), Morph::None, true, None) = (scene.info, self.morph, cur_key == KEY, self.multi.pop) {
            self.draw_volume(&mut c, info, ox, oy, lw, lh);
        }
        self.draw_card_face(&mut c, ox, oy, lw, lh);
        c.pop_clip();
        let banner_alpha = if self.shape == Shape::Banner { cur_alpha } else { prev_alpha };
        self.draw_banner_outline(&mut c, ox, oy, lw, lh, lr, banner_alpha);
        self.draw_appear_glow(&mut c, ox, oy, lw, lh, lr);
        self.draw_bubbles(&mut c, &bubbles, scene, wx, wy);
        (c.finish(), (wx as i32, wy as i32))
    }

    /// A opacidade do conteúdo (de antes, de agora): a troca entre as camadas, e ainda menos
    /// nascendo ou sumindo (o conteúdo acende depois e apaga antes da pílula).
    fn content_alphas(&self) -> (f32, f32) {
        let (prev, cur) = self.layer_alpha();
        let (_, pop) = self.pop_alpha();
        (prev * pop, cur * pop)
    }

    /// O corpo da pílula: uma sombra curta (a ilha fica por cima das janelas, então nada de
    /// sombra longa capturando cliques), o fundo preto (na transformação, passando da cor do
    /// cartão para o preto) e um fio claro na borda.
    #[allow(clippy::too_many_arguments)]
    fn draw_pill_body(&self, c: &mut Canvas, ox: f32, oy: f32, lw: f32, lh: f32, lr: f32) {
        for (spread, alpha) in [(4.0, 22u8), (2.0, 40)] {
            c.fill_round_rect(ox - spread, oy - spread + 2.0, ox + lw + spread, oy + lh + spread + 2.0, lr + spread, Rgba(0, 0, 0, alpha));
        }
        let progress = self.progress();
        let background = match self.morph {
            Morph::In => self.card_color.mix(BLACK, ease_in_out((progress * 1.4).min(1.0))),
            Morph::Out => BLACK.mix(self.card_color, ease_in_out(progress)),
            Morph::None => BLACK,
        };
        c.fill_round_rect(ox, oy, ox + lw, oy + lh, lr, background);
        c.stroke_round_rect(ox, oy, ox + lw, oy + lh, lr, WHITE.with_a(26), 1.0 / self.s);
    }

    /// Na transformação, o cartão de verdade por cima, do tamanho da pílula de agora (sem
    /// deformar: cobre e centraliza). Na volta, termina igual ao cartão que reaparece no lugar.
    fn draw_card_face(&self, c: &mut Canvas, ox: f32, oy: f32, lw: f32, lh: f32) {
        let alpha = self.face_alpha();
        let Some(face) = self.card_face.as_ref().filter(|_| alpha > 0.01) else { return };
        let s = self.s;
        let (box_w, box_h) = (lw * s, lh * s);
        let cover = (box_w / face.w as f32).max(box_h / face.h as f32);
        let (face_w, face_h) = (face.w as f32 * cover, face.h as f32 * cover);
        c.image_px(face, (ox * s + (box_w - face_w) / 2.0).round(), (oy * s + (box_h - face_h) / 2.0).round(), face_w.round(), face_h.round(), alpha);
    }

    /// Esticada com um aviso: um contorno na cor de quem avisou.
    #[allow(clippy::too_many_arguments)]
    fn draw_banner_outline(&self, c: &mut Canvas, ox: f32, oy: f32, lw: f32, lh: f32, lr: f32, alpha: f32) {
        if self.shape != Shape::Banner && self.prev_shape != Shape::Banner {
            return;
        }
        let color = self.accent_of(self.key_for(Shape::Banner));
        c.stroke_round_rect(ox + 0.75, oy + 0.75, ox + lw - 0.75, oy + lh - 0.75, lr - 0.75, color.alpha(0.75 * alpha), 1.5);
    }

    /// Nascendo: um brilho na cor de quem chegou, que acende com a gota e apaga quando a pílula
    /// chega no tamanho.
    #[allow(clippy::too_many_arguments)]
    fn draw_appear_glow(&self, c: &mut Canvas, ox: f32, oy: f32, lw: f32, lh: f32, lr: f32) {
        if !self.appearing() {
            return;
        }
        let glow = (self.pop_progress() * std::f32::consts::PI).sin().max(0.0);
        let color = self.accent_of(self.key_for(self.shape));
        // um contorno firme e outro, por fora, mais fraco
        for (spread, strength) in [(0.0, 0.85), (1.75, 0.3)] {
            c.stroke_round_rect(ox + 0.75 - spread, oy + 0.75 - spread, ox + lw - 0.75 + spread, oy + lh - 0.75 + spread, lr - 0.75 + spread, color.alpha(strength * glow), 1.5);
        }
    }

    /// As bolhas: o widget em miniatura, o pontinho do aviso não visto e quantos mais esperam.
    fn draw_bubbles(&mut self, c: &mut Canvas, bubbles: &[(f32, f32, f32)], scene: &Scene, wx: f32, wy: f32) {
        let s = self.s;
        let fade = (self.multi.bubbles_at.elapsed().as_secs_f32() / 0.28).clamp(0.0, 1.0);
        let list = self.bubble_entries();
        let waiting = list.len().saturating_sub(1);
        for (index, &(cx, cy, r)) in bubbles.iter().enumerate() {
            let Some(entry) = list.get(index).copied() else { continue };
            // a bolha na tela de desenho, em pontos: centro (bx, by) e raio
            let (bx, by, br) = ((cx - wx) / s, (cy - wy) / s, r / s);
            // a primeira entra esmaecendo; as de trás (abertas com o mouse) já entram inteiras
            let alpha = if index == 0 { ease_in_out(fade) } else { 1.0 };
            c.push_opacity(alpha);
            c.fill_circle(bx, by + 2.0, br + 3.0, Rgba(0, 0, 0, 30));
            c.fill_circle(bx, by + 1.0, br + 1.5, Rgba(0, 0, 0, 44));
            c.fill_circle(bx, by, br, BLACK);
            let hot = self.hot == Some(Btn::Bubble(index));
            c.stroke_circle(bx, by, br - 0.5, WHITE.with_a(if hot { 70 } else { 26 }), if hot { 1.5 } else { 1.0 / s });
            if entry.key == KEY {
                // a música: a capa redonda (guardada no tamanho da bolha)
                if let Some(art) = scene.info.and_then(|i| i.art.clone()) {
                    let diameter = br * 2.0 - 12.0;
                    let px = (diameter * s).round() as u32;
                    let id = Arc::as_ptr(&art) as usize;
                    if self.multi.bubble_art.as_ref().map(|cached| (cached.0, cached.1)) != Some((id, px)) {
                        self.multi.bubble_art = Some((id, px, media::cover_image(&art, px, px)));
                    }
                    let (left, top) = (bx - diameter / 2.0, by - diameter / 2.0);
                    c.push_round_clip(left, top, left + diameter, top + diameter, diameter / 2.0);
                    c.image_px(&self.multi.bubble_art.as_ref().unwrap().2, (left * s).round(), (top * s).round(), px as f32, px as f32, 1.0);
                    c.pop_clip();
                } else {
                    let col = scene.info.map_or(WHITE, |i| i.colors[0]);
                    c.fill_circle(bx, by, br - 6.0, col);
                }
            } else if let Some(kind) = scene.kind(entry.key) {
                kind.island_draw(c, IslandForm::Bubble, (bx - br, by - br, br * 2.0, br * 2.0), None, &scene.env(entry.key));
            }
            // um aviso que ninguém viu: o pontinho na cor dele, no alto à direita
            if self.multi.unseen.contains(&entry.key) {
                let (dot_x, dot_y) = (bx + br * 0.72, by - br * 0.72);
                c.fill_circle(dot_x, dot_y, 5.5, BLACK);
                c.fill_circle(dot_x, dot_y, 4.0, entry.item.accent);
            }
            // quantos mais esperam, no alto à esquerda (com o mouse em cima, todos abrem)
            if index == 0 && waiting > 0 && !self.multi.strip {
                let (tag_x, tag_y) = (bx - br * 0.72, by - br * 0.72);
                let label = format!("+{waiting}");
                let tag_w = c.text_width(&label, Face::Semibold, 10.0).max(9.0) + 8.0;
                c.fill_round_rect(tag_x - tag_w / 2.0 - 1.5, tag_y - 9.0, tag_x + tag_w / 2.0 + 1.5, tag_y + 9.0, 9.0, BLACK);
                c.fill_round_rect(tag_x - tag_w / 2.0, tag_y - 7.5, tag_x + tag_w / 2.0, tag_y + 7.5, 7.5, ACCENT);
                c.text(tag_x, tag_y, &label, Face::Semibold, 10.0, WHITE, Anchor::MM, None);
            }
            c.pop_clip();
        }
    }

    /// Uma camada do conteúdo da pílula: o widget `key` na forma `shape`, com opacidade `alpha`
    /// (na troca de forma ou de widget, a camada de antes e a nova se cruzam).
    #[allow(clippy::too_many_arguments)]
    fn draw_layer(&mut self, c: &mut Canvas, shape: Shape, key: &'static str, scene: &Scene, ox: f32, oy: f32, lw: f32, lh: f32, alpha: f32) {
        if key != KEY {
            let Some(kind) = scene.kind(key) else { return };
            let hot = match self.hot {
                Some(Btn::Widget(action)) if shape == self.shape => Some(action),
                _ => None,
            };
            c.push_opacity(alpha);
            kind.island_draw(c, shape.form(), (ox, oy, lw, lh), hot, &scene.env(key));
            c.pop_clip();
            return;
        }
        let Some(info) = scene.info else {
            c.text(ox + lw / 2.0, oy + lh / 2.0, "Nada tocando", Face::Regular, 12.0, faded(MUTED, alpha), Anchor::MM, None);
            return;
        };
        match shape {
            // a música não tem forma de aviso nem de gota: fica como fechada
            Shape::Dot | Shape::Compact | Shape::Banner => self.draw_media_compact(c, info, ox, oy, lw, lh, alpha),
            Shape::Medium => self.draw_media_medium(c, info, ox, oy, lw, lh, alpha),
            Shape::Large => self.draw_media_large(c, info, ox, oy, lw, lh, alpha),
        }
    }

    /// O botão `b` está sob o mouse? (Na barra de progresso, vale qualquer ponto dela.)
    fn is_hot(&self, b: Btn) -> bool {
        self.hot.is_some_and(|hot| std::mem::discriminant(&hot) == std::mem::discriminant(&b))
    }

    /// A capa num quadrado de lado `size` com cantos `radius` (sem capa, a cor dela).
    #[allow(clippy::too_many_arguments)]
    fn draw_cover(&mut self, c: &mut Canvas, info: &IslandInfo, x: f32, y: f32, size: f32, radius: f32, alpha: f32) {
        let s = c.s;
        c.push_round_clip(x, y, x + size, y + size, radius);
        match &info.art {
            Some(img) => {
                let px = (size * s).round() as u32;
                let fitted = self.art(img, px);
                c.image_px(&fitted, (x * s).round(), (y * s).round(), px as f32, px as f32, alpha);
            }
            None => c.fill_round_rect(x, y, x + size, y + size, radius, faded(info.colors[0], alpha)),
        }
        c.pop_clip();
    }

    /// A música fechada: a capa e as ondas.
    #[allow(clippy::too_many_arguments)]
    fn draw_media_compact(&mut self, c: &mut Canvas, info: &IslandInfo, ox: f32, oy: f32, lw: f32, lh: f32, alpha: f32) {
        self.draw_cover(c, info, ox + 8.0, oy + (lh - 26.0) / 2.0, 26.0, 7.0, alpha);
        self.draw_eq(c, ox + lw - 14.0 - 27.0, oy + lh / 2.0, 18.0, info, alpha);
    }

    /// A música com o mouse em cima: a capa, o nome e o artista, tocar/pausar e avançar.
    #[allow(clippy::too_many_arguments)]
    fn draw_media_medium(&mut self, c: &mut Canvas, info: &IslandInfo, ox: f32, oy: f32, lw: f32, lh: f32, alpha: f32) {
        self.draw_cover(c, info, ox + 14.0, oy + (lh - 50.0) / 2.0, 50.0, 12.0, alpha);
        let text_w = lw - 76.0 - 98.0;
        c.text(ox + 76.0, oy + lh / 2.0 - 2.0, &info.title, Face::Semibold, 14.5, faded(FG, alpha), Anchor::LS, Some(text_w));
        c.text(ox + 76.0, oy + lh / 2.0 + 3.0, &info.artist, Face::Regular, 12.5, faded(MUTED, alpha), Anchor::LA, Some(text_w));
        for (b, r) in buttons(Shape::Medium, lw, lh) {
            let (cx, cy) = (ox + (r.0 + r.2) / 2.0, oy + (r.1 + r.3) / 2.0);
            if self.is_hot(b) {
                c.fill_circle(cx, cy, (r.2 - r.0) / 2.0, faded(WHITE.with_a(34), alpha));
            }
            match b {
                Btn::Toggle if info.playing => drawing::pause(c, cx, cy, 16.0, faded(FG, alpha)),
                Btn::Toggle => drawing::play(c, cx, cy, 16.0, faded(FG, alpha)),
                _ => drawing::skip(c, cx, cy, 15.0, faded(if info.can_next { FG } else { FAINT }, alpha), true),
            }
        }
    }

    /// A música aberta: a capa, o nome, o artista e o app, as ondas, a barra de progresso,
    /// voltar, tocar/pausar e avançar, "Abrir no ..." e "Voltar para a área de trabalho".
    #[allow(clippy::too_many_arguments)]
    fn draw_media_large(&mut self, c: &mut Canvas, info: &IslandInfo, ox: f32, oy: f32, lw: f32, lh: f32, alpha: f32) {
        let fade = |color: Rgba| faded(color, alpha);
        self.draw_cover(c, info, ox + 20.0, oy + 18.0, 60.0, 14.0, alpha);
        let text_w = lw - 92.0 - 60.0;
        c.text(ox + 92.0, oy + 22.0, &info.title, Face::Semibold, 15.0, fade(FG), Anchor::LA, Some(text_w));
        c.text(ox + 92.0, oy + 42.0, &info.artist, Face::Regular, 12.5, fade(MUTED), Anchor::LA, Some(text_w));
        c.fill_circle(ox + 95.5, oy + 66.5, 3.5, fade(info.colors[0]));
        c.text(ox + 104.0, oy + 66.5, &info.app, Face::Regular, 11.5, fade(FAINT), Anchor::LM, None);
        self.draw_eq(c, ox + lw - 20.0 - 27.0, oy + 34.0, 18.0, info, alpha);
        // a barra de progresso (engrossa com o mouse em cima)
        let played = if info.duration > 0.0 { (info.position / info.duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let thick = if self.is_hot(Btn::Seek(0)) { 7.0 } else { 4.0 };
        let (bar_x0, bar_x1, bar_y) = (ox + 20.0, ox + lw - 20.0, oy + 96.0);
        c.fill_round_rect(bar_x0, bar_y - thick / 2.0, bar_x1, bar_y + thick / 2.0, thick / 2.0, fade(WHITE.with_a(64)));
        if played > 0.0 {
            let played_x = bar_x0 + ((bar_x1 - bar_x0) * played).max(thick);
            c.fill_round_rect(bar_x0, bar_y - thick / 2.0, played_x, bar_y + thick / 2.0, thick / 2.0, fade(WHITE.with_a(240)));
        }
        c.text(bar_x0, bar_y + 8.0, &fmt_time(info.position), Face::Regular, 11.0, fade(MUTED), Anchor::LA, None);
        let remaining = format!("-{}", fmt_time((info.duration - info.position).max(0.0)));
        c.text(bar_x1, bar_y + 8.0, &remaining, Face::Regular, 11.0, fade(MUTED), Anchor::RA, None);
        for (b, r) in buttons(Shape::Large, lw, lh) {
            let (cx, cy) = (ox + (r.0 + r.2) / 2.0, oy + (r.1 + r.3) / 2.0);
            let hot = self.is_hot(b);
            match b {
                Btn::Toggle => {
                    c.fill_circle(cx, cy, 23.0, fade(if hot { WHITE } else { FG }));
                    if info.playing {
                        drawing::pause(c, cx, cy, 15.0, fade(DARK_GLYPH));
                    } else {
                        drawing::play(c, cx + 1.0, cy, 15.0, fade(DARK_GLYPH));
                    }
                }
                Btn::Prev | Btn::Next => {
                    let enabled = if b == Btn::Prev { info.can_prev } else { info.can_next };
                    if hot {
                        c.fill_circle(cx, cy, 18.0, fade(WHITE.with_a(34)));
                    }
                    drawing::skip(c, cx, cy, 17.0, fade(if enabled { FG } else { FAINT }), b == Btn::Next);
                }
                Btn::Open => {
                    let color = if hot { FG } else { Rgba::rgb(143, 211, 255) };
                    c.text(ox + 20.0, cy, &format!("Abrir no {}", info.app), Face::Semibold, 12.0, fade(color), Anchor::LM, None);
                }
                Btn::Back => {
                    c.fill_round_rect(ox + r.0, oy + r.1, ox + r.2, oy + r.3, 10.0, fade(WHITE.with_a(if hot { 46 } else { 26 })));
                    c.text(cx + 8.0, cy, "Voltar para a área de trabalho", Face::Regular, 12.0, fade(FG), Anchor::MM, None);
                    // uma seta para baixo, entrando na área de trabalho
                    let arrow_x = ox + r.0 + 14.0;
                    c.line(&[(arrow_x, cy - 5.0), (arrow_x, cy + 4.0)], fade(FG), 1.5);
                    c.line(&[(arrow_x - 3.5, cy + 0.5), (arrow_x, cy + 4.0), (arrow_x + 3.5, cy + 0.5)], fade(FG), 1.5);
                }
                Btn::Seek(_) | Btn::Widget(_) | Btn::Bubble(_) => {}
            }
        }
    }

    /// A roda do mouse mudou o volume: uma barra com o alto-falante, por cima do que fica no meio
    /// da pílula (no meio da fechada; no lugar do título da média; no lugar do app da aberta).
    fn draw_volume(&self, c: &mut Canvas, info: &IslandInfo, ox: f32, oy: f32, lw: f32, lh: f32) {
        let (Some(level), shown) = (info.volume, info.volume_hud) else { return };
        if shown <= 0.01 {
            return;
        }
        // (de onde a onde vai a barra, a altura dela e o bloco de texto que ela cobre)
        let (x0, x1, cy, top, bottom) = match self.shape {
            Shape::Dot | Shape::Compact | Shape::Banner => (ox + 42.0, ox + lw - 50.0, oy + lh / 2.0, oy + lh / 2.0 - 11.0, oy + lh / 2.0 + 11.0),
            Shape::Medium => (ox + 76.0, ox + lw - 104.0, oy + lh / 2.0, oy + 8.0, oy + lh - 8.0),
            Shape::Large => (ox + 92.0, ox + lw - 64.0, oy + 58.0, oy + 40.0, oy + 76.0),
        };
        // o fundo preto da pílula por cima do texto, que some enquanto a barra aparece
        c.fill_round_rect(x0 - 6.0, top, x1 + 6.0, bottom, 8.0, BLACK.alpha(shown));
        let fade = |color: Rgba| faded(color, shown);
        c.text(x0 + 7.0, cy, "\u{e995}", Face::Icons, 12.0, fade(FG), Anchor::MM, None);
        let (bx0, bx1) = (x0 + 22.0, x1);
        c.fill_round_rect(bx0, cy - 2.0, bx1, cy + 2.0, 2.0, fade(WHITE.with_a(64)));
        c.fill_round_rect(bx0, cy - 2.0, bx0 + ((bx1 - bx0) * level.clamp(0.0, 1.0)).max(4.0), cy + 2.0, 2.0, fade(WHITE.with_a(240)));
    }

    /// Cinco ondas no ritmo do som (alternando as duas cores da capa).
    fn draw_eq(&self, c: &mut Canvas, x: f32, cy: f32, max_h: f32, info: &IslandInfo, alpha: f32) {
        for (i, level) in self.levels.iter().enumerate() {
            let bar_h = (4.0 + (max_h - 4.0) * level.clamp(0.0, 1.0)).max(3.0);
            let color = info.colors[i % 2];
            let bar_x = x + i as f32 * 6.0;
            c.fill_round_rect(bar_x, cy - bar_h / 2.0, bar_x + 3.0, cy + bar_h / 2.0, 1.5, Rgba(color.0, color.1, color.2, (255.0 * alpha) as u8));
        }
    }

    /// Some em tela cheia no monitor dela (`paused`: os monitores com jogo ou vídeo em tela
    /// cheia) e volta quando acaba. No modo foco, fica (só com a hora e o dia).
    fn hide_in_fullscreen(&mut self, paused: &[Rect]) {
        let pill = self.geo();
        let (cx, cy) = ((pill.x + pill.w / 2.0) as i32, (pill.y + pill.h / 2.0) as i32);
        let hidden = paused.iter().any(|r| cx >= r.0 && cx < r.2 && cy >= r.1 && cy < r.3);
        if hidden != self.hidden {
            self.hidden = hidden;
            self.drawn = None;
            events::show(self.hwnd, !hidden);
            if !hidden {
                win::set_z(self.hwnd, Z::Topmost);
            }
        }
        if !self.hidden {
            // clicada, a barra de tarefas sobe por cima de tudo: a ilha volta para cima dela
            win::stay_over_taskbar(self.hwnd);
        }
    }

    /// Na barra, acompanha o botão Iniciar (ele anda quando abrem e fecham apps): de tempos em
    /// tempos, vê se o lugar mudou e desliza até lá.
    fn follow_start_button(&mut self, now: Instant) {
        if !self.docked || self.morph != Morph::None || self.dragging || self.vanishing() || now < self.dock_check {
            return;
        }
        self.dock_check = now + DOCK_CHECK;
        let Some((slot, scale)) = dock_slot(self.anchor.0 as i32, self.anchor.1 as i32 + 10) else { return };
        let moved = (slot.0 - self.anchor.0).abs() >= 1.0 || (slot.1 - self.anchor.1).abs() >= 1.0 || (scale - self.s).abs() > 0.01;
        if moved {
            self.from = self.geo();
            self.anchor = slot;
            self.s = scale;
            self.to = self.target(self.shape);
            self.anim_at = now;
            self.anim_ms = STATE_MS;
        }
    }

    /// O mouse nas bolhas: com mais gente esperando, todas abrem; longe, fecham de novo. Devolve
    /// se o mouse está nelas.
    fn follow_pointer_on_bubbles(&mut self, px: i32, py: i32, now: Instant) -> bool {
        let on_bubbles = self.bubble_at(px, py).is_some() || (self.multi.strip && self.near_bubbles(px, py));
        if on_bubbles {
            self.multi.strip_out = None;
            let lingered = self.inside_since.is_some_and(|t| t.elapsed() >= HOVER_IN);
            if !self.multi.strip && self.bubble_entries().len() > 1 && lingered {
                self.multi.strip = true;
            }
        } else if self.multi.strip && self.multi.strip_out.get_or_insert(now).elapsed() >= HOVER_OUT {
            self.multi.strip = false;
            self.multi.strip_out = None;
        }
        on_bubbles
    }

    /// O mouse na pílula: com ele em cima um instante, ela estica (a forma média); longe, ou com
    /// um clique fora, recolhe. A gota só abre com cliques. Devolve true se a escolha feita só
    /// por passar o mouse acabou (a ordem da ilha volta ao normal).
    fn follow_pointer_on_pill(&mut self, over: bool, on_bubbles: bool, now: Instant) -> bool {
        if over || on_bubbles {
            self.outside_since = None;
            self.inside_since.get_or_insert(now);
        } else {
            self.inside_since = None;
        }
        if over {
            let since = self.inside_since.unwrap_or(now);
            let only_by_click = self.rest_item().is_some_and(|i| i.dot);
            if !only_by_click && matches!(self.shape, Shape::Compact | Shape::Banner) && since.elapsed() >= HOVER_IN {
                // passou o mouse no aviso: ele fica na pílula enquanto ela está aberta
                if self.shape == Shape::Banner {
                    if let Some((key, _)) = self.multi.banner {
                        self.choose(key, false);
                    }
                }
                self.set_shape(Shape::Medium);
            }
            // o que está na pílula aberta foi visto
            if matches!(self.shape, Shape::Medium | Shape::Large) {
                let key = self.multi.key;
                self.multi.unseen.retain(|k| *k != key);
            }
            return false;
        }
        let since = *self.outside_since.get_or_insert(now);
        // a gota aberta com um clique volta a ser gota com o mouse longe
        if self.opened_dot && self.shape.closed() && since.elapsed() >= DOT_BACK {
            self.opened_dot = false;
        }
        let clicked_away = win::key_down(VK_LBUTTON) || win::key_down(VK_RBUTTON);
        let collapse = match self.shape {
            Shape::Medium => since.elapsed() >= HOVER_OUT,
            Shape::Large => clicked_away || since.elapsed() >= LARGE_IDLE,
            _ => false,
        };
        if !collapse {
            return false;
        }
        self.set_shape(self.closed_shape());
        // a escolha só por passar o mouse vale enquanto ela estava aberta
        if self.multi.pinned.is_some_and(|(_, sticky)| !sticky) {
            self.multi.pinned = None;
            return true;
        }
        false
    }

    /// Um aviso chegando estica a ilha por alguns segundos (com o mouse em cima, não: ela já
    /// está aberta); depois, ela volta à forma fechada de quem está nela (a pílula ou a gota).
    fn stretch_for_alerts(&mut self, over: bool) {
        match self.multi.banner {
            Some((_, at)) if at.elapsed() < BANNER_FOR => {
                if self.shape.closed() && !over {
                    self.set_shape(Shape::Banner);
                }
            }
            Some(_) => {
                self.multi.banner = None;
                if self.shape == Shape::Banner {
                    self.set_shape(self.closed_shape());
                }
            }
            None if self.shape == Shape::Banner => self.set_shape(self.closed_shape()),
            None if self.shape.closed() && self.shape != self.closed_shape() => self.set_shape(self.closed_shape()),
            None => {}
        }
    }

    /// O PC parado: a ilha em repouso apaga devagar, e acende de novo no primeiro movimento.
    fn step_dim(&mut self) {
        let target = self.dim_target();
        self.dim = if target < self.dim {
            (self.dim - DIM_OUT_STEP).max(target)
        } else {
            (self.dim + DIM_IN_STEP).min(target)
        };
    }

    /// Ondas do próximo quadro: seguem o som de verdade quando dá para medir.
    fn step_levels(&mut self, info: Option<&IslandInfo>, peak: Option<f32>) {
        let secs = self.started.elapsed().as_secs_f32();
        let playing = info.is_some_and(|i| i.playing);
        for i in 0..5 {
            // sem medir o som, cada onda sobe e desce no seu ritmo
            let wave = 0.5 + 0.5 * (secs * (5.2 + i as f32 * 1.7) + i as f32 * 1.3).sin();
            let target = match (playing, peak) {
                (false, _) => 0.08,
                (true, Some(p)) => ((p * 1.8).min(1.0) * (0.45 + 0.55 * wave)).max(0.08),
                (true, None) => 0.25 + 0.65 * wave,
            };
            // sobe rápido e desce devagar, como um medidor
            let follow = if target > self.levels[i] { 0.6 } else { 0.25 };
            self.levels[i] += (target - self.levels[i]) * follow;
        }
    }
}

impl App {
    pub(super) fn island_owns(&self, hwnd: isize) -> bool {
        self.island.as_ref().is_some_and(|i| i.hwnd.0 as isize == hwnd)
    }

    /// O Tocando agora está na ilha (e o cartão dele, escondido)?
    pub(super) fn media_in_island(&self) -> bool {
        self.island.as_ref().is_some_and(|i| i.media)
    }

    fn media_index(&self) -> Option<usize> {
        self.find(KEY)
    }

    fn island_info(&self) -> (Option<IslandInfo>, Option<f32>) {
        match self.media_index() {
            Some(j) if self.media_in_island() => (self.cards[j].kind.island(), self.cards[j].kind.sound_level()),
            _ => (None, None),
        }
    }

    /// O espaço que a ilha ocupa aberta (do tamanho grande), para a arrumação deixar livre.
    pub(super) fn island_keep_clear(&self) -> Option<Rect> {
        let isl = self.island.as_ref().filter(|i| !i.hidden)?;
        let g = isl.target(Shape::Large);
        Some((g.x.floor() as i32, g.y.floor() as i32, (g.x + g.w).ceil() as i32, (g.y + g.h).ceil() as i32))
    }

    /// No alto e no meio do monitor onde está o retângulo.
    fn island_default_anchor(&self, near: Rect, s: f32) -> (f32, f32) {
        let work = win::area_for(near);
        ((work.0 + work.2) as f32 / 2.0, work.1 as f32 + TOP_GAP * s)
    }

    /// O lugar padrão (âncora, na barra?): na barra de tarefas do monitor onde está o retângulo;
    /// sem barra à vista (escondida sozinha), no alto e no meio.
    fn island_home(&self, near: Rect, s: f32) -> ((f32, f32), bool) {
        let work = win::area_for(near);
        match dock_slot((work.0 + work.2) / 2, work.3 - 1) {
            Some((anchor, _)) => (anchor, true),
            None => (self.island_default_anchor(near, s), false),
        }
    }

    /// Onde a ilha abre: o lugar guardado (se ainda está numa tela) ou o padrão perto de `near`.
    fn island_start(&self, near: Rect, s: f32) -> ((f32, f32), bool) {
        let saved = self.sh.cfg[KEY]["ilha_pos"].as_array().and_then(|a| Some((a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32)));
        match saved {
            Some((x, y)) if win::on_screen(x as i32, (y + 10.0) as i32) => ((x, y), false),
            _ => self.island_home(near, s),
        }
    }

    /// Leva a ilha (animando) para a âncora dada e guarda o lugar.
    fn move_island(&mut self, anchor: (f32, f32), docked: bool) {
        if let Some(isl) = &mut self.island {
            isl.glide_to(anchor, docked);
        }
        self.save_island_pos();
    }

    /// Com "Seguir a tela em uso" ligado: a janela em uso ficou um instante em outro monitor, a
    /// ilha vai para lá, no mesmo lugar relativo (na barra de tarefas, na barra de lá). Só
    /// fechada e sem ninguém mexendo nela, para não fugir do mouse. Não guarda o lugar: o lugar
    /// guardado continua o que a pessoa escolheu.
    fn follow_active_screen(&mut self, now: Instant) {
        if !follows_screen(&self.sh.cfg) {
            return;
        }
        let Some(isl) = &mut self.island else { return };
        if now < isl.screen_check {
            return;
        }
        isl.screen_check = now + SCREEN_CHECK;
        let (px, py) = win::pointer();
        let busy = isl.morph != Morph::None || isl.dragging || isl.press.is_some() || isl.vanishing() || isl.hidden || !isl.shape.closed();
        let (here, from) = win::monitor_at(isl.anchor.0 as i32, isl.anchor.1 as i32 + 10);
        let Some(active) = win::foreground_screen().filter(|screen| *screen != here) else {
            isl.screen_pending = None;
            return;
        };
        let since = match isl.screen_pending {
            Some((screen, since)) if screen == active => since,
            _ => {
                isl.screen_pending = Some((active, now));
                now
            }
        };
        if busy || isl.geo().contains(px, py) || now.saturating_duration_since(since) < SCREEN_SETTLE {
            return;
        }
        isl.screen_pending = None;
        let to = win::area_for(active);
        let slot = if isl.docked { dock_slot((to.0 + to.2) / 2, to.3 - 1) } else { None };
        let (anchor, docked) = match slot {
            Some((slot, _)) => (slot, true),
            None => {
                let s_to = island_scale((to.0 + to.2) / 2, (to.1 + to.3) / 2);
                (anchor_on(isl.anchor, isl.s, from, to, s_to), false)
            }
        };
        isl.glide_to(anchor, docked);
    }

    /// Uma ilha nova (sem janela na tela ainda), fechada, na âncora dada.
    fn new_island(&self, anchor: (f32, f32), docked: bool, media: bool) -> Island {
        let s = island_scale(anchor.0 as i32, (anchor.1 + 10.0) as i32);
        let now = Instant::now();
        let mut isl = Island {
            hwnd: events::create_layered(CARD_CLASS, false),
            shape: Shape::Compact,
            prev_shape: Shape::Compact,
            shape_at: now - Duration::from_secs(1),
            from: Geo { x: 0.0, y: 0.0, w: 1.0, h: 1.0, r: 0.0 },
            to: Geo { x: 0.0, y: 0.0, w: 1.0, h: 1.0, r: 0.0 },
            anim_at: now,
            anim_ms: 0.0,
            morph: Morph::None,
            anchor,
            s,
            docked,
            dock_check: now,
            press: None,
            dragging: false,
            hot: None,
            inside_since: None,
            outside_since: None,
            hidden: false,
            quiet_since: None,
            levels: [0.1; 5],
            started: now,
            next_frame: now,
            card_color: Rgba::rgb(40, 30, 60),
            art_cache: None,
            card_face: None,
            media,
            multi: Multi::new(now),
            dim: 1.0,
            drawn: None,
            opened_dot: false,
            screen_check: now,
            screen_pending: None,
        };
        isl.to = isl.target(Shape::Compact);
        isl.from = isl.to;
        isl
    }

    /// O Tocando agora vira a ilha (`animate`: a transformação a partir do cartão). Com a ilha
    /// já na tela (com outros widgets), a música entra nela.
    pub(super) fn open_island(&mut self, animate: bool) {
        let Some(j) = self.media_index() else { return };
        if let Some(isl) = &mut self.island {
            if !isl.media {
                isl.media = true;
                isl.quiet_since = None;
                isl.multi.touched.insert(KEY, Instant::now());
                isl.multi.pinned = Some((KEY, true));
                self.tooltip.hide();
                events::show(self.cards[j].hwnd, false);
                self.sh.cfg.obj_mut(KEY).insert("ilha".into(), json!(true));
                self.sh.save();
                self.island_sync_at = Instant::now();
                self.island_sync();
            }
            return;
        }
        let card_rect = self.cards[j].card_rect();
        let (anchor, docked) = self.island_start(card_rect, self.cards[j].s);
        let info = self.cards[j].kind.island();
        let animate = animate && self.sh.animations;
        let mut isl = self.new_island(anchor, docked, true);
        isl.card_color = info.as_ref().map(|i| i.colors[0].mix(BLACK, 0.55)).unwrap_or(Rgba::rgb(40, 30, 60));
        if animate {
            isl.card_face = Some(self.cards[j].face(&self.sh));
            isl.anim_ms = MORPH_MS;
            isl.morph = Morph::In;
            let r = self.cards[j].radius_px(self.cards[j].mode);
            isl.from = Geo { x: card_rect.0 as f32, y: card_rect.1 as f32, w: (card_rect.2 - card_rect.0) as f32, h: (card_rect.3 - card_rect.1) as f32, r };
        }
        let hwnd = isl.hwnd;
        {
            let scene = Scene::of(info.as_ref(), &self.cards, &self.feeds, &self.sh.cfg);
            let (img, pos) = isl.render(&scene);
            win::blit(hwnd, &img, Some(pos), 255);
        }
        win::set_z(hwnd, Z::Topmost);
        events::show(hwnd, !self.sh.focus);
        // o cartão fica guardado (escondido) no lugar dele
        self.tooltip.hide();
        events::show(self.cards[j].hwnd, false);
        self.island = Some(isl);
        self.sh.cfg.obj_mut(KEY).insert("ilha".into(), json!(true));
        self.sh.save();
        self.island_sync_at = Instant::now();
        self.island_sync();
    }

    /// A ilha aparece sozinha (sem a música), porque algum widget tem o que mostrar: nasce
    /// como uma bolinha e estica até a pílula.
    fn open_bare_island(&mut self) {
        let near = self.cards.first().map(|c| c.card_rect()).unwrap_or_else(|| {
            let a = win::work_areas()[0];
            (a.0, a.1, a.0 + 1, a.1 + 1)
        });
        let s0 = win::scale_at(near.0, near.1);
        let (anchor, docked) = self.island_start(near, s0);
        let mut isl = self.new_island(anchor, docked, false);
        if self.sh.animations {
            // uma gota que acende e estica até a pílula, com a mola
            isl.from = isl.drop_of(isl.to, 0.5);
            isl.anim_ms = APPEAR_MS;
            isl.multi.pop = Some((Pop::Appear, isl.anim_at));
        }
        win::set_z(isl.hwnd, Z::Topmost);
        events::show(isl.hwnd, !self.sh.focus);
        self.island = Some(isl);
    }

    /// A ilha volta a ser o cartão (`animate`: voando de volta para o lugar dele). Com outros
    /// widgets na ilha, só a música sai: o cartão reaparece e a ilha continua.
    pub(super) fn close_island(&mut self, animate: bool) {
        let Some(j) = self.media_index() else {
            self.drop_island();
            return;
        };
        let others = self.island.as_ref().is_some_and(|i| i.multi.entries.iter().any(|e| e.key != KEY));
        if others {
            self.release_media();
            return;
        }
        let card_rect = self.cards[j].card_rect();
        let r = self.cards[j].radius_px(self.cards[j].mode);
        if !animate || !self.sh.animations || self.island.as_ref().is_some_and(|i| i.hidden) {
            self.finish_close();
            return;
        }
        if self.island.as_ref().is_some_and(|i| i.morph == Morph::Out) {
            return;
        }
        let face = self.cards[j].face(&self.sh);
        let Some(isl) = &mut self.island else { return };
        isl.multi.strip = false;
        isl.card_face = Some(face);
        isl.from = isl.geo();
        isl.to = Geo { x: card_rect.0 as f32, y: card_rect.1 as f32, w: (card_rect.2 - card_rect.0) as f32, h: (card_rect.3 - card_rect.1) as f32, r };
        isl.anim_at = Instant::now();
        isl.anim_ms = MORPH_MS;
        isl.morph = Morph::Out;
        isl.press = None;
        isl.dragging = false;
        isl.next_frame = Instant::now();
    }

    /// A música sai da ilha (que fica com os outros widgets) e o cartão volta ao lugar dele.
    fn release_media(&mut self) {
        if let Some(isl) = &mut self.island {
            isl.media = false;
            isl.multi.entries.retain(|e| e.key != KEY);
            if isl.multi.pinned.is_some_and(|p| p.0 == KEY) {
                isl.multi.pinned = None;
            }
            if isl.shape != Shape::Compact {
                isl.set_shape(Shape::Compact);
            }
        }
        self.sh.cfg.obj_mut(KEY).insert("ilha".into(), json!(false));
        self.sh.save();
        self.show_media_card();
        self.island_sync_at = Instant::now();
        self.island_sync();
    }

    fn show_media_card(&mut self) {
        if let Some(j) = self.media_index() {
            events::show(self.cards[j].hwnd, !self.sh.focus && !self.cards[j].stacked);
            self.cards[j].refresh(&self.sh);
            self.cards[j].present(&self.sh, true);
            self.send_to_back(j);
        }
    }

    fn finish_close(&mut self) {
        self.drop_island();
        self.sh.cfg.obj_mut(KEY).insert("ilha".into(), json!(false));
        self.sh.save();
        self.show_media_card();
        // os outros widgets podem pedir a ilha de novo
        self.island_sync_at = Instant::now();
    }

    /// Some com a ilha sem animação (o widget foi fechado, o app vai sair...).
    pub(super) fn drop_island(&mut self) {
        if let Some(isl) = self.island.take() {
            events::destroy(isl.hwnd);
        }
    }

    /// O Tocando agora foi fechado: com outros widgets na ilha, ela continua sem a música.
    pub(super) fn island_forget_media(&mut self) {
        let others = self.island.as_ref().is_some_and(|i| i.multi.entries.iter().any(|e| e.key != KEY));
        match &mut self.island {
            Some(isl) if others => {
                isl.media = false;
                isl.multi.entries.retain(|e| e.key != KEY);
            }
            _ => self.drop_island(),
        }
    }

    pub(super) fn island_deadline(&self) -> Option<Instant> {
        let frame = self.island.as_ref().map(|i| i.next_frame);
        // perguntar aos widgets só se algum pode aparecer na ilha
        let sync = config::setting_bool(&self.sh.cfg, "ilha_widgets").then_some(self.island_sync_at);
        [frame, sync].into_iter().flatten().min()
    }

    /// Mexeram no widget `key` (no cartão): se ele está na ilha, ele vai para a pílula.
    pub(super) fn island_touch(&mut self, key: &'static str) {
        if let Some(isl) = &mut self.island {
            isl.multi.touched.insert(key, Instant::now());
            if isl.multi.pinned.is_some_and(|p| p.0 != key) {
                isl.multi.pinned = None;
            }
        }
        self.island_sync_at = Instant::now();
    }

    /// Pergunta aos widgets o que eles têm para a ilha: abre a ilha quando alguém tem, fecha
    /// quando ninguém tem (e a música não está nela), e acerta quem fica na pílula.
    pub(super) fn island_sync(&mut self) {
        let now = Instant::now();
        if now < self.island_sync_at {
            return;
        }
        self.island_sync_at = now + if self.island.is_some() { SYNC_EVERY } else { SYNC_IDLE };
        self.note_media();
        let mut found: Vec<Entry> = Vec::new();
        let mut rest = None;
        if self.focusing() {
            // modo foco: a ilha só com a hora, o dia e o ícone do foco (nem avisos, nem a música)
            rest = self.feeds.iter().find(|f| f.key() == REST).and_then(|f| f.island_item(&Env { cfg: &self.sh.cfg, key: REST })).map(|item| Entry { key: REST, item });
        } else if config::setting_bool(&self.sh.cfg, "ilha_widgets") {
            for c in &self.cards {
                let key = c.key();
                if key == KEY || !widgets::on_island(&self.sh.cfg, key) {
                    continue;
                }
                let env = Env { cfg: &self.sh.cfg, key };
                if let Some(item) = c.kind.island_item(&env) {
                    found.push(Entry { key, item });
                }
            }
            // o mundo lá fora (o pôr do sol, a lua, a ISS, a cotação, o expediente)
            for f in &self.feeds {
                let key = f.key();
                if !widgets::on_island(&self.sh.cfg, key) {
                    continue;
                }
                if let Some(item) = f.island_item(&Env { cfg: &self.sh.cfg, key }) {
                    if key == REST {
                        rest = Some(Entry { key, item });
                    } else {
                        found.push(Entry { key, item });
                    }
                }
            }
        }
        if self.media_in_island() && !self.focusing() {
            let (info, _) = self.island_info();
            let mut item = IslandItem::live(info.as_ref().map_or(WHITE, |i| i.colors[0]));
            item.idle = !info.as_ref().is_some_and(|i| i.playing);
            found.push(Entry { key: KEY, item });
        }
        // a ilha em repouso: só com ninguém mais (nunca vira bolha)
        if found.is_empty() {
            found.extend(rest);
        }
        if self.island.is_none() {
            if found.is_empty() {
                return;
            }
            self.open_bare_island();
        }
        let Some(isl) = &mut self.island else { return };
        if found.is_empty() && !isl.media {
            // ninguém mais: o conteúdo apaga, a pílula encolhe numa gota e some
            if !isl.vanishing() && isl.morph == Morph::None {
                isl.start_vanish(self.sh.animations, now);
            }
            return;
        }
        if isl.vanishing() {
            isl.cancel_vanish(now);
        }
        isl.update_entries(found, now);
    }

    /// O modo foco ligou ou desligou: a ilha em repouso passa a mostrar só a hora e o dia (ou
    /// volta ao normal), já.
    pub(super) fn focus_changed(&mut self) {
        let value = match self.focus_until {
            None => "off".to_string(),
            Some(None) => "on".to_string(),
            Some(Some(t)) => win::clock_at(t),
        };
        if let Some(f) = self.feeds.iter().position(|f| f.key() == REST) {
            self.with_feed(f, |k, ctx| k.input(crate::widgets::outside::rest::FOCUS_FIELD, &value, None, ctx));
        }
        if let Some(isl) = &mut self.island {
            if isl.shape != isl.closed_shape() && !isl.shape.closed() {
                isl.set_shape(Shape::Compact);
            }
        }
        self.rest_changed();
    }

    /// Mudaram as escolhas da ilha em repouso: ela se atualiza já (sem esperar o próximo segundo).
    pub(super) fn rest_changed(&mut self) {
        if let Some(f) = self.feeds.iter().position(|f| f.key() == REST) {
            self.with_feed(f, |k, ctx| k.tick(ctx));
        }
        self.island_sync_at = Instant::now();
        self.island_sync();
        if let Some(isl) = &mut self.island {
            isl.drawn = None;
            isl.next_frame = Instant::now();
        }
    }

    /// Anota a música do Tocando agora (a "última música" da ilha em repouso), se ela mudou.
    fn note_media(&mut self) {
        let Some(j) = self.media_index() else { return };
        let info = self.cards[j].kind.island();
        if crate::widgets::outside::rest::note_track(&mut self.sh.cache, info.as_ref()) {
            self.sh.save_cache();
        }
    }

    /// Guarda onde a ilha está; na barra de tarefas não guarda nada (é o lugar padrão, e o
    /// botão Iniciar muda de lugar).
    fn save_island_pos(&mut self) {
        if let Some(isl) = &self.island {
            let (x, y) = isl.anchor;
            if isl.docked {
                self.sh.cfg.obj_mut(KEY).remove("ilha_pos");
            } else {
                self.sh.cfg.obj_mut(KEY).insert("ilha_pos".into(), json!([x.round(), y.round()]));
            }
            self.sh.save();
        }
    }

    pub(super) fn island_mouse(&mut self, kind: MouseKind, _x: i32, _y: i32, sx: i32, sy: i32) {
        let (info, _) = self.island_info();
        let focus_menu = if kind == MouseKind::Right { self.focus_menu() } else { Vec::new() };
        let scene = Scene::of(info.as_ref(), &self.cards, &self.feeds, &self.sh.cfg);
        let Some(isl) = &mut self.island else { return };
        if isl.morph == Morph::Out || isl.vanishing() {
            return;
        }
        match kind {
            MouseKind::Down => {
                isl.press = Some((sx, sy, isl.btn_at(sx, sy, &scene), isl.anchor));
            }
            MouseKind::Move => {
                if let Some((px, py, btn, start)) = isl.press {
                    let far = (((sx - px).pow(2) + (sy - py).pow(2)) as f32).sqrt() >= DRAG_START * isl.s;
                    // arrasta pela pílula ou pelas bolhas (fora dos botões da pílula)
                    let grabbable = matches!(btn, None | Some(Btn::Bubble(_)));
                    if !isl.dragging && far && grabbable {
                        isl.dragging = true;
                        isl.multi.strip = false;
                        events::set_cursor(isl.hwnd, Cursor::Move);
                    }
                    if isl.dragging {
                        let mut anchor = (start.0 + (sx - px) as f32, start.1 + (sy - py) as f32);
                        // perto do lugar na barra de tarefas, encaixa lá
                        let dock = dock_slot(sx, sy).filter(|&((dx, dy), s)| (anchor.0 - dx).abs() < DOCK_SNAP * s && (anchor.1 - dy).abs() < DOCK_SNAP * s);
                        isl.docked = dock.is_some();
                        if let Some((slot, _)) = dock {
                            isl.anchor = slot;
                            isl.snap_to_target();
                            return;
                        }
                        // perto do alto e do meio do monitor, encaixa lá
                        let (_, work) = win::monitor_at(anchor.0 as i32, anchor.1 as i32 + 10);
                        let center = (work.0 + work.2) as f32 / 2.0;
                        let top = work.1 as f32 + TOP_GAP * isl.s;
                        if (anchor.0 - center).abs() < SNAP * isl.s {
                            anchor.0 = center;
                        }
                        if (anchor.1 - top).abs() < SNAP * isl.s {
                            anchor.1 = top;
                        }
                        isl.anchor = anchor;
                        isl.snap_to_target();
                        return;
                    }
                }
                let hot = isl.btn_at(sx, sy, &scene);
                // na barra de progresso, o ponto exato não muda o desenho
                let changed = match (hot, isl.hot) {
                    (Some(Btn::Seek(_)), Some(Btn::Seek(_))) => false,
                    (a, b) => a != b,
                };
                if changed {
                    isl.hot = hot;
                    isl.next_frame = Instant::now();
                }
                if !isl.dragging {
                    events::set_cursor(isl.hwnd, if hot.is_some() { Cursor::Hand } else { Cursor::Arrow });
                }
            }
            MouseKind::Up => {
                let press = isl.press.take();
                if isl.dragging {
                    isl.dragging = false;
                    // no monitor novo, a escala dele
                    isl.s = island_scale(isl.anchor.0 as i32, isl.anchor.1 as i32 + 10);
                    isl.snap_to_target();
                    events::set_cursor(isl.hwnd, Cursor::Arrow);
                    self.save_island_pos();
                    return;
                }
                match press {
                    Some((_, _, Some(btn), _)) => self.island_button(btn),
                    // a gota: um clique abre a pílula; outro, a ilha inteira
                    Some(_) if isl.shape == Shape::Dot => {
                        isl.opened_dot = true;
                        isl.set_shape(Shape::Compact);
                    }
                    Some(_) if isl.shape != Shape::Large => isl.set_shape(Shape::Large),
                    _ => {}
                }
            }
            MouseKind::Right => {
                // o menu de quem está debaixo do mouse (a bolha ou a pílula)
                let key = match isl.bubble_at(sx, sy) {
                    Some(k) => isl.bubble_entries().get(k).map_or(isl.multi.key, |e| e.key),
                    None => isl.key_for(isl.shape),
                };
                isl.multi.menu_key = key;
                let mut items = Vec::new();
                if isl.media {
                    let app = info.as_ref().map(|i| i.app.clone()).unwrap_or_else(|| "player".into());
                    items.push(MenuItem::Cmd("Voltar para a área de trabalho".into(), M_ISLAND_BACK).icon("pilha"));
                    if key == KEY {
                        items.push(MenuItem::Cmd(format!("Abrir no {app}"), M_ISLAND_OPEN).icon("midia"));
                    }
                }
                if key == REST {
                    // o que a ilha em repouso mostra (mais de um: eles se alternam)
                    use crate::widgets::outside::rest;
                    let cfg = &self.sh.cfg;
                    let chosen = rest::chosen(cfg);
                    let mut looks: Vec<MenuItem> = rest::Look::ALL.iter().enumerate().map(|(i, l)| MenuItem::Check(l.title().into(), chosen.contains(l), M_ISLAND_REST + i as u32)).collect();
                    looks.push(MenuItem::Sep);
                    looks.push(MenuItem::Check("Só uma gota (o resto com o mouse em cima)".into(), rest::dot_on(cfg), M_ISLAND_REST_DOT));
                    items.push(MenuItem::Sub("Mostrar em repouso".into(), looks).icon("widgets"));
                    items.push(MenuItem::Cmd("Desligar a ilha em repouso".into(), M_ISLAND_HIDE).icon("close"));
                } else if key != KEY {
                    // os itens da própria fonte (silenciar o app da notificação)
                    let env = Env { cfg: &self.sh.cfg, key };
                    let own = self.feeds.iter().find(|f| f.key() == key).map(|f| f.menu(&env)).unwrap_or_default();
                    items.extend(own.into_iter().filter_map(|item| match item {
                        MenuItem::Cmd(label, action) if action < FEED_ACTIONS => Some(MenuItem::Cmd(label, M_ISLAND_FEED + action).icon("close")),
                        _ => None,
                    }));
                    items.push(MenuItem::Cmd(format!("Não mostrar {} na ilha", widgets::title(key)), M_ISLAND_HIDE).icon("close"));
                }
                items.push(MenuItem::Sep);
                items.push(MenuItem::Sub("Modo foco".into(), focus_menu).icon("foco"));
                items.push(MenuItem::Check("Na barra de tarefas".into(), isl.docked, M_ISLAND_DOCK).icon("pin"));
                items.push(MenuItem::Check("Seguir a tela em uso".into(), follows_screen(&self.sh.cfg), M_ISLAND_FOLLOW).icon("mover"));
                items.push(MenuItem::Cmd("Pôr no alto, no meio da tela".into(), M_ISLAND_CENTER).icon("mover"));
                self.menu.popup(items, sx, sy, Some(KEY));
            }
            MouseKind::Leave => {
                if isl.hot.take().is_some() {
                    isl.next_frame = Instant::now();
                }
            }
            MouseKind::Double => {}
        }
    }

    fn island_button(&mut self, btn: Btn) {
        match btn {
            Btn::Back => {
                self.close_island(true);
                return;
            }
            Btn::Bubble(k) => {
                if let Some(isl) = &mut self.island {
                    let list = isl.bubble_entries();
                    match list.get(k).map(|e| e.key) {
                        // com mais gente esperando, o primeiro clique abre todas as bolhas
                        Some(_) if k == 0 && !isl.multi.strip && list.len() > 1 => isl.multi.strip = true,
                        Some(key) => isl.choose(key, true),
                        None => {}
                    }
                    isl.next_frame = Instant::now();
                }
                return;
            }
            Btn::Widget(action) => {
                let Some(isl) = &self.island else { return };
                let key = isl.key_for(isl.shape);
                if let Some(j) = self.find(key) {
                    self.with_kind(j, |k, ctx| k.act(action, ctx));
                    self.cards[j].refresh(&self.sh);
                } else if let Some(f) = self.feeds.iter().position(|f| f.key() == key) {
                    self.with_feed(f, |k, ctx| k.act(action, ctx));
                } else {
                    return;
                }
                self.island_touch(key);
                self.island_sync();
                if let Some(isl) = &mut self.island {
                    isl.next_frame = Instant::now();
                }
                return;
            }
            _ => {}
        }
        let Some(j) = self.media_index() else { return };
        let action = match btn {
            Btn::Toggle => island_act::TOGGLE,
            Btn::Next => island_act::NEXT,
            Btn::Prev => island_act::PREV,
            Btn::Open => island_act::OPEN_PLAYER,
            Btn::Seek(f) => island_act::SEEK + f.min(1000),
            _ => return,
        };
        self.with_kind(j, |k, ctx| k.act(action, ctx));
        self.island_touch(KEY);
        if let Some(isl) = &mut self.island {
            isl.next_frame = Instant::now();
        }
    }

    /// Comandos do menu da ilha (clique direito).
    pub(super) fn island_menu(&mut self, id: u32) -> bool {
        match id {
            M_ISLAND_BACK => self.close_island(true),
            M_ISLAND_OPEN => self.island_button(Btn::Open),
            M_ISLAND_HIDE => {
                let Some(key) = self.island.as_ref().map(|i| i.multi.menu_key) else { return true };
                self.sh.cfg.obj_mut(key).insert("na_ilha".into(), json!(false));
                self.sh.save();
                self.island_sync_at = Instant::now();
                self.island_sync();
            }
            M_ISLAND_REST..=M_ISLAND_REST_DOT => {
                use crate::widgets::outside::rest;
                let id = match rest::Look::ALL.get((id - M_ISLAND_REST) as usize) {
                    Some(l) => l.setting_id(),
                    None if id == M_ISLAND_REST_DOT => rest::DOT_ID,
                    None => return true,
                };
                rest::toggle(&mut self.sh.cfg, id);
                self.sh.save();
                self.rest_changed();
            }
            id if (M_ISLAND_FEED..M_ISLAND_FEED + FEED_ACTIONS).contains(&id) => {
                let Some(key) = self.island.as_ref().map(|i| i.multi.menu_key) else { return true };
                if let Some(f) = self.feeds.iter().position(|f| f.key() == key) {
                    self.with_feed(f, |k, ctx| k.act(id - M_ISLAND_FEED, ctx));
                }
                self.island_sync_at = Instant::now();
                self.island_sync();
            }
            M_ISLAND_FOLLOW => {
                let on = !follows_screen(&self.sh.cfg);
                self.sh.cfg.obj_mut(KEY).insert(FOLLOW_FIELD.into(), json!(on));
                self.sh.save();
            }
            M_ISLAND_CENTER | M_ISLAND_DOCK => {
                let Some(isl) = &self.island else { return true };
                let g = isl.geo();
                let near = (g.x as i32, g.y as i32, (g.x + g.w) as i32, (g.y + g.h) as i32);
                let (anchor, docked) = if id == M_ISLAND_DOCK { self.island_home(near, isl.s) } else { (self.island_default_anchor(near, isl.s), false) };
                self.move_island(anchor, docked);
            }
            _ => return false,
        }
        true
    }

    /// A roda do mouse na ilha: nas bolhas, troca o widget da pílula; na pílula, muda o volume
    /// (com a música nela), troca o que a ilha em repouso mostra ou troca o widget (veja
    /// `wheel_does`).
    pub(super) fn island_wheel(&mut self, delta: i32) {
        if delta == 0 {
            return;
        }
        let (sx, sy) = win::pointer();
        let steps = delta as f32 / 120.0;
        let now = Instant::now();
        let Some(isl) = &mut self.island else { return };
        if isl.morph != Morph::None || isl.vanishing() {
            return;
        }
        match wheel_does(isl.near_bubbles(sx, sy), isl.key_for(isl.shape)) {
            WheelDoes::Volume => {
                if !isl.media {
                    return;
                }
                let Some(j) = self.media_index() else { return };
                self.with_kind(j, |k, ctx| k.wheel(0.0, 0.0, steps, Mode::M, ctx));
            }
            WheelDoes::Looks => {
                let Some(dir) = isl.wheel_turn(steps, now) else { return };
                let Some(f) = self.feeds.iter().position(|f| f.key() == REST) else { return };
                // para a ilha em repouso, um passo para baixo (negativo) é o próximo
                self.with_feed(f, |k, ctx| k.wheel(0.0, 0.0, -(dir as f32), Mode::M, ctx));
            }
            WheelDoes::Widgets => {
                let Some(dir) = isl.wheel_turn(steps, now) else { return };
                if !isl.cycle(dir) {
                    return;
                }
            }
        }
        // a troca (ou a barra de volume) aparece já, sem esperar o próximo quadro
        if let Some(isl) = &mut self.island {
            isl.next_frame = Instant::now();
        }
    }

    /// Depois de mudar a combinação de monitores: a ilha continua na tela.
    pub(super) fn island_screens_changed(&mut self) {
        let card = self.media_index().map(|j| (self.cards[j].card_rect(), self.cards[j].s));
        let Some(isl) = &self.island else { return };
        let visible = win::on_screen(isl.anchor.0 as i32, isl.anchor.1 as i32 + 10);
        let slot = if isl.docked { dock_slot(isl.anchor.0 as i32, isl.anchor.1 as i32 + 10) } else { None };
        let (anchor, docked) = match (slot, visible, card) {
            (Some((slot, _)), _, _) => (slot, true),
            (None, true, _) => (isl.anchor, false),
            (None, false, Some((rect, s))) => self.island_home(rect, s),
            (None, false, None) => (isl.anchor, false),
        };
        if let Some(isl) = &mut self.island {
            isl.anchor = anchor;
            isl.docked = docked;
            isl.s = island_scale(anchor.0 as i32, anchor.1 as i32 + 10);
            isl.snap_to_target();
        }
    }

    /// Um quadro da ilha: termina as animações, some em tela cheia, acompanha o botão Iniciar e
    /// o mouse, estica com os avisos, apaga com o PC parado, desenha (só se algo mudou) e marca
    /// quando volta.
    pub(super) fn island_tick(&mut self) {
        let now = Instant::now();
        if !self.island.as_ref().is_some_and(|i| now >= i.next_frame) {
            return;
        }
        let (info, peak) = self.island_info();
        if self.finish_island_animations(info.as_ref(), now) {
            return;
        }
        let paused = self.sh.paused.clone();
        self.follow_active_screen(now);
        let Some(isl) = &mut self.island else { return };
        isl.hide_in_fullscreen(&paused);
        isl.follow_start_button(now);
        let (px, py) = win::pointer();
        if isl.morph == Morph::None && !isl.dragging && !isl.vanishing() {
            let over = isl.geo().contains(px, py);
            let on_bubbles = isl.follow_pointer_on_bubbles(px, py, now);
            if isl.follow_pointer_on_pill(over, on_bubbles, now) {
                self.island_sync_at = now;
            }
            isl.stretch_for_alerts(over);
        }
        isl.step_dim();
        isl.step_levels(info.as_ref(), peak);
        let shown = isl.key_for(isl.shape);
        let playing = info.as_ref().is_some_and(|i| i.playing) && shown == KEY;
        // o widget na pílula pode pedir quadros (a chuva caindo na ilha aberta do Tempo)
        let fps = match (shown, isl.morph) {
            (KEY, _) | (_, Morph::In | Morph::Out) => 0,
            _ => {
                let form = isl.shape.form();
                let card = self.cards.iter().find(|c| c.key() == shown).map(|c| c.kind.island_fps(form));
                card.or_else(|| self.feeds.iter().find(|f| f.key() == shown).map(|f| f.island_fps(form))).unwrap_or(0)
            }
        };
        let dimming = (isl.dim - isl.dim_target()).abs() > 0.001;
        let busy = isl.animating() || isl.dragging || isl.multi.bubbles_at.elapsed() < Duration::from_millis(300) || dimming;
        if !isl.hidden {
            let scene = Scene::of(info.as_ref(), &self.cards, &self.feeds, &self.sh.cfg);
            // parada e sem nada mudando (a ilha em repouso muda a cada minuto), não redesenha
            let signature = if busy || fps > 0 || playing { None } else { isl.signature(&scene, shown) };
            if signature.is_none() || signature != isl.drawn {
                let (img, pos) = isl.render(&scene);
                let (window_alpha, _) = isl.pop_alpha();
                win::blit(isl.hwnd, &img, Some(pos), (window_alpha * isl.dim * 255.0).round() as u8);
                isl.drawn = signature;
            }
        }
        let near = isl.near(px, py) && !isl.hidden;
        let visible = !isl.hidden;
        isl.next_frame = now + next_frame_wait(busy, fps * visible as u32, playing && visible, near);
    }

    /// As animações da ilha que terminaram agora. Devolve true se a ilha saiu da tela (voltou a
    /// ser o cartão, sumiu, ou a música voltou para a área de trabalho): o quadro acaba aí.
    fn finish_island_animations(&mut self, info: Option<&IslandInfo>, now: Instant) -> bool {
        let Some(isl) = &mut self.island else { return true };
        // terminou de voar de volta: o cartão reaparece
        if isl.morph == Morph::Out && isl.progress() >= 1.0 {
            self.finish_close();
            return true;
        }
        // terminou de sumir (ninguém mais na ilha)
        if isl.vanishing() && isl.pop_progress() >= 1.0 {
            self.drop_island();
            self.island_sync_at = now + SYNC_IDLE;
            return true;
        }
        // terminou de nascer: as bolhas entram esmaecendo
        if isl.appearing() && isl.pop_progress() >= 1.0 {
            isl.multi.pop = None;
            isl.multi.bubbles_at = now;
        }
        if isl.morph == Morph::In && isl.progress() >= 1.0 {
            isl.morph = Morph::None;
        }
        // nada tocando por um bom tempo: a música volta para a área de trabalho
        if !isl.media || info.is_some_and(|i| i.playing) {
            isl.quiet_since = None;
        } else if isl.quiet_since.get_or_insert(now).elapsed() >= QUIET {
            self.close_island(true);
            return true;
        }
        false
    }
}

/// Quadros da ilha para a animação do README (sem janela): None é "o cartão no lugar"; Some, a
/// pílula e onde ela fica. `card`: retângulo do cartão (x, y, w, h, raio) em pixels.
#[cfg(test)]
pub(crate) fn readme_frames(info: &IslandInfo, card: (f32, f32, f32, f32, f32), face: &Image, anchor: (f32, f32), s: f32) -> Vec<(Option<(Image, (i32, i32))>, u32)> {
    let now = Instant::now();
    let card_geo = Geo { x: card.0, y: card.1, w: card.2, h: card.3, r: card.4 };
    let mut isl = Island {
        hwnd: HWND(std::ptr::null_mut()),
        shape: Shape::Compact,
        prev_shape: Shape::Compact,
        shape_at: now - Duration::from_secs(5),
        from: card_geo,
        to: card_geo,
        anim_at: now,
        anim_ms: MORPH_MS,
        morph: Morph::In,
        anchor,
        s,
        docked: false,
        dock_check: now,
        press: None,
        dragging: false,
        hot: None,
        inside_since: None,
        outside_since: None,
        hidden: false,
        quiet_since: None,
        levels: [0.2; 5],
        started: now,
        next_frame: now,
        card_color: info.colors[0].mix(BLACK, 0.55),
        art_cache: None,
        media: true,
        multi: Multi::new(now),
        card_face: Some(face.clone()),
        dim: 1.0,
        drawn: None,
        opened_dot: false,
        screen_check: now,
        screen_pending: None,
    };
    isl.to = isl.target(Shape::Compact);
    let mut out: Vec<(Option<(Image, (i32, i32))>, u32)> = vec![(None, 1100)];
    let mut clock = 0u64;
    // um quadro em `t` ms da animação atual (o relógio da ilha é "voltado" para lá)
    let mut frame = |isl: &mut Island, t: u64, ms: u32, out: &mut Vec<_>| {
        clock += ms as u64;
        let base = Instant::now();
        isl.anim_at = base - Duration::from_millis(t);
        isl.shape_at = base - Duration::from_millis(t);
        isl.started = base - Duration::from_millis(clock);
        let peak = 0.45 + 0.35 * ((clock as f32) / 170.0).sin().abs();
        isl.step_levels(Some(info), Some(peak));
        let (img, pos) = isl.render(&Scene::media(Some(info)));
        out.push((Some((img, pos)), ms));
    };
    let anim = |isl: &mut Island, total: f32, out: &mut Vec<_>, frame: &mut dyn FnMut(&mut Island, u64, u32, &mut Vec<(Option<(Image, (i32, i32))>, u32)>)| {
        let n = (total / 40.0).ceil() as u64;
        for k in 1..=n {
            frame(isl, (k * 40).min(total as u64 + 60), 40, out);
        }
    };
    // o cartão vira ilha
    anim(&mut isl, MORPH_MS, &mut out, &mut frame);
    isl.morph = Morph::None;
    for _ in 0..12 {
        frame(&mut isl, 2000, 80, &mut out);
    }
    for shape in [Shape::Medium, Shape::Large, Shape::Compact] {
        isl.set_shape(shape);
        anim(&mut isl, STATE_MS + 40.0, &mut out, &mut frame);
        let hold = if shape == Shape::Large { 16 } else { 10 };
        for _ in 0..hold {
            frame(&mut isl, 2000, 80, &mut out);
        }
    }
    // e volta para o lugar
    isl.from = isl.geo();
    isl.to = card_geo;
    isl.morph = Morph::Out;
    anim(&mut isl, MORPH_MS, &mut out, &mut frame);
    out.push((None, 1200));
    out
}

/// As três formas paradas (para a imagem do README).
#[cfg(test)]
pub(crate) fn readme_shapes(info: &IslandInfo, s: f32) -> Vec<Image> {
    [Shape::Compact, Shape::Medium, Shape::Large]
        .into_iter()
        .map(|shape| {
            let now = Instant::now();
            let island_ui::FormSize { w, h, radius: r } = shape.size();
            let g = Geo { x: 0.0, y: 0.0, w: w * s, h: h * s, r: r * s };
            let mut isl = Island {
                hwnd: HWND(std::ptr::null_mut()),
                shape,
                prev_shape: shape,
                shape_at: now - Duration::from_secs(5),
                from: g,
                to: g,
                anim_at: now - Duration::from_secs(5),
                anim_ms: 0.0,
                morph: Morph::None,
                anchor: (0.0, 0.0),
                s,
                docked: false,
                dock_check: now,
                press: None,
                dragging: false,
                hot: None,
                inside_since: None,
                outside_since: None,
                hidden: false,
                quiet_since: None,
                levels: [0.35, 0.8, 0.55, 1.0, 0.45],
                started: now,
                next_frame: now,
                card_color: BLACK,
                art_cache: None,
                media: true,
                multi: Multi::new(now),
                card_face: None,
                dim: 1.0,
                drawn: None,
                opened_dot: false,
                screen_check: now,
                screen_pending: None,
            };
            isl.render(&Scene::media(Some(info))).0
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn island(shape: Shape, morph: Morph, progress: f32) -> Island {
        let now = Instant::now();
        let mut isl = Island {
            hwnd: HWND(std::ptr::null_mut()),
            shape,
            prev_shape: shape,
            shape_at: now - Duration::from_secs(2),
            from: Geo { x: 0.0, y: 0.0, w: 1.0, h: 1.0, r: 0.0 },
            to: Geo { x: 0.0, y: 0.0, w: 1.0, h: 1.0, r: 0.0 },
            anim_at: now - Duration::from_millis((MORPH_MS * progress) as u64),
            anim_ms: MORPH_MS,
            morph,
            anchor: (600.0, 20.0),
            s: 1.25,
            docked: false,
            dock_check: now,
            press: None,
            dragging: false,
            hot: if shape == Shape::Large { Some(Btn::Back) } else { None },
            inside_since: None,
            outside_since: None,
            hidden: false,
            quiet_since: None,
            levels: [0.3, 0.8, 0.5, 1.0, 0.4],
            started: now,
            next_frame: now,
            card_color: Rgba::rgb(110, 30, 70),
            art_cache: None,
            media: true,
            multi: Multi::new(now),
            card_face: None,
            dim: 1.0,
            drawn: None,
            opened_dot: false,
            screen_check: now,
            screen_pending: None,
        };
        let card = Geo { x: 500.0, y: 300.0, w: 400.0, h: 188.0, r: 32.0 };
        let pill = Geo { x: 0.0, y: 0.0, w: shape.size().w * 1.25, h: shape.size().h * 1.25, r: shape.size().radius * 1.25 };
        isl.from = if morph == Morph::None { pill } else { card };
        isl.to = pill;
        isl
    }

    /// Levada para o outro monitor, a ilha fica no mesmo lugar relativo: no alto e no meio continua
    /// no alto e no meio (na escala de lá), perto de baixo continua perto de baixo.
    #[test]
    fn the_island_keeps_its_place_on_the_other_screen() {
        let left = (0, 0, 1920, 1040);
        let right = (1920, 0, 1920 + 2560, 1400);
        // no alto e no meio, a 10 pontos do topo na escala 1,25 (12,5 px)
        let top = anchor_on((960.0, 12.5), 1.25, left, right, 1.5);
        assert_eq!(top, (1920.0 + 1280.0, 15.0));
        // perto de baixo, à esquerda
        let low = anchor_on((480.0, 1000.0), 1.0, left, right, 1.0);
        assert_eq!(low, (1920.0 + 640.0, 1360.0));
        // e de volta, no mesmo lugar
        let back = anchor_on(top, 1.5, right, left, 1.25);
        assert!((back.0 - 960.0).abs() < 0.01 && (back.1 - 12.5).abs() < 0.01, "{back:?}");
        // um monitor mais baixo não deixa a ilha sair da tela
        let short = anchor_on((100.0, 900.0), 1.0, (0, 0, 1920, 2000), (0, 0, 1920, 600), 1.0);
        assert!(short.1 >= 0.0 && short.1 <= 600.0);
    }

    /// Na barra de tarefas: à esquerda do Iniciar, no meio da altura da barra; com a barra
    /// alinhada à esquerda (sem espaço antes do Iniciar), antes da área de notificação.
    #[test]
    fn taskbar_slot_sits_left_of_start() {
        let s = 1.25 * SIZE;
        let island_ui::FormSize { w, h, .. } = Shape::Compact.size();
        let (w, h) = (w * s, h * s);
        let centered = win::Taskbar { bar: (0, 1020, 1920, 1080), start: Some((684, 1020, 740, 1080)), tray: Some((1524, 1020, 1920, 1080)) };
        let (cx, top) = taskbar_slot(&centered, s);
        assert!((cx + w / 2.0 - (684.0 - DOCK_GAP * s)).abs() <= 1.0, "a borda direita encosta no espaço antes do Iniciar: {cx}");
        assert!((top - (1020.0 + (60.0 - h) / 2.0)).abs() <= 1.0 && top >= 1020.0 && top + h <= 1080.0, "{top}");
        let left = win::Taskbar { start: Some((12, 1020, 68, 1080)), ..centered };
        let (cx, _) = taskbar_slot(&left, s);
        assert!((cx + w / 2.0 - (1524.0 - DOCK_GAP * s)).abs() <= 1.0, "antes da área de notificação: {cx}");
        let bare = win::Taskbar { bar: (1920, 1020, 3840, 1080), start: None, tray: None };
        let (cx, top) = taskbar_slot(&bare, s);
        assert!(cx + w / 2.0 <= 3840.0 && cx - w / 2.0 >= 1920.0 && top >= 1020.0, "{cx} {top}");
    }

    /// Na barra, a ilha cresce para a esquerda e para cima: a borda direita (perto do Iniciar) e
    /// a de baixo ficam onde estavam, e nada cobre o botão.
    #[test]
    fn docked_island_grows_away_from_start() {
        let scr = win::monitors()[0];
        let mut isl = island(Shape::Compact, Morph::None, 1.0);
        isl.docked = true;
        isl.anchor = (((scr.0 + scr.2) / 2) as f32, scr.3 as f32 - 44.0);
        let compact = isl.target(Shape::Compact);
        for shape in [Shape::Medium, Shape::Large] {
            let g = isl.target(shape);
            assert!((g.x + g.w - (compact.x + compact.w)).abs() < 0.5, "{shape:?}: a direita andou");
            assert!((g.y + g.h - (compact.y + compact.h)).abs() < 0.5, "{shape:?}: o fundo andou");
        }
        // solta (fora da barra), continua crescendo a partir do meio
        isl.docked = false;
        let (c, m) = (isl.target(Shape::Compact), isl.target(Shape::Medium));
        assert!((c.x + c.w / 2.0 - (m.x + m.w / 2.0)).abs() < 0.5);
    }

    /// A barra de tarefas deste PC, se ela está à vista (só para conferir à mão):
    /// cargo test --release taskbar_here -- --ignored --nocapture
    #[test]
    #[ignore]
    fn taskbar_here() {
        let scr = win::monitors()[0];
        let t = win::taskbar_at((scr.0 + scr.2) / 2, scr.3 - 2);
        println!("{t:?}");
        println!("{:?}", dock_slot((scr.0 + scr.2) / 2, scr.3 - 2));
    }

    fn entry(key: &'static str, item: IslandItem) -> Entry {
        Entry { key, item }
    }

    fn keys(e: &[Entry]) -> Vec<&'static str> {
        e.iter().map(|e| e.key).collect()
    }

    /// Quem fica na pílula: o mexido por último entre os que andam; pausado atrás de quem toca;
    /// o último minuto do Pomodoro na frente; aviso nunca toma a pílula de quem anda (só o
    /// urgente ainda não visto); a escolha da pessoa vale mais que tudo isso.
    #[test]
    fn rank_puts_the_right_one_in_the_pill() {
        let now = Instant::now();
        let live = IslandItem::live(WHITE);
        let paused = IslandItem { idle: true, ..live };
        let hurry = IslandItem { hurry: true, ..live };
        let alert = IslandItem::alert(WHITE, 1);
        let urgent = IslandItem { urgent: true, ..alert };
        let mut touched = HashMap::new();
        touched.insert("midia", now - Duration::from_secs(10));
        touched.insert("pomodoro", now);
        touched.insert("github", now + Duration::from_secs(5));
        touched.insert("bateria", now + Duration::from_secs(5));

        let mut e = vec![entry("midia", live), entry("pomodoro", live)];
        rank(&mut e, &touched, None, &[]);
        assert_eq!(keys(&e), ["pomodoro", "midia"], "o mexido por último");

        let mut e = vec![entry("pomodoro", paused), entry("midia", live)];
        rank(&mut e, &touched, None, &[]);
        assert_eq!(keys(&e), ["midia", "pomodoro"], "pausado fica atrás de quem toca");

        let mut t2 = touched.clone();
        t2.insert("midia", now + Duration::from_secs(60));
        let mut e = vec![entry("midia", live), entry("pomodoro", hurry)];
        rank(&mut e, &t2, None, &[]);
        assert_eq!(keys(&e), ["pomodoro", "midia"], "o último minuto passa na frente");

        let mut e = vec![entry("github", alert), entry("midia", live)];
        rank(&mut e, &touched, None, &["github"]);
        assert_eq!(keys(&e), ["midia", "github"], "aviso não toma a pílula");

        let mut e = vec![entry("midia", live), entry("bateria", urgent)];
        rank(&mut e, &touched, None, &["bateria"]);
        assert_eq!(e[0].key, "bateria", "urgente e ninguém viu");
        rank(&mut e, &touched, None, &[]);
        assert_eq!(e[0].key, "midia", "urgente já visto volta para a bolha");

        let mut e = vec![entry("midia", live), entry("github", alert)];
        rank(&mut e, &touched, Some("github"), &[]);
        assert_eq!(e[0].key, "github", "a escolha da pessoa");

        let mut e = vec![entry("github", alert)];
        rank(&mut e, &touched, None, &[]);
        assert_eq!(e[0].key, "github", "sozinho, o aviso fica na pílula");
    }

    /// Um aviso novo estica a ilha uma vez só (o mesmo aviso não estica de novo; outro estica) e
    /// ganha o pontinho; quem sai da ilha é esquecido.
    #[test]
    fn new_alert_banners_once() {
        let now = Instant::now();
        let live = IslandItem::live(WHITE);
        let mut isl = island(Shape::Compact, Morph::None, 1.0);
        isl.update_entries(vec![entry("midia", live), entry("github", IslandItem::alert(WHITE, 7))], now);
        assert_eq!(isl.multi.key, "midia");
        assert_eq!(isl.multi.banner.map(|b| b.0), Some("github"));
        assert_eq!(isl.multi.unseen, ["github"]);
        assert_eq!(isl.bubble_count(), 1);
        isl.multi.banner = None;
        isl.update_entries(vec![entry("midia", live), entry("github", IslandItem::alert(WHITE, 7))], now);
        assert!(isl.multi.banner.is_none(), "o mesmo aviso não estica de novo");
        isl.update_entries(vec![entry("midia", live), entry("github", IslandItem::alert(WHITE, 8))], now);
        assert_eq!(isl.multi.banner.map(|b| b.0), Some("github"), "um aviso novo estica");
        isl.update_entries(vec![entry("midia", live)], now);
        assert!(isl.multi.unseen.is_empty() && isl.multi.banner.is_none() && isl.bubble_count() == 0);
    }

    /// Nascendo: a gota começa apagada e pequena, acende antes do conteúdo e termina inteira, sem
    /// bolhas no caminho. Sumindo: o conteúdo apaga antes da janela, a pílula encolhe numa gota e
    /// tudo some; sem animação, some de uma vez; voltando alguém, ela estica de novo sem piscar.
    #[test]
    fn island_pops_in_and_out() {
        let now = Instant::now();
        let live = IslandItem::live(WHITE);
        let mut isl = island(Shape::Compact, Morph::None, 1.0);
        isl.media = false;
        isl.update_entries(vec![entry("pomodoro", live), entry("github", live)], now);
        let pill = isl.target(Shape::Compact);
        isl.to = pill;
        isl.from = isl.drop_of(pill, 0.5);
        isl.anim_at = now;
        isl.anim_ms = APPEAR_MS;
        let at = |ms: f32| Instant::now() - Duration::from_millis(ms as u64);

        isl.multi.pop = Some((Pop::Appear, now));
        let (wa, ca) = isl.pop_alpha();
        assert!(wa < 0.01 && ca == 0.0, "nasce apagada: {wa} {ca}");
        isl.anim_at = Instant::now();
        let g = isl.geo();
        assert!(g.w < pill.h * 0.6 && (g.w - g.h).abs() < 2.0, "nasce uma gota redonda: {g:?}");
        assert!((g.x + g.w / 2.0 - (pill.x + pill.w / 2.0)).abs() < 2.0, "no meio da pílula");
        assert_eq!(isl.bubble_count(), 0, "sem bolhas nascendo");
        assert!(isl.animating());
        isl.multi.pop = Some((Pop::Appear, at(APPEAR_MS * 0.3)));
        let (wa, ca) = isl.pop_alpha();
        assert!(wa > 0.99 && ca == 0.0, "a gota acende antes do conteúdo: {wa} {ca}");
        isl.multi.pop = Some((Pop::Appear, at(APPEAR_MS + 10.0)));
        assert_eq!(isl.pop_alpha(), (1.0, 1.0));
        assert_eq!(isl.pop_progress(), 1.0);

        // na barra de tarefas, nasce do lado do botão Iniciar
        isl.docked = true;
        let d = isl.drop_of(pill, 0.5);
        assert!((d.x + d.w / 2.0 - (pill.x + pill.w - pill.h / 2.0)).abs() < 0.5);
        isl.docked = false;

        isl.multi.pop = None;
        isl.from = pill;
        isl.anim_ms = 0.0;
        isl.start_vanish(true, now);
        assert!(isl.vanishing() && isl.animating());
        let (wa, ca) = isl.pop_alpha();
        assert!(wa == 1.0 && ca > 0.99, "começa inteira: {wa} {ca}");
        assert!(isl.to.w < pill.h && (isl.to.w - isl.to.h).abs() < 0.5, "encolhe numa gota: {:?}", isl.to);
        assert_eq!(isl.bubble_count(), 0);
        isl.multi.pop = Some((Pop::Vanish, at(VANISH_MS * 0.4)));
        let (wa, ca) = isl.pop_alpha();
        assert!(wa > 0.99 && ca == 0.0, "o conteúdo apaga antes da janela: {wa} {ca}");
        isl.multi.pop = Some((Pop::Vanish, at(VANISH_MS + 10.0)));
        assert_eq!(isl.pop_alpha(), (0.0, 0.0), "termina apagada");

        // voltou alguém no meio: estica de novo, sem a janela piscar
        isl.multi.pop = Some((Pop::Vanish, at(VANISH_MS * 0.75)));
        let (before, _) = isl.pop_alpha();
        isl.cancel_vanish(Instant::now());
        assert!(isl.appearing());
        let (after, _) = isl.pop_alpha();
        assert!((after - before).abs() < 0.03, "a janela continua de onde estava: {before} -> {after}");
        assert_eq!(isl.to.w, pill.w, "volta para o tamanho da pílula");

        // sem animação: some no próximo quadro
        isl.multi.pop = None;
        isl.start_vanish(false, now);
        assert!(isl.vanishing() && isl.pop_progress() >= 1.0 && isl.progress() >= 1.0);
    }

    /// Nascendo e sumindo, a ilha desenha em todos os passos (com o brilho, sem o conteúdo no
    /// começo do nascer e no fim do sumir).
    #[test]
    fn island_pop_draws_every_step() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let now = Instant::now();
        for pop in [Pop::Appear, Pop::Vanish] {
            let total = if pop == Pop::Appear { APPEAR_MS } else { VANISH_MS };
            for k in 0..=10 {
                let mut isl = island(Shape::Compact, Morph::None, 1.0);
                isl.multi.pop = Some((pop, now - Duration::from_millis((total * k as f32 / 10.0) as u64)));
                let (img, _) = isl.render(&Scene::media(None));
                assert!(img.w > 0 && img.h > 0);
            }
        }
    }

    /// A ilha em repouso: com o "só uma gota" (ou o PC parado), fechada vira gota, encostada no
    /// mesmo lado da pílula (na barra, do lado do Iniciar); chegou alguém de verdade, volta a ser
    /// pílula. Apagada só com o PC parado e fechada.
    #[test]
    fn rest_closes_into_a_dot_and_dims() {
        let now = Instant::now();
        let mut isl = island(Shape::Compact, Morph::None, 1.0);
        isl.media = false;
        isl.docked = true;
        let rest = IslandItem { idle: true, ..IslandItem::live(WHITE) };
        isl.update_entries(vec![entry(REST, rest)], now);
        assert_eq!(isl.closed_shape(), Shape::Compact);
        assert_eq!(isl.dim_target(), 1.0);
        isl.update_entries(vec![entry(REST, IslandItem { dot: true, dim: true, ..rest })], now);
        assert_eq!(isl.closed_shape(), Shape::Dot);
        assert_eq!(isl.dim_target(), DIM);
        let (dot, pill) = (isl.target(Shape::Dot), isl.target(Shape::Compact));
        assert!((dot.w - dot.h).abs() < 0.5 && dot.w < pill.w / 3.0, "uma gota redonda: {dot:?}");
        assert!((dot.x + dot.w - (pill.x + pill.w)).abs() < 0.5, "do lado do Iniciar");
        isl.set_shape(Shape::Medium);
        assert_eq!(isl.dim_target(), 1.0, "aberta, acesa");
        // chegou o Pomodoro: a pílula volta, acesa
        isl.update_entries(vec![entry("pomodoro", IslandItem::live(WHITE))], now);
        assert_eq!(isl.closed_shape(), Shape::Compact);
        isl.set_shape(Shape::Compact);
        assert_eq!(isl.dim_target(), 1.0);
        assert_eq!(Shape::Dot.form(), IslandForm::Bubble);
        assert!(Shape::Dot.closed() && Shape::Compact.closed() && !Shape::Banner.closed());
    }

    /// Aberta inteira, a ilha volta ao tamanho normal 1 s depois que o mouse sai dela (e não
    /// antes).
    #[test]
    fn large_island_closes_a_second_after_the_mouse_leaves() {
        let mut isl = island(Shape::Large, Morph::None, 1.0);
        isl.outside_since = Some(Instant::now() - Duration::from_millis(900));
        isl.follow_pointer_on_pill(false, false, Instant::now());
        assert_eq!(isl.shape, Shape::Large, "com 0,9 s fora, ainda aberta");
        isl.outside_since = Some(Instant::now() - Duration::from_millis(1100));
        isl.follow_pointer_on_pill(false, false, Instant::now());
        assert!(isl.shape.closed(), "com 1,1 s fora, fechada: {:?}", isl.shape);
        // o mouse voltando zera a conta
        isl.set_shape(Shape::Large);
        isl.follow_pointer_on_pill(true, false, Instant::now());
        assert_eq!(isl.outside_since, None);
    }

    /// A gota (e a troca de gota para pílula) desenha em todos os passos.
    #[test]
    fn dot_draws() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let mut isl = island(Shape::Dot, Morph::None, 1.0);
        isl.media = false;
        let (img, _) = isl.render(&Scene::media(None));
        let island_ui::FormSize { w, h, .. } = Shape::Dot.size();
        assert!(img.w as f32 >= w * isl.s && (img.h as f32) >= h * isl.s);
        isl.set_shape(Shape::Compact);
        let (img, _) = isl.render(&Scene::media(None));
        assert!(img.w > 0);
    }

    /// Com três ou mais, só uma bolha aparece (com o "+N"); abertas, todas, da pílula para a
    /// esquerda, sem se encostar; clicar numa leva o widget dela para a pílula.
    #[test]
    fn bubbles_line_up_and_choose() {
        let now = Instant::now();
        let live = IslandItem::live(WHITE);
        let mut isl = island(Shape::Compact, Morph::None, 1.0);
        isl.update_entries(vec![entry("midia", live), entry("pomodoro", IslandItem { idle: true, ..live }), entry("github", IslandItem::alert(WHITE, 1)), entry("bateria", IslandItem::alert(WHITE, 2))], now);
        assert_eq!(isl.bubble_count(), 1);
        isl.multi.strip = true;
        let b = isl.bubbles();
        assert_eq!(b.len(), 3);
        let g = isl.geo();
        assert!(b[0].0 + b[0].2 < g.x, "a primeira bolha à esquerda da pílula");
        for w in b.windows(2) {
            assert!(w[1].0 + w[1].2 < w[0].0 - w[0].2, "bolhas encostadas: {w:?}");
            assert_eq!(w[0].1, w[1].1, "na mesma altura");
        }
        let target = isl.bubble_entries()[1].key;
        isl.choose(target, true);
        assert_eq!(isl.multi.key, target);
        assert_eq!(isl.multi.entries[0].key, target);
        assert!(!isl.multi.strip);
    }

    #[test]
    fn wheel_goes_where_the_mouse_is() {
        assert_eq!(wheel_does(false, KEY), WheelDoes::Volume, "a música na pílula: o volume");
        assert_eq!(wheel_does(false, REST), WheelDoes::Looks, "em repouso: o que ela mostra");
        assert_eq!(wheel_does(false, "pomodoro"), WheelDoes::Widgets);
        assert_eq!(wheel_does(true, KEY), WheelDoes::Widgets, "nas bolhas, sempre troca o widget");
    }

    #[test]
    fn wheel_walks_the_widgets_in_a_fixed_order() {
        let now = Instant::now();
        let live = IslandItem::live(WHITE);
        let mut isl = island(Shape::Compact, Morph::None, 1.0);
        isl.update_entries(vec![entry("pomodoro", IslandItem { hurry: true, ..live }), entry("github", IslandItem::alert(WHITE, 1)), entry("midia", live)], now);
        isl.multi.banner = None;
        assert_eq!(isl.multi.key, "pomodoro");
        // um passo por vez, na ordem em que o App acha os widgets, dando a volta
        let mut seen = Vec::new();
        for _ in 0..3 {
            assert!(isl.cycle(1));
            seen.push(isl.multi.key);
        }
        assert_eq!(seen, ["github", "midia", "pomodoro"]);
        assert!(isl.cycle(-1));
        assert_eq!(isl.multi.key, "midia");
        assert_eq!(isl.multi.pinned, Some(("midia", true)), "fica até escolher outro");
        // reordenar a ilha não muda a ordem da roda
        isl.update_entries(vec![entry("pomodoro", IslandItem { hurry: true, ..live }), entry("github", IslandItem::alert(WHITE, 1)), entry("midia", live)], now);
        assert!(isl.cycle(1));
        assert_eq!(isl.multi.key, "pomodoro");
        // com um só, nada
        isl.update_entries(vec![entry("pomodoro", live)], now);
        assert!(!isl.cycle(1));
    }

    #[test]
    fn wheel_turns_one_step_at_a_time() {
        let mut isl = island(Shape::Compact, Morph::None, 1.0);
        let start = Instant::now();
        // o touchpad manda pedacinhos: só troca quando somam um passo
        assert_eq!(isl.wheel_turn(-0.4, start), None);
        assert_eq!(isl.wheel_turn(-0.4, start), None);
        assert_eq!(isl.wheel_turn(-0.4, start), Some(1), "para baixo: o próximo");
        // girando durante o esmaecer, não pula outro
        let during = start + Duration::from_millis(100);
        assert_eq!(isl.wheel_turn(-3.0, during), None);
        let after = start + Duration::from_millis(LAYER_FADE_MS as u64);
        assert_eq!(isl.wheel_turn(1.0, after), Some(-1), "para cima: o anterior");
    }

    /// A ilha com vários widgets num PNG (para conferir):
    /// cargo test --release island_multi -- --ignored --nocapture
    #[test]
    #[ignore]
    fn draw_island_multi() {
        use crate::widgets::{battery::Battery, github::GitHub, pomodoro::Pomodoro, system::System};
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let cfg = json!({"pomodoro": {"estado": {"fase": "foco", "ciclo": 1, "total": 1500.0, "fim": win::now() + 1122.0}}, "github": {}, "app": {}});
        let pomodoro = Pomodoro::new(&cfg);
        let github = GitHub::sample_reviews(&cfg);
        let battery = Battery::sample();
        let system = System::sample_alarm();
        let mut art = Canvas::new(200, 200, 1.0);
        art.gradient_round_rect(0.0, 0.0, 200.0, 200.0, 0.0, &[(0.0, Rgba::rgb(58, 24, 112)), (0.6, Rgba::rgb(214, 64, 120)), (1.0, Rgba::rgb(255, 140, 66))], 0.0, 200.0);
        let info = IslandInfo {
            title: "Sunset Drive".into(),
            artist: "Neon Avenue".into(),
            app: "Spotify".into(),
            art: Some(Arc::new(art.finish())),
            colors: [Rgba::rgb(255, 160, 80), Rgba::rgb(240, 90, 150)],
            playing: true,
            position: 96.0,
            duration: 231.0,
            can_prev: true,
            can_toggle: true,
            can_next: true,
            can_seek: true,
            volume: None,
            volume_hud: 0.0,
        };
        let scene = Scene { info: Some(&info), kinds: vec![&pomodoro, &github, &battery, &system], cfg: &cfg };
        let env = |k: &'static str| Env { cfg: &cfg, key: k };
        let found = |keys: &[&'static str]| -> Vec<Entry> {
            keys.iter()
                .map(|&k| match k {
                    "midia" => entry(k, IslandItem::live(info.colors[0])),
                    k => entry(k, scene.kind(k).unwrap().island_item(&env(k)).unwrap()),
                })
                .collect()
        };
        let shots: Vec<(&str, Shape, Vec<&'static str>, &'static str, bool, bool)> = vec![
            ("musica_e_pomodoro", Shape::Compact, vec!["midia", "pomodoro"], "pomodoro", false, false),
            ("quatro", Shape::Compact, vec!["midia", "pomodoro", "github", "bateria"], "midia", false, false),
            ("quatro_abertas", Shape::Compact, vec!["midia", "pomodoro", "github", "bateria"], "midia", true, false),
            ("aviso", Shape::Banner, vec!["midia", "bateria"], "midia", false, true),
            ("pomodoro_media", Shape::Medium, vec!["pomodoro", "midia"], "pomodoro", false, false),
            ("pomodoro_grande", Shape::Large, vec!["pomodoro"], "pomodoro", false, false),
            ("sistema_media", Shape::Medium, vec!["sistema"], "sistema", false, false),
            ("sistema_grande", Shape::Large, vec!["sistema"], "sistema", false, false),
            ("github_media", Shape::Medium, vec!["github"], "github", false, false),
            ("github_grande", Shape::Large, vec!["github"], "github", false, false),
            ("bateria_media", Shape::Medium, vec!["bateria"], "bateria", false, false),
            ("bateria_grande", Shape::Large, vec!["bateria"], "bateria", false, false),
            ("sistema_aviso", Shape::Banner, vec!["sistema"], "sistema", false, true),
        ];
        for (name, shape, list, first, strip, banner) in shots {
            let mut isl = island(shape, Morph::None, 1.0);
            isl.hot = None;
            isl.multi.entries = found(&list);
            let k = isl.multi.entries.iter().position(|e| e.key == first).unwrap();
            let e = isl.multi.entries.remove(k);
            isl.multi.entries.insert(0, e);
            isl.multi.key = first;
            isl.multi.prev_key = first;
            isl.multi.strip = strip;
            isl.multi.bubbles_at = Instant::now() - Duration::from_secs(5);
            if banner {
                isl.multi.banner = Some((list[1.min(list.len() - 1)], Instant::now()));
            }
            isl.multi.unseen = list.iter().copied().filter(|k| *k == "github" || *k == "bateria").collect();
            let (img, _) = isl.render(&scene);
            let path = std::env::temp_dir().join(format!("ilha_multi_{name}.png"));
            crate::gfx::save_png(&img, &path).unwrap();
            println!("{}", path.display());
        }
    }

    /// Prévia das saudações (bom dia, boa tarde, boa noite), quadro a quadro, com a ilha de
    /// verdade numa barra de tarefas de mentira: primeiro como ela aparece sozinha (nasce,
    /// estica com a saudação, volta e alterna o tempo e as cotações a cada 7 s até sumir), depois
    /// com o mouse em cima (a forma média, também alternando) e aberta com um clique.
    /// Grava PNGs em %TEMP%/widgets-previa/<nome>/ (30 por segundo):
    /// cargo test --release greeting_preview -- --ignored --nocapture
    #[test]
    #[ignore]
    fn greeting_preview() {
        use crate::widgets::outside::{Greeting, Period};
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        const S: f32 = 2.0;
        const W: f32 = 460.0;
        const H: f32 = 300.0;
        const FPS: f64 = 30.0;
        let cfg = json!({"tempo": {"lat": -23.55, "lon": -46.63, "place": "São Paulo"}, "app": {}});
        for (name, period) in [("bom_dia", Period::Morning), ("boa_tarde", Period::Afternoon), ("boa_noite", Period::Night)] {
            let mut greeting = Greeting::sample(period);
            let item = greeting.island_item(&Env { cfg: &cfg, key: "saudacao" }).unwrap();
            let dir = std::env::temp_dir().join("widgets-previa").join(name);
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();

            // a ilha na barra de tarefas (na tela de verdade, para as contas de onde ela cabe)
            let work = win::work_areas()[0];
            let mut isl = island(Shape::Compact, Morph::None, 1.0);
            isl.media = false;
            isl.docked = true;
            isl.s = S * SIZE;
            let compact_h = Shape::Compact.size().h * isl.s;
            isl.anchor = (((work.0 + work.2) / 2) as f32, work.3 as f32 - compact_h - 40.0);
            isl.multi.entries = vec![entry("saudacao", item)];
            isl.multi.key = "saudacao";
            isl.multi.prev_key = "saudacao";
            isl.multi.bubbles_at = Instant::now() - Duration::from_secs(5);
            isl.hot = None;
            // onde a âncora da ilha cai no quadro (em pontos): perto da direita, no meio da barra
            let compact_w = Shape::Compact.size().w * SIZE;
            let (xa, ya) = (W - 70.0 - compact_w / 2.0, H - 24.0 - Shape::Compact.size().h * SIZE / 2.0);
            let origin = (isl.anchor.0 - xa * S, isl.anchor.1 - ya * S);

            struct Clocks {
                t: f64,
                anim: f64,
                shape: f64,
                pop: Option<(Pop, f64)>,
                /// quando a saudação apareceu (o relógio dela anda junto com o vídeo)
                greeted: f64,
                visible: bool,
                n: u32,
            }
            let mut k = Clocks { t: 0.0, anim: 0.0, shape: -5000.0, pop: None, greeted: 0.0, visible: false, n: 0 };
            let sync = |isl: &mut Island, greeting: &mut Greeting, k: &Clocks| {
                let base = Instant::now();
                let ago = |ms: f64| base - Duration::from_secs_f64(ms.max(0.0) / 1000.0);
                isl.anim_at = ago(k.t - k.anim);
                isl.shape_at = ago(k.t - k.shape);
                isl.multi.pop = k.pop.map(|(kind, at)| (kind, ago(k.t - at)));
                greeting.preview_at((k.t - k.greeted) / 1000.0);
            };
            let frame = |isl: &mut Island, greeting: &Greeting, k: &mut Clocks, caption: &str| {
                let mut c = Canvas::new((W * S) as u32, (H * S) as u32, S);
                c.gradient_round_rect(0.0, 0.0, W, H, 0.0, &[(0.0, Rgba::rgb(38, 52, 104)), (0.55, Rgba::rgb(92, 64, 128)), (1.0, Rgba::rgb(196, 112, 120))], 0.0, H);
                c.fill_round_rect(0.0, H - 48.0, W, H, 0.0, Rgba(22, 22, 28, 235));
                c.fill_round_rect(0.0, H - 48.0, W, H - 47.0, 0.0, Rgba(255, 255, 255, 22));
                // o botão Iniciar, à direita da ilha
                let (sx, sy) = (xa + compact_w / 2.0 + 10.0 * SIZE + 14.0, H - 24.0);
                for (dx, dy) in [(-5.5f32, -5.5f32), (0.5, -5.5), (-5.5, 0.5), (0.5, 0.5)] {
                    c.fill_round_rect(sx + dx, sy + dy, sx + dx + 5.0, sy + dy + 5.0, 0.6, Rgba::rgb(86, 170, 255));
                }
                for (j, x) in [sx + 26.0, sx + 52.0, sx + 78.0].iter().enumerate() {
                    c.fill_round_rect(*x - 8.0, sy - 8.0, *x + 8.0, sy + 8.0, 3.0, Rgba(255, 255, 255, [40u8, 30, 30][j]));
                }
                c.text(18.0, 26.0, period.hello_text(), Face::Semibold, 17.0, WHITE, Anchor::LM, None);
                c.text(18.0, 48.0, caption, Face::Regular, 12.0, Rgba(255, 255, 255, 190), Anchor::LM, None);
                if k.visible {
                    let scene = Scene { info: None, kinds: vec![greeting], cfg: &cfg };
                    let (img, pos) = isl.render(&scene);
                    let (wa, _) = isl.pop_alpha();
                    c.image_px(&img, (pos.0 as f32 - origin.0).round(), (pos.1 as f32 - origin.1).round(), img.w as f32, img.h as f32, wa);
                }
                crate::gfx::save_png(&c.finish(), &dir.join(format!("f{:05}.png", k.n))).unwrap();
                k.n += 1;
            };
            let step = 1000.0 / FPS;
            // roda o vídeo por `ms`, com `caption` embaixo do título
            let play = |isl: &mut Island, greeting: &mut Greeting, k: &mut Clocks, ms: f64, caption: &str| {
                let until = k.t + ms;
                while k.t < until {
                    if let Some((Pop::Appear, at)) = k.pop {
                        if k.t - at >= APPEAR_MS as f64 {
                            k.pop = None;
                        }
                    }
                    sync(isl, greeting, k);
                    frame(isl, greeting, k, caption);
                    k.t += step;
                }
            };
            let shape = |isl: &mut Island, greeting: &mut Greeting, k: &mut Clocks, to: Shape| {
                sync(isl, greeting, k);
                isl.set_shape(to);
                k.anim = k.t;
                k.shape = k.t;
            };
            let appear = |isl: &mut Island, k: &mut Clocks| {
                k.visible = true;
                isl.to = isl.target(Shape::Compact);
                isl.from = isl.drop_of(isl.to, 0.5);
                isl.anim_ms = APPEAR_MS;
                k.anim = k.t;
                k.greeted = k.t;
                k.pop = Some((Pop::Appear, k.t));
            };
            let vanish = |isl: &mut Island, greeting: &mut Greeting, k: &mut Clocks| {
                sync(isl, greeting, k);
                isl.start_vanish(true, Instant::now());
                k.anim = k.t;
                k.pop = Some((Pop::Vanish, k.t));
            };

            let caption_a = "Sozinha: o tempo e as cotações se alternam a cada 7 s";
            play(&mut isl, &mut greeting, &mut k, 700.0, caption_a);
            // nasce: a gota acende e estica direto com a saudação (o aviso estica a ilha)
            appear(&mut isl, &mut k);
            isl.multi.banner = Some(("saudacao", Instant::now()));
            play(&mut isl, &mut greeting, &mut k, 40.0, caption_a);
            shape(&mut isl, &mut greeting, &mut k, Shape::Banner);
            play(&mut isl, &mut greeting, &mut k, 4000.0, caption_a);
            isl.multi.banner = None;
            shape(&mut isl, &mut greeting, &mut k, Shape::Compact);
            // até o tempo da saudação acabar (28 s desde que apareceu)
            let left = 28_000.0 - (k.t - k.greeted);
            play(&mut isl, &mut greeting, &mut k, left, caption_a);
            // some: o conteúdo apaga, encolhe numa gota e a gota some
            vanish(&mut isl, &mut greeting, &mut k);
            play(&mut isl, &mut greeting, &mut k, VANISH_MS as f64 + 40.0, caption_a);
            k.visible = false;
            k.pop = None;
            play(&mut isl, &mut greeting, &mut k, 900.0, caption_a);

            // de novo, para mostrar com o mouse em cima e aberta
            let caption_b = "Com o mouse em cima (também alterna) e aberta com um clique";
            appear(&mut isl, &mut k);
            play(&mut isl, &mut greeting, &mut k, 900.0, caption_b);
            shape(&mut isl, &mut greeting, &mut k, Shape::Medium);
            play(&mut isl, &mut greeting, &mut k, 8600.0, caption_b);
            shape(&mut isl, &mut greeting, &mut k, Shape::Large);
            play(&mut isl, &mut greeting, &mut k, 4200.0, caption_b);
            shape(&mut isl, &mut greeting, &mut k, Shape::Compact);
            play(&mut isl, &mut greeting, &mut k, 800.0, caption_b);
            vanish(&mut isl, &mut greeting, &mut k);
            play(&mut isl, &mut greeting, &mut k, VANISH_MS as f64 + 40.0, caption_b);
            k.visible = false;
            k.pop = None;
            play(&mut isl, &mut greeting, &mut k, 700.0, caption_b);
            println!("{} ({} quadros)", dir.display(), k.n);
        }
    }

    /// Desenha as formas da ilha num PNG: cargo test island -- --ignored
    #[test]
    #[ignore]
    fn draw_island() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let mut art = Canvas::new(200, 200, 1.0);
        art.gradient_round_rect(0.0, 0.0, 200.0, 200.0, 0.0, &[(0.0, Rgba::rgb(58, 24, 112)), (0.6, Rgba::rgb(214, 64, 120)), (1.0, Rgba::rgb(255, 140, 66))], 0.0, 200.0);
        art.fill_circle(100.0, 115.0, 52.0, Rgba::rgb(255, 210, 92));
        let info = IslandInfo {
            title: "Sunset Drive".into(),
            artist: "Neon Avenue  ·  Night Shift".into(),
            app: "Spotify".into(),
            art: Some(Arc::new(art.finish())),
            colors: [Rgba::rgb(255, 160, 80), Rgba::rgb(240, 90, 150)],
            playing: true,
            position: 96.0,
            duration: 231.0,
            can_prev: true,
            can_toggle: true,
            can_next: true,
            can_seek: true,
            volume: Some(0.62),
            volume_hud: 0.0,
        };
        let with_volume = IslandInfo { volume_hud: 1.0, ..info.clone() };
        for shape in [Shape::Compact, Shape::Medium, Shape::Large] {
            let mut isl = island(shape, Morph::None, 1.0);
            let (img, _) = isl.render(&Scene::media(Some(&with_volume)));
            let path = std::env::temp_dir().join(format!("ilha_volume_{shape:?}.png"));
            crate::gfx::save_png(&img, &path).unwrap();
            println!("{}", path.display());
        }
        for (shape, morph, p, name) in [
            (Shape::Compact, Morph::None, 1.0, "compacta"),
            (Shape::Medium, Morph::None, 1.0, "media"),
            (Shape::Large, Morph::None, 1.0, "grande"),
            (Shape::Compact, Morph::In, 0.35, "virando"),
        ] {
            let mut isl = island(shape, morph, p);
            let (img, _) = isl.render(&Scene::media(Some(&info)));
            let path = std::env::temp_dir().join(format!("ilha_{name}.png"));
            crate::gfx::save_png(&img, &path).unwrap();
            println!("{}", path.display());
        }
    }
}
