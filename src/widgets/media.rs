//! Widget "Tocando agora": a mídia em reprodução no Windows (Spotify, YouTube no navegador...),
//! com capa, barra de progresso e controles.
//!
//! A central de mídia do Windows (a mesma do controle de volume) é lida numa thread própria; o
//! resultado fica num "retrato" que a interface consulta 4 vezes por segundo.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use windows::core::Interface;
use windows_future::IAsyncOperation;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows::Storage::Streams::DataReader;

use super::{Ctx, Env, Hot, Kind, Layout, MenuItem, Mode};
use crate::config::Get;
use crate::drawing;
use crate::gfx::{self, Anchor, Canvas, Face, Image};
use crate::theme::*;
use crate::win;

const BROWSERS: [&str; 7] = ["chrome", "msedge", "edge", "brave", "firefox", "opera", "vivaldi"];
const SOURCES: [(&str, &str); 3] = [("auto", "Automático (o que estiver tocando)"), ("spotify", "Spotify"), ("youtube", "YouTube (navegador)")];
const FUNNY: [(&str, &str); 10] = [
    ("Silêncio total…", "O DJ saiu pra tomar um café"),
    ("Nada tocando", "Seus fones estão com saudade de você"),
    ("Palco vazio", "Bora dar play em alguma coisa?"),
    ("Shhh…", "A playlist está tirando um cochilo"),
    ("Modo biblioteca", "Só dá pra ouvir o ventilador do PC"),
    ("Cadê a trilha sonora?", "Todo momento merece uma"),
    ("A música parou", "Mas a vibe continua"),
    ("Tudo quieto por aqui", "O vizinho agradece"),
    ("Intervalo", "Até os músicos precisam descansar"),
    ("Sem som no momento", "Que tal aquela música que gruda?"),
];
const IDLE_FPS: u32 = 15;
const IDLE_FRAMES: usize = 60; // 4 segundos em loop

fn source_label(kind: &str) -> &'static str {
    match kind {
        "spotify" => "SPOTIFY",
        "youtube" => "YOUTUBE",
        _ => "MÍDIA",
    }
}

fn source_color(kind: &str) -> Rgba {
    match kind {
        "spotify" => Rgba::rgb(30, 215, 96),
        "youtube" => Rgba::rgb(255, 72, 72),
        _ => MUTED,
    }
}

fn idle_gradient(pref: &str) -> (Rgba, Rgba) {
    match pref {
        "spotify" => (Rgba::rgb(30, 215, 96), Rgba::rgb(20, 170, 200)),
        "youtube" => (Rgba::rgb(255, 72, 72), Rgba::rgb(255, 150, 60)),
        _ => (Rgba::rgb(10, 132, 255), Rgba::rgb(191, 90, 242)),
    }
}

fn media_kind(app_id: &str) -> &'static str {
    let a = app_id.to_lowercase();
    if a.contains("spotify") {
        "spotify"
    } else if BROWSERS.iter().any(|b| a.contains(b)) {
        "youtube"
    } else {
        "outro"
    }
}

/// Remove emojis (a fonte Segoe UI não tem, e eles virariam quadradinhos).
fn clean_text(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter(|&c| (c as u32) <= 0xFFFF && !(0x2600..=0x27BF).contains(&(c as u32)) && c != '\u{fe0f}' && c != '\u{200d}')
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn fmt_time(seconds: f64) -> String {
    let s = seconds.max(0.0) as i64;
    let (h, m, sec) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{sec:02}") } else { format!("{m}:{sec:02}") }
}

/// O que está tocando agora (o "retrato" lido pela thread de mídia).
#[derive(Clone, Default)]
pub struct Snapshot {
    pub empty: bool,
    pub pref: String,
    pub kind: &'static str,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    /// quando a posição foi medida (segundos desde 1970)
    pub updated: Option<f64>,
    pub can_toggle: bool,
    pub can_prev: bool,
    pub can_next: bool,
    pub art: Option<Arc<Image>>,
    pub art_key: Option<String>,
    pub error: Option<String>,
}

enum Cmd {
    Toggle,
    Next,
    Prev,
    Wake,
}

// ---------- thread da central de mídia ----------

fn ticks_to_secs(ticks: i64) -> f64 {
    ticks as f64 / 10_000_000.0
}

fn choose(manager: &Manager, pref: &str) -> windows::core::Result<Option<Session>> {
    let mut sessions: Vec<Session> = manager.GetSessions()?.into_iter().collect();
    if pref != "auto" {
        sessions.retain(|s| s.SourceAppUserModelId().map(|id| media_kind(&id.to_string()) == pref).unwrap_or(false));
    }
    if sessions.is_empty() {
        return Ok(None);
    }
    let playing = |s: &Session| s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).map(|st| st == PlaybackStatus::Playing).unwrap_or(false);
    if let Some(s) = sessions.iter().find(|s| playing(s)) {
        return Ok(Some(s.clone()));
    }
    if let Ok(current) = manager.GetCurrentSession() {
        let id = current.SourceAppUserModelId().unwrap_or_default();
        if sessions.iter().any(|s| s.SourceAppUserModelId().unwrap_or_default() == id) {
            return Ok(Some(current));
        }
    }
    Ok(Some(sessions[0].clone()))
}

