//! Ícones desenhados como caminhos SVG (numa grade de 24, como os do mockup), em vez dos da fonte
//! do Windows: o traço fica fino e igual em todos, e dá para preencher (o raio da carga).
//!
//! O caminho vira linhas (os arcos e as curvas em pedacinhos) uma vez só e fica guardado.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::gfx::{Canvas, Fill};
use crate::theme::Rgba;

/// Um pedaço do caminho: os pontos e se ele fecha.
pub type Part = (Vec<(f32, f32)>, bool);

// ---------- os ícones (grade de 24, traço de 1,6) ----------
pub const LAPTOP: &str = "M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5v8.5h-17V7A1.5 1.5 0 0 1 5 5.5zM2 15.5h20l-.6 1.7a1.8 1.8 0 0 1-1.7 1.3H4.3a1.8 1.8 0 0 1-1.7-1.3zM10.5 15.5l.4 1h2.2l.4-1";
pub const KEYBOARD: &str = "M4 6h16a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2zM6 9.6h.01M9 9.6h.01M12 9.6h.01M15 9.6h.01M18 9.6h.01M7.5 12.3h.01M10.5 12.3h.01M13.5 12.3h.01M16.5 12.3h.01M8.5 15h7";
pub const MOUSE: &str = "M12 2.75a6.25 6.25 0 0 1 6.25 6.25v6a6.25 6.25 0 0 1-12.5 0V9A6.25 6.25 0 0 1 12 2.75zM12 6.5v2.75M5.75 11.25h12.5";
pub const HEADPHONES: &str = "M4 15.5V12a8 8 0 0 1 16 0v3.5M5.5 14H7a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H5.5A1.5 1.5 0 0 1 4 18.5v-3A1.5 1.5 0 0 1 5.5 14zM18.5 14H17a1 1 0 0 0-1 1v4a1 1 0 0 0 1 1h1.5a1.5 1.5 0 0 0 1.5-1.5v-3a1.5 1.5 0 0 0-1.5-1.5z";
pub const GAMEPAD: &str = "M7.6 6.5h8.8a4.3 4.3 0 0 1 4.2 3.5l1 5.3a2.5 2.5 0 0 1-4.3 2.2l-1.9-2h-6.8l-1.9 2a2.5 2.5 0 0 1-4.3-2.2l1-5.3a4.3 4.3 0 0 1 4.2-3.5zM8 9.6v3M6.5 11.1h3M15.6 10.3h.01M17.6 12h.01";
pub const SPEAKER: &str = "M7 3h10a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM12 7h.01M15.5 14.5a3.5 3.5 0 0 1-7 0a3.5 3.5 0 0 1 7 0z";
pub const PHONE: &str = "M8 2.75h8a2 2 0 0 1 2 2v14.5a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V4.75a2 2 0 0 1 2-2zM10.5 18h3";
pub const PEN: &str = "M15.5 4.5l4 4L9 19l-5 1 1-5zM13.5 6.5l4 4";
pub const DEVICE: &str = "M9 3.5h6a2 2 0 0 1 2 2v13a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2v-13a2 2 0 0 1 2-2zM10.5 3.5v-1h3v1";
/// o raio da carga (preenchido)
pub const BOLT: &str = "M13.4 2.3 5.3 13.2a.7.7 0 0 0 .6 1.1h4.9l-1.1 7a.5.5 0 0 0 .9.4l8.1-10.9a.7.7 0 0 0-.6-1.1h-4.9l1.1-7a.5.5 0 0 0-.9-.4z";
/// o ✓ da carga completa
pub const CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";

thread_local! {
    static CACHE: RefCell<HashMap<&'static str, Rc<Vec<Part>>>> = RefCell::new(HashMap::new());
}

/// O caminho em linhas (guardado depois da primeira vez).
pub fn parts(d: &'static str) -> Rc<Vec<Part>> {
    CACHE.with(|c| c.borrow_mut().entry(d).or_insert_with(|| Rc::new(parse(d))).clone())
}

/// Um ícone de `size` centrado em (cx, cy), no traço `stroke` (na grade de 24) ou preenchido
/// (`stroke` 0).
pub fn icon(c: &mut Canvas, d: &'static str, cx: f32, cy: f32, size: f32, color: Rgba, stroke: f32) {
    let k = size / 24.0;
    for (pts, closed) in parts(d).iter() {
        let pts: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| (cx + (x - 12.0) * k, cy + (y - 12.0) * k)).collect();
        if stroke > 0.0 {
            c.line_with(&pts, Fill::Solid(color), stroke * k, *closed);
        } else {
            c.polygon(&pts, color);
        }
    }
}

