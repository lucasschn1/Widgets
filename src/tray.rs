//! Ícone na bandeja do sistema, notificações do Windows e atalhos globais de teclado.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::events::WM_TRAY;
use crate::gfx::{wide, Canvas, Fill, Image};
use crate::theme::Rgba;

pub const TIP: &str = "Widgets da área de trabalho";
pub const NIN_BALLOONUSERCLICK: u32 = 0x405;

/// Ícone: um quadrado com degradê e três "widgets" brancos.
pub fn icon_image(px: u32) -> Image {
    let s = px as f32 / 16.0;
    let mut c = Canvas::new(px, px, s);
    c.fill_round_rect_with(0.0, 0.0, 16.0, 16.0, 3.6, Fill::VGrad(0.0, 16.0, Rgba::rgb(10, 132, 255), Rgba::rgb(191, 90, 242)));
    for (x0, y0, x1, y1) in [(3.0, 3.0, 13.0, 7.0), (3.0, 9.0, 7.4, 13.0), (8.6, 9.0, 13.0, 13.0)] {
        c.fill_round_rect(x0, y0, x1, y1, 1.2, Rgba(255, 255, 255, 240));
    }
    c.finish()
}

/// HICON a partir de uma imagem (com alfa).
pub fn hicon(img: &Image) -> HICON {
    unsafe {
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: img.w as i32,
            biHeight: -(img.h as i32),
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
        // ícones usam alfa "reto": desfaz a pré-multiplicação
        let out = std::slice::from_raw_parts_mut(bits as *mut u8, img.px.len());
        for (d, p) in out.chunks_exact_mut(4).zip(img.px.chunks_exact(4)) {
            let a = p[3] as u32;
            for k in 0..3 {
                d[k] = if a == 0 { 0 } else { ((p[k] as u32 * 255 + a / 2) / a).min(255) as u8 };
            }
            d[3] = p[3];
        }
        let mask = CreateBitmap(img.w as i32, img.h as i32, 1, 1, None);
        let ii = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&ii).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

pub struct Tray {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
    pub ok: bool,
}

fn copy_into(dst: &mut [u16], text: &str) {
    let w: Vec<u16> = text.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

impl Tray {
    pub fn new(hwnd: HWND) -> Tray {
        let px = unsafe { GetSystemMetrics(SM_CXSMICON) } as u32;
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: hicon(&icon_image(px)),
            ..Default::default()
        };
        copy_into(&mut nid.szTip, TIP);
        let mut tray = Tray { hwnd, nid, ok: false };
        tray.add();
        tray
    }

    /// Põe o ícone na bandeja (de novo, se o Explorer reiniciou).
    pub fn add(&mut self) {
        self.nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        self.ok = unsafe { Shell_NotifyIconW(NIM_ADD, &self.nid).as_bool() };
    }

    pub fn set_tip(&mut self, text: &str) {
        self.nid.uFlags = NIF_TIP;
        copy_into(&mut self.nid.szTip, text);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
        }
    }

    /// Notificação do Windows saindo do ícone da bandeja.
    pub fn notify(&mut self, title: &str, text: &str) {
        self.nid.uFlags = NIF_INFO;
        copy_into(&mut self.nid.szInfoTitle, title);
        copy_into(&mut self.nid.szInfo, text);
        self.nid.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
        }
    }

    pub fn remove(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
        }
    }

    pub fn window(&self) -> HWND {
        self.hwnd
    }
}

// ---------- atalhos globais ----------

/// alternativas para "mostrar na frente", se o atalho escolhido já for de outro programa
pub const FALLBACKS: [&str; 4] = ["Ctrl+Alt+W", "Win+Alt+W", "Ctrl+Shift+W", "Ctrl+Alt+Espaço"];
pub const HOTKEY_PEEK: i32 = 1;
pub const HOTKEY_POMODORO: i32 = 2;

const MODIFIERS: [(&str, u32); 4] = [("Ctrl", 0x2), ("Alt", 0x1), ("Shift", 0x4), ("Win", 0x8)];
const KEY_NAMES: [(u32, &str); 25] = [
    (0x20, "Espaço"),
    (0x0D, "Enter"),
    (0x09, "Tab"),
    (0x25, "Esquerda"),
    (0x26, "Cima"),
    (0x27, "Direita"),
    (0x28, "Baixo"),
    (0x2D, "Insert"),
    (0x2E, "Delete"),
    (0x24, "Home"),
    (0x23, "End"),
    (0x21, "PageUp"),
    (0x22, "PageDown"),
    (0xBC, ","),
    (0xBE, "."),
    (0xBD, "-"),
    (0xBB, "="),
    (0xC0, "'"),
    (0xDE, "~"),
    (0xBA, "Ç"),
    (0xBF, ";"),
    (0xDB, "´"),
    (0xDD, "["),
    (0xDC, "]"),
    (0xE2, "\\"),
];

pub fn key_name(vk: u32) -> Option<String> {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => Some(char::from_u32(vk).unwrap().to_string()),
        0x70..=0x87 => Some(format!("F{}", vk - 0x6F)),
        0x60..=0x69 => Some(format!("Num{}", vk - 0x60)),
        _ => KEY_NAMES.iter().find(|(v, _)| *v == vk).map(|(_, n)| n.to_string()),
    }
}

/// "Ctrl+Alt+W" -> (modificadores, tecla virtual).
pub fn parse_hotkey(label: &str) -> Option<(u32, u32)> {
    if label.is_empty() {
        return None;
    }
    let parts: Vec<&str> = label.split('+').collect();
    let (key, mods) = parts.split_last()?;
    let mut flags = 0;
    for m in mods {
        flags |= MODIFIERS.iter().find(|(n, _)| n == m)?.1;
    }
    let vk = if key.chars().count() == 1 && key.is_ascii() && key.chars().all(|c| c.is_ascii_alphanumeric()) {
        key.to_ascii_uppercase().chars().next()? as u32
    } else if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        0x6F + n
    } else if let Some(n) = key.strip_prefix("Num").and_then(|n| n.parse::<u32>().ok()) {
        0x60 + n
    } else {
        KEY_NAMES.iter().find(|(_, n)| n == key)?.0
    };
    Some((flags, vk))
}

pub fn register_hotkey(hwnd: HWND, id: i32, label: &str) -> bool {
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), id);
    }
    match parse_hotkey(label) {
        Some((mods, vk)) => unsafe { RegisterHotKey(Some(hwnd), id, HOT_KEY_MODIFIERS(mods | 0x4000), vk).is_ok() },
        None => false,
    }
}
