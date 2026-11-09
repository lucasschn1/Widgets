//! As janelas de painel do app: configurações, perguntas de vidro e boas-vindas.
//! (Módulo filho de `app`: mexe direto no estado do App.)

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::*;
use crate::card::render_kind;
use crate::gfx::{Canvas, Image};
use crate::panel::{PanelWin, Ui, A};
use crate::prompt::{self, PromptState, Purpose, WelcomeState};
use crate::settings::{self, SettingsState, View};
use crate::startup;

pub enum PanelKind {
    Settings(SettingsState),
    Prompt(PromptState),
    Welcome(WelcomeState),
}

pub struct Panel {
    pub win: PanelWin,
    pub kind: PanelKind,
    pub opened: Instant,
}

impl App {
    // ---------- abrir ----------

    pub(super) fn panel_index(&self, hwnd: isize) -> Option<usize> {
        self.panels.iter().position(|p| p.win.id() == hwnd)
    }

    fn find_panel(&self, f: impl Fn(&PanelKind) -> bool) -> Option<usize> {
        self.panels.iter().position(|p| f(&p.kind))
    }

    pub(super) fn open_settings(&mut self, page: &'static str) {
        if let Some(i) = self.find_panel(|k| matches!(k, PanelKind::Settings(_))) {
            if let PanelKind::Settings(st) = &mut self.panels[i].kind {
                st.go(page);
            }
            self.render_panel(i);
            self.panels[i].win.show();
            return;
        }
        let win = PanelWin::new("Configurações dos widgets", settings::W, settings::H, None, false);
        self.panels.push(Panel { win, kind: PanelKind::Settings(SettingsState::new(page)), opened: Instant::now() });
        let i = self.panels.len() - 1;
        self.render_panel(i);
        self.panels[i].win.show();
    }

    pub(super) fn open_welcome(&mut self) {
        if let Some(i) = self.find_panel(|k| matches!(k, PanelKind::Welcome(_))) {
            self.panels[i].win.show();
            return;
        }
        let visible: Vec<&'static str> = self.cards.iter().map(|c| c.key()).collect();
        let first = !self.sh.cfg["app"].bool_of("dicas_vistas").unwrap_or(false);
        let st = WelcomeState::new(&self.sh.cfg, &visible, startup::enabled(), first);
        let win = PanelWin::new("Bem-vindo aos widgets", prompt::WELCOME_W, prompt::WELCOME_H, None, false);
        self.panels.push(Panel { win, kind: PanelKind::Welcome(st), opened: Instant::now() });
        let i = self.panels.len() - 1;
        self.render_panel(i);
        self.panels[i].win.show();
    }

    /// Pergunta de vidro junto de um widget (ou no meio da tela).
    pub(super) fn open_prompt(&mut self, purpose: Purpose, anchor: Option<Rect>) {
        if let Some(i) = self.find_panel(|k| matches!(k, PanelKind::Prompt(_))) {
            self.close_panel(i);
        }
        let (title, message, value, placeholder): (&str, &str, String, &str) = match purpose {
            Purpose::City => (
                "Cidade do clima",
                "Digite o nome da cidade e escolha na lista.",
                self.sh.cfg["tempo"].str_of("city").unwrap_or("").into(),
                "Ex.: São Paulo",
            ),
            Purpose::GitHubUser => (
                "GitHub",
                "Seu usuário do GitHub (o que vem depois de github.com/).",
                self.sh.cfg["github"].str_of("usuario").unwrap_or("").into(),
                "usuario",
            ),
            Purpose::LayoutName => ("Salvar layout", "Dê um nome para a posição atual dos widgets (ex.: Trabalho, Jogos).", String::new(), "Nome do layout"),
        };
        let st = PromptState {
            purpose,
            title: title.into(),
            message: message.into(),
            value,
            placeholder: placeholder.into(),
            ok_label: "Salvar".into(),
            suggest: Default::default(),
            picked: None,
        };
        let h = st.height();
        let pos = anchor.map(|(x0, y0, x1, y1)| {
            let s = win::scale_at((x0 + x1) / 2, (y0 + y1) / 2);
            let (w, hh) = ((prompt::PROMPT_W * s) as i32, ((h + 6.0 * crate::panel::SUGGEST_ROW) * s) as i32);
            let area = win::area_for((x0, y0, x1, y1));
            let gap = (10.0 * s) as i32;
            let x = (x1 - w).max(area.0 + gap).min(area.2 - w - gap);
            let y = if y1 + gap + hh <= area.3 { y1 + gap } else { (y0 - gap - (h * s) as i32).max(area.1 + gap) };
            (x, y)
        });
        let win = PanelWin::new(title, prompt::PROMPT_W, h, pos, true);
        self.panels.push(Panel { win, kind: PanelKind::Prompt(st), opened: Instant::now() });
        let i = self.panels.len() - 1;
        self.render_panel(i);
        self.panels[i].win.show();
        self.panels[i].win.focus_entry("valor", true);
    }

