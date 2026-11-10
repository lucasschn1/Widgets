//! Base de todo widget: cartão de vidro com até três tamanhos, botão de tamanho, destaque sob o
//! mouse, barra de "carregando", aviso de erro e as animações (esmaecer, deslizar, redimensionar).
//!
//! O que envolve outros widgets (arrastar, encaixar, reorganizar) fica no `App`.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;

use crate::gfx::{Canvas, Image};
use crate::shared::Shared;
use crate::theme::*;
use crate::widgets::{Env, Hot, HoverStyle, Kind, Layout, Mode, Status};
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
    /// posição da janela no fim (pode andar para não passar da borda da tela)
    to: (i32, i32),
    start: Instant,
    dur: Duration,
    pub save: bool,
    /// na metade direita da tela, a borda direita fica parada
    right: bool,
    /// o tamanho de antes e se o widget desenha cada quadro da troca (`Kind::draw_between`);
    /// senão, a imagem de um tamanho vai dando lugar à do outro
    from: Mode,
    morph: bool,
}

/// Quanto dura a troca de tamanho desenhada quadro a quadro: mais que a de sempre, para dar tempo
/// de cada coisa andar até o lugar novo.
pub const MORPH_MS: u64 = 650;

pub struct Slide {
    from: (i32, i32),
    pub to: (i32, i32),
    start: Instant,
    dur: Duration,
}

