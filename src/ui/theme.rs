//! Medidas, cores e tempos compartilhados por todo o visual (os mesmos da versão em Python).

/// Cor com alfa "reto" (não pré-multiplicado): (r, g, b, a).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    /// Uma cor opaca.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba(r, g, b, 255)
    }
    /// A mesma cor com outra opacidade (0 a 1): deixa o vidro aparecer por trás.
    pub fn alpha(self, amount: f32) -> Rgba {
        Rgba(self.0, self.1, self.2, (255.0 * amount).round().clamp(0.0, 255.0) as u8)
    }
    /// A mesma cor com o alfa `alpha` (0 a 255).
    pub fn with_a(self, alpha: u8) -> Rgba {
        Rgba(self.0, self.1, self.2, alpha)
    }
    /// Mistura linear entre duas cores (t de 0 a 1).
    pub fn mix(self, other: Rgba, t: f32) -> Rgba {
        let channel = |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * t).round() as u8;
        Rgba(channel(self.0, other.0), channel(self.1, other.1), channel(self.2, other.2), channel(self.3, other.3))
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

// as cores de estado, as mesmas em todos os widgets: um pouco menos saturadas que as do sistema
// (o verde, o vermelho e o azul puros gritavam no vidro escuro). As de marca (Spotify, YouTube,
// GitHub, Claude) ficam como são.
pub const GREEN: Rgba = Rgba::rgb(88, 204, 130);
pub const AMBER: Rgba = Rgba::rgb(240, 184, 84);
pub const RED: Rgba = Rgba::rgb(242, 102, 92);
pub const BLUE: Rgba = Rgba::rgb(82, 158, 245);
pub const PINK: Rgba = Rgba::rgb(242, 96, 128);
pub const CYAN: Rgba = Rgba::rgb(112, 200, 242);

// medidas em pixels "lógicos" (96 dpi); multiplicadas pela escala da tela
pub const MARGIN: f32 = 8.0; // área transparente em volta do cartão, para o botão sobressair
pub const BADGE_D: f32 = 20.0;
pub const ANIM_MS: u64 = 300;
pub const GROW_MS: u64 = 420; // widget abrindo ou fechando detalhes
pub const BADGE_FADE_MS: u64 = 120;
pub const FRAME_MS: u64 = 16;
pub const WIDGET_GAP: f32 = 12.0; // espaço entre widgets encaixados
pub const EDGE_GAP: f32 = 24.0; // espaço até a borda da tela, ao encaixar nela
pub const SNAP_DIST: f32 = 12.0; // a que distância o encaixe "puxa" o widget
pub const DRAG_START: f32 = 4.0; // quanto o mouse anda para um clique virar arraste
pub const TIP_DELAY_MS: u64 = 700; // sem pipocar dicas enquanto o mouse só passa
pub const PEEK_IDLE_MS: u64 = 5000;
pub const HOVER_COLLAPSE_MS: u64 = 350; // e fecha pouco depois de o mouse sair
pub const REORDER_MS: u64 = 180;

// vidro: o papel de parede atrás do cartão, desfocado, mais saturado e escurecido
pub const BLUR: f64 = 12.0;
pub const SATURATION: f32 = 1.6;
pub const TINT: (u8, u8, u8) = (18, 18, 24);
pub const TINT_AMOUNT: f64 = 0.57;
pub const SHEEN: f32 = 0.12; // reflexo branco no topo do vidro
pub const EDGE: (f32, f32) = (0.42, 0.07); // opacidade da borda clara, no topo e na base

/// Desaceleração com um leve passo além do ponto, voltando (a "mola" das animações).
pub fn ease_spring(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let (c1, c3) = (0.9, 1.9);
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

// o que está sob o mouse fica só um pouco mais claro que o vidro, com um fio de contorno; o que
// está sendo apertado escurece um pouco
pub const HOVER_SURFACE: Rgba = Rgba(255, 255, 255, 20);
pub const HOVER_EDGE: Rgba = Rgba(255, 255, 255, 40);
pub const PRESS_SURFACE: Rgba = Rgba(0, 0, 0, 34);
pub const HOVER_FADE_MS: u64 = 120;
pub const STALE_DOT: Rgba = AMBER; // aviso de dados desatualizados
pub const STALE_XY: (f32, f32) = (12.0, 12.0);

/// Movimento que sai devagar, acelera e assenta suave no ponto, sem passar dele (uma mola
/// "criticamente amortecida"): para voos longos, em que o passo além do ponto pareceria um tranco.
pub fn ease_settle(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // a rigidez da mola (quanto maior, mais cedo assenta)
    const STIFFNESS: f32 = 8.0;
    let settled = |t: f32| 1.0 - (1.0 + STIFFNESS * t) * (-STIFFNESS * t).exp();
    settled(t) / settled(1.0)
}

/// Começa devagar, acelera no meio e chega devagar (cúbica).
pub fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Códigos dos ícones nas fontes de ícones do Windows (Segoe Fluent Icons / MDL2 Assets).
pub fn icon(name: &str) -> &'static str {
    match name {
        "geral" => "\u{e713}",
        "ilha" => "\u{e7c4}",
        "aparencia" => "\u{e790}",
        "widgets" => "\u{eca5}",
        "clima" => "\u{e753}",
        "sistema" => "\u{e9d9}",
        "midia" => "\u{e8d6}",
        "github" => "\u{e943}",
        "pomodoro" => "\u{e916}",
        "claude" => "\u{e734}",
        "bateria" => "\u{e83f}",
        "carregando" => "\u{e945}",
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
        "mover" => "\u{e7c2}",
        "tamanho" => "\u{e740}",
        "mouse" => "\u{e962}",
        "menu" => "\u{e712}",
        "organizar" => "\u{e8a9}",
        "layouts" => "\u{f246}",
        "frente" => "\u{e7c4}",
        "foco" => "\u{e708}",
        "lock" => "\u{e72e}",
        "dicas" => "\u{e82f}",
        "sair" => "\u{e7e8}",
        "pilha" => "\u{e8a7}",
        "play" => "\u{e768}",
        "pause" => "\u{e769}",
        "next" => "\u{e893}",
        "fones" => "\u{e7f6}",
        "teclado" => "\u{e765}",
        "controle" => "\u{e7fc}",
        "celular" => "\u{e8ea}",
        "caneta" => "\u{edc6}",
        "notebook" => "\u{e7f8}",
        "caixa" => "\u{e7f5}",
        _ => "\u{e946}",
    }
}
