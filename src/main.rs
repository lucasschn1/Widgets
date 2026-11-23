//! Widgets de vidro para a área de trabalho do Windows: clima e sistema, "tocando agora",
//! GitHub e relógio Pomodoro.
//!
//! O visual é desenhado com Direct2D/DirectWrite e mostrado em janelas com transparência por
//! pixel; o fundo dos cartões é "vidro": o pedaço do papel de parede atrás deles, desfocado.
//!
//! As pastas: `app` (o coordenador: liga os widgets, as janelas, os menus, a ilha e os modos),
//! `card` (o cartão de cada widget), `widgets` (o que cada um mostra), `ui` (as janelas
//! desenhadas), `gfx` (o desenho), `platform` (a conversa com o Windows), `data` (opções, estado e
//! cálculos) e `testes` (só nos testes).

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

pub(crate) mod app;
mod card;
mod data;
mod gfx;
mod platform;
#[cfg(test)]
mod testes;
mod ui;
mod widgets;

use data::claude_usage;
use gfx::meter;
use platform::{events, startup, updates};

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

/// enquanto o próximo quadro vem em menos que isto, o relógio do Windows fica fino (1 ms)
const FINE_TIMER_BELOW: std::time::Duration = std::time::Duration::from_millis(50);
/// o laço espera no máximo 1 s por vez (sem nada para fazer, ainda confere o mundo)
const MAX_WAIT_MS: u32 = 1000;
/// uma volta do laço mais lenta que isto vai para o registro de depuração
const SLOW_LOOP: std::time::Duration = std::time::Duration::from_millis(200);

/// Os argumentos que fazem o Widgets.exe responder e sair, sem abrir os widgets. Devolve true se
/// era um deles.
fn answered_command_line() -> bool {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        // chamado pelo Claude Code como linha de status: responde e sai
        Some(claude_usage::FLAG) => claude_usage::run_status_line(),
        // chamado pelo Claude Code quando precisa de você: guarda o aviso e sai
        Some(claude_usage::NOTIFY_FLAG) => claude_usage::run_notify(),
        // `--versao`: responde e sai, sem abrir nada nem mexer nas opções
        Some(updates::VERSION_FLAG) => println!("{}", updates::VERSION),
        _ => return false,
    }
    true
}

/// Liga (ou desliga) o relógio fino do Windows. O relógio normal anda de 15,6 em 15,6 ms:
/// esperar "16 ms" virava 31 e as animações ficavam a uns 32 quadros por segundo. Parado, o
/// relógio volta ao normal (sem gastar bateria).
fn set_fine_timer(on: bool) {
    unsafe {
        if on {
            windows::Win32::Media::timeBeginPeriod(1);
        } else {
            windows::Win32::Media::timeEndPeriod(1);
        }
    }
}

/// Espera até `ms` milissegundos por uma mensagem do Windows e entrega as que chegaram. Devolve
/// false se o app deve sair (WM_QUIT).
fn pump_messages(msg: &mut MSG, ms: u32) -> bool {
    unsafe {
        MsgWaitForMultipleObjectsEx(None, ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        while PeekMessageW(msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return false;
            }
            let _ = TranslateMessage(msg);
            DispatchMessageW(msg);
        }
    }
    true
}

fn main() {
    if answered_command_line() || already_running() {
        return;
    }
    unsafe {
        // cada monitor com a sua escala (os widgets se redesenham ao mudar de monitor)
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    startup::refresh();
    // conectado ao Claude Code por uma versão sem o aviso de "precisa de você": põe o aviso
    claude_usage::upgrade();
    let mut app = app::App::new();
    let mut msg = MSG::default();
    let mut fine_timer = false;
    loop {
        let loop_start = std::time::Instant::now();
        let wait = app.next_deadline().saturating_duration_since(std::time::Instant::now());
        // enquanto algo anima (o próximo quadro vem logo), o relógio do Windows fica fino
        let animating = wait < FINE_TIMER_BELOW;
        if animating != fine_timer {
            fine_timer = animating;
            set_fine_timer(animating);
        }
        // arredonda para cima: acordar 1 ms antes do prazo custaria uma espera inteira a mais
        let ms = (wait.as_micros().div_ceil(1000) as u32).min(MAX_WAIT_MS);
        if !pump_messages(&mut msg, ms) {
            return;
        }
        for event in events::drain() {
            app.handle(event);
            if app.quit {
                return;
            }
        }
        app.after_events();
        app.run_timers();
        meter::flush();
        if loop_start.elapsed() > SLOW_LOOP {
            debug_log!("laço lento: {:?}", loop_start.elapsed());
        }
    }
}
