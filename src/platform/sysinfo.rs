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

/// Uma unidade de disco fixa: o nome como no Explorador ("Disco local (C:)"), o espaço em bytes
/// e o ícone.
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
    let mut px = draw_icon(icon, size)?;
    make_drawn_opaque_if_no_alpha(&mut px);
    Some(Image { w: size, h: size, px })
}

/// Desenha o ícone num bitmap BGRA de `size` × `size` e devolve os pixels (None: o Windows não
/// desenhou).
fn draw_icon(icon: HICON, size: u32) -> Option<Vec<u8>> {
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
        let byte_count = (size * size * 4) as usize;
        let px = std::slice::from_raw_parts(bits as *const u8, byte_count).to_vec();
        SelectObject(dc, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(dc);
        drawn.then_some(px)
    }
}

/// Ícones antigos não têm canal alfa (vem tudo transparente): o que foi desenhado (qualquer cor
/// que não seja preto puro) fica opaco.
fn make_drawn_opaque_if_no_alpha(px: &mut [u8]) {
    if px.chunks(4).all(|pixel| pixel[3] == 0) {
        for pixel in px.chunks_mut(4) {
            if pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0 {
                pixel[3] = 255;
            }
        }
    }
}

/// Lado em pixels dos ícones guardados (desenhados menores, ficam nítidos em qualquer escala).
pub const ICON_PX: u32 = 64;

/// Até esse alfa, o pixel conta como transparente (a sombra quase invisível de alguns ícones).
const VISIBLE_ALPHA: u8 = 8;

/// O ícone grande de verdade ocupa ao menos 70% do lado da imagem (em décimos); menos que isso,
/// é um ícone pequeno que o Windows pôs num canto.
const FILLS_THE_IMAGE_TENTHS: u32 = 7;

