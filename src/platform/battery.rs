//! A bateria do notebook e a dos dispositivos conectados (fones, mouses, teclados, controles).
//!
//! - O notebook: a carga, se está na tomada e quanto falta vêm do Windows (`GetSystemPowerStatus`);
//!   a saúde (a capacidade de agora contra a de fábrica), os ciclos e o ritmo de carga, do driver
//!   da bateria (o mesmo do `powercfg /batteryreport`).
//! - Os dispositivos: o Windows guarda a bateria dos que mandam esse dado (a maioria dos Bluetooth
//!   LE e dos fones com perfil de chamada) numa propriedade do aparelho no Gerenciador de
//!   Dispositivos; se cada um está conectado agora, o próprio aparelho responde, pelo endereço
//!   Bluetooth (WinRT). A lista de pareados do Windows também diria, mas demora meio minuto.
//!
//! Ler os dispositivos leva uns 100 ms e chama o WinRT: é feito num fio à parte (`watch_devices`),
//! que lê de novo a cada meio minuto (a carga) e logo depois de um aparelho conectar ou
//! desconectar: o Windows avisa na hora, por aparelho.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW, SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    HDEVINFO, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::DEVPROPTYPE;
use windows::Win32::Foundation::DEVPROPKEY;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::IO::DeviceIoControl;

// ---------- o notebook ----------

/// A bateria do notebook agora.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Power {
    /// carga, de 0 a 100
    pub percent: f32,
    /// na tomada
    pub plugged: bool,
    /// carregando agora (na tomada, mas cheia ou parada num limite, não carrega)
    pub charging: bool,
    /// quanto falta para acabar, na bateria (o Windows calcula), em segundos
    pub secs_left: Option<u32>,
    /// quanto falta para encher, carregando, em segundos (pelo ritmo de carga do driver)
    pub secs_to_full: Option<u32>,
    /// a capacidade de agora sobre a de fábrica (0 a 1)
    pub health: Option<f32>,
    /// quantas cargas completas a bateria já fez (do driver; None: o driver não conta)
    pub cycles: Option<u32>,
    /// a energia que resta, em mWh (do driver)
    pub left_mwh: Option<u32>,
    /// o consumo agora, na bateria, em mW (do driver; varia muito de um segundo para outro)
    pub drain_mw: Option<u32>,
}

/// `ACLineStatus` do `SYSTEM_POWER_STATUS`: fora e dentro da tomada (255: não sabe).
const AC_OFFLINE: u8 = 0;
const AC_ONLINE: u8 = 1;
/// Os bits de `BatteryFlag`: carregando e sem bateria; o valor inteiro 255 é "não sabe".
const FLAG_CHARGING: u8 = 8;
const FLAG_NO_BATTERY: u8 = 128;
const FLAG_UNKNOWN: u8 = 255;
/// `BatteryLifeTime` quando o Windows não sabe quanto falta (na tomada, ou logo depois de tirar).
const LIFETIME_UNKNOWN: u32 = u32::MAX;

/// O estado de energia que o Windows informa (None: não deu para ler).
fn system_power_status() -> Option<SYSTEM_POWER_STATUS> {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status).ok()? };
    Some(status)
}

/// Só se está na tomada (None: não sabe). Barato (sem perguntar ao driver): dá para conferir a
/// cada segundo e ler o resto (`power`) só quando muda.
pub fn on_ac() -> Option<bool> {
    match system_power_status()?.ACLineStatus {
        AC_OFFLINE => Some(false),
        AC_ONLINE => Some(true),
        _ => None,
    }
}

/// A bateria do notebook (None: o PC não tem bateria).
pub fn power() -> Option<Power> {
    let mut power = windows_power(&system_power_status()?)?;
    if let Some(details) = driver_details() {
        add_driver_details(&mut power, &details);
    }
    Some(power)
}

/// O que o Windows diz da bateria: a carga, a tomada e quanto falta (None: sem bateria, ou o
/// Windows não sabe).
fn windows_power(status: &SYSTEM_POWER_STATUS) -> Option<Power> {
    if status.BatteryFlag & FLAG_NO_BATTERY != 0 || status.BatteryFlag == FLAG_UNKNOWN || status.BatteryLifePercent > 100 {
        return None;
    }
    let plugged = status.ACLineStatus == AC_ONLINE;
    Some(Power {
        percent: status.BatteryLifePercent as f32,
        plugged,
        charging: plugged && status.BatteryFlag & FLAG_CHARGING != 0,
        secs_left: (!plugged && status.BatteryLifeTime != LIFETIME_UNKNOWN).then_some(status.BatteryLifeTime),
        ..Default::default()
    })
}

const SECS_PER_HOUR: f64 = 3600.0;

