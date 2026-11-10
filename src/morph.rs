//! A troca de tamanho de qualquer widget, como a do Claude: o que aparece nos dois tamanhos
//! anda (e muda de tamanho) de um lugar para o outro, o que é só do tamanho de antes some logo
//! no começo e o que é só do novo entra depois.
//!
//! Sem que cada widget precise descrever a troca: a `Canvas` grava o que o widget desenha em
//! cada tamanho, e as duas gravações são ligadas peça a peça. O mesmo texto (o nome da cidade,
//! "24°", "Sunset Drive") é a mesma peça; formas da mesma cor também (o anel verde da bateria, a
//! trilha das barras), na ordem em que aparecem; imagens, pelo formato e pela cor média (a capa
//! do disco, o sol do tempo). Uma peça que aparece bem mais vezes num tamanho que no outro (o
//! mapa de contribuições do GitHub, com 12 semanas no pequeno e 28 no médio) não tem par: troca
//! esmaecendo, em vez de voar.

use std::collections::HashMap;

use crate::gfx::{Canvas, Clip, Face, Fill, Image, Op, Rec};
use crate::theme::{ease_in_out, ease_spring, Rgba};
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
    let l = Layout::of(mode);
    Recording { ops: c.take_recording(), w: l.w, h: l.h }
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
    let pairs = pair(&a.ops, &b.ops);
    let mut used = vec![false; a.ops.len()];
    for i in pairs.iter().flatten() {
        used[*i] = true;
    }
    // (com a mesma mola do cartão: o fundo que cobre o cartão inteiro cresce junto com ele)
    let e = ease_spring(p);
    let out = 1.0 - ease_in_out(((p - OUT.0) / (OUT.1 - OUT.0)).clamp(0.0, 1.0));
    let inn = ease_in_out(((p - IN.0) / (IN.1 - IN.0)).clamp(0.0, 1.0));
    let mut d = Drawer { c, clip: None };
    // o que sai, por baixo
    for (i, r) in a.ops.iter().enumerate() {
        if !used[i] {
            d.draw(&r.op, r.clip, r.alpha * out);
        }
    }
    // e o que fica e o que entra, na ordem do tamanho novo
    for (j, r) in b.ops.iter().enumerate() {
        match pairs[j] {
            Some(i) => {
                let ra = &a.ops[i];
                let clip = match (ra.clip, r.clip) {
                    (None, None) => None,
                    (ca, cb) => Some(lerp_clip(ca.unwrap_or(card(a)), cb.unwrap_or(card(b)), e)),
                };
                d.draw(&lerp(&ra.op, &r.op, e), clip, ra.alpha + (r.alpha - ra.alpha) * e);
            }
            None => d.draw(&r.op, r.clip, r.alpha * inn),
        }
    }
    d.end();
}

/// O cartão inteiro de uma gravação, como recorte (para quem não tinha recorte).
fn card(r: &Recording) -> Clip {
    (0.0, 0.0, r.w, r.h, 0.0)
}

/// Desenha as operações abrindo e fechando recortes só quando eles mudam (muitas peças
/// seguidas dividem o mesmo recorte, como as horas do Tempo).
struct Drawer<'a> {
    c: &'a mut Canvas,
    clip: Option<Clip>,
}

impl Drawer<'_> {
    fn draw(&mut self, op: &Op, clip: Option<Clip>, alpha: f32) {
        if alpha <= 0.004 {
            return;
        }
        if clip != self.clip {
            if self.clip.is_some() {
                self.c.pop_clip();
            }
            if let Some((x0, y0, x1, y1, r)) = clip {
                self.c.push_round_clip(x0, y0, x1, y1, r);
            }
            self.clip = clip;
        }
        if alpha >= 0.999 {
            self.c.play(op);
        } else {
            self.c.play(&faded(op, alpha));
        }
    }

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
}

fn rgba(c: Rgba) -> [u8; 4] {
    [c.0, c.1, c.2, c.3]
}

fn colors(a: Rgba, b: Rgba) -> [u8; 8] {
    let (a, b) = (rgba(a), rgba(b));
    [a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]]
}

fn fill_colors(f: &Fill) -> [u8; 8] {
    match *f {
        Fill::Solid(c) => colors(c, c),
        Fill::VGrad(_, _, top, bottom) => colors(top, bottom),
    }
}

