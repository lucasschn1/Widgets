//! As janelas de painel do app: configurações, perguntas de vidro e boas-vindas.
//! (Módulo filho de `app`: mexe direto no estado do App.)

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::*;
use crate::card::render_kind;
use crate::gfx::{Canvas, Image};
use crate::ui::panel::{PanelWin, Ui, Action};
use crate::ui::prompt::{self, PromptState, Purpose, WelcomeState};
use crate::ui::settings::{self, SettingsState, View};
use crate::platform::startup;

/// a opção mexida num slider é aplicada (o vidro refeito) no máximo a cada 150 ms
const SLIDER_APPLY: Duration = Duration::from_millis(150);
/// a pergunta aberta confere a cada 200 ms se ainda tem o foco (some ao clicar fora)...
const PROMPT_FOCUS_CHECK: Duration = Duration::from_millis(200);
/// ...depois de 600 ms aberta (o clique que a abriu ainda pode estar terminando)
const PROMPT_GRACE: Duration = Duration::from_millis(600);
/// um "dente" da roda do mouse, no Windows
const WHEEL_DELTA: f32 = 120.0;

/// O que um painel é.
pub enum PanelKind {
    Settings(SettingsState),
    Prompt(PromptState),
    Welcome(WelcomeState),
}

/// Um painel aberto: a janela e o que ela mostra.
pub struct Panel {
    pub win: PanelWin,
    pub kind: PanelKind,
    pub opened: Instant,
}

impl App {
    // ---------- abrir ----------

    /// O painel da janela `hwnd`.
    pub(super) fn panel_index(&self, hwnd: isize) -> Option<usize> {
        self.panels.iter().position(|panel| panel.win.id() == hwnd)
    }

    /// O primeiro painel aberto que passa no teste.
    fn find_panel(&self, test: impl Fn(&PanelKind) -> bool) -> Option<usize> {
        self.panels.iter().position(|panel| test(&panel.kind))
    }

