//! Os widgets. Cada um implementa `Kind`: o que ele desenha em cada tamanho, o que acontece nos
//! cliques e como se atualiza. Arrastar, encaixar, mudar de tamanho e o vidro ficam no `Card`.
//!
//! Para criar um widget novo: um módulo aqui com um tipo que implemente `Kind`, e uma linha em
//! `create` e em `KEYS`.

pub mod claude;
pub mod clawd;
pub mod github;
pub mod media;
pub mod pomodoro;
pub mod system;
pub mod tempo;

use std::time::Instant;

use serde_json::Value;

use crate::config::{self, Get};
use crate::gfx::Canvas;

// ---------- medidas dos três tamanhos (as mesmas em todos os widgets) ----------
//
// Cada widget desenha o conteúdo dentro destas medidas; elas nunca mudam com o que ele mostra.
//
// O pequeno é um quadrado; o médio tem a mesma altura e a largura de dois pequenos lado a lado
// (com o espaço do encaixe entre eles), como nos widgets do Mac; o grande tem a mesma largura do
// médio e altura para as listas (dias, processos, atividades) e a capa grande.

/// Lado do tamanho pequeno (um quadrado).
pub const SMALL: f32 = 154.0;
/// Largura do médio e do grande: dois pequenos e o espaço do encaixe.
pub const WIDE: f32 = 2.0 * SMALL + crate::theme::WIDGET_GAP;
pub const MEDIUM_H: f32 = SMALL;
pub const LARGE_H: f32 = 448.0;
/// cantos de cada tamanho
pub const SMALL_RADIUS: f32 = 24.0;
pub const MEDIUM_RADIUS: f32 = 26.0;
pub const LARGE_RADIUS: f32 = 28.0;
/// Altura da antiga "pílula" pequena (até a versão 3.1), para acertar os layouts.
pub const OLD_SMALL_H: f32 = 64.0;

/// Tamanho com que um widget aparece pela primeira vez.
pub const DEFAULT_MODE: Mode = Mode::M;

/// Arrumação da primeira vez: colunas a partir da borda direita da tela, cada uma de cima para
/// baixo. Um widget aberto depois vai para baixo dos da coluna dele (ou começa a coluna dele).
pub const FIRST_LAYOUT: &[&[&str]] = &[&["sistema", "pomodoro", "github", "tempo"], &["midia", "claude"]];

/// Coluna do widget na arrumação da primeira vez.
pub fn first_column(key: &str) -> usize {
    FIRST_LAYOUT.iter().position(|col| col.contains(&key)).unwrap_or(0)
}

/// Os widgets disponíveis, na ordem dos menus e das configurações.
pub const KEYS: &[&str] = &["tempo", "sistema", "midia", "github", "pomodoro", "claude"];

/// Widgets que chegaram depois: quem já usava o app não os vê aparecer sozinhos (ficam na galeria).
pub const ADDED_LATER: &[&str] = &["claude"];

/// Nome do widget `key` (sem criar o widget).
pub fn title(key: &str) -> &'static str {
    match key {
        "tempo" => "Tempo",
        "sistema" => "Sistema",
        "midia" => "Tocando agora",
        "github" => "GitHub",
        "pomodoro" => "Pomodoro",
        "claude" => "Uso do Claude",
        _ => "Widget",
    }
}

/// Frase curta sobre o widget (galeria e boas-vindas).
pub fn description(key: &str) -> &'static str {
    match key {
        "tempo" => "O céu de agora, as próximas horas e os próximos dias.",
        "sistema" => "CPU, memória e disco em anéis, com os apps que mais usam.",
        "midia" => "A música ou o vídeo tocando no Windows, com capa e controles.",
        "github" => "Mapa de contribuições, sequência de dias e atividades recentes.",
        "pomodoro" => "Relógio com timer de foco e pausas, ciclos e o total do dia.",
        "claude" => "Quanto você já usou da sessão de 5 horas e da semana no Claude Code.",
        _ => "",
    }
}

/// Ícone do widget na barra lateral das configurações (ver theme::icon).
pub fn icon_name(key: &str) -> &'static str {
    match key {
        "tempo" => "clima",
        "sistema" => "sistema",
        "midia" => "midia",
        "github" => "github",
        "pomodoro" => "pomodoro",
        "claude" => "claude",
        _ => "widgets",
    }
}

