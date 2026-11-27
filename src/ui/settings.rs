//! Configurações: barra lateral com páginas, controles próprios, prévia ao vivo e aplicação
//! imediata (sem "Salvar" nem "Cancelar").

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use crate::data::config::{self, Get};
use crate::platform::events::Cursor;
use crate::gfx::{Anchor, Face, Image};
use crate::ui::panel::{Suggest, Ui, Action};
use crate::data::shared::Shared;
use crate::ui::theme::*;
use crate::widgets::{self, pomodoro, Mode};

/// O tamanho da janela: alta o bastante para o menu lateral caber inteiro, sem rolagem (a
/// escala diminui sozinha numa tela pequena, ver `panel::fit_scale`).
pub const W: f32 = 960.0;
pub const H: f32 = 744.0;
/// Onde termina a ilha do menu (a página começa aqui, com o respiro `GUTTER` antes do conteúdo).
pub const SIDEBAR: f32 = 240.0;
/// O respiro entre a ilha e o conteúdo, igual ao da borda direita da janela.
const GUTTER: f32 = 32.0;
/// O campo de busca no alto do menu: onde começa e a altura. A página começa na mesma altura,
/// para a busca e o conteúdo andarem juntos na grade.
const SEARCH_TOP: f32 = 80.0;
const SEARCH_H: f32 = 32.0;
pub const TOP: f32 = SEARCH_TOP;
/// O id do campo de busca (os eventos de texto chegam com ele).
pub const SEARCH_ID: &str = "busca";
/// O centro do cabeçalho: o ícone do app na ilha e o da página ficam nesta altura.
const HEADER_CY: f32 = NAV_MARGIN + 31.0;
/// Altura do degradê que avisa que a página continua embaixo (no lugar da barra de rolagem).
const PAGE_FADE: f32 = 36.0;

/// A ilha do menu lateral: a distância até a borda da janela, o raio dos cantos e o fundo preto
/// (o mesmo da ilha dinâmica na barra de tarefas).
const NAV_MARGIN: f32 = 12.0;
const NAV_RADIUS: f32 = 22.0;
const NAV_BG: Rgba = Rgba::rgb(0, 0, 0);
/// Onde a lista do menu começa e termina dentro da ilha (acima dela ficam o nome do app e a
/// busca, fixos).
const NAV_TOP: f32 = SEARCH_TOP + SEARCH_H + 12.0;
/// (o fim só é usado no teste que garante que o menu inteiro cabe acima dele)
#[cfg(test)]
const NAV_BOTTOM: f32 = H - NAV_MARGIN - 10.0;
/// Altura de um item do menu, a distância de um item ao próximo e a de um título de grupo.
const NAV_ITEM: f32 = 36.0;
const NAV_STEP: f32 = 38.0;
const NAV_GROUP: f32 = 30.0;
/// As colunas do menu (a partir da borda da ilha): o fundo dos itens, os ícones e os textos.
/// O ícone do app no alto fica na mesma coluna dos ícones, e o nome dele na dos textos.
const NAV_PILL_X: f32 = 8.0;
const NAV_TILE_X: f32 = 16.0;
const NAV_TILE: f32 = 24.0;
const NAV_TEXT_X: f32 = 50.0;
const NAV_ICON: f32 = 30.0;
/// Quanto um "dente" da roda do mouse rola (a página e o menu).
const WHEEL_STEP: f32 = 64.0;
pub use crate::platform::updates::VERSION;
use crate::platform::notifications::Access;
use crate::widgets::outside::apps;

/// Os botões das notificações dos apps (pelo `Action::Widget("app", ...)`): pedir a permissão,
/// abrir a privacidade das notificações e as notificações do Windows.
pub const ASK_NOTIFY_ACCESS: u32 = 3;
pub const OPEN_NOTIFY_PRIVACY: u32 = 4;
pub const OPEN_NOTIFY_SETTINGS: u32 = 5;
/// no máximo estes apps na lista das notificações (os vistos por último)
const APP_LIST_MAX: usize = 12;
/// o interruptor de cada app da lista (o id da área precisa durar a vida toda do app)
const APP_SWITCHES: [&str; APP_LIST_MAX] =
    ["app_notif:0", "app_notif:1", "app_notif:2", "app_notif:3", "app_notif:4", "app_notif:5", "app_notif:6", "app_notif:7", "app_notif:8", "app_notif:9", "app_notif:10", "app_notif:11"];
/// "Seguir a tela em uso" e "Mostrar o texto das mensagens"
pub const FOLLOW_SWITCH: &str = "ilha_segue_tela";
pub const TEXT_SWITCH: &str = "notif_texto";

pub const NOTIFY: [(&str, &str, &str); 7] = [
    ("cpu", "CPU alta", "Quando a CPU passa de 90% por 3 minutos"),
    ("ram", "Memória quase cheia", "Quando a memória passa de 90% por 3 minutos"),
    ("disco", "Disco quase cheio", "Quando o disco C: passa de 95% de uso"),
    ("chuva", "Chuva chegando", "Quando a previsão indica chuva na próxima hora"),
    ("github", "Revisão pedida no GitHub", "Quando alguém pede sua revisão num pull request"),
    ("pomodoro", "Pomodoro", "No fim de cada foco e de cada pausa"),
    ("bateria", "Pouca bateria num dispositivo", "Quando um fone, mouse ou teclado conectado passa de 15%"),
];

pub const TIPS: [(&str, &str); 5] = [
    ("Mover", "Arraste de qualquer lugar. Os widgets se encaixam uns nos outros e nas bordas da tela; segure Alt para soltar livre."),
    ("Reordenar", "Arraste um widget para dentro de uma coluna: os outros abrem espaço."),
    ("Tamanho", "Botão no canto (aparece com o mouse) ou dois cliques: pequeno, médio e grande."),
    ("Roda do mouse", "Volume no Tocando agora; ±1 minuto no Pomodoro."),
    ("Menu", "Clique direito num widget ou no ícone da bandeja: layouts, modo foco, edição e mais."),
];

pub const APPEARANCE_KEYS: [&str; 6] = [
    "desfoque",
    "escurecimento",
    crate::data::shared::WALL_COLOR,
    crate::data::shared::LIGHT_THEME,
    "texto",
    "animacoes",
];
pub const BEHAVIOR_KEYS: [&str; 4] = ["atalhos_clique", "dicas", "economia_bateria", "placa_de_video"];

/// Faixas das durações do Pomodoro: (fase, rótulo, mínimo, máximo, passo).
pub const PHASE_RANGES: [(&str, &str, f64, f64, f64); 3] =
    [("foco", "Foco", 5.0, 90.0, 5.0), ("pausa", "Pausa curta", 1.0, 30.0, 1.0), ("longa", "Pausa longa", 5.0, 60.0, 5.0)];

#[derive(Default)]
pub struct SettingsState {
    pub page: &'static str,
    pub scrolls: HashMap<&'static str, f32>,
    pub content_h: f32,
    pub thumbs: HashMap<&'static str, (bool, Option<Image>)>,
    /// a prévia do alto da página do widget: (widget, visível, imagem)
    hero: Option<(&'static str, bool, Option<Image>)>,
    /// a prévia da Aparência e do que ela depende (lugar, tamanho, desfoque, escurecimento,
    /// versão do fundo e as opções de cor: papel de parede e tema claro)
    preview: Option<((i32, i32, u32, u32, i64, i64, u64, (bool, bool)), Image)>,
    /// o papel de parede atrás do palco do alto da página de um widget e do que ele depende
    /// (lugar e tamanho na tela, versão do fundo)
    stage: Option<((i32, i32, u32, u32, u64), Image)>,
    pub city: Suggest,
    /// o que está escrito na busca do menu (vazio: a página escolhida)
    pub search: String,
    /// todas as opções de todas as páginas, para a busca (refeito ao abrir a busca)
    index: Option<Vec<Found>>,
    /// a opção achada pela busca, piscando na página dela: (página, altura da linha, desde quando)
    pub flash: Option<(&'static str, f32, Instant)>,
}

/// Uma opção achada pela busca: a página, a seção, o título, a descrição e a altura da linha na
/// página (para rolar até ela).
#[derive(Clone, Debug)]
pub struct Found {
    pub page: &'static str,
    pub section: String,
    pub title: String,
    pub desc: String,
    pub y: f32,
}

/// Quanto tempo a opção achada pela busca pisca, na página dela.
const FLASH_MS: f32 = 1400.0;
/// Quanto acima da opção achada a página para, ao rolar até ela (o título da seção aparece).
const FIND_MARGIN: f32 = 40.0;

/// O que as linhas desenhadas agora contam para o resto da janela: a cor da página (para os
/// ícones), as ações que voltam as opções fora do padrão (para o "Restaurar tudo" do título), a
/// seção atual e, montando o índice da busca, as linhas vistas.
#[derive(Default)]
struct Painting {
    color: Option<Rgba>,
    undo: Vec<Action>,
    section: String,
    collect: Option<(&'static str, Vec<Found>)>,
    /// a linha achada pela busca: a altura dela e o quanto ainda brilha (0 a 1)
    flash: Option<(f32, f32)>,
}

thread_local! {
    static PAINTING: RefCell<Painting> = RefCell::new(Painting::default());
}

impl SettingsState {
    pub fn new(page: &'static str) -> SettingsState {
        SettingsState { page, ..Default::default() }
    }

    pub fn go(&mut self, page: &'static str) {
        if page != self.page {
            self.page = page;
            self.city.clear();
            self.preview = None;
            self.hero = None;
            self.stage = None;
            if page == "widgets" {
                self.thumbs.clear();
            }
        }
    }

    /// Rola a página; devolve se ela andou.
    pub fn scroll(&mut self, steps: f32) -> bool {
        let view = H - TOP;
        let top = (self.content_h - view).max(0.0);
        let page = self.scroll_key();
        let cur = self.scrolls.get(page).copied().unwrap_or(0.0);
        let new = (cur - steps * WHEEL_STEP).clamp(0.0, top);
        if new != cur {
            self.scrolls.insert(page, new);
            self.city.clear();
            return true;
        }
        false
    }

    /// O texto da busca mudou: os resultados recomeçam do alto (e o índice é montado de novo na
    /// próxima busca, com as opções de agora).
    pub fn searched(&mut self, text: &str) {
        if text.trim().is_empty() || self.search.trim().is_empty() {
            self.index = None;
        }
        self.search = text.to_string();
        self.scrolls.insert(RESULTS_PAGE, 0.0);
    }

    /// Vai até a opção achada pela busca: a página dela, rolada até a linha, que pisca.
    pub fn find(&mut self, page: &'static str, y: f32) {
        self.search.clear();
        self.index = None;
        self.go(page);
        self.scrolls.insert(page, (y - FIND_MARGIN).max(0.0));
        self.flash = Some((page, y, Instant::now()));
    }

    /// A primeira opção achada pela busca (o Enter vai até ela): a página e a altura dela.
    pub fn first_result(&self) -> Option<(&'static str, f32)> {
        let index = self.index.as_ref()?;
        search_results(index, &self.search).first().map(|found| (found.page, found.y))
    }

    /// A página que rola agora: a dos resultados, buscando, ou a escolhida.
    fn scroll_key(&self) -> &'static str {
        if self.searching() { RESULTS_PAGE } else { self.page }
    }

    /// Há algo escrito na busca?
    pub fn searching(&self) -> bool {
        !self.search.trim().is_empty()
    }
}

/// A "página" dos resultados da busca (só para guardar a rolagem dela).
const RESULTS_PAGE: &str = "resultados";

/// O que as páginas precisam saber do app.
pub struct View<'a> {
    pub sh: &'a Shared,
    pub visible: Vec<&'static str>,
    pub modes: HashMap<&'static str, Mode>,
    /// canto da área de desenho na tela (para o vidro das miniaturas e da prévia)
    pub origin: (i32, i32),
    /// cartão de um widget: (conteúdo, raio) no tamanho e escala pedidos
    pub render: &'a dyn Fn(&'static str, Mode, f32) -> Option<(Image, f32)>,
    pub startup: bool,
    pub update_status: String,
    pub update_version: Option<String>,
    /// a permissão do Windows para ler as notificações dos outros apps
    pub notify_access: Access,
}

fn nav_items() -> Vec<(&'static str, String, &'static str)> {
    let mut items = vec![
        ("geral", "Geral".to_string(), "geral"),
        ("ilha", "Ilha dinâmica".into(), "ilha"),
        ("aparencia", "Aparência".into(), "aparencia"),
        ("widgets", "Galeria de widgets".into(), "widgets"),
        ("-", "SEUS WIDGETS".into(), ""),
    ];
    for key in widgets::KEYS {
        items.push((key, widgets::title(key).into(), widgets::icon_name(key)));
    }
    items.extend([
        ("-", "MAIS".into(), ""),
        ("avisos", "Notificações".into(), "avisos"),
        ("dicas", "Dicas".into(), "atalhos"),
        ("sobre", "Sobre".into(), "sobre"),
    ]);
    items
}

/// Um item do menu lateral no lugar: o y dentro da lista (sem a rolagem) e o que ele é. Os
/// títulos de grupo ("SEUS WIDGETS") têm `page` = "-".
struct NavSpot {
    y: f32,
    page: &'static str,
    label: String,
    icon: &'static str,
}

/// Onde fica cada item do menu lateral, na ordem.
fn nav_spots() -> Vec<NavSpot> {
    // o primeiro item começa na mesma altura que o título da primeira seção da página
    let mut y = 4.0;
    let mut spots = Vec::new();
    for (page, label, icon) in nav_items() {
        let step = match (page, label.is_empty()) {
            ("-", true) => 17.0,
            ("-", false) => NAV_GROUP,
            _ => NAV_STEP,
        };
        spots.push(NavSpot { y, page, label, icon });
        y += step;
    }
    spots
}

/// A altura da lista do menu inteira (até o fim do último item, com um respiro).
#[cfg(test)]
fn nav_height() -> f32 {
    nav_spots().last().map_or(0.0, |spot| spot.y + NAV_ITEM + 4.0)
}

