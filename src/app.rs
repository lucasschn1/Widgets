//! O aplicativo: cria os widgets e coordena o mouse (arrastar, encaixar, reordenar), os menus,
//! os modos (mostrar na frente), os atalhos, as notificações e as verificações periódicas.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;

use crate::card::{Act, Card, Press};
use crate::config::{self, Get};
use crate::events::{self, Cursor, Ev, MouseKind, CARD_CLASS};
use crate::overlay::{GuideLines, Guides, Tooltip};
use crate::popup::{GlassBar, GlassMenu, Part, Place};

#[path = "app_panels.rs"]
mod panels;
use panels::{EditWhat, Panel};
#[path = "app_screens.rs"]
mod screens;
#[path = "app_island.rs"]
pub(crate) mod island;
#[path = "app_perch.rs"]
mod perch;
use crate::prompt;
use crate::updates;
use crate::shared::Shared;
use crate::theme::*;
use crate::tray::{self, Tray, NIN_BALLOONUSERCLICK};
use crate::widgets::{self, Ctx, Kind, MenuItem, Mode, NotifyAction, Request};
use crate::win::{self, Rect, Z};

const BACKDROP_EVERY: Duration = Duration::from_secs(5);
const FULLSCREEN_EVERY: Duration = Duration::from_secs(2);
const POWER_EVERY: Duration = Duration::from_secs(30);
const ALERTS_EVERY: Duration = Duration::from_secs(10);
const WATCH_EVERY: Duration = Duration::from_millis(120);

// itens do menu (os do widget usam os próprios números, abaixo de 100 000)
const M_SIZE: u32 = 200_000; // + índice do tamanho
const M_PEEK: u32 = 300_001;
const M_ARRANGE: u32 = 300_002;
const M_LOCK: u32 = 300_003;
const M_CLOSE: u32 = 300_004;
const M_QUIT: u32 = 300_009;
const M_EDIT: u32 = 300_010;
const M_FOCUS_END: u32 = 300_011;
const M_FOCUS: u32 = 300_020; // + minutos (0 = até desligar)
const M_WIDGET: u32 = 300_100; // + índice em widgets::KEYS
const M_LAYOUT: u32 = 301_000; // + índice do layout
const M_LAYOUT_DEL: u32 = 302_000; // + índice do layout
const M_SETTINGS: u32 = 300_030;
const M_TIPS: u32 = 300_031;
const M_GALLERY: u32 = 300_032;
const M_SAVE_LAYOUT: u32 = 300_033;

// botões das pílulas
const B_UNDO: u32 = 1;
const B_EDIT_ADD: u32 = 2;
const B_EDIT_DONE: u32 = 3;

const TOAST_SECONDS: f32 = 3.5;
const TOAST_ACTION_SECONDS: f32 = 6.0;
const SUCCESS: Rgba = Rgba::rgb(48, 209, 88);
const WARNING: Rgba = Rgba::rgb(255, 184, 0);

/// O que o botão "Desfazer" de um aviso desfaz.
enum Undo {
    /// reabre um widget fechado no mesmo lugar, devolvendo os vizinhos que subiram
    Reopen { key: &'static str, pos: (i32, i32), mode: Mode, followers: Vec<(&'static str, (i32, i32))> },
    /// volta um layout excluído
    Layout { name: String, snapshot: Value },
    /// mostra de novo um widget ocultado na galeria
    Show(&'static str),
    /// volta as opções gerais como estavam (ex.: depois de "Restaurar padrão")
    AppSettings(Value),
    /// instala a versão nova (botão "Atualizar" do aviso de atualização)
    Update,
}

/// primeira busca por atualização logo depois de abrir, e depois a cada 6 horas
const UPDATE_FIRST: Duration = Duration::from_secs(20);
const UPDATE_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

