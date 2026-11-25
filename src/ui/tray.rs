//! Ícone na bandeja do sistema e notificações do Windows.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::platform::events::WM_TRAY;
use crate::gfx::{Canvas, Fill, Image};
use crate::ui::theme::{Rgba, WHITE};

pub const TIP: &str = "Widgets da área de trabalho";
pub const NIN_BALLOONUSERCLICK: u32 = 0x405;

// o ícone do app, num quadro de 16 × 16 (ampliado para cada tamanho)
/// A placa arredondada (como os ícones do macOS): o raio dos cantos e o degradê escuro dela.
const PLATE_RADIUS: f32 = 3.6;
const PLATE_TOP: Rgba = Rgba::rgb(42, 40, 64);
const PLATE_BOTTOM: Rgba = Rgba::rgb(18, 17, 25);
/// As três cores do verde do ícone: a clara do começo do anel, a do meio e a mais clara da ponta.
const GREEN_LIGHT: Rgba = Rgba::rgb(52, 211, 153);
const GREEN: Rgba = Rgba::rgb(16, 163, 127);
const GREEN_PALE: Rgba = Rgba::rgb(167, 243, 208);
/// O anel: o raio da linha do meio, a grossura e quanto dele está cheio.
const RING_RADIUS: f32 = 4.8;
const RING_WIDTH: f32 = 2.4;
const RING_FILL: f32 = 0.74;
/// Em quantos pedaços o anel é desenhado para o degradê correr por ele.
const RING_STEPS: usize = 48;
/// O vidro no meio do anel.
const GLASS_RADIUS: f32 = 2.72;

/// Ícone do app: um anel só, grosso, num degradê verde, com um vidro no meio, sobre uma placa
/// escura arredondada com a luz verde vindo do alto (como os ícones do macOS). Simples o bastante
/// para continuar claro em 16 px, na bandeja. Desenhado num quadro de 16 × 16 ampliado para `px`.
pub fn icon_image(px: u32) -> Image {
    let s = px as f32 / 16.0;
    let mut c = Canvas::new(px, px, s);
    let r = PLATE_RADIUS;
    c.fill_round_rect_with(0.0, 0.0, 16.0, 16.0, r, Fill::VGrad(0.0, 16.0, PLATE_TOP, PLATE_BOTTOM));
    // a luz verde vindo do canto de cima à esquerda
    c.push_round_clip(0.0, 0.0, 16.0, 16.0, r);
    c.fill_radial(4.0, 0.0, 17.6, 17.6, &[(0.0, GREEN_LIGHT.alpha(0.55)), (0.6, GREEN.alpha(0.15)), (1.0, GREEN.alpha(0.0))]);
    c.pop_clip();
    c.stroke_round_rect(0.0, 0.0, 16.0, 16.0, r, WHITE.alpha(0.14), (1.0 / s).max(0.06));
    // o anel: o trilho apagado e a parte cheia em degradê, começando no alto
    c.stroke_circle(8.0, 8.0, RING_RADIUS, GREEN.alpha(0.25), RING_WIDTH);
    let sweep = 360.0 * RING_FILL;
    for step in 0..RING_STEPS {
        let (from, to) = (step as f32 / RING_STEPS as f32, (step + 1) as f32 / RING_STEPS as f32);
        let color = if from < 0.5 { GREEN_LIGHT.mix(GREEN, from * 2.0) } else { GREEN.mix(GREEN_PALE, from * 2.0 - 1.0) };
        c.arc(8.0, 8.0, RING_RADIUS, -90.0 + sweep * from, -90.0 + sweep * to, color, RING_WIDTH);
    }
    // o vidro no meio: claro e translúcido, com a borda e o brilho no alto
    c.fill_circle(8.0, 8.0, GLASS_RADIUS, GREEN_PALE.alpha(0.12));
    c.fill_circle_with(8.0, 8.0, GLASS_RADIUS, Fill::VGrad(8.0 - GLASS_RADIUS, 8.0 + GLASS_RADIUS * 0.1, WHITE.alpha(0.32), WHITE.alpha(0.0)));
    c.stroke_circle(8.0, 8.0, GLASS_RADIUS - 0.08, WHITE.alpha(0.45), 0.16);
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
        for (straight, premultiplied) in out.chunks_exact_mut(4).zip(img.px.chunks_exact(4)) {
            let alpha = premultiplied[3] as u32;
            for channel in 0..3 {
                straight[channel] = if alpha == 0 { 0 } else { ((premultiplied[channel] as u32 * 255 + alpha / 2) / alpha).min(255) as u8 };
            }
            straight[3] = premultiplied[3];
        }
        let mask = CreateBitmap(img.w as i32, img.h as i32, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

/// O ícone do app na bandeja do sistema.
pub struct Tray {
    nid: NOTIFYICONDATAW,
    pub ok: bool,
}

/// Copia o texto (cortado, se não couber) num campo de texto do Windows, terminado em zero.
fn copy_into(dst: &mut [u16], text: &str) {
    let utf16: Vec<u16> = text.encode_utf16().take(dst.len() - 1).collect();
    dst[..utf16.len()].copy_from_slice(&utf16);
    dst[utf16.len()] = 0;
}

impl Tray {
    /// Põe o ícone na bandeja, avisando a janela `hwnd` dos cliques.
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
        let mut tray = Tray { nid, ok: false };
        tray.add();
        tray
    }

    /// Põe o ícone na bandeja (de novo, se o Explorer reiniciou).
    pub fn add(&mut self) {
        self.nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        self.ok = unsafe { Shell_NotifyIconW(NIM_ADD, &self.nid).as_bool() };
    }

    /// Troca o texto que aparece com o mouse em cima do ícone.
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

    /// Tira o ícone da bandeja.
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

    /// O ícone: a placa arredondada (cantos transparentes), o anel verde no alto (onde ele começa)
    /// e o vidro claro no meio; também em 16 px, o tamanho da bandeja.
    #[test]
    fn icon_has_the_green_ring() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        for px in [64u32, 16] {
            let img = icon_image(px);
            assert_eq!((img.w, img.h), (px, px));
            assert_eq!(img.px[3], 0, "{px}: canto transparente");
            let at = |x: f32, y: f32| {
                let i = (((y * px as f32 / 16.0) as u32 * px + (x * px as f32 / 16.0) as u32) * 4) as usize;
                (img.px[i + 2], img.px[i + 1], img.px[i])
            };
            // o alto do anel (o arco começa no alto, em -90°): verde
            let (r, g, b) = at(8.0, 8.0 - RING_RADIUS);
            assert!(g > 150 && g > r + 60 && g > b, "{px}: verde no alto do anel: {:?}", (r, g, b));
            // o meio: o vidro, mais claro que a placa em volta do anel
            let glass = at(8.0, 8.0);
            let plate = at(1.5, 14.5);
            assert!(glass.0 as u32 + glass.1 as u32 + glass.2 as u32 > plate.0 as u32 + plate.1 as u32 + plate.2 as u32, "{px}: o vidro do meio não aparece");
        }
    }
}