/// Completa a bateria com o que só o driver sabe: a saúde, os ciclos, a energia que resta, o
/// consumo (na bateria) e quanto falta para encher (carregando, pelo ritmo de agora).
fn add_driver_details(power: &mut Power, details: &Details) {
    if details.designed > 0 && details.full > 0 {
        power.health = Some((details.full as f32 / details.designed as f32).min(1.0));
    }
    power.cycles = (details.cycles > 0).then_some(details.cycles);
    power.left_mwh = (details.capacity > 0).then_some(details.capacity);
    if !power.plugged && details.rate < 0 {
        power.drain_mw = Some(details.rate.unsigned_abs());
    }
    if power.charging && details.rate > 0 && details.full > details.capacity {
        let hours_to_full = (details.full - details.capacity) as f64 / details.rate as f64;
        power.secs_to_full = Some((hours_to_full * SECS_PER_HOUR) as u32);
    }
}

/// O que o driver da bateria informa: capacidades em mWh, ciclos e o ritmo de agora em mW
/// (positivo carregando). Zero onde o driver não sabe.
struct Details {
    /// a capacidade de fábrica
    designed: u32,
    /// a capacidade de uma carga completa hoje (cai com o uso)
    full: u32,
    cycles: u32,
    /// a energia que resta agora
    capacity: u32,
    rate: i32,
}

/// A interface das baterias (GUID_DEVICE_BATTERY) e os pedidos ao driver (batclass.h).
const GUID_DEVICE_BATTERY: GUID = GUID::from_u128(0x72631e54_78a4_11d0_bcf7_00aa00b7b32a);
const IOCTL_BATTERY_QUERY_TAG: u32 = 0x294040;
const IOCTL_BATTERY_QUERY_INFORMATION: u32 = 0x294044;
const IOCTL_BATTERY_QUERY_STATUS: u32 = 0x29404c;
/// O nível de `IOCTL_BATTERY_QUERY_INFORMATION` que devolve a `BatteryInformation`.
const LEVEL_BATTERY_INFORMATION: i32 = 0;
/// Os valores que o driver devolve quando não sabe (a capacidade e o ritmo).
const DRIVER_UNKNOWN: u32 = u32::MAX;
const DRIVER_UNKNOWN_RATE: i32 = i32::MIN;

#[repr(C)]
#[derive(Default)]
struct BatteryQueryInformation {
    tag: u32,
    level: i32,
    at_rate: i32,
}

#[repr(C)]
#[derive(Default)]
struct BatteryInformation {
    capabilities: u32,
    technology: u8,
    reserved: [u8; 3],
    chemistry: [u8; 4],
    designed_capacity: u32,
    full_charged_capacity: u32,
    default_alert1: u32,
    default_alert2: u32,
    critical_bias: u32,
    cycle_count: u32,
}

#[repr(C)]
#[derive(Default)]
struct BatteryWaitStatus {
    tag: u32,
    timeout: u32,
    power_state: u32,
    low_capacity: u32,
    high_capacity: u32,
}

#[repr(C)]
#[derive(Default)]
struct BatteryStatus {
    power_state: u32,
    capacity: u32,
    voltage: u32,
    rate: i32,
}

/// Um pedido ao driver: manda `input` e recebe `output`.
unsafe fn ioctl<I, O>(device: HANDLE, code: u32, input: &I, output: &mut O) -> bool {
    let mut got = 0u32;
    unsafe { DeviceIoControl(device, code, Some(input as *const I as *const _), size_of::<I>() as u32, Some(output as *mut O as *mut _), size_of::<O>() as u32, Some(&mut got), None).is_ok() }
}

