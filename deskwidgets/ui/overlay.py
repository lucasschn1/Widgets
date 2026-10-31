"""Janelas por cima de tudo que deixam os cliques passarem: guias de encaixe e dicas."""

import time
import tkinter as tk

from PIL import Image, ImageDraw

from deskwidgets.theme import FG, MUTED, TIP_DELAY_MS, font
from deskwidgets.win32 import HWND_TOPMOST, blit, make_layered, set_z, user32, work_areas


# ---------- janelas por cima de tudo: guias e dicas ----------

class OverlayWindow:
    """Janela transparente acima das outras, que deixa os cliques passarem."""

    def __init__(self, root):
        self.win = tk.Toplevel(root)
        self.win.overrideredirect(True)
        self.win.attributes("-alpha", 0.0)
        self.win.geometry("1x1+0+0")
        self.win.update()
        self.hwnd = user32.GetParent(self.win.winfo_id())
        make_layered(self.hwnd, click_through=True)
        # "escondida" é vazia e transparente: o Windows não atualiza janelas assim ocultas
        self.visible = True
        self.hide()

    def show(self, img, pos):
        blit(self.hwnd, img, pos)
        if not self.visible:
            set_z(self.hwnd, HWND_TOPMOST)
            self.visible = True

    def hide(self):
        if self.visible:
            blit(self.hwnd, Image.new("RGBA", (1, 1), (0, 0, 0, 0)), (-10, -10))
            self.visible = False


class Guides:
    """Linhas finas que mostram com o que o widget arrastado está alinhado."""
    COLOR = (255, 255, 255, 190)

    def __init__(self, root, s):
        self.s = s
        self.lines = {"v": OverlayWindow(root), "h": OverlayWindow(root)}

    def show(self, guides):
        """`guides`: {"v": (x, y0, y1), "h": (y, x0, x1)}, em pixels da tela."""
        width = max(1, round(1.5 * self.s))
        for axis, window in self.lines.items():
            guide = guides.get(axis)
            if not guide:
                window.hide()
                continue
            at, start, end = (round(v) for v in guide)
            length = max(1, end - start)
            if axis == "v":
                window.show(Image.new("RGBA", (width, length), self.COLOR), (at - width // 2, start))
            else:
                window.show(Image.new("RGBA", (length, width), self.COLOR), (start, at - width // 2))

    def hide(self):
        for window in self.lines.values():
            window.hide()


class Tooltip:
    """Dica que aparece perto do mouse depois de um instante parado sobre algo."""

    def __init__(self, root, s):
        self.root = root
        self.s = s
        self.window = OverlayWindow(root)
        self.text = None
        self._job = None
        self._hidden_at = 0.0

    def request(self, text, x, y):
        """Pede a dica `text` para o mouse em (x, y) da tela; None esconde."""
        if text == self.text:
            return
        self.text = text
        if self._job:
            self.root.after_cancel(self._job)
            self._job = None
        if self.window.visible:
            self.window.hide()
            self._hidden_at = time.perf_counter()
        if text:
            # logo depois de outra dica, a próxima aparece sem esperar
            warm = time.perf_counter() - self._hidden_at < 0.4
            self._job = self.root.after(0 if warm else TIP_DELAY_MS, lambda: self._show(text, x, y))

    def hide(self):
        self.request(None, 0, 0)
        self.window.hide()

    def _show(self, text, x, y):
        self._job = None
        s = self.s
        # o título (até a primeira linha vazia, ou só a primeira linha) vai em negrito
        head, _, tail = text.partition("\n\n") if "\n\n" in text else text.partition("\n")
        bold, muted = head.split("\n"), tail.split("\n") if tail else []
        lines = bold + muted
        faces = [font("seguisb.ttf", round(11.5 * s))] * len(bold) + [font("segoeui.ttf", round(11 * s))] * len(muted)
        pad_x, pad_y, line_h = round(10 * s), round(6 * s), round(17 * s)
        w = round(max(f.getlength(line) for f, line in zip(faces, lines))) + 2 * pad_x
        h = line_h * len(lines) + 2 * pad_y - round(2 * s)
        k = 2
        big = Image.new("RGBA", (w * k, h * k), (0, 0, 0, 0))
        ImageDraw.Draw(big).rounded_rectangle((0, 0, w * k - 1, h * k - 1), radius=8 * s * k,
                                              fill=(34, 34, 40, 240), outline=(255, 255, 255, 40),
                                              width=max(1, round(s * k)))
        img = big.resize((w, h), Image.LANCZOS)
        d = ImageDraw.Draw(img, "RGBA")
        for i, (f, line) in enumerate(zip(faces, lines)):
            d.text((pad_x, pad_y + i * line_h + f.getmetrics()[0]), line, font=f,
                   fill=FG if i < len(bold) else MUTED, anchor="ls")
        # acima do mouse, centralizada; embaixo se não couber
        area = next((wa for wa in work_areas() if wa[0] <= x < wa[2] and wa[1] <= y < wa[3]), work_areas()[0])
        px = min(max(x - w // 2, area[0] + 4), area[2] - w - 4)
        py = y - h - round(14 * s)
        if py < area[1]:
            py = y + round(22 * s)
        self.window.show(img, (px, py))
