//! Menu de vidro (com submenus e teclado) e as pílulas de vidro escuro: avisos rápidos com
//! "Desfazer" e a barra do modo edição.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;

use crate::events::{self, Cursor};
use crate::gfx::{Anchor, Canvas, Face, Image};
use crate::imgops::blur;
use crate::theme::*;
use crate::widgets::MenuItem;
use crate::win::{self, Z};

// ---------- menu ----------

const ITEM_H: f32 = 30.0;
const SEP_H: f32 = 9.0;
const PAD: f32 = 6.0;
const MIN_W: f32 = 210.0;
const MENU_BG: Rgba = Rgba(36, 36, 42, 244);
const WATCH: Duration = Duration::from_millis(40);

struct Level {
    hwnd: HWND,
    items: Vec<MenuItem>,
    /// retângulo de cada item, em pixels da janela
    rects: Vec<(f32, f32, f32, f32)>,
    pos: (i32, i32),
    size: (i32, i32),
    hot: Option<usize>,
}

pub struct GlassMenu {
    levels: Vec<Level>,
    armed: bool,
    s: f32,
    next_watch: Instant,
    /// o widget que abriu o menu (para os itens dele)
    pub context: Option<&'static str>,
}

fn label(item: &MenuItem) -> Option<&str> {
    match item {
        MenuItem::Cmd(l, _) | MenuItem::Hint(l, _, _) | MenuItem::Check(l, _, _) | MenuItem::Radio(l, _, _) | MenuItem::Sub(l, _) => Some(l),
        MenuItem::Sep => None,
    }
}

impl GlassMenu {
    pub fn new() -> GlassMenu {
        GlassMenu { levels: Vec::new(), armed: false, s: 1.0, next_watch: Instant::now(), context: None }
    }

    pub fn is_open(&self) -> bool {
        !self.levels.is_empty()
    }

    pub fn owns(&self, hwnd: isize) -> bool {
        self.levels.iter().any(|l| l.hwnd.0 as isize == hwnd)
    }

    pub fn popup(&mut self, items: Vec<MenuItem>, x: i32, y: i32, context: Option<&'static str>) {
        self.close(0);
        self.context = context;
        self.armed = false; // o botão que abriu o menu ainda pode estar apertado
        self.s = win::scale_at(x, y);
        self.open_level(items, x, y, None);
        self.next_watch = Instant::now() + WATCH;
        if let Some(l) = self.levels.first() {
            events::focus(l.hwnd);
        }
    }

    pub fn close(&mut self, depth: usize) {
        while self.levels.len() > depth {
            let l = self.levels.pop().unwrap();
            events::destroy(l.hwnd);
        }
    }

    fn open_level(&mut self, items: Vec<MenuItem>, x: i32, y: i32, parent: Option<(i32, i32, i32, i32)>) {
        let s = self.s;
        let probe = Canvas::new(1, 1, s);
        let (pad, ih, sh) = (PAD * s, ITEM_H * s, SEP_H * s);
        let (left_col, right_col) = (30.0 * s, 30.0 * s);
        let widest = items
            .iter()
            .filter_map(|e| {
                let l = label(e)?;
                let mut w = probe.text_width(l, Face::Regular, 12.0) * s;
                if let MenuItem::Hint(_, _, hint) = e {
                    w += probe.text_width(hint, Face::Regular, 11.0) * s + 24.0 * s;
                }
                Some(w)
            })
            .fold(0.0f32, f32::max);
        let w = (MIN_W * s).max(left_col + widest + right_col).round();
        let mut rects = Vec::new();
        let mut top = pad;
        for e in &items {
            let h = if matches!(e, MenuItem::Sep) { sh } else { ih };
            rects.push((pad, top, w - pad, top + h));
            top += h;
        }
        let h = (top + pad).round();
        let area = win::area_for((x, y, x + 1, y + 1));
        let (wi, hi) = (w as i32, h as i32);
        let (mut x, mut y) = (x, y);
        if let Some((px0, py0, px1, _)) = parent {
            // submenu ao lado do item, para a direita se couber
            x = px1 - (4.0 * s) as i32;
            y = py0 - pad as i32;
            if x + wi > area.2 {
                x = px0 - wi + (4.0 * s) as i32;
            }
        } else if x + wi > area.2 {
            x -= wi;
        }
        if y + hi > area.3 {
            y = if parent.is_none() { y - hi } else { area.3 - hi };
        }
        let (x, y) = (x.max(area.0), y.max(area.1));
        let hwnd = events::create_popup();
        self.levels.push(Level { hwnd, items, rects, pos: (x, y), size: (wi, hi), hot: None });
        let i = self.levels.len() - 1;
        self.draw(i);
        events::show(hwnd, true);
        win::set_z(hwnd, Z::Topmost);
    }

