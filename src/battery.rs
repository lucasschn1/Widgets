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
//! Ler os dispositivos leva uns 100 ms e chama o WinRT: é feito num fio à parte.

use std::collections::HashMap;

use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW, SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA,
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
    pub cycles: Option<u32>,
    /// a energia que resta, em mWh (do driver)
    pub left_mwh: Option<u32>,
    /// o consumo agora, na bateria, em mW (do driver; varia muito de um segundo para outro)
    pub drain_mw: Option<u32>,
}

/// A bateria do notebook (None: o PC não tem bateria).
pub fn power() -> Option<Power> {
    let mut st = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut st).ok()? };
    // 128: sem bateria; 255: não sabe
    if st.BatteryFlag & 128 != 0 || st.BatteryFlag == 255 || st.BatteryLifePercent > 100 {
        return None;
    }
    let plugged = st.ACLineStatus == 1;
    let mut p = Power {
        percent: st.BatteryLifePercent as f32,
        plugged,
        charging: plugged && st.BatteryFlag & 8 != 0,
        secs_left: (!plugged && st.BatteryLifeTime != u32::MAX).then_some(st.BatteryLifeTime),
        ..Default::default()
    };
    if let Some(d) = driver_details() {
        if d.designed > 0 && d.full > 0 {
            p.health = Some((d.full as f32 / d.designed as f32).min(1.0));
        }
        p.cycles = (d.cycles > 0).then_some(d.cycles);
        p.left_mwh = (d.capacity > 0).then_some(d.capacity);
        if !plugged && d.rate < 0 {
            p.drain_mw = Some(d.rate.unsigned_abs());
        }
        if p.charging && d.rate > 0 && d.full > d.capacity {
            p.secs_to_full = Some(((d.full - d.capacity) as f64 / d.rate as f64 * 3600.0) as u32);
        }
    }
    Some(p)
}

/// O que o driver da bateria informa: capacidades em mWh, ciclos e o ritmo de agora em mW
/// (positivo carregando).
struct Details {
    designed: u32,
    full: u32,
    cycles: u32,
    capacity: u32,
    rate: i32,
}

/// A interface das baterias (GUID_DEVICE_BATTERY) e os pedidos ao driver (batclass.h).
const GUID_DEVICE_BATTERY: GUID = GUID::from_u128(0x72631e54_78a4_11d0_bcf7_00aa00b7b32a);
const IOCTL_BATTERY_QUERY_TAG: u32 = 0x294040;
const IOCTL_BATTERY_QUERY_INFORMATION: u32 = 0x294044;
const IOCTL_BATTERY_QUERY_STATUS: u32 = 0x29404c;

