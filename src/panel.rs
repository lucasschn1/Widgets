//! Janelas de painel (configurações, boas-vindas, perguntas) desenhadas no visual dos widgets.
//!
//! Modo "imediato": a cada quadro a página redesenha tudo chamando os controles do `Ui`
//! (`ui.switch`, `ui.slider`, `ui.button`...), que desenham e, ao mesmo tempo, registram as
//! áreas clicáveis com a ação (`A`) que o app executa no clique. Os campos de texto são
//! controles EDIT de verdade, posicionados por cima do desenho.

use std::collections::HashMap;
use std::time::Instant;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::backdrop::Backdrop;
use crate::events::{self, Cursor, PANEL_CLASS};
use crate::gfx::{wide, Anchor, Canvas, Face, Image};
use crate::theme::*;
use crate::widgets::Mode;
use crate::win;

pub const BASE: Rgba = Rgba::rgb(24, 24, 30); // fundo, misturado ao papel de parede (efeito "Mica")
const BASE_AMOUNT: f32 = 0.84;
pub const FIELD: Rgba = Rgba::rgb(40, 40, 48); // campos de texto (opacos, para o EDIT combinar)
pub const FIELD_HOVER: Rgba = Rgba::rgb(48, 48, 57);
pub const LINE: Rgba = Rgba(255, 255, 255, 26);
const SURFACE: Rgba = Rgba(255, 255, 255, 11);
const SURFACE_HOVER: Rgba = Rgba(255, 255, 255, 20);
const SURFACE_DOWN: Rgba = Rgba(255, 255, 255, 7);