/// Lê o driver da primeira bateria.
fn driver_details() -> Option<Details> {
    let path = battery_device_path()?;
    unsafe {
        let battery = CreateFileW(PCWSTR(path.as_ptr()), (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None).ok()?;
        let details = query_driver(battery);
        let _ = CloseHandle(battery);
        details
    }
}

/// O caminho do aparelho da primeira bateria (terminado em nulo, para o `CreateFileW`).
fn battery_device_path() -> Option<Vec<u16>> {
    unsafe {
        let set = SetupDiGetClassDevsW(Some(&GUID_DEVICE_BATTERY), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE).ok()?;
        let mut iface = SP_DEVICE_INTERFACE_DATA { cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
        let found = SetupDiEnumDeviceInterfaces(set, None, &GUID_DEVICE_BATTERY, 0, &mut iface).is_ok();
        let path = if found { interface_path(set, &iface) } else { None };
        let _ = SetupDiDestroyDeviceInfoList(set);
        path
    }
}

/// O caminho de uma interface de aparelho: o Windows o escreve logo depois de um cabeçalho, num
/// espaço do tamanho que ele mesmo diz na primeira chamada.
unsafe fn interface_path(set: HDEVINFO, iface: &SP_DEVICE_INTERFACE_DATA) -> Option<Vec<u16>> {
    unsafe {
        let mut need = 0u32;
        let _ = SetupDiGetDeviceInterfaceDetailW(set, iface, None, 0, Some(&mut need), None);
        if need as usize <= size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() {
            return None;
        }
        // o tamanho do cabeçalho e o caminho logo depois (em u32, para ficar alinhado)
        let mut buf = vec![0u32; (need as usize).div_ceil(4)];
        let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        SetupDiGetDeviceInterfaceDetailW(set, iface, Some(detail), need, None, None).ok()?;
        let start = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
        let len = (0..).take_while(|&i| *start.add(i) != 0).count();
        let mut path = std::slice::from_raw_parts(start, len).to_vec();
        path.push(0);
        Some(path)
    }
}

/// Pergunta ao driver aberto em `battery` a etiqueta da bateria e, com ela, as capacidades, os
/// ciclos e o estado de agora (None: sem bateria no compartimento, ou o driver não respondeu).
unsafe fn query_driver(battery: HANDLE) -> Option<Details> {
    unsafe {
        let mut tag = 0u32;
        let no_wait = 0u32;
        if !ioctl(battery, IOCTL_BATTERY_QUERY_TAG, &no_wait, &mut tag) || tag == 0 {
            return None;
        }
        let mut info = BatteryInformation::default();
        let query = BatteryQueryInformation { tag, level: LEVEL_BATTERY_INFORMATION, at_rate: 0 };
        if !ioctl(battery, IOCTL_BATTERY_QUERY_INFORMATION, &query, &mut info) {
            return None;
        }
        let mut status = BatteryStatus::default();
        let have_status = ioctl(battery, IOCTL_BATTERY_QUERY_STATUS, &BatteryWaitStatus { tag, ..Default::default() }, &mut status);
        Some(Details {
            designed: known_or_zero(info.designed_capacity),
            full: known_or_zero(info.full_charged_capacity),
            cycles: known_or_zero(info.cycle_count),
            capacity: if have_status { known_or_zero(status.capacity) } else { 0 },
            rate: if have_status && status.rate != DRIVER_UNKNOWN_RATE { status.rate } else { 0 },
        })
    }
}

/// Um número do driver, com zero no lugar do "não sei".
fn known_or_zero(value: u32) -> u32 {
    if value == DRIVER_UNKNOWN {
        0
    } else {
        value
    }
}

// ---------- os dispositivos ----------

/// Que tipo de aparelho (o ícone dele).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevKind {
    Headphones,
    Speaker,
    Mouse,
    Keyboard,
    Gamepad,
    Phone,
    Pen,
    Other,
}

/// Um dispositivo com bateria (ou conectado agora, mesmo sem informar a bateria).
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub name: String,
    pub kind: DevKind,
    /// a bateria (0 a 100); None: o aparelho não informa
    pub level: Option<u8>,
    pub connected: bool,
}

/// O tipo pelo que o Windows diz do aparelho: a categoria do contêiner ("Input.Keyboard"), a
/// classe Bluetooth (o "Class of Device") ou, sem nada disso, o nome.
pub fn kind_of(category: &str, cod: Option<u32>, name: &str) -> DevKind {
    kind_by_category(category).or_else(|| cod.and_then(kind_by_class)).unwrap_or_else(|| kind_by_name(name))
}

/// Pela categoria do contêiner ("Input.Keyboard", "Audio.Headphone").
fn kind_by_category(category: &str) -> Option<DevKind> {
    let category = category.to_lowercase();
    let by_category = [
        ("keyboard", DevKind::Keyboard),
        ("mouse", DevKind::Mouse),
        ("gaming", DevKind::Gamepad),
        ("gamepad", DevKind::Gamepad),
        ("headphone", DevKind::Headphones),
        ("headset", DevKind::Headphones),
        ("speaker", DevKind::Speaker),
        ("phone", DevKind::Phone),
        ("pen", DevKind::Pen),
    ];
    by_category.iter().find(|(word, _)| category.contains(word)).map(|&(_, kind)| kind)
}

/// As classes maiores do "Class of Device" do Bluetooth (bits 8 a 12) que interessam.
const COD_MAJOR_PHONE: u32 = 0x02;
const COD_MAJOR_AUDIO: u32 = 0x04;
const COD_MAJOR_PERIPHERAL: u32 = 0x05;
/// A classe menor (bits 2 a 7) do alto-falante, entre os aparelhos de áudio.
const COD_MINOR_SPEAKER: u32 = 0x05;
/// Entre os periféricos, os 4 bits de baixo da classe menor: joystick e controle de jogo.
const COD_MINOR_JOYSTICK: u32 = 0x01;
const COD_MINOR_GAMEPAD: u32 = 0x02;