/// Tamanho do desenho dentro do ícone: alguns apps só têm ícone pequeno, e o Windows o devolve
/// num canto da imagem grande.
fn drawn_size(img: &Image) -> u32 {
    let (mut right, mut bottom) = (0, 0);
    for y in 0..img.h {
        for x in 0..img.w {
            if img.px[((y * img.w + x) * 4 + 3) as usize] > VISIBLE_ALPHA {
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
    }
    right.max(bottom)
}

/// Ícone de um arquivo ou unidade em alta definição, como no Explorador (lista "jumbo" de
/// 256 px; sem ela, a de 48 px), reduzido com boa qualidade para ICON_PX.
pub fn shell_icon(path: &str) -> Option<Image> {
    let path = wide(path);
    large_shell_icon(&path).or_else(|| small_shell_icon(&path))
}

/// O ícone das listas grandes do sistema (256 px e 48 px), se o desenho ocupa a imagem.
fn large_shell_icon(path: &[u16]) -> Option<Image> {
    let mut info = SHFILEINFOW::default();
    unsafe {
        let found = SHGetFileInfoW(PCWSTR(path.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX);
        if found == 0 {
            return None;
        }
        for (list, size) in [(SHIL_JUMBO, 256u32), (SHIL_EXTRALARGE, 48)] {
            let Ok(images) = SHGetImageList::<IImageList>(list as i32) else { continue };
            let Ok(icon) = images.GetIcon(info.iIcon, ILD_TRANSPARENT.0) else { continue };
            let img = icon_image(icon, size);
            let _ = DestroyIcon(icon);
            let Some(img) = img else { continue };
            if drawn_size(&img) * 10 < size * FILLS_THE_IMAGE_TENTHS {
                continue;
            }
            return Some(crate::gfx::resize(&img, ICON_PX, ICON_PX));
        }
        None
    }
}

/// O último recurso: o ícone comum de 32 px.
fn small_shell_icon(path: &[u16]) -> Option<Image> {
    let mut info = SHFILEINFOW::default();
    unsafe {
        let found = SHGetFileInfoW(PCWSTR(path.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_LARGEICON);
        if found == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let img = icon_image(info.hIcon, 32);
        let _ = DestroyIcon(info.hIcon);
        img
    }
}

/// O idioma usado quando o executável não traz a tabela de traduções: inglês dos EUA, Unicode.
const DEFAULT_LANGUAGE: &str = "040904b0";

/// "Descrição do arquivo" do executável (ex.: "Google Chrome"), em vez de "chrome.exe".
fn file_description(path: &str) -> Option<String> {
    let path = wide(path);
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr() as _).ok()?;
        // primeiro idioma da tabela de traduções
        let mut value: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let query = wide("\\VarFileInfo\\Translation");
        let language = if VerQueryValueW(data.as_ptr() as _, PCWSTR(query.as_ptr()), &mut value, &mut len).as_bool() && len >= 4 {
            let pair = std::slice::from_raw_parts(value as *const u16, 2);
            format!("{:04x}{:04x}", pair[0], pair[1])
        } else {
            DEFAULT_LANGUAGE.into()
        };
        let query = wide(&format!("\\StringFileInfo\\{language}\\FileDescription"));
        if !VerQueryValueW(data.as_ptr() as _, PCWSTR(query.as_ptr()), &mut value, &mut len).as_bool() || len == 0 {
            return None;
        }
        let text = std::slice::from_raw_parts(value as *const u16, len as usize);
        // o texto termina no primeiro nulo (depois dele pode vir lixo da tabela)
        let end = text.iter().position(|&unit| unit == 0).unwrap_or(text.len());
        let description = String::from_utf16_lossy(&text[..end]).trim().to_string();
        (!description.is_empty()).then_some(description)
    }
}

/// "msedge.exe" -> "Msedge" (quando o executável não tem descrição).
fn pretty_exe(exe: &str) -> String {
    let stem = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    let stem = stem.strip_suffix(".exe").or(stem.strip_suffix(".EXE")).unwrap_or(stem);
    let mut chars = stem.chars();
    chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

// ---------- processos ----------

/// Um FILETIME (dois u32) como um número só, em unidades de 100 ns.
fn filetime_ticks(time: FILETIME) -> u64 {
    ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64
}

/// Os processos até este número são do próprio Windows (0 é o "ocioso", 4 é o "Sistema") e
/// não dão para ler.
const LAST_KERNEL_PID: u32 = 4;

/// Um processo aberto: o número e o nome do executável ("chrome.exe").
struct Process {
    pid: u32,
    exe: String,
}

/// Os processos abertos agora (sem os do próprio Windows).
fn list_processes() -> Vec<Process> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let len = entry.szExeFile.iter().position(|&unit| unit == 0).unwrap_or(entry.szExeFile.len());
            let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if entry.th32ProcessID > LAST_KERNEL_PID {
                out.push(Process { pid: entry.th32ProcessID, exe });
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

/// O caminho completo do executável de um processo aberto.
fn image_path(process: HANDLE) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).ok()? };
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

/// O que se lê de um processo de cada vez.
struct ProcessUse {
    /// o caminho do executável (None: o Windows não deixou ler)
    path: Option<String>,
    /// o tempo de CPU desde que abriu (núcleo e usuário), em unidades de 100 ns
    cpu_ticks: u64,
    /// a memória privada em uso, em bytes
    private_bytes: u64,
}

/// Lê um processo (None: não deixou abrir, como os protegidos do sistema).
fn read_process(pid: u32) -> Option<ProcessUse> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let cpu_ticks = if GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user).is_ok() { filetime_ticks(kernel) + filetime_ticks(user) } else { 0 };
        let mut memory = PROCESS_MEMORY_COUNTERS_EX2 { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
        let private_bytes = if GetProcessMemoryInfo(process, &mut memory as *mut _ as *mut PROCESS_MEMORY_COUNTERS, memory.cb).is_ok() {
            memory.PrivateWorkingSetSize as u64
        } else {
            0
        };
        let path = image_path(process);
        let _ = CloseHandle(process);
        Some(ProcessUse { path, cpu_ticks, private_bytes })
    }
}

/// Unidades de 100 ns (as do tempo de CPU do Windows) num segundo.
const TICKS_PER_SEC: f64 = 1e7;

/// A parte da CPU inteira (todos os núcleos, em %) que `cpu_ticks` de tempo de CPU representam
/// em `wall_secs` de relógio (0 sem uma leitura anterior para comparar).
fn cpu_share(cpu_ticks: u64, wall_secs: f64, cores: f64) -> f64 {
    if wall_secs > 0.0 {
        cpu_ticks as f64 / TICKS_PER_SEC / (wall_secs * cores) * 100.0
    } else {
        0.0
    }
}

/// De quanto em quanto tempo os apps são lidos enquanto a lista está na tela.
const SAMPLE_EVERY: Duration = Duration::from_millis(2000);
/// A CPU de um app é a diferença entre duas leituras: a segunda, logo depois da primeira, para a
/// lista não esperar `SAMPLE_EVERY` mostrando zero.
const SECOND_SAMPLE_AFTER: Duration = Duration::from_millis(700);
/// Com a lista fora da tela, de quanto em quanto tempo a thread confere se ela voltou.
const IDLE_CHECK_EVERY: Duration = Duration::from_millis(400);

