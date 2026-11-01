//! Fundo de vidro: o papel de parede desfocado atrás dos cartões.
//!
//! Os widgets ficam sempre atrás das outras janelas, então o que há atrás deles é o papel de
//! parede: basta mostrar o pedaço dele que fica sob cada cartão. A tela inteira é desfocada uma
//! vez só, em escala reduzida; recortar depois é barato.

use std::path::PathBuf;

use crate::gfx::{self, Canvas, Image};
use crate::imgops::{blur, Rgb};
use crate::theme::*;
use crate::win;

const DOWN: f32 = 4.0; // redução da imagem desfocada
const PAD: u32 = 64; // borda extra, em pixels reduzidos
const DESKTOP: &str = r"Control Panel\Desktop";
const COLORS: &str = r"Control Panel\Colors";

fn transcoded() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(r"Microsoft\Windows\Themes\TranscodedWallpaper")
}

#[derive(PartialEq, Clone, Debug)]
struct Signature {
    path: String,
    style: String,
    tile: String,
    color: String,
    mtime: Option<std::time::SystemTime>,
    screens: Vec<win::Rect>,
    blur: i64,
    scale: i64,
}

pub struct Backdrop {
    sig: Option<Signature>,
    pub version: u64,
    origin: (i32, i32),
    s: f32,
    image: Rgb,
    sharp: Rgb,
    gray: Vec<u8>,
}

impl Backdrop {
    pub fn new(blur_px: f64, s: f32) -> Backdrop {
        let mut b = Backdrop { sig: None, version: 0, origin: (0, 0), s, image: Rgb::filled(1, 1, (0, 0, 0)), sharp: Rgb::filled(1, 1, (0, 0, 0)), gray: vec![0] };
        b.check(blur_px);
        b
    }

    fn signature(&self, blur_px: f64) -> Signature {
        Signature {
            path: win::registry(DESKTOP, "WallPaper"),
            style: win::registry(DESKTOP, "WallpaperStyle"),
            tile: win::registry(DESKTOP, "TileWallpaper"),
            color: win::registry(COLORS, "Background"),
            mtime: std::fs::metadata(transcoded()).and_then(|m| m.modified()).ok(),
            screens: win::monitors(),
            blur: blur_px.round() as i64,
            scale: (self.s * 100.0) as i64,
        }
    }

    /// Refaz o fundo se o papel de parede, os monitores ou o desfoque mudaram; diz se refez.
    pub fn check(&mut self, blur_px: f64) -> bool {
        let sig = self.signature(blur_px);
        if self.sig.as_ref() == Some(&sig) {
            return false;
        }
        self.build(&sig, blur_px);
        self.sig = Some(sig);
        self.version += 1;
        true
    }

    fn build(&mut self, sig: &Signature, blur_px: f64) {
        let mut screens = sig.screens.clone();
        if screens.is_empty() {
            screens = vec![win::work_areas()[0]];
        }
        let left = screens.iter().map(|r| r.0).min().unwrap();
        let top = screens.iter().map(|r| r.1).min().unwrap();
        let right = screens.iter().map(|r| r.2).max().unwrap();
        let bottom = screens.iter().map(|r| r.3).max().unwrap();
        self.origin = (left, top);
        let bg: Vec<u8> = sig.color.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        let bg = if bg.len() >= 3 { (bg[0], bg[1], bg[2]) } else { (0, 0, 0) };
        let w = ((right - left) as f32 / DOWN).ceil() as u32;
        let h = ((bottom - top) as f32 / DOWN).ceil() as u32;
        let mut canvas = Rgb::filled(w.max(1), h.max(1), bg);
        if let Some(picture) = load_wallpaper(&sig.path) {
            // "Estender" (22) cobre todos os monitores com uma imagem só
            let areas = if sig.style == "22" { vec![(left, top, right, bottom)] } else { screens };
            for area in areas {
                self.paint(&mut canvas, &picture, area, &sig.style, sig.tile == "1");
            }
        }
        canvas.saturate(SATURATION);
        self.sharp = canvas.extend_edges(PAD);
        let (cw, ch) = (canvas.w, canvas.h);
        blur(&mut canvas.px, cw, ch, 3, (blur_px as f32) * self.s / DOWN);
        self.image = canvas.extend_edges(PAD);
        self.gray = self.image.gray();
    }

