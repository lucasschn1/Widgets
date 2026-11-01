//! O que o widget Sistema mostra além dos anéis: os apps que mais usam CPU e memória (com o
//! ícone de cada um), as unidades de disco, a rede e há quanto tempo o PC está ligado.
//!
//! Os apps são lidos numa thread a cada 2 s, e só enquanto a lista está na tela. Tudo funciona
//! sem administrador: processos protegidos (do sistema) que não deixam ler ficam de fora.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHGetImageList, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_SYSICONINDEX, SHIL_EXTRALARGE, SHIL_JUMBO};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL, HICON};

use crate::gfx::{wide, Image};

/// Um app (todos os processos do mesmo executável somados).
#[derive(Clone)]
pub struct AppUse {
    pub name: String,
    /// % da CPU inteira (todos os núcleos), como no Gerenciador de Tarefas
    pub cpu: f32,
    /// memória privada em uso (bytes), a coluna "Memória" do Gerenciador de Tarefas
    pub mem: u64,
    pub icon: Option<Arc<Image>>,
}

#[derive(Clone)]
pub struct Drive {
    pub label: String,
    pub used: u64,
    pub total: u64,
    pub icon: Option<Arc<Image>>,
}

// ---------- ícones ----------

/// Ícone do Windows (HICON) em imagem BGRA pré-multiplicada, no tamanho pedido.
pub fn icon_image(icon: HICON, size: u32) -> Option<Image> {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32), // de cima para baixo
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(dib) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(dc);
            return None;
        };
        let old = SelectObject(dc, dib.into());
        let drawn = DrawIconEx(dc, 0, 0, icon, size as i32, size as i32, 0, None, DI_NORMAL).is_ok();
        let n = (size * size * 4) as usize;
        let mut px = std::slice::from_raw_parts(bits as *const u8, n).to_vec();
        SelectObject(dc, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(dc);
        if !drawn {
            return None;
        }
        // ícones antigos (sem canal alfa): o que foi desenhado fica opaco
        if px.chunks(4).all(|p| p[3] == 0) {
            for p in px.chunks_mut(4) {
                if p[0] != 0 || p[1] != 0 || p[2] != 0 {
                    p[3] = 255;
                }
            }
        }
        Some(Image { w: size, h: size, px })
    }
}

/// Lado em pixels dos ícones guardados (desenhados menores, ficam nítidos em qualquer escala).
pub const ICON_PX: u32 = 64;