/// Pela classe Bluetooth (None: uma classe maior que não diz o tipo; um periférico que não é
/// teclado, mouse nem controle fica `Other`).
fn kind_by_class(cod: u32) -> Option<DevKind> {
    let (major, minor) = ((cod >> 8) & 0x1f, (cod >> 2) & 0x3f);
    match major {
        COD_MAJOR_AUDIO => Some(if minor == COD_MINOR_SPEAKER { DevKind::Speaker } else { DevKind::Headphones }),
        COD_MAJOR_PERIPHERAL => {
            let device_type = minor & 0x0f;
            if device_type == COD_MINOR_GAMEPAD || device_type == COD_MINOR_JOYSTICK {
                return Some(DevKind::Gamepad);
            }
            // os bits 4 e 5 da classe menor: teclado, apontador ou os dois
            Some(match (minor >> 4) & 0x3 {
                1 | 3 => DevKind::Keyboard,
                2 => DevKind::Mouse,
                _ => DevKind::Other,
            })
        }
        COD_MAJOR_PHONE => Some(DevKind::Phone),
        _ => None,
    }
}

/// Pelo nome do aparelho, o último recurso.
fn kind_by_name(name: &str) -> DevKind {
    let name = name.to_lowercase();
    let by_name = [
        ("mouse", DevKind::Mouse),
        ("keyboard", DevKind::Keyboard),
        ("teclado", DevKind::Keyboard),
        ("controller", DevKind::Gamepad),
        ("controle", DevKind::Gamepad),
        ("xbox", DevKind::Gamepad),
        ("dualsense", DevKind::Gamepad),
        ("buds", DevKind::Headphones),
        ("airpods", DevKind::Headphones),
        ("headphone", DevKind::Headphones),
        ("fone", DevKind::Headphones),
        ("pencil", DevKind::Pen),
        ("caneta", DevKind::Pen),
        ("speaker", DevKind::Speaker),
        ("caixa", DevKind::Speaker),
    ];
    by_name.iter().find(|(word, _)| name.contains(word)).map_or(DevKind::Other, |&(_, kind)| kind)
}

/// O nome do aparelho sem o nome do perfil que o Windows põe no fim ("JBL Wave Beam 2
/// Hands-Free AG" → "JBL Wave Beam 2").
pub fn clean_name(name: &str) -> String {
    let mut clean = name.trim().to_string();
    for suffix in [" Hands-Free AG", " Hands-Free", " Avrcp Transport", " Stereo", " LE"] {
        if let Some(stripped) = clean.strip_suffix(suffix) {
            clean = stripped.trim_end().to_string();
        }
    }
    clean
}

/// As propriedades do aparelho que interessam (devpkey.h e as de Bluetooth).
const KEY_BATTERY: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5), pid: 2 };
const KEY_NAME: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };
const KEY_CONTAINER: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x8c7ed206_3f8a_4827_b3ab_ae9e1faefc6c), pid: 2 };
const KEY_CATEGORY: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x78c34fc8_104a_4aca_9ea4_524d52996e57), pid: 90 };
const KEY_COD: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x2bd67d8b_8beb_48d5_87e0_6cda3428040a), pid: 10 };
const KEY_ADDRESS: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x2bd67d8b_8beb_48d5_87e0_6cda3428040a), pid: 1 };

/// Uma parte de aparelho (o Windows cria uma por perfil) que informa a bateria.
struct Part {
    name: String,
    level: u8,
    category: String,
    cod: Option<u32>,
    /// o endereço Bluetooth (para perguntar ao aparelho se está conectado)
    address: Option<u64>,
}

