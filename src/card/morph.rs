//! A troca de tamanho de qualquer widget, como a do Claude: o que aparece nos dois tamanhos
//! anda (e muda de tamanho) de um lugar para o outro, o que é só do tamanho de antes some logo
//! no começo e o que é só do novo entra depois.
//!
//! Sem que cada widget precise descrever a troca: a `Canvas` grava o que o widget desenha em
//! cada tamanho, e as duas gravações são ligadas peça a peça. O mesmo texto (o nome da cidade,
//! "24°", "Sunset Drive") é a mesma peça; formas da mesma cor também (o anel verde da bateria, a
//! trilha das barras), na ordem em que aparecem; imagens, pelo formato e pela cor média (a capa
//! do disco, o sol do tempo); uma imagem que cobre o cartão inteiro é o fundo dele (a capa viva
//! da Música) e cresce junto com o cartão, mesmo mudando de proporção. Uma peça que aparece bem
//! mais vezes num tamanho que no outro (o mapa de contribuições do GitHub, com 12 semanas no
//! pequeno e 28 no médio) não tem par: troca esmaecendo, em vez de voar.

use std::collections::HashMap;

use crate::gfx::{Canvas, Clip, Face, Fill, Image, Op, Rec};
use crate::ui::theme::{ease_in_out, ease_spring, Rgba};
use crate::widgets::{Env, Kind, Layout, Mode};

/// O que um widget desenha num tamanho.
pub struct Recording {
    pub ops: Vec<Rec>,
    pub w: f32,
    pub h: f32,
}

/// Grava o desenho do widget no tamanho `mode` (sem pintar nada).
pub fn record(kind: &dyn Kind, mode: Mode, s: f32, text_scale: f32, env: &Env) -> Recording {
    let mut c = Canvas::recorder(s, text_scale);
    kind.draw(&mut c, mode, env);
    let layout = Layout::of(mode);
    Recording { ops: c.take_recording(), w: layout.w, h: layout.h }
}

/// Quando o que é só do tamanho de antes some e o que é só do novo aparece (frações da troca).
/// Os dois se cruzam um pouco: sem isso, num widget que muda quase tudo (a Bateria do pequeno
/// para o médio), o meio da troca ficava vazio.
const OUT: (f32, f32) = (0.0, 0.4);
const IN: (f32, f32) = (0.2, 0.8);
/// uma peça com bem mais cópias num tamanho que no outro (mais que isto vezes) não tem par
const MAX_RATIO: usize = 2;

/// Um quadro da troca, a `p` (0 a 1, no tempo) do caminho de `a` para `b`.
pub fn draw(c: &mut Canvas, a: &Recording, b: &Recording, p: f32) {
    let pairs = pair(a, b);
    let mut used = vec![false; a.ops.len()];
    for i in pairs.iter().flatten() {
        used[*i] = true;
    }
    // (com a mesma mola do cartão: o fundo que cobre o cartão inteiro cresce junto com ele)
    let along = ease_spring(p);
    let out = 1.0 - ease_in_out(((p - OUT.0) / (OUT.1 - OUT.0)).clamp(0.0, 1.0));
    let inn = ease_in_out(((p - IN.0) / (IN.1 - IN.0)).clamp(0.0, 1.0));
    let mut drawer = Drawer { c, clip: None };
    // o que sai, por baixo
    for (i, rec) in a.ops.iter().enumerate() {
        if !used[i] {
            drawer.draw(&rec.op, rec.clip, rec.alpha * out);
        }
    }
    // e o que fica e o que entra, na ordem do tamanho novo
    for (j, rec) in b.ops.iter().enumerate() {
        match pairs[j] {
            Some(i) => {
                let from = &a.ops[i];
                let clip = match (from.clip, rec.clip) {
                    (None, None) => None,
                    (from_clip, to_clip) => Some(lerp_clip(from_clip.unwrap_or(card(a)), to_clip.unwrap_or(card(b)), along)),
                };
                drawer.draw(&lerp(&from.op, &rec.op, along), clip, from.alpha + (rec.alpha - from.alpha) * along);
            }
            None => drawer.draw(&rec.op, rec.clip, rec.alpha * inn),
        }
    }
    drawer.end();
}

