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
use crate::drawing;
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
/// aberta e o mouse longe: recolhe sozinha
const LARGE_IDLE: Duration = Duration::from_secs(6);
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

const BLACK: Rgba = Rgba::rgb(0, 0, 0);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    Compact,
    /// esticada com um aviso, por alguns segundos
    Banner,
    Medium,
    Large,
}

impl Shape {
    /// (largura, altura, raio) em pixels lógicos
    fn size(self) -> (f32, f32, f32) {
        island_ui::size(self.form())
    }

    fn form(self) -> IslandForm {
        match self {
            Shape::Compact => IslandForm::Compact,
            Shape::Banner => IslandForm::Banner,
            Shape::Medium => IslandForm::Medium,
            Shape::Large => IslandForm::Large,
        }
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
        let m = |p: f32, q: f32| p + (q - p) * t;
        Geo { x: m(a.x, b.x), y: m(a.y, b.y), w: m(a.w, b.w).max(1.0), h: m(a.h, b.h).max(1.0), r: m(a.r, b.r).max(0.0) }
    }

    fn contains(&self, px: i32, py: i32) -> bool {
        let (px, py) = (px as f32, py as f32);
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

type Rect4 = (f32, f32, f32, f32);

fn inside(r: Rect4, x: f32, y: f32) -> bool {
    x >= r.0 && x <= r.2 && y >= r.1 && y <= r.3
}

/// O lugar da pílula fechada na barra de tarefas (centro x e topo y, em pixels da tela): à esquerda
/// do botão Iniciar, no meio da altura da barra. Sem espaço ali (barra alinhada à esquerda), fica
/// antes da área de notificação.
fn taskbar_slot(t: &win::Taskbar, s: f32) -> (f32, f32) {
    let (w, h, _) = Shape::Compact.size();
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
    let t = win::taskbar_at(x, y)?;
    let s = island_scale((t.bar.0 + t.bar.2) / 2, (t.bar.1 + t.bar.3) / 2);
    Some((taskbar_slot(&t, s), s))
}

/// Botões de cada forma, em pixels lógicos a partir do canto da pílula.
fn buttons(shape: Shape, lw: f32, lh: f32) -> Vec<(Btn, Rect4)> {
    let circle = |cx: f32, cy: f32, r: f32| (cx - r, cy - r, cx + r, cy + r);
    match shape {
        Shape::Compact | Shape::Banner => Vec::new(),
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
}

impl Island {
    fn progress(&self) -> f32 {
        if self.anim_ms <= 0.0 { 1.0 } else { (self.anim_at.elapsed().as_secs_f32() * 1000.0 / self.anim_ms).min(1.0) }
    }

    fn animating(&self) -> bool {
        self.progress() < 1.0 || self.shape_at.elapsed().as_secs_f32() < 0.5 || self.multi.pop.is_some()
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
        let p = match self.multi.pop {
            Some((Pop::Appear, at)) => at.elapsed().as_secs_f32() * 1000.0 / APPEAR_MS,
            Some((Pop::Vanish, at)) => at.elapsed().as_secs_f32() * 1000.0 / VANISH_MS,
            None => 1.0,
        };
        p.min(1.0)
    }

    /// Opacidade (da janela, do conteúdo) nascendo ou sumindo. Nascendo, a gota acende primeiro
    /// e o conteúdo aparece quando a pílula já está quase no tamanho; sumindo, o conteúdo apaga
    /// primeiro e a gota some no fim.
    fn pop_alpha(&self) -> (f32, f32) {
        let p = self.pop_progress();
        match self.multi.pop {
            Some((Pop::Appear, _)) => (ramp(p, 0.0, 0.25), ramp(p, 0.4, 0.85)),
            Some((Pop::Vanish, _)) => (1.0 - ramp(p, 0.55, 1.0), 1.0 - ramp(p, 0.0, 0.3)),
            None => (1.0, 1.0),
        }
    }

    /// A gota de onde a ilha nasce (e onde ela some), `k` da altura da pílula `g`: do lado do
    /// botão Iniciar na barra de tarefas; fora dela, no meio.
    fn drop_of(&self, g: Geo, k: f32) -> Geo {
        let d = g.h * k;
        let cx = if self.docked { g.x + g.w - g.h / 2.0 } else { g.x + g.w / 2.0 };
        let cy = g.y + g.h / 2.0;
        Geo { x: cx - d / 2.0, y: cy - d / 2.0, w: d, h: d, r: d / 2.0 }
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
        let (wa, _) = self.pop_alpha();
        // a janela ainda inteira: o conteúdo volta a acender; já apagando, a gota acende de onde
        // estava (o passo do nascer com a mesma opacidade: o inverso da rampa)
        let unease = |y: f32| if y < 0.5 { (y / 4.0).cbrt() } else { 1.0 - (2.0 * (1.0 - y)).cbrt() / 2.0 };
        let back = if wa >= 0.999 { 0.4 } else { 0.25 * unease(wa) };
        self.multi.pop = Some((Pop::Appear, now - Duration::from_millis((back * APPEAR_MS) as u64)));
        self.from = self.geo();
        self.to = self.target(self.shape);
        self.anim_at = now;
        self.anim_ms = STATE_MS;
        self.next_frame = now;
    }

    fn geo(&self) -> Geo {
        let t = self.progress();
        if t >= 1.0 {
            return self.to;
        }
        // entre as formas da ilha, mola (estica e volta, como uma ilha de verdade); voando entre
        // a ilha e o cartão, sem passar do ponto
        let e = if self.vanishing() {
            // sumindo: sai devagar e acelera para dentro da gota, sem mola
            ease_in_out(t)
        } else if self.morph == Morph::None {
            ease_spring(t)
        } else {
            ease_settle(t)
        };
        Geo::lerp(self.from, self.to, e)
    }

    /// Opacidade do cartão desenhado por cima da pílula, na transformação.
    fn face_alpha(&self) -> f32 {
        let p = self.progress();
        match self.morph {
            // indo para a ilha: o cartão some enquanto a ilha aparece
            Morph::In => 1.0 - ease_in_out(((p - 0.1) / 0.45).clamp(0.0, 1.0)),
            // voltando: o cartão aparece enquanto a ilha some (sem caixa vazia no meio) e
            // termina inteiro, no lugar dele
            Morph::Out => ease_in_out(((p - 0.08) / 0.42).clamp(0.0, 1.0)),
            Morph::None => 0.0,
        }
    }

    /// Onde fica a pílula na forma dada: centrada na âncora; na metade de baixo da tela cresce
    /// para cima; na barra de tarefas, para a esquerda. Nunca sai da tela.
    fn target(&self, shape: Shape) -> Geo {
        let s = self.s;
        let (w, h, r) = shape.size();
        let (w, h, r) = (w * s, h * s, r * s);
        let compact_h = Shape::Compact.size().1 * s;
        let (scr, _) = win::monitor_at(self.anchor.0 as i32, (self.anchor.1 + compact_h / 2.0) as i32);
        let lower = self.anchor.1 + compact_h / 2.0 > (scr.1 + scr.3) as f32 / 2.0;
        let y = if lower { self.anchor.1 + compact_h - h } else { self.anchor.1 };
        let compact_w = Shape::Compact.size().0 * s;
        let x = if self.docked { self.anchor.0 + compact_w / 2.0 - w } else { self.anchor.0 - w / 2.0 };
        let (mx, my) = (6.0 * s, 4.0 * s);
        let x = x.clamp(scr.0 as f32 + mx, (scr.2 as f32 - mx - w).max(scr.0 as f32 + mx));
        let y = y.clamp(scr.1 as f32 + my, (scr.3 as f32 - my - h).max(scr.1 as f32 + my));
        Geo { x, y, w, h, r }
    }

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
                let p = self.shape_at.elapsed().as_secs_f32() * 1000.0 / 420.0;
                ((1.0 - p / 0.3).clamp(0.0, 1.0), ((p - 0.25) / 0.5).clamp(0.0, 1.0))
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
        let compact_h = Shape::Compact.size().1 * self.s;
        let (scr, _) = win::monitor_at(self.anchor.0 as i32, (self.anchor.1 + compact_h / 2.0) as i32);
        self.anchor.1 + compact_h / 2.0 > (scr.1 + scr.3) as f32 / 2.0
    }

    /// Quem vai nas bolhas: todos menos o da pílula (esticada com um aviso, a pílula mostra o
    /// aviso e o de sempre vai para a primeira bolha).
    fn bubble_entries(&self) -> Vec<Entry> {
        let shown = self.key_for(self.shape);
        let mut list: Vec<Entry> = self.multi.entries.iter().copied().filter(|e| e.key != shown).collect();
        if shown != self.multi.key {
            if let Some(k) = list.iter().position(|e| e.key == self.multi.key) {
                let e = list.remove(k);
                list.insert(0, e);
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
        let g = self.geo();
        let s = self.s;
        let d = island_ui::size(IslandForm::Bubble).0 * s;
        let gap = BUBBLE_GAP * s;
        let compact_h = Shape::Compact.size().1 * s;
        let cy = if self.lower() { g.y + g.h - compact_h / 2.0 } else { g.y + compact_h / 2.0 };
        (0..self.bubble_count()).map(|k| (g.x - gap - d / 2.0 - k as f32 * (d + gap), cy, d / 2.0)).collect()
    }

    /// A bolha sob o ponto da tela.
    fn bubble_at(&self, sx: i32, sy: i32) -> Option<usize> {
        self.bubbles().iter().position(|&(cx, cy, r)| (sx as f32 - cx).hypot(sy as f32 - cy) <= r)
    }

    /// O mouse está perto das bolhas abertas (no espaço entre elas também)?
    fn near_bubbles(&self, sx: i32, sy: i32) -> bool {
        let b = self.bubbles();
        let (Some(first), Some(last)) = (b.first(), b.last()) else { return false };
        let r = first.2 + BUBBLE_GAP * self.s;
        (sx as f32) >= last.0 - r && (sx as f32) <= first.0 + r && (sy as f32 - first.1).abs() <= r
    }

    /// O mouse está perto da ilha (para responder rápido quando ele chegar)?
    fn near(&self, sx: i32, sy: i32) -> bool {
        let g = self.geo();
        let m = 120.0 * self.s;
        let left = self.bubbles().last().map_or(g.x, |b| b.0 - b.2);
        (sx as f32) >= left - m && (sx as f32) <= g.x + g.w + m && (sy as f32) >= g.y - m && (sy as f32) <= g.y + g.h + m
    }

    /// Botão sob o ponto (sx, sy) da tela, na forma atual.
    fn btn_at(&self, sx: i32, sy: i32, scene: &Scene) -> Option<Btn> {
        if self.morph != Morph::None {
            return None;
        }
        if let Some(k) = self.bubble_at(sx, sy) {
            return Some(Btn::Bubble(k));
        }
        let g = self.geo();
        let (lx, ly) = ((sx as f32 - g.x) / self.s, (sy as f32 - g.y) / self.s);
        let (lw, lh) = (g.w / self.s, g.h / self.s);
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
        let m = &mut self.multi;
        let present = |k: &str| found.iter().any(|e| e.key == k);
        m.touched.retain(|k, _| present(k));
        m.unseen.retain(|k| present(k));
        m.bannered.retain(|k, _| present(k));
        if m.pinned.is_some_and(|(k, _)| !present(k)) {
            m.pinned = None;
        }
        if m.banner.is_some_and(|(k, _)| !present(k)) {
            m.banner = None;
        }
        for e in &found {
            m.touched.entry(e.key).or_insert(now);
            if e.item.alert && m.bannered.get(e.key) != Some(&e.item.alert_id) {
                m.bannered.insert(e.key, e.item.alert_id);
                if !m.unseen.contains(&e.key) {
                    m.unseen.push(e.key);
                }
                if m.banner.is_none() {
                    m.banner = Some((e.key, now));
                }
            }
        }
        m.entries = found;
        rank(&mut m.entries, &m.touched, m.pinned.map(|p| p.0), &m.unseen);
        if !had_bubble && m.entries.len() > 1 {
            m.bubbles_at = now;
        }
        if m.entries.len() <= 2 {
            m.strip = false;
        }
        let first = m.entries.first().map_or(m.key, |e| e.key);
        self.switch_key(first);
    }

    /// A pessoa escolheu este widget (clicou na bolha ou passou o mouse no aviso): ele vai para
    /// a pílula. `sticky`: fica lá até ela escolher outro.
    fn choose(&mut self, key: &'static str, sticky: bool) {
        let m = &mut self.multi;
        m.pinned = Some((key, sticky));
        m.touched.insert(key, Instant::now());
        m.unseen.retain(|k| *k != key);
        if m.banner.is_some_and(|(k, _)| k == key) {
            m.banner = None;
        }
        rank(&mut m.entries, &m.touched, m.pinned.map(|p| p.0), &m.unseen);
        m.strip = false;
        self.switch_key(key);
    }

    fn accent_of(&self, key: &str) -> Rgba {
        self.multi.entries.iter().find(|e| e.key == key).map_or(WHITE, |e| e.item.accent)
    }

    fn art(&mut self, art: &Arc<Image>, px: u32) -> Image {
        let key = Arc::as_ptr(art) as usize;
        if self.art_cache.as_ref().map(|c| (c.0, c.1)) != Some((key, px)) {
            self.art_cache = Some((key, px, media::cover_image(art, px, px)));
        }
        self.art_cache.as_ref().unwrap().2.clone()
    }

    /// A pílula (e as bolhas) desenhada e onde fica a janela.
    fn render(&mut self, scene: &Scene) -> (Image, (i32, i32)) {
        let g = self.geo();
        let s = self.s;
        let pad = (PAD * s).round();
        let bubbles = self.bubbles();
        let (mut x0, mut y0, mut x1, mut y1) = (g.x, g.y, g.x + g.w, g.y + g.h);
        for &(cx, cy, r) in &bubbles {
            x0 = x0.min(cx - r);
            y0 = y0.min(cy - r);
            x1 = x1.max(cx + r);
            y1 = y1.max(cy + r);
        }
        let (wx, wy) = ((x0 - pad).round(), (y0 - pad).round());
        let (ww, wh) = ((x1 + pad - wx).round().max(1.0) as u32, (y1 + pad - wy).round().max(1.0) as u32);
        let mut c = Canvas::new(ww, wh, s);
        let (ox, oy, lw, lh, lr) = ((g.x - wx) / s, (g.y - wy) / s, g.w / s, g.h / s, g.r / s);
        // sombra curta (a ilha fica por cima das janelas: nada de capturar cliques longe dela)
        for (e, a) in [(4.0, 22u8), (2.0, 40)] {
            c.fill_round_rect(ox - e, oy - e + 2.0, ox + lw + e, oy + lh + e + 2.0, lr + e, Rgba(0, 0, 0, a));
        }
        let p = self.progress();
        let bg = match self.morph {
            Morph::In => self.card_color.mix(BLACK, ease_in_out((p * 1.4).min(1.0))),
            Morph::Out => BLACK.mix(self.card_color, ease_in_out(p)),
            Morph::None => BLACK,
        };
        c.fill_round_rect(ox, oy, ox + lw, oy + lh, lr, bg);
        c.stroke_round_rect(ox, oy, ox + lw, oy + lh, lr, WHITE.with_a(26), 1.0 / s);
        c.push_round_clip(ox, oy, ox + lw, oy + lh, lr);
        let (a_prev, a_cur) = self.layer_alpha();
        // nascendo ou sumindo, o conteúdo acende depois e apaga antes da pílula
        let (_, a_pop) = self.pop_alpha();
        let (a_prev, a_cur) = (a_prev * a_pop, a_cur * a_pop);
        let (prev_key, cur_key) = (self.multi.prev_key, self.key_for(self.shape));
        if a_prev > 0.01 && (self.prev_shape != self.shape || prev_key != cur_key) {
            self.draw_layer(&mut c, self.prev_shape, prev_key, scene, ox, oy, lw, lh, a_prev);
        }
        if a_cur > 0.01 {
            self.draw_layer(&mut c, self.shape, cur_key, scene, ox, oy, lw, lh, a_cur);
        }
        if let (Some(i), Morph::None, true, None) = (scene.info, self.morph, cur_key == KEY, self.multi.pop) {
            self.draw_volume(&mut c, i, ox, oy, lw, lh);
        }
        // o cartão de verdade, esticado na forma de agora: na volta, termina exatamente como o
        // cartão que reaparece no lugar
        let fa = self.face_alpha();
        if let (Some(face), true) = (&self.card_face, fa > 0.01) {
            // sem deformar: o cartão cobre a pílula (que vai virando o cartão), centralizado
            let (bw, bh) = (lw * s, lh * s);
            let k = (bw / face.w as f32).max(bh / face.h as f32);
            let (fw, fh) = (face.w as f32 * k, face.h as f32 * k);
            c.image_px(face, (ox * s + (bw - fw) / 2.0).round(), (oy * s + (bh - fh) / 2.0).round(), fw.round(), fh.round(), fa);
        }
        c.pop_clip();
        // esticada com um aviso: um contorno na cor dele
        if self.shape == Shape::Banner || self.prev_shape == Shape::Banner {
            let a = if self.shape == Shape::Banner { a_cur } else { a_prev };
            let col = self.accent_of(self.key_for(Shape::Banner));
            c.stroke_round_rect(ox + 0.75, oy + 0.75, ox + lw - 0.75, oy + lh - 0.75, lr - 0.75, col.alpha(0.75 * a), 1.5);
        }
        // nascendo: um brilho na cor de quem chegou, que acende com a gota e apaga quando a
        // pílula chega no tamanho
        if self.appearing() {
            let glow = (self.pop_progress() * std::f32::consts::PI).sin().max(0.0);
            let col = self.accent_of(self.key_for(self.shape));
            for (e, a) in [(0.0, 0.85), (1.75, 0.3)] {
                c.stroke_round_rect(ox + 0.75 - e, oy + 0.75 - e, ox + lw - 0.75 + e, oy + lh - 0.75 + e, lr - 0.75 + e, col.alpha(a * glow), 1.5);
            }
        }
        self.draw_bubbles(&mut c, &bubbles, scene, wx, wy);
        (c.finish(), (wx as i32, wy as i32))
    }

    /// As bolhas: o widget em miniatura, o pontinho do aviso não visto e quantos mais esperam.
    fn draw_bubbles(&mut self, c: &mut Canvas, bubbles: &[(f32, f32, f32)], scene: &Scene, wx: f32, wy: f32) {
        let s = self.s;
        let fade = (self.multi.bubbles_at.elapsed().as_secs_f32() / 0.28).clamp(0.0, 1.0);
        let list = self.bubble_entries();
        let waiting = list.len().saturating_sub(1);
        for (k, &(cx, cy, r)) in bubbles.iter().enumerate() {
            let Some(e) = list.get(k).copied() else { continue };
            let (bx, by, br) = ((cx - wx) / s, (cy - wy) / s, r / s);
            // as de trás (abertas com o mouse) entram uma depois da outra
            let a = if k == 0 { ease_in_out(fade) } else { 1.0 };
            c.push_opacity(a);
            c.fill_circle(bx, by + 2.0, br + 3.0, Rgba(0, 0, 0, 30));
            c.fill_circle(bx, by + 1.0, br + 1.5, Rgba(0, 0, 0, 44));
            c.fill_circle(bx, by, br, BLACK);
            let hot = self.hot == Some(Btn::Bubble(k));
            c.stroke_circle(bx, by, br - 0.5, WHITE.with_a(if hot { 70 } else { 26 }), if hot { 1.5 } else { 1.0 / s });
            if e.key == KEY {
                if let Some(art) = scene.info.and_then(|i| i.art.clone()) {
                    let d = br * 2.0 - 12.0;
                    let px = (d * s).round() as u32;
                    let id = Arc::as_ptr(&art) as usize;
                    if self.multi.bubble_art.as_ref().map(|c| (c.0, c.1)) != Some((id, px)) {
                        self.multi.bubble_art = Some((id, px, media::cover_image(&art, px, px)));
                    }
                    c.push_round_clip(bx - d / 2.0, by - d / 2.0, bx + d / 2.0, by + d / 2.0, d / 2.0);
                    c.image_px(&self.multi.bubble_art.as_ref().unwrap().2, ((bx - d / 2.0) * s).round(), ((by - d / 2.0) * s).round(), px as f32, px as f32, 1.0);
                    c.pop_clip();
                } else {
                    let col = scene.info.map_or(WHITE, |i| i.colors[0]);
                    c.fill_circle(bx, by, br - 6.0, col);
                }
            } else if let Some(kind) = scene.kind(e.key) {
                kind.island_draw(c, IslandForm::Bubble, (bx - br, by - br, br * 2.0, br * 2.0), None, &scene.env(e.key));
            }
            // um aviso que ninguém viu: o pontinho na cor dele
            if self.multi.unseen.contains(&e.key) {
                let (dx, dy) = (bx + br * 0.72, by - br * 0.72);
                c.fill_circle(dx, dy, 5.5, BLACK);
                c.fill_circle(dx, dy, 4.0, e.item.accent);
            }
            // quantos mais esperam (com o mouse em cima, todos abrem)
            if k == 0 && waiting > 0 && !self.multi.strip {
                let (dx, dy) = (bx - br * 0.72, by - br * 0.72);
                let label = format!("+{waiting}");
                let w = c.text_width(&label, Face::Semibold, 10.0).max(9.0) + 8.0;
                c.fill_round_rect(dx - w / 2.0 - 1.5, dy - 9.0, dx + w / 2.0 + 1.5, dy + 9.0, 9.0, BLACK);
                c.fill_round_rect(dx - w / 2.0, dy - 7.5, dx + w / 2.0, dy + 7.5, 7.5, ACCENT);
                c.text(dx, dy, &label, Face::Semibold, 10.0, WHITE, Anchor::MM, None);
            }
            c.pop_clip();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_layer(&mut self, c: &mut Canvas, shape: Shape, key: &'static str, scene: &Scene, ox: f32, oy: f32, lw: f32, lh: f32, a: f32) {
        if key != KEY {
            let Some(kind) = scene.kind(key) else { return };
            let hot = match self.hot {
                Some(Btn::Widget(n)) if shape == self.shape => Some(n),
                _ => None,
            };
            c.push_opacity(a);
            kind.island_draw(c, shape.form(), (ox, oy, lw, lh), hot, &scene.env(key));
            c.pop_clip();
            return;
        }
        // a música não tem forma de aviso: esticada, fica como fechada
        let shape = if shape == Shape::Banner { Shape::Compact } else { shape };
        let fade = |col: Rgba| Rgba(col.0, col.1, col.2, (col.3 as f32 * a) as u8);
        let Some(info) = scene.info else {
            c.text(ox + lw / 2.0, oy + lh / 2.0, "Nada tocando", Face::Regular, 12.0, fade(MUTED), Anchor::MM, None);
            return;
        };
        let s = c.s;
        let art = |c: &mut Canvas, x: f32, y: f32, size: f32, r: f32, isl: &mut Island| {
            c.push_round_clip(x, y, x + size, y + size, r);
            match &info.art {
                Some(img) => {
                    let px = (size * s).round() as u32;
                    let fitted = isl.art(img, px);
                    c.image_px(&fitted, (x * s).round(), (y * s).round(), px as f32, px as f32, a);
                }
                None => c.fill_round_rect(x, y, x + size, y + size, r, fade(info.colors[0])),
            }
            c.pop_clip();
        };
        let hot_btn = self.hot;
        let hot = |b: Btn| hot_btn.is_some_and(|h| std::mem::discriminant(&h) == std::mem::discriminant(&b));
        match shape {
            Shape::Compact | Shape::Banner => {
                art(c, ox + 8.0, oy + (lh - 26.0) / 2.0, 26.0, 7.0, self);
                self.draw_eq(c, ox + lw - 14.0 - 27.0, oy + lh / 2.0, 18.0, info, a);
            }
            Shape::Medium => {
                art(c, ox + 14.0, oy + (lh - 50.0) / 2.0, 50.0, 12.0, self);
                let tw = lw - 76.0 - 98.0;
                c.text(ox + 76.0, oy + lh / 2.0 - 2.0, &info.title, Face::Semibold, 14.5, fade(FG), Anchor::LS, Some(tw));
                c.text(ox + 76.0, oy + lh / 2.0 + 3.0, &info.artist, Face::Regular, 12.5, fade(MUTED), Anchor::LA, Some(tw));
                for (b, r) in buttons(Shape::Medium, lw, lh) {
                    let (cx, cy) = (ox + (r.0 + r.2) / 2.0, oy + (r.1 + r.3) / 2.0);
                    if hot(b) {
                        c.fill_circle(cx, cy, (r.2 - r.0) / 2.0, fade(WHITE.with_a(34)));
                    }
                    match b {
                        Btn::Toggle if info.playing => drawing::pause(c, cx, cy, 16.0, fade(FG)),
                        Btn::Toggle => drawing::play(c, cx, cy, 16.0, fade(FG)),
                        _ => drawing::skip(c, cx, cy, 15.0, fade(if info.can_next { FG } else { FAINT }), true),
                    }
                }
            }
            Shape::Large => {
                art(c, ox + 20.0, oy + 18.0, 60.0, 14.0, self);
                let tw = lw - 92.0 - 60.0;
                c.text(ox + 92.0, oy + 22.0, &info.title, Face::Semibold, 15.0, fade(FG), Anchor::LA, Some(tw));
                c.text(ox + 92.0, oy + 42.0, &info.artist, Face::Regular, 12.5, fade(MUTED), Anchor::LA, Some(tw));
                c.fill_circle(ox + 95.5, oy + 66.5, 3.5, fade(info.colors[0]));
                c.text(ox + 104.0, oy + 66.5, &info.app, Face::Regular, 11.5, fade(FAINT), Anchor::LM, None);
                self.draw_eq(c, ox + lw - 20.0 - 27.0, oy + 34.0, 18.0, info, a);
                // barra de progresso (engrossa com o mouse em cima)
                let frac = if info.duration > 0.0 { (info.position / info.duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
                let th = if hot(Btn::Seek(0)) { 7.0 } else { 4.0 };
                let (bx0, bx1, by) = (ox + 20.0, ox + lw - 20.0, oy + 96.0);
                c.fill_round_rect(bx0, by - th / 2.0, bx1, by + th / 2.0, th / 2.0, fade(WHITE.with_a(64)));
                if frac > 0.0 {
                    c.fill_round_rect(bx0, by - th / 2.0, bx0 + ((bx1 - bx0) * frac).max(th), by + th / 2.0, th / 2.0, fade(WHITE.with_a(240)));
                }
                c.text(bx0, by + 8.0, &fmt_time(info.position), Face::Regular, 11.0, fade(MUTED), Anchor::LA, None);
                c.text(bx1, by + 8.0, &format!("-{}", fmt_time((info.duration - info.position).max(0.0))), Face::Regular, 11.0, fade(MUTED), Anchor::RA, None);
                for (b, r) in buttons(Shape::Large, lw, lh) {
                    let (cx, cy) = (ox + (r.0 + r.2) / 2.0, oy + (r.1 + r.3) / 2.0);
                    match b {
                        Btn::Toggle => {
                            c.fill_circle(cx, cy, 23.0, fade(if hot(b) { WHITE } else { FG }));
                            if info.playing {
                                drawing::pause(c, cx, cy, 15.0, fade(DARK_GLYPH));
                            } else {
                                drawing::play(c, cx + 1.0, cy, 15.0, fade(DARK_GLYPH));
                            }
                        }
                        Btn::Prev | Btn::Next => {
                            let enabled = if b == Btn::Prev { info.can_prev } else { info.can_next };
                            if hot(b) {
                                c.fill_circle(cx, cy, 18.0, fade(WHITE.with_a(34)));
                            }
                            drawing::skip(c, cx, cy, 17.0, fade(if enabled { FG } else { FAINT }), b == Btn::Next);
                        }
                        Btn::Open => {
                            let col = if hot(b) { FG } else { Rgba::rgb(143, 211, 255) };
                            c.text(ox + 20.0, cy, &format!("Abrir no {}", info.app), Face::Semibold, 12.0, fade(col), Anchor::LM, None);
                        }
                        Btn::Back => {
                            c.fill_round_rect(ox + r.0, oy + r.1, ox + r.2, oy + r.3, 10.0, fade(WHITE.with_a(if hot(b) { 46 } else { 26 })));
                            let label = "Voltar para a área de trabalho";
                            c.text(cx + 8.0, cy, label, Face::Regular, 12.0, fade(FG), Anchor::MM, None);
                            // seta para baixo, entrando na área de trabalho
                            let ax = ox + r.0 + 14.0;
                            c.line(&[(ax, cy - 5.0), (ax, cy + 4.0)], fade(FG), 1.5);
                            c.line(&[(ax - 3.5, cy + 0.5), (ax, cy + 4.0), (ax + 3.5, cy + 0.5)], fade(FG), 1.5);
                        }
                        Btn::Seek(_) | Btn::Widget(_) | Btn::Bubble(_) => {}
                    }
                }
            }
        }
    }

    /// A roda do mouse mudou o volume: uma barra com o alto-falante, por cima do que fica no meio
    /// da pílula (no meio da fechada; no lugar do título da média; no lugar do app da aberta).
    fn draw_volume(&self, c: &mut Canvas, info: &IslandInfo, ox: f32, oy: f32, lw: f32, lh: f32) {
        let (Some(level), a) = (info.volume, info.volume_hud) else { return };
        if a <= 0.01 {
            return;
        }
        // (de onde a onde vai a barra, a altura dela e o bloco de texto que ela cobre)
        let (x0, x1, cy, top, bottom) = match self.shape {
            Shape::Compact | Shape::Banner => (ox + 42.0, ox + lw - 50.0, oy + lh / 2.0, oy + lh / 2.0 - 11.0, oy + lh / 2.0 + 11.0),
            Shape::Medium => (ox + 76.0, ox + lw - 104.0, oy + lh / 2.0, oy + 8.0, oy + lh - 8.0),
            Shape::Large => (ox + 92.0, ox + lw - 64.0, oy + 58.0, oy + 40.0, oy + 76.0),
        };
        // o fundo preto da pílula por cima do texto, que some enquanto a barra aparece
        c.fill_round_rect(x0 - 6.0, top, x1 + 6.0, bottom, 8.0, BLACK.alpha(a));
        let fade = |col: Rgba| col.with_a((col.3 as f32 * a) as u8);
        c.text(x0 + 7.0, cy, "\u{e995}", Face::Icons, 12.0, fade(FG), Anchor::MM, None);
        let (bx0, bx1) = (x0 + 22.0, x1);
        c.fill_round_rect(bx0, cy - 2.0, bx1, cy + 2.0, 2.0, fade(WHITE.with_a(64)));
        c.fill_round_rect(bx0, cy - 2.0, bx0 + ((bx1 - bx0) * level.clamp(0.0, 1.0)).max(4.0), cy + 2.0, 2.0, fade(WHITE.with_a(240)));
    }

    /// Cinco ondas no ritmo do som (alternando as duas cores da capa).
    fn draw_eq(&self, c: &mut Canvas, x: f32, cy: f32, max_h: f32, info: &IslandInfo, a: f32) {
        for (i, level) in self.levels.iter().enumerate() {
            let h = (4.0 + (max_h - 4.0) * level.clamp(0.0, 1.0)).max(3.0);
            let col = info.colors[i % 2];
            let bx = x + i as f32 * 6.0;
            c.fill_round_rect(bx, cy - h / 2.0, bx + 3.0, cy + h / 2.0, 1.5, Rgba(col.0, col.1, col.2, (255.0 * a) as u8));
        }
    }

    /// Ondas do próximo quadro: seguem o som de verdade quando dá para medir.
    fn step_levels(&mut self, info: Option<&IslandInfo>, peak: Option<f32>) {
        let t = self.started.elapsed().as_secs_f32();
        let playing = info.is_some_and(|i| i.playing);
        for i in 0..5 {
            let wave = 0.5 + 0.5 * (t * (5.2 + i as f32 * 1.7) + i as f32 * 1.3).sin();
            let target = match (playing, peak) {
                (false, _) => 0.08,
                (true, Some(p)) => ((p * 1.8).min(1.0) * (0.45 + 0.55 * wave)).max(0.08),
                (true, None) => 0.25 + 0.65 * wave,
            };
            // sobe rápido e desce devagar, como um medidor
            let k = if target > self.levels[i] { 0.6 } else { 0.25 };
            self.levels[i] += (target - self.levels[i]) * k;
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
            isl.from = isl.geo();
            isl.anchor = anchor;
            isl.docked = docked;
            isl.s = island_scale(anchor.0 as i32, anchor.1 as i32 + 10);
            isl.to = isl.target(isl.shape);
            isl.anim_at = Instant::now();
            isl.anim_ms = STATE_MS;
            isl.next_frame = Instant::now();
        }
        self.save_island_pos();
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
        let mut found: Vec<Entry> = Vec::new();
        if config::setting_bool(&self.sh.cfg, "ilha_widgets") {
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
                    found.push(Entry { key, item });
                }
            }
        }
        if self.media_in_island() {
            let (info, _) = self.island_info();
            let mut item = IslandItem::live(info.as_ref().map_or(WHITE, |i| i.colors[0]));
            item.idle = !info.as_ref().is_some_and(|i| i.playing);
            found.push(Entry { key: KEY, item });
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
                if key != KEY {
                    items.push(MenuItem::Cmd(format!("Não mostrar {} na ilha", widgets::title(key)), M_ISLAND_HIDE).icon("close"));
                }
                items.push(MenuItem::Sep);
                items.push(MenuItem::Check("Na barra de tarefas".into(), isl.docked, M_ISLAND_DOCK).icon("pin"));
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

    pub(super) fn island_wheel(&mut self, delta: i32) {
        // a roda mexe no volume só com a música na pílula
        if !self.island.as_ref().is_some_and(|i| i.media && i.key_for(i.shape) == KEY) {
            return;
        }
        let Some(j) = self.media_index() else { return };
        let steps = delta as f32 / 120.0;
        self.with_kind(j, |k, ctx| k.wheel(0.0, 0.0, steps, Mode::M, ctx));
        // a barra de volume aparece já, sem esperar o próximo quadro
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

    /// Um quadro da ilha: forma (mouse em cima, clique fora, aviso chegando), bolhas, ondas,
    /// esconder em tela cheia.
    pub(super) fn island_tick(&mut self) {
        let now = Instant::now();
        if !self.island.as_ref().is_some_and(|i| now >= i.next_frame) {
            return;
        }
        let (info, peak) = self.island_info();
        let focus = self.sh.focus;
        let paused = self.sh.paused.clone();
        let Some(isl) = &mut self.island else { return };
        // terminou de voar de volta: o cartão reaparece
        if isl.morph == Morph::Out && isl.progress() >= 1.0 {
            self.finish_close();
            return;
        }
        // terminou de sumir (ninguém mais na ilha)
        if isl.vanishing() && isl.pop_progress() >= 1.0 {
            self.drop_island();
            self.island_sync_at = now + SYNC_IDLE;
            return;
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
        if !isl.media || info.as_ref().is_some_and(|i| i.playing) {
            isl.quiet_since = None;
        } else if isl.quiet_since.get_or_insert(now).elapsed() >= QUIET {
            self.close_island(true);
            return;
        }
        // em tela cheia no monitor dela (ou no modo foco), some
        let g = isl.geo();
        let (cx, cy) = ((g.x + g.w / 2.0) as i32, (g.y + g.h / 2.0) as i32);
        let hidden = focus || paused.iter().any(|r| cx >= r.0 && cx < r.2 && cy >= r.1 && cy < r.3);
        if hidden != isl.hidden {
            isl.hidden = hidden;
            events::show(isl.hwnd, !hidden);
            if !hidden {
                win::set_z(isl.hwnd, Z::Topmost);
            }
        }
        if !isl.hidden {
            // clicada, a barra de tarefas sobe por cima de tudo: a ilha volta para cima dela
            win::stay_over_taskbar(isl.hwnd);
        }
        // na barra, acompanha o botão Iniciar (ele anda quando abrem e fecham apps)
        if isl.docked && isl.morph == Morph::None && !isl.dragging && !isl.vanishing() && now >= isl.dock_check {
            isl.dock_check = now + DOCK_CHECK;
            if let Some((slot, s)) = dock_slot(isl.anchor.0 as i32, isl.anchor.1 as i32 + 10) {
                if (slot.0 - isl.anchor.0).abs() >= 1.0 || (slot.1 - isl.anchor.1).abs() >= 1.0 || (s - isl.s).abs() > 0.01 {
                    isl.from = isl.geo();
                    isl.anchor = slot;
                    isl.s = s;
                    isl.to = isl.target(isl.shape);
                    isl.anim_at = now;
                    isl.anim_ms = STATE_MS;
                }
            }
        }
        let (px, py) = win::pointer();
        if isl.morph == Morph::None && !isl.dragging && !isl.vanishing() {
            // o mouse nas bolhas: com mais gente esperando, todas abrem
            let on_bubbles = isl.bubble_at(px, py).is_some() || (isl.multi.strip && isl.near_bubbles(px, py));
            if on_bubbles {
                isl.multi.strip_out = None;
                if !isl.multi.strip && isl.bubble_entries().len() > 1 && isl.inside_since.is_some_and(|t| t.elapsed() >= HOVER_IN) {
                    isl.multi.strip = true;
                }
            } else if isl.multi.strip && isl.multi.strip_out.get_or_insert(now).elapsed() >= HOVER_OUT {
                isl.multi.strip = false;
                isl.multi.strip_out = None;
            }
            // o mouse na pílula estica; longe, recolhe
            let over = g.contains(px, py);
            if over || on_bubbles {
                isl.outside_since = None;
                isl.inside_since.get_or_insert(now);
            } else {
                isl.inside_since = None;
            }
            if over {
                let since = isl.inside_since.unwrap_or(now);
                if matches!(isl.shape, Shape::Compact | Shape::Banner) && since.elapsed() >= HOVER_IN {
                    // passou o mouse no aviso: ele fica na pílula enquanto ela está aberta
                    if isl.shape == Shape::Banner {
                        if let Some((k, _)) = isl.multi.banner {
                            isl.choose(k, false);
                        }
                    }
                    isl.set_shape(Shape::Medium);
                }
                // o que está na pílula aberta foi visto
                if matches!(isl.shape, Shape::Medium | Shape::Large) {
                    let key = isl.multi.key;
                    isl.multi.unseen.retain(|k| *k != key);
                }
            } else {
                let since = *isl.outside_since.get_or_insert(now);
                let clicked_away = win::key_down(0x01) || win::key_down(0x02);
                let collapse = match isl.shape {
                    Shape::Medium => since.elapsed() >= HOVER_OUT,
                    Shape::Large => clicked_away || since.elapsed() >= LARGE_IDLE,
                    _ => false,
                };
                if collapse {
                    isl.set_shape(Shape::Compact);
                    // a escolha só por passar o mouse vale enquanto ela estava aberta
                    if isl.multi.pinned.is_some_and(|p| !p.1) {
                        isl.multi.pinned = None;
                        self.island_sync_at = now;
                    }
                }
            }
            // um aviso chegando estica a ilha por alguns segundos
            match isl.multi.banner {
                Some((_, at)) if at.elapsed() < BANNER_FOR => {
                    if isl.shape == Shape::Compact && !over {
                        isl.set_shape(Shape::Banner);
                    }
                }
                Some(_) => {
                    isl.multi.banner = None;
                    if isl.shape == Shape::Banner {
                        isl.set_shape(Shape::Compact);
                    }
                }
                None if isl.shape == Shape::Banner => isl.set_shape(Shape::Compact),
                None => {}
            }
        }
        isl.step_levels(info.as_ref(), peak);
        if !isl.hidden {
            let scene = Scene::of(info.as_ref(), &self.cards, &self.feeds, &self.sh.cfg);
            let (img, pos) = isl.render(&scene);
            let (wa, _) = isl.pop_alpha();
            win::blit(isl.hwnd, &img, Some(pos), (wa * 255.0).round() as u8);
        }
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
        let near = isl.near(px, py);
        let wait = if isl.animating() || isl.dragging || isl.multi.bubbles_at.elapsed() < Duration::from_millis(300) {
            16
        } else if fps > 0 && !isl.hidden {
            (1000 / fps.min(60)) as u64
        } else if playing && !isl.hidden {
            33
        } else if near && !isl.hidden {
            60
        } else {
            // parada e o mouse longe: só o relógio do Pomodoro e os avisos mudam
            250
        };
        isl.next_frame = now + Duration::from_millis(wait);
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
            let (w, h, r) = shape.size();
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
        };
        let card = Geo { x: 500.0, y: 300.0, w: 400.0, h: 188.0, r: 32.0 };
        let pill = Geo { x: 0.0, y: 0.0, w: shape.size().0 * 1.25, h: shape.size().1 * 1.25, r: shape.size().2 * 1.25 };
        isl.from = if morph == Morph::None { pill } else { card };
        isl.to = pill;
        isl
    }

    /// Na barra de tarefas: à esquerda do Iniciar, no meio da altura da barra; com a barra
    /// alinhada à esquerda (sem espaço antes do Iniciar), antes da área de notificação.
    #[test]
    fn taskbar_slot_sits_left_of_start() {
        let s = 1.25 * SIZE;
        let (w, h, _) = Shape::Compact.size();
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
            let compact_h = Shape::Compact.size().1 * isl.s;
            isl.anchor = (((work.0 + work.2) / 2) as f32, work.3 as f32 - compact_h - 40.0);
            isl.multi.entries = vec![entry("saudacao", item)];
            isl.multi.key = "saudacao";
            isl.multi.prev_key = "saudacao";
            isl.multi.bubbles_at = Instant::now() - Duration::from_secs(5);
            isl.hot = None;
            // onde a âncora da ilha cai no quadro (em pontos): perto da direita, no meio da barra
            let compact_w = Shape::Compact.size().0 * SIZE;
            let (xa, ya) = (W - 70.0 - compact_w / 2.0, H - 24.0 - Shape::Compact.size().1 * SIZE / 2.0);
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
            let mut play = |isl: &mut Island, greeting: &mut Greeting, k: &mut Clocks, ms: f64, caption: &str| {
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
