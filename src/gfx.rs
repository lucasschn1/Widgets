//! Desenho: Direct2D (formas com bordas suaves), DirectWrite (texto com as fontes do Windows)
//! e WIC (imagens). Tudo é desenhado numa `Canvas` em memória e vira uma `Image` com alfa
//! pré-multiplicado (BGRA), que é o formato que as janelas transparentes do Windows usam.
//!
//! As coordenadas são "lógicas" (96 dpi); a canvas multiplica pela escala `s` do monitor.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows_numerics::Vector2;

use crate::theme::Rgba;

/// Imagem com alfa pré-multiplicado, em BGRA (4 bytes por pixel, linha a linha).
#[derive(Clone)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Image {
    pub fn new(w: u32, h: u32) -> Image {
        Image { w, h, px: vec![0; (w * h * 4) as usize] }
    }

    /// Copia `src` por cima desta imagem (composição "over"), com o canto em (x, y) pixels.
    pub fn over(&mut self, src: &Image, x: i32, y: i32) {
        for sy in 0..src.h as i32 {
            let dy = y + sy;
            if dy < 0 || dy >= self.h as i32 {
                continue;
            }
            for sx in 0..src.w as i32 {
                let dx = x + sx;
                if dx < 0 || dx >= self.w as i32 {
                    continue;
                }
                let si = ((sy as u32 * src.w + sx as u32) * 4) as usize;
                let a = src.px[si + 3] as u32;
                if a == 0 {
                    continue;
                }
                let di = ((dy as u32 * self.w + dx as u32) * 4) as usize;
                for k in 0..4 {
                    let d = self.px[di + k] as u32;
                    self.px[di + k] = (src.px[si + k] as u32 + d * (255 - a) / 255).min(255) as u8;
                }
            }
        }
    }

    /// Multiplica a imagem toda por `alpha` (0 a 1).
    pub fn fade(&mut self, alpha: f32) {
        let a = (alpha.clamp(0.0, 1.0) * 256.0) as u32;
        for v in self.px.iter_mut() {
            *v = ((*v as u32 * a) >> 8) as u8;
        }
    }

    /// Mistura duas imagens do mesmo tamanho: (1 - t) * a + t * b.
    pub fn blend(a: &Image, b: &Image, t: f32) -> Image {
        let t = (t.clamp(0.0, 1.0) * 256.0) as u32;
        let px = a.px.iter().zip(&b.px).map(|(&x, &y)| ((x as u32 * (256 - t) + y as u32 * t) >> 8) as u8).collect();
        Image { w: a.w, h: a.h, px }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Face {
    Regular,
    Semibold,
    Light,
    Symbol,
    Icons,
    /// Qualquer família, no peso dado (100 a 900): primeiro entre as fontes embutidas no app
    /// (a Outfit), senão entre as do Windows.
    Custom(&'static str, u16),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anchor {
    /// (horizontal, vertical): l/m/r e a (topo da fonte) / m (meio do texto) / s (linha de base)
    LA,
    LM,
    LS,
    MA,
    MM,
    MS,
    RA,
    RM,
    RS,
}

impl Anchor {
    pub fn parts(self) -> (char, char) {
        match self {
            Anchor::LA => ('l', 'a'),
            Anchor::LM => ('l', 'm'),
            Anchor::LS => ('l', 's'),
            Anchor::MA => ('m', 'a'),
            Anchor::MM => ('m', 'm'),
            Anchor::MS => ('m', 's'),
            Anchor::RA => ('r', 'a'),
            Anchor::RM => ('r', 'm'),
            Anchor::RS => ('r', 's'),
        }
    }
}

pub struct Gfx {
    pub d2d: ID2D1Factory,
    pub dw: IDWriteFactory,
    pub wic: IWICImagingFactory,
    formats: RefCell<HashMap<(Face, u32), IDWriteTextFormat>>,
    round: ID2D1StrokeStyle,
    icon_family: &'static str,
    /// números grandes (temperatura, relógio): a fonte de títulos do Windows 11, se houver
    display_family: &'static str,
    /// algarismos de largura igual (o relógio não "treme" quando os números mudam)
    tabular: Option<IDWriteTypography>,
    /// as fontes embutidas no executável (None se o Windows não conseguir carregá-las)
    embedded: Option<IDWriteFontCollection>,
}

/// As fontes embutidas: a Outfit, variável do peso 100 ao 900 (licença SIL Open Font License, ver
/// res/fonts/OFL.txt).
static EMBEDDED: [&[u8]; 1] = [include_bytes!("../res/fonts/Outfit.ttf")];

/// Coleção com as fontes destes arquivos, lidos da memória (sem instalar nada no Windows; o
/// DirectWrite guarda uma cópia dos bytes).
fn memory_collection(dw: &IDWriteFactory, files: &[&[u8]]) -> windows::core::Result<IDWriteFontCollection> {
    unsafe {
        let f5: IDWriteFactory5 = dw.cast()?;
        let loader = f5.CreateInMemoryFontFileLoader()?;
        f5.RegisterFontFileLoader(&loader)?;
        let builder = f5.CreateFontSetBuilder()?;
        for data in files {
            let file = loader.CreateInMemoryFontFileReference(dw, data.as_ptr() as *const _, data.len() as u32, None)?;
            builder.AddFontFile(&file)?;
        }
        let set = builder.CreateFontSet()?;
        f5.CreateFontCollectionFromFontSet(&set)?.cast()
    }
}

thread_local! {
    static GFX: Gfx = Gfx::new().expect("Direct2D/DirectWrite indisponível");
}

/// Roda `f` com as fábricas de desenho (criadas uma vez por thread).
pub fn with<R>(f: impl FnOnce(&Gfx) -> R) -> R {
    GFX.with(f)
}

impl Gfx {
    fn new() -> windows::core::Result<Gfx> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
            };
            let round = d2d.CreateStrokeStyle(&props, None)?;
            let icon_family = if crate::win::font_exists("SegoeIcons.ttf") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" };
            let display_family = if crate::win::font_exists("SegUIVar.ttf") { "Segoe UI Variable Display" } else { "Segoe UI" };
            let tabular = dw.CreateTypography().ok().filter(|t| {
                t.AddFontFeature(DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, parameter: 1 }).is_ok()
            });
            let embedded = memory_collection(&dw, &EMBEDDED).ok();
            Ok(Gfx { d2d, dw, wic, formats: RefCell::new(HashMap::new()), round, icon_family, display_family, tabular, embedded })
        }
    }

    fn format(&self, face: Face, px: f32) -> IDWriteTextFormat {
        let key = (face, (px * 100.0).round() as u32);
        if let Some(f) = self.formats.borrow().get(&key) {
            return f.clone();
        }
        // uma família embutida, se for uma delas (senão, a do Windows com esse nome)
        let custom = |family: &'static str| {
            let name = wide(family);
            let (mut index, mut exists) = (0u32, windows::core::BOOL(0));
            let found = self.embedded.as_ref().filter(|c| unsafe { c.FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists).is_ok() } && exists.as_bool());
            (family, found)
        };
        let (family, weight, collection): (&str, DWRITE_FONT_WEIGHT, Option<&IDWriteFontCollection>) = match face {
            Face::Regular => ("Segoe UI", DWRITE_FONT_WEIGHT_NORMAL, None),
            Face::Semibold => ("Segoe UI", DWRITE_FONT_WEIGHT_SEMI_BOLD, None),
            Face::Light => (self.display_family, DWRITE_FONT_WEIGHT_LIGHT, None),
            Face::Symbol => ("Segoe UI Symbol", DWRITE_FONT_WEIGHT_NORMAL, None),
            Face::Icons => (self.icon_family, DWRITE_FONT_WEIGHT_NORMAL, None),
            Face::Custom(family, weight) => {
                let (family, c) = custom(family);
                (family, DWRITE_FONT_WEIGHT(weight as i32), c)
            }
        };
        let family = wide(family);
        let format = unsafe {
            self.dw
                .CreateTextFormat(PCWSTR(family.as_ptr()), collection, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, px, w!("pt-br"))
                .expect("CreateTextFormat")
        };
        unsafe {
            let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        }
        self.formats.borrow_mut().insert(key, format.clone());
        format
    }

    /// Mede um texto: (largura, distância do topo até a linha de base, topo e base da "tinta" em
    /// relação à linha de base), em pixels.
    fn measure(&self, text: &str, face: Face, px: f32) -> (IDWriteTextLayout, f32, f32, f32, f32) {
        let format = self.format(face, px);
        let utf16: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout = self.dw.CreateTextLayout(&utf16, &format, 100_000.0, 0.0).expect("CreateTextLayout");
            if let (Some(t), false) = (&self.tabular, matches!(face, Face::Icons | Face::Symbol)) {
                let _ = layout.SetTypography(t, DWRITE_TEXT_RANGE { startPosition: 0, length: utf16.len() as u32 });
            }
            let mut metrics = DWRITE_TEXT_METRICS::default();
            let _ = layout.GetMetrics(&mut metrics);
            let mut lines = [DWRITE_LINE_METRICS::default(); 1];
            let mut count = 0;
            let _ = layout.GetLineMetrics(Some(&mut lines), &mut count);
            let baseline = lines[0].baseline;
            let over = layout.GetOverhangMetrics().unwrap_or_default();
            // com altura máxima 0, a "caixa" do texto começa e termina no topo da linha
            let ink_top = -over.top - baseline;
            let ink_bottom = over.bottom - baseline;
            (layout, metrics.widthIncludingTrailingWhitespace, baseline, ink_top, ink_bottom)
        }
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// O canto esquerdo e a linha de base de um texto (em pixels) com o ponto `anchor` em (x, y).
fn text_origin(x: f32, y: f32, anchor: Anchor, width: f32, baseline: f32, ink_top: f32, ink_bottom: f32) -> (f32, f32) {
    let (h, v) = anchor.parts();
    let left = match h {
        'm' => x - width / 2.0,
        'r' => x - width,
        _ => x,
    };
    let base = match v {
        'a' => y + baseline,
        'm' => y - (ink_top + ink_bottom) / 2.0,
        _ => y,
    };
    (left, base)
}

