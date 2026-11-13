//! Ilha flutuante: o Tocando agora vira uma pílula preta por cima de todas as janelas, que dá
//! para arrastar para qualquer lugar da tela. Parada, mostra a capa e ondas no ritmo do som; com
//! o mouse em cima, estica e mostra a música, tocar e avançar; com um clique, abre inteira, com a
//! barra de progresso. "Voltar para a área de trabalho" (ou o clique direito) devolve o widget
//! ao lugar dele, que fica guardado enquanto isso.
//! (Módulo filho de `app`: mexe direto no estado do App.)

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;

use super::*;
use crate::drawing;
use crate::gfx::{Anchor, Canvas, Face, Image};
use crate::widgets::media::{self, fmt_time};
use crate::widgets::{island_act, IslandInfo};

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
/// distância do alto da tela, no lugar padrão
const TOP_GAP: f32 = 10.0;

pub const M_ISLAND_BACK: u32 = 300_040;
pub const M_ISLAND_OPEN: u32 = 300_041;
pub const M_ISLAND_CENTER: u32 = 300_042;

const BLACK: Rgba = Rgba::rgb(0, 0, 0);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    Compact,
    Medium,
    Large,
}

impl Shape {
    /// (largura, altura, raio) em pixels lógicos
    fn size(self) -> (f32, f32, f32) {
        match self {
            Shape::Compact => (236.0, 42.0, 21.0),
            Shape::Medium => (340.0, 78.0, 30.0),
            Shape::Large => (364.0, 220.0, 38.0),
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

/// Botões de cada forma, em pixels lógicos a partir do canto da pílula.
fn buttons(shape: Shape, lw: f32, lh: f32) -> Vec<(Btn, Rect4)> {
    let circle = |cx: f32, cy: f32, r: f32| (cx - r, cy - r, cx + r, cy + r);
    match shape {
        Shape::Compact => Vec::new(),
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
}

impl Island {
    fn progress(&self) -> f32 {
        if self.anim_ms <= 0.0 { 1.0 } else { (self.anim_at.elapsed().as_secs_f32() * 1000.0 / self.anim_ms).min(1.0) }
    }

    fn animating(&self) -> bool {
        self.progress() < 1.0 || self.shape_at.elapsed().as_secs_f32() < 0.5
    }

    fn geo(&self) -> Geo {
        let t = self.progress();
        if t >= 1.0 {
            return self.to;
        }
        // entre as formas da ilha, mola (estica e volta, como uma ilha de verdade); voando entre
        // a ilha e o cartão, sem passar do ponto
        let e = if self.morph == Morph::None { ease_spring(t) } else { ease_settle(t) };
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
    /// para cima. Nunca sai da tela.
    fn target(&self, shape: Shape) -> Geo {
        let s = self.s;
        let (w, h, r) = shape.size();
        let (w, h, r) = (w * s, h * s, r * s);
        let compact_h = Shape::Compact.size().1 * s;
        let (scr, _) = win::monitor_at(self.anchor.0 as i32, (self.anchor.1 + compact_h / 2.0) as i32);
        let lower = self.anchor.1 + compact_h / 2.0 > (scr.1 + scr.3) as f32 / 2.0;
        let y = if lower { self.anchor.1 + compact_h - h } else { self.anchor.1 };
        let x = self.anchor.0 - w / 2.0;
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

    /// Botão sob (x, y) da janela (pixels), na forma atual.
    fn btn_at(&self, x: i32, y: i32, info: Option<&IslandInfo>) -> Option<Btn> {
        if self.morph != Morph::None || info.is_none() {
            return None;
        }
        let g = self.geo();
        let pad = (PAD * self.s).round();
        let (lx, ly) = ((x as f32 - pad) / self.s, (y as f32 - pad) / self.s);
        let (lw, lh) = (g.w / self.s, g.h / self.s);
        let info = info?;
        buttons(self.shape, lw, lh).into_iter().find(|(_, r)| inside(*r, lx, ly)).and_then(|(b, r)| match b {
            Btn::Seek(_) if !info.can_seek || info.duration <= 0.0 => None,
            Btn::Seek(_) => Some(Btn::Seek((((lx - r.0) / (r.2 - r.0)).clamp(0.0, 1.0) * 1000.0) as u32)),
            Btn::Prev if !info.can_prev => None,
            Btn::Next if !info.can_next => None,
            Btn::Toggle if !info.can_toggle => None,
            b => Some(b),
        })
    }

    fn art(&mut self, art: &Arc<Image>, px: u32) -> Image {
        let key = Arc::as_ptr(art) as usize;
        if self.art_cache.as_ref().map(|c| (c.0, c.1)) != Some((key, px)) {
            self.art_cache = Some((key, px, media::cover_image(art, px, px)));
        }
        self.art_cache.as_ref().unwrap().2.clone()
    }

    /// A pílula desenhada e onde fica a janela.
    fn render(&mut self, info: Option<&IslandInfo>) -> (Image, (i32, i32)) {
        let g = self.geo();
        let s = self.s;
        let pad = (PAD * s).round();
        let (ww, wh) = ((g.w + 2.0 * pad).round() as u32, (g.h + 2.0 * pad).round() as u32);
        let mut c = Canvas::new(ww, wh, s);
        let (ox, oy, lw, lh, lr) = (pad / s, pad / s, g.w / s, g.h / s, g.r / s);
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
        if a_prev > 0.01 && self.prev_shape != self.shape {
            self.draw_layer(&mut c, self.prev_shape, info, ox, oy, lw, lh, a_prev);
        }
        if a_cur > 0.01 {
            self.draw_layer(&mut c, self.shape, info, ox, oy, lw, lh, a_cur);
        }
        if let (Some(i), Morph::None) = (info, self.morph) {
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
        (c.finish(), ((g.x - pad).round() as i32, (g.y - pad).round() as i32))
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_layer(&mut self, c: &mut Canvas, shape: Shape, info: Option<&IslandInfo>, ox: f32, oy: f32, lw: f32, lh: f32, a: f32) {
        let fade = |col: Rgba| Rgba(col.0, col.1, col.2, (col.3 as f32 * a) as u8);
        let Some(info) = info else {
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
            Shape::Compact => {
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
                        Btn::Seek(_) => {}
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
            Shape::Compact => (ox + 42.0, ox + lw - 50.0, oy + lh / 2.0, oy + lh / 2.0 - 11.0, oy + lh / 2.0 + 11.0),
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

    fn media_index(&self) -> Option<usize> {
        self.find(KEY)
    }

    fn island_info(&self) -> (Option<IslandInfo>, Option<f32>) {
        match self.media_index() {
            Some(j) => (self.cards[j].kind.island(), self.cards[j].kind.sound_level()),
            None => (None, None),
        }
    }

    /// Lugar padrão: no alto e no meio do monitor onde o widget está.
    /// O espaço que a ilha ocupa aberta (do tamanho grande), para a arrumação deixar livre.
    pub(super) fn island_keep_clear(&self) -> Option<Rect> {
        let isl = self.island.as_ref().filter(|i| !i.hidden)?;
        let g = isl.target(Shape::Large);
        Some((g.x.floor() as i32, g.y.floor() as i32, (g.x + g.w).ceil() as i32, (g.y + g.h).ceil() as i32))
    }

    fn island_default_anchor(&self, near: Rect, s: f32) -> (f32, f32) {
        let work = win::area_for(near);
        ((work.0 + work.2) as f32 / 2.0, work.1 as f32 + TOP_GAP * s)
    }

    /// O Tocando agora vira a ilha (`animate`: a transformação a partir do cartão).
    pub(super) fn open_island(&mut self, animate: bool) {
        if self.island.is_some() {
            return;
        }
        let Some(j) = self.media_index() else { return };
        let card_rect = self.cards[j].card_rect();
        let s0 = self.cards[j].s;
        let saved = self.sh.cfg[KEY]["ilha_pos"].as_array().and_then(|a| Some((a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32)));
        let anchor = match saved {
            Some((x, y)) if win::on_screen(x as i32, (y + 10.0) as i32) => (x, y),
            _ => self.island_default_anchor(card_rect, s0),
        };
        let s = island_scale(anchor.0 as i32, (anchor.1 + 10.0) as i32);
        let info = self.cards[j].kind.island();
        let card_color = info.as_ref().map(|i| i.colors[0].mix(BLACK, 0.55)).unwrap_or(Rgba::rgb(40, 30, 60));
        let card_face = (animate && self.sh.animations).then(|| self.cards[j].face(&self.sh));
        let hwnd = events::create_layered(CARD_CLASS, false);
        let now = Instant::now();
        let mut isl = Island {
            hwnd,
            shape: Shape::Compact,
            prev_shape: Shape::Compact,
            shape_at: now - Duration::from_secs(1),
            from: Geo { x: 0.0, y: 0.0, w: 1.0, h: 1.0, r: 0.0 },
            to: Geo { x: 0.0, y: 0.0, w: 1.0, h: 1.0, r: 0.0 },
            anim_at: now,
            anim_ms: if animate && self.sh.animations { MORPH_MS } else { 0.0 },
            morph: if animate && self.sh.animations { Morph::In } else { Morph::None },
            anchor,
            s,
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
            card_color,
            art_cache: None,
            card_face,
        };
        isl.to = isl.target(Shape::Compact);
        isl.from = if animate {
            let r = self.cards[j].radius_px(self.cards[j].mode);
            Geo { x: card_rect.0 as f32, y: card_rect.1 as f32, w: (card_rect.2 - card_rect.0) as f32, h: (card_rect.3 - card_rect.1) as f32, r }
        } else {
            isl.to
        };
        let (img, pos) = isl.render(info.as_ref());
        win::blit(hwnd, &img, Some(pos), 255);
        win::set_z(hwnd, Z::Topmost);
        events::show(hwnd, !self.sh.focus);
        // o cartão fica guardado (escondido) no lugar dele
        self.tooltip.hide();
        events::show(self.cards[j].hwnd, false);
        self.island = Some(isl);
        self.sh.cfg.obj_mut(KEY).insert("ilha".into(), json!(true));
        self.sh.save();
    }

    /// A ilha volta a ser o cartão (`animate`: voando de volta para o lugar dele).
    pub(super) fn close_island(&mut self, animate: bool) {
        let Some(j) = self.media_index() else {
            self.drop_island();
            return;
        };
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

    fn finish_close(&mut self) {
        self.drop_island();
        self.sh.cfg.obj_mut(KEY).insert("ilha".into(), json!(false));
        self.sh.save();
        if let Some(j) = self.media_index() {
            events::show(self.cards[j].hwnd, !self.sh.focus && !self.cards[j].stacked);
            self.cards[j].refresh(&self.sh);
            self.cards[j].present(&self.sh, true);
            self.send_to_back(j);
        }
    }

    /// Some com a ilha sem animação (o widget foi fechado, o app vai sair...).
    pub(super) fn drop_island(&mut self) {
        if let Some(isl) = self.island.take() {
            events::destroy(isl.hwnd);
        }
    }

    pub(super) fn island_deadline(&self) -> Option<Instant> {
        self.island.as_ref().map(|i| i.next_frame)
    }

    fn save_island_pos(&mut self) {
        if let Some(isl) = &self.island {
            let (x, y) = isl.anchor;
            self.sh.cfg.obj_mut(KEY).insert("ilha_pos".into(), json!([x.round(), y.round()]));
            self.sh.save();
        }
    }

    pub(super) fn island_mouse(&mut self, kind: MouseKind, x: i32, y: i32, sx: i32, sy: i32) {
        let (info, _) = self.island_info();
        let Some(isl) = &mut self.island else { return };
        if isl.morph == Morph::Out {
            return;
        }
        match kind {
            MouseKind::Down => {
                isl.press = Some((sx, sy, isl.btn_at(x, y, info.as_ref()), isl.anchor));
            }
            MouseKind::Move => {
                if let Some((px, py, btn, start)) = isl.press {
                    let far = (((sx - px).pow(2) + (sy - py).pow(2)) as f32).sqrt() >= DRAG_START * isl.s;
                    if !isl.dragging && far && btn.is_none() {
                        isl.dragging = true;
                        events::set_cursor(isl.hwnd, Cursor::Move);
                    }
                    if isl.dragging {
                        let mut anchor = (start.0 + (sx - px) as f32, start.1 + (sy - py) as f32);
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
                let hot = isl.btn_at(x, y, info.as_ref());
                if hot.as_ref().map(std::mem::discriminant) != isl.hot.as_ref().map(std::mem::discriminant) {
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
                let app = info.as_ref().map(|i| i.app.clone()).unwrap_or_else(|| "player".into());
                let items = vec![
                    MenuItem::Cmd("Voltar para a área de trabalho".into(), M_ISLAND_BACK),
                    MenuItem::Cmd(format!("Abrir no {app}"), M_ISLAND_OPEN),
                    MenuItem::Sep,
                    MenuItem::Cmd("Pôr no alto, no meio da tela".into(), M_ISLAND_CENTER),
                ];
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
        if btn == Btn::Back {
            self.close_island(true);
            return;
        }
        let Some(j) = self.media_index() else { return };
        let action = match btn {
            Btn::Toggle => island_act::TOGGLE,
            Btn::Next => island_act::NEXT,
            Btn::Prev => island_act::PREV,
            Btn::Open => island_act::OPEN_PLAYER,
            Btn::Seek(f) => island_act::SEEK + f.min(1000),
            Btn::Back => return,
        };
        self.with_kind(j, |k, ctx| k.act(action, ctx));
        if let Some(isl) = &mut self.island {
            isl.next_frame = Instant::now();
        }
    }

    /// Comandos do menu da ilha (clique direito).
    pub(super) fn island_menu(&mut self, id: u32) -> bool {
        match id {
            M_ISLAND_BACK => self.close_island(true),
            M_ISLAND_OPEN => self.island_button(Btn::Open),
            M_ISLAND_CENTER => {
                let Some(isl) = &self.island else { return true };
                let g = isl.geo();
                let near = (g.x as i32, g.y as i32, (g.x + g.w) as i32, (g.y + g.h) as i32);
                let anchor = self.island_default_anchor(near, isl.s);
                if let Some(isl) = &mut self.island {
                    isl.anchor = anchor;
                    isl.from = isl.geo();
                    isl.to = isl.target(isl.shape);
                    isl.anim_at = Instant::now();
                    isl.anim_ms = STATE_MS;
                    isl.next_frame = Instant::now();
                }
                self.save_island_pos();
            }
            _ => return false,
        }
        true
    }

    pub(super) fn island_wheel(&mut self, delta: i32) {
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
        let anchor = match (visible, card) {
            (true, _) => isl.anchor,
            (false, Some((rect, s))) => self.island_default_anchor(rect, s),
            (false, None) => isl.anchor,
        };
        if let Some(isl) = &mut self.island {
            isl.anchor = anchor;
            isl.s = island_scale(anchor.0 as i32, anchor.1 as i32 + 10);
            isl.snap_to_target();
        }
    }

    /// Um quadro da ilha: forma (mouse em cima, clique fora), ondas, esconder em tela cheia.
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
        if isl.morph == Morph::In && isl.progress() >= 1.0 {
            isl.morph = Morph::None;
        }
        // nada tocando por um bom tempo: volta para a área de trabalho
        if info.as_ref().is_some_and(|i| i.playing) {
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
        // o mouse em cima estica; longe, recolhe
        if isl.morph == Morph::None && !isl.dragging {
            let (px, py) = win::pointer();
            let over = g.contains(px, py);
            if over {
                isl.outside_since = None;
                let since = *isl.inside_since.get_or_insert(now);
                if isl.shape == Shape::Compact && since.elapsed() >= HOVER_IN {
                    isl.set_shape(Shape::Medium);
                }
            } else {
                isl.inside_since = None;
                let since = *isl.outside_since.get_or_insert(now);
                let clicked_away = win::key_down(0x01) || win::key_down(0x02);
                match isl.shape {
                    Shape::Medium if since.elapsed() >= HOVER_OUT => isl.set_shape(Shape::Compact),
                    Shape::Large if clicked_away || since.elapsed() >= LARGE_IDLE => isl.set_shape(Shape::Compact),
                    _ => {}
                }
            }
        }
        isl.step_levels(info.as_ref(), peak);
        if !isl.hidden {
            let (img, pos) = isl.render(info.as_ref());
            win::blit(isl.hwnd, &img, Some(pos), 255);
        }
        let playing = info.as_ref().is_some_and(|i| i.playing);
        let wait = if isl.animating() || isl.dragging {
            16
        } else if playing && !isl.hidden {
            33
        } else {
            90
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
        let (img, pos) = isl.render(Some(info));
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
                card_face: None,
            };
            isl.render(Some(info)).0
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
            card_face: None,
        };
        let card = Geo { x: 500.0, y: 300.0, w: 400.0, h: 188.0, r: 32.0 };
        let pill = Geo { x: 0.0, y: 0.0, w: shape.size().0 * 1.25, h: shape.size().1 * 1.25, r: shape.size().2 * 1.25 };
        isl.from = if morph == Morph::None { pill } else { card };
        isl.to = pill;
        isl
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
            let (img, _) = isl.render(Some(&with_volume));
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
            let (img, _) = isl.render(Some(&info));
            let path = std::env::temp_dir().join(format!("ilha_{name}.png"));
            crate::gfx::save_png(&img, &path).unwrap();
            println!("{}", path.display());
        }
    }
}
