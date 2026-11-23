//! As notificações dos outros apps na ilha (WhatsApp, Teams, Outlook...): a que acabou de chegar
//! estica a ilha com o ícone do app, quem mandou e o começo da mensagem, e fica um tempo como
//! bolha. Ler não apaga nada: dispensar aqui não mexe na central de notificações do Windows, e
//! dispensar lá tira daqui.
//!
//! Desligado até a pessoa ligar em Configurações → Ilha dinâmica (ler as notificações dos outros
//! apps é coisa que se pede). Dá para esconder o texto (só o app e quem mandou) e silenciar apps.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{draw_buttons, hit, OK, OPEN};
use crate::data::config::{self, Get};
use crate::gfx::{self, Anchor, Canvas, Face, Image};
use crate::platform::notifications::{self, Access, Batch, Toast, Watch};
use crate::platform::win;
use crate::ui::theme::{Rgba, FG, MUTED};
use crate::widgets::island_ui as iu;
use crate::widgets::{alert_hash, Ctx, Env, IslandForm, IslandItem, Kind, MenuItem, Mode, Rect4, Request};

pub const KEY: &str = "notificacoes_apps";
/// "Mostrar o texto das mensagens" (em `cfg[KEY]`); desligado, só o app e quem mandou
pub const TEXT_FIELD: &str = "texto";
/// os apps silenciados (a identidade de cada um no Windows), em `cfg[KEY]`
pub const MUTED_FIELD: &str = "silenciados";
/// os apps já vistos na central (identidade → nome), em `cache[KEY]`, para a lista das
/// Configurações
pub const SEEN_FIELD: &str = "apps";
/// pelo clique direito na ilha: não mostrar mais as notificações deste app
pub const MUTE: u32 = 3;

/// uma notificação fica na ilha (como bolha, depois de esticar) por este tempo
const KEEP_FOR: Duration = Duration::from_secs(3 * 60);
/// notificações mais velhas que isto, quando aparecem na central, não entram (o Windows às vezes
/// devolve as antigas de uma vez)
const FRESH: f64 = 2.0 * 60.0;
/// no máximo estas esperando na ilha (a mais nova na frente)
const MAX_PENDING: usize = 5;
/// de quanto em quanto tempo ver se chegou algo do fio (só olhar o canal: barato)
const CHECK_EVERY: Duration = Duration::from_millis(500);
/// sem a permissão, de quanto em quanto tempo conferir de novo (a pessoa pode ter liberado)
const ACCESS_EVERY: Duration = Duration::from_secs(20);
/// abaixo desta opacidade (0 a 255), o pixel do ícone conta como margem transparente
const LOGO_EDGE_ALPHA: u8 = 16;
/// o tamanho do ícone do app (em pontos) em cada forma da ilha: a bolha (de 42), a pílula
/// fechada e esticada (de 42 de altura), a média e a aberta
const LOGO_BUBBLE: f32 = 26.0;
const LOGO_PILL: f32 = 24.0;
const LOGO_MEDIUM: f32 = 42.0;
const LOGO_LARGE: f32 = 44.0;
/// a cor de quem não está na lista de cores conhecidas
const PALETTE: [Rgba; 6] = [Rgba::rgb(10, 132, 255), Rgba::rgb(191, 90, 242), Rgba::rgb(255, 159, 10), Rgba::rgb(48, 209, 88), Rgba::rgb(255, 69, 58), Rgba::rgb(100, 210, 255)];

/// Uma notificação esperando na ilha, e quando ela chegou.
#[derive(Clone, Debug)]
struct Pending {
    toast: Toast,
    arrived: Instant,
}

pub struct AppNotices {
    watch: Option<Watch>,
    next_access: Instant,
    next_check: Instant,
    /// as notificações que já estavam na central (não são novas); a primeira leitura só anota
    known: HashSet<u32>,
    baseline: bool,
    /// as que estão na ilha, a mais nova primeiro
    pending: Vec<Pending>,
    /// o ícone de cada app (pela identidade dele)
    logos: HashMap<String, Arc<Image>>,
    /// o nome deste app no Windows (as notificações dele mesmo não entram)
    own_exe: String,
}

impl Default for AppNotices {
    fn default() -> AppNotices {
        AppNotices::new()
    }
}