/// O cartão inteiro de uma gravação, como recorte (para quem não tinha recorte).
fn card(recording: &Recording) -> Clip {
    Clip { x0: 0.0, y0: 0.0, x1: recording.w, y1: recording.h, radius: 0.0 }
}

/// Desenha as operações abrindo e fechando recortes só quando eles mudam (muitas peças
/// seguidas dividem o mesmo recorte, como as horas do Tempo).
struct Drawer<'a> {
    c: &'a mut Canvas,
    clip: Option<Clip>,
}

impl Drawer<'_> {
    /// Desenha `op` no recorte `clip`, com a opacidade multiplicada por `alpha`.
    fn draw(&mut self, op: &Op, clip: Option<Clip>, alpha: f32) {
        if alpha <= 0.004 {
            return;
        }
        if clip != self.clip {
            if self.clip.is_some() {
                self.c.pop_clip();
            }
            if let Some(Clip { x0, y0, x1, y1, radius }) = clip {
                self.c.push_round_clip(x0, y0, x1, y1, radius);
            }
            self.clip = clip;
        }
        if alpha >= 0.999 {
            self.c.play(op);
        } else {
            self.c.play(&faded(op, alpha));
        }
    }

    /// Fecha o recorte que ficou aberto.
    fn end(&mut self) {
        if self.clip.take().is_some() {
            self.c.pop_clip();
        }
    }
}

// ---------- os pares ----------

/// O que faz duas peças serem "a mesma" nos dois tamanhos.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Text(String, Face),
    Shape(u8, [u8; 8], u8),
    Image(i32, [u8; 3]),
    /// uma imagem que cobre o cartão inteiro: o fundo, que só muda de proporção
    Backdrop,
}

/// Os quatro bytes de uma cor.
fn rgba(color: Rgba) -> [u8; 4] {
    [color.0, color.1, color.2, color.3]
}

/// As duas cores de uma peça (a do começo e a do fim; iguais numa cor só), numa chave.
fn colors(first: Rgba, last: Rgba) -> [u8; 8] {
    let (first, last) = (rgba(first), rgba(last));
    [first[0], first[1], first[2], first[3], last[0], last[1], last[2], last[3]]
}

/// As cores de um preenchimento, numa chave.
fn fill_colors(fill: &Fill) -> [u8; 8] {
    match *fill {
        Fill::Solid(color) => colors(color, color),
        Fill::VGrad(_, _, top, bottom) => colors(top, bottom),
    }
}

/// A primeira e a última cor de um degradê, numa chave.
fn stop_colors(stops: &[(f32, Rgba)]) -> [u8; 8] {
    match (stops.first(), stops.last()) {
        (Some(first), Some(last)) => colors(first.1, last.1),
        _ => [0; 8],
    }
}

/// A cor média de uma imagem, em poucos tons (para reconhecer o mesmo ícone em outro tamanho).
fn mean_color(img: &Image) -> [u8; 3] {
    let pixels = (img.w * img.h) as usize;
    if pixels == 0 {
        return [0; 3];
    }
    // uns 256 pixels espalhados bastam
    let step = (pixels / 256).max(1);
    let (mut sum, mut count) = ([0u64; 3], 0u64);
    for i in (0..pixels).step_by(step) {
        let pixel = &img.px[i * 4..i * 4 + 4];
        if pixel[3] < 16 {
            continue;
        }
        for channel in 0..3 {
            // (de volta da cor pré-multiplicada)
            sum[channel] += pixel[channel] as u64 * 255 / pixel[3] as u64;
        }
        count += 1;
    }
    let count = count.max(1);
    [(sum[0] / count / 48) as u8, (sum[1] / count / 48) as u8, (sum[2] / count / 48) as u8]
}

/// A imagem cobre o cartão (w×h) inteiro?
fn covers(x: f32, y: f32, w: f32, h: f32, card_w: f32, card_h: f32) -> bool {
    x <= 0.5 && y <= 0.5 && x + w >= card_w - 0.5 && y + h >= card_h - 0.5
}

/// A gravação tem uma imagem cobrindo o cartão inteiro (um fundo)?
#[cfg(test)]
pub fn has_backdrop(recording: &Recording) -> bool {
    recording.ops.iter().any(|rec| key(&rec.op, recording.w, recording.h) == Some(Key::Backdrop))
}