    fn draw(&self, i: usize) {
        let l = &self.levels[i];
        let s = self.s;
        let (w, h) = (l.size.0 as f32 / s, l.size.1 as f32 / s);
        let mut c = Canvas::new(l.size.0 as u32, l.size.1 as u32, s);
        c.fill_round_rect(0.0, 0.0, w, h, 12.0, MENU_BG);
        c.stroke_round_rect(0.0, 0.0, w, h, 12.0, WHITE.with_a(34), 1.0 / s);
        let (left_col, right_col) = (30.0, 30.0);
        for (n, (item, &(x0, y0, x1, y1))) in l.items.iter().zip(&l.rects).enumerate() {
            let (x0, y0, x1, y1) = (x0 / s, y0 / s, x1 / s, y1 / s);
            let ym = (y0 + y1) / 2.0;
            if matches!(item, MenuItem::Sep) {
                c.hline_px(x0 + 8.0, x1 - 8.0, ym, Rgba(66, 66, 74, 244));
                continue;
            }
            if l.hot == Some(n) {
                c.fill_round_rect(x0, y0, x1, y1, 7.0, WHITE.with_a(30));
            }
            c.text(x0 + left_col - 2.0, ym, label(item).unwrap(), Face::Regular, 12.0, FG, Anchor::LM, None);
            let cx = x0 + left_col / 2.0;
            match item {
                MenuItem::Check(_, true, _) => c.line(&[(cx - 5.0, ym), (cx - 1.5, ym + 3.5), (cx + 5.0, ym - 4.0)], FG, 1.6),
                MenuItem::Radio(_, true, _) => c.fill_circle(cx, ym, 3.2, FG),
                MenuItem::Sub(..) => {
                    let ax = x1 - right_col / 2.0;
                    c.line(&[(ax - 2.0, ym - 4.5), (ax + 2.5, ym), (ax - 2.0, ym + 4.5)], Rgba::rgb(176, 176, 184), 1.5);
                }
                MenuItem::Hint(_, _, hint) => {
                    c.text(x1 - 10.0, ym, hint, Face::Regular, 11.0, MUTED, Anchor::RM, None);
                }
                _ => {}
            }
        }
        win::blit(l.hwnd, &c.finish(), Some(l.pos), 255);
    }

    fn index_at(&self, level: usize, x: i32, y: i32) -> Option<usize> {
        let l = &self.levels[level];
        l.rects.iter().enumerate().find_map(|(n, &(x0, y0, x1, y1))| {
            let inside = (x as f32) >= x0 && (x as f32) < x1 && (y as f32) >= y0 && (y as f32) < y1;
            (inside && !matches!(l.items[n], MenuItem::Sep)).then_some(n)
        })
    }

    fn set_hot(&mut self, level: usize, index: Option<usize>) {
        if self.levels[level].hot == index {
            return;
        }
        self.levels[level].hot = index;
        self.close(level + 1);
        self.draw(level);
        if let Some(n) = index {
            if let MenuItem::Sub(_, sub) = &self.levels[level].items[n] {
                let sub = sub.clone();
                let l = &self.levels[level];
                let (x0, y0, x1, y1) = l.rects[n];
                let parent = (l.pos.0 + x0 as i32, l.pos.1 + y0 as i32, l.pos.0 + x1 as i32, l.pos.1 + y1 as i32);
                self.open_level(sub, 0, 0, Some(parent));
            }
        }
    }

