"""Ícone na bandeja do sistema, notificações e atalhos globais de teclado."""

import ctypes
import io
import threading
from ctypes import wintypes

from PIL import Image, ImageDraw

from deskwidgets.win32 import (
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, NOTIFYICONDATAW, WNDCLASSW, WNDPROC, kernel32,
    shell32, user32,
)


# alternativas para "mostrar na frente", se o atalho escolhido já for de outro programa
FALLBACKS = ["Ctrl+Alt+W", "Win+Alt+W", "Ctrl+Shift+W", "Ctrl+Alt+Espaço"]
MODIFIERS = (("Ctrl", MOD_CONTROL), ("Alt", MOD_ALT), ("Shift", MOD_SHIFT), ("Win", MOD_WIN))
KEY_NAMES = {0x20: "Espaço", 0x0D: "Enter", 0x09: "Tab", 0x25: "Esquerda", 0x26: "Cima", 0x27: "Direita",
             0x28: "Baixo", 0x2D: "Insert", 0x2E: "Delete", 0x24: "Home", 0x23: "End", 0x21: "PageUp",
             0x22: "PageDown", 0xBC: ",", 0xBE: ".", 0xBD: "-", 0xBB: "=", 0xC0: "'", 0xDE: "~", 0xBA: "Ç",
             0xBF: ";", 0xDB: "´", 0xDD: "[", 0xDC: "]", 0xE2: "\\"}
NAMED_KEYS = {name: vk for vk, name in KEY_NAMES.items()}
HOTKEY_IDS = {"peek": 1, "pomodoro": 2}  # nome do atalho: id no RegisterHotKey


def key_name(vk):
    if 0x30 <= vk <= 0x39 or 0x41 <= vk <= 0x5A:
        return chr(vk)
    if 0x70 <= vk <= 0x87:
        return f"F{vk - 0x6F}"
    if 0x60 <= vk <= 0x69:
        return f"Num{vk - 0x60}"
    return KEY_NAMES.get(vk)


def parse_hotkey(label):
    """"Ctrl+Alt+W" -> (modificadores, tecla virtual); None se não der para entender."""
    if not label:
        return None
    *mods, key = label.split("+")
    flags = 0
    for mod in mods:
        flag = dict(MODIFIERS).get(mod)
        if flag is None:
            return None
        flags |= flag
    if len(key) == 1 and key.isascii() and key.isalnum():
        vk = ord(key.upper())
    elif key.startswith("F") and key[1:].isdigit():
        vk = 0x6F + int(key[1:])
    elif key.startswith("Num") and key[3:].isdigit():
        vk = 0x60 + int(key[3:])
    else:
        vk = NAMED_KEYS.get(key)
    return (flags, vk) if vk else None


def hotkey_label(flags, vk):
    name = key_name(vk)
    if not name:
        return None
    return "+".join([n for n, flag in MODIFIERS if flags & flag] + [name])


def hotkey_from_event(e):
    """Atalho da tecla apertada numa janela do Tk; None se ainda falta a tecla (só modificadores)."""
    vk = e.keycode
    if vk in (0x10, 0x11, 0x12, 0x5B, 0x5C, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5):
        return None  # só um modificador por enquanto
    flags = 0
    if e.state & 0x4:
        flags |= MOD_CONTROL
    if e.state & 0x20000:
        flags |= MOD_ALT
    if e.state & 0x1:
        flags |= MOD_SHIFT
    if user32.GetAsyncKeyState(0x5B) & 0x8000 or user32.GetAsyncKeyState(0x5C) & 0x8000:
        flags |= MOD_WIN
    is_function = 0x70 <= vk <= 0x87
    if not (flags & (MOD_CONTROL | MOD_ALT | MOD_WIN)) and not is_function:
        return None  # sem Ctrl, Alt ou Win o atalho atrapalharia a digitação
    return hotkey_label(flags, vk)


