//! Formas e ícones reaproveitados pelos widgets: anéis de progresso, play, pausa, avançar,
//! reiniciar e os ícones do tempo (sol, nuvens, chuva...).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::gfx::{Canvas, Fill, Image};
use crate::imgops::blur;
use crate::theme::Rgba;

/// Anel de progresso no estilo "anéis de atividade", com pontas arredondadas.
pub fn ring(c: &mut Canvas, cx: f32, cy: f32, diameter: f32, width: f32, frac: f32, color: Rgba) {
    let rc = diameter / 2.0 - width / 2.0; // raio da linha central do anel
    c.stroke_circle(cx, cy, rc, color.alpha(0.28), width);
    if frac <= 0.0 {
        return;
    }
    let end = -90.0 + 360.0 * frac.min(1.0);
    if frac >= 1.0 {
        c.stroke_circle(cx, cy, rc, color, width);
    } else {
        c.arc(cx, cy, rc, -90.0, end, color, width);
        // a ponta brilha um pouco, como uma luz no fim do traço
        let a = end.to_radians();
        let (tx, ty) = (cx + rc * a.cos(), cy + rc * a.sin());
        c.fill_circle(tx, ty, width * 0.95, color.alpha(0.22));
        c.fill_circle(tx, ty, width * 0.62, color.mix(Rgba::rgb(255, 255, 255), 0.35));
    }
}

/// Triângulo com cantos arredondados, no estilo dos ícones da Apple.
pub fn rounded_triangle(c: &mut Canvas, pts: [(f32, f32); 3], r: f32, color: Rgba) {
    let [(ax, ay), (bx, by), (cx, cy)] = pts;
    let a = ((bx - cx).powi(2) + (by - cy).powi(2)).sqrt();
    let b = ((ax - cx).powi(2) + (ay - cy).powi(2)).sqrt();
    let cc = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
    let perimeter = a + b + cc;
    let ix = (a * ax + b * bx + cc * cx) / perimeter;
    let iy = (a * ay + b * by + cc * cy) / perimeter;
    let inradius = ((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)).abs() / perimeter;
    // encolhe o triângulo em r a partir do incentro e o contorna com traço redondo: as bordas
    // voltam ao lugar e as pontas ficam arredondadas
    let f = ((inradius - r) / inradius).max(0.0);
    let sp: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| (ix + (x - ix) * f, iy + (y - iy) * f)).collect();
    c.polygon(&sp, color);
    c.line_with(&sp, Fill::Solid(color), 2.0 * r, true);
}

pub fn play(c: &mut Canvas, cx: f32, cy: f32, size: f32, color: Rgba) {
    let (w, h) = (size * 0.9, size);
    // um triângulo centrado pela caixa parece deslocado para a esquerda; empurrar um pouco
    // para a direita o centraliza visualmente
    let x0 = cx - w / 2.0 + w * 0.09;
    rounded_triangle(c, [(x0, cy - h / 2.0), (x0, cy + h / 2.0), (x0 + w, cy)], size * 0.08, color);
}

pub fn pause(c: &mut Canvas, cx: f32, cy: f32, size: f32, color: Rgba) {
    let (bar, gap, h) = (size * 0.3, size * 0.26, size * 0.92);
    for x in [cx - gap / 2.0 - bar, cx + gap / 2.0] {
        c.fill_round_rect(x, cy - h / 2.0, x + bar, cy + h / 2.0, bar * 0.35, color);
    }
}

/// Dois triângulos (⏩ / ⏪) centrados em cx.
pub fn skip(c: &mut Canvas, cx: f32, cy: f32, size: f32, color: Rgba, forward: bool) {
    let (w, h) = (size * 0.52, size * 0.74);
    let sign = if forward { 1.0 } else { -1.0 };
    for base in [cx - sign * w, cx] {
        rounded_triangle(c, [(base, cy - h / 2.0), (base, cy + h / 2.0), (base + sign * w, cy)], size * 0.07, color);
    }
}

