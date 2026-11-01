//! Widget "Tocando agora" ("capa viva"): a mídia em reprodução no Windows (Spotify, YouTube no
//! navegador...). O fundo é a própria capa, desfocada; clicar na capa abre o player grande, com
//! volume do app, aleatório e repetir. A barra de progresso aceita clique para pular.
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
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows::Media::MediaPlaybackAutoRepeatMode;
use windows::Storage::Streams::DataReader;
use windows_future::IAsyncOperation;

use super::{Ctx, Env, Hot, IslandInfo, Kind, Layout, MenuItem, Mode, Request, SMALL, SMALL_RADIUS};
use crate::config::Get;
use crate::drawing;
use crate::gfx::{self, Anchor, Canvas, Face, Image};
use crate::imgops::{blur, Rgb};
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

fn source_name(kind: &str) -> &'static str {
    match kind {
        "spotify" => "Spotify",
        "youtube" => "o navegador",
        _ => "o player",
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
    /// quem está tocando (ex.: "Spotify.exe", "MSEdge"): para achar o áudio dele no mixer
    pub app_id: String,
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
    pub can_seek: bool,
    /// aleatório ligado (None: o player não informa)
    pub shuffle: Option<bool>,
    /// 0 sem repetir, 1 a música, 2 a lista (None: o player não informa)
    pub repeat: Option<i32>,
    pub can_shuffle: bool,
    pub can_repeat: bool,
    /// volume do app no mixer do Windows (None: não achou a sessão de áudio dele)
    pub volume: Option<f32>,
    pub art: Option<Arc<Image>>,
    pub art_key: Option<String>,
    pub error: Option<String>,
}

enum Cmd {
    Toggle,
    Next,
    Prev,
    Wake,
    Seek(f64),
    Shuffle(bool),
    Repeat(i32),
    /// volume do app, de 0 a 1
    Volume(f32),
}

// ---------- volume do app (mixer do Windows) ----------