/// Ligado nas Configurações (desligado até a pessoa ligar).
pub fn enabled(cfg: &Value) -> bool {
    config::setting_bool(cfg, "ilha_widgets") && cfg[KEY].bool_of("na_ilha").unwrap_or(false)
}

/// Mostrar o texto das mensagens (ligado até a pessoa desligar).
pub fn shows_text(cfg: &Value) -> bool {
    cfg[KEY].bool_of(TEXT_FIELD).unwrap_or(true)
}

/// Os apps silenciados.
pub fn muted(cfg: &Value) -> Vec<String> {
    cfg[KEY][MUTED_FIELD].as_array().map(|list| list.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// Silencia o app (ou volta a mostrar, se já estava silenciado).
pub fn toggle_muted(cfg: &mut Value, app_id: &str) {
    let mut list = muted(cfg);
    match list.iter().position(|id| id == app_id) {
        Some(at) => {
            list.remove(at);
        }
        None => list.push(app_id.to_string()),
    }
    cfg.obj_mut(KEY).insert(MUTED_FIELD.into(), json!(list));
}

/// Os apps já vistos na central, do mais recente ao mais antigo: (identidade, nome).
pub fn seen_apps(cache: &Value) -> Vec<(String, String)> {
    cache[KEY][SEEN_FIELD].as_object().map(|apps| apps.iter().rev().map(|(id, name)| (id.clone(), name.as_str().unwrap_or("").to_string())).collect()).unwrap_or_default()
}

/// A cor do app na ilha: a dele, para os conhecidos; para os outros, uma da paleta pelo nome.
fn app_color(app: &str) -> Rgba {
    let name = app.to_lowercase();
    match () {
        _ if name.contains("whatsapp") => Rgba::rgb(37, 211, 102),
        _ if name.contains("teams") => Rgba::rgb(123, 131, 235),
        _ if name.contains("outlook") || name.contains("mail") => Rgba::rgb(40, 146, 235),
        _ if name.contains("discord") => Rgba::rgb(88, 101, 242),
        _ if name.contains("telegram") => Rgba::rgb(42, 171, 238),
        _ if name.contains("slack") => Rgba::rgb(236, 72, 132),
        _ => PALETTE[(alert_hash(&name) % PALETTE.len() as u64) as usize],
    }
}

/// O ícone sem a margem transparente em volta: o Windows entrega muitos ícones com o desenho
/// ocupando só o meio da imagem, e na ilha eles pareciam minúsculos. Corta um quadrado em volta
/// do desenho (sem deformar); todo transparente, fica como está.
fn trim_margin(image: &Image) -> Image {
    let (w, h) = (image.w as usize, image.h as usize);
    let alpha = |x: usize, y: usize| image.px[(y * w + x) * 4 + 3];
    let (mut left, mut top, mut right, mut bottom) = (w, h, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            if alpha(x, y) > LOGO_EDGE_ALPHA {
                left = left.min(x);
                right = right.max(x);
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }
    if left > right || top > bottom {
        return image.clone();
    }
    // um quadrado com o lado do maior dos dois, centrado no desenho e dentro da imagem
    let side = (right - left + 1).max(bottom - top + 1).min(w).min(h);
    let cx = (left + right + 1) / 2;
    let cy = (top + bottom + 1) / 2;
    let x0 = cx.saturating_sub(side / 2).min(w - side);
    let y0 = cy.saturating_sub(side / 2).min(h - side);
    let mut px = Vec::with_capacity(side * side * 4);
    for y in y0..y0 + side {
        px.extend_from_slice(&image.px[(y * w + x0) * 4..(y * w + x0 + side) * 4]);
    }
    Image { w: side as u32, h: side as u32, px }
}

/// "agora", "há 3 min"
fn age_text(secs: f64) -> String {
    let minutes = (secs / 60.0).floor() as i64;
    if minutes < 1 { "agora".into() } else { format!("há {minutes} min") }
}

impl AppNotices {
    pub fn new() -> AppNotices {
        let own_exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase())).unwrap_or_default();
        AppNotices {
            watch: None,
            next_access: Instant::now(),
            next_check: Instant::now(),
            known: HashSet::new(),
            baseline: false,
            pending: Vec::new(),
            logos: HashMap::new(),
            own_exe,
        }
    }

    /// Com `toasts` esperando na ilha (a primeira é a mais nova).
    #[cfg(test)]
    pub fn sample(toasts: &[(&str, &str, &str)]) -> AppNotices {
        let mut notices = AppNotices::new();
        let now = win::now();
        notices.pending = toasts
            .iter()
            .enumerate()
            .map(|(i, (app, title, body))| Pending {
                toast: Toast { id: i as u32 + 1, app: app.to_string(), app_id: format!("{app}!App"), title: title.to_string(), body: body.to_string(), at: now - 30.0 * i as f64 },
                arrived: Instant::now(),
            })
            .collect();
        // o primeiro com um ícone de verdade: um círculo no meio de uma margem transparente
        // larga, como os que o Windows entrega
        if let Some(first) = notices.pending.first() {
            let logo = sample_logo(128, 40.0, app_color(&first.toast.app));
            notices.logos.insert(first.toast.app_id.clone(), Arc::new(trim_margin(&logo)));
        }
        notices
    }

    /// A notificação é deste próprio app (os avisos dos widgets já aparecem do jeito deles)?
    fn own(&self, toast: &Toast) -> bool {
        (!self.own_exe.is_empty() && toast.app_id.to_lowercase().contains(&self.own_exe)) || toast.app == "Widgets"
    }

    /// Uma leitura nova da central: as que não estavam lá entram na ilha (menos as velhas, as
    /// silenciadas e as deste app); as que sumiram de lá (dispensadas no Windows) saem daqui. A
    /// primeira leitura só anota o que já estava. Devolve true se a ilha muda.
    fn apply(&mut self, batch: Batch, cfg: &Value, cache: &mut Value, now: f64) -> bool {
        for (app_id, bytes) in batch.logos {
            if let Some(image) = gfx::decode_image(&bytes) {
                self.logos.insert(app_id, Arc::new(trim_margin(&image)));
            }
        }
        // os apps que aparecem na central entram na lista das Configurações (para silenciar)
        for toast in &batch.toasts {
            if !toast.app_id.is_empty() && !self.own(toast) && cache[KEY][SEEN_FIELD].get(&toast.app_id).is_none() {
                cache[KEY][SEEN_FIELD][&toast.app_id] = json!(toast.app);
            }
        }
        let ids: HashSet<u32> = batch.toasts.iter().map(|t| t.id).collect();
        let before = self.pending.len();
        let mut changed = false;
        if self.baseline {
            let silenced = muted(cfg);
            let mut fresh: Vec<Toast> = batch
                .toasts
                .into_iter()
                .filter(|t| !self.known.contains(&t.id) && now - t.at < FRESH && !silenced.contains(&t.app_id) && !self.own(t))
                .collect();
            fresh.sort_by(|a, b| b.at.total_cmp(&a.at));
            changed |= !fresh.is_empty();
            let arrived = Instant::now();
            let mut list: Vec<Pending> = fresh.into_iter().map(|toast| Pending { toast, arrived }).collect();
            list.append(&mut self.pending);
            self.pending = list;
        }
        self.baseline = true;
        self.known = ids.clone();
        self.pending.retain(|p| ids.contains(&p.toast.id));
        self.pending.truncate(MAX_PENDING);
        changed | (self.pending.len() != before)
    }

    /// A mais nova esperando.
    fn newest(&self) -> Option<&Toast> {
        self.pending.first().map(|p| &p.toast)
    }

    /// O título e o corpo como a ilha mostra: com o texto escondido, o app e quem mandou.
    fn texts(&self, toast: &Toast, cfg: &Value) -> (String, String) {
        let title = if toast.title.is_empty() { toast.app.clone() } else { toast.title.clone() };
        let others = self.pending.len().saturating_sub(1);
        let more = match others {
            0 => String::new(),
            1 => " · +1 notificação".into(),
            n => format!(" · +{n} notificações"),
        };
        let body = if shows_text(cfg) && !toast.body.is_empty() { toast.body.clone() } else { format!("{}{more}", if toast.app.is_empty() { "Nova notificação" } else { &toast.app }) };
        (title, body)
    }

    /// As outras esperando, pela ilha aberta: "+1 do Teams", "+3 notificações".
    fn others_line(&self) -> Option<String> {
        let others = &self.pending.get(1..)?;
        let first = others.first()?;
        let same_app = others.iter().all(|p| p.toast.app == first.toast.app) && !first.toast.app.is_empty();
        Some(match (others.len(), same_app) {
            (n, true) => format!("+{n} do {}", first.toast.app),
            (1, false) => "+1 notificação".into(),
            (n, false) => format!("+{n} notificações"),
        })
    }

    /// O ícone do app (ou, sem ele, a inicial num círculo da cor do app).
    fn draw_logo(&self, c: &mut Canvas, toast: &Toast, cx: f32, cy: f32, size: f32) {
        match self.logos.get(&toast.app_id) {
            Some(logo) => c.image_px(logo, ((cx - size / 2.0) * c.s).round(), ((cy - size / 2.0) * c.s).round(), (size * c.s).round(), (size * c.s).round(), 1.0),
            None => {
                let color = app_color(&toast.app);
                c.fill_circle(cx, cy, size / 2.0, color.alpha(0.22));
                let initial: String = toast.app.chars().next().map(|ch| ch.to_uppercase().collect()).unwrap_or_else(|| "•".into());
                c.text(cx, cy, &initial, Face::Semibold, size * 0.46, color, Anchor::MM, None);
            }
        }
    }
}

impl Kind for AppNotices {
    fn key(&self) -> &'static str {
        KEY
    }
    fn title(&self) -> &'static str {
        "Notificações dos apps"
    }
    fn draw(&self, _c: &mut Canvas, _mode: Mode, _env: &Env) {}

    /// Sempre um horário guardado, nunca "agora + algo": o app compara com o agora dele, tirado
    /// um instante antes, e um horário calculado na hora ficaria sempre no futuro (a fonte nunca
    /// rodaria).
    fn next_tick(&self) -> Option<Instant> {
        let mut next = if self.watch.is_some() { self.next_check } else { self.next_access };
        if let Some(oldest) = self.pending.iter().map(|p| p.arrived).min() {
            next = next.min(oldest + KEEP_FOR);
        }
        Some(next)
    }

    /// Liga ou desliga o fio conforme as opções e a permissão, recebe o que ele leu e tira da
    /// ilha as que ficaram tempo demais.
    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        let now = Instant::now();
        let mut changed = false;
        if !enabled(ctx.cfg) {
            // desligada: só confere de novo daqui a pouco (ligando nas Configurações, começa logo)
            self.next_access = now + CHECK_EVERY;
            changed = !self.pending.is_empty();
            self.watch = None;
            self.pending.clear();
            self.baseline = false;
            return changed;
        }
        if self.watch.is_none() && now >= self.next_access {
            self.next_access = now + ACCESS_EVERY;
            if notifications::access() == Access::Allowed {
                self.watch = notifications::watch();
                self.baseline = false;
            }
        }
        if now >= self.next_check {
            self.next_check = now + CHECK_EVERY;
            let mut latest = None;
            if let Some(watch) = &self.watch {
                while let Ok(batch) = watch.rx.try_recv() {
                    // os ícones de cada leitura valem (a lista, só a última)
                    latest = Some(match latest {
                        Some(Batch { logos: mut old_logos, .. }) => {
                            let mut batch = batch;
                            old_logos.append(&mut batch.logos);
                            Batch { toasts: batch.toasts, logos: old_logos }
                        }
                        None => batch,
                    });
                }
            }
            if let Some(batch) = latest {
                let had = ctx.cache[KEY][SEEN_FIELD].as_object().map_or(0, |apps| apps.len());
                changed |= self.apply(batch, ctx.cfg, ctx.cache, win::now());
                if ctx.cache[KEY][SEEN_FIELD].as_object().map_or(0, |apps| apps.len()) != had {
                    ctx.out.push(Request::SaveCache);
                }
            }
        }
        let before = self.pending.len();
        self.pending.retain(|p| now.saturating_duration_since(p.arrived) < KEEP_FOR);
        changed || self.pending.len() != before
    }

    fn island_item(&self, env: &Env) -> Option<IslandItem> {
        if !enabled(env.cfg) {
            return None;
        }
        let toast = self.newest()?;
        Some(IslandItem::alert(app_color(&toast.app), alert_hash(&format!("notificacao|{}|{}", toast.app_id, toast.id))))
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, env: &Env) {
        let Some(toast) = self.newest() else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let color = app_color(&toast.app);
        let (title, body) = self.texts(toast, env.cfg);
        match form {
            IslandForm::Bubble => self.draw_logo(c, toast, x + w / 2.0, cy, LOGO_BUBBLE),
            IslandForm::Compact | IslandForm::Banner => {
                self.draw_logo(c, toast, x + 26.0, cy, LOGO_PILL);
                // as outras esperando: "+2", forte à direita
                let others = self.pending.len().saturating_sub(1);
                let count = if others > 0 { format!("+{others}") } else { String::new() };
                let count_w = if count.is_empty() { 0.0 } else { c.text(x + w - 16.0, cy, &count, Face::Semibold, 14.5, color, Anchor::RM, None) + 10.0 };
                let text_x = x + 26.0 + LOGO_PILL / 2.0 + 9.0;
                let room = x + w - 16.0 - count_w - text_x;
                // quem mandou, em branco (esticada: com a mensagem; não cabendo, com o app)
                let (joined, with_app) = (format!("{title}: {body}"), format!("{title} · {}", toast.app));
                let options: Vec<&str> = if form == IslandForm::Banner && !toast.app.is_empty() { vec![&joined, &with_app, &title] } else { vec![&title] };
                let label = iu::fitting(c, &options, Face::Semibold, 13.5, room);
                c.text(text_x, cy, label, Face::Semibold, 13.5, FG, Anchor::LM, Some(room));
            }
            IslandForm::Medium => {
                // quem mandou e até duas linhas da mensagem
                self.draw_logo(c, toast, x + 41.0, cy, LOGO_MEDIUM);
                let text_x = x + 41.0 + LOGO_MEDIUM / 2.0 + 12.0;
                let room = x + w - 86.0 - text_x;
                let lines = c.wrap(&body, Face::Regular, 13.0, room, 2);
                let top = cy - 9.0 * lines.len() as f32;
                c.text(text_x, top, &title, Face::Semibold, 15.0, FG, Anchor::LM, Some(room));
                for (n, line) in lines.iter().enumerate() {
                    c.text(text_x, top + 19.0 + n as f32 * 17.0, line, Face::Regular, 13.0, MUTED, Anchor::LM, Some(room));
                }
                draw_buttons(c, form, r, hot, None);
            }
            IslandForm::Large => {
                let app = if toast.app.is_empty() { "NOTIFICAÇÃO".to_string() } else { toast.app.to_uppercase() };
                iu::header(c, r, &app, color, &age_text(win::now() - toast.at), MUTED);
                self.draw_logo(c, toast, x + 22.0 + LOGO_LARGE / 2.0, y + 72.0, LOGO_LARGE);
                let text_x = x + 22.0 + LOGO_LARGE + 14.0;
                c.text(text_x, y + 72.0, &title, Face::Semibold, 18.0, FG, Anchor::LM, Some(x + w - 22.0 - text_x));
                let lines = c.wrap(&body, Face::Regular, 14.0, w - 44.0, 3);
                for (n, line) in lines.iter().enumerate() {
                    c.text(x + 22.0, y + 116.0 + n as f32 * 20.0, line, Face::Regular, 14.0, MUTED, Anchor::LM, Some(w - 44.0));
                }
                // as outras esperando, embaixo à esquerda (os botões ficam à direita)
                if let Some(others) = self.others_line() {
                    c.text(x + 22.0, y + h - 30.0, &others, Face::Regular, 11.5, MUTED, Anchor::LM, Some(w - 186.0 - 32.0));
                }
                draw_buttons(c, form, r, hot, Some("Abrir"));
            }
        }
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        hit(form, x, y, w, h, true)
    }

    /// OK: tira todas da ilha (na central do Windows continuam). Abrir: abre o app da mais nova.
    /// Silenciar: não mostra mais as deste app.
    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            OK => self.pending.clear(),
            OPEN => {
                if let Some(toast) = self.newest().cloned() {
                    let target = if toast.app_id.is_empty() { "ms-actioncenter:".to_string() } else { format!("shell:AppsFolder\\{}", toast.app_id) };
                    ctx.open(target);
                    self.pending.retain(|p| p.toast.app_id != toast.app_id);
                }
            }
            MUTE => {
                if let Some(toast) = self.newest().cloned() {
                    toggle_muted(ctx.cfg, &toast.app_id);
                    ctx.out.push(Request::SaveConfig);
                    self.pending.retain(|p| p.toast.app_id != toast.app_id);
                }
            }
            _ => {}
        }
    }

    /// No clique direito da ilha: silenciar o app da notificação.
    fn menu(&self, _env: &Env) -> Vec<MenuItem> {
        match self.newest() {
            Some(toast) if !toast.app_id.is_empty() => {
                let app = if toast.app.is_empty() { "deste app".to_string() } else { format!("do {}", toast.app) };
                vec![MenuItem::Cmd(format!("Não mostrar notificações {app}"), MUTE)]
            }
            _ => Vec::new(),
        }
    }
}