fn read_art(props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties) -> Option<Image> {
    let thumb = props.Thumbnail().ok()?;
    let stream = thumb.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let op: IAsyncOperation<u32> = reader.LoadAsync(size).ok()?.cast().ok()?;
    op.join().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    gfx::decode_image(&buf)
}

struct Reader {
    art_key: Option<String>,
    art: Option<Arc<Image>>,
}

impl Reader {
    fn read(&mut self, manager: &Manager, pref: &str, current: &mut Option<Session>) -> windows::core::Result<Snapshot> {
        let session = choose(manager, pref)?;
        *current = session.clone();
        let Some(session) = session else {
            return Ok(Snapshot { empty: true, pref: pref.into(), ..Default::default() });
        };
        let app_id = session.SourceAppUserModelId()?.to_string();
        let info = session.GetPlaybackInfo()?;
        let timeline = session.GetTimelineProperties()?;
        let props = session.TryGetMediaPropertiesAsync()?.join()?;
        let title = clean_text(&props.Title()?.to_string());
        let artist = clean_text(&props.Artist()?.to_string());
        let key = format!("{app_id}\u{1}{title}\u{1}{artist}");
        if self.art_key.as_deref() != Some(key.as_str()) || self.art.is_none() {
            self.art_key = Some(key.clone());
            self.art = read_art(&props).map(Arc::new);
        }
        let controls = info.Controls()?;
        let updated = timeline.LastUpdatedTime()?.UniversalTime;
        // DateTime conta de 100 em 100 ns desde 1601; antes de 2001 é "sem horário"
        let updated = (updated > 126_227_808_000_000_000).then(|| (updated - 116_444_736_000_000_000) as f64 / 10_000_000.0);
        let duration = ticks_to_secs(timeline.EndTime()?.Duration - timeline.StartTime()?.Duration);
        Ok(Snapshot {
            empty: false,
            pref: pref.into(),
            kind: media_kind(&app_id),
            title: if title.is_empty() { "Sem título".into() } else { title },
            artist,
            playing: info.PlaybackStatus()? == PlaybackStatus::Playing,
            position: ticks_to_secs(timeline.Position()?.Duration),
            duration: duration.max(0.0),
            updated,
            can_toggle: controls.IsPlayPauseToggleEnabled()?,
            can_prev: controls.IsPreviousEnabled()?,
            can_next: controls.IsNextEnabled()?,
            art: self.art.clone(),
            art_key: self.art.as_ref().map(|_| key),
            error: None,
        })
    }
}