    pub(super) fn close_panel(&mut self, i: usize) {
        let p = self.panels.remove(i);
        if let PanelKind::Welcome(_) = &p.kind {
            if !self.sh.cfg["app"].bool_of("dicas_vistas").unwrap_or(false) {
                self.sh.cfg.obj_mut("app").insert("dicas_vistas".into(), json!(true));
                self.sh.save();
            }
        }
        p.win.destroy();
    }

    /// Algo mudou fora das configurações: elas se redesenham (e as miniaturas são refeitas).
    pub(super) fn panels_changed(&mut self) {
        for p in &mut self.panels {
            if let PanelKind::Settings(st) = &mut p.kind {
                st.thumbs.clear();
            }
            p.win.dirty = true;
        }
    }

    // ---------- desenhar ----------

    /// Um widget desenhado no tamanho e escala pedidos: o de verdade, ou um de exemplo.
    fn render_widget(&self, key: &'static str, mode: Mode, s: f32) -> Option<(Image, f32)> {
        if let Some(c) = self.cards.iter().find(|c| c.key() == key) {
            return Some((render_kind(c.kind.as_ref(), mode, s, &self.sh), widgets::Layout::of(mode).radius * s));
        }
        let kind = widgets::sample(key, &self.sh.cfg, &self.sh.cache)?;
        Some((render_kind(kind.as_ref(), mode, s, &self.sh), widgets::Layout::of(mode).radius * s))
    }

    pub(super) fn render_panel(&mut self, i: usize) {
        let mut p = self.panels.remove(i);
        if let PanelKind::Prompt(st) = &p.kind {
            let h = st.height();
            if (h - p.win.h).abs() > 0.5 {
                p.win.resize(prompt::PROMPT_W, h);
            }
        }
        let bg = p.win.background(&self.sh.backdrop);
        let (pw, ph) = p.win.px();
        let mut c = Canvas::new(pw, ph, p.win.s);
        c.text_scale = self.sh.text_scale;
        c.image(&bg, 0.0, 0.0);
        let render = |key: &'static str, mode: Mode, s: f32| self.render_widget(key, mode, s);
        let view = View {
            sh: &self.sh,
            visible: self.cards.iter().map(|c| c.key()).collect(),
            modes: self.cards.iter().map(|c| (c.key(), if c.temp { Mode::P } else { c.mode })).collect(),
            origin: p.win.client_origin(),
            render: &render,
            startup: startup::enabled(),
            update_status: self.update_status.clone(),
            update_version: self.update_version.clone(),
        };
        p.win.anims.running = false;
        let (w, h) = (p.win.w, p.win.h);
        let (regions, entries) = {
            let mut ui = Ui {
                c: &mut c,
                regions: Vec::new(),
                entries: Vec::new(),
                hover: p.win.hover.as_deref(),
                pressed: p.win.pressed.as_deref(),
                focus: p.win.focus.as_deref(),
                anims: &mut p.win.anims,
                animations: self.sh.animations,
                ox: 0.0,
                oy: 0.0,
                clip: (0.0, 0.0, w, h),
            };
            match &mut p.kind {
                PanelKind::Settings(st) => settings::draw(st, &mut ui, &view),
                PanelKind::Prompt(st) => prompt::draw_prompt(st, &mut ui, h),
                PanelKind::Welcome(st) => prompt::draw_welcome(st, &mut ui, &view),
            }
            (ui.regions, ui.entries)
        };
        p.win.regions = regions;
        p.win.sync_entries(&entries, self.sh.text_scale);
        events::set_frame(p.win.hwnd, &c.finish());
        p.win.dirty = false;
        self.panels.insert(i, p);
    }

    // ---------- mouse e teclado ----------

