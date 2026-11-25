//! Acesso à API do Windows: monitores, janelas transparentes, registro e informações do sistema.

use std::path::PathBuf;

use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::*;
use windows::Win32::System::SystemInformation::{GetLocalTime, GetTickCount, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, GetAsyncKeyState, GetLastInputInfo, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, LASTINPUTINFO};
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::{wide, Image};

/// Retângulo em pixels da tela: (esquerda, topo, direita, base).
pub type Rect = (i32, i32, i32, i32);

/// A pasta das fontes do Windows.
pub fn fonts_dir() -> PathBuf {
    PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into())).join("Fonts")
}

/// A fonte `name` (o nome do arquivo) está instalada?
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

/// O retângulo do Windows como `Rect`.
fn rect_of(rect: RECT) -> Rect {
    (rect.left, rect.top, rect.right, rect.bottom)
}

/// O texto de um buffer UTF-16 do Windows (até o primeiro zero).
fn wide_str(buf: &[u16]) -> String {
    let end = buf.iter().position(|&unit| unit == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Caminho do monitor ligado a uma saída de vídeo (ex.: "\\.\DISPLAY1"): diz qual é o aparelho,
/// mesmo quando o Windows troca o número da saída. Vazio se não descobrir.
fn monitor_path(device: &[u16]) -> String {
    let mut index = 0;
    loop {
        let mut display = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        // 1 = EDD_GET_DEVICE_INTERFACE_NAME
        if !unsafe { EnumDisplayDevicesW(PCWSTR(device.as_ptr()), index, &mut display, 1) }.as_bool() {
            return String::new();
        }
        if (display.StateFlags & DISPLAY_DEVICE_ACTIVE).0 != 0 {
            return wide_str(&display.DeviceID).to_lowercase();
        }
        index += 1;
    }
}

/// O monitor `monitor` e se ele é o principal.
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
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    unsafe {
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    }
    let found = info.monitorInfo;
    // (dwFlags 1 = MONITORINFOF_PRIMARY)
    Some((Monitor { id, screen: rect_of(found.rcMonitor), work: rect_of(found.rcWork), scale: dpi_x as f32 / 96.0 }, found.dwFlags & 1 != 0))
}

/// Monitores ligados, o principal primeiro.
pub fn monitor_list() -> Vec<Monitor> {
    unsafe extern "system" fn found(monitor: HMONITOR, _dc: HDC, _rect: *mut RECT, data: LPARAM) -> BOOL {
        let out = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
        if let Some((found, primary)) = monitor_of(monitor) {
            if primary {
                out.insert(0, found);
            } else {
                out.push(found);
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

/// A tela inteira de cada monitor, o principal primeiro.
pub fn monitors() -> Vec<Rect> {
    monitor_list().into_iter().map(|monitor| monitor.screen).collect()
}

/// A área de trabalho de cada monitor (sem a barra de tarefas), o principal primeiro.
pub fn work_areas() -> Vec<Rect> {
    let areas: Vec<Rect> = monitor_list().into_iter().map(|monitor| monitor.work).collect();
    if areas.is_empty() {
        unsafe { vec![(0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))] }
    } else {
        areas
    }
}

/// Área de trabalho do monitor onde está (ou mais perto de) o ponto.
pub fn area_at(x: i32, y: i32) -> Rect {
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    monitor_of(monitor).map(|(found, _)| found.work).unwrap_or_else(|| work_areas()[0])
}

/// O monitor onde está (ou mais perto de) o ponto: (tela inteira, área de trabalho).
pub fn monitor_at(x: i32, y: i32) -> (Rect, Rect) {
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    monitor_of(monitor).map(|(found, _)| (found.screen, found.work)).unwrap_or_else(|| {
        let first = work_areas()[0];
        (first, first)
    })
}

/// Se o ponto está em algum monitor.
pub fn on_screen(x: i32, y: i32) -> bool {
    monitors().iter().any(|screen| x >= screen.0 && x < screen.2 && y >= screen.1 && y < screen.3)
}

/// A tela inteira do monitor da janela em uso (a que recebe o teclado). None quando não dá para
/// dizer onde a pessoa está: nenhuma janela, minimizada, uma janela deste app (a ilha, um
/// cartão), a área de trabalho ou a barra de tarefas (que se espalham por todos os monitores).
pub fn foreground_screen() -> Option<Rect> {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() || !IsWindowVisible(foreground).as_bool() || IsIconic(foreground).as_bool() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut pid));
        if pid == std::process::id() {
            return None;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(foreground, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..len]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return None;
        }
        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONULL);
        (!monitor.is_invalid()).then(|| monitor_of(monitor)).flatten().map(|(found, _)| found.screen)
    }
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
        .max_by_key(|area| {
            let (w, h) = overlap(rect, *area);
            w as i64 * h as i64
        })
        .unwrap_or(first)
}

