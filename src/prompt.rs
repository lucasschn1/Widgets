//! Pergunta de vidro (um campo de texto) aberta junto do widget, e as boas-vindas em 3 passos.

use std::collections::HashMap;

use serde_json::Value;

use crate::config::Get;
use crate::events::Cursor;
use crate::gfx::{Anchor, Face, Image};
use crate::panel::{Suggest, Ui, A};
use crate::settings::{View, TIPS};
use crate::theme::*;
use crate::widgets::{self, Mode};

// ---------- pergunta ----------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Purpose {
    City,
    GitHubUser,
    LayoutName,
}

pub struct PromptState {
    pub purpose: Purpose,
    pub title: String,
    pub message: String,
    pub value: String,
    pub placeholder: String,
    pub ok_label: String,
    pub suggest: Suggest,
    pub picked: Option<Value>,
}

pub const PROMPT_W: f32 = 380.0;

impl PromptState {
    pub fn height(&self) -> f32 {
        let lines = if self.message.chars().count() > 52 { 2.0 } else { 1.0 };
        let sug = if self.suggest.results.is_empty() { 0.0 } else { self.suggest.height() + 8.0 };
        150.0 + 17.0 * lines + sug
    }
}

pub fn draw_prompt(st: &PromptState, ui: &mut Ui, h: f32) {
    let w = PROMPT_W;
    ui.text(20.0, 18.0, &st.title, 15.0, FG, Face::Semibold, Anchor::LA, Some(w - 40.0));
    let mut y = 44.0;
    for line in ui.wrap(&st.message, 12.0, w - 40.0, 2) {
        ui.text(20.0, y, &line, 12.0, MUTED, Face::Regular, Anchor::LA, None);
        y += 17.0;
    }
    y += 10.0;
    ui.entry("valor", 20.0, y, w - 20.0, y + 36.0, &st.value, &st.placeholder, false);
    y += 36.0;
    if st.purpose == Purpose::City {
        if !st.suggest.results.is_empty() {
            y += 6.0;
        }
        st.suggest.draw(ui, 20.0, y, w - 40.0);
    }
    let by = h - 52.0;
    let ok_w = ui.button("ok", w - 20.0, by, &st.ok_label, A::Ok, "primary", 'r', Some(96.0), None);
    ui.button("cancelar", w - 20.0 - ok_w - 8.0, by, "Cancelar", A::Cancel, "secondary", 'r', Some(96.0), None);
}

// ---------- boas-vindas ----------
//
// Três passos na mesma moldura: em cima, os pontos dos passos, o título e a frase; embaixo, o
// rodapé com os botões; no meio, a área do passo (de CONTENT_Y, com CONTENT_H de altura), que
// cada passo ocupa sem passar dela.

pub const WELCOME_W: f32 = 760.0;
pub const WELCOME_H: f32 = 580.0;
/// margem dos lados
const SIDE: f32 = 48.0;
/// onde começa a área do passo e onde fica a linha do rodapé
const CONTENT_Y: f32 = 138.0;
const FOOT_LINE: f32 = WELCOME_H - 68.0;
const CONTENT_H: f32 = FOOT_LINE - 18.0 - CONTENT_Y;
const BUTTONS_Y: f32 = WELCOME_H - 50.0;

const STEPS: [(&str, &str); 3] = [
    ("Bem-vindo aos Widgets", "Escolha o que você quer na área de trabalho. Dá para mudar depois, pelo clique direito."),
    ("Deixe com a sua cara", "Algumas informações para os widgets mostrarem o que importa para você."),
    ("Vamos começar", "Quatro dicas para aproveitar melhor os widgets."),
];

pub struct WelcomeState {
    pub step: usize,
    pub chosen: HashMap<&'static str, bool>,
    pub city_text: String,
    pub city_place: Option<Value>,
    pub user: String,
    pub startup: bool,
    pub city: Suggest,
    thumbs: HashMap<&'static str, Option<Image>>,
}

