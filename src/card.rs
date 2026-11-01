//! Base de todo widget: cartão de vidro com até três tamanhos, botão de tamanho, destaque sob o
//! mouse, barra de "carregando", aviso de erro e as animações (esmaecer, deslizar, redimensionar).
//!
//! O que envolve outros widgets (arrastar, encaixar, reorganizar) fica no `App`.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;

use crate::gfx::{Canvas, Image};
use crate::shared::Shared;
use crate::theme::*;
use crate::widgets::{Env, Hot, Kind, Layout, Mode, Status};
use crate::win;

/// Ação de um clique, decidida ao apertar e executada ao soltar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Badge,
    Close,
    Retry,
    Kind(u32),
}

pub struct Press {
    pub x: i32,
    pub y: i32,
    pub action: Option<Act>,
}

pub struct ResizeAnim {
    old: Image,
    new: Image,
    r0: f32,
    r1: f32,
    x: i32,
    y: i32,
    start: Instant,
    dur: Duration,
    pub save: bool,
    /// na metade direita da tela, a borda direita fica parada
    right: bool,
}

pub struct Slide {
    from: (i32, i32),
    pub to: (i32, i32),
    start: Instant,
    dur: Duration,
}

/// Desenha um widget num tamanho e escala quaisquer (miniaturas, prévia).
pub fn render_kind(kind: &dyn Kind, mode: Mode, s: f32, sh: &Shared) -> Image {
    let l = kind.layout(mode);
    let mut c = Canvas::new((l.w * s).round() as u32, (l.h * s).round() as u32, s);
    c.text_scale = sh.text_scale;
    let env = Env { cfg: &sh.cfg, key: kind.key() };
    kind.draw(&mut c, mode, &env);
    c.finish()
}

pub struct Card {
    pub kind: Box<dyn Kind>,
    pub hwnd: HWND,
    pub owner: Option<HWND>,
    pub mode: Mode,
    pub s: f32,
    pub margin: i32,
    pub pos: (i32, i32),
    /// aberto só enquanto o mouse está em cima (era pequeno)
    pub temp: bool,
    pub hover: f32,
    pub hover_target: f32,
    pub hot: Option<Hot>,
    pub pressing: bool,
    pub press: Option<Press>,
    pub drag: Option<(i32, i32)>,
    pub anim: Option<ResizeAnim>,
    pub slide: Option<Slide>,
    pub expand_at: Option<Instant>,
    pub outside_since: Option<Instant>,
    pub content: Option<Image>,
    composed: Option<((i32, i32), u64, (u32, u32), Image)>,
    /// mudou algo enquanto estava pausado ou escondido
    pub stale: bool,
    pub close_hot: bool,
    pub status: Status,
    /// quadros por segundo da camada animada do widget (0: parada)
    pub fps: u32,
    /// cresceu ou encolheu sem mudar de tamanho (abriu detalhes): (altura antiga, diferença), em
    /// pixels, para o App levar junto os widgets de baixo
    pub grew: Option<(i32, i32)>,
    loading_since: Instant,
    last_frame: Instant,
}

impl Card {
    pub fn new(kind: Box<dyn Kind>, hwnd: HWND, mode: Mode, s: f32) -> Card {
        Card {
            kind,
            hwnd,
            owner: None,
            mode,
            s,
            margin: (MARGIN * s).round() as i32,
            pos: (0, 0),
            temp: false,
            hover: 0.0,
            hover_target: 0.0,
            hot: None,
            pressing: false,
            press: None,
            drag: None,
            anim: None,
            slide: None,
            expand_at: None,
            outside_since: None,
            content: None,
            composed: None,
            stale: false,
            close_hot: false,
            status: Status::default(),
            fps: 0,
            grew: None,
            loading_since: Instant::now(),
            last_frame: Instant::now(),
        }
    }