/// Tamanho do desenho dentro do ícone: alguns apps só têm ícone pequeno, e o Windows o devolve
/// num canto da imagem grande.
fn drawn_size(img: &Image) -> u32 {
    let (mut x1, mut y1) = (0, 0);
    for y in 0..img.h {
        for x in 0..img.w {
            if img.px[((y * img.w + x) * 4 + 3) as usize] > 8 {
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    x1.max(y1)
}

/// Ícone de um arquivo ou unidade em alta definição, como no Explorador (lista "jumbo" de
/// 256 px; sem ela, a de 48 px), reduzido com boa qualidade para ICON_PX.
pub fn shell_icon(path: &str) -> Option<Image> {
    let w = wide(path);
    let mut info = SHFILEINFOW::default();
    unsafe {
        let ok = SHGetFileInfoW(PCWSTR(w.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX);
        if ok != 0 {
            for (list, size) in [(SHIL_JUMBO, 256u32), (SHIL_EXTRALARGE, 48)] {
                let Ok(images) = SHGetImageList::<IImageList>(list as i32) else { continue };
                let Ok(icon) = images.GetIcon(info.iIcon, ILD_TRANSPARENT.0) else { continue };
                let img = icon_image(icon, size);
                let _ = DestroyIcon(icon);
                let Some(img) = img else { continue };
                // o ícone de verdade tem que ocupar a imagem (senão era um pequeno num canto)
                if drawn_size(&img) * 10 < size * 7 {
                    continue;
                }
                return Some(crate::gfx::resize(&img, ICON_PX, ICON_PX));
            }
        }
        // último recurso: o ícone de 32 px
        let ok = SHGetFileInfoW(PCWSTR(w.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_LARGEICON);
        if ok == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let img = icon_image(info.hIcon, 32);
        let _ = DestroyIcon(info.hIcon);
        img
    }
}

/// "Descrição do arquivo" do executável (ex.: "Google Chrome"), em vez de "chrome.exe".
fn file_description(path: &str) -> Option<String> {
    let w = wide(path);
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(w.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(w.as_ptr()), None, size, data.as_mut_ptr() as _).ok()?;
        // primeiro idioma da tabela de traduções
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let sub = wide("\\VarFileInfo\\Translation");
        let lang = if VerQueryValueW(data.as_ptr() as _, PCWSTR(sub.as_ptr()), &mut ptr, &mut len).as_bool() && len >= 4 {
            let pair = std::slice::from_raw_parts(ptr as *const u16, 2);
            format!("{:04x}{:04x}", pair[0], pair[1])
        } else {
            "040904b0".into()
        };
        let sub = wide(&format!("\\StringFileInfo\\{lang}\\FileDescription"));
        if !VerQueryValueW(data.as_ptr() as _, PCWSTR(sub.as_ptr()), &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let text = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        // o texto termina no primeiro nulo (depois dele pode vir lixo da tabela)
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        let s = String::from_utf16_lossy(&text[..end]).trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

/// "msedge.exe" -> "Msedge" (quando o executável não tem descrição).
fn pretty_exe(exe: &str) -> String {
    let stem = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    let stem = stem.strip_suffix(".exe").or(stem.strip_suffix(".EXE")).unwrap_or(stem);
    let mut c = stem.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

// ---------- processos ----------

fn ft(t: FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
}

struct Proc {
    pid: u32,
    exe: String,
}

fn list_processes() -> Vec<Proc> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if entry.th32ProcessID > 4 {
                out.push(Proc { pid: entry.th32ProcessID, exe });
            }
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn image_path(h: HANDLE) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).ok()? };
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

/// (caminho do executável, tempo de CPU em unidades de 100 ns, memória privada) de um processo.
fn read_process(pid: u32) -> Option<(Option<String>, u64, u64)> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let cpu = if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok() { ft(k) + ft(u) } else { 0 };
        let mut mem = PROCESS_MEMORY_COUNTERS_EX2 { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
        let bytes = if GetProcessMemoryInfo(h, &mut mem as *mut _ as *mut PROCESS_MEMORY_COUNTERS, mem.cb).is_ok() {
            mem.PrivateWorkingSetSize as u64
        } else {
            0
        };
        let path = image_path(h);
        let _ = CloseHandle(h);
        Some((path, cpu, bytes))
    }
}

/// Lê os apps numa thread, só enquanto `active` estiver ligado.
pub struct Sampler {
    shared: Arc<Mutex<Option<Vec<AppUse>>>>,
    active: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

impl Sampler {
    pub fn new() -> Sampler {
        let shared = Arc::new(Mutex::new(None));
        let active = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let (s, a, l) = (shared.clone(), active.clone(), alive.clone());
        std::thread::spawn(move || sample_loop(s, a, l));
        Sampler { shared, active, alive }
    }

    /// Sem thread (miniatura da galeria).
    pub fn idle() -> Sampler {
        Sampler { shared: Arc::new(Mutex::new(None)), active: Arc::new(AtomicBool::new(false)), alive: Arc::new(AtomicBool::new(false)) }
    }

    /// Liga ou desliga a leitura (desligada, a thread só dorme).
    pub fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::Relaxed);
    }

    /// A leitura mais recente (None até a primeira ficar pronta).
    pub fn latest(&self) -> Option<Vec<AppUse>> {
        self.shared.lock().unwrap().clone()
    }
}

fn sample_loop(shared: Arc<Mutex<Option<Vec<AppUse>>>>, active: Arc<AtomicBool>, alive: Arc<AtomicBool>) {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
    // por executável: nome bonito e ícone (lidos uma vez só)
    let mut known: HashMap<String, (String, Option<Arc<Image>>)> = HashMap::new();
    let mut last: HashMap<u32, u64> = HashMap::new();
    let mut last_at: Option<Instant> = None;
    // a primeira leitura acontece logo ao abrir: nomes e ícones já ficam prontos para o clique
    let mut warm = true;
    while alive.load(Ordering::Relaxed) {
        if !warm && !active.load(Ordering::Relaxed) {
            last.clear();
            last_at = None;
            std::thread::sleep(Duration::from_millis(400));
            continue;
        }
        let now = Instant::now();
        let wall = last_at.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        let mut groups: HashMap<String, (f64, u64)> = HashMap::new();
        let mut seen = HashMap::new();
        for p in list_processes() {
            let Some((path, cpu, mem)) = read_process(p.pid) else { continue };
            let key = path.clone().unwrap_or_else(|| p.exe.clone());
            let delta = last.get(&p.pid).map(|&prev| cpu.saturating_sub(prev)).unwrap_or(0);
            seen.insert(p.pid, cpu);
            let g = groups.entry(key.clone()).or_insert((0.0, 0));
            if wall > 0.0 {
                g.0 += delta as f64 / 1e7 / (wall * cores) * 100.0;
            }
            g.1 += mem;
            known.entry(key).or_insert_with(|| {
                let name = path.as_deref().and_then(file_description).unwrap_or_else(|| pretty_exe(&p.exe));
                let icon = path.as_deref().and_then(shell_icon).map(Arc::new);
                (name, icon)
            });
        }
        last = seen;
        let first = last_at.is_none();
        last_at = Some(now);
        warm = false;
        let apps: Vec<AppUse> = groups
            .into_iter()
            .map(|(key, (cpu, mem))| {
                let (name, icon) = known.get(&key).cloned().unwrap_or_default();
                AppUse { name, cpu: cpu as f32, mem, icon }
            })
            .collect();
        *shared.lock().unwrap() = Some(apps);
        crate::events::wake();
        // a primeira medida da CPU precisa de duas leituras: a segunda vem logo
        std::thread::sleep(Duration::from_millis(if first { 700 } else { 2000 }));
    }
}

// ---------- discos, rede, tempo ligado ----------

/// Unidades fixas (C:, D:...), com nome, espaço e ícone.
pub fn drives() -> Vec<Drive> {
    let mut buf = [0u16; 256];
    let n = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) } as usize;
    let mut out = Vec::new();
    for root in String::from_utf16_lossy(&buf[..n.min(buf.len())]).split('\0').filter(|r| !r.is_empty()) {
        let w = wide(root);
        if unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) } != 3 {
            continue; // DRIVE_FIXED
        }
        let (mut free, mut total) = (0u64, 0u64);
        if unsafe { GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), None, Some(&mut total), Some(&mut free)) }.is_err() || total == 0 {
            continue;
        }
        let mut name = [0u16; 128];
        let _ = unsafe { GetVolumeInformationW(PCWSTR(w.as_ptr()), Some(&mut name), None, None, None, None) };
        let len = name.iter().position(|&c| c == 0).unwrap_or(0);
        let label = String::from_utf16_lossy(&name[..len]);
        let letter = root.trim_end_matches('\\');
        let label = if label.is_empty() { format!("Disco local ({letter})") } else { format!("{label} ({letter})") };
        out.push(Drive { label, used: total - free, total, icon: shell_icon(root).map(Arc::new) });
    }
    out
}

