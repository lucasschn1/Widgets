//! Acesso à API do Windows: monitores, janelas transparentes, registro e informações do sistema.

use std::path::PathBuf;

use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::*;
use windows::Win32::System::SystemInformation::{GetLocalTime, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, GetAsyncKeyState, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP};
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::{wide, Image};

/// Retângulo em pixels da tela: (esquerda, topo, direita, base).
pub type Rect = (i32, i32, i32, i32);

pub fn fonts_dir() -> PathBuf {
    PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into())).join("Fonts")
}

pub fn font_exists(name: &str) -> bool {
    fonts_dir().join(name).exists()
}

// ---------- monitores ----------

/// Um monitor ligado.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// identifica o aparelho (continua o mesmo se ele virar o principal ou mudar de lado)
    pub id: String,
    pub screen: Rect,
    /// área de trabalho, sem a barra de tarefas
    pub work: Rect,
    /// 1 = 96 dpi
    pub scale: f32,
}

fn rect_of(r: RECT) -> Rect {
    (r.left, r.top, r.right, r.bottom)
}

fn wide_str(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Caminho do monitor ligado a uma saída de vídeo (ex.: "\\.\DISPLAY1"): diz qual é o aparelho,
/// mesmo quando o Windows troca o número da saída. Vazio se não descobrir.
fn monitor_path(device: &[u16]) -> String {
    let mut n = 0;
    loop {
        let mut dd = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        // 1 = EDD_GET_DEVICE_INTERFACE_NAME
        if !unsafe { EnumDisplayDevicesW(PCWSTR(device.as_ptr()), n, &mut dd, 1) }.as_bool() {
            return String::new();
        }
        if (dd.StateFlags & DISPLAY_DEVICE_ACTIVE).0 != 0 {
            return wide_str(&dd.DeviceID).to_lowercase();
        }
        n += 1;
    }
}

fn monitor_of(monitor: HMONITOR) -> Option<(Monitor, bool)> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if !unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) }.as_bool() {
        return None;
    }
    let mut id = monitor_path(&info.szDevice);
    if id.is_empty() {
        id = wide_str(&info.szDevice).to_lowercase();
    }
    let (mut dx, mut dy) = (96u32, 96u32);
    unsafe {
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    }
    let m = info.monitorInfo;
    Some((Monitor { id, screen: rect_of(m.rcMonitor), work: rect_of(m.rcWork), scale: dx as f32 / 96.0 }, m.dwFlags & 1 != 0))
}

/// Monitores ligados, o principal primeiro.
pub fn monitor_list() -> Vec<Monitor> {
    unsafe extern "system" fn found(monitor: HMONITOR, _dc: HDC, _rect: *mut RECT, data: LPARAM) -> BOOL {
        let out = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
        if let Some((m, primary)) = monitor_of(monitor) {
            if primary {
                out.insert(0, m);
            } else {
                out.push(m);
            }
        }
        true.into()
    }
    let mut out: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(found), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

pub fn monitors() -> Vec<Rect> {
    monitor_list().into_iter().map(|m| m.screen).collect()
}

pub fn work_areas() -> Vec<Rect> {
    let areas: Vec<Rect> = monitor_list().into_iter().map(|m| m.work).collect();
    if areas.is_empty() {
        unsafe { vec![(0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))] }
    } else {
        areas
    }
}

/// Área de trabalho do monitor onde está (ou mais perto de) o ponto.
pub fn area_at(x: i32, y: i32) -> Rect {
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    monitor_of(monitor).map(|(m, _)| m.work).unwrap_or_else(|| work_areas()[0])
}

/// O monitor onde está (ou mais perto de) o ponto: (tela inteira, área de trabalho).
pub fn monitor_at(x: i32, y: i32) -> (Rect, Rect) {
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    monitor_of(monitor).map(|(m, _)| (m.screen, m.work)).unwrap_or_else(|| {
        let a = work_areas()[0];
        (a, a)
    })
}

