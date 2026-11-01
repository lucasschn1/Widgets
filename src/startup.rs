//! Iniciar com o Windows (chave Run do registro).
//!
//! As compilações de teste usam outro nome na chave, para nunca trocar a inicialização de verdade.

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};

use crate::gfx::wide;
use crate::win;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

fn run_name() -> &'static str {
    if cfg!(debug_assertions) { "DesktopWidgetsDev" } else { "DesktopWidgets" }
}

/// O comando que abre o app: este Widgets.exe.
pub fn launch_command() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    format!("\"{}\"", exe.display())
}

pub fn enabled() -> bool {
    !win::registry(RUN_KEY, run_name()).is_empty()
}

pub fn set(on: bool) -> bool {
    let (key, name) = (wide(RUN_KEY), wide(run_name()));
    unsafe {
        if on {
            let value = wide(&launch_command());
            RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()), REG_SZ.0, Some(value.as_ptr() as _), (value.len() * 2) as u32).is_ok()
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()));
            true
        }
    }
}

/// Se a inicialização está ligada mas aponta para outro lugar (ex.: a versão em Python, ou o
/// app mudou de pasta), corrige o caminho.
pub fn refresh() {
    let current = win::registry(RUN_KEY, run_name());
    if !current.is_empty() && current != launch_command() {
        set(true);
    }
}
