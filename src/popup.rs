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
/// o título (o widget que abriu o menu)
const TITLE_H: f32 = 42.0;
/// a ação em destaque, um botão azul
const PRIMARY_H: f32 = 40.0;
const SEP_H: f32 = 9.0;
const PAD: f32 = 6.0;
const MIN_W: f32 = 224.0;
/// coluna dos ícones (e das marcas), à esquerda do texto
const ICON_COL: f32 = 32.0;
const RADIUS: f32 = 14.0;
/// margem transparente em volta do menu, para a sombra
const MARGIN: f32 = 14.0;
const MENU_BG: Rgba = Rgba(30, 30, 36, 246);
const ICON_COLOR: Rgba = Rgba(255, 255, 255, 200);
const WATCH: Duration = Duration::from_millis(40);

struct Level {
    hwnd: HWND,
    items: Vec<MenuItem>,
    /// retângulo de cada item, em pixels da janela
    rects: Vec<(f32, f32, f32, f32)>,
    /// canto da janela na tela e tamanho dela (com a margem da sombra)
    pos: (i32, i32),
    size: (i32, i32),
    hot: Option<usize>,
    /// a sombra, feita uma vez ao abrir
    shadow: Image,
}

pub struct GlassMenu {
    levels: Vec<Level>,
    armed: bool,
    s: f32,
    next_watch: Instant,
    /// o widget que abriu o menu (para os itens dele)
    pub context: Option<&'static str>,
}

/// O item sem o ícone (o que ele é de verdade).
fn base(item: &MenuItem) -> &MenuItem {
    match item {
        MenuItem::Icon(_, inner) => base(inner),
        other => other,
    }
}

fn icon_of(item: &MenuItem) -> Option<&'static str> {
    match item {
        MenuItem::Icon(name, _) => Some(name),
        MenuItem::Primary(_, _, name) | MenuItem::Title(_, name) => Some(name),
        _ => None,
    }
}

fn label(item: &MenuItem) -> Option<&str> {
    match base(item) {
        MenuItem::Cmd(l, _) | MenuItem::Hint(l, _, _) | MenuItem::Check(l, _, _) | MenuItem::Radio(l, _, _) | MenuItem::Sub(l, _) => Some(l),
        MenuItem::Title(l, _) | MenuItem::Primary(l, _, _) => Some(l),
        MenuItem::Sep | MenuItem::Icon(..) => None,
    }
}

/// Itens que o mouse e o teclado escolhem (não as linhas nem o título).
fn selectable(item: &MenuItem) -> bool {
    !matches!(base(item), MenuItem::Sep | MenuItem::Title(..))
}

fn height(item: &MenuItem) -> f32 {
    match base(item) {
        MenuItem::Sep => SEP_H,
        MenuItem::Title(..) => TITLE_H,
        MenuItem::Primary(..) => PRIMARY_H,
        _ => ITEM_H,
    }
}

/// Largura (lógica) que o texto do item pede, sem as colunas.
fn text_width(probe: &Canvas, item: &MenuItem) -> f32 {
    let Some(l) = label(item) else { return 0.0 };
    match base(item) {
        MenuItem::Title(..) => probe.text_width(l, Face::Semibold, 13.0) + 10.0,
        MenuItem::Primary(..) => probe.text_width(l, Face::Semibold, 12.5),
        MenuItem::Hint(_, _, hint) => probe.text_width(l, Face::Regular, 12.0) + probe.text_width(hint, Face::Regular, 11.0) + 24.0,
        _ => probe.text_width(l, Face::Regular, 12.0),
    }
}

/// Medidas de um menu (lógicas, sem a margem da sombra): largura, altura e o retângulo de cada
/// item a partir do canto do menu.
fn measure(items: &[MenuItem]) -> (f32, f32, Vec<(f32, f32, f32, f32)>) {
    let probe = Canvas::new(1, 1, 1.0);
    let widest = items.iter().map(|e| text_width(&probe, e)).fold(0.0f32, f32::max);
    let w = MIN_W.max(PAD + ICON_COL + widest + 30.0 + PAD).ceil();
    let mut rects = Vec::new();
    let mut top = PAD;
    for (n, e) in items.iter().enumerate() {
        let h = height(e);
        rects.push((PAD, top, w - PAD, top + h));
        top += h;
        // o título e o botão em destaque ganham um respiro embaixo
        if matches!(base(e), MenuItem::Title(..) | MenuItem::Primary(..)) && items.get(n + 1).is_some_and(|e| !matches!(e, MenuItem::Sep)) {
            top += 4.0;
        }
    }
    (w, (top + PAD).ceil(), rects)
}