/// Seta circular (↺), aberta no topo, com a ponta apontando para a esquerda.
pub fn restart(c: &mut Canvas, cx: f32, cy: f32, size: f32, color: Rgba) {
    let r = size * 0.4;
    let w = (size * 0.13).max(1.0 / c.s);
    let (start, end) = (-70.0f32, 215.0f32);
    c.arc(cx, cy, r, start, end, color, w);
    let a = start.to_radians();
    let (px, py) = (cx + r * a.cos(), cy + r * a.sin());
    let (tx, ty) = (a.sin(), -a.cos()); // tangente no sentido anti-horário
    let (nx, ny) = (a.cos(), a.sin()); // direção radial
    let head = size * 0.36;
    rounded_triangle(
        c,
        [
            (px + tx * head * 0.7, py + ty * head * 0.7),
            (px - tx * head * 0.3 + nx * head * 0.55, py - ty * head * 0.3 + ny * head * 0.55),
            (px - tx * head * 0.3 - nx * head * 0.55, py - ty * head * 0.3 - ny * head * 0.55),
        ],
        size * 0.05,
        color,
    );
}

// ---------- ícones do tempo ----------
// Desenhados num quadro de 100 × 100 unidades, com degradês, no estilo dos ícones da Apple.

const SUN_CORE: (Rgba, Rgba) = (Rgba::rgb(255, 226, 96), Rgba::rgb(255, 149, 0));
const SUN_RAY: Rgba = Rgba(255, 186, 10, 255);
const MOON: (Rgba, Rgba) = (Rgba::rgb(255, 249, 224), Rgba::rgb(255, 210, 100));
const CLOUD_LIGHT: (Rgba, Rgba) = (Rgba::rgb(255, 255, 255), Rgba::rgb(208, 220, 236));
const CLOUD_GRAY: (Rgba, Rgba) = (Rgba::rgb(214, 222, 234), Rgba::rgb(150, 163, 184));
const CLOUD_DARK: (Rgba, Rgba) = (Rgba::rgb(176, 186, 202), Rgba::rgb(100, 112, 134));
const RAIN: (Rgba, Rgba) = (Rgba::rgb(110, 214, 255), Rgba::rgb(10, 132, 255));
const BOLT: (Rgba, Rgba) = (Rgba::rgb(255, 232, 90), Rgba::rgb(255, 159, 10));
const FOG_LINE: Rgba = Rgba(214, 222, 234, 230);

/// Partes de uma nuvem: (retângulo arredondado, círculo, círculo) em unidades.
fn cloud_parts(x0: f32, y0: f32, w: f32) -> ((f32, f32, f32, f32, f32), (f32, f32, f32), (f32, f32, f32)) {
    let h = w / 1.6;
    let base_top = y0 + h * 0.46;
    (
        (x0, base_top, x0 + w, y0 + h, (y0 + h - base_top) / 2.0),
        (x0 + w * 0.31, y0 + h * 0.60, h * 0.30),
        (x0 + w * 0.58, y0 + h * 0.42, h * 0.42),
    )
}

fn draw_cloud_shape(c: &mut Canvas, x0: f32, y0: f32, w: f32, f: Fill) {
    let (rect, a, b) = cloud_parts(x0, y0, w);
    c.fill_round_rect_with(rect.0, rect.1, rect.2, rect.3, rect.4, f);
    c.fill_circle_with(a.0, a.1, a.2, f);
    c.fill_circle_with(b.0, b.1, b.2, f);
}

/// Nuvem de largura w com o canto superior esquerdo em (x0, y0), com sombra suave embaixo.
fn cloud(c: &mut Canvas, x0: f32, y0: f32, w: f32, colors: (Rgba, Rgba)) {
    let h = w / 1.6;
    // sombra discreta: a mesma forma em preto, desfocada e mais abaixo
    let mut shadow = Canvas::new(c.w, c.h, c.s);
    draw_cloud_shape(&mut shadow, x0, y0, w, Fill::Solid(Rgba(0, 0, 0, 255)));
    let mut img = shadow.finish();
    let (iw, ih) = (img.w, img.h);
    blur(&mut img.px, iw, ih, 4, 2.5 * c.s);
    img.fade(0.3);
    c.image(&img, 0.0, 2.0);
    draw_cloud_shape(c, x0, y0, w, Fill::VGrad(y0, y0 + h, colors.0, colors.1));
}

fn sun(c: &mut Canvas, cx: f32, cy: f32, r: f32) {
    for i in 0..8 {
        let a = (i as f32 * 45.0).to_radians();
        let p0 = (cx + a.cos() * r * 1.42, cy + a.sin() * r * 1.42);
        let p1 = (cx + a.cos() * r * 1.78, cy + a.sin() * r * 1.78);
        c.line(&[p0, p1], SUN_RAY, r * 0.24);
    }
    c.fill_circle_with(cx, cy, r, Fill::VGrad(cy - r, cy + r, SUN_CORE.0, SUN_CORE.1));
}