/// Bytes recebidos e enviados até agora, somando as placas de rede físicas ligadas.
pub fn net_bytes() -> (u64, u64) {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    unsafe {
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return (0, 0);
        }
        let n = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
        let (mut rx, mut tx) = (0u64, 0u64);
        for row in rows {
            let hardware = row.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
            if hardware && row.OperStatus == IfOperStatusUp && row.Type != 24 {
                rx += row.InOctets;
                tx += row.OutOctets;
            }
        }
        FreeMibTable(table as _);
        (rx, tx)
    }
}

/// Velocidade atual do processador, como o Gerenciador de Tarefas mostra: a velocidade base
/// vezes o "% de desempenho do processador" (passa de 100% no turbo, cai na economia).
pub struct CpuSpeed {
    query: windows::Win32::System::Performance::PDH_HQUERY,
    counter: windows::Win32::System::Performance::PDH_HCOUNTER,
    base_mhz: f64,
}

impl CpuSpeed {
    pub fn new() -> Option<CpuSpeed> {
        use windows::Win32::System::Performance::*;
        use windows::Win32::System::Power::{CallNtPowerInformation, ProcessorInformation, PROCESSOR_POWER_INFORMATION};
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let mut info = vec![PROCESSOR_POWER_INFORMATION::default(); n];
        let size = (n * std::mem::size_of::<PROCESSOR_POWER_INFORMATION>()) as u32;
        let ok = unsafe { CallNtPowerInformation(ProcessorInformation, None, 0, Some(info.as_mut_ptr() as _), size) };
        let base_mhz = info.first().map(|p| p.MaxMhz as f64).unwrap_or(0.0);
        if ok.is_err() || base_mhz <= 0.0 {
            return None;
        }
        unsafe {
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            let mut counter = PDH_HCOUNTER::default();
            let path = wide(r"\Processor Information(_Total)\% Processor Performance");
            if PdhAddEnglishCounterW(query, PCWSTR(path.as_ptr()), 0, &mut counter) != 0 {
                let _ = PdhCloseQuery(query);
                return None;
            }
            let _ = PdhCollectQueryData(query); // a primeira leitura só serve de base
            Some(CpuSpeed { query, counter, base_mhz })
        }
    }