/// Desenha um menu (sem a sombra) em `c`, com o canto em (ox, oy), em pixels lógicos.
fn paint(c: &mut Canvas, items: &[MenuItem], rects: &[(f32, f32, f32, f32)], w: f32, h: f32, ox: f32, oy: f32, hot: Option<usize>, s: f32) {
    c.fill_round_rect(ox, oy, ox + w, oy + h, RADIUS, MENU_BG);
    c.stroke_round_rect(ox, oy, ox + w, oy + h, RADIUS, WHITE.with_a(30), 1.0 / s);
    for (n, (item, &(x0, y0, x1, y1))) in items.iter().zip(rects).enumerate() {
        let (x0, y0, x1, y1) = (ox + x0, oy + y0, ox + x1, oy + y1);
        let ym = (y0 + y1) / 2.0;
        let cx = x0 + ICON_COL / 2.0;
        let is_hot = hot == Some(n);
        let text = label(item).unwrap_or("");
        match base(item) {
            MenuItem::Sep => {
                c.hline_px(x0 + 10.0, x1 - 10.0, ym, Rgba(255, 255, 255, 26));
                continue;
            }
            MenuItem::Title(..) => {
                // o ícone do widget num quadradinho e o nome
                let t = 26.0;
                c.fill_round_rect(x0 + 4.0, ym - t / 2.0, x0 + 4.0 + t, ym + t / 2.0, 8.0, WHITE.with_a(28));
                c.text(x0 + 4.0 + t / 2.0, ym, icon(icon_of(item).unwrap_or("")), Face::Icons, 13.0, FG, Anchor::MM, None);
                c.text(x0 + t + 14.0, ym, text, Face::Semibold, 13.0, FG, Anchor::LM, Some(x1 - x0 - t - 18.0));
                continue;
            }
            MenuItem::Primary(..) => {
                let fill = if is_hot { ACCENT_HOVER } else { ACCENT };
                c.fill_round_rect(x0 + 1.0, y0 + 3.0, x1 - 1.0, y1 - 3.0, 10.0, fill);
                c.text(cx + 1.0, ym, icon(icon_of(item).unwrap_or("")), Face::Icons, 14.0, WHITE, Anchor::MM, None);
                c.text(x0 + ICON_COL, ym, text, Face::Semibold, 12.5, WHITE, Anchor::LM, None);
                continue;
            }
            _ => {}
        }
        if is_hot {
            c.fill_round_rect(x0, y0, x1, y1, 8.0, WHITE.with_a(26));
        }
        let ic = icon_of(item);
        if let Some(name) = ic {
            c.text(cx, ym, icon(name), Face::Icons, 13.0, if is_hot { FG } else { ICON_COLOR }, Anchor::MM, None);
        }
        c.text(x0 + ICON_COL, ym, text, Face::Regular, 12.0, FG, Anchor::LM, None);
        let right = x1 - 15.0;
        // a marca vai à esquerda; se ali já há um ícone, à direita
        let mark_x = if ic.is_some() { right } else { cx };
        match base(item) {
            MenuItem::Check(_, true, _) => c.line(&[(mark_x - 5.0, ym), (mark_x - 1.5, ym + 3.5), (mark_x + 5.0, ym - 4.0)], ACCENT_HOVER, 1.7),
            MenuItem::Radio(_, true, _) => c.fill_circle(mark_x, ym, 3.4, ACCENT_HOVER),
            MenuItem::Sub(..) => {
                c.line(&[(right - 2.0, ym - 4.5), (right + 2.5, ym), (right - 2.0, ym + 4.5)], Rgba(255, 255, 255, 150), 1.5);
            }
            MenuItem::Hint(_, _, hint) => {
                c.text(x1 - 10.0, ym, hint, Face::Regular, 11.0, MUTED, Anchor::RM, None);
            }
            _ => {}
        }
    }
}

