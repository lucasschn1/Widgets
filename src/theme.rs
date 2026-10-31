//! Medidas, cores e tempos compartilhados por todo o visual (os mesmos da versão em Python).

/// Cor com alfa "reto" (não pré-multiplicado): (r, g, b, a).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba(r, g, b, 255)
    }
    /// A mesma cor com outra opacidade (0 a 1): deixa o vidro aparecer por trás.
    pub fn alpha(self, amount: f32) -> Rgba {
        Rgba(self.0, self.1, self.2, (255.0 * amount).round().clamp(0.0, 255.0) as u8)
    }
    pub fn with_a(self, a: u8) -> Rgba {
        Rgba(self.0, self.1, self.2, a)
    }
    /// Mistura linear entre duas cores (t de 0 a 1).
    pub fn mix(self, other: Rgba, t: f32) -> Rgba {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgba(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2), m(self.3, other.3))
    }
}

pub const WHITE: Rgba = Rgba(255, 255, 255, 255);
pub const FG: Rgba = Rgba(250, 250, 252, 255);
pub const MUTED: Rgba = Rgba(255, 255, 255, 170);
pub const FAINT: Rgba = Rgba(255, 255, 255, 110);
pub const DIVIDER: Rgba = Rgba(255, 255, 255, 41); // branco a 16%
pub const ACCENT: Rgba = Rgba::rgb(10, 132, 255);
pub const ACCENT_HOVER: Rgba = Rgba::rgb(64, 156, 255);
pub const DANGER: Rgba = Rgba::rgb(255, 69, 58);
pub const DARK_GLYPH: Rgba = Rgba(28, 28, 30, 255);

// medidas em pixels "lógicos" (96 dpi); multiplicadas pela escala da tela
pub const MARGIN: f32 = 8.0; // área transparente em volta do cartão, para o botão sobressair
pub const BADGE_D: f32 = 20.0;
pub const ANIM_MS: u64 = 300;
pub const BADGE_FADE_MS: u64 = 120;
pub const FRAME_MS: u64 = 16;
pub const WIDGET_GAP: f32 = 12.0; // espaço entre widgets encaixados
pub const EDGE_GAP: f32 = 24.0; // espaço até a borda da tela, ao encaixar nela
pub const SNAP_DIST: f32 = 12.0; // a que distância o encaixe "puxa" o widget
pub const DRAG_START: f32 = 4.0; // quanto o mouse anda para um clique virar arraste
pub const TIP_DELAY_MS: u64 = 450;
pub const PEEK_IDLE_MS: u64 = 5000;
pub const HOVER_EXPAND_MS: u64 = 650; // mouse parado sobre um widget pequeno: ele abre
pub const HOVER_COLLAPSE_MS: u64 = 350; // e fecha pouco depois de o mouse sair
pub const REORDER_MS: u64 = 180;

// vidro: o papel de parede atrás do cartão, desfocado, mais saturado e escurecido
pub const BLUR: f64 = 26.0;
pub const SATURATION: f32 = 1.6;
pub const TINT: (u8, u8, u8) = (18, 18, 24);
pub const TINT_AMOUNT: f64 = 0.40;
pub const SHEEN: f32 = 0.10; // reflexo branco no topo do vidro
pub const EDGE: (f32, f32) = (0.45, 0.10); // opacidade da borda clara, no topo e na base

pub const HOT_FILL: Rgba = Rgba(255, 255, 255, 42); // destaque do que está sob o mouse
pub const PRESS_FILL: Rgba = Rgba(0, 0, 0, 80); // o que está sendo apertado "afunda"
pub const STALE_DOT: Rgba = Rgba(255, 184, 0, 255); // aviso de dados desatualizados
pub const STALE_XY: (f32, f32) = (12.0, 12.0);

pub fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// "1,5" (número com vírgula, como no Brasil).
pub fn br(n: f64, decimals: usize) -> String {
    format!("{n:.decimals$}").replace('.', ",")
}

/// Códigos dos ícones nas fontes de ícones do Windows (Segoe Fluent Icons / MDL2 Assets).
pub fn icon(name: &str) -> &'static str {
    match name {
        "geral" => "\u{e713}",
        "aparencia" => "\u{e790}",
        "widgets" => "\u{eca5}",
        "clima" => "\u{e753}",
        "midia" => "\u{e8d6}",
        "github" => "\u{e943}",
        "pomodoro" => "\u{e916}",
        "avisos" => "\u{ea8f}",
        "atalhos" => "\u{e765}",
        "sobre" => "\u{e946}",
        "add" => "\u{e710}",
        "check" => "\u{e73e}",
        "close" => "\u{e711}",
        "edit" => "\u{e70f}",
        "folder" => "\u{e8b7}",
        "refresh" => "\u{e72c}",
        "search" => "\u{e721}",
        "undo" => "\u{e7a7}",
        "pin" => "\u{e718}",
        "location" => "\u{e81d}",
        _ => "\u{e946}",
    }
}