    pub(super) fn panel_mouse(&mut self, i: usize, kind: MouseKind, x: i32, y: i32) {
        let s = self.panels[i].win.s;
        let (lx, ly) = (x as f32 / s, y as f32 / s);
        match kind {
            MouseKind::Move => {
                if let Some(id) = self.panels[i].win.dragging.clone() {
                    if let Some(slider) = self.panels[i].win.region(&id).and_then(|r| r.slider) {
                        self.slider_moved(&id, slider.value_at(lx), false);
                    }
                    return;
                }
                let (id, cursor) = match self.panels[i].win.hit(lx, ly) {
                    Some(r) => (Some(r.id.clone()), r.cursor),
                    None => (None, Cursor::Arrow),
                };
                events::set_cursor(self.panels[i].win.hwnd, cursor);
                if id != self.panels[i].win.hover {
                    self.panels[i].win.hover = id;
                    self.panels[i].win.dirty = true;
                }
            }
            MouseKind::Leave => {
                if self.panels[i].win.hover.take().is_some() && self.panels[i].win.dragging.is_none() {
                    self.panels[i].win.dirty = true;
                }
            }
            MouseKind::Down => {
                let Some((id, slider)) = self.panels[i].win.hit(lx, ly).map(|r| (r.id.clone(), r.slider)) else { return };
                self.panels[i].win.pressed = Some(id.clone());
                if let Some(slider) = slider {
                    self.panels[i].win.dragging = Some(id.clone());
                    self.slider_moved(&id, slider.value_at(lx), false);
                }
                self.panels[i].win.dirty = true;
            }
            MouseKind::Up => {
                let pressed = self.panels[i].win.pressed.take();
                if let Some(id) = self.panels[i].win.dragging.take() {
                    if let Some(slider) = self.panels[i].win.region(&id).and_then(|r| r.slider) {
                        self.slider_moved(&id, slider.value_at(lx), true);
                    }
                } else if let Some(pressed) = pressed {
                    let hit = self.panels[i].win.hit(lx, ly).filter(|r| r.id == pressed).map(|r| r.action.clone());
                    if let Some(action) = hit {
                        self.panel_action(i, action);
                    }
                }
                if let Some(p) = self.panels.get_mut(i) {
                    p.win.dirty = true;
                }
            }
            _ => {}
        }
    }

    pub(super) fn panel_wheel(&mut self, i: usize, delta: i32) {
        if let PanelKind::Settings(st) = &mut self.panels[i].kind {
            if st.scroll(delta as f32 / 120.0) {
                self.panels[i].win.dirty = true;
            }
        }
    }

    pub(super) fn panel_key(&mut self, i: usize, vk: u32) {
        if vk == 0x1B && self.panels[i].win.focus.is_none() {
            self.close_panel(i);
        } else if vk == 0x0D {
            self.welcome_enter(i);
        }
    }

    /// Enter nas boas-vindas: o botão azul (continuar ou começar a usar).
    fn welcome_enter(&mut self, i: usize) {
        if let PanelKind::Welcome(st) = &self.panels[i].kind {
            let action = if st.last() { A::Finish } else { A::Next };
            self.panel_action(i, action);
            if let Some(p) = self.panels.get_mut(i) {
                p.win.dirty = true;
            }
        }
    }

    pub(super) fn edit_event(&mut self, i: usize, ctrl: u16, what: EditWhat) {
        let Some(id) = self.panels[i].win.entry_id(ctrl) else { return };
        let text = self.panels[i].win.entry_text(&id);
        match what {
            EditWhat::Focus(on) => {
                self.panels[i].win.focus = on.then(|| id.clone());
                if !on && id == "usuario" {
                    if let PanelKind::Settings(_) = self.panels[i].kind {
                        self.apply_github_user(&text);
                    } else if let PanelKind::Welcome(st) = &mut self.panels[i].kind {
                        st.user = text.trim().into();
                    }
                }
                self.panels[i].win.dirty = true;
            }
            EditWhat::Change => {
                match &mut self.panels[i].kind {
                    PanelKind::Prompt(st) => {
                        st.value = text.clone();
                        st.picked = None;
                        if st.purpose == Purpose::City {
                            st.suggest.typed(&text);
                        }
                    }
                    PanelKind::Settings(st) if id == "cidade" => st.city.typed(&text),
                    PanelKind::Welcome(st) if id == "cidade" => {
                        st.city_text = text.clone();
                        st.city_place = None;
                        st.city.typed(&text);
                    }
                    PanelKind::Welcome(st) if id == "usuario" => st.user = text.trim().into(),
                    _ => {}
                }
                self.panels[i].win.dirty = true;
            }
            EditWhat::Key(vk) => self.edit_key(i, &id, &text, vk),
        }
    }