/// Um ícone de exemplo: um círculo de raio `radius` no meio de uma imagem de `size` pixels, o
/// resto transparente.
#[cfg(test)]
fn sample_logo(size: u32, radius: f32, color: Rgba) -> Image {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let center = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            // a borda com meio pixel de suavização, como num ícone de verdade
            let coverage = (radius + 0.5 - (x as f32 + 0.5 - center).hypot(y as f32 + 0.5 - center)).clamp(0.0, 1.0);
            if coverage > 0.0 {
                let i = ((y * size + x) * 4) as usize;
                let a = (coverage * 255.0).round() as u8;
                let pre = |v: u8| (v as f32 * coverage).round() as u8;
                px[i..i + 4].copy_from_slice(&[pre(color.2), pre(color.1), pre(color.0), a]);
            }
        }
    }
    Image { w: size, h: size, px }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A margem transparente em volta do ícone sai (o desenho ocupa a imagem toda), sem deformar.
    #[test]
    fn the_logo_loses_its_empty_margin() {
        let logo = sample_logo(64, 18.0, Rgba::rgb(37, 211, 102));
        let trimmed = trim_margin(&logo);
        assert_eq!((trimmed.w, trimmed.h), (36, 36), "o círculo de 36 px, sem a margem (a borda suave mais fraca conta como margem)");
        assert_eq!(trimmed.px.len(), 36 * 36 * 4);
        // o meio continua sendo o círculo
        let center = ((18 * 36 + 18) * 4 + 3) as usize;
        assert_eq!(trimmed.px[center], 255);
        // todo transparente: fica como está
        let empty = Image { w: 8, h: 8, px: vec![0; 8 * 8 * 4] };
        assert_eq!(trim_margin(&empty).w, 8);
        // já sem margem: igual
        let full = sample_logo(10, 20.0, Rgba::rgb(255, 0, 0));
        assert_eq!((trim_margin(&full).w, trim_margin(&full).h), (10, 10));
    }

    fn toast(id: u32, app: &str, at: f64) -> Toast {
        Toast { id, app: app.into(), app_id: format!("{app}!App"), title: "Maria".into(), body: "chego em 10 min".into(), at }
    }

    fn batch(toasts: Vec<Toast>) -> Batch {
        Batch { toasts, logos: Vec::new() }
    }

    #[test]
    fn only_new_notifications_reach_the_island() {
        let cfg = json!({"app": {}, KEY: {"na_ilha": true}});
        let mut cache = json!({});
        let now = 1_800_000_000.0;
        let mut n = AppNotices::new();
        // a primeira leitura só anota o que já estava na central
        assert!(!n.apply(batch(vec![toast(1, "WhatsApp", now - 10.0)]), &cfg, &mut cache, now));
        assert!(n.newest().is_none());
        assert_eq!(seen_apps(&cache), [("WhatsApp!App".to_string(), "WhatsApp".to_string())], "o app entra na lista das Configurações");
        // chegou uma nova: na ilha
        assert!(n.apply(batch(vec![toast(1, "WhatsApp", now - 10.0), toast(2, "Teams", now - 1.0)]), &cfg, &mut cache, now));
        assert_eq!(n.newest().map(|t| t.id), Some(2));
        let env = Env { cfg: &cfg, key: KEY };
        assert!(n.island_item(&env).is_some_and(|item| item.alert));
        // uma velha que apareceu agora (o Windows devolvendo antigas) não entra
        n.apply(batch(vec![toast(1, "WhatsApp", now - 10.0), toast(2, "Teams", now - 1.0), toast(3, "Outlook", now - 600.0)]), &cfg, &mut cache, now);
        assert_eq!(n.pending.len(), 1);
        // dispensada na central do Windows: sai da ilha
        n.apply(batch(vec![toast(1, "WhatsApp", now - 10.0)]), &cfg, &mut cache, now);
        assert!(n.newest().is_none());
    }

    /// O próximo tick é um horário guardado: o app tira o "agora" antes de perguntar, e um
    /// horário calculado na hora ficava sempre no futuro (a fonte nunca rodava e nenhuma
    /// notificação chegava à ilha).
    #[test]
    fn the_next_tick_is_a_fixed_time_that_comes() {
        let (mut cfg, mut cache, mut out) = (json!({"app": {}}), json!({}), Vec::new());
        let mut n = AppNotices::new();
        let mut ctx = Ctx { cfg: &mut cfg, cache: &mut cache, key: KEY, out: &mut out };
        n.tick(&mut ctx);
        let first = n.next_tick().unwrap();
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(n.next_tick(), Some(first), "não anda com o relógio");
        // como o app faz: o agora tirado antes; passado o horário, a fonte roda
        n.next_access = Instant::now() - Duration::from_millis(1);
        let now = Instant::now();
        assert!(n.next_tick().is_some_and(|t| now >= t));
        // ligada e vigiando: o próximo é o da conferida do canal
        n.watch = notifications::watch();
        n.next_check = now - Duration::from_millis(1);
        assert!(n.next_tick().is_some_and(|t| now >= t));
    }

    #[test]
    fn muted_apps_and_this_app_stay_out() {
        let mut cfg = json!({"app": {}, KEY: {"na_ilha": true}});
        toggle_muted(&mut cfg, "Teams!App");
        assert_eq!(muted(&cfg), ["Teams!App"]);
        let mut cache = json!({});
        let now = 1_800_000_000.0;
        let mut n = AppNotices::new();
        n.apply(batch(Vec::new()), &cfg, &mut cache, now);
        let mut own = toast(3, "Widgets", now);
        own.app_id = "C:\\Widgets\\Widgets.exe".into();
        n.apply(batch(vec![toast(1, "Teams", now), toast(2, "WhatsApp", now), own]), &cfg, &mut cache, now);
        assert_eq!(n.pending.iter().map(|p| p.toast.id).collect::<Vec<_>>(), [2]);
        toggle_muted(&mut cfg, "Teams!App");
        assert!(muted(&cfg).is_empty(), "de novo: volta a mostrar");
    }

    #[test]
    fn off_until_turned_on_and_text_can_be_hidden() {
        let cfg = json!({"app": {}});
        assert!(!enabled(&cfg), "desligado até a pessoa ligar");
        let n = AppNotices::sample(&[("WhatsApp", "Maria", "chego em 10 min"), ("Teams", "Reunião", "começa às 15h")]);
        assert!(n.island_item(&Env { cfg: &cfg, key: KEY }).is_none());
        let on = json!({"app": {}, KEY: {"na_ilha": true}});
        let toast = n.newest().unwrap().clone();
        assert_eq!(n.texts(&toast, &on), ("Maria".into(), "chego em 10 min".into()));
        let hidden = json!({"app": {}, KEY: {"na_ilha": true, TEXT_FIELD: false}});
        assert_eq!(n.texts(&toast, &hidden), ("Maria".into(), "WhatsApp · +1 notificação".into()), "sem o texto: o app e quantas mais");
    }

    #[test]
    fn ok_open_and_mute() {
        let mut n = AppNotices::sample(&[("WhatsApp", "Maria", "oi"), ("Teams", "Reunião", "15h")]);
        let (mut cfg, mut cache, mut out) = (json!({"app": {}, KEY: {"na_ilha": true}}), json!({}), Vec::new());
        let mut ctx = Ctx { cfg: &mut cfg, cache: &mut cache, key: KEY, out: &mut out };
        n.act(OPEN, &mut ctx);
        assert_eq!(n.pending.len(), 1, "abriu o WhatsApp: a dele sai");
        n.act(MUTE, &mut ctx);
        assert!(n.pending.is_empty());
        assert_eq!(muted(&cfg), ["Teams!App"]);
        assert!(matches!(out.first(), Some(Request::Open(target)) if target == "shell:AppsFolder\\WhatsApp!App"));
    }
}