fn ease_out_quick(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// Desenha um widget num tamanho e escala quaisquer (miniaturas, prévia).
pub fn render_kind(kind: &dyn Kind, mode: Mode, s: f32, sh: &Shared) -> Image {
    let l = Layout::of(mode);
    let mut c = Canvas::new((l.w * s).round() as u32, (l.h * s).round() as u32, s);
    c.text_scale = sh.text_scale;
    let env = Env { cfg: &sh.cfg, key: kind.key() };
    kind.draw(&mut c, mode, &env);
    c.finish()
}

/// A luz do mouse na borda: até onde ela chega (pontos) e de quanto em quanto tempo acompanha o
/// cursor (no máximo 30 quadros por segundo, e só quando ele anda).
const GLOW_RADIUS: f32 = 150.0;
const GLOW_FRAME_MS: u64 = 33;

/// Apertando uma área grande do widget (um anel, a capa, o relógio), o cartão inteiro afunda até
/// este tamanho e volta com mola ao soltar, como no macOS; botões pequenos só escurecem.
pub const PRESS_SCALE: f32 = 0.97;
/// a área apertada precisa ocupar pelo menos isto do cartão para ele afundar inteiro
const PRESS_WHOLE: f32 = 0.2;

/// O esqueleto pulsando enquanto o widget busca os dados pela primeira vez: o ciclo e a
/// opacidade dos blocos, do mais fraco ao mais forte.
const SKELETON_MS: f32 = 1400.0;
const SKELETON_ALPHA: (f32, f32) = (0.07, 0.16);

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
    /// onde o pequeno estava antes de abrir pelo mouse (volta para lá ao fechar)
    pub temp_home: Option<(i32, i32)>,
    pub hover: f32,
    pub hover_target: f32,
    /// o que está sob o mouse, e desde quando (o destaque aparece aos poucos)
    pub hot: Option<(Hot, HoverStyle)>,
    hot_since: Instant,
    /// o destaque anterior, sumindo: (área, estilo, desde quando)
    hot_prev: Option<(Hot, HoverStyle, Instant)>,
    pub pressing: bool,
    /// o botão apertado afunda e volta com mola: (quando, true = apertou / false = soltou)
    pub press_anim: Option<(Instant, bool)>,
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
    /// o mouse sobre o cartão (pontos, no canto do conteúdo): a borda de vidro reflete a luz dele;
    /// e se ele andou desde o último quadro
    glow: Option<(f32, f32)>,
    glow_moved: bool,
    /// a área apertada é grande: o cartão inteiro afunda
    press_whole: bool,
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
            temp_home: None,
            hover: 0.0,
            hover_target: 0.0,
            hot: None,
            hot_since: Instant::now(),
            hot_prev: None,
            pressing: false,
            press_anim: None,
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
            glow: None,
            glow_moved: false,
            press_whole: false,
        }
    }

    /// Começou a apertar uma área clicável (a que está sob o mouse).
    pub fn press_down(&mut self) {
        let l = self.layout(self.mode);
        self.press_whole = self.hot.is_some_and(|((x0, y0, x1, y1, _), _)| (x1 - x0) * (y1 - y0) >= PRESS_WHOLE * l.w * l.h);
        self.pressing = true;
        self.press_anim = Some((Instant::now(), true));
    }

    /// Soltou: o realce (ou o cartão inteiro) volta com mola.
    pub fn press_up(&mut self) {
        self.pressing = false;
        self.press_anim = Some((Instant::now(), false));
    }

    /// O tamanho do cartão agora (1: normal; menor apertando uma área grande; um tico maior na
    /// volta da mola).
    pub fn press_scale(&self) -> f32 {
        if !self.press_whole {
            return 1.0;
        }
        let depth = 1.0 - PRESS_SCALE;
        match self.press_anim {
            Some((at, true)) => 1.0 - depth * ease_out_quick(at.elapsed().as_secs_f32() * 1000.0 / 90.0),
            Some((at, false)) => 1.0 - depth * (1.0 - ease_spring(at.elapsed().as_secs_f32() * 1000.0 / 240.0)),
            None => 1.0,
        }
    }

    /// O esqueleto: os blocos do widget pulsando, no lugar do texto que ainda não chegou.
    fn draw_skeleton(&self, c: &mut Canvas, blocks: &[Hot]) {
        let t = self.loading_since.elapsed().as_secs_f32() * 1000.0 / SKELETON_MS;
        let wave = 0.5 - 0.5 * (t * std::f32::consts::TAU).cos();
        let (lo, hi) = SKELETON_ALPHA;
        let col = WHITE.alpha(lo + (hi - lo) * wave);
        for &(x0, y0, x1, y1, r) in blocks {
            c.fill_round_rect(MARGIN + x0, MARGIN + y0, MARGIN + x1, MARGIN + y1, r, col);
        }
    }

    /// O mouse está em (x, y) pontos sobre o cartão: a luz da borda vai junto (no próximo quadro).
    pub fn move_glow(&mut self, x: f32, y: f32) {
        if self.glow.is_none_or(|(gx, gy)| (gx - x).abs() + (gy - y).abs() >= 0.5) {
            self.glow = Some((x, y));
            self.glow_moved = true;
        }
    }

    /// A luz do mouse na borda de vidro: um traço liso, mais forte perto do cursor, que acende e
    /// apaga junto com o resto do que aparece com o mouse em cima (`hover`). Só na borda: o
    /// interior do cartão não clareia com o mouse (como nos widgets do macOS).
    fn draw_glow(&self, c: &mut Canvas, sh: &Shared) {
        let Some((gx, gy)) = self.glow else { return };
        if self.hover <= 0.0 || sh.editing || !sh.animations || self.drag.is_some() {
            return;
        }
        let lay = self.layout(self.mode);
        let m = MARGIN;
        let a = self.hover.clamp(0.0, 1.0);
        c.glow_round_rect(m, m, m + lay.w, m + lay.h, lay.radius, m + gx, m + gy, GLOW_RADIUS, WHITE.with_a((150.0 * a) as u8), 1.2, 0.0);
    }

    pub fn key(&self) -> &'static str {
        self.kind.key()
    }

    pub fn animating(&self) -> bool {
        self.anim.is_some()
    }

    // --- geometria ---

    pub fn layout(&self, mode: Mode) -> Layout {
        Layout::of(mode)
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

    /// Na metade direita do monitor, o cartão cresce para a esquerda (a borda direita fica parada).
    pub fn grows_left(&self, rect: win::Rect) -> bool {
        let a = win::area_for(rect);
        rect.0 + rect.2 > a.0 + a.2
    }

    /// Onde fica a janela quando o cartão em `old` passa a ter `size`: cresce para o lado de
    /// dentro da tela e, se o lado que cresceu passaria da borda, anda só o necessário para
    /// caber (nunca fica mais cortado do que já estava).
    pub fn target_for(&self, old: win::Rect, size: (i32, i32)) -> (i32, i32) {
        let a = win::area_for(old);
        let edge = (EDGE_GAP * self.s).round() as i32;
        let (w, h) = size;
        let mut x0 = if self.grows_left(old) { old.2 - w } else { old.0 };
        let mut y0 = old.1;
        if x0 + w > old.2 && x0 + w > a.2 - edge {
            x0 = ((a.2 - edge).max(old.2.min(a.2)) - w).max(a.0);
        }
        if x0 < old.0 && x0 < a.0 + edge {
            x0 = (a.0 + edge).min(old.0.max(a.0));
        }
        if y0 + h > old.3 && y0 + h > a.3 - edge {
            y0 = ((a.3 - edge).max(old.3.min(a.3)) - h).max(a.1);
        }
        (x0 - self.margin, y0 - self.margin)
    }

    /// Posição em que termina a animação de tamanho em andamento.
    pub fn resize_goal(&self) -> Option<(i32, i32)> {
        self.anim.as_ref().map(|a| a.to)
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
        if sh.suspended_at(self.card_rect()) && self.content.is_some() {
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
        let to = self.target_for(self.card_rect_at(self.pos, Some((old.w as i32, old_h))), (w, h));
        let new = self.render(self.mode, sh);
        let r = self.radius_px(self.mode);
        self.anim = Some(ResizeAnim {
            old,
            new,
            r0: r,
            r1: r,
            x: self.pos.0,
            y: self.pos.1,
            to,
            start: Instant::now(),
            dur: Duration::from_millis(if sh.animations { GROW_MS } else { 0 }),
            save: false,
            right: false,
            from: self.mode,
            morph: false,
        });
        self.hot = None;
        self.composed = None;
        self.grew = Some((old_h, h - old_h));
        true
    }

    /// O cartão como aparece na tela (vidro com o conteúdo de agora, sem a margem nem a sombra),
    /// em pixels: a ilha usa para se transformar nele (e a partir dele) sem pulo.
    pub fn face(&self, sh: &Shared) -> Image {
        let content = self.render(self.mode, sh);
        let m = self.margin;
        sh.backdrop.glass(&content, self.radius_px(self.mode), self.pos.0 + m, self.pos.1 + m, sh.tint)
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
        if sh.suspended_at(self.card_rect()) && !move_win {
            return;
        }
        let m = self.margin;
        let key = (self.pos, sh.backdrop.version, (content.w, content.h));
        if self.composed.as_ref().map(|c| (c.0, c.1, c.2)) != Some(key) {
            let glass = sh.backdrop.glass(content, self.radius_px(self.mode), self.pos.0 + m, self.pos.1 + m, sh.tint);
            self.composed = Some((key.0, key.1, key.2, glass));
        }
        let glass = &self.composed.as_ref().unwrap().3;
        let (w, h) = (glass.w + 2 * m as u32, glass.h + 2 * m as u32);
        let mut c = Canvas::new(w, h, self.s);
        // sem sombra, como os widgets do macOS: em volta do vidro, a margem fica transparente (e
        // os cliques ali passam para a área de trabalho)
        // apertando uma área grande, o cartão inteiro afunda (e volta com mola)
        let k = self.press_scale();
        if k != 1.0 && sh.animations {
            let lay = self.layout(self.mode);
            c.scale_about(MARGIN + lay.w / 2.0, MARGIN + lay.h / 2.0, k);
        }
        // o tremor do trovão leva o vidro e a camada animada juntos
        let (dx, dy) = if sh.animations && self.anim.is_none() { self.kind.shake() } else { (0.0, 0.0) };
        c.image(glass, MARGIN + dx, MARGIN + dy);
        if self.anim.is_none() {
            self.kind.overlay(&mut c, self.mode, MARGIN + dx, MARGIN + dy);
        }
        self.decorate(&mut c, sh);
        c.reset_transform();
        let img = c.finish();
        win::blit(self.hwnd, &img, move_win.then_some(self.pos), self.opacity(sh));
        self.last_frame = Instant::now();
    }

    /// O destaque de quem está sob o mouse (área em unidades do cartão, canto do cartão em (ox, oy)),
    /// com a opacidade `a` (0 a 1) da entrada ou da saída.
    pub fn paint_hover(c: &mut Canvas, (x0, y0, x1, y1, r): Hot, style: HoverStyle, pressing: bool, ox: f32, oy: f32, a: f32) {
        let fade = |col: Rgba| col.with_a((col.3 as f32 * a.clamp(0.0, 1.0)) as u8);
        match (style, pressing) {
            (_, true) => c.fill_round_rect(ox + x0, oy + y0, ox + x1, oy + y1, r, fade(PRESS_SURFACE)),
            (HoverStyle::Surface, false) => {
                c.fill_round_rect(ox + x0, oy + y0, ox + x1, oy + y1, r, fade(HOVER_SURFACE));
                c.stroke_round_rect(ox + x0 + 0.5, oy + y0 + 0.5, ox + x1 - 0.5, oy + y1 - 0.5, r, fade(HOVER_EDGE), 1.0 / c.s);
            }
            // áreas grandes de conteúdo: nada por cima (o cursor de mãozinha basta)
            (HoverStyle::Quiet, false) => {}
        }
    }

    /// Muda o que está sob o mouse: o destaque novo entra e o antigo sai aos poucos.
    pub fn set_hot(&mut self, hot: Option<(Hot, HoverStyle)>, animations: bool) {
        if hot.map(|h| h.0) == self.hot.map(|h| h.0) {
            self.hot = hot;
            return;
        }
        self.hot_prev = match (self.hot.take(), animations) {
            (Some((area, style)), true) => Some((area, style, Instant::now())),
            _ => None,
        };
        self.hot = hot;
        self.hot_since = if animations { Instant::now() } else { Instant::now() - Duration::from_millis(HOVER_FADE_MS) };
    }

    /// Opacidade de um destaque que começou (ou começou a sumir) em `since`.
    fn hover_progress(since: Instant) -> f32 {
        (since.elapsed().as_secs_f32() * 1000.0 / HOVER_FADE_MS as f32).min(1.0)
    }

    fn hover_fading(&self) -> bool {
        (self.hot.is_some() && Self::hover_progress(self.hot_since) < 1.0) || self.hot_prev.is_some_and(|p| Self::hover_progress(p.2) < 1.0)
    }

    /// Detalhes por cima do vidro: destaque do mouse, aviso de erro, "carregando" e botões.
    fn decorate(&self, c: &mut Canvas, sh: &Shared) {
        let m = MARGIN;
        self.draw_glow(c, sh);
        if self.drag.is_none() {
            if let Some((area, style, since)) = self.hot_prev {
                Self::paint_hover(c, area, style, false, m, m, 1.0 - Self::hover_progress(since));
            }
            if let Some(((x0, y0, x1, y1, r), style)) = self.hot {
                if x1 - x0 >= 2.0 && y1 - y0 >= 2.0 {
                    // apertado, o realce encolhe um pouco; solto, volta passando um tico do tamanho
                    let k = self.press_inset().min((x1 - x0).min(y1 - y0) / 4.0);
                    let area = (x0 + k, y0 + k, x1 - k, y1 - k, (r - k).max(0.0));
                    Self::paint_hover(c, area, style, self.pressing, m, m, Self::hover_progress(self.hot_since));
                }
            }
        }
        if self.status.error.is_some() && !sh.editing {
            c.fill_circle(m + STALE_XY.0, m + STALE_XY.1, 3.5, STALE_DOT);
        }
        if self.status.loading {
            let blocks = self.kind.skeleton(self.mode);
            if blocks.is_empty() {
                self.draw_loading(c);
            } else {
                self.draw_skeleton(c, &blocks);
            }
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
        // vidro escuro com um fio claro (sem o círculo cinza sólido)
        let r = r - 1.0;
        c.fill_circle(cx, cy, r, fade(Rgba(28, 28, 34, 190)));
        c.stroke_circle(cx, cy, r - 0.5, fade(WHITE.with_a(46)), 1.0);
        let arm = 4.0;
        c.line(&[(cx - arm, cy), (cx + arm, cy)], fade(FG.alpha(0.9)), 1.5);
        // "+" quando o próximo tamanho é maior (ou para fixar aberto), "−" quando volta ao menor
        let sizes = self.kind.sizes();
        let bigger = self.temp || sizes.iter().position(|&m| m == self.next_mode()) > sizes.iter().position(|&m| m == self.mode);
        if bigger {
            c.line(&[(cx, cy - arm), (cx, cy + arm)], fade(FG.alpha(0.9)), 1.5);
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

    pub fn hot_at(&self, x: i32, y: i32, sh: &Shared) -> Option<(Hot, HoverStyle)> {
        let (lx, ly) = self.logical(x, y);
        if self.on_stale_dot(lx, ly) {
            let (sx, sy) = STALE_XY;
            return Some(((sx - 8.0, sy - 8.0, sx + 8.0, sy + 8.0, 8.0), HoverStyle::Surface));
        }
        let env = Env { cfg: &sh.cfg, key: self.key() };
        let area = self.kind.hot_rect(lx, ly, self.mode, &env)?;
        Some((area, self.kind.hover_style(lx, ly, self.mode, &env)))
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

    /// Começa a animação de mudar de tamanho, terminando com a janela em `to` (o App move os
    /// vizinhos).
    pub fn resize_to(&mut self, mode: Mode, sh: &Shared, save: bool, to: (i32, i32)) {
        if self.anim.is_some() || mode == self.mode || !self.kind.sizes().contains(&mode) {
            return;
        }
        let right = self.grows_left(self.card_rect());
        let from = self.mode;
        let morph = sh.animations && self.kind.morphs(from, mode);
        let old = self.render(from, sh);
        let r0 = self.radius_px(from);
        self.mode = mode;
        let new = self.render(mode, sh);
        let r1 = self.radius_px(mode);
        let (x, y) = self.pos;
        let ms = match (sh.animations, morph) {
            (false, _) => 0,
            (true, true) => MORPH_MS,
            (true, false) => ANIM_MS,
        };
        self.anim = Some(ResizeAnim {
            old,
            new,
            r0,
            r1,
            x,
            y,
            to,
            start: Instant::now(),
            dur: Duration::from_millis(ms),
            save,
            right,
            from,
            morph,
        });
        self.hot = None;
        self.content = None;
    }

    /// Quanto dura a troca de tamanho em andamento (os vizinhos saem da frente no mesmo tempo).
    pub fn resize_ms(&self) -> u64 {
        self.anim.as_ref().map_or(ANIM_MS, |a| a.dur.as_millis() as u64)
    }

    /// Um quadro da animação de tamanho; devolve true quando terminou.
    fn anim_step(&mut self, sh: &Shared) -> bool {
        let a = self.anim.as_ref().unwrap();
        let t = if a.dur.is_zero() { 1.0 } else { (a.start.elapsed().as_secs_f32() / a.dur.as_secs_f32()).min(1.0) };
        // o cartão cresce com uma mola (passa um pouco e volta), no mesmo ritmo dos vizinhos que
        // saem da frente (que também deslizam com mola)
        let e = ease_spring(t);
        let w = (a.old.w as f32 + (a.new.w as f32 - a.old.w as f32) * e).round().max(1.0) as u32;
        let h = (a.old.h as f32 + (a.new.h as f32 - a.old.h as f32) * e).round().max(1.0) as u32;
        let content = if a.morph {
            // o widget desenha o caminho de um tamanho ao outro
            let mut c = Canvas::new(w, h, self.s);
            c.text_scale = sh.text_scale;
            let env = Env { cfg: &sh.cfg, key: self.key() };
            self.kind.draw_between(&mut c, a.from, self.mode, t, &env);
            c.finish()
        } else {
            // os dois estados presos no mesmo canto, um sumindo e o outro aparecendo
            let place = |card: &Image| {
                let mut layer = Image::new(w, h);
                let x = if a.right { w as i32 - card.w as i32 } else { 0 };
                layer.over(card, x, 0);
                layer
            };
            let fade = ease_in_out(((t - 0.1) / 0.7).clamp(0.0, 1.0));
            Image::blend(&place(&a.old), &place(&a.new), fade)
        };
        let m = self.margin;
        // a janela vai de onde estava até `to` junto com o tamanho
        let x = (a.x as f32 + (a.to.0 - a.x) as f32 * e).round() as i32;
        let y = (a.y as f32 + (a.to.1 - a.y) as f32 * e).round() as i32;
        let radius = a.r0 + (a.r1 - a.r0) * e;
        let glass = sh.backdrop.glass(&content, radius, x + m, y + m, sh.tint);
        let mut img = Image::new(w + 2 * m as u32, h + 2 * m as u32);
        img.over(&glass, m, m);
        win::blit(self.hwnd, &img, Some((x, y)), self.opacity(sh));
        if t >= 1.0 {
            self.pos = a.to;
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
    /// Quanto o realce do botão apertado encolhe agora (pixels lógicos; negativo: passou do tamanho).
    fn press_inset(&self) -> f32 {
        const DEPTH: f32 = 2.5;
        match self.press_anim {
            Some((at, true)) => DEPTH * ease_out_quick(at.elapsed().as_secs_f32() * 1000.0 / 90.0),
            Some((at, false)) => DEPTH * (1.0 - ease_spring(at.elapsed().as_secs_f32() * 1000.0 / 240.0)),
            None => 0.0,
        }
    }

    fn press_animating(&self) -> bool {
        match self.press_anim {
            Some((at, down)) => at.elapsed() < Duration::from_millis(if down { 110 } else { 260 }),
            None => false,
        }
    }

    pub fn next_frame(&self) -> Option<Instant> {
        if self.anim.is_some() || self.slide.is_some() || self.hover != self.hover_target || self.press_animating() || self.hover_fading() {
            return Some(self.last_frame + Duration::from_millis(FRAME_MS));
        }
        let mut wait = None;
        if self.status.loading || (self.glow_moved && self.hover > 0.0) {
            wait = Some(GLOW_FRAME_MS);
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
            let e = ease_spring(t);
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
        self.glow_moved = false;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Card {
        Card::new(Box::new(crate::widgets::battery::Battery::sample()), HWND::default(), Mode::M, 1.0)
    }

    /// Apertando uma área grande (o anel), o cartão inteiro afunda e volta com mola, passando um
    /// tico do tamanho; apertando um botão pequeno, só o realce dele muda.
    #[test]
    fn pressing_a_big_area_sinks_the_whole_card_and_springs_back() {
        let mut c = card();
        c.hot = Some(((10.0, 10.0, 40.0, 40.0, 8.0), HoverStyle::Surface));
        c.press_down();
        c.press_anim = Some((Instant::now() - Duration::from_millis(200), true));
        assert_eq!(c.press_scale(), 1.0, "botão pequeno");
        c.hot = Some(((0.0, 0.0, 140.0, 140.0, 70.0), HoverStyle::Quiet));
        c.press_down();
        assert!(c.pressing);
        c.press_anim = Some((Instant::now() - Duration::from_millis(200), true));
        assert!((c.press_scale() - PRESS_SCALE).abs() < 1e-3, "{}", c.press_scale());
        c.press_up();
        assert!(!c.pressing);
        c.press_anim = Some((Instant::now() - Duration::from_millis(192), false));
        assert!(c.press_scale() > 1.0, "a mola passa do tamanho: {}", c.press_scale());
        c.press_anim = Some((Instant::now() - Duration::from_secs(1), false));
        assert!((c.press_scale() - 1.0).abs() < 1e-4);
    }
}
