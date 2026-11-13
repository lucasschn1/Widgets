//! Widget "Tocando agora" ("capa viva"): a mídia em reprodução no Windows (Spotify, YouTube no
//! navegador...). O fundo é a própria capa, desfocada; clicar na capa abre o player grande, com
//! volume do app, aleatório e repetir. A barra de progresso aceita clique para pular.
//!
//! A central de mídia do Windows (a mesma do controle de volume) é lida numa thread própria; o
//! resultado fica num "retrato" que a interface consulta 4 vezes por segundo.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
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

use super::{Ctx, Env, Hot, IslandInfo, Kind, Layout, MenuItem, Mode, Request, LARGE_H, MEDIUM_H, SMALL, WIDE};
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

// ---------- a capa ----------

/// Proporções até 50% diferentes da área (uma capa quadrada no grande, um vídeo 16:9 no grande):
/// a capa cobre a área inteira, cortando o mesmo tanto dos dois lados.
const COVER_SLACK: f32 = 1.5;
/// Quanto uma capa pode ser ampliada antes de os pixels aparecerem.
const MAX_ZOOM: f32 = 1.6;

/// A capa pronta para uma área de w×h pixels, sem deformar e centralizada: cobrindo a área
/// (recorte no meio) se as proporções são parecidas; senão (um vídeo 16:9 num quadrado, ou uma
/// capa pequena demais para ampliar), inteira no meio, sobre ela mesma desfocada e escurecida.
pub fn cover_image(art: &Image, w: u32, h: u32) -> Image {
    let (w, h) = (w.max(1), h.max(1));
    let (aw, ah) = (art.w.max(1) as f32, art.h.max(1) as f32);
    let ratio = (aw / ah) / (w as f32 / h as f32);
    let cover_zoom = (w as f32 / aw).max(h as f32 / ah);
    if ratio.max(1.0 / ratio) <= COVER_SLACK && cover_zoom <= MAX_ZOOM {
        return gfx::fit_cover(art, w, h);
    }
    // fundo: a própria capa, bem desfocada (em tamanho pequeno, que basta) e mais escura
    let (bw, bh) = ((w / 4).max(4), (h / 4).max(4));
    let mut small = gfx::fit_cover(art, bw, bh);
    blur(&mut small.px, bw, bh, 4, bh.min(bw) as f32 / 5.0);
    for p in small.px.chunks_mut(4) {
        for ch in 0..3 {
            p[ch] = (p[ch] as f32 * 0.55) as u8;
        }
        p[3] = 255;
    }
    let mut out = gfx::resize_photo(&small, w, h);
    let zoom = (w as f32 / aw).min(h as f32 / ah).min(MAX_ZOOM);
    let (fw, fh) = ((aw * zoom).round().max(1.0) as u32, (ah * zoom).round().max(1.0) as u32);
    let front = gfx::resize_photo(art, fw.min(w), fh.min(h));
    out.over(&front, ((w - front.w) / 2) as i32, ((h - front.h) / 2) as i32);
    out
}

/// A opção "Capa em alta do YouTube" (ligada, se nunca mudou).
pub fn hd_covers(cfg: &Value) -> bool {
    cfg["midia"].bool_of("capa_hd").unwrap_or(true)
}

/// Imagens mais largas ou mais altas que isto (um panorama) são cortadas nas pontas, para a
/// capa não virar uma tira fina.
const ART_ASPECT: (f32, f32) = (0.5, 2.4);

/// A moldura de uma capa com a proporção `aspect` (largura / altura) dentro da área
/// (x, y, largura, altura, raio): a maior que cabe, centralizada.
fn fit_art(area: (f32, f32, f32, f32, f32), aspect: f32) -> (f32, f32, f32, f32, f32) {
    let (x, y, w, h, radius) = area;
    let aspect = if aspect.is_finite() { aspect.clamp(ART_ASPECT.0, ART_ASPECT.1) } else { 1.0 };
    let (fw, fh) = if aspect >= w / h { (w, w / aspect) } else { (h * aspect, h) };
    // cantos proporcionais ao lado menor, para uma capa baixa não parecer uma pílula
    let radius = radius.min(fw.min(fh) * 0.2);
    (x + (w - fw) / 2.0, y + (h - fh) / 2.0, fw, fh, radius)
}

/// Capa em alta resolução para vídeos do YouTube. O navegador passa para o Windows só uma
/// miniatura (o Edge, 150 × 83), que fica pixelada no widget: aqui se acha o vídeo pelo título e
/// pelo canal e baixa a miniatura original na maior resolução que houver, conferindo que é a
/// mesma imagem (senão fica a do navegador).
mod youtube {
    use super::*;
    use crate::util;

    /// Do maior para o menor: 1280 × 720, 640 × 480, 480 × 360 e 320 × 180.
    const SIZES: [&str; 4] = ["maxresdefault", "sddefault", "hqdefault", "mqdefault"];
    /// diferença média (0 a 255) até a qual duas miniaturas são a mesma imagem
    const SAME: f32 = 20.0;

    /// Parece a miniatura de um vídeo e é pequena: vale procurar a original.
    pub fn worth_upgrading(img: &Image) -> bool {
        let aspect = img.w as f32 / img.h.max(1) as f32;
        img.w < 640 && (1.25..=1.9).contains(&aspect)
    }

    /// Os primeiros `n` vídeos de uma página de busca do YouTube.
    pub fn video_ids(html: &str, n: usize) -> Vec<String> {
        let tag = "\"videoRenderer\":{\"videoId\":\"";
        let mut ids: Vec<String> = Vec::new();
        for (i, _) in html.match_indices(tag) {
            let id: String = html[i + tag.len()..].chars().take_while(|c| *c != '"').collect();
            let valid = id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if valid && !ids.contains(&id) {
                ids.push(id);
                if ids.len() == n {
                    break;
                }
            }
        }
        ids
    }

    /// As duas são a mesma imagem (em qualquer tamanho): comparadas pequenas, na proporção da
    /// primeira.
    pub fn same_picture(small: &Image, big: &Image) -> bool {
        let aspect = small.w as f32 / small.h.max(1) as f32;
        let (w, h) = (32, ((32.0 / aspect).round() as u32).clamp(8, 64));
        let (a, b) = (gfx::fit_cover(small, w, h), gfx::fit_cover(big, w, h));
        let diff: u64 = a.px.chunks(4).zip(b.px.chunks(4)).map(|(p, q)| (0..3).map(|c| p[c].abs_diff(q[c]) as u64).sum::<u64>()).sum();
        diff as f32 / (w * h * 3) as f32 <= SAME
    }

