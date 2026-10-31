"""Acesso à API do Windows: janelas com transparência, monitores e utilidades do sistema."""

import ctypes
import math
import os
import winreg
from ctypes import wintypes

from PIL import Image, ImageChops


# ---------- janela com transparência por pixel ----------

class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD),
        ("biWidth", wintypes.LONG),
        ("biHeight", wintypes.LONG),
        ("biPlanes", wintypes.WORD),
        ("biBitCount", wintypes.WORD),
        ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD),
        ("biXPelsPerMeter", wintypes.LONG),
        ("biYPelsPerMeter", wintypes.LONG),
        ("biClrUsed", wintypes.DWORD),
        ("biClrImportant", wintypes.DWORD),
    ]


class BLENDFUNCTION(ctypes.Structure):
    _fields_ = [
        ("BlendOp", ctypes.c_ubyte),
        ("BlendFlags", ctypes.c_ubyte),
        ("SourceConstantAlpha", ctypes.c_ubyte),
        ("AlphaFormat", ctypes.c_ubyte),
    ]


user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32
HANDLE = ctypes.c_void_p
user32.GetParent.restype = HANDLE
user32.GetParent.argtypes = [HANDLE]
user32.GetDC.restype = HANDLE
user32.GetDC.argtypes = [HANDLE]
user32.ReleaseDC.argtypes = [HANDLE, HANDLE]
user32.GetWindowLongPtrW.restype = ctypes.c_ssize_t
user32.GetWindowLongPtrW.argtypes = [HANDLE, ctypes.c_int]
user32.SetWindowLongPtrW.restype = ctypes.c_ssize_t
user32.SetWindowLongPtrW.argtypes = [HANDLE, ctypes.c_int, ctypes.c_ssize_t]
user32.SetWindowPos.argtypes = [HANDLE, HANDLE, ctypes.c_int, ctypes.c_int,
                                ctypes.c_int, ctypes.c_int, ctypes.c_uint]
user32.UpdateLayeredWindow.argtypes = [
    HANDLE, HANDLE, ctypes.POINTER(wintypes.POINT), ctypes.POINTER(wintypes.SIZE),
    HANDLE, ctypes.POINTER(wintypes.POINT), wintypes.DWORD,
    ctypes.POINTER(BLENDFUNCTION), wintypes.DWORD]
gdi32.CreateCompatibleDC.restype = HANDLE
gdi32.CreateCompatibleDC.argtypes = [HANDLE]
gdi32.CreateDIBSection.restype = HANDLE
gdi32.CreateDIBSection.argtypes = [HANDLE, ctypes.POINTER(BITMAPINFOHEADER), ctypes.c_uint,
                                   ctypes.POINTER(ctypes.c_void_p), HANDLE, wintypes.DWORD]
gdi32.SelectObject.restype = HANDLE
gdi32.SelectObject.argtypes = [HANDLE, HANDLE]
gdi32.DeleteObject.argtypes = [HANDLE]
gdi32.DeleteDC.argtypes = [HANDLE]


user32.FindWindowW.restype = HANDLE
user32.FindWindowW.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR]
user32.ShowWindow.argtypes = [HANDLE, ctypes.c_int]
user32.IsWindowVisible.argtypes = [HANDLE]
user32.IsIconic.argtypes = [HANDLE]
user32.SetForegroundWindow.argtypes = [HANDLE]
user32.GetWindowTextW.argtypes = [HANDLE, wintypes.LPWSTR, ctypes.c_int]
user32.GetMonitorInfoW.argtypes = [HANDLE, ctypes.c_void_p]
user32.GetAsyncKeyState.restype = ctypes.c_short

HWND_TOP, HWND_BOTTOM, HWND_TOPMOST, HWND_NOTOPMOST = 0, 1, -1, -2
SWP_NOSIZE, SWP_NOMOVE, SWP_NOACTIVATE, SWP_SHOWWINDOW = 0x1, 0x2, 0x10, 0x40
GWLP_HWNDPARENT = -8


def set_z(hwnd, where):
    user32.SetWindowPos(hwnd, where, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)


def desktop_window():
    return user32.FindWindowW("Progman", None)


def pin_to_desktop(hwnd):
    """Faz a área de trabalho ser a "dona" da janela.

    Assim o Windows a trata como parte do desktop: ela continua visível no
    "Mostrar área de trabalho" (Win+D) e fica sempre logo acima do papel de parede.
    """
    progman = desktop_window()
    if progman:
        user32.SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman)
    return progman