fn watch(rx: std::sync::mpsc::Receiver<Cmd>, snapshot: Arc<Mutex<Option<Snapshot>>>, pref: Arc<Mutex<String>>) {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let mut manager: Option<Manager> = None;
    let mut current: Option<Session> = None;
    let mut reader = Reader { art_key: None, art: None };
    loop {
        let p = pref.lock().unwrap().clone();
        let result = (|| {
            if manager.is_none() {
                manager = Some(Manager::RequestAsync()?.join()?);
            }
            reader.read(manager.as_ref().unwrap(), &p, &mut current)
        })();
        let snap = result.unwrap_or_else(|_| {
            // a sessão pode sumir no meio da leitura; tenta de novo
            manager = None;
            current = None;
            Snapshot { empty: true, pref: p.clone(), ..Default::default() }
        });
        *snapshot.lock().unwrap() = Some(snap);
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Cmd::Wake) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
            Ok(cmd) => {
                if let Some(s) = &current {
                    let op = match cmd {
                        Cmd::Toggle => s.TryTogglePlayPauseAsync(),
                        Cmd::Next => s.TrySkipNextAsync(),
                        _ => s.TrySkipPreviousAsync(),
                    };
                    if let Ok(op) = op {
                        let _ = op.join();
                    }
                    std::thread::sleep(Duration::from_millis(150)); // dá tempo do player atualizar o estado
                }
            }
        }
    }
}

// ---------- o widget ----------

const W: f32 = 320.0;
const H: f32 = 196.0;
const RADIUS: f32 = 26.0;
const ART: f32 = 76.0;
const ART_RADIUS: f32 = 14.0;
const ART_XY: (f32, f32) = (18.0, 18.0);
const TEXT_X: f32 = 18.0 + 76.0 + 14.0;
const TEXT_W: f32 = W - TEXT_X - 18.0;
const BAR_Y: f32 = 114.0;
const CTRL_Y: f32 = 162.0;
const PLAY_D: f32 = 44.0;
const IDLE_BOX: (f32, f32, f32, f32) = (18.0, 110.0, W - 36.0, 70.0); // x, y, largura, altura do equalizador

const COMPACT_H: f32 = 64.0;
const COMPACT_RADIUS: f32 = 22.0;
const C_ART: f32 = 40.0;
const C_ART_RADIUS: f32 = 9.0;
const C_ART_XY: (f32, f32) = (12.0, 12.0);
const C_TEXT_X: f32 = 12.0 + 40.0 + 10.0;
const C_TEXT_W: f32 = W - C_TEXT_X - 92.0;
const C_PLAY_D: f32 = 32.0;
const C_IDLE_BOX: (f32, f32, f32, f32) = (20.0, 20.0, 24.0, 24.0); // mini equalizador no lugar da capa

// médio: capa, título e artista, controles e uma barra de progresso fina
const MEDIUM_H: f32 = 102.0;
const M_RADIUS: f32 = 24.0;
const M_ART: f32 = 74.0;
const M_ART_RADIUS: f32 = 12.0;
const M_ART_XY: (f32, f32) = (14.0, 14.0);
const M_TEXT_X: f32 = 14.0 + 74.0 + 14.0;
const M_TEXT_W: f32 = W - M_TEXT_X - 16.0;
const M_CTRL_Y: f32 = 66.0;
const M_PLAY_D: f32 = 32.0;
const M_BAR_Y: f32 = 92.0;
const M_IDLE_BOX: (f32, f32, f32, f32) = (14.0 + 17.0, 14.0 + 17.0, 40.0, 40.0);

const POLL: Duration = Duration::from_millis(250);
const MESSAGE_EVERY: Duration = Duration::from_secs(12);

// ações
const TOGGLE: u32 = 1;
const NEXT: u32 = 2;
const PREV: u32 = 3;
const OPEN_PLAYER: u32 = 4;
const SOURCE: u32 = 10; // + índice em SOURCES

#[derive(Clone, Copy, PartialEq)]
enum Ctl {
    Prev,
    Toggle(bool), // true = tocando (mostra pausa)
    Next,
}

pub struct Media {
    tx: Sender<Cmd>,
    shared: Arc<Mutex<Option<Snapshot>>>,
    pref: Arc<Mutex<String>>,
    media: Option<Snapshot>,
    sig: Option<String>,
    msg: usize,
    next_poll: Instant,
    next_message: Instant,
    art_cache: RefCell<Option<((String, u32), Image)>>,
    idle_cache: RefCell<HashMap<(Mode, String, u32), Vec<Image>>>,
    started: Instant,
}

