//! Os widgets. Cada um implementa `Kind`: o que ele desenha em cada tamanho, o que acontece nos
//! cliques e como se atualiza. Arrastar, encaixar, mudar de tamanho e o vidro ficam no `Card`.
//!
//! Para criar um widget novo: um módulo aqui com um tipo que implemente `Kind`, e uma linha em
//! `create` e em `KEYS`.

pub mod battery;
pub mod claude;
pub mod clawd;
pub mod github;
pub mod island_ui;
pub mod media;
pub mod odometer;
pub mod outside;
pub mod pomodoro;
pub mod sky;
pub mod system;
pub mod tempo;
pub mod vector;

use std::time::Instant;

use serde_json::Value;

use crate::data::config::{self, Get};
use crate::gfx::Canvas;

// ---------- medidas dos três tamanhos (as mesmas em todos os widgets) ----------
//
// Cada widget desenha o conteúdo dentro destas medidas; elas nunca mudam com o que ele mostra.
//
// Pequenos e médios cabem numa grade de quadrados do tamanho do pequeno, com o espaço do encaixe
// entre eles (como na tela de início do iPhone): o pequeno é um quadrado; o médio ocupa dois
// quadrados lado a lado (a mesma altura do pequeno). Assim, lado a lado na área de trabalho, as
// linhas de todas as colunas ficam na mesma altura, e o espaço entre os widgets é sempre o mesmo.
// O grande tem a largura do médio e altura própria, para as listas (dias, processos, atividades)
// e a capa grande: dois quadrados ficavam baixos para elas, e três, compridos demais.

/// Lado do tamanho pequeno (um quadrado).
pub const SMALL: f32 = 154.0;
/// Largura do médio e do grande: dois pequenos e o espaço do encaixe.
pub const WIDE: f32 = 2.0 * SMALL + crate::ui::theme::WIDGET_GAP;
pub const MEDIUM_H: f32 = SMALL;
/// Altura do grande: entre dois pequenos (320, baixo para as listas) e três (486, comprido).
pub const LARGE_H: f32 = 432.0;
/// A margem de dentro do cartão, a mesma em todos os widgets e tamanhos (como os 16 pt dos
/// widgets do macOS): o texto e os gráficos começam e terminam a esta distância da borda.
pub const CARD_PAD: f32 = 16.0;
/// O título do widget no cabeçalho ("GitHub", "Sistema", "Pomodoro"...): a mesma letra e a mesma
/// altura em todos (lado a lado, ficam alinhados), sem caixa alta e na cor do texto, como os do
/// macOS. A cor de destaque de cada widget fica no conteúdo (anéis, barras, quadrados).
pub const LABEL_Y: f32 = 12.0;
pub const LABEL_SIZE: f32 = 12.0;
/// Escreve o título do widget (o "GitHub", o "Sistema") em (x, y).
pub fn draw_label(c: &mut Canvas, x: f32, y: f32, text: &str) {
    draw_label_in(c, x, y, text, crate::ui::theme::FG);
}

/// O título do widget numa cor (o "Spotify" no verde do Spotify).
pub fn draw_label_in(c: &mut Canvas, x: f32, y: f32, text: &str, color: crate::ui::theme::Rgba) {
    c.text(x, y, text, crate::gfx::Face::Semibold, LABEL_SIZE, color, crate::gfx::Anchor::LA, None);
}

/// Os números grandes de todos os widgets (o tempo do Pomodoro, os valores dos painéis do Tempo):
/// a mesma letra em todos, para que, lado a lado na área de trabalho, pareçam da mesma família.
/// É a Geist, leve.
pub const HERO: crate::gfx::Face = crate::gfx::Face::NumbersLight;
/// As porcentagens no meio dos anéis (Uso do Claude e bateria), indicadores: a mesma, leve.
pub const PERCENT: crate::gfx::Face = crate::gfx::Face::NumbersLight;
/// Os valores menores, ao lado de um nome ou numa lista (as porcentagens do Sistema, a
/// temperatura de cada hora, a saúde da bateria): a mesma fonte, no peso médio, para marcar sem
/// pesar.
pub const VALUE: crate::gfx::Face = crate::gfx::Face::NumbersMedium;