    /// A miniatura original do vídeo que está tocando (já sem faixas pretas), ou None.
    pub fn hd_thumbnail(title: &str, channel: &str, small: &Image) -> Option<Image> {
        let query = format!("{title} {channel}");
        let url = format!("https://www.youtube.com/results?search_query={}", util::urlencode(query.trim()));
        let (status, html) = util::fetch_text(&url, &[("Accept-Language", "pt-BR,pt;q=0.9")]).ok()?;
        if status >= 400 {
            return None;
        }
        for id in video_ids(&html, 3) {
            for size in SIZES {
                let Ok((200, bytes)) = util::fetch_bytes(&format!("https://i.ytimg.com/vi/{id}/{size}.jpg")) else { continue };
                // quando o tamanho não existe, vem uma imagem cinza de 120 × 90
                let Some(img) = gfx::decode_image(&bytes).filter(|i| i.w >= 320) else { continue };
                let img = gfx::trim_bars(&img);
                if same_picture(small, &img) {
                    return Some(img);
                }
                break; // os outros tamanhos deste vídeo são a mesma imagem: tenta o próximo vídeo
            }
        }
        None
    }
}

struct Reader {
    art_key: Option<String>,
    art: Option<Arc<Image>>,
    /// a capa em alta já chegou (muda a chave da capa, para os desenhos se refazerem)
    art_hd: bool,
    /// capa em alta sendo procurada: (música, resposta)
    upgrade: Option<(String, std::sync::mpsc::Receiver<Option<Image>>)>,
    /// pode procurar a capa em alta no youtube.com (opção "Capa em alta do YouTube")
    hd_allowed: Arc<AtomicBool>,
}

/// Vale procurar a capa em alta? Só de vídeo do YouTube com miniatura pequena, e só com a opção
/// ligada (a busca manda o título e o canal do vídeo para o youtube.com).
fn wants_hd(kind: &str, art: &Image, allowed: bool) -> bool {
    allowed && kind == "youtube" && youtube::worth_upgrading(art)
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
            // sem as faixas pretas de vídeos 16:9 em miniaturas 4:3: senão a capa fica deslocada
            self.art = read_art(&props).map(|img| Arc::new(gfx::trim_bars(&img)));
            self.art_hd = false;
            self.upgrade = None;
            let allowed = self.hd_allowed.load(Ordering::Relaxed);
            let small = self.art.clone().filter(|a| wants_hd(media_kind(&app_id), a, allowed));
            if let Some(small) = small {
                let (tx, rx) = channel();
                let (title, artist) = (title.clone(), artist.clone());
                std::thread::spawn(move || {
                    unsafe {
                        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
                    }
                    let _ = tx.send(youtube::hd_thumbnail(&title, &artist, &small));
                });
                self.upgrade = Some((key.clone(), rx));
            }
        }
        if let Some((for_key, rx)) = &self.upgrade {
            if *for_key == key {
                if let Ok(found) = rx.try_recv() {
                    if let Some(img) = found {
                        self.art = Some(Arc::new(img));
                        self.art_hd = true;
                    }
                    self.upgrade = None;
                }
            }
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
            art_key: self.art.as_ref().map(|_| if self.art_hd { format!("{key}\u{1}hd") } else { key }),
            error: None,
        })
    }
}

fn watch(rx: std::sync::mpsc::Receiver<Cmd>, snapshot: Arc<Mutex<Option<Snapshot>>>, pref: Arc<Mutex<String>>, hd_allowed: Arc<AtomicBool>) {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let mut manager: Option<Manager> = None;
    let mut current: Option<Session> = None;
    let mut reader = Reader { art_key: None, art: None, art_hd: false, upgrade: None, hd_allowed };
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

const W: f32 = WIDE;
// grande: a capa grande em cima; embaixo, a fonte, título e
// artista, a barra, aleatório/controles/repetir numa linha só e o volume do app no pé
const G_ART: (f32, f32, f32, f32, f32) = (16.0, 16.0, W - 32.0, 240.0, 18.0); // x, y, largura, altura, raio
const G_TEXT_Y: f32 = 266.0;
const G_VOL_Y: f32 = LARGE_H - 26.0;
/// aleatório e repetir: nas pontas da linha dos controles
const G_TOG_X: f32 = 34.0;
const VOL_X: (f32, f32) = (44.0, W - 44.0);
const ART: f32 = 76.0;
const ART_RADIUS: f32 = 14.0;
const ART_XY: (f32, f32) = (18.0, 18.0);
const TEXT_X: f32 = 18.0;
const TEXT_W: f32 = W - 36.0;
const BAR_Y: f32 = 328.0;
const CTRL_Y: f32 = 372.0;
const PLAY_D: f32 = 44.0;
// x, y, largura, altura do equalizador (no meio da capa)
const IDLE_BOX: (f32, f32, f32, f32) = (36.0, G_ART.1 + (G_ART.3 - 100.0) / 2.0, W - 72.0, 100.0);

// pequeno (quadrado): capa e tocar em cima, título e artista embaixo, barra fina no pé
const C_ART: f32 = 64.0;
const C_ART_RADIUS: f32 = 12.0;
const C_ART_XY: (f32, f32) = (14.0, 14.0);
const C_TEXT_W: f32 = SMALL - 28.0;
const C_PLAY_D: f32 = 36.0;
const C_BAR_Y: f32 = 138.0;
const C_IDLE_BOX: (f32, f32, f32, f32) = (14.0 + 12.0, 14.0 + 12.0, 40.0, 40.0); // equalizador no lugar da capa

// médio: a capa na altura toda à esquerda; à direita, fonte, título e artista, os controles e
// a barra de progresso alinhada com o pé da capa
const M_ART: f32 = MEDIUM_H - 28.0;
const M_ART_RADIUS: f32 = 14.0;
const M_ART_XY: (f32, f32) = (14.0, 14.0);
const M_TEXT_X: f32 = M_ART_XY.0 + M_ART + 16.0;
const M_TEXT_W: f32 = W - M_TEXT_X - 16.0;
const M_CTRL_Y: f32 = 96.0;
const M_PLAY_D: f32 = 36.0;
const M_BAR_Y: f32 = MEDIUM_H - 17.0;
const M_IDLE_D: f32 = 56.0;
const M_IDLE_BOX: (f32, f32, f32, f32) = (M_ART_XY.0 + (M_ART - M_IDLE_D) / 2.0, M_ART_XY.1 + (M_ART - M_IDLE_D) / 2.0, M_IDLE_D, M_IDLE_D);

const POLL: Duration = Duration::from_millis(250);
const MESSAGE_EVERY: Duration = Duration::from_secs(12);
const FADE_MS: f32 = 600.0;
/// o fundo é desenhado um pouco maior que o cartão (as bordas da ampliação ficam de fora)
const BG_ZOOM: f32 = 1.2;
/// quantos fundos ficam guardados (um para cada tamanho do cartão)
const BG_SIZES: usize = 3;
const HUD: Duration = Duration::from_millis(1100);

// título comprido demais: ele dá uma volta para o lado (até mostrar o fim e o começo de novo)
// quando a música troca e quando o mouse entra no widget; no resto do tempo fica parado, com
// "...", sem gastar quadros
/// pontos por segundo
const MARQUEE_SPEED: f32 = 30.0;
/// segundos parado, depois que a música troca, antes da volta (para ler o começo)
const MARQUEE_PAUSE: f32 = 2.5;
/// espaço entre o fim do título e o começo dele de novo
const MARQUEE_GAP: f32 = 40.0;
const MARQUEE_FPS: u32 = 30;

/// Quanto o título andou para a esquerda, `elapsed` segundos depois do começo da volta, se a
/// volta (o título e o espaço) mede `span` pontos. Fora da volta (antes ou depois), parado: None.
fn marquee_offset(elapsed: f32, span: f32) -> Option<f32> {
    let offset = elapsed * MARQUEE_SPEED;
    (elapsed > 0.0 && offset < span).then_some(offset)
}

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
    art_cache: RefCell<Option<((String, u32, u32), Image)>>,
    /// fundo: a capa desfocada, um pouco maior que o cartão; um por tamanho (a troca de
    /// tamanho desenha os dois), o último usado primeiro
    bg_cache: RefCell<Vec<((String, u32, u32), Image)>>,
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
    /// a opção "Capa em alta do YouTube", dividida com a thread que lê a mídia
    hd_allowed: Arc<AtomicBool>,
    /// o título não coube no último desenho: o tamanho de uma volta dele (título e espaço), em
    /// pontos (0: coube, ou não há o fundo da capa para repintar embaixo dele)
    marquee: Cell<f32>,
    /// o tamanho do texto do último desenho (a camada animada usa o mesmo)
    text_scale: Cell<f32>,
    /// a volta do título: (título, quando ela começa)
    lap: RefCell<Option<(String, Instant)>>,
    /// o título estava dando a volta no último `tick` (para avisar o cartão quando muda)
    sliding: Cell<bool>,
}