/// Escala (1 = 96 dpi) do monitor mais perto do ponto.
pub fn scale_at(x: i32, y: i32) -> f32 {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        dpi_x as f32 / 96.0
    }
}

/// Onde está o mouse na tela.
pub fn pointer() -> (i32, i32) {
    let mut point = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut point);
    }
    (point.x, point.y)
}

/// A tecla (ou o botão do mouse) `vk` está apertada agora?
pub fn key_down(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

// ---------- janelas ----------

/// A janela da área de trabalho (a "Progman").
pub fn desktop_window() -> Option<HWND> {
    unsafe { FindWindowW(w!("Progman"), PCWSTR::null()).ok().filter(|hwnd| !hwnd.is_invalid()) }
}

/// Faz a área de trabalho ser a "dona" da janela: ela continua visível no "Mostrar área de
/// trabalho" (Win+D) e fica sempre logo acima do papel de parede.
pub fn pin_to_desktop(hwnd: HWND) -> Option<HWND> {
    let progman = desktop_window();
    if let Some(desktop) = progman {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, desktop.0 as isize);
        }
    }
    progman
}

/// Onde pôr uma janela na pilha das janelas.
#[derive(Clone, Copy)]
pub enum Z {
    Bottom,
    Topmost,
    NoTopmost,
}

/// Muda a janela de lugar na pilha das janelas (sem mexer no tamanho nem no foco).
pub fn set_z(hwnd: HWND, order: Z) {
    let after = match order {
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
                dest.as_ref().map(|point| point as *const POINT),
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

/// Troca o texto de um valor em HKEY_CURRENT_USER, só se ele já existe (nunca cria chave nem
/// valor: ex., a versão na entrada de "Aplicativos instalados", que só existe se o app foi
/// instalado pelo install.ps1 ou pelo instalador).
pub fn update_registry_text(path: &str, name: &str, value: &str) {
    if registry(path, name).is_empty() {
        return;
    }
    let (wpath, wname, wvalue) = (wide(path), wide(name), wide(value));
    unsafe {
        let _ = RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(wpath.as_ptr()), PCWSTR(wname.as_ptr()), REG_SZ.0, Some(wvalue.as_ptr() as _), (wvalue.len() * 2) as u32);
    }
}

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
                let u16s: Vec<u16> = buf[..size as usize].chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
                String::from_utf16_lossy(&u16s).trim_end_matches('\0').to_string()
            }
            REG_DWORD => u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]).to_string(),
            _ => String::new(),
        }
    }
}

/// Abre um programa, endereço ou página de configurações do Windows.
pub fn open_target(target: &str) {
    let target = wide(target);
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(target.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// (tempo ocioso, tempo total) da CPU desde que o PC ligou, em unidades de 100 ns.
pub fn cpu_times() -> (u64, u64) {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe {
        let _ = GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user));
    }
    let ticks = |time: FILETIME| ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64;
    // o tempo de kernel já inclui o tempo ocioso
    (ticks(idle), ticks(kernel) + ticks(user))
}

/// (memória usada, memória total), em bytes.
pub fn memory() -> (u64, u64) {
    let mut stat = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe {
        let _ = GlobalMemoryStatusEx(&mut stat);
    }
    (stat.ullTotalPhys - stat.ullAvailPhys, stat.ullTotalPhys)
}

/// O espaço de um disco, em bytes.
#[derive(Clone, Copy, Debug)]
pub struct DiskUsage {
    pub used: u64,
    pub total: u64,
    /// o que este usuário ainda pode usar (com cotas, menos que total - used)
    pub free: u64,
}

/// O espaço do disco de `root` ("C:\\").
pub fn disk_usage(root: &str) -> DiskUsage {
    let root = wide(root);
    let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
    unsafe {
        let _ = GetDiskFreeSpaceExW(PCWSTR(root.as_ptr()), Some(&mut free), Some(&mut total), Some(&mut total_free));
    }
    DiskUsage { used: total.saturating_sub(total_free), total: total.max(1), free }
}