/// A lista dos apps, compartilhada entre a thread que lê e o widget (None até a primeira leitura).
type SharedApps = Arc<Mutex<Option<Vec<AppUse>>>>;

/// Lê os apps numa thread, só enquanto `active` estiver ligado.
pub struct Sampler {
    shared: SharedApps,
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
        let (thread_shared, thread_active, thread_alive) = (shared.clone(), active.clone(), alive.clone());
        std::thread::spawn(move || sample_loop(thread_shared, thread_active, thread_alive));
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

/// Um executável já visto: o nome bonito e o ícone (lidos uma vez só, porque custam caro).
type Known = (String, Option<Arc<Image>>);

/// O que a thread lembra de uma leitura para a outra.
struct Sampling {
    /// os núcleos do processador (a CPU de um app é a parte deles todos)
    cores: f64,
    /// por caminho do executável
    known: HashMap<String, Known>,
    /// o tempo de CPU de cada processo na leitura anterior
    last_ticks: HashMap<u32, u64>,
    /// quando foi a leitura anterior (None: ainda não houve, ou a lista saiu da tela)
    last_at: Option<Instant>,
}

impl Sampling {
    /// Lê todos os processos e soma por executável: a CPU desde a leitura anterior e a memória.
    fn sample(&mut self) -> Vec<AppUse> {
        let now = Instant::now();
        let wall_secs = self.last_at.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        // por executável: (% de CPU, bytes de memória)
        let mut groups: HashMap<String, (f64, u64)> = HashMap::new();
        let mut ticks_now = HashMap::new();
        for process in list_processes() {
            let Some(usage) = read_process(process.pid) else { continue };
            let key = usage.path.clone().unwrap_or_else(|| process.exe.clone());
            let ticks_since = self.last_ticks.get(&process.pid).map(|&prev| usage.cpu_ticks.saturating_sub(prev)).unwrap_or(0);
            ticks_now.insert(process.pid, usage.cpu_ticks);
            let group = groups.entry(key.clone()).or_insert((0.0, 0));
            group.0 += cpu_share(ticks_since, wall_secs, self.cores);
            group.1 += usage.private_bytes;
            self.known.entry(key).or_insert_with(|| {
                let name = usage.path.as_deref().and_then(file_description).unwrap_or_else(|| pretty_exe(&process.exe));
                let icon = usage.path.as_deref().and_then(shell_icon).map(Arc::new);
                (name, icon)
            });
        }
        self.last_ticks = ticks_now;
        self.last_at = Some(now);
        groups
            .into_iter()
            .map(|(key, (cpu, mem))| {
                let (name, icon) = self.known.get(&key).cloned().unwrap_or_default();
                AppUse { name, cpu: cpu as f32, mem, icon }
            })
            .collect()
    }

    /// A lista saiu da tela: a próxima leitura começa do zero (sem diferença para comparar).
    fn forget(&mut self) {
        self.last_ticks.clear();
        self.last_at = None;
    }
}

/// A thread dos apps: lê enquanto `active` estiver ligado e até `alive` desligar.
fn sample_loop(shared: SharedApps, active: Arc<AtomicBool>, alive: Arc<AtomicBool>) {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
    let mut sampling = Sampling { cores, known: HashMap::new(), last_ticks: HashMap::new(), last_at: None };
    // a primeira leitura acontece logo ao abrir: nomes e ícones já ficam prontos para o clique
    let mut warm = true;
    while alive.load(Ordering::Relaxed) {
        if !warm && !active.load(Ordering::Relaxed) {
            sampling.forget();
            std::thread::sleep(IDLE_CHECK_EVERY);
            continue;
        }
        let first = sampling.last_at.is_none();
        let apps = sampling.sample();
        warm = false;
        *shared.lock().unwrap() = Some(apps);
        crate::platform::events::wake();
        std::thread::sleep(if first { SECOND_SAMPLE_AFTER } else { SAMPLE_EVERY });
    }
}

// ---------- discos, rede, tempo ligado ----------

/// O tipo das unidades fixas (os discos do PC; não pendrives, rede nem leitores de CD), como o
/// `GetDriveTypeW` devolve.
const DRIVE_FIXED: u32 = 3;

/// Unidades fixas (C:, D:...), com nome, espaço e ícone.
pub fn drives() -> Vec<Drive> {
    let mut buf = [0u16; 256];
    let len = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) } as usize;
    String::from_utf16_lossy(&buf[..len.min(buf.len())]).split('\0').filter(|root| !root.is_empty()).filter_map(fixed_drive).collect()
}