    /// Abre as Configurações na página `page` (ou leva as já abertas até ela).
    pub(super) fn open_settings(&mut self, page: &'static str) {
        if let Some(i) = self.find_panel(|kind| matches!(kind, PanelKind::Settings(_))) {
            if let PanelKind::Settings(state) = &mut self.panels[i].kind {
                state.go(page);
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

    /// Abre as boas-vindas.
    pub(super) fn open_welcome(&mut self) {
        if let Some(i) = self.find_panel(|kind| matches!(kind, PanelKind::Welcome(_))) {
            self.panels[i].win.show();
            return;
        }
        let visible: Vec<&'static str> = self.cards.iter().map(|card| card.key()).collect();
        let first = !self.sh.cfg["app"].bool_of("dicas_vistas").unwrap_or(false);
        let state = WelcomeState::new(&self.sh.cfg, &visible, startup::enabled(), first);
        let win = PanelWin::new("Bem-vindo aos widgets", prompt::WELCOME_W, prompt::WELCOME_H, None, false);
        self.panels.push(Panel { win, kind: PanelKind::Welcome(state), opened: Instant::now() });
        let i = self.panels.len() - 1;
        self.render_panel(i);
        self.panels[i].win.show();
    }

    /// Pergunta de vidro junto de um widget (ou no meio da tela).
    pub(super) fn open_prompt(&mut self, purpose: Purpose, anchor: Option<Rect>) {
        if let Some(i) = self.find_panel(|kind| matches!(kind, PanelKind::Prompt(_))) {
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
        let state = PromptState {
            purpose,
            title: title.into(),
            message: message.into(),
            value,
            placeholder: placeholder.into(),
            ok_label: "Salvar".into(),
            suggest: Default::default(),
            picked: None,
        };
        let h = state.height();
        // embaixo do widget (com espaço para a lista de sugestões aberta) ou, sem espaço, em cima
        let pos = anchor.map(|(x0, y0, x1, y1)| {
            let s = win::scale_at((x0 + x1) / 2, (y0 + y1) / 2);
            let (w, tallest) = ((prompt::PROMPT_W * s) as i32, ((h + 6.0 * crate::ui::panel::SUGGEST_ROW) * s) as i32);
            let area = win::area_for((x0, y0, x1, y1));
            let gap = (10.0 * s) as i32;
            let x = (x1 - w).max(area.0 + gap).min(area.2 - w - gap);
            let y = if y1 + gap + tallest <= area.3 { y1 + gap } else { (y0 - gap - (h * s) as i32).max(area.1 + gap) };
            (x, y)
        });
        let win = PanelWin::new(title, prompt::PROMPT_W, h, pos, true);
        self.panels.push(Panel { win, kind: PanelKind::Prompt(state), opened: Instant::now() });
        let i = self.panels.len() - 1;
        self.render_panel(i);
        self.panels[i].win.show();
        self.panels[i].win.focus_entry("valor", true);
    }

    /// Fecha o painel `i` (as boas-vindas fechadas contam como vistas).
    pub(super) fn close_panel(&mut self, i: usize) {
        let panel = self.panels.remove(i);
        if let PanelKind::Welcome(_) = &panel.kind {
            if !self.sh.cfg["app"].bool_of("dicas_vistas").unwrap_or(false) {
                self.sh.cfg.obj_mut("app").insert("dicas_vistas".into(), json!(true));
                self.sh.save();
            }
        }
        panel.win.destroy();
    }

    /// Algo mudou fora das configurações: elas se redesenham (e as miniaturas são refeitas).
    pub(super) fn panels_changed(&mut self) {
        for panel in &mut self.panels {
            if let PanelKind::Settings(state) = &mut panel.kind {
                state.thumbs.clear();
            }
            panel.win.dirty = true;
        }
    }

    // ---------- desenhar ----------

    /// Um widget desenhado no tamanho e escala pedidos: o de verdade, ou um de exemplo.
    fn render_widget(&self, key: &'static str, mode: Mode, s: f32) -> Option<(Image, f32)> {
        if let Some(card) = self.cards.iter().find(|card| card.key() == key) {
            return Some((render_kind(card.kind.as_ref(), mode, s, &self.sh), widgets::Layout::of(mode).radius * s));
        }
        let kind = widgets::sample(key, &self.sh.cfg, &self.sh.cache)?;
        Some((render_kind(kind.as_ref(), mode, s, &self.sh), widgets::Layout::of(mode).radius * s))
    }

    /// Redesenha o painel `i` (o fundo de vidro, a página, as áreas clicáveis e os campos).
    pub(super) fn render_panel(&mut self, i: usize) {
        let mut panel = self.panels.remove(i);
        if let PanelKind::Prompt(state) = &panel.kind {
            let h = state.height();
            if (h - panel.win.h).abs() > 0.5 {
                panel.win.resize(prompt::PROMPT_W, h);
            }
        }
        let bg = panel.win.background(&self.sh.backdrop);
        let (pw, ph) = panel.win.px();
        let mut c = Canvas::new(pw, ph, panel.win.s);
        c.text_scale = self.sh.text_scale;
        c.image(&bg, 0.0, 0.0);
        let render = |key: &'static str, mode: Mode, s: f32| self.render_widget(key, mode, s);
        let view = View {
            sh: &self.sh,
            visible: self.cards.iter().map(|card| card.key()).collect(),
            modes: self.cards.iter().map(|card| (card.key(), if card.temp { Mode::P } else { card.mode })).collect(),
            origin: panel.win.client_origin(),
            render: &render,
            startup: startup::enabled(),
            update_status: self.update_status.clone(),
            update_version: self.update_version.clone(),
            notify_access: crate::platform::notifications::access(),
        };
        panel.win.anims.running = false;
        let (w, h) = (panel.win.w, panel.win.h);
        let (regions, entries) = {
            let mut ui = Ui {
                c: &mut c,
                regions: Vec::new(),
                entries: Vec::new(),
                hover: panel.win.hover.as_deref(),
                pressed: panel.win.pressed.as_deref(),
                focus: panel.win.focus.as_deref(),
                anims: &mut panel.win.anims,
                animations: self.sh.animations,
                ox: 0.0,
                oy: 0.0,
                clip: (0.0, 0.0, w, h),
            };
            match &mut panel.kind {
                PanelKind::Settings(state) => settings::draw(state, &mut ui, &view),
                PanelKind::Prompt(state) => prompt::draw_prompt(state, &mut ui, h),
                PanelKind::Welcome(state) => prompt::draw_welcome(state, &mut ui, &view),
            }
            (ui.regions, ui.entries)
        };
        panel.win.regions = regions;
        panel.win.sync_entries(&entries, self.sh.text_scale);
        events::set_frame(panel.win.hwnd, &c.finish());
        panel.win.dirty = false;
        self.panels.insert(i, panel);
    }

    // ---------- mouse e teclado ----------

    /// O mouse no painel `i`: o destaque, os sliders arrastados e os cliques.
    pub(super) fn panel_mouse(&mut self, i: usize, kind: MouseKind, x: i32, y: i32) {
        let s = self.panels[i].win.s;
        let (lx, ly) = (x as f32 / s, y as f32 / s);
        match kind {
            MouseKind::Move => {
                if let Some(id) = self.panels[i].win.dragging.clone() {
                    if let Some(slider) = self.panels[i].win.region(&id).and_then(|region| region.slider) {
                        self.slider_moved(&id, slider.value_at(lx), false);
                    }
                    return;
                }
                let (id, cursor) = match self.panels[i].win.hit(lx, ly) {
                    Some(region) => (Some(region.id.clone()), region.cursor),
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
                let Some((id, slider)) = self.panels[i].win.hit(lx, ly).map(|region| (region.id.clone(), region.slider)) else { return };
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
                    if let Some(slider) = self.panels[i].win.region(&id).and_then(|region| region.slider) {
                        self.slider_moved(&id, slider.value_at(lx), true);
                    }
                } else if let Some(pressed) = pressed {
                    // um clique conta se soltou em cima de onde apertou
                    let hit = self.panels[i].win.hit(lx, ly).filter(|region| region.id == pressed).map(|region| region.action.clone());
                    if let Some(action) = hit {
                        self.panel_action(i, action);
                    }
                }
                if let Some(panel) = self.panels.get_mut(i) {
                    panel.win.dirty = true;
                }
            }
            _ => {}
        }
    }

    /// A roda do mouse no painel `i` (rola as Configurações).
    pub(super) fn panel_wheel(&mut self, i: usize, delta: i32) {
        if let PanelKind::Settings(state) = &mut self.panels[i].kind {
            if state.scroll(delta as f32 / WHEEL_DELTA) {
                self.panels[i].win.dirty = true;
            }
        }
    }

    /// Uma tecla no painel `i` (fora dos campos): Esc fecha, Enter segue nas boas-vindas.
    pub(super) fn panel_key(&mut self, i: usize, vk: u32) {
        if vk == 0x1B && self.panels[i].win.focus.is_none() {
            self.close_panel(i);
        } else if vk == 0x0D {
            self.welcome_enter(i);
        }
    }

    /// Enter nas boas-vindas: o botão azul (continuar ou começar a usar).
    fn welcome_enter(&mut self, i: usize) {
        if let PanelKind::Welcome(state) = &self.panels[i].kind {
            let action = if state.last() { Action::Finish } else { Action::Next };
            self.panel_action(i, action);
            if let Some(panel) = self.panels.get_mut(i) {
                panel.win.dirty = true;
            }
        }
    }

    /// Um campo de texto do painel `i` ganhou ou perdeu o foco, mudou ou recebeu uma tecla.
    pub(super) fn edit_event(&mut self, i: usize, ctrl: u16, what: EditWhat) {
        let Some(id) = self.panels[i].win.entry_id(ctrl) else { return };
        let text = self.panels[i].win.entry_text(&id);
        match what {
            EditWhat::Focus(on) => {
                self.panels[i].win.focus = on.then(|| id.clone());
                if !on && id == "usuario" {
                    if let PanelKind::Settings(_) = self.panels[i].kind {
                        self.apply_github_user(&text);
                    } else if let PanelKind::Welcome(state) = &mut self.panels[i].kind {
                        state.user = text.trim().into();
                    }
                }
                self.panels[i].win.dirty = true;
            }
            EditWhat::Change => {
                match &mut self.panels[i].kind {
                    PanelKind::Prompt(state) => {
                        state.value = text.clone();
                        state.picked = None;
                        if state.purpose == Purpose::City {
                            state.suggest.typed(&text);
                        }
                    }
                    PanelKind::Settings(state) if id == "cidade" => state.city.typed(&text),
                    PanelKind::Welcome(state) if id == "cidade" => {
                        state.city_text = text.clone();
                        state.city_place = None;
                        state.city.typed(&text);
                    }
                    PanelKind::Welcome(state) if id == "usuario" => state.user = text.trim().into(),
                    _ => {}
                }
                self.panels[i].win.dirty = true;
            }
            EditWhat::Key(vk) => self.edit_key(i, &id, &text, vk),
        }
    }

    /// Uma tecla num campo: as setas, Enter e Esc andam na lista de sugestões aberta; Enter
    /// confirma a pergunta (ou a cidade, o usuário, o passo das boas-vindas); Esc fecha a pergunta.
    fn edit_key(&mut self, i: usize, id: &str, text: &str, vk: u32) {
        let suggest = match &mut self.panels[i].kind {
            PanelKind::Prompt(state) => Some(&mut state.suggest),
            PanelKind::Settings(state) if id == "cidade" => Some(&mut state.city),
            PanelKind::Welcome(state) if id == "cidade" => Some(&mut state.city),
            _ => None,
        };
        if let Some(suggest) = suggest {
            match vk {
                0x26 | 0x28 => {
                    if suggest.arrow(vk == 0x28) {
                        self.panels[i].win.dirty = true;
                    }
                    return;
                }
                0x0D if suggest.index.is_some() => {
                    let pick = suggest.index.unwrap();
                    self.panel_action(i, Action::Pick(pick));
                    return;
                }
                0x1B if !suggest.results.is_empty() => {
                    suggest.clear();
                    self.panels[i].win.dirty = true;
                    return;
                }
                _ => {}
            }
        }
        match (&self.panels[i].kind, vk) {
            (PanelKind::Prompt(_), 0x0D) => self.panel_action(i, Action::Ok),
            (PanelKind::Prompt(_), 0x1B) => self.close_panel(i),
            (PanelKind::Welcome(_), 0x0D) => {
                if id == "usuario" {
                    if let PanelKind::Welcome(state) = &mut self.panels[i].kind {
                        state.user = text.trim().into();
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

    /// Faz a ação de uma área clicada no painel `i`.
    fn panel_action(&mut self, i: usize, action: Action) {
        self.panels[i].win.dirty = true;
        match action {
            Action::Go(page) => {
                if let PanelKind::Settings(state) = &mut self.panels[i].kind {
                    state.go(page);
                }
            }
            Action::Focus(id) => self.panels[i].win.focus_entry(&id, false),
            Action::Toggle(id) => self.toggle(i, id),
            Action::Notify(kind) => {
                let mut all = config::setting(&self.sh.cfg, "notificacoes");
                let on = config::notify_enabled(&self.sh.cfg, kind);
                all[kind] = json!(!on);
                self.sh.cfg.obj_mut("app").insert("notificacoes".into(), all);
                self.sh.save();
            }
            Action::Startup => {
                if !startup::set(!startup::enabled()) {
                    self.toast("Não deu para mudar a inicialização", None, "close", WARNING, None);
                }
            }
            Action::Lock => {
                let locked = self.sh.locked();
                self.sh.cfg.obj_mut("app").insert("travado".into(), json!(!locked));
                self.sh.save();
            }
            Action::Seg(id, value) => self.segment(id, &value),
            Action::Visible(key, on) => {
                if self.set_visible(key, on) && !on {
                    let text = format!("{} oculto", widgets::title(key));
                    self.toast(&text, None, "close", WHITE, Some(("Desfazer", Undo::Show(key))));
                }
            }
            Action::Widget(key, number) => self.widget_command(key, number),
            Action::Edit => self.start_edit(),
            Action::Arrange => self.arrange(),
            Action::SaveLayout => self.open_prompt(Purpose::LayoutName, None),
            Action::ApplyLayout(name) => self.apply_layout(&name),
            Action::DeleteLayout(name) => self.delete_layout(&name),
            Action::Restore(page) => self.restore_page(page),
            Action::MuteApp(app_id) => {
                widgets::outside::apps::toggle_muted(&mut self.sh.cfg, &app_id);
                self.sh.save();
            }
            Action::TestNotify => self.tray.notify("Tudo certo", "As notificações dos widgets estão funcionando."),
            Action::Welcome => self.open_welcome(),
            Action::OpenData => win::open_target(&config::data_dir().display().to_string()),
            Action::ResetAll => self.reset_all(),
            Action::Pick(index) => self.pick(i, index),
            Action::Ok => self.prompt_ok(i),
            Action::Cancel => self.close_panel(i),
            Action::Tile(_) | Action::Next | Action::Back | Action::Step(_) => self.welcome_step(i, action),
            Action::Finish => self.finish_welcome(i),
            Action::Slider(_) => {}
        }
    }

    /// Um interruptor das Configurações (ou das boas-vindas) mudou.
    fn toggle(&mut self, i: usize, id: &'static str) {
        match id {
            "texto_auto" => {
                let follow = config::setting_f64(&self.sh.cfg, "texto") > 0.0;
                let value = if follow { 0.0 } else { (self.sh.text_scale as f64 * 100.0).round() / 100.0 };
                self.set_setting("texto", json!(value));
            }
            "pomodoro_auto" => self.widget_or_cfg("pomodoro", |section| {
                let on = section.bool_of("auto").unwrap_or(false);
                section.as_object_mut().unwrap().insert("auto".into(), json!(!on));
            }),
            "capa_hd" => {
                let on = widgets::media::hd_covers(&self.sh.cfg);
                self.widget_or_cfg("midia", |section| set_flag(section, "capa_hd", !on));
            }
            "tempo_tremer" => {
                let on = widgets::tempo::shakes(&self.sh.cfg);
                self.widget_or_cfg("tempo", |section| set_flag(section, "tremer", !on));
            }
            "claude_nomes" => {
                let on = widgets::claude::show_names(&self.sh.cfg);
                self.widget_or_cfg("claude", |section| set_flag(section, "balao_nomes", !on));
            }
            settings::FOLLOW_SWITCH => {
                let on = self.sh.cfg["midia"].bool_of("ilha_segue_tela").unwrap_or(false);
                self.sh.cfg.obj_mut("midia").insert("ilha_segue_tela".into(), json!(!on));
                self.sh.save();
            }
            settings::TEXT_SWITCH => {
                use widgets::outside::apps;
                let on = apps::shows_text(&self.sh.cfg);
                self.sh.cfg.obj_mut(apps::KEY).insert(apps::TEXT_FIELD.into(), json!(!on));
                self.sh.save();
            }
            "welcome_startup" => {
                if let PanelKind::Welcome(state) = &mut self.panels[i].kind {
                    state.startup = !state.startup;
                }
            }
            id if id.starts_with("na_ilha:") => {
                // "Mostrar na ilha" de um widget (na seção dele)
                let key = &id["na_ilha:".len()..];
                if config::setting_bool(&self.sh.cfg, "ilha_widgets") {
                    let on = widgets::on_island(&self.sh.cfg, key);
                    self.sh.cfg.obj_mut(key).insert("na_ilha".into(), json!(!on));
                } else {
                    // com a ilha dos widgets desligada em Geral, ligar um liga as duas
                    self.sh.cfg.obj_mut(key).insert("na_ilha".into(), json!(true));
                    self.sh.cfg.obj_mut("app").insert("ilha_widgets".into(), json!(true));
                }
                self.sh.save();
                self.island_sync_at = Instant::now();
                if key == widgets::outside::REST {
                    self.rest_changed();
                }
                // ligando as notificações dos apps pela primeira vez: o Windows pergunta
                use crate::platform::notifications::{self, Access};
                if key == widgets::outside::apps::KEY && widgets::on_island(&self.sh.cfg, key) && notifications::access() == Access::Unasked {
                    notifications::request_access_async();
                }
            }
            "modo_foco" => {
                if self.focusing() {
                    self.end_focus();
                } else {
                    self.start_focus(0);
                }
            }
            id if id.starts_with("repouso:") => {
                // o que a ilha em repouso mostra (e o "só uma gota")
                widgets::outside::rest::toggle(&mut self.sh.cfg, id);
                self.sh.save();
                self.rest_changed();
            }
            name => {
                let on = config::setting_bool(&self.sh.cfg, name);
                self.set_setting(name, json!(!on));
                if name == "ilha_widgets" {
                    self.island_sync_at = Instant::now();
                }
            }
        }
    }

    /// Um botão das Configurações que manda uma ação a um widget (ou ao app, com "app"). Com o
    /// widget fechado, algumas valem direto nas opções.
    fn widget_command(&mut self, key: &'static str, number: u32) {
        match (key, number) {
            ("app", 1) => self.install_update(),
            ("app", 2) => self.check_updates(true),
            ("app", settings::ASK_NOTIFY_ACCESS) => crate::platform::notifications::request_access_async(),
            ("app", settings::OPEN_NOTIFY_PRIVACY) => win::open_target("ms-settings:privacy-notifications"),
            ("app", settings::OPEN_NOTIFY_SETTINGS) => win::open_target("ms-settings:notifications"),
            _ => {
                if let Some(j) = self.find(key) {
                    self.run_action(j, Act::Kind(number));
                } else if key == "claude" {
                    // o widget está oculto: liga ou desliga direto
                    let result = if number == widgets::claude::CONNECT { crate::data::claude_usage::connect() } else { crate::data::claude_usage::disconnect() };
                    if let Err(error) = result {
                        self.toast(&error, None, "close", WHITE, None);
                    }
                } else if key == "pomodoro" && number == widgets::pomodoro::RESET_DAY {
                    self.sh.cfg.obj_mut("pomodoro").remove("hoje");
                    self.sh.save();
                }
            }
        }
    }

    /// Os passos das boas-vindas: escolher um widget, seguir, voltar ou pular para um passo visto.
    fn welcome_step(&mut self, i: usize, action: Action) {
        let PanelKind::Welcome(state) = &mut self.panels[i].kind else { return };
        match action {
            Action::Tile(key) => {
                let on = state.chosen.get(key).copied().unwrap_or(false);
                state.chosen.insert(key, !on);
            }
            Action::Next => {
                if state.step == 0 && !state.chosen.values().any(|on| *on) {
                    self.toast("Escolha pelo menos um widget", None, "close", WARNING, None);
                    return;
                }
                state.step += 1;
            }
            Action::Back => {
                state.step = state.step.saturating_sub(1);
                state.city.clear();
            }
            Action::Step(step) => {
                state.step = step.min(state.step);
                state.city.clear();
            }
            _ => {}
        }
    }

    /// Mexe na seção de um widget: pelo widget aberto (que se redesenha) ou direto no config.
    fn widget_or_cfg(&mut self, key: &'static str, change: impl FnOnce(&mut Value)) {
        change(&mut self.sh.cfg[key]);
        self.sh.save();
        if let Some(j) = self.find(key) {
            self.cards[j].refresh(&self.sh);
        }
    }

    /// Um botão de uma escolha segmentada mudou.
    fn segment(&mut self, id: &'static str, value: &str) {
        match id {
            "animacoes" => self.set_setting("animacoes", json!(value)),
            "fim_expediente" => {
                self.sh.cfg.obj_mut("expediente").insert("fim".into(), json!(value));
                self.sh.save();
                self.island_sync_at = Instant::now();
            }
            "fonte" => {
                let index = ["auto", "spotify", "youtube"].iter().position(|source| *source == value).unwrap_or(0) as u32;
                match self.find("midia") {
                    Some(j) => self.run_action(j, Act::Kind(widgets::media::SOURCE + index)),
                    None => {
                        self.sh.cfg.obj_mut("midia").insert("fonte".into(), json!(value));
                        self.sh.save();
                    }
                }
            }
            "claude_natal" => {
                let value = value.to_string();
                self.widget_or_cfg("claude", |section| {
                    if !section.is_object() {
                        *section = json!({});
                    }
                    section.as_object_mut().unwrap().insert("natal".into(), json!(value));
                });
            }
            "longa_a_cada" => {
                let every: u32 = value.parse().unwrap_or(4);
                match self.find("pomodoro") {
                    Some(j) => self.run_action(j, Act::Kind(widgets::pomodoro::EVERY + every)),
                    None => {
                        self.sh.cfg.obj_mut("pomodoro").insert("longa_a_cada".into(), json!(every));
                        self.sh.save();
                    }
                }
            }
            id if id.starts_with("tamanho:") => {
                let key = widgets::KEYS.iter().copied().find(|key| id.ends_with(key)).unwrap_or("sistema");
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
                let index = ["foco", "pausa", "longa"].iter().position(|known| *known == phase).unwrap_or(0) as u32;
                match self.find("pomodoro") {
                    Some(j) => self.run_action(j, Act::Kind(widgets::pomodoro::DURATION + index * 1000 + minutes)),
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
            self.pending_setting = Some((Instant::now() + SLIDER_APPLY, name));
        }
    }

    /// Todos os painéis se redesenham no próximo quadro.
    fn panels_dirty(&mut self) {
        for panel in &mut self.panels {
            panel.win.dirty = true;
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

    /// Aplica a opção `name` ("*": todas) aos cartões e aos painéis.
    pub(super) fn apply_settings(&mut self, name: &str) {
        self.sh.read_preferences();
        if matches!(name, "desfoque" | "escurecimento" | "*") {
            let blur = config::setting_f64(&self.sh.cfg, "desfoque");
            self.sh.backdrop.check(blur);
        }
        let redraw = matches!(name, "texto" | "*" | "animacoes");
        for card in &mut self.cards {
            if redraw {
                card.refresh(&self.sh);
            } else {
                card.present(&self.sh, false);
            }
        }
        self.panels_changed();
    }

    /// "Restaurar o padrão" de uma página das Configurações (com Desfazer).
    fn restore_page(&mut self, page: &'static str) {
        let keys: Vec<&str> = match page {
            "geral" => settings::BEHAVIOR_KEYS.to_vec(),
            "aparencia" => settings::APPEARANCE_KEYS.to_vec(),
            _ => vec!["notificacoes"],
        };
        let old = self.sh.cfg["app"].clone();
        let changed = keys.iter().any(|key| old.get(*key).is_some());
        if !changed {
            self.toast("Já está tudo no padrão", None, "check", SUCCESS, None);
            return;
        }
        for key in keys {
            self.sh.cfg.obj_mut("app").remove(key);
        }
        self.sh.save();
        self.apply_settings("*");
        self.toast("Restaurado o padrão", None, "undo", SUCCESS, Some(("Desfazer", Undo::AppSettings(old))));
    }

    /// Todas as opções gerais de volta ao padrão (menos os layouts, as dicas vistas e a trava),
    /// com Desfazer.
    fn reset_all(&mut self) {
        let old = self.sh.cfg["app"].clone();
        let keep: serde_json::Map<String, Value> = old
            .as_object()
            .map(|app| app.iter().filter(|(name, _)| matches!(name.as_str(), "layouts" | "dicas_vistas" | "travado")).map(|(name, value)| (name.clone(), value.clone())).collect())
            .unwrap_or_default();
        self.sh.cfg["app"] = Value::Object(keep);
        self.sh.save();
        self.apply_settings("*");
        self.toast("Configurações restauradas", None, "undo", SUCCESS, Some(("Desfazer", Undo::AppSettings(old))));
    }

    // ---------- cidade e usuário ----------

    /// A cidade `index` da lista de sugestões foi escolhida.
    fn pick(&mut self, i: usize, index: usize) {
        match &mut self.panels[i].kind {
            PanelKind::Prompt(state) => {
                if let Some(place) = state.suggest.results.get(index).cloned() {
                    state.value = place.str_of("label").unwrap_or("").into();
                    state.picked = Some(place);
                    state.suggest.clear();
                    self.prompt_ok(i);
                }
            }
            PanelKind::Settings(state) => {
                if let Some(place) = state.city.results.get(index).cloned() {
                    state.city.clear();
                    let label = place.str_of("label").unwrap_or("").to_string();
                    self.panels[i].win.set_entry("cidade", &label);
                    self.apply_city(&label, Some(place));
                }
            }
            PanelKind::Welcome(state) => {
                if let Some(place) = state.city.results.get(index).cloned() {
                    state.city.clear();
                    let label = place.str_of("label").unwrap_or("").to_string();
                    state.city_text = label.clone();
                    state.city_place = Some(place);
                    self.panels[i].win.set_entry("cidade", &label);
                }
            }
        }
    }

    /// Muda a cidade do Tempo (com o lugar escolhido na lista, se houver).
    pub(super) fn apply_city(&mut self, text: &str, place: Option<Value>) {
        match self.find("tempo") {
            Some(j) => {
                self.with_kind(j, |kind, ctx| kind.input("city", text, place.as_ref(), ctx));
                self.cards[j].refresh(&self.sh);
            }
            None => {
                let section = self.sh.cfg.obj_mut("tempo");
                match &place {
                    Some(chosen) => {
                        section.insert("city".into(), chosen["label"].clone());
                        section.insert("lat".into(), chosen["lat"].clone());
                        section.insert("lon".into(), chosen["lon"].clone());
                        section.insert("place".into(), chosen["label"].clone());
                    }
                    None => {
                        section.insert("city".into(), json!(text));
                        for key in ["lat", "lon", "place"] {
                            section.insert(key.into(), Value::Null);
                        }
                    }
                }
                self.sh.save();
            }
        }
        let label = place.as_ref().and_then(|chosen| chosen.str_of("label")).unwrap_or(text).to_string();
        self.toast(&format!("Cidade: {label}"), None, "location", ACCENT, None);
    }

    /// Muda o usuário do GitHub (sem o "@").
    fn apply_github_user(&mut self, text: &str) {
        let user = text.trim().trim_start_matches('@').to_string();
        if user.is_empty() || self.sh.cfg["github"].str_of("usuario") == Some(user.as_str()) {
            return;
        }
        match self.find("github") {
            Some(j) => {
                self.with_kind(j, |kind, ctx| kind.input("user", &user, None, ctx));
                self.cards[j].refresh(&self.sh);
            }
            None => {
                self.sh.cfg.obj_mut("github").insert("usuario".into(), json!(user));
                self.sh.save();
            }
        }
        self.toast(&format!("GitHub: @{user}"), None, "check", SUCCESS, None);
    }

    /// "Salvar" na pergunta: aplica o que foi digitado (a cidade escolhida na lista vale só se o
    /// texto ainda é o dela).
    fn prompt_ok(&mut self, i: usize) {
        let text = self.panels[i].win.entry_text("valor").trim().to_string();
        let PanelKind::Prompt(state) = &self.panels[i].kind else { return };
        let purpose = state.purpose;
        let picked = state.picked.clone().filter(|place| place.str_of("label") == Some(text.as_str()));
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

    /// Guarda a posição de agora de cada widget com o nome `name`.
    fn save_layout(&mut self, name: &str) {
        let mut snapshot = serde_json::Map::new();
        for key in widgets::KEYS {
            let state = match self.find(key) {
                Some(j) => {
                    let card = &self.cards[j];
                    json!({"visible": true, "x": card.pos.0, "y": card.pos.1, "tamanho": if card.temp { "p" } else { card.mode.code() }})
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

    /// "Começar a usar": aplica o que foi escolhido nas boas-vindas.
    fn finish_welcome(&mut self, i: usize) {
        let PanelKind::Welcome(state) = &self.panels[i].kind else { return };
        let chosen = state.chosen.clone();
        let (city, place, startup_on) = (state.city_text.trim().to_string(), state.city_place.clone(), state.startup);
        let user = self.panels[i].win.entry_text("usuario");
        let user = if user.trim().is_empty() { state.user.clone() } else { user };
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

    /// Quando os painéis precisam de `panels_tick` de novo.
    pub(super) fn panels_due(&self) -> Option<Instant> {
        let mut next: Option<Instant> = self.pending_setting.map(|(when, _)| when);
        for panel in &self.panels {
            let mut frame = None;
            if panel.win.dirty || panel.win.anims.running {
                frame = Some(Instant::now() + Duration::from_millis(FRAME_MS));
            }
            let suggest = match &panel.kind {
                PanelKind::Settings(state) => state.city.next_due(),
                PanelKind::Prompt(state) => state.suggest.next_due(),
                PanelKind::Welcome(state) => state.city.next_due(),
            };
            if matches!(panel.kind, PanelKind::Prompt(_)) {
                frame = Some(frame.map_or(Instant::now() + PROMPT_FOCUS_CHECK, |due: Instant| due.min(Instant::now() + PROMPT_FOCUS_CHECK)));
            }
            for due in [frame, suggest].into_iter().flatten() {
                next = Some(next.map_or(due, |earliest: Instant| earliest.min(due)));
            }
        }
        next
    }

    /// A opção do slider esperando, a pergunta que perdeu o foco, as sugestões chegando e os
    /// painéis que precisam se redesenhar.
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
            if matches!(self.panels[i].kind, PanelKind::Prompt(_)) && self.panels[i].opened.elapsed() > PROMPT_GRACE {
                let foreground = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
                if foreground != self.panels[i].win.hwnd {
                    self.close_panel(i);
                    continue;
                }
            }
            let changed = match &mut self.panels[i].kind {
                PanelKind::Settings(state) => state.city.poll(),
                PanelKind::Prompt(state) => state.suggest.poll(),
                PanelKind::Welcome(state) => state.city.poll(),
            };
            if changed || self.panels[i].win.dirty || self.panels[i].win.anims.running {
                self.render_panel(i);
            }
            i += 1;
        }
    }
}

/// O que aconteceu num campo de texto.
pub enum EditWhat {
    Focus(bool),
    Change,
    Key(u32),
}

/// Liga ou desliga `name` na seção `section` de um widget (criando a seção, se ainda não há).
fn set_flag(section: &mut Value, name: &str, on: bool) {
    if !section.is_object() {
        *section = json!({});
    }
    section.as_object_mut().unwrap().insert(name.into(), json!(on));
}
