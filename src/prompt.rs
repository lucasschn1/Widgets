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

pub const WELCOME_W: f32 = 760.0;
pub const WELCOME_H: f32 = 580.0;

const STEPS: [(&str, &str); 3] = [
    ("Escolha seus widgets", "Clique para marcar os que você quer na área de trabalho. Dá para mudar depois."),
    ("Deixe com a sua cara", "Algumas informações para os widgets mostrarem o que importa para você."),
    ("Tudo pronto!", "Algumas dicas para aproveitar melhor."),
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
        let system = &cfg["sistema"];
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
    let (w, h) = (WELCOME_W, WELCOME_H);
    // pontos dos passos
    for i in 0..STEPS.len() {
        let cx = w / 2.0 + (i as f32 - 1.0) * 22.0;
        let width = ui.anim(&format!("dot{i}"), if i == st.step { 22.0 } else { 8.0 }, 220);
        ui.fill(cx - width / 2.0, 22.0, cx + width / 2.0, 30.0, 4.0, if i <= st.step { ACCENT } else { WHITE.with_a(50) });
    }
    let (title, subtitle) = STEPS[st.step];
    ui.text(w / 2.0, 62.0, title, 24.0, FG, Face::Semibold, Anchor::MA, None);
    ui.text(w / 2.0, 100.0, subtitle, 12.5, MUTED, Face::Regular, Anchor::MA, Some(w - 120.0));
    let (x, y, cw) = (56.0, 140.0, w - 112.0);
    match st.step {
        0 => step_widgets(st, ui, x, y, cw, v),
        1 => step_personal(st, ui, x, y, cw),
        _ => step_tips(ui, x, y, cw, v),
    }
    // rodapé
    let fy = h - 64.0;
    ui.fill(0.0, fy - 12.0, w, fy - 11.0, 0.0, WHITE.with_a(14));
    let last = st.last();
    let nw = ui.button("proximo", w - 40.0, fy, if last { "Começar a usar" } else { "Continuar" }, if last { A::Finish } else { A::Next }, "primary", 'r', Some(150.0), None);
    if st.step > 0 {
        ui.button("voltar", w - 40.0 - nw - 10.0, fy, "Voltar", A::Back, "secondary", 'r', Some(100.0), None);
    }
    if !last {
        ui.button("pular", 40.0, fy, "Pular", A::Cancel, "ghost", 'l', None, None);
    }
}

fn step_widgets(st: &mut WelcomeState, ui: &mut Ui, x: f32, y: f32, w: f32, v: &View) {
    let gap = 16.0;
    let (tile_w, tile_h, stage_h) = ((w - gap) / 2.0, 172.0, 112.0);
    let s = ui.s();
    for (i, key) in widgets::KEYS.iter().copied().enumerate() {
        let tx = x + (i % 2) as f32 * (tile_w + gap);
        let ty = y + (i / 2) as f32 * (tile_h + gap);
        let on = st.chosen.get(key).copied().unwrap_or(false);
        let id = format!("tile:{key}");
        let hot = ui.hot(&id);
        let outline = if on { ACCENT } else { WHITE.with_a(22) };
        ui.rect(tx, ty, tx + tile_w, ty + tile_h, 12.0, Some(WHITE.with_a(if hot { 18 } else { 11 })), Some(outline), if on { 2.0 } else { 1.0 });
        if !st.thumbs.contains_key(key) {
            let thumb = (v.render)(key, Mode::M, s * 0.6).map(|(mut content, mut radius)| {
                let (mw, mh) = ((tile_w - 36.0) * s, (stage_h - 12.0) * s);
                let f = (mw / content.w as f32).min(mh / content.h as f32).min(1.0);
                if f < 1.0 {
                    content = crate::gfx::resize(&content, (content.w as f32 * f) as u32, (content.h as f32 * f) as u32);
                    radius *= f;
                }
                let sx = v.origin.0 + ((ui.ox + tx + 10.0) * s) as i32;
                let sy = v.origin.1 + ((ui.oy + ty + 10.0) * s) as i32;
                v.sh.backdrop.glass(&content, radius, sx, sy, v.sh.tint)
            });
            st.thumbs.insert(key, thumb);
        }
        if let Some(Some(t)) = st.thumbs.get(key).cloned() {
            ui.image(&t, tx + 10.0 + (tile_w - 20.0 - t.w as f32 / s) / 2.0, ty + 10.0 + (stage_h - t.h as f32 / s) / 2.0);
        }
        ui.text(tx + 16.0, ty + stage_h + 26.0, widgets::title(key), 13.5, FG, Face::Semibold, Anchor::LM, Some(tile_w - 60.0));
        ui.text(tx + 16.0, ty + stage_h + 46.0, widgets::description(key), 11.0, MUTED, Face::Regular, Anchor::LM, Some(tile_w - 32.0));
        let (cx, cy) = (tx + tile_w - 22.0, ty + stage_h + 26.0);
        if on {
            ui.circle(cx, cy, 10.0, ACCENT);
            ui.icon(cx, cy, "check", 11.0, FG);
        } else {
            ui.ring(cx, cy, 9.5, WHITE.with_a(120), 1.3);
        }
        ui.region(&id, tx, ty, tx + tile_w, ty + tile_h, A::Tile(key), Cursor::Hand);
    }
}