/// Bytes em gigabytes (de 1024³).
pub fn gb(bytes: u64) -> f64 {
    bytes as f64 / 1024f64.powi(3)
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
        if SHQueryUserNotificationState().is_ok_and(|state| state.0 == 4) {
            return monitors();
        }
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() || !IsWindowVisible(foreground).as_bool() || IsIconic(foreground).as_bool() {
            return Vec::new();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut pid));
        if pid == std::process::id() {
            return Vec::new();
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(foreground, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..len]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return Vec::new(); // a própria área de trabalho ou a barra de tarefas
        }
        // janela comum maximizada (com barra de título) não conta, mesmo cobrindo a tela
        let style = GetWindowLongW(foreground, GWL_STYLE) as u32;
        if style & WS_CAPTION.0 == WS_CAPTION.0 {
            return Vec::new();
        }
        let mut window = RECT::default();
        if GetWindowRect(foreground, &mut window).is_err() {
            return Vec::new();
        }
        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONULL);
        let Some((found, _)) = (!monitor.is_invalid()).then(|| monitor_of(monitor)).flatten() else { return Vec::new() };
        let (left, top, right, bottom) = found.screen;
        if window.left <= left && window.top <= top && window.right >= right && window.bottom >= bottom {
            vec![found.screen]
        } else {
            Vec::new()
        }
    }
}

/// Papel de parede de cada monitor (retângulo, arquivo), quando o Windows informa.
pub struct Wallpapers(Option<windows::Win32::UI::Shell::IDesktopWallpaper>);

impl Wallpapers {
    /// Ainda sem falar com o Windows (só na primeira `list`).
    pub fn new() -> Wallpapers {
        Wallpapers(None)
    }