impl Media {
    pub fn new(cfg: &Value) -> Media {
        let pref = Arc::new(Mutex::new(cfg["midia"].str_of("fonte").unwrap_or("auto").to_string()));
        let shared = Arc::new(Mutex::new(None));
        let (tx, rx) = channel();
        {
            let (shared, pref) = (shared.clone(), pref.clone());
            std::thread::spawn(move || watch(rx, shared, pref));
        }
        let seed = (win::now() * 1000.0) as usize;
        Media {
            tx,
            shared,
            pref,
            media: None,
            sig: None,
            msg: seed % FUNNY.len(),
            next_poll: Instant::now(),
            next_message: Instant::now() + MESSAGE_EVERY,
            art_cache: RefCell::new(None),
            idle_cache: RefCell::new(HashMap::new()),
            started: Instant::now(),
        }
    }

    fn has_media(&self) -> bool {
        self.media.as_ref().is_some_and(|m| !m.empty)
    }

    fn pref(&self) -> String {
        self.media.as_ref().map(|m| m.pref.clone()).filter(|p| !p.is_empty()).unwrap_or_else(|| self.pref.lock().unwrap().clone())
    }

    fn position(&self) -> f64 {
        let Some(m) = &self.media else { return 0.0 };
        let mut pos = m.position;
        if m.playing {
            if let Some(updated) = m.updated {
                pos += win::now() - updated;
            }
        }
        if m.duration > 0.0 { pos.clamp(0.0, m.duration) } else { pos }
    }

    fn signature(&self) -> String {
        match &self.media {
            Some(m) if !m.empty => {
                let second = if m.playing && m.duration > 0.0 { self.position() as i64 } else { -1 };
                format!(
                    "{}|{}|{}|{}|{:?}|{}{}{}|{}|{second}",
                    m.kind, m.title, m.artist, m.playing, m.art_key, m.can_toggle, m.can_prev, m.can_next, m.position
                )
            }
            m => format!("vazio|{}|{:?}", self.pref(), m.as_ref().and_then(|m| m.error.clone())),
        }
    }

    /// ((x, y), lado, raio) da capa no tamanho dado.
    fn art_geom(mode: Mode) -> ((f32, f32), f32, f32) {
        match mode {
            Mode::P => (C_ART_XY, C_ART, C_ART_RADIUS),
            Mode::M => (M_ART_XY, M_ART, M_ART_RADIUS),
            Mode::G => (ART_XY, ART, ART_RADIUS),
        }
    }

    fn idle_box(mode: Mode) -> (f32, f32, f32, f32) {
        match mode {
            Mode::P => C_IDLE_BOX,
            Mode::M => M_IDLE_BOX,
            Mode::G => IDLE_BOX,
        }
    }

    /// (controle, x, y, tamanho, habilitado, em círculo) de cada botão.
    fn controls(&self, mode: Mode) -> Vec<(Ctl, f32, f32, f32, bool, bool)> {
        let Some(m) = self.media.as_ref().filter(|m| !m.empty) else { return Vec::new() };
        let toggle = Ctl::Toggle(m.playing);
        match mode {
            Mode::P => {
                let y = COMPACT_H / 2.0;
                vec![(toggle, W - 60.0, y, C_PLAY_D, m.can_toggle, true), (Ctl::Next, W - 22.0, y, 12.0, m.can_next, false)]
            }
            Mode::M => {
                let (x, y) = (M_TEXT_X, M_CTRL_Y);
                vec![
                    (Ctl::Prev, x + 14.0, y, 15.0, m.can_prev, false),
                    (toggle, x + 62.0, y, M_PLAY_D, m.can_toggle, true),
                    (Ctl::Next, x + 110.0, y, 15.0, m.can_next, false),
                ]
            }
            Mode::G => {
                let (cx, y) = (W / 2.0, CTRL_Y);
                vec![
                    (Ctl::Prev, cx - 72.0, y, 18.0, m.can_prev, false),
                    (toggle, cx, y, PLAY_D, m.can_toggle, true),
                    (Ctl::Next, cx + 72.0, y, 18.0, m.can_next, false),
                ]
            }
        }
    }

    fn reach(size: f32, circle: bool) -> f32 {
        if circle { size / 2.0 + 2.0 } else { size.max(14.0) * 0.9 }
    }