#[repr(C)]
#[derive(Default)]
struct BatteryQueryInformation {
    tag: u32,
    level: i32, // 0: BatteryInformation
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
unsafe fn ioctl<I, O>(h: HANDLE, code: u32, input: &I, output: &mut O) -> bool {
    let mut got = 0u32;
    unsafe { DeviceIoControl(h, code, Some(input as *const I as *const _), size_of::<I>() as u32, Some(output as *mut O as *mut _), size_of::<O>() as u32, Some(&mut got), None).is_ok() }
}

/// Lê o driver da primeira bateria.
fn driver_details() -> Option<Details> {
    unsafe {
        let set = SetupDiGetClassDevsW(Some(&GUID_DEVICE_BATTERY), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE).ok()?;
        let mut iface = SP_DEVICE_INTERFACE_DATA { cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
        let found = SetupDiEnumDeviceInterfaces(set, None, &GUID_DEVICE_BATTERY, 0, &mut iface).is_ok();
        let mut path: Option<Vec<u16>> = None;
        if found {
            let mut need = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(set, &iface, None, 0, Some(&mut need), None);
            if need as usize > size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() {
                // o tamanho do cabeçalho e o caminho logo depois (em u32, para ficar alinhado)
                let mut buf = vec![0u32; (need as usize).div_ceil(4)];
                let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
                (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
                if SetupDiGetDeviceInterfaceDetailW(set, &iface, Some(detail), need, None, None).is_ok() {
                    let start = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
                    let len = (0..).take_while(|&i| *start.add(i) != 0).count();
                    let mut p = std::slice::from_raw_parts(start, len).to_vec();
                    p.push(0);
                    path = Some(p);
                }
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
        let path = path?;
        let h = CreateFileW(PCWSTR(path.as_ptr()), (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None).ok()?;
        let mut tag = 0u32;
        let wait = 0u32;
        let mut out = None;
        if ioctl(h, IOCTL_BATTERY_QUERY_TAG, &wait, &mut tag) && tag != 0 {
            let mut info = BatteryInformation::default();
            let mut status = BatteryStatus::default();
            let q = BatteryQueryInformation { tag, level: 0, at_rate: 0 };
            let w = BatteryWaitStatus { tag, ..Default::default() };
            if ioctl(h, IOCTL_BATTERY_QUERY_INFORMATION, &q, &mut info) {
                let have_status = ioctl(h, IOCTL_BATTERY_QUERY_STATUS, &w, &mut status);
                // 0xFFFFFFFF e 0x80000000: valores que o driver não sabe
                let known = |v: u32| if v == u32::MAX { 0 } else { v };
                out = Some(Details {
                    designed: known(info.designed_capacity),
                    full: known(info.full_charged_capacity),
                    cycles: known(info.cycle_count),
                    capacity: if have_status { known(status.capacity) } else { 0 },
                    rate: if have_status && status.rate != i32::MIN { status.rate } else { 0 },
                });
            }
        }
        let _ = CloseHandle(h);
        out
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
    let c = category.to_lowercase();
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
    if let Some(&(_, k)) = by_category.iter().find(|(w, _)| c.contains(w)) {
        return k;
    }
    if let Some(cod) = cod {
        let (major, minor) = ((cod >> 8) & 0x1f, (cod >> 2) & 0x3f);
        match major {
            // áudio: alto-falante (5) ou fone (o resto)
            0x04 => return if minor == 0x05 { DevKind::Speaker } else { DevKind::Headphones },
            // periféricos: teclado, apontador ou os dois (bits 6 e 7) e o controle de jogo
            0x05 => {
                if minor & 0x0f == 0x02 || minor & 0x0f == 0x01 {
                    return DevKind::Gamepad;
                }
                return match (minor >> 4) & 0x3 {
                    1 | 3 => DevKind::Keyboard,
                    2 => DevKind::Mouse,
                    _ => DevKind::Other,
                };
            }
            0x02 => return DevKind::Phone,
            _ => {}
        }
    }
    let n = name.to_lowercase();
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
    by_name.iter().find(|(w, _)| n.contains(w)).map_or(DevKind::Other, |&(_, k)| k)
}

/// O nome do aparelho sem o nome do perfil que o Windows põe no fim ("JBL Wave Beam 2
/// Hands-Free AG" → "JBL Wave Beam 2").
pub fn clean_name(name: &str) -> String {
    let mut n = name.trim().to_string();
    for suffix in [" Hands-Free AG", " Hands-Free", " Avrcp Transport", " Stereo", " LE"] {
        if let Some(stripped) = n.strip_suffix(suffix) {
            n = stripped.trim_end().to_string();
        }
    }
    n
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

/// As partes de aparelho presentes que informam a bateria, por contêiner.
fn battery_parts() -> HashMap<GUID, Part> {
    let mut out: HashMap<GUID, Part> = HashMap::new();
    unsafe {
        let Ok(set) = SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_ALLCLASSES | DIGCF_PRESENT) else { return out };
        let read = |info: &SP_DEVINFO_DATA, key: &DEVPROPKEY| -> Option<Vec<u8>> {
            let mut ty = DEVPROPTYPE(0);
            let mut need = 0u32;
            let mut buf = vec![0u8; 512];
            SetupDiGetDevicePropertyW(set, info, key, &mut ty, Some(&mut buf), Some(&mut need), 0).ok()?;
            buf.truncate(need as usize);
            Some(buf)
        };
        let text = |b: Vec<u8>| {
            let w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            // (listas de textos: o primeiro)
            String::from_utf16_lossy(&w).split('\0').next().unwrap_or("").to_string()
        };
        let mut i = 0;
        loop {
            let mut info = SP_DEVINFO_DATA { cbSize: size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() };
            if SetupDiEnumDeviceInfo(set, i, &mut info).is_err() {
                break;
            }
            i += 1;
            let Some(level) = read(&info, &KEY_BATTERY).and_then(|b| b.first().copied()) else { continue };
            let Some(container) = read(&info, &KEY_CONTAINER).filter(|b| b.len() == 16).map(|b| GUID::from_u128(u128::from_le_bytes(b.try_into().unwrap()).swap_guid())) else { continue };
            let part = Part {
                name: read(&info, &KEY_NAME).map(text).unwrap_or_default(),
                level: level.min(100),
                category: read(&info, &KEY_CATEGORY).map(text).unwrap_or_default(),
                cod: read(&info, &KEY_COD).filter(|b| b.len() >= 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                address: read(&info, &KEY_ADDRESS).map(text).and_then(|a| u64::from_str_radix(a.trim(), 16).ok()),
            };
            // (um aparelho com várias partes: fica a que tem categoria)
            match out.get(&container) {
                Some(old) if !old.category.is_empty() || part.category.is_empty() => {}
                _ => {
                    out.insert(container, part);
                }
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out
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
    if let Some(d) = BluetoothLEDevice::FromBluetoothAddressAsync(address).ok().and_then(|op| op.join().ok()) {
        return d.ConnectionStatus().ok().map(|s| s == BluetoothConnectionStatus::Connected);
    }
    let d = BluetoothDevice::FromBluetoothAddressAsync(address).ok()?.join().ok()?;
    d.ConnectionStatus().ok().map(|s| s == BluetoothConnectionStatus::Connected)
}

/// Os dispositivos que informam a bateria (conectados ou não; desconectados, com a última
/// carga), cada um perguntado pelo endereço se está conectado agora. Conectados primeiro, e entre
/// eles a carga mais baixa primeiro (o que vai acabar antes aparece antes).
pub fn devices() -> Vec<Device> {
    let mut out: Vec<Device> = battery_parts()
        .into_values()
        .filter(|p| !p.name.is_empty())
        .map(|p| {
            let name = clean_name(&p.name);
            let connected = p.address.and_then(connected_at).unwrap_or(true);
            Device { kind: kind_of(&p.category, p.cod, &name), name, level: Some(p.level), connected }
        })
        .collect();
    sort(&mut out);
    out
}

/// Conectados primeiro; entre eles, a carga mais baixa primeiro (sem carga, no fim).
pub fn sort(list: &mut [Device]) {
    list.sort_by_key(|d| (!d.connected, d.level.map_or(101, |l| l as i32), d.name.to_lowercase()));
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

    #[test]
    fn names_lose_the_profile_suffix() {
        assert_eq!(clean_name("JBL Wave Beam 2 Hands-Free AG"), "JBL Wave Beam 2");
        assert_eq!(clean_name("WH-1000XM5 Hands-Free"), "WH-1000XM5");
        assert_eq!(clean_name("KV790-BT1"), "KV790-BT1");
    }

    #[test]
    fn connected_and_low_come_first() {
        let d = |name: &str, level: Option<u8>, connected| Device { name: name.into(), kind: DevKind::Other, level, connected };
        let mut list = vec![d("a", Some(80), true), d("b", Some(10), false), d("c", Some(12), true), d("d", None, true)];
        sort(&mut list);
        let names: Vec<_> = list.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["c", "a", "d", "b"]);
    }

    #[test]
    fn guids_read_from_bytes_match_windows() {
        // {95EA12ED-3DF5-5313-A8A2-E974D4BB3334}, como fica na memória
        let bytes: [u8; 16] = [0xED, 0x12, 0xEA, 0x95, 0xF5, 0x3D, 0x13, 0x53, 0xA8, 0xA2, 0xE9, 0x74, 0xD4, 0xBB, 0x33, 0x34];
        let g = GUID::from_u128(u128::from_le_bytes(bytes).swap_guid());
        assert_eq!(g, GUID::from_u128(0x95ea12ed_3df5_5313_a8a2_e974d4bb3334));
    }

    /// Os dispositivos e a bateria deste PC (para conferir na mão):
    ///   cargo test --release battery::tests::this_pc -- --ignored --nocapture
    /// Um minuto lendo a bateria deste PC a cada segundo, mostrando cada leitura (para ver se o
    /// Windows ou o driver devolvem valores estranhos de vez em quando).
    ///
    ///   cargo test --release battery::tests::watch_a_minute -- --ignored --nocapture
    #[test]
    #[ignore]
    fn watch_a_minute() {
        for i in 0..60 {
            let p = power();
            println!("{i:2} {:?}", p.map(|p| (p.percent, p.plugged, p.charging, p.secs_left, p.left_mwh, p.drain_mw)));
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    #[test]
    #[ignore]
    fn this_pc() {
        let t = std::time::Instant::now();
        println!("{:?} ({:?})", power(), t.elapsed());
        if let Some(d) = driver_details() {
            println!("driver: projetada {} mWh, cheia {} mWh, agora {} mWh, ritmo {} mW, ciclos {}", d.designed, d.full, d.capacity, d.rate, d.cycles);
        }
        let t = std::time::Instant::now();
        for d in devices() {
            println!("{d:?}");
        }
        println!("dispositivos: {:?}", t.elapsed());
    }
}