    /// (monitor, arquivo) de cada monitor ligado.
    pub fn list(&mut self) -> Vec<(Rect, String)> {
        use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};
        use windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper};
        if self.0.is_none() {
            self.0 = unsafe { CoCreateInstance::<_, IDesktopWallpaper>(&DesktopWallpaper, None, CLSCTX_ALL) }.ok();
        }
        let Some(wallpaper) = &self.0 else { return Vec::new() };
        let mut out = Vec::new();
        unsafe {
            let Ok(count) = wallpaper.GetMonitorDevicePathCount() else {
                self.0 = None; // o Explorer reiniciou: pega de novo da próxima vez
                return out;
            };
            for i in 0..count {
                let Ok(id) = wallpaper.GetMonitorDevicePathAt(i) else { continue };
                let id_str = PCWSTR(id.0);
                // monitores desligados não têm retângulo
                if let (Ok(rect), Ok(path)) = (wallpaper.GetMonitorRECT(id_str), wallpaper.GetWallpaper(id_str)) {
                    out.push((rect_of(rect), path.to_string().unwrap_or_default()));
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

/// A data e a hora locais.
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

/// A data e a hora locais de agora.
pub fn local_time() -> LocalTime {
    let clock = unsafe { GetLocalTime() };
    LocalTime {
        year: clock.wYear,
        month: clock.wMonth,
        day: clock.wDay,
        // o Windows conta do domingo (0)
        weekday: (clock.wDayOfWeek + 6) % 7,
        hour: clock.wHour,
        minute: clock.wMinute,
        second: clock.wSecond,
    }
}

impl LocalTime {
    /// "2026-11-21"
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
    clock_with_offset(epoch, utc_offset(local_secs(), now()))
}

/// Há quantos segundos o teclado e o mouse estão parados (a pessoa longe do PC).
pub fn idle_secs() -> f64 {
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if GetLastInputInfo(&mut info).as_bool() {
            GetTickCount().wrapping_sub(info.dwTime) as f64 / 1000.0
        } else {
            0.0
        }
    }
}

/// O fuso do PC agora, em segundos (-3 h = -10800).
pub fn utc_offset_now() -> f64 {
    let offset = utc_offset(local_secs(), now());
    // os fusos vão de -12 h a +14 h
    if offset > 14.0 * 3600.0 { offset - 86400.0 } else { offset }
}

/// A hora local do dia agora, em segundos (com os milissegundos).
fn local_secs() -> f64 {
    let clock = unsafe { GetLocalTime() };
    clock.wHour as f64 * 3600.0 + clock.wMinute as f64 * 60.0 + clock.wSecond as f64 + clock.wMilliseconds as f64 / 1000.0
}

/// O fuso de agora em segundos (múltiplo de 1 minuto), da hora local do dia (`local`, em
/// segundos) e do horário em segundos desde 1970 (`utc`), lidos quase juntos.
fn utc_offset(local: f64, utc: f64) -> f64 {
    let offset = (local - utc.rem_euclid(86400.0)).rem_euclid(86400.0);
    (offset / 60.0).round() * 60.0
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
                let len = GetWindowTextW(hwnd, &mut buf);
                let title = String::from_utf16_lossy(&buf[..len.max(0) as usize]).to_lowercase();
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

// ---------- barra de tarefas ----------

/// A barra de tarefas de um monitor, em pixels da tela.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Taskbar {
    pub bar: Rect,
    /// o botão Iniciar (o Windows 11 guarda uma janela "Start" escondida que acompanha o ícone)
    pub start: Option<Rect>,
    /// a área de notificação (relógio, ícones), na barra principal
    pub tray: Option<Rect>,
}

const TASKBAR_CLASSES: [PCWSTR; 2] = [w!("Shell_TrayWnd"), w!("Shell_SecondaryTrayWnd")];

/// O retângulo da janela na tela (None se ela não tem tamanho).
fn window_rect(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    let rect = rect_of(rect);
    (rect.2 > rect.0 && rect.3 > rect.1).then_some(rect)
}

/// O retângulo da primeira janela filha da classe `class`.
fn child_rect(parent: HWND, class: PCWSTR) -> Option<Rect> {
    let child = unsafe { FindWindowExW(Some(parent), None, class, PCWSTR::null()) }.ok().filter(|hwnd| !hwnd.is_invalid())?;
    window_rect(child)
}

/// A barra de tarefas à vista, embaixo, no monitor do ponto (escondida sozinha, de lado ou em
/// cima: nada).
pub fn taskbar_at(x: i32, y: i32) -> Option<Taskbar> {
    let (screen, _) = monitor_at(x, y);
    for class in TASKBAR_CLASSES {
        let mut after: Option<HWND> = None;
        loop {
            let Some(hwnd) = (unsafe { FindWindowExW(None, after, class, PCWSTR::null()) }).ok().filter(|hwnd| !hwnd.is_invalid()) else { break };
            after = Some(hwnd);
            let Some(bar) = window_rect(hwnd) else { continue };
            if !unsafe { IsWindowVisible(hwnd) }.as_bool() || !taskbar_in_view(bar, screen) {
                continue;
            }
            let inside = |rect: Rect| rect.0 >= bar.0 && rect.2 <= bar.2 && rect.1 >= bar.1 && rect.3 <= bar.3;
            let start = child_rect(hwnd, w!("Start")).filter(|&rect| inside(rect));
            let tray = child_rect(hwnd, w!("TrayNotifyWnd")).filter(|&rect| inside(rect));
            return Some(Taskbar { bar, start, tray });
        }
    }
    None
}

/// Se a barra está inteira à vista, embaixo do monitor e de largura cheia.
fn taskbar_in_view(bar: Rect, screen: Rect) -> bool {
    let h = bar.3 - bar.1;
    bar.0 <= screen.0 + 2 && bar.2 >= screen.2 - 2 && (bar.3 - screen.3).abs() <= 2 && h >= 16 && h < (screen.3 - screen.1) / 3
}

/// A barra de tarefas foi para a frente da janela (ela sobe ao ser clicada): volta para cima dela.
pub fn stay_over_taskbar(hwnd: HWND) {
    let mut above = hwnd;
    for _ in 0..64 {
        let Ok(prev) = (unsafe { GetWindow(above, GW_HWNDPREV) }) else { return };
        if prev.is_invalid() {
            return;
        }
        let mut buf = [0u16; 64];
        let len = unsafe { GetClassNameW(prev, &mut buf) }.max(0) as usize;
        let class = String::from_utf16_lossy(&buf[..len]);
        if class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd" {
            set_z(hwnd, Z::Topmost);
            return;
        }
        above = prev;
    }
}

/// Traz a janela para a frente (e de volta do minimizado).
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
pub fn style_window(hwnd: HWND, caption: Option<crate::ui::theme::Rgba>, frameless: bool) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
    let set = |attr: i32, value: u32| unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(attr), &value as *const u32 as _, 4);
    };
    let colorref = |color: crate::ui::theme::Rgba| color.0 as u32 | (color.1 as u32) << 8 | (color.2 as u32) << 16;
    set(20, 1); // modo escuro
    set(33, 2); // cantos arredondados
    if let Some(color) = caption {
        set(35, colorref(color)); // cor da barra de título
        set(36, 0x00F5F2F2); // cor do título
    }
    if frameless {
        set(34, colorref(crate::ui::theme::Rgba::rgb(60, 60, 70))); // borda
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
