//! Estado que todos os widgets consultam: configuração, cache, o vidro e as preferências.

use serde_json::Value;

use crate::backdrop::Backdrop;
use crate::config::{self, Get};
use crate::win;

pub struct Shared {
    pub cfg: Value,
    pub cache: Value,
    pub backdrop: Backdrop,
    pub text_scale: f32,
    pub animations: bool,
    pub idle_opacity: f32,
    pub tint: f64,
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
        let mut sh = Shared {
            backdrop: Backdrop::new(blur, s),
            cfg,
            cache,
            text_scale: 1.0,
            animations: true,
            idle_opacity: 1.0,
            tint: 0.4,
            peeking: false,
            editing: false,
            paused: Vec::new(),
            focus: false,
            saving: false,
        };
        sh.read_preferences();
        sh
    }

    /// Não vale a pena redesenhar o widget nesse retângulo: modo foco, ou tela cheia no monitor dele.
    pub fn suspended_at(&self, rect: win::Rect) -> bool {
        let (cx, cy) = ((rect.0 + rect.2) / 2, (rect.1 + rect.3) / 2);
        self.focus || self.paused.iter().any(|r| cx >= r.0 && cx < r.2 && cy >= r.1 && cy < r.3)
    }

    pub fn locked(&self) -> bool {
        self.cfg["app"].bool_of("travado").unwrap_or(false)
    }

    /// Valores que dependem das configurações e do Windows (texto, animações, opacidade).
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
        self.idle_opacity =
            if config::setting_bool(&self.cfg, "esmaecer") { config::setting_f64(&self.cfg, "opacidade_ociosa") as f32 } else { 1.0 };
        self.tint = config::setting_f64(&self.cfg, "escurecimento");
    }

    pub fn save(&self) {
        config::save_config(&self.cfg);
    }

    pub fn save_cache(&self) {
        config::save_cache(&self.cache);
    }
}