/// A cor do quadradinho de cada página (a barra lateral e o título), como nos Ajustes do Mac.
fn page_color(page: &str) -> Rgba {
    match page {
        "geral" => Rgba::rgb(142, 142, 152),
        "ilha" => Rgba::rgb(94, 92, 230),
        "aparencia" => Rgba::rgb(175, 92, 230),
        "widgets" => Rgba::rgb(10, 122, 245),
        "tempo" => Rgba::rgb(48, 166, 230),
        "sistema" => Rgba::rgb(230, 82, 118),
        "midia" => Rgba::rgb(240, 96, 70),
        "github" => Rgba::rgb(46, 160, 67),
        "pomodoro" => Rgba::rgb(235, 88, 76),
        "claude" => Rgba::rgb(214, 120, 84),
        "bateria" => Rgba::rgb(52, 180, 90),
        "avisos" => Rgba::rgb(232, 64, 58),
        "dicas" => Rgba::rgb(222, 160, 20),
        _ => Rgba::rgb(110, 116, 130),
    }
}

/// O ícone de cada página.
fn page_icon(page: &str) -> &'static str {
    nav_items().into_iter().find(|(p, _, _)| *p == page).map_or("sobre", |(_, _, icon)| icon)
}

/// Os widgets que aparecem na ilha flutuante (além do Tocando agora, que vira a ilha).
pub const ISLAND_KEYS: [&str; 5] = ["pomodoro", "sistema", "github", "bateria", "tempo"];

/// O interruptor "Mostrar na ilha" de cada widget.
pub fn island_id(key: &str) -> &'static str {
    match key {
        "pomodoro" => "na_ilha:pomodoro",
        "sistema" => "na_ilha:sistema",
        "github" => "na_ilha:github",
        "tempo" => "na_ilha:tempo",
        _ => "na_ilha:bateria",
    }
}

fn page_title(page: &str) -> (String, Option<String>) {
    if widgets::KEYS.contains(&page) {
        return (widgets::title(page).into(), Some(widgets::description(page).into()));
    }
    let (title, subtitle) = match page {
        "geral" => ("Geral", Some("Como os widgets se comportam no dia a dia.")),
        "ilha" => ("Ilha dinâmica", Some("O que aparece na ilha, onde ela fica e as notificações dos outros apps.")),
        "aparencia" => ("Aparência", Some("Vidro, texto e animações. Tudo muda na hora.")),
        "widgets" => ("Galeria de widgets", Some("Escolha quais widgets ficam na sua área de trabalho.")),
        "avisos" => ("Notificações", Some("Quando os widgets podem chamar sua atenção.")),
        "dicas" => ("Dicas", Some("Gestos do mouse e o que dá para fazer com o clique direito.")),
        _ => ("Sobre", None),
    };
    (title.into(), subtitle.map(String::from))
}

// ---------- linhas e controles ----------