/// Os números e comandos de um caminho SVG ("M5 5.5h14", ".7.7", "-1.3").
fn tokens(d: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let ch = b[i] as char;
        if ch.is_ascii_alphabetic() {
            out.push(Tok::Cmd(ch));
            i += 1;
        } else if ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == '+' {
            let start = i;
            i += 1;
            let mut dot = ch == '.';
            while i < b.len() {
                let c = b[i] as char;
                if c.is_ascii_digit() {
                    i += 1;
                } else if c == '.' && !dot {
                    dot = true;
                    i += 1;
                } else if (c == 'e' || c == 'E') && i + 1 < b.len() && !(b[i + 1] as char).is_ascii_alphabetic() {
                    i += 1;
                    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            out.push(Tok::Num(d[start..i].parse().unwrap_or(0.0)));
        } else {
            i += 1;
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok {
    Cmd(char),
    Num(f32),
}

/// O caminho SVG (M, L, H, V, A, C, Z, maiúsculos ou não) em pedaços de linha.
pub fn parse(d: &str) -> Vec<Part> {
    let toks = tokens(d);
    let mut out: Vec<Part> = Vec::new();
    let mut cur: Vec<(f32, f32)> = Vec::new();
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut i = 0;
    let mut cmd = 'M';
    let num = |i: &mut usize| -> Option<f32> {
        match toks.get(*i) {
            Some(Tok::Num(v)) => {
                *i += 1;
                Some(*v)
            }
            _ => None,
        }
    };
    let flush = |cur: &mut Vec<(f32, f32)>, out: &mut Vec<Part>, closed: bool| {
        if cur.len() >= 2 {
            out.push((std::mem::take(cur), closed));
        } else {
            cur.clear();
        }
    };
    while i < toks.len() {
        if let Tok::Cmd(c) = toks[i] {
            cmd = c;
            i += 1;
            if c == 'Z' || c == 'z' {
                flush(&mut cur, &mut out, true);
                (x, y) = (sx, sy);
                continue;
            }
        }
        // (depois de um Z, o pedaço novo começa onde o outro fechou)
        if cur.is_empty() && !cmd.eq_ignore_ascii_case(&'M') {
            cur.push((x, y));
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
        let start = i;
        match cmd.to_ascii_uppercase() {
            'M' => {
                let (Some(a), Some(b)) = (num(&mut i), num(&mut i)) else { break };
                flush(&mut cur, &mut out, false);
                (x, y) = (ox + a, oy + b);
                (sx, sy) = (x, y);
                cur.push((x, y));
                // os pares seguintes são linhas
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' => {
                let (Some(a), Some(b)) = (num(&mut i), num(&mut i)) else { break };
                (x, y) = (ox + a, oy + b);
                cur.push((x, y));
            }
            'H' => {
                let Some(a) = num(&mut i) else { break };
                x = ox + a;
                cur.push((x, y));
            }
            'V' => {
                let Some(b) = num(&mut i) else { break };
                y = oy + b;
                cur.push((x, y));
            }
            'C' => {
                let v: Vec<f32> = (0..6).map_while(|_| num(&mut i)).collect();
                if v.len() < 6 {
                    break;
                }
                let (p0, p1, p2, p3) = ((x, y), (ox + v[0], oy + v[1]), (ox + v[2], oy + v[3]), (ox + v[4], oy + v[5]));
                for k in 1..=8 {
                    let t = k as f32 / 8.0;
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    cur.push((w[0] * p0.0 + w[1] * p1.0 + w[2] * p2.0 + w[3] * p3.0, w[0] * p0.1 + w[1] * p1.1 + w[2] * p2.1 + w[3] * p3.1));
                }
                (x, y) = p3;
            }
            'A' => {
                let v: Vec<f32> = (0..7).map_while(|_| num(&mut i)).collect();
                if v.len() < 7 {
                    break;
                }
                let (ex, ey) = (ox + v[5], oy + v[6]);
                arc_points(&mut cur, (x, y), (ex, ey), v[0], v[1], v[2], v[3] != 0.0, v[4] != 0.0);
                (x, y) = (ex, ey);
            }
            _ => break,
        }
        if i == start {
            break;
        }
    }
    flush(&mut cur, &mut out, false);
    out
}

/// O arco do SVG (do ponto atual a `to`) em pedacinhos de no máximo 15°.
#[allow(clippy::too_many_arguments)]
fn arc_points(out: &mut Vec<(f32, f32)>, from: (f32, f32), to: (f32, f32), rx: f32, ry: f32, rot: f32, large: bool, sweep: bool) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-6 || ry < 1e-6 || (from.0 - to.0).hypot(from.1 - to.1) < 1e-6 {
        out.push(to);
        return;
    }
    // (a conta do SVG: do ponto inicial e final para o centro e os ângulos)
    let phi = rot.to_radians();
    let (cp, sp) = (phi.cos(), phi.sin());
    let (dx, dy) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
    let (x1, y1) = (cp * dx + sp * dy, -sp * dx + cp * dy);
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut co = (num / den).sqrt();
    if large == sweep {
        co = -co;
    }
    let (cx1, cy1) = (co * rx * y1 / ry, -co * ry * x1 / rx);
    let (cx, cy) = (cp * cx1 - sp * cy1 + (from.0 + to.0) / 2.0, sp * cx1 + cp * cy1 + (from.1 + to.1) / 2.0);
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = ang(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dt = ang((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !sweep && dt > 0.0 {
        dt -= std::f32::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f32::consts::TAU;
    }
    let n = ((dt.abs() / 15f32.to_radians()).ceil() as usize).max(2);
    for k in 1..=n {
        let t = t1 + dt * k as f32 / n as f32;
        let (px, py) = (rx * t.cos(), ry * t.sin());
        out.push((cp * px - sp * py + cx, sp * px + cp * py + cy));
    }
    // (termina exatamente no ponto pedido)
    if let Some(last) = out.last_mut() {
        *last = to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_split_like_the_svg() {
        assert_eq!(tokens("M13.4 2.3 5.3 13.2a.7.7 0 0 0 .6 1.1l-.6 1.7"), vec![
            Tok::Cmd('M'), Tok::Num(13.4), Tok::Num(2.3), Tok::Num(5.3), Tok::Num(13.2),
            Tok::Cmd('a'), Tok::Num(0.7), Tok::Num(0.7), Tok::Num(0.0), Tok::Num(0.0), Tok::Num(0.0), Tok::Num(0.6), Tok::Num(1.1),
            Tok::Cmd('l'), Tok::Num(-0.6), Tok::Num(1.7),
        ]);
    }

    /// Cada comando chega aonde devia: linhas, horizontais, verticais, relativos, os pares a mais
    /// depois do M (linhas) e o Z voltando ao começo.
    #[test]
    fn paths_end_where_the_svg_ends() {
        let p = parse("M2 3h4v5l-2 1H1V2z");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0], (vec![(2.0, 3.0), (6.0, 3.0), (6.0, 8.0), (4.0, 9.0), (1.0, 9.0), (1.0, 2.0)], true));
        let p = parse("M13.4 2.3 5.3 13.2");
        assert_eq!(p[0].0, vec![(13.4, 2.3), (5.3, 13.2)]);
        // um ponto ("h.01") vira um traço curtinho (a ponta redonda desenha o ponto)
        let p = parse("M6 9.6h.01M9 9.6h.01");
        assert_eq!(p.len(), 2);
    }

    /// Meia volta de raio 8 (a do fone): passa pelo alto e termina no outro lado.
    #[test]
    fn arcs_go_around_the_right_way() {
        let p = parse("M4 12a8 8 0 0 1 16 0");
        let pts = &p[0].0;
        assert_eq!(*pts.last().unwrap(), (20.0, 12.0));
        let top = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        assert!((top - 4.0).abs() < 0.05, "o alto do arco: {top}");
        // o canto arredondado de 1,5 do notebook: fica dentro do quadrado do canto
        let p = parse("M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5");
        for &(x, y) in &p[0].0[2..] {
            assert!((19.0..=20.5).contains(&x) && (5.5..=7.0).contains(&y), "{x},{y}");
        }
    }

    /// Todos os ícones leem até o fim (nenhum comando que o leitor não conhece).
    #[test]
    fn every_icon_parses() {
        for d in [LAPTOP, KEYBOARD, MOUSE, HEADPHONES, GAMEPAD, SPEAKER, PHONE, PEN, DEVICE, BOLT, CHECK] {
            let p = parse(d);
            assert!(!p.is_empty(), "{d}");
            for (pts, _) in p {
                for (x, y) in pts {
                    assert!((-0.5..=24.5).contains(&x) && (-0.5..=24.5).contains(&y), "{d}: {x},{y}");
                }
            }
        }
    }
}
