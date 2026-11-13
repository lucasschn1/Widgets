//! A superfície de uma janela na placa de vídeo: uma cadeia de dois quadros (um sendo mostrado,
//! outro sendo desenhado) com alfa, ligada à janela pela DirectComposition. Ela só cresce: um
//! cartão que diminui desenha num pedaço dela, e a janela recorta o resto (refazer os quadros a
//! cada passo da troca de tamanho custaria mais que sobrar um pouco).

use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{ID2D1RenderTarget, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1};
use windows::Win32::Graphics::DirectComposition::{IDCompositionTarget, IDCompositionVisual};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    IDXGISurface, IDXGISwapChain1, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

use super::device::{self, Device};
use crate::gfx::Canvas;

/// Quanto a superfície cresce além do pedido (pixels), para não refazer a cada pixel a mais.
const ROOM: u32 = 64;

/// A superfície de uma janela (criada no primeiro quadro).
pub struct Surface {
    hwnd: HWND,
    made: Option<Made>,
}

/// O que foi criado para a janela, com o dispositivo de `generation`.
struct Made {
    generation: u64,
    swap: IDXGISwapChain1,
    size: (u32, u32),
    // (guardados: soltar o alvo ou o visual tiraria o desenho da janela)
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
}

impl Surface {
    pub fn new(hwnd: HWND) -> Surface {
        Surface { hwnd, made: None }
    }

    /// Desenha um quadro de `w` × `h` pixels (escala `s`) com `draw` e mostra na janela. Se a placa
    /// de vídeo foi perdida, ela é refeita no próximo quadro.
    pub fn present(&mut self, w: u32, h: u32, s: f32, draw: impl FnOnce(&mut Canvas)) -> windows::core::Result<()> {
        let result = device::with(|d| self.frame(d, w.max(1), h.max(1), s, draw)).unwrap_or_else(|| Err(windows::Win32::Foundation::E_FAIL.into()));
        if let Err(e) = &result {
            if e.code() == DXGI_ERROR_DEVICE_REMOVED || e.code() == DXGI_ERROR_DEVICE_RESET {
                device::lost();
            }
            self.made = None;
        }
        result
    }

    fn frame(&mut self, d: &Device, w: u32, h: u32, s: f32, draw: impl FnOnce(&mut Canvas)) -> windows::core::Result<()> {
        let swap = self.swap_for(d, w, h)?;
        unsafe {
            let buffer: IDXGISurface = swap.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let target = d.dc.CreateBitmapFromDxgiSurface(&buffer, Some(&props))?;
            d.dc.SetTarget(&target);
            let rt: ID2D1RenderTarget = d.dc.cast()?;
            let mut c = Canvas::on_target(rt, w, h, s);
            draw(&mut c);
            let ended = c.end();
            d.dc.SetTarget(None);
            ended?;
            swap.Present(0, DXGI_PRESENT(0)).ok()
        }
    }

    /// A cadeia de quadros da janela, de pelo menos `w` × `h` (criada ou aumentada agora).
    fn swap_for(&mut self, d: &Device, w: u32, h: u32) -> windows::core::Result<IDXGISwapChain1> {
        match &mut self.made {
            Some(m) if m.generation == d.generation => {
                if w > m.size.0 || h > m.size.1 {
                    let size = (m.size.0.max(w + ROOM), m.size.1.max(h + ROOM));
                    unsafe { m.swap.ResizeBuffers(0, size.0, size.1, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))? };
                    m.size = size;
                }
                Ok(m.swap.clone())
            }
            _ => {
                let made = make(d, self.hwnd, (w + ROOM, h + ROOM))?;
                let swap = made.swap.clone();
                self.made = Some(made);
                Ok(swap)
            }
        }
    }
}

/// A cadeia de quadros de `size` pixels, ligada à janela.
fn make(d: &Device, hwnd: HWND, size: (u32, u32)) -> windows::core::Result<Made> {
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: size.0,
        Height: size.1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        ..Default::default()
    };
    unsafe {
        let swap = d.factory.CreateSwapChainForComposition(&d.d3d, &desc, None)?;
        let target = d.comp.CreateTargetForHwnd(hwnd, true)?;
        let visual = d.comp.CreateVisual()?;
        visual.SetContent(&swap)?;
        target.SetRoot(&visual)?;
        d.comp.Commit()?;
        Ok(Made { generation: d.generation, swap, size, _target: target, _visual: visual })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Rgba;
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP};

    /// Desenha quadros de verdade numa janela (escondida) pela placa de vídeo: a superfície nasce
    /// no primeiro, cresce quando o quadro cresce e não é refeita quando ele diminui.
    #[test]
    fn draws_frames_on_a_window() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        if !super::super::available() {
            eprintln!("sem Direct3D neste PC: nada a conferir");
            return;
        }
        let hwnd = unsafe { CreateWindowExW(WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW, w!("STATIC"), w!(""), WS_POPUP, 0, 0, 200, 100, None, None, None, None).unwrap() };
        let mut surface = Surface::new(hwnd);
        let draw = |c: &mut Canvas| {
            c.fill_round_rect(4.0, 4.0, 120.0, 60.0, 12.0, Rgba::rgb(40, 120, 220));
            c.text(10.0, 10.0, "Widgets", crate::gfx::Face::Semibold, 13.0, Rgba::rgb(255, 255, 255), crate::gfx::Anchor::LA, None);
        };
        // (sem uma área de trabalho de verdade, como em alguns servidores de testes, a
        // DirectComposition não liga a janela: nada a conferir)
        if let Err(e) = surface.present(200, 100, 1.0, draw) {
            eprintln!("sem DirectComposition aqui ({e:?}): nada a conferir");
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return;
        }
        let first = surface.made.as_ref().unwrap().size;
        assert!(first.0 >= 200 && first.1 >= 100);
        surface.present(400, 300, 1.25, draw).unwrap();
        let grown = surface.made.as_ref().unwrap().size;
        assert!(grown.0 >= 400 && grown.1 >= 300, "não cresceu: {grown:?}");
        surface.present(100, 50, 1.0, draw).unwrap();
        assert_eq!(surface.made.as_ref().unwrap().size, grown, "refez ao diminuir");
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
