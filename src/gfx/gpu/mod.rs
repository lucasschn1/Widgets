//! Desenhar na placa de vídeo: em vez de montar cada quadro do cartão em memória e copiar a
//! janela inteira para o Windows (`win::blit`), o quadro é desenhado direto numa superfície da
//! placa de vídeo da janela e mostrado pela DirectComposition, no ritmo da tela. O desenho em si
//! continua o mesmo (`Canvas`): só muda onde ele cai.
//!
//! - `device`: o Direct3D 11, o Direct2D ligado nele e a DirectComposition (um por thread);
//! - `surface`: a superfície de uma janela, que cresce quando o cartão cresce.
//!
//! Se a placa de vídeo sumir no meio (o PC dormiu, o driver reiniciou), o dispositivo é criado de
//! novo e as superfícies se refazem no próximo quadro.

mod device;
mod surface;

pub use device::available;
pub use surface::Surface;

/// Desenha com `draw` numa tela da placa de vídeo fora de qualquer janela (`w` × `h` pixels), só
/// para medir quanto o desenho custa (`perf`).
#[cfg(test)]
pub fn draw_offscreen(w: u32, h: u32, s: f32, draw: impl FnOnce(&mut crate::gfx::Canvas)) -> windows::core::Result<()> {
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_SIZE_U};
    use windows::Win32::Graphics::Direct2D::{ID2D1RenderTarget, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1};
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    device::with(|device| unsafe {
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            ..Default::default()
        };
        let target = device.dc.CreateBitmap(D2D_SIZE_U { width: w, height: h }, None, 0, &props)?;
        device.dc.SetTarget(&target);
        let rt: ID2D1RenderTarget = device.dc.cast()?;
        let mut c = crate::gfx::Canvas::on_target(rt, w, h, s, device.generation);
        draw(&mut c);
        let ended = c.end();
        device.dc.SetTarget(None);
        ended
    })
    .unwrap_or_else(|| Err(windows::Win32::Foundation::E_FAIL.into()))
}

/// O dispositivo é de uma placa de vídeo de verdade (e não o WARP, o Direct3D em software)?
#[cfg(test)]
pub fn hardware() -> bool {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::{IDXGIAdapter1, IDXGIDevice, DXGI_ADAPTER_FLAG_SOFTWARE};
    device::with(|device| unsafe {
        let dxgi: IDXGIDevice = device.d3d.cast().ok()?;
        let desc = dxgi.GetAdapter().ok()?.cast::<IDXGIAdapter1>().ok()?.GetDesc1().ok()?;
        Some(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0)
    })
    .flatten()
    .unwrap_or(false)
}