/// Se o ponto está em algum monitor.
pub fn on_screen(x: i32, y: i32) -> bool {
    monitors().iter().any(|r| x >= r.0 && x < r.2 && y >= r.1 && y < r.3)
}

/// Área de trabalho do monitor onde está o mouse (onde a pessoa está olhando).
pub fn area_at_pointer() -> Rect {
    let (x, y) = pointer();
    area_at(x, y)
}

/// Largura e altura da interseção de dois retângulos (0 se não se tocam).
pub fn overlap(a: Rect, b: Rect) -> (i32, i32) {
    ((a.2.min(b.2) - a.0.max(b.0)).max(0), (a.3.min(b.3) - a.1.max(b.1)).max(0))
}

/// Área de trabalho do monitor onde está a maior parte do retângulo.
pub fn area_for(rect: Rect) -> Rect {
    let areas = work_areas();
    let first = areas[0];
    areas
        .into_iter()
        .max_by_key(|a| {
            let (w, h) = overlap(rect, *a);
            w as i64 * h as i64
        })
        .unwrap_or(first)
}

/// Escala (1 = 96 dpi) do monitor mais perto do ponto.
pub fn scale_at(x: i32, y: i32) -> f32 {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        dx as f32 / 96.0
    }
}

pub fn pointer() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

pub fn key_down(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

// ---------- janelas ----------

pub fn desktop_window() -> Option<HWND> {
    unsafe { FindWindowW(w!("Progman"), PCWSTR::null()).ok().filter(|h| !h.is_invalid()) }
}

/// Faz a área de trabalho ser a "dona" da janela: ela continua visível no "Mostrar área de
/// trabalho" (Win+D) e fica sempre logo acima do papel de parede.
pub fn pin_to_desktop(hwnd: HWND) -> Option<HWND> {
    let progman = desktop_window();
    if let Some(p) = progman {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, p.0 as isize);
        }
    }
    progman
}

#[derive(Clone, Copy)]
pub enum Z {
    Bottom,
    Topmost,
    NoTopmost,
}

pub fn set_z(hwnd: HWND, z: Z) {
    let after = match z {
        Z::Bottom => HWND_BOTTOM,
        Z::Topmost => HWND_TOPMOST,
        Z::NoTopmost => HWND_NOTOPMOST,
    };
    unsafe {
        let _ = SetWindowPos(hwnd, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Mostra a imagem como o conteúdo de uma janela transparente (alfa por pixel). Com `pos`,
/// move a janela no mesmo passo; `alpha` deixa a janela inteira mais transparente.
pub fn blit(hwnd: HWND, img: &Image, pos: Option<(i32, i32)>, alpha: u8) {
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: img.w as i32,
            biHeight: -(img.h as i32), // de cima para baixo
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        if let Ok(bitmap) = CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            std::ptr::copy_nonoverlapping(img.px.as_ptr(), bits as *mut u8, img.px.len());
            let old = SelectObject(mem, bitmap.into());
            let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: 1 };
            let dest = pos.map(|(x, y)| POINT { x, y });
            let size = SIZE { cx: img.w as i32, cy: img.h as i32 };
            let src = POINT { x: 0, y: 0 };
            let _ = UpdateLayeredWindow(
                hwnd,
                Some(screen),
                dest.as_ref().map(|p| p as *const POINT),
                Some(&size),
                Some(mem),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(mem, old);
            let _ = DeleteObject(bitmap.into());
        }
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

/// Leva a janela para `pos` (None: fica onde está) com `size` pixels, sem mexer na ordem nem no
/// foco (a janela da placa de vídeo, que não usa `blit`).
pub fn place(hwnd: HWND, pos: Option<(i32, i32)>, size: (i32, i32)) {
    let (x, y) = pos.unwrap_or((0, 0));
    let mut flags = SWP_NOZORDER | SWP_NOACTIVATE;
    if pos.is_none() {
        flags |= SWP_NOMOVE;
    }
    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, size.0, size.1, flags);
    }
}

/// Só `area` (em pixels da janela) recebe cliques; fora dela, eles passam para o que está
/// embaixo. None: a janela toda.
pub fn set_hit_area(hwnd: HWND, area: Option<Rect>) {
    unsafe {
        let region = area.map(|(x0, y0, x1, y1)| CreateRectRgn(x0, y0, x1, y1));
        // (a janela fica dona da região e a apaga quando trocar)
        let _ = SetWindowRgn(hwnd, region, true);
    }
}

// ---------- registro e sistema ----------

/// Valor de texto (ou número, convertido) em HKEY_CURRENT_USER; "" se não existir.
pub fn registry(path: &str, name: &str) -> String {
    let (wpath, wname) = (wide(path), wide(name));
    unsafe {
        let mut size = 0u32;
        let mut kind = REG_VALUE_TYPE(0);
        if RegGetValueW(HKEY_CURRENT_USER, PCWSTR(wpath.as_ptr()), PCWSTR(wname.as_ptr()), RRF_RT_ANY, Some(&mut kind), None, Some(&mut size))
            != ERROR_SUCCESS
        {
            return String::new();
        }
        let mut buf = vec![0u8; size as usize + 2];
        if RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(wpath.as_ptr()),
            PCWSTR(wname.as_ptr()),
            RRF_RT_ANY,
            Some(&mut kind),
            Some(buf.as_mut_ptr() as _),
            Some(&mut size),
        ) != ERROR_SUCCESS
        {
            return String::new();
        }
        match kind {
            REG_SZ | REG_EXPAND_SZ => {
                let u16s: Vec<u16> = buf[..size as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&u16s).trim_end_matches('\0').to_string()
            }
            REG_DWORD => u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]).to_string(),
            _ => String::new(),
        }
    }
}