    fn edit_key(&mut self, i: usize, id: &str, text: &str, vk: u32) {
        let suggest = match &mut self.panels[i].kind {
            PanelKind::Prompt(st) => Some(&mut st.suggest),
            PanelKind::Settings(st) if id == "cidade" => Some(&mut st.city),
            PanelKind::Welcome(st) if id == "cidade" => Some(&mut st.city),
            _ => None,
        };
        if let Some(sg) = suggest {
            match vk {
                0x26 | 0x28 => {
                    if sg.arrow(vk == 0x28) {
                        self.panels[i].win.dirty = true;
                    }
                    return;
                }
                0x0D if sg.index.is_some() => {
                    let pick = sg.index.unwrap();
                    self.panel_action(i, A::Pick(pick));
                    return;
                }
                0x1B if !sg.results.is_empty() => {
                    sg.clear();
                    self.panels[i].win.dirty = true;
                    return;
                }
                _ => {}
            }
        }
        match (&self.panels[i].kind, vk) {
            (PanelKind::Prompt(_), 0x0D) => self.panel_action(i, A::Ok),
            (PanelKind::Prompt(_), 0x1B) => self.close_panel(i),
            (PanelKind::Welcome(_), 0x0D) => {
                if id == "usuario" {
                    if let PanelKind::Welcome(st) = &mut self.panels[i].kind {
                        st.user = text.trim().into();
                    }
                }
                self.welcome_enter(i);
            }
            (PanelKind::Settings(_), 0x0D) if id == "cidade" => {
                if !text.trim().is_empty() {
                    self.apply_city(text.trim(), None);
                }
            }
            (PanelKind::Settings(_), 0x0D) if id == "usuario" => self.apply_github_user(text),
            _ => {}
        }
    }

    // ---------- ações ----------