impl Media {
    pub fn new(cfg: &Value) -> Media {
        let pref = Arc::new(Mutex::new(cfg["midia"].str_of("fonte").unwrap_or("auto").to_string()));
        let shared = Arc::new(Mutex::new(None));
        let hd_allowed = Arc::new(AtomicBool::new(hd_covers(cfg)));
        let (tx, rx) = channel();
        {
            let (shared, pref, hd_allowed) = (shared.clone(), pref.clone(), hd_allowed.clone());
            std::thread::spawn(move || watch(rx, shared, pref, hd_allowed));
        }
        let seed = (win::now() * 1000.0) as usize;
        let mut m = Media::with(tx, shared, pref, None, seed % FUNNY.len(), Instant::now());
        m.hd_allowed = hd_allowed;
        m
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
            bg_cache: RefCell::new(Vec::new()),
            fade: None,
            idle_cache: RefCell::new(HashMap::new()),
            started: Instant::now(),
            hud_until: None,
            volume_wanted: None,
            frame_at: next,
            meters: RefCell::new(None),
            colors: RefCell::new(None),
            hd_allowed: Arc::new(AtomicBool::new(true)),
            marquee: Cell::new(0.0),
            text_scale: Cell::new(1.0),
            lap: RefCell::new(None),
            sliding: Cell::new(false),
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

    /// Exemplo sem nada tocando (o equalizador cochilando).
    #[cfg(test)]
    pub fn sample_idle() -> Media {
        let mut m = Media::sample();
        m.media = None;
        m
    }

    /// Exemplo tocando com uma capa lisa de w×h pixels (300×300 do Spotify, 150×84 do navegador).
    #[cfg(test)]
    pub fn sample_art(kind: &'static str, w: u32, h: u32) -> Media {
        let mut art = Image::new(w, h);
        for p in art.px.chunks_mut(4) {
            p.copy_from_slice(&[40, 220, 40, 255]);
        }
        Media::sample_song(kind, "Nome da música", "Artista", Arc::new(art), &format!("capa{w}x{h}"), 74.0, 213.0)
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

    /// (x, y, largura, altura, raio) da área da capa: quadrada no pequeno e no médio, larga no
    /// grande.
    fn art_box(&self, mode: Mode) -> (f32, f32, f32, f32, f32) {
        if mode == Mode::G {
            return G_ART;
        }
        let ((x, y), size, r) = self.art_geom(mode);
        (x, y, size, size, r)
    }

    /// (x, y, largura, altura, raio) da capa: na proporção da própria imagem, o maior possível
    /// dentro da área e centralizada nela (inteira, sem cortes nem faixas). Sem imagem, a área.
    fn art_rect(&self, mode: Mode) -> (f32, f32, f32, f32, f32) {
        let area = self.art_box(mode);
        match self.media.as_ref().filter(|m| !m.empty).and_then(|m| m.art.as_ref()) {
            Some(art) => fit_art(area, art.w as f32 / art.h.max(1) as f32),
            None => area,
        }
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
                let (cx, y) = (M_TEXT_X + M_TEXT_W / 2.0, M_CTRL_Y);
                vec![
                    (Ctl::Prev, cx - 50.0, y, 16.0, m.can_prev, false),
                    (toggle, cx, y, M_PLAY_D, m.can_toggle, true),
                    (Ctl::Next, cx + 50.0, y, 16.0, m.can_next, false),
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

    /// (ação, x0, y0, x1, y1) dos botões extras do grande: aleatório e repetir nas pontas da
    /// linha dos controles.
    fn toggles(&self) -> Vec<(u32, (f32, f32, f32, f32))> {
        if !self.has_media() {
            return Vec::new();
        }
        let square = |cx: f32| (cx - 18.0, CTRL_Y - 16.0, cx + 18.0, CTRL_Y + 16.0);
        vec![(SHUFFLE, square(G_TOG_X)), (REPEAT, square(W - G_TOG_X))]
    }

    fn toggle_at(&self, x: f32, y: f32, mode: Mode) -> Option<(u32, (f32, f32, f32, f32))> {
        if mode != Mode::G {
            return None;
        }
        let m = self.media.as_ref()?;
        self.toggles().into_iter().find(|&(a, (x0, y0, x1, y1))| {
            let enabled = match a {
                SHUFFLE => m.can_shuffle && m.shuffle.is_some(),
                REPEAT => m.can_repeat && m.repeat.is_some(),
                _ => true,
            };
            enabled && (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
        })
    }

    /// Capa arredondada, feita no tamanho exato da área em pixels (guardada até mudar).
    fn paste_art(&self, c: &mut Canvas, mode: Mode) -> bool {
        let Some(m) = self.media.as_ref().filter(|m| !m.empty) else { return false };
        let (Some(art), Some(key)) = (&m.art, &m.art_key) else { return false };
        let (x, y, w, h, radius) = self.art_rect(mode);
        // os quatro lados em pixels inteiros: a imagem cai pixel a pixel, sem ser esticada de novo
        let s = c.s;
        let (px0, py0) = ((x * s).round(), (y * s).round());
        let (pw, ph) = (((x + w) * s).round() - px0, ((y + h) * s).round() - py0);
        let cache_key = (key.clone(), pw as u32, ph as u32);
        let mut cache = self.art_cache.borrow_mut();
        if cache.as_ref().map(|c| &c.0) != Some(&cache_key) {
            // a moldura já tem a proporção da capa: ela entra inteira, mesmo que precise ampliar
            // (uma miniatura pequena do navegador fica um pouco suave, mas ocupa o lugar todo)
            *cache = Some((cache_key, gfx::fit_cover(art, pw as u32, ph as u32)));
        }
        c.push_round_clip(x, y, x + w, y + h, radius);
        c.image_px(&cache.as_ref().unwrap().1, px0, py0, pw, ph, 1.0);
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
        let entry = match cache.iter().position(|c| c.0 == cache_key) {
            Some(i) => cache.remove(i),
            None => (cache_key, live_background(art, w, h)),
        };
        let img = entry.1.clone();
        cache.insert(0, entry);
        cache.truncate(BG_SIZES);
        Some(img)
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
                // sem o volume do app: a mesma linha, apagada (a roda do mouse continua mudando)
                icon(c, 24.0, G_VOL_Y, "\u{e992}", 14.0, FAINT);
                icon(c, W - 24.0, G_VOL_Y, "\u{e995}", 14.0, FAINT);
                c.fill_round_rect(x0, G_VOL_Y - 2.5, x1, G_VOL_Y + 2.5, 2.5, WHITE.alpha(0.12));
            }
        }
        for (action, (x0, y0, x1, y1)) in self.toggles() {
            let (cx, y) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
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
    }

    fn layout_of(&self, mode: Mode) -> Layout {
        Layout::of(mode)
    }

    /// (x, y, largura, tamanho da letra) do título da música.
    fn title_spot(mode: Mode) -> (f32, f32, f32, f32) {
        match mode {
            Mode::P => (14.0, 90.0, C_TEXT_W, 12.5),
            Mode::M => (M_TEXT_X, 30.0, M_TEXT_W, 13.5),
            Mode::G => (TEXT_X, G_TEXT_Y + 15.0, TEXT_W, 15.5),
        }
    }

    /// O título, parado (com "..." se não couber). Se não cabe e há o fundo da capa, a camada
    /// animada o faz dar uma volta (`overlay_title`); aqui fica a versão parada, que aparece fora
    /// das voltas, nas animações do cartão e com a economia de energia do Windows ligada.
    fn draw_title(&self, c: &mut Canvas, m: &Snapshot, mode: Mode, has_bg: bool) {
        let (x, y, w, size) = Self::title_spot(mode);
        c.text(x, y, &m.title, Face::Semibold, size, FG, Anchor::LA, Some(w));
        let tw = c.text_width(&m.title, Face::Semibold, size);
        self.marquee.set(if has_bg && tw > w { tw + MARQUEE_GAP } else { 0.0 });
        self.text_scale.set(c.text_scale);
        // música nova (ou título novo): uma volta, depois de dar tempo de ler o começo
        let mut lap = self.lap.borrow_mut();
        if lap.as_ref().map(|l| l.0.as_str()) != Some(m.title.as_str()) {
            *lap = Some((m.title.clone(), Instant::now() + Duration::from_secs_f32(MARQUEE_PAUSE)));
        }
    }

    /// Quanto o título comprido andou agora (None: parado, fora da volta, ou ele cabe).
    fn marquee_now(&self) -> Option<f32> {
        let span = self.marquee.get();
        let m = self.media.as_ref().filter(|m| !m.empty && span > 0.0)?;
        let lap = self.lap.borrow();
        let (_, start) = lap.as_ref().filter(|l| l.0 == m.title)?;
        let now = Instant::now();
        if now < *start {
            return None;
        }
        marquee_offset((now - *start).as_secs_f32().max(f32::MIN_POSITIVE), span)
    }

    /// O mouse entrou no widget: o título comprido dá uma volta (se já não está dando).
    fn slide_title(&self) -> bool {
        if self.marquee.get() <= 0.0 || self.marquee_now().is_some() {
            return false;
        }
        let Some(m) = self.media.as_ref().filter(|m| !m.empty) else { return false };
        *self.lap.borrow_mut() = Some((m.title.clone(), Instant::now()));
        true
    }

    /// O título deslizando (só quando ele não cabe): repinta o fundo na faixa dele, por cima do
    /// título parado, e escreve o título deslocado, com uma segunda cópia logo atrás.
    fn overlay_title(&self, c: &mut Canvas, mode: Mode, ox: f32, oy: f32) {
        let (Some(m), Some(offset)) = (self.media.as_ref(), self.marquee_now()) else { return };
        let span = self.marquee.get();
        let scale = std::mem::replace(&mut c.text_scale, self.text_scale.get());
        let (x, y, w, size) = Self::title_spot(mode);
        // a faixa do título, um pouco mais alta que a letra (acentos e a perna do "g")
        let (x0, y0, x1, y1) = (ox + x, oy + y - 3.0, ox + x + w, oy + y + size * 1.6);
        c.push_round_clip(x0, y0, x1, y1, 0.0);
        self.paint_bg(c, mode, ox, oy);
        for k in 0..2 {
            c.text(x0 - offset + k as f32 * span, oy + y, &m.title, Face::Semibold, size, FG, Anchor::LA, None);
        }
        c.pop_clip();
        c.text_scale = scale;
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
                    self.draw_title(c, m, mode, has_bg);
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
                    c.text(x, 16.0, source_label(m.kind), Face::Semibold, 9.5, source_color(m.kind), Anchor::LA, None);
                    self.draw_title(c, m, mode, has_bg);
                    c.text(x, 50.0, &m.artist, Face::Regular, 11.5, MUTED, Anchor::LA, Some(w));
                } else {
                    let kind = self.pref();
                    c.text(x, 16.0, source_label(&kind), Face::Semibold, 9.5, source_color(&kind), Anchor::LA, None);
                    c.text(x, 30.0, &funny_title, Face::Semibold, 13.5, FG, Anchor::LA, Some(w));
                    for (i, line) in c.wrap(&funny_sub, Face::Regular, 11.5, w, 3).iter().enumerate() {
                        c.text(x, 50.0 + 16.0 * i as f32, line, Face::Regular, 11.5, MUTED, Anchor::LA, Some(w));
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
                    self.draw_title(c, m, mode, has_bg);
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

    /// O quanto o aviso de volume aparece agora (0 a 1): inteiro e, no fim, sumindo.
    fn hud_alpha(&self) -> f32 {
        self.hud_until.filter(|t| Instant::now() < *t).map_or(0.0, |until| (until.saturating_duration_since(Instant::now()).as_secs_f32() / 0.25).min(1.0))
    }

    fn overlay_rest(&self, c: &mut Canvas, mode: Mode, ox: f32, oy: f32) {
        let lw = self.layout_of(mode).w;
        if self.hud_until.is_some_and(|t| Instant::now() < t) {
            let alpha = self.hud_alpha();
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

    fn draw(&self, c: &mut Canvas, mode: Mode, _env: &Env) {
        // fundo "capa viva" (com a capa anterior sumindo, logo depois de trocar de música)
        let has_bg = self.paint_bg(c, mode, 0.0, 0.0);
        self.draw_fg(c, mode, has_bg);
    }

    /// O título comprido deslizando, o equalizador "cochilando" quando nada está tocando e o
    /// aviso de volume.
    fn overlay(&self, c: &mut Canvas, mode: Mode, ox: f32, oy: f32) {
        self.overlay_title(c, mode, ox, oy);
        self.overlay_rest(c, mode, ox, oy);
    }

    fn pointer_entered(&mut self) -> bool {
        self.slide_title()
    }

    fn overlay_fps(&self) -> u32 {
        if self.hud_until.is_some_and(|t| Instant::now() < t) {
            30
        } else if self.has_media() {
            // quadros só enquanto o título dá a volta
            if self.marquee_now().is_some() { MARQUEE_FPS } else { 0 }
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
        if let Some((a, _)) = self.toggle_at(x, y, mode) {
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
        if let Some((_, (x0, y0, x1, y1))) = self.toggle_at(x, y, mode) {
            return Some((x0, y0, x1, y1, 10.0));
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

    fn hover_style(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> super::HoverStyle {
        if self.control_at(x, y, mode).is_none() && self.on_art(x, y, mode) { super::HoverStyle::Quiet } else { super::HoverStyle::Surface }
    }

    fn tooltip(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> Option<String> {
        let m = self.media.as_ref().filter(|m| !m.empty)?;
        if self.control_at(x, y, mode).is_some() {
            return None;
        }
        if let Some((a, _)) = self.toggle_at(x, y, mode) {
            return match (a, m.repeat) {
                (REPEAT, Some(1)) => Some("Repetindo esta música".into()),
                (REPEAT, Some(2)) => Some("Repetindo a lista".into()),
                _ => None,
            };
        }
        if let Some(v) = self.volume_at(x, y, mode) {
            return Some(format!("Volume {}%", (v * 100.0).round()));
        }
        if let Some(f) = self.bar_at(x, y, mode) {
            return m.can_seek.then(|| fmt_time(m.duration * f as f64));
        }
        // o nome inteiro, só quando ele aparece cortado
        let (width, size) = match mode {
            Mode::P => (C_TEXT_W, 12.5),
            Mode::M => (M_TEXT_W, 13.5),
            Mode::G => (TEXT_W, 15.5),
        };
        let probe = Canvas::new(1, 1, 1.0);
        if probe.text_width(&m.title, Face::Semibold, size) <= width {
            return None;
        }
        let song = probe.wrap(&m.title, Face::Regular, 11.5, 300.0, 3).join("\n");
        Some(if m.artist.is_empty() { song } else { format!("{song}\n\n{}", m.artist) })
    }

    /// Roda do mouse: o volume do app que está tocando (ou o do Windows, se não achar o app).
    /// O aviso de volume aparece em cima, menos no grande, que já tem o volume no pé.
    fn uses_wheel(&self, _x: f32, y: f32, mode: Mode) -> bool {
        // numa pilha, o volume pela roda só em cima da barra de volume do grande
        mode == Mode::G && (G_VOL_Y - 12.0..=G_VOL_Y + 12.0).contains(&y) && self.volume().is_some()
    }

    fn wheel(&mut self, _x: f32, _y: f32, steps: f32, mode: Mode, _ctx: &mut Ctx) {
        match self.volume() {
            Some(v) => {
                let level = (v + steps * 0.04).clamp(0.0, 1.0);
                let _ = self.tx.send(Cmd::Volume(level));
                self.volume_wanted = Some((level, Instant::now()));
                if mode != Mode::G {
                    self.hud_until = Some(Instant::now() + HUD);
                }
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
            volume: self.volume(),
            volume_hud: self.hud_alpha(),
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
        // o começo e o fim da volta do título, na hora certa
        if let Some((_, start)) = self.lap.borrow().as_ref().filter(|_| self.marquee.get() > 0.0) {
            let end = *start + Duration::from_secs_f32(self.marquee.get() / MARQUEE_SPEED);
            if let Some(t) = [*start, end].into_iter().find(|t| *t > Instant::now()) {
                next = next.min(t);
            }
        }
        Some(next)
    }

    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        let now = Instant::now();
        // a opção pode mudar nas configurações a qualquer momento
        self.hd_allowed.store(hd_covers(ctx.cfg), Ordering::Relaxed);
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
        // a volta do título começou ou acabou: o cartão liga ou desliga os quadros
        let sliding = self.marquee_now().is_some();
        if sliding != self.sliding.get() {
            self.sliding.set(sliding);
            changed = true;
        }
        if now >= self.next_poll {
            self.next_poll = now + POLL;
            if let Some(snap) = self.shared.lock().unwrap().clone() {
                // trocou de música: o fundo antigo some aos poucos por cima do novo
                let old_key = self.media.as_ref().and_then(|m| m.art_key.clone());
                if old_key.is_some() && snap.art_key != old_key {
                    if let Some((_, img)) = self.bg_cache.borrow().first() {
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

    /// O fundo de cada tamanho fica guardado: a troca de tamanho desenha os dois sem desfocar a
    /// capa de novo, e o último usado é o que some aos poucos quando a música muda.
    #[test]
    fn each_size_keeps_its_background() {
        let m = Media::sample_art("spotify", 300, 300);
        let (p, g) = (m.background(1.0, Mode::P).unwrap(), m.background(1.0, Mode::G).unwrap());
        assert!(p.w != g.w || p.h != g.h);
        let p2 = m.background(1.0, Mode::P).unwrap();
        assert_eq!(m.bg_cache.borrow().len(), 2, "pequeno e grande");
        assert_eq!((m.bg_cache.borrow()[0].0 .1, p2.w), (p.w, p.w), "o pequeno, o último usado, primeiro");
        for mode in [Mode::M, Mode::G, Mode::P, Mode::M] {
            m.background(1.0, mode);
        }
        assert_eq!(m.bg_cache.borrow().len(), BG_SIZES);
    }

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

#[cfg(test)]
mod session_tests {
    use super::*;

    /// Lista as sessões de mídia abertas no Windows e salva a capa de cada uma, para ver de onde
    /// ela vem e em que resolução (%TEMP%\capa_sessao_N.png):
    ///   cargo test sessoes -- --ignored --nocapture
    #[test]
    #[ignore]
    fn sessoes() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let manager = Manager::RequestAsync().unwrap().join().unwrap();
        for (i, session) in manager.GetSessions().unwrap().into_iter().enumerate() {
            let app = session.SourceAppUserModelId().map(|s| s.to_string()).unwrap_or_default();
            let props = session.TryGetMediaPropertiesAsync().unwrap().join().unwrap();
            let title = props.Title().map(|s| s.to_string()).unwrap_or_default();
            let artist = props.Artist().map(|s| s.to_string()).unwrap_or_default();
            let Some(img) = read_art(&props) else {
                println!("{i}: [{}] {app} | {title} | {artist} | sem capa", media_kind(&app));
                continue;
            };
            let path = std::env::temp_dir().join(format!("capa_sessao_{i}.png"));
            gfx::save_png(&img, &path).unwrap();
            println!("{i}: [{}] {app} | {title} | {artist} | capa {}x{} -> {}", media_kind(&app), img.w, img.h, path.display());
            let small = gfx::trim_bars(&img);
            let mut arts = vec![("navegador", small.clone())];
            if media_kind(&app) == "youtube" && youtube::worth_upgrading(&small) {
                match youtube::hd_thumbnail(&title, &artist, &small) {
                    Some(hd) => {
                        println!("   em alta: {}x{}", hd.w, hd.h);
                        arts.push(("alta", hd));
                    }
                    None => println!("   em alta: não achou"),
                }
            }
            // o widget nos três tamanhos com cada capa
            for (name, art) in arts {
                let art = Arc::new(art);
                for mode in Mode::ALL {
                    let mut m = Media::sample();
                    if let Some(snap) = &mut m.media {
                        (snap.title, snap.artist, snap.kind) = (title.clone(), artist.clone(), media_kind(&app));
                        snap.art = Some(art.clone());
                        snap.art_key = Some(name.into());
                    }
                    let l = Layout::of(mode);
                    let mut c = Canvas::new((l.w * 1.25).round() as u32, (l.h * 1.25).round() as u32, 1.25);
                    c.fill_round_rect(0.0, 0.0, l.w, l.h, l.radius, Rgba::rgb(40, 40, 50));
                    Kind::draw(&m, &mut c, mode, &Env { cfg: &json!({"midia": {}, "app": {}}), key: "midia" });
                    let out = std::env::temp_dir().join(format!("capa_sessao_{i}_{name}_{}.png", mode.code()));
                    gfx::save_png(&c.finish(), &out).unwrap();
                }
            }
        }
    }
}

#[cfg(test)]
mod cover_tests {
    use super::*;

    fn com() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
    }

    /// Degradê com um ponto claro no meio (uma "miniatura" qualquer).
    fn picture(w: u32, h: u32, seed: u32) -> Image {
        let mut img = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let v = |k: u32| ((x * (k + seed) / w.max(1) * 40 + y * (3 + k * seed) / h.max(1) * 30) % 256) as u8;
                img.px[i..i + 4].copy_from_slice(&[v(1), v(2), v(3), 255]);
            }
        }
        img
    }

    #[test]
    fn video_in_a_square_is_shown_whole_and_centered() {
        com();
        // um vídeo 16:9 todo verde: no quadrado, a faixa verde clara é ele inteiro (158 × 89), no
        // meio; em cima e embaixo fica o fundo (ele mesmo desfocado e mais escuro)
        let mut art = Image::new(1280, 720);
        for p in art.px.chunks_mut(4) {
            p.copy_from_slice(&[40, 220, 40, 255]);
        }
        let out = cover_image(&art, 158, 158);
        assert_eq!((out.w, out.h), (158, 158));
        let bright: Vec<u32> = (0..out.h).filter(|&y| out.px[((y * out.w + 79) * 4 + 1) as usize] > 200).collect();
        let (y0, y1) = (bright[0], *bright.last().unwrap());
        assert_eq!(y1 - y0 + 1, 89, "a faixa tem a altura do vídeo inteiro");
        assert!((y0 + y1).abs_diff(157) <= 1, "faixa centralizada: {y0}..{y1}");
        assert!(out.px[((5 * out.w + 79) * 4 + 1) as usize] < 140, "fundo mais escuro");
    }

    #[test]
    fn square_cover_fills_square_area() {
        com();
        let art = picture(300, 300, 2);
        let direct = gfx::fit_cover(&art, 158, 158);
        assert_eq!(cover_image(&art, 158, 158).px, direct.px, "capa quadrada num quadrado: só redimensiona");
    }

    #[test]
    fn tiny_cover_is_not_blown_up() {
        com();
        // a miniatura do Edge (150 × 83) no grande, a 125% (360 × 250): no máximo 1,6 vez maior
        let out = cover_image(&picture(150, 83, 3), 360, 250);
        assert_eq!((out.w, out.h), (360, 250));
        assert!(!youtube::worth_upgrading(&picture(1280, 720, 1)), "já está em alta");
        assert!(youtube::worth_upgrading(&picture(150, 83, 1)));
        assert!(!youtube::worth_upgrading(&picture(300, 300, 1)), "capa quadrada de música");
    }

    /// A capa desenhada no widget, numa escala de 1,25: o retângulo (x0, y0, x1, y1), em pontos,
    /// com exatamente a cor da capa (o fundo é feito dela, mas desfocado, saturado e escurecido).
    fn drawn_art(m: &Media, mode: Mode) -> (f32, f32, f32, f32) {
        let l = Layout::of(mode);
        let s = 1.25;
        let mut c = Canvas::new((l.w * s).round() as u32, (l.h * s).round() as u32, s);
        Kind::draw(m, &mut c, mode, &Env { cfg: &serde_json::json!({"midia": {}, "app": {}}), key: "midia" });
        let img = c.finish();
        let green = |x: u32, y: u32| {
            let i = ((y * img.w + x) * 4) as usize;
            [40u8, 220, 40].iter().zip(&img.px[i..i + 3]).all(|(a, b)| a.abs_diff(*b) <= 4)
        };
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..img.h {
            for x in 0..img.w {
                if green(x, y) {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
        }
        assert!(x0 < x1, "a capa não apareceu");
        (x0 as f32 / s, y0 as f32 / s, x1 as f32 / s, y1 as f32 / s)
    }

    #[test]
    fn cover_is_whole_in_every_size() {
        com();
        // capa quadrada do Spotify, miniatura do navegador e miniatura em alta do YouTube
        for (kind, w, h) in [("spotify", 300, 300), ("youtube", 150, 84), ("youtube", 1280, 720)] {
            let m = Media::sample_art(kind, w, h);
            let aspect = w as f32 / h as f32;
            for mode in Mode::ALL {
                let (bx, by, bw, bh, _) = m.art_box(mode);
                let (x, y, fw, fh, _) = m.art_rect(mode);
                // a moldura tem a proporção da capa (nada é cortado), cabe na área e encosta
                // nela em um dos sentidos (o maior tamanho possível), centralizada
                assert!((fw / fh - aspect).abs() < 0.01, "{kind} {w}x{h} {mode:?}: moldura {fw}x{fh}");
                assert!(fw <= bw + 0.01 && fh <= bh + 0.01, "{kind} {w}x{h} {mode:?}: fora da área");
                assert!((fw - bw).abs() < 0.01 || (fh - bh).abs() < 0.01, "{kind} {w}x{h} {mode:?}: menor que o possível");
                assert!(((x - bx) - (bx + bw - x - fw)).abs() < 0.01 && ((y - by) - (by + bh - y - fh)).abs() < 0.01, "{kind} {mode:?}: fora do centro");
                // e é isso que aparece desenhado: a capa inteira ocupa a moldura toda (sem
                // faixas em volta nem pedaço de fora); os cantos são arredondados, então mede
                // pelo meio de cada lado
                let (dx0, dy0, dx1, dy1) = drawn_art(&m, mode);
                let tol = 1.5;
                assert!((dx0 - x).abs() <= tol && (dx1 - (x + fw)).abs() <= tol, "{kind} {w}x{h} {mode:?}: largura desenhada {dx0}..{dx1}, moldura {x}..{}", x + fw);
                assert!((dy0 - y).abs() <= tol && (dy1 - (y + fh)).abs() <= tol, "{kind} {w}x{h} {mode:?}: altura desenhada {dy0}..{dy1}, moldura {y}..{}", y + fh);
            }
        }
    }

    #[test]
    fn square_cover_is_not_cropped_in_large() {
        com();
        // antes, a capa quadrada esticava até a largura toda do grande e perdia o alto e o pé
        let m = Media::sample_art("spotify", 300, 300);
        let (_, _, w, h, _) = m.art_rect(Mode::G);
        assert_eq!((w, h), (G_ART.3, G_ART.3), "quadrada, na altura da área");
        // sem capa, o lugar dela continua sendo a área toda
        let (_, _, w, h, _) = Media::sample().art_rect(Mode::G);
        assert_eq!((w, h), (G_ART.2, G_ART.3));
    }

    #[test]
    fn large_has_room_for_a_big_cover() {
        com();
        // a capa quadrada do grande ocupa bem mais que antes (200), sem sobrar tanto dos lados
        assert!(G_ART.3 >= 240.0);
        let mut m = Media::sample_art("spotify", 300, 300);
        if let Some(snap) = &mut m.media {
            (snap.can_shuffle, snap.shuffle, snap.can_repeat, snap.repeat) = (true, Some(false), true, Some(0));
        }
        let env = Env { cfg: &serde_json::json!({"midia": {}, "app": {}}), key: "midia" };
        let art_bottom = G_ART.1 + G_ART.3;
        assert!(G_TEXT_Y >= art_bottom + 8.0, "o texto começa embaixo da capa");
        // botões: abaixo da capa, dentro do cartão e sem encostar uns nos outros
        let mut boxes: Vec<(String, (f32, f32, f32, f32))> = m
            .controls(Mode::G)
            .into_iter()
            .enumerate()
            .map(|(i, (_, cx, cy, size, _, circle))| {
                let r = Media::reach(size, circle);
                (format!("controle {i}"), (cx - r, cy - r, cx + r, cy + r))
            })
            .collect();
        boxes.extend(m.toggles().into_iter().map(|(a, r)| (format!("botão {a}"), r)));
        for (name, (x0, y0, x1, y1)) in &boxes {
            assert!(*y0 > art_bottom && *x0 >= 0.0 && *x1 <= W && *y1 <= LARGE_H - 8.0, "{name} fora do lugar: {x0},{y0} {x1},{y1}");
        }
        for (i, (a, ra)) in boxes.iter().enumerate() {
            for (b, rb) in &boxes[i + 1..] {
                let overlap = ra.0 < rb.2 && rb.0 < ra.2 && ra.1 < rb.3 && rb.1 < ra.3;
                assert!(!overlap, "{a} e {b} se sobrepõem");
            }
        }
        // a barra, os controles e o volume, de cima para baixo, sem se cruzar
        let (_, _, bar_y, _) = m.bar_geom(Mode::G).unwrap();
        assert!(bar_y > G_TEXT_Y + 50.0 && CTRL_Y - PLAY_D / 2.0 > bar_y + 16.0 && G_VOL_Y - 12.0 > CTRL_Y + PLAY_D / 2.0);
        // cada botão responde no lugar dele
        for (a, (x0, y0, x1, y1)) in m.toggles() {
            assert_eq!(Kind::hit(&m, (x0 + x1) / 2.0, (y0 + y1) / 2.0, Mode::G, &env), Some(a));
        }
    }

    #[test]
    fn marquee_makes_a_single_lap() {
        let span = 300.0;
        let lap = span / MARQUEE_SPEED;
        assert_eq!(marquee_offset(0.0, span), None, "antes da volta: parado");
        assert!((marquee_offset(2.0, span).unwrap() - 2.0 * MARQUEE_SPEED).abs() < 0.01, "anda na velocidade certa");
        assert!(marquee_offset(lap - 0.01, span).unwrap() < span, "nunca passa de uma volta");
        assert_eq!(marquee_offset(lap + 0.1, span), None, "depois da volta: parado de novo (sem recomeçar)");
    }

    /// Põe a volta do título do widget para ter começado `ago` segundos atrás.
    fn lap_started(m: &Media, ago: f32) {
        let title = m.media.as_ref().unwrap().title.clone();
        *m.lap.borrow_mut() = Some((title, Instant::now() - Duration::from_secs_f32(ago)));
    }

    /// O widget com um título, desenhado parado (como o cartão faz) a 1,25.
    fn drawn_with_title(m: &Media, mode: Mode) -> Canvas {
        let l = Layout::of(mode);
        let s = 1.25;
        let mut c = Canvas::new((l.w * s).round() as u32, (l.h * s).round() as u32, s);
        Kind::draw(m, &mut c, mode, &Env { cfg: &serde_json::json!({"midia": {}, "app": {}}), key: "midia" });
        c
    }

    fn with_title(mut m: Media, title: &str) -> Media {
        if let Some(snap) = &mut m.media {
            snap.title = title.into();
        }
        m
    }

    const LONG: &str = "Computer use chegou ao Maestri! Seu computador, agora compartilhado com seus agentes";

    #[test]
    fn only_long_titles_slide() {
        com();
        for mode in Mode::ALL {
            let mut long = with_title(Media::sample_art("youtube", 1280, 720), LONG);
            drawn_with_title(&long, mode);
            // música nova: espera um pouco (sem quadros), dá uma volta e para de novo
            assert_eq!(long.overlay_fps(), 0, "{mode:?}: parado enquanto dá para ler o começo");
            let start = long.lap.borrow().as_ref().unwrap().1;
            let wait = start.saturating_duration_since(Instant::now()).as_secs_f32();
            assert!(wait > MARQUEE_PAUSE - 0.5, "{mode:?}: a volta começa depois da pausa");
            assert!(long.next_tick().unwrap() <= start, "{mode:?}: o widget acorda quando a volta começa");
            lap_started(&long, 1.0);
            assert_eq!(long.overlay_fps(), MARQUEE_FPS, "{mode:?}: dando a volta");
            let lap = long.marquee.get() / MARQUEE_SPEED;
            lap_started(&long, lap + 0.5);
            assert_eq!(long.overlay_fps(), 0, "{mode:?}: acabou a volta, sem gastar quadros");
            // o mouse entra: mais uma volta, já
            assert!(Kind::pointer_entered(&mut long), "{mode:?}");
            assert_eq!(long.overlay_fps(), MARQUEE_FPS, "{mode:?}: o mouse entrou");
            assert!(!Kind::pointer_entered(&mut long), "{mode:?}: já está dando a volta");
            let short = with_title(Media::sample_art("spotify", 300, 300), "Thriller");
            drawn_with_title(&short, mode);
            assert_eq!(short.overlay_fps(), 0, "{mode:?}: título curto fica parado (sem gastar quadros)");
            let mut short = short;
            assert!(!Kind::pointer_entered(&mut short), "{mode:?}: título curto não dá volta");
            // sem capa não há o fundo para repintar embaixo dele: fica parado, com "..."
            let bare = with_title(Media::sample(), LONG);
            drawn_with_title(&bare, mode);
            assert_eq!(bare.overlay_fps(), 0, "{mode:?}: sem capa");
        }
    }

    #[test]
    fn sliding_title_moves_only_inside_its_strip() {
        com();
        for mode in Mode::ALL {
            let m = with_title(Media::sample_art("youtube", 1280, 720), LONG);
            let still = drawn_with_title(&m, mode).finish();
            // logo que aparece: parado (a camada não mexe em nada)
            let mut c = drawn_with_title(&m, mode);
            Kind::overlay(&m, &mut c, mode, 0.0, 0.0);
            assert_eq!(c.finish().px, still.px, "{mode:?}: na pausa, o título parado aparece como está");
            // alguns segundos depois: só a faixa do título muda
            lap_started(&m, 3.0);
            let mut c = drawn_with_title(&m, mode);
            Kind::overlay(&m, &mut c, mode, 0.0, 0.0);
            let moved = c.finish();
            let (x, y, w, size) = Media::title_spot(mode);
            let s = 1.25;
            let mut changed = 0;
            for py in 0..still.h {
                for px in 0..still.w {
                    let i = ((py * still.w + px) * 4) as usize;
                    if still.px[i..i + 4] != moved.px[i..i + 4] {
                        changed += 1;
                        let (fx, fy) = (px as f32 / s, py as f32 / s);
                        let inside = fx >= x - 1.0 && fx <= x + w + 1.0 && fy >= y - 4.0 && fy <= y + size * 1.6 + 1.0;
                        assert!(inside, "{mode:?}: mudou fora da faixa do título em ({fx}, {fy})");
                    }
                }
            }
            assert!(changed > 50, "{mode:?}: o título andou");
        }
    }

    #[test]
    fn odd_shapes_are_kept_sensible() {
        let area = (0.0, 0.0, 120.0, 120.0, 14.0);
        // um panorama não vira uma tira fina; uma imagem vazia não quebra a conta
        let (_, _, w, h, _) = fit_art(area, 10.0);
        assert!((w / h - ART_ASPECT.1).abs() < 0.01);
        assert_eq!(fit_art(area, f32::NAN), area);
        // capa baixa: cantos menores
        let (_, _, _, h, r) = fit_art(area, 2.0);
        assert!(r <= h * 0.2 + 0.01);
    }

    #[test]
    fn hd_cover_search_can_be_turned_off() {
        com();
        let small = picture(150, 83, 1);
        assert!(wants_hd("youtube", &small, true));
        assert!(!wants_hd("youtube", &small, false), "opção desligada: não manda nada ao youtube.com");
        assert!(!wants_hd("spotify", &small, true), "só vídeos do YouTube");
        assert!(hd_covers(&serde_json::json!({})), "ligada por padrão");
        assert!(!hd_covers(&serde_json::json!({"midia": {"capa_hd": false}})));
    }

    #[test]
    fn video_ids_from_search_page() {
        let html = r#"..."videoRenderer":{"videoId":"dQw4w9WgXcQ","x":1}..."videoRenderer":{"videoId":"dQw4w9WgXcQ"}
            ..."videoRenderer":{"videoId":"LQ4w9xiHkrY"}..."videoRenderer":{"videoId":"curto"}..."videoRenderer":{"videoId":"udTd_rpa1kA"}"#;
        assert_eq!(youtube::video_ids(html, 3), vec!["dQw4w9WgXcQ", "LQ4w9xiHkrY", "udTd_rpa1kA"]);
        assert_eq!(youtube::video_ids(html, 1), vec!["dQw4w9WgXcQ"]);
    }

    #[test]
    fn same_picture_in_any_size() {
        com();
        let big = picture(1280, 720, 4);
        let small = gfx::resize(&big, 150, 83);
        assert!(youtube::same_picture(&small, &big));
        assert!(!youtube::same_picture(&small, &picture(1280, 720, 9)));
    }
}