/// Abre um programa, endereço ou página de configurações do Windows.
pub fn open_target(target: &str) {
    let t = wide(target);
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(t.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// (tempo ocioso, tempo total) da CPU desde que o PC ligou, em unidades de 100 ns.
pub fn cpu_times() -> (u64, u64) {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe {
        let _ = GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user));
    }
    let n = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
    // o tempo de kernel já inclui o tempo ocioso
    (n(idle), n(kernel) + n(user))
}

/// (memória usada, memória total), em bytes.
pub fn memory() -> (u64, u64) {
    let mut stat = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe {
        let _ = GlobalMemoryStatusEx(&mut stat);
    }
    (stat.ullTotalPhys - stat.ullAvailPhys, stat.ullTotalPhys)
}

/// (usado, total, livre) do disco, em bytes.
pub fn disk_usage(root: &str) -> (u64, u64, u64) {
    let r = wide(root);
    let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
    unsafe {
        let _ = GetDiskFreeSpaceExW(PCWSTR(r.as_ptr()), Some(&mut free), Some(&mut total), Some(&mut total_free));
    }
    (total.saturating_sub(total_free), total.max(1), free)
}

pub fn gb(n: u64) -> f64 {
    n as f64 / 1024f64.powi(3)
}

/// A economia de energia do Windows está ligada? (Na tomada ou na bateria: só desconectar o
/// notebook não conta.)
pub fn energy_saver() -> bool {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status).is_ok() && saver_on(&status) }
}

/// Deixa (`allow`) ou não o Windows pôr os widgets no "modo de eficiência". Na bateria, ele faz
/// isso sozinho com programas em segundo plano: a CPU fica mais lenta para eles e os relógios
/// passam a acordar de 15,6 em 15,6 ms (ou mais), e as animações andam aos trancos. Fora da
/// economia de energia, os widgets pedem para rodar normal; com ela, voltam a economizar.
pub fn allow_throttling(allow: bool) -> bool {
    use windows::Win32::System::Threading::*;
    let state = throttling_state(allow);
    unsafe {
        SetProcessInformation(GetCurrentProcess(), ProcessPowerThrottling, &state as *const _ as *const _, std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32).is_ok()
    }
}