    fn panel_action(&mut self, i: usize, action: A) {
        self.panels[i].win.dirty = true;
        match action {
            A::Go(page) => {
                if let PanelKind::Settings(st) = &mut self.panels[i].kind {
                    st.go(page);
                }
            }
            A::Focus(id) => self.panels[i].win.focus_entry(&id, false),
            A::Toggle("texto_auto") => {
                let follow = config::setting_f64(&self.sh.cfg, "texto") > 0.0;
                let value = if follow { 0.0 } else { (self.sh.text_scale as f64 * 100.0).round() / 100.0 };
                self.set_setting("texto", json!(value));
            }
            A::Toggle("pomodoro_auto") => self.widget_or_cfg("pomodoro", |w| {
                let on = w.bool_of("auto").unwrap_or(false);
                w.as_object_mut().unwrap().insert("auto".into(), json!(!on));
            }),
            A::Toggle("capa_hd") => {
                let on = widgets::media::hd_covers(&self.sh.cfg);
                self.widget_or_cfg("midia", |w| {
                    if !w.is_object() {
                        *w = json!({});
                    }
                    w.as_object_mut().unwrap().insert("capa_hd".into(), json!(!on));
                });
            }
            A::Toggle("tempo_tremer") => {
                let on = widgets::tempo::shakes(&self.sh.cfg);
                self.widget_or_cfg("tempo", |w| {
                    if !w.is_object() {
                        *w = json!({});
                    }
                    w.as_object_mut().unwrap().insert("tremer".into(), json!(!on));
                });
            }
            A::Toggle("claude_nomes") => {
                let on = widgets::claude::show_names(&self.sh.cfg);
                self.widget_or_cfg("claude", |w| {
                    if !w.is_object() {
                        *w = json!({});
                    }
                    w.as_object_mut().unwrap().insert("balao_nomes".into(), json!(!on));
                });
            }
            A::Toggle("welcome_startup") => {
                if let PanelKind::Welcome(st) = &mut self.panels[i].kind {
                    st.startup = !st.startup;
                }
            }
            A::Toggle(name) => {
                let on = config::setting_bool(&self.sh.cfg, name);
                self.set_setting(name, json!(!on));
            }
            A::Notify(kind) => {
                let mut all = config::setting(&self.sh.cfg, "notificacoes");
                let on = config::notify_enabled(&self.sh.cfg, kind);
                all[kind] = json!(!on);
                self.sh.cfg.obj_mut("app").insert("notificacoes".into(), all);
                self.sh.save();
            }
            A::Startup => {
                if !startup::set(!startup::enabled()) {
                    self.toast("Não deu para mudar a inicialização", None, "close", WARNING, None);
                }
            }
            A::Lock => {
                let locked = self.sh.locked();
                self.sh.cfg.obj_mut("app").insert("travado".into(), json!(!locked));
                self.sh.save();
            }
            A::Seg(id, value) => self.segment(id, &value),
            A::Visible(key, on) => {
                if self.set_visible(key, on) && !on {
                    let text = format!("{} oculto", widgets::title(key));
                    self.toast(&text, None, "close", WHITE, Some(("Desfazer", Undo::Show(key))));
                }
            }
            A::Widget("app", 1) => self.install_update(),
            A::Widget("app", 2) => self.check_updates(true),
            A::Widget(key, n) => {
                if let Some(j) = self.find(key) {
                    self.run_action(j, Act::Kind(n));
                } else if key == "claude" {
                    // o widget está oculto: liga ou desliga direto
                    let result = if n == widgets::claude::CONNECT { crate::claude_usage::connect() } else { crate::claude_usage::disconnect() };
                    if let Err(e) = result {
                        self.toast(&e, None, "close", WHITE, None);
                    }
                } else if key == "pomodoro" && n == widgets::pomodoro::RESET_DAY {
                    self.sh.cfg.obj_mut("pomodoro").remove("hoje");
                    self.sh.save();
                }
            }
            A::Edit => self.start_edit(),
            A::Arrange => self.arrange(),
            A::SaveLayout => self.open_prompt(Purpose::LayoutName, None),
            A::ApplyLayout(name) => self.apply_layout(&name),
            A::DeleteLayout(name) => self.delete_layout(&name),
            A::Restore(page) => self.restore_page(page),
            A::TestNotify => self.tray.notify("Tudo certo", "As notificações dos widgets estão funcionando."),
            A::Welcome => self.open_welcome(),
            A::OpenData => win::open_target(&config::data_dir().display().to_string()),
            A::ResetAll => self.reset_all(),
            A::Pick(n) => self.pick(i, n),
            A::Ok => self.prompt_ok(i),
            A::Cancel => self.close_panel(i),
            A::Tile(key) => {
                if let PanelKind::Welcome(st) = &mut self.panels[i].kind {
                    let on = st.chosen.get(key).copied().unwrap_or(false);
                    st.chosen.insert(key, !on);
                }
            }
            A::Next => {
                if let PanelKind::Welcome(st) = &mut self.panels[i].kind {
                    if st.step == 0 && !st.chosen.values().any(|v| *v) {
                        self.toast("Escolha pelo menos um widget", None, "close", WARNING, None);
                        return;
                    }
                    st.step += 1;
                }
            }
            A::Back => {
                if let PanelKind::Welcome(st) = &mut self.panels[i].kind {
                    st.step = st.step.saturating_sub(1);
                    st.city.clear();
                }
            }
            A::Step(n) => {
                if let PanelKind::Welcome(st) = &mut self.panels[i].kind {
                    st.step = n.min(st.step);
                    st.city.clear();
                }
            }
            A::Finish => self.finish_welcome(i),
            A::Size(..) | A::Slider(_) => {}
        }
    }

    /// Mexe na seção de um widget: pelo widget aberto (que se redesenha) ou direto no config.
    fn widget_or_cfg(&mut self, key: &'static str, f: impl FnOnce(&mut Value)) {
        f(&mut self.sh.cfg[key]);
        self.sh.save();
        if let Some(j) = self.find(key) {
            self.cards[j].refresh(&self.sh);
        }
    }