def make_layered(hwnd, click_through=False):
    GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_TOOLWINDOW = -20, 0x80000, 0x80
    style = user32.GetWindowLongPtrW(hwnd, GWL_EXSTYLE) | WS_EX_TOOLWINDOW
    if click_through:  # não recebe cliques nem foco (guias e dicas)
        style |= 0x20 | 0x08000000  # WS_EX_TRANSPARENT | WS_EX_NOACTIVATE
    # A janela foi mapeada invisível com "-alpha 0" do Tk; desligar e religar o estilo
    # descarta essa transparência fixa e libera o uso de UpdateLayeredWindow.
    user32.SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & ~WS_EX_LAYERED)
    user32.SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED)


def blit(hwnd, image, pos=None, alpha=255):
    """Mostra uma imagem RGBA como o conteúdo da janela, respeitando o alfa.

    Com `pos`, move e redimensiona a janela no mesmo passo (usado na animação);
    `alpha` deixa a janela inteira mais transparente.
    """
    w, h = image.size
    r, g, b, a = image.split()
    r, g, b = (ImageChops.multiply(c, a) for c in (r, g, b))  # alfa pré-multiplicado
    data = Image.merge("RGBA", (b, g, r, a)).tobytes()

    screen = user32.GetDC(None)
    mem = gdi32.CreateCompatibleDC(screen)
    header = BITMAPINFOHEADER(ctypes.sizeof(BITMAPINFOHEADER), w, -h, 1, 32, 0, 0, 0, 0, 0, 0)
    bits = ctypes.c_void_p()
    bitmap = gdi32.CreateDIBSection(screen, ctypes.byref(header), 0, ctypes.byref(bits), None, 0)
    ctypes.memmove(bits, data, len(data))
    old = gdi32.SelectObject(mem, bitmap)
    blend = BLENDFUNCTION(0, 0, alpha, 1)  # AC_SRC_OVER com AC_SRC_ALPHA
    dest = ctypes.byref(wintypes.POINT(*pos)) if pos else None
    user32.UpdateLayeredWindow(hwnd, screen, dest, ctypes.byref(wintypes.SIZE(w, h)), mem,
                               ctypes.byref(wintypes.POINT(0, 0)), 0, ctypes.byref(blend), 2)
    gdi32.SelectObject(mem, old)
    gdi32.DeleteObject(bitmap)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(None, screen)


# ---------- vidro ----------

MONITORENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, HANDLE, HANDLE, ctypes.POINTER(wintypes.RECT),
                                     wintypes.LPARAM)
user32.EnumDisplayMonitors.argtypes = [HANDLE, ctypes.c_void_p, MONITORENUMPROC, wintypes.LPARAM]


class MONITORINFO(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.DWORD), ("rcMonitor", wintypes.RECT),
                ("rcWork", wintypes.RECT), ("dwFlags", wintypes.DWORD)]


def monitor_areas():
    """[(retângulo do monitor, área de trabalho sem a barra de tarefas)] de cada monitor."""
    areas = []

    def found(monitor, _dc, _rect, _data):
        info = MONITORINFO()
        info.cbSize = ctypes.sizeof(info)
        user32.GetMonitorInfoW(monitor, ctypes.byref(info))
        rect = lambda r: (r.left, r.top, r.right, r.bottom)
        area = (rect(info.rcMonitor), rect(info.rcWork))
        if info.dwFlags & 1:  # o monitor principal vem primeiro
            areas.insert(0, area)
        else:
            areas.append(area)
        return True

    user32.EnumDisplayMonitors(None, None, MONITORENUMPROC(found), 0)
    return areas


def monitors():
    return tuple(screen for screen, _work in monitor_areas())


def work_areas():
    areas = [work for _screen, work in monitor_areas()]
    return areas or [(0, 0, user32.GetSystemMetrics(0), user32.GetSystemMetrics(1))]


def overlap(a, b):
    """Largura e altura da interseção de dois retângulos (0 se não se tocam)."""
    return max(0, min(a[2], b[2]) - max(a[0], b[0])), max(0, min(a[3], b[3]) - max(a[1], b[1]))


def area_for(rect):
    """Área de trabalho do monitor onde está a maior parte do retângulo."""
    areas = work_areas()
    return max(areas, key=lambda a: math.prod(overlap(rect, a)), default=areas[0])


