//! Fundo de vidro: o papel de parede desfocado atrás dos cartões.
//!
//! Os widgets ficam sempre atrás das outras janelas, então o que há atrás deles é o papel de
//! parede: basta mostrar o pedaço dele que fica sob cada cartão. A tela inteira é desfocada uma
//! vez só, em escala reduzida; recortar depois é barato.

use std::path::PathBuf;

use crate::gfx::{self, Canvas, Image};
use crate::gfx::wallcolor::WallColor;
use crate::gfx::imgops::{blur, round_u8, Rgb};
use crate::ui::theme::*;
use crate::platform::win;

const DOWN: f32 = 4.0; // redução da imagem desfocada
const PAD: u32 = 64; // borda extra, em pixels reduzidos
const DESKTOP: &str = r"Control Panel\Desktop";
const COLORS: &str = r"Control Panel\Colors";

/// A cópia do papel de parede que o Windows guarda (a imagem que está na tela).
fn transcoded() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(r"Microsoft\Windows\Themes\TranscodedWallpaper")
}

/// Um monitor como ele entra na assinatura do fundo.
#[derive(PartialEq, Clone, Debug)]
struct ScreenSig {
    area: win::Rect,
    /// a escala × 100 (125 para 125%)
    scale_pct: i64,
    /// o papel de parede só dele (vazio: o de todos)
    wallpaper: String,
}

/// Tudo de que o fundo depende: se nada disso mudou, o fundo pronto ainda vale.
#[derive(PartialEq, Clone, Debug)]
struct Signature {
    path: String,
    style: String,
    tile: String,
    color: String,
    mtime: Option<std::time::SystemTime>,
    screens: Vec<ScreenSig>,
    blur: i64,
}

/// O papel de parede de todos os monitores, desfocado e reduzido, pronto para recortar.
pub struct Backdrop {
    sig: Option<Signature>,
    pub version: u64,
    origin: (i32, i32),
    s: f32,
    /// cada monitor e a sua escala (o vidro de cada um segue a escala dele)
    screens: Vec<(win::Rect, f32)>,
    walls: win::Wallpapers,
    image: Rgb,
    sharp: Rgb,
    gray: Vec<u8>,
    /// a cor que domina o papel de parede de cada monitor (Aparência → "Cor do papel de parede")
    wall_colors: Vec<(win::Rect, WallColor)>,
}

impl Backdrop {
    pub fn new(blur_px: f64, s: f32) -> Backdrop {
        let mut backdrop = Backdrop {
            sig: None,
            version: 0,
            origin: (0, 0),
            s,
            screens: Vec::new(),
            walls: win::Wallpapers::new(),
            image: Rgb::filled(1, 1, (0, 0, 0)),
            sharp: Rgb::filled(1, 1, (0, 0, 0)),
            gray: vec![0],
            wall_colors: Vec::new(),
        };
        backdrop.check(blur_px);
        backdrop
    }

    /// A assinatura de agora (o registro do Windows, os monitores e o desfoque pedido).
    fn signature(&mut self, blur_px: f64) -> Signature {
        let walls = self.walls.list();
        let screens = win::monitor_list()
            .into_iter()
            .map(|monitor| {
                let own = walls.iter().find(|(area, _)| *area == monitor.screen).map(|(_, path)| path.clone()).unwrap_or_default();
                ScreenSig { area: monitor.screen, scale_pct: (monitor.scale * 100.0).round() as i64, wallpaper: own }
            })
            .collect();
        Signature {
            path: win::registry(DESKTOP, "WallPaper"),
            style: win::registry(DESKTOP, "WallpaperStyle"),
            tile: win::registry(DESKTOP, "TileWallpaper"),
            color: win::registry(COLORS, "Background"),
            mtime: std::fs::metadata(transcoded()).and_then(|meta| meta.modified()).ok(),
            screens,
            blur: blur_px.round() as i64,
        }
    }