impl WelcomeState {
    pub fn new(cfg: &Value, visible: &[&'static str], startup: bool, first_time: bool) -> WelcomeState {
        let mut chosen: HashMap<&'static str, bool> = widgets::KEYS.iter().map(|k| (*k, visible.contains(k))).collect();
        if !chosen.values().any(|v| *v) {
            chosen.values_mut().for_each(|v| *v = true);
        }
        let system = &cfg["tempo"];
        WelcomeState {
            step: 0,
            chosen,
            city_text: system.str_of("place").or(system.str_of("city")).unwrap_or("").into(),
            city_place: None,
            user: cfg["github"].str_of("usuario").unwrap_or("").into(),
            startup: startup || first_time,
            city: Suggest::default(),
            thumbs: HashMap::new(),
        }
    }

    pub fn last(&self) -> bool {
        self.step == STEPS.len() - 1
    }
}

pub fn draw_welcome(st: &mut WelcomeState, ui: &mut Ui, v: &View) {
    let w = WELCOME_W;
    // pontos dos passos (os já vistos voltam com um clique)
    for i in 0..STEPS.len() {
        let cx = w / 2.0 + (i as f32 - 1.0) * 22.0;
        let width = ui.anim(&format!("dot{i}"), if i == st.step { 22.0 } else { 8.0 }, 220);
        ui.fill(cx - width / 2.0, 26.0, cx + width / 2.0, 34.0, 4.0, if i <= st.step { ACCENT } else { WHITE.with_a(50) });
        if i < st.step {
            ui.region(&format!("dot{i}"), cx - 11.0, 18.0, cx + 11.0, 42.0, A::Step(i), Cursor::Hand);
        }
    }
    let (title, subtitle) = STEPS[st.step];
    ui.text(w / 2.0, 56.0, title, 24.0, FG, Face::Semibold, Anchor::MA, None);
    for (i, line) in ui.wrap(subtitle, 12.5, w - 2.0 * SIDE - 40.0, 2).iter().enumerate() {
        ui.text(w / 2.0, 96.0 + 18.0 * i as f32, line, 12.5, MUTED, Face::Regular, Anchor::MA, None);
    }
    let (x, cw) = (SIDE, w - 2.0 * SIDE);
    match st.step {
        0 => step_widgets(st, ui, x, cw, v),
        1 => step_personal(st, ui, x, cw),
        _ => step_tips(st, ui, x, cw),
    }
    // rodapé
    ui.fill(0.0, FOOT_LINE, w, FOOT_LINE + 1.0, 0.0, WHITE.with_a(14));
    let last = st.last();
    let (label, action) = if last { ("Começar a usar", A::Finish) } else { ("Continuar", A::Next) };
    let nw = ui.button("proximo", w - SIDE, BUTTONS_Y, label, action, "primary", 'r', Some(150.0), None);
    if st.step > 0 {
        ui.button("voltar", w - SIDE - nw - 10.0, BUTTONS_Y, "Voltar", A::Back, "secondary", 'r', Some(100.0), None);
    }
    if !last {
        ui.button("pular", SIDE - 12.0, BUTTONS_Y, "Pular", A::Cancel, "ghost", 'l', None, None);
    }
    if st.step == 0 {
        let n = st.chosen.values().filter(|v| **v).count();
        let text = match n {
            0 => "Nenhum widget escolhido".to_string(),
            1 => "1 widget escolhido".to_string(),
            n => format!("{n} widgets escolhidos"),
        };
        let color = if n == 0 { Rgba::rgb(255, 184, 0) } else { MUTED };
        ui.text(w / 2.0, BUTTONS_Y + 16.0, &text, 11.5, color, Face::Regular, Anchor::MM, None);
    }
}

/// Passo 1: os widgets em cartões (3 em cima e o resto embaixo, no meio), com a miniatura de
/// verdade no tamanho médio.
/// Os cartões das boas-vindas: quantos por linha e quantas linhas de descrição cabem. Até seis
/// widgets, três por linha (duas linhas de descrição); com mais, quatro por linha, mais estreitos,
/// com uma linha a mais de descrição (a altura que sobra das duas linhas de cartões).
pub(crate) fn welcome_grid() -> (usize, usize) {
    if widgets::KEYS.len() > 6 { (4, 3) } else { (3, 2) }
}

fn step_widgets(st: &mut WelcomeState, ui: &mut Ui, x: f32, w: f32, v: &View) {
    let gap = 14.0;
    let (cols, lines) = welcome_grid();
    let count = widgets::KEYS.len();
    let rows = count.div_ceil(cols);
    let tile_w = (w - gap * (cols - 1) as f32) / cols as f32;
    let tile_h = (CONTENT_H - gap * (rows - 1) as f32) / rows as f32;
    // embaixo da miniatura: o nome e as linhas de descrição
    let stage_h = tile_h - 44.0 - 15.0 * lines as f32;
    let s = ui.s();
    let medium = widgets::Layout::of(Mode::M);
    let zoom = ((tile_w - 20.0) / medium.w).min((stage_h - 8.0) / medium.h);
    for (i, key) in widgets::KEYS.iter().copied().enumerate() {
        let (row, col) = (i / cols, i % cols);
        let in_row = (count - row * cols).min(cols);
        let row_x = x + (w - (in_row as f32 * tile_w + (in_row - 1) as f32 * gap)) / 2.0;
        let tx = row_x + col as f32 * (tile_w + gap);
        let ty = CONTENT_Y + row as f32 * (tile_h + gap);
        let on = st.chosen.get(key).copied().unwrap_or(false);
        let id = format!("tile:{key}");
        let hot = ui.hot(&id);
        let outline = if on { ACCENT } else { WHITE.with_a(if hot { 40 } else { 22 }) };
        ui.rect(tx, ty, tx + tile_w, ty + tile_h, 12.0, Some(WHITE.with_a(if hot { 18 } else { 11 })), Some(outline), if on { 2.0 } else { 1.0 });
        if !st.thumbs.contains_key(key) {
            let thumb = (v.render)(key, Mode::M, s * zoom).map(|(content, radius)| {
                let sx = v.origin.0 + ((ui.ox + tx + (tile_w - medium.w * zoom) / 2.0) * s).round() as i32;
                let sy = v.origin.1 + ((ui.oy + ty + 10.0 + (stage_h - medium.h * zoom) / 2.0) * s).round() as i32;
                v.sh.backdrop.glass(&content, radius, sx, sy, v.sh.tint)
            });
            st.thumbs.insert(key, thumb);
        }
        if let Some(Some(t)) = st.thumbs.get(key).cloned() {
            ui.image(&t, tx + (tile_w - t.w as f32 / s) / 2.0, ty + 10.0 + (stage_h - t.h as f32 / s) / 2.0);
        }
        let title_y = ty + stage_h + 24.0;
        ui.text(tx + 14.0, title_y, widgets::title(key), 13.5, FG, Face::Semibold, Anchor::LM, Some(tile_w - 52.0));
        for (j, line) in ui.wrap(widgets::description(key), 11.0, tile_w - 28.0, lines).iter().enumerate() {
            ui.text(tx + 14.0, title_y + 20.0 + 15.0 * j as f32, line, 11.0, MUTED, Face::Regular, Anchor::LM, Some(tile_w - 28.0));
        }
        let (cx, cy) = (tx + tile_w - 22.0, title_y);
        if on {
            ui.circle(cx, cy, 10.0, ACCENT);
            ui.icon(cx, cy, "check", 11.0, FG);
        } else {
            ui.ring(cx, cy, 9.5, WHITE.with_a(120), 1.3);
        }
        ui.region(&id, tx, ty, tx + tile_w, ty + tile_h, A::Tile(key), Cursor::Hand);
    }
}

const ROW_H: f32 = 66.0;
const ROW_GAP: f32 = 12.0;
/// onde começa o campo, a partir da esquerda da linha
const FIELD_X: f32 = 320.0;

/// Uma linha em cartão: ícone, título e explicação à esquerda (o controle vai à direita).
fn setting_row(ui: &mut Ui, x: f32, y: f32, w: f32, icon: &str, title: &str, sub: &str) {
    let cy = y + ROW_H / 2.0;
    ui.card(x, y, x + w, y + ROW_H);
    ui.circle(x + 30.0, cy, 16.0, ACCENT.with_a(46));
    ui.icon(x + 30.0, cy, icon, 14.0, ACCENT_HOVER);
    ui.text(x + 58.0, cy - 9.0, title, 13.0, FG, Face::Semibold, Anchor::LM, None);
    ui.text(x + 58.0, cy + 10.0, sub, 11.0, MUTED, Face::Regular, Anchor::LM, Some(FIELD_X - 72.0));
}

/// Passo 2: cidade do clima, usuário do GitHub e iniciar com o Windows.
fn step_personal(st: &WelcomeState, ui: &mut Ui, x: f32, w: f32) {
    let (fx, fw) = (x + FIELD_X, w - FIELD_X - 16.0);
    let field_y = |y: f32| y + (ROW_H - 36.0) / 2.0;
    let mut y = CONTENT_Y;
    setting_row(ui, x, y, w, "location", "Cidade do clima", "Para a previsão do tempo");
    let city_y = field_y(y);
    ui.entry("cidade", fx, city_y, fx + fw, city_y + 36.0, &st.city_text, "Ex.: São Paulo", false);
    y += ROW_H + ROW_GAP;
    setting_row(ui, x, y, w, "github", "Usuário do GitHub", "Opcional (o que vem depois de github.com/)");
    ui.entry("usuario", fx, field_y(y), fx + fw, field_y(y) + 36.0, &st.user, "seu-usuario", false);
    y += ROW_H + ROW_GAP;
    setting_row(ui, x, y, w, "pin", "Iniciar com o Windows", "Os widgets abrem sozinhos quando você entra");
    ui.switch("iniciar", x + w - 16.0 - 40.0, y + (ROW_H - 22.0) / 2.0, st.startup, A::Toggle("welcome_startup"), Some((x, y, x + w, y + ROW_H)));
    y += ROW_H + ROW_GAP;
    if !st.chosen.get("tempo").copied().unwrap_or(false) {
        ui.text(x + 4.0, y + 4.0, "O Tempo não foi escolhido; a cidade fica guardada para quando você abrir.", 11.0, FAINT, Face::Regular, Anchor::LA, Some(w));
    }
    // por último, para a lista de sugestões ficar por cima das linhas de baixo
    st.city.draw(ui, fx, city_y + 40.0, fw);
}

/// Passo 3: as dicas em cartões (2 × 2) e quais widgets vão aparecer.
fn step_tips(st: &WelcomeState, ui: &mut Ui, x: f32, w: f32) {
    let tips = [(TIPS[0], "mover"), (TIPS[2], "tamanho"), (TIPS[3], "mouse"), (TIPS[4], "menu")];
    let gap = 14.0;
    let (tw, th) = ((w - gap) / 2.0, 112.0);
    for (i, ((title, desc), icon)) in tips.iter().enumerate() {
        let tx = x + (i % 2) as f32 * (tw + gap);
        let ty = CONTENT_Y + (i / 2) as f32 * (th + gap);
        ui.card(tx, ty, tx + tw, ty + th);
        ui.circle(tx + 32.0, ty + 32.0, 16.0, ACCENT.with_a(46));
        ui.icon(tx + 32.0, ty + 32.0, icon, 15.0, ACCENT_HOVER);
        ui.text(tx + 60.0, ty + 32.0, title, 13.5, FG, Face::Semibold, Anchor::LM, Some(tw - 76.0));
        for (j, line) in ui.wrap(desc, 11.5, tw - 36.0, 3).iter().enumerate() {
            ui.text(tx + 18.0, ty + 58.0 + 16.0 * j as f32, line, 11.5, MUTED, Face::Regular, Anchor::LA, None);
        }
    }
    let chosen: Vec<&str> = widgets::KEYS.iter().copied().filter(|k| st.chosen.get(k).copied().unwrap_or(false)).map(widgets::title).collect();
    if !chosen.is_empty() {
        let y = CONTENT_Y + 2.0 * th + gap + 26.0;
        let text = format!("Vão aparecer na área de trabalho: {}.", join_names(&chosen));
        for (j, line) in ui.wrap(&text, 12.0, w, 2).iter().enumerate() {
            ui.text(x + w / 2.0, y + 17.0 * j as f32, line, 12.0, FG.alpha(0.85), Face::Regular, Anchor::MA, None);
        }
    }
}

/// "Tempo, Sistema e GitHub".
fn join_names(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} e {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_fit_in_two_lines() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        // a largura do texto num cartão das boas-vindas (como em step_widgets)
        let (cols, n) = welcome_grid();
        let tile_w = (WELCOME_W - 2.0 * SIDE - (cols - 1) as f32 * 14.0) / cols as f32;
        let max_w = tile_w - 28.0;
        let c = crate::gfx::Canvas::new(10, 10, 1.0);
        for key in widgets::KEYS {
            let lines = c.wrap(widgets::description(key), Face::Regular, 11.0, max_w, n);
            let last = lines.last().unwrap();
            assert!(c.text_width(last, Face::Regular, 11.0) <= max_w && !last.ends_with('…'), "{key}: a descrição passa de {n} linhas e vira \"…\": {last:?}");
        }
    }