EnumWindowsProc = ctypes.WINFUNCTYPE(wintypes.BOOL, HANDLE, wintypes.LPARAM)
user32.EnumWindows.argtypes = [EnumWindowsProc, wintypes.LPARAM]
user32.keybd_event.argtypes = [ctypes.c_ubyte, ctypes.c_ubyte, wintypes.DWORD, ctypes.c_size_t]


def find_window(title_part):
    """Primeira janela visível cujo título contém `title_part`."""
    found = []

    def check(hwnd, _data):
        if user32.IsWindowVisible(hwnd):
            buf = ctypes.create_unicode_buffer(512)
            user32.GetWindowTextW(hwnd, buf, 512)
            if title_part.lower() in buf.value.lower():
                found.append(hwnd)
                return False
        return True

    user32.EnumWindows(EnumWindowsProc(check), 0)
    return found[0] if found else None


def bring_to_front(hwnd):
    if user32.IsIconic(hwnd):
        user32.ShowWindow(hwnd, 9)  # SW_RESTORE
    user32.SetForegroundWindow(hwnd)


def open_target(target):
    """Abre um programa, endereço ou página de configurações do Windows."""
    try:
        os.startfile(target)
    except OSError:
        pass


def change_volume(steps):
    """Sobe ou desce o volume do Windows (2% por passo), com o aviso de volume na tela."""
    key = 0xAF if steps > 0 else 0xAE  # VK_VOLUME_UP / VK_VOLUME_DOWN
    for _ in range(max(1, min(10, round(abs(steps))))):
        user32.keybd_event(key, 0, 0, 0)
        user32.keybd_event(key, 0, 2, 0)  # KEYEVENTF_KEYUP


def registry(path, name):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as key:
            return str(winreg.QueryValueEx(key, name)[0])
    except OSError:
        return ""


class MEMORYSTATUSEX(ctypes.Structure):
    _fields_ = [
        ("dwLength", wintypes.DWORD),
        ("dwMemoryLoad", wintypes.DWORD),
        ("ullTotalPhys", ctypes.c_ulonglong),
        ("ullAvailPhys", ctypes.c_ulonglong),
        ("ullTotalPageFile", ctypes.c_ulonglong),
        ("ullAvailPageFile", ctypes.c_ulonglong),
        ("ullTotalVirtual", ctypes.c_ulonglong),
        ("ullAvailVirtual", ctypes.c_ulonglong),
        ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
    ]


def cpu_times():
    idle, kernel, user = wintypes.FILETIME(), wintypes.FILETIME(), wintypes.FILETIME()
    ctypes.windll.kernel32.GetSystemTimes(ctypes.byref(idle), ctypes.byref(kernel), ctypes.byref(user))
    as_int = lambda ft: (ft.dwHighDateTime << 32) | ft.dwLowDateTime
    # o tempo de kernel já inclui o tempo ocioso
    return as_int(idle), as_int(kernel) + as_int(user)


def memory():
    stat = MEMORYSTATUSEX()
    stat.dwLength = ctypes.sizeof(stat)
    ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(stat))
    used = stat.ullTotalPhys - stat.ullAvailPhys
    return used, stat.ullTotalPhys


def gb(n):
    return n / 1024 ** 3


# ---------- bandeja do sistema e atalho de teclado ----------

WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, HANDLE, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM)


class WNDCLASSW(ctypes.Structure):
    _fields_ = [("style", wintypes.UINT), ("lpfnWndProc", WNDPROC), ("cbClsExtra", ctypes.c_int),
                ("cbWndExtra", ctypes.c_int), ("hInstance", HANDLE), ("hIcon", HANDLE),
                ("hCursor", HANDLE), ("hbrBackground", HANDLE), ("lpszMenuName", wintypes.LPCWSTR),
                ("lpszClassName", wintypes.LPCWSTR)]


class NOTIFYICONDATAW(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.DWORD), ("hWnd", HANDLE), ("uID", wintypes.UINT),
                ("uFlags", wintypes.UINT), ("uCallbackMessage", wintypes.UINT), ("hIcon", HANDLE),
                ("szTip", wintypes.WCHAR * 128), ("dwState", wintypes.DWORD),
                ("dwStateMask", wintypes.DWORD), ("szInfo", wintypes.WCHAR * 256),
                ("uVersion", wintypes.UINT), ("szInfoTitle", wintypes.WCHAR * 64),
                ("dwInfoFlags", wintypes.DWORD), ("guidItem", ctypes.c_byte * 16),
                ("hBalloonIcon", HANDLE)]