    fn segment(&mut self, id: &'static str, value: &str) {
        match id {
            "animacoes" => self.set_setting("animacoes", json!(value)),
            "fonte" => {
                let index = ["auto", "spotify", "youtube"].iter().position(|v| *v == value).unwrap_or(0) as u32;
                match self.find("midia") {
                    Some(j) => self.run_action(j, Act::Kind(10 + index)),
                    None => {
                        self.sh.cfg.obj_mut("midia").insert("fonte".into(), json!(value));
                        self.sh.save();
                    }
                }
            }
            "longa_a_cada" => {
                let n: u32 = value.parse().unwrap_or(4);
                match self.find("pomodoro") {
                    Some(j) => self.run_action(j, Act::Kind(900 + n)),
                    None => {
                        self.sh.cfg.obj_mut("pomodoro").insert("longa_a_cada".into(), json!(n));
                        self.sh.save();
                    }
                }
            }
            id if id.starts_with("tamanho:") => {
                let key = widgets::KEYS.iter().copied().find(|k| id.ends_with(k)).unwrap_or("sistema");
                let Some(mode) = Mode::from_code(value) else { return };
                match self.find(key) {
                    Some(j) => self.set_mode(j, mode),
                    None => {
                        widgets::store_size(self.sh.cfg.obj_mut(key), mode);
                        self.sh.save();
                    }
                }
            }
            _ => {}
        }
    }

    /// Slider arrastado: a opção muda na hora; o vidro (mais pesado) acompanha a cada ~150 ms.
    fn slider_moved(&mut self, id: &str, value: f64, release: bool) {
        let (name, stored): (&'static str, Value) = match id {
            // números redondos no config.json (40, não 40.0; 0.41, não 0.41000000000000003)
            "desfoque" => ("desfoque", json!(value.round() as i64)),
            "escurecimento" => ("escurecimento", json!(value.round() / 100.0)),
            "texto" => ("texto", json!(value.round() / 100.0)),
            "opacidade_ociosa" => ("opacidade_ociosa", json!(value.round() / 100.0)),
            "dur:foco" | "dur:pausa" | "dur:longa" => {
                let phase = &id[4..];
                let minutes = value.round() as u32;
                let index = ["foco", "pausa", "longa"].iter().position(|p| *p == phase).unwrap_or(0) as u32;
                match self.find("pomodoro") {
                    Some(j) => self.run_action(j, Act::Kind(1000 + index * 1000 + minutes)),
                    None => {
                        self.sh.cfg.obj_mut("pomodoro").entry("duracoes").or_insert(json!({}))[phase] = json!(minutes);
                        self.sh.save();
                    }
                }
                self.panels_changed();
                return;
            }
            _ => return,
        };
        self.sh.cfg.obj_mut("app").insert(name.into(), stored.clone());
        if name == "texto" {
            self.sh.read_preferences();
        }
        self.panels_dirty();
        if release {
            self.pending_setting = None;
            self.set_setting(name, stored);
        } else if self.pending_setting.is_none() {
            self.pending_setting = Some((Instant::now() + Duration::from_millis(150), name));
        }
    }

    fn panels_dirty(&mut self) {
        for p in &mut self.panels {
            p.win.dirty = true;
        }
    }

    /// Grava uma opção geral e aplica na hora (as configurações não têm botão "Salvar").
    pub(super) fn set_setting(&mut self, name: &'static str, value: Value) {
        if value == config::default_setting(name) {
            self.sh.cfg.obj_mut("app").remove(name);
        } else {
            self.sh.cfg.obj_mut("app").insert(name.into(), value);
        }
        self.sh.save();
        self.apply_settings(name);
    }

    pub(super) fn apply_settings(&mut self, name: &str) {
        self.sh.read_preferences();
        if matches!(name, "desfoque" | "escurecimento" | "*") {
            let blur = config::setting_f64(&self.sh.cfg, "desfoque");
            self.sh.backdrop.check(blur);
        }
        let redraw = matches!(name, "texto" | "*" | "animacoes");
        for c in &mut self.cards {
            if redraw {
                c.refresh(&self.sh);
            } else {
                c.present(&self.sh, false);
            }
        }
        self.panels_changed();
    }

    fn restore_page(&mut self, page: &'static str) {
        let keys: Vec<&str> = match page {
            "geral" => settings::BEHAVIOR_KEYS.to_vec(),
            "aparencia" => settings::APPEARANCE_KEYS.to_vec(),
            _ => vec!["notificacoes"],
        };
        let old = self.sh.cfg["app"].clone();
        let changed = keys.iter().any(|k| old.get(*k).is_some());
        if !changed {
            self.toast("Já está tudo no padrão", None, "check", SUCCESS, None);
            return;
        }
        for k in keys {
            self.sh.cfg.obj_mut("app").remove(k);
        }
        self.sh.save();
        self.apply_settings("*");
        self.toast("Restaurado o padrão", None, "undo", SUCCESS, Some(("Desfazer", Undo::AppSettings(old))));
    }