    fn level_of(&self, hwnd: isize) -> Option<usize> {
        self.levels.iter().position(|l| l.hwnd.0 as isize == hwnd)
    }

    pub fn motion(&mut self, hwnd: isize, x: i32, y: i32) {
        let Some(level) = self.level_of(hwnd) else { return };
        let index = self.index_at(level, x, y);
        events::set_cursor(self.levels[level].hwnd, Cursor::Arrow);
        if index.is_some() || self.levels.len() == level + 1 {
            self.set_hot(level, index);
        }
    }

    /// Clique: devolve o número do item escolhido (e fecha o menu).
    pub fn click(&mut self, hwnd: isize, x: i32, y: i32) -> Option<u32> {
        let level = self.level_of(hwnd)?;
        let index = self.index_at(level, x, y)?;
        self.activate(level, index)
    }

    fn activate(&mut self, level: usize, index: usize) -> Option<u32> {
        let id = match &self.levels[level].items[index] {
            MenuItem::Sub(..) => {
                self.set_hot(level, Some(index));
                return None;
            }
            MenuItem::Cmd(_, id) | MenuItem::Hint(_, id, _) | MenuItem::Check(_, _, id) | MenuItem::Radio(_, _, id) => *id,
            MenuItem::Sep => return None,
        };
        self.close(0);
        Some(id)
    }

    /// Teclado: setas, Enter, Esc. Devolve o item escolhido, se houver.
    pub fn key(&mut self, vk: u32) -> Option<u32> {
        let level = self.levels.len().checked_sub(1)?;
        let items: Vec<usize> = (0..self.levels[level].items.len()).filter(|&n| !matches!(self.levels[level].items[n], MenuItem::Sep)).collect();
        match vk {
            0x1B => self.close(0), // Esc
            0x26 | 0x28 => {
                // cima / baixo
                let hot = self.levels[level].hot;
                let pos = items.iter().position(|&n| Some(n) == hot).map(|p| p as i32).unwrap_or(-1);
                let next = (pos + if vk == 0x28 { 1 } else { -1 }).rem_euclid(items.len() as i32) as usize;
                self.levels[level].hot = Some(items[next]);
                self.draw(level);
            }
            0x0D | 0x27 => {
                // Enter / direita
                let hot = self.levels[level].hot?;
                if let MenuItem::Sub(..) = self.levels[level].items[hot] {
                    self.levels[level].hot = None;
                    self.set_hot(level, Some(hot));
                    let sub = self.levels.len() - 1;
                    self.levels[sub].hot = (0..self.levels[sub].items.len()).find(|&n| !matches!(self.levels[sub].items[n], MenuItem::Sep));
                    self.draw(sub);
                } else if vk == 0x0D {
                    return self.activate(level, hot);
                }
            }
            0x25 if self.levels.len() > 1 => self.close(self.levels.len() - 1), // esquerda
            _ => {}
        }
        None
    }

    pub fn next_due(&self) -> Option<Instant> {
        self.is_open().then_some(self.next_watch)
    }

    /// Fecha o menu ao clicar fora dele.
    pub fn watch(&mut self) {
        if !self.is_open() || Instant::now() < self.next_watch {
            return;
        }
        self.next_watch = Instant::now() + WATCH;
        let pressed = [0x01, 0x02, 0x04].iter().any(|&vk| win::key_down(vk));
        if !pressed {
            self.armed = true;
        } else if self.armed {
            let (px, py) = win::pointer();
            let inside = self.levels.iter().any(|l| px >= l.pos.0 && px < l.pos.0 + l.size.0 && py >= l.pos.1 && py < l.pos.1 + l.size.1);
            if !inside {
                self.close(0);
            }
        }
    }
}

// ---------- pílulas: avisos e barra de edição ----------