/// As partes de aparelho presentes que informam a bateria, por contêiner (o aparelho de
/// verdade, que junta as partes).
fn battery_parts() -> HashMap<GUID, Part> {
    let mut out: HashMap<GUID, Part> = HashMap::new();
    unsafe {
        let Ok(set) = SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_ALLCLASSES | DIGCF_PRESENT) else { return out };
        let mut index = 0;
        loop {
            let mut info = SP_DEVINFO_DATA { cbSize: size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() };
            if SetupDiEnumDeviceInfo(set, index, &mut info).is_err() {
                break;
            }
            index += 1;
            if let Some((container, part)) = battery_part(set, &info) {
                keep_part(&mut out, container, part);
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out
}

/// Uma parte de aparelho e o contêiner dela, se ela informa a bateria.
unsafe fn battery_part(set: HDEVINFO, info: &SP_DEVINFO_DATA) -> Option<(GUID, Part)> {
    let read = |key: &DEVPROPKEY| unsafe { read_property(set, info, key) };
    let level = read(&KEY_BATTERY).and_then(|bytes| bytes.first().copied())?;
    let container = read(&KEY_CONTAINER).filter(|bytes| bytes.len() == 16).map(|bytes| GUID::from_u128(u128::from_le_bytes(bytes.try_into().unwrap()).swap_guid()))?;
    let part = Part {
        name: read(&KEY_NAME).map(property_text).unwrap_or_default(),
        level: level.min(100),
        category: read(&KEY_CATEGORY).map(property_text).unwrap_or_default(),
        cod: read(&KEY_COD).filter(|bytes| bytes.len() >= 4).map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        address: read(&KEY_ADDRESS).map(property_text).and_then(|address| u64::from_str_radix(address.trim(), 16).ok()),
    };
    Some((container, part))
}

/// Junta a parte às do mesmo aparelho: de várias partes, fica a que tem categoria (a primeira
/// delas).
fn keep_part(parts: &mut HashMap<GUID, Part>, container: GUID, part: Part) {
    match parts.get(&container) {
        Some(old) if !old.category.is_empty() || part.category.is_empty() => {}
        _ => {
            parts.insert(container, part);
        }
    }
}

/// Os bytes de uma propriedade do aparelho (None: ele não tem essa propriedade).
unsafe fn read_property(set: HDEVINFO, info: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<Vec<u8>> {
    let mut property_type = DEVPROPTYPE(0);
    let mut need = 0u32;
    let mut buf = vec![0u8; 512];
    unsafe { SetupDiGetDevicePropertyW(set, info, key, &mut property_type, Some(&mut buf), Some(&mut need), 0).ok()? };
    buf.truncate(need as usize);
    Some(buf)
}

/// Uma propriedade de texto (UTF-16); numa lista de textos, o primeiro.
fn property_text(bytes: Vec<u8>) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    String::from_utf16_lossy(&units).split('\0').next().unwrap_or("").to_string()
}

/// O GUID guardado em bytes (os três primeiros campos em little-endian) no formato do `GUID`.
trait SwapGuid {
    fn swap_guid(self) -> u128;
}

impl SwapGuid for u128 {
    fn swap_guid(self) -> u128 {
        let b = self.to_le_bytes();
        let d1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let d2 = u16::from_le_bytes([b[4], b[5]]);
        let d3 = u16::from_le_bytes([b[6], b[7]]);
        let rest = u64::from_be_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]);
        ((d1 as u128) << 96) | ((d2 as u128) << 80) | ((d3 as u128) << 64) | rest as u128
    }
}

/// O aparelho Bluetooth no endereço `address` está conectado agora? (LE ou comum; None: não
/// deu para saber.)
fn connected_at(address: u64) -> Option<bool> {
    use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
    if let Some(device) = BluetoothLEDevice::FromBluetoothAddressAsync(address).ok().and_then(|op| op.join().ok()) {
        return device.ConnectionStatus().ok().map(|status| status == BluetoothConnectionStatus::Connected);
    }
    let device = BluetoothDevice::FromBluetoothAddressAsync(address).ok()?.join().ok()?;
    device.ConnectionStatus().ok().map(|status| status == BluetoothConnectionStatus::Connected)
}

/// Os dispositivos que informam a bateria (conectados ou não; desconectados, com a última
/// carga), cada um perguntado pelo endereço se está conectado agora. Conectados primeiro, e entre
/// eles a carga mais baixa primeiro (o que vai acabar antes aparece antes).
#[cfg(test)]
pub fn devices() -> Vec<Device> {
    devices_from(battery_parts(), &Watched::default())
}

/// A lista a partir das partes; conectado ou não pelos aparelhos já vigiados (sem perguntar de
/// novo) ou, os outros, perguntando pelo endereço.
fn devices_from(parts: HashMap<GUID, Part>, watched: &Watched) -> Vec<Device> {
    let mut out: Vec<Device> = parts
        .into_values()
        .filter(|part| !part.name.is_empty())
        .map(|part| {
            let name = clean_name(&part.name);
            let connected = part.address.and_then(|address| watched.connected(address).or_else(|| connected_at(address))).unwrap_or(true);
            Device { kind: kind_of(&part.category, part.cod, &name), name, level: Some(part.level), connected }
        })
        .collect();
    sort(&mut out);
    out
}

/// De quanto em quanto tempo ler os dispositivos de novo (a carga deles muda devagar; conectar e
/// desconectar chegam na hora, pelos avisos).
pub const DEVICES_EVERY: Duration = Duration::from_secs(30);
/// Depois de um aviso, espera os outros do mesmo momento (um fone avisa pelo perfil de áudio e
/// pelo de chamada) e lê; lê de novo depois, porque a carga de quem acabou de conectar chega uns
/// segundos depois da conexão.
const SETTLE: Duration = Duration::from_millis(250);
const AFTER_EVENT: [Duration; 3] = [Duration::ZERO, Duration::from_secs(4), Duration::from_secs(12)];