class SYSTEM_POWER_STATUS(ctypes.Structure):
    _fields_ = [("ACLineStatus", ctypes.c_ubyte), ("BatteryFlag", ctypes.c_ubyte),
                ("BatteryLifePercent", ctypes.c_ubyte), ("SystemStatusFlag", ctypes.c_ubyte),
                ("BatteryLifeTime", wintypes.DWORD), ("BatteryFullLifeTime", wintypes.DWORD)]


kernel32 = ctypes.windll.kernel32
shell32 = ctypes.windll.shell32
kernel32.GetModuleHandleW.restype = HANDLE
kernel32.GetModuleHandleW.argtypes = [wintypes.LPCWSTR]
user32.DefWindowProcW.restype = ctypes.c_ssize_t
user32.DefWindowProcW.argtypes = [HANDLE, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
user32.RegisterClassW.argtypes = [ctypes.POINTER(WNDCLASSW)]
user32.CreateWindowExW.restype = HANDLE
user32.CreateWindowExW.argtypes = [wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD,
                                   ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                   HANDLE, HANDLE, HANDLE, ctypes.c_void_p]
user32.DestroyWindow.argtypes = [HANDLE]
user32.PostMessageW.argtypes = [HANDLE, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
user32.GetMessageW.argtypes = [ctypes.POINTER(wintypes.MSG), HANDLE, wintypes.UINT, wintypes.UINT]
user32.TranslateMessage.argtypes = [ctypes.POINTER(wintypes.MSG)]
user32.DispatchMessageW.argtypes = [ctypes.POINTER(wintypes.MSG)]
user32.RegisterWindowMessageW.argtypes = [wintypes.LPCWSTR]
user32.RegisterHotKey.argtypes = [HANDLE, ctypes.c_int, wintypes.UINT, wintypes.UINT]
user32.UnregisterHotKey.argtypes = [HANDLE, ctypes.c_int]
user32.GetCursorPos.argtypes = [ctypes.POINTER(wintypes.POINT)]
user32.CreateIconFromResourceEx.restype = HANDLE
user32.CreateIconFromResourceEx.argtypes = [ctypes.c_char_p, wintypes.DWORD, wintypes.BOOL, wintypes.DWORD,
                                            ctypes.c_int, ctypes.c_int, wintypes.UINT]
user32.MonitorFromPoint.restype = HANDLE
user32.MonitorFromPoint.argtypes = [wintypes.POINT, wintypes.DWORD]
user32.SystemParametersInfoW.argtypes = [wintypes.UINT, wintypes.UINT, ctypes.c_void_p, wintypes.UINT]
shell32.Shell_NotifyIconW.argtypes = [wintypes.DWORD, ctypes.POINTER(NOTIFYICONDATAW)]
shell32.SHQueryUserNotificationState.argtypes = [ctypes.POINTER(ctypes.c_int)]
kernel32.GetSystemPowerStatus.argtypes = [ctypes.POINTER(SYSTEM_POWER_STATUS)]

# atalhos que dá para escolher: (rótulo, modificadores, tecla)
MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, MOD_NOREPEAT = 0x1, 0x2, 0x4, 0x8, 0x4000


# ---------- janelas comuns com visual escuro (DWM) ----------

dwmapi = ctypes.windll.dwmapi
dwmapi.DwmSetWindowAttribute.argtypes = [HANDLE, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD]
DWMWA_DARK, DWMWA_CORNERS, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR = 20, 33, 34, 35, 36


def dwm_attr(hwnd, attr, value):
    v = wintypes.DWORD(value)
    try:
        dwmapi.DwmSetWindowAttribute(hwnd, attr, ctypes.byref(v), ctypes.sizeof(v))
    except OSError:
        pass  # Windows antigo: fica com o visual padrão


def colorref(rgb):
    r, g, b = rgb[:3]
    return r | g << 8 | b << 16


def style_window(hwnd, caption=None, border=None):
    """Barra de título escura (da cor do fundo, no Windows 11) e cantos arredondados."""
    dwm_attr(hwnd, DWMWA_DARK, 1)
    dwm_attr(hwnd, DWMWA_CORNERS, 2)  # DWMWCP_ROUND
    if caption:
        dwm_attr(hwnd, DWMWA_CAPTION_COLOR, colorref(caption))
        dwm_attr(hwnd, DWMWA_TEXT_COLOR, colorref((242, 242, 245)))
    if border:
        dwm_attr(hwnd, DWMWA_BORDER_COLOR, colorref(border))