pub enum Part {
    Icon(&'static str, Rgba),
    Text(String, bool),
    Muted(String),
    /// botão: rótulo, número da ação, principal (azul)
    Button(String, u32, bool),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Place {
    Bottom,
    Top,
}

pub struct GlassBar {
    hwnd: Option<HWND>,
    parts: Vec<Part>,
    buttons: Vec<((i32, i32, i32, i32), u32)>,
    hot: Option<usize>,
    pub pos: (i32, i32),
    pub size: (i32, i32),
    pub visible: bool,
    place: Place,
    alpha: f32,
    fade: Option<(f32, f32, Instant)>,
    img: Option<Image>,
    s: f32,
}

const BAR_H: f32 = 46.0;
const BAR_PAD: f32 = 16.0;
const BAR_GAP: f32 = 10.0;
const SHADOW: f32 = 14.0;
const FADE_MS: u64 = 160;

impl GlassBar {
    pub fn new(place: Place) -> GlassBar {
        GlassBar {
            hwnd: None,
            parts: Vec::new(),
            buttons: Vec::new(),
            hot: None,
            pos: (0, 0),
            size: (0, 0),
            visible: false,
            place,
            alpha: 0.0,
            fade: None,
            img: None,
            s: 1.0,
        }
    }

    pub fn hwnd(&self) -> Option<isize> {
        self.hwnd.map(|h| h.0 as isize)
    }

    pub fn show(&mut self, parts: Vec<Part>, animations: bool) {
        self.parts = parts;
        self.hot = None;
        if self.hwnd.is_none() {
            let h = events::create_popup();
            events::show(h, true);
            self.hwnd = Some(h);
        }
        self.render();
        win::set_z(self.hwnd.unwrap(), Z::Topmost);
        if !self.visible {
            self.visible = true;
            self.fade_to(1.0, animations);
        } else {
            self.present(0);
        }
    }

    pub fn hide(&mut self, animations: bool) {
        if self.visible {
            self.visible = false;
            self.fade_to(0.0, animations);
        }
    }

    fn fade_to(&mut self, target: f32, animations: bool) {
        if animations {
            self.fade = Some((self.alpha, target, Instant::now()));
        } else {
            self.alpha = target;
            self.fade = None;
            self.present(0);
            if target == 0.0 {
                self.blank();
            }
        }
    }

    fn blank(&self) {
        if let Some(h) = self.hwnd {
            win::blit(h, &Image::new(1, 1), Some((-10, -10)), 255);
        }
    }

    pub fn next_frame(&self) -> Option<Instant> {
        self.fade.as_ref().map(|_| Instant::now() + Duration::from_millis(FRAME_MS))
    }

    pub fn frame(&mut self) {
        let Some((from, to, start)) = self.fade else { return };
        let t = (start.elapsed().as_secs_f32() * 1000.0 / FADE_MS as f32).min(1.0);
        self.alpha = from + (to - from) * ease_in_out(t);
        self.present(((1.0 - self.alpha) * 10.0 * self.s).round() as i32);
        if t >= 1.0 {
            self.fade = None;
            if to == 0.0 {
                self.blank();
            }
        }
    }

    fn render(&mut self) {
        let area = win::work_areas()[0];
        let s = win::scale_at(area.0, area.1);
        self.s = s;
        let probe = Canvas::new(1, 1, s);
        // medidas de cada parte (lógicas)
        let mut layout = Vec::new();
        let mut x = BAR_PAD;
        for part in &self.parts {
            let w = match part {
                Part::Icon(..) => 18.0,
                Part::Text(t, bold) => probe.text_width(t, if *bold { Face::Semibold } else { Face::Regular }, 12.5),
                Part::Muted(t) => probe.text_width(t, Face::Regular, 12.5),
                Part::Button(t, ..) => probe.text_width(t, Face::Semibold, 12.5) + 28.0,
            };
            layout.push((x, w));
            x += w + BAR_GAP;
        }
        let w = x - BAR_GAP + BAR_PAD;
        let (W, H) = (w + 2.0 * SHADOW, BAR_H + 2.0 * SHADOW);
        let (pw, ph) = ((W * s).round() as u32, (H * s).round() as u32);
        // sombra suave embaixo da pílula
        let mut shadow = Canvas::new(pw, ph, s);
        shadow.fill_round_rect(SHADOW, SHADOW + 4.0, SHADOW + w, SHADOW + BAR_H + 4.0, BAR_H / 2.0, Rgba(0, 0, 0, 110));
        let mut shadow_img = shadow.finish();
        blur(&mut shadow_img.px, pw, ph, 4, 8.0 * s);
        let mut c = Canvas::new(pw, ph, s);
        c.image(&shadow_img, 0.0, 0.0);
        c.fill_round_rect(SHADOW, SHADOW, SHADOW + w, SHADOW + BAR_H, BAR_H / 2.0, Rgba(34, 34, 40, 246));
        c.stroke_round_rect(SHADOW, SHADOW, SHADOW + w, SHADOW + BAR_H, BAR_H / 2.0, WHITE.with_a(40), 1.0 / s);
        self.buttons.clear();
        let cy = SHADOW + BAR_H / 2.0;
        let bh = 30.0;
        let mut button_index = 0;
        for (part, (px, pw_)) in self.parts.iter().zip(layout) {
            let px = SHADOW + px;
            match part {
                Part::Icon(name, color) => {
                    c.text(px + pw_ / 2.0, cy, icon(name), Face::Icons, 15.0, *color, Anchor::MM, None);
                }
                Part::Text(t, bold) => {
                    c.text(px, cy, t, if *bold { Face::Semibold } else { Face::Regular }, 12.5, FG, Anchor::LM, None);
                }
                Part::Muted(t) => {
                    c.text(px, cy, t, Face::Regular, 12.5, MUTED, Anchor::LM, None);
                }
                Part::Button(t, id, primary) => {
                    let hot = self.hot == Some(button_index);
                    let fill = if *primary {
                        if hot { ACCENT_HOVER } else { ACCENT }
                    } else {
                        WHITE.with_a(if hot { 40 } else { 22 })
                    };
                    let y0 = cy - bh / 2.0;
                    c.fill_round_rect(px, y0, px + pw_, y0 + bh, bh / 2.0, fill);
                    c.text(px + pw_ / 2.0, cy, t, Face::Semibold, 12.5, FG, Anchor::MM, None);
                    let r = ((px * s) as i32, (y0 * s) as i32, ((px + pw_) * s) as i32, ((y0 + bh) * s) as i32);
                    self.buttons.push((r, *id));
                    button_index += 1;
                }
            }
        }
        let img = c.finish();
        let (iw, ih) = (img.w as i32, img.h as i32);
        self.pos = match self.place {
            Place::Bottom => ((area.0 + area.2 - iw) / 2, area.3 - ih - (18.0 * s) as i32),
            Place::Top => ((area.0 + area.2 - iw) / 2, area.1 + (10.0 * s) as i32),
        };
        self.size = (iw, ih);
        self.img = Some(img);
    }

    fn present(&self, offset: i32) {
        if let (Some(h), Some(img)) = (self.hwnd, &self.img) {
            win::blit(h, img, Some((self.pos.0, self.pos.1 + offset)), (255.0 * self.alpha).round() as u8);
        }
    }

    fn button_at(&self, x: i32, y: i32) -> Option<usize> {
        self.buttons.iter().position(|&((x0, y0, x1, y1), _)| x >= x0 && x < x1 && y >= y0 && y < y1)
    }

    pub fn motion(&mut self, x: i32, y: i32) {
        let index = self.button_at(x, y);
        if let Some(h) = self.hwnd {
            events::set_cursor(h, if index.is_some() { Cursor::Hand } else { Cursor::Arrow });
        }
        if index != self.hot && self.visible {
            self.hot = index;
            self.render();
            self.present(0);
        }
    }

    pub fn leave(&mut self) {
        if self.hot.is_some() && self.visible {
            self.hot = None;
            self.render();
            self.present(0);
        }
    }

    /// Clique: o número da ação do botão, se houver.
    pub fn click(&self, x: i32, y: i32) -> Option<u32> {
        if !self.visible {
            return None;
        }
        self.button_at(x, y).map(|i| self.buttons[i].1)
    }

    pub fn pointer_inside(&self) -> bool {
        let (px, py) = win::pointer();
        self.visible && px >= self.pos.0 && px < self.pos.0 + self.size.0 && py >= self.pos.1 && py < self.pos.1 + self.size.1
    }

    pub fn focus(&self) {
        if let Some(h) = self.hwnd {
            events::focus(h);
        }
    }
}
