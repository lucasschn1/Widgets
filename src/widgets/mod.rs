//! Os widgets. Cada um implementa `Kind`: o que ele desenha em cada tamanho, o que acontece nos
//! cliques e como se atualiza. Arrastar, encaixar, mudar de tamanho e o vidro ficam no `Card`.
//!
//! Para criar um widget novo: um módulo aqui com um tipo que implemente `Kind`, e uma linha em
//! `create` e em `KEYS`.

pub mod pomodoro;
pub mod system;

use std::time::Instant;

use serde_json::Value;

use crate::config::{self, Get};
use crate::gfx::Canvas;

/// Os widgets disponíveis, na ordem dos menus e das configurações.
pub const KEYS: &[&str] = &["sistema", "midia", "github", "pomodoro"];

/// Cria o widget `key` (None para os que ainda não existem nesta versão).
pub fn create(key: &str, cfg: &Value, cache: &Value) -> Option<Box<dyn Kind>> {
    match key {
        "sistema" => Some(Box::new(system::System::new(cfg, cache))),
        "pomodoro" => Some(Box::new(pomodoro::Pomodoro::new(cfg))),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Mode {
    P,
    M,
    G,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::P, Mode::M, Mode::G];

    pub fn code(self) -> &'static str {
        match self {
            Mode::P => "p",
            Mode::M => "m",
            Mode::G => "g",
        }
    }

    pub fn from_code(s: &str) -> Option<Mode> {
        match s {
            "p" => Some(Mode::P),
            "m" => Some(Mode::M),
            "g" => Some(Mode::G),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::P => "Pequeno",
            Mode::M => "Médio",
            Mode::G => "Grande",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub w: f32,
    pub h: f32,
    pub radius: f32,
}

/// Área a destacar sob o mouse: (x0, y0, x1, y1, raio), em unidades lógicas do cartão.
pub type Hot = (f32, f32, f32, f32, f32);

/// O que o desenho e os cliques podem consultar.
pub struct Env<'a> {
    pub cfg: &'a Value,
    pub key: &'static str,
}

impl Env<'_> {
    /// A seção do widget no config.json.
    pub fn wcfg(&self) -> &Value {
        &self.cfg[self.key]
    }
    /// "Cliques abrem programas e sites".
    pub fn shortcuts(&self) -> bool {
        config::setting_bool(self.cfg, "atalhos_clique")
    }
    pub fn tips(&self) -> bool {
        config::setting_bool(self.cfg, "dicas")
    }
}

/// Pedidos de um widget para o app (gravar, avisar, abrir algo...).
pub enum Request {
    SaveConfig,
    SaveCache,
    /// notificação do Windows: (tipo, título, texto, ação ao clicar)
    Notify(&'static str, String, String, Option<NotifyAction>),
    Open(String),
    Redraw,
}

#[derive(Clone, Debug)]
pub enum NotifyAction {
    Open(String),
    /// chama `act` do widget com esta ação
    Widget(&'static str, u32),
}

/// O que um widget pode mexer ao agir: a configuração, o cache e a lista de pedidos.
pub struct Ctx<'a> {
    pub cfg: &'a mut Value,
    pub cache: &'a mut Value,
    pub key: &'static str,
    pub out: &'a mut Vec<Request>,
}

impl Ctx<'_> {
    pub fn wcfg(&mut self) -> &mut serde_json::Map<String, Value> {
        self.cfg.obj_mut(self.key)
    }
    pub fn save(&mut self) {
        self.out.push(Request::SaveConfig);
    }
    pub fn notify(&mut self, kind: &'static str, title: impl Into<String>, text: impl Into<String>, action: Option<NotifyAction>) {
        self.out.push(Request::Notify(kind, title.into(), text.into(), action));
    }
    pub fn open(&mut self, target: impl Into<String>) {
        self.out.push(Request::Open(target.into()));
    }
    pub fn env(&self) -> Env<'_> {
        Env { cfg: self.cfg, key: self.key }
    }
}

/// Estado mostrado pela base do cartão: buscando dados (barra no topo) e erro (ponto no canto).
#[derive(Default, Clone)]
pub struct Status {
    pub loading: bool,
    pub error: Option<String>,
    /// quando os dados foram atualizados (segundos desde 1970)
    pub updated: Option<f64>,
}

/// Item de menu (o mesmo formato serve para o menu de vidro).
#[derive(Clone)]
pub enum MenuItem {
    Cmd(String, u32),
    Check(String, bool, u32),
    Radio(String, bool, u32),
    Sub(String, Vec<MenuItem>),
    Sep,
}

pub trait Kind {
    fn key(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn sizes(&self) -> &'static [Mode] {
        &Mode::ALL
    }
    fn default_mode(&self) -> Mode {
        Mode::G
    }
    fn layout(&self, mode: Mode) -> Layout;
    /// Desenha o conteúdo (o vidro e os detalhes da base são feitos pelo `Card`).
    fn draw(&self, c: &mut Canvas, mode: Mode, env: &Env);
    /// Ação (número) da área clicável em (x, y).
    fn hit(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> Option<u32> {
        None
    }
    fn act(&mut self, _action: u32, _ctx: &mut Ctx) {}
    fn hot_rect(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> Option<Hot> {
        None
    }
    fn tooltip(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> Option<String> {
        None
    }
    fn wheel(&mut self, _x: f32, _y: f32, _steps: f32, _mode: Mode, _ctx: &mut Ctx) {}
    fn menu(&self, _env: &Env) -> Vec<MenuItem> {
        Vec::new()
    }
    /// Quando o widget quer `tick` de novo (None: não precisa).
    fn next_tick(&self) -> Option<Instant> {
        None
    }
    /// Atualiza (relógio, dados chegando...); devolve true se precisa redesenhar.
    fn tick(&mut self, _ctx: &mut Ctx) -> bool {
        false
    }
    fn status(&self) -> Status {
        Status::default()
    }
    /// Clique no aviso de erro: tenta de novo.
    fn retry(&mut self, _ctx: &mut Ctx) {}
    /// Logo depois de criado (começar a buscar dados etc.).
    fn start(&mut self, _ctx: &mut Ctx) {}
}

/// Número no config: aceita inteiro ou fracionário.
pub fn num(v: &Value, key: &str) -> Option<f64> {
    v.f64_of(key)
}