/// Quanto o "%" conta ao centralizar uma porcentagem no meio de um anel (Uso do Claude e
/// bateria). Com 0, só os algarismos ficam no centro, e o "%" pendurado parece empurrar tudo para
/// a direita; com 1, o número e o "%" ficam juntos no centro, e os algarismos, que pesam muito
/// mais aos olhos, parecem ir para a esquerda. No meio-termo o conjunto parece centrado (o
/// "centro óptico").
pub const PERCENT_SIGN_WEIGHT: f32 = 0.5;

/// Onde começam os algarismos de uma porcentagem com o centro óptico em `x`: `digits` é a
/// largura do número e `sign`, a do "%" com o espaço antes dele.
pub fn optical_start(x: f32, digits: f32, sign: f32) -> f32 {
    x - digits / 2.0 - PERCENT_SIGN_WEIGHT * sign / 2.0
}

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
pub const FIRST_LAYOUT: &[&[&str]] = &[&["sistema", "pomodoro", "github", "tempo"], &["midia", "claude", "bateria"]];

/// Coluna do widget na arrumação da primeira vez.
pub fn first_column(key: &str) -> usize {
    FIRST_LAYOUT.iter().position(|col| col.contains(&key)).unwrap_or(0)
}

/// Os widgets disponíveis, na ordem dos menus e das configurações.
pub const KEYS: &[&str] = &["tempo", "sistema", "midia", "github", "pomodoro", "claude", "bateria"];

/// Widgets que chegaram depois: quem já usava o app não os vê aparecer sozinhos (ficam na galeria).
pub const ADDED_LATER: &[&str] = &["claude", "bateria"];

/// Os widgets que pintam o próprio fundo (o céu de agora no Tempo, a capa desfocada na Mídia),
/// com o texto branco por cima: no tema claro, ficam como estão, sobre o vidro escuro.
const OWN_BACKGROUND: [&str; 2] = ["tempo", "midia"];

/// O widget `key` pinta o próprio fundo (veja `OWN_BACKGROUND`)?
pub fn own_background(key: &str) -> bool {
    OWN_BACKGROUND.contains(&key)
}

/// Nome do widget `key` (sem criar o widget).
pub fn title(key: &str) -> &'static str {
    match key {
        "tempo" => "Tempo",
        "sistema" => "Sistema",
        "midia" => "Tocando agora",
        "github" => "GitHub",
        "pomodoro" => "Pomodoro",
        "claude" => "Uso do Claude",
        "bateria" => "Bateria",
        // as fontes da ilha que não são widgets (o mundo lá fora e a ilha em repouso)
        source if outside::KEYS.contains(&source) => outside::title(source),
        outside::REST => "Ilha em repouso",
        outside::apps::KEY => "Notificações dos apps",
        _ => "Widget",
    }
}

/// Frase curta sobre o widget (galeria e boas-vindas).
pub fn description(key: &str) -> &'static str {
    match key {
        "tempo" => "O céu de agora, as próximas horas e os próximos dias.",
        "sistema" => "CPU, memória e disco em anéis, com os apps que mais usam.",
        "midia" => "A música ou o vídeo tocando no Windows, com capa e controles.",
        "github" => "Mapa de contribuições do último ano.",
        "pomodoro" => "Relógio com timer de foco e pausas, ciclos e o total do dia.",
        "claude" => "Quanto você já usou da sessão de 5 horas e da semana no Claude Code.",
        "bateria" => "A bateria do notebook e a dos fones, mouses e teclados conectados.",
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
        "bateria" => "bateria",
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
        "bateria" => Some(Box::new(battery::Battery::sample())),
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
        "bateria" => Some(Box::new(battery::Battery::new(cache))),
        _ => None,
    }
}

