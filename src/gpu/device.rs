//! O dispositivo da placa de vídeo: Direct3D 11 (ou o WARP, o Direct3D em software do Windows,
//! sem placa), o contexto de desenho do Direct2D ligado nele e a DirectComposition. Um por
//! thread, criado na primeira vez e de novo se a placa de vídeo for perdida.
//!
//! O Direct2D vem da mesma fábrica de `gfx`: as geometrias e os formatos de texto que a `Canvas`
//! cria servem tanto na tela em memória quanto na da placa de vídeo.

use std::cell::{Cell, RefCell};

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct2D::{ID2D1DeviceContext, ID2D1Factory1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION};
use windows::Win32::Graphics::DirectComposition::{DCompositionCreateDevice, IDCompositionDevice};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIDevice, IDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS};

/// O que é preciso para desenhar e mostrar na placa de vídeo.
pub struct Device {
    pub d3d: ID3D11Device,
    pub factory: IDXGIFactory2,
    pub dc: ID2D1DeviceContext,
    pub comp: IDCompositionDevice,
    /// muda a cada dispositivo criado: as superfícies feitas com um anterior se refazem
    pub generation: u64,
}

thread_local! {
    static DEVICE: RefCell<Option<Device>> = const { RefCell::new(None) };
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

impl Device {
    fn new() -> windows::core::Result<Device> {
        let d3d = create_d3d(D3D_DRIVER_TYPE_HARDWARE).or_else(|_| create_d3d(D3D_DRIVER_TYPE_WARP))?;
        let dxgi: IDXGIDevice = d3d.cast()?;
        unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let d2d: ID2D1Factory1 = crate::gfx::with(|g| g.d2d.cast())?;
            let dc = d2d.CreateDevice(&dxgi)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let comp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let generation = GENERATION.with(|g| {
                g.set(g.get() + 1);
                g.get()
            });
            Ok(Device { d3d, factory, dc, comp, generation })
        }
    }
}

fn create_d3d(kind: D3D_DRIVER_TYPE) -> windows::core::Result<ID3D11Device> {
    let mut device = None;
    unsafe {
        D3D11CreateDevice(None, kind, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, None)?;
    }
    device.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
}

/// Roda `f` com o dispositivo (criado agora, se ainda não há); None se não deu para criar.
pub fn with<R>(f: impl FnOnce(&Device) -> R) -> Option<R> {
    DEVICE.with(|d| {
        let mut d = d.borrow_mut();
        if d.is_none() {
            *d = Device::new().ok();
        }
        d.as_ref().map(f)
    })
}

/// A placa de vídeo foi perdida (o PC dormiu, o driver reiniciou): o próximo `with` cria outro.
pub fn lost() {
    DEVICE.with(|d| *d.borrow_mut() = None);
}

/// Dá para desenhar na placa de vídeo neste PC?
pub fn available() -> bool {
    with(|_| ()).is_some()
}