/// A unidade na raiz `root` ("C:\"), se for fixa e der para ler o espaço.
fn fixed_drive(root: &str) -> Option<Drive> {
    let root_w = wide(root);
    if unsafe { GetDriveTypeW(PCWSTR(root_w.as_ptr())) } != DRIVE_FIXED {
        return None;
    }
    let (mut free, mut total) = (0u64, 0u64);
    if unsafe { GetDiskFreeSpaceExW(PCWSTR(root_w.as_ptr()), None, Some(&mut total), Some(&mut free)) }.is_err() || total == 0 {
        return None;
    }
    let mut name = [0u16; 128];
    let _ = unsafe { GetVolumeInformationW(PCWSTR(root_w.as_ptr()), Some(&mut name), None, None, None, None) };
    let len = name.iter().position(|&unit| unit == 0).unwrap_or(0);
    let name = String::from_utf16_lossy(&name[..len]);
    let letter = root.trim_end_matches('\\');
    let label = if name.is_empty() { format!("Disco local ({letter})") } else { format!("{name} ({letter})") };
    Some(Drive { label, used: total - free, total, icon: shell_icon(root).map(Arc::new) })
}

/// O bit de `InterfaceAndOperStatusFlags` que marca uma placa de rede física (não virtual).
const HARDWARE_INTERFACE: u8 = 1;
/// O tipo da interface de loopback (a rede de dentro do próprio PC), que não conta.
const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;

/// Bytes recebidos e enviados até agora, somando as placas de rede físicas ligadas.
pub fn net_bytes() -> (u64, u64) {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    unsafe {
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return (0, 0);
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let (mut received, mut sent) = (0u64, 0u64);
        for row in rows {
            let hardware = row.InterfaceAndOperStatusFlags._bitfield & HARDWARE_INTERFACE != 0;
            if hardware && row.OperStatus == IfOperStatusUp && row.Type != IF_TYPE_SOFTWARE_LOOPBACK {
                received += row.InOctets;
                sent += row.OutOctets;
            }
        }
        FreeMibTable(table as _);
        (received, sent)
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
        let base_mhz = base_mhz()?;
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
            let percent = value.Anonymous.doubleValue;
            (percent > 0.0).then(|| self.base_mhz * percent / 100.0 / 1000.0)
        }
    }
}

