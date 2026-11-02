//! Configurações: barra lateral com páginas, controles próprios, prévia ao vivo e aplicação
//! imediata (sem "Salvar" nem "Cancelar").

use std::collections::HashMap;

use crate::config::{self, Get};
use crate::events::Cursor;
use crate::gfx::{Anchor, Face, Image};
use crate::panel::{Suggest, Ui, A};
use crate::shared::Shared;
use crate::theme::*;
use crate::widgets::{self, pomodoro, Mode};

pub const W: f32 = 900.0;
pub const H: f32 = 640.0;
pub const SIDEBAR: f32 = 240.0;
pub const TOP: f32 = 78.0;
pub use crate::updates::VERSION;

pub const NOTIFY: [(&str, &str, &str); 6] = [
    ("cpu", "CPU alta", "Quando a CPU passa de 90% por 3 minutos"),
    ("ram", "Memória quase cheia", "Quando a memória passa de 90% por 3 minutos"),
    ("disco", "Disco quase cheio", "Quando o disco C: passa de 95% de uso"),
    ("chuva", "Chuva chegando", "Quando a previsão indica chuva na próxima hora"),
    ("github", "Revisão pedida no GitHub", "Quando alguém pede sua revisão num pull request"),
    ("pomodoro", "Pomodoro", "No fim de cada foco e de cada pausa"),
];

pub const TIPS: [(&str, &str); 5] = [
    ("Mover", "Arraste de qualquer lugar. Os widgets se encaixam uns nos outros e nas bordas da tela; segure Alt para soltar livre."),
    ("Reordenar", "Arraste um widget para dentro de uma coluna: os outros abrem espaço."),
    ("Tamanho", "Botão no canto (aparece com o mouse) ou dois cliques: pequeno, médio e grande."),
    ("Roda do mouse", "Volume no Tocando agora; rola as atividades do GitHub; ±1 minuto no Pomodoro."),
    ("Menu", "Clique direito num widget ou no ícone da bandeja: layouts, modo foco, edição e mais."),
];

pub const APPEARANCE_KEYS: [&str; 6] = ["desfoque", "escurecimento", "texto", "esmaecer", "opacidade_ociosa", "animacoes"];
pub const BEHAVIOR_KEYS: [&str; 3] = ["atalhos_clique", "dicas", "economia_bateria"];

/// Faixas das durações do Pomodoro: (fase, rótulo, mínimo, máximo, passo).
pub const PHASE_RANGES: [(&str, &str, f64, f64, f64); 3] =
    [("foco", "Foco", 5.0, 90.0, 5.0), ("pausa", "Pausa curta", 1.0, 30.0, 1.0), ("longa", "Pausa longa", 5.0, 60.0, 5.0)];

#[derive(Default)]
pub struct SettingsState {
    pub page: &'static str,
    pub scrolls: HashMap<&'static str, f32>,
    pub content_h: f32,
    pub thumbs: HashMap<&'static str, (bool, Option<Image>)>,
    preview: Option<((i32, i32, u32, u32, i64, i64, u64), Image)>,
    pub city: Suggest,
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
            if page == "widgets" {
                self.thumbs.clear();
            }
        }
    }

    pub fn scroll(&mut self, steps: f32) -> bool {
        let view = H - TOP;
        let top = (self.content_h - view).max(0.0);
        let cur = self.scrolls.get(self.page).copied().unwrap_or(0.0);
        let new = (cur - steps * 64.0).clamp(0.0, top);
        if new != cur {
            self.scrolls.insert(self.page, new);
            self.city.clear();
            return true;
        }
        false
    }
}

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
}

fn nav_items() -> Vec<(&'static str, String, &'static str)> {
    let mut items = vec![
        ("geral", "Geral".to_string(), "geral"),
        ("aparencia", "Aparência".into(), "aparencia"),
        ("widgets", "Galeria de widgets".into(), "widgets"),
        ("-", "SEUS WIDGETS".into(), ""),
    ];
    for key in widgets::KEYS {
        items.push((key, widgets::title(key).into(), widgets::icon_name(key)));
    }
    items.extend([
        ("-", String::new(), ""),
        ("avisos", "Notificações".into(), "avisos"),
        ("dicas", "Dicas".into(), "atalhos"),
        ("sobre", "Sobre".into(), "sobre"),
    ]);
    items
}