/// Os aparelhos Bluetooth vigiados (pelo endereço): o objeto do Windows de cada um e o registro
/// do aviso de conexão.
#[derive(Default)]
struct Watched {
    by_address: HashMap<u64, Vec<Watch>>,
}

/// Um aparelho vigiado, pelo lado LE ou pelo comum do Bluetooth, e o registro do aviso (para
/// tirar o aviso no fim).
enum Watch {
    Le(windows::Devices::Bluetooth::BluetoothLEDevice, i64),
    Classic(windows::Devices::Bluetooth::BluetoothDevice, i64),
}

/// Um aviso do Windows que só manda um sinal em `wake` (o fio lê tudo de novo).
fn wake_on_change<T: windows::core::RuntimeType + 'static>(wake: &Sender<()>) -> windows::Foundation::TypedEventHandler<T, windows::core::IInspectable> {
    let wake = wake.clone();
    windows::Foundation::TypedEventHandler::new(move |_, _| {
        let _ = wake.send(());
        Ok(())
    })
}

impl Watched {
    /// Passa a vigiar os endereços novos: cada conexão ou desconexão manda um aviso em `wake`.
    fn add(&mut self, parts: &HashMap<GUID, Part>, wake: &Sender<()>) {
        use windows::Devices::Bluetooth::{BluetoothDevice, BluetoothLEDevice};
        for address in parts.values().filter_map(|part| part.address) {
            if self.by_address.contains_key(&address) {
                continue;
            }
            let mut list = Vec::new();
            if let Some(device) = BluetoothLEDevice::FromBluetoothAddressAsync(address).ok().and_then(|op| op.join().ok()) {
                if let Ok(token) = device.ConnectionStatusChanged(&wake_on_change(wake)) {
                    list.push(Watch::Le(device, token));
                }
            }
            if let Some(device) = BluetoothDevice::FromBluetoothAddressAsync(address).ok().and_then(|op| op.join().ok()) {
                if let Ok(token) = device.ConnectionStatusChanged(&wake_on_change(wake)) {
                    list.push(Watch::Classic(device, token));
                }
            }
            // (sem nenhum dos dois: fica a lista vazia, para não tentar de novo a cada leitura)
            self.by_address.insert(address, list);
        }
    }

    /// Conectado agora, pelo objeto vigiado (None: não vigiado).
    fn connected(&self, address: u64) -> Option<bool> {
        use windows::Devices::Bluetooth::BluetoothConnectionStatus;
        let list = self.by_address.get(&address).filter(|list| !list.is_empty())?;
        let states: Vec<bool> = list
            .iter()
            .filter_map(|watch| match watch {
                Watch::Le(device, _) => device.ConnectionStatus().ok(),
                Watch::Classic(device, _) => device.ConnectionStatus().ok(),
            })
            .map(|status| status == BluetoothConnectionStatus::Connected)
            .collect();
        // (um fone que fala os dois: conectado por qualquer um)
        (!states.is_empty()).then(|| states.contains(&true))
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        for watch in self.by_address.values().flatten() {
            let _ = match watch {
                Watch::Le(device, token) => device.RemoveConnectionStatusChanged(*token),
                Watch::Classic(device, token) => device.RemoveConnectionStatusChanged(*token),
            };
        }
    }
}

/// Lê os dispositivos num fio à parte, agora, a cada `DEVICES_EVERY` e logo depois de um aparelho
/// conectar ou desconectar: cada lista chega no canal. O fio acaba quando o canal é largado.
pub fn watch_devices() -> Option<Receiver<Vec<Device>>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new().name("bateria-dispositivos".into()).spawn(move || device_loop(tx)).ok()?;
    Some(rx)
}

/// O fio dos dispositivos: lê, manda a lista e dorme até a próxima leitura (a de sempre ou uma
/// das que vêm depois de um aviso de conexão).
fn device_loop(tx: Sender<Vec<Device>>) {
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
    }
    let (wake_tx, wake_rx) = mpsc::channel::<()>();
    let mut watched = Watched::default();
    let mut next_read = Instant::now();
    // as leituras marcadas depois de um aviso (`AFTER_EVENT`)
    let mut after_event: Vec<Instant> = Vec::new();
    loop {
        let now = Instant::now();
        if now >= next_read || after_event.iter().any(|&at| now >= at) {
            after_event.retain(|&at| at > now);
            let parts = battery_parts();
            watched.add(&parts, &wake_tx);
            if tx.send(devices_from(parts, &watched)).is_err() {
                break;
            }
            next_read = Instant::now() + DEVICES_EVERY;
        }
        let until = after_event.iter().copied().fold(next_read, Instant::min);
        if wake_rx.recv_timeout(until.saturating_duration_since(Instant::now())).is_ok() {
            std::thread::sleep(SETTLE);
            while wake_rx.try_recv().is_ok() {}
            let woke_at = Instant::now();
            after_event = AFTER_EVENT.iter().map(|delay| woke_at + *delay).collect();
        }
    }
}