    /// Vidro sobre uma imagem qualquer, como se fosse o papel de parede de um monitor só (para
    /// gerar as imagens do README).
    #[cfg(test)]
    pub fn from_picture(picture: &Image, blur_px: f64, s: f32) -> Backdrop {
        let (pw, ph) = (picture.w, picture.h);
        let w = (pw as f32 / DOWN).ceil() as u32;
        let h = (ph as f32 / DOWN).ceil() as u32;
        let mut canvas = Rgb::from_image(&gfx::resize(picture, w, h));
        canvas.saturate(SATURATION);
        let sharp = canvas.extend_edges(PAD);
        blur(&mut canvas.px, w, h, 3, blur_px as f32 * s / DOWN);
        let image = canvas.extend_edges(PAD);
        let gray = image.gray();
        let screens = vec![((0, 0, pw as i32, ph as i32), s)];
        let mut backdrop = Backdrop { sig: None, version: 1, origin: (0, 0), s, screens, walls: win::Wallpapers::new(), image, sharp, gray, wall_colors: Vec::new() };
        backdrop.find_wall_colors();
        backdrop
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

    /// Monta o fundo: o papel de parede de cada monitor (como o Windows o põe), saturado e
    /// desfocado na escala de cada um.
    fn build(&mut self, sig: &Signature, blur_px: f64) {
        let mut screens = sig.screens.clone();
        if screens.is_empty() {
            screens = vec![ScreenSig { area: win::work_areas()[0], scale_pct: (self.s * 100.0).round() as i64, wallpaper: String::new() }];
        }
        self.screens = screens.iter().map(|screen| (screen.area, screen.scale_pct as f32 / 100.0)).collect();
        let left = screens.iter().map(|screen| screen.area.0).min().unwrap();
        let top = screens.iter().map(|screen| screen.area.1).min().unwrap();
        let right = screens.iter().map(|screen| screen.area.2).max().unwrap();
        let bottom = screens.iter().map(|screen| screen.area.3).max().unwrap();
        self.origin = (left, top);
        // a cor de fundo do Windows ("R G B")
        let bg: Vec<u8> = sig.color.split_whitespace().filter_map(|part| part.parse().ok()).collect();
        let bg = if bg.len() >= 3 { (bg[0], bg[1], bg[2]) } else { (0, 0, 0) };
        let w = ((right - left) as f32 / DOWN).ceil() as u32;
        let h = ((bottom - top) as f32 / DOWN).ceil() as u32;
        let mut canvas = Rgb::filled(w.max(1), h.max(1), bg);
        // cada monitor pode ter o seu papel de parede ("Personalizar" > clique direito na imagem)
        let first = screens[0].wallpaper.clone();
        let mixed = sig.style != "22" && screens.iter().any(|screen| !screen.wallpaper.eq_ignore_ascii_case(&first));
        if mixed {
            let mut loaded: Vec<(String, Option<Image>)> = Vec::new();
            for ScreenSig { area, wallpaper: path, .. } in &screens {
                if path.is_empty() {
                    continue; // cor sólida
                }
                if !loaded.iter().any(|(loaded_path, _)| loaded_path == path) {
                    let picture = gfx::load_image(std::path::Path::new(path)).or_else(|| load_wallpaper(&sig.path));
                    loaded.push((path.clone(), picture));
                }
                if let Some((_, Some(picture))) = loaded.iter().find(|(loaded_path, _)| loaded_path == path) {
                    self.paint(&mut canvas, picture, *area, &sig.style, sig.tile == "1");
                }
            }
        } else if let Some(picture) = load_wallpaper(&sig.path) {
            // "Estender" (22) cobre todos os monitores com uma imagem só
            let areas: Vec<win::Rect> = if sig.style == "22" { vec![(left, top, right, bottom)] } else { screens.iter().map(|screen| screen.area).collect() };
            for area in areas {
                self.paint(&mut canvas, &picture, area, &sig.style, sig.tile == "1");
            }
        }
        canvas.saturate(SATURATION);
        self.sharp = canvas.extend_edges(PAD);
        let (cw, ch) = (canvas.w, canvas.h);
        let sigma = |scale: i64| (blur_px as f32) * scale as f32 / 100.0 / DOWN;
        if screens.iter().all(|screen| screen.scale_pct == screens[0].scale_pct) {
            blur(&mut canvas.px, cw, ch, 3, sigma(screens[0].scale_pct));
        } else {
            // escalas diferentes: cada monitor desfocado na medida dele
            for ScreenSig { area, scale_pct: scale, .. } in &screens {
                let x0 = ((area.0 - left) as f32 / DOWN).round().max(0.0) as u32;
                let y0 = ((area.1 - top) as f32 / DOWN).round().max(0.0) as u32;
                let w = (((area.2 - area.0) as f32 / DOWN).round() as u32).clamp(1, cw.saturating_sub(x0).max(1));
                let h = (((area.3 - area.1) as f32 / DOWN).round() as u32).clamp(1, ch.saturating_sub(y0).max(1));
                let mut piece = canvas.crop(x0, y0, w, h);
                blur(&mut piece.px, w, h, 3, sigma(*scale));
                canvas.paste(&piece, x0 as i32, y0 as i32);
            }
        }
        self.image = canvas.extend_edges(PAD);
        self.gray = self.image.gray();
        self.find_wall_colors();
    }

    /// A cor que domina o papel de parede de cada monitor (na imagem nítida, já montada).
    fn find_wall_colors(&mut self) {
        self.wall_colors = self
            .screens
            .iter()
            .map(|&(area, _)| {
                let (bx, by, bw, bh) = self.bbox(area.0, area.1, (area.2 - area.0).max(1) as u32, (area.3 - area.1).max(1) as u32);
                let piece = self.sharp.crop(bx.round() as u32, by.round() as u32, (bw.round() as u32).max(1), (bh.round() as u32).max(1));
                (area, WallColor::of_pixels(&piece.px))
            })
            .collect();
    }

    /// A cor do papel de parede do monitor onde está o ponto (x, y) da tela (fora de todos: a do
    /// primeiro).
    pub fn wall_color_at(&self, x: i32, y: i32) -> WallColor {
        let inside = |area: &win::Rect| x >= area.0 && x < area.2 && y >= area.1 && y < area.3;
        self.wall_colors
            .iter()
            .find(|(area, _)| inside(area))
            .or(self.wall_colors.first())
            .map_or(WallColor { hue: 0.0, chroma: 0.0 }, |&(_, color)| color)
    }

    /// Desenha o papel de parede numa área da tela, como o Windows faz em cada modo.
    fn paint(&self, canvas: &mut Rgb, picture: &Image, area: win::Rect, style: &str, tile: bool) {
        let (left, top, right, bottom) = area;
        let x0 = ((left - self.origin.0) as f32 / DOWN).round() as i32;
        let y0 = ((top - self.origin.1) as f32 / DOWN).round() as i32;
        let aw = (((right - left) as f32 / DOWN).round() as u32).max(1);
        let ah = (((bottom - top) as f32 / DOWN).round() as u32).max(1);
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

    /// Escala do monitor onde está o ponto.
    fn scale_for(&self, x: i32, y: i32) -> f32 {
        self.screens.iter().find(|(area, _)| x >= area.0 && x < area.2 && y >= area.1 && y < area.3).map(|&(_, scale)| scale).unwrap_or(self.s)
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
        let base = self.glass_base(content.w, content.h, x, y, tint);
        self.glass_on(&base, content, radius, x, y)
    }

    /// Como `glass`, sobre um fundo já pronto (`glass_base`, que o cartão guarda enquanto não sai
    /// do lugar).
    pub fn glass_on(&self, base: &Image, content: &Image, radius: f32, x: i32, y: i32) -> Image {
        finish_glass(base, content, radius, self.scale_for(x, y))
    }

    /// As peças do vidro sobre este fundo (`GlassParts`), para montar com conteúdos novos.
    pub fn glass_parts(&self, base: &Image, radius: f32, x: i32, y: i32) -> GlassParts {
        GlassParts::new(base, radius, self.scale_for(x, y))
    }

    /// O fundo do vidro (w × h pixels, canto em (x, y) na tela): o papel de parede desfocado,
    /// escurecido e com o granulado; só depende do lugar, do tamanho e do fundo, não do conteúdo.
    ///
    /// É o recorte com interpolação (`Rgb::sample`), o escurecimento e o granulado (`grain`) numa
    /// passada só, com as mesmas contas na mesma ordem (o resultado é o mesmo, byte a byte: veja o
    /// teste), sem as imagens intermediárias e com o que é de cada coluna calculado uma vez.
    pub fn glass_base(&self, w: u32, h: u32, x: i32, y: i32, tint: f64) -> Image {
        let (w, h) = (w.max(1), h.max(1));
        let (bx, by, bw, bh) = self.bbox(x, y, w, h);
        // contraste automático: quanto mais claro o fundo, mais o vidro escurece
        let light = self.lightness(bx, by, bw, bh);
        let amount = tint_amount(tint, light);
        let tint_rgb = [TINT.0 as f32, TINT.1 as f32, TINT.2 as f32];
        let src = &self.image;
        let (fx, fy) = (bw / w as f32, bh / h as f32);
        // para cada coluna: os dois pixels vizinhos na imagem desfocada e o peso entre eles
        let cols: Vec<Column> = (0..w)
            .map(|ox| {
                let sx = (bx + (ox as f32 + 0.5) * fx - 0.5).clamp(0.0, (src.w - 1) as f32);
                let x0 = sx.floor() as u32;
                let x1 = (x0 + 1).min(src.w - 1);
                Column { left: x0 as usize * 3, right: x1 as usize * 3, weight: sx - x0 as f32 }
            })
            .collect();
        let stride = src.w as usize * 3;
        let mut out = Image::new(w, h);
        for (oy, row) in out.px.chunks_exact_mut(w as usize * 4).enumerate() {
            let sy = (by + (oy as f32 + 0.5) * fy - 0.5).clamp(0.0, (src.h - 1) as f32);
            let y0 = sy.floor() as u32;
            let y1 = (y0 + 1).min(src.h - 1);
            let ty = sy - y0 as f32;
            let r0 = &src.px[y0 as usize * stride..][..stride];
            let r1 = &src.px[y1 as usize * stride..][..stride];
            let py = y + oy as i32;
            for (ox, (&Column { left, right, weight: tx }, out_px)) in cols.iter().zip(row.chunks_exact_mut(4)).enumerate() {
                let grain_shift = (noise(x + ox as i32, py) - 0.5) * 2.0 * GRAIN;
                for channel in 0..3 {
                    let (p00, p10, p01, p11) = (r0[left + channel] as f32, r0[right + channel] as f32, r1[left + channel] as f32, r1[right + channel] as f32);
                    let top = p00 + (p10 - p00) * tx;
                    let bottom = p01 + (p11 - p01) * tx;
                    let sampled = round_u8(top + (bottom - top) * ty);
                    let tinted = round_u8(sampled as f32 + (tint_rgb[channel] - sampled as f32) * amount);
                    // (RGB para BGRA)
                    out_px[2 - channel] = round_u8(tinted as f32 + grain_shift);
                }
                out_px[3] = 255;
            }
        }
        out
    }

    /// O fundo do vidro do jeito antigo, passo a passo (para conferir que o rápido dá o mesmo).
    #[cfg(test)]
    pub fn glass_base_steps(&self, w: u32, h: u32, x: i32, y: i32, tint: f64) -> Image {
        let (bx, by, bw, bh) = self.bbox(x, y, w, h);
        let mut base = self.image.sample(bx, by, bw, bh, w, h);
        let light = self.lightness(bx, by, bw, bh);
        let amount = tint_amount(tint, light);
        let tint_rgb = [TINT.0 as f32, TINT.1 as f32, TINT.2 as f32];
        for pixel in base.px.chunks_exact_mut(3) {
            for (channel, tint) in pixel.iter_mut().zip(tint_rgb) {
                *channel = (*channel as f32 + (tint - *channel as f32) * amount).round() as u8;
            }
        }
        grain(&mut base, x, y);
        base.to_image()
    }
}

/// Uma coluna do fundo do vidro: os dois pixels vizinhos na imagem desfocada (o índice do byte
/// de cada um na linha) e o peso entre eles.
#[derive(Clone, Copy)]
struct Column {
    left: usize,
    right: usize,
    weight: f32,
}

/// O contraste mínimo do texto secundário (o `MUTED`, das notas e legendas) sobre o vidro: 4,5:1,
/// o da WCAG para texto pequeno.
const MIN_TEXT_CONTRAST: f32 = 4.5;
/// O máximo que o vidro escurece (mais que isso, vira um cartão preto).
const MAX_TINT: f32 = 0.85;
/// O mínimo (mesmo num papel de parede escuro, o vidro escurece um pouco).
const MIN_TINT: f32 = 0.12;
/// De quanto em quanto o escurecimento sobe até o texto ficar legível.
const TINT_STEP: f32 = 0.01;

/// Quanto o vidro escurece (0 a 1) sobre um papel de parede de claridade `light` (0 a 1), com o
/// escurecimento escolhido `tint`: quanto mais claro o fundo, mais escurece; e nunca menos que o
/// necessário para o texto secundário ficar legível (`MIN_TEXT_CONTRAST`), mesmo com o
/// escurecimento no mínimo das configurações (lá, um papel de parede médio-claro deixava o texto
/// em 3,5:1).
pub fn tint_amount(tint: f64, light: f32) -> f32 {
    let mut amount = (tint as f32 + (light - 0.3) * 0.8).clamp(MIN_TINT, MAX_TINT);
    while amount < MAX_TINT && muted_contrast(light, amount) < MIN_TEXT_CONTRAST {
        amount = (amount + TINT_STEP).min(MAX_TINT);
    }
    amount
}

/// O contraste do texto secundário sobre o vidro escurecido `amount` (0 a 1) em cima de um papel
/// de parede cinza de claridade `light` (0 a 1).
fn muted_contrast(light: f32, amount: f32) -> f32 {
    let wall = (light.clamp(0.0, 1.0) * 255.0).round() as u8;
    let glass = Rgba::rgb(wall, wall, wall).mix(Rgba::rgb(TINT.0, TINT.1, TINT.2), amount);
    contrast(MUTED.over(glass), glass)
}

/// Vidro pronto: base (já escurecida), reflexo no topo, borda clara, conteúdo e cantos redondos.
pub fn finish_glass(base: &Image, content: &Image, radius: f32, s: f32) -> Image {
    let (w, h) = (base.w as f32, base.h as f32);
    let mut c = Canvas::new(base.w, base.h, 1.0);
    c.push_round_clip(0.0, 0.0, w, h, radius);
    c.image(base, 0.0, 0.0);
    // reflexo branco no topo, sumindo até 60% da altura
    c.gradient_round_rect(0.0, 0.0, w, h, radius, &[(0.0, WHITE.alpha(SHEEN)), (0.6, WHITE.alpha(0.0)), (1.0, WHITE.alpha(0.0))], 0.0, h);
    c.image(content, 0.0, 0.0);
    // borda fina e clara por cima de tudo, mais forte no alto, e um fio de luz logo abaixo dela
    let line = s.round().max(1.0);
    c.gradient_stroke_round_rect(0.0, 0.0, w, h, radius, WHITE.alpha(EDGE.0), WHITE.alpha(EDGE.1), line);
    c.gradient_stroke_round_rect(line, line, w - line, h - line, (radius - line).max(0.0), WHITE.alpha(0.10), WHITE.alpha(0.0), line);
    c.pop_clip();
    c.finish()
}

/// O vidro de um cartão em peças, para montar com um conteúdo novo sem refazer o resto: o mesmo
/// que `finish_glass`, que repete tudo a cada vez (camada com recorte, fundo, reflexo, conteúdo e
/// as duas bordas em degradê, numa tela em memória) quando só o conteúdo mudou.
///
/// As peças são desenhadas com as mesmas chamadas de `finish_glass`; montar é só a composição
/// "por cima" de cada pixel (a mesma conta que o Direct2D faz), que pula o que não muda: pixels
/// transparentes do conteúdo, bordas só onde há borda, cantos só onde há canto.
pub struct GlassParts {
    /// fundo com o reflexo (opaco)
    under: Image,
    /// as duas bordas (alfa pré-multiplicado) e onde elas têm tinta (índices de pixel)
    edges: Image,
    edge_px: Vec<u32>,
    /// quanto de cada pixel fica dentro do retângulo arredondado, onde não é inteiro: (índice, 0 a 255)
    corners: Vec<(u32, u8)>,
}

impl GlassParts {
    /// Desenha as peças sobre o fundo `base`, com cantos de raio `radius` (pixels) na escala `s`.
    pub fn new(base: &Image, radius: f32, s: f32) -> GlassParts {
        let (w, h) = (base.w as f32, base.h as f32);
        let mut c = Canvas::new(base.w, base.h, 1.0);
        c.image(base, 0.0, 0.0);
        c.gradient_round_rect(0.0, 0.0, w, h, radius, &[(0.0, WHITE.alpha(SHEEN)), (0.6, WHITE.alpha(0.0)), (1.0, WHITE.alpha(0.0))], 0.0, h);
        let under = c.finish();
        let mut c = Canvas::new(base.w, base.h, 1.0);
        let line = s.round().max(1.0);
        c.gradient_stroke_round_rect(0.0, 0.0, w, h, radius, WHITE.alpha(EDGE.0), WHITE.alpha(EDGE.1), line);
        c.gradient_stroke_round_rect(line, line, w - line, h - line, (radius - line).max(0.0), WHITE.alpha(0.10), WHITE.alpha(0.0), line);
        let edges = c.finish();
        let edge_px = edges.px.chunks_exact(4).enumerate().filter(|(_, pixel)| pixel.iter().any(|&value| value != 0)).map(|(i, _)| i as u32).collect();
        let mut c = Canvas::new(base.w, base.h, 1.0);
        c.fill_round_rect(0.0, 0.0, w, h, radius, WHITE);
        let corners = c.finish().px.chunks_exact(4).enumerate().filter(|(_, pixel)| pixel[3] != 255).map(|(i, pixel)| (i as u32, pixel[3])).collect();
        GlassParts { under, edges, edge_px, corners }
    }

    /// O vidro com este conteúdo (do mesmo tamanho).
    pub fn compose(&self, content: &Image) -> Image {
        // (a mesma conta do Direct2D: cor de cima + a de baixo × o que a de cima deixa passar,
        // com alfa pré-multiplicado, arredondada)
        #[inline(always)]
        fn mul(value: u8, factor: u32) -> u32 {
            (value as u32 * factor + 127) / 255
        }
        let mut out = self.under.clone();
        if content.w == out.w && content.h == out.h {
            for (under, over) in out.px.chunks_exact_mut(4).zip(content.px.chunks_exact(4)) {
                let alpha = over[3];
                if alpha == 0 {
                    continue;
                }
                if alpha == 255 {
                    under.copy_from_slice(over);
                    continue;
                }
                let keep = 255 - alpha as u32;
                for channel in 0..4 {
                    under[channel] = (over[channel] as u32 + mul(under[channel], keep)).min(255) as u8;
                }
            }
        }
        for &i in &self.edge_px {
            let i = i as usize * 4;
            let edge = &self.edges.px[i..i + 4];
            let keep = 255 - edge[3] as u32;
            for channel in 0..4 {
                out.px[i + channel] = (edge[channel] as u32 + mul(out.px[i + channel], keep)).min(255) as u8;
            }
        }
        for &(i, cover) in &self.corners {
            let i = i as usize * 4;
            for channel in 0..4 {
                out.px[i + channel] = mul(out.px[i + channel], cover as u32) as u8;
            }
        }
        out
    }
}

/// quanto o granulado do vidro clareia ou escurece cada pixel, no máximo (de 0 a 255)
pub const GRAIN: f32 = 2.5;

/// Um granulado quase invisível no vidro (o pedaço `base` do fundo, com o canto em (x, y) na
/// tela): os degradês grandes e escuros do vidro formavam faixas visíveis, e o ruído as desfaz.
/// O grão é preso à tela, e não ao cartão: arrastando o widget, ele não "nada" junto.
pub fn grain(base: &mut Rgb, x: i32, y: i32) {
    let w = base.w as i32;
    for (i, pixel) in base.px.chunks_exact_mut(3).enumerate() {
        let (px, py) = (x + i as i32 % w, y + i as i32 / w);
        let shift = (noise(px, py) - 0.5) * 2.0 * GRAIN;
        for channel in pixel.iter_mut() {
            *channel = (*channel as f32 + shift).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Um número de 0 a 1 que parece sorteado, mas é sempre o mesmo para o mesmo pixel.
fn noise(x: i32, y: i32) -> f32 {
    let mut hash = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0x5bd1_e995);
    hash ^= hash >> 15;
    (hash & 0xffff) as f32 / 65535.0
}

/// A imagem do papel de parede de `path` (a cópia do Windows, se ela existir).
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
        let s = self.scale_for(x, y);
        let sigma = blur_px as f32 * s / DOWN;
        let pad = (3.0 * sigma + 2.0).ceil();
        let ox = (bx - pad).max(0.0).floor();
        let oy = (by - pad).max(0.0).floor();
        let cw = ((bx + bw + pad).min(self.sharp.w as f32) - ox).ceil().max(1.0) as u32;
        let ch = ((by + bh + pad).min(self.sharp.h as f32) - oy).ceil().max(1.0) as u32;
        let mut piece = self.sharp.crop(ox as u32, oy as u32, cw, ch);
        blur(&mut piece.px, cw, ch, 3, sigma);
        let mut base = piece.sample(bx - ox, by - oy, bw, bh, w, h);
        let gray = base.gray();
        let light = gray.iter().map(|&level| level as u64).sum::<u64>() as f32 / gray.len().max(1) as f32 / 255.0;
        let amount = tint_amount(tint, light);
        let tint_rgb = [TINT.0 as f32, TINT.1 as f32, TINT.2 as f32];
        for pixel in base.px.chunks_exact_mut(3) {
            for (channel, tint_channel) in pixel.iter_mut().zip(tint_rgb) {
                *channel = (*channel as f32 + (tint_channel - *channel as f32) * amount).round() as u8;
            }
        }
        grain(&mut base, x, y);
        finish_glass(&base.to_image(), content, radius, s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O texto secundário fica legível (4,5:1) sobre o vidro em qualquer papel de parede, do
    /// preto ao branco, com o escurecimento no padrão e no mínimo das configurações; e no padrão,
    /// onde já ficava legível, o escurecimento é o mesmo de antes.
    #[test]
    fn secondary_text_is_always_legible() {
        for tint in [TINT_AMOUNT, 0.15, 0.35] {
            for step in 0..=100 {
                let light = step as f32 / 100.0;
                let amount = tint_amount(tint, light);
                assert!(muted_contrast(light, amount) >= MIN_TEXT_CONTRAST, "escurecimento {tint}, claridade {light}: {:.2}", muted_contrast(light, amount));
                if tint == TINT_AMOUNT {
                    assert_eq!(amount, (tint as f32 + (light - 0.3) * 0.8).clamp(MIN_TINT, MAX_TINT), "o padrão mudou em {light}");
                }
            }
        }
    }

    /// O vidro montado por peças sai igual ao feito de uma vez (`finish_glass`), com o conteúdo de
    /// todos os widgets em todos os tamanhos: no máximo um tom de diferença em algum canal (o
    /// arredondamento da conta do Direct2D por dentro), e quase sempre nenhuma.
    #[test]
    fn glass_parts_match_finish_glass() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let mut picture = Image::new(800, 600);
        for (i, p) in picture.px.chunks_exact_mut(4).enumerate() {
            let (x, y) = (i as u32 % 800, i as u32 / 800);
            p.copy_from_slice(&[(x * 5 % 256) as u8, (y * 3 % 256) as u8, ((x + y) % 200) as u8, 255]);
        }
        let s = 1.25;
        let bd = Backdrop::from_picture(&picture, 8.0, s);
        let (mut worst, mut differing, mut total, mut drawn) = (0u8, 0usize, 0usize, 0);
        for (name, kind, cfg) in crate::testes::cases().into_iter().step_by(7) {
            for &mode in kind.sizes() {
                let l = crate::widgets::Layout::of(mode);
                let (w, h) = ((l.w * s).round() as u32, (l.h * s).round() as u32);
                let mut c = Canvas::new(w, h, s);
                kind.draw(&mut c, mode, &crate::widgets::Env { cfg: &cfg, key: kind.key() });
                let content = c.finish();
                let base = bd.glass_base(w, h, 40, 30, 0.4);
                let r = l.radius * s;
                let whole = finish_glass(&base, &content, r, s);
                let parts = GlassParts::new(&base, r, s).compose(&content);
                assert_eq!(whole.px.len(), parts.px.len(), "{name}");
                for (a, b) in whole.px.iter().zip(&parts.px) {
                    let d = a.abs_diff(*b);
                    worst = worst.max(d);
                    differing += (d > 0) as usize;
                }
                total += whole.px.len();
                drawn += 1;
            }
        }
        println!("vidros: {drawn}; maior diferença: {worst}; bytes diferentes: {differing} de {total}");
        assert!(drawn > 20);
        assert!(worst <= 1, "maior diferença {worst}");
    }

    /// O fundo do vidro numa passada só sai igual, byte a byte, ao feito passo a passo: em vários
    /// lugares (até saindo da tela), tamanhos, escurecimentos e escalas.
    #[test]
    fn fast_glass_base_is_identical() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let (w, h) = (640u32, 400u32);
        let mut picture = Image::new(w, h);
        for (i, p) in picture.px.chunks_exact_mut(4).enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            p.copy_from_slice(&[(x * 7 % 256) as u8, (y * 3 % 256) as u8, ((x ^ y) % 256) as u8, 255]);
        }
        for s in [1.0, 1.25, 1.5] {
            let bd = Backdrop::from_picture(&picture, 8.0, s);
            for (x, y, cw, ch, tint) in [(0, 0, 200, 120, 0.4), (37, 51, 455, 282, 0.49), (-30, -12, 158, 158, 0.12), (600, 380, 300, 200, 0.85), (123, 77, 1, 1, 0.3)] {
                let fast = bd.glass_base(cw, ch, x, y, tint);
                let slow = bd.glass_base_steps(cw, ch, x, y, tint);
                assert_eq!((fast.w, fast.h), (slow.w, slow.h));
                let first = fast.px.iter().zip(&slow.px).position(|(a, b)| a != b);
                assert!(first.is_none(), "diferente em {:?} na escala {s}: byte {first:?} ({:?} × {:?})", (x, y, cw, ch, tint), first.map(|i| &fast.px[i / 4 * 4..i / 4 * 4 + 4]), first.map(|i| &slow.px[i / 4 * 4..i / 4 * 4 + 4]));
            }
        }
    }

    /// O granulado é leve, preso à tela e não muda a cor média do vidro.
    #[test]
    fn grain_is_faint_fixed_to_the_screen_and_neutral() {
        let flat = Rgb::filled(64, 64, (40, 40, 48));
        let mut a = Rgb::filled(64, 64, (40, 40, 48));
        grain(&mut a, 100, 200);
        let diffs: Vec<i32> = a.px.iter().zip(&flat.px).map(|(x, y)| *x as i32 - *y as i32).collect();
        assert!(diffs.iter().all(|d| d.abs() <= GRAIN.ceil() as i32), "forte demais");
        assert!(diffs.iter().any(|d| *d != 0), "sem grão");
        let mean = diffs.iter().sum::<i32>() as f32 / diffs.len() as f32;
        assert!(mean.abs() < 0.3, "mudou a cor média: {mean}");
        // o mesmo pedaço da tela tem o mesmo grão, venha de que cartão vier
        let mut b = Rgb::filled(64, 64, (40, 40, 48));
        grain(&mut b, 99, 200);
        assert_eq!(&a.px[..63 * 3], &b.px[3..64 * 3]);
        // sem cinza puro saindo colorido: os três canais mexem juntos
        assert!(a.px.chunks(3).all(|p| p[0] as i32 - 40 == p[2] as i32 - 48));
    }
}