    fn control_at(&self, x: f32, y: f32, mode: Mode) -> Option<(Ctl, f32, f32, f32, bool, bool)> {
        self.controls(mode)
            .into_iter()
            .find(|&(_, cx, cy, size, _, circle)| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() <= Self::reach(size, circle))
    }

    fn on_art(&self, x: f32, y: f32, mode: Mode) -> bool {
        let ((ax, ay), art, _) = Self::art_geom(mode);
        self.has_media() && (ax..=ax + art).contains(&x) && (ay..=ay + art).contains(&y)
    }

    fn on_bar(&self, x: f32, y: f32, mode: Mode) -> bool {
        let Some(m) = self.media.as_ref().filter(|m| !m.empty && m.duration > 0.0) else { return false };
        let _ = m;
        match mode {
            Mode::P => false,
            Mode::M => x >= M_TEXT_X - 4.0 && (M_BAR_Y - 8.0..=MEDIUM_H).contains(&y),
            Mode::G => (BAR_Y - 8.0..=BAR_Y + 26.0).contains(&y),
        }
    }

    /// Capa arredondada (guardada no tamanho atual).
    fn paste_art(&self, c: &mut Canvas, mode: Mode) -> bool {
        let Some(m) = self.media.as_ref().filter(|m| !m.empty) else { return false };
        let (Some(art), Some(key)) = (&m.art, &m.art_key) else { return false };
        let ((x, y), size, radius) = Self::art_geom(mode);
        let px = (size * c.s).round() as u32;
        let cache_key = (key.clone(), px);
        let mut cache = self.art_cache.borrow_mut();
        if cache.as_ref().map(|c| &c.0) != Some(&cache_key) {
            *cache = Some((cache_key, gfx::fit_square(art, px)));
        }
        c.push_round_clip(x, y, x + size, y + size, radius);
        c.image(&cache.as_ref().unwrap().1, x, y);
        c.pop_clip();
        true
    }

    /// Nota musical no lugar da capa, quando há música mas não há imagem.
    fn note(c: &mut Canvas, mode: Mode, glyph: f32) {
        let ((x, y), size, _) = Self::art_geom(mode);
        c.text(x + size / 2.0, y + size / 2.0, "♪", Face::Symbol, glyph, MUTED, Anchor::MM, None);
    }

    /// Quadros de um equalizador "cochilando", com zzz subindo. Desenhados uma vez só.
    fn idle_frames(&self, mode: Mode, s: f32) -> std::cell::Ref<'_, Vec<Image>> {
        let pref = self.pref();
        let key = (mode, pref.clone(), (s * 100.0) as u32);
        if !self.idle_cache.borrow().contains_key(&key) {
            let (_, _, w, h) = Self::idle_box(mode);
            let bars = match mode {
                Mode::P => 4,
                Mode::M => 5,
                Mode::G => 23,
            };
            let colors = idle_gradient(&pref);
            let bar = w / (bars as f32 * 1.9);
            let step = (w - bar) / (bars - 1) as f32;
            let center = (bars - 1) as f32 / 2.0;
            let tau = std::f32::consts::TAU;
            let mut frames = Vec::with_capacity(IDLE_FRAMES);
            for f in 0..IDLE_FRAMES {
                let p = f as f32 / IDLE_FRAMES as f32;
                let mut c = Canvas::new((w * s).round() as u32, (h * s).round() as u32, s);
                for i in 0..bars {
                    // ondas com número inteiro de ciclos por loop, para emendar sem salto
                    let envelope = 0.35 + 0.65 * (-((i as f32 - center) / (bars as f32 * 0.32)).powi(2)).exp();
                    let wave = 0.5 + 0.5 * (tau * (2.0 * p - i as f32 / bars as f32 * 1.5)).sin();
                    let drift = 0.5 + 0.5 * (tau * (p + i as f32 * 0.21)).sin();
                    let level = 0.12 + 0.6 * envelope * (0.7 * wave + 0.3 * drift);
                    let bh = bar.max(h * 0.78 * level);
                    let t = i as f32 / (bars - 1) as f32;
                    let x = i as f32 * step;
                    c.fill_round_rect(x, h / 2.0 - bh / 2.0, x + bar, h / 2.0 + bh / 2.0, bar / 2.0, colors.0.mix(colors.1, t));
                }
                if mode == Mode::G {
                    for j in 0..3 {
                        let q = (p + j as f32 / 3.0).fract();
                        let size = (11.0 + 9.0 * q) * h / 70.0;
                        let alpha = (220.0 * (std::f32::consts::PI * q).sin()) as u8;
                        // sobem por cima das últimas barras (as mais baixas), balançando de leve
                        let zx = w * (0.85 + 0.1 * q + 0.01 * (tau * q * 2.0).sin());
                        let zy = h * (0.4 - 0.34 * q);
                        c.text(zx, zy, "z", Face::Semibold, size, Rgba(220, 220, 228, alpha), Anchor::MM, None);
                    }
                }
                frames.push(c.finish());
            }
            self.idle_cache.borrow_mut().insert(key.clone(), frames);
        }
        std::cell::Ref::map(self.idle_cache.borrow(), |c| c.get(&key).unwrap())
    }

    fn open_player(&self, ctx: &mut Ctx) {
        let Some(m) = &self.media else { return };
        let part: String = m.title.chars().take(40).collect();
        match (!part.is_empty()).then(|| win::find_window(&part)).flatten() {
            Some(hwnd) => win::bring_to_front(hwnd),
            None if m.kind == "spotify" => ctx.open("spotify:"),
            None => {}
        }
    }
}