mod mixer {
    use windows::core::{Interface, PWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

    fn exe_stem(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            if !ok {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
            let file = path.rsplit('\\').next()?.to_string();
            Some(file.trim_end_matches(".exe").to_string())
        }
    }

    /// Controles de volume das sessões de áudio do app (ex.: "Spotify.exe", "Chrome", "MSEdge").
    pub fn sessions(app_id: &str) -> Vec<ISimpleAudioVolume> {
        let id = app_id.to_lowercase();
        let mut out = Vec::new();
        unsafe {
            let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return out };
            let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eMultimedia) else { return out };
            let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return out };
            let Ok(list) = mgr.GetSessionEnumerator() else { return out };
            for i in 0..list.GetCount().unwrap_or(0) {
                let Ok(ctl) = list.GetSession(i) else { continue };
                let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
                let pid = ctl2.GetProcessId().unwrap_or(0);
                if pid == 0 {
                    continue;
                }
                let Some(stem) = exe_stem(pid) else { continue };
                if stem.len() >= 3 && id.contains(&stem) {
                    if let Ok(v) = ctl.cast::<ISimpleAudioVolume>() {
                        out.push(v);
                    }
                }
            }
        }
        out
    }

    /// Medidores de pico das sessões de áudio do app (a "luz" do volume no mixer do Windows).
    pub fn meters(app_id: &str) -> Vec<IAudioMeterInformation> {
        sessions(app_id).into_iter().filter_map(|v| v.cast::<IAudioMeterInformation>().ok()).collect()
    }

    pub fn get(app_id: &str) -> Option<f32> {
        sessions(app_id).first().and_then(|v| unsafe { v.GetMasterVolume().ok() })
    }

    pub fn set(app_id: &str, level: f32) -> bool {
        let list = sessions(app_id);
        for v in &list {
            unsafe {
                let _ = v.SetMasterVolume(level.clamp(0.0, 1.0), std::ptr::null());
            }
        }
        !list.is_empty()
    }
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
            app_id: app_id.clone(),
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
            can_seek: controls.IsPlaybackPositionEnabled().unwrap_or(false),
            shuffle: info.IsShuffleActive().and_then(|v| v.Value()).ok(),
            repeat: info.AutoRepeatMode().and_then(|v| v.Value()).ok().map(|m| m.0),
            can_shuffle: controls.IsShuffleEnabled().unwrap_or(false),
            can_repeat: controls.IsRepeatEnabled().unwrap_or(false),
            volume: mixer::get(&app_id),
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
        crate::events::wake();
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Cmd::Wake) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
            Ok(cmd) => {
                if let Some(s) = &current {
                    let op = match cmd {
                        Cmd::Toggle => s.TryTogglePlayPauseAsync(),
                        Cmd::Next => s.TrySkipNextAsync(),
                        Cmd::Prev => s.TrySkipPreviousAsync(),
                        Cmd::Seek(secs) => s.TryChangePlaybackPositionAsync((secs * 10_000_000.0) as i64),
                        Cmd::Shuffle(on) => s.TryChangeShuffleActiveAsync(on),
                        Cmd::Repeat(mode) => s.TryChangeAutoRepeatModeAsync(MediaPlaybackAutoRepeatMode(mode)),
                        Cmd::Volume(level) => {
                            let id = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
                            mixer::set(&id, level);
                            continue; // relê na hora, sem esperar
                        }
                        Cmd::Wake => continue,
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
// grande: a capa na largura toda em cima; embaixo, fonte, título e artista, a barra, os
// controles, o volume do app e aleatório/abrir/repetir
const G_ART: (f32, f32, f32, f32, f32) = (16.0, 16.0, W - 32.0, 200.0, 18.0); // x, y, largura, altura, raio
const G_TEXT_Y: f32 = 228.0;
const G_VOL_Y: f32 = 384.0;
const G_TOG_Y: f32 = 420.0;
const H: f32 = G_TOG_Y + 28.0;
const VOL_X: (f32, f32) = (44.0, W - 44.0);
const RADIUS: f32 = RADIUS_G;
const ART: f32 = 76.0;
const ART_RADIUS: f32 = 14.0;
const ART_XY: (f32, f32) = (18.0, 18.0);
const TEXT_X: f32 = 18.0;
const TEXT_W: f32 = W - 36.0;
const BAR_Y: f32 = 290.0;
const CTRL_Y: f32 = 336.0;
const PLAY_D: f32 = 44.0;
const IDLE_BOX: (f32, f32, f32, f32) = (36.0, 66.0, W - 72.0, 100.0); // x, y, largura, altura do equalizador (na capa)

// pequeno (quadrado): capa e tocar em cima, título e artista embaixo, barra fina no pé
const C_ART: f32 = 64.0;
const C_ART_RADIUS: f32 = 12.0;
const C_ART_XY: (f32, f32) = (14.0, 14.0);
const C_TEXT_W: f32 = SMALL - 28.0;
const C_PLAY_D: f32 = 36.0;
const C_BAR_Y: f32 = 138.0;
const C_IDLE_BOX: (f32, f32, f32, f32) = (14.0 + 12.0, 14.0 + 12.0, 40.0, 40.0); // equalizador no lugar da capa

// médio: capa, fonte, título e artista, controles e uma barra de progresso fina
const MEDIUM_H: f32 = 110.0;
const M_RADIUS: f32 = RADIUS_M;
const M_ART: f32 = 74.0;
const M_ART_RADIUS: f32 = 12.0;
const M_ART_XY: (f32, f32) = (14.0, 14.0);
const M_TEXT_X: f32 = 14.0 + 74.0 + 14.0;
const M_TEXT_W: f32 = W - M_TEXT_X - 16.0;
const M_CTRL_Y: f32 = 74.0;
const M_PLAY_D: f32 = 32.0;
const M_BAR_Y: f32 = 100.0;
const M_IDLE_BOX: (f32, f32, f32, f32) = (14.0 + 17.0, 14.0 + 17.0, 40.0, 40.0);

const POLL: Duration = Duration::from_millis(250);
const MESSAGE_EVERY: Duration = Duration::from_secs(12);
const FADE_MS: f32 = 600.0;
/// o fundo é desenhado um pouco maior que o cartão (as bordas da ampliação ficam de fora)
const BG_ZOOM: f32 = 1.2;
const HUD: Duration = Duration::from_millis(1100);

/// Duas cores fortes e diferentes da capa (para as ondas da ilha).
fn art_colors(art: &Image) -> [Rgba; 2] {
    let tiny = gfx::resize(&gfx::fit_square(art, 64), 6, 6);
    // as mais vivas (saturadas e claras) primeiro
    let mut px: Vec<(f32, Rgba)> = tiny
        .px
        .chunks(4)
        .map(|p| {
            let (b, g, r) = (p[0] as f32, p[1] as f32, p[2] as f32);
            let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
            let sat = if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
            (sat * 0.7 + mx / 255.0 * 0.3, Rgba::rgb(p[2], p[1], p[0]))
        })
        .collect();
    px.sort_by(|a, b| b.0.total_cmp(&a.0));
    let first = px[0].1;
    let far = |c: Rgba| (c.0 as i32 - first.0 as i32).abs() + (c.1 as i32 - first.1 as i32).abs() + (c.2 as i32 - first.2 as i32).abs();
    let second = px.iter().map(|p| p.1).find(|&c| far(c) > 90).unwrap_or(first.mix(WHITE, 0.4));
    // claras o bastante para aparecer no preto
    let lift = |c: Rgba| {
        let l = c.0.max(c.1).max(c.2) as f32;
        if l < 150.0 { c.mix(WHITE, (150.0 - l) / 255.0 + 0.1) } else { c }
    };
    [lift(first), lift(second)]
}

/// O fundo "capa viva" (w×h pixels) a partir da capa: ver `Media::background`.
fn live_background(art: &Image, w: u32, h: u32) -> Image {
    // a capa pequena guarda onde fica cada cor; mais saturada, para o fundo não ficar apagado
    let tiny = gfx::resize(&gfx::fit_square(art, 96), 24, 24);
    let mut rgb = Rgb::from_image(&tiny);
    rgb.saturate(1.5);
    let tiny = rgb.to_image();
    // desfoque de verdade em meia resolução: manchas redondas, sem os quadrados da capa
    let (hw, hh) = ((w / 2).max(8), (h / 2).max(8));
    let mut detail = gfx::resize(&tiny, hw, hh);
    let mut soft = detail.clone();
    blur(&mut detail.px, hw, hh, 4, hh as f32 / 8.0);
    blur(&mut soft.px, hw, hh, 4, hh as f32 / 3.0);
    for (p, q) in detail.px.chunks_mut(4).zip(soft.px.chunks(4)) {
        for ch in 0..3 {
            p[ch] = ((p[ch] as f32 * 0.55 + q[ch] as f32 * 0.45) * 0.85) as u8;
        }
        p[3] = 255;
    }
    // já está tudo suave: ampliar agora não cria degraus
    let mut big = gfx::resize(&detail, w, h);
    // ruído fino (o mesmo nos três canais) para o degradê não virar faixas
    for (i, p) in big.px.chunks_mut(4).enumerate() {
        let mut n = (i as u32).wrapping_mul(0x9E37_79B9);
        n ^= n >> 15;
        n = n.wrapping_mul(0x85EB_CA6B);
        n ^= n >> 13;
        let d = (n % 3) as i16 - 1;
        for ch in 0..3 {
            p[ch] = (p[ch] as i16 + d).clamp(0, 255) as u8;
        }
    }
    big
}

// ações
const TOGGLE: u32 = 1;
const NEXT: u32 = 2;
const PREV: u32 = 3;
const OPEN_PLAYER: u32 = 4;
const SHUFFLE: u32 = 6;
const REPEAT: u32 = 7;
const SOURCE: u32 = 10; // + índice em SOURCES
const ISLAND: u32 = 8;
const SEEK: u32 = 1000; // + milésimos da música (0 a 1000)
const VOLUME: u32 = 3000; // + volume (0 a 100)

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
    /// fundo: a capa desfocada, um pouco maior que o cartão
    bg_cache: RefCell<Option<((String, u32, u32), Image)>>,
    /// fundo da música anterior, sumindo aos poucos depois de trocar de música
    fade: Option<(Image, Instant)>,
    idle_cache: RefCell<HashMap<(Mode, String, u32), Vec<Image>>>,
    started: Instant,
    /// aviso de volume no topo (roda do mouse)
    hud_until: Option<Instant>,
    /// volume pedido agora há pouco (mostrado até o mixer confirmar)
    volume_wanted: Option<(f32, Instant)>,
    /// próximo quadro da troca de fundo (um prazo fixo)
    frame_at: Instant,
    /// medidores de som do app que está tocando (para as ondas da ilha): (app, medidores)
    meters: RefCell<Option<(String, Vec<windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation>)>>,
    /// cores da capa já calculadas: (capa, cores)
    colors: RefCell<Option<(String, [Rgba; 2])>>,
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
        Media::with(tx, shared, pref, None, seed % FUNNY.len(), Instant::now())
    }

    fn with(tx: Sender<Cmd>, shared: Arc<Mutex<Option<Snapshot>>>, pref: Arc<Mutex<String>>, media: Option<Snapshot>, msg: usize, next: Instant) -> Media {
        Media {
            tx,
            shared,
            pref,
            media,
            sig: None,
            msg,
            next_poll: next,
            next_message: next + MESSAGE_EVERY,
            art_cache: RefCell::new(None),
            bg_cache: RefCell::new(None),
            fade: None,
            idle_cache: RefCell::new(HashMap::new()),
            started: Instant::now(),
            hud_until: None,
            volume_wanted: None,
            frame_at: next,
            meters: RefCell::new(None),
            colors: RefCell::new(None),
        }
    }

    /// Exemplo para a galeria: uma música tocando, sem a thread da central de mídia.
    pub fn sample() -> Media {
        let (tx, _rx) = channel();
        let snap = Snapshot {
            empty: false,
            pref: "auto".into(),
            kind: "spotify",
            title: "Nome da música".into(),
            artist: "Artista  ·  Álbum".into(),
            playing: true,
            position: 74.0,
            duration: 213.0,
            can_toggle: true,
            can_prev: true,
            can_next: true,
            ..Default::default()
        };
        let later = Instant::now() + Duration::from_secs(3600);
        Media::with(tx, Arc::new(Mutex::new(None)), Arc::new(Mutex::new("auto".into())), Some(snap), 0, later)
    }

    /// Exemplo com uma música e capa quaisquer (imagens do README).
    #[cfg(test)]
    pub fn sample_song(kind: &'static str, title: &str, artist: &str, art: Arc<Image>, key: &str, position: f64, duration: f64) -> Media {
        let mut m = Media::sample();
        if let Some(snap) = &mut m.media {
            snap.kind = kind;
            snap.title = title.into();
            snap.artist = artist.into();
            snap.art = Some(art);
            snap.art_key = Some(key.into());
            snap.position = position;
            snap.duration = duration;
            snap.updated = None;
        }
        m
    }

    /// Como logo depois de trocar de música: o fundo da música anterior (`old`) ainda sumindo,
    /// `progress` (0 a 1) da troca já feita (imagens do README).
    #[cfg(test)]
    pub fn fading_from(mut self, old: &Media, mode: Mode, s: f32, progress: f32) -> Media {
        if let Some(bg) = old.background(s, mode) {
            let ago = Duration::from_millis((progress.clamp(0.0, 1.0) * FADE_MS) as u64);
            self.fade = Some((bg, Instant::now() - ago));
        }
        self
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

    fn volume(&self) -> Option<f32> {
        if let Some((v, at)) = self.volume_wanted {
            if at.elapsed() < Duration::from_millis(1500) {
                return Some(v);
            }
        }
        self.media.as_ref().and_then(|m| m.volume)
    }

    fn signature(&self) -> String {
        match &self.media {
            Some(m) if !m.empty => {
                let second = if m.playing && m.duration > 0.0 { self.position() as i64 } else { -1 };
                format!(
                    "{}|{}|{}|{}|{:?}|{}{}{}|{}|{second}|{:?}|{:?}|{:?}",
                    m.kind, m.title, m.artist, m.playing, m.art_key, m.can_toggle, m.can_prev, m.can_next, m.position, m.shuffle, m.repeat,
                    m.volume.map(|v| (v * 100.0).round() as i32)
                )
            }
            m => format!("vazio|{}|{:?}", self.pref(), m.as_ref().and_then(|m| m.error.clone())),
        }
    }

    /// ((x, y), lado, raio) da capa no tamanho dado.
    fn art_geom(&self, mode: Mode) -> ((f32, f32), f32, f32) {
        match mode {
            Mode::P => (C_ART_XY, C_ART, C_ART_RADIUS),
            Mode::M => (M_ART_XY, M_ART, M_ART_RADIUS),
            Mode::G => (ART_XY, ART, ART_RADIUS),
        }
    }

    /// (x, y, largura, altura, raio) da capa: quadrada no pequeno e no médio, larga no grande.
    fn art_rect(&self, mode: Mode) -> (f32, f32, f32, f32, f32) {
        if mode == Mode::G {
            return G_ART;
        }
        let ((x, y), size, r) = self.art_geom(mode);
        (x, y, size, size, r)
    }

    fn idle_box(mode: Mode) -> (f32, f32, f32, f32) {
        match mode {
            Mode::P => C_IDLE_BOX,
            Mode::M => M_IDLE_BOX,
            Mode::G => IDLE_BOX,
        }
    }

    /// (x0, x1, y, espessura) da barra de progresso.
    fn bar_geom(&self, mode: Mode) -> Option<(f32, f32, f32, f32)> {
        match mode {
            Mode::P => Some((14.0, SMALL - 14.0, C_BAR_Y, 3.0)),
            Mode::M => Some((M_TEXT_X, W - 16.0, M_BAR_Y, 3.0)),
            Mode::G => Some((18.0, W - 18.0, BAR_Y, 4.0)),
        }
    }

    /// (controle, x, y, tamanho, habilitado, em círculo) de cada botão.
    fn controls(&self, mode: Mode) -> Vec<(Ctl, f32, f32, f32, bool, bool)> {
        let Some(m) = self.media.as_ref().filter(|m| !m.empty) else { return Vec::new() };
        let toggle = Ctl::Toggle(m.playing);
        match mode {
            Mode::P => {
                let (x, y) = (SMALL - 14.0 - C_PLAY_D / 2.0, 14.0 + C_PLAY_D / 2.0);
                vec![(toggle, x, y, C_PLAY_D, m.can_toggle, true), (Ctl::Next, x, y + 40.0, 13.0, m.can_next, false)]
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
        let (ax, ay, aw, ah, _) = self.art_rect(mode);
        self.has_media() && (ax..=ax + aw).contains(&x) && (ay..=ay + ah).contains(&y)
    }

    /// Fração (0 a 1) da música no ponto x, se (x, y) está sobre a barra de progresso.
    fn bar_at(&self, x: f32, y: f32, mode: Mode) -> Option<f32> {
        let m = self.media.as_ref().filter(|m| !m.empty && m.duration > 0.0)?;
        let (x0, x1, by, _) = self.bar_geom(mode)?;
        let reach_down = if mode == Mode::M { 10.0 } else { 14.0 };
        if !(by - 8.0..=by + reach_down).contains(&y) || x < x0 - 6.0 || x > x1 + 6.0 {
            return None;
        }
        let _ = m;
        Some(((x - x0) / (x1 - x0)).clamp(0.0, 1.0))
    }

    fn volume_at(&self, x: f32, y: f32, mode: Mode) -> Option<f32> {
        if mode != Mode::G || !(G_VOL_Y - 12.0..=G_VOL_Y + 12.0).contains(&y) || self.volume().is_none() || !self.has_media() {
            return None;
        }
        let (x0, x1) = VOL_X;
        (x >= x0 - 8.0 && x <= x1 + 8.0).then(|| ((x - x0) / (x1 - x0)).clamp(0.0, 1.0))
    }

    /// (ação, centro x) dos botões da última linha do grande.
    fn toggles(&self) -> [(u32, f32); 3] {
        [(SHUFFLE, 34.0), (OPEN_PLAYER, W / 2.0), (REPEAT, W - 34.0)]
    }

    fn toggle_at(&self, x: f32, y: f32, mode: Mode) -> Option<u32> {
        if mode != Mode::G || !self.has_media() || !(G_TOG_Y - 16.0..=G_TOG_Y + 16.0).contains(&y) {
            return None;
        }
        let m = self.media.as_ref()?;
        self.toggles().into_iter().find_map(|(a, cx)| {
            let half = if a == OPEN_PLAYER { 70.0 } else { 18.0 };
            let enabled = match a {
                SHUFFLE => m.can_shuffle && m.shuffle.is_some(),
                REPEAT => m.can_repeat && m.repeat.is_some(),
                _ => true,
            };
            ((cx - half..=cx + half).contains(&x) && enabled).then_some(a)
        })
    }

    /// Capa arredondada (guardada no tamanho atual).
    fn paste_art(&self, c: &mut Canvas, mode: Mode) -> bool {
        let Some(m) = self.media.as_ref().filter(|m| !m.empty) else { return false };
        let (Some(art), Some(key)) = (&m.art, &m.art_key) else { return false };
        let (x, y, w, h, radius) = self.art_rect(mode);
        let px = (w * c.s).round() as u32;
        let cache_key = (key.clone(), px);
        let mut cache = self.art_cache.borrow_mut();
        if cache.as_ref().map(|c| &c.0) != Some(&cache_key) {
            *cache = Some((cache_key, gfx::fit_square(art, px)));
        }
        // quadrada na largura da área; se a área é mais baixa, mostra o meio da capa
        c.push_round_clip(x, y, x + w, y + h, radius);
        c.image(&cache.as_ref().unwrap().1, x, y - (w - h) / 2.0);
        c.pop_clip();
        true
    }

    /// Fundo "capa viva": as cores da capa no mesmo lugar em que estão nela, mais saturadas e
    /// bem desfocadas (manchas de cor), misturadas com uma versão ainda mais borrada. Fica maior
    /// que o cartão para ter espaço de se mexer.
    ///
    /// A capa é reduzida e o desfoque é gaussiano, feito em meia resolução: ampliar uma imagem
    /// minúscula direto deixaria degraus e dobras no lugar de cada pixel. No fim entra um ruído
    /// quase invisível, para o degradê não formar faixas.
    fn background(&self, s: f32, mode: Mode) -> Option<Image> {
        let m = self.media.as_ref().filter(|m| !m.empty)?;
        let (art, key) = (m.art.as_ref()?, m.art_key.as_ref()?);
        let l = self.layout_of(mode);
        let (w, h) = ((l.w * s * BG_ZOOM).round() as u32, (l.h * s * BG_ZOOM).round() as u32);
        let cache_key = (key.clone(), w, h);
        let mut cache = self.bg_cache.borrow_mut();
        if cache.as_ref().map(|c| &c.0) != Some(&cache_key) {
            *cache = Some((cache_key, live_background(art, w, h)));
        }
        cache.as_ref().map(|c| c.1.clone())
    }

    /// Pinta o fundo (parado) com o canto do cartão em (ox, oy).
    fn paint_bg(&self, c: &mut Canvas, mode: Mode, ox: f32, oy: f32) -> bool {
        let Some(bg) = self.background(c.s, mode) else { return false };
        let l = self.layout_of(mode);
        let s = c.s;
        let room = (BG_ZOOM - 1.0) / 2.0;
        let place = |c: &mut Canvas, img: &Image, opacity: f32| {
            let (x, y) = (ox - room * l.w, oy - room * l.h);
            c.image_px(img, (x * s).round(), (y * s).round(), (l.w * BG_ZOOM * s).round(), (l.h * BG_ZOOM * s).round(), opacity);
        };
        c.push_round_clip(ox, oy, ox + l.w, oy + l.h, l.radius);
        place(c, &bg, 1.0);
        if let Some((old, since)) = &self.fade {
            let f = since.elapsed().as_secs_f32() * 1000.0 / FADE_MS;
            if f < 1.0 && old.w == bg.w && old.h == bg.h {
                place(c, old, 1.0 - f);
            }
        }
        // mais escuro embaixo, onde ficam o texto e os botões
        let k = |a: f32| Rgba(0, 0, 0, (255.0 * a) as u8);
        c.gradient_round_rect(ox, oy, ox + l.w, oy + l.h, l.radius, &[(0.0, k(0.12)), (0.5, k(0.32)), (1.0, k(0.55))], oy, oy + l.h);
        c.pop_clip();
        true
    }

    /// Sombra suave embaixo da capa (dá profundidade sobre o fundo colorido).
    fn art_shadow(&self, c: &mut Canvas, mode: Mode) {
        let (x, y, w, h, radius) = self.art_rect(mode);
        for i in 1..=4 {
            let g = i as f32 * 2.0;
            c.fill_round_rect(x - g / 2.0, y + 3.0 + g / 2.0, x + w + g / 2.0, y + h + 3.0 + g, radius + g, Rgba(0, 0, 0, 16));
        }
    }

    /// Nota musical no lugar da capa, quando há música mas não há imagem.
    fn note(&self, c: &mut Canvas, mode: Mode, glyph: f32) {
        let (x, y, w, h, _) = self.art_rect(mode);
        c.text(x + w / 2.0, y + h / 2.0, "♪", Face::Symbol, glyph, MUTED, Anchor::MM, None);
    }

    /// Quadros de um equalizador "cochilando", com zzz subindo. Desenhados uma vez só.
    fn idle_frames(&self, mode: Mode, s: f32) -> std::cell::Ref<'_, Vec<Image>> {
        let pref = self.pref();
        let key = (mode, pref.clone(), (s * 100.0) as u32);
        if !self.idle_cache.borrow().contains_key(&key) {
            let (_, _, w, h) = Self::idle_box(mode);
            let bars = match mode {
                Mode::P => 5,
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

    fn draw_controls(&self, c: &mut Canvas, mode: Mode) {
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
    }

    fn draw_bar(&self, c: &mut Canvas, mode: Mode) {
        let Some(m) = self.media.as_ref().filter(|m| !m.empty && m.duration > 0.0) else { return };
        let Some((x0, x1, y, h)) = self.bar_geom(mode) else { return };
        let frac = (self.position() / m.duration) as f32;
        c.fill_round_rect(x0, y, x1, y + h, h / 2.0, WHITE.alpha(0.22));
        if frac > 0.0 {
            c.fill_round_rect(x0, y, (x0 + h).max(x0 + (x1 - x0) * frac), y + h, h / 2.0, Rgba::rgb(235, 235, 240));
        }
        if mode == Mode::G {
            let pos = self.position();
            let ty = y + h + 6.0;
            c.text(x0, ty, &fmt_time(pos), Face::Regular, 10.0, MUTED, Anchor::LA, None);
            c.text(x1, ty, &format!("-{}", fmt_time(m.duration - pos)), Face::Regular, 10.0, MUTED, Anchor::RA, None);
        }
    }

    /// Linhas extras do grande: volume do app, aleatório, abrir o app e repetir.
    fn draw_extras(&self, c: &mut Canvas, m: &Snapshot) {
        c.hline_px(18.0, W - 18.0, G_VOL_Y - 18.0, DIVIDER);
        let icon = |c: &mut Canvas, x: f32, y: f32, glyph: &str, size: f32, col: Rgba| {
            c.text(x, y, glyph, Face::Icons, size, col, Anchor::MM, None);
        };
        let (x0, x1) = VOL_X;
        match self.volume() {
            Some(v) => {
                icon(c, 24.0, G_VOL_Y, "\u{e992}", 14.0, MUTED);
                icon(c, W - 24.0, G_VOL_Y, "\u{e995}", 14.0, MUTED);
                c.fill_round_rect(x0, G_VOL_Y - 2.5, x1, G_VOL_Y + 2.5, 2.5, WHITE.alpha(0.22));
                c.fill_round_rect(x0, G_VOL_Y - 2.5, x0 + 5.0f32.max((x1 - x0) * v), G_VOL_Y + 2.5, 2.5, Rgba::rgb(235, 235, 240));
            }
            None => {
                c.text(W / 2.0, G_VOL_Y, "Role a roda do mouse para o volume", Face::Regular, 11.5, MUTED, Anchor::MM, None);
            }
        }
        for (action, cx) in self.toggles() {
            let y = G_TOG_Y;
            match action {
                SHUFFLE | REPEAT => {
                    let (on, enabled) = if action == SHUFFLE {
                        (m.shuffle == Some(true), m.can_shuffle && m.shuffle.is_some())
                    } else {
                        (m.repeat.is_some_and(|r| r > 0), m.can_repeat && m.repeat.is_some())
                    };
                    if on {
                        c.fill_round_rect(cx - 16.0, y - 15.0, cx + 16.0, y + 15.0, 9.0, WHITE.alpha(0.2));
                    }
                    let glyph = match action {
                        SHUFFLE => "\u{e8b1}",
                        _ if m.repeat == Some(1) => "\u{e8ed}",
                        _ => "\u{e8ee}",
                    };
                    let col = if !enabled { WHITE.alpha(0.25) } else if on { FG } else { MUTED };
                    icon(c, cx, y, glyph, 15.0, col);
                }
                _ => {
                    let label = format!("Abrir {}", source_name(m.kind));
                    let tw = c.text_width(&label, Face::Semibold, 12.0);
                    c.text(cx - 9.0, y, &label, Face::Semibold, 12.0, Rgba::rgb(90, 200, 250), Anchor::MM, None);
                    icon(c, cx - 9.0 + tw / 2.0 + 12.0, y, "\u{e8a7}", 11.0, Rgba::rgb(90, 200, 250));
                }
            }
        }
    }

    fn layout_of(&self, mode: Mode) -> Layout {
        match mode {
            Mode::P => Layout { w: SMALL, h: SMALL, radius: SMALL_RADIUS },
            Mode::M => Layout { w: W, h: MEDIUM_H, radius: M_RADIUS },
            Mode::G => Layout { w: W, h: H, radius: RADIUS },
        }
    }

    /// Tudo o que fica por cima do fundo.
    fn draw_fg(&self, c: &mut Canvas, mode: Mode, has_bg: bool) {
        let m = self.media.as_ref().filter(|m| !m.empty);
        let (ax, ay, aw, ah, radius) = self.art_rect(mode);
        if m.is_none_or(|m| m.art.is_none()) {
            c.fill_round_rect(ax, ay, ax + aw, ay + ah, radius, WHITE.alpha(0.12));
        } else if has_bg {
            self.art_shadow(c, mode);
        }
        self.draw_controls(c, mode);
        self.draw_bar(c, mode);

        let error = self.media.as_ref().and_then(|m| m.error.clone());
        let (funny_title, mut funny_sub) = (FUNNY[self.msg].0.to_string(), FUNNY[self.msg].1.to_string());
        if let Some(e) = error {
            funny_sub = e;
        }
        match mode {
            Mode::P => {
                let x = 14.0;
                if let Some(m) = m {
                    if !self.paste_art(c, mode) {
                        self.note(c, mode, 26.0);
                    }
                    c.text(x, 90.0, &m.title, Face::Semibold, 12.5, FG, Anchor::LA, Some(C_TEXT_W));
                    c.text(x, 108.0, &m.artist, Face::Regular, 11.0, MUTED, Anchor::LA, Some(C_TEXT_W));
                } else {
                    c.text(x, 92.0, &funny_title, Face::Semibold, 12.5, FG, Anchor::LA, Some(C_TEXT_W));
                    for (i, line) in c.wrap(&funny_sub, Face::Regular, 11.0, C_TEXT_W, 2).iter().enumerate() {
                        c.text(x, 110.0 + 15.0 * i as f32, line, Face::Regular, 11.0, MUTED, Anchor::LA, Some(C_TEXT_W));
                    }
                }
            }
            Mode::M => {
                let (x, w) = (M_TEXT_X, M_TEXT_W);
                if let Some(m) = m {
                    if !self.paste_art(c, mode) {
                        self.note(c, mode, 28.0);
                    }
                    c.text(x, 12.0, source_label(m.kind), Face::Semibold, 9.5, source_color(m.kind), Anchor::LA, None);
                    c.text(x, 26.0, &m.title, Face::Semibold, 13.0, FG, Anchor::LA, Some(w));
                    c.text(x, 44.0, &m.artist, Face::Regular, 11.0, MUTED, Anchor::LA, Some(w));
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
                let y = G_TEXT_Y;
                c.text(x, y, source_label(&kind), Face::Semibold, 10.0, source_color(&kind), Anchor::LA, None);
                if let Some(m) = m {
                    if !self.paste_art(c, mode) {
                        self.note(c, mode, 44.0);
                    }
                    c.text(x, y + 15.0, &m.title, Face::Semibold, 15.5, FG, Anchor::LA, Some(w));
                    c.text(x, y + 37.0, &m.artist, Face::Regular, 12.5, MUTED, Anchor::LA, Some(w));
                    self.draw_extras(c, m);
                } else {
                    c.text(x, y + 15.0, &funny_title, Face::Semibold, 15.5, FG, Anchor::LA, Some(w));
                    for (i, line) in c.wrap(&funny_sub, Face::Regular, 12.0, w, 2).iter().enumerate() {
                        c.text(x, y + 37.0 + 17.0 * i as f32, line, Face::Regular, 12.0, MUTED, Anchor::LA, Some(w));
                    }
                }
            }
        }
    }

    fn overlay_rest(&self, c: &mut Canvas, mode: Mode, ox: f32, oy: f32) {
        let lw = self.layout_of(mode).w;
        if let Some(until) = self.hud_until.filter(|t| Instant::now() < *t) {
            let left = until.saturating_duration_since(Instant::now()).as_secs_f32();
            let alpha = (left / 0.25).min(1.0);
            let level = self.volume().unwrap_or(0.0);
            let (w, h) = (130.0f32.min(lw - 16.0), 28.0);
            let (x, y) = (ox + (lw - w) / 2.0, oy + 8.0);
            c.fill_round_rect(x, y, x + w, y + h, h / 2.0, Rgba(0, 0, 0, (150.0 * alpha) as u8));
            c.text(x + 18.0, y + h / 2.0, "\u{e995}", Face::Icons, 12.0, WHITE.alpha(alpha), Anchor::MM, None);
            let (bx0, bx1) = (x + 34.0, x + w - 14.0);
            c.fill_round_rect(bx0, y + h / 2.0 - 2.0, bx1, y + h / 2.0 + 2.0, 2.0, WHITE.alpha(0.25 * alpha));
            c.fill_round_rect(bx0, y + h / 2.0 - 2.0, bx0 + 4.0f32.max((bx1 - bx0) * level), y + h / 2.0 + 2.0, 2.0, WHITE.alpha(alpha));
        }
        if self.has_media() {
            return;
        }
        let s = c.s;
        let frames = self.idle_frames(mode, s);
        let index = (self.started.elapsed().as_secs_f32() * IDLE_FPS as f32) as usize % frames.len();
        let (x, y, _, _) = Self::idle_box(mode);
        c.image(&frames[index], ox + x, oy + y);
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
        self.layout_of(mode)
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, _env: &Env) {
        // fundo "capa viva" (com a capa anterior sumindo, logo depois de trocar de música)
        let has_bg = self.paint_bg(c, mode, 0.0, 0.0);
        self.draw_fg(c, mode, has_bg);
    }

    /// O equalizador "cochilando" quando nada está tocando e o aviso de volume.
    fn overlay(&self, c: &mut Canvas, mode: Mode, ox: f32, oy: f32) {
        self.overlay_rest(c, mode, ox, oy);
    }

    fn overlay_fps(&self) -> u32 {
        if self.hud_until.is_some_and(|t| Instant::now() < t) {
            30
        } else if self.has_media() {
            0
        } else {
            IDLE_FPS
        }
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        if let Some((ctl, _, _, _, enabled, _)) = self.control_at(x, y, mode) {
            return enabled.then_some(match ctl {
                Ctl::Prev => PREV,
                Ctl::Toggle(_) => TOGGLE,
                Ctl::Next => NEXT,
            });
        }
        if let Some(a) = self.toggle_at(x, y, mode) {
            return Some(a);
        }
        if let Some(v) = self.volume_at(x, y, mode) {
            return Some(VOLUME + (v * 100.0).round() as u32);
        }
        if let Some(f) = self.bar_at(x, y, mode) {
            if self.media.as_ref().is_some_and(|m| m.can_seek) {
                return Some(SEEK + (f * 1000.0).round() as u32);
            }
        }
        // fora dos botões: o widget vira a ilha flutuante
        let _ = env;
        self.has_media().then_some(ISLAND)
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        let m = self.media.clone().unwrap_or_default();
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
            ISLAND => ctx.out.push(Request::Island),
            SHUFFLE => {
                let _ = self.tx.send(Cmd::Shuffle(m.shuffle != Some(true)));
            }
            REPEAT => {
                // desligado -> repetir a lista -> repetir a música -> desligado
                let next = match m.repeat.unwrap_or(0) {
                    0 => 2,
                    2 => 1,
                    _ => 0,
                };
                let _ = self.tx.send(Cmd::Repeat(next));
            }
            a if (SEEK..=SEEK + 1000).contains(&a) => {
                let secs = m.duration * (a - SEEK) as f64 / 1000.0;
                let _ = self.tx.send(Cmd::Seek(secs));
                // mostra na hora, sem esperar o player confirmar
                if let Some(snap) = &mut self.media {
                    snap.position = secs;
                    snap.updated = Some(win::now());
                }
            }
            a if (VOLUME..=VOLUME + 100).contains(&a) => {
                let level = (a - VOLUME) as f32 / 100.0;
                let _ = self.tx.send(Cmd::Volume(level));
                self.volume_wanted = Some((level, Instant::now()));
            }
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
        if let Some(a) = self.toggle_at(x, y, mode) {
            let cx = self.toggles().iter().find(|t| t.0 == a).map(|t| t.1).unwrap_or(0.0);
            let half = if a == OPEN_PLAYER { 72.0 } else { 18.0 };
            return Some((cx - half, G_TOG_Y - 16.0, cx + half, G_TOG_Y + 16.0, 10.0));
        }
        // a barra de progresso "engrossa" com o mouse em cima
        if let (Some(_), Some((x0, x1, by, h))) = (self.bar_at(x, y, mode), self.bar_geom(mode)) {
            if self.media.as_ref().is_some_and(|m| m.can_seek) {
                let cy = by + h / 2.0;
                return Some((x0, cy - 3.5, x1, cy + 3.5, 3.5));
            }
        }
        if self.on_art(x, y, mode) {
            let (ax, ay, aw, ah, radius) = self.art_rect(mode);
            return Some((ax, ay, ax + aw, ay + ah, radius));
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
        if let Some(a) = self.toggle_at(x, y, mode) {
            return Some(
                match a {
                    SHUFFLE => if m.shuffle == Some(true) { "Aleatório ligado" } else { "Aleatório" },
                    REPEAT => match m.repeat {
                        Some(1) => "Repetindo esta música",
                        Some(2) => "Repetindo a lista",
                        _ => "Repetir",
                    },
                    _ => "Abrir o player",
                }
                .into(),
            );
        }
        if let Some(v) = self.volume_at(x, y, mode) {
            return Some(format!("Volume do app: {}%\nClique para mudar", (v * 100.0).round()));
        }
        if let Some(f) = self.bar_at(x, y, mode) {
            let at = fmt_time(m.duration * f as f64);
            return Some(if m.can_seek { format!("{at}\nClique para pular para cá") } else { format!("{} de {}", fmt_time(self.position()), fmt_time(m.duration)) });
        }
        let probe = Canvas::new(1, 1, 1.0);
        let mut song = probe.wrap(&m.title, Face::Regular, 11.5, 300.0, 3).join("\n");
        if !m.artist.is_empty() {
            song += &format!("\n\n{}", m.artist);
        }
        let _ = (env, x, y, mode);
        Some(song + if m.artist.is_empty() { "\n\n" } else { "\n\n" } + "Clique para virar a ilha flutuante")
    }

    /// Roda do mouse: o volume do app que está tocando (ou o do Windows, se não achar o app).
    fn wheel(&mut self, _x: f32, _y: f32, steps: f32, _mode: Mode, _ctx: &mut Ctx) {
        match self.volume() {
            Some(v) => {
                let level = (v + steps * 0.04).clamp(0.0, 1.0);
                let _ = self.tx.send(Cmd::Volume(level));
                self.volume_wanted = Some((level, Instant::now()));
                self.hud_until = Some(Instant::now() + HUD);
            }
            None => win::change_volume(steps),
        }
    }

    fn menu(&self, env: &Env) -> Vec<MenuItem> {
        let pref = env.wcfg().str_of("fonte").unwrap_or("auto");
        let sources = SOURCES.iter().enumerate().map(|(i, (v, label))| MenuItem::Radio(label.to_string(), *v == pref, SOURCE + i as u32)).collect();
        vec![MenuItem::Sub("Mostrar".into(), sources)]
    }

    fn island(&self) -> Option<IslandInfo> {
        let m = self.media.as_ref().filter(|m| !m.empty)?;
        let key = m.art_key.clone().unwrap_or_default();
        let cached = self.colors.borrow().as_ref().filter(|c| c.0 == key).map(|c| c.1);
        let colors = cached.unwrap_or_else(|| {
            let found = m.art.as_ref().map(|a| art_colors(a)).unwrap_or([Rgba::rgb(10, 132, 255), Rgba::rgb(191, 90, 242)]);
            *self.colors.borrow_mut() = Some((key, found));
            found
        });
        let app = match m.kind {
            "spotify" => "Spotify",
            "youtube" => "YouTube",
            _ => "Mídia",
        };
        Some(IslandInfo {
            title: m.title.clone(),
            artist: m.artist.clone(),
            app: app.into(),
            art: m.art.clone(),
            colors,
            playing: m.playing,
            position: self.position(),
            duration: m.duration,
            can_prev: m.can_prev,
            can_toggle: m.can_toggle,
            can_next: m.can_next,
            can_seek: m.can_seek,
        })
    }

    fn sound_level(&self) -> Option<f32> {
        let m = self.media.as_ref().filter(|m| !m.empty && !m.app_id.is_empty())?;
        let mut meters = self.meters.borrow_mut();
        if meters.as_ref().map(|x| x.0 != m.app_id).unwrap_or(true) {
            *meters = Some((m.app_id.clone(), mixer::meters(&m.app_id)));
        }
        let list = &meters.as_ref()?.1;
        if list.is_empty() {
            return None;
        }
        let peak = list.iter().filter_map(|meter| unsafe { meter.GetPeakValue().ok() }).fold(0.0f32, f32::max);
        Some(peak)
    }

    fn next_tick(&self) -> Option<Instant> {
        let mut next = self.next_poll.min(self.next_message);
        if self.fade.is_some() {
            next = next.min(self.frame_at);
        }
        if let Some(t) = self.hud_until {
            next = next.min(t);
        }
        Some(next)
    }

    fn tick(&mut self, _ctx: &mut Ctx) -> bool {
        let now = Instant::now();
        let mut changed = false;
        if let Some((_, since)) = &self.fade {
            changed = true; // um quadro da troca de fundo
            if since.elapsed().as_secs_f32() * 1000.0 >= FADE_MS {
                self.fade = None;
            }
            self.frame_at = now + Duration::from_millis(33);
        }
        if self.hud_until.is_some_and(|t| now >= t) {
            self.hud_until = None;
            changed = true;
        }
        if now >= self.next_poll {
            self.next_poll = now + POLL;
            if let Some(snap) = self.shared.lock().unwrap().clone() {
                // trocou de música: o fundo antigo some aos poucos por cima do novo
                let old_key = self.media.as_ref().and_then(|m| m.art_key.clone());
                if old_key.is_some() && snap.art_key != old_key {
                    if let Some((_, img)) = self.bg_cache.borrow().as_ref() {
                        self.fade = Some((img.clone(), now));
                    }
                    self.frame_at = now;
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // desenha num PNG: cargo test -- --ignored
    fn draw_cover() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        // uma "capa" de exemplo: laranja, rosa e roxo
        let mut art = Canvas::new(300, 300, 1.0);
        art.fill_round_rect(0.0, 0.0, 300.0, 300.0, 0.0, Rgba::rgb(106, 43, 184));
        art.fill_circle(80.0, 80.0, 90.0, Rgba::rgb(255, 122, 89));
        art.fill_circle(230.0, 220.0, 110.0, Rgba::rgb(194, 54, 122));
        art.fill_circle(90.0, 90.0, 30.0, Rgba::rgb(255, 214, 160));
        let art = Arc::new(art.finish());
        let cfg = json!({"midia": {}, "app": {}});
        for (mode, name) in [(Mode::P, "p"), (Mode::M, "m"), (Mode::G, "g")] {
            let mut m = Media::sample();
            if let Some(snap) = &mut m.media {
                snap.title = "Céu de Vidro".into();
                snap.artist = "Aurora Lima".into();
                snap.art = Some(art.clone());
                snap.art_key = Some("exemplo".into());
                snap.volume = Some(0.6);
                snap.shuffle = Some(false);
                snap.repeat = Some(2);
                snap.can_shuffle = true;
                snap.can_repeat = true;
            }
            let l = m.layout_of(mode);
            let mut c = Canvas::new((l.w * 1.25) as u32, (l.h * 1.25) as u32, 1.25);
            c.fill_round_rect(0.0, 0.0, l.w, l.h, l.radius, Rgba::rgb(40, 40, 50));
            Kind::draw(&m, &mut c, mode, &Env { cfg: &cfg, key: "midia" });
            let path = std::env::temp_dir().join(format!("capa_{name}.png"));
            crate::gfx::save_png(&c.finish(), &path).unwrap();
            println!("{}", path.display());
        }
    }
}