/// O pedido ao Windows: sempre sobre a velocidade e os relógios; ligados só se `allow`.
fn throttling_state(allow: bool) -> windows::Win32::System::Threading::PROCESS_POWER_THROTTLING_STATE {
    use windows::Win32::System::Threading::*;
    let both = PROCESS_POWER_THROTTLING_EXECUTION_SPEED | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    PROCESS_POWER_THROTTLING_STATE { Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION, ControlMask: both, StateMask: if allow { both } else { 0 } }
}

/// `SystemStatusFlag` é 1 com a economia de energia (ou a "economia de bateria" das versões
/// antigas) ligada; `ACLineStatus` (na tomada ou não) não entra na conta.
fn saver_on(status: &SYSTEM_POWER_STATUS) -> bool {
    status.SystemStatusFlag == 1
}

/// Jogo, vídeo em tela cheia ou apresentação: os widgets nem aparecem.
/// Devolve os monitores tomados (o da janela em tela cheia; todos no modo apresentação), para
/// pausar só os widgets que ficam neles.
pub fn fullscreen_screens() -> Vec<Rect> {
    unsafe {
        // 4 = QUNS_PRESENTATION_MODE: o Windows pede silêncio em tudo
        if SHQueryUserNotificationState().is_ok_and(|s| s.0 == 4) {
            return monitors();
        }
        let fg = GetForegroundWindow();
        if fg.is_invalid() || !IsWindowVisible(fg).as_bool() || IsIconic(fg).as_bool() {
            return Vec::new();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid == std::process::id() {
            return Vec::new();
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(fg, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return Vec::new(); // a própria área de trabalho ou a barra de tarefas
        }
        // janela comum maximizada (com barra de título) não conta, mesmo cobrindo a tela
        let style = GetWindowLongW(fg, GWL_STYLE) as u32;
        if style & WS_CAPTION.0 == WS_CAPTION.0 {
            return Vec::new();
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return Vec::new();
        }
        let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
        let Some((m, _)) = (!monitor.is_invalid()).then(|| monitor_of(monitor)).flatten() else { return Vec::new() };
        let (l, t, rr, b) = m.screen;
        if r.left <= l && r.top <= t && r.right >= rr && r.bottom >= b {
            vec![m.screen]
        } else {
            Vec::new()
        }
    }
}

/// Papel de parede de cada monitor (retângulo, arquivo), quando o Windows informa.
pub struct Wallpapers(Option<windows::Win32::UI::Shell::IDesktopWallpaper>);

impl Wallpapers {
    pub fn new() -> Wallpapers {
        Wallpapers(None)
    }

    pub fn list(&mut self) -> Vec<(Rect, String)> {
        use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};
        use windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper};
        if self.0.is_none() {
            self.0 = unsafe { CoCreateInstance::<_, IDesktopWallpaper>(&DesktopWallpaper, None, CLSCTX_ALL) }.ok();
        }
        let Some(dw) = &self.0 else { return Vec::new() };
        let mut out = Vec::new();
        unsafe {
            let Ok(count) = dw.GetMonitorDevicePathCount() else {
                self.0 = None; // o Explorer reiniciou: pega de novo da próxima vez
                return out;
            };
            for i in 0..count {
                let Ok(id) = dw.GetMonitorDevicePathAt(i) else { continue };
                let id_str = PCWSTR(id.0);
                // monitores desligados não têm retângulo
                if let (Ok(r), Ok(path)) = (dw.GetMonitorRECT(id_str), dw.GetWallpaper(id_str)) {
                    out.push((rect_of(r), path.to_string().unwrap_or_default()));
                    CoTaskMemFree(Some(path.0 as _));
                }
                CoTaskMemFree(Some(id.0 as _));
            }
        }
        out
    }
}

/// "Mostrar animações no Windows" (Acessibilidade → Efeitos visuais).
pub fn animations_enabled() -> bool {
    let mut enabled = BOOL(1);
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut enabled as *mut _ as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
    enabled.as_bool()
}