// ---------- gravação (a troca de tamanho) ----------

/// Um desenho gravado, em unidades lógicas (o que cada função da `Canvas` recebeu).
#[derive(Clone)]
pub enum Op {
    RRect { x0: f32, y0: f32, x1: f32, y1: f32, r: f32, fill: Fill },
    StrokeRRect { x0: f32, y0: f32, x1: f32, y1: f32, r: f32, c: Rgba, width: f32 },
    GradV { x0: f32, y0: f32, x1: f32, y1: f32, r: f32, stops: Vec<(f32, Rgba)>, g0: f32, g1: f32 },
    GradH { x0: f32, y0: f32, x1: f32, y1: f32, r: f32, stops: Vec<(f32, Rgba)>, g0: f32, g1: f32 },
    StrokeV { x0: f32, y0: f32, x1: f32, y1: f32, r: f32, top: Rgba, bottom: Rgba, width: f32 },
    Glow { x0: f32, y0: f32, x1: f32, y1: f32, r: f32, cx: f32, cy: f32, radius: f32, c: Rgba, width: f32, veil: f32 },
    Circle { cx: f32, cy: f32, r: f32, fill: Fill },
    StrokeCircle { cx: f32, cy: f32, r: f32, c: Rgba, width: f32 },
    Crescent { cx: f32, cy: f32, r: f32, cx2: f32, cy2: f32, r2: f32, fill: Fill },
    Radial { cx: f32, cy: f32, rx: f32, ry: f32, stops: Vec<(f32, Rgba)> },
    /// linha (com espessura) ou polígono preenchido (sem)
    Path { pts: Vec<(f32, f32)>, fill: Fill, width: Option<f32>, closed: bool },
    Arc { cx: f32, cy: f32, r: f32, start: f32, end: f32, c: Rgba, width: f32 },
    Segments { segs: Vec<(f32, f32, f32, f32)>, c: Rgba, width: f32 },
    HLine { x0: f32, x1: f32, y: f32, c: Rgba },
    VLine { x: f32, y0: f32, y1: f32, c: Rgba },
    /// no retângulo (x, y, w, h); `sharp`: redimensionada com filtro de qualidade
    Image { img: Image, x: f32, y: f32, w: f32, h: f32, opacity: f32, sharp: bool },
    /// já cortado com "…", com o canto esquerdo e a linha de base
    Text { left: f32, base: f32, text: String, face: Face, size: f32, c: Rgba },
}

/// Um recorte arredondado: (x0, y0, x1, y1, raio).
pub type Clip = (f32, f32, f32, f32, f32);

/// Um desenho gravado com o recorte e a opacidade em que foi feito (as camadas abertas na
/// hora: cada desenho fica com as dele, sem depender da ordem).
#[derive(Clone)]
pub struct Rec {
    pub op: Op,
    pub clip: Option<Clip>,
    pub alpha: f32,
}

#[derive(Clone, Copy, Debug)]
enum Layer {
    Clip(f32, f32, f32, f32, f32),
    Rect(f32, f32, f32, f32),
    Alpha(f32),
}

#[derive(Default)]
struct Recorder {
    ops: Vec<Rec>,
    layers: Vec<Layer>,
}

impl Recorder {
    fn push(&mut self, op: Op) {
        let (mut clip, mut alpha): (Option<Clip>, f32) = (None, 1.0);
        for l in &self.layers {
            let (b, r) = match *l {
                Layer::Alpha(a) => {
                    alpha *= a;
                    continue;
                }
                Layer::Clip(x0, y0, x1, y1, r) => ((x0, y0, x1, y1), Some(r)),
                Layer::Rect(x0, y0, x1, y1) => ((x0, y0, x1, y1), None),
            };
            clip = Some(match clip {
                None => (b.0, b.1, b.2, b.3, r.unwrap_or(0.0)),
                Some(a) => (a.0.max(b.0), a.1.max(b.1), a.2.min(b.2), a.3.min(b.3), r.unwrap_or(a.4)),
            });
        }
        self.ops.push(Rec { op, clip, alpha });
    }
}

/// Corta o texto com "…" até caber em `max_px` (como o Python fazia).
fn fit_text(g: &Gfx, text: &str, face: Face, px: f32, max_px: Option<f32>) -> String {
    let Some(max_px) = max_px else { return text.to_string() };
    if g.measure(text, face, px).1 <= max_px {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    // busca binária pelo maior prefixo que cabe junto com o "…"
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if g.measure(&candidate, face, px).1 <= max_px {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let head: String = chars[..lo].iter().collect();
    head.trim_end_matches([' ', ',', '-', '·', '|']).to_string() + "…"
}

/// Como pintar uma forma: cor sólida ou degradê vertical (de `top` em y0 a `bottom` em y1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    Solid(Rgba),
    VGrad(f32, f32, Rgba, Rgba),
}

fn color(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0 as f32 / 255.0, g: c.1 as f32 / 255.0, b: c.2 as f32 / 255.0, a: c.3 as f32 / 255.0 }
}

/// Quanto o canto "contínuo" se estende além do arco (0: o canto redondo comum; 0,6: o do
/// iOS e do macOS).
pub const CORNER_SMOOTHING: f32 = 0.6;
/// raios menores que isto (em pixels) ficam com o canto redondo comum: a diferença não aparece
const SMOOTH_MIN_PX: f32 = 6.0;

/// As medidas de um canto contínuo de raio `r` (pixels), sem passar de `budget` (metade do lado
/// menor): `p` é até onde ele vai ao longo de cada lado; a curva de entrada vai de `p` até
/// `p - a - b - c` (já `d` para dentro), o arco de raio `r` cobre o meio e a de saída é o espelho
/// da de entrada. (As contas são as do "corner smoothing" do Figma.)
#[derive(Clone, Copy, Debug, PartialEq)]
struct Corner {
    p: f32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
}

fn corner(r: f32, budget: f32) -> Corner {
    let p = ((1.0 + CORNER_SMOOTHING) * r).min(budget);
    // sem espaço para tudo, o canto suaviza menos (no limite, vira o redondo comum)
    let sm = CORNER_SMOOTHING.min((budget / r - 1.0).max(0.0));
    let arc = 90.0 * (1.0 - sm);
    let arc_side = (arc / 2.0f32).to_radians().sin() * r * std::f32::consts::SQRT_2;
    let alpha = (90.0 - arc) / 2.0;
    let p34 = r * (alpha / 2.0f32).to_radians().tan();
    let beta = (45.0 * sm).to_radians();
    let c = p34 * beta.cos();
    let d = c * beta.tan();
    let b = (p - arc_side - c - d) / 3.0;
    Corner { p, a: 2.0 * b, b, c, d }
}

/// Retângulo arredondado com cantos contínuos, como os do macOS: a curvatura cresce aos poucos,
/// sem a "quina" onde a reta encontra o arco do canto redondo comum. Em pixels.
fn round_rect_geometry(g: &Gfx, rect: D2D_RECT_F, r: f32) -> ID2D1Geometry {
    let budget = ((rect.right - rect.left).min(rect.bottom - rect.top) / 2.0).max(0.0);
    let r = r.clamp(0.0, budget);
    unsafe {
        if r < SMOOTH_MIN_PX {
            return g.d2d.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect, radiusX: r, radiusY: r }).expect("geom").cast().unwrap();
        }
        let k = corner(r, budget);
        let path = g.d2d.CreatePathGeometry().expect("path");
        let sink = path.Open().expect("sink");
        // cada canto, no sentido horário: o ponto do canto, o sentido do lado que chega nele e o
        // do lado que sai dele (um ponto é `e` antes do canto no lado que chega e `f` adiante
        // no que sai)
        let corners = [
            ((rect.right, rect.top), (1.0, 0.0), (0.0, 1.0)),
            ((rect.right, rect.bottom), (0.0, 1.0), (-1.0, 0.0)),
            ((rect.left, rect.bottom), (-1.0, 0.0), (0.0, -1.0)),
            ((rect.left, rect.top), (0.0, -1.0), (1.0, 0.0)),
        ];
        for (i, &((kx, ky), (ux, uy), (vx, vy))) in corners.iter().enumerate() {
            let at = |e: f32, f: f32| v2(kx - ux * e + vx * f, ky - uy * e + vy * f);
            let start = at(k.p, 0.0);
            if i == 0 {
                sink.BeginFigure(start, D2D1_FIGURE_BEGIN_FILLED);
            } else {
                sink.AddLine(start);
            }
            let mid = k.p - k.a - k.b - k.c;
            sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: at(k.p - k.a, 0.0), point2: at(k.p - k.a - k.b, 0.0), point3: at(mid, k.d) });
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: at(k.d, mid),
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: at(0.0, k.p - k.a - k.b), point2: at(0.0, k.p - k.a), point3: at(0.0, k.p) });
        }
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        let _ = sink.Close();
        path.cast().unwrap()
    }
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