    #[test]
    fn welcome_steps_fit_between_header_and_footer() {
        assert!(CONTENT_Y + CONTENT_H <= FOOT_LINE - 12.0);
        assert!(BUTTONS_Y + 32.0 <= WELCOME_H - 12.0, "os botões cabem embaixo da linha do rodapé");
        // passo 1: todos os widgets em cartões com espaço para a miniatura, o nome e a descrição
        let (cols, lines) = welcome_grid();
        let rows = widgets::KEYS.len().div_ceil(cols) as f32;
        let tile_h = (CONTENT_H - 14.0 * (rows - 1.0)) / rows;
        let stage = tile_h - 44.0 - 15.0 * lines as f32;
        assert!(stage >= 80.0, "miniatura de {stage} px");
        // passo 2: as três linhas e o aviso
        assert!(CONTENT_Y + 3.0 * (ROW_H + ROW_GAP) + 20.0 <= FOOT_LINE);
        // passo 3: as dicas (2 × 2) e a lista dos widgets
        assert!(CONTENT_Y + 2.0 * 112.0 + 14.0 + 26.0 + 2.0 * 17.0 <= FOOT_LINE);
    }

    #[test]
    fn names_in_a_sentence() {
        assert_eq!(join_names(&["Tempo"]), "Tempo");
        assert_eq!(join_names(&["Tempo", "Sistema"]), "Tempo e Sistema");
        assert_eq!(join_names(&["Tempo", "Sistema", "GitHub"]), "Tempo, Sistema e GitHub");
    }
}
