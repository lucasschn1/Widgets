//! Estado que todos os widgets consultam: configuração, cache, o vidro e as preferências.

use serde_json::Value;

use crate::card::backdrop::{Backdrop, GlassLook};
use crate::gfx::wallcolor::WallColor;
use crate::data::config::{self, Get};
use crate::platform::win;

/// A opção "Cor do papel de parede" (Aparência), em cfg["app"].
pub const WALL_COLOR: &str = "cor_do_papel_de_parede";
/// A opção "Tema claro do Windows" (Aparência): com o Windows no tema claro, os widgets ficam
/// claros também (vidro claro, texto escuro), como no macOS.
pub const LIGHT_THEME: &str = "tema_claro";

/// O que o App e todos os cartões dividem.
pub struct Shared {
    pub cfg: Value,
    pub cache: Value,
    pub backdrop: Backdrop,
    pub text_scale: f32,
    pub animations: bool,
    pub tint: f64,
    /// os widgets na cor do papel de parede (Aparência → "Cor do papel de parede")
    pub wall_color: bool,
    /// o tema claro: a opção ligada e o Windows no tema claro
    pub light: bool,
    /// widgets mostrados na frente das janelas
    pub peeking: bool,
    /// modo edição: fechar, redimensionar e arrastar à vontade
    pub editing: bool,
    /// monitores com um programa em tela cheia: os widgets neles ficam pausados
    pub paused: Vec<win::Rect>,
    /// modo foco: widgets escondidos
    pub focus: bool,
    /// a economia de energia do Windows está ligada (e a opção também): pausa as animações contínuas
    pub saving: bool,
}

impl Shared {
    pub fn new(cfg: Value, cache: Value, s: f32) -> Shared {
        let blur = config::setting_f64(&cfg, "desfoque");
        let mut shared = Shared {
            backdrop: Backdrop::new(blur, s),
            cfg,
            cache,
            text_scale: 1.0,
            animations: true,
            tint: 0.4,
            wall_color: false,
            light: false,
            peeking: false,
            editing: false,
            paused: Vec::new(),
            focus: false,
            saving: false,
        };
        shared.read_preferences();
        shared
    }

    /// Não vale a pena redesenhar o widget nesse retângulo: modo foco, ou tela cheia no monitor dele.
    pub fn suspended_at(&self, rect: win::Rect) -> bool {
        let (cx, cy) = ((rect.0 + rect.2) / 2, (rect.1 + rect.3) / 2);
        self.focus || self.paused.iter().any(|screen| cx >= screen.0 && cx < screen.2 && cy >= screen.1 && cy < screen.3)
    }

    /// "Travar widgets": nada se arrasta nem muda de tamanho.
    pub fn locked(&self) -> bool {
        self.cfg["app"].bool_of("travado").unwrap_or(false)
    }

    /// Valores que dependem das configurações e do Windows (texto, animações, vidro).
    pub fn read_preferences(&mut self) {
        let mut text = config::setting_f64(&self.cfg, "texto");
        if text <= 0.0 {
            // segue o "Tamanho do texto" da acessibilidade do Windows, até 120%
            text = win::registry(r"Software\Microsoft\Accessibility", "TextScaleFactor").parse::<f64>().unwrap_or(100.0) / 100.0;
        }
        self.text_scale = text.clamp(0.9, 1.2) as f32;
        self.animations = match config::setting_str(&self.cfg, "animacoes").as_str() {
            "sempre" => true,
            "nunca" => false,
            _ => win::animations_enabled(),
        };
        self.tint = config::setting_f64(&self.cfg, "escurecimento");
        self.wall_color = config::setting_bool(&self.cfg, WALL_COLOR);
        self.light = config::setting_bool(&self.cfg, LIGHT_THEME) && win::light_theme();
    }

    /// O vidro dos widgets agora (claro ou escuro). `own_background`: o widget pinta o próprio
    /// fundo (o céu do Tempo, a cor da capa na Mídia) e fica com o vidro escuro também no tema
    /// claro, com o texto branco que combina com esse fundo.
    pub fn glass_look(&self, own_background: bool) -> GlassLook {
        GlassLook { light: self.light && !own_background }
    }

    /// A cor do papel de parede para um widget nesse retângulo da tela (a do monitor dele), ou
    /// None com a opção desligada.
    pub fn wall_color_at(&self, rect: win::Rect) -> Option<WallColor> {
        self.wall_color.then(|| self.backdrop.wall_color_at((rect.0 + rect.2) / 2, (rect.1 + rect.3) / 2))
    }

    /// A cor do papel de parede para um widget desenhado fora da área de trabalho (as miniaturas e
    /// a prévia das configurações): a do monitor principal.
    pub fn wall_color_main(&self) -> Option<WallColor> {
        self.wall_color.then(|| self.backdrop.wall_color_at(0, 0))
    }

    /// Grava as opções.
    pub fn save(&self) {
        config::save_config(&self.cfg);
    }

    /// Grava o cache.
    pub fn save_cache(&self) {
        config::save_cache(&self.cache);
    }
}