/// Um widget com dados de exemplo, sem janela nem threads (miniaturas da galeria).
pub fn sample(key: &str, cfg: &Value, cache: &Value) -> Option<Box<dyn Kind>> {
    match key {
        "midia" => Some(Box::new(media::Media::sample())),
        "sistema" => Some(Box::new(system::System::sample())),
        "github" => Some(Box::new(github::GitHub::sample(cfg, cache))),
        "claude" => Some(Box::new(claude::Claude::sample())),
        _ => create(key, cfg, cache),
    }
}

/// Cria o widget `key` (None para os que ainda não existem nesta versão).
pub fn create(key: &str, cfg: &Value, cache: &Value) -> Option<Box<dyn Kind>> {
    match key {
        "tempo" => Some(Box::new(tempo::Tempo::new(cfg, cache))),
        "sistema" => Some(Box::new(system::System::new())),
        "midia" => Some(Box::new(media::Media::new(cfg))),
        "github" => Some(Box::new(github::GitHub::new(cfg, cache))),
        "pomodoro" => Some(Box::new(pomodoro::Pomodoro::new(cfg))),
        "claude" => Some(Box::new(claude::Claude::new())),
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub w: f32,
    pub h: f32,
    pub radius: f32,
}

impl Layout {
    /// Medidas do cartão em cada tamanho (em pixels lógicos), iguais em todos os widgets.
    pub const fn of(mode: Mode) -> Layout {
        match mode {
            Mode::P => Layout { w: SMALL, h: SMALL, radius: SMALL_RADIUS },
            Mode::M => Layout { w: WIDE, h: MEDIUM_H, radius: MEDIUM_RADIUS },
            Mode::G => Layout { w: WIDE, h: LARGE_H, radius: LARGE_RADIUS },
        }
    }
}

/// Área a destacar sob o mouse: (x0, y0, x1, y1, raio), em unidades lógicas do cartão.
pub type Hot = (f32, f32, f32, f32, f32);

/// Como o hover aparece numa área clicável.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HoverStyle {
    /// botões e linhas: a superfície fica um pouco mais clara, com um fio de contorno
    Surface,
    /// áreas grandes de conteúdo (capa, anéis, relógio, o topo do Tempo): nada por cima; o
    /// cursor de mãozinha já diz que dá para clicar
    Quiet,
}

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
    /// abrir a pergunta de vidro junto do widget: "city" ou "user"
    Ask(&'static str),
    /// o Tocando agora vira a ilha flutuante
    Island,
    /// o bichinho do widget vai para a barra de tarefas (true) ou volta de lá (false)
    Perch(bool),
}

/// O que a ilha flutuante mostra (vem do Tocando agora).
#[derive(Clone)]
pub struct IslandInfo {
    pub title: String,
    pub artist: String,
    /// nome do app que está tocando ("Spotify", "YouTube"...)
    pub app: String,
    pub art: Option<std::sync::Arc<crate::gfx::Image>>,
    /// duas cores fortes da capa (as ondas e o fundo da transformação)
    pub colors: [crate::theme::Rgba; 2],
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub can_prev: bool,
    pub can_toggle: bool,
    pub can_next: bool,
    pub can_seek: bool,
}