pub struct App {
    sh: Shared,
    cards: Vec<Card>,
    tray: Tray,
    tooltip: Tooltip,
    guides: Guides,
    /// enquanto arrasta: para onde cada outro widget vai se o arrastado sair do lugar
    rest: HashMap<usize, (i32, i32)>,
    origin: Rect,
    preview: Option<(i32, i32)>,
    move_dirty: Option<usize>,
    notification_action: Option<(NotifyAction, Option<&'static str>)>,
    notified: HashMap<&'static str, f64>,
    high_since: HashMap<&'static str, f64>,
    alert_cpu: (u64, u64),
    next_backdrop: Instant,
    next_fullscreen: Instant,
    next_power: Instant,
    next_alerts: Instant,
    next_watch: Instant,
    peek_seen: Instant,
    menu: GlassMenu,
    toast: GlassBar,
    toast_until: Option<Instant>,
    toast_undo: Option<Undo>,
    edit_bar: GlassBar,
    /// modo foco: até quando (None dentro = até desligar)
    focus_until: Option<Option<f64>>,
    /// deslizes marcados para depois (ex.: depois de mudar de tamanho ao aplicar um layout)
    later: Vec<(Instant, &'static str, (i32, i32))>,
    /// janelas de configurações, perguntas e boas-vindas
    panels: Vec<Panel>,
    update_status: String,
    update_version: Option<String>,
    next_update: Instant,
    /// busca em andamento: a resposta e se foi pedida pelo botão
    update_rx: Option<(std::sync::mpsc::Receiver<updates::Found>, bool)>,
    /// o install.ps1 rodando (se ele terminar e o app continuar aberto, a atualização não foi)
    installer: Option<std::process::Child>,
    /// opção mexida por um slider, para aplicar no vidro daqui a pouco (sem pesar a cada pixel)
    pending_setting: Option<(Instant, &'static str)>,
    /// abrir as boas-vindas (primeira vez)
    welcome_at: Option<Instant>,
    /// monitores da combinação em que as posições guardadas valem, e o nome dela
    screens: Vec<win::Monitor>,
    screens_sig: String,
    /// combinação nova esperando ficar parada (nome, desde quando)
    screens_pending: Option<(String, Instant)>,
    next_screens: Instant,
    /// ao abrir com outros monitores: os widgets nascem na posição guardada, mesmo fora da tela
    keep_saved_pos: bool,
    /// o Tocando agora como ilha flutuante (o cartão fica escondido no lugar dele)
    island: Option<island::Island>,
    /// o Clawd na barra de tarefas
    perch: Option<perch::Perch>,
    pub quit: bool,
}

/// Acorda o laço principal (para as threads que buscam dados).
pub fn wake() {
    events::wake();
}

impl App {
    pub fn new() -> App {
        events::register_classes();
        let msg_hwnd = events::create_message_window();
        let cfg = config::load_config(widgets::KEYS);
        let cache = config::load_cache();
        let primary = win::work_areas()[0];
        let s = win::scale_at(primary.0, primary.1);
        let sh = Shared::new(cfg, cache, s);
        let tray = Tray::new(msg_hwnd);
        let mut app = App {
            sh,
            cards: Vec::new(),
            tray,
            tooltip: Tooltip::new(),
            guides: Guides::new(),
            rest: HashMap::new(),
            origin: (0, 0, 0, 0),
            preview: None,
            move_dirty: None,
            notification_action: None,
            notified: HashMap::new(),
            high_since: HashMap::new(),
            alert_cpu: win::cpu_times(),
            next_backdrop: Instant::now() + BACKDROP_EVERY,
            next_fullscreen: Instant::now() + FULLSCREEN_EVERY,
            next_power: Instant::now(),
            next_alerts: Instant::now() + ALERTS_EVERY,
            next_watch: Instant::now(),
            peek_seen: Instant::now(),
            menu: GlassMenu::new(),
            toast: GlassBar::new(Place::Bottom),
            toast_until: None,
            toast_undo: None,
            edit_bar: GlassBar::new(Place::Top),
            focus_until: None,
            later: Vec::new(),
            panels: Vec::new(),
            update_status: "Ainda não procurou".into(),
            update_version: None,
            next_update: Instant::now() + UPDATE_FIRST,
            update_rx: None,
            installer: None,
            pending_setting: None,
            welcome_at: None,
            screens: Vec::new(),
            screens_sig: String::new(),
            screens_pending: None,
            next_screens: Instant::now() + screens::SCREENS_EVERY,
            keep_saved_pos: false,
            island: None,
            perch: None,
            quit: false,
        };
        let screens_change = app.begin_screens_startup();
        // na ordem da arrumação da primeira vez (para quem ainda não tem posição guardada)
        let order = widgets::FIRST_LAYOUT.iter().flat_map(|col| col.iter().copied()).chain(widgets::KEYS.iter().copied());
        let mut visible: Vec<&str> = Vec::new();
        for key in order.filter(|k| widgets::KEYS.contains(k) && app.sh.cfg[*k].bool_of("visible").unwrap_or(true)) {
            if !visible.contains(&key) {
                visible.push(key);
            }
        }
        for key in visible {
            app.create_card(key, true);
        }
        app.place_split_system();
        app.grow_old_small();
        app.clear_small_overlaps();
        app.fit_new_heights();
        if app.cards.is_empty() && !app.tray.ok {
            app.create_card("tempo", true); // sem bandeja, não pode ficar sem nada na tela
        }
        app.finish_screens_startup(screens_change);
        // estava como ilha quando o app fechou: continua assim
        if app.sh.cfg["midia"].bool_of("ilha").unwrap_or(false) && app.find("midia").is_some() {
            app.open_island(false);
        }
        // e o Clawd, se estava na barra de tarefas
        if app.sh.cfg["claude"].bool_of("na_barra").unwrap_or(false) && app.find("claude").is_some() {
            app.open_perch(false);
        }
        app.sh.save();
        if !app.sh.cfg["app"].bool_of("dicas_vistas").unwrap_or(false) {
            app.welcome_at = Some(Instant::now() + Duration::from_millis(1500));
        }
        app
    }

    // ---------- widgets ----------

    fn index(&self, hwnd: isize) -> Option<usize> {
        self.cards.iter().position(|c| c.hwnd.0 as isize == hwnd)
    }

    fn find(&self, key: &str) -> Option<usize> {
        self.cards.iter().position(|c| c.key() == key)
    }

    /// Chama o widget `i` com acesso à configuração e trata o que ele pedir.
    fn with_kind<R>(&mut self, i: usize, f: impl FnOnce(&mut dyn Kind, &mut Ctx) -> R) -> R {
        let mut out = Vec::new();
        let key = self.cards[i].key();
        let r = {
            let mut ctx = Ctx { cfg: &mut self.sh.cfg, cache: &mut self.sh.cache, key, out: &mut out };
            f(self.cards[i].kind.as_mut(), &mut ctx)
        };
        self.requests(out, Some(i));
        r
    }

    fn requests(&mut self, out: Vec<Request>, card: Option<usize>) {
        for r in out {
            match r {
                Request::SaveConfig => self.sh.save(),
                Request::SaveCache => self.sh.save_cache(),
                Request::Open(target) => win::open_target(&target),
                Request::Redraw => {
                    if let Some(i) = card {
                        self.cards[i].refresh(&self.sh);
                    }
                }
                Request::Island => self.open_island(true),
                Request::Perch(true) => self.open_perch(true),
                Request::Perch(false) => self.close_perch(true),
                Request::Ask(what) => {
                    let anchor = card.map(|i| self.cards[i].card_rect());
                    let purpose = if what == "city" { prompt::Purpose::City } else { prompt::Purpose::GitHubUser };
                    self.open_prompt(purpose, anchor);
                }
                Request::Notify(kind, title, text, action) => {
                    let key = card.map(|i| self.cards[i].key());
                    self.notify(kind, &title, &text, 0.0, action.map(|a| (a, key)));
                }
            }
        }
    }

    fn create_card(&mut self, key: &str, startup: bool) -> Option<usize> {
        let kind = widgets::create(key, &self.sh.cfg, &self.sh.cache)?;
        let wcfg = self.sh.cfg[key].clone();
        let mode = wcfg
            .str_of("tamanho")
            .and_then(Mode::from_code)
            .or_else(|| wcfg.bool_of("compact").map(|c| if c { Mode::P } else { Mode::G }))
            .filter(|m| kind.sizes().contains(m))
            .unwrap_or(kind.default_mode());
        let hwnd = events::create_layered(CARD_CLASS, false);
        let saved = match (wcfg.i64_of("x"), wcfg.i64_of("y")) {
            (Some(x), Some(y)) => Some((x as i32, y as i32)),
            _ => None,
        };
        let anchor = saved.unwrap_or_else(|| {
            let a = win::work_areas()[0];
            (a.2 - 200, a.1 + 100)
        });
        let s = win::scale_at(anchor.0 + 150, anchor.1 + 100);
        let mut card = Card::new(kind, hwnd, mode, s);
        card.pos = match saved {
            Some(p) if self.keep_saved_pos || card.visible_on_screen(p) => p,
            _ => self.default_pos(&card),
        };
        // está num monitor com outra escala?
        let (x0, y0, x1, y1) = card.card_rect();
        let scale = win::scale_at((x0 + x1) / 2, (y0 + y1) / 2);
        if (scale - card.s).abs() > 0.01 {
            card.set_scale(scale);
        }
        card.owner = win::pin_to_desktop(hwnd);
        self.cards.push(card);
        let i = self.cards.len() - 1;
        self.with_kind(i, |k, ctx| k.start(ctx));
        self.cards[i].refresh(&self.sh);
        self.cards[i].present(&self.sh, true);
        events::show(hwnd, !self.sh.focus);
        self.send_to_back(i);
        if !startup && (self.sh.peeking || self.sh.editing) {
            win::set_z(hwnd, Z::Topmost);
        }
        Some(i)
    }

    /// Depois de separar o "Clima e Sistema": o Sistema entra logo abaixo do Tempo, e os widgets
    /// que estavam embaixo descem para abrir espaço.
    fn place_split_system(&mut self) {
        if !self.sh.cfg["sistema"].bool_of("sistema_abaixo").unwrap_or(false) {
            return;
        }
        self.sh.cfg.obj_mut("sistema").remove("sistema_abaixo");
        let (Some(t), Some(s)) = (self.find("tempo"), self.find("sistema")) else { return };
        let below: Vec<usize> = self.stack_below(t, None).into_iter().filter(|&j| j != s).collect();
        let pos = self.cards[s].below(&self.cards[t]);
        let shift = self.cards[s].card_size().1 + (WIDGET_GAP * self.cards[s].s).round() as i32;
        self.cards[s].pos = pos;
        self.cards[s].present(&self.sh, true);
        for j in below.into_iter().chain([s]) {
            if j != s {
                self.cards[j].pos.1 += shift;
                self.cards[j].present(&self.sh, true);
            }
            let (key, (x, y)) = (self.cards[j].key(), self.cards[j].pos);
            let w = self.sh.cfg.obj_mut(key);
            w.insert("x".into(), json!(x));
            w.insert("y".into(), json!(y));
        }
    }

    /// Versão 3.2: o tamanho pequeno virou um quadrado, mais alto que a pílula de antes. Os
    /// widgets que estavam encaixados embaixo de um pequeno descem para não ficar por baixo dele.
    fn grow_old_small(&mut self) {
        if self.sh.cfg["app"].bool_of("pequeno_quadrado").unwrap_or(false) {
            return;
        }
        self.sh.cfg.obj_mut("app").insert("pequeno_quadrado".into(), json!(true));
        let small: Vec<usize> = (0..self.cards.len()).filter(|&i| self.cards[i].mode == Mode::P).collect();
        for i in small {
            let s = self.cards[i].s;
            let old = ((320.0 * s).round() as i32, (widgets::OLD_SMALL_H * s).round() as i32);
            let rect = self.cards[i].card_rect_at(self.cards[i].pos, Some(old));
            let delta = self.cards[i].card_size().1 - old.1;
            for j in self.stack_below(i, Some(rect)) {
                if j == i {
                    continue;
                }
                self.cards[j].pos.1 += delta;
                self.cards[j].present(&self.sh, true);
                let (key, (x, y)) = (self.cards[j].key(), self.cards[j].pos);
                let w = self.sh.cfg.obj_mut(key);
                w.insert("x".into(), json!(x));
                w.insert("y".into(), json!(y));
            }
        }
    }

    /// Depois do pequeno virar quadrado: se um widget que estava solto embaixo de um pequeno
    /// (sem encaixar) ficou por baixo dele, desce o suficiente, levando junto os encaixados nele.
    /// Um widget mudou de altura numa versão nova (ex.: o Tocando agora grande ficou com a capa
    /// na largura toda): os vizinhos saem da frente uma vez, como ao mudar de tamanho. A altura de
    /// cada um fica guardada para a próxima comparação.
    fn fit_new_heights(&mut self) {
        // alturas antes da versão 3.6 (quando ainda não eram guardadas)
        let legacy = |key: &str, mode: Mode| -> Option<f32> {
            match (key, mode) {
                ("midia", Mode::M) => Some(102.0),
                ("midia", Mode::G) => Some(278.0),
                ("github", Mode::M) => Some(233.0),
                ("github", Mode::G) => Some(340.0),
                ("sistema", Mode::G) => Some(424.0),
                _ => None,
            }
        };
        // alturas da versão 3.6, a última em que cada widget tinha as próprias medidas; a 3.6 só
        // guardava a altura ao abrir, então quem mudou de tamanho depois ficou com a de outro
        let v36 = |key: &str, mode: Mode| -> Option<f32> {
            match (key, mode) {
                (_, Mode::P) => Some(widgets::SMALL),
                ("midia", Mode::M) => Some(110.0),
                ("github", Mode::M) => Some(163.0),
                (_, Mode::M) => Some(150.0),
                ("tempo", Mode::G) => Some(348.0),
                ("github", Mode::G) => Some(377.0),
                ("pomodoro", Mode::G) => Some(340.0),
                (_, Mode::G) => Some(448.0),
            }
        };
        // a altura de antes de cada um (None: não mudou, ou não havia)
        let olds: Vec<Option<f32>> = (0..self.cards.len())
            .map(|i| {
                let (key, mode) = (self.cards[i].key(), self.cards[i].mode);
                let w = &self.sh.cfg[key];
                let old_h = match (w.f64_of("altura"), w.str_of("altura_tamanho")) {
                    // nunca esteve na tela (instalação nova ou widget aberto agora): não havia altura antes
                    _ if self.cards[i].temp || w.get("x").is_none() => None,
                    (Some(h), Some(m)) if m == mode.code() => Some(h as f32),
                    (Some(_), Some(_)) => v36(key, mode),
                    // antes da 3.6, os que não estão na lista já tinham a altura da 3.6
                    _ => legacy(key, mode).or(v36(key, mode)),
                };
                old_h.filter(|h| (h - self.cards[i].layout(mode).h).abs() > 0.5)
            })
            .collect();
        // onde cada um estava, com a altura de antes: quem está encaixado embaixo de quem se mede
        // assim, até cada um ser acertado
        let mut was: Vec<Rect> = self
            .cards
            .iter()
            .zip(&olds)
            .map(|(c, h)| c.card_rect_at(c.pos, h.map(|h| (c.card_size().0, (h * c.s).round() as i32))))
            .collect();
        // de cima para baixo: quem está em cima abre espaço primeiro e leva junto os de baixo
        let mut order: Vec<usize> = (0..self.cards.len()).collect();
        order.sort_by_key(|&i| (self.cards[i].pos.1, self.cards[i].pos.0));
        for i in order {
            if olds[i].is_some() {
                let (old, new) = (was[i], self.cards[i].card_rect());
                let limit = ((WIDGET_GAP + 8.0) * self.cards[i].s) as i32;
                let start = stack_in(&was, i, old, limit).into_iter().map(|j| (j, (self.cards[j].pos.0, self.cards[j].pos.1 + new.3 - old.3))).collect();
                for (j, to) in self.make_room(i, old, new, start) {
                    let (dx, dy) = (to.0 - self.cards[j].pos.0, to.1 - self.cards[j].pos.1);
                    was[j] = (was[j].0 + dx, was[j].1 + dy, was[j].2 + dx, was[j].3 + dy);
                    self.cards[j].pos = to;
                    self.cards[j].present(&self.sh, true);
                    let k = self.cards[j].key();
                    let wj = self.sh.cfg.obj_mut(k);
                    wj.insert("x".into(), json!(to.0));
                    wj.insert("y".into(), json!(to.1));
                }
                was[i] = new;
            }
            if !self.cards[i].temp {
                let (key, mode) = (self.cards[i].key(), self.cards[i].mode);
                widgets::store_size(self.sh.cfg.obj_mut(key), mode);
            }
        }
    }

    fn clear_small_overlaps(&mut self) {
        if self.sh.cfg["app"].bool_of("pequeno_quadrado_v2").unwrap_or(false) {
            return;
        }
        self.sh.cfg.obj_mut("app").insert("pequeno_quadrado_v2".into(), json!(true));
        let mut small: Vec<usize> = (0..self.cards.len()).filter(|&i| self.cards[i].mode == Mode::P).collect();
        small.sort_by_key(|&i| self.cards[i].pos.1);
        for i in small {
            let r = self.cards[i].card_rect();
            let gap = (WIDGET_GAP * self.cards[i].s).round() as i32;
            let hit: Vec<usize> = (0..self.cards.len())
                .filter(|&j| j != i)
                .filter(|&j| {
                    let o = self.cards[j].card_rect();
                    let (w, h) = win::overlap(r, o);
                    w > 0 && h > 0 && o.1 > r.1
                })
                .collect();
            for j in hit {
                let o = self.cards[j].card_rect();
                let shift = r.3 + gap - o.1;
                if shift <= 0 {
                    continue;
                }
                let mut moving = vec![j];
                moving.extend(self.stack_below(j, None).into_iter().filter(|&k| k != i && k != j));
                for k in moving {
                    self.cards[k].pos.1 += shift;
                    self.cards[k].present(&self.sh, true);
                    let (key, (x, y)) = (self.cards[k].key(), self.cards[k].pos);
                    let w = self.sh.cfg.obj_mut(key);
                    w.insert("x".into(), json!(x));
                    w.insert("y".into(), json!(y));
                }
            }
        }
    }

    /// Onde um widget novo aparece: embaixo do mais baixo da coluna dele na arrumação da primeira
    /// vez (widgets::FIRST_LAYOUT), ou numa coluna nova à esquerda.
    fn default_pos(&self, card: &Card) -> (i32, i32) {
        let s = card.s;
        let (w, h) = card.size();
        let m = card.margin;
        if self.cards.is_empty() {
            let (_, top, right, _) = win::work_areas()[0];
            let edge = (EDGE_GAP * s).round() as i32;
            return (right - edge - (w - 2 * m) - m, top + edge - m);
        }
        let col = widgets::first_column(card.key());
        let bottom_of = |c: &&Card| c.card_rect().3;
        let lowest = match self.cards.iter().filter(|c| widgets::first_column(c.key()) == col).max_by_key(bottom_of) {
            Some(c) => c,
            // o primeiro da coluna dele: começa uma coluna nova
            None if col > 0 => return self.new_column_pos(card),
            None => self.cards.iter().max_by_key(bottom_of).unwrap(),
        };
        let (x, y) = card.below(lowest);
        let (_, _, _, bottom) = win::area_for(lowest.card_rect());
        if y + h - m <= bottom - (EDGE_GAP * s) as i32 {
            return (x, y);
        }
        self.new_column_pos(card)
    }

    /// No alto de uma coluna nova, à esquerda de todos os widgets.
    fn new_column_pos(&self, card: &Card) -> (i32, i32) {
        let (s, (w, _), m) = (card.s, card.size(), card.margin);
        let leftmost = self.cards.iter().min_by_key(|c| c.card_rect().0).unwrap();
        let (_, top, _, _) = win::area_for(leftmost.card_rect());
        (leftmost.card_rect().0 - (WIDGET_GAP * s).round() as i32 - (w - 2 * m) - m, top + (EDGE_GAP * s).round() as i32 - m)
    }

    fn send_to_back(&self, i: usize) {
        if !self.sh.peeking && !self.sh.editing {
            win::set_z(self.cards[i].hwnd, Z::Bottom);
        }
    }

    /// Widgets empilhados embaixo do retângulo (encaixados um no outro), em cadeia.
    fn stack_below(&self, i: usize, rect: Option<Rect>) -> Vec<usize> {
        let limit = ((WIDGET_GAP + 8.0) * self.cards[i].s) as i32;
        let rects: Vec<Rect> = self.cards.iter().map(|c| c.card_rect()).collect();
        stack_in(&rects, i, rect.unwrap_or(rects[i]), limit)
    }

    /// Depois de mudar de lugar: guarda a posição e confere a escala do monitor.
    fn moved(&mut self, i: usize) {
        let (x, y) = self.cards[i].pos;
        let key = self.cards[i].key();
        let w = self.sh.cfg.obj_mut(key);
        w.insert("x".into(), json!(x));
        w.insert("y".into(), json!(y));
        self.sh.save();
        self.send_to_back(i);
        let (x0, y0, x1, y1) = self.cards[i].card_rect();
        let scale = win::scale_at((x0 + x1) / 2, (y0 + y1) / 2);
        if (scale - self.cards[i].s).abs() > 0.01 {
            self.cards[i].set_scale(scale);
            self.cards[i].refresh(&self.sh);
            self.cards[i].present(&self.sh, true);
        }
    }

    pub fn set_visible(&mut self, key: &str, on: bool) -> bool {
        self.set_visible_with(key, on, true, true)
    }

    /// `reflow`: os vizinhos de baixo sobem ao fechar; `place`: ao abrir, sai de cima dos outros.
    fn set_visible_with(&mut self, key: &str, on: bool, reflow: bool, place: bool) -> bool {
        self.panels_changed();
        if !on && !self.tray.ok && self.cards.len() == 1 && self.find(key).is_some() {
            return false; // sem a bandeja, fechar o último widget encerraria o programa
        }
        self.sh.cfg.obj_mut(key).insert("visible".into(), json!(on));
        self.sh.save();
        if on && self.find(key).is_none() {
            if let Some(i) = self.create_card(key, false) {
                // se a posição guardada ficou ocupada, vai para baixo do último widget
                let rect = self.cards[i].card_rect();
                let busy = self.cards.iter().enumerate().any(|(j, o)| {
                    let (w, h) = win::overlap(rect, o.card_rect());
                    j != i && w.min(h) > 0
                });
                if self.sh.editing || self.sh.peeking {
                    win::set_z(self.cards[i].hwnd, Z::Topmost);
                }
                if busy && place {
                    let lowest = (0..self.cards.len()).filter(|&j| j != i).max_by_key(|&j| self.cards[j].card_rect().3).unwrap();
                    let to = self.cards[i].below(&self.cards[lowest]);
                    self.cards[i].slide_to(to, ANIM_MS, &self.sh);
                }
            }
        } else if !on {
            if let Some(i) = self.find(key) {
                // os widgets encaixados embaixo sobem para ocupar o lugar
                let followers = if reflow { self.stack_below(i, None) } else { Vec::new() };
                let shift = self.cards[i].card_size().1 + (WIDGET_GAP * self.cards[i].s).round() as i32;
                if key == "midia" {
                    self.drop_island();
                    self.sh.cfg.obj_mut("midia").insert("ilha".into(), json!(false));
                }
                if key == "claude" {
                    self.drop_perch();
                    self.sh.cfg.obj_mut("claude").insert("na_barra".into(), json!(false));
                }
                let card = self.cards.remove(i);
                self.tooltip.hide();
                events::destroy(card.hwnd);
                for j in followers {
                    let j = if j > i { j - 1 } else { j };
                    let to = (self.cards[j].pos.0, self.cards[j].pos.1 - shift);
                    self.cards[j].slide_to(to, ANIM_MS, &self.sh);
                }
            }
        }
        true
    }

    // ---------- tamanho ----------

    fn set_mode(&mut self, i: usize, mode: Mode) {
        if self.cards[i].temp && mode == self.cards[i].mode {
            self.pin_open(i);
            return;
        }
        if self.cards[i].temp {
            self.cards[i].temp = false;
            self.cards[i].mode = Mode::P; // o tamanho "de verdade" era o pequeno
            if let Some(home) = self.cards[i].temp_home.take() {
                self.cards[i].pos = home;
            }
        }
        self.resize_to(i, mode, true, false);
    }

    /// `reflow`: os vizinhos saem da frente; `temporary`: aberto só pelo mouse (fica por cima
    /// dos vizinhos e, ao fechar, volta para onde estava).
    fn resize_to(&mut self, i: usize, mode: Mode, reflow: bool, temporary: bool) {
        if self.cards[i].animating() || mode == self.cards[i].mode {
            return;
        }
        let followers = if reflow { self.stack_below(i, None) } else { Vec::new() };
        let old = self.cards[i].card_rect();
        let home = if temporary && mode == Mode::P { self.cards[i].temp_home.take() } else { None };
        let to = home.unwrap_or_else(|| self.cards[i].target_for(old, self.cards[i].card_size_of(mode)));
        self.cards[i].resize_to(mode, &self.sh, !temporary, to);
        events::set_cursor(self.cards[i].hwnd, Cursor::Arrow);
        self.tooltip.hide();
        if !reflow {
            return;
        }
        // os empilhados embaixo acompanham a borda de baixo, como no iPhone; e quem ficaria por
        // baixo ou por cima do widget sai da frente
        let new = self.cards[i].card_rect_at(to, None);
        let dy = new.3 - old.3;
        let start = followers.into_iter().map(|j| (j, (self.cards[j].pos.0, self.cards[j].pos.1 + dy))).collect();
        let ms = self.cards[i].resize_ms();
        for (j, to) in self.make_room(i, old, new, start) {
            self.cards[j].slide_to(to, ms, &self.sh);
        }
    }

    /// Depois de um widget mudar de tamanho (de `old` para `new`): os vizinhos saem da frente,
    /// para ninguém ficar por cima ou por baixo de outro nem cortado pela borda da tela.
    /// `start`: para onde alguns já iam (os encaixados embaixo). Devolve quem muda de lugar.
    fn make_room(&self, i: usize, old: Rect, new: Rect, start: HashMap<usize, (i32, i32)>) -> Vec<(usize, (i32, i32))> {
        let now: Vec<(i32, i32)> = self.cards.iter().map(|c| c.slide.as_ref().map(|s| s.to).unwrap_or(c.pos)).collect();
        let rects: Vec<Rect> = self.cards.iter().zip(&now).map(|(c, p)| c.card_rect_at(*p, None)).collect();
        let below: Vec<Vec<usize>> = (0..self.cards.len()).map(|j| self.stack_below(j, None)).collect();
        let start_rects = start.iter().map(|(&j, &p)| (j, self.cards[j].card_rect_at(p, None))).collect();
        let s = self.cards[i].s;
        screens::make_room(i, old, new, &rects, &start_rects, &below, s, win::area_for)
            .into_iter()
            .map(|(j, r)| {
                let m = self.cards[j].margin;
                (j, (r.0 - m, r.1 - m))
            })
            .collect()
    }

    fn badge_action(&mut self, i: usize) {
        if self.cards[i].temp {
            self.pin_open(i);
        } else {
            let next = self.cards[i].next_mode();
            self.resize_to(i, next, true, false);
        }
    }

    /// Mantém aberto o widget que só estava aberto por causa do mouse.
    fn pin_open(&mut self, i: usize) {
        if !self.cards[i].temp || self.cards[i].animating() {
            return;
        }
        self.cards[i].temp = false;
        let home = self.cards[i].temp_home.take().unwrap_or(self.cards[i].pos);
        let small = self.cards[i].card_rect_at(home, Some(self.cards[i].card_size_of(Mode::P)));
        let big = self.cards[i].card_rect();
        let start = self.stack_below(i, Some(small)).into_iter().map(|j| (j, (self.cards[j].pos.0, self.cards[j].pos.1 + big.3 - small.3))).collect();
        for (j, to) in self.make_room(i, small, big, start) {
            self.cards[j].slide_to(to, ANIM_MS, &self.sh);
        }
        let (mode, pos) = (self.cards[i].mode, self.cards[i].pos);
        let key = self.cards[i].key();
        let w = self.sh.cfg.obj_mut(key);
        widgets::store_size(w, mode);
        w.insert("x".into(), json!(pos.0));
        w.insert("y".into(), json!(pos.1));
        w.remove("compact");
        self.sh.save();
        self.cards[i].present(&self.sh, false);
    }

    fn hover_expand(&mut self, i: usize) {
        self.cards[i].expand_at = None;
        let c = &self.cards[i];
        if c.mode != Mode::P || c.press.is_some() || c.animating() || !c.pointer_inside() {
            return;
        }
        for j in 0..self.cards.len() {
            if j != i {
                self.send_to_back(j); // este fica por cima dos vizinhos enquanto aberto
            }
        }
        win::set_z(self.cards[i].hwnd, Z::Topmost);
        win::set_z(self.cards[i].hwnd, Z::NoTopmost);
        self.cards[i].temp = true;
        self.cards[i].temp_home = Some(self.cards[i].pos);
        let big = *self.cards[i].kind.sizes().last().unwrap();
        self.resize_to(i, big, false, true);
        self.cards[i].outside_since = None;
    }

    fn watch_pointer(&mut self, i: usize) {
        let c = &self.cards[i];
        if !c.temp {
            return;
        }
        if c.pointer_inside() || c.press.is_some() || c.animating() {
            self.cards[i].outside_since = None;
        } else if c.outside_since.is_none() {
            self.cards[i].outside_since = Some(Instant::now());
        } else if c.outside_since.unwrap().elapsed() >= Duration::from_millis(HOVER_COLLAPSE_MS) {
            self.resize_to(i, Mode::P, false, true);
            self.cards[i].temp = false;
        }
    }

    // ---------- mouse ----------

    fn screen_xy(&self, i: usize, x: i32, y: i32) -> (i32, i32) {
        (self.cards[i].pos.0 + x, self.cards[i].pos.1 + y)
    }

    fn on_motion(&mut self, i: usize, x: i32, y: i32) {
        if self.cards[i].hover_target == 0.0 {
            self.cards[i].set_hover(true);
            // (o título comprido do Tocando agora dá uma volta, por exemplo)
            if self.cards[i].kind.pointer_entered() && !self.sh.saving {
                self.cards[i].fps = self.cards[i].kind.overlay_fps();
            }
        }
        if self.cards[i].drag.is_some() {
            return;
        }
        // a borda de vidro reflete a luz do mouse
        let (gx, gy) = self.cards[i].logical(x, y);
        self.cards[i].move_glow(gx, gy);
        if !self.cards[i].animating() && !self.sh.editing {
            let (lx, ly) = self.cards[i].logical(x, y);
            let mode = self.cards[i].mode;
            if self.cards[i].kind.pointer_moved(lx, ly, mode) {
                self.cards[i].refresh(&self.sh);
            }
        }
        let sh = &self.sh;
        let card = &self.cards[i];
        let close_hot = card.on_close(x, y, sh);
        let on_badge = card.on_badge(x, y, sh) || close_hot;
        let action = if on_badge { None } else { card.action_at(x, y, sh) };
        let cursor = if on_badge || action.is_some() {
            Cursor::Hand
        } else if sh.editing {
            Cursor::Move
        } else {
            Cursor::Arrow
        };
        let hot = if action.is_some() && !card.animating() { card.hot_at(x, y, sh) } else { None };
        let tip = if !card.animating() && !on_badge && config::setting_bool(&sh.cfg, "dicas") { card.tooltip_at(x, y, sh) } else { None };
        events::set_cursor(card.hwnd, cursor);
        let changed = hot.map(|h| h.0) != card.hot.map(|h| h.0) || close_hot != card.close_hot;
        let animations = sh.animations;
        let card = &mut self.cards[i];
        card.close_hot = close_hot;
        card.set_hot(hot, animations);
        if changed {
            card.present(&self.sh, false);
        }
        let (sx, sy) = self.screen_xy(i, x, y);
        self.tooltip.request(tip, sx, sy);
    }

    fn on_leave(&mut self, i: usize) {
        if self.cards[i].kind.pointer_left() && !self.cards[i].animating() {
            self.cards[i].refresh(&self.sh);
        }
        let card = &mut self.cards[i];
        card.set_hover(false);
        card.expand_at = None;
        let changed = card.hot.is_some() || card.close_hot;
        card.set_hot(None, self.sh.animations);
        card.close_hot = false;
        if changed {
            card.present(&self.sh, false);
        }
        self.tooltip.hide();
    }

    fn on_press(&mut self, i: usize, x: i32, y: i32, sx: i32, sy: i32) {
        self.tooltip.hide();
        if self.cards[i].animating() {
            return;
        }
        let sh = &self.sh;
        let card = &self.cards[i];
        let action = if card.on_close(x, y, sh) {
            Some(Act::Close)
        } else if card.on_badge(x, y, sh) {
            Some(Act::Badge)
        } else {
            card.action_at(x, y, sh)
        };
        let card = &mut self.cards[i];
        card.press = Some(Press { x: sx, y: sy, action });
        card.drag = None;
        if action.is_some() && card.hot.is_some() {
            card.press_down();
            card.present(&self.sh, false);
        }
        // o widget pode reagir já ao apertar (o Clawd pula), sem esperar soltar
        if let Some(Act::Kind(n)) = action {
            if self.cards[i].kind.pressed(n) {
                self.cards[i].refresh(&self.sh);
            }
        }
    }

    fn on_release(&mut self, i: usize, x: i32, y: i32) {
        let press = self.cards[i].press.take();
        if self.cards[i].pressing {
            self.cards[i].press_up();
            self.cards[i].present(&self.sh, false);
        }
        if self.cards[i].drag.is_some() {
            let (px, py) = (x, y);
            self.drag_end(i, px, py);
        } else if let Some(Press { action: Some(action), .. }) = press {
            if !self.cards[i].animating() {
                self.run_action(i, action);
            }
        }
    }

    fn on_double(&mut self, i: usize, x: i32, y: i32) {
        let sh = &self.sh;
        let card = &self.cards[i];
        if !card.animating() && !card.on_badge(x, y, sh) && !card.on_close(x, y, sh) && card.action_at(x, y, sh).is_none() {
            self.badge_action(i);
        }
    }

    fn run_action(&mut self, i: usize, action: Act) {
        match action {
            Act::Badge => self.badge_action(i),
            Act::Close => {
                let key = self.cards[i].key();
                self.close_widget(key);
                return;
            }
            Act::Retry => self.with_kind(i, |k, ctx| k.retry(ctx)),
            Act::Kind(n) => self.with_kind(i, |k, ctx| k.act(n, ctx)),
        }
        if let Some(card) = self.cards.get_mut(i) {
            if !card.animating() {
                card.refresh(&self.sh);
            }
        }
    }

    fn on_wheel(&mut self, i: usize, delta: i32) {
        if self.cards[i].animating() || delta == 0 {
            return;
        }
        self.tooltip.hide();
        let (px, py) = win::pointer();
        let (x, y) = (px - self.cards[i].pos.0, py - self.cards[i].pos.1);
        let (lx, ly) = self.cards[i].logical(x, y);
        let mode = self.cards[i].mode;
        let steps = delta as f32 / 120.0;
        self.with_kind(i, |k, ctx| k.wheel(lx, ly, steps, mode, ctx));
        self.cards[i].refresh(&self.sh);
    }

    // ---------- arrastar e encaixar ----------

    /// (sx, sy): posição do mouse na tela.
    fn drag_move(&mut self, i: usize, sx: i32, sy: i32) {
        let card = &self.cards[i];
        let Some(press) = &card.press else { return };
        if card.animating() || card.temp {
            return;
        }
        if card.drag.is_none() {
            let locked = self.sh.locked() && !self.sh.editing;
            let dist = (((sx - press.x).pow(2) + (sy - press.y).pow(2)) as f32).sqrt();
            if locked || dist < DRAG_START * card.s {
                return;
            }
            let offset = (press.x - card.pos.0, press.y - card.pos.1);
            let card = &mut self.cards[i];
            card.drag = Some(offset);
            card.pressing = false;
            card.hot = None;
            events::set_cursor(card.hwnd, Cursor::Move);
            self.tooltip.hide();
            self.drag_begin(i);
        }
        let (ox, oy) = self.cards[i].drag.unwrap();
        let mut pos = (sx - ox, sy - oy);
        let free = win::key_down(0x12); // com Alt, solta o widget sem encaixar nem reordenar
        let column = if free { None } else { self.column_at(i, pos) };
        let mut targets: HashMap<usize, (i32, i32)> = HashMap::new();
        let mut guides = GuideLines::default();
        self.preview = None;
        if let Some(col) = column {
            // sobre uma coluna: os outros abrem espaço para ele, como na tela do iPhone
            let (t, slot) = self.layout_column(i, &col, pos);
            targets = t;
            self.preview = slot;
        } else if !free {
            let (p, g) = self.snap(i, pos);
            pos = p;
            guides = g;
        }
        self.guides.show(guides, self.cards[i].s);
        for j in 0..self.cards.len() {
            if j == i {
                continue;
            }
            let to = targets.get(&j).or(self.rest.get(&j)).copied().unwrap_or(self.cards[j].pos);
            self.cards[j].slide_to(to, REORDER_MS, &self.sh);
        }
        self.cards[i].pos = pos;
        self.move_dirty = Some(i);
    }

    /// Guarda onde cada widget fica se este sair do lugar (a coluna dele fecha o espaço).
    fn drag_begin(&mut self, i: usize) {
        let followers = self.stack_below(i, None);
        let shift = self.cards[i].card_size().1 + (WIDGET_GAP * self.cards[i].s).round() as i32;
        self.rest = (0..self.cards.len())
            .filter(|&j| j != i)
            .map(|j| {
                let p = self.cards[j].pos;
                (j, if followers.contains(&j) { (p.0, p.1 - shift) } else { p })
            })
            .collect();
        self.origin = self.cards[i].card_rect();
    }

    /// Coluna de widgets sob o cartão arrastado: (esquerda, topo, membros) ou None.
    fn column_at(&self, i: usize, pos: (i32, i32)) -> Option<(i32, i32, Vec<usize>)> {
        let card = &self.cards[i];
        let rect = card.card_rect_at(pos, None);
        let (w, h) = (rect.2 - rect.0, rect.3 - rect.1);
        let tol = (12.0 * card.s) as i32;
        let mut groups: Vec<(i32, Vec<usize>)> = Vec::new();
        for (&j, &rp) in &self.rest {
            let left = rp.0 + self.cards[j].margin;
            match groups.iter_mut().find(|g| (g.0 - left).abs() <= tol) {
                Some(g) => g.1.push(j),
                None => groups.push((left, vec![j])),
            }
        }
        let cy = (rect.1 + rect.3) as f32 / 2.0;
        for (left, mut members) in groups {
            let col_w = members.iter().map(|&j| self.cards[j].card_size().0).max().unwrap_or(0);
            if (win::overlap(rect, (left, rect.1, left + col_w, rect.3)).0 as f32) < 0.6 * w.min(col_w) as f32 {
                continue;
            }
            let tops: Vec<i32> = members.iter().map(|&j| self.rest[&j].1 + self.cards[j].margin).collect();
            let bottoms: Vec<i32> = members.iter().zip(&tops).map(|(&j, t)| t + self.cards[j].card_size().1).collect();
            let mut top = *tops.iter().min().unwrap();
            if (self.origin.0 - left).abs() <= tol && self.origin.1 < top {
                top = self.origin.1; // ele era o primeiro desta coluna
            }
            if (top as f32 - h as f32 / 2.0) <= cy && cy <= *bottoms.iter().max().unwrap() as f32 + h as f32 / 2.0 {
                members.sort_by_key(|j| self.rest[j].1);
                return Some((left, top, members));
            }
        }
        None
    }

    /// Posições da coluna com este widget inserido onde o mouse está.
    fn layout_column(&self, i: usize, col: &(i32, i32, Vec<usize>), pos: (i32, i32)) -> (HashMap<usize, (i32, i32)>, Option<(i32, i32)>) {
        let (left, top, members) = col;
        let card = &self.cards[i];
        let rect = card.card_rect_at(pos, None);
        let cy = (rect.1 + rect.3) as f32 / 2.0;
        let gap = (WIDGET_GAP * card.s).round() as i32;
        let index = members
            .iter()
            .filter(|&&j| ((self.rest[&j].1 + self.cards[j].margin) as f32 + self.cards[j].card_size().1 as f32 / 2.0) < cy)
            .count();
        let mut targets = HashMap::new();
        let mut y = *top;
        let mut slot = None;
        for k in 0..=members.len() {
            if k == index {
                slot = Some((left - card.margin, y - card.margin));
                y += rect.3 - rect.1 + gap;
            }
            if k == members.len() {
                break;
            }
            let j = members[k];
            targets.insert(j, (left - self.cards[j].margin, y - self.cards[j].margin));
            y += self.cards[j].card_size().1 + gap;
        }
        (targets, slot)
    }

    /// Encaixa a posição nas bordas da tela e nos outros widgets; devolve também as guias.
    fn snap(&self, i: usize, pos: (i32, i32)) -> ((i32, i32), GuideLines) {
        let card = &self.cards[i];
        let (s, m) = (card.s, card.margin);
        let (w, h) = card.card_size();
        let (x, y) = (pos.0 + m, pos.1 + m);
        let (gap, edge, reach) = (WIDGET_GAP * s, EDGE_GAP * s, SNAP_DIST * s);
        // candidatos: (posição do canto do cartão, linha-guia, retângulo de referência)
        let mut xs: Vec<(f32, Option<f32>, Rect)> = Vec::new();
        let mut ys: Vec<(f32, Option<f32>, Rect)> = Vec::new();
        for area in win::work_areas() {
            let (l, t, r, b) = (area.0 as f32, area.1 as f32, area.2 as f32, area.3 as f32);
            xs.push((l + edge, Some(l + edge), area));
            xs.push((r - edge - w as f32, Some(r - edge), area));
            ys.push((t + edge, Some(t + edge), area));
            ys.push((b - edge - h as f32, Some(b - edge), area));
        }
        for (j, other) in self.cards.iter().enumerate() {
            if j == i {
                continue;
            }
            let rect = other.card_rect();
            let (l, t, r, b) = (rect.0 as f32, rect.1 as f32, rect.2 as f32, rect.3 as f32);
            xs.extend([(l, Some(l), rect), (r - w as f32, Some(r), rect), ((l + r - w as f32) / 2.0, Some((l + r) / 2.0), rect)]);
            ys.extend([(t, Some(t), rect), (b - h as f32, Some(b), rect), ((t + b - h as f32) / 2.0, Some((t + b) / 2.0), rect)]);
            // lado a lado ou um embaixo do outro, com o espaço padrão (só se estiverem perto)
            if win::overlap((x, y - reach as i32, x + w, y + h + reach as i32), rect).1 > 0 {
                xs.push((r + gap, None, rect));
                xs.push((l - gap - w as f32, None, rect));
            }
            if win::overlap((x - reach as i32, y, x + w + reach as i32, y + h), rect).0 > 0 {
                ys.push((b + gap, None, rect));
                ys.push((t - gap - h as f32, None, rect));
            }
        }
        let best = |opts: &[(f32, Option<f32>, Rect)], cur: f32| {
            opts.iter().filter(|o| (o.0 - cur).abs() <= reach).min_by(|a, b| (a.0 - cur).abs().total_cmp(&(b.0 - cur).abs())).copied()
        };
        let bx = best(&xs, x as f32);
        let by = best(&ys, y as f32);
        let nx = bx.map(|b| b.0.round() as i32).unwrap_or(x);
        let ny = by.map(|b| b.0.round() as i32).unwrap_or(y);
        let mut guides = GuideLines::default();
        if let Some((_, Some(line), r)) = bx {
            guides.v = Some((line.round() as i32, r.1.min(ny), r.3.max(ny + h)));
        }
        if let Some((_, Some(line), r)) = by {
            guides.h = Some((line.round() as i32, r.0.min(nx), r.2.max(nx + w)));
        }
        ((nx - m, ny - m), guides)
    }

    fn drag_end(&mut self, i: usize, x: i32, y: i32) {
        self.cards[i].drag = None;
        self.guides.hide();
        self.cards[i].present(&self.sh, true);
        self.move_dirty = None;
        if let Some(slot) = self.preview.take() {
            self.cards[i].slide_to(slot, REORDER_MS, &self.sh); // para o espaço aberto na coluna
        } else {
            self.moved(i);
        }
        self.rest.clear();
        self.on_motion(i, x, y); // volta o cursor e as dicas ao normal
    }

    // ---------- menus ----------

    fn layout_names(&self) -> Vec<String> {
        self.sh.cfg["app"]["layouts"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default()
    }

    /// Itens do menu que valem para todos os widgets (também é o menu da bandeja).
    fn common_entries(&self, with_close: bool) -> Vec<MenuItem> {
        let widgets_menu = widgets::KEYS
            .iter()
            .enumerate()
            .map(|(n, key)| MenuItem::Check(widgets::title(key).into(), self.find(key).is_some(), M_WIDGET + n as u32))
            .chain([MenuItem::Sep, MenuItem::Cmd("Galeria de widgets…".into(), M_GALLERY)])
            .collect();
        let names = self.layout_names();
        let mut layouts: Vec<MenuItem> = names.iter().enumerate().map(|(n, name)| MenuItem::Cmd(name.clone(), M_LAYOUT + n as u32)).collect();
        if !names.is_empty() {
            layouts.push(MenuItem::Sep);
            layouts.push(MenuItem::Sub(
                "Excluir".into(),
                names.iter().enumerate().map(|(n, name)| MenuItem::Cmd(name.clone(), M_LAYOUT_DEL + n as u32)).collect(),
            ));
        }
        if !names.is_empty() {
            layouts.insert(names.len(), MenuItem::Cmd("Salvar layout atual…".into(), M_SAVE_LAYOUT));
        } else {
            layouts.push(MenuItem::Cmd("Salvar layout atual…".into(), M_SAVE_LAYOUT));
        }
        let focus = match self.focus_until {
            Some(until) => {
                let when = until.map(|t| format!(" (até {})", win::clock_at(t))).unwrap_or_default();
                vec![MenuItem::Cmd(format!("Sair do modo foco{when}"), M_FOCUS_END)]
            }
            None => [("30 minutos", 30), ("1 hora", 60), ("2 horas", 120), ("Até eu desligar", 0)]
                .iter()
                .map(|(label, minutes)| MenuItem::Cmd(label.to_string(), M_FOCUS + minutes))
                .collect(),
        };
        let mut entries = vec![
            MenuItem::Sub("Widgets".into(), widgets_menu),
            MenuItem::Sub("Layouts".into(), layouts),
            MenuItem::Sep,
            MenuItem::Cmd((if self.sh.editing { "Concluir edição" } else { "Editar widgets" }).into(), M_EDIT),
            MenuItem::Cmd("Mostrar na frente".into(), M_PEEK),
            MenuItem::Sub("Modo foco".into(), focus),
            MenuItem::Cmd("Organizar widgets".into(), M_ARRANGE),
            MenuItem::Check("Travar posições".into(), self.sh.locked(), M_LOCK),
            MenuItem::Sep,
            MenuItem::Cmd("Configurações…".into(), M_SETTINGS),
            MenuItem::Cmd("Dicas".into(), M_TIPS),
            MenuItem::Sep,
        ];
        if with_close {
            entries.push(MenuItem::Cmd("Fechar este widget".into(), M_CLOSE));
        }
        entries.push(MenuItem::Cmd("Sair".into(), M_QUIT));
        entries
    }

    fn show_menu(&mut self, card: Option<usize>) {
        self.tooltip.hide();
        let mut items = Vec::new();
        if let Some(i) = card {
            let c = &self.cards[i];
            let sizes = c.kind.sizes();
            if sizes.len() > 1 {
                let current = c.mode;
                let temp = c.temp;
                items.push(MenuItem::Sub(
                    "Tamanho".into(),
                    sizes.iter().enumerate().map(|(n, &m)| MenuItem::Radio(m.name().into(), m == current && !temp, M_SIZE + n as u32)).collect(),
                ));
            }
            let env = widgets::Env { cfg: &self.sh.cfg, key: c.key() };
            let own = c.kind.menu(&env);
            if !own.is_empty() {
                items.push(MenuItem::Sep);
                items.extend(own);
            }
            items.push(MenuItem::Sep);
        }
        items.extend(self.common_entries(card.is_some()));
        let (x, y) = win::pointer();
        let context = card.map(|i| self.cards[i].key());
        self.menu.popup(items, x, y, context);
    }

    fn menu_command(&mut self, id: u32, card: Option<usize>) {
        if self.island_menu(id) || self.perch_menu(id) {
            return;
        }
        match id {
            0 => {}
            M_SETTINGS => self.open_settings("geral"),
            M_TIPS => self.open_settings("dicas"),
            M_GALLERY => self.open_settings("widgets"),
            M_SAVE_LAYOUT => self.open_prompt(prompt::Purpose::LayoutName, None),
            M_EDIT => {
                if self.sh.editing {
                    self.end_edit();
                } else {
                    self.start_edit();
                }
            }
            M_FOCUS_END => self.end_focus(),
            id if (M_FOCUS..M_FOCUS + 1000).contains(&id) => self.start_focus(id - M_FOCUS),
            id if (M_LAYOUT..M_LAYOUT + 1000).contains(&id) => {
                if let Some(name) = self.layout_names().get((id - M_LAYOUT) as usize).cloned() {
                    self.apply_layout(&name);
                }
            }
            id if (M_LAYOUT_DEL..M_LAYOUT_DEL + 1000).contains(&id) => {
                if let Some(name) = self.layout_names().get((id - M_LAYOUT_DEL) as usize).cloned() {
                    self.delete_layout(&name);
                }
            }
            M_PEEK => self.toggle_peek(),
            M_ARRANGE => self.arrange(),
            M_LOCK => {
                let locked = self.sh.locked();
                self.sh.cfg.obj_mut("app").insert("travado".into(), json!(!locked));
                self.sh.save();
            }
            M_QUIT => self.exit(),
            M_CLOSE => {
                if let Some(i) = card {
                    let key = self.cards[i].key();
                    self.close_widget(key);
                }
            }
            id if id >= M_WIDGET && id < M_WIDGET + 100 => {
                let key = widgets::KEYS[(id - M_WIDGET) as usize];
                let on = self.find(key).is_none();
                self.set_visible(key, on);
            }
            id if id >= M_SIZE && id < M_SIZE + 10 => {
                if let Some(i) = card {
                    let mode = self.cards[i].kind.sizes()[(id - M_SIZE) as usize];
                    self.set_mode(i, mode);
                }
            }
            id if id < 100_000 => {
                if let Some(i) = card {
                    self.run_action(i, Act::Kind(id));
                }
            }
            _ => {}
        }
    }

    // ---------- avisos ----------

    /// Aviso rápido na parte de baixo da tela, com um botão opcional (ex.: "Desfazer").
    fn toast(&mut self, text: &str, detail: Option<&str>, icon: &'static str, color: Rgba, undo: Option<(&str, Undo)>) {
        let mut parts = vec![Part::Icon(icon, color), Part::Text(text.into(), true)];
        if let Some(d) = detail {
            parts.push(Part::Muted(d.into()));
        }
        let seconds = if undo.is_some() { TOAST_ACTION_SECONDS } else { TOAST_SECONDS };
        if let Some((label, action)) = undo {
            parts.push(Part::Button(label.into(), B_UNDO, false));
            self.toast_undo = Some(action);
        } else {
            self.toast_undo = None;
        }
        self.toast.show(parts, self.sh.animations);
        self.toast_until = Some(Instant::now() + Duration::from_secs_f32(seconds));
    }

    fn run_undo(&mut self) {
        let Some(undo) = self.toast_undo.take() else { return };
        self.toast.hide(self.sh.animations);
        self.toast_until = None;
        match undo {
            Undo::Reopen { key, pos, mode, followers } => {
                if self.find(key).is_some() {
                    return;
                }
                for (other, old) in followers {
                    if let Some(j) = self.find(other) {
                        self.cards[j].slide_to(old, ANIM_MS, &self.sh);
                    }
                }
                let w = self.sh.cfg.obj_mut(key);
                w.insert("x".into(), json!(pos.0));
                w.insert("y".into(), json!(pos.1));
                widgets::store_size(w, mode);
                self.set_visible_with(key, true, true, false);
            }
            Undo::Show(key) => {
                self.set_visible(key, true);
            }
            Undo::AppSettings(old) => {
                self.sh.cfg["app"] = old;
                self.sh.save();
                self.apply_settings("*");
            }
            Undo::Update => self.install_update(),
            Undo::Layout { name, snapshot } => {
                self.sh.cfg.obj_mut("app").entry("layouts").or_insert(json!({}));
                if let Some(l) = self.sh.cfg["app"]["layouts"].as_object_mut() {
                    l.insert(name, snapshot);
                }
                self.sh.save();
            }
        }
    }

    /// Fecha um widget com aviso e "Desfazer" (que o devolve ao lugar, com os vizinhos).
    fn close_widget(&mut self, key: &'static str) {
        let Some(i) = self.find(key) else { return };
        let followers: Vec<(&'static str, (i32, i32))> = self.stack_below(i, None).into_iter().map(|j| (self.cards[j].key(), self.cards[j].pos)).collect();
        let pos = self.cards[i].pos;
        let mode = if self.cards[i].temp { Mode::P } else { self.cards[i].mode };
        if !self.set_visible(key, false) {
            self.toast("Não dá para fechar o último widget sem o ícone na bandeja", None, "close", WARNING, None);
            return;
        }
        let text = format!("{} fechado", widgets::title(key));
        self.toast(&text, None, "close", WHITE, Some(("Desfazer", Undo::Reopen { key, pos, mode, followers })));
    }

    // ---------- layouts ----------

    fn apply_layout(&mut self, name: &str) {
        let Some(snapshot) = self.sh.cfg["app"]["layouts"].get(name).cloned() else { return };
        let Some(map) = snapshot.as_object() else { return };
        // salvo com outros monitores: cada widget vai para o lugar equivalente nestes
        let moves = self.layout_moves(&snapshot);
        for (key, state) in map {
            let Some(&key) = widgets::KEYS.iter().find(|k| **k == key.as_str()) else { continue };
            if !state.bool_of("visible").unwrap_or(false) {
                self.set_visible_with(key, false, false, true);
                continue;
            }
            let (mut x, mut y) = (state.i64_of("x").unwrap_or(0) as i32, state.i64_of("y").unwrap_or(0) as i32);
            let mode = state.str_of("tamanho").and_then(Mode::from_code).unwrap_or(widgets::DEFAULT_MODE);
            if let Some(&(_, pos, s)) = moves.iter().find(|m| m.0 == key) {
                (x, y) = pos;
                if let Some(i) = self.find(key) {
                    if (self.cards[i].s - s).abs() > 0.01 {
                        self.cards[i].set_scale(s);
                        self.cards[i].refresh(&self.sh);
                    }
                }
            }
            match self.find(key) {
                None => {
                    let w = self.sh.cfg.obj_mut(key);
                    w.insert("x".into(), json!(x));
                    w.insert("y".into(), json!(y));
                    widgets::store_size(w, mode);
                    self.set_visible_with(key, true, true, false);
                }
                Some(i) => {
                    if self.cards[i].temp {
                        self.cards[i].temp = false;
                        self.cards[i].mode = Mode::P;
                    }
                    if self.cards[i].mode != mode {
                        self.resize_to(i, mode, false, false);
                        self.later.push((Instant::now() + Duration::from_millis(ANIM_MS + 40), key, (x, y)));
                    } else {
                        self.cards[i].slide_to((x, y), ANIM_MS, &self.sh);
                    }
                }
            }
        }
        self.toast(&format!("Layout \"{name}\" aplicado"), None, "check", SUCCESS, None);
    }

    fn delete_layout(&mut self, name: &str) {
        let removed = self.sh.cfg["app"]["layouts"].as_object_mut().and_then(|l| l.remove(name));
        self.sh.save();
        if let Some(snapshot) = removed {
            let undo = Undo::Layout { name: name.to_string(), snapshot };
            self.toast(&format!("Layout \"{name}\" excluído"), None, "close", WHITE, Some(("Desfazer", undo)));
        }
    }

    // ---------- modo edição ----------

    /// Todos os widgets na frente, com "−" para fechar e o botão de tamanho sempre visível.
    fn start_edit(&mut self) {
        if self.sh.editing {
            return;
        }
        if self.focus_until.is_some() {
            self.end_focus();
        }
        self.end_peek();
        self.sh.editing = true;
        for c in &mut self.cards {
            win::set_z(c.hwnd, Z::Topmost);
            c.present(&self.sh, false);
        }
        self.edit_bar.show(
            vec![
                Part::Icon("edit", WHITE),
                Part::Text("Editando widgets".into(), true),
                Part::Muted("Arraste para mover  ·  −  fecha  ·  +  muda o tamanho".into()),
                Part::Button("Adicionar widget".into(), B_EDIT_ADD, false),
                Part::Button("Concluir".into(), B_EDIT_DONE, true),
            ],
            self.sh.animations,
        );
        self.edit_bar.focus();
    }

    fn end_edit(&mut self) {
        if !self.sh.editing {
            return;
        }
        self.sh.editing = false;
        self.edit_bar.hide(self.sh.animations);
        for i in 0..self.cards.len() {
            win::set_z(self.cards[i].hwnd, Z::NoTopmost);
            self.cards[i].close_hot = false;
            self.send_to_back(i);
            self.cards[i].present(&self.sh, false);
        }
    }

    // ---------- modo foco ----------

    /// Esconde os widgets por um tempo (ou até desligar), sem fechar nada.
    fn start_focus(&mut self, minutes: u32) {
        self.end_peek();
        self.end_edit();
        let until = (minutes > 0).then(|| win::now() + minutes as f64 * 60.0);
        self.focus_until = Some(until);
        self.sh.focus = true;
        for c in &self.cards {
            events::show(c.hwnd, false);
        }
        match until {
            Some(t) => self.tray.set_tip(&format!("Modo foco até {}", win::clock_at(t))),
            None => self.tray.set_tip("Modo foco ligado"),
        }
        self.toast("Modo foco ligado", Some("Os widgets voltam pelo ícone da bandeja"), "pin", Rgba::rgb(191, 90, 242), None);
    }

    fn end_focus(&mut self) {
        if self.focus_until.take().is_none() {
            return;
        }
        self.sh.focus = false;
        self.tray.set_tip(tray::TIP);
        for i in 0..self.cards.len() {
            if self.island.is_some() && self.cards[i].key() == "midia" {
                continue; // está como ilha
            }
            events::show(self.cards[i].hwnd, true);
            self.send_to_back(i);
        }
        self.wake_all();
    }

    // ---------- modos ----------

    /// Mostra os widgets por cima das janelas, até chamar de novo ou o mouse sair deles.
    pub fn toggle_peek(&mut self) {
        if self.sh.editing {
            return;
        }
        if self.focus_until.is_some() {
            self.end_focus();
        }
        if self.sh.peeking {
            self.end_peek();
            return;
        }
        self.sh.peeking = true;
        for c in &mut self.cards {
            win::set_z(c.hwnd, Z::Topmost);
            c.present(&self.sh, false); // opacidade total enquanto estão na frente
        }
        self.peek_seen = Instant::now();
    }

    fn end_peek(&mut self) {
        if !self.sh.peeking {
            return;
        }
        self.sh.peeking = false;
        for i in 0..self.cards.len() {
            win::set_z(self.cards[i].hwnd, Z::NoTopmost);
            self.send_to_back(i);
            self.cards[i].present(&self.sh, false);
        }
    }

    /// Empilha os widgets em colunas a partir da direita da tela, com o espaço padrão.
    pub fn arrange(&mut self) {
        let mut order: Vec<usize> = (0..self.cards.len()).filter(|&i| self.cards[i].content.is_some() && !self.cards[i].temp).collect();
        if order.is_empty() {
            return;
        }
        order.sort_by_key(|&i| (-self.cards[i].card_rect().2, self.cards[i].card_rect().1));
        let (_, top, right, bottom) = win::area_for(self.cards[order[0]].card_rect());
        let s = self.cards[order[0]].s;
        let (edge, gap) = ((EDGE_GAP * s) as i32, (WIDGET_GAP * s) as i32);
        let (mut y, mut column_w, mut right) = (top + edge, 0, right - edge);
        for i in order {
            let (w, h) = self.cards[i].card_size();
            if column_w > 0 && y + h > bottom - edge {
                right -= column_w + gap; // não cabe: próxima coluna
                y = top + edge;
                column_w = 0;
            }
            let m = self.cards[i].margin;
            self.cards[i].slide_to((right - w - m, y - m), ANIM_MS, &self.sh);
            y += h + gap;
            column_w = column_w.max(w);
        }
    }

    fn wake_all(&mut self) {
        for c in &mut self.cards {
            if c.stale {
                c.refresh(&self.sh);
            } else {
                c.present(&self.sh, false);
            }
        }
    }

    // ---------- notificações ----------

    fn notify(&mut self, kind: &'static str, title: &str, text: &str, cooldown: f64, action: Option<(NotifyAction, Option<&'static str>)>) {
        if !config::notify_enabled(&self.sh.cfg, kind) {
            return;
        }
        let now = win::now();
        if now - self.notified.get(kind).copied().unwrap_or(0.0) < cooldown {
            return;
        }
        self.notified.insert(kind, now);
        self.notification_action = action;
        self.tray.notify(title, text);
    }

    fn notification_clicked(&mut self) {
        let Some((action, _)) = self.notification_action.clone() else { return };
        match action {
            NotifyAction::Open(target) => win::open_target(&target),
            NotifyAction::Widget(key, n) => {
                if let Some(i) = self.find(key) {
                    self.run_action(i, Act::Kind(n));
                }
            }
        }
    }

    // ---------- verificações periódicas ----------

    fn check_backdrop(&mut self) {
        self.sh.read_preferences();
        let blur = config::setting_f64(&self.sh.cfg, "desfoque");
        let changed = self.sh.backdrop.check(blur);
        let progman = win::desktop_window();
        for i in 0..self.cards.len() {
            let c = &mut self.cards[i];
            if progman.is_some() && c.owner != progman {
                c.owner = win::pin_to_desktop(c.hwnd); // o Explorer reiniciou
            }
            // (trocar de monitor é com o `check_screens`)
            if changed && c.content.is_some() && !c.animating() {
                c.present(&self.sh, false);
            }
        }
    }

    fn check_fullscreen(&mut self) {
        let busy = win::fullscreen_screens();
        if busy != self.sh.paused {
            self.sh.paused = busy;
            self.wake_all(); // os que voltaram a aparecer se redesenham (os pausados esperam)
        }
    }

    /// Notifica CPU ou memória altas por alguns minutos e disco quase cheio.
    fn check_alerts(&mut self) {
        let now = win::now();
        let (idle, total) = win::cpu_times();
        let (last_idle, last_total) = self.alert_cpu;
        self.alert_cpu = (idle, total);
        let cpu = if total > last_total { 1.0 - (idle - last_idle) as f64 / (total - last_total) as f64 } else { 0.0 };
        let (used, total_mem) = win::memory();
        for (kind, value, text) in [
            ("cpu", cpu, "A CPU está acima de 90% há 3 minutos."),
            ("ram", used as f64 / total_mem as f64, "A memória está acima de 90% há 3 minutos."),
        ] {
            if value >= 0.9 && config::notify_enabled(&self.sh.cfg, kind) {
                let since = *self.high_since.entry(kind).or_insert(now);
                if now - since >= 180.0 {
                    let title = if kind == "cpu" { "CPU alta" } else { "Memória quase cheia" };
                    self.notify(kind, title, text, 1800.0, Some((NotifyAction::Open("taskmgr.exe".into()), None)));
                }
            } else {
                self.high_since.remove(kind);
            }
        }
        let (dused, dtotal, free) = win::disk_usage("C:\\");
        if dused as f64 / dtotal as f64 >= 0.95 {
            let text = format!("O disco C: está com {:.0}% de uso ({:.0} GB livres).", dused as f64 / dtotal as f64 * 100.0, win::gb(free));
            self.notify("disco", "Disco quase cheio", &text, 86400.0, Some((NotifyAction::Open("ms-settings:storagesense".into()), None)));
        }
    }

    // ---------- laço ----------

    pub fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::Mouse { hwnd, kind, x, y, sx, sy } => {
                if let Some(i) = self.panel_index(hwnd) {
                    self.panel_mouse(i, kind, x, y);
                    return;
                }
                let _ = (sx, sy);
                if self.menu.owns(hwnd) {
                    match kind {
                        MouseKind::Move => self.menu.motion(hwnd, x, y),
                        MouseKind::Up => {
                            let context = self.menu.context;
                            if let Some(id) = self.menu.click(hwnd, x, y) {
                                let card = context.and_then(|k| self.find(k));
                                self.menu_command(id, card);
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                if self.toast.hwnd() == Some(hwnd) {
                    match kind {
                        MouseKind::Move => self.toast.motion(x, y),
                        MouseKind::Leave => self.toast.leave(),
                        MouseKind::Up if self.toast.click(x, y) == Some(B_UNDO) => self.run_undo(),
                        _ => {}
                    }
                    return;
                }
                if self.edit_bar.hwnd() == Some(hwnd) {
                    match kind {
                        MouseKind::Move => self.edit_bar.motion(x, y),
                        MouseKind::Leave => self.edit_bar.leave(),
                        MouseKind::Up => match self.edit_bar.click(x, y) {
                            Some(B_EDIT_DONE) => self.end_edit(),
                            Some(B_EDIT_ADD) => self.open_settings("widgets"),
                            _ => {}
                        },
                        _ => {}
                    }
                    return;
                }
                if self.island_owns(hwnd) {
                    self.island_mouse(kind, x, y, sx, sy);
                    return;
                }
                if self.perch_owns(hwnd) {
                    self.perch_mouse(kind, sx, sy);
                    return;
                }
                let Some(i) = self.index(hwnd) else { return };
                match kind {
                    MouseKind::Move => {
                        if self.cards[i].press.is_some() {
                            self.drag_move(i, sx, sy);
                        }
                        self.on_motion(i, x, y);
                    }
                    MouseKind::Down => {
                        self.on_press(i, x, y, sx, sy);
                    }
                    MouseKind::Up => self.on_release(i, x, y),
                    MouseKind::Double => self.on_double(i, x, y),
                    MouseKind::Right => self.show_menu(Some(i)),
                    MouseKind::Leave => self.on_leave(i),
                }
            }
            Ev::Key { hwnd, vk } if self.panel_index(hwnd).is_some() => {
                let i = self.panel_index(hwnd).unwrap();
                self.panel_key(i, vk);
            }
            Ev::Wheel { hwnd, delta } if self.panel_index(hwnd).is_some() => {
                let i = self.panel_index(hwnd).unwrap();
                self.panel_wheel(i, delta);
            }
            Ev::PanelClose(hwnd) => {
                if let Some(i) = self.panel_index(hwnd) {
                    self.close_panel(i);
                }
            }
            Ev::PanelMoved(hwnd) => {
                if let Some(i) = self.panel_index(hwnd) {
                    self.panels[i].win.dirty = true;
                }
            }
            Ev::EditChange(hwnd, ctrl) | Ev::EditFocus(hwnd, ctrl, _) | Ev::EditKey(hwnd, ctrl, _) if self.panel_index(hwnd).is_some() => {
                let i = self.panel_index(hwnd).unwrap();
                let what = match ev {
                    Ev::EditFocus(_, _, on) => EditWhat::Focus(on),
                    Ev::EditKey(_, _, vk) => EditWhat::Key(vk),
                    _ => EditWhat::Change,
                };
                self.edit_event(i, ctrl, what);
            }
            Ev::EditChange(..) | Ev::EditFocus(..) | Ev::EditKey(..) => {}
            Ev::Key { hwnd, vk } => {
                if self.menu.is_open() {
                    let context = self.menu.context;
                    if let Some(id) = self.menu.key(vk) {
                        let card = context.and_then(|k| self.find(k));
                        self.menu_command(id, card);
                    }
                } else if self.edit_bar.hwnd() == Some(hwnd) && vk == 0x1B {
                    self.end_edit();
                }
            }
            Ev::Wheel { hwnd, delta } => {
                if self.island_owns(hwnd) {
                    self.island_wheel(delta);
                } else if let Some(i) = self.index(hwnd) {
                    self.on_wheel(i, delta);
                }
            }
            Ev::Tray(msg) => match msg {
                0x202 => self.toggle_peek(),   // WM_LBUTTONUP
                0x205 => self.show_menu(None), // WM_RBUTTONUP
                NIN_BALLOONUSERCLICK => self.notification_clicked(),
                _ => {}
            },
            Ev::Wake => {}
            Ev::TaskbarCreated => self.tray.add(),
            Ev::OtherInstance => {
                self.toast("Os widgets já estão abertos", Some("Clique direito num widget ou no ícone da bandeja"), "widgets", ACCENT, None);
                if !self.sh.peeking && !self.sh.editing {
                    self.toggle_peek();
                }
            }
            Ev::DisplayChange => {
                self.next_backdrop = Instant::now();
                self.next_screens = Instant::now();
            }
        }
    }

    /// Depois de tratar os eventos de uma leva: mostra o widget arrastado na posição nova.
    pub fn after_events(&mut self) {
        if let Some(i) = self.move_dirty.take() {
            if i < self.cards.len() {
                self.cards[i].present(&self.sh, true);
            }
        }
    }

    pub fn next_deadline(&self) -> Instant {
        let mut next = self.next_backdrop.min(self.next_fullscreen).min(self.next_power).min(self.next_alerts).min(self.next_screens);
        for t in [self.island_deadline(), self.perch_deadline()].into_iter().flatten() {
            next = next.min(t);
        }
        if let Some(t) = self.tooltip.next_due() {
            next = next.min(t);
        }
        for t in [self.menu.next_due(), self.toast.next_frame(), self.edit_bar.next_frame(), self.toast_until].into_iter().flatten() {
            next = next.min(t);
        }
        for (t, ..) in &self.later {
            next = next.min(*t);
        }
        for t in [self.panels_due(), self.welcome_at, Some(self.next_update)].into_iter().flatten() {
            next = next.min(t);
        }
        if let Some(Some(until)) = self.focus_until {
            let secs = (until - win::now()).max(0.0);
            next = next.min(Instant::now() + Duration::from_secs_f64(secs));
        }
        if self.sh.peeking {
            next = next.min(Instant::now() + Duration::from_millis(250));
        }
        for c in &self.cards {
            for t in [c.next_frame(), c.kind.next_tick(), c.expand_at].into_iter().flatten() {
                next = next.min(t);
            }
            if c.temp {
                next = next.min(self.next_watch);
            }
        }
        next
    }

    pub fn run_timers(&mut self) {
        let now = Instant::now();
        self.tooltip.tick();
        self.menu.watch();
        self.panels_tick();
        self.island_tick();
        if self.welcome_at.is_some_and(|t| now >= t) {
            self.welcome_at = None;
            self.open_welcome();
        }
        if now >= self.next_update {
            if config::setting_bool(&self.sh.cfg, "procurar_atualizacoes") {
                self.check_updates(false);
            } else {
                self.next_update = now + UPDATE_EVERY; // desligado: só pelo botão em Sobre
            }
        }
        self.poll_updates();
        self.toast.frame();
        self.edit_bar.frame();
        if self.toast_until.is_some_and(|t| now >= t) {
            if self.toast.pointer_inside() {
                self.toast_until = Some(now + Duration::from_millis(1500)); // o mouse está em cima: espera
            } else {
                self.toast_until = None;
                self.toast_undo = None;
                self.toast.hide(self.sh.animations);
            }
        }
        if let Some(Some(until)) = self.focus_until {
            if win::now() >= until {
                self.end_focus();
            }
        }
        let due: Vec<(&'static str, (i32, i32))> = self.later.iter().filter(|l| now >= l.0).map(|l| (l.1, l.2)).collect();
        self.later.retain(|l| now < l.0);
        for (key, to) in due {
            if let Some(j) = self.find(key) {
                self.cards[j].slide_to(to, ANIM_MS, &self.sh);
            }
        }
        for i in 0..self.cards.len() {
            if self.cards[i].kind.next_tick().is_some_and(|t| now >= t) {
                let was_loading = self.cards[i].status.loading;
                let redraw = self.with_kind(i, |k, ctx| k.tick(ctx));
                let loading = self.cards[i].kind.status().loading;
                if redraw && !self.cards[i].animating() {
                    self.cards[i].refresh(&self.sh);
                } else if loading != was_loading {
                    self.cards[i].status = self.cards[i].kind.status();
                    self.cards[i].present(&self.sh, false);
                }
            }
            if self.cards[i].next_frame().is_some_and(|t| now >= t) {
                let (resized, slid) = self.cards[i].frame(&self.sh);
                if resized {
                    let save = self.cards[i].finish_resize(&self.sh);
                    if save {
                        let (mode, pos) = (self.cards[i].mode, self.cards[i].pos);
                        let key = self.cards[i].key();
                        let w = self.sh.cfg.obj_mut(key);
                        widgets::store_size(w, mode);
                        w.insert("x".into(), json!(pos.0));
                        w.insert("y".into(), json!(pos.1));
                        w.remove("compact");
                        self.sh.save();
                    } else if !self.cards[i].temp {
                        self.moved(i); // abriu detalhes perto da borda e subiu um pouco
                    }
                    if !self.cards[i].temp {
                        self.send_to_back(i);
                    }
                }
                if slid && self.cards[i].drag.is_none() {
                    self.moved(i);
                }
            }
            if self.cards[i].expand_at.is_some_and(|t| now >= t) {
                self.hover_expand(i);
            }
            // abriu ou fechou detalhes: os widgets empilhados embaixo acompanham
            if let Some((old_h, _)) = self.cards[i].grew.take() {
                let w = self.cards[i].card_size().0;
                let old = self.cards[i].card_rect_at(self.cards[i].pos, Some((w, old_h)));
                let new = self.cards[i].card_rect_at(self.cards[i].resize_goal().unwrap_or(self.cards[i].pos), None);
                let start = self.stack_below(i, Some(old)).into_iter().map(|j| (j, (self.cards[j].pos.0, self.cards[j].pos.1 + new.3 - old.3))).collect();
                for (j, to) in self.make_room(i, old, new, start) {
                    self.cards[j].slide_to(to, GROW_MS, &self.sh);
                }
            }
        }
        // o Clawd na barra de tarefas (depois dos widgets: o desenho dele acabou de mudar)
        self.perch_tick();
        if now >= self.next_watch {
            self.next_watch = now + WATCH_EVERY;
            for i in 0..self.cards.len() {
                self.watch_pointer(i);
            }
        }
        if self.sh.peeking {
            if self.cards.iter().any(|c| c.hover_target > 0.0) || self.menu.is_open() {
                self.peek_seen = now;
            }
            if self.peek_seen.elapsed() > Duration::from_millis(PEEK_IDLE_MS) {
                self.end_peek();
            }
        }
        if now >= self.next_screens {
            self.check_screens();
        }
        if now >= self.next_backdrop {
            self.next_backdrop = now + BACKDROP_EVERY;
            self.check_backdrop();
        }
        if now >= self.next_fullscreen {
            self.next_fullscreen = now + FULLSCREEN_EVERY;
            self.check_fullscreen();
        }
        if now >= self.next_power {
            self.next_power = now + POWER_EVERY;
            self.sh.saving = win::on_battery() && config::setting_bool(&self.sh.cfg, "economia_bateria");
        }
        if now >= self.next_alerts {
            self.next_alerts = now + ALERTS_EVERY;
            self.check_alerts();
        }
    }

    // ---------- atualizações ----------

    /// Procura uma versão nova no GitHub (sozinho a cada 6 h, ou pelo botão em Sobre).
    fn check_updates(&mut self, manual: bool) {
        if !manual {
            self.next_update = Instant::now() + UPDATE_EVERY;
        }
        if self.update_rx.as_ref().is_some_and(|(_, m)| *m || !manual) {
            return; // já está procurando
        }
        self.update_rx = Some((updates::check_async(), manual));
        self.update_status = "Procurando…".into();
        self.panels_changed();
    }

    fn poll_updates(&mut self) {
        self.poll_installer();
        let Some((rx, manual)) = &self.update_rx else { return };
        let manual = *manual;
        let found = match rx.try_recv() {
            Ok(found) => found,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("a busca parou".into()),
        };
        self.update_rx = None;
        let t = win::local_time();
        let when = format!("{:02}:{:02}", t.hour, t.minute);
        match found {
            Err(_) => {
                self.update_status = format!("Não deu para verificar ({when})");
                if manual {
                    self.toast("Não deu para procurar atualizações", Some("Verifique a conexão"), "close", WARNING, None);
                }
            }
            Ok(Some(version)) => {
                self.update_status = format!("Versão {version} disponível");
                let notified = self.sh.cache["update_notified"].as_str() == Some(version.as_str());
                if manual || !notified {
                    self.sh.cache["update_notified"] = json!(version);
                    self.sh.save_cache();
                    let label = if updates::installed() { "Atualizar" } else { "Baixar" };
                    self.toast(&format!("Versão {version} disponível"), None, "refresh", ACCENT, Some((label, Undo::Update)));
                }
                self.update_version = Some(version);
            }
            Ok(None) => {
                self.update_version = None;
                self.update_status = format!("Você tem a versão mais recente (verificado às {when})");
                if manual {
                    self.toast("Você já tem a versão mais recente", None, "check", ACCENT, None);
                }
            }
        }
        self.panels_changed();
    }

    fn install_update(&mut self) {
        if self.installer.is_some() {
            return; // já está atualizando
        }
        if updates::installed() {
            self.toast("Atualizando…", Some("Os widgets fecham e abrem de novo sozinhos"), "refresh", ACCENT, None);
        }
        self.installer = updates::install();
    }

    /// O install.ps1 terminou e este app continua aberto: a atualização não aconteceu (deu
    /// certo, ele teria fechado este app). Diz o porquê.
    fn poll_installer(&mut self) {
        let Some(child) = &mut self.installer else { return };
        let code = match child.try_wait() {
            Ok(None) => return, // ainda rodando
            Ok(Some(status)) => status.code(),
            Err(_) => None,
        };
        self.installer = None;
        let (title, detail) = updates::failure_message(code);
        self.update_status = title.to_string();
        self.toast(title, Some(detail), "close", WARNING, None);
        self.panels_changed();
    }

    pub fn exit(&mut self) {
        self.drop_island();
        self.drop_perch();
        self.tray.remove();
        for c in self.cards.drain(..) {
            events::destroy(c.hwnd);
        }
        self.quit = true;
        unsafe { PostQuitMessage(0) };
    }
}

#[allow(dead_code)]
fn unused(_: HWND, _: Value) {}

/// Os cartões (dos retângulos `rects`) empilhados embaixo de `top`, que é o do cartão `i`:
/// encaixados um no outro (a até `limit` pixels), em cadeia.
fn stack_in(rects: &[Rect], i: usize, top: Rect, limit: i32) -> Vec<usize> {
    let mut stack = Vec::new();
    let mut frontier = vec![top];
    while let Some(top_rect) = frontier.pop() {
        for (j, &r) in rects.iter().enumerate() {
            if j == i || stack.contains(&j) {
                continue;
            }
            let gap = r.1 - top_rect.3;
            if (0..=limit).contains(&gap) && win::overlap(top_rect, r).0 > 0 {
                stack.push(j);
                frontier.push(r);
            }
        }
    }
    stack
}

#[cfg(test)]
mod tests {
    use super::stack_in;

    #[test]
    fn stack_follows_the_chain_below() {
        // uma coluna (0, 1, 2) e um widget de outra coluna (3)
        let rects = [(0, 0, 400, 193), (0, 208, 400, 401), (0, 416, 400, 609), (500, 208, 900, 401)];
        assert_eq!(stack_in(&rects, 0, rects[0], 25), vec![1, 2]);
        // medindo com a altura antiga do primeiro (mais alta), o vão até o segundo fica pequeno
        // demais e a pilha continua a mesma
        assert_eq!(stack_in(&rects, 0, (0, 0, 400, 204), 25), vec![1, 2]);
        // longe demais: não está encaixado
        assert!(stack_in(&rects, 0, (0, 0, 400, 150), 25).is_empty());
    }
}
