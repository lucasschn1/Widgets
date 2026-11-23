//! Operações simples de imagem feitas à mão (desfoque, saturação, recorte com interpolação),
//! para não precisar de bibliotecas de imagem.

use crate::gfx::Image;

/// Imagem RGB opaca (3 bytes por pixel), usada para o papel de parede.
#[derive(Clone)]
pub struct Rgb {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Rgb {
    /// Uma imagem w×h toda de uma cor (r, g, b).
    pub fn filled(w: u32, h: u32, color: (u8, u8, u8)) -> Rgb {
        let mut px = Vec::with_capacity((w * h * 3) as usize);
        for _ in 0..w * h {
            px.extend_from_slice(&[color.0, color.1, color.2]);
        }
        Rgb { w, h, px }
    }

    /// Converte uma imagem BGRA (opaca) em RGB.
    pub fn from_image(img: &Image) -> Rgb {
        let px = img.px.chunks_exact(4).flat_map(|bgra| [bgra[2], bgra[1], bgra[0]]).collect();
        Rgb { w: img.w, h: img.h, px }
    }

    /// BGRA opaca, para desenhar com o Direct2D.
    pub fn to_image(&self) -> Image {
        let px = self.px.chunks_exact(3).flat_map(|rgb| [rgb[2], rgb[1], rgb[0], 255]).collect();
        Image { w: self.w, h: self.h, px }
    }

    /// Cola `src` com o canto em (x, y), cortando o que sair desta imagem.
    pub fn paste(&mut self, src: &Rgb, x: i32, y: i32) {
        for sy in 0..src.h as i32 {
            let dy = y + sy;
            if dy < 0 || dy >= self.h as i32 {
                continue;
            }
            let x0 = x.max(0);
            let x1 = (x + src.w as i32).min(self.w as i32);
            if x1 <= x0 {
                continue;
            }
            let from = ((sy as u32 * src.w) as i32 + (x0 - x)) as usize * 3;
            let to = (dy as u32 * self.w + x0 as u32) as usize * 3;
            let bytes = (x1 - x0) as usize * 3;
            self.px[to..to + bytes].copy_from_slice(&src.px[from..from + bytes]);
        }
    }

    /// O pedaço w×h a partir de (x, y) (o que passar da imagem fica preto).
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Rgb {
        let mut out = Rgb::filled(w, h, (0, 0, 0));
        out.paste(self, -(x as i32), -(y as i32));
        out
    }