    /// Lê de novo (chamar a cada poucos segundos); None até ter duas leituras.
    pub fn ghz(&self) -> Option<f64> {
        use windows::Win32::System::Performance::*;
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let mut value = PDH_FMT_COUNTERVALUE::default();
            if PdhGetFormattedCounterValue(self.counter, PDH_FMT_DOUBLE, None, &mut value) != 0 {
                return None;
            }
            let pct = value.Anonymous.doubleValue;
            (pct > 0.0).then(|| self.base_mhz * pct / 100.0 / 1000.0)
        }
    }
}

impl Drop for CpuSpeed {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Performance::PdhCloseQuery(self.query);
        }
    }
}

/// Segundos desde que o Windows ligou.
pub fn uptime() -> u64 {
    unsafe { GetTickCount64() / 1000 }
}

/// "3 h 12 min", "2 dias e 4 h".
pub fn fmt_uptime(secs: u64) -> String {
    let (d, h, m) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60);
    match (d, h) {
        (0, 0) => format!("{m} min"),
        (0, _) => format!("{h} h {m} min"),
        (1, _) => format!("1 dia e {h} h"),
        _ => format!("{d} dias e {h} h"),
    }
}

/// Bytes por segundo de forma curta: "2,4 MB/s", "180 KB/s".
pub fn fmt_rate(bytes: f64) -> String {
    if bytes >= 1e6 {
        format!("{} MB/s", format!("{:.1}", bytes / 1e6).replace('.', ","))
    } else {
        format!("{} KB/s", (bytes / 1e3).round() as i64)
    }
}

/// "2,4 GB", "610 MB".
pub fn fmt_bytes(b: u64) -> String {
    let gb = b as f64 / 1024f64.powi(3);
    if gb >= 1.0 {
        format!("{} GB", format!("{gb:.1}").replace('.', ","))
    } else {
        format!("{} MB", (b as f64 / 1024f64.powi(2)).round() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // lê o sistema de verdade: cargo test -- --ignored --nocapture
    fn read_everything() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let speed = CpuSpeed::new().expect("velocidade da CPU");
        let s = Sampler::new();
        s.set_active(true);
        std::thread::sleep(Duration::from_millis(1000));
        println!("CPU: {:.2} GHz (base {:.0} MHz)", speed.ghz().expect("leitura da velocidade"), speed.base_mhz);
        std::thread::sleep(Duration::from_millis(3500));
        let mut apps = s.latest().expect("leitura");
        apps.sort_by(|a, b| b.mem.cmp(&a.mem));
        for a in apps.iter().take(6) {
            println!("mem  {:<36} {:>9}  ícone: {}", a.name, fmt_bytes(a.mem), a.icon.is_some());
        }
        apps.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        for a in apps.iter().take(4) {
            println!("cpu  {:<36} {:>5.1}%", a.name, a.cpu);
        }
        for d in drives() {
            println!("disco {} {} de {}  ícone: {}", d.label, fmt_bytes(d.used), fmt_bytes(d.total), d.icon.is_some());
        }
        let (rx, tx) = net_bytes();
        println!("rede: {rx} / {tx} bytes; ligado há {}", fmt_uptime(uptime()));
        assert!(!apps.is_empty());
    }
}

#[cfg(test)]
mod icon_sheet {
    use super::*;

    #[test]
    #[ignore] // folha com os ícones dos apps abertos: cargo test -- --ignored
    fn icons() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let s = Sampler::new();
        s.set_active(true);
        std::thread::sleep(Duration::from_millis(2500));
        let mut apps = s.latest().unwrap_or_default();
        apps.sort_by(|a, b| b.mem.cmp(&a.mem));
        let apps: Vec<AppUse> = apps.into_iter().filter(|a| a.icon.is_some()).take(16).collect();
        let mut c = crate::gfx::Canvas::new(16 * 44 + 8, 52, 1.0);
        c.fill_round_rect(0.0, 0.0, 16.0 * 44.0 + 8.0, 52.0, 0.0, crate::theme::Rgba::rgb(40, 40, 50));
        for (i, a) in apps.iter().enumerate() {
            c.image_px(a.icon.as_ref().unwrap(), 10.0 + 44.0 * i as f32, 10.0, 32.0, 32.0, 1.0);
            println!("{}", a.name);
        }
        let path = std::env::temp_dir().join("icones.png");
        crate::gfx::save_png(&c.finish(), &path).unwrap();
        println!("{}", path.display());
    }
}