fn page_title(page: &str) -> (String, Option<String>) {
    if widgets::KEYS.contains(&page) {
        return (widgets::title(page).into(), Some(widgets::description(page).into()));
    }
    let (t, s) = match page {
        "geral" => ("Geral", Some("Como os widgets se comportam no dia a dia.")),
        "aparencia" => ("Aparência", Some("Vidro, texto e animações. Tudo muda na hora.")),
        "widgets" => ("Galeria de widgets", Some("Escolha quais widgets ficam na sua área de trabalho.")),
        "avisos" => ("Notificações", Some("Quando os widgets podem chamar sua atenção.")),
        "dicas" => ("Dicas", Some("Gestos do mouse e o que dá para fazer com o clique direito.")),
        _ => ("Sobre", None),
    };
    (t.into(), s.map(String::from))
}

// ---------- linhas e controles ----------

pub enum Control {
    None,
    Switch(&'static str, bool, A),
    Slider(&'static str, f64, f64, f64, f64, String),
    Seg(&'static str, Vec<(String, String)>, String),
    Button(&'static str, String, A, &'static str, Option<&'static str>),
    Buttons(Vec<(String, String, A)>),
    Entry(&'static str, String, String, f32),
}

impl Control {
    fn width(&self, w: f32) -> f32 {
        match self {
            Control::None => 0.0,
            Control::Switch(..) => 70.0,
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
                let width = 210.0;
                ui.text(rx, cy, label, 12.0, MUTED, Face::Regular, Anchor::RM, None);
                ui.slider(id, rx - 64.0 - width, cy, width, *value, *lo, *hi, *step);
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

fn section(ui: &mut Ui, x: f32, y: f32, title: &str) -> f32 {
    ui.text(x + 2.0, y, title, 13.0, FG, Face::Semibold, Anchor::LA, None);
    y + 28.0
}

/// Cartão com linhas (título, descrição, controle). Devolve o y logo abaixo.
fn rows(ui: &mut Ui, x: f32, y: f32, w: f32, rows: Vec<Row>) -> f32 {
    let mut heights = Vec::new();
    let mut lines = Vec::new();
    for Row(_, desc, control) in &rows {
        let text_w = w - 36.0 - control.width(w);
        let full = desc.as_ref().map(|d| ui.wrap(d, 11.5, text_w, 99)).unwrap_or_default();
        let ls = if full.len() <= 2 { full } else { vec![full[0].clone(), full[1..].join(" ")] };
        heights.push(if ls.is_empty() { 54.0 } else { 66.0 + 17.0 * (ls.len() as f32 - 1.0) });
        lines.push((text_w, ls));
    }
    ui.card(x, y, x + w, y + heights.iter().sum::<f32>());
    let mut top = y;
    for (i, (Row(title, _, control), (h, (text_w, ls)))) in rows.iter().zip(heights.iter().zip(lines)).enumerate() {
        let row = (x, top, x + w, top + h);
        if i > 0 {
            ui.divider(x + 16.0, x + w - 16.0, top);
        }
        control.draw(ui, x + w - 18.0, top + h / 2.0, row);
        if ls.is_empty() {
            ui.text(x + 18.0, top + h / 2.0, title, 13.0, FG, Face::Regular, Anchor::LM, Some(text_w));
        } else {
            ui.text(x + 18.0, top + 15.0, title, 13.0, FG, Face::Regular, Anchor::LA, Some(text_w));
            for (j, line) in ls.iter().enumerate() {
                ui.text(x + 18.0, top + 36.0 + 17.0 * j as f32, line, 11.5, MUTED, Face::Regular, Anchor::LA, Some(text_w));
            }
        }
        top += h;
    }
    top + 26.0
}

fn restore(ui: &mut Ui, x: f32, y: f32, w: f32, page: &'static str) -> f32 {
    ui.button(&format!("restaurar:{page}"), x + w, y - 10.0, "Restaurar padrão", A::Restore(page), "ghost", 'r', None, Some("undo"));
    y + 30.0
}

fn sw(id: &'static str, on: bool, action: A) -> Control {
    Control::Switch(id, on, action)
}

fn setting_switch(v: &View, name: &'static str) -> Control {
    sw(name, config::setting_bool(&v.sh.cfg, name), A::Toggle(name))
}

fn btn(id: &'static str, label: &str, action: A) -> Control {
    Control::Button(id, label.into(), action, "secondary", None)
}

// ---------- desenho ----------

/// Desenha a janela inteira: barra lateral, título e a página (rolável).
pub fn draw(st: &mut SettingsState, ui: &mut Ui, v: &View) {
    // barra lateral
    ui.fill(0.0, 0.0, SIDEBAR, H, 0.0, WHITE.with_a(7));
    ui.fill(SIDEBAR, 0.0, SIDEBAR + 1.0, H, 0.0, WHITE.with_a(16));
    let icon = crate::tray::icon_image((30.0 * ui.s()).round() as u32);
    ui.image(&icon, 22.0, 22.0);
    ui.text(62.0, 22.0, "Widgets", 15.0, FG, Face::Semibold, Anchor::LA, None);
    ui.text(62.0, 41.0, "Configurações", 11.0, MUTED, Face::Regular, Anchor::LA, None);
    let mut y = 82.0;
    let mut selected = None;
    for (page, label, icon_name) in nav_items() {
        if page == "-" {
            if label.is_empty() {
                ui.fill(20.0, y + 8.0, SIDEBAR - 20.0, y + 9.0, 0.0, WHITE.with_a(16));
                y += 17.0;
            } else {
                ui.text(26.0, y + 12.0, &label, 9.5, FAINT, Face::Semibold, Anchor::LA, None);
                y += 30.0;
            }
            continue;
        }
        let id = format!("nav:{page}");
        let active = page == st.page;
        if active {
            selected = Some(y);
            ui.fill(12.0, y, SIDEBAR - 12.0, y + 36.0, 7.0, WHITE.with_a(22));
        } else if ui.hot(&id) {
            ui.fill(12.0, y, SIDEBAR - 12.0, y + 36.0, 7.0, WHITE.with_a(11));
        }
        let hidden = widgets::KEYS.contains(&page) && !v.visible.contains(&page);
        let color = if hidden { FAINT } else { FG };
        ui.icon(36.0, y + 18.0, icon_name, 15.0, color);
        ui.text(58.0, y + 18.0, &label, 12.5, color, if active { Face::Semibold } else { Face::Regular }, Anchor::LM, Some(SIDEBAR - 80.0));
        if hidden {
            ui.text(SIDEBAR - 24.0, y + 18.0, "oculto", 10.0, FAINT, Face::Regular, Anchor::RM, None);
        }
        ui.region(&id, 12.0, y, SIDEBAR - 12.0, y + 36.0, A::Go(page), Cursor::Hand);
        y += 38.0;
    }
    if let Some(sy) = selected {
        // indicador azul que desliza até a página escolhida
        let iy = ui.anim("nav-indicator", sy, 220);
        ui.fill(12.0, iy + 10.0, 15.0, iy + 26.0, 1.5, ACCENT);
    }
    let (title, subtitle) = page_title(st.page);
    ui.text(SIDEBAR + 32.0, 24.0, &title, 22.0, FG, Face::Semibold, Anchor::LA, None);
    if let Some(sub) = subtitle {
        ui.text(SIDEBAR + 32.0, 54.0, &sub, 11.5, MUTED, Face::Regular, Anchor::LA, Some(W - SIDEBAR - 64.0));
    }

    // conteúdo rolável
    let view_h = H - TOP;
    let scroll = st.scrolls.get(st.page).copied().unwrap_or(0.0);
    let left = SIDEBAR + 1.0;
    ui.c.push_rect_clip(left, TOP, W, H);
    let (old_ox, old_oy, old_clip) = (ui.ox, ui.oy, ui.clip);
    ui.ox = left;
    ui.oy = TOP - scroll;
    ui.clip = (left, TOP, W, H);
    let (x, w) = (32.0, W - SIDEBAR - 64.0);
    let height = match st.page {
        "geral" => page_geral(ui, x, w, v),
        "aparencia" => page_aparencia(st, ui, x, w, v),
        "widgets" => page_widgets(st, ui, x, w, v),
        "avisos" => page_avisos(ui, x, w, v),
        "dicas" => page_dicas(ui, x, w),
        "sobre" => page_sobre(ui, x, w, v),
        key => page_widget(st, ui, x, w, v, key),
    };
    ui.ox = old_ox;
    ui.oy = old_oy;
    ui.clip = old_clip;
    ui.c.pop_rect_clip();
    st.content_h = height + 32.0;
    let top = (st.content_h - view_h).max(0.0);
    if scroll > top {
        st.scrolls.insert(st.page, top);
    }
    if st.content_h > view_h {
        // barra de rolagem fininha
        let track = view_h - 16.0;
        let thumb = (track * view_h / st.content_h).max(30.0);
        let ty = TOP + 8.0 + (track - thumb) * scroll.min(top) / top.max(1.0);
        ui.fill(W - 8.0, ty, W - 5.0, ty + thumb, 1.5, WHITE.with_a(60));
    }
    if scroll > 0.0 {
        ui.fill(left, TOP, W, TOP + 1.0, 0.0, WHITE.with_a(20));
    }
}

fn page_geral(ui: &mut Ui, x: f32, w: f32, v: &View) -> f32 {
    let y = section(ui, x, 4.0, "Comportamento");
    let y = rows(ui, x, y, w, vec![
        Row("Iniciar com o Windows".into(), Some("Abre os widgets quando você entra no Windows".into()), sw("iniciar", v.startup, A::Startup)),
        Row("Travar posições".into(), Some("Evita arrastar sem querer (o modo edição continua liberado)".into()), sw("travado", v.sh.locked(), A::Lock)),
        Row("Cliques abrem programas e sites".into(), Some("Gerenciador de Tarefas, previsão do tempo, GitHub…".into()), setting_switch(v, "atalhos_clique")),
        Row("Dicas ao passar o mouse".into(), Some("Explicações curtas sobre o que está sob o mouse".into()), setting_switch(v, "dicas")),
        Row("Economizar energia na bateria".into(), Some("Atualiza menos e pausa as animações contínuas".into()), setting_switch(v, "economia_bateria")),
    ]);
    let y = section(ui, x, y, "Organizar");
    let y = rows(ui, x, y, w, vec![
        Row("Modo edição".into(), Some("Feche, redimensione e mova todos os widgets de uma vez".into()), Control::Button("editar", "Editar widgets".into(), A::Edit, "primary", Some("edit"))),
        Row("Organizar automaticamente".into(), Some("Empilha os widgets em colunas na direita da tela".into()), btn("organizar", "Organizar", A::Arrange)),
    ]);
    let y = section(ui, x, y, "Layouts");
    let mut layout_rows = Vec::new();
    if let Some(layouts) = v.sh.cfg["app"]["layouts"].as_object() {
        for (name, snap) in layouts {
            let count = snap.as_object().map(|m| m.values().filter(|s| s.bool_of("visible").unwrap_or(false)).count()).unwrap_or(0);
            let buttons = vec![
                (format!("aplicar:{name}"), "Aplicar".to_string(), A::ApplyLayout(name.clone())),
                (format!("excluir:{name}"), "Excluir".to_string(), A::DeleteLayout(name.clone())),
            ];
            layout_rows.push(Row(name.clone(), Some(format!("{count} widget{}", if count != 1 { "s" } else { "" })), Control::Buttons(buttons)));
        }
    }
    layout_rows.push(Row(
        "Salvar layout atual".into(),
        Some("Guarda a posição e o tamanho de cada widget".into()),
        Control::Button("salvar_layout", "Salvar…".into(), A::SaveLayout, "secondary", Some("add")),
    ));
    let y = rows(ui, x, y, w, layout_rows);
    restore(ui, x, y, w, "geral")
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
    ]);
    let following = config::setting_f64(cfg, "texto") <= 0.0;
    let esmaecer = config::setting_bool(cfg, "esmaecer");
    let mut r = vec![Row("Seguir o tamanho de texto do Windows".into(), Some("Configurações → Acessibilidade → Tamanho do texto".into()), sw("texto_auto", following, A::Toggle("texto_auto")))];
    if !following {
        let pct = (v.sh.text_scale as f64 * 100.0).round();
        r.push(Row("Tamanho do texto".into(), None, Control::Slider("texto", pct, 90.0, 120.0, 1.0, format!("{pct}%"))));
    }
    r.push(Row("Esmaecer sem o mouse".into(), Some("Os widgets ficam mais transparentes até o mouse passar por cima".into()), setting_switch(v, "esmaecer")));
    if esmaecer {
        let pct = (config::setting_f64(cfg, "opacidade_ociosa") * 100.0).round();
        r.push(Row("Opacidade sem o mouse".into(), None, Control::Slider("opacidade_ociosa", pct, 40.0, 95.0, 1.0, format!("{pct}%"))));
    }
    let y = section(ui, x, y, "Texto e transparência");
    let y = rows(ui, x, y, w, r);
    let y = section(ui, x, y, "Movimento");
    let options = [("auto", "Seguir o Windows"), ("sempre", "Sempre"), ("nunca", "Nunca")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    let y = rows(ui, x, y, w, vec![Row(
        "Animações".into(),
        Some("Ao mudar de tamanho, reorganizar e abrir menus".into()),
        Control::Seg("animacoes", options, config::setting_str(cfg, "animacoes")),
    )]);
    restore(ui, x, y, w, "aparencia")
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
    let key = (sx, sy, pw, ph, (blur * 10.0) as i64, (tint * 1000.0) as i64, v.sh.backdrop.version);
    if st.preview.as_ref().map(|p| p.0) != Some(key) {
        let order = ["tempo", "sistema", "pomodoro", "midia", "github"];
        let mut shown: Vec<&'static str> = order.into_iter().filter(|k| v.visible.contains(k)).take(2).collect();
        if shown.is_empty() {
            shown.push("tempo");
        }
        let cards: Vec<(Image, f32)> = shown.iter().zip([Mode::M, Mode::P]).filter_map(|(k, m)| (v.render)(k, m, s)).collect();
        if !cards.is_empty() {
            let mut stage = v.sh.backdrop.region(sx, sy, pw, ph, true).to_image();
            let gap = (16.0 * s).round() as i32;
            let total: i32 = cards.iter().map(|c| c.0.w as i32).sum::<i32>() + gap * (cards.len() as i32 - 1);
            let mut cx = (pw as i32 - total) / 2;
            for (content, radius) in &cards {
                let cy = (ph as i32 - content.h as i32) / 2;
                let glass = v.sh.backdrop.glass_with(content, *radius, sx + cx, sy + cy, blur, tint);
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
    ui.text(x + 14.0, y + h - 10.0, "Prévia ao vivo", 10.5, WHITE.with_a(200), Face::Semibold, Anchor::LS, None);
}

fn page_widgets(st: &mut SettingsState, ui: &mut Ui, x: f32, w: f32, v: &View) -> f32 {
    let bw = ui.button("editar", x, 4.0, "Editar widgets", A::Edit, "primary", 'l', None, Some("edit"));
    ui.button("organizar", x + bw + 8.0, 4.0, "Organizar", A::Arrange, "secondary", 'l', None, Some("refresh"));
    ui.text(x + w, 20.0, "Clique direito num widget para mais opções", 11.0, MUTED, Face::Regular, Anchor::RM, None);
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
        let cached = st.thumbs.get(key).filter(|t| t.0 == visible).map(|t| t.1.clone());
        let thumb = match cached {
            Some(t) => t,
            None => {
                let t = thumbnail(key, ui, tx + 8.0, ty + 8.0, tile_w - 16.0, stage_h, v);
                st.thumbs.insert(key, (visible, t.clone()));
                t
            }
        };
        if let Some(t) = thumb {
            ui.image(&t, tx + 8.0 + (tile_w - 16.0 - t.w as f32 / s) / 2.0, ty + 8.0 + (stage_h - t.h as f32 / s) / 2.0);
        }
        let ny = ty + stage_h + 22.0;
        ui.text(tx + 16.0, ny, widgets::title(key), 14.0, FG, Face::Semibold, Anchor::LM, Some(tile_w - 90.0));
        ui.switch(&format!("ver:{key}"), tx + tile_w - 56.0, ny - 11.0, visible, A::Visible(key, !visible), None);
        for (j, line) in ui.wrap(widgets::description(key), 11.5, tile_w - 32.0, 2).iter().enumerate() {
            ui.text(tx + 16.0, ny + 18.0 + j as f32 * 17.0, line, 11.5, MUTED, Face::Regular, Anchor::LA, None);
        }
        ui.button(&format!("opcoes:{key}"), tx + 10.0, ty + tile_h - 40.0, "Opções", A::Go(key), "ghost", 'l', None, None);
    }
    // todas as linhas de cartões (com 5 widgets são 3): senão a rolagem para antes da última
    let rows = widgets::KEYS.len().div_ceil(2) as f32;
    y + rows * (tile_h + gap)
}

/// O widget em vidro, cabendo no palco da galeria.
fn thumbnail(key: &'static str, ui: &Ui, x: f32, y: f32, w: f32, h: f32, v: &View) -> Option<Image> {
    let s = ui.s();
    let (mut content, mut radius) = (v.render)(key, Mode::M, s * 0.72)?;
    let max_w = (w - 24.0) * s;
    let max_h = (h - 20.0) * s;
    let f = (max_w / content.w as f32).min(max_h / content.h as f32).min(1.0);
    if f < 1.0 {
        content = crate::gfx::resize(&content, (content.w as f32 * f).round() as u32, (content.h as f32 * f).round() as u32);
        radius *= f;
    }
    let sx = v.origin.0 + ((ui.ox + x) * s) as i32;
    let sy = v.origin.1 + ((ui.oy + y) * s) as i32;
    Some(v.sh.backdrop.glass(&content, radius, sx, sy, v.sh.tint))
}

fn page_widget(st: &mut SettingsState, ui: &mut Ui, x: f32, w: f32, v: &View, key: &'static str) -> f32 {
    let wcfg = &v.sh.cfg[key];
    let mut y = 4.0;
    let visible = v.visible.contains(&key);
    if !visible {
        ui.rect(x, y, x + w, y + 56.0, 10.0, Some(Rgba(255, 184, 0, 22)), Some(Rgba(255, 184, 0, 90)), 1.0);
        ui.text(x + 18.0, y + 28.0, "Este widget está oculto.", 12.5, FG, Face::Semibold, Anchor::LM, None);
        ui.button("mostrar", x + w - 14.0, y + 12.0, "Mostrar na tela", A::Visible(key, true), "primary", 'r', None, None);
        y += 56.0 + 22.0;
    }
    let current = v.modes.get(key).copied().or_else(|| wcfg.str_of("tamanho").and_then(Mode::from_code)).unwrap_or(widgets::DEFAULT_MODE);
    let sizes: Vec<(String, String)> = Mode::ALL.iter().map(|m| (m.code().to_string(), m.name().to_string())).collect();
    let y = section(ui, x, y, "Visual");
    let y = rows(ui, x, y, w, vec![Row(
        "Tamanho".into(),
        Some("Também dá para mudar com o botão no canto ou com dois cliques".into()),
        Control::Seg(size_id(key), sizes, current.code().into()),
    )]);
    let notify = |kind: &'static str| sw(notify_id(kind), config::notify_enabled(&v.sh.cfg, kind), A::Notify(kind));
    match key {
        "sistema" => {
            let y = section(ui, x, y, "Ao clicar nos anéis");
            let y = rows(ui, x, y, w, vec![Row(
                "Apps que mais usam".into(),
                Some("CPU e memória mostram os 5 apps que mais usam; o disco mostra as unidades".into()),
                Control::None,
            )]);
            let y = section(ui, x, y, "Avisos");
            rows(ui, x, y, w, vec![
                Row(NOTIFY[0].1.into(), Some(NOTIFY[0].2.into()), notify("cpu")),
                Row(NOTIFY[1].1.into(), Some(NOTIFY[1].2.into()), notify("ram")),
                Row(NOTIFY[2].1.into(), Some(NOTIFY[2].2.into()), notify("disco")),
            ])
        }
        "tempo" => {
            let place = wcfg.str_of("place").or(wcfg.str_of("city")).unwrap_or("");
            let y = section(ui, x, y, "Clima");
            let entry_top = y;
            let y = rows(ui, x, y, w, vec![
                Row("Cidade".into(), Some(if place.is_empty() { "Nenhuma cidade escolhida".into() } else { format!("Agora: {place}") }),
                    Control::Entry("cidade", place.into(), "Digite e escolha na lista".into(), 300.0)),
                Row("Atualizar agora".into(), Some("O clima atualiza sozinho a cada 15 minutos".into()),
                    if visible { Control::Button("atualizar_clima", "Atualizar".into(), A::Widget("tempo", widgets::tempo::UPDATE_WEATHER), "secondary", Some("refresh")) } else { Control::None }),
                Row("Avisar quando for chover".into(), Some(NOTIFY[3].2.into()), notify("chuva")),
            ]);
            // sugestões por cima do resto da página (a primeira linha tem 66 de altura)
            st.city.draw(ui, x + w - 18.0 - 300.0, entry_top + 66.0 / 2.0 + 21.0, 300.0);
            y
        }
        "midia" => {
            let options = [("auto", "Automático"), ("spotify", "Spotify"), ("youtube", "YouTube")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
            let y = section(ui, x, y, "Reprodução");
            rows(ui, x, y, w, vec![
                Row("Mostrar".into(), Some("Qual player o widget acompanha quando há mais de um tocando".into()),
                    Control::Seg("fonte", options, wcfg.str_of("fonte").unwrap_or("auto").into())),
                Row("Roda do mouse".into(), Some("Sobre o widget, ajusta o volume do app que está tocando (ou o do Windows)".into()), Control::None),
            ])
        }
        "github" => {
            let y = section(ui, x, y, "Conta");
            rows(ui, x, y, w, vec![
                Row("Usuário".into(), Some("Com o GitHub CLI (gh) conectado, entram também os repositórios privados".into()),
                    Control::Entry("usuario", wcfg.str_of("usuario").unwrap_or("").into(), "seu-usuario".into(), 270.0)),
                Row("Atualizar agora".into(), Some("As atividades atualizam sozinhas a cada 10 minutos".into()),
                    if visible { Control::Button("atualizar_github", "Atualizar".into(), A::Widget("github", widgets::github::UPDATE), "secondary", Some("refresh")) } else { Control::None }),
                Row("Avisar pedidos de revisão".into(), Some(NOTIFY[4].2.into()), notify("github")),
            ])
        }
        "claude" => {
            use crate::claude_usage::{self as usage, Link};
            use widgets::claude::{CONNECT, DISCONNECT};
            let (state, control) = match usage::link() {
                Link::Connected => ("Conectado: os números chegam a cada resposta do Claude Code", btn("claude_desconectar", "Desconectar", A::Widget("claude", DISCONNECT))),
                Link::Off => ("Ainda não conectado", Control::Button("claude_conectar", "Conectar".into(), A::Widget("claude", CONNECT), "primary", None)),
                Link::Other => ("O Claude Code já usa outra linha de status; tire-a para conectar", Control::None),
                Link::Missing => ("O Claude Code não foi encontrado neste computador", Control::None),
            };
            let y = section(ui, x, y, "Claude Code");
            rows(ui, x, y, w, vec![
                Row("Conexão".into(), Some(state.into()), control),
                Row("Como funciona".into(), Some("O widget vira a linha de status do Claude Code, que informa o uso da sessão de 5 horas e da semana".into()), Control::None),
            ])
        }
        _ => {
            let durations = &wcfg["duracoes"];
            let mut r = Vec::new();
            for (phase, label, lo, hi, step) in PHASE_RANGES {
                let default = match phase { "foco" => 25, "pausa" => 5, _ => 15 };
                let value = durations.i64_of(phase).unwrap_or(default) as f64;
                let id: &'static str = match phase { "foco" => "dur:foco", "pausa" => "dur:pausa", _ => "dur:longa" };
                r.push(Row(label.into(), None, Control::Slider(id, value, lo, hi, step, pomodoro::fmt_minutes(value))));
            }
            let every = wcfg.i64_of("longa_a_cada").unwrap_or(4);
            r.push(Row("Pausa longa a cada".into(), None, Control::Seg("longa_a_cada", (2..=6).map(|n| (n.to_string(), n.to_string())).collect(), every.to_string())));
            let y = section(ui, x, y, "Durações");
            let y = rows(ui, x, y, w, r);
            let y = section(ui, x, y, "Comportamento");
            let y = rows(ui, x, y, w, vec![
                Row("Começar a próxima fase sozinho".into(), Some("Sem precisar clicar quando o foco ou a pausa acaba".into()),
                    sw("pomodoro_auto", wcfg.bool_of("auto").unwrap_or(false), A::Toggle("pomodoro_auto"))),
                Row("Avisar no fim de cada fase".into(), Some(NOTIFY[5].2.into()), notify("pomodoro")),
            ]);
            let date = crate::win::local_time().iso_date();
            let stats = &wcfg["hoje"];
            let (n, minutes) = if stats.str_of("data") == Some(date.as_str()) { (stats.i64_of("pomodoros").unwrap_or(0), stats.i64_of("minutos").unwrap_or(0)) } else { (0, 0) };
            let y = section(ui, x, y, "Hoje");
            rows(ui, x, y, w, vec![Row(
                format!("{n} pomodoro{}  ·  {} de foco", if n != 1 { "s" } else { "" }, pomodoro::fmt_minutes(minutes as f64)),
                Some("A contagem recomeça sozinha à meia-noite".into()),
                if n > 0 { btn("zerar", "Zerar o dia", A::Widget("pomodoro", pomodoro::RESET_DAY)) } else { Control::None },
            )])
        }
    }
}

pub fn size_id(key: &str) -> &'static str {
    match key {
        "tempo" => "tamanho:tempo",
        "sistema" => "tamanho:sistema",
        "midia" => "tamanho:midia",
        "github" => "tamanho:github",
        "claude" => "tamanho:claude",
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
        _ => "aviso:pomodoro",
    }
}

fn page_avisos(ui: &mut Ui, x: f32, w: f32, v: &View) -> f32 {
    let r = NOTIFY.iter().map(|&(kind, title, desc)| Row(title.into(), Some(desc.into()), sw(notify_id(kind), config::notify_enabled(&v.sh.cfg, kind), A::Notify(kind)))).collect();
    let y = section(ui, x, 4.0, "Avisar quando");
    let y = rows(ui, x, y, w, r);
    let y = rows(ui, x, y, w, vec![Row("Testar".into(), Some("Mostra uma notificação de exemplo agora".into()), btn("testar", "Enviar teste", A::TestNotify))]);
    restore(ui, x, y, w, "avisos")
}

fn page_dicas(ui: &mut Ui, x: f32, w: f32) -> f32 {
    let y = section(ui, x, 4.0, "Mouse");
    rows(ui, x, y, w, TIPS.iter().map(|(t, d)| Row(t.to_string(), Some(d.to_string()), Control::None)).collect())
}

fn page_sobre(ui: &mut Ui, x: f32, w: f32, v: &View) -> f32 {
    let icon = crate::tray::icon_image((56.0 * ui.s()).round() as u32);
    ui.image(&icon, x, 4.0);
    ui.text(x + 74.0, 12.0, "Widgets da área de trabalho", 18.0, FG, Face::Semibold, Anchor::LA, None);
    ui.text(x + 74.0, 40.0, &format!("Versão {VERSION}  ·  feito em Rust, com Direct2D"), 12.0, MUTED, Face::Regular, Anchor::LA, None);
    let update = match &v.update_version {
        Some(ver) => Control::Button("atualizar", format!("Atualizar para {ver}"), A::Widget("app", 1), "primary", Some("refresh")),
        None => Control::Button("procurar", "Procurar agora".into(), A::Widget("app", 2), "secondary", Some("refresh")),
    };
    let y = rows(ui, x, 90.0, w, vec![Row("Atualizações".into(), Some(v.update_status.clone()), update)]);
    let data = crate::config::data_dir().display().to_string();
    rows(ui, x, y, w, vec![
        Row("Boas-vindas".into(), Some("Rever a apresentação e escolher os widgets de novo".into()), btn("boas_vindas", "Abrir", A::Welcome)),
        Row("Pasta das configurações".into(), Some(data), Control::Button("pasta", "Abrir pasta".into(), A::OpenData, "secondary", Some("folder"))),
        Row("Restaurar todas as configurações".into(), Some("Aparência, comportamento e notificações (layouts ficam)".into()),
            Control::Button("restaurar_tudo", "Restaurar".into(), A::ResetAll, "danger", None)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::Anims;

    fn com() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
    }

    /// Desenha uma página das Configurações (como a janela desenha) e devolve a imagem e a altura
    /// do conteúdo. `scroll`: quanto a página está rolada.
    fn page(name: &'static str, scroll: f32, s: f32) -> (Image, f32, f32) {
        let cfg = serde_json::json!({"tempo": {}, "sistema": {}, "midia": {}, "github": {}, "pomodoro": {}, "app": {}});
        let sh = Shared::new(cfg, serde_json::json!({}), s);
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

    /// As páginas de Aparência e da Galeria (no alto e rolada até o fim) num PNG:
    ///   cargo test settings_pages -- --ignored --nocapture
    #[test]
    #[ignore]
    fn settings_pages() {
        com();
        for (name, end) in [("aparencia", false), ("widgets", false), ("widgets", true)] {
            let (_, _, max_scroll) = page(name, 0.0, 1.25);
            let (img, _, _) = page(name, if end { max_scroll } else { 0.0 }, 1.25);
            let path = std::env::temp_dir().join(format!("config_{name}{}.png", if end { "_fim" } else { "" }));
            crate::gfx::save_png(&img, &path).unwrap();
            println!("{}", path.display());
        }
    }
}

