//! Janelas por cima de tudo que deixam os cliques passarem: guias de encaixe e dicas do mouse.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;

use crate::events::{self, OVERLAY_CLASS};
use crate::gfx::{Anchor, Canvas, Face, Image};
use crate::theme::*;
use crate::win::{self, Z};

pub struct Overlay {
    pub hwnd: HWND,
    pub visible: bool,
}

impl Overlay {
    pub fn new() -> Overlay {
        let hwnd = events::create_layered(OVERLAY_CLASS, true);
        events::show(hwnd, true);
        let mut o = Overlay { hwnd, visible: true };
        o.hide();
        o
    }

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
    v: Overlay,
    h: Overlay,
}

/// Guias: vertical (x, y0, y1) e horizontal (y, x0, x1), em pixels da tela.
#[derive(Default, Clone, Copy, PartialEq)]
pub struct GuideLines {
    pub v: Option<(i32, i32, i32)>,
    pub h: Option<(i32, i32, i32)>,
}

impl Guides {
    pub fn new() -> Guides {
        Guides { v: Overlay::new(), h: Overlay::new() }
    }

    pub fn show(&mut self, g: GuideLines, s: f32) {
        let width = (1.5 * s).round().max(1.0) as u32;
        let line = |w: u32, h: u32| {
            let mut img = Image::new(w, h);
            for p in img.px.chunks_exact_mut(4) {
                p.copy_from_slice(&[190, 190, 190, 190]); // branco a 75%, pré-multiplicado
            }
            img
        };
        match g.v {
            Some((at, start, end)) => self.v.show(&line(width, (end - start).max(1) as u32), (at - width as i32 / 2, start)),
            None => self.v.hide(),
        }
        match g.h {
            Some((at, start, end)) => self.h.show(&line((end - start).max(1) as u32, width), (start, at - width as i32 / 2)),
            None => self.h.hide(),
        }
    }

    pub fn hide(&mut self) {
        self.v.hide();
        self.h.hide();
    }
}

/// Dica que aparece perto do mouse depois de um instante parado sobre algo.
pub struct Tooltip {
    window: Overlay,
    text: Option<String>,
    due: Option<(Instant, String, i32, i32)>,
    hidden_at: Instant,
}

impl Tooltip {
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
            self.due = Some((Instant::now() + Duration::from_millis(delay), text, x, y));
        }
    }

    pub fn hide(&mut self) {
        self.request(None, 0, 0);
        self.window.hide();
    }

    pub fn next_due(&self) -> Option<Instant> {
        self.due.as_ref().map(|d| d.0)
    }

    pub fn tick(&mut self) {
        if let Some((when, text, x, y)) = self.due.take() {
            if Instant::now() >= when {
                self.show(&text, x, y);
            } else {
                self.due = Some((when, text, x, y));
            }
        }
    }

    fn show(&mut self, text: &str, x: i32, y: i32) {
        let s = win::scale_at(x, y);
        // o título (até a primeira linha vazia, ou só a primeira linha) vai em negrito
        let (head, tail) = match text.split_once("\n\n") {
            Some((h, t)) => (h.to_string(), t.to_string()),
            None => match text.split_once('\n') {
                Some((h, t)) => (h.to_string(), t.to_string()),
                None => (text.to_string(), String::new()),
            },
        };
        let mut lines: Vec<(String, bool)> = head.lines().map(|l| (l.to_string(), true)).collect();
        if !tail.is_empty() {
            lines.extend(tail.lines().map(|l| (l.to_string(), false)));
        }
        let probe = Canvas::new(1, 1, s);
        let width = lines
            .iter()
            .map(|(l, bold)| probe.text_width(l, if *bold { Face::Semibold } else { Face::Regular }, if *bold { 11.5 } else { 11.0 }))
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
            let (face, size, color) = if *bold { (Face::Semibold, 11.5, FG) } else { (Face::Regular, 11.0, MUTED) };
            c.text(pad_x, pad_y + i as f32 * line_h, line, face, size, color, Anchor::LA, None);
        }
        let img = c.finish();
        // acima do mouse, centralizada; embaixo se não couber
        let areas = win::work_areas();
        let area = areas.iter().copied().find(|a| x >= a.0 && x < a.2 && y >= a.1 && y < a.3).unwrap_or(areas[0]);
        let px = (x - pw as i32 / 2).max(area.0 + 4).min(area.2 - pw as i32 - 4);
        let mut py = y - ph as i32 - (14.0 * s) as i32;
        if py < area.1 {
            py = y + (22.0 * s) as i32;
        }
        self.window.show(&img, (px, py));
    }
}