/// O tamanho do cartão: pequeno, médio ou grande.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Mode {
    P,
    M,
    G,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::P, Mode::M, Mode::G];

    /// Como o tamanho fica guardado nas opções ("p", "m", "g").
    pub fn code(self) -> &'static str {
        match self {
            Mode::P => "p",
            Mode::M => "m",
            Mode::G => "g",
        }
    }

    pub fn from_code(code: &str) -> Option<Mode> {
        match code {
            "p" => Some(Mode::P),
            "m" => Some(Mode::M),
            "g" => Some(Mode::G),
            _ => None,
        }
    }

    /// O nome do tamanho nos menus.
    pub fn name(self) -> &'static str {
        match self {
            Mode::P => "Pequeno",
            Mode::M => "Médio",
            Mode::G => "Grande",
        }
    }
}

/// As medidas do cartão num tamanho.
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

    /// O tamanho do cartão em pixels num monitor de escala `s`, montado pela grade: o quadrado e o
    /// espaço do encaixe arredondados uma vez só, e somados. Arredondar cada medida por conta
    /// própria (154 × 1,25 = 192,5 → 193, mas 320 × 1,25 = 400) deixava dois pequenos lado a
    /// lado um pixel mais largos que um médio, e as colunas e linhas desalinhadas. (A altura do
    /// grande não é da grade: arredonda sozinha.)
    pub fn px(mode: Mode, s: f32) -> (i32, i32) {
        let (cell, gap) = grid_px(s);
        let wide = 2 * cell + gap;
        match mode {
            Mode::P => (cell, cell),
            Mode::M => (wide, cell),
            Mode::G => (wide, (LARGE_H * s).round() as i32),
        }
    }
}

/// O lado de um quadrado da grade e o espaço do encaixe, em pixels num monitor de escala `s`.
pub fn grid_px(s: f32) -> (i32, i32) {
    ((SMALL * s).round() as i32, (crate::ui::theme::WIDGET_GAP * s).round() as i32)
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
}

