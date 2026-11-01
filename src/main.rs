//! Widgets de vidro para a área de trabalho do Windows: clima e sistema, "tocando agora",
//! GitHub e relógio Pomodoro.
//!
//! O visual é desenhado com Direct2D/DirectWrite e mostrado em janelas com transparência por
//! pixel; o fundo dos cartões é "vidro": o pedaço do papel de parede atrás deles, desfocado.

// sem janela de terminal na versão final
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Registro de depuração (só nas compilações de teste): %TEMP%\widgets-debug.log
#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if cfg!(debug_assertions) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true)
                .open(std::env::temp_dir().join("widgets-debug.log")) {
                let _ = writeln!(f, "{:?} {}", std::time::SystemTime::now(), format!($($arg)*));
            }
        }
    };
}

mod app;
mod backdrop;
mod card;
mod config;
mod drawing;
mod events;
mod gfx;
mod imgops;
mod overlay;
mod panel;
mod popup;
mod prompt;
mod settings;
mod shared;
mod startup;
mod theme;
mod tray;
mod util;
mod widgets;
mod win;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Se já há um app aberto, pede para ele se mostrar e devolve true.
fn already_running() -> bool {
    let name = if cfg!(debug_assertions) { w!("Local\\DesktopWidgets-lucasschn1-dev") } else { w!("Local\\DesktopWidgets-lucasschn1") };
    unsafe {
        let mutex = CreateMutexW(None, false, name);
        if mutex.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(hwnd) = FindWindowW(events::message_class(), PCWSTR::null()) {
                let _ = PostMessageW(Some(hwnd), events::WM_OTHER_INSTANCE, WPARAM(0), LPARAM(0));
            }
            return true;
        }
        // o mutex fica aberto até o processo terminar
        std::mem::forget(mutex);
    }
    false
}

fn main() {
    if already_running() {
        return;
    }
    unsafe {
        // cada monitor com a sua escala (os widgets se redesenham ao mudar de monitor)
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    startup::refresh();
    let mut app = app::App::new();
    let mut msg = MSG::default();
    loop {
        let t0 = std::time::Instant::now();
        let wait = app.next_deadline().saturating_duration_since(std::time::Instant::now());
        let ms = (wait.as_millis() as u32).min(1000);
        unsafe {
            MsgWaitForMultipleObjectsEx(None, ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        for ev in events::drain() {
            app.handle(ev);
            if app.quit {
                return;
            }
        }
        app.after_events();
        app.run_timers();
        if t0.elapsed() > std::time::Duration::from_millis(200) {
            debug_log!("laço lento: {:?}", t0.elapsed());
        }
    }
}