/// Um menu inteiro, com a sombra, numa imagem (para conferir o desenho num teste).
#[cfg(test)]
pub fn menu_image(items: &[MenuItem], hot: Option<usize>, s: f32) -> Image {
    let (w, h, rects) = measure(items);
    let (pw, ph) = (((w + 2.0 * MARGIN) * s).round() as u32, ((h + 2.0 * MARGIN) * s).round() as u32);
    let mut c = Canvas::new(pw, ph, s);
    c.image(&shadow(w, h, s, pw, ph), 0.0, 0.0);
    paint(&mut c, items, &rects, w, h, MARGIN, MARGIN, hot, s);
    c.finish()
}

/// A sombra suave embaixo do menu, numa imagem do tamanho da janela.
fn shadow(w: f32, h: f32, s: f32, pw: u32, ph: u32) -> Image {
    let mut sh = Canvas::new(pw, ph, s);
    sh.fill_round_rect(MARGIN + 2.0, MARGIN + 5.0, MARGIN + w - 2.0, MARGIN + h + 3.0, RADIUS, Rgba(0, 0, 0, 120));
    let mut img = sh.finish();
    blur(&mut img.px, pw, ph, 4, 7.0 * s);
    img
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
        let (w, h, rects) = measure(&items);
        let m = (MARGIN * s).round() as i32;
        let rects: Vec<_> = rects.into_iter().map(|(x0, y0, x1, y1)| (x0 * s + m as f32, y0 * s + m as f32, x1 * s + m as f32, y1 * s + m as f32)).collect();
        let (wi, hi) = ((w * s).round() as i32, (h * s).round() as i32);
        let area = win::area_for((x, y, x + 1, y + 1));
        let pad = (PAD * s) as i32;
        let (mut x, mut y) = (x, y);
        if let Some((px0, py0, px1, _)) = parent {
            // submenu ao lado do item, para a direita se couber
            x = px1 + pad;
            y = py0 - pad;
            if x + wi > area.2 {
                x = px0 - wi - pad;
            }
        } else if x + wi > area.2 {
            x -= wi;
        }
        if y + hi > area.3 {
            y = if parent.is_none() { y - hi } else { area.3 - hi };
        }
        let (x, y) = (x.max(area.0), y.max(area.1));
        let size = (wi + 2 * m, hi + 2 * m);
        let hwnd = events::create_popup();
        let shadow = shadow(w, h, s, size.0 as u32, size.1 as u32);
        self.levels.push(Level { hwnd, items, rects, pos: (x - m, y - m), size, hot: None, shadow });
        let i = self.levels.len() - 1;
        self.draw(i);
        events::show(hwnd, true);
        win::set_z(hwnd, Z::Topmost);
    }

    fn draw(&self, i: usize) {
        let l = &self.levels[i];
        let s = self.s;
        let m = (MARGIN * s).round();
        let (w, h) = ((l.size.0 as f32 - 2.0 * m) / s, (l.size.1 as f32 - 2.0 * m) / s);
        let rects: Vec<_> = l.rects.iter().map(|&(x0, y0, x1, y1)| ((x0 - m) / s, (y0 - m) / s, (x1 - m) / s, (y1 - m) / s)).collect();
        let mut c = Canvas::new(l.size.0 as u32, l.size.1 as u32, s);
        c.image(&l.shadow, 0.0, 0.0);
        paint(&mut c, &l.items, &rects, w, h, m / s, m / s, l.hot, s);
        win::blit(l.hwnd, &c.finish(), Some(l.pos), 255);
    }

    fn index_at(&self, level: usize, x: i32, y: i32) -> Option<usize> {
        let l = &self.levels[level];
        l.rects.iter().enumerate().find_map(|(n, &(x0, y0, x1, y1))| {
            let inside = (x as f32) >= x0 && (x as f32) < x1 && (y as f32) >= y0 && (y as f32) < y1;
            (inside && selectable(&l.items[n])).then_some(n)
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
            if let MenuItem::Sub(_, sub) = base(&self.levels[level].items[n]) {
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
        let id = match base(&self.levels[level].items[index]) {
            MenuItem::Sub(..) => {
                self.set_hot(level, Some(index));
                return None;
            }
            MenuItem::Cmd(_, id) | MenuItem::Hint(_, id, _) | MenuItem::Check(_, _, id) | MenuItem::Radio(_, _, id) | MenuItem::Primary(_, id, _) => *id,
            MenuItem::Sep | MenuItem::Title(..) | MenuItem::Icon(..) => return None,
        };
        self.close(0);
        Some(id)
    }

    /// Teclado: setas, Enter, Esc. Devolve o item escolhido, se houver.
    pub fn key(&mut self, vk: u32) -> Option<u32> {
        let level = self.levels.len().checked_sub(1)?;
        let items: Vec<usize> = (0..self.levels[level].items.len()).filter(|&n| selectable(&self.levels[level].items[n])).collect();
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
                if let MenuItem::Sub(..) = base(&self.levels[level].items[hot]) {
                    self.levels[level].hot = None;
                    self.set_hot(level, Some(hot));
                    let sub = self.levels.len() - 1;
                    self.levels[sub].hot = (0..self.levels[sub].items.len()).find(|&n| selectable(&self.levels[sub].items[n]));
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
    /// área de trabalho do monitor onde a pílula aparece (escolhida ao mostrar)
    area: win::Rect,
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
            area: (0, 0, 0, 0),
        }
    }

    pub fn hwnd(&self) -> Option<isize> {
        self.hwnd.map(|h| h.0 as isize)
    }

    pub fn show(&mut self, parts: Vec<Part>, animations: bool) {
        self.parts = parts;
        self.hot = None;
        // aparece no monitor onde está o mouse (ali que a pessoa está olhando); se já estava
        // visível, continua no mesmo lugar
        if !self.visible || self.area.2 <= self.area.0 {
            self.area = win::area_at_pointer();
        }
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
        let area = self.area;
        let s = win::scale_at((area.0 + area.2) / 2, (area.1 + area.3) / 2);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn com() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
    }

    /// Um menu de widget como o app monta: título, o botão em destaque e itens com ícones.
    fn sample() -> Vec<MenuItem> {
        vec![
            MenuItem::Title("GitHub".into(), "github"),
            MenuItem::Primary("Organizar widgets".into(), 1, "organizar"),
            MenuItem::Sep,
            MenuItem::Sub("Tamanho".into(), vec![MenuItem::Radio("Médio".into(), true, 2)]).icon("tamanho"),
            MenuItem::Sep,
            MenuItem::Cmd("Atualizar".into(), 3),
            MenuItem::Cmd("Mudar usuário…".into(), 4),
            MenuItem::Sep,
            MenuItem::Sub("Widgets".into(), Vec::new()).icon("widgets"),
            MenuItem::Check("Travar posições".into(), true, 5).icon("lock"),
            MenuItem::Check("Sem ícone".into(), true, 6),
            MenuItem::Hint("Com atalho".into(), 7, "Ctrl+K".into()),
            MenuItem::Cmd("Sair".into(), 8).icon("sair"),
        ]
    }

    /// O título e as linhas não são escolhidos pelo mouse nem pelo teclado; os outros (com
    /// ícone ou não) sim, e o botão em destaque devolve o número dele.
    #[test]
    fn menu_items_are_chosen_right() {
        com();
        let items = sample();
        let pick: Vec<bool> = items.iter().map(selectable).collect();
        assert_eq!(pick, [false, true, false, true, false, true, true, false, true, true, true, true, true]);
        assert_eq!(label(&items[3]), Some("Tamanho"));
        assert!(matches!(base(&items[3]), MenuItem::Sub(..)));
        assert_eq!(icon_of(&items[12]), Some("sair"));
        // as alturas: o título e o botão são maiores e ganham um respiro; nada se sobrepõe
        let (w, h, rects) = measure(&items);
        assert!(w >= MIN_W);
        assert_eq!(rects[0].3 - rects[0].1, TITLE_H);
        assert_eq!(rects[1].3 - rects[1].1, PRIMARY_H);
        for r in rects.windows(2) {
            assert!(r[1].1 >= r[0].3, "{r:?}");
        }
        assert!(rects.last().unwrap().3 + PAD <= h);
    }

    /// Desenha o menu num PNG (para conferir): cargo test --release popup::tests::draw_menu -- --ignored
    #[test]
    #[ignore]
    fn draw_menu() {
        com();
        for (name, hot) in [("menu", None), ("menu_organizar", Some(1)), ("menu_item", Some(5))] {
            let img = menu_image(&sample(), hot, 1.5);
            let path = std::env::temp_dir().join(format!("{name}.png"));
            crate::gfx::save_png(&img, &path).unwrap();
            println!("{}", path.display());
        }
    }
}
