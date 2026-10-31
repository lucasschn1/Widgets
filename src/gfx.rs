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
    fn parts(self) -> (char, char) {
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
            Ok(Gfx { d2d, dw, wic, formats: RefCell::new(HashMap::new()), round, icon_family })
        }
    }

    fn format(&self, face: Face, px: f32) -> IDWriteTextFormat {
        let key = (face, (px * 100.0).round() as u32);
        if let Some(f) = self.formats.borrow().get(&key) {
            return f.clone();
        }
        let (family, weight): (&str, DWRITE_FONT_WEIGHT) = match face {
            Face::Regular => ("Segoe UI", DWRITE_FONT_WEIGHT_NORMAL),
            Face::Semibold => ("Segoe UI", DWRITE_FONT_WEIGHT_SEMI_BOLD),
            Face::Light => ("Segoe UI", DWRITE_FONT_WEIGHT_LIGHT),
            Face::Symbol => ("Segoe UI Symbol", DWRITE_FONT_WEIGHT_NORMAL),
            Face::Icons => (self.icon_family, DWRITE_FONT_WEIGHT_NORMAL),
        };
        let family = wide(family);
        let format = unsafe {
            self.dw
                .CreateTextFormat(PCWSTR(family.as_ptr()), None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, px, w!("pt-br"))
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
#[derive(Clone, Copy)]
pub enum Fill {
    Solid(Rgba),
    VGrad(f32, f32, Rgba, Rgba),
}

fn color(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0 as f32 / 255.0, g: c.1 as f32 / 255.0, b: c.2 as f32 / 255.0, a: c.3 as f32 / 255.0 }
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

/// Tela de desenho em memória. Coordenadas lógicas; `s` é a escala do monitor.
pub struct Canvas {
    rt: ID2D1RenderTarget,
    bmp: IWICBitmap,
    pub w: u32,
    pub h: u32,
    pub s: f32,
    /// tamanho do texto escolhido pelo usuário (acessibilidade), multiplica as fontes
    pub text_scale: f32,
    layers: u32,
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
            Canvas { rt, bmp, w, h, s, text_scale: 1.0, layers: 0 }
        })
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
        let rr = D2D1_ROUNDED_RECT { rect: self.rect_px(x0, y0, x1, y1), radiusX: r * self.s, radiusY: r * self.s };
        unsafe { self.rt.FillRoundedRectangle(&rr, &self.brush_of(f)) }
    }

    pub fn fill_circle_with(&mut self, cx: f32, cy: f32, r: f32, f: Fill) {
        let s = self.s;
        let e = D2D1_ELLIPSE { point: v2(cx * s, cy * s), radiusX: r * s, radiusY: r * s };
        unsafe { self.rt.FillEllipse(&e, &self.brush_of(f)) }
    }

    /// Lua crescente: o círculo (cx, cy, r) sem o círculo (cx2, cy2, r2).
    pub fn crescent(&mut self, cx: f32, cy: f32, r: f32, cx2: f32, cy2: f32, r2: f32, f: Fill) {
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
        let rr = D2D1_ROUNDED_RECT { rect: self.rect_px(x0, y0, x1, y1), radiusX: r * self.s, radiusY: r * self.s };
        unsafe { self.rt.FillRoundedRectangle(&rr, &self.brush(c)) }
    }

    pub fn stroke_round_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, c: Rgba, width: f32) {
        // o traço fica centrado na borda: recua meia espessura para ficar todo por dentro
        let hw = width / 2.0;
        let rr = D2D1_ROUNDED_RECT {
            rect: self.rect_px(x0 + hw, y0 + hw, x1 - hw, y1 - hw),
            radiusX: (r - hw).max(0.0) * self.s,
            radiusY: (r - hw).max(0.0) * self.s,
        };
        unsafe { self.rt.DrawRoundedRectangle(&rr, &self.brush(c), width * self.s, None) }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let s = self.s;
        let e = D2D1_ELLIPSE { point: v2(cx * s, cy * s), radiusX: r * s, radiusY: r * s };
        unsafe { self.rt.FillEllipse(&e, &self.brush(c)) }
    }

    pub fn stroke_circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba, width: f32) {
        let s = self.s;
        let e = D2D1_ELLIPSE { point: v2(cx * s, cy * s), radiusX: r * s, radiusY: r * s };
        unsafe { self.rt.DrawEllipse(&e, &self.brush(c), width * s, None) }
    }

    /// Linha (ou poligonal) com pontas e junções arredondadas.
    pub fn line(&mut self, pts: &[(f32, f32)], c: Rgba, width: f32) {
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
        let s = self.s;
        unsafe {
            let stops: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|&(p, c)| D2D1_GRADIENT_STOP { position: p, color: color(c) }).collect();
            let coll = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).expect("stops");
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: v2(0.0, gy0 * s), endPoint: v2(0.0, gy1 * s) };
            let brush = self.rt.CreateLinearGradientBrush(&props, None, &coll).expect("gradient");
            let rr = D2D1_ROUNDED_RECT { rect: self.rect_px(x0, y0, x1, y1), radiusX: r * s, radiusY: r * s };
            self.rt.FillRoundedRectangle(&rr, &brush);
        }
    }

    /// Contorno arredondado com degradê vertical (a borda clara do vidro).
    pub fn gradient_stroke_round_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, top: Rgba, bottom: Rgba, width: f32) {
        let s = self.s;
        let hw = width / 2.0;
        unsafe {
            let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: color(top) }, D2D1_GRADIENT_STOP { position: 1.0, color: color(bottom) }];
            let coll = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).expect("stops");
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: v2(0.0, y0 * s), endPoint: v2(0.0, y1 * s) };
            let brush = self.rt.CreateLinearGradientBrush(&props, None, &coll).expect("gradient");
            let rr = D2D1_ROUNDED_RECT {
                rect: self.rect_px(x0 + hw, y0 + hw, x1 - hw, y1 - hw),
                radiusX: (r - hw).max(0.0) * s,
                radiusY: (r - hw).max(0.0) * s,
            };
            self.rt.DrawRoundedRectangle(&rr, &brush, width * s, None);
        }
    }

    /// Linha horizontal fina, alinhada ao pixel (divisórias): espessura de 1 pixel lógico.
    pub fn hline_px(&mut self, x0: f32, x1: f32, y: f32, c: Rgba) {
        let t = self.s.round().max(1.0);
        let py = (y * self.s).round();
        let r = D2D_RECT_F { left: x0 * self.s, top: py, right: x1 * self.s, bottom: py + t };
        unsafe { self.rt.FillRectangle(&r, &self.brush(c)) }
    }

    /// Linha vertical fina, alinhada ao pixel.
    pub fn vline_px(&mut self, x: f32, y0: f32, y1: f32, c: Rgba) {
        let t = self.s.round().max(1.0);
        let px = (x * self.s).round();
        let r = D2D_RECT_F { left: px, top: y0 * self.s, right: px + t, bottom: y1 * self.s };
        unsafe { self.rt.FillRectangle(&r, &self.brush(c)) }
    }

    /// Daqui até `pop_clip`, só aparece o que estiver dentro do retângulo arredondado.
    pub fn push_round_clip(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) {
        let s = self.s;
        with(|g| unsafe {
            let rr = D2D1_ROUNDED_RECT { rect: self.rect_px(x0, y0, x1, y1), radiusX: r * s, radiusY: r * s };
            let geom = g.d2d.CreateRoundedRectangleGeometry(&rr).expect("geom");
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
        });
        self.layers += 1;
    }

    pub fn pop_clip(&mut self) {
        if self.layers > 0 {
            unsafe { self.rt.PopLayer() };
            self.layers -= 1;
        }
    }

    /// Cola uma imagem (pixels) com o canto em (x, y) lógicos, no tamanho dela.
    pub fn image(&mut self, img: &Image, x: f32, y: f32) {
        let (px, py) = ((x * self.s).round(), (y * self.s).round());
        self.image_px(img, px, py, img.w as f32, img.h as f32, 1.0);
    }

    /// Cola uma imagem num retângulo em pixels (esticando, com suavização).
    pub fn image_px(&mut self, img: &Image, x: f32, y: f32, w: f32, h: f32, opacity: f32) {
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

    /// Escreve um texto; (x, y) é o ponto de `anchor`. Corta com "…" se passar de `max_w`.
    /// Devolve a largura escrita (lógica).
    pub fn text(&mut self, x: f32, y: f32, text: &str, face: Face, size: f32, c: Rgba, anchor: Anchor, max_w: Option<f32>) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let px = self.font_px(size, face);
        let s = self.s;
        with(|g| {
            let text = fit_text(g, text, face, px, max_w.map(|m| m * s));
            let (layout, width, baseline, ink_top, ink_bottom) = g.measure(&text, face, px);
            let (h, v) = anchor.parts();
            let mut left = x * s;
            match h {
                'm' => left -= width / 2.0,
                'r' => left -= width,
                _ => {}
            }
            // posição da linha de base, depois arredondada para o pixel (texto nítido)
            let base = match v {
                'a' => y * s + baseline,
                'm' => y * s - (ink_top + ink_bottom) / 2.0,
                _ => y * s,
            };
            let top = base.round() - baseline;
            unsafe {
                self.rt.DrawTextLayout(v2(left.round(), top), &layout, &self.brush(c), D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
            width / s
        })
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
            let _ = self.bmp.CopyPixels(&rect, self.w * 4, &mut img.px);
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
    let (w, h) = (w.max(1), h.max(1));
    with(|g| unsafe {
        let src = g
            .wic
            .CreateBitmapFromMemory(img.w, img.h, &GUID_WICPixelFormat32bppPBGRA, img.w * 4, &img.px)
            .expect("bitmap");
        let scaler = g.wic.CreateBitmapScaler().expect("scaler");
        scaler.Initialize(&src, w, h, WICBitmapInterpolationModeFant).expect("scale");
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