def tray_icon_image(px):
    """Ícone da bandeja: um quadrado com gradiente e três "widgets" brancos."""
    k = 8
    big = Image.new("RGBA", (16 * k, 16 * k), (0, 0, 0, 0))
    mask = Image.new("L", big.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, 16 * k - 1, 16 * k - 1), radius=3.6 * k, fill=255)
    grad = Image.linear_gradient("L").resize(big.size)
    color = Image.composite(Image.new("RGBA", big.size, (191, 90, 242, 255)),
                            Image.new("RGBA", big.size, (10, 132, 255, 255)), grad)
    big.paste(color, (0, 0), mask)
    d = ImageDraw.Draw(big)
    for box in ((3, 3, 13, 7), (3, 9, 7.4, 13), (8.6, 9, 13, 13)):
        d.rounded_rectangle(tuple(v * k for v in box), radius=1.2 * k, fill=(255, 255, 255, 240))
    return big.resize((px, px), Image.LANCZOS)


class Tray(threading.Thread):
    """Ícone na bandeja do sistema, notificações e atalho global de teclado.

    Roda numa thread com o laço de mensagens do Windows; cliques e o atalho viram
    eventos numa fila, que o App consome na thread do Tk.
    """
    WM_TRAY, WM_SET_HOTKEY, WM_PAUSE_HOTKEYS = 0x8001, 0x8002, 0x8003  # WM_APP + 1, + 2, + 3
    WM_OTHER_INSTANCE = 0x8004  # alguém abriu o app de novo: este, que já está aberto, se mostra
    CLASS_NAME = "DesktopWidgetsTray"
    WM_CLOSE, WM_DESTROY, WM_HOTKEY = 0x10, 0x2, 0x312
    WM_LBUTTONUP, WM_RBUTTONUP, NIN_BALLOONUSERCLICK = 0x202, 0x205, 0x405
    TIP = "Widgets da área de trabalho"

    def __init__(self, events, hotkeys):
        super().__init__(daemon=True)
        self.events = events
        self.wanted = dict(hotkeys)  # nome: atalho escolhido ("" = nenhum)
        self.hwnd = None
        self.hotkeys = {}  # nome: atalho que deu para registrar (outro programa pode já usar)
        self.ok = False
        self.ready = threading.Event()
        self._taskbar_created = None
        self._nid = None

    def run(self):
        try:
            self._start()
        finally:
            self.ready.set()
        msg = wintypes.MSG()
        while self.hwnd and user32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))

    def _start(self):
        instance = kernel32.GetModuleHandleW(None)
        self._proc = WNDPROC(self._wndproc)  # guardado: o Windows chama de volta
        wc = WNDCLASSW(lpfnWndProc=self._proc, hInstance=instance, lpszClassName=self.CLASS_NAME)
        user32.RegisterClassW(ctypes.byref(wc))
        self.hwnd = user32.CreateWindowExW(0, self.CLASS_NAME, "Widgets", 0, 0, 0, 0, 0,
                                           None, None, instance, None)
        # o Explorer avisa com esta mensagem quando reinicia; aí o ícone precisa voltar
        self._taskbar_created = user32.RegisterWindowMessageW("TaskbarCreated")
        px = user32.GetSystemMetrics(49)  # SM_CXSMICON, já na escala da tela
        buf = io.BytesIO()
        tray_icon_image(px).save(buf, "PNG")
        data = buf.getvalue()
        icon = user32.CreateIconFromResourceEx(data, len(data), True, 0x00030000, px, px, 0)
        nid = self._nid = NOTIFYICONDATAW()
        nid.cbSize = ctypes.sizeof(nid)
        nid.hWnd, nid.uID = self.hwnd, 1
        nid.uFlags = 0x1 | 0x2 | 0x4  # NIF_MESSAGE | NIF_ICON | NIF_TIP
        nid.uCallbackMessage, nid.hIcon = self.WM_TRAY, icon
        nid.szTip = self.TIP
        self.ok = bool(shell32.Shell_NotifyIconW(0, ctypes.byref(nid)))  # NIM_ADD
        for name in HOTKEY_IDS:
            self._register(name, fallback=name == "peek")

    @property
    def hotkey(self):
        """O atalho de "mostrar na frente" (o principal)."""
        return self.hotkeys.get("peek")

    def _register(self, name, fallback=False):
        """Registra o atalho `name`; com `fallback`, tenta os alternativos se ele já estiver em uso."""
        hid = HOTKEY_IDS[name]
        user32.UnregisterHotKey(self.hwnd, hid)
        self.hotkeys[name] = None
        choices = [self.wanted.get(name)]
        if fallback:
            choices += [h for h in FALLBACKS if h != choices[0]]
        for label in choices:
            parsed = parse_hotkey(label)
            if parsed and user32.RegisterHotKey(self.hwnd, hid, parsed[0] | MOD_NOREPEAT, parsed[1]):
                self.hotkeys[name] = label
                break

    def _wndproc(self, hwnd, msg, wparam, lparam):
        if msg == self.WM_TRAY:
            if lparam == self.WM_LBUTTONUP:
                self.events.put(("peek", None))
            elif lparam == self.WM_RBUTTONUP:
                self.events.put(("tray_menu", None))
            elif lparam == self.NIN_BALLOONUSERCLICK:
                self.events.put(("notification", None))
            return 0
        if msg == self.WM_HOTKEY:
            name = next((n for n, i in HOTKEY_IDS.items() if i == wparam), None)
            if name:
                self.events.put(("hotkey", name))
            return 0
        if msg == self.WM_SET_HOTKEY:
            name = next(n for n, i in HOTKEY_IDS.items() if i == wparam)
            self._register(name)
            self.events.put(("hotkey_set", (name, self.hotkeys.get(name))))
            return 0
        if msg == self.WM_OTHER_INSTANCE:
            self.events.put(("other_instance", None))
            return 0
        if msg == self.WM_PAUSE_HOTKEYS:
            for name, hid in HOTKEY_IDS.items():
                if wparam:
                    user32.UnregisterHotKey(hwnd, hid)
                else:  # volta exatamente os que estavam valendo antes da pausa
                    parsed = parse_hotkey(self.hotkeys.get(name))
                    if parsed:
                        user32.RegisterHotKey(hwnd, hid, parsed[0] | MOD_NOREPEAT, parsed[1])
            return 0
        if msg and msg == self._taskbar_created:
            shell32.Shell_NotifyIconW(0, ctypes.byref(self._nid))
            return 0
        if msg == self.WM_CLOSE:
            for hid in HOTKEY_IDS.values():
                user32.UnregisterHotKey(hwnd, hid)
            user32.DestroyWindow(hwnd)
            return 0
        if msg == self.WM_DESTROY:
            user32.PostQuitMessage(0)
            return 0
        return user32.DefWindowProcW(hwnd, msg, wparam, lparam)

    def set_hotkey(self, name, label):
        """Troca um atalho (o resultado chega como evento "hotkey_set")."""
        self.wanted[name] = label
        if self.hwnd:
            user32.PostMessageW(self.hwnd, self.WM_SET_HOTKEY, HOTKEY_IDS[name], 0)

    def pause_hotkeys(self, on):
        """Desliga os atalhos enquanto o usuário grava um novo (senão o Windows os engole)."""
        if self.hwnd:
            user32.PostMessageW(self.hwnd, self.WM_PAUSE_HOTKEYS, 1 if on else 0, 0)

    def set_tip(self, text):
        if self._nid is not None:
            self._nid.uFlags = 0x4  # NIF_TIP
            self._nid.szTip = text[:127]
            shell32.Shell_NotifyIconW(1, ctypes.byref(self._nid))  # NIM_MODIFY

    def notify(self, title, text):
        """Notificação do Windows saindo do ícone da bandeja."""
        if self._nid is None:
            return
        self._nid.uFlags = 0x10  # NIF_INFO
        self._nid.szInfoTitle = title[:63]
        self._nid.szInfo = text[:255]
        self._nid.dwInfoFlags = 0x4 | 0x20  # NIIF_USER (ícone do app) | NIIF_LARGE_ICON
        shell32.Shell_NotifyIconW(1, ctypes.byref(self._nid))

    def stop(self):
        if self._nid is not None:
            shell32.Shell_NotifyIconW(2, ctypes.byref(self._nid))  # NIM_DELETE, já, antes de sair
        if self.hwnd:
            user32.PostMessageW(self.hwnd, self.WM_CLOSE, 0, 0)