/// Tela de desenho, em memória (`new`) ou direto na superfície da placa de vídeo de uma janela
/// (`on_target`). Coordenadas lógicas; `s` é a escala do monitor.
pub struct Canvas {
    rt: ID2D1RenderTarget,
    /// os pixels, na tela em memória (None: desenhando na placa de vídeo)
    bmp: Option<IWICBitmap>,
    pub w: u32,
    pub h: u32,
    pub s: f32,
    /// tamanho do texto escolhido pelo usuário (acessibilidade), multiplica as fontes
    pub text_scale: f32,
    layers: u32,
    /// gravando: o que seria desenhado vai para cá, e nada é desenhado (a troca de tamanho
    /// liga o que cada tamanho desenha e anda de um para o outro)
    rec: Option<Recorder>,
}

impl Canvas {
    /// Canvas de `w` × `h` pixels, transparente.
    pub fn new(w: u32, h: u32, s: f32) -> Canvas {
        let (w, h) = (w.max(1), h.max(1));
        with(|g| unsafe {
            let bmp = g
                .wic
                .CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnDemand)
                .expect("CreateBitmap");
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = g.d2d.CreateWicBitmapRenderTarget(&bmp, &props).expect("CreateWicBitmapRenderTarget");
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            Canvas { rt, bmp: Some(bmp), w, h, s, text_scale: 1.0, layers: 0, rec: None }
        })
    }

    /// Tela desenhando direto em `rt` (a superfície da placa de vídeo de uma janela, veja `gpu`),
    /// transparente. Termina com `end`, que não lê os pixels de volta.
    pub fn on_target(rt: ID2D1RenderTarget, w: u32, h: u32, s: f32) -> Canvas {
        unsafe {
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        }
        Canvas { rt, bmp: None, w, h, s, text_scale: 1.0, layers: 0, rec: None }
    }

    /// Termina o desenho numa tela da placa de vídeo (`on_target`).
    pub fn end(mut self) -> windows::core::Result<()> {
        while self.layers > 0 {
            self.pop_clip();
        }
        unsafe { self.rt.EndDraw(None, None) }
    }

    fn brush(&self, c: Rgba) -> ID2D1SolidColorBrush {
        unsafe { self.rt.CreateSolidColorBrush(&color(c), None).expect("brush") }
    }

    fn brush_of(&self, f: Fill) -> ID2D1Brush {
        match f {
            Fill::Solid(c) => self.brush(c).cast().unwrap(),
            Fill::VGrad(y0, y1, top, bottom) => unsafe {
                let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: color(top) }, D2D1_GRADIENT_STOP { position: 1.0, color: color(bottom) }];
                let coll = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).expect("stops");
                let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: v2(0.0, y0 * self.s), endPoint: v2(0.0, y1 * self.s) };
                self.rt.CreateLinearGradientBrush(&props, None, &coll).expect("gradient").cast().unwrap()
            },
        }
    }

    pub fn fill_round_rect_with(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, f: Fill) {
        if self.recording(|| Op::RRect { x0, y0, x1, y1, r, fill: f }) {
            return;
        }
        let geom = self.round_geom(x0, y0, x1, y1, r);
        unsafe { self.rt.FillGeometry(&geom, &self.brush_of(f), None) }
    }

    /// O contorno de um retângulo arredondado (cantos contínuos), em unidades lógicas.
    fn round_geom(&self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> ID2D1Geometry {
        let rect = self.rect_px(x0, y0, x1, y1);
        with(|g| round_rect_geometry(g, rect, r * self.s))
    }

    pub fn fill_circle_with(&mut self, cx: f32, cy: f32, r: f32, f: Fill) {
        if self.recording(|| Op::Circle { cx, cy, r, fill: f }) {
            return;
        }
        let s = self.s;
        let e = D2D1_ELLIPSE { point: v2(cx * s, cy * s), radiusX: r * s, radiusY: r * s };
        unsafe { self.rt.FillEllipse(&e, &self.brush_of(f)) }
    }

    /// Lua crescente: o círculo (cx, cy, r) sem o círculo (cx2, cy2, r2).
    pub fn crescent(&mut self, cx: f32, cy: f32, r: f32, cx2: f32, cy2: f32, r2: f32, f: Fill) {
        if self.recording(|| Op::Crescent { cx, cy, r, cx2, cy2, r2, fill: f }) {
            return;
        }
        let s = self.s;
        with(|g| unsafe {
            let a = g.d2d.CreateEllipseGeometry(&D2D1_ELLIPSE { point: v2(cx * s, cy * s), radiusX: r * s, radiusY: r * s }).expect("ellipse");
            let b = g.d2d.CreateEllipseGeometry(&D2D1_ELLIPSE { point: v2(cx2 * s, cy2 * s), radiusX: r2 * s, radiusY: r2 * s }).expect("ellipse");
            let path = g.d2d.CreatePathGeometry().expect("path");
            let sink = path.Open().expect("sink");
            let _ = a.CombineWithGeometry(&b, D2D1_COMBINE_MODE_EXCLUDE, None, 0.25, &sink);
            let _ = sink.Close();
            self.rt.FillGeometry(&path, &self.brush_of(f), None);
        })
    }

    pub fn polygon_with(&mut self, pts: &[(f32, f32)], f: Fill) {
        if self.recording(|| Op::Path { pts: pts.to_vec(), fill: f, width: None, closed: true }) {
            return;
        }
        if pts.len() < 3 {
            return;
        }
        let s = self.s;
        with(|g| unsafe {
            let path = g.d2d.CreatePathGeometry().expect("path");
            let sink = path.Open().expect("sink");
            sink.BeginFigure(v2(pts[0].0 * s, pts[0].1 * s), D2D1_FIGURE_BEGIN_FILLED);
            for p in &pts[1..] {
                sink.AddLine(v2(p.0 * s, p.1 * s));
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            let _ = sink.Close();
            self.rt.FillGeometry(&path, &self.brush_of(f), None);
        })
    }

    /// Linha com pontas redondas pintada com `f`; `closed` fecha o contorno.
    pub fn line_with(&mut self, pts: &[(f32, f32)], f: Fill, width: f32, closed: bool) {
        if self.recording(|| Op::Path { pts: pts.to_vec(), fill: f, width: Some(width), closed }) {
            return;
        }
        if pts.len() < 2 {
            return;
        }
        let s = self.s;
        with(|g| unsafe {
            let path = g.d2d.CreatePathGeometry().expect("path");
            let sink = path.Open().expect("sink");
            sink.BeginFigure(v2(pts[0].0 * s, pts[0].1 * s), D2D1_FIGURE_BEGIN_HOLLOW);
            for p in &pts[1..] {
                sink.AddLine(v2(p.0 * s, p.1 * s));
            }
            sink.EndFigure(if closed { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
            let _ = sink.Close();
            self.rt.DrawGeometry(&path, &self.brush_of(f), width * s, &g.round);
        })
    }

    fn rect_px(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> D2D_RECT_F {
        let s = self.s;
        D2D_RECT_F { left: x0 * s, top: y0 * s, right: x1 * s, bottom: y1 * s }
    }

    pub fn fill_round_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, c: Rgba) {
        if self.recording(|| Op::RRect { x0, y0, x1, y1, r, fill: Fill::Solid(c) }) {
            return;
        }
        let geom = self.round_geom(x0, y0, x1, y1, r);
        unsafe { self.rt.FillGeometry(&geom, &self.brush(c), None) }
    }

    pub fn stroke_round_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, c: Rgba, width: f32) {
        if self.recording(|| Op::StrokeRRect { x0, y0, x1, y1, r, c, width }) {
            return;
        }
        // o traço fica centrado na borda: recua meia espessura para ficar todo por dentro
        let hw = width / 2.0;
        let geom = self.round_geom(x0 + hw, y0 + hw, x1 - hw, y1 - hw, (r - hw).max(0.0));
        unsafe { self.rt.DrawGeometry(&geom, &self.brush(c), width * self.s, None) }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        if self.recording(|| Op::Circle { cx, cy, r, fill: Fill::Solid(c) }) {
            return;
        }
        let s = self.s;
        let e = D2D1_ELLIPSE { point: v2(cx * s, cy * s), radiusX: r * s, radiusY: r * s };
        unsafe { self.rt.FillEllipse(&e, &self.brush(c)) }
    }

    pub fn stroke_circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba, width: f32) {
        if self.recording(|| Op::StrokeCircle { cx, cy, r, c, width }) {
            return;
        }
        let s = self.s;
        let e = D2D1_ELLIPSE { point: v2(cx * s, cy * s), radiusX: r * s, radiusY: r * s };
        unsafe { self.rt.DrawEllipse(&e, &self.brush(c), width * s, None) }
    }

    /// Linha (ou poligonal) com pontas e junções arredondadas.
    pub fn line(&mut self, pts: &[(f32, f32)], c: Rgba, width: f32) {
        if self.recording(|| Op::Path { pts: pts.to_vec(), fill: Fill::Solid(c), width: Some(width), closed: false }) {
            return;
        }
        if pts.len() < 2 {
            return;
        }
        let s = self.s;
        with(|g| unsafe {
            let path = g.d2d.CreatePathGeometry().expect("path");
            let sink = path.Open().expect("sink");
            sink.BeginFigure(v2(pts[0].0 * s, pts[0].1 * s), D2D1_FIGURE_BEGIN_HOLLOW);
            for p in &pts[1..] {
                sink.AddLine(v2(p.0 * s, p.1 * s));
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = sink.Close();
            self.rt.DrawGeometry(&path, &self.brush(c), width * s, &g.round);
        })
    }

    /// Polígono preenchido.
    pub fn polygon(&mut self, pts: &[(f32, f32)], c: Rgba) {
        if self.recording(|| Op::Path { pts: pts.to_vec(), fill: Fill::Solid(c), width: None, closed: true }) {
            return;
        }
        if pts.len() < 3 {
            return;
        }
        let s = self.s;
        with(|g| unsafe {
            let path = g.d2d.CreatePathGeometry().expect("path");
            let sink = path.Open().expect("sink");
            sink.BeginFigure(v2(pts[0].0 * s, pts[0].1 * s), D2D1_FIGURE_BEGIN_FILLED);
            for p in &pts[1..] {
                sink.AddLine(v2(p.0 * s, p.1 * s));
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            let _ = sink.Close();
            self.rt.FillGeometry(&path, &self.brush(c), None);
        })
    }

    /// Arco de círculo de `start` a `end` graus (0 = 3 horas, sentido horário), com pontas redondas.
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, start: f32, end: f32, c: Rgba, width: f32) {
        if self.recording(|| Op::Arc { cx, cy, r, start, end, c, width }) {
            return;
        }
        let sweep = end - start;
        if sweep.abs() < 0.01 {
            return;
        }
        let s = self.s;
        let pt = |deg: f32| {
            let a = deg.to_radians();
            v2((cx + r * a.cos()) * s, (cy + r * a.sin()) * s)
        };
        with(|g| unsafe {
            let path = g.d2d.CreatePathGeometry().expect("path");
            let sink = path.Open().expect("sink");
            sink.BeginFigure(pt(start), D2D1_FIGURE_BEGIN_HOLLOW);
            // arcos de no máximo 180°, para o Direct2D não se confundir com o sentido
            let steps = (sweep.abs() / 179.0).ceil() as i32;
            for i in 1..=steps {
                let a = start + sweep * i as f32 / steps as f32;
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: pt(a),
                    size: D2D_SIZE_F { width: r * s, height: r * s },
                    rotationAngle: 0.0,
                    sweepDirection: if sweep > 0.0 { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
                    arcSize: D2D1_ARC_SIZE_SMALL,
                });
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = sink.Close();
            self.rt.DrawGeometry(&path, &self.brush(c), width * s, &g.round);
        })
    }

    /// Retângulo (arredondado) com degradê vertical de `top` a `bottom`, entre as alturas y0 e y1.
    pub fn gradient_round_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, stops: &[(f32, Rgba)], gy0: f32, gy1: f32) {
        if self.recording(|| Op::GradV { x0, y0, x1, y1, r, stops: stops.to_vec(), g0: gy0, g1: gy1 }) {
            return;
        }
        let s = self.s;
        unsafe {
            let stops: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|&(p, c)| D2D1_GRADIENT_STOP { position: p, color: color(c) }).collect();
            let coll = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).expect("stops");
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: v2(0.0, gy0 * s), endPoint: v2(0.0, gy1 * s) };
            let brush = self.rt.CreateLinearGradientBrush(&props, None, &coll).expect("gradient");
            self.rt.FillGeometry(&self.round_geom(x0, y0, x1, y1, r), &brush, None);
        }
    }

    /// Retângulo (arredondado) com degradê horizontal; as posições das cores vão de gx0 a gx1.
    pub fn gradient_h(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, stops: &[(f32, Rgba)], gx0: f32, gx1: f32) {
        if self.recording(|| Op::GradH { x0, y0, x1, y1, r, stops: stops.to_vec(), g0: gx0, g1: gx1 }) {
            return;
        }
        let s = self.s;
        unsafe {
            let stops: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|&(p, c)| D2D1_GRADIENT_STOP { position: p, color: color(c) }).collect();
            let coll = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).expect("stops");
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: v2(gx0 * s, 0.0), endPoint: v2(gx1 * s, 0.0) };
            let brush = self.rt.CreateLinearGradientBrush(&props, None, &coll).expect("gradient");
            self.rt.FillGeometry(&self.round_geom(x0, y0, x1, y1, r), &brush, None);
        }
    }

    /// Contorno arredondado (todo por dentro do retângulo) e, por dentro dele, um véu bem leve,
    /// os dois com uma luz redonda em (cx, cy): `c` no centro, sumindo até `radius`. É o reflexo
    /// do mouse na borda do vidro; um traço só, liso (sem as emendas de traços curtos).
    #[allow(clippy::too_many_arguments)]
    pub fn glow_round_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, cx: f32, cy: f32, radius: f32, c: Rgba, width: f32, veil: f32) {
        if self.recording(|| Op::Glow { x0, y0, x1, y1, r, cx, cy, radius, c, width, veil }) {
            return;
        }
        let s = self.s;
        let hw = width / 2.0;
        unsafe {
            let brush = |c: Rgba| {
                let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: color(c) }, D2D1_GRADIENT_STOP { position: 1.0, color: color(c.with_a(0)) }];
                let coll = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).expect("stops");
                let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: v2(cx * s, cy * s), gradientOriginOffset: v2(0.0, 0.0), radiusX: radius * s, radiusY: radius * s };
                self.rt.CreateRadialGradientBrush(&props, None, &coll).expect("radial")
            };
            if veil > 0.0 {
                self.rt.FillGeometry(&self.round_geom(x0, y0, x1, y1, r), &brush(c.with_a((c.3 as f32 * veil) as u8)), None);
            }
            let geom = self.round_geom(x0 + hw, y0 + hw, x1 - hw, y1 - hw, (r - hw).max(0.0));
            self.rt.DrawGeometry(&geom, &brush(c), width * s, None);
        }
    }

    /// Elipse (cx, cy, rx, ry) com degradê radial do centro para fora: `stops` de 0 (centro) a 1
    /// (borda). Luzes suaves: o sol no céu, faixas de neblina, flocos macios.
    pub fn fill_radial(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, stops: &[(f32, Rgba)]) {
        if self.recording(|| Op::Radial { cx, cy, rx, ry, stops: stops.to_vec() }) {
            return;
        }
        if rx <= 0.0 || ry <= 0.0 {
            return;
        }
        let s = self.s;
        unsafe {
            let stops: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|&(p, c)| D2D1_GRADIENT_STOP { position: p, color: color(c) }).collect();
            let coll = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).expect("stops");
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: v2(cx * s, cy * s), gradientOriginOffset: v2(0.0, 0.0), radiusX: rx * s, radiusY: ry * s };
            let brush = self.rt.CreateRadialGradientBrush(&props, None, &coll).expect("radial");
            let e = D2D1_ELLIPSE { point: v2(cx * s, cy * s), radiusX: rx * s, radiusY: ry * s };
            self.rt.FillEllipse(&e, &brush);
        }
    }

    /// Muitos traços retos (x0, y0, x1, y1) da mesma cor e espessura, de uma vez só (a chuva:
    /// centenas de gotas por quadro custariam um desenho cada).
    pub fn segments(&mut self, segs: &[(f32, f32, f32, f32)], c: Rgba, width: f32) {
        if self.recording(|| Op::Segments { segs: segs.to_vec(), c, width }) {
            return;
        }
        if segs.is_empty() {
            return;
        }
        let s = self.s;
        with(|g| unsafe {
            let path = g.d2d.CreatePathGeometry().expect("path");
            let sink = path.Open().expect("sink");
            for &(x0, y0, x1, y1) in segs {
                sink.BeginFigure(v2(x0 * s, y0 * s), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(v2(x1 * s, y1 * s));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            let _ = sink.Close();
            self.rt.DrawGeometry(&path, &self.brush(c), width * s, &g.round);
        })
    }

    /// Contorno arredondado com degradê vertical (a borda clara do vidro).
    pub fn gradient_stroke_round_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, top: Rgba, bottom: Rgba, width: f32) {
        if self.recording(|| Op::StrokeV { x0, y0, x1, y1, r, top, bottom, width }) {
            return;
        }
        let s = self.s;
        let hw = width / 2.0;
        unsafe {
            let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: color(top) }, D2D1_GRADIENT_STOP { position: 1.0, color: color(bottom) }];
            let coll = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).expect("stops");
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: v2(0.0, y0 * s), endPoint: v2(0.0, y1 * s) };
            let brush = self.rt.CreateLinearGradientBrush(&props, None, &coll).expect("gradient");
            let geom = self.round_geom(x0 + hw, y0 + hw, x1 - hw, y1 - hw, (r - hw).max(0.0));
            self.rt.DrawGeometry(&geom, &brush, width * s, None);
        }
    }

    /// Linha horizontal fina, alinhada ao pixel (divisórias): espessura de 1 pixel lógico.
    pub fn hline_px(&mut self, x0: f32, x1: f32, y: f32, c: Rgba) {
        if self.recording(|| Op::HLine { x0, x1, y, c }) {
            return;
        }
        let t = self.s.round().max(1.0);
        let py = (y * self.s).round();
        let r = D2D_RECT_F { left: x0 * self.s, top: py, right: x1 * self.s, bottom: py + t };
        unsafe { self.rt.FillRectangle(&r, &self.brush(c)) }
    }

    /// Linha vertical fina, alinhada ao pixel.
    pub fn vline_px(&mut self, x: f32, y0: f32, y1: f32, c: Rgba) {
        if self.recording(|| Op::VLine { x, y0, y1, c }) {
            return;
        }
        let t = self.s.round().max(1.0);
        let px = (x * self.s).round();
        let r = D2D_RECT_F { left: px, top: y0 * self.s, right: px + t, bottom: y1 * self.s };
        unsafe { self.rt.FillRectangle(&r, &self.brush(c)) }
    }

    /// Recorte retangular (área rolável): até `pop_rect_clip`, só aparece o que estiver dentro.
    pub fn push_rect_clip(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        if let Some(r) = self.rec.as_mut() {
            r.layers.push(Layer::Rect(x0, y0, x1, y1));
            return;
        }
        let r = self.rect_px(x0, y0, x1, y1);
        unsafe { self.rt.PushAxisAlignedClip(&r, D2D1_ANTIALIAS_MODE_ALIASED) }
    }

    pub fn pop_rect_clip(&mut self) {
        if let Some(r) = self.rec.as_mut() {
            if let Some(i) = r.layers.iter().rposition(|l| matches!(l, Layer::Rect(..))) {
                r.layers.remove(i);
            }
            return;
        }
        unsafe { self.rt.PopAxisAlignedClip() }
    }

    /// Daqui até `reset_transform`, tudo sai `k` vezes maior (ou menor), em volta de (cx, cy).
    pub fn scale_about(&mut self, cx: f32, cy: f32, k: f32) {
        let (px, py) = (cx * self.s, cy * self.s);
        let m = windows_numerics::Matrix3x2 { M11: k, M12: 0.0, M21: 0.0, M22: k, M31: px * (1.0 - k), M32: py * (1.0 - k) };
        unsafe { self.rt.SetTransform(&m) }
    }

    pub fn reset_transform(&mut self) {
        unsafe { self.rt.SetTransform(&windows_numerics::Matrix3x2::identity()) }
    }

    /// Daqui até `pop_clip`, só aparece o que estiver dentro do retângulo arredondado.
    pub fn push_round_clip(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) {
        if let Some(rec) = self.rec.as_mut() {
            rec.layers.push(Layer::Clip(x0, y0, x1, y1, r));
            return;
        }
        let geom = self.round_geom(x0, y0, x1, y1, r);
        unsafe {
            let layer = self.rt.CreateLayer(None).expect("layer");
            let params = D2D1_LAYER_PARAMETERS {
                contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
                geometricMask: std::mem::ManuallyDrop::new(Some(geom.cast().unwrap())),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: windows_numerics::Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS_NONE,
            };
            self.rt.PushLayer(&params, &layer);
            // devolve a referência guardada no ManuallyDrop
            let _ = std::mem::ManuallyDrop::into_inner(params.geometricMask);
        }
        self.layers += 1;
    }

    /// Daqui até `pop_clip`, tudo é desenhado com a opacidade `alpha` (0 a 1), como uma camada só.
    pub fn push_opacity(&mut self, alpha: f32) {
        if let Some(r) = self.rec.as_mut() {
            r.layers.push(Layer::Alpha(alpha.clamp(0.0, 1.0)));
            return;
        }
        unsafe {
            let layer = self.rt.CreateLayer(None).expect("layer");
            let params = D2D1_LAYER_PARAMETERS {
                contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
                geometricMask: std::mem::ManuallyDrop::new(None),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: windows_numerics::Matrix3x2::identity(),
                opacity: alpha.clamp(0.0, 1.0),
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS_NONE,
            };
            self.rt.PushLayer(&params, &layer);
        }
        self.layers += 1;
    }

    pub fn pop_clip(&mut self) {
        if let Some(r) = self.rec.as_mut() {
            if let Some(i) = r.layers.iter().rposition(|l| !matches!(l, Layer::Rect(..))) {
                r.layers.remove(i);
            }
            return;
        }
        if self.layers > 0 {
            unsafe { self.rt.PopLayer() };
            self.layers -= 1;
        }
    }

    /// Cola uma imagem (pixels) com o canto em (x, y) lógicos, no tamanho dela.
    pub fn image(&mut self, img: &Image, x: f32, y: f32) {
        let (px, py) = ((x * self.s).round(), (y * self.s).round());
        let s = self.s;
        if self.recording(|| Op::Image { img: img.clone(), x: px / s, y: py / s, w: img.w as f32 / s, h: img.h as f32 / s, opacity: 1.0, sharp: false }) {
            return;
        }
        self.image_px(img, px, py, img.w as f32, img.h as f32, 1.0);
    }

    /// Cola uma imagem no retângulo w×h com o canto em (x, y) (em pontos), nítida: ela é
    /// redimensionada antes, com um filtro de qualidade, para o tamanho exato em pixels, e cai
    /// pixel a pixel. Esticar na hora (`image_px`) usa interpolação bilinear, que serrilha as
    /// bordas quando reduz muito (um ícone de 64 px desenhado com 33).
    pub fn image_sharp(&mut self, img: &Image, x: f32, y: f32, w: f32, h: f32, opacity: f32) {
        if self.recording(|| Op::Image { img: img.clone(), x, y, w, h, opacity, sharp: true }) {
            return;
        }
        let s = self.s;
        let (px, py) = ((x * s).round(), (y * s).round());
        let (pw, ph) = (((x + w) * s).round() - px, ((y + h) * s).round() - py);
        if pw < 1.0 || ph < 1.0 {
            return;
        }
        if (pw as u32, ph as u32) == (img.w, img.h) {
            self.image_px(img, px, py, pw, ph, opacity);
        } else {
            let fitted = resize_photo(img, pw as u32, ph as u32);
            self.image_px(&fitted, px, py, pw, ph, opacity);
        }
    }

    /// Cola uma imagem num retângulo em pixels (esticando, com suavização).
    pub fn image_px(&mut self, img: &Image, x: f32, y: f32, w: f32, h: f32, opacity: f32) {
        let sc = self.s;
        if self.recording(|| Op::Image { img: img.clone(), x: x / sc, y: y / sc, w: w / sc, h: h / sc, opacity, sharp: false }) {
            return;
        }
        if img.w == 0 || img.h == 0 {
            return;
        }
        unsafe {
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            let bmp = self
                .rt
                .CreateBitmap(D2D_SIZE_U { width: img.w, height: img.h }, Some(img.px.as_ptr() as _), img.w * 4, &props)
                .expect("bitmap");
            let dest = D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h };
            self.rt.DrawBitmap(&bmp, Some(&dest), opacity, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
        }
    }

    fn font_px(&self, size: f32, face: Face) -> f32 {
        if face == Face::Icons {
            size * self.s
        } else {
            (size * self.s * self.text_scale).round()
        }
    }

    /// Largura de um texto, em unidades lógicas.
    pub fn text_width(&self, text: &str, face: Face, size: f32) -> f32 {
        let px = self.font_px(size, face);
        with(|g| g.measure(text, face, px).1) / self.s
    }

    /// Distância do topo da fonte (onde fica o `y` das âncoras `*A`) até a linha de base, em
    /// unidades lógicas.
    pub fn baseline(&self, face: Face, size: f32) -> f32 {
        let px = self.font_px(size, face);
        with(|g| g.measure("0", face, px).2) / self.s
    }

    /// Quebra o texto em até `max_lines` linhas de largura `max_w`; a última leva o resto
    /// (e é cortada com "…" ao escrever).
    pub fn wrap(&self, text: &str, face: Face, size: f32, max_w: f32, max_lines: usize) -> Vec<String> {
        // o espaço que não quebra (\u{a0}) mantém as palavras juntas ("12 h")
        let words: Vec<&str> = text.split(|c: char| c.is_whitespace() && c != '\u{a0}').filter(|w| !w.is_empty()).collect();
        // numa linha só, vai tudo nela (quem escreve corta com "…" o que passar)
        if max_lines <= 1 {
            return if words.is_empty() { Vec::new() } else { vec![words.join(" ")] };
        }
        let mut lines = Vec::new();
        let mut current = String::new();
        let mut i = 0;
        while i < words.len() {
            let trial = if current.is_empty() { words[i].to_string() } else { format!("{current} {}", words[i]) };
            if current.is_empty() || self.text_width(&trial, face, size) <= max_w {
                current = trial;
                i += 1;
                continue;
            }
            lines.push(std::mem::take(&mut current));
            if lines.len() == max_lines - 1 {
                current = words[i..].join(" ");
                i = words.len();
                break;
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines
    }

    /// Escreve um texto; (x, y) é o ponto de `anchor`. Corta com "…" se passar de `max_w`.
    /// Devolve a largura escrita (lógica).
    pub fn text(&mut self, x: f32, y: f32, text: &str, face: Face, size: f32, c: Rgba, anchor: Anchor, max_w: Option<f32>) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let px = self.font_px(size, face);
        let s = self.s;
        if self.rec.is_some() {
            let (text, left, base, width) = with(|g| {
                let text = fit_text(g, text, face, px, max_w.map(|m| m * s));
                let (_, width, baseline, ink_top, ink_bottom) = g.measure(&text, face, px);
                let (left, base) = text_origin(x * s, y * s, anchor, width, baseline, ink_top, ink_bottom);
                (text, left / s, base / s, width / s)
            });
            self.recording(|| Op::Text { left, base, text, face, size, c });
            return width;
        }
        with(|g| {
            let text = fit_text(g, text, face, px, max_w.map(|m| m * s));
            let (layout, width, baseline, ink_top, ink_bottom) = g.measure(&text, face, px);
            let (left, base) = text_origin(x * s, y * s, anchor, width, baseline, ink_top, ink_bottom);
            // (a linha de base arredondada para o pixel: texto nítido)
            let top = base.round() - baseline;
            unsafe {
                self.rt.DrawTextLayout(v2(left.round(), top), &layout, &self.brush(c), D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
            width / s
        })
    }

    /// Um texto já posicionado: o canto esquerdo e a linha de base (de uma gravação).
    fn text_at(&mut self, left: f32, base: f32, text: &str, face: Face, size: f32, c: Rgba) {
        if text.is_empty() || size <= 0.5 {
            return;
        }
        let px = self.font_px(size, face);
        let s = self.s;
        with(|g| {
            let (layout, _, baseline, _, _) = g.measure(text, face, px);
            let top = (base * s).round() - baseline;
            unsafe {
                self.rt.DrawTextLayout(v2((left * s).round(), top), &layout, &self.brush(c), D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
        })
    }

    /// Uma tela que só grava: os desenhos viram uma lista (com o recorte e a opacidade de cada
    /// um), sem pintar nada. As medidas de texto funcionam como numa tela comum.
    pub fn recorder(s: f32, text_scale: f32) -> Canvas {
        let mut c = Canvas::new(1, 1, s);
        c.text_scale = text_scale;
        c.rec = Some(Recorder::default());
        c
    }

    /// O que foi gravado.
    pub fn take_recording(mut self) -> Vec<Rec> {
        let ops = self.rec.take().map(|r| r.ops).unwrap_or_default();
        let _ = self.finish();
        ops
    }

    /// Grava `op` se esta tela está gravando (e aí não desenha nada).
    fn recording(&mut self, op: impl FnOnce() -> Op) -> bool {
        match self.rec.as_mut() {
            Some(r) => {
                r.push(op());
                true
            }
            None => false,
        }
    }

    /// Desenha uma operação gravada.
    pub fn play(&mut self, op: &Op) {
        match op {
            &Op::RRect { x0, y0, x1, y1, r, fill } => self.fill_round_rect_with(x0, y0, x1, y1, r, fill),
            &Op::StrokeRRect { x0, y0, x1, y1, r, c, width } => self.stroke_round_rect(x0, y0, x1, y1, r, c, width),
            Op::GradV { x0, y0, x1, y1, r, stops, g0, g1 } => self.gradient_round_rect(*x0, *y0, *x1, *y1, *r, stops, *g0, *g1),
            Op::GradH { x0, y0, x1, y1, r, stops, g0, g1 } => self.gradient_h(*x0, *y0, *x1, *y1, *r, stops, *g0, *g1),
            &Op::StrokeV { x0, y0, x1, y1, r, top, bottom, width } => self.gradient_stroke_round_rect(x0, y0, x1, y1, r, top, bottom, width),
            &Op::Glow { x0, y0, x1, y1, r, cx, cy, radius, c, width, veil } => self.glow_round_rect(x0, y0, x1, y1, r, cx, cy, radius, c, width, veil),
            &Op::Circle { cx, cy, r, fill } => self.fill_circle_with(cx, cy, r, fill),
            &Op::StrokeCircle { cx, cy, r, c, width } => self.stroke_circle(cx, cy, r, c, width),
            &Op::Crescent { cx, cy, r, cx2, cy2, r2, fill } => self.crescent(cx, cy, r, cx2, cy2, r2, fill),
            Op::Radial { cx, cy, rx, ry, stops } => self.fill_radial(*cx, *cy, *rx, *ry, stops),
            Op::Path { pts, fill, width, closed } => match width {
                Some(w) => self.line_with(pts, *fill, *w, *closed),
                None => self.polygon_with(pts, *fill),
            },
            &Op::Arc { cx, cy, r, start, end, c, width } => self.arc(cx, cy, r, start, end, c, width),
            Op::Segments { segs, c, width } => self.segments(segs, *c, *width),
            &Op::HLine { x0, x1, y, c } => self.hline_px(x0, x1, y, c),
            &Op::VLine { x, y0, y1, c } => self.vline_px(x, y0, y1, c),
            Op::Image { img, x, y, w, h, opacity, sharp } => {
                if *sharp {
                    self.image_sharp(img, *x, *y, *w, *h, *opacity);
                } else {
                    let s = self.s;
                    self.image_px(img, x * s, y * s, w * s, h * s, *opacity);
                }
            }
            Op::Text { left, base, text, face, size, c } => self.text_at(*left, *base, text, *face, *size, *c),
        }
    }

    /// Termina o desenho e devolve os pixels.
    pub fn finish(mut self) -> Image {
        while self.layers > 0 {
            self.pop_clip();
        }
        unsafe {
            let _ = self.rt.EndDraw(None, None);
            let mut img = Image::new(self.w, self.h);
            let rect = WICRect { X: 0, Y: 0, Width: self.w as i32, Height: self.h as i32 };
            if let Some(bmp) = &self.bmp {
                let _ = bmp.CopyPixels(&rect, self.w * 4, &mut img.px);
            }
            img
        }
    }
}

/// Abre uma imagem do disco (JPG, PNG, BMP...) como BGRA pré-multiplicado.
pub fn load_image(path: &std::path::Path) -> Option<Image> {
    let wpath = wide(&path.to_string_lossy());
    with(|g| unsafe {
        let dec = g.wic.CreateDecoderFromFilename(PCWSTR(wpath.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let conv = g.wic.CreateFormatConverter().ok()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let (mut w, mut h) = (0, 0);
        conv.GetSize(&mut w, &mut h).ok()?;
        let mut img = Image::new(w, h);
        conv.CopyPixels(std::ptr::null(), w * 4, &mut img.px).ok()?;
        Some(img)
    })
}

/// Redimensiona uma imagem com boa qualidade (WIC "Fant").
pub fn resize(img: &Image, w: u32, h: u32) -> Image {
    scale_with(img, w, h, WICBitmapInterpolationModeFant)
}

/// Redimensiona para mostrar uma foto (capas): "Fant" para reduzir e cúbica de alta qualidade
/// para ampliar, que não deixa os pixels quadrados aparecerem.
pub fn resize_photo(img: &Image, w: u32, h: u32) -> Image {
    let mode = if w > img.w || h > img.h { WICBitmapInterpolationModeHighQualityCubic } else { WICBitmapInterpolationModeFant };
    scale_with(img, w, h, mode)
}

fn scale_with(img: &Image, w: u32, h: u32, mode: WICBitmapInterpolationMode) -> Image {
    let (w, h) = (w.max(1), h.max(1));
    with(|g| unsafe {
        let src = g
            .wic
            .CreateBitmapFromMemory(img.w, img.h, &GUID_WICPixelFormat32bppPBGRA, img.w * 4, &img.px)
            .expect("bitmap");
        let scaler = g.wic.CreateBitmapScaler().expect("scaler");
        scaler.Initialize(&src, w, h, mode).expect("scale");
        let mut out = Image::new(w, h);
        let _ = scaler.CopyPixels(std::ptr::null(), w * 4, &mut out.px);
        out
    })
}

#[allow(dead_code)]
pub fn as_unknown<T: Interface>(x: &T) -> windows::core::IUnknown {
    x.cast().unwrap()
}

/// Salva a imagem como PNG (para testes e para as imagens do README).
pub fn save_png(img: &Image, path: &std::path::Path) -> windows::core::Result<()> {
    let wpath = wide(&path.to_string_lossy());
    with(|g| unsafe {
        let src = g.wic.CreateBitmapFromMemory(img.w, img.h, &GUID_WICPixelFormat32bppPBGRA, img.w * 4, &img.px)?;
        let conv = g.wic.CreateFormatConverter()?;
        conv.Initialize(&src, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let stream = g.wic.CreateStream()?;
        stream.InitializeFromFilename(PCWSTR(wpath.as_ptr()), windows::Win32::Foundation::GENERIC_WRITE.0)?;
        let enc = g.wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        enc.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        enc.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.unwrap();
        frame.Initialize(None)?;
        frame.SetSize(img.w, img.h)?;
        let mut fmt = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut fmt)?;
        frame.WriteSource(&conv, std::ptr::null())?;
        frame.Commit()?;
        enc.Commit()
    })
}

/// Decodifica uma imagem que está na memória (ex.: a capa do disco vinda do Windows).
pub fn decode_image(bytes: &[u8]) -> Option<Image> {
    with(|g| unsafe {
        let stream = g.wic.CreateStream().ok()?;
        stream.InitializeFromMemory(bytes).ok()?;
        let dec = g.wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let conv = g.wic.CreateFormatConverter().ok()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let (mut w, mut h) = (0, 0);
        conv.GetSize(&mut w, &mut h).ok()?;
        let mut img = Image::new(w, h);
        conv.CopyPixels(std::ptr::null(), w * 4, &mut img.px).ok()?;
        Some(img)
    })
}

/// Recorta o meio da imagem num quadrado e redimensiona para `px` (como o ImageOps.fit).
pub fn fit_square(img: &Image, px: u32) -> Image {
    fit_cover(img, px, px)
}

/// Pedaço w×h da imagem a partir de (x, y) (que precisa estar dentro dela).
pub fn crop(img: &Image, x: u32, y: u32, w: u32, h: u32) -> Image {
    let (w, h) = (w.min(img.w - x).max(1), h.min(img.h - y).max(1));
    let mut out = Image::new(w, h);
    for row in 0..h {
        let s = (((row + y) * img.w + x) * 4) as usize;
        let d = (row * w * 4) as usize;
        out.px[d..d + w as usize * 4].copy_from_slice(&img.px[s..s + w as usize * 4]);
    }
    out
}

/// O meio da imagem na proporção w:h (o maior pedaço que cabe), já em w×h pixels: a imagem
/// cobre o retângulo inteiro sem deformar, cortando o mesmo tanto dos dois lados.
pub fn fit_cover(img: &Image, w: u32, h: u32) -> Image {
    let (w, h) = (w.max(1), h.max(1));
    let (sw, sh) = (img.w.max(1) as u64, img.h.max(1) as u64);
    // largura e altura do recorte: a imagem inteira num sentido, a proporção pedida no outro
    let (cw, ch) = if sw * h as u64 > sh * w as u64 {
        (((sh * w as u64 + h as u64 / 2) / h as u64).max(1) as u32, sh as u32)
    } else {
        (sw as u32, ((sw * h as u64 + w as u64 / 2) / w as u64).max(1) as u32)
    };
    let part = crop(img, (img.w - cw.min(img.w)) / 2, (img.h - ch.min(img.h)) / 2, cw, ch);
    resize_photo(&part, w, h)
}

/// Tira as faixas pretas em volta da imagem (vídeos 16:9 dentro de uma miniatura 4:3, por
/// exemplo). Uma faixa é uma linha (ou coluna) quase toda escura; tira no máximo 1/4 de cada lado.
///
/// As faixas de verdade ficam centralizadas e acabam antes de 1/4: tira o mesmo tanto dos dois
/// lados (o menor dos dois, porque uma cena escura pode encostar numa das faixas), e nada se o
/// escuro passa de 1/4 dos dois lados (é a própria imagem, como a capa preta do "AM"). Imagem
/// quadrada não é miniatura de vídeo (é capa de música): fica como está.
pub fn trim_bars(img: &Image) -> Image {
    if img.w.abs_diff(img.h) * 50 <= img.w.max(img.h) {
        return img.clone();
    }
    let dark = |x: u32, y: u32| {
        let i = ((y * img.w + x) * 4) as usize;
        img.px[i].max(img.px[i + 1]).max(img.px[i + 2]) < 24
    };
    let row_dark = |y: u32| (0..img.w).filter(|&x| dark(x, y)).count() as u32 * 20 >= img.w * 19;
    let col_dark = |x: u32, y0: u32, y1: u32| (y0..y1).filter(|&y| dark(x, y)).count() as u32 * 20 >= (y1 - y0) * 19;
    // quanto tirar dos dois lados, a partir das faixas escuras de cada um
    let both = |a: u32, b: u32, limit: u32| if a.min(b) >= limit { 0 } else { a.min(b) };
    let (vlimit, hlimit) = (img.h / 4, img.w / 4);
    let (mut top, mut bottom) = (0, 0);
    while top < vlimit && row_dark(top) {
        top += 1;
    }
    while bottom < vlimit && row_dark(img.h - 1 - bottom) {
        bottom += 1;
    }
    let bar = both(top, bottom, vlimit);
    let (y0, y1) = (bar, img.h - bar);
    let (mut left, mut right) = (0, 0);
    while left < hlimit && col_dark(left, y0, y1) {
        left += 1;
    }
    while right < hlimit && col_dark(img.w - 1 - right, y0, y1) {
        right += 1;
    }
    let side = both(left, right, hlimit);
    if (bar, side) == (0, 0) {
        return img.clone();
    }
    crop(img, side, bar, img.w - 2 * side, img.h - 2 * bar)
}

#[cfg(test)]
mod digit_tests {
    use super::*;

    /// Algarismos de largura igual: "11:11" e "00:00" ocupam o mesmo espaço.
    #[test]
    fn tabular_digits() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let c = Canvas::new(1, 1, 1.0);
        for face in [Face::Light, Face::Regular, Face::Semibold] {
            let (a, b) = (c.text_width("11:11", face, 40.0), c.text_width("00:00", face, 40.0));
            assert!((a - b).abs() < 0.5, "{face:?}: 11:11 = {a}, 00:00 = {b}");
        }
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::*;

    #[test]
    fn wrap_keeps_the_line_limit() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let c = Canvas::new(1, 1, 1.0);
        let text = "O ponto de orvalho está em dezesseis graus agora";
        assert_eq!(c.wrap(text, Face::Regular, 11.0, 60.0, 1), vec![text.to_string()]);
        assert_eq!(c.wrap(text, Face::Regular, 11.0, 60.0, 2).len(), 2);
        assert!(c.wrap(text, Face::Regular, 11.0, 60.0, 9).len() > 2);
        assert!(c.wrap("", Face::Regular, 11.0, 60.0, 1).is_empty());
    }
}

#[cfg(test)]
mod picture_tests {
    use super::*;

    fn com() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
    }

    /// Imagem w×h de uma cor, com um quadrado de outra cor no meio.
    fn target(w: u32, h: u32, mark: u32) -> Image {
        let mut img = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let inside = x.abs_diff(w / 2) < mark && y.abs_diff(h / 2) < mark;
                img.px[i..i + 4].copy_from_slice(if inside { &[0, 0, 255, 255] } else { &[255, 160, 60, 255] });
            }
        }
        img
    }

    fn red_box(img: &Image) -> (u32, u32, u32, u32) {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..img.h {
            for x in 0..img.w {
                let i = ((y * img.w + x) * 4) as usize;
                if img.px[i + 2] > 200 && img.px[i] < 60 {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                }
            }
        }
        (x0, y0, x1, y1)
    }

    #[test]
    fn cover_keeps_the_middle_centered() {
        com();
        // 16:9 para uma área quadrada e para uma larga: o meio continua no meio
        for (w, h) in [(158, 158), (360, 250), (250, 360)] {
            let out = fit_cover(&target(1280, 720, 60), w, h);
            assert_eq!((out.w, out.h), (w, h));
            let (x0, y0, x1, y1) = red_box(&out);
            assert!((x0 + x1).abs_diff(w - 1) <= 2 && (y0 + y1).abs_diff(h - 1) <= 2, "{w}x{h}: {:?}", (x0, y0, x1, y1));
        }
    }

    #[test]
    fn letterbox_bars_are_trimmed() {
        // um vídeo 16:9 (480 × 270) no meio de uma miniatura 4:3 (480 × 360) com faixas pretas
        let mut img = Image::new(480, 360);
        img.over(&target(480, 270, 20), 0, 45);
        for p in img.px.chunks_mut(4) {
            p[3] = 255;
        }
        let t = trim_bars(&img);
        assert_eq!((t.w, t.h), (480, 270));
        // sem faixas: fica igual
        let plain = target(300, 300, 30);
        assert_eq!((trim_bars(&plain).w, trim_bars(&plain).h), (300, 300));
        // cena escura encostada na faixa de baixo: tira só o tanto das faixas, dos dois lados
        let mut night = img.clone();
        for p in night.px[(360 - 70) * 480 * 4..(360 - 45) * 480 * 4].chunks_mut(4) {
            p[..3].copy_from_slice(&[5, 5, 5]);
        }
        let t = trim_bars(&night);
        assert_eq!((t.w, t.h), (480, 270));
    }

    /// Uma faixa clara no meio de uma imagem preta (como a capa do "AM", do Arctic Monkeys).
    fn dark_with_band(w: u32, h: u32, band: u32) -> Image {
        let mut img = Image::new(w, h);
        img.over(&target(w, band, 10), 0, ((h - band) / 2) as i32);
        for p in img.px.chunks_mut(4) {
            p[3] = 255;
        }
        img
    }

    #[test]
    fn dark_covers_are_not_cropped() {
        // capa quadrada de música, preta em cima e embaixo: inteira (antes virava uma tira)
        let t = trim_bars(&dark_with_band(300, 300, 100));
        assert_eq!((t.w, t.h), (300, 300));
        // não quadrada, mas o escuro passa de 1/4 dos dois lados: é a imagem, não faixa
        let t = trim_bars(&dark_with_band(400, 300, 100));
        assert_eq!((t.w, t.h), (400, 300));
    }
}

#[cfg(test)]
mod embedded_font_tests {
    use super::*;

    #[test]
    fn outfit_is_embedded() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        with(|g| {
            let collection = g.embedded.as_ref().expect("a Outfit carregou");
            for family in ["Outfit"] {
                let name = wide(family);
                let (mut index, mut exists) = (0u32, windows::core::BOOL(0));
                unsafe { collection.FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists).unwrap() };
                assert!(exists.as_bool(), "{family}");
            }
        });
        // e o texto sai mesmo nela, e nos pesos pedidos: a largura muda em relação à Segoe UI e
        // de um peso para outro
        let c = Canvas::new(10, 10, 1.0);
        let w = |face| c.text_width("Uso diário 46%", face, 20.0);
        assert_ne!(w(Face::Custom("Outfit", 400)), w(Face::Regular));
        assert_ne!(w(Face::Custom("Outfit", 300)), w(Face::Custom("Outfit", 600)));
    }
}