    /// Desenha o papel de parede numa área da tela, como o Windows faz em cada modo.
    fn paint(&self, canvas: &mut Rgb, picture: &Image, area: win::Rect, style: &str, tile: bool) {
        let (l, t, r, b) = area;
        let x0 = ((l - self.origin.0) as f32 / DOWN).round() as i32;
        let y0 = ((t - self.origin.1) as f32 / DOWN).round() as i32;
        let aw = (((r - l) as f32 / DOWN).round() as u32).max(1);
        let ah = (((b - t) as f32 / DOWN).round() as u32).max(1);
        let (pw, ph) = (picture.w as f32 / DOWN, picture.h as f32 / DOWN);
        let mut region = canvas.crop(x0.max(0) as u32, y0.max(0) as u32, aw, ah);
        if tile {
            let piece = Rgb::from_image(&gfx::resize(picture, pw.round() as u32, ph.round() as u32));
            let mut ty = 0;
            while ty < ah {
                let mut tx = 0;
                while tx < aw {
                    region.paste(&piece, tx as i32, ty as i32);
                    tx += piece.w.max(1);
                }
                ty += piece.h.max(1);
            }
        } else {
            let (w, h) = match style {
                "2" => (aw as f32, ah as f32), // esticar
                "6" | "10" | "22" => {
                    // ajustar / preencher / estender
                    let scale = if style == "6" { (aw as f32 / pw).min(ah as f32 / ph) } else { (aw as f32 / pw).max(ah as f32 / ph) };
                    (pw * scale, ph * scale)
                }
                _ => (pw, ph), // centralizar, no tamanho original
            };
            let (w, h) = ((w.round() as u32).max(1), (h.round() as u32).max(1));
            let resized = Rgb::from_image(&gfx::resize(picture, w, h));
            region.paste(&resized, (aw as i32 - w as i32) / 2, (ah as i32 - h as i32) / 2);
        }
        canvas.paste(&region, x0, y0);
    }

    /// Caixa (na imagem reduzida) correspondente a um retângulo da tela.
    fn bbox(&self, x: i32, y: i32, w: u32, h: u32) -> (f32, f32, f32, f32) {
        let (bw, bh) = (w as f32 / DOWN, h as f32 / DOWN);
        let pad = PAD as f32;
        let bx = (((x - self.origin.0) as f32 / DOWN + pad).max(0.0)).min(self.image.w as f32 - bw);
        let by = (((y - self.origin.1) as f32 / DOWN + pad).max(0.0)).min(self.image.h as f32 - bh);
        (bx, by, bw, bh)
    }

    /// Média de claridade (0 a 1) do fundo nessa caixa.
    fn lightness(&self, bx: f32, by: f32, bw: f32, bh: f32) -> f32 {
        let (x0, y0) = (bx.round().max(0.0) as u32, by.round().max(0.0) as u32);
        let x1 = ((bx + bw).round() as u32).min(self.image.w).max(x0 + 1);
        let y1 = ((by + bh).round() as u32).min(self.image.h).max(y0 + 1);
        let mut sum = 0u64;
        for y in y0..y1 {
            for x in x0..x1 {
                sum += self.gray[(y * self.image.w + x) as usize] as u64;
            }
        }
        sum as f32 / ((x1 - x0) * (y1 - y0)) as f32 / 255.0
    }

    /// O papel de parede (desfocado, ou nítido com `sharp`) sob um retângulo da tela.
    pub fn region(&self, x: i32, y: i32, w: u32, h: u32, sharp: bool) -> Rgb {
        let (bx, by, bw, bh) = self.bbox(x, y, w, h);
        let source = if sharp { &self.sharp } else { &self.image };
        source.sample(bx, by, bw, bh, w.max(1), h.max(1))
    }