/// Sobe ou desce o volume do Windows (2% por passo), com o aviso de volume na tela.
pub fn change_volume(steps: f32) {
    let key: u8 = if steps > 0.0 { 0xAF } else { 0xAE };
    for _ in 0..(steps.abs().round() as i32).clamp(1, 10) {
        unsafe {
            keybd_event(key, 0, KEYBD_EVENT_FLAGS(0), 0);
            keybd_event(key, 0, KEYEVENTF_KEYUP, 0);
        }
    }
}

// ---------- tempo ----------

#[derive(Clone, Copy, Debug)]
pub struct LocalTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    /// 0 = segunda ... 6 = domingo (como no Python)
    pub weekday: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

pub fn local_time() -> LocalTime {
    let t = unsafe { GetLocalTime() };
    LocalTime {
        year: t.wYear,
        month: t.wMonth,
        day: t.wDay,
        weekday: (t.wDayOfWeek + 6) % 7,
        hour: t.wHour,
        minute: t.wMinute,
        second: t.wSecond,
    }
}

impl LocalTime {
    pub fn iso_date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Segundos desde 1970 (para guardar horários no config.json, como o Python fazia).
pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// "14:32" de um horário em segundos desde 1970.
pub fn clock_at(epoch: f64) -> String {
    let t = unsafe { GetLocalTime() };
    let local = t.wHour as f64 * 3600.0 + t.wMinute as f64 * 60.0 + t.wSecond as f64 + t.wMilliseconds as f64 / 1000.0;
    clock_with_offset(epoch, utc_offset(local, now()))
}

/// O fuso de agora em segundos (múltiplo de 1 minuto), da hora local do dia (`local`, em
/// segundos) e do horário em segundos desde 1970 (`utc`), lidos quase juntos.
fn utc_offset(local: f64, utc: f64) -> f64 {
    let off = (local - utc.rem_euclid(86400.0)).rem_euclid(86400.0);
    (off / 60.0).round() * 60.0
}

/// "HH:MM" de `epoch` num fuso de `offset` segundos. (Antes, a hora local vinha sem os
/// milissegundos e o resto da conta com eles: um horário bem na virada do minuto, como o
/// "Reinicia às 14:00" do Claude, alternava entre 13:59 e 14:00 a cada redesenho.)
fn clock_with_offset(epoch: f64, offset: f64) -> String {
    let secs = ((epoch + offset).floor() as i64).rem_euclid(86400);
    format!("{:02}:{:02}", secs / 3600, secs / 60 % 60)
}

/// Primeira janela visível cujo título contém `part` (sem diferenciar maiúsculas).
pub fn find_window(part: &str) -> Option<HWND> {
    unsafe extern "system" fn check(hwnd: HWND, data: LPARAM) -> BOOL {
        let (part, found) = unsafe { &mut *(data.0 as *mut (String, Option<HWND>)) };
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                let mut buf = [0u16; 512];
                let n = GetWindowTextW(hwnd, &mut buf);
                let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]).to_lowercase();
                if title.contains(part.as_str()) {
                    *found = Some(hwnd);
                    return false.into();
                }
            }
        }
        true.into()
    }
    let mut data = (part.to_lowercase(), None);
    unsafe {
        let _ = EnumWindows(Some(check), LPARAM(&mut data as *mut _ as isize));
    }
    data.1
}