pub enum Control {
    None,
    Switch(&'static str, bool, Action),
    Slider(&'static str, f64, f64, f64, f64, String),
    Seg(&'static str, Vec<(String, String)>, String),
    Button(&'static str, String, Action, &'static str, Option<&'static str>),
    Buttons(Vec<(String, String, Action)>),
    Entry(&'static str, String, String, f32),
}

/// Slider das configurações: a trilha, o espaço do valor ("12 px") à direita dela, o tanto que a
/// bolinha passa da ponta esquerda e o respiro até o texto da linha.
const SLIDER_W: f32 = 160.0;
const SLIDER_VALUE: f32 = 64.0;
const SLIDER_KNOB: f32 = 10.0;
const SLIDER_GAP: f32 = 20.0;

impl Control {
    /// Largura que o controle ocupa na direita da linha (o texto fica com o resto).
    fn width(&self, w: f32) -> f32 {
        match self {
            Control::None => 0.0,
            Control::Switch(..) => 70.0,
            Control::Slider(..) => SLIDER_W + SLIDER_VALUE + SLIDER_KNOB + SLIDER_GAP,
            _ => w * 0.44,
        }
    }

    fn draw(&self, ui: &mut Ui, rx: f32, cy: f32, row: (f32, f32, f32, f32)) {
        match self {
            Control::None => {}
            Control::Switch(id, on, action) => {
                if ui.hot(id) {
                    ui.fill(row.0 + 1.0, row.1 + 1.0, row.2 - 1.0, row.3 - 1.0, 9.0, WHITE.with_a(7));
                }
                ui.switch(id, rx - 40.0, cy - 11.0, *on, action.clone(), Some(row));
            }
            Control::Slider(id, value, lo, hi, step, label) => {
                ui.text(rx, cy, label, 12.0, MUTED, Face::UiRegular, Anchor::RM, None);
                ui.slider(id, rx - SLIDER_VALUE - SLIDER_W, cy, SLIDER_W, *value, *lo, *hi, *step);
            }
            Control::Seg(id, options, current) => {
                ui.segmented(id, rx, cy - 16.0, options, current, true);
            }
            Control::Button(id, label, action, kind, icon) => {
                ui.button(id, rx, cy - 16.0, label, action.clone(), kind, 'r', None, *icon);
            }
            Control::Buttons(buttons) => {
                let mut x = rx;
                for (id, label, action) in buttons.iter().rev() {
                    x -= ui.button(id, x, cy - 16.0, label, action.clone(), "secondary", 'r', None, None) + 8.0;
                }
            }
            Control::Entry(id, value, placeholder, width) => {
                ui.entry(id, rx - width, cy - 17.0, rx, cy + 17.0, value, placeholder, false);
            }
        }
    }
}

pub struct Row(pub String, pub Option<String>, pub Control);

/// O título de uma seção, em seminegrito e sem caixa alta (como os grupos dos Ajustes do macOS),
/// alinhado com o texto das linhas do cartão logo abaixo. Devolve o y do cartão.
fn section(ui: &mut Ui, x: f32, y: f32, title: &str) -> f32 {
    PAINTING.with(|p| p.borrow_mut().section = title.to_string());
    ui.text(x + ROW_TEXT_X, y, title, 13.0, FG.alpha(0.92), Face::UiSemibold, Anchor::LA, None);
    #[cfg(test)]
    tests::placed(x, y, y + 18.0);
    y + 28.0
}

/// As explicações de uma seção (o que não é uma opção: como algo funciona, o que o clique faz),
/// logo abaixo do cartão dela, em letra menor e com um ícone de dica. Como linhas sem controle
/// dentro do cartão, pareciam opções que não faziam nada. `y` é o que `rows` devolveu. Devolve
/// o y logo abaixo.
fn notes(ui: &mut Ui, x: f32, y: f32, w: f32, notes: &[&str]) -> f32 {
    // `rows` deixa o espaço até a próxima seção: as dicas ficam mais perto do cartão
    let top = y - NOTE_PULL;
    let text_x = x + ROW_TEXT_X + NOTE_INDENT;
    let text_w = x + w - ROW_TEXT_X - text_x;
    let mut line_y = top;
    for note in notes {
        let lines = ui.wrap(note, 11.5, text_w, NOTE_MAX_LINES);
        ui.icon(x + ROW_TEXT_X + 6.0, line_y + 8.5, "dicas", 12.0, FAINT);
        for (j, line) in lines.iter().enumerate() {
            ui.text(text_x, line_y + NOTE_LINE * j as f32, line, 11.5, MUTED, Face::UiRegular, Anchor::LA, Some(text_w));
        }
        line_y += NOTE_LINE * lines.len() as f32 + NOTE_GAP;
    }
    #[cfg(test)]
    tests::placed(x, top, line_y);
    line_y + NOTE_PULL + 4.0
}

/// As dicas embaixo de um cartão: quanto sobem para perto dele, o recuo do texto (o ícone fica
/// na frente), a altura de cada linha, o espaço entre uma dica e outra e o máximo de linhas.
const NOTE_PULL: f32 = 12.0;
const NOTE_INDENT: f32 = 22.0;
const NOTE_LINE: f32 = 17.0;
const NOTE_GAP: f32 = 8.0;
const NOTE_MAX_LINES: usize = 4;

/// Onde o texto das linhas começa dentro de um cartão (o título da seção acompanha).
const ROW_TEXT_X: f32 = 18.0;
/// O ícone colorido de cada linha (como nos Ajustes do macOS): o lado do quadradinho e o espaço
/// que ele ocupa antes do texto.
const ROW_ICON: f32 = 26.0;
const ROW_ICON_SPACE: f32 = 38.0;
/// A marca de opção fora do padrão: o raio do ponto e o espaço dele até o título.
const CHANGED_DOT: f32 = 3.5;
const CHANGED_GAP: f32 = 12.0;

/// O ícone de uma linha, pelo que o controle dela faz (o id dele): como nos Ajustes do macOS,
/// dá para achar uma opção de olho, sem ler tudo. Linhas só de texto ficam com o "i".
fn row_icon(control: &Control) -> &'static str {
    let id = match control {
        Control::Switch(id, ..) | Control::Slider(id, ..) | Control::Seg(id, ..) | Control::Button(id, ..) | Control::Entry(id, ..) => *id,
        Control::Buttons(_) => return "layouts",
        Control::None => return "sobre",
    };
    match id {
        "iniciar" => "sair",
        "travado" => "lock",
        "atalhos_clique" => "mouse",
        "dicas" => "dicas",
        "economia_bateria" => "bateria",
        "placa_de_video" => "sistema",
        "procurar" | "atualizar" | "procurar_atualizacoes" | "atualizar_clima" | "atualizar_github" => "refresh",
        "ir_ilha" | "ilha_widgets" => "ilha",
        "editar" => "edit",
        "organizar" => "organizar",
        "salvar_layout" => "add",
        "desfoque" | "escurecimento" | crate::data::shared::WALL_COLOR => "aparencia",
        crate::data::shared::LIGHT_THEME => "brilho",
        "texto_auto" | "texto" | "claude_nomes" | TEXT_SWITCH => "fonte",
        "animacoes" | "pomodoro_auto" => "play",
        "cidade" => "location",
        "tempo_tremer" => "carregando",
        "fonte" => "midia",
        "capa_hd" => "foto",
        "usuario" => "pessoa",
        "longa_a_cada" | "fim_expediente" => "pomodoro",
        "zerar" | "restaurar_tudo" => "undo",
        "claude_conectar" | "claude_desconectar" => "link",
        "claude_natal" | "modo_foco" => "foco",
        FOLLOW_SWITCH => "mover",
        "boas_vindas" => "widgets",
        "pasta" => "folder",
        id if id.starts_with("dur:") => "pomodoro",
        id if id.starts_with("aviso:") || id.starts_with("notif_") || id.starts_with("app_notif:") || id == "testar" => "avisos",
        id if id.starts_with("na_ilha:") => "ilha",
        id if id.starts_with("tamanho:") => "tamanho",
        _ => "geral",
    }
}

/// A cor do quadradinho de um ícone de linha, pelo tipo dele (avisos em vermelho, ilha em índigo,
/// aparência em roxo...), como nos Ajustes do macOS; None: a cor da página.
fn icon_color(icon: &str) -> Option<Rgba> {
    let page = match icon {
        "avisos" => "avisos",
        "ilha" | "foco" | "mover" => "ilha",
        "aparencia" | "fonte" | "foto" => "aparencia",
        "brilho" | "dicas" | "carregando" => "dicas",
        "bateria" | "play" | "add" => "bateria",
        "mouse" | "refresh" | "location" | "pessoa" | "link" | "widgets" | "edit" | "organizar" | "layouts" => "widgets",
        "pomodoro" | "undo" => "pomodoro",
        "midia" => "midia",
        "sistema" => "sistema",
        _ => return None,
    };
    Some(page_color(page))
}

/// O valor padrão de um interruptor pelo id dele (None: sem um padrão fixo, como o "Iniciar
/// com o Windows", que depende do Windows).
fn switch_default(id: &str) -> Option<bool> {
    if let Some(on) = config::default_setting(id).as_bool() {
        return Some(on);
    }
    match id {
        "travado" | "pomodoro_auto" | FOLLOW_SWITCH => Some(false),
        "tempo_tremer" | "claude_nomes" | "capa_hd" | TEXT_SWITCH => Some(true),
        // (as notificações nascem todas ligadas)
        id if id.starts_with("aviso:") => Some(true),
        _ => None,
    }
}

/// O valor padrão de um slider pelo id dele (na unidade que ele mostra).
fn slider_default(id: &str) -> Option<f64> {
    match id {
        "desfoque" => Some(BLUR),
        "escurecimento" => Some((TINT_AMOUNT * 100.0).round()),
        "dur:foco" => Some(25.0),
        "dur:pausa" => Some(5.0),
        "dur:longa" => Some(15.0),
        _ => None,
    }
}

/// O valor padrão de um seletor segmentado pelo id dele.
fn seg_default(id: &str) -> Option<String> {
    match id {
        "animacoes" => config::default_setting(id).as_str().map(String::from),
        "longa_a_cada" => Some("4".into()),
        "fonte" => Some("auto".into()),
        "claude_natal" => Some("dezembro".into()),
        "fim_expediente" => Some(widgets::outside::DEFAULT_END.into()),
        _ => None,
    }
}

/// O que volta a opção de uma linha ao padrão, se ela está fora dele: o próprio clique, num
/// interruptor; o valor padrão, num slider ou num seletor. None: no padrão, ou sem padrão
/// conhecido (não ganha a marca de alterada).
fn undo_of(control: &Control) -> Option<Action> {
    match control {
        Control::Switch(id, on, action) => (switch_default(id)? != *on).then(|| action.clone()),
        Control::Slider(id, value, ..) => {
            let default = slider_default(id)?;
            ((value - default).abs() > 0.01).then_some(Action::SliderSet(id, default))
        }
        Control::Seg(id, _, current) => {
            let default = seg_default(id)?;
            (default != *current).then_some(Action::Seg(id, default))
        }
        _ => None,
    }
}

/// Cartão com linhas (ícone, título, descrição e controle). Uma opção fora do padrão ganha um
/// ponto azul antes do título e um "Desfazer" depois dele. Devolve o y logo abaixo.
fn rows(ui: &mut Ui, x: f32, y: f32, w: f32, rows: Vec<Row>) -> f32 {
    let mut heights = Vec::new();
    let mut lines = Vec::new();
    for Row(_, desc, control) in &rows {
        let text_w = w - 36.0 - ROW_ICON_SPACE - control.width(w);
        let full = desc.as_ref().map(|d| ui.wrap(d, 11.5, text_w, 99)).unwrap_or_default();
        #[cfg(test)]
        if full.len() > 2 {
            tests::cut(desc.as_deref().unwrap_or(""));
        }
        let ls = if full.len() <= 2 { full } else { vec![full[0].clone(), full[1..].join(" ")] };
        heights.push(if ls.is_empty() { 54.0 } else { 66.0 + 17.0 * (ls.len() as f32 - 1.0) });
        lines.push((text_w, ls));
    }
    ui.card(x, y, x + w, y + heights.iter().sum::<f32>());
    #[cfg(test)]
    tests::placed(x, y, y + heights.iter().sum::<f32>());
    let (color, flash) = PAINTING.with(|p| {
        let p = p.borrow();
        (p.color.unwrap_or(ACCENT), p.flash)
    });
    let mut top = y;
    for (i, (Row(title, desc, control), (h, (text_w, ls)))) in rows.iter().zip(heights.iter().zip(lines)).enumerate() {
        let row = (x, top, x + w, top + h);
        record_found(title, desc.as_deref().unwrap_or(""), top);
        if let Some((_, glow)) = flash.filter(|(flash_y, _)| (flash_y - top).abs() < 0.5) {
            ui.fill(x + 1.0, top + 1.0, x + w - 1.0, top + h - 1.0, 12.0, ACCENT.alpha(0.28 * glow));
        }
        if i > 0 {
            ui.divider(x + ROW_TEXT_X + ROW_ICON_SPACE, x + w - 16.0, top);
        }
        control.draw(ui, x + w - 18.0, top + h / 2.0, row);
        let icon = row_icon(control);
        ui.tile(x + ROW_TEXT_X, top + h / 2.0 - ROW_ICON / 2.0, ROW_ICON, icon_color(icon).unwrap_or(color), icon, false);
        let mut text_x = x + ROW_TEXT_X + ROW_ICON_SPACE;
        let title_y = if ls.is_empty() { top + h / 2.0 } else { top + 23.0 };
        let undo = undo_of(control);
        if undo.is_some() {
            ui.circle(text_x + CHANGED_DOT, title_y, CHANGED_DOT, ACCENT);
            text_x += CHANGED_GAP;
        }
        let tw = ui.text(text_x, title_y, title, 13.0, FG, Face::UiRegular, Anchor::LM, Some(text_w - (text_x - x - ROW_TEXT_X - ROW_ICON_SPACE)));
        if let Some(action) = undo {
            let id = format!("desfazer:{}:{top}", title);
            let ux = text_x + tw + 10.0;
            let uw = ui.text_width("Desfazer", 11.5, Face::UiSemibold);
            let hot = ui.hot(&id);
            ui.text(ux, title_y, "Desfazer", 11.5, if hot { ACCENT_HOVER } else { ACCENT }, Face::UiSemibold, Anchor::LM, None);
            ui.region(&id, ux - 4.0, title_y - 10.0, ux + uw + 4.0, title_y + 10.0, action.clone(), Cursor::Hand);
            PAINTING.with(|p| p.borrow_mut().undo.push(action));
        }
        let desc_x = x + ROW_TEXT_X + ROW_ICON_SPACE;
        for (j, line) in ls.iter().enumerate() {
            ui.text(desc_x, top + 36.0 + 17.0 * j as f32, line, 11.5, MUTED, Face::UiRegular, Anchor::LA, Some(text_w));
        }
        top += h;
    }
    top + 26.0
}

/// Montando o índice da busca: guarda a linha `title` (na altura `y` da página).
fn record_found(title: &str, desc: &str, y: f32) {
    PAINTING.with(|p| {
        let mut p = p.borrow_mut();
        let section = p.section.clone();
        if let Some((page, found)) = p.collect.as_mut() {
            found.push(Found { page, section, title: title.to_string(), desc: desc.to_string(), y });
        }
    });
}

fn sw(id: &'static str, on: bool, action: Action) -> Control {
    Control::Switch(id, on, action)
}

fn setting_switch(v: &View, name: &'static str) -> Control {
    sw(name, config::setting_bool(&v.sh.cfg, name), Action::Toggle(name))
}

fn btn(id: &'static str, label: &str, action: Action) -> Control {
    Control::Button(id, label.into(), action, "secondary", None)
}

// ---------- desenho ----------

/// Desenha a janela inteira: barra lateral, a página (rolável) e o título dela (depois da página,
/// que conta quantas opções estão fora do padrão para o "Restaurar tudo" do título).
pub fn draw(st: &mut SettingsState, ui: &mut Ui, v: &View) {
    draw_sidebar(st, ui, v);
    let flash = st.flash.filter(|(page, _, at)| *page == st.page && at.elapsed().as_secs_f32() * 1000.0 < FLASH_MS);
    if flash.is_some() {
        ui.anims.running = true; // continua redesenhando até o brilho apagar
    } else {
        st.flash = None;
    }
    PAINTING.with(|p| {
        *p.borrow_mut() = Painting {
            color: Some(page_color(st.page)),
            flash: flash.map(|(_, y, at)| (y, 1.0 - at.elapsed().as_secs_f32() * 1000.0 / FLASH_MS)),
            ..Painting::default()
        };
    });
    draw_page(st, ui, v);
    let undo = PAINTING.with(|p| std::mem::take(&mut p.borrow_mut().undo));
    draw_page_title(st, ui, v, undo);
}

/// Todas as opções de todas as páginas, para a busca: cada página desenhada numa tela de
/// rascunho, sem as miniaturas dos widgets, só para guardar as linhas (título, descrição, seção
/// e altura).
fn search_index(v: &View) -> Vec<Found> {
    let no_render = |_: &'static str, _: Mode, _: f32| None;
    let quiet = View {
        sh: v.sh,
        visible: v.visible.clone(),
        modes: v.modes.clone(),
        origin: v.origin,
        render: &no_render,
        startup: v.startup,
        update_status: v.update_status.clone(),
        update_version: v.update_version.clone(),
        notify_access: v.notify_access,
    };
    let mut all = Vec::new();
    for (page, _, _) in nav_items().into_iter().filter(|(page, _, _)| *page != "-" && *page != "widgets") {
        let mut c = crate::gfx::Canvas::new(8, 8, 1.0);
        let mut anims = crate::ui::panel::Anims::default();
        let mut ui = Ui { c: &mut c, regions: Vec::new(), entries: Vec::new(), hover: None, pressed: None, focus: None, anims: &mut anims, animations: false, ox: 0.0, oy: 0.0, clip: (0.0, 0.0, 8.0, 8.0) };
        let mut scratch = SettingsState::new(page);
        PAINTING.with(|p| *p.borrow_mut() = Painting { collect: Some((page, Vec::new())), ..Painting::default() });
        let (x, w) = (GUTTER, W - SIDEBAR - 2.0 * GUTTER);
        draw_page_body(&mut scratch, &mut ui, x, w, &quiet, page);
        if let Some((_, found)) = PAINTING.with(|p| p.borrow_mut().collect.take()) {
            all.extend(found);
        }
    }
    PAINTING.with(|p| *p.borrow_mut() = Painting::default());
    all
}

/// O texto em minúsculas e sem acentos, para a busca achar "notificacoes" em "Notificações".
fn folded(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|ch| match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// As opções que batem com a busca `query`: todas as palavras dela no título, na descrição, na
/// seção ou no nome da página (em qualquer ordem).
fn search_results<'a>(index: &'a [Found], query: &str) -> Vec<&'a Found> {
    let words: Vec<String> = folded(query).split_whitespace().map(String::from).collect();
    index
        .iter()
        .filter(|found| {
            let haystack = folded(&format!("{} {} {} {}", found.title, found.desc, found.section, page_title(found.page).0));
            words.iter().all(|word| haystack.contains(word.as_str()))
        })
        .collect()
}

/// A altura de um resultado da busca e o espaço entre o título da página e o cartão.
const RESULT_H: f32 = 58.0;

/// Os resultados da busca, agrupados por página: cada um com o título, a descrição e a seção; o
/// clique leva à opção, na página dela.
fn page_results(st: &mut SettingsState, ui: &mut Ui, x: f32, w: f32, v: &View) -> f32 {
    if st.index.is_none() {
        st.index = Some(search_index(v));
    }
    let index = st.index.clone().unwrap_or_default();
    let found = search_results(&index, &st.search);
    if found.is_empty() {
        ui.text(x + w / 2.0, 60.0, &format!("Nenhuma opção com “{}”", st.search.trim()), 14.0, FG, Face::UiSemibold, Anchor::MM, Some(w));
        ui.text(x + w / 2.0, 86.0, "Tente “chuva”, “bateria”, “vidro” ou “ilha”", 12.0, MUTED, Face::UiRegular, Anchor::MM, Some(w));
        return 120.0;
    }
    let mut y = 4.0;
    let mut pages: Vec<&'static str> = Vec::new();
    for item in &found {
        if !pages.contains(&item.page) {
            pages.push(item.page);
        }
    }
    for page in pages {
        let items: Vec<&&Found> = found.iter().filter(|item| item.page == page).collect();
        y = section(ui, x, y, &page_title(page).0);
        let h = RESULT_H * items.len() as f32;
        ui.card(x, y, x + w, y + h);
        #[cfg(test)]
        tests::placed(x, y, y + h);
        for (i, item) in items.iter().enumerate() {
            let top = y + RESULT_H * i as f32;
            let id = format!("achado:{page}:{}", item.y);
            if ui.hot(&id) {
                ui.fill(x + 1.0, top + 1.0, x + w - 1.0, top + RESULT_H - 1.0, 12.0, WHITE.with_a(10));
            }
            if i > 0 {
                ui.divider(x + ROW_TEXT_X + ROW_ICON_SPACE, x + w - 16.0, top);
            }
            ui.tile(x + ROW_TEXT_X, top + RESULT_H / 2.0 - ROW_ICON / 2.0, ROW_ICON, page_color(page), page_icon(page), false);
            let text_x = x + ROW_TEXT_X + ROW_ICON_SPACE;
            let crumb_w = ui.text(x + w - 18.0, top + 21.0, &item.section, 11.0, FAINT, Face::UiRegular, Anchor::RM, Some(w * 0.3));
            ui.text(text_x, top + 21.0, &item.title, 13.0, FG, Face::UiRegular, Anchor::LM, Some(x + w - 18.0 - crumb_w - 16.0 - text_x));
            ui.text(text_x, top + 34.0, &item.desc, 11.5, MUTED, Face::UiRegular, Anchor::LA, Some(x + w - 18.0 - text_x));
            ui.region(&id, x, top, x + w, top + RESULT_H, Action::Find(page, item.y), Cursor::Hand);
        }
        y += h + 26.0;
    }
    y
}

/// O menu lateral: uma ilha preta flutuando sobre o fundo, com o app no alto e a lista das
/// páginas embaixo (inteira, sem rolagem: a janela tem a altura para isso).
fn draw_sidebar(st: &SettingsState, ui: &mut Ui, v: &View) {
    let island = (NAV_MARGIN, NAV_MARGIN, SIDEBAR, H - NAV_MARGIN);
    draw_nav_island(ui, island);
    let (x0, _, x1, _) = island;
    // o ícone do app na mesma coluna dos ícones das páginas, e o nome centrado com ele
    let icon = crate::ui::tray::icon_image((NAV_ICON * ui.s()).round() as u32);
    ui.image(&icon, x0 + NAV_TILE_X - 3.0, HEADER_CY - NAV_ICON / 2.0);
    let text_x = x0 + NAV_TEXT_X;
    ui.text(text_x, HEADER_CY - 1.0, "Widgets", 15.0, FG, Face::UiSemibold, Anchor::LS, None);
    ui.text(text_x, HEADER_CY + 2.0, "Configurações", 11.0, MUTED, Face::UiRegular, Anchor::LA, None);
    // a busca: digite "chuva" ou "bateria" e a página mostra as opções de qualquer página
    ui.entry(SEARCH_ID, x0 + NAV_PILL_X, SEARCH_TOP, x1 - NAV_PILL_X, SEARCH_TOP + SEARCH_H, &st.search, "Buscar uma opção", false);
    draw_nav_list(st, ui, v, x0, x1);
}

/// O fundo da ilha do menu: uma sombra macia embaixo, o preto e a borda que brilha mais em cima.
fn draw_nav_island(ui: &mut Ui, (x0, y0, x1, y1): (f32, f32, f32, f32)) {
    for (spread, alpha) in [(8.0, 10), (4.0, 18), (1.5, 34)] {
        ui.fill(x0 - spread, y0 - spread + 4.0, x1 + spread, y1 + spread + 4.0, NAV_RADIUS + spread, Rgba(0, 0, 0, alpha));
    }
    ui.fill(x0, y0, x1, y1, NAV_RADIUS, NAV_BG);
    let line = 1.0 / ui.s();
    let (ox, oy) = (ui.ox, ui.oy);
    ui.c.gradient_stroke_round_rect(x0 + ox, y0 + oy, x1 + ox, y1 + oy, NAV_RADIUS, WHITE.with_a(34), WHITE.with_a(12), line);
}

/// A lista das páginas, a partir de `NAV_TOP` (o fundo do escolhido desliza até ele).
fn draw_nav_list(st: &SettingsState, ui: &mut Ui, v: &View, x0: f32, x1: f32) {
    let old_oy = ui.oy;
    ui.oy = old_oy + NAV_TOP;
    let (pill_x0, pill_x1) = (x0 + NAV_PILL_X, x1 - NAV_PILL_X);
    let spots = nav_spots();
    // buscando, nenhuma página fica marcada (a página mostra os resultados)
    if let Some(spot) = spots.iter().find(|spot| spot.page == st.page && !st.searching()) {
        let iy = ui.anim("nav-indicator", spot.y, 220);
        ui.fill(pill_x0, iy, pill_x1, iy + NAV_ITEM, 10.0, ACCENT.with_a(64));
        ui.rect(pill_x0, iy, pill_x1, iy + NAV_ITEM, 10.0, None, Some(ACCENT.with_a(96)), 1.0);
    }
    for NavSpot { y, page, label, icon } in spots {
        if page == "-" {
            if label.is_empty() {
                ui.fill(x0 + 18.0, y + 8.0, x1 - 18.0, y + 9.0, 0.0, WHITE.with_a(16));
            } else {
                ui.text(x0 + NAV_TILE_X, y + 13.0, &label, 9.5, FAINT, Face::UiSemibold, Anchor::LA, None);
            }
            continue;
        }
        let id = format!("nav:{page}");
        let active = page == st.page && !st.searching();
        if !active && ui.hot(&id) {
            ui.fill(pill_x0, y, pill_x1, y + NAV_ITEM, 10.0, WHITE.with_a(16));
        }
        let hidden = widgets::KEYS.contains(&page) && !v.visible.contains(&page);
        ui.tile(x0 + NAV_TILE_X, y + (NAV_ITEM - NAV_TILE) / 2.0, NAV_TILE, page_color(page), icon, hidden);
        let color = if hidden { FAINT } else { FG };
        let face = if active { Face::UiSemibold } else { Face::UiRegular };
        let tag_w = if hidden { ui.text_width("oculto", 9.5, Face::UiSemibold) + 12.0 } else { 0.0 };
        ui.text(x0 + NAV_TEXT_X, y + NAV_ITEM / 2.0, &label, 12.5, color, face, Anchor::LM, Some(x1 - x0 - NAV_TEXT_X - 20.0 - tag_w));
        if hidden {
            // a etiqueta "oculto": o widget existe, mas não está na área de trabalho
            let (tag_x1, cy) = (x1 - 16.0, y + NAV_ITEM / 2.0);
            ui.fill(tag_x1 - tag_w, cy - 8.0, tag_x1, cy + 8.0, 8.0, WHITE.with_a(18));
            ui.text(tag_x1 - tag_w / 2.0, cy, "oculto", 9.5, FAINT, Face::UiSemibold, Anchor::MM, None);
        }
        ui.region(&id, pill_x0, y, pill_x1, y + NAV_ITEM, Action::Go(page), Cursor::Hand);
    }
    ui.oy = old_oy;
}

/// O título da página, com o ícone dela e a frase de baixo, centrados na altura do nome do app
/// na ilha do menu. À direita: "Na área de trabalho", na página de um widget, e "N alterações ·
/// Restaurar tudo", com opções fora do padrão (`undo`: o que volta cada uma).
fn draw_page_title(st: &SettingsState, ui: &mut Ui, v: &View, undo: Vec<Action>) {
    let (title, subtitle) = if st.searching() {
        ("Resultados".to_string(), Some(format!("Opções com “{}”, em todas as páginas", st.search.trim())))
    } else {
        page_title(st.page)
    };
    let x = SIDEBAR + GUTTER;
    // no Sobre, o ícone do app já está logo abaixo
    let with_tile = st.page != "sobre" || st.searching();
    let tile = 40.0;
    if with_tile {
        let (color, icon) = if st.searching() { (page_color("widgets"), "search") } else { (page_color(st.page), page_icon(st.page)) };
        ui.tile(x, HEADER_CY - tile / 2.0, tile, color, icon, false);
    }
    let mut right = W - GUTTER;
    if !st.searching() {
        if widgets::KEYS.contains(&st.page) {
            // o widget na área de trabalho: o interruptor no título, como o de um app nos Ajustes
            let visible = v.visible.contains(&st.page);
            ui.switch("ver:hero", right - 40.0, HEADER_CY - 11.0, visible, Action::Visible(st.page, !visible), None);
            let lw = ui.text(right - 52.0, HEADER_CY, "Na área de trabalho", 12.5, FG, Face::UiRegular, Anchor::RM, None);
            ui.region("ver:hero:texto", right - 56.0 - lw, HEADER_CY - 12.0, right - 44.0, HEADER_CY + 12.0, Action::Visible(st.page, !visible), Cursor::Hand);
            right -= 56.0 + lw + 22.0;
        }
        if !undo.is_empty() {
            let n = undo.len();
            let label = format!("{n} {}", if n == 1 { "alteração" } else { "alterações" });
            // nas páginas das opções gerais, o restaurar de antes (com um aviso e Desfazer); nas dos
            // widgets, o "Desfazer" de cada opção
            let action = match st.page {
                "geral" | "aparencia" | "avisos" => Action::Restore(st.page),
                _ => Action::Many(undo),
            };
            let bw = ui.button("restaurar:tudo", right, HEADER_CY - 16.0, "Restaurar tudo", action, "ghost", 'r', None, Some("undo"));
            let lw = ui.text(right - bw - 6.0, HEADER_CY, &label, 11.5, MUTED, Face::UiRegular, Anchor::RM, None);
            ui.circle(right - bw - 14.0 - lw, HEADER_CY, CHANGED_DOT, ACCENT);
            right -= bw + 24.0 + lw;
        }
    }
    let tx = if with_tile { x + tile + 14.0 } else { x };
    let room = right - tx - 12.0;
    match subtitle {
        Some(sub) => {
            ui.text(tx, HEADER_CY - 1.0, &title, 21.0, FG, Face::UiSemibold, Anchor::LS, Some(room));
            ui.text(tx, HEADER_CY + 3.0, &sub, 11.5, MUTED, Face::UiRegular, Anchor::LA, Some(room));
        }
        None => {
            ui.text(tx, HEADER_CY, &title, 21.0, FG, Face::UiSemibold, Anchor::LM, Some(room));
        }
    }
}

/// A página escolhida, rolável com a roda do mouse. Sem barra de rolagem: um degradê embaixo
/// avisa que ela continua, e uma linha fina no alto, que ela já foi rolada.
fn draw_page(st: &mut SettingsState, ui: &mut Ui, v: &View) {
    let view_h = H - TOP;
    let key = st.scroll_key();
    let scroll = st.scrolls.get(key).copied().unwrap_or(0.0);
    let left = SIDEBAR;
    ui.c.push_rect_clip(left, TOP, W, H);
    let (old_ox, old_oy, old_clip) = (ui.ox, ui.oy, ui.clip);
    ui.ox = left;
    ui.oy = TOP - scroll;
    ui.clip = (left, TOP, W, H);
    let (x, w) = (GUTTER, W - SIDEBAR - 2.0 * GUTTER);
    let height = if st.searching() { page_results(st, ui, x, w, v) } else { draw_page_body(st, ui, x, w, v, st.page) };
    ui.ox = old_ox;
    ui.oy = old_oy;
    ui.clip = old_clip;
    ui.c.pop_rect_clip();
    st.content_h = height + 32.0;
    let top = (st.content_h - view_h).max(0.0);
    if scroll > top {
        st.scrolls.insert(key, top);
    }
    let scroll = scroll.min(top);
    if scroll < top {
        // o fundo da janela (o papel de parede já escurecido até quase `BASE`) cobrindo a ponta
        // do que continua embaixo
        let from = H - PAGE_FADE;
        let (clear, base) = (crate::ui::panel::BASE.with_a(0), crate::ui::panel::BASE.with_a(230));
        ui.c.gradient_round_rect(left, from, W, H, 0.0, &[(0.0, clear), (1.0, base)], from, H);
    }
    if scroll > 0.0 {
        ui.fill(left + GUTTER, TOP, W - GUTTER, TOP + 1.0, 0.0, WHITE.with_a(20));
    }
}

/// O conteúdo da página `page` (sem o título), com o canto em `x` e a largura `w`; devolve a
/// altura dele.
fn draw_page_body(st: &mut SettingsState, ui: &mut Ui, x: f32, w: f32, v: &View, page: &'static str) -> f32 {
    match page {
        "geral" => page_geral(ui, x, w, v),
        "ilha" => page_ilha(ui, x, w, v),
        "aparencia" => page_aparencia(st, ui, x, w, v),
        "widgets" => page_widgets(st, ui, x, w, v),
        "avisos" => page_avisos(ui, x, w, v),
        "dicas" => page_dicas(ui, x, w),
        "sobre" => page_sobre(ui, x, w, v),
        key => page_widget(st, ui, x, w, v, key),
    }
}

fn page_geral(ui: &mut Ui, x: f32, w: f32, v: &View) -> f32 {
    let y = section(ui, x, 4.0, "Comportamento");
    let y = rows(ui, x, y, w, vec![
        Row("Iniciar com o Windows".into(), Some("Abre os widgets quando você entra no Windows".into()), sw("iniciar", v.startup, Action::Startup)),
        Row("Travar posições".into(), Some("Evita arrastar sem querer (o modo edição continua liberado)".into()), sw("travado", v.sh.locked(), Action::Lock)),
        Row("Cliques abrem programas e sites".into(), Some("Gerenciador de Tarefas, previsão do tempo, GitHub…".into()), setting_switch(v, "atalhos_clique")),
        Row("Dicas ao passar o mouse".into(), Some("Explicações curtas sobre o que está sob o mouse".into()), setting_switch(v, "dicas")),
        Row("Acompanhar a economia de energia".into(), Some("Pausa as animações contínuas quando a economia de energia do Windows está ligada".into()), setting_switch(v, "economia_bateria")),
        Row("Desenhar pela placa de vídeo".into(), Some("Animações mais lisas, com menos CPU (experimental; vale ao abrir o app de novo)".into()), setting_switch(v, "placa_de_video")),
    ]);
    let y = section(ui, x, y, "Ilha dinâmica");
    let y = rows(ui, x, y, w, vec![Row(
        "Tudo da ilha num lugar só".into(),
        Some("Os widgets na ilha, as notificações dos apps, a ilha em repouso e o mundo lá fora".into()),
        btn("ir_ilha", "Abrir", Action::Go("ilha")),
    )]);
    let y = section(ui, x, y, "Organizar");
    let y = rows(ui, x, y, w, vec![
        Row("Modo edição".into(), Some("Feche, redimensione e mova todos os widgets de uma vez".into()), Control::Button("editar", "Editar widgets".into(), Action::Edit, "primary", Some("edit"))),
        Row("Organizar automaticamente".into(), Some("Empilha os widgets em colunas na direita da tela".into()), btn("organizar", "Organizar", Action::Arrange)),
    ]);
    let y = section(ui, x, y, "Layouts");
    let mut layout_rows = Vec::new();
    if let Some(layouts) = v.sh.cfg["app"]["layouts"].as_object() {
        for (name, snap) in layouts {
            let count = snap.as_object().map(|m| m.values().filter(|s| s.bool_of("visible").unwrap_or(false)).count()).unwrap_or(0);
            let buttons = vec![
                (format!("aplicar:{name}"), "Aplicar".to_string(), Action::ApplyLayout(name.clone())),
                (format!("excluir:{name}"), "Excluir".to_string(), Action::DeleteLayout(name.clone())),
            ];
            layout_rows.push(Row(name.clone(), Some(format!("{count} widget{}", if count != 1 { "s" } else { "" })), Control::Buttons(buttons)));
        }
    }
    layout_rows.push(Row(
        "Salvar layout atual".into(),
        Some("Guarda a posição e o tamanho de cada widget".into()),
        Control::Button("salvar_layout", "Salvar…".into(), Action::SaveLayout, "secondary", Some("add")),
    ));
    let y = rows(ui, x, y, w, layout_rows);
    y
}

/// O que cada widget mostra na ilha (na página da ilha).
fn island_description(key: &str) -> &'static str {
    match key {
        "pomodoro" => "O tempo correndo, com o último minuto na frente de tudo",
        "sistema" => "A CPU ou a memória altas por alguns minutos, com os apps que mais gastam",
        "github" => "Um pedido de revisão novo",
        "bateria" => "Bateria fraca, a tomada entrando ou saindo e um fone ou mouse conectando",
        _ => "Chuva chegando, trovoada e amanhã bem mais frio ou mais quente",
    }
}

/// A página da ilha dinâmica: os widgets nela, onde ela fica, as notificações dos outros apps, a
/// ilha em repouso e o mundo lá fora.
fn page_ilha(ui: &mut Ui, x: f32, w: f32, v: &View) -> f32 {
    let cfg = &v.sh.cfg;
    let y = section(ui, x, 4.0, "Widgets na ilha");
    let mut widget_rows = vec![Row(
        "Widgets na ilha".into(),
        Some("Os widgets abaixo aparecem na ilha quando têm algo a dizer, ao lado da música. O Tocando agora vira a ilha com um clique no widget".into()),
        setting_switch(v, "ilha_widgets"),
    )];
    for key in ISLAND_KEYS {
        widget_rows.push(Row(widgets::title(key).into(), Some(island_description(key).into()), sw(island_id(key), widgets::on_island(cfg, key), Action::Toggle(island_id(key)))));
    }
    let y = rows(ui, x, y, w, widget_rows);
    let y = section(ui, x, y, "Notificações dos apps");
    let y = rows(ui, x, y, w, notify_rows(v));
    let apps_rows = muted_rows(v);
    let y = if apps_rows.is_empty() {
        y
    } else {
        let y = section(ui, x, y, "Apps que aparecem na ilha");
        rows(ui, x, y, w, apps_rows)
    };
    let y = section(ui, x, y, "Onde fica");
    let y = rows(ui, x, y, w, vec![
        Row("Na barra de tarefas".into(), Some("À esquerda do Iniciar. Arraste para qualquer lugar; clique direito nela para voltar à barra ou ir para o alto da tela".into()), Control::None),
        Row("Seguir a tela em uso".into(), Some("Com dois monitores, a ilha vai para o monitor da janela em que você está, no mesmo lugar".into()), sw(FOLLOW_SWITCH, cfg["midia"].bool_of("ilha_segue_tela").unwrap_or(false), Action::Toggle(FOLLOW_SWITCH))),
    ]);
    let y = section(ui, x, y, "A ilha em repouso");
    let y = rows(ui, x, y, w, rest_rows(v));
    let y = section(ui, x, y, "O mundo lá fora, na ilha");
    rows(ui, x, y, w, outside_rows(v))
}

/// As notificações dos outros apps: ligar, a permissão do Windows, o texto e como não ver duas
/// vezes.
fn notify_rows(v: &View) -> Vec<Row> {
    let cfg = &v.sh.cfg;
    let switch_id = "na_ilha:notificacoes_apps";
    let on = apps::enabled(cfg);
    let mut r = vec![Row(
        "Mostrar as notificações dos apps".into(),
        Some("WhatsApp, Teams, Outlook… A que chega estica a ilha com o ícone do app, quem mandou e o começo da mensagem. Nada sai do seu PC".into()),
        sw(switch_id, on, Action::Toggle(switch_id)),
    )];
    if !on {
        return r;
    }
    r.push(match v.notify_access {
        Access::Allowed => Row("Permissão do Windows".into(), Some("Liberada".into()), Control::None),
        Access::Denied => Row(
            "Permissão do Windows".into(),
            Some("Negada. Libere o Widgets em Privacidade → Notificações".into()),
            btn("notif_privacidade", "Abrir o Windows", Action::Widget("app", OPEN_NOTIFY_PRIVACY)),
        ),
        Access::Unasked => Row("Permissão do Windows".into(), Some("O Windows pergunta uma vez".into()), Control::Button("notif_permitir", "Permitir".into(), Action::Widget("app", ASK_NOTIFY_ACCESS), "primary", None)),
        Access::Unavailable => Row("Permissão do Windows".into(), Some("Este Windows não deixa ler as notificações".into()), Control::None),
    });
    r.push(Row(
        "Mostrar o texto das mensagens".into(),
        Some("Desligado, a ilha mostra só o app e quem mandou (bom para compartilhar a tela)".into()),
        sw(TEXT_SWITCH, apps::shows_text(cfg), Action::Toggle(TEXT_SWITCH)),
    ));
    r.push(Row(
        "Sem o aviso repetido do Windows".into(),
        Some("Para ver só a ilha, desligue os banners do app nas notificações do Windows".into()),
        btn("notif_windows", "Abrir o Windows", Action::Widget("app", OPEN_NOTIFY_SETTINGS)),
    ));
    r
}

/// Os apps já vistos na central, cada um com o interruptor de aparecer na ilha.
fn muted_rows(v: &View) -> Vec<Row> {
    if !apps::enabled(&v.sh.cfg) {
        return Vec::new();
    }
    let muted = apps::muted(&v.sh.cfg);
    apps::seen_apps(&v.sh.cache)
        .into_iter()
        .take(APP_LIST_MAX)
        .zip(APP_SWITCHES)
        .map(|((app_id, name), switch)| {
            let label = if name.is_empty() { app_id.clone() } else { name };
            let on = !muted.contains(&app_id);
            Row(label, None, sw(switch, on, Action::MuteApp(app_id)))
        })
        .collect()
}

/// O interruptor de cada fonte do mundo lá fora (o pôr do sol, a lua...).
fn outside_id(key: &str) -> &'static str {
    match key {
        "saudacao" => "na_ilha:saudacao",
        "sol" => "na_ilha:sol",
        "lua" => "na_ilha:lua",
        "iss" => "na_ilha:iss",
        "cotacao" => "na_ilha:cotacao",
        _ => "na_ilha:expediente",
    }
}

/// A ilha em repouso: ligada ou não, o que ela mostra (mais de um: alternando) e a gota.
fn rest_rows(v: &View) -> Vec<Row> {
    use widgets::outside::{rest, REST};
    let cfg = &v.sh.cfg;
    let on = widgets::on_island(cfg, REST);
    let mut r = vec![Row(
        "Modo foco".into(),
        Some("Esconde os widgets e deixa a ilha só com a hora, o dia e o ícone do foco, sem avisos. Também no clique direito da ilha, dos widgets e do ícone da bandeja".into()),
        sw("modo_foco", v.sh.focus, Action::Toggle("modo_foco")),
    ), Row(
        "Ilha sempre na barra".into(),
        Some("Sem nada acontecendo, a ilha fica na barra de tarefas com o que você escolher abaixo (mais de um: um a cada 7 s). Some em tela cheia e apaga com o PC parado".into()),
        sw("na_ilha:repouso", on, Action::Toggle("na_ilha:repouso")),
    )];
    if on {
        let chosen = rest::chosen(cfg);
        for look in rest::Look::ALL {
            let mut desc = look.description().to_string();
            if look == rest::Look::Day && widgets::outside::location(cfg).is_none() {
                desc.push_str(". Escolha a cidade no widget Tempo");
            }
            r.push(Row(look.title().into(), Some(desc), sw(look.setting_id(), chosen.contains(&look), Action::Toggle(look.setting_id()))));
        }
        r.push(Row("Só uma gota".into(), Some("Fechada, a ilha é só um ponto na cor da hora do dia; o resto aparece com o mouse em cima".into()), sw(rest::DOT_ID, rest::dot_on(cfg), Action::Toggle(rest::DOT_ID))));
    }
    r
}

/// As fontes do mundo lá fora (cada uma liga e desliga) e o horário do fim do expediente.
fn outside_rows(v: &View) -> Vec<Row> {
    use widgets::outside::{self, workday_end_choices, DEFAULT_END};
    let cfg = &v.sh.cfg;
    let mut r: Vec<Row> = outside::KEYS
        .iter()
        .map(|&key| {
            let mut desc = outside::description(key).to_string();
            if matches!(key, "sol" | "lua" | "iss") && outside::location(cfg).is_none() {
                desc.push_str(". Escolha a cidade no widget Tempo");
            }
            Row(outside::title(key).into(), Some(desc), sw(outside_id(key), widgets::on_island(cfg, key), Action::Toggle(outside_id(key))))
        })
        .collect();
    let options = workday_end_choices().iter().map(|h| (h.to_string(), h.to_string())).collect();
    let end = cfg["expediente"].str_of("fim").unwrap_or(DEFAULT_END).to_string();
    r.push(Row("O expediente termina às".into(), None, Control::Seg("fim_expediente", options, end)));
    r
}

fn page_aparencia(st: &mut SettingsState, ui: &mut Ui, x: f32, w: f32, v: &View) -> f32 {
    let cfg = &v.sh.cfg;
    let blur = config::setting_f64(cfg, "desfoque");
    let tint = config::setting_f64(cfg, "escurecimento");
    draw_preview(st, ui, x, 4.0, w, PREVIEW_H, v, blur, tint);
    let y = section(ui, x, 4.0 + PREVIEW_H + 26.0, "Vidro");
    let y = rows(ui, x, y, w, vec![
        Row("Desfoque".into(), Some("Quanto o papel de parede fica borrado atrás dos widgets".into()),
            Control::Slider("desfoque", blur.round(), 8.0, 40.0, 1.0, format!("{} px", blur.round()))),
        Row("Escurecimento".into(), Some("Deixa o texto legível mesmo em papéis de parede claros".into()),
            Control::Slider("escurecimento", (tint * 100.0).round(), 15.0, 70.0, 1.0, format!("{}%", (tint * 100.0).round()))),
        Row("Cor do papel de parede".into(), Some("As cores dos widgets seguem a do papel de parede (a ilha e o Clawd ficam como são)".into()),
            setting_switch(v, crate::data::shared::WALL_COLOR)),
        Row("Tema claro do Windows".into(), Some("Com o Windows no tema claro, vidro claro e texto escuro (Tempo e Mídia ficam com o fundo deles)".into()),
            setting_switch(v, crate::data::shared::LIGHT_THEME)),
    ]);
    let following = config::setting_f64(cfg, "texto") <= 0.0;
    let mut r = vec![Row("Seguir o tamanho de texto do Windows".into(), Some("Configurações → Acessibilidade → Tamanho do texto".into()), sw("texto_auto", following, Action::Toggle("texto_auto")))];
    if !following {
        let pct = (v.sh.text_scale as f64 * 100.0).round();
        r.push(Row("Tamanho do texto".into(), None, Control::Slider("texto", pct, 90.0, 120.0, 1.0, format!("{pct}%"))));
    }
    let y = section(ui, x, y, "Texto");
    let y = rows(ui, x, y, w, r);
    let y = section(ui, x, y, "Movimento");
    let options = [("auto", "Seguir o Windows"), ("sempre", "Sempre"), ("nunca", "Nunca")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    let y = rows(ui, x, y, w, vec![Row(
        "Animações".into(),
        Some("Ao mudar de tamanho, reorganizar e abrir menus".into()),
        Control::Seg("animacoes", options, config::setting_str(cfg, "animacoes")),
    )]);
    y
}

/// Altura da prévia: um widget médio (154) com folga em cima e embaixo.
const PREVIEW_H: f32 = 214.0;

/// Widgets de verdade sobre o papel de parede, com o vidro das opções atuais: um médio e um
/// pequeno lado a lado (os primeiros que estão na tela), no meio do palco.
fn draw_preview(st: &mut SettingsState, ui: &mut Ui, x: f32, y: f32, w: f32, h: f32, v: &View, blur: f64, tint: f64) {
    let s = ui.s();
    let sx = v.origin.0 + ((ui.ox + x) * s).round() as i32;
    let sy = v.origin.1 + ((ui.oy + y) * s).round() as i32;
    let (pw, ph) = ((w * s).round() as u32, (h * s).round() as u32);
    let key = (sx, sy, pw, ph, (blur * 10.0) as i64, (tint * 1000.0) as i64, v.sh.backdrop.version, (v.sh.wall_color, v.sh.light));
    if st.preview.as_ref().map(|p| p.0) != Some(key) {
        let order = ["tempo", "sistema", "pomodoro", "midia", "github"];
        let mut shown: Vec<&'static str> = order.into_iter().filter(|k| v.visible.contains(k)).take(2).collect();
        if shown.is_empty() {
            shown.push("tempo");
        }
        let cards: Vec<(Image, f32, &str)> = shown.iter().zip([Mode::M, Mode::P]).filter_map(|(k, m)| (v.render)(k, m, s).map(|(img, radius)| (img, radius, *k))).collect();
        if !cards.is_empty() {
            let mut stage = v.sh.backdrop.region(sx, sy, pw, ph, true).to_image();
            let gap = (16.0 * s).round() as i32;
            let total: i32 = cards.iter().map(|c| c.0.w as i32).sum::<i32>() + gap * (cards.len() as i32 - 1);
            let mut cx = (pw as i32 - total) / 2;
            for (content, radius, key) in &cards {
                let cy = (ph as i32 - content.h as i32) / 2;
                let look = v.sh.glass_look(crate::widgets::own_background(key));
                let glass = v.sh.backdrop.glass_with(content, *radius, sx + cx, sy + cy, blur, tint, look);
                stage.over(&glass, cx, cy);
                cx += content.w as i32 + gap;
            }
            // cantos arredondados do palco
            let mut c = crate::gfx::Canvas::new(pw, ph, s);
            c.push_round_clip(0.0, 0.0, w, h, 12.0);
            c.image(&stage, 0.0, 0.0);
            c.pop_clip();
            st.preview = Some((key, c.finish()));
        }
    }
    if let Some((_, img)) = &st.preview {
        let img = img.clone();
        ui.image(&img, x, y);
    }
    ui.rect(x, y, x + w, y + h, 12.0, None, Some(WHITE.with_a(30)), 1.0);
    ui.text(x + 14.0, y + h - 10.0, "Prévia ao vivo", 10.5, WHITE.with_a(200), Face::UiSemibold, Anchor::LS, None);
}

fn page_widgets(st: &mut SettingsState, ui: &mut Ui, x: f32, w: f32, v: &View) -> f32 {
    let bw = ui.button("editar", x, 4.0, "Editar widgets", Action::Edit, "primary", 'l', None, Some("edit"));
    ui.button("organizar", x + bw + 8.0, 4.0, "Organizar", Action::Arrange, "secondary", 'l', None, Some("refresh"));
    ui.text(x + w, 20.0, "Clique direito num widget para mais opções", 11.0, MUTED, Face::UiRegular, Anchor::RM, None);
    let y = 56.0;
    let gap = 16.0;
    let (tile_w, tile_h, stage_h) = ((w - gap) / 2.0, 262.0, 146.0);
    let s = ui.s();
    for (i, key) in widgets::KEYS.iter().copied().enumerate() {
        let tx = x + (i % 2) as f32 * (tile_w + gap);
        let ty = y + (i / 2) as f32 * (tile_h + gap);
        let visible = v.visible.contains(&key);
        let outline = if visible { ACCENT.with_a(150) } else { WHITE.with_a(16) };
        ui.rect(tx, ty, tx + tile_w, ty + tile_h, 12.0, Some(WHITE.with_a(11)), Some(outline), if visible { 1.2 } else { 1.0 });
        ui.fill(tx + 8.0, ty + 8.0, tx + tile_w - 8.0, ty + 8.0 + stage_h, 9.0, Rgba(0, 0, 0, 34));
        // a miniatura em vidro (guardada: refazer a cada quadro custaria caro)
        let cached = st.thumbs.get(key).filter(|(was_visible, _)| *was_visible == visible).map(|(_, img)| img.clone());
        let thumb = match cached {
            Some(img) => img,
            None => {
                let img = thumbnail(key, ui, tx + 8.0, ty + 8.0, tile_w - 16.0, stage_h, v, GALLERY_ZOOM);
                st.thumbs.insert(key, (visible, img.clone()));
                img
            }
        };
        if let Some(img) = thumb {
            ui.image(&img, tx + 8.0 + (tile_w - 16.0 - img.w as f32 / s) / 2.0, ty + 8.0 + (stage_h - img.h as f32 / s) / 2.0);
        }
        let ny = ty + stage_h + 22.0;
        ui.text(tx + 16.0, ny, widgets::title(key), 14.0, FG, Face::UiSemibold, Anchor::LM, Some(tile_w - 90.0));
        ui.switch(&format!("ver:{key}"), tx + tile_w - 56.0, ny - 11.0, visible, Action::Visible(key, !visible), None);
        for (j, line) in ui.wrap(widgets::description(key), 11.5, tile_w - 32.0, 2).iter().enumerate() {
            ui.text(tx + 16.0, ny + 18.0 + j as f32 * 17.0, line, 11.5, MUTED, Face::UiRegular, Anchor::LA, Some(tile_w - 32.0));
        }
        ui.button(&format!("opcoes:{key}"), tx + 10.0, ty + tile_h - 40.0, "Opções", Action::Go(key), "ghost", 'l', None, None);
    }
    // todas as linhas de cartões (com 5 widgets são 3): senão a rolagem para antes da última
    let rows = widgets::KEYS.len().div_ceil(2) as f32;
    y + rows * (tile_h + gap)
}

/// O widget médio em vidro, a `zoom` do tamanho de verdade, cabendo no palco (x, y, w, h).
#[allow(clippy::too_many_arguments)]
fn thumbnail(key: &'static str, ui: &Ui, x: f32, y: f32, w: f32, h: f32, v: &View, zoom: f32) -> Option<Image> {
    let s = ui.s();
    let (mut content, mut radius) = (v.render)(key, Mode::M, s * zoom)?;
    let max_w = (w - 24.0) * s;
    let max_h = (h - 20.0) * s;
    // grande demais para o palco: reduz (nunca amplia)
    let shrink = (max_w / content.w as f32).min(max_h / content.h as f32).min(1.0);
    if shrink < 1.0 {
        content = crate::gfx::resize(&content, (content.w as f32 * shrink).round() as u32, (content.h as f32 * shrink).round() as u32);
        radius *= shrink;
    }
    let sx = v.origin.0 + ((ui.ox + x) * s) as i32;
    let sy = v.origin.1 + ((ui.oy + y) * s) as i32;
    Some(v.sh.backdrop.glass(&content, radius, sx, sy, v.sh.tint, v.sh.glass_look(crate::widgets::own_background(key))))
}

/// A página de um widget: o alto (a prévia, o tamanho e onde ele aparece) e as opções dele.
fn page_widget(st: &mut SettingsState, ui: &mut Ui, x: f32, w: f32, v: &View, key: &'static str) -> f32 {
    let visible = v.visible.contains(&key);
    let y = hero(st, ui, x, 4.0, w, v, key, visible);
    match key {
        "sistema" => widget_sistema(ui, x, y, w, v),
        "tempo" => widget_tempo(st, ui, x, y, w, v),
        "midia" => widget_midia(ui, x, y, w, v),
        "github" => widget_github(ui, x, y, w, v),
        "bateria" => widget_bateria(ui, x, y, w, v),
        "claude" => widget_claude(ui, x, y, w, v),
        _ => widget_pomodoro(ui, x, y, w, v),
    }
}

/// O interruptor de um aviso (as notificações do Windows) do tipo `kind`.
fn notify_switch(v: &View, kind: &'static str) -> Control {
    sw(notify_id(kind), config::notify_enabled(&v.sh.cfg, kind), Action::Notify(kind))
}

/// O Sistema: o que os anéis abrem e os avisos de CPU, memória e disco.
#[allow(clippy::too_many_arguments)]
fn widget_sistema(ui: &mut Ui, x: f32, y: f32, w: f32, v: &View) -> f32 {
    let y = section(ui, x, y, "Avisos");
    let y = rows(ui, x, y, w, vec![
        Row(NOTIFY[0].1.into(), Some(NOTIFY[0].2.into()), notify_switch(v, "cpu")),
        Row(NOTIFY[1].1.into(), Some(NOTIFY[1].2.into()), notify_switch(v, "ram")),
        Row(NOTIFY[2].1.into(), Some(NOTIFY[2].2.into()), notify_switch(v, "disco")),
    ]);
    notes(ui, x, y, w, &["Clique num anel: a CPU e a memória mostram os 5 apps que mais usam; o disco mostra as unidades."])
}

/// O Tempo: a cidade (com as sugestões por cima do resto), avisar de chuva e o tremor do trovão
/// (o "Atualizar agora" é um atalho no palco, no alto da página).
#[allow(clippy::too_many_arguments)]
fn widget_tempo(st: &mut SettingsState, ui: &mut Ui, x: f32, y: f32, w: f32, v: &View) -> f32 {
    let wcfg = &v.sh.cfg["tempo"];
    let place = wcfg.str_of("place").or(wcfg.str_of("city")).unwrap_or("");
    let y = section(ui, x, y, "Clima");
    let entry_top = y;
    let y = rows(ui, x, y, w, vec![
        Row("Cidade".into(), Some(if place.is_empty() { "Nenhuma cidade escolhida".into() } else { format!("Agora: {place}") }),
            Control::Entry("cidade", place.into(), "Digite e escolha na lista".into(), 300.0)),
        Row("Avisar quando for chover".into(), Some(NOTIFY[3].2.into()), notify_switch(v, "chuva")),
        Row("Tremer com o trovão".into(), Some("Nas trovoadas, o widget treme de leve logo depois de cada raio".into()),
            sw("tempo_tremer", widgets::tempo::shakes(&v.sh.cfg), Action::Toggle("tempo_tremer"))),
    ]);
    let y = notes(ui, x, y, w, &["O clima atualiza sozinho a cada 15 minutos."]);
    // sugestões por cima do resto da página (a primeira linha tem 66 de altura)
    st.city.draw(ui, x + w - 18.0 - 300.0, entry_top + 66.0 / 2.0 + 21.0, 300.0);
    y
}

/// O Tocando agora: qual player ele acompanha, a roda do mouse e a capa em alta do YouTube.
#[allow(clippy::too_many_arguments)]
fn widget_midia(ui: &mut Ui, x: f32, y: f32, w: f32, v: &View) -> f32 {
    let wcfg = &v.sh.cfg["midia"];
    let options = [("auto", "Automático"), ("spotify", "Spotify"), ("youtube", "YouTube")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    let y = section(ui, x, y, "Reprodução");
    let y = rows(ui, x, y, w, vec![
        Row("Mostrar".into(), Some("Qual player o widget acompanha quando há mais de um tocando".into()),
            Control::Seg("fonte", options, wcfg.str_of("fonte").unwrap_or("auto").into())),
        Row("Capa em alta do YouTube".into(), Some("O navegador só passa uma miniatura pequena; esta opção busca a original no youtube.com pelo título e canal do vídeo".into()),
            sw("capa_hd", widgets::media::hd_covers(&v.sh.cfg), Action::Toggle("capa_hd"))),
    ]);
    notes(ui, x, y, w, &["A roda do mouse sobre o widget ajusta o volume do app que está tocando (ou o do Windows)."])
}

/// O GitHub: o usuário e avisar dos pedidos de revisão (o "Atualizar agora" é um atalho no palco,
/// no alto da página).
#[allow(clippy::too_many_arguments)]
fn widget_github(ui: &mut Ui, x: f32, y: f32, w: f32, v: &View) -> f32 {
    let wcfg = &v.sh.cfg["github"];
    let y = section(ui, x, y, "Conta");
    let y = rows(ui, x, y, w, vec![
        Row("Usuário".into(), Some("Com o GitHub CLI (gh) conectado, entram também os repositórios privados".into()),
            Control::Entry("usuario", wcfg.str_of("usuario").unwrap_or("").into(), "seu-usuario".into(), 270.0)),
        Row("Avisar pedidos de revisão".into(), Some(NOTIFY[4].2.into()), notify_switch(v, "github")),
    ]);
    notes(ui, x, y, w, &["As contribuições atualizam sozinhas a cada 10 minutos."])
}

/// A Bateria: o aviso de pouca bateria, quais aparelhos aparecem e o que o clique abre.
#[allow(clippy::too_many_arguments)]
fn widget_bateria(ui: &mut Ui, x: f32, y: f32, w: f32, v: &View) -> f32 {
    let y = section(ui, x, y, "Avisos");
    let y = rows(ui, x, y, w, vec![Row("Avisar com pouca bateria".into(), Some(NOTIFY[6].2.into()), notify_switch(v, "bateria"))]);
    notes(ui, x, y, w, &[
        "Aparecem os aparelhos que informam a bateria ao Windows: a maioria dos fones e dos mouses e teclados Bluetooth. Os que usam um receptor USB próprio só mostram a carga no app do fabricante.",
        "Clique no anel para abrir as configurações de energia; na lista, as de Bluetooth.",
    ])
}

/// O Uso do Claude: a conexão com o Claude Code, o Clawd na barra de tarefas e o tema de Natal.
#[allow(clippy::too_many_arguments)]
fn widget_claude(ui: &mut Ui, x: f32, y: f32, w: f32, v: &View) -> f32 {
    let wcfg = &v.sh.cfg["claude"];
    use crate::data::claude_usage::{self as usage, Link};
    use widgets::claude::{CONNECT, DISCONNECT};
    let (state, control) = match usage::link() {
        Link::Connected => ("Conectado: os números chegam a cada resposta do Claude Code", btn("claude_desconectar", "Desconectar", Action::Widget("claude", DISCONNECT))),
        Link::Off => ("Ainda não conectado", Control::Button("claude_conectar", "Conectar".into(), Action::Widget("claude", CONNECT), "primary", None)),
        Link::Other => ("O Claude Code já usa outra linha de status; tire-a para conectar", Control::None),
        Link::Missing => ("O Claude Code não foi encontrado neste computador", Control::None),
    };
    let y = section(ui, x, y, "Claude Code");
    let y = rows(ui, x, y, w, vec![Row("Conexão".into(), Some(state.into()), control)]);
    let y = notes(ui, x, y, w, &["O widget vira a linha de status do Claude Code, que informa o uso da sessão de 5 horas e da semana, e recebe o aviso de quando ele precisa de você."]);
    let natal = [("dezembro", "Em dezembro"), ("sempre", "Sempre"), ("nunca", "Nunca")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    let y = section(ui, x, y, "Clawd");
    let y = rows(ui, x, y, w, vec![
        Row("Nomes de arquivos no balão".into(), Some("O balão mostra o arquivo ou o comando em uso; desligado, só a ação (bom para compartilhar a tela)".into()),
            sw("claude_nomes", widgets::claude::show_names(&v.sh.cfg), Action::Toggle("claude_nomes"))),
        Row("Tema de Natal".into(), Some("Gorro de Papai Noel e neve caindo".into()),
            Control::Seg("claude_natal", natal, wcfg.str_of("natal").unwrap_or("dezembro").into())),
    ]);
    notes(ui, x, y, w, &["Clique no Clawd e ele pula para cima da barra de tarefas; outro clique o traz de volta."])
}

/// O Pomodoro: as durações, o comportamento e o total de hoje.
#[allow(clippy::too_many_arguments)]
fn widget_pomodoro(ui: &mut Ui, x: f32, y: f32, w: f32, v: &View) -> f32 {
    let wcfg = &v.sh.cfg["pomodoro"];
    let durations = &wcfg["duracoes"];
    let mut duration_rows = Vec::new();
    for (phase, label, lo, hi, step) in PHASE_RANGES {
        let default = match phase { "foco" => 25, "pausa" => 5, _ => 15 };
        let value = durations.i64_of(phase).unwrap_or(default) as f64;
        let id: &'static str = match phase { "foco" => "dur:foco", "pausa" => "dur:pausa", _ => "dur:longa" };
        duration_rows.push(Row(label.into(), None, Control::Slider(id, value, lo, hi, step, pomodoro::fmt_minutes(value))));
    }
    let every = wcfg.i64_of("longa_a_cada").unwrap_or(4);
    duration_rows.push(Row("Pausa longa a cada".into(), None, Control::Seg("longa_a_cada", (2..=6).map(|n| (n.to_string(), n.to_string())).collect(), every.to_string())));
    let y = section(ui, x, y, "Durações");
    let y = rows(ui, x, y, w, duration_rows);
    let y = section(ui, x, y, "Comportamento");
    let y = rows(ui, x, y, w, vec![
        Row("Começar a próxima fase sozinho".into(), Some("Sem precisar clicar quando o foco ou a pausa acaba".into()),
            sw("pomodoro_auto", wcfg.bool_of("auto").unwrap_or(false), Action::Toggle("pomodoro_auto"))),
        Row("Avisar no fim de cada fase".into(), Some(NOTIFY[5].2.into()), notify_switch(v, "pomodoro")),
    ]);
    let date = crate::platform::win::local_time().iso_date();
    let stats = &wcfg["hoje"];
    let (done, minutes) = if stats.str_of("data") == Some(date.as_str()) { (stats.i64_of("pomodoros").unwrap_or(0), stats.i64_of("minutos").unwrap_or(0)) } else { (0, 0) };
    let y = section(ui, x, y, "Hoje");
    rows(ui, x, y, w, vec![Row(
        format!("{done} pomodoro{}  ·  {} de foco", if done != 1 { "s" } else { "" }, pomodoro::fmt_minutes(minutes as f64)),
        Some("A contagem recomeça sozinha à meia-noite".into()),
        if done > 0 { btn("zerar", "Zerar o dia", Action::Widget("pomodoro", pomodoro::RESET_DAY)) } else { Control::None },
    )])
}

/// O tamanho das miniaturas da galeria e da prévia no alto da página de um widget (fração do
/// widget de verdade).
const GALLERY_ZOOM: f32 = 0.72;
const HERO_ZOOM: f32 = 1.0;
/// O alto da página de um widget: a altura do palco.
const HERO_H: f32 = 252.0;
/// A barra de tamanho em cima do palco: a altura, o espaço dela até a base do palco e a caixa em
/// que a forma de cada tamanho cabe (largura, altura).
const SIZE_BAR_H: f32 = 32.0;
const SIZE_BAR_BOTTOM: f32 = 12.0;
const SIZE_SHAPE_BOX: (f32, f32) = (16.0, 14.0);
/// Os atalhos no canto do palco: a altura de cada um e o espaço até a borda.
const CHIP_H: f32 = 30.0;
const CHIP_INSET: f32 = 12.0;

/// O alto da página de um widget: o widget grande, em vidro, sobre o papel de parede de verdade
/// (o que está atrás da janela, como ele fica na área de trabalho), com o tamanho numa barra em
/// cima do palco e os atalhos dele (como "Atualizar agora") no canto; embaixo, se ele aparece na
/// ilha. O "Na área de trabalho" fica no título da página. Devolve o y logo abaixo.
#[allow(clippy::too_many_arguments)]
fn hero(st: &mut SettingsState, ui: &mut Ui, x: f32, y: f32, w: f32, v: &View, key: &'static str, visible: bool) -> f32 {
    let h = HERO_H;
    #[cfg(test)]
    tests::placed(x, y, y + h);
    draw_wallpaper_stage(st, ui, v, (x, y, w, h));
    let cached = st.hero.as_ref().filter(|(k, vis, _)| *k == key && *vis == visible).map(|(_, _, img)| img.clone());
    let thumb = match cached {
        Some(img) => img,
        None => {
            let img = thumbnail(key, ui, x, y, w, h - SIZE_BAR_H - SIZE_BAR_BOTTOM - 8.0, v, HERO_ZOOM);
            st.hero = Some((key, visible, img.clone()));
            img
        }
    };
    let s = ui.s();
    if let Some(img) = thumb {
        // o widget no meio do espaço acima da barra de tamanho
        let room = h - SIZE_BAR_H - SIZE_BAR_BOTTOM;
        ui.image(&img, x + (w - img.w as f32 / s) / 2.0, y + (room - img.h as f32 / s) / 2.0);
    }
    let current = v.modes.get(key).copied().or_else(|| v.sh.cfg[key].str_of("tamanho").and_then(Mode::from_code)).unwrap_or(widgets::DEFAULT_MODE);
    size_bar(ui, key, x + w / 2.0, y + h - SIZE_BAR_BOTTOM - SIZE_BAR_H, current);
    let mut chip_x = x + w - CHIP_INSET;
    for (id, label, action) in hero_actions(key, visible).into_iter().rev() {
        chip_x -= chip(ui, id, chip_x, y + CHIP_INSET, label, action) + 8.0;
    }
    let y = y + h + 18.0;
    if ISLAND_KEYS.contains(&key) {
        let on = widgets::on_island(&v.sh.cfg, key);
        let desc = if config::setting_bool(&v.sh.cfg, "ilha_widgets") { island_description(key) } else { "A ilha está sem widgets agora: ligue em Ilha dinâmica" };
        rows(ui, x, y, w, vec![Row("Na ilha dinâmica".into(), Some(desc.into()), sw(island_id(key), on, Action::Toggle(island_id(key))))])
    } else if key == "midia" {
        notes(ui, x, y + NOTE_PULL, w, &["Clique no widget e ele vira a ilha dinâmica, com a capa e os controles."])
    } else {
        y + 8.0
    }
}

/// Os atalhos de um widget no palco da página dele: (id, rótulo, ação).
fn hero_actions(key: &str, visible: bool) -> Vec<(&'static str, &'static str, Action)> {
    match key {
        "tempo" if visible => vec![("atualizar_clima", "Atualizar agora", Action::Widget("tempo", widgets::tempo::UPDATE_WEATHER))],
        "github" if visible => vec![("atualizar_github", "Atualizar agora", Action::Widget("github", widgets::github::UPDATE))],
        _ => Vec::new(),
    }
}

/// Um atalho no palco: uma pílula escura translúcida com o ícone de atualizar e o rótulo, com a
/// borda direita em `right`. Devolve a largura.
fn chip(ui: &mut Ui, id: &str, right: f32, y: f32, label: &str, action: Action) -> f32 {
    let w = ui.text_width(label, 11.5, Face::UiSemibold) + 42.0;
    let hot = ui.hot(id);
    ui.rect(right - w, y, right, y + CHIP_H, CHIP_H / 2.0, Some(Rgba(0, 0, 0, if hot { 150 } else { 110 })), Some(WHITE.with_a(40)), 1.0);
    ui.icon(right - w + 17.0, y + CHIP_H / 2.0, "refresh", 12.0, WHITE);
    ui.text(right - w + 29.0, y + CHIP_H / 2.0, label, 11.5, WHITE, Face::UiSemibold, Anchor::LM, None);
    ui.region(id, right - w, y, right, y + CHIP_H, action, Cursor::Hand);
    w
}

/// O palco do alto da página de um widget (x, y, largura, altura): o papel de parede de verdade,
/// nítido e um pouco escurecido, que fica atrás desta parte da janela (guardado enquanto a janela
/// não anda), com os cantos arredondados e uma borda fina.
fn draw_wallpaper_stage(st: &mut SettingsState, ui: &mut Ui, v: &View, (x, y, w, h): (f32, f32, f32, f32)) {
    let s = ui.s();
    let sx = v.origin.0 + ((ui.ox + x) * s).round() as i32;
    let sy = v.origin.1 + ((ui.oy + y) * s).round() as i32;
    let (pw, ph) = ((w * s).round() as u32, (h * s).round() as u32);
    let key = (sx, sy, pw, ph, v.sh.backdrop.version);
    if st.stage.as_ref().map(|stage| stage.0) != Some(key) {
        let mut wall = v.sh.backdrop.region(sx, sy, pw, ph, true).to_image();
        // um pouco mais escuro, para o vidro e os botões por cima se destacarem
        for pixel in wall.px.chunks_exact_mut(4) {
            for channel in &mut pixel[..3] {
                *channel = (*channel as f32 * STAGE_SHADE).round() as u8;
            }
        }
        st.stage = Some((key, wall));
    }
    let (gx, gy) = (x + ui.ox, y + ui.oy);
    ui.c.push_round_clip(gx, gy, gx + w, gy + h, STAGE_RADIUS);
    if let Some((_, wall)) = &st.stage {
        let wall = wall.clone();
        ui.image(&wall, x, y);
    }
    ui.c.pop_clip();
    ui.rect(x, y, x + w, y + h, STAGE_RADIUS, None, Some(WHITE.with_a(26)), 1.0);
}

/// O palco: o raio dos cantos e quanto o papel de parede escurece nele.
const STAGE_RADIUS: f32 = 14.0;
const STAGE_SHADE: f32 = 0.82;

/// A barra de tamanho em cima do palco, centrada em `cx`: uma pílula escura translúcida com os
/// três tamanhos (o desenho de cada um e o nome), o escolhido em branco. Cada um responde como uma
/// opção do seletor segmentado (`Action::Seg` com o id do tamanho do widget).
fn size_bar(ui: &mut Ui, key: &str, cx: f32, y: f32, current: Mode) {
    let id = size_id(key);
    // pontos da forma por ponto do widget: o maior dos três cabendo na caixa
    let unit = Mode::ALL.iter().map(|&m| {
        let l = widgets::Layout::of(m);
        (SIZE_SHAPE_BOX.0 / l.w).min(SIZE_SHAPE_BOX.1 / l.h)
    }).fold(f32::INFINITY, f32::min);
    let widths: Vec<f32> = Mode::ALL.iter().map(|m| ui.text_width(m.name(), 11.5, Face::UiSemibold) + SIZE_SHAPE_BOX.0 + 30.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 6.0;
    let x0 = cx - total / 2.0;
    ui.rect(x0, y, x0 + total, y + SIZE_BAR_H, SIZE_BAR_H / 2.0, Some(Rgba(0, 0, 0, 120)), Some(WHITE.with_a(40)), 1.0);
    let mut ox = x0 + 3.0;
    for (&mode, bw) in Mode::ALL.iter().zip(widths) {
        let option = format!("{id}:{}", mode.code());
        let (active, hot) = (mode == current, ui.hot(&option));
        if active {
            ui.fill(ox, y + 3.0, ox + bw, y + SIZE_BAR_H - 3.0, (SIZE_BAR_H - 6.0) / 2.0, WHITE.with_a(235));
        } else if hot {
            ui.fill(ox, y + 3.0, ox + bw, y + SIZE_BAR_H - 3.0, (SIZE_BAR_H - 6.0) / 2.0, WHITE.with_a(30));
        }
        let ink = if active { DARK_GLYPH } else { WHITE.with_a(200) };
        let l = widgets::Layout::of(mode);
        let (shape_w, shape_h) = (l.w * unit, l.h * unit);
        let shape_cx = ox + 12.0 + SIZE_SHAPE_BOX.0 / 2.0;
        let cy = y + SIZE_BAR_H / 2.0;
        ui.fill(shape_cx - shape_w / 2.0, cy - shape_h / 2.0, shape_cx + shape_w / 2.0, cy + shape_h / 2.0, 2.0, ink);
        ui.text(ox + 18.0 + SIZE_SHAPE_BOX.0, cy, mode.name(), 11.5, ink, Face::UiSemibold, Anchor::LM, None);
        ui.region(&option, ox, y, ox + bw, y + SIZE_BAR_H, Action::Seg(id, mode.code().to_string()), Cursor::Hand);
        ox += bw;
    }
}
pub fn size_id(key: &str) -> &'static str {
    match key {
        "tempo" => "tamanho:tempo",
        "sistema" => "tamanho:sistema",
        "midia" => "tamanho:midia",
        "github" => "tamanho:github",
        "claude" => "tamanho:claude",
        "bateria" => "tamanho:bateria",
        _ => "tamanho:pomodoro",
    }
}

fn notify_id(kind: &str) -> &'static str {
    match kind {
        "cpu" => "aviso:cpu",
        "ram" => "aviso:ram",
        "disco" => "aviso:disco",
        "chuva" => "aviso:chuva",
        "github" => "aviso:github",
        "bateria" => "aviso:bateria",
        _ => "aviso:pomodoro",
    }
}

fn page_avisos(ui: &mut Ui, x: f32, w: f32, v: &View) -> f32 {
    let notify_rows = NOTIFY.iter().map(|&(kind, title, desc)| Row(title.into(), Some(desc.into()), notify_switch(v, kind))).collect();
    let y = section(ui, x, 4.0, "Avisar quando");
    let y = rows(ui, x, y, w, notify_rows);
    let y = rows(ui, x, y, w, vec![Row("Testar".into(), Some("Mostra uma notificação de exemplo agora".into()), btn("testar", "Enviar teste", Action::TestNotify))]);
    y
}

fn page_dicas(ui: &mut Ui, x: f32, w: f32) -> f32 {
    let y = section(ui, x, 4.0, "Mouse");
    rows(ui, x, y, w, TIPS.iter().map(|(t, d)| Row(t.to_string(), Some(d.to_string()), Control::None)).collect())
}

fn page_sobre(ui: &mut Ui, x: f32, w: f32, v: &View) -> f32 {
    let icon = crate::ui::tray::icon_image((56.0 * ui.s()).round() as u32);
    ui.image(&icon, x, 4.0);
    ui.text(x + 74.0, 12.0, "Widgets da área de trabalho", 18.0, FG, Face::UiSemibold, Anchor::LA, None);
    ui.text(x + 74.0, 40.0, &format!("Versão {VERSION}  ·  feito em Rust, com Direct2D"), 12.0, MUTED, Face::UiRegular, Anchor::LA, None);
    let update = match &v.update_version {
        Some(ver) => Control::Button("atualizar", format!("Atualizar para {ver}"), Action::Widget("app", 1), "primary", Some("refresh")),
        None => Control::Button("procurar", "Procurar agora".into(), Action::Widget("app", 2), "secondary", Some("refresh")),
    };
    let y = rows(ui, x, 90.0, w, vec![
        Row("Atualizações".into(), Some(v.update_status.clone()), update),
        Row("Procurar atualizações sozinho".into(), Some("A cada 6 horas, pergunta ao GitHub qual é a versão mais recente (não envia nada sobre você)".into()),
            setting_switch(v, "procurar_atualizacoes")),
    ]);
    let data = crate::data::config::data_dir().display().to_string();
    rows(ui, x, y, w, vec![
        Row("Boas-vindas".into(), Some("Rever a apresentação e escolher os widgets de novo".into()), btn("boas_vindas", "Abrir", Action::Welcome)),
        Row("Pasta das configurações".into(), Some(data), Control::Button("pasta", "Abrir pasta".into(), Action::OpenData, "secondary", Some("folder"))),
        Row("Restaurar todas as configurações".into(), Some("Aparência, comportamento e notificações (layouts ficam)".into()),
            Control::Button("restaurar_tudo", "Restaurar".into(), Action::ResetAll, "danger", None)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O texto das notificações fala dos mesmos limites que a notificação e a ilha usam.
    #[test]
    fn notify_texts_show_the_real_limits() {
        let text = |kind: &str| NOTIFY.iter().find(|(k, _, _)| *k == kind).unwrap().2;
        let high = format!("{} por {}", crate::data::config::high_use_pct(), crate::data::config::high_use_time());
        assert!(text("cpu").contains(&high), "{}", text("cpu"));
        assert!(text("ram").contains(&high), "{}", text("ram"));
        assert!(text("bateria").contains(&format!("{}%", crate::data::config::LOW_BATTERY)), "{}", text("bateria"));
    }

    #[test]
    fn slider_rows_leave_room_for_the_text() {
        // o texto da linha fica com o que sobra da largura do controle: ela tem que cobrir o
        // slider inteiro (o valor, a trilha e a bolinha na ponta esquerda) e ainda um respiro
        let slider = Control::Slider("x", 50.0, 0.0, 100.0, 1.0, "50%".into());
        let drawn = SLIDER_VALUE + SLIDER_W + SLIDER_KNOB;
        assert!(slider.width(600.0) >= drawn + 16.0, "slider encostado no texto: {} < {}", slider.width(600.0), drawn + 16.0);
        // e não come a linha: o texto fica com mais da metade do cartão (a janela tem largura fixa)
        let card = W - SIDEBAR - 64.0;
        assert!(slider.width(card) < card * 0.5);
    }
    use crate::ui::panel::Anims;

    fn com() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
    }

    /// Desenha uma página das Configurações (como a janela desenha) e devolve a imagem e a altura
    /// do conteúdo. `scroll`: quanto a página está rolada.
    fn page(name: &'static str, scroll: f32, s: f32) -> (Image, f32, f32) {
        // as notificações dos apps ligadas, com dois apps já vistos (a página da ilha inteira)
        let cfg = serde_json::json!({"tempo": {}, "sistema": {}, "midia": {}, "github": {}, "pomodoro": {}, "app": {}, apps::KEY: {"na_ilha": true, apps::MUTED_FIELD: ["Teams!App"]}});
        let cache = serde_json::json!({apps::KEY: {apps::SEEN_FIELD: {"WhatsApp!App": "WhatsApp", "Teams!App": "Microsoft Teams"}}});
        let sh = Shared::new(cfg, cache, s);
        let render = |key: &'static str, mode: Mode, scale: f32| {
            let kind = widgets::sample(key, &sh.cfg, &sh.cache)?;
            Some((crate::card::render_kind(kind.as_ref(), mode, scale, &sh), widgets::Layout::of(mode).radius * scale))
        };
        let view = View {
            sh: &sh,
            visible: widgets::KEYS.to_vec(),
            modes: HashMap::new(),
            origin: (0, 0),
            render: &render,
            startup: false,
            update_status: String::new(),
            update_version: None,
            notify_access: Access::Allowed,
        };
        let mut st = SettingsState::new(name);
        st.scrolls.insert(name, scroll);
        let mut c = crate::gfx::Canvas::new((W * s) as u32, (H * s) as u32, s);
        c.fill_round_rect(0.0, 0.0, W, H, 0.0, Rgba::rgb(28, 28, 34));
        let mut anims = Anims::default();
        let mut ui = Ui {
            c: &mut c,
            regions: Vec::new(),
            entries: Vec::new(),
            hover: None,
            pressed: None,
            focus: None,
            anims: &mut anims,
            animations: false,
            ox: 0.0,
            oy: 0.0,
            clip: (0.0, 0.0, W, H),
        };
        draw(&mut st, &mut ui, &view);
        let max_scroll = (st.content_h - (H - TOP)).max(0.0);
        (c.finish(), st.content_h, max_scroll)
    }

    #[test]
    fn gallery_scrolls_to_every_widget() {
        com();
        let (_, content_h, max_scroll) = page("widgets", 0.0, 1.0);
        // a galeria tem um cartão por widget, 2 por linha, com 262 de altura e 16 entre eles
        let rows = widgets::KEYS.len().div_ceil(2) as f32;
        assert!(content_h >= 56.0 + rows * (262.0 + 16.0), "altura {content_h}");
        assert!(max_scroll > 0.0, "a página precisa rolar até o último cartão");
    }

    /// O menu lateral não cabia na janela e não rolava: os últimos itens (Notificações, Dicas,
    /// Sobre) ficavam fora de alcance. Agora a janela tem a altura do menu inteiro; um widget
    /// novo que o faça passar daqui pede uma janela mais alta.
    #[test]
    fn sidebar_fits_without_scrolling() {
        com();
        let room = NAV_BOTTOM - NAV_TOP;
        assert!(nav_height() <= room, "o menu ({}) não cabe na ilha ({room}): aumente H", nav_height());
        let regions = sidebar_regions("sobre");
        let pages: Vec<&str> = nav_items().into_iter().map(|(page, _, _)| page).filter(|p| *p != "-").collect();
        for page in pages {
            let id = format!("nav:{page}");
            let (_, rect) = regions.iter().find(|(region, _)| *region == id).unwrap_or_else(|| panic!("{id} não é clicável"));
            assert!(rect.1 >= NAV_TOP - 0.5 && rect.3 <= NAV_BOTTOM + 0.5, "{id} fora da ilha: {rect:?}");
        }
    }

    /// O cabeçalho da página fica na altura do nome do app na ilha, a página começa na altura da
    /// busca do menu, e a lista do menu, logo abaixo da busca.
    #[test]
    fn page_lines_up_with_the_sidebar() {
        assert_eq!(TOP, SEARCH_TOP);
        assert!(NAV_TOP > SEARCH_TOP + SEARCH_H, "a lista começa embaixo da busca");
        assert!(HEADER_CY > NAV_MARGIN && HEADER_CY < SEARCH_TOP);
        let first = nav_spots().into_iter().next().unwrap();
        // o primeiro item e o título da primeira seção (y = 4 em todas as páginas) começam juntos
        assert_eq!(first.y, 4.0);
    }

    /// As áreas clicáveis (id, retângulo) da janela das Configurações aberta em `page_name`.
    fn sidebar_regions(page_name: &'static str) -> Vec<(String, (f32, f32, f32, f32))> {
        let cfg = serde_json::json!({"tempo": {}, "sistema": {}, "midia": {}, "github": {}, "pomodoro": {}, "app": {}});
        let sh = Shared::new(cfg, serde_json::json!({}), 1.0);
        let render = |_: &'static str, _: Mode, _: f32| None;
        let view = View {
            sh: &sh,
            visible: widgets::KEYS.to_vec(),
            modes: HashMap::new(),
            origin: (0, 0),
            render: &render,
            startup: false,
            update_status: String::new(),
            update_version: None,
            notify_access: Access::Allowed,
        };
        let mut st = SettingsState::new(page_name);
        let mut c = crate::gfx::Canvas::new(W as u32, H as u32, 1.0);
        let mut anims = Anims::default();
        let mut ui = Ui { c: &mut c, regions: Vec::new(), entries: Vec::new(), hover: None, pressed: None, focus: None, anims: &mut anims, animations: false, ox: 0.0, oy: 0.0, clip: (0.0, 0.0, W, H) };
        draw(&mut st, &mut ui, &view);
        ui.regions.into_iter().map(|region| (region.id, region.rect)).collect()
    }

    thread_local! {
        /// títulos de seção e cartões desenhados: (x, topo, base)
        static PLACED: std::cell::RefCell<Vec<(f32, f32, f32)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    pub(super) fn placed(x: f32, top: f32, bottom: f32) {
        PLACED.with(|p| p.borrow_mut().push((x, top, bottom)));
    }

    thread_local! {
        /// as descrições que não couberam em duas linhas (a segunda acaba em "…")
        static CUT: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    pub(super) fn cut(desc: &str) {
        CUT.with(|c| c.borrow_mut().push(desc.to_string()));
    }

    /// A busca acha as opções de qualquer página, sem ligar para acentos e maiúsculas, e cada
    /// resultado leva à página dele, rolada até a opção (que pisca).
    #[test]
    fn search_finds_options_on_every_page() {
        com();
        let cfg = serde_json::json!({"tempo": {}, "sistema": {}, "midia": {}, "github": {}, "pomodoro": {}, "app": {}});
        let sh = Shared::new(cfg, serde_json::json!({}), 1.0);
        let render = |_: &'static str, _: Mode, _: f32| None;
        let view = View { sh: &sh, visible: widgets::KEYS.to_vec(), modes: HashMap::new(), origin: (0, 0), render: &render, startup: false, update_status: String::new(), update_version: None, notify_access: Access::Allowed };
        let index = search_index(&view);
        let pages = |query: &str| search_results(&index, query).iter().map(|found| found.page).collect::<Vec<_>>();
        assert!(pages("chuva").contains(&"tempo") && pages("chuva").contains(&"avisos"), "{:?}", pages("chuva"));
        assert!(pages("ESCURECIMENTO").contains(&"aparencia"));
        assert!(pages("notificacoes dos apps").contains(&"ilha"), "sem acento: {:?}", pages("notificacoes dos apps"));
        assert!(pages("xyzzy").is_empty());
        // o resultado leva à página, rolada até a linha, e ela pisca
        let found = search_results(&index, "tremer").first().map(|found| (found.page, found.y)).expect("tremer");
        let mut st = SettingsState::new("geral");
        st.searched("tremer");
        assert!(st.searching());
        st.find(found.0, found.1);
        assert!(!st.searching() && st.page == "tempo" && st.flash.is_some());
        assert_eq!(st.scrolls.get("tempo").copied(), Some((found.1 - FIND_MARGIN).max(0.0)));
    }

    /// Uma opção fora do padrão ganha o "Desfazer": o próprio clique num interruptor, o valor
    /// padrão num slider ou num seletor; no padrão, nada.
    #[test]
    fn options_off_their_default_can_be_undone() {
        let on = |id: &'static str, value: bool| Control::Switch(id, value, Action::Toggle(id));
        assert_eq!(undo_of(&on("dicas", false)), Some(Action::Toggle("dicas")));
        assert_eq!(undo_of(&on("dicas", true)), None);
        assert_eq!(undo_of(&Control::Slider("desfoque", 20.0, 8.0, 40.0, 1.0, String::new())), Some(Action::SliderSet("desfoque", BLUR)));
        assert_eq!(undo_of(&Control::Slider("desfoque", BLUR, 8.0, 40.0, 1.0, String::new())), None);
        assert_eq!(undo_of(&Control::Seg("animacoes", Vec::new(), "nunca".into())), Some(Action::Seg("animacoes", "sempre".into())));
        // sem um padrão conhecido (depende do Windows): sem a marca
        assert_eq!(undo_of(&on("iniciar", false)), None);
    }

    /// Nenhuma descrição das Configurações fica cortada com "…" (cabe em duas linhas, ao lado do
    /// controle da linha).
    #[test]
    fn descriptions_are_never_cut() {
        com();
        let pages: Vec<&'static str> = nav_items().into_iter().map(|(page, _, _)| page).filter(|p| *p != "-").collect();
        let mut cut_off = Vec::new();
        for page in pages {
            CUT.with(|c| c.borrow_mut().clear());
            let (_, _, max_scroll) = page_draw(page, 0.0);
            let _ = page_draw(page, max_scroll);
            cut_off.extend(CUT.with(|c| c.borrow().clone()).into_iter().map(|d| format!("{page}: {d}")));
        }
        cut_off.dedup();
        assert!(cut_off.is_empty(), "cortadas: {cut_off:#?}");
    }

    #[test]
    fn titles_and_cards_never_overlap() {
        // cada título fica acima do seu cartão, e o cartão seguinte começa abaixo do anterior
        // (a página do Claude Code desenhava a segunda seção por cima da primeira)
        com();
        let pages: Vec<&'static str> = nav_items().into_iter().map(|(page, _, _)| page).filter(|p| *p != "-").collect();
        for page in pages {
            PLACED.with(|p| p.borrow_mut().clear());
            let _ = page_draw(page, 0.0);
            let mut placed = PLACED.with(|p| p.borrow().clone());
            placed.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap());
            for pair in placed.windows(2) {
                let ((xa, _, bottom), (xb, top, _)) = (pair[0], pair[1]);
                if xa == xb {
                    assert!(top >= bottom - 0.5, "{page}: um bloco em y={top} começa antes do anterior acabar (y={bottom})");
                }
            }
        }
    }

    #[test]
    fn every_page_draws_at_the_top_and_at_the_end() {
        com();
        let pages: Vec<&'static str> = nav_items().into_iter().map(|(page, _, _)| page).filter(|p| *p != "-").collect();
        assert!(pages.len() >= widgets::KEYS.len() + 5, "{pages:?}");
        for page in pages {
            let (_, content_h, max_scroll) = page_draw(page, 0.0);
            assert!(content_h > 0.0, "{page}: página vazia");
            let _ = page_draw(page, max_scroll);
            // rolar além do fim não quebra
            let _ = page_draw(page, max_scroll + 500.0);
        }
    }

    #[test]
    fn every_welcome_step_draws() {
        com();
        let cfg = serde_json::json!({"tempo": {}, "sistema": {}, "midia": {}, "github": {}, "pomodoro": {}, "app": {}});
        let sh = Shared::new(cfg, serde_json::json!({}), 1.0);
        let render = |key: &'static str, mode: Mode, scale: f32| {
            let kind = widgets::sample(key, &sh.cfg, &sh.cache)?;
            Some((crate::card::render_kind(kind.as_ref(), mode, scale, &sh), widgets::Layout::of(mode).radius * scale))
        };
        let view = View {
            sh: &sh,
            visible: widgets::KEYS.to_vec(),
            modes: HashMap::new(),
            origin: (0, 0),
            render: &render,
            startup: false,
            update_status: String::new(),
            update_version: None,
            notify_access: Access::Allowed,
        };
        for step in 0..3 {
            let mut st = crate::ui::prompt::WelcomeState::new(&sh.cfg, widgets::KEYS, false, true);
            st.step = step;
            let (w, h) = (crate::ui::prompt::WELCOME_W, crate::ui::prompt::WELCOME_H);
            let mut c = crate::gfx::Canvas::new(w as u32, h as u32, 1.0);
            let mut anims = Anims::default();
            let mut ui = Ui { c: &mut c, regions: Vec::new(), entries: Vec::new(), hover: None, pressed: None, focus: None, anims: &mut anims, animations: false, ox: 0.0, oy: 0.0, clip: (0.0, 0.0, w, h) };
            crate::ui::prompt::draw_welcome(&mut st, &mut ui, &view);
            assert!(!ui.regions.is_empty(), "passo {step} sem nada clicável");
        }
    }

    fn page_draw(name: &'static str, scroll: f32) -> (Image, f32, f32) {
        page(name, scroll, 1.0)
    }

    /// As páginas de Aparência e da Galeria (no alto e rolada até o fim) num PNG:
    ///   cargo test settings_pages -- --ignored --nocapture
    #[test]
    #[ignore]
    fn settings_pages() {
        com();
        let pages = ["geral", "ilha", "aparencia", "widgets", "pomodoro", "midia", "avisos", "dicas", "sobre"];
        let mut list: Vec<(&'static str, bool)> = pages.iter().map(|p| (*p, false)).collect();
        list.push(("widgets", true));
        list.push(("geral", true));
        list.push(("ilha", true));
        for (name, end) in list {
            let (_, _, max_scroll) = page(name, 0.0, 1.25);
            let (img, _, _) = page(name, if end { max_scroll } else { 0.0 }, 1.25);
            let path = std::env::temp_dir().join(format!("config_{name}{}.png", if end { "_fim" } else { "" }));
            crate::gfx::save_png(&img, &path).unwrap();
            println!("{}", path.display());
        }
        // a página da ilha é comprida: o meio também (as notificações dos apps)
        let (_, _, max_scroll) = page("ilha", 0.0, 1.25);
        let (img, _, _) = page("ilha", max_scroll * 0.38, 1.25);
        let path = std::env::temp_dir().join("config_ilha_meio.png");
        crate::gfx::save_png(&img, &path).unwrap();
        println!("{}", path.display());
    }
}