    /// Mais saturada (como o ImageEnhance.Color do Pillow): afasta cada cor do cinza.
    pub fn saturate(&mut self, factor: f32) {
        for pixel in self.px.chunks_exact_mut(3) {
            let gray = 0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32;
            for channel in pixel.iter_mut() {
                *channel = (gray + (*channel as f32 - gray) * factor).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    /// Aumenta a imagem repetindo as bordas (para widgets que saem um pouco da tela).
    pub fn extend_edges(&self, pad: u32) -> Rgb {
        let (w, h) = (self.w + 2 * pad, self.h + 2 * pad);
        let mut out = Rgb::filled(w, h, (0, 0, 0));
        for y in 0..h {
            let sy = (y as i32 - pad as i32).clamp(0, self.h as i32 - 1) as u32;
            for x in 0..w {
                let sx = (x as i32 - pad as i32).clamp(0, self.w as i32 - 1) as u32;
                let from = ((sy * self.w + sx) * 3) as usize;
                let to = ((y * w + x) * 3) as usize;
                out.px[to..to + 3].copy_from_slice(&self.px[from..from + 3]);
            }
        }
        out
    }

    /// O brilho de cada pixel (0 a 255), pelos pesos de sempre do vermelho, verde e azul.
    pub fn gray(&self) -> Vec<u8> {
        self.px.chunks_exact(3).map(|rgb| ((rgb[0] as u32 * 299 + rgb[1] as u32 * 587 + rgb[2] as u32 * 114) / 1000) as u8).collect()
    }

    /// Recorte (x, y, w, h) em coordenadas fracionárias, ampliado para out_w × out_h (bilinear).
    pub fn sample(&self, bx: f32, by: f32, bw: f32, bh: f32, out_w: u32, out_h: u32) -> Rgb {
        let mut out = Rgb::filled(out_w, out_h, (0, 0, 0));
        // quanto da imagem cada pixel de saída cobre
        let fx = bw / out_w as f32;
        let fy = bh / out_h as f32;
        for oy in 0..out_h {
            let sy = (by + (oy as f32 + 0.5) * fy - 0.5).clamp(0.0, (self.h - 1) as f32);
            let y0 = sy.floor() as u32;
            let y1 = (y0 + 1).min(self.h - 1);
            let ty = sy - y0 as f32;
            for ox in 0..out_w {
                let sx = (bx + (ox as f32 + 0.5) * fx - 0.5).clamp(0.0, (self.w - 1) as f32);
                let x0 = sx.floor() as u32;
                let x1 = (x0 + 1).min(self.w - 1);
                let tx = sx - x0 as f32;
                let to = ((oy * out_w + ox) * 3) as usize;
                for channel in 0..3 {
                    let at = |x: u32, y: u32| self.px[((y * self.w + x) * 3) as usize + channel] as f32;
                    let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
                    let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
                    out.px[to + channel] = (top + (bottom - top) * ty).round() as u8;
                }
            }
        }
        out
    }
}

/// O mesmo que `x.round().clamp(0.0, 255.0) as u8`, mas rápido: no x86 sem extensões, o
/// `round` do Rust chama a `roundf` da biblioteca do C, e as passadas por pixel faziam dezenas
/// dessas chamadas. Arredonda a metade para longe do zero, como o `round`; a conta `x - t` é exata
/// nesta faixa, então o resultado é sempre o mesmo (veja o teste). Sem desvios: se a fração passa
/// de meio ou não é quase sorteio, e o processador errava a aposta a cada pixel.
#[inline(always)]
pub fn round_u8(x: f32) -> u8 {
    // (o `max` ignora NaN: ele vira 0, como no `as u8`)
    let x = x.max(0.0).min(255.0);
    let t = x as i32;
    (t + (x - t as f32 >= 0.5) as i32) as u8
}

#[cfg(test)]
mod tests {
    use super::round_u8;

    /// Igual ao `round` com `clamp` em toda a faixa, inclusive nos casos difíceis: as metades, o
    /// maior número abaixo de cada metade, negativos, os extremos e NaN.
    #[test]
    fn round_u8_matches_round() {
        let slow = |x: f32| x.round().clamp(0.0, 255.0) as u8;
        let mut checked = 0;
        // todos os f32 de -2 a 300, pulando de 127 em 127 (passa por todos os expoentes)
        let (mut bits, end) = (0u32, 300f32.to_bits());
        while bits <= end {
            for x in [f32::from_bits(bits), -f32::from_bits(bits)] {
                assert_eq!(round_u8(x), slow(x), "{x:e}");
            }
            bits += 127;
            checked += 1;
        }
        for k in -4..520 {
            let half = k as f32 / 2.0;
            for x in [half, f32::from_bits(half.to_bits().wrapping_sub(1)), f32::from_bits(half.to_bits() + 1)] {
                assert_eq!(round_u8(x), slow(x), "{x:e}");
            }
        }
        for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, f32::MIN_POSITIVE, 0.0, -0.0] {
            assert_eq!(round_u8(x), slow(x), "{x:e}");
        }
        assert!(checked > 5_000_000);
    }
}

/// Desfoque gaussiano aproximado por três desfoques de caixa (rápido e quase igual).
pub fn blur(px: &mut [u8], w: u32, h: u32, channels: usize, sigma: f32) {
    if sigma < 0.5 || w == 0 || h == 0 {
        return;
    }
    for radius in box_radii(sigma) {
        box_pass(px, w as usize, h as usize, channels, radius, true);
        box_pass(px, w as usize, h as usize, channels, radius, false);
    }
}

/// Os raios das três caixas cuja soma se aproxima da gaussiana de `sigma` (Wells, 1986): as
/// primeiras `m` com a largura `wl` (ímpar), as outras com `wu` = `wl` + 2.
fn box_radii(sigma: f32) -> [usize; 3] {
    let n = 3.0;
    let ideal = ((12.0 * sigma * sigma / n) + 1.0).sqrt();
    let mut wl = ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n) / (-4.0 * wl as f32 - 4.0)).round() as i32;
    let mut out = [0usize; 3];
    for (i, radius) in out.iter_mut().enumerate() {
        let size = if (i as i32) < m { wl } else { wu };
        *radius = ((size - 1) / 2).max(0) as usize;
    }
    out
}

/// Um desfoque de caixa de raio `radius`, linha a linha (ou coluna a coluna).
fn box_pass(px: &mut [u8], w: usize, h: usize, channels: usize, radius: usize, horizontal: bool) {
    if radius == 0 {
        return;
    }
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let idx = |line: usize, i: usize| if horizontal { (line * w + i) * channels } else { (i * w + line) * channels };
    let mut src = vec![0u32; len * channels];
    let div = (2 * radius + 1) as u32;
    for line in 0..lines {
        for i in 0..len {
            for channel in 0..channels {
                src[i * channels + channel] = px[idx(line, i) + channel] as u32;
            }
        }
        for channel in 0..channels {
            // soma deslizante, repetindo as bordas
            let at = |i: isize| src[(i.clamp(0, len as isize - 1) as usize) * channels + channel];
            let mut sum: u32 = (-(radius as isize)..=radius as isize).map(at).sum();
            for i in 0..len {
                px[idx(line, i) + channel] = ((sum + div / 2) / div) as u8;
                sum += at(i as isize + radius as isize + 1);
                sum -= at(i as isize - radius as isize);
            }
        }
    }
}
