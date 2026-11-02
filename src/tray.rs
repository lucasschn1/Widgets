//! Ícone na bandeja do sistema e notificações do Windows.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::drawing;
use crate::events::WM_TRAY;
use crate::gfx::{wide, Canvas, Fill, Image};
use crate::theme::{Rgba, WHITE};
use crate::widgets::system;

pub const TIP: &str = "Widgets da área de trabalho";
pub const NIN_BALLOONUSERCLICK: u32 = 0x405;

/// Ícone do app: os anéis do widget Sistema (CPU, memória e disco, desenhados pela mesma função
/// do widget) sobre um quadrado preto, num quadro de 16 × 16 ampliado para `px`.
pub fn icon_image(px: u32) -> Image {
    let s = px as f32 / 16.0;
    let mut c = Canvas::new(px, px, s);
    let r = 3.6;
    c.fill_round_rect_with(0.0, 0.0, 16.0, 16.0, r, Fill::VGrad(0.0, 16.0, Rgba::rgb(30, 30, 36), Rgba::rgb(6, 6, 9)));
    // um reflexo leve no alto e um fio claro na borda, como o vidro dos widgets
    c.fill_round_rect_with(0.0, 0.0, 16.0, 16.0, r, Fill::VGrad(0.0, 8.0, WHITE.alpha(0.07), WHITE.alpha(0.0)));
    c.stroke_round_rect(0.0, 0.0, 16.0, 16.0, r, WHITE.alpha(0.14), (1.0 / s).max(0.06));
    // três anéis por dentro do outro, cada um um pouco cheio, com a ponta brilhante
    let (width, gap) = (1.52, 0.29);
    for (i, frac) in [0.72, 0.62, 0.84].into_iter().enumerate() {
        let diameter = 11.52 - 2.0 * (width + gap) * i as f32;
        drawing::ring(&mut c, 8.0, 8.0, diameter, width, frac, system::color(i));
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grava res/widgets.ico com o ícone do app em cada tamanho que o Windows usa, cada um
    /// desenhado no seu tamanho (nítido também nos pequenos):
    ///   cargo test icon_file -- --ignored
    #[test]
    #[ignore]
    fn icon_file() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        const SIZES: [u32; 10] = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256];
        let tmp = std::env::temp_dir().join("widgets-icone.png");
        let pngs: Vec<Vec<u8>> = SIZES
            .iter()
            .map(|&px| {
                crate::gfx::save_png(&icon_image(px), &tmp).unwrap();
                std::fs::read(&tmp).unwrap()
            })
            .collect();
        // .ico: cabeçalho, uma entrada de 16 bytes por tamanho e as imagens (PNG) em seguida
        let mut ico = vec![0u8, 0, 1, 0];
        ico.extend((SIZES.len() as u16).to_le_bytes());
        let mut offset = 6 + 16 * SIZES.len() as u32;
        for (&px, png) in SIZES.iter().zip(&pngs) {
            let side = if px >= 256 { 0 } else { px as u8 };
            ico.extend([side, side, 0, 0]);
            ico.extend(1u16.to_le_bytes());
            ico.extend(32u16.to_le_bytes());
            ico.extend((png.len() as u32).to_le_bytes());
            ico.extend(offset.to_le_bytes());
            offset += png.len() as u32;
        }
        for png in &pngs {
            ico.extend(png);
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("res").join("widgets.ico");
        std::fs::write(&path, ico).unwrap();
        println!("{}", path.display());
    }

    #[test]
    fn icon_has_the_system_rings() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let img = icon_image(64);
        assert_eq!((img.w, img.h), (64, 64));
        // os cantos ficam transparentes (quadrado arredondado) e o meio do anel de fora tem a cor da CPU
        assert_eq!(img.px[3], 0, "canto transparente");
        let at = |x: u32, y: u32| {
            let i = ((y * 64 + x) * 4) as usize;
            (img.px[i + 2], img.px[i + 1], img.px[i])
        };
        // alto do anel de fora (o arco começa no alto, em -90°)
        let (r, g, b) = at(32, 32 - 21);
        assert!(r > 200 && g < 120 && b < 150, "vermelho da CPU no alto: {:?}", (r, g, b));
    }
}