/// A velocidade base do processador em MHz (a do primeiro núcleo; None: o Windows não disse).
fn base_mhz() -> Option<f64> {
    use windows::Win32::System::Power::{CallNtPowerInformation, ProcessorInformation, PROCESSOR_POWER_INFORMATION};
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let mut info = vec![PROCESSOR_POWER_INFORMATION::default(); cores];
    let size = (cores * std::mem::size_of::<PROCESSOR_POWER_INFORMATION>()) as u32;
    let read = unsafe { CallNtPowerInformation(ProcessorInformation, None, 0, Some(info.as_mut_ptr() as _), size) };
    let base_mhz = info.first().map(|core| core.MaxMhz as f64).unwrap_or(0.0);
    (read.is_ok() && base_mhz > 0.0).then_some(base_mhz)
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
    let (days, hours, minutes) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60);
    match (days, hours) {
        (0, 0) => format!("{minutes} min"),
        (0, _) => format!("{hours} h {minutes} min"),
        (1, _) => format!("1 dia e {hours} h"),
        _ => format!("{days} dias e {hours} h"),
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
pub fn fmt_bytes(bytes: u64) -> String {
    let gb = bytes as f64 / 1024f64.powi(3);
    if gb >= 1.0 {
        format!("{} GB", format!("{gb:.1}").replace('.', ","))
    } else {
        format!("{} MB", (bytes as f64 / 1024f64.powi(2)).round() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_reads_in_minutes_hours_and_days() {
        assert_eq!(fmt_uptime(59), "0 min");
        assert_eq!(fmt_uptime(12 * 60), "12 min");
        assert_eq!(fmt_uptime(3 * 3600 + 12 * 60 + 30), "3 h 12 min");
        assert_eq!(fmt_uptime(86_400 + 4 * 3600 + 59 * 60), "1 dia e 4 h");
        assert_eq!(fmt_uptime(2 * 86_400 + 4 * 3600), "2 dias e 4 h");
    }

    #[test]
    fn rates_and_sizes_are_short_with_a_comma() {
        assert_eq!(fmt_rate(2_400_000.0), "2,4 MB/s");
        assert_eq!(fmt_rate(1_000_000.0), "1,0 MB/s");
        assert_eq!(fmt_rate(180_400.0), "180 KB/s");
        assert_eq!(fmt_rate(0.0), "0 KB/s");
        assert_eq!(fmt_bytes(2_576_980_378), "2,4 GB");
        assert_eq!(fmt_bytes(1024 * 1024 * 1024), "1,0 GB");
        assert_eq!(fmt_bytes(610 * 1024 * 1024), "610 MB");
        assert_eq!(fmt_bytes(0), "0 MB");
    }

    #[test]
    fn executables_without_a_description_get_a_tidy_name() {
        assert_eq!(pretty_exe("msedge.exe"), "Msedge");
        assert_eq!(pretty_exe(r"C:\Program Files\App\TOOL.EXE"), "TOOL");
        assert_eq!(pretty_exe("C:/x/notepad"), "Notepad");
        assert_eq!(pretty_exe(""), "");
    }

    /// O tamanho do desenho é o lado do quadrado (a partir do canto) que cobre todos os pixels
    /// visíveis; quase transparentes (alfa até 8) não contam.
    #[test]
    fn drawn_size_finds_a_small_icon_in_the_corner() {
        let mut img = Image { w: 64, h: 64, px: vec![0; 64 * 64 * 4] };
        assert_eq!(drawn_size(&img), 0);
        let alpha_at = |x: u32, y: u32| ((y * 64 + x) * 4 + 3) as usize;
        img.px[alpha_at(20, 31)] = 255;
        assert_eq!(drawn_size(&img), 32);
        img.px[alpha_at(50, 2)] = 8;
        assert_eq!(drawn_size(&img), 32);
        img.px[alpha_at(50, 2)] = 9;
        assert_eq!(drawn_size(&img), 51);
    }

    /// A parte da CPU inteira usada por um processo entre duas leituras.
    #[test]
    fn cpu_share_is_of_the_whole_processor() {
        // 1 s de CPU (10 milhões de unidades de 100 ns) em 2 s, num processador de 4 núcleos
        assert!((cpu_share(10_000_000, 2.0, 4.0) - 12.5).abs() < 1e-9);
        assert_eq!(cpu_share(10_000_000, 0.0, 4.0), 0.0);
    }

    #[test]
    #[ignore] // lê o sistema de verdade: cargo test -- --ignored --nocapture
    fn read_everything() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let speed = CpuSpeed::new().expect("velocidade da CPU");
        let sampler = Sampler::new();
        sampler.set_active(true);
        std::thread::sleep(Duration::from_millis(1000));
        println!("CPU: {:.2} GHz (base {:.0} MHz)", speed.ghz().expect("leitura da velocidade"), speed.base_mhz);
        std::thread::sleep(Duration::from_millis(3500));
        let mut apps = sampler.latest().expect("leitura");
        apps.sort_by(|a, b| b.mem.cmp(&a.mem));
        for app in apps.iter().take(6) {
            println!("mem  {:<36} {:>9}  ícone: {}", app.name, fmt_bytes(app.mem), app.icon.is_some());
        }
        apps.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        for app in apps.iter().take(4) {
            println!("cpu  {:<36} {:>5.1}%", app.name, app.cpu);
        }
        for drive in drives() {
            println!("disco {} {} de {}  ícone: {}", drive.label, fmt_bytes(drive.used), fmt_bytes(drive.total), drive.icon.is_some());
        }
        let (received, sent) = net_bytes();
        println!("rede: {received} / {sent} bytes; ligado há {}", fmt_uptime(uptime()));
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
        let sampler = Sampler::new();
        sampler.set_active(true);
        std::thread::sleep(Duration::from_millis(2500));
        let mut apps = sampler.latest().unwrap_or_default();
        apps.sort_by(|a, b| b.mem.cmp(&a.mem));
        let apps: Vec<AppUse> = apps.into_iter().filter(|app| app.icon.is_some()).take(16).collect();
        let mut c = crate::gfx::Canvas::new(16 * 44 + 8, 52, 1.0);
        c.fill_round_rect(0.0, 0.0, 16.0 * 44.0 + 8.0, 52.0, 0.0, crate::ui::theme::Rgba::rgb(40, 40, 50));
        for (index, app) in apps.iter().enumerate() {
            c.image_px(app.icon.as_ref().unwrap(), 10.0 + 44.0 * index as f32, 10.0, 32.0, 32.0, 1.0);
            println!("{}", app.name);
        }
        let path = std::env::temp_dir().join("icones.png");
        crate::gfx::save_png(&c.finish(), &path).unwrap();
        println!("{}", path.display());
    }
}