impl Kind for Media {
    fn key(&self) -> &'static str {
        "midia"
    }
    fn title(&self) -> &'static str {
        "Tocando agora"
    }

    fn layout(&self, mode: Mode) -> Layout {
        match mode {
            Mode::P => Layout { w: W, h: COMPACT_H, radius: COMPACT_RADIUS },
            Mode::M => Layout { w: W, h: MEDIUM_H, radius: M_RADIUS },
            Mode::G => Layout { w: W, h: H, radius: RADIUS },
        }
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, _env: &Env) {
        let m = self.media.as_ref().filter(|m| !m.empty);
        let ((ax, ay), art, radius) = Self::art_geom(mode);
        if m.is_none_or(|m| m.art.is_none()) {
            c.fill_round_rect(ax, ay, ax + art, ay + art, radius, WHITE.alpha(0.12));
        }
        let dim = WHITE.alpha(0.3);
        for (ctl, cx, cy, size, enabled, circle) in self.controls(mode) {
            let (glyph, color) = if circle {
                c.fill_circle(cx, cy, size / 2.0, if enabled { FG } else { dim });
                (size * 0.38, DARK_GLYPH)
            } else {
                (size, if enabled { FG } else { dim })
            };
            match ctl {
                Ctl::Toggle(false) => drawing::play(c, cx, cy, glyph, color),
                Ctl::Toggle(true) => drawing::pause(c, cx, cy, glyph, color),
                Ctl::Prev => drawing::skip(c, cx, cy, glyph, color, false),
                Ctl::Next => drawing::skip(c, cx, cy, glyph, color, true),
            }
        }
        if let Some(m) = m.filter(|m| mode != Mode::P && m.duration > 0.0) {
            let frac = (self.position() / m.duration) as f32;
            let (x0, x1, y, h) = if mode == Mode::M { (M_TEXT_X, W - 16.0, M_BAR_Y, 3.0) } else { (18.0, W - 18.0, BAR_Y, 4.0) };
            c.fill_round_rect(x0, y, x1, y + h, h / 2.0, WHITE.alpha(0.22));
            if frac > 0.0 {
                c.fill_round_rect(x0, y, (x0 + h).max(x0 + (x1 - x0) * frac), y + h, h / 2.0, Rgba::rgb(235, 235, 240));
            }
        }

        let error = self.media.as_ref().and_then(|m| m.error.clone());
        let (funny_title, mut funny_sub) = (FUNNY[self.msg].0.to_string(), FUNNY[self.msg].1.to_string());
        if let Some(e) = error {
            funny_sub = e;
        }
        match mode {
            Mode::P => {
                let x = C_TEXT_X;
                if let Some(m) = m {
                    if !self.paste_art(c, mode) {
                        Self::note(c, mode, 18.0);
                    }
                    c.text(x, 14.0, &m.title, Face::Semibold, 12.0, FG, Anchor::LA, Some(C_TEXT_W));
                    c.text(x, 33.0, &m.artist, Face::Regular, 10.5, MUTED, Anchor::LA, Some(C_TEXT_W));
                } else {
                    let w = W - x - 18.0;
                    c.text(x, 14.0, &funny_title, Face::Semibold, 12.0, FG, Anchor::LA, Some(w));
                    c.text(x, 33.0, &funny_sub, Face::Regular, 10.5, MUTED, Anchor::LA, Some(w));
                }
            }
            Mode::M => {
                let (x, w) = (M_TEXT_X, M_TEXT_W);
                if let Some(m) = m {
                    if !self.paste_art(c, mode) {
                        Self::note(c, mode, 28.0);
                    }
                    c.text(x, 13.0, &m.title, Face::Semibold, 13.0, FG, Anchor::LA, Some(w));
                    c.text(x, 32.0, &m.artist, Face::Regular, 11.0, MUTED, Anchor::LA, Some(w));
                } else {
                    c.text(x, 22.0, &funny_title, Face::Semibold, 13.0, FG, Anchor::LA, Some(w));
                    for (i, line) in c.wrap(&funny_sub, Face::Regular, 11.0, w, 2).iter().enumerate() {
                        c.text(x, 42.0 + 16.0 * i as f32, line, Face::Regular, 11.0, MUTED, Anchor::LA, Some(w));
                    }
                }
            }
            Mode::G => {
                let (x, w) = (TEXT_X, TEXT_W);
                let kind = m.map(|m| m.kind.to_string()).unwrap_or_else(|| self.pref());
                c.text(x, 19.0, source_label(&kind), Face::Semibold, 10.0, source_color(&kind), Anchor::LA, None);
                if let Some(m) = m {
                    if !self.paste_art(c, mode) {
                        Self::note(c, mode, 32.0);
                    }
                    let lines = c.wrap(&m.title, Face::Semibold, 14.0, w, 2);
                    for (i, line) in lines.iter().enumerate() {
                        c.text(x, 35.0 + 19.0 * i as f32, line, Face::Semibold, 14.0, FG, Anchor::LA, Some(w));
                    }
                    c.text(x, 37.0 + 19.0 * lines.len() as f32, &m.artist, Face::Regular, 12.0, MUTED, Anchor::LA, Some(w));
                    if m.duration > 0.0 {
                        let pos = self.position();
                        let ty = BAR_Y + 10.0;
                        c.text(18.0, ty, &fmt_time(pos), Face::Regular, 10.0, MUTED, Anchor::LA, None);
                        c.text(W - 18.0, ty, &format!("-{}", fmt_time(m.duration - pos)), Face::Regular, 10.0, MUTED, Anchor::RA, None);
                    }
                } else {
                    Self::note(c, mode, 32.0);
                    c.text(x, 35.0, &funny_title, Face::Semibold, 14.0, FG, Anchor::LA, Some(w));
                    for (i, line) in c.wrap(&funny_sub, Face::Regular, 12.0, w, 2).iter().enumerate() {
                        c.text(x, 57.0 + 17.0 * i as f32, line, Face::Regular, 12.0, MUTED, Anchor::LA, Some(w));
                    }
                }
            }
        }
    }

    /// O equalizador "cochilando" quando nada está tocando.
    fn overlay(&self, c: &mut Canvas, mode: Mode, ox: f32, oy: f32) {
        if self.has_media() {
            return;
        }
        let s = c.s;
        let frames = self.idle_frames(mode, s);
        let index = (self.started.elapsed().as_secs_f32() * IDLE_FPS as f32) as usize % frames.len();
        let (x, y, _, _) = Self::idle_box(mode);
        c.image(&frames[index], ox + x, oy + y);
    }

    fn overlay_fps(&self) -> u32 {
        if self.has_media() { 0 } else { IDLE_FPS }
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        if let Some((ctl, _, _, _, enabled, _)) = self.control_at(x, y, mode) {
            return enabled.then_some(match ctl {
                Ctl::Prev => PREV,
                Ctl::Toggle(_) => TOGGLE,
                Ctl::Next => NEXT,
            });
        }
        (self.on_art(x, y, mode) && env.shortcuts()).then_some(OPEN_PLAYER)
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            TOGGLE => {
                let _ = self.tx.send(Cmd::Toggle);
            }
            NEXT => {
                let _ = self.tx.send(Cmd::Next);
            }
            PREV => {
                let _ = self.tx.send(Cmd::Prev);
            }
            OPEN_PLAYER => self.open_player(ctx),
            a if (SOURCE..SOURCE + SOURCES.len() as u32).contains(&a) => {
                let value = SOURCES[(a - SOURCE) as usize].0;
                ctx.wcfg().insert("fonte".into(), json!(value));
                ctx.save();
                *self.pref.lock().unwrap() = value.to_string();
                let _ = self.tx.send(Cmd::Wake);
            }
            _ => {}
        }
        self.next_poll = Instant::now() + Duration::from_millis(200);
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> Option<Hot> {
        if let Some((_, cx, cy, size, _, circle)) = self.control_at(x, y, mode) {
            let r = Self::reach(size, circle) + 2.0;
            return Some((cx - r, cy - r, cx + r, cy + r, r));
        }
        if self.on_art(x, y, mode) {
            let ((ax, ay), art, radius) = Self::art_geom(mode);
            return Some((ax, ay, ax + art, ay + art, radius));
        }
        None
    }

    fn tooltip(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<String> {
        let m = self.media.as_ref().filter(|m| !m.empty)?;
        if let Some((ctl, ..)) = self.control_at(x, y, mode) {
            return Some(
                match ctl {
                    Ctl::Prev => "Anterior",
                    Ctl::Next => "Próxima",
                    Ctl::Toggle(false) => "Tocar",
                    Ctl::Toggle(true) => "Pausar",
                }
                .into(),
            );
        }
        let probe = Canvas::new(1, 1, 1.0);
        let mut song = probe.wrap(&m.title, Face::Regular, 11.5, 300.0, 3).join("\n");
        if !m.artist.is_empty() {
            song += &format!("\n\n{}", m.artist);
        }
        if self.on_art(x, y, mode) {
            if !env.shortcuts() {
                return Some(song);
            }
            return Some(song + if m.artist.is_empty() { "\n\n" } else { "\n" } + "Clique para abrir o player");
        }
        if self.on_bar(x, y, mode) {
            return Some(format!("{} de {}", fmt_time(self.position()), fmt_time(m.duration)));
        }
        let text_area = match mode {
            Mode::P => (C_TEXT_X..=C_TEXT_X + C_TEXT_W).contains(&x),
            Mode::M => x >= M_TEXT_X && y < M_CTRL_Y - 18.0,
            Mode::G => x >= TEXT_X && y < BAR_Y - 6.0,
        };
        text_area.then_some(song)
    }

    fn wheel(&mut self, _x: f32, _y: f32, steps: f32, _mode: Mode, _ctx: &mut Ctx) {
        win::change_volume(steps);
    }

    fn menu(&self, env: &Env) -> Vec<MenuItem> {
        let pref = env.wcfg().str_of("fonte").unwrap_or("auto");
        let sources = SOURCES.iter().enumerate().map(|(i, (v, label))| MenuItem::Radio(label.to_string(), *v == pref, SOURCE + i as u32)).collect();
        vec![MenuItem::Sub("Mostrar".into(), sources)]
    }

    fn next_tick(&self) -> Option<Instant> {
        Some(self.next_poll.min(self.next_message))
    }

    fn tick(&mut self, _ctx: &mut Ctx) -> bool {
        let now = Instant::now();
        let mut changed = false;
        if now >= self.next_poll {
            self.next_poll = now + POLL;
            if let Some(snap) = self.shared.lock().unwrap().clone() {
                self.media = Some(snap);
            }
            let sig = self.signature();
            if self.sig.as_ref() != Some(&sig) {
                self.sig = Some(sig);
                changed = true;
            }
        }
        if now >= self.next_message {
            self.next_message = now + MESSAGE_EVERY;
            self.msg = (self.msg + 1 + (win::now() as usize % (FUNNY.len() - 1))) % FUNNY.len();
            changed |= !self.has_media();
        }
        changed
    }
}
