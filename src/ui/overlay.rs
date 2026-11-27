//! Janelas por cima de tudo que deixam os cliques passarem: guias de encaixe e dicas do mouse.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;

use crate::platform::events::{self, OVERLAY_CLASS};
use crate::gfx::{Anchor, Canvas, Face, Image};
use crate::ui::theme::*;
use crate::platform::win::{self, Z};

/// Uma janela transparente por cima de tudo, que os cliques atravessam.
pub struct Overlay {
    pub hwnd: HWND,
    pub visible: bool,
}

impl Overlay {
    /// Cria a janela (escondida).
    pub fn new() -> Overlay {
        let hwnd = events::create_layered(OVERLAY_CLASS, true);
        events::show(hwnd, true);
        let mut overlay = Overlay { hwnd, visible: true };
        overlay.hide();
        overlay
    }

    /// Mostra a imagem com o canto em `pos` na tela.
    pub fn show(&mut self, img: &Image, pos: (i32, i32)) {
        win::blit(self.hwnd, img, Some(pos), 255);
        if !self.visible {
            win::set_z(self.hwnd, Z::Topmost);
            self.visible = true;
        }
    }

    /// "Escondida" é vazia e fora da tela (o Windows não atualiza janelas transparentes ocultas).
    pub fn hide(&mut self) {
        if self.visible {
            win::blit(self.hwnd, &Image::new(1, 1), Some((-10, -10)), 255);
            self.visible = false;
        }
    }
}

/// Linhas finas que mostram com o que o widget arrastado está alinhado.
pub struct Guides {
    vertical: Overlay,
    horizontal: Overlay,
}

/// Uma guia: onde ela está (x da vertical, y da horizontal) e de onde a onde vai, em pixels da
/// tela.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GuideLine {
    pub at: i32,
    pub from: i32,
    pub to: i32,
}

/// As guias de agora: a vertical e a horizontal.
#[derive(Default, Clone, Copy, PartialEq)]
pub struct GuideLines {
    pub v: Option<GuideLine>,
    pub h: Option<GuideLine>,
}

impl Guides {
    /// As duas janelas das guias (escondidas).
    pub fn new() -> Guides {
        Guides { vertical: Overlay::new(), horizontal: Overlay::new() }
    }

    /// Mostra as guias de `lines` (e esconde as que não vêm).
    pub fn show(&mut self, lines: GuideLines, s: f32) {
        let width = (1.5 * s).round().max(1.0) as u32;
        let line = |w: u32, h: u32| {
            let mut img = Image::new(w, h);
            for pixel in img.px.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[190, 190, 190, 190]); // branco a 75%, pré-multiplicado
            }
            img
        };
        match lines.v {
            Some(GuideLine { at, from, to }) => self.vertical.show(&line(width, (to - from).max(1) as u32), (at - width as i32 / 2, from)),
            None => self.vertical.hide(),
        }
        match lines.h {
            Some(GuideLine { at, from, to }) => self.horizontal.show(&line((to - from).max(1) as u32, width), (from, at - width as i32 / 2)),
            None => self.horizontal.hide(),
        }
    }

    /// Esconde as duas guias.
    pub fn hide(&mut self) {
        self.vertical.hide();
        self.horizontal.hide();
    }
}

/// Dica que aparece perto do mouse depois de um instante parado sobre algo.
pub struct Tooltip {
    window: Overlay,
    text: Option<String>,
    due: Option<PendingTip>,
    hidden_at: Instant,
}

/// Uma dica esperando a hora de aparecer: quando, o texto e onde está o mouse.
struct PendingTip {
    when: Instant,
    text: String,
    x: i32,
    y: i32,
}

impl Tooltip {
    /// A janela da dica (escondida).
    pub fn new() -> Tooltip {
        Tooltip { window: Overlay::new(), text: None, due: None, hidden_at: Instant::now() - Duration::from_secs(10) }
    }