    pub fn key(&self) -> &'static str {
        self.kind.key()
    }

    pub fn animating(&self) -> bool {
        self.anim.is_some()
    }

    // --- geometria ---

    pub fn layout(&self, mode: Mode) -> Layout {
        self.kind.layout(mode)
    }

    pub fn card_size_of(&self, mode: Mode) -> (i32, i32) {
        let l = self.layout(mode);
        ((l.w * self.s).round() as i32, (l.h * self.s).round() as i32)
    }

    pub fn card_size(&self) -> (i32, i32) {
        self.card_size_of(self.mode)
    }

    /// Tamanho da janela: o cartão mais a margem transparente.
    pub fn size(&self) -> (i32, i32) {
        let (w, h) = self.card_size();
        (w + 2 * self.margin, h + 2 * self.margin)
    }

    /// Retângulo do cartão na tela (sem a margem transparente).
    pub fn card_rect_at(&self, pos: (i32, i32), size: Option<(i32, i32)>) -> win::Rect {
        let (w, h) = size.unwrap_or_else(|| self.card_size());
        let m = self.margin;
        (pos.0 + m, pos.1 + m, pos.0 + m + w, pos.1 + m + h)
    }

    pub fn card_rect(&self) -> win::Rect {
        self.card_rect_at(self.pos, None)
    }

    pub fn radius_px(&self, mode: Mode) -> f32 {
        self.layout(mode).radius * self.s
    }

    /// Posição logo abaixo de outro widget, encaixada e alinhada pela direita.
    pub fn below(&self, other: &Card) -> (i32, i32) {
        let (w, _) = self.card_size();
        let (_, _, right, bottom) = other.card_rect();
        (right - w - self.margin, bottom + (WIDGET_GAP * self.s).round() as i32 - self.margin)
    }

    pub fn set_scale(&mut self, s: f32) {
        self.s = s;
        self.margin = (MARGIN * s).round() as i32;
        self.composed = None;
        self.content = None;
    }

    /// Se um pedaço razoável do cartão aparece em algum monitor.
    pub fn visible_on_screen(&self, pos: (i32, i32)) -> bool {
        let rect = self.card_rect_at(pos, None);
        let need = (60.0 * self.s) as i32;
        win::monitors().into_iter().any(|screen| {
            let (w, h) = win::overlap(rect, screen);
            w.min(h) >= need
        })
    }

    pub fn logical(&self, x: i32, y: i32) -> (f32, f32) {
        ((x - self.margin) as f32 / self.s, (y - self.margin) as f32 / self.s)
    }

    pub fn pointer_inside(&self) -> bool {
        let (px, py) = win::pointer();
        let (x0, y0, x1, y1) = self.card_rect();
        px >= x0 && px < x1 && py >= y0 && py < y1
    }

    // --- desenho ---

    pub fn render(&self, mode: Mode, sh: &Shared) -> Image {
        let (w, h) = self.card_size_of(mode);
        let mut c = Canvas::new(w as u32, h as u32, self.s);
        c.text_scale = sh.text_scale;
        let env = Env { cfg: &sh.cfg, key: self.key() };
        self.kind.draw(&mut c, mode, &env);
        c.finish()
    }

    /// Redesenha o cartão com os dados atuais e mostra na tela.
    pub fn refresh(&mut self, sh: &Shared) {
        self.status = self.kind.status();
        // na bateria, com a economia ligada, as animações contínuas param
        self.fps = if sh.saving { 0 } else { self.kind.overlay_fps() };
        if sh.suspended() && self.content.is_some() {
            self.stale = true; // redesenha quando voltar a aparecer
            return;
        }
        self.stale = false;
        if self.regrow(sh) {
            return;
        }
        self.content = Some(self.render(self.mode, sh));
        self.composed = None; // o vidro guardado tinha o conteúdo antigo dentro
        if self.anim.is_none() {
            self.present(sh, false);
        }
    }

    /// O widget mudou de altura no mesmo tamanho (ex.: abriu os detalhes de um anel): em vez de
    /// pular, anima do desenho antigo para o novo. Devolve true se começou a animar.
    fn regrow(&mut self, sh: &Shared) -> bool {
        let Some(old) = &self.content else { return false };
        let (w, h) = self.card_size();
        if self.anim.is_some() || (old.w as i32, old.h as i32) == (w, h) || old.w as i32 != w {
            return false;
        }
        let old = self.content.take().unwrap();
        let old_h = old.h as i32;
        let new = self.render(self.mode, sh);
        let r = self.radius_px(self.mode);
        self.anim = Some(ResizeAnim {
            old,
            new,
            r0: r,
            r1: r,
            x: self.pos.0,
            y: self.pos.1,
            start: Instant::now(),
            dur: Duration::from_millis(if sh.animations { GROW_MS } else { 0 }),
            save: false,
            right: false,
        });
        self.hot = None;
        self.composed = None;
        self.grew = Some((old_h, h - old_h));
        true
    }

    /// Opacidade da janela: os widgets esmaecem quando o mouse não está em cima.
    pub fn opacity(&self, sh: &Shared) -> u8 {
        let idle = sh.idle_opacity;
        if sh.peeking || sh.editing || self.drag.is_some() || self.temp || idle >= 1.0 {
            return 255;
        }
        (255.0 * (idle + (1.0 - idle) * self.hover)).round() as u8
    }

    /// Mostra o cartão já desenhado sobre o vidro, com a margem e os detalhes por cima.
    /// Barato: não redesenha o conteúdo. Com `move_win`, também leva a janela para `pos`.
    pub fn present(&mut self, sh: &Shared, move_win: bool) {
        let Some(content) = &self.content else { return };
        if sh.suspended() && !move_win {
            return;
        }        let m = self.margin;
        let key = (self.pos, sh.backdrop.version, (content.w, content.h));
        if self.composed.as_ref().map(|c| (c.0, c.1, c.2)) != Some(key) {
            let glass = sh.backdrop.glass(content, self.radius_px(self.mode), self.pos.0 + m, self.pos.1 + m, sh.tint);
            self.composed = Some((key.0, key.1, key.2, glass));
        }
        let glass = &self.composed.as_ref().unwrap().3;
        let (w, h) = (glass.w + 2 * m as u32, glass.h + 2 * m as u32);
        let mut c = Canvas::new(w, h, self.s);
        c.image(glass, MARGIN, MARGIN);
        if self.anim.is_none() {
            self.kind.overlay(&mut c, self.mode, MARGIN, MARGIN);
        }
        self.decorate(&mut c, sh);
        let img = c.finish();
        win::blit(self.hwnd, &img, move_win.then_some(self.pos), self.opacity(sh));
        self.last_frame = Instant::now();
    }

    /// Detalhes por cima do vidro: destaque do mouse, aviso de erro, "carregando" e botões.
    fn decorate(&self, c: &mut Canvas, sh: &Shared) {
        let m = MARGIN;
        if let (Some((x0, y0, x1, y1, r)), None) = (self.hot, self.drag) {
            if x1 - x0 >= 2.0 && y1 - y0 >= 2.0 {
                c.fill_round_rect(m + x0, m + y0, m + x1, m + y1, r, if self.pressing { PRESS_FILL } else { HOT_FILL });
            }
        }
        if self.status.error.is_some() && !sh.editing {
            c.fill_circle(m + STALE_XY.0, m + STALE_XY.1, 3.5, STALE_DOT);
        }
        if self.status.loading {
            self.draw_loading(c);
        }
        if self.hover > 0.0 || sh.editing {
            self.draw_badge(c, sh);
        }
        if sh.editing {
            self.draw_close(c);
        }
    }

    /// Brilho correndo ao longo do topo do cartão enquanto busca dados.
    fn draw_loading(&self, c: &mut Canvas) {
        let lay = self.layout(self.mode);
        let (x0, x1) = (MARGIN + lay.radius, MARGIN + lay.w - lay.radius);
        let length = x1 - x0;
        if length <= 0.0 {
            return;
        }
        let seg = (length / 3.0).max(8.0);
        let t = (self.loading_since.elapsed().as_secs_f32() * 0.9).fract();
        let start = x0 - seg + (length + seg) * t;
        let slices = 24;
        for i in 0..slices {
            let a = start + seg * i as f32 / slices as f32;
            let b = start + seg * (i + 1) as f32 / slices as f32;
            let (a, b) = (a.max(x0), b.min(x1));
            if b <= a {
                continue;
            }
            let alpha = (230.0 * (std::f32::consts::PI * (i as f32 + 0.5) / slices as f32).sin()) as u8;
            c.fill_round_rect(a, MARGIN + 1.0, b, MARGIN + 3.0, 0.0, Rgba(255, 255, 255, alpha));
        }
    }

    /// Botão de tamanho: no canto superior direito, sobressaindo da borda do cartão.
    pub fn badge_center(&self) -> (f32, f32) {
        let lay = self.layout(self.mode);
        (MARGIN + lay.w - 5.0, MARGIN + 5.0)
    }

    pub fn next_mode(&self) -> Mode {
        let sizes = self.kind.sizes();
        let i = sizes.iter().position(|&m| m == self.mode).unwrap_or(0);
        sizes[(i + 1) % sizes.len()]
    }

    fn draw_badge(&self, c: &mut Canvas, sh: &Shared) {
        let alpha = if sh.editing { 1.0 } else { self.hover };
        let (cx, cy) = self.badge_center();
        let r = BADGE_D / 2.0;
        let fade = |col: Rgba| col.with_a((col.3 as f32 * alpha) as u8);
        c.fill_circle(cx, cy, r, fade(Rgba::rgb(78, 78, 84)));
        c.stroke_circle(cx, cy, r - 0.5, fade(Rgba::rgb(110, 110, 118)), 1.0);
        let arm = 4.5;
        c.line(&[(cx - arm, cy), (cx + arm, cy)], fade(FG), 1.8);
        // "+" quando o próximo tamanho é maior (ou para fixar aberto), "−" quando volta ao menor
        let sizes = self.kind.sizes();
        let bigger = self.temp || sizes.iter().position(|&m| m == self.next_mode()) > sizes.iter().position(|&m| m == self.mode);
        if bigger {
            c.line(&[(cx, cy - arm), (cx, cy + arm)], fade(FG), 1.8);
        }
    }

    pub fn close_center(&self) -> (f32, f32) {
        (MARGIN + 5.0, MARGIN + 5.0)
    }

    fn draw_close(&self, c: &mut Canvas) {
        let (cx, cy) = self.close_center();
        let r = BADGE_D / 2.0;
        c.fill_circle(cx, cy, r, if self.close_hot { DANGER } else { Rgba::rgb(92, 92, 100) });
        c.stroke_circle(cx, cy, r - 0.5, Rgba::rgb(120, 120, 128), 1.0);
        c.line(&[(cx - 4.5, cy), (cx + 4.5, cy)], FG, 1.8);
    }

    /// (x, y) em pixels da janela está sobre o botão de tamanho?
    pub fn on_badge(&self, x: i32, y: i32, sh: &Shared) -> bool {
        if self.hover <= 0.0 && !sh.editing {
            return false;
        }
        let (cx, cy) = self.badge_center();
        let d = ((x as f32 / self.s - cx).powi(2) + (y as f32 / self.s - cy).powi(2)).sqrt();
        d <= BADGE_D / 2.0 + 2.0
    }

    pub fn on_close(&self, x: i32, y: i32, sh: &Shared) -> bool {
        if !sh.editing {
            return false;
        }
        let (cx, cy) = self.close_center();
        ((x as f32 / self.s - cx).powi(2) + (y as f32 / self.s - cy).powi(2)).sqrt() <= BADGE_D / 2.0 + 2.0
    }

    pub fn on_stale_dot(&self, lx: f32, ly: f32) -> bool {
        self.status.error.is_some() && ((lx - STALE_XY.0).powi(2) + (ly - STALE_XY.1).powi(2)).sqrt() <= 9.0
    }

    /// Ação da área clicável em (x, y) pixels da janela (sem contar os botões da base).
    pub fn action_at(&self, x: i32, y: i32, sh: &Shared) -> Option<Act> {
        if self.anim.is_some() || sh.editing {
            return None; // no modo edição, o cartão todo serve para arrastar
        }
        let (lx, ly) = self.logical(x, y);
        if self.on_stale_dot(lx, ly) {
            return Some(Act::Retry);
        }
        let env = Env { cfg: &sh.cfg, key: self.key() };
        self.kind.hit(lx, ly, self.mode, &env).map(Act::Kind)
    }

    pub fn hot_at(&self, x: i32, y: i32, sh: &Shared) -> Option<Hot> {
        let (lx, ly) = self.logical(x, y);
        if self.on_stale_dot(lx, ly) {
            let (sx, sy) = STALE_XY;
            return Some((sx - 8.0, sy - 8.0, sx + 8.0, sy + 8.0, 8.0));
        }
        let env = Env { cfg: &sh.cfg, key: self.key() };
        self.kind.hot_rect(lx, ly, self.mode, &env)
    }

    pub fn tooltip_at(&self, x: i32, y: i32, sh: &Shared) -> Option<String> {
        let (lx, ly) = self.logical(x, y);
        if self.on_stale_dot(lx, ly) {
            let when = self.status.updated.map(|t| format!("\nDados de {}", crate::util::time_ago(t))).unwrap_or_default();
            return Some(format!("{}{when}\nClique para tentar de novo", self.status.error.clone().unwrap_or_default()));
        }
        let env = Env { cfg: &sh.cfg, key: self.key() };
        self.kind.tooltip(lx, ly, self.mode, &env)
    }

    // --- animações ---

    pub fn set_hover(&mut self, on: bool) {
        self.hover_target = if on { 1.0 } else { 0.0 };
    }

    /// Começa a animação de mudar de tamanho (o App move os vizinhos).
    pub fn resize_to(&mut self, mode: Mode, sh: &Shared, save: bool) {
        if self.anim.is_some() || mode == self.mode || !self.kind.sizes().contains(&mode) {
            return;
        }
        let old = self.render(self.mode, sh);
        let r0 = self.radius_px(self.mode);
        let old_mode = self.mode;
        self.mode = mode;
        let new = self.render(mode, sh);
        let r1 = self.radius_px(mode);
        let (x, y) = self.pos;
        let win_w = old.w as i32 + 2 * self.margin;
        let screen_w = win::area_for(self.card_rect_at(self.pos, Some(self.card_size_of(old_mode)))).2;
        self.anim = Some(ResizeAnim {
            old,
            new,
            r0,
            r1,
            x,
            y,
            start: Instant::now(),
            dur: Duration::from_millis(if sh.animations { ANIM_MS } else { 0 }),
            save,
            right: x + win_w / 2 > screen_w / 2,
        });
        self.hot = None;
        self.content = None;
    }

    /// Um quadro da animação de tamanho; devolve true quando terminou.
    fn anim_step(&mut self, sh: &Shared) -> bool {
        let a = self.anim.as_ref().unwrap();
        let t = if a.dur.is_zero() { 1.0 } else { (a.start.elapsed().as_secs_f32() / a.dur.as_secs_f32()).min(1.0) };
        let e = ease_in_out(t);
        let w = (a.old.w as f32 + (a.new.w as f32 - a.old.w as f32) * e).round() as u32;
        let h = (a.old.h as f32 + (a.new.h as f32 - a.old.h as f32) * e).round() as u32;
        // conteúdo: os dois estados presos no mesmo canto, um sumindo e o outro aparecendo
        let place = |card: &Image| {
            let mut layer = Image::new(w, h);
            let x = if a.right { w as i32 - card.w as i32 } else { 0 };
            layer.over(card, x, 0);
            layer
        };
        let fade = ease_in_out(((t - 0.1) / 0.7).clamp(0.0, 1.0));
        let content = Image::blend(&place(&a.old), &place(&a.new), fade);
        let m = self.margin;
        let x = a.x + if a.right { a.old.w as i32 - w as i32 } else { 0 };
        let radius = a.r0 + (a.r1 - a.r0) * e;
        let glass = sh.backdrop.glass(&content, radius, x + m, a.y + m, sh.tint);
        let mut img = Image::new(w + 2 * m as u32, h + 2 * m as u32);
        img.over(&glass, m, m);
        win::blit(self.hwnd, &img, Some((x, a.y)), self.opacity(sh));
        if t >= 1.0 {
            self.pos = (x, a.y);
            return true;
        }
        false
    }

    /// Move o widget até `to` com animação.
    pub fn slide_to(&mut self, to: (i32, i32), ms: u64, sh: &Shared) {
        if self.slide.as_ref().is_some_and(|s| s.to == to) {
            return;
        }
        if self.content.is_none() || self.anim.is_some() || to == self.pos {
            self.slide = None;
            return;
        }
        let dur = Duration::from_millis(if sh.animations { ms } else { 0 });
        self.slide = Some(Slide { from: self.pos, to, start: Instant::now(), dur });
    }

    /// Quando a próxima etapa de animação precisa acontecer.
    pub fn next_frame(&self) -> Option<Instant> {
        if self.anim.is_some() || self.slide.is_some() || self.hover != self.hover_target {
            return Some(self.last_frame + Duration::from_millis(FRAME_MS));
        }
        let mut wait = None;
        if self.status.loading {
            wait = Some(33);
        }
        if self.fps > 0 {
            let ms = 1000 / self.fps as u64;
            wait = Some(wait.map_or(ms, |w: u64| w.min(ms)));
        }
        wait.map(|ms| self.last_frame + Duration::from_millis(ms))
    }

    /// Avança as animações. Devolve (terminou de redimensionar, terminou de deslizar).
    pub fn frame(&mut self, sh: &Shared) -> (bool, bool) {
        let mut resized = false;
        let mut slid = false;
        if self.anim.is_some() {
            if self.anim_step(sh) {
                resized = true;
            }
            self.last_frame = Instant::now();
            return (resized, false);
        }
        if let Some(s) = &self.slide {
            let t = if s.dur.is_zero() { 1.0 } else { (s.start.elapsed().as_secs_f32() / s.dur.as_secs_f32()).min(1.0) };
            let e = ease_in_out(t);
            self.pos = (
                (s.from.0 as f32 + (s.to.0 - s.from.0) as f32 * e).round() as i32,
                (s.from.1 as f32 + (s.to.1 - s.from.1) as f32 * e).round() as i32,
            );
            if t >= 1.0 {
                self.slide = None;
                slid = true;
            }
            self.present(sh, true);
            return (false, slid);
        }
        if self.hover != self.hover_target {
            let step = if sh.animations { FRAME_MS as f32 / BADGE_FADE_MS as f32 } else { 1.0 };
            self.hover = if self.hover < self.hover_target {
                (self.hover + step).min(self.hover_target)
            } else {
                (self.hover - step).max(self.hover_target)
            };
        }
        self.present(sh, false);
        (false, false)
    }

    /// Fim da animação de tamanho: a janela fica no tamanho novo.
    pub fn finish_resize(&mut self, sh: &Shared) -> bool {
        let save = self.anim.take().map(|a| a.save).unwrap_or(false);
        self.hover = if self.pointer_inside() { 1.0 } else { 0.0 };
        self.hover_target = self.hover;
        self.refresh(sh);
        self.present(sh, true);
        save
    }
}
