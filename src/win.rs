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

/// [(retângulo do monitor, área de trabalho sem a barra de tarefas)], o principal primeiro.
pub fn monitor_areas() -> Vec<(Rect, Rect)> {
    unsafe extern "system" fn found(monitor: HMONITOR, _dc: HDC, _rect: *mut RECT, data: LPARAM) -> BOOL {
        let out = &mut *(data.0 as *mut Vec<(Rect, Rect)>);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let r = |r: RECT| (r.left, r.top, r.right, r.bottom);
            let area = (r(info.rcMonitor), r(info.rcWork));
            if info.dwFlags & 1 != 0 {
                out.insert(0, area); // o monitor principal vem primeiro
            } else {
                out.push(area);
            }
        }
        true.into()
    }
    let mut out: Vec<(Rect, Rect)> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(found), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

pub fn monitors() -> Vec<Rect> {
    monitor_areas().into_iter().map(|(screen, _)| screen).collect()
}

pub fn work_areas() -> Vec<Rect> {
    let areas: Vec<Rect> = monitor_areas().into_iter().map(|(_, work)| work).collect();
    if areas.is_empty() {
        unsafe { vec![(0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))] }
    } else {
        areas
    }
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

pub fn on_battery() -> bool {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status).is_ok() && status.ACLineStatus == 0 }
}

/// Jogo, vídeo em tela cheia ou apresentação: os widgets nem aparecem.
pub fn fullscreen_busy() -> bool {
    unsafe { SHQueryUserNotificationState().map(|s| matches!(s.0, 2..=4)).unwrap_or(false) }
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
    let delta = epoch - now();
    let t = local_time();
    let secs = (t.hour as i64 * 3600 + t.minute as i64 * 60 + t.second as i64 + delta.round() as i64).rem_euclid(86400);
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