/// Pedidos de um widget para o app (gravar, avisar, abrir algo...).
pub enum Request {
    SaveConfig,
    SaveCache,
    /// notificação do Windows: (tipo, título, texto, ação ao clicar)
    Notify(&'static str, String, String, Option<NotifyAction>),
    Open(String),
    /// abrir a pergunta de vidro junto do widget: "city" ou "user"
    Ask(&'static str),
    /// o Tocando agora vira a ilha flutuante
    Island,
    /// o bichinho do widget vai para a barra de tarefas (true) ou volta de lá (false)
    Perch(bool),
    /// a ação `n` de outro widget (a ilha em repouso começando um foco no Pomodoro)
    Act(&'static str, u32),
    /// liga (até desligar) ou desliga o modo foco do app
    Focus(bool),
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
    pub colors: [crate::ui::theme::Rgba; 2],
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub can_prev: bool,
    pub can_toggle: bool,
    pub can_next: bool,
    pub can_seek: bool,
    /// o volume do app (0 a 1; None: sem o volume dele) e o quanto o aviso de volume aparece
    /// agora (0 a 1; a roda do mouse acabou de mudar o volume)
    pub volume: Option<f32>,
    pub volume_hud: f32,
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

/// O que acontece ao clicar numa notificação.
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
    /// A seção do widget no config.json, para mudar.
    pub fn wcfg(&mut self) -> &mut serde_json::Map<String, Value> {
        self.cfg.obj_mut(self.key)
    }
    /// Pede para gravar as opções.
    pub fn save(&mut self) {
        self.out.push(Request::SaveConfig);
    }
    /// Pede uma notificação do Windows (`kind` diz qual interruptor das Configurações vale).
    pub fn notify(&mut self, kind: &'static str, title: impl Into<String>, text: impl Into<String>, action: Option<NotifyAction>) {
        self.out.push(Request::Notify(kind, title.into(), text.into(), action));
    }
    /// Pede para abrir um site ou um programa.
    pub fn open(&mut self, target: impl Into<String>) {
        self.out.push(Request::Open(target.into()));
    }
    /// O mesmo, só para ler.
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
    /// comando com o atalho de teclado escrito à direita (o menu sabe desenhar; nenhum menu
    /// usa ainda, só os testes)
    #[cfg_attr(not(test), allow(dead_code))]
    Hint(String, u32, String),
    Check(String, bool, u32),
    Radio(String, bool, u32),
    Sub(String, Vec<MenuItem>),
    Sep,
    /// o título do menu (o widget que o abriu): ícone e nome, sem clique
    Title(String, &'static str),
    /// a ação em destaque, num botão azul: texto, número e ícone
    Primary(String, u32, &'static str),
    /// um item com um ícone (de `theme::icon`) à esquerda
    Icon(&'static str, Box<MenuItem>),
}

impl MenuItem {
    /// O mesmo item, com um ícone à esquerda.
    pub fn icon(self, name: &'static str) -> MenuItem {
        MenuItem::Icon(name, Box::new(self))
    }
}

/// Um widget: o que ele desenha, como responde ao mouse e como se atualiza. Quase tudo tem um
/// jeito padrão (não faz nada); cada widget escreve só o que usa.
pub trait Kind {
    /// O nome interno (a seção no config.json: "tempo", "sistema"...).
    fn key(&self) -> &'static str;
    /// O nome na tela (os testes conferem que é o mesmo do registro, `widgets::title`).
    #[cfg_attr(not(test), allow(dead_code))]
    fn title(&self) -> &'static str;
    /// Os tamanhos que o widget tem.
    fn sizes(&self) -> &'static [Mode] {
        &Mode::ALL
    }
    /// O tamanho com que ele aparece pela primeira vez.
    fn default_mode(&self) -> Mode {
        DEFAULT_MODE
    }
    /// Desenha o conteúdo (o vidro e os detalhes da base são feitos pelo `Card`).
    fn draw(&self, c: &mut Canvas, mode: Mode, env: &Env);
    /// O bichinho que pode sair do widget para a barra de tarefas (o Clawd): onde ele fica no
    /// cartão neste tamanho: o centro, em pontos, e o tamanho dos quadradinhos dele (o `unit` de
    /// `clawd::draw`, para o pulo sair e chegar do mesmo tamanho). None: o widget não tem.
    fn perch_from(&self, _mode: Mode) -> Option<(f32, f32, f32)> {
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
    /// O cartão inteiro deslocado agora, em pontos (o tremor do trovão no Tempo).
    fn shake(&self) -> (f32, f32) {
        (0.0, 0.0)
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
    /// Faz a ação `action` (de `hit`, do menu, da ilha ou de uma notificação).
    fn act(&mut self, _action: u32, _ctx: &mut Ctx) {}
    /// O botão do mouse desceu na área da ação `action` (o `act` vem só ao soltar). Para reagir
    /// já ao apertar, sem esperar o clique terminar; devolve true se precisa redesenhar.
    fn pressed(&mut self, _action: u32) -> bool {
        false
    }
    /// A área clicável em (x, y), para destacar com o mouse em cima.
    fn hot_rect(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> Option<Hot> {
        None
    }
    /// A legenda com o mouse parado em (x, y) (só o que não está escrito no cartão).
    fn tooltip(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> Option<String> {
        None
    }
    /// Como destacar a área clicável em (x, y).
    fn hover_style(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> HoverStyle {
        HoverStyle::Surface
    }
    /// A roda do mouse em (x, y): `steps` passos (positivo: para cima).
    fn wheel(&mut self, _x: f32, _y: f32, _steps: f32, _mode: Mode, _ctx: &mut Ctx) {}
    /// A roda do mouse em (x, y) faz algo neste widget (rolar as horas, mudar o volume)? Numa
    /// pilha, a roda troca de widget, menos onde ele mesmo a usa.
    fn uses_wheel(&self, _x: f32, _y: f32, _mode: Mode) -> bool {
        false
    }
    /// Os itens do widget no clique direito.
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
    /// Buscando, com erro, atualizado quando (o que a base do cartão mostra).
    fn status(&self) -> Status {
        Status::default()
    }
    /// Enquanto busca os dados pela primeira vez (ainda sem nada para mostrar): os blocos que
    /// ficam no lugar do texto, pulsando ("esqueleto"), em unidades do cartão. Vazio: o widget
    /// tem o que mostrar (e o "carregando" é o brilho correndo no topo).
    fn skeleton(&self, _mode: Mode) -> Vec<Hot> {
        Vec::new()
    }
    /// Com a cor do papel de parede ligada: a faixa de matiz (graus no OKLab, de-até) das cores
    /// que são alerta neste widget e por isso não mudam (o vermelho da bateria fraca). None: tudo
    /// segue o papel de parede (o vermelho da Pomodoro é a cor dela, não um alerta).
    fn alert_hues(&self) -> Option<(f32, f32)> {
        None
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
    /// O que o widget tem para mostrar na ilha agora (None: nada). A ilha escolhe quem fica
    /// na pílula; os outros viram bolhas ao lado.
    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        None
    }
    /// Desenha o widget na ilha, na forma dada, dentro de `r` (x, y, largura, altura, em pixels
    /// lógicos da ilha); `hot` é a ação do botão sob o mouse.
    fn island_draw(&self, _c: &mut Canvas, _form: IslandForm, _r: Rect4, _hot: Option<u32>, _env: &Env) {}
    /// A ação (de `act`) do botão da ilha no ponto (x, y) relativo ao canto de `r`.
    fn island_hit(&self, _form: IslandForm, _x: f32, _y: f32, _w: f32, _h: f32, _env: &Env) -> Option<u32> {
        None
    }
    /// Quadros por segundo que o widget quer na ilha, nesta forma (0: só quando algo muda; a
    /// chuva caindo na ilha aberta do Tempo, por exemplo).
    fn island_fps(&self, _form: IslandForm) -> u32 {
        0
    }
    /// Muda quando o desenho na ilha muda; parada (sem animação nem mouse), a ilha só redesenha
    /// então. None: redesenha sempre.
    fn island_stamp(&self, _env: &Env) -> Option<u64> {
        None
    }
}

/// Retângulo (x, y, largura, altura).
pub type Rect4 = (f32, f32, f32, f32);

/// A forma em que um widget aparece na ilha: bolha ao lado da pílula, pílula fechada, pílula
/// esticada com um aviso, com o mouse em cima ou aberta.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IslandForm {
    Bubble,
    Compact,
    Banner,
    Medium,
    Large,
}

/// O que um widget mostra na ilha agora.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct IslandItem {
    /// um aviso: estica a ilha por um instante e espera na bolha, sem tomar a pílula
    pub alert: bool,
    /// aviso que toma a pílula até alguém ver (a bateria quase no fim)
    pub urgent: bool,
    /// passa na frente de quem está tocando (o Pomodoro no último minuto)
    pub hurry: bool,
    /// parado (pausado): fica atrás de quem está andando
    pub idle: bool,
    /// muda quando o aviso é outro (a ilha estica de novo)
    pub alert_id: u64,
    /// a cor do widget (o contorno do aviso, o pontinho na bolha)
    pub accent: crate::ui::theme::Rgba,
    /// fechada, a pílula vira uma gota (a ilha em repouso)
    pub dot: bool,
    /// a ilha fica mais apagada (o PC parado)
    pub dim: bool,
}

impl IslandItem {
    /// Algo andando (o Pomodoro correndo, a música tocando).
    pub fn live(accent: crate::ui::theme::Rgba) -> IslandItem {
        IslandItem { alert: false, urgent: false, hurry: false, idle: false, alert_id: 0, accent, dot: false, dim: false }
    }
    /// Um aviso, com o número que o identifica.
    pub fn alert(accent: crate::ui::theme::Rgba, alert_id: u64) -> IslandItem {
        IslandItem { alert: true, alert_id, ..IslandItem::live(accent) }
    }
}

/// Um número estável para um texto (o `alert_id` de um aviso).
pub fn alert_hash(text: &str) -> u64 {
    // FNV-1a de 64 bits
    text.bytes().fold(0xcbf29ce484222325u64, |hash, byte| (hash ^ byte as u64).wrapping_mul(0x100000001b3))
}

/// Se o widget `key` pode aparecer na ilha (Configurações → o widget → "Mostrar na ilha").
pub fn on_island(cfg: &Value, key: &str) -> bool {
    // as notificações dos outros apps só com a pessoa ligando (é ler o que chega para ela)
    config::setting_bool(cfg, "ilha_widgets") && cfg[key].bool_of("na_ilha").unwrap_or(key != outside::apps::KEY)
}

/// Grava o tamanho do widget na seção dele do config.json, junto com a altura que ele tem nesse
/// tamanho: numa versão nova, se a altura mudar, o App nota e abre espaço embaixo (ver
/// `App::fit_new_heights`).
pub fn store_size(section: &mut serde_json::Map<String, Value>, mode: Mode) {
    section.insert("tamanho".into(), serde_json::json!(mode.code()));
    section.insert("altura".into(), serde_json::json!(Layout::of(mode).h));
    section.insert("altura_tamanho".into(), serde_json::json!(mode.code()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::WIDGET_GAP;

    #[test]
    fn sizes_are_shared() {
        let (p, m, g) = (Layout::of(Mode::P), Layout::of(Mode::M), Layout::of(Mode::G));
        assert_eq!((p.w, p.h), (154.0, 154.0), "o pequeno é um quadrado");
        assert_eq!((m.w, m.h), (320.0, 154.0));
        assert_eq!((g.w, g.h), (320.0, 432.0));
        // o grande fica entre duas e três linhas de pequenos
        assert!(g.h > 2.0 * p.h + WIDGET_GAP && g.h < 3.0 * p.h + 2.0 * WIDGET_GAP);
        // pequenos e médios se encaixam em linhas: dois pequenos e o encaixe = um médio
        assert_eq!(2.0 * p.w + WIDGET_GAP, m.w);
        assert_eq!(m.h, p.h);
        assert_eq!(g.w, m.w);
        assert!(p.radius < m.radius && m.radius < g.radius);
    }

    /// Em pixels, em qualquer escala do Windows, a grade fecha: dois pequenos e o espaço dão a
    /// largura do médio e do grande; cada
    /// tamanho fica a menos de um pixel da medida em pontos. (Arredondando cada medida sozinha,
    /// em 125% e 175% dois pequenos passavam um pixel do médio.)
    #[test]
    fn sizes_close_the_grid_in_pixels() {
        for s in [1.0f32, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0, 3.5] {
            let (cell, gap) = grid_px(s);
            let (p, m, g) = (Layout::px(Mode::P, s), Layout::px(Mode::M, s), Layout::px(Mode::G, s));
            assert_eq!(p, (cell, cell), "{s}");
            assert_eq!(m, (2 * p.0 + gap, p.1), "{s}: dois pequenos não dão um médio");
            assert_eq!(g.0, m.0, "{s}: o grande não tem a largura do médio");
            for mode in Mode::ALL {
                let (l, px) = (Layout::of(mode), Layout::px(mode, s));
                assert!((px.0 as f32 - l.w * s).abs() <= 1.5 && (px.1 as f32 - l.h * s).abs() <= 1.5, "{s} {mode:?}: {px:?}");
            }
        }
    }

    #[test]
    fn store_size_records_the_height() {
        let mut w = serde_json::Map::new();
        store_size(&mut w, Mode::G);
        assert_eq!(w["tamanho"], "g");
        assert_eq!(w["altura"], 432.0);
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