/// Ações do Tocando agora que a ilha usa (os mesmos números do widget).
pub mod island_act {
    pub const TOGGLE: u32 = 1;
    pub const NEXT: u32 = 2;
    pub const PREV: u32 = 3;
    pub const OPEN_PLAYER: u32 = 4;
    /// + milésimos da música (0 a 1000)
    pub const SEEK: u32 = 1000;
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
    /// comando com o atalho de teclado escrito à direita
    Hint(String, u32, String),
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
        DEFAULT_MODE
    }
    /// Desenha o conteúdo (o vidro e os detalhes da base são feitos pelo `Card`).
    fn draw(&self, c: &mut Canvas, mode: Mode, env: &Env);
    /// O bichinho que pode sair do widget para a barra de tarefas (o Clawd): onde ele fica no
    /// cartão neste tamanho (centro, em pontos). None: o widget não tem.
    fn perch_from(&self, _mode: Mode) -> Option<(f32, f32)> {
        None
    }
    /// Ele foi para a barra de tarefas (true) ou voltou para o widget.
    fn set_perched(&mut self, _on: bool) {}
    /// Desenha a janela dele na barra de tarefas (ver `claude::PERCH_W`).
    fn draw_perch(&self, _c: &mut Canvas, _env: &Env) {}
    /// Muda quando o desenho da barra de tarefas muda (só então ela é redesenhada).
    fn perch_key(&self, _env: &Env) -> u64 {
        0
    }
    /// Sabe desenhar a troca de tamanho de `from` para `to` quadro a quadro (`draw_between`)?
    /// Senão, o cartão muda de tamanho trocando a imagem de um pela do outro aos poucos.
    fn morphs(&self, _from: Mode, _to: Mode) -> bool {
        false
    }
    /// Um quadro da troca de tamanho, a `p` (0 a 1, no tempo) do caminho de `from` para `to`,
    /// numa tela do tamanho do cartão nesse momento.
    fn draw_between(&self, _c: &mut Canvas, _from: Mode, _to: Mode, _p: f32, _env: &Env) {}
    /// Camada redesenhada a cada quadro por cima do cartão pronto (animações leves); (ox, oy) é
    /// onde fica o canto do cartão na tela de desenho.
    fn overlay(&self, _c: &mut Canvas, _mode: Mode, _ox: f32, _oy: f32) {}
    /// Quadros por segundo da camada animada (0: sem animação).
    fn overlay_fps(&self) -> u32 {
        0
    }
    /// O mouse acabou de entrar no widget; devolve true se a camada animada mudou de ritmo
    /// (`overlay_fps`).
    fn pointer_entered(&mut self) -> bool {
        false
    }
    /// O mouse se mexeu sobre o widget, em (x, y) pontos; devolve true se o widget precisa ser
    /// redesenhado (o Clawd seguindo o mouse com os olhos, por exemplo).
    fn pointer_moved(&mut self, _x: f32, _y: f32, _mode: Mode) -> bool {
        false
    }
    /// O mouse saiu do widget; devolve true se ele precisa ser redesenhado.
    fn pointer_left(&mut self) -> bool {
        false
    }
    /// Ação (número) da área clicável em (x, y).
    fn hit(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> Option<u32> {
        None
    }
    fn act(&mut self, _action: u32, _ctx: &mut Ctx) {}
    /// O botão do mouse desceu na área da ação `action` (o `act` vem só ao soltar). Para reagir
    /// já ao apertar, sem esperar o clique terminar; devolve true se precisa redesenhar.
    fn pressed(&mut self, _action: u32) -> bool {
        false
    }
    fn hot_rect(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> Option<Hot> {
        None
    }
    fn tooltip(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> Option<String> {
        None
    }
    fn hover_style(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> HoverStyle {
        HoverStyle::Surface
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
    /// Texto vindo de uma pergunta ou das configurações (ex.: "city", "user"); `extra` é a
    /// cidade escolhida na lista de sugestões.
    fn input(&mut self, _field: &str, _value: &str, _extra: Option<&Value>, _ctx: &mut Ctx) {}
    /// Clique no aviso de erro: tenta de novo.
    fn retry(&mut self, _ctx: &mut Ctx) {}
    /// Logo depois de criado (começar a buscar dados etc.).
    fn start(&mut self, _ctx: &mut Ctx) {}
    /// Para a ilha flutuante: o que está tocando (None: nada, ou o widget não é de música).
    fn island(&self) -> Option<IslandInfo> {
        None
    }
    /// Nível do som do app que está tocando agora (0 a 1; None se não dá para medir).
    fn sound_level(&self) -> Option<f32> {
        None
    }
}

/// Grava o tamanho do widget na seção dele do config.json, junto com a altura que ele tem nesse
/// tamanho: numa versão nova, se a altura mudar, o App nota e abre espaço embaixo (ver
/// `App::fit_new_heights`).
pub fn store_size(w: &mut serde_json::Map<String, Value>, mode: Mode) {
    w.insert("tamanho".into(), serde_json::json!(mode.code()));
    w.insert("altura".into(), serde_json::json!(Layout::of(mode).h));
    w.insert("altura_tamanho".into(), serde_json::json!(mode.code()));
}

/// Número no config: aceita inteiro ou fracionário.
pub fn num(v: &Value, key: &str) -> Option<f64> {
    v.f64_of(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WIDGET_GAP;

    #[test]
    fn sizes_are_shared() {
        let (p, m, g) = (Layout::of(Mode::P), Layout::of(Mode::M), Layout::of(Mode::G));
        assert_eq!((p.w, p.h), (154.0, 154.0), "o pequeno é um quadrado");
        assert_eq!((m.w, m.h), (320.0, 154.0));
        assert_eq!((g.w, g.h), (320.0, 448.0));
        // pequenos e médios se encaixam em linhas: dois pequenos e o encaixe = um médio
        assert_eq!(2.0 * p.w + WIDGET_GAP, m.w);
        assert_eq!(m.h, p.h);
        assert_eq!(g.w, m.w);
        assert!(p.radius < m.radius && m.radius < g.radius);
    }

    #[test]
    fn store_size_records_the_height() {
        let mut w = serde_json::Map::new();
        store_size(&mut w, Mode::G);
        assert_eq!(w["tamanho"], "g");
        assert_eq!(w["altura"], 448.0);
        assert_eq!(w["altura_tamanho"], "g");
        store_size(&mut w, Mode::M);
        assert_eq!((w["tamanho"].as_str(), w["altura"].as_f64()), (Some("m"), Some(154.0)));
    }

    #[test]
    fn first_layout_has_every_widget_once() {
        let mut placed: Vec<&str> = FIRST_LAYOUT.iter().flat_map(|c| c.iter().copied()).collect();
        placed.sort();
        let mut keys = KEYS.to_vec();
        keys.sort();
        assert_eq!(placed, keys);
        assert_eq!(first_column("sistema"), 0);
        assert_eq!(first_column("midia"), 1);
        assert_eq!(DEFAULT_MODE, Mode::M);
    }

    fn com() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
    }

    #[test]
    fn content_areas_have_a_quiet_hover() {
        com();
        let cfg = serde_json::json!({"midia": {}, "app": {}, "pomodoro": {}, "sistema": {}});
        let env = |key| Env { cfg: &cfg, key };
        let media = media::Media::sample();
        // a capa: nada por cima; o botão de tocar: a superfície
        assert_eq!(media.hover_style(77.0, 77.0, Mode::M, &env("midia")), HoverStyle::Quiet);
        let play = media.hover_style(230.0, 96.0, Mode::M, &env("midia"));
        assert_eq!(play, HoverStyle::Surface);
        let pomodoro = pomodoro::Pomodoro::new(&cfg);
        assert_eq!(pomodoro.hover_style(77.0, 30.0, Mode::M, &env("pomodoro")), HoverStyle::Quiet);
        let system = system::System::sample();
        assert_eq!(system.hover_style(76.0, 22.0, Mode::M, &env("sistema")), HoverStyle::Quiet);
    }

    #[test]
    fn tooltips_only_say_what_is_not_written() {
        com();
        let cfg = serde_json::json!({"tempo": {}, "midia": {}, "app": {}, "pomodoro": {}, "sistema": {}, "github": {}});
        let env = |key| Env { cfg: &cfg, key };
        // tudo o que a dica do Tempo e do Sistema dizia já está no cartão
        let tempo = tempo::Tempo::sample("inicio", 0, 0.0, Mode::G);
        for (x, y) in [(70.0, 48.0), (88.0, 108.0), (150.0, 240.0)] {
            assert_eq!(tempo.tooltip(x, y, Mode::G, &env("tempo")), None);
        }
        assert_eq!(system::System::sample().tooltip(76.0, 22.0, Mode::M, &env("sistema")), None);
        // os botões do Tocando agora não têm legenda; a capa também não (o título cabe)
        let media = media::Media::sample();
        assert_eq!(media.tooltip(230.0, 96.0, Mode::M, &env("midia")), None);
        assert_eq!(media.tooltip(77.0, 77.0, Mode::M, &env("midia")), None);
        // nenhuma dica manda clicar
        let github = github::GitHub::sample(&cfg, &serde_json::json!({}));
        for (x, y) in [(150.0, 182.0), (150.0, 20.0)] {
            if let Some(tip) = github.tooltip(x, y, Mode::G, &env("github")) {
                assert!(!tip.contains("Clique"), "{tip}");
            }
        }
    }

    #[test]
    fn every_widget_starts_medium() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let cfg = serde_json::json!({"tempo": {}, "sistema": {}, "midia": {}, "github": {}, "pomodoro": {}, "app": {}});
        for key in KEYS {
            let kind = sample(key, &cfg, &serde_json::json!({})).unwrap();
            assert_eq!(kind.default_mode(), Mode::M, "{key}");
        }
    }
}
