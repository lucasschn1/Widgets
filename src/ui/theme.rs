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
    /// Esta cor (com a transparência dela) pintada por cima de `back`: a cor opaca que se vê.
    pub fn over(self, back: Rgba) -> Rgba {
        let a = self.3 as f32 / 255.0;
        let channel = |front: u8, behind: u8| (front as f32 * a + behind as f32 * (1.0 - a)).round() as u8;
        Rgba::rgb(channel(self.0, back.0), channel(self.1, back.1), channel(self.2, back.2))
    }
    /// A luminância relativa (0 a 1), como a WCAG define: quanto a cor (opaca) parece clara.
    pub fn luminance(self) -> f32 {
        let channel = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(self.0) + 0.7152 * channel(self.1) + 0.0722 * channel(self.2)
    }
}

/// O contraste entre duas cores opacas, de 1 (iguais) a 21 (preto e branco), como a WCAG define.
pub fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

pub const WHITE: Rgba = Rgba(255, 255, 255, 255);
pub const FG: Rgba = Rgba(250, 250, 252, 255);
/// O texto secundário e o apagado: brancos translúcidos. Mais fracos que isso, sobre o vidro
/// (que mostra o papel de parede borrado atrás), as letras pareciam fora de foco.
pub const MUTED: Rgba = Rgba(255, 255, 255, 190);
pub const FAINT: Rgba = Rgba(255, 255, 255, 128);
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
/// O fio de luz logo abaixo da borda, no topo (some até a base).
pub const EDGE_GLOW: f32 = 0.05;
/// O vidro claro (tema claro do Windows): o papel de parede desfocado clareado para este branco
/// levemente frio, como o dos widgets do macOS no modo claro.
pub const LIGHT_TINT: (u8, u8, u8) = (246, 246, 250);

// o vidro líquido, como o do macOS 26: mais transparente, a beirada curvando o fundo como uma
// lente e a luz batendo nas quinas
/// Escurece (ou clareia) só esta parte do escolhido, deixando o papel de parede aparecer mais; o
/// contraste mínimo do texto continua valendo.
pub const LIQUID_TINT: f64 = 0.55;
/// A borda: mais forte no topo, como a luz batendo na beirada.
pub const EDGE_LIQUID: (f32, f32) = (0.34, 0.10);
/// O brilho suave vindo do canto de cima à esquerda, por baixo do conteúdo (opacidade no centro).
pub const LIQUID_SHINE: f32 = 0.16;
/// Os reflexos na borda: forte na quina de cima à esquerda, de onde vem a luz, e mais fraco na de
/// baixo à direita, onde ela sai do outro lado do vidro (opacidade no centro de cada um).
pub const SPECULAR: (f32, f32) = (0.9, 0.45);
/// A espessura: a parte de baixo do vidro um pouco mais escura, por dentro (opacidade na base).
pub const LIQUID_DEPTH: f32 = 0.14;
/// A lente da beirada: a largura da faixa em que o fundo se curva e quanto ele é puxado de dentro
/// na borda (pontos), e quanto o vermelho e o azul se separam do verde (a franja de cor).
pub const REFRACT_BAND: f32 = 24.0;
pub const REFRACT_PUSH: f32 = 20.0;
pub const REFRACT_SPREAD: f32 = 0.3;
/// A luz presa na espessura do vidro: a faixa clara logo dentro da borda, com esta largura (em
/// linhas de 1 pixel), desenhada em tantos passos e com esta opacidade junto da borda.
pub const RIM_WIDTH: f32 = 5.0;
pub const RIM_STEPS: usize = 5;
pub const RIM_LIGHT: f32 = 0.07;

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
        // abrir o player (a música da ilha, com ele fechado)
        "abrir" => "\u{e8a7}",
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
        "brilho" => "\u{e706}",
        "fonte" => "\u{e8d2}",
        "foto" => "\u{e91b}",
        "pessoa" => "\u{e77b}",
        "link" => "\u{e71b}",
        _ => "\u{e946}",
    }
}