    /// Cartão de vidro sob `content`, com o canto superior esquerdo em (x, y) na tela.
    /// `tint` é o escurecimento base (sobe sozinho em papéis de parede claros).
    pub fn glass(&self, content: &Image, radius: f32, x: i32, y: i32, tint: f64) -> Image {
        let (w, h) = (content.w, content.h);
        let (bx, by, bw, bh) = self.bbox(x, y, w, h);
        let mut base = self.image.sample(bx, by, bw, bh, w, h);
        // contraste automático: quanto mais claro o fundo, mais o vidro escurece
        let light = self.lightness(bx, by, bw, bh);
        let amount = (tint as f32 + (light - 0.3) * 0.8).clamp(0.12, 0.85);
        let tint_rgb = [TINT.0 as f32, TINT.1 as f32, TINT.2 as f32];
        for p in base.px.chunks_exact_mut(3) {
            for (c, t) in p.iter_mut().zip(tint_rgb) {
                *c = (*c as f32 + (t - *c as f32) * amount).round() as u8;
            }
        }
        finish_glass(&base.to_image(), content, radius, self.s)
    }
}

/// Vidro pronto: base (já escurecida), reflexo no topo, borda clara, conteúdo e cantos redondos.
pub fn finish_glass(base: &Image, content: &Image, radius: f32, s: f32) -> Image {
    let (w, h) = (base.w as f32, base.h as f32);
    let mut c = Canvas::new(base.w, base.h, 1.0);
    c.push_round_clip(0.0, 0.0, w, h, radius);
    c.image(base, 0.0, 0.0);
    // reflexo branco no topo, sumindo até 60% da altura
    c.gradient_round_rect(0.0, 0.0, w, h, radius, &[(0.0, WHITE.alpha(SHEEN)), (0.6, WHITE.alpha(0.0)), (1.0, WHITE.alpha(0.0))], 0.0, h);
    let line = s.round().max(1.0);
    c.gradient_stroke_round_rect(0.0, 0.0, w, h, radius, WHITE.alpha(EDGE.0), WHITE.alpha(EDGE.1), line);
    c.image(content, 0.0, 0.0);
    c.pop_clip();
    c.finish()
}

fn load_wallpaper(path: &str) -> Option<Image> {
    if path.is_empty() {
        return None; // cor sólida, sem imagem
    }
    // a cópia do Windows é a imagem que está na tela
    gfx::load_image(&transcoded()).or_else(|| gfx::load_image(std::path::Path::new(path)))
}

impl Backdrop {
    /// Como `glass`, mas com outro desfoque e escurecimento (prévia ao vivo nas configurações).
    pub fn glass_with(&self, content: &Image, radius: f32, x: i32, y: i32, blur_px: f64, tint: f64) -> Image {
        let (w, h) = (content.w, content.h);
        let (bx, by, bw, bh) = self.bbox(x, y, w, h);
        let sigma = blur_px as f32 * self.s / DOWN;
        let pad = (3.0 * sigma + 2.0).ceil();
        let ox = (bx - pad).max(0.0).floor();
        let oy = (by - pad).max(0.0).floor();
        let cw = ((bx + bw + pad).min(self.sharp.w as f32) - ox).ceil().max(1.0) as u32;
        let ch = ((by + bh + pad).min(self.sharp.h as f32) - oy).ceil().max(1.0) as u32;
        let mut piece = self.sharp.crop(ox as u32, oy as u32, cw, ch);
        blur(&mut piece.px, cw, ch, 3, sigma);
        let mut base = piece.sample(bx - ox, by - oy, bw, bh, w, h);
        let gray = base.gray();
        let light = gray.iter().map(|&g| g as u64).sum::<u64>() as f32 / gray.len().max(1) as f32 / 255.0;
        let amount = (tint as f32 + (light - 0.3) * 0.8).clamp(0.12, 0.85);
        let t = [TINT.0 as f32, TINT.1 as f32, TINT.2 as f32];
        for p in base.px.chunks_exact_mut(3) {
            for (c, tc) in p.iter_mut().zip(t) {
                *c = (*c as f32 + (tc - *c as f32) * amount).round() as u8;
            }
        }
        finish_glass(&base.to_image(), content, radius, self.s)
    }
}
