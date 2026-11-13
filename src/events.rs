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
    /// tecla numa janela pop-up (menu, barra de edição): código virtual
    Key { hwnd: isize, vk: u32 },
    Tray(u32),
    Wake,
    TaskbarCreated,
    /// janela de painel: pediram para fechar (X da barra de título)
    PanelClose(isize),
    /// janela de painel mudou de lugar (o fundo "Mica" depende da posição)
    PanelMoved(isize),
    /// campo de texto (painel, número do campo): texto mudou / ganhou ou perdeu o foco / tecla especial
    EditChange(isize, u16),
    EditFocus(isize, u16, bool),
    EditKey(isize, u16, u32),
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

/// Janelas pop-up (menu de vidro, avisos, barra de edição): recebem mouse e teclado e podem
/// ganhar o foco, para as setas, o Enter e o Esc funcionarem.
unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
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
            mouse(MouseKind::Down);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            mouse(MouseKind::Up);
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            push(Ev::Key { hwnd: id, vk: wp.0 as u32 });
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let cursor = CURSORS.with(|c| c.borrow().get(&id).copied()).unwrap_or(Cursor::Arrow);
            unsafe {
                SetCursor(Some(load_cursor(cursor)));
            }
            LRESULT(1)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

thread_local! {
    /// o último quadro desenhado de cada janela de painel (pintado no WM_PAINT)
    static FRAMES: RefCell<HashMap<isize, (windows::Win32::Graphics::Gdi::HBITMAP, i32, i32)>> = RefCell::new(HashMap::new());
    /// cor de fundo dos campos de texto: (cor, pincel), por campo
    static EDIT_COLORS: RefCell<HashMap<isize, (u32, windows::Win32::Graphics::Gdi::HBRUSH)>> = RefCell::new(HashMap::new());
}

/// Guarda o quadro de um painel (pixels BGRA opacos) e pede para a janela se pintar.
pub fn set_frame(hwnd: HWND, img: &crate::gfx::Image) {
    use windows::Win32::Graphics::Gdi::*;
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
        let Ok(bmp) = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) else { return };
        std::ptr::copy_nonoverlapping(img.px.as_ptr(), bits as *mut u8, img.px.len());
        if let Some((old, ..)) = FRAMES.with(|f| f.borrow_mut().insert(hwnd.0 as isize, (bmp, img.w as i32, img.h as i32))) {
            let _ = DeleteObject(old.into());
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

pub fn set_edit_color(edit: HWND, rgb: (u8, u8, u8)) {
    let color = rgb.0 as u32 | (rgb.1 as u32) << 8 | (rgb.2 as u32) << 16;
    let id = edit.0 as isize;
    let same = EDIT_COLORS.with(|c| c.borrow().get(&id).is_some_and(|e| e.0 == color));
    if same {
        return;
    }
    unsafe {
        let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(COLORREF(color));
        if let Some((_, old)) = EDIT_COLORS.with(|c| c.borrow_mut().insert(id, (color, brush))) {
            let _ = windows::Win32::Graphics::Gdi::DeleteObject(old.into());
        }
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(edit), None, true);
    }
}

unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    use windows::Win32::Graphics::Gdi::*;
    let id = hwnd.0 as isize;
    let mouse = |kind| {
        let (x, y) = lparam_xy(lp);
        push(Ev::Mouse { hwnd: id, kind, x, y, sx: 0, sy: 0 });
    };
    match msg {
        WM_PAINT => unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            if let Some((bmp, w, h)) = FRAMES.with(|f| f.borrow().get(&id).copied()) {
                let mem = CreateCompatibleDC(Some(dc));
                let old = SelectObject(mem, bmp.into());
                let _ = BitBlt(dc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
                SelectObject(mem, old);
                let _ = DeleteDC(mem);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        },
        WM_ERASEBKGND => LRESULT(1),
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
                // tira o foco dos campos de texto (eles salvam ao perder o foco)
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(hwnd));
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
        WM_MOUSEWHEEL => {
            push(Ev::Wheel { hwnd: id, delta: ((wp.0 >> 16) & 0xFFFF) as i16 as i32 });
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            push(Ev::Key { hwnd: id, vk: wp.0 as u32 });
            LRESULT(0)
        }
        WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
            let cursor = CURSORS.with(|c| c.borrow().get(&id).copied()).unwrap_or(Cursor::Arrow);
            unsafe {
                SetCursor(Some(load_cursor(cursor)));
            }
            LRESULT(1)
        }
        WM_CTLCOLOREDIT => {
            let dc = HDC(wp.0 as _);
            let edit = lp.0;
            match EDIT_COLORS.with(|c| c.borrow().get(&edit).copied()) {
                Some((color, brush)) => unsafe {
                    SetBkColor(dc, COLORREF(color));
                    SetTextColor(dc, COLORREF(0x00F5F2F2));
                    LRESULT(brush.0 as isize)
                },
                None => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
            }
        }
        WM_COMMAND => {
            let ctrl = (wp.0 & 0xFFFF) as u16;
            match ((wp.0 >> 16) & 0xFFFF) as u32 {
                // EN_CHANGE (só quando quem mudou foi a pessoa, não o programa)
                0x0300 if !QUIET_EDIT.with(|q| q.get()) => push(Ev::EditChange(id, ctrl)),
                0x0100 => push(Ev::EditFocus(id, ctrl, true)),  // EN_SETFOCUS
                0x0200 => push(Ev::EditFocus(id, ctrl, false)), // EN_KILLFOCUS
                _ => {}
            }
            LRESULT(0)
        }
        WM_MOVE => {
            push(Ev::PanelMoved(id));
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_CLOSE => {
            push(Ev::PanelClose(id));
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some((bmp, ..)) = FRAMES.with(|f| f.borrow_mut().remove(&id)) {
                unsafe {
                    let _ = DeleteObject(bmp.into());
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Teclas especiais dentro de um campo de texto (Enter, Esc, setas): vão para o app.
unsafe extern "system" fn edit_subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    use windows::Win32::UI::Shell::DefSubclassProc;
    match msg {
        WM_KEYDOWN if matches!(wp.0 as u32, 0x0D | 0x1B | 0x26 | 0x28) => {
            let parent = unsafe { GetParent(hwnd).unwrap_or_default() };
            let ctrl = unsafe { GetDlgCtrlID(hwnd) } as u16;
            push(Ev::EditKey(parent.0 as isize, ctrl, wp.0 as u32));
            LRESULT(0)
        }
        // sem o "plim" do Windows ao apertar Enter ou Esc num campo de uma linha
        WM_CHAR if matches!(wp.0 as u32, 0x0D | 0x1B) => LRESULT(0),
        _ => unsafe { DefSubclassProc(hwnd, msg, wp, lp) },
    }
}

/// Campo de texto (EDIT) dentro de um painel, com a fonte Segoe UI de `px` pixels.
pub fn create_edit(parent: HWND, ctrl: u16, px: i32, placeholder: &str) -> HWND {
    use windows::Win32::Graphics::Gdi::*;
    unsafe {
        let style = WS_CHILD | WS_TABSTOP | WINDOW_STYLE(0x0080); // ES_AUTOHSCROLL
        let edit = CreateWindowExW(WINDOW_EX_STYLE(0), w!("EDIT"), w!(""), style, 0, 0, 10, 10, Some(parent), Some(HMENU(ctrl as _)), None, None)
            .expect("EDIT");
        let font = CreateFontW(-px, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"));
        SendMessageW(edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        // o texto de exemplo (some ao digitar)
        let cue = crate::gfx::wide(placeholder);
        SendMessageW(edit, 0x1501, Some(WPARAM(1)), Some(LPARAM(cue.as_ptr() as isize))); // EM_SETCUEBANNER
        let _ = windows::Win32::UI::Shell::SetWindowSubclass(edit, Some(edit_subclass), 1, 0);
        edit
    }
}

pub fn edit_text(edit: HWND) -> String {
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetWindowTextW(edit, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

thread_local! {
    /// ligado enquanto o programa troca o texto de um campo: o EN_CHANGE dessa troca não conta
    static QUIET_EDIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn set_edit_text(edit: HWND, text: &str) {
    let w = crate::gfx::wide(text);
    unsafe {
        QUIET_EDIT.with(|q| q.set(true));
        let _ = SetWindowTextW(edit, PCWSTR(w.as_ptr()));
        QUIET_EDIT.with(|q| q.set(false));
        let end = w.len() - 1;
        SendMessageW(edit, 0x00B1, Some(WPARAM(end)), Some(LPARAM(end as isize))); // EM_SETSEL: cursor no fim
    }
}

/// Cursor no começo do campo (mostra o início do texto).
pub fn caret_home(edit: HWND) {
    unsafe {
        SendMessageW(edit, 0x00B1, Some(WPARAM(0)), Some(LPARAM(0))); // EM_SETSEL
        SendMessageW(edit, 0x00B7, None, None); // EM_SCROLLCARET
    }
}

pub fn select_all(edit: HWND) {
    unsafe {
        SendMessageW(edit, 0x00B1, Some(WPARAM(0)), Some(LPARAM(-1)));
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
pub const POPUP_CLASS: PCWSTR = w!("DesktopWidgetsPopup");
pub const PANEL_CLASS: PCWSTR = w!("DesktopWidgetsPanel");

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
        let classes: [(PCWSTR, WNDPROC, WNDCLASS_STYLES); 5] = [
            (PANEL_CLASS, WNDPROC::Some(panel_proc), WNDCLASS_STYLES(0)),
            (POPUP_CLASS, WNDPROC::Some(popup_proc), WNDCLASS_STYLES(0)),
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

/// Janela de cartão desenhada pela placa de vídeo (veja `gpu`): sem a cópia em memória que o
/// Windows guarda das janelas comuns, nem a transparência por pixel das janelas transparentes (o
/// desenho chega pela DirectComposition).
pub fn create_composed(class: PCWSTR) -> HWND {
    let ex = WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    unsafe { CreateWindowExW(ex, class, w!("Widgets"), WS_POPUP, 0, 0, 1, 1, None, None, None, None).expect("CreateWindowExW") }
}

/// Janela pop-up transparente (menu, avisos): recebe cliques e pode ganhar o foco do teclado.
pub fn create_popup() -> HWND {
    let ex = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
    unsafe { CreateWindowExW(ex, POPUP_CLASS, w!("Widgets"), WS_POPUP, 0, 0, 1, 1, None, None, None, None).expect("CreateWindowExW") }
}

/// Traz a janela para a frente com o foco (o Windows só deixa se o app estiver ativo, o que é
/// o caso logo depois de um clique num widget ou na bandeja).
pub fn focus(hwnd: HWND) {
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(hwnd));
    }
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
