//! Peças que os widgets usam para se desenhar na ilha (Pomodoro, Sistema, GitHub, Bateria): os
//! botões, o cabeçalho da ilha aberta, as barras e as medidas de cada forma, para todos ficarem
//! com a mesma cara da ilha do Tocando agora.

use crate::gfx::{Anchor, Canvas, Face};
use crate::ui::theme::{icon, Rgba, FG, MUTED, WHITE};

use super::{IslandForm, Rect4};

/// Texto apagado da ilha (sobre o preto, ainda legível).
pub const SOFT: Rgba = Rgba(255, 255, 255, 150);
/// Fundo dos botões redondos e das pílulas secundárias.
const BUTTON: Rgba = Rgba(255, 255, 255, 34);
const BUTTON_HOT: Rgba = Rgba(255, 255, 255, 58);

/// O tamanho de uma forma da ilha, em pixels lógicos.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormSize {
    pub w: f32,
    pub h: f32,
    /// o raio dos cantos (metade da altura na pílula; na bolha, um círculo)
    pub radius: f32,
}

const fn form_size(w: f32, h: f32, radius: f32) -> FormSize {
    FormSize { w, h, radius }
}

/// O tamanho de cada forma (a bolha é um círculo).
pub fn size(form: IslandForm) -> FormSize {
    match form {
        IslandForm::Bubble => form_size(42.0, 42.0, 21.0),
        IslandForm::Compact => form_size(236.0, 42.0, 21.0),
        IslandForm::Banner => form_size(300.0, 42.0, 21.0),
        IslandForm::Medium => form_size(340.0, 78.0, 30.0),
        IslandForm::Large => form_size(364.0, 220.0, 38.0),
    }
}

/// O ponto (x, y) está dentro do retângulo (x0, y0, x1, y1)?
pub fn inside(rect: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= rect.0 && x <= rect.2 && y >= rect.1 && y <= rect.3
}

/// Botão redondo com um ícone (das fontes de ícones do Windows).
pub fn round_button(c: &mut Canvas, cx: f32, cy: f32, r: f32, glyph: &str, hot: bool) {
    c.fill_circle(cx, cy, r, if hot { BUTTON_HOT } else { BUTTON });
    c.text(cx, cy, icon(glyph), Face::Icons, r * 0.7, FG, Anchor::MM, None);
}

/// Pílula com texto no retângulo `rect` (x0, y0, x1, y1): `primary` é a clara (a ação principal),
/// as outras são de vidro.
pub fn pill_button(c: &mut Canvas, rect: (f32, f32, f32, f32), label: &str, primary: bool, hot: bool) {
    let (fill, text) = match (primary, hot) {
        (true, false) => (Rgba::rgb(245, 245, 247), Rgba::rgb(18, 18, 20)),
        (true, true) => (WHITE, Rgba::rgb(18, 18, 20)),
        (false, false) => (BUTTON, FG),
        (false, true) => (BUTTON_HOT, FG),
    };
    let (x0, y0, x1, y1) = rect;
    c.fill_round_rect(x0, y0, x1, y1, (y1 - y0) / 2.0, fill);
    c.text((x0 + x1) / 2.0, (y0 + y1) / 2.0, label, Face::Semibold, 12.0, text, Anchor::MM, Some(x1 - x0 - 8.0));
}

/// Retângulo (x0, y0, x1, y1) de um círculo.
pub fn circle(cx: f32, cy: f32, r: f32) -> (f32, f32, f32, f32) {
    (cx - r, cy - r, cx + r, cy + r)
}

/// Texto que é um link (sublinhado só com o mouse em cima). Devolve o retângulo.
pub fn link(c: &mut Canvas, x: f32, y: f32, label: &str, anchor_right: bool, hot: bool) -> (f32, f32, f32, f32) {
    let w = c.text_width(label, Face::Regular, 12.0);
    let x0 = if anchor_right { x - w } else { x };
    c.text(x0, y, label, Face::Regular, 12.0, if hot { FG } else { SOFT }, Anchor::LM, None);
    if hot {
        c.hline_px(x0, x0 + w, y + 8.0, SOFT);
    }
    (x0 - 4.0, y - 10.0, x0 + w + 4.0, y + 10.0)
}