fn step_personal(st: &WelcomeState, ui: &mut Ui, x: f32, y: f32, w: f32) {
    let fx = x + 200.0;
    let fw = w - 200.0;
    ui.text(x, y + 18.0, "Cidade do clima", 13.0, FG, Face::Semibold, Anchor::LM, None);
    ui.text(x, y + 38.0, "Para a previsão do tempo", 11.0, MUTED, Face::Regular, Anchor::LM, None);
    ui.entry("cidade", fx, y, fx + fw, y + 36.0, &st.city_text, "Ex.: São Paulo", false);
    let sug_y = y + 40.0;
    let y = y + 74.0;
    ui.text(x, y + 18.0, "Usuário do GitHub", 13.0, FG, Face::Semibold, Anchor::LM, None);
    ui.text(x, y + 38.0, "Opcional", 11.0, MUTED, Face::Regular, Anchor::LM, None);
    ui.entry("usuario", fx, y, fx + fw, y + 36.0, &st.user, "seu-usuario", false);
    let y = y + 74.0;
    ui.text(x, y + 11.0, "Iniciar com o Windows", 13.0, FG, Face::Semibold, Anchor::LM, None);
    ui.text(x, y + 31.0, "Os widgets abrem sozinhos quando você entra", 11.0, MUTED, Face::Regular, Anchor::LM, None);
    ui.switch("iniciar", fx, y, st.startup, A::Toggle("welcome_startup"), None);
    if !st.chosen.get("sistema").copied().unwrap_or(false) {
        ui.text(fx, y + 60.0, "O widget de clima não foi escolhido; a cidade fica guardada para depois.", 11.0, FAINT, Face::Regular, Anchor::LA, None);
    }
    st.city.draw(ui, fx, sug_y, fw);
}

fn step_tips(ui: &mut Ui, x: f32, mut y: f32, w: f32, v: &View) {
    let _ = v;
    let items: Vec<(String, String)> = [TIPS[0], TIPS[2], TIPS[3], TIPS[5]].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    for (title, desc) in items {
        if title.contains('+') {
            ui.keycaps(x, y + 12.0, &title);
        } else {
            ui.text(x, y + 12.0, &title, 13.0, FG, Face::Semibold, Anchor::LM, None);
        }
        let lines = ui.wrap(&desc, 12.0, w - 190.0, 2);
        for (j, line) in lines.iter().enumerate() {
            ui.text(x + 190.0, y + 4.0 + j as f32 * 17.0, line, 12.0, MUTED, Face::Regular, Anchor::LA, None);
        }
        y += 26.0 + 17.0 * lines.len().max(1) as f32;
    }
}