fn moon(c: &mut Canvas, cx: f32, cy: f32, r: f32) {
    c.crescent(cx, cy, r, cx + r * 0.52, cy - r * 0.38, r * 0.84, Fill::VGrad(cy - r, cy + r, MOON.0, MOON.1));
}

fn drops(c: &mut Canvas, xs: &[f32], y: f32, length: f32, width: f32) {
    for &x in xs {
        let f = Fill::VGrad(y - width / 2.0, y + length + width / 2.0, RAIN.0, RAIN.1);
        c.line_with(&[(x, y), (x - length * 0.3, y + length)], f, width, false);
    }
}

fn flakes(c: &mut Canvas, points: &[(f32, f32)], r: f32) {
    for &(cx, cy) in points {
        for i in 0..3 {
            let a = (90.0 + i as f32 * 60.0f32).to_radians();
            let (dx, dy) = (a.cos() * r, a.sin() * r);
            c.line(&[(cx - dx, cy - dy), (cx + dx, cy + dy)], Rgba::rgb(255, 255, 255), r * 0.42);
        }
        c.fill_circle(cx, cy, r * 0.3, Rgba::rgb(255, 255, 255));
    }
}

fn bolt(c: &mut Canvas) {
    let pts = [(56.0, 56.0), (38.0, 80.0), (50.0, 80.0), (43.0, 98.0), (66.0, 70.0), (54.0, 70.0), (63.0, 56.0)];
    c.polygon_with(&pts, Fill::VGrad(56.0, 98.0, BOLT.0, BOLT.1));
}

thread_local! {
    static ART: RefCell<HashMap<(String, u32), Image>> = RefCell::new(HashMap::new());
}

/// Ícone do tempo `name` com px × px pixels (guardado depois da primeira vez).
pub fn weather_art(name: &str, px: u32) -> Image {
    let key = (name.to_string(), px);
    if let Some(img) = ART.with(|a| a.borrow().get(&key).cloned()) {
        return img;
    }
    let mut c = Canvas::new(px, px, px as f32 / 100.0);
    match name {
        "sun" => sun(&mut c, 50.0, 50.0, 24.0),
        "moon" => moon(&mut c, 50.0, 50.0, 34.0),
        "partly_day" => {
            sun(&mut c, 37.0, 37.0, 16.0);
            cloud(&mut c, 20.0, 42.0, 74.0, CLOUD_LIGHT);
        }
        "partly_night" => {
            moon(&mut c, 38.0, 36.0, 22.0);
            cloud(&mut c, 20.0, 42.0, 74.0, CLOUD_LIGHT);
        }
        "fog" => {
            cloud(&mut c, 12.0, 10.0, 76.0, CLOUD_GRAY);
            for (y, x0, x1) in [(70.0, 14.0, 78.0), (82.0, 24.0, 88.0), (94.0, 18.0, 66.0)] {
                c.fill_round_rect(x0, y - 3.0, x1, y + 3.0, 3.0, FOG_LINE);
            }
        }
        "drizzle" => {
            cloud(&mut c, 10.0, 8.0, 80.0, CLOUD_GRAY);
            drops(&mut c, &[34.0, 52.0, 70.0], 76.0, 7.0, 6.0);
        }
        "rain" => {
            cloud(&mut c, 10.0, 6.0, 80.0, CLOUD_GRAY);
            drops(&mut c, &[32.0, 50.0, 68.0], 70.0, 20.0, 6.0);
        }
        "snow" => {
            cloud(&mut c, 10.0, 6.0, 80.0, CLOUD_GRAY);
            flakes(&mut c, &[(30.0, 78.0), (50.0, 90.0), (70.0, 78.0)], 7.0);
        }
        "thunder" => {
            cloud(&mut c, 10.0, 4.0, 80.0, CLOUD_DARK);
            bolt(&mut c);
        }
        _ => {
            // "clouds"
            cloud(&mut c, 34.0, 18.0, 60.0, CLOUD_GRAY);
            cloud(&mut c, 6.0, 34.0, 76.0, CLOUD_LIGHT);
        }
    }
    let img = c.finish();
    ART.with(|a| a.borrow_mut().insert(key, img.clone()));
    img
}
