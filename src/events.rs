//! Janelas e eventos. As funções que o Windows chama (window procedures) só põem o que aconteceu
//! numa fila; o laço principal processa a fila com acesso ao app inteiro. Assim nada do app é
//! usado "duas vezes ao mesmo tempo" quando o Windows chama de volta no meio de outra função.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_MOUSELEAVE: u32 = 0x2A3; // (fica em UI::Controls no crate windows)
pub const WM_TRAY: u32 = WM_APP + 1;
pub const WM_OTHER_INSTANCE: u32 = WM_APP + 4;
pub const WM_WAKE: u32 = WM_APP + 9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseKind {
    Move,
    Down,
    Up,
    Double,
    Right,
    Leave,
}

#[derive(Debug)]
pub enum Ev {
    /// (x, y) na janela e (sx, sy) na tela, calculado na hora em que o Windows entregou a mensagem
    Mouse { hwnd: isize, kind: MouseKind, x: i32, y: i32, sx: i32, sy: i32 },
    Wheel { hwnd: isize, delta: i32 },
    Tray(u32),
    Hotkey(i32),
    Wake,
    TaskbarCreated,
    OtherInstance,
    DisplayChange,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cursor {
    Arrow,
    Hand,
    Move,
}

thread_local! {
    static QUEUE: RefCell<VecDeque<Ev>> = RefCell::new(VecDeque::new());
    static CURSORS: RefCell<HashMap<isize, Cursor>> = RefCell::new(HashMap::new());
    static TRACKING: RefCell<HashSet<isize>> = RefCell::new(HashSet::new());
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

/// A janela de mensagens (bandeja, atalhos, acordar): usada pelas threads para acordar o laço.
static MESSAGE_HWND: AtomicIsize = AtomicIsize::new(0);

fn push(ev: Ev) {
    QUEUE.with(|q| q.borrow_mut().push_back(ev));
}

pub fn drain() -> Vec<Ev> {
    QUEUE.with(|q| q.borrow_mut().drain(..).collect())
}

pub fn set_cursor(hwnd: HWND, cursor: Cursor) {
    CURSORS.with(|c| c.borrow_mut().insert(hwnd.0 as isize, cursor));
    unsafe {
        let _ = SetCursor(Some(load_cursor(cursor)));
    }
}

fn load_cursor(cursor: Cursor) -> HCURSOR {
    let id = match cursor {
        Cursor::Arrow => IDC_ARROW,
        Cursor::Hand => IDC_HAND,
        Cursor::Move => IDC_SIZEALL,
    };
    unsafe { LoadCursorW(None, id).unwrap_or_default() }
}

/// Acorda o laço principal (chamado de outras threads quando algo fica pronto).
pub fn wake() {
    let h = MESSAGE_HWND.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as _)), WM_WAKE, WPARAM(0), LPARAM(0));
        }
    }
}

fn lparam_xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

unsafe extern "system" fn card_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let id = hwnd.0 as isize;
    let mouse = |kind| {
        let (x, y) = lparam_xy(lp);
        let mut pt = POINT { x, y };
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut pt);
        }
        push(Ev::Mouse { hwnd: id, kind, x, y, sx: pt.x, sy: pt.y });
    };
    match msg {
        WM_MOUSEMOVE => {
            let new = TRACKING.with(|t| t.borrow_mut().insert(id));
            if new {
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                unsafe {
                    let _ = TrackMouseEvent(&mut tme);
                }
            }
            mouse(MouseKind::Move);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            TRACKING.with(|t| t.borrow_mut().remove(&id));
            push(Ev::Mouse { hwnd: id, kind: MouseKind::Leave, x: 0, y: 0, sx: 0, sy: 0 });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            unsafe {
                SetCapture(hwnd);
            }
            mouse(MouseKind::Down);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            unsafe {
                let _ = ReleaseCapture();
            }
            mouse(MouseKind::Up);
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            mouse(MouseKind::Double);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            mouse(MouseKind::Right);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            push(Ev::Wheel { hwnd: id, delta: ((wp.0 >> 16) & 0xFFFF) as i16 as i32 });
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let cursor = CURSORS.with(|c| c.borrow().get(&id).copied()).unwrap_or(Cursor::Arrow);
            unsafe {
                SetCursor(Some(load_cursor(cursor)));
            }
            LRESULT(1)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

unsafe extern "system" fn message_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            push(Ev::Tray((lp.0 & 0xFFFF) as u32));
            LRESULT(0)
        }
        WM_HOTKEY => {
            push(Ev::Hotkey(wp.0 as i32));
            LRESULT(0)
        }
        WM_WAKE => {
            push(Ev::Wake);
            LRESULT(0)
        }
        WM_OTHER_INSTANCE => {
            push(Ev::OtherInstance);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            push(Ev::DisplayChange);
            LRESULT(0)
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.with(Cell::get) => {
            push(Ev::TaskbarCreated);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

pub const CARD_CLASS: PCWSTR = w!("DesktopWidgetsCard");
pub const OVERLAY_CLASS: PCWSTR = w!("DesktopWidgetsOverlay");

/// Nome da janela de mensagens: a segunda cópia do app a procura para pedir que esta se mostre.
pub fn message_class() -> PCWSTR {
    if cfg!(debug_assertions) {
        w!("DesktopWidgetsTrayDev")
    } else {
        w!("DesktopWidgetsTray")
    }
}

pub fn register_classes() {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let classes: [(PCWSTR, WNDPROC, WNDCLASS_STYLES); 3] = [
            (CARD_CLASS, WNDPROC::Some(card_proc), CS_DBLCLKS),
            (OVERLAY_CLASS, WNDPROC::Some(overlay_proc), WNDCLASS_STYLES(0)),
            (message_class(), WNDPROC::Some(message_proc), WNDCLASS_STYLES(0)),
        ];
        for (name, proc, style) in classes {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style,
                lpfnWndProc: proc,
                hInstance: instance,
                lpszClassName: name,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }
        TASKBAR_CREATED.with(|c| c.set(RegisterWindowMessageW(w!("TaskbarCreated"))));
    }
}

/// Janela transparente sem borda (cartão ou camada por cima de tudo). `click_through`: os cliques
/// passam direto (guias e dicas).
pub fn create_layered(class: PCWSTR, click_through: bool) -> HWND {
    let mut ex = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    if click_through {
        ex |= WS_EX_TRANSPARENT;
    }
    unsafe { CreateWindowExW(ex, class, w!("Widgets"), WS_POPUP, 0, 0, 1, 1, None, None, None, None).expect("CreateWindowExW") }
}

pub fn create_message_window() -> HWND {
    let hwnd = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), message_class(), w!("Widgets"), WINDOW_STYLE(0), 0, 0, 0, 0, None, None, None, None) }
        .expect("janela de mensagens");
    MESSAGE_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
    hwnd
}

pub fn show(hwnd: HWND, on: bool) {
    unsafe {
        let _ = ShowWindow(hwnd, if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

pub fn destroy(hwnd: HWND) {
    CURSORS.with(|c| c.borrow_mut().remove(&(hwnd.0 as isize)));
    TRACKING.with(|t| t.borrow_mut().remove(&(hwnd.0 as isize)));
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}