    fn reset_all(&mut self) {
        let old = self.sh.cfg["app"].clone();
        let keep: serde_json::Map<String, Value> =
            old.as_object().map(|m| m.iter().filter(|(k, _)| matches!(k.as_str(), "layouts" | "dicas_vistas" | "travado")).map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
        self.sh.cfg["app"] = Value::Object(keep);
        self.sh.save();
        self.apply_settings("*");
        self.toast("Configurações restauradas", None, "undo", SUCCESS, Some(("Desfazer", Undo::AppSettings(old))));
    }

    // ---------- cidade e usuário ----------

    fn pick(&mut self, i: usize, n: usize) {
        match &mut self.panels[i].kind {
            PanelKind::Prompt(st) => {
                if let Some(place) = st.suggest.results.get(n).cloned() {
                    st.value = place.str_of("label").unwrap_or("").into();
                    st.picked = Some(place);
                    st.suggest.clear();
                    self.prompt_ok(i);
                }
            }
            PanelKind::Settings(st) => {
                if let Some(place) = st.city.results.get(n).cloned() {
                    st.city.clear();
                    let label = place.str_of("label").unwrap_or("").to_string();
                    self.panels[i].win.set_entry("cidade", &label);
                    self.apply_city(&label, Some(place));
                }
            }
            PanelKind::Welcome(st) => {
                if let Some(place) = st.city.results.get(n).cloned() {
                    st.city.clear();
                    let label = place.str_of("label").unwrap_or("").to_string();
                    st.city_text = label.clone();
                    st.city_place = Some(place);
                    self.panels[i].win.set_entry("cidade", &label);
                }
            }
        }
    }

    pub(super) fn apply_city(&mut self, text: &str, place: Option<Value>) {
        match self.find("tempo") {
            Some(j) => {
                self.with_kind(j, |k, ctx| k.input("city", text, place.as_ref(), ctx));
                self.cards[j].refresh(&self.sh);
            }
            None => {
                let w = self.sh.cfg.obj_mut("tempo");
                match &place {
                    Some(p) => {
                        w.insert("city".into(), p["label"].clone());
                        w.insert("lat".into(), p["lat"].clone());
                        w.insert("lon".into(), p["lon"].clone());
                        w.insert("place".into(), p["label"].clone());
                    }
                    None => {
                        w.insert("city".into(), json!(text));
                        for k in ["lat", "lon", "place"] {
                            w.insert(k.into(), Value::Null);
                        }
                    }
                }
                self.sh.save();
            }
        }
        let label = place.as_ref().and_then(|p| p.str_of("label")).unwrap_or(text).to_string();
        self.toast(&format!("Cidade: {label}"), None, "location", ACCENT, None);
    }

    fn apply_github_user(&mut self, text: &str) {
        let user = text.trim().trim_start_matches('@').to_string();
        if user.is_empty() || self.sh.cfg["github"].str_of("usuario") == Some(user.as_str()) {
            return;
        }
        match self.find("github") {
            Some(j) => {
                self.with_kind(j, |k, ctx| k.input("user", &user, None, ctx));
                self.cards[j].refresh(&self.sh);
            }
            None => {
                self.sh.cfg.obj_mut("github").insert("usuario".into(), json!(user));
                self.sh.save();
            }
        }
        self.toast(&format!("GitHub: @{user}"), None, "check", SUCCESS, None);
    }

    fn prompt_ok(&mut self, i: usize) {
        let text = self.panels[i].win.entry_text("valor").trim().to_string();
        let PanelKind::Prompt(st) = &self.panels[i].kind else { return };
        let purpose = st.purpose;
        let picked = st.picked.clone().filter(|p| p.str_of("label") == Some(text.as_str()));
        self.close_panel(i);
        if text.is_empty() {
            return;
        }
        match purpose {
            Purpose::City => self.apply_city(&text, picked),
            Purpose::GitHubUser => self.apply_github_user(&text),
            Purpose::LayoutName => self.save_layout(&text),
        }
    }

    fn save_layout(&mut self, name: &str) {
        let mut snapshot = serde_json::Map::new();
        for key in widgets::KEYS {
            let state = match self.find(key) {
                Some(j) => {
                    let c = &self.cards[j];
                    json!({"visible": true, "x": c.pos.0, "y": c.pos.1, "tamanho": if c.temp { "p" } else { c.mode.code() }})
                }
                None => json!({"visible": false}),
            };
            snapshot.insert(key.to_string(), state);
        }
        // em que monitores estava (para aplicar certo com outra combinação de telas)
        snapshot.insert("_monitores".into(), super::screens::monitors_to_json(&self.screens));
        self.sh.cfg.obj_mut("app").entry("layouts").or_insert(json!({}))[name] = Value::Object(snapshot);
        self.sh.save();
        self.panels_changed();
        self.toast(&format!("Layout \"{name}\" salvo"), Some("Aplique pelo menu Layouts"), "check", SUCCESS, None);
    }

    fn finish_welcome(&mut self, i: usize) {
        let PanelKind::Welcome(st) = &self.panels[i].kind else { return };
        let chosen = st.chosen.clone();
        let (city, place, startup_on) = (st.city_text.trim().to_string(), st.city_place.clone(), st.startup);
        let user = self.panels[i].win.entry_text("usuario");
        let user = if user.trim().is_empty() { st.user.clone() } else { user };
        for key in widgets::KEYS {
            let on = chosen.get(key).copied().unwrap_or(false);
            if on != self.find(key).is_some() {
                self.set_visible(key, on);
            }
        }
        let system = &self.sh.cfg["tempo"];
        if !city.is_empty() && Some(city.as_str()) != system.str_of("place").or(system.str_of("city")) {
            self.apply_city(&city, place);
        }
        self.apply_github_user(&user);
        startup::set(startup_on);
        self.sh.cfg.obj_mut("app").insert("dicas_vistas".into(), json!(true));
        self.sh.save();
        self.close_panel(i);
        self.toast("Tudo pronto!", Some("Clique direito num widget para ver as opções"), "check", SUCCESS, None);
    }

    // ---------- timers ----------

    pub(super) fn panels_due(&self) -> Option<Instant> {
        let mut next: Option<Instant> = self.pending_setting.map(|p| p.0);
        for p in &self.panels {
            let mut t = None;
            if p.win.dirty || p.win.anims.running {
                t = Some(Instant::now() + Duration::from_millis(FRAME_MS));
            }
            let sg = match &p.kind {
                PanelKind::Settings(st) => st.city.next_due(),
                PanelKind::Prompt(st) => st.suggest.next_due(),
                PanelKind::Welcome(st) => st.city.next_due(),
            };
            if matches!(p.kind, PanelKind::Prompt(_)) {
                t = Some(t.map_or(Instant::now() + Duration::from_millis(200), |x: Instant| x.min(Instant::now() + Duration::from_millis(200))));
            }
            for x in [t, sg].into_iter().flatten() {
                next = Some(next.map_or(x, |n: Instant| n.min(x)));
            }
        }
        next
    }

    pub(super) fn panels_tick(&mut self) {
        if let Some((when, name)) = self.pending_setting {
            if Instant::now() >= when {
                self.pending_setting = None;
                let value = self.sh.cfg["app"].get(name).cloned().unwrap_or(Value::Null);
                if !value.is_null() {
                    self.set_setting(name, value);
                }
            }
        }
        let mut i = 0;
        while i < self.panels.len() {
            // a pergunta some ao clicar fora dela
            if matches!(self.panels[i].kind, PanelKind::Prompt(_)) && self.panels[i].opened.elapsed() > Duration::from_millis(600) {
                let fg = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
                if fg != self.panels[i].win.hwnd {
                    self.close_panel(i);
                    continue;
                }
            }
            let changed = match &mut self.panels[i].kind {
                PanelKind::Settings(st) => st.city.poll(),
                PanelKind::Prompt(st) => st.suggest.poll(),
                PanelKind::Welcome(st) => st.city.poll(),
            };
            if changed || self.panels[i].win.dirty || self.panels[i].win.anims.running {
                self.render_panel(i);
            }
            i += 1;
        }
    }
}

pub enum EditWhat {
    Focus(bool),
    Change,
    Key(u32),
}