pub fn bring_to_front(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Barra de título escura (da cor do fundo, no Windows 11), cantos arredondados e, nas janelas
/// sem barra, uma borda discreta.
pub fn style_window(hwnd: HWND, caption: Option<crate::theme::Rgba>, frameless: bool) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
    let set = |attr: i32, value: u32| unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(attr), &value as *const u32 as _, 4);
    };
    let colorref = |c: crate::theme::Rgba| c.0 as u32 | (c.1 as u32) << 8 | (c.2 as u32) << 16;
    set(20, 1); // modo escuro
    set(33, 2); // cantos arredondados
    if let Some(c) = caption {
        set(35, colorref(c)); // cor da barra de título
        set(36, 0x00F5F2F2); // cor do título
    }
    if frameless {
        set(34, colorref(crate::theme::Rgba::rgb(60, 60, 70))); // borda
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Só a economia de energia do Windows pausa as animações: o notebook fora da tomada, sem
    /// ela, anima normalmente.
    #[test]
    fn only_energy_saver_counts_not_the_battery() {
        let status = |ac: u8, flag: u8| SYSTEM_POWER_STATUS { ACLineStatus: ac, SystemStatusFlag: flag, ..Default::default() };
        assert!(!saver_on(&status(0, 0)), "na bateria, sem a economia");
        assert!(!saver_on(&status(1, 0)), "na tomada");
        assert!(saver_on(&status(0, 1)), "na bateria, com a economia");
        assert!(saver_on(&status(1, 1)), "na tomada, com a economia");
    }

    /// Fora da economia de energia, os widgets pedem ao Windows para não entrar no modo de
    /// eficiência (CPU lenta e relógios espaçados deixavam as animações aos trancos na bateria);
    /// com ela, liberam. (Só neste processo de teste.)
    #[test]
    fn asks_windows_not_to_slow_down_the_animations() {
        use windows::Win32::System::Threading::*;
        let off = throttling_state(false);
        assert_eq!(off.StateMask, 0);
        assert_ne!(off.ControlMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED, 0);
        assert_ne!(off.ControlMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, 0);
        assert_eq!(throttling_state(true).StateMask, off.ControlMask);
        let read = || {
            let mut st = PROCESS_POWER_THROTTLING_STATE { Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION, ..Default::default() };
            unsafe { GetProcessInformation(GetCurrentProcess(), ProcessPowerThrottling, &mut st as *mut _ as *mut _, std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32).unwrap() };
            st
        };
        assert!(allow_throttling(false));
        assert_eq!((read().ControlMask, read().StateMask), (off.ControlMask, 0));
        assert!(allow_throttling(true));
        assert_eq!(read().StateMask, off.ControlMask);
    }

    #[test]
    fn clock_does_not_flicker_at_the_turn_of_the_minute() {
        // reinicia às 14:00:00 em ponto (UTC-3): lido em qualquer fração de segundo, é sempre 14:00
        let resets = 1_762_794_000.0; // 17:00:00 UTC
        for ms in [0.0f64, 0.2, 0.5, 0.7, 0.999] {
            let utc = resets - 5400.0 + ms; // 12:30:00,xxx no horário local
            let local = 12.0 * 3600.0 + 30.0 * 60.0 + (ms * 1000.0).floor() / 1000.0;
            assert_eq!(clock_with_offset(resets, utc_offset(local, utc)), "14:00", "{ms}");
        }
        // um segundo antes ainda é 13:59
        assert_eq!(clock_with_offset(resets - 1.0, -3.0 * 3600.0), "13:59");
    }

    #[test]
    fn offset_across_midnight() {
        // 23:50 local em UTC-3 (02:50 UTC do dia seguinte) e 01:00 local em UTC+5 (20:00 UTC)
        assert_eq!(utc_offset(23.0 * 3600.0 + 50.0 * 60.0, 86400.0 * 10.0 + 2.0 * 3600.0 + 50.0 * 60.0), 21.0 * 3600.0);
        assert_eq!(clock_with_offset(86400.0 * 10.0 + 3.0 * 3600.0, 21.0 * 3600.0), "00:00");
        assert_eq!(clock_with_offset(86400.0 * 10.0 + 20.0 * 3600.0, utc_offset(3600.0, 86400.0 * 10.0 + 20.0 * 3600.0)), "01:00");
    }

    #[test]
    fn clock_at_now_is_the_local_time() {
        let t = local_time();
        let shown = clock_at(now());
        let (h, m) = (shown[..2].parse::<u16>().unwrap(), shown[3..].parse::<u16>().unwrap());
        // (a virada do minuto pode cair entre as duas leituras)
        let mins = |h: u16, m: u16| (h * 60 + m) as i32;
        assert!((mins(h, m) - mins(t.hour, t.minute)).rem_euclid(1440) <= 1, "{shown} vs {}:{}", t.hour, t.minute);
    }
}