/// O que identifica a peça `op` de um cartão w×h.
fn key(op: &Op, card_w: f32, card_h: f32) -> Option<Key> {
    let shape = |tag: u8, cols: [u8; 8]| Some(Key::Shape(tag, cols, 0));
    match op {
        Op::Text { text, face, .. } => Some(Key::Text(text.clone(), *face)),
        // o fundo de um tamanho é o fundo do outro, mesmo com outra proporção: sem par, ele
        // sumiria e o novo apareceria esmaecendo, deixando o cartão transparente no meio
        &Op::Image { x, y, w, h, .. } if covers(x, y, w, h, card_w, card_h) => Some(Key::Backdrop),
        Op::Image { img, w, h, .. } => Some(Key::Image((w / h.max(0.01) * 10.0).round() as i32, mean_color(img))),
        Op::RRect { fill, .. } => shape(1, fill_colors(fill)),
        Op::StrokeRRect { color, .. } => shape(2, colors(*color, *color)),
        Op::GradV { stops, .. } => shape(3, stop_colors(stops)),
        Op::GradH { stops, .. } => shape(4, stop_colors(stops)),
        Op::StrokeV { top, bottom, .. } => shape(5, colors(*top, *bottom)),
        Op::Glow { color, .. } => shape(6, colors(*color, *color)),
        Op::Circle { fill, .. } => shape(7, fill_colors(fill)),
        Op::StrokeCircle { color, .. } => shape(8, colors(*color, *color)),
        Op::Crescent { fill, .. } => shape(9, fill_colors(fill)),
        Op::Radial { stops, .. } => shape(10, stop_colors(stops)),
        Op::Path { fill, width, closed, .. } => Some(Key::Shape(11, fill_colors(fill), width.is_some() as u8 * 2 + *closed as u8)),
        Op::Arc { color, .. } => shape(12, colors(*color, *color)),
        Op::HLine { color, .. } => shape(13, colors(*color, *color)),
        Op::VLine { color, .. } => shape(14, colors(*color, *color)),
        // a chuva e a neve: cada quadro é outro
        Op::Segments { .. } => None,
    }
}