/// Conectados primeiro; entre eles, a carga mais baixa primeiro (sem carga, no fim).
pub fn sort(list: &mut [Device]) {
    list.sort_by_key(|device| (!device.connected, device.level.map_or(101, |level| level as i32), device.name.to_lowercase()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_come_from_the_category_the_bluetooth_class_or_the_name() {
        assert_eq!(kind_of("Input.Keyboard", None, "KV790-BT1"), DevKind::Keyboard);
        assert_eq!(kind_of("Input.Mouse", None, "x"), DevKind::Mouse);
        // a classe de um fone de ouvido (0x240404: áudio, fone)
        assert_eq!(kind_of("", Some(0x240404), "JBL Wave Beam 2"), DevKind::Headphones);
        assert_eq!(kind_of("", Some(0x240414), "Caixinha"), DevKind::Speaker);
        // periféricos: teclado (0x540), mouse (0x580), controle (0x508)
        assert_eq!(kind_of("", Some(0x002540), "?"), DevKind::Keyboard);
        assert_eq!(kind_of("", Some(0x002580), "?"), DevKind::Mouse);
        assert_eq!(kind_of("", Some(0x002508), "?"), DevKind::Gamepad);
        assert_eq!(kind_of("", Some(0x5a020c), "?"), DevKind::Phone);
        // sem nada, pelo nome
        assert_eq!(kind_of("", None, "Xbox Wireless Controller"), DevKind::Gamepad);
        assert_eq!(kind_of("", None, "MX Master Mouse"), DevKind::Mouse);
        assert_eq!(kind_of("", None, "KN330"), DevKind::Other);
    }

    /// Um periférico que não é teclado, mouse nem controle fica "outro" (não vai para o nome); uma
    /// classe maior que não diz o tipo (computador, 0x01) vai para o nome.
    #[test]
    fn the_bluetooth_class_decides_before_the_name() {
        assert_eq!(kind_of("", Some(0x000500), "Mouse"), DevKind::Other);
        assert_eq!(kind_of("", Some(0x000100), "Mouse"), DevKind::Mouse);
        assert_eq!(kind_of("Audio.Headphone", Some(0x002540), "Mouse"), DevKind::Headphones);
    }

    #[test]
    fn names_lose_the_profile_suffix() {
        assert_eq!(clean_name("JBL Wave Beam 2 Hands-Free AG"), "JBL Wave Beam 2");
        assert_eq!(clean_name("WH-1000XM5 Hands-Free"), "WH-1000XM5");
        assert_eq!(clean_name("KV790-BT1"), "KV790-BT1");
    }

    #[test]
    fn connected_and_low_come_first() {
        let device = |name: &str, level: Option<u8>, connected| Device { name: name.into(), kind: DevKind::Other, level, connected };
        let mut list = vec![device("a", Some(80), true), device("b", Some(10), false), device("c", Some(12), true), device("d", None, true)];
        sort(&mut list);
        let names: Vec<_> = list.iter().map(|device| device.name.as_str()).collect();
        assert_eq!(names, ["c", "a", "d", "b"]);
    }

    fn status(ac: u8, flag: u8, percent: u8, lifetime: u32) -> SYSTEM_POWER_STATUS {
        SYSTEM_POWER_STATUS { ACLineStatus: ac, BatteryFlag: flag, BatteryLifePercent: percent, BatteryLifeTime: lifetime, ..Default::default() }
    }

    /// O que o Windows diz: sem bateria ou sem saber, nada; na tomada, carregando pelo bit; o
    /// tempo que falta só fora da tomada.
    #[test]
    fn the_windows_status_becomes_the_battery() {
        assert_eq!(windows_power(&status(AC_ONLINE, FLAG_NO_BATTERY, 100, LIFETIME_UNKNOWN)), None);
        assert_eq!(windows_power(&status(AC_ONLINE, FLAG_UNKNOWN, 50, LIFETIME_UNKNOWN)), None);
        assert_eq!(windows_power(&status(AC_ONLINE, 0, 101, LIFETIME_UNKNOWN)), None);
        let charging = windows_power(&status(AC_ONLINE, FLAG_CHARGING, 31, 5000)).unwrap();
        assert_eq!((charging.percent, charging.plugged, charging.charging, charging.secs_left), (31.0, true, true, None));
        let full = windows_power(&status(AC_ONLINE, 0, 100, LIFETIME_UNKNOWN)).unwrap();
        assert_eq!((full.plugged, full.charging), (true, false));
        let on_battery = windows_power(&status(AC_OFFLINE, FLAG_CHARGING, 64, 7200)).unwrap();
        assert_eq!((on_battery.plugged, on_battery.charging, on_battery.secs_left), (false, false, Some(7200)));
        assert_eq!(windows_power(&status(AC_OFFLINE, 0, 64, LIFETIME_UNKNOWN)).unwrap().secs_left, None);
    }

    /// O que só o driver sabe: a saúde (no máximo 1), o consumo fora da tomada e quanto falta para
    /// encher carregando; zeros do driver ficam de fora.
    #[test]
    fn the_driver_adds_health_drain_and_time_to_full() {
        let charging = windows_power(&status(AC_ONLINE, FLAG_CHARGING, 31, LIFETIME_UNKNOWN)).unwrap();
        let mut power = charging.clone();
        add_driver_details(&mut power, &Details { designed: 41006, full: 41006, cycles: 0, capacity: 12859, rate: 19099 });
        assert_eq!(power.health, Some(1.0));
        assert_eq!(power.cycles, None);
        assert_eq!(power.left_mwh, Some(12859));
        assert_eq!(power.drain_mw, None);
        assert_eq!(power.secs_to_full, Some(5305));

        let mut worn = charging.clone();
        add_driver_details(&mut worn, &Details { designed: 50000, full: 40000, cycles: 312, capacity: 40000, rate: 1000 });
        assert_eq!((worn.health, worn.cycles, worn.secs_to_full), (Some(0.8), Some(312), None));

        let mut on_battery = windows_power(&status(AC_OFFLINE, 0, 64, 7200)).unwrap();
        add_driver_details(&mut on_battery, &Details { designed: 0, full: 30000, cycles: 0, capacity: 0, rate: -8500 });
        assert_eq!((on_battery.health, on_battery.left_mwh, on_battery.drain_mw, on_battery.secs_to_full), (None, None, Some(8500), None));
    }

    /// O fio dos dispositivos manda a primeira lista sozinho e acaba quando o widget larga o canal.
    #[test]
    fn the_watcher_sends_a_list_right_away() {
        let rx = watch_devices().expect("o fio");
        assert!(rx.recv_timeout(Duration::from_secs(20)).is_ok());
        drop(rx);
    }

    #[test]
    fn guids_read_from_bytes_match_windows() {
        // {95EA12ED-3DF5-5313-A8A2-E974D4BB3334}, como fica na memória
        let bytes: [u8; 16] = [0xED, 0x12, 0xEA, 0x95, 0xF5, 0x3D, 0x13, 0x53, 0xA8, 0xA2, 0xE9, 0x74, 0xD4, 0xBB, 0x33, 0x34];
        let guid = GUID::from_u128(u128::from_le_bytes(bytes).swap_guid());
        assert_eq!(guid, GUID::from_u128(0x95ea12ed_3df5_5313_a8a2_e974d4bb3334));
    }

    #[test]
    fn device_properties_read_as_text_and_parts_keep_the_one_with_a_category() {
        let utf16 = |text: &str| text.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
        assert_eq!(property_text(utf16("Audio.Headphone\0Audio\0\0")), "Audio.Headphone");
        assert_eq!(property_text(Vec::new()), "");

        let part = |name: &str, category: &str| Part { name: name.into(), level: 50, category: category.into(), cod: None, address: None };
        let container = GUID::from_u128(1);
        let mut parts = HashMap::new();
        keep_part(&mut parts, container, part("sem categoria", ""));
        keep_part(&mut parts, container, part("com categoria", "Audio.Headphone"));
        keep_part(&mut parts, container, part("outra com categoria", "Audio.Headset"));
        keep_part(&mut parts, container, part("outra sem", ""));
        assert_eq!(parts[&container].name, "com categoria");
    }

    /// Um minuto lendo a bateria deste PC a cada segundo, mostrando cada leitura (para ver se o
    /// Windows ou o driver devolvem valores estranhos de vez em quando).
    ///
    ///   cargo test --release battery::tests::watch_a_minute -- --ignored --nocapture
    #[test]
    #[ignore]
    fn watch_a_minute() {
        for second in 0..60 {
            let power = power();
            println!("{second:2} {:?}", power.map(|power| (power.percent, power.plugged, power.charging, power.secs_left, power.left_mwh, power.drain_mw)));
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    /// Os dispositivos e a bateria deste PC (para conferir na mão):
    ///
    ///   cargo test --release battery::tests::this_pc -- --ignored --nocapture
    #[test]
    #[ignore]
    fn this_pc() {
        let started = std::time::Instant::now();
        println!("{:?} ({:?})", power(), started.elapsed());
        if let Some(details) = driver_details() {
            println!("driver: projetada {} mWh, cheia {} mWh, agora {} mWh, ritmo {} mW, ciclos {}", details.designed, details.full, details.capacity, details.rate, details.cycles);
        }
        let started = std::time::Instant::now();
        for device in devices() {
            println!("{device:?}");
        }
        println!("dispositivos: {:?}", started.elapsed());
    }
}