    /// Pede a dica `text` para o mouse em (x, y) da tela; None esconde.
    pub fn request(&mut self, text: Option<String>, x: i32, y: i32) {
        if text == self.text {
            return;
        }
        self.text = text.clone();
        self.due = None;
        if self.window.visible {
            self.window.hide();
            self.hidden_at = Instant::now();
        }
        if let Some(text) = text {
            // logo depois de outra dica, a próxima aparece sem esperar
            let warm = self.hidden_at.elapsed() < Duration::from_millis(400);
            let delay = if warm { 0 } else { TIP_DELAY_MS };
            self.due = Some(PendingTip { when: Instant::now() + Duration::from_millis(delay), text, x, y });
        }
    }

    /// Esconde a dica (e esquece a que ia aparecer).
    pub fn hide(&mut self) {
        self.request(None, 0, 0);
        self.window.hide();
    }

    /// Quando a dica pedida deve aparecer.
    pub fn next_due(&self) -> Option<Instant> {
        self.due.as_ref().map(|tip| tip.when)
    }

    /// Mostra a dica pedida, se chegou a hora.
    pub fn tick(&mut self) {
        if let Some(tip) = self.due.take() {
            if Instant::now() >= tip.when {
                self.show(&tip.text, tip.x, tip.y);
            } else {
                self.due = Some(tip);
            }
        }
    }

    /// Desenha a dica e põe acima do mouse (embaixo, se não couber).
    fn show(&mut self, text: &str, x: i32, y: i32) {
        let s = win::scale_at(x, y);
        // o título (até a primeira linha vazia, ou só a primeira linha) vai em negrito
        let (head, tail) = match text.split_once("\n\n") {
            Some((head, tail)) => (head.to_string(), tail.to_string()),
            None => match text.split_once('\n') {
                Some((head, tail)) => (head.to_string(), tail.to_string()),
                None => (text.to_string(), String::new()),
            },
        };
        let mut lines: Vec<(String, bool)> = head.lines().map(|line| (line.to_string(), true)).collect();
        if !tail.is_empty() {
            lines.extend(tail.lines().map(|line| (line.to_string(), false)));
        }
        let probe = Canvas::new(1, 1, s);
        let width = lines
            .iter()
            .map(|(line, bold)| probe.text_width(line, if *bold { Face::UiSemibold } else { Face::UiRegular }, if *bold { 11.5 } else { 11.0 }))
            .fold(0.0f32, f32::max);
        drop(probe);
        let (pad_x, pad_y, line_h) = (10.0, 6.0, 17.0);
        let w = width + 2.0 * pad_x;
        let h = line_h * lines.len() as f32 + 2.0 * pad_y - 2.0;
        let (pw, ph) = ((w * s).round() as u32, (h * s).round() as u32);
        let mut c = Canvas::new(pw, ph, s);
        c.fill_round_rect(0.0, 0.0, w, h, 8.0, Rgba(34, 34, 40, 240));
        c.stroke_round_rect(0.0, 0.0, w, h, 8.0, WHITE.with_a(40), 1.0 / s);
        for (i, (line, bold)) in lines.iter().enumerate() {
            let (face, size, color) = if *bold { (Face::UiSemibold, 11.5, FG) } else { (Face::UiRegular, 11.0, MUTED) };
            c.text(pad_x, pad_y + i as f32 * line_h, line, face, size, color, Anchor::LA, None);
        }
        let img = c.finish();
        // acima do mouse, centralizada; embaixo se não couber
        let areas = win::work_areas();
        let area = areas.iter().copied().find(|area| x >= area.0 && x < area.2 && y >= area.1 && y < area.3).unwrap_or(areas[0]);
        let px = (x - pw as i32 / 2).max(area.0 + 4).min(area.2 - pw as i32 - 4);
        let mut py = y - ph as i32 - (14.0 * s) as i32;
        if py < area.1 {
            py = y + (22.0 * s) as i32;
        }
        self.window.show(&img, (px, py));
    }
}