/// A linha de cima da ilha aberta: o nome do widget na cor dele, e um texto à direita.
pub fn header(c: &mut Canvas, r: Rect4, label: &str, color: Rgba, right: &str, right_color: Rgba) {
    let (x, y, w, _) = r;
    c.text(x + 22.0, y + 24.0, label, Face::Semibold, 10.5, color, Anchor::LM, None);
    c.text(x + w - 22.0, y + 24.0, right, Face::Regular, 11.5, right_color, Anchor::RM, Some(w - 120.0));
}

/// Barra fina de progresso.
pub fn bar(c: &mut Canvas, x0: f32, x1: f32, cy: f32, frac: f32, color: Rgba) {
    c.fill_round_rect(x0, cy - 2.5, x1, cy + 2.5, 2.5, Rgba(255, 255, 255, 26));
    let filled = frac.clamp(0.0, 1.0);
    if filled > 0.0 {
        c.fill_round_rect(x0, cy - 2.5, x0 + ((x1 - x0) * filled).max(5.0), cy + 2.5, 2.5, color);
    }
}

/// O círculo colorido com um ícone, à esquerda da forma média.
pub fn badge_icon(c: &mut Canvas, cx: f32, cy: f32, r: f32, glyph: &str, color: Rgba) {
    c.fill_circle(cx, cy, r, color.alpha(0.18));
    c.text(cx, cy, icon(glyph), Face::Icons, r * 0.82, color, Anchor::MM, None);
}

/// As duas linhas de texto da forma média (título forte, linha de baixo na cor dada).
pub fn two_lines(c: &mut Canvas, x: f32, cy: f32, max_w: f32, title: &str, sub: &str, sub_color: Rgba) {
    c.text(x, cy - 2.0, title, Face::Semibold, 13.5, FG, Anchor::LS, Some(max_w));
    c.text(x, cy + 3.0, sub, Face::Regular, 11.5, sub_color, Anchor::LA, Some(max_w));
}

/// O primeiro de `options` (do mais completo ao mais curto) que cabe inteiro em `max_w`, para a
/// ilha nunca mostrar um texto cortado com "…"; nenhum cabendo, o último (o mais curto).
pub fn fitting<'a>(c: &Canvas, options: &[&'a str], face: Face, size: f32, max_w: f32) -> &'a str {
    options.iter().copied().find(|text| c.text_width(text, face, size) <= max_w).or(options.last().copied()).unwrap_or("")
}

/// Quanto sobra para o nome na forma fechada (`compact_text`), com `value` forte à direita e o
/// nome começando em `x`.
pub fn compact_room(c: &Canvas, x: f32, r: Rect4, value: &str) -> f32 {
    let (left, _, w, _) = r;
    let vw = if value.is_empty() { 0.0 } else { c.text_width(value, Face::Semibold, 14.5) };
    left + w - 16.0 - vw - 10.0 - x
}

/// Texto da forma fechada (o nome, apagado) e o valor forte à direita.
pub fn compact_text(c: &mut Canvas, x: f32, r: Rect4, label: &str, value: &str, value_color: Rgba) {
    let (left, top, w, h) = r;
    let cy = top + h / 2.0;
    let vw = c.text(left + w - 16.0, cy, value, Face::Semibold, 14.5, value_color, Anchor::RM, None);
    c.text(x, cy, label, Face::Regular, 12.0, MUTED, Anchor::LM, Some((left + w - 16.0 - vw - 10.0 - x).max(10.0)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_grow_in_order() {
        let order = [IslandForm::Compact, IslandForm::Banner, IslandForm::Medium, IslandForm::Large];
        for pair in order.windows(2) {
            let (smaller, bigger) = (size(pair[0]), size(pair[1]));
            assert!(bigger.w > smaller.w && bigger.h >= smaller.h, "{pair:?}");
        }
        assert_eq!(size(IslandForm::Bubble).h, size(IslandForm::Compact).h, "a bolha tem a altura da pílula fechada");
    }
}