fn stop_colors(stops: &[(f32, Rgba)]) -> [u8; 8] {
    match (stops.first(), stops.last()) {
        (Some(a), Some(b)) => colors(a.1, b.1),
        _ => [0; 8],
    }
}

/// A cor média de uma imagem, em poucos tons (para reconhecer o mesmo ícone em outro tamanho).
fn mean_color(img: &Image) -> [u8; 3] {
    let n = (img.w * img.h) as usize;
    if n == 0 {
        return [0; 3];
    }
    let step = (n / 256).max(1);
    let (mut sum, mut count) = ([0u64; 3], 0u64);
    for i in (0..n).step_by(step) {
        let p = &img.px[i * 4..i * 4 + 4];
        if p[3] < 16 {
            continue;
        }
        for k in 0..3 {
            // (de volta da cor pré-multiplicada)
            sum[k] += p[k] as u64 * 255 / p[3] as u64;
        }
        count += 1;
    }
    let count = count.max(1);
    [(sum[0] / count / 48) as u8, (sum[1] / count / 48) as u8, (sum[2] / count / 48) as u8]
}

fn key(op: &Op) -> Option<Key> {
    let shape = |tag: u8, cols: [u8; 8]| Some(Key::Shape(tag, cols, 0));
    match op {
        Op::Text { text, face, .. } => Some(Key::Text(text.clone(), *face)),
        Op::Image { img, w, h, .. } => Some(Key::Image((w / h.max(0.01) * 10.0).round() as i32, mean_color(img))),
        Op::RRect { fill, .. } => shape(1, fill_colors(fill)),
        Op::StrokeRRect { c, .. } => shape(2, colors(*c, *c)),
        Op::GradV { stops, .. } => shape(3, stop_colors(stops)),
        Op::GradH { stops, .. } => shape(4, stop_colors(stops)),
        Op::StrokeV { top, bottom, .. } => shape(5, colors(*top, *bottom)),
        Op::Glow { c, .. } => shape(6, colors(*c, *c)),
        Op::Circle { fill, .. } => shape(7, fill_colors(fill)),
        Op::StrokeCircle { c, .. } => shape(8, colors(*c, *c)),
        Op::Crescent { fill, .. } => shape(9, fill_colors(fill)),
        Op::Radial { stops, .. } => shape(10, stop_colors(stops)),
        Op::Path { fill, width, closed, .. } => Some(Key::Shape(11, fill_colors(fill), width.is_some() as u8 * 2 + *closed as u8)),
        Op::Arc { c, .. } => shape(12, colors(*c, *c)),
        Op::HLine { c, .. } => shape(13, colors(*c, *c)),
        Op::VLine { c, .. } => shape(14, colors(*c, *c)),
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
fn pair(a: &[Rec], b: &[Rec]) -> Vec<Option<usize>> {
    let mut groups: HashMap<Key, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (i, r) in a.iter().enumerate() {
        if let Some(k) = key(&r.op) {
            groups.entry(k).or_default().0.push(i);
        }
    }
    for (j, r) in b.iter().enumerate() {
        if let Some(k) = key(&r.op) {
            groups.entry(k).or_default().1.push(j);
        }
    }
    let mut out = vec![None; b.len()];
    for (ia, jb) in groups.values() {
        let (na, nb) = (ia.len(), jb.len());
        if na == 0 || nb == 0 || na.max(nb) > MAX_RATIO * na.min(nb) {
            continue;
        }
        for (&i, &j) in ia.iter().zip(jb) {
            if compatible(&a[i].op, &b[j].op) {
                out[j] = Some(i);
            }
        }
    }
    out
}

// ---------- o meio do caminho ----------

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_clip(a: Clip, b: Clip, t: f32) -> Clip {
    (mix(a.0, b.0, t), mix(a.1, b.1, t), mix(a.2, b.2, t), mix(a.3, b.3, t), mix(a.4, b.4, t))
}

fn lerp_fill(a: Fill, b: Fill, t: f32) -> Fill {
    match (a, b) {
        (Fill::Solid(x), Fill::Solid(y)) => Fill::Solid(x.mix(y, t)),
        (Fill::VGrad(a0, a1, at, ab), Fill::VGrad(b0, b1, bt, bb)) => Fill::VGrad(mix(a0, b0, t), mix(a1, b1, t), at.mix(bt, t), ab.mix(bb, t)),
        (_, b) => b,
    }
}

fn lerp_stops(a: &[(f32, Rgba)], b: &[(f32, Rgba)], t: f32) -> Vec<(f32, Rgba)> {
    if a.len() != b.len() {
        return b.to_vec();
    }
    a.iter().zip(b).map(|(x, y)| (mix(x.0, y.0, t), x.1.mix(y.1, t))).collect()
}

/// A peça no meio do caminho de `a` (t = 0) para `b` (t = 1).
fn lerp(a: &Op, b: &Op, t: f32) -> Op {
    // no fim, exatamente a do tamanho novo (a imagem dela, não a de mais pixels)
    if t >= 1.0 {
        return b.clone();
    }
    let m = |x: f32, y: f32| mix(x, y, t);
    match (a, b) {
        (&Op::RRect { x0, y0, x1, y1, r, fill }, &Op::RRect { x0: bx0, y0: by0, x1: bx1, y1: by1, r: br, fill: bf }) => {
            Op::RRect { x0: m(x0, bx0), y0: m(y0, by0), x1: m(x1, bx1), y1: m(y1, by1), r: m(r, br), fill: lerp_fill(fill, bf, t) }
        }
        (&Op::StrokeRRect { x0, y0, x1, y1, r, c, width }, &Op::StrokeRRect { x0: bx0, y0: by0, x1: bx1, y1: by1, r: br, c: bc, width: bw }) => {
            Op::StrokeRRect { x0: m(x0, bx0), y0: m(y0, by0), x1: m(x1, bx1), y1: m(y1, by1), r: m(r, br), c: c.mix(bc, t), width: m(width, bw) }
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
        (&Op::Glow { x0, y0, x1, y1, r, cx, cy, radius, c, width, veil }, &Op::Glow { x0: bx0, y0: by0, x1: bx1, y1: by1, r: br, cx: bcx, cy: bcy, radius: brad, c: bc, width: bw, veil: bv }) => Op::Glow {
            x0: m(x0, bx0),
            y0: m(y0, by0),
            x1: m(x1, bx1),
            y1: m(y1, by1),
            r: m(r, br),
            cx: m(cx, bcx),
            cy: m(cy, bcy),
            radius: m(radius, brad),
            c: c.mix(bc, t),
            width: m(width, bw),
            veil: m(veil, bv),
        },
        (&Op::Circle { cx, cy, r, fill }, &Op::Circle { cx: bx, cy: by, r: br, fill: bf }) => Op::Circle { cx: m(cx, bx), cy: m(cy, by), r: m(r, br), fill: lerp_fill(fill, bf, t) },
        (&Op::StrokeCircle { cx, cy, r, c, width }, &Op::StrokeCircle { cx: bx, cy: by, r: br, c: bc, width: bw }) => {
            Op::StrokeCircle { cx: m(cx, bx), cy: m(cy, by), r: m(r, br), c: c.mix(bc, t), width: m(width, bw) }
        }
        (&Op::Crescent { cx, cy, r, cx2, cy2, r2, fill }, &Op::Crescent { cx: bx, cy: by, r: br, cx2: bx2, cy2: by2, r2: br2, fill: bf }) => {
            Op::Crescent { cx: m(cx, bx), cy: m(cy, by), r: m(r, br), cx2: m(cx2, bx2), cy2: m(cy2, by2), r2: m(r2, br2), fill: lerp_fill(fill, bf, t) }
        }
        (Op::Radial { cx, cy, rx, ry, stops }, Op::Radial { cx: bx, cy: by, rx: brx, ry: bry, stops: bs }) => {
            Op::Radial { cx: m(*cx, *bx), cy: m(*cy, *by), rx: m(*rx, *brx), ry: m(*ry, *bry), stops: lerp_stops(stops, bs, t) }
        }
        (Op::Path { pts, fill, width, closed }, Op::Path { pts: bp, fill: bf, width: bw, .. }) => Op::Path {
            pts: pts.iter().zip(bp).map(|(p, q)| (m(p.0, q.0), m(p.1, q.1))).collect(),
            fill: lerp_fill(*fill, *bf, t),
            width: match (width, bw) {
                (Some(x), Some(y)) => Some(m(*x, *y)),
                _ => *bw,
            },
            closed: *closed,
        },
        (&Op::Arc { cx, cy, r, start, end, c, width }, &Op::Arc { cx: bx, cy: by, r: br, start: bs, end: be, c: bc, width: bw }) => {
            Op::Arc { cx: m(cx, bx), cy: m(cy, by), r: m(r, br), start: m(start, bs), end: m(end, be), c: c.mix(bc, t), width: m(width, bw) }
        }
        (&Op::HLine { x0, x1, y, c }, &Op::HLine { x0: bx0, x1: bx1, y: by, c: bc }) => Op::HLine { x0: m(x0, bx0), x1: m(x1, bx1), y: m(y, by), c: c.mix(bc, t) },
        (&Op::VLine { x, y0, y1, c }, &Op::VLine { x: bx, y0: by0, y1: by1, c: bc }) => Op::VLine { x: m(x, bx), y0: m(y0, by0), y1: m(y1, by1), c: c.mix(bc, t) },
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
        (Op::Text { left, base, size, c, .. }, Op::Text { left: bl, base: bb, text, face, size: bs, c: bc }) => Op::Text {
            left: m(*left, *bl),
            base: m(*base, *bb),
            text: text.clone(),
            face: *face,
            // (em quartos de ponto: cada tamanho de fonte novo fica guardado)
            size: (m(*size, *bs) * 4.0).round() / 4.0,
            c: c.mix(*bc, t),
        },
        (_, b) => b.clone(),
    }
}

/// A peça com a opacidade multiplicada por `k`.
fn faded(op: &Op, k: f32) -> Op {
    let c = |x: Rgba| x.with_a((x.3 as f32 * k).round() as u8);
    let f = |x: Fill| match x {
        Fill::Solid(a) => Fill::Solid(c(a)),
        Fill::VGrad(y0, y1, top, bottom) => Fill::VGrad(y0, y1, c(top), c(bottom)),
    };
    let st = |s: &[(f32, Rgba)]| s.iter().map(|&(p, x)| (p, c(x))).collect::<Vec<_>>();
    let mut op = op.clone();
    match &mut op {
        Op::RRect { fill, .. } | Op::Circle { fill, .. } | Op::Crescent { fill, .. } | Op::Path { fill, .. } => *fill = f(*fill),
        Op::StrokeRRect { c: x, .. }
        | Op::Glow { c: x, .. }
        | Op::StrokeCircle { c: x, .. }
        | Op::Arc { c: x, .. }
        | Op::Segments { c: x, .. }
        | Op::HLine { c: x, .. }
        | Op::VLine { c: x, .. }
        | Op::Text { c: x, .. } => *x = c(*x),
        Op::GradV { stops, .. } | Op::GradH { stops, .. } | Op::Radial { stops, .. } => *stops = st(stops),
        Op::StrokeV { top, bottom, .. } => {
            *top = c(*top);
            *bottom = c(*bottom);
        }
        Op::Image { opacity, .. } => *opacity *= k,
    }
    op
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::{Anchor, Canvas};
    use crate::theme::WHITE;

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
        assert_eq!(t.clip, Some((10.0, 10.0, 90.0, 40.0, 8.0)));
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
            c.text(16.0, 16.0, "24°", Face::Light, 40.0, WHITE, Anchor::LA, None);
            c.fill_circle(20.0, 100.0, 5.0, Rgba::rgb(0, 200, 0));
            for i in 0..2 {
                c.fill_round_rect(i as f32 * 10.0, 0.0, i as f32 * 10.0 + 8.0, 8.0, 2.0, Rgba::rgb(9, 9, 9));
            }
        });
        let b = rec(|c| {
            c.text(16.0, 16.0, "Céu limpo", Face::Semibold, 12.0, WHITE, Anchor::LA, None);
            c.text(20.0, 18.0, "24°", Face::Light, 54.0, WHITE, Anchor::LA, None);
            c.fill_circle(200.0, 100.0, 10.0, Rgba::rgb(0, 200, 0));
            for i in 0..5 {
                c.fill_round_rect(i as f32 * 10.0, 0.0, i as f32 * 10.0 + 8.0, 8.0, 2.0, Rgba::rgb(9, 9, 9));
            }
        });
        let p = pair(&a.ops, &b.ops);
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