/// As duas peças podem andar uma para a outra? (Uma linha só vira outra com o mesmo número
/// de pontos.)
fn compatible(a: &Op, b: &Op) -> bool {
    match (a, b) {
        (Op::Path { pts: pa, .. }, Op::Path { pts: pb, .. }) => pa.len() == pb.len(),
        (Op::GradV { stops: sa, .. }, Op::GradV { stops: sb, .. }) | (Op::GradH { stops: sa, .. }, Op::GradH { stops: sb, .. }) => sa.len() == sb.len(),
        (Op::Radial { stops: sa, .. }, Op::Radial { stops: sb, .. }) => sa.len() == sb.len(),
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

/// Para cada peça do tamanho novo, a do tamanho de antes que é a mesma (None: é nova).
fn pair(ra: &Recording, rb: &Recording) -> Vec<Option<usize>> {
    let (a, b) = (&ra.ops, &rb.ops);
    // cada chave: as peças dela no tamanho de antes e no novo, na ordem em que foram desenhadas
    let mut groups: HashMap<Key, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (i, rec) in a.iter().enumerate() {
        if let Some(found) = key(&rec.op, ra.w, ra.h) {
            groups.entry(found).or_default().0.push(i);
        }
    }
    for (j, rec) in b.iter().enumerate() {
        if let Some(found) = key(&rec.op, rb.w, rb.h) {
            groups.entry(found).or_default().1.push(j);
        }
    }
    let mut out = vec![None; b.len()];
    for (in_a, in_b) in groups.values() {
        let (count_a, count_b) = (in_a.len(), in_b.len());
        if count_a == 0 || count_b == 0 || count_a.max(count_b) > MAX_RATIO * count_a.min(count_b) {
            continue;
        }
        for (&i, &j) in in_a.iter().zip(in_b) {
            if compatible(&a[i].op, &b[j].op) {
                out[j] = Some(i);
            }
        }
    }
    out
}

// ---------- o meio do caminho ----------

/// O número a `t` (0 a 1) do caminho de `from` a `to`.
fn mix(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// O recorte no meio do caminho.
fn lerp_clip(from: Clip, to: Clip, t: f32) -> Clip {
    Clip { x0: mix(from.x0, to.x0, t), y0: mix(from.y0, to.y0, t), x1: mix(from.x1, to.x1, t), y1: mix(from.y1, to.y1, t), radius: mix(from.radius, to.radius, t) }
}

/// O preenchimento no meio do caminho (de tipos diferentes, já o novo).
fn lerp_fill(from: Fill, to: Fill, t: f32) -> Fill {
    match (from, to) {
        (Fill::Solid(from), Fill::Solid(to)) => Fill::Solid(from.mix(to, t)),
        (Fill::VGrad(a0, a1, a_top, a_bottom), Fill::VGrad(b0, b1, b_top, b_bottom)) => Fill::VGrad(mix(a0, b0, t), mix(a1, b1, t), a_top.mix(b_top, t), a_bottom.mix(b_bottom, t)),
        (_, to) => to,
    }
}

/// O degradê no meio do caminho (com outro número de cores, já o novo).
fn lerp_stops(from: &[(f32, Rgba)], to: &[(f32, Rgba)], t: f32) -> Vec<(f32, Rgba)> {
    if from.len() != to.len() {
        return to.to_vec();
    }
    from.iter().zip(to).map(|(stop_a, stop_b)| (mix(stop_a.0, stop_b.0, t), stop_a.1.mix(stop_b.1, t))).collect()
}

/// A peça no meio do caminho de `a` (t = 0) para `b` (t = 1). (Os campos de `b` vêm com um "b"
/// na frente: `bx0` é o `x0` de `b`.)
fn lerp(a: &Op, b: &Op, t: f32) -> Op {
    // no fim, exatamente a do tamanho novo (a imagem dela, não a de mais pixels)
    if t >= 1.0 {
        return b.clone();
    }
    let m = |from: f32, to: f32| mix(from, to, t);
    match (a, b) {
        (&Op::RRect { x0, y0, x1, y1, r, fill }, &Op::RRect { x0: bx0, y0: by0, x1: bx1, y1: by1, r: br, fill: bf }) => {
            Op::RRect { x0: m(x0, bx0), y0: m(y0, by0), x1: m(x1, bx1), y1: m(y1, by1), r: m(r, br), fill: lerp_fill(fill, bf, t) }
        }
        (&Op::StrokeRRect { x0, y0, x1, y1, r, color, width }, &Op::StrokeRRect { x0: bx0, y0: by0, x1: bx1, y1: by1, r: br, color: bc, width: bw }) => {
            Op::StrokeRRect { x0: m(x0, bx0), y0: m(y0, by0), x1: m(x1, bx1), y1: m(y1, by1), r: m(r, br), color: color.mix(bc, t), width: m(width, bw) }
        }
        (Op::GradV { x0, y0, x1, y1, r, stops, g0, g1 }, Op::GradV { x0: bx0, y0: by0, x1: bx1, y1: by1, r: br, stops: bs, g0: bg0, g1: bg1 }) => Op::GradV {
            x0: m(*x0, *bx0),
            y0: m(*y0, *by0),
            x1: m(*x1, *bx1),
            y1: m(*y1, *by1),
            r: m(*r, *br),
            stops: lerp_stops(stops, bs, t),
            g0: m(*g0, *bg0),
            g1: m(*g1, *bg1),
        },
        (Op::GradH { x0, y0, x1, y1, r, stops, g0, g1 }, Op::GradH { x0: bx0, y0: by0, x1: bx1, y1: by1, r: br, stops: bs, g0: bg0, g1: bg1 }) => Op::GradH {
            x0: m(*x0, *bx0),
            y0: m(*y0, *by0),
            x1: m(*x1, *bx1),
            y1: m(*y1, *by1),
            r: m(*r, *br),
            stops: lerp_stops(stops, bs, t),
            g0: m(*g0, *bg0),
            g1: m(*g1, *bg1),
        },
        (&Op::StrokeV { x0, y0, x1, y1, r, top, bottom, width }, &Op::StrokeV { x0: bx0, y0: by0, x1: bx1, y1: by1, r: br, top: bt, bottom: bb, width: bw }) => Op::StrokeV {
            x0: m(x0, bx0),
            y0: m(y0, by0),
            x1: m(x1, bx1),
            y1: m(y1, by1),
            r: m(r, br),
            top: top.mix(bt, t),
            bottom: bottom.mix(bb, t),
            width: m(width, bw),
        },
        (&Op::Glow { x0, y0, x1, y1, r, cx, cy, radius, color, width, veil }, &Op::Glow { x0: bx0, y0: by0, x1: bx1, y1: by1, r: br, cx: bcx, cy: bcy, radius: brad, color: bc, width: bw, veil: bv }) => Op::Glow {
            x0: m(x0, bx0),
            y0: m(y0, by0),
            x1: m(x1, bx1),
            y1: m(y1, by1),
            r: m(r, br),
            cx: m(cx, bcx),
            cy: m(cy, bcy),
            radius: m(radius, brad),
            color: color.mix(bc, t),
            width: m(width, bw),
            veil: m(veil, bv),
        },
        (&Op::Circle { cx, cy, r, fill }, &Op::Circle { cx: bx, cy: by, r: br, fill: bf }) => Op::Circle { cx: m(cx, bx), cy: m(cy, by), r: m(r, br), fill: lerp_fill(fill, bf, t) },
        (&Op::StrokeCircle { cx, cy, r, color, width }, &Op::StrokeCircle { cx: bx, cy: by, r: br, color: bc, width: bw }) => {
            Op::StrokeCircle { cx: m(cx, bx), cy: m(cy, by), r: m(r, br), color: color.mix(bc, t), width: m(width, bw) }
        }
        (&Op::Crescent { cx, cy, r, cx2, cy2, r2, fill }, &Op::Crescent { cx: bx, cy: by, r: br, cx2: bx2, cy2: by2, r2: br2, fill: bf }) => {
            Op::Crescent { cx: m(cx, bx), cy: m(cy, by), r: m(r, br), cx2: m(cx2, bx2), cy2: m(cy2, by2), r2: m(r2, br2), fill: lerp_fill(fill, bf, t) }
        }
        (Op::Radial { cx, cy, rx, ry, stops }, Op::Radial { cx: bx, cy: by, rx: brx, ry: bry, stops: bs }) => {
            Op::Radial { cx: m(*cx, *bx), cy: m(*cy, *by), rx: m(*rx, *brx), ry: m(*ry, *bry), stops: lerp_stops(stops, bs, t) }
        }
        (Op::Path { pts, fill, width, closed }, Op::Path { pts: bp, fill: bf, width: bw, .. }) => Op::Path {
            pts: pts.iter().zip(bp).map(|(from, to)| (m(from.0, to.0), m(from.1, to.1))).collect(),
            fill: lerp_fill(*fill, *bf, t),
            width: match (width, bw) {
                (Some(from), Some(to)) => Some(m(*from, *to)),
                _ => *bw,
            },
            closed: *closed,
        },
        (&Op::Arc { cx, cy, r, start, end, color, width }, &Op::Arc { cx: bx, cy: by, r: br, start: bs, end: be, color: bc, width: bw }) => {
            Op::Arc { cx: m(cx, bx), cy: m(cy, by), r: m(r, br), start: m(start, bs), end: m(end, be), color: color.mix(bc, t), width: m(width, bw) }
        }
        (&Op::HLine { x0, x1, y, color }, &Op::HLine { x0: bx0, x1: bx1, y: by, color: bc }) => Op::HLine { x0: m(x0, bx0), x1: m(x1, bx1), y: m(y, by), color: color.mix(bc, t) },
        (&Op::VLine { x, y0, y1, color }, &Op::VLine { x: bx, y0: by0, y1: by1, color: bc }) => Op::VLine { x: m(x, bx), y0: m(y0, by0), y1: m(y1, by1), color: color.mix(bc, t) },
        (Op::Image { img, x, y, w, h, opacity, .. }, Op::Image { img: bi, x: bx, y: by, w: bw, h: bh, opacity: bo, .. }) => Op::Image {
            // a de mais pixels: reduzir fica bom, ampliar fica borrado
            img: if bi.w * bi.h >= img.w * img.h { bi.clone() } else { img.clone() },
            x: m(*x, *bx),
            y: m(*y, *by),
            w: m(*w, *bw),
            h: m(*h, *bh),
            opacity: m(*opacity, *bo),
            sharp: false,
        },
        (Op::Text { left, base, size, color, .. }, Op::Text { left: bl, base: bb, text, face, size: bs, color: bc }) => Op::Text {
            left: m(*left, *bl),
            base: m(*base, *bb),
            text: text.clone(),
            face: *face,
            // (em quartos de ponto: cada tamanho de fonte novo fica guardado)
            size: (m(*size, *bs) * 4.0).round() / 4.0,
            color: color.mix(*bc, t),
        },
        (_, b) => b.clone(),
    }
}

/// A peça com a opacidade multiplicada por `factor`.
fn faded(op: &Op, factor: f32) -> Op {
    let fade = |color: Rgba| color.with_a((color.3 as f32 * factor).round() as u8);
    let fade_fill = |fill: Fill| match fill {
        Fill::Solid(color) => Fill::Solid(fade(color)),
        Fill::VGrad(y0, y1, top, bottom) => Fill::VGrad(y0, y1, fade(top), fade(bottom)),
    };
    let fade_stops = |stops: &[(f32, Rgba)]| stops.iter().map(|&(position, color)| (position, fade(color))).collect::<Vec<_>>();
    let mut op = op.clone();
    match &mut op {
        Op::RRect { fill, .. } | Op::Circle { fill, .. } | Op::Crescent { fill, .. } | Op::Path { fill, .. } => *fill = fade_fill(*fill),
        Op::StrokeRRect { color, .. }
        | Op::Glow { color, .. }
        | Op::StrokeCircle { color, .. }
        | Op::Arc { color, .. }
        | Op::Segments { color, .. }
        | Op::HLine { color, .. }
        | Op::VLine { color, .. }
        | Op::Text { color, .. } => *color = fade(*color),
        Op::GradV { stops, .. } | Op::GradH { stops, .. } | Op::Radial { stops, .. } => *stops = fade_stops(stops),
        Op::StrokeV { top, bottom, .. } => {
            *top = fade(*top);
            *bottom = fade(*bottom);
        }
        Op::Image { opacity, .. } => *opacity *= factor,
    }
    op
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::{Anchor, Canvas};
    use crate::ui::theme::WHITE;

    fn com() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
    }

    fn rec(f: impl FnOnce(&mut Canvas)) -> Recording {
        let mut c = Canvas::recorder(1.0, 1.0);
        f(&mut c);
        Recording { ops: c.take_recording(), w: 300.0, h: 150.0 }
    }

    /// A gravação guarda cada desenho (o texto já na posição final) com o recorte e a opacidade
    /// em que foi feito, sem pintar nada.
    #[test]
    fn recording_keeps_each_drawing_with_its_clip_and_opacity() {
        com();
        let r = rec(|c| {
            c.fill_round_rect(0.0, 0.0, 100.0, 50.0, 12.0, WHITE);
            c.push_round_clip(10.0, 10.0, 90.0, 40.0, 8.0);
            c.push_opacity(0.5);
            c.text(50.0, 20.0, "Florianópolis", Face::Semibold, 13.0, WHITE, Anchor::MA, None);
            c.pop_clip();
            c.pop_clip();
            c.arc(50.0, 50.0, 20.0, 135.0, 300.0, WHITE, 6.0);
        });
        assert_eq!(r.ops.len(), 3);
        assert!(r.ops[0].clip.is_none() && r.ops[0].alpha == 1.0);
        let t = &r.ops[1];
        assert_eq!(t.clip, Some(Clip { x0: 10.0, y0: 10.0, x1: 90.0, y1: 40.0, radius: 8.0 }));
        assert!((t.alpha - 0.5).abs() < 1e-6);
        match &t.op {
            Op::Text { left, base, text, .. } => {
                assert_eq!(text, "Florianópolis");
                assert!(*left < 50.0 && *base > 20.0, "centrado em 50, abaixo do topo: {left} {base}");
            }
            _ => panic!("texto"),
        }
        assert!(r.ops[2].clip.is_none() && r.ops[2].alpha == 1.0);
    }

    /// As peças iguais (o mesmo texto, a mesma cor) andam de um lugar para o outro; as que só
    /// existem num tamanho somem ou aparecem; e uma peça com muito mais cópias num dos tamanhos
    /// não tem par.
    #[test]
    fn the_same_pieces_move_and_the_others_fade() {
        com();
        let a = rec(|c| {
            c.text(16.0, 16.0, "24°", Face::Numbers, 40.0, WHITE, Anchor::LA, None);
            c.fill_circle(20.0, 100.0, 5.0, Rgba::rgb(0, 200, 0));
            for i in 0..2 {
                c.fill_round_rect(i as f32 * 10.0, 0.0, i as f32 * 10.0 + 8.0, 8.0, 2.0, Rgba::rgb(9, 9, 9));
            }
        });
        let b = rec(|c| {
            c.text(16.0, 16.0, "Céu limpo", Face::Semibold, 12.0, WHITE, Anchor::LA, None);
            c.text(20.0, 18.0, "24°", Face::Numbers, 54.0, WHITE, Anchor::LA, None);
            c.fill_circle(200.0, 100.0, 10.0, Rgba::rgb(0, 200, 0));
            for i in 0..5 {
                c.fill_round_rect(i as f32 * 10.0, 0.0, i as f32 * 10.0 + 8.0, 8.0, 2.0, Rgba::rgb(9, 9, 9));
            }
        });
        let p = pair(&a, &b);
        assert_eq!(p[0], None, "texto novo");
        assert_eq!(p[1], Some(0), "o mesmo texto");
        assert_eq!(p[2], Some(1), "o círculo da mesma cor");
        assert!(p[3..].iter().all(|x| x.is_none()), "2 contra 5 quadradinhos: sem par");
        // no meio do caminho: o texto cresce e o círculo anda
        match lerp(&a.ops[0].op, &b.ops[1].op, 0.5) {
            Op::Text { size, .. } => assert_eq!(size, 47.0),
            _ => panic!(),
        }
        match lerp(&a.ops[1].op, &b.ops[2].op, 0.5) {
            Op::Circle { cx, r, .. } => assert_eq!((cx, r), (110.0, 7.5)),
            _ => panic!(),
        }
        // e o quadro desenha, do começo ao fim
        for p in [0.0, 0.3, 0.6, 1.0] {
            let mut c = Canvas::new(300, 150, 1.0);
            draw(&mut c, &a, &b, p);
            let _ = c.finish();
        }
    }

    /// A imagem que cobre o cartão inteiro (o fundo) tem par no outro tamanho mesmo com outra
    /// proporção e outra cor média; uma imagem menor, não.
    #[test]
    fn the_backdrop_pairs_even_with_another_shape() {
        let img = |w: u32, h: u32, v: u8| {
            let mut i = Image::new(w, h);
            i.px.chunks_mut(4).for_each(|p| p.copy_from_slice(&[v, v, v, 255]));
            i
        };
        let op = |i: Image, x: f32, y: f32, w: f32, h: f32| Rec { op: Op::Image { img: i, x, y, w, h, opacity: 1.0, sharp: false }, clip: None, alpha: 1.0 };
        let a = Recording { ops: vec![op(img(184, 184, 40), -15.0, -15.0, 184.0, 184.0), op(img(20, 40, 200), 10.0, 10.0, 20.0, 40.0)], w: 154.0, h: 154.0 };
        let b = Recording { ops: vec![op(img(380, 184, 120), -32.0, -15.0, 380.0, 184.0), op(img(30, 30, 200), 10.0, 10.0, 30.0, 30.0)], w: 316.0, h: 154.0 };
        assert_eq!(pair(&a, &b), vec![Some(0), None]);
    }

    /// Esmaecer multiplica a opacidade de tudo o que a peça pinta.
    #[test]
    fn fading_multiplies_the_opacity() {
        match faded(&Op::Circle { cx: 0.0, cy: 0.0, r: 1.0, fill: Fill::Solid(Rgba(10, 20, 30, 200)) }, 0.5) {
            Op::Circle { fill: Fill::Solid(c), .. } => assert_eq!(c, Rgba(10, 20, 30, 100)),
            _ => panic!(),
        }
        match faded(&Op::Image { img: Image::new(1, 1), x: 0.0, y: 0.0, w: 1.0, h: 1.0, opacity: 0.8, sharp: false }, 0.5) {
            Op::Image { opacity, .. } => assert!((opacity - 0.4).abs() < 1e-6),
            _ => panic!(),
        }
    }
}