/// O que acontece ao clicar (ou arrastar) uma área de um painel.
#[derive(Clone, Debug, PartialEq)]
pub enum A {
    Go(&'static str),
    Toggle(&'static str),
    Notify(&'static str),
    Startup,
    Lock,
    Slider(&'static str),
    Seg(&'static str, String),
    Size(&'static str, Mode),
    Visible(&'static str, bool),
    Widget(&'static str, u32),
    Edit,
    Arrange,
    SaveLayout,
    ApplyLayout(String),
    DeleteLayout(String),
    Restore(&'static str),
    TestNotify,
    Welcome,
    OpenData,
    ResetAll,
    Pick(usize),
    Focus(String),
    Ok,
    Cancel,
    Tile(&'static str),
    Next,
    Back,
    Finish,
}

#[derive(Clone, Copy)]
pub struct Slider {
    pub x0: f32,
    pub w: f32,
    pub lo: f64,
    pub hi: f64,
    pub step: f64,
}

impl Slider {
    /// Valor do slider para o mouse em x (lógico, na janela).
    pub fn value_at(&self, x: f32) -> f64 {
        let t = ((x - self.x0) / self.w).clamp(0.0, 1.0) as f64;
        let v = self.lo + t * (self.hi - self.lo);
        (v / self.step).round() * self.step
    }
}

pub struct Region {
    pub id: String,
    pub rect: (f32, f32, f32, f32),
    pub action: A,
    pub slider: Option<Slider>,
    pub cursor: Cursor,
}

pub struct EntrySpec {
    pub id: String,
    pub rect: (f32, f32, f32, f32),
    pub value: String,
    pub placeholder: String,
    pub visible: bool,
    pub bg: Rgba,
}

/// Valores animados (com aceleração suave) por nome.
#[derive(Default)]
pub struct Anims {
    map: HashMap<String, (f32, f32, Instant, u64)>,
    pub running: bool,
}

impl Anims {
    pub fn value(&mut self, id: &str, target: f32, ms: u64, animations: bool) -> f32 {
        let now = Instant::now();
        let Some(&(start, end, t0, dur)) = self.map.get(id) else {
            self.map.insert(id.to_string(), (target, target, now, ms));
            return target;
        };
        let t = if dur == 0 { 1.0 } else { (now.duration_since(t0).as_secs_f32() * 1000.0 / dur as f32).min(1.0) };
        let current = start + (end - start) * ease_in_out(t);
        if target != end {
            self.map.insert(id.to_string(), (current, target, now, if animations { ms } else { 0 }));
            self.running = true;
            return current;
        }
        if t < 1.0 {
            self.running = true;
        }
        current
    }
}

// ---------- a janela ----------

pub struct PanelWin {
    pub hwnd: HWND,
    pub s: f32,
    pub w: f32,
    pub h: f32,
    pub frameless: bool,
    edits: HashMap<String, (u16, HWND)>,
    next_ctrl: u16,
    pub regions: Vec<Region>,
    pub hover: Option<String>,
    pub pressed: Option<String>,
    pub dragging: Option<String>,
    pub focus: Option<String>,
    pub anims: Anims,
    bg: Option<((i32, i32, u32, u32, u64), Image)>,
    pub caption: Option<Rgba>,
    pub dirty: bool,
}

impl PanelWin {
    /// Cria a janela (ainda escondida). `pos`: canto da área de desenho na tela; None: centralizada.
    pub fn new(title: &str, w: f32, h: f32, pos: Option<(i32, i32)>, frameless: bool) -> PanelWin {
        // sem posição: no meio do monitor onde está o mouse
        let screen = win::area_at_pointer();
        let anchor = pos.unwrap_or(((screen.0 + screen.2) / 2, (screen.1 + screen.3) / 2));
        let s = win::scale_at(anchor.0, anchor.1);
        let (pw, ph) = ((w * s).round() as i32, (h * s).round() as i32);
        let (style, ex) = if frameless {
            (WS_POPUP, WS_EX_TOOLWINDOW)
        } else {
            (WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, WINDOW_EX_STYLE(0))
        };
        let mut r = RECT { left: 0, top: 0, right: pw, bottom: ph };
        unsafe {
            let _ = AdjustWindowRectExForDpi(&mut r, style, false, ex, (s * 96.0) as u32);
        }
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let (x, y) = match pos {
            Some((x, y)) => (x + r.left, y + r.top),
            None => {
                let a = screen;
                ((a.0 + a.2 - ww) / 2, ((a.1 + a.3 - wh) / 2 - (20.0 * s) as i32).max(a.1))
            }
        };
        let t = wide(title);
        let hwnd = unsafe { CreateWindowExW(ex, PANEL_CLASS, PCWSTR(t.as_ptr()), style, x, y, ww, wh, None, None, None, None).expect("painel") };
        PanelWin {
            hwnd,
            s,
            w,
            h,
            frameless,
            edits: HashMap::new(),
            next_ctrl: 100,
            regions: Vec::new(),
            hover: None,
            pressed: None,
            dragging: None,
            focus: None,
            anims: Anims::default(),
            bg: None,
            caption: None,
            dirty: true,
        }
    }

    pub fn show(&self) {
        unsafe {
            // o ícone do app na barra de título e no Alt+Tab
            let small = crate::tray::hicon(&crate::tray::icon_image(GetSystemMetrics(SM_CXSMICON) as u32));
            let big = crate::tray::hicon(&crate::tray::icon_image(GetSystemMetrics(SM_CXICON) as u32));
            SendMessageW(self.hwnd, WM_SETICON, Some(windows::Win32::Foundation::WPARAM(0)), Some(windows::Win32::Foundation::LPARAM(small.0 as isize)));
            SendMessageW(self.hwnd, WM_SETICON, Some(windows::Win32::Foundation::WPARAM(1)), Some(windows::Win32::Foundation::LPARAM(big.0 as isize)));
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
        win::style_window(self.hwnd, self.caption, self.frameless);
        // por cima de tudo por um instante: o Windows nem sempre deixa trazer para a frente
        win::set_z(self.hwnd, win::Z::Topmost);
        win::set_z(self.hwnd, win::Z::NoTopmost);
        events::focus(self.hwnd);
    }

    pub fn id(&self) -> isize {
        self.hwnd.0 as isize
    }

    pub fn client_origin(&self) -> (i32, i32) {
        let mut p = POINT { x: 0, y: 0 };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut p);
        }
        (p.x, p.y)
    }

    pub fn px(&self) -> (u32, u32) {
        ((self.w * self.s).round() as u32, (self.h * self.s).round() as u32)
    }

    /// Papel de parede desfocado sob a janela, bem escurecido (como o Mica do Windows 11).
    pub fn background(&mut self, backdrop: &Backdrop) -> Image {
        let (w, h) = self.px();
        let (x, y) = self.client_origin();
        let key = (x, y, w, h, backdrop.version);
        if self.bg.as_ref().map(|b| b.0) != Some(key) {
            let mut region = backdrop.region(x, y, w, h, false);
            for p in region.px.chunks_exact_mut(3) {
                for (c, b) in p.iter_mut().zip([BASE.0, BASE.1, BASE.2]) {
                    *c = (*c as f32 + (b as f32 - *c as f32) * BASE_AMOUNT).round() as u8;
                }
            }
            let img = region.to_image();
            let n = (img.w * (6.0 * self.s) as u32).min(img.w * img.h) as usize;
            let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
            for p in img.px.chunks_exact(4).take(n.max(1)) {
                b += p[0] as u64;
                g += p[1] as u64;
                r += p[2] as u64;
            }
            let n = n.max(1) as u64;
            let caption = Rgba::rgb((r / n) as u8, (g / n) as u8, (b / n) as u8);
            if self.caption != Some(caption) {
                self.caption = Some(caption);
                win::style_window(self.hwnd, self.caption, self.frameless);
            }
            self.bg = Some((key, img));
        }
        self.bg.as_ref().unwrap().1.clone()
    }

    pub fn resize(&mut self, w: f32, h: f32) {
        self.w = w;
        self.h = h;
        let (pw, ph) = self.px();
        let mut r = RECT { left: 0, top: 0, right: pw as i32, bottom: ph as i32 };
        unsafe {
            let style = WINDOW_STYLE(GetWindowLongW(self.hwnd, GWL_STYLE) as u32);
            let ex = WINDOW_EX_STYLE(GetWindowLongW(self.hwnd, GWL_EXSTYLE) as u32);
            let _ = AdjustWindowRectExForDpi(&mut r, style, false, ex, (self.s * 96.0) as u32);
            let _ = SetWindowPos(self.hwnd, None, 0, 0, r.right - r.left, r.bottom - r.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        self.dirty = true;
    }

    /// Cria, move, mostra e esconde os campos de texto deste quadro.
    pub fn sync_entries(&mut self, specs: &[EntrySpec], text_scale: f32) {
        let s = self.s;
        for spec in specs {
            let fresh = !self.edits.contains_key(&spec.id);
            if fresh {
                let ctrl = self.next_ctrl;
                self.next_ctrl += 1;
                let px = (12.5 * s * text_scale).round() as i32;
                let edit = events::create_edit(self.hwnd, ctrl, px, &spec.placeholder);
                self.edits.insert(spec.id.clone(), (ctrl, edit));
            }
            let (_, edit) = self.edits[&spec.id];
            let (x0, y0, x1, y1) = spec.rect;
            // centraliza na vertical uma linha de texto de ~1,4 × a fonte
            let line = 12.5 * text_scale * 1.45;
            let top = (y0 + y1) / 2.0 - line / 2.0;
            events::set_edit_color(edit, (spec.bg.0, spec.bg.1, spec.bg.2));
            unsafe {
                let _ = SetWindowPos(
                    edit,
                    None,
                    (x0 * s).round() as i32,
                    (top * s).round() as i32,
                    ((x1 - x0) * s).round() as i32,
                    (line * s).round() as i32,
                    SWP_NOZORDER | SWP_NOACTIVATE | if spec.visible { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW },
                );
            }
            if fresh {
                // o texto entra só agora, com o campo já na largura certa: senão ele fica rolado
                // até o fim e só aparecem as últimas letras
                events::set_edit_text(edit, &spec.value);
                events::caret_home(edit);
            }
        }
        for (id, (_, edit)) in &self.edits {
            if !specs.iter().any(|sp| &sp.id == id) {
                unsafe {
                    let _ = ShowWindow(*edit, SW_HIDE);
                }
            }
        }
    }

    pub fn entry_id(&self, ctrl: u16) -> Option<String> {
        self.edits.iter().find(|(_, (c, _))| *c == ctrl).map(|(id, _)| id.clone())
    }

    pub fn entry_text(&self, id: &str) -> String {
        self.edits.get(id).map(|(_, e)| events::edit_text(*e)).unwrap_or_default()
    }

    pub fn set_entry(&self, id: &str, text: &str) {
        if let Some((_, e)) = self.edits.get(id) {
            events::set_edit_text(*e, text);
        }
    }

    pub fn focus_entry(&self, id: &str, select: bool) {
        if let Some((_, e)) = self.edits.get(id) {
            events::focus(*e);
            if select {
                events::select_all(*e);
            }
        }
    }

    pub fn has_entry(&self, id: &str) -> bool {
        self.edits.contains_key(id)
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<&Region> {
        self.regions.iter().rev().find(|r| x >= r.rect.0 && x < r.rect.2 && y >= r.rect.1 && y < r.rect.3)
    }

    pub fn region(&self, id: &str) -> Option<&Region> {
        self.regions.iter().find(|r| r.id == id)
    }

    pub fn destroy(&self) {
        events::destroy(self.hwnd);
    }
}

// ---------- o kit de controles ----------

pub struct Ui<'a> {
    pub c: &'a mut Canvas,
    pub regions: Vec<Region>,
    pub entries: Vec<EntrySpec>,
    pub hover: Option<&'a str>,
    pub pressed: Option<&'a str>,
    pub focus: Option<&'a str>,
    pub anims: &'a mut Anims,
    pub animations: bool,
    /// deslocamento desta parte do desenho na janela (lógico) e o recorte das áreas clicáveis
    pub ox: f32,
    pub oy: f32,
    pub clip: (f32, f32, f32, f32),
}

impl Ui<'_> {
    pub fn s(&self) -> f32 {
        self.c.s
    }
    pub fn hot(&self, id: &str) -> bool {
        self.hover == Some(id)
    }
    pub fn down(&self, id: &str) -> bool {
        self.pressed == Some(id) && self.hover == Some(id)
    }
    pub fn anim(&mut self, id: &str, target: f32, ms: u64) -> f32 {
        self.anims.value(id, target, ms, self.animations)
    }

    // --- desenho (coordenadas desta parte) ---

    pub fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, fill: Option<Rgba>, outline: Option<Rgba>, width: f32) {
        let (ox, oy) = (self.ox, self.oy);
        if let Some(f) = fill {
            self.c.fill_round_rect(x0 + ox, y0 + oy, x1 + ox, y1 + oy, r, f);
        }
        if let Some(o) = outline {
            self.c.stroke_round_rect(x0 + ox, y0 + oy, x1 + ox, y1 + oy, r, o, width);
        }
    }
    pub fn fill(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, color: Rgba) {
        self.rect(x0, y0, x1, y1, r, Some(color), None, 1.0);
    }
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: Rgba) {
        self.c.fill_circle(cx + self.ox, cy + self.oy, r, color);
    }
    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, color: Rgba, width: f32) {
        self.c.stroke_circle(cx + self.ox, cy + self.oy, r, color, width);
    }
    pub fn line(&mut self, pts: &[(f32, f32)], color: Rgba, width: f32) {
        let pts: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| (x + self.ox, y + self.oy)).collect();
        self.c.line(&pts, color, width);
    }
    pub fn text(&mut self, x: f32, y: f32, text: &str, size: f32, color: Rgba, face: Face, anchor: Anchor, max_w: Option<f32>) -> f32 {
        self.c.text(x + self.ox, y + self.oy, text, face, size, color, anchor, max_w)
    }
    pub fn icon(&mut self, x: f32, y: f32, name: &str, size: f32, color: Rgba) {
        self.c.text(x + self.ox, y + self.oy, icon(name), Face::Icons, size, color, Anchor::MM, None);
    }
    pub fn image(&mut self, img: &Image, x: f32, y: f32) {
        self.c.image(img, x + self.ox, y + self.oy);
    }
    pub fn text_width(&self, text: &str, size: f32, face: Face) -> f32 {
        self.c.text_width(text, face, size)
    }
    pub fn wrap(&self, text: &str, size: f32, max_w: f32, max_lines: usize) -> Vec<String> {
        self.c.wrap(text, Face::Regular, size, max_w, max_lines)
    }
    pub fn card(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.rect(x0, y0, x1, y1, 10.0, Some(SURFACE), Some(WHITE.with_a(16)), 1.0 / self.s());
    }
    pub fn divider(&mut self, x0: f32, x1: f32, y: f32) {
        self.c.hline_px(x0 + self.ox, x1 + self.ox, y + self.oy, WHITE.with_a(18));
    }

    // --- interação ---

    pub fn region(&mut self, id: &str, x0: f32, y0: f32, x1: f32, y1: f32, action: A, cursor: Cursor) -> bool {
        self.region_with(id, (x0, y0, x1, y1), action, None, cursor)
    }

    fn region_with(&mut self, id: &str, r: (f32, f32, f32, f32), action: A, slider: Option<Slider>, cursor: Cursor) -> bool {
        let (cx0, cy0, cx1, cy1) = self.clip;
        let rect = (
            (r.0 + self.ox).max(cx0),
            (r.1 + self.oy).max(cy0),
            (r.2 + self.ox).min(cx1),
            (r.3 + self.oy).min(cy1),
        );
        if rect.2 > rect.0 && rect.3 > rect.1 {
            self.regions.push(Region { id: id.to_string(), rect, action, slider, cursor });
            return true;
        }
        false
    }

    // --- controles ---

    /// Interruptor (40 × 22) com o canto em (x, y); `area` amplia o clique (a linha inteira).
    pub fn switch(&mut self, id: &str, x: f32, y: f32, on: bool, action: A, area: Option<(f32, f32, f32, f32)>) {
        let (w, h) = (40.0, 22.0);
        let t = self.anim(id, if on { 1.0 } else { 0.0 }, 170);
        let hot = self.hot(id);
        let off = WHITE.with_a(if hot { 36 } else { 22 });
        let on_color = if hot { ACCENT_HOVER } else { ACCENT };
        self.fill(x, y, x + w, y + h, h / 2.0, off.mix(on_color, t));
        if t < 1.0 {
            self.rect(x, y, x + w, y + h, h / 2.0, None, Some(WHITE.with_a((110.0 * (1.0 - t)) as u8)), 1.0);
        }
        let r = 7.0 + 1.5 * t + if self.down(id) { 0.8 } else { 0.0 };
        let cx = x + h / 2.0 + (w - h) * t;
        self.circle(cx, y + h / 2.0, r, if t > 0.5 { WHITE } else { Rgba(235, 235, 240, 230) });
        let r = area.unwrap_or((x - 4.0, y - 4.0, x + w + 4.0, y + h + 4.0));
        self.region(id, r.0, r.1, r.2, r.3, action, Cursor::Hand);
    }

    /// Slider com a trilha centrada em y.
    pub fn slider(&mut self, id: &'static str, x: f32, y: f32, w: f32, value: f64, lo: f64, hi: f64, step: f64) {
        let frac = if hi == lo { 0.0 } else { ((value - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 };
        self.fill(x, y - 2.0, x + w, y + 2.0, 2.0, WHITE.with_a(48));
        if frac > 0.0 {
            self.fill(x, y - 2.0, x + w * frac, y + 2.0, 2.0, ACCENT);
        }
        let (hot, down) = (self.hot(id), self.down(id));
        let cx = x + w * frac;
        self.circle(cx, y, 10.0, Rgba::rgb(70, 70, 80));
        self.ring(cx, y, 9.5, WHITE.with_a(40), 1.0);
        self.circle(cx, y, if down { 5.5 } else if hot { 7.0 } else { 6.0 }, if hot { ACCENT_HOVER } else { ACCENT });
        let slider = Slider { x0: x + self.ox, w, lo, hi, step };
        self.region_with(id, (x - 10.0, y - 13.0, x + w + 10.0, y + 13.0), A::Slider(id), Some(slider), Cursor::Hand);
    }

    /// Botões segmentados; `options` = [(valor, rótulo)]. `right`: x é a borda direita. Devolve a largura.
    pub fn segmented(&mut self, id: &'static str, x: f32, y: f32, options: &[(String, String)], current: &str, right: bool) -> f32 {
        let (pad, h, size) = (14.0, 32.0, 12.0);
        let widths: Vec<f32> = options.iter().map(|(_, l)| self.text_width(l, size, Face::Regular) + 2.0 * pad).collect();
        let total: f32 = widths.iter().sum::<f32>() + 4.0;
        let x = if right { x - total } else { x };
        self.rect(x, y, x + total, y + h, 8.0, Some(WHITE.with_a(14)), Some(LINE), 1.0);
        let index = options.iter().position(|(v, _)| v == current).unwrap_or(0);
        let target = 2.0 + widths[..index].iter().sum::<f32>();
        let pos = self.anim(&format!("{id}:pos"), target, 200);
        let width = self.anim(&format!("{id}:w"), widths[index], 200);
        self.fill(x + pos, y + 2.0, x + pos + width, y + h - 2.0, 6.0, WHITE.with_a(46));
        let mut cx = x + 2.0;
        for ((value, label), sw) in options.iter().zip(widths) {
            let sid = format!("{id}:{value}");
            let active = value == current;
            if self.hot(&sid) && !active {
                self.fill(cx, y + 2.0, cx + sw, y + h - 2.0, 6.0, WHITE.with_a(14));
            }
            let face = if active { Face::Semibold } else { Face::Regular };
            self.text(cx + sw / 2.0, y + h / 2.0, label, size, if active { FG } else { MUTED }, face, Anchor::MM, None);
            self.region(&sid, cx, y, cx + sw, y + h, A::Seg(id, value.clone()), Cursor::Hand);
            cx += sw;
        }
        total
    }

    /// Botão: "primary" (azul), "secondary", "danger" ou "ghost". `align`: 'l', 'm' ou 'r'. Devolve a largura.
    pub fn button(&mut self, id: &str, x: f32, y: f32, label: &str, action: A, kind: &str, align: char, w: Option<f32>, icon_name: Option<&str>) -> f32 {
        let (h, size) = (32.0, 12.5);
        let iw = if icon_name.is_some() { 22.0 } else { 0.0 };
        let w = w.unwrap_or_else(|| self.text_width(label, size, Face::Semibold) + 32.0 + iw);
        let x = match align {
            'r' => x - w,
            'm' => x - w / 2.0,
            _ => x,
        };
        let (hot, down) = (self.hot(id), self.down(id));
        let (fill, outline, color) = match kind {
            "primary" => (Some(if hot && !down { ACCENT_HOVER } else { ACCENT }), None, FG),
            "danger" => (Some(DANGER.with_a(if hot { 60 } else { 34 })), Some(DANGER.with_a(120)), Rgba::rgb(255, 140, 130)),
            "ghost" => (hot.then_some(SURFACE_HOVER), None, FG),
            _ => (Some(if down { SURFACE_DOWN } else if hot { SURFACE_HOVER } else { WHITE.with_a(16) }), Some(LINE), FG),
        };
        self.rect(x, y, x + w, y + h, 7.0, fill, outline, 1.0);
        let tx = x + w / 2.0 + iw / 2.0;
        let dy = if down { 1.0 } else { 0.0 };
        if let Some(name) = icon_name {
            let lw = self.text_width(label, size, Face::Semibold);
            self.icon(tx - lw / 2.0 - 12.0, y + h / 2.0 + dy, name, 13.0, color);
        }
        self.text(tx, y + h / 2.0 + dy, label, size, color, Face::Semibold, Anchor::MM, None);
        self.region(id, x, y, x + w, y + h, action, Cursor::Hand);
        w
    }

    /// Campo de texto (um EDIT de verdade por cima do desenho).
    pub fn entry(&mut self, id: &str, x0: f32, y0: f32, x1: f32, y1: f32, value: &str, placeholder: &str, error: bool) {
        let focused = self.focus == Some(id);
        let hot = self.hot(id);
        let bg = if hot && !focused { FIELD_HOVER } else { FIELD };
        let outline = if error { DANGER.with_a(200) } else { WHITE.with_a(if focused { 40 } else { 26 }) };
        self.rect(x0, y0, x1, y1, 7.0, Some(bg), Some(outline), 1.0);
        if focused {
            self.fill(x0 + 1.0, y1 - 2.5, x1 - 1.0, y1 - 0.5, 1.2, ACCENT);
        }
        self.region(id, x0, y0, x1, y1, A::Focus(id.to_string()), Cursor::Arrow);
        let (cx0, cy0, cx1, cy1) = self.clip;
        let rect = (x0 + 11.0 + self.ox, y0 + 4.0 + self.oy, x1 - 11.0 + self.ox, y1 - 5.0 + self.oy);
        let visible = rect.1 >= cy0 && rect.3 <= cy1 && rect.0 >= cx0 && rect.2 <= cx1;
        self.entries.push(EntrySpec { id: id.into(), rect, value: value.into(), placeholder: placeholder.into(), visible, bg });
    }

    /// Teclas desenhadas (Ctrl, Alt, W) a partir de (x, cy); devolve a largura.
    pub fn keycaps(&mut self, x: f32, cy: f32, label: &str) -> f32 {
        let mut kx = x;
        for part in label.split('+') {
            let tw = self.text_width(part, 11.5, Face::Semibold) + 16.0;
            self.rect(kx, cy - 12.0, kx + tw, cy + 12.0, 5.0, Some(WHITE.with_a(22)), Some(WHITE.with_a(40)), 1.0);
            self.text(kx + tw / 2.0, cy, part, 11.5, FG, Face::Semibold, Anchor::MM, None);
            kx += tw + 5.0;
        }
        kx - x
    }
}

/// Lista de sugestões de cidades (busca enquanto se digita), usada nas perguntas e nas configurações.
#[derive(Default)]
pub struct Suggest {
    pub results: Vec<serde_json::Value>,
    pub index: Option<usize>,
    pub busy: bool,
    pub query: String,
    pub due: Option<Instant>,
    seq: u64,
    rx: Option<std::sync::mpsc::Receiver<(u64, Vec<serde_json::Value>)>>,
}

pub const SUGGEST_ROW: f32 = 38.0;

impl Suggest {
    /// O texto mudou: busca depois de uma pausa.
    pub fn typed(&mut self, text: &str) {
        self.query = text.trim().to_string();
        if self.query.chars().count() < 2 {
            self.clear();
            return;
        }
        self.due = Some(Instant::now() + std::time::Duration::from_millis(350));
    }

    pub fn clear(&mut self) {
        self.results.clear();
        self.index = None;
        self.busy = false;
        self.due = None;
        self.seq += 1;
        self.rx = None;
    }

    /// Começa a busca se chegou a hora, e recolhe o resultado; devolve true se mudou algo.
    pub fn poll(&mut self) -> bool {
        if self.due.is_some_and(|t| Instant::now() >= t) {
            self.due = None;
            self.seq += 1;
            let (seq, query) = (self.seq, self.query.clone());
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let found = crate::widgets::tempo::search_cities(&query, 6).unwrap_or_default();
                let _ = tx.send((seq, found));
                events::wake();
            });
            self.rx = Some(rx);
            self.busy = true;
            return true;
        }
        if let Some(rx) = &self.rx {
            if let Ok((seq, found)) = rx.try_recv() {
                self.rx = None;
                if seq == self.seq {
                    self.results = found;
                    self.index = None;
                    self.busy = false;
                    return true;
                }
            }
        }
        false
    }

    pub fn next_due(&self) -> Option<Instant> {
        self.due.or(self.rx.as_ref().map(|_| Instant::now() + std::time::Duration::from_millis(120)))
    }

    /// Setas mudam a escolha; devolve true se usou a tecla.
    pub fn arrow(&mut self, down: bool) -> bool {
        if self.results.is_empty() {
            return false;
        }
        let n = self.results.len() as i64;
        let i = self.index.map(|i| i as i64).unwrap_or(-1) + if down { 1 } else { -1 };
        self.index = Some(i.rem_euclid(n) as usize);
        true
    }

    pub fn height(&self) -> f32 {
        if self.results.is_empty() { 0.0 } else { self.results.len() as f32 * SUGGEST_ROW + 8.0 }
    }

    /// Desenha a lista com o canto em (x, y); devolve a altura.
    pub fn draw(&self, ui: &mut Ui, x: f32, y: f32, w: f32) -> f32 {
        if self.results.is_empty() {
            if self.busy {
                ui.text(x + w - 12.0, y - 22.0, "buscando…", 11.0, MUTED, Face::Regular, Anchor::RM, None);
            }
            return 0.0;
        }
        let h = self.height();
        ui.rect(x, y, x + w, y + h, 8.0, Some(Rgba::rgb(46, 46, 55)), Some(WHITE.with_a(30)), 1.0);
        for (i, place) in self.results.iter().enumerate() {
            let top = y + 4.0 + i as f32 * SUGGEST_ROW;
            let id = format!("cidade:{i}");
            if ui.hot(&id) || self.index == Some(i) {
                ui.fill(x + 4.0, top, x + w - 4.0, top + SUGGEST_ROW, 6.0, Rgba::rgb(62, 62, 72));
            }
            ui.icon(x + 22.0, top + SUGGEST_ROW / 2.0, "location", 13.0, ACCENT);
            ui.text(x + 40.0, top + 7.0, place["name"].as_str().unwrap_or(""), 12.5, FG, Face::Semibold, Anchor::LA, Some(w - 56.0));
            ui.text(x + 40.0, top + 23.0, place["detail"].as_str().unwrap_or(""), 10.5, MUTED, Face::Regular, Anchor::LA, Some(w - 56.0));
            ui.region(&id, x + 4.0, top, x + w - 4.0, top + SUGGEST_ROW, A::Pick(i), Cursor::Hand);
        }
        h
    }
}