#[cfg(test)]
mod glow_tests {
    use super::*;

    fn com() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
    }

    /// Os cantos contínuos: começam a curvar antes do redondo comum (1,6 vezes o raio), passam
    /// pelo mesmo ponto na diagonal e, sem espaço, viram o redondo comum.
    #[test]
    fn corners_are_continuous_like_the_mac() {
        let k = corner(40.0, 100.0);
        assert!((k.p - 64.0).abs() < 1e-3, "{k:?}");
        assert!(k.a > 0.0 && k.b > 0.0 && k.c > 0.0 && k.d > 0.0, "{k:?}");
        let k = corner(10.0, 10.0);
        assert!((k.p - 10.0).abs() < 1e-4 && k.c.abs() < 1e-4 && k.d.abs() < 1e-4, "{k:?}");
        com();
        let mut c = Canvas::new(200, 200, 1.0);
        c.fill_round_rect(0.0, 0.0, 200.0, 200.0, 40.0, Rgba(255, 255, 255, 255));
        let img = c.finish();
        let alpha = |x: u32, y: u32| img.px[((y * img.w + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(100, 100), 255);
        // a 38 px da quina, o redondo comum já seria reto (quase opaco); o contínuo ainda curva
        assert!(alpha(38, 0) < 200, "{}", alpha(38, 0));
        // depois de 1,6 raio, reto
        assert_eq!(alpha(70, 0), 255);
        // no meio do canto, o mesmo ponto do redondo comum (40 - 40/√2 ≈ 11,7 px da quina)
        assert_eq!(alpha(9, 9), 0);
        assert_eq!(alpha(15, 15), 255);
        // raios pequenos: o redondo comum (barras de 2 px de raio)
        let mut c = Canvas::new(40, 10, 1.0);
        c.fill_round_rect(0.0, 0.0, 40.0, 4.0, 2.0, Rgba(255, 255, 255, 255));
        assert_eq!(c.finish().px[((1 * 40 + 20) * 4 + 3) as usize], 255);
    }

    /// Todos os dígitos têm a mesma largura (números "tabulares"), na Outfit e na fonte do
    /// Windows: um número mudando (o relógio, a porcentagem, o tempo restante) não faz o resto
    /// do texto tremer.
    #[test]
    fn digits_have_the_same_width() {
        com();
        let c = Canvas::new(10, 10, 1.0);
        for face in [Face::Custom("Outfit", 300), Face::Custom("Outfit", 600), Face::Regular, Face::Light] {
            let widths: Vec<f32> = (0..10).map(|d| c.text_width(&d.to_string(), face, 30.0)).collect();
            assert!(widths.iter().all(|w| (w - widths[0]).abs() < 0.01), "{face:?}: {widths:?}");
            assert!((c.text_width("11:11", face, 30.0) - c.text_width("00:00", face, 30.0)).abs() < 0.01, "{face:?}");
        }
    }

    /// A escala em volta de um ponto (o cartão afundando ao apertar): o centro fica, as bordas
    /// entram.
    #[test]
    fn scale_about_a_point() {
        com();
        let mut c = Canvas::new(100, 100, 1.0);
        c.scale_about(50.0, 50.0, 0.9);
        c.fill_round_rect(0.0, 0.0, 100.0, 100.0, 0.0, Rgba(255, 255, 255, 255));
        c.reset_transform();
        c.fill_round_rect(0.0, 98.0, 2.0, 100.0, 0.0, Rgba(255, 255, 255, 255));
        let img = c.finish();
        let alpha = |x: u32, y: u32| img.px[((y * img.w + x) * 4 + 3) as usize];
        assert_eq!(alpha(50, 50), 255);
        assert_eq!(alpha(2, 50), 0, "a borda entrou 5 px");
        assert_eq!(alpha(7, 50), 255);
        // depois de reset_transform, de volta ao normal
        assert_eq!(alpha(0, 99), 255);
    }

    #[test]
    fn the_glow_is_brighter_near_the_mouse_and_fades_away() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let mut c = Canvas::new(400, 200, 1.0);
        c.glow_round_rect(0.0, 0.0, 400.0, 200.0, 26.0, 60.0, 0.0, 150.0, Rgba(255, 255, 255, 150), 1.2, 0.0);
        let img = c.finish();
        let alpha = |x: u32, y: u32| img.px[((y * img.w + x) * 4 + 3) as usize];
        // forte em cima, perto do mouse; nada do outro lado, longe dele
        assert!(alpha(60, 0) > 60, "{}", alpha(60, 0));
        assert!(alpha(250, 0) < alpha(120, 0));
        assert_eq!(alpha(399, 199), 0);
        // e o meio do cartão fica limpo (o traço é só na borda)
        assert_eq!(alpha(200, 100), 0);
    }

    /// Prévia da luz do mouse na borda, em três posições (%TEMP%\brilho_borda.png):
    ///   cargo test --release glow_sheet -- --ignored --nocapture
    #[test]
    #[ignore]
    fn glow_sheet() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let s = 2.0;
        let mut c = Canvas::new((1060.0 * s) as u32, (200.0 * s) as u32, s);
        c.fill_round_rect(0.0, 0.0, 1060.0, 200.0, 0.0, Rgba::rgb(30, 28, 50));
        for (i, (mx, my)) in [(40.0, 10.0), (300.0, 80.0), (170.0, 150.0)].into_iter().enumerate() {
            let x0 = 20.0 + i as f32 * 345.0;
            c.fill_round_rect(x0, 20.0, x0 + 320.0, 174.0, 26.0, Rgba(58, 62, 92, 220));
            c.stroke_round_rect(x0, 20.0, x0 + 320.0, 174.0, 26.0, Rgba(255, 255, 255, 40), 1.0);
            c.glow_round_rect(x0, 20.0, x0 + 320.0, 174.0, 26.0, x0 + mx, 20.0 + my, 150.0, Rgba(255, 255, 255, 150), 1.2, 0.07);
            c.fill_circle(x0 + mx, 20.0 + my, 2.0, Rgba::rgb(255, 200, 0));
        }
        let path = std::env::temp_dir().join("brilho_borda.png");
        save_png(&c.finish(), &path).unwrap();
        println!("{}", path.display());
    }
}
