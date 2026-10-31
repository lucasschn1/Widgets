"""Pílulas de vidro escuro acima das janelas: avisos rápidos (com "Desfazer") e a barra do modo edição."""

import time
import tkinter as tk

from PIL import Image, ImageDraw, ImageFilter

from deskwidgets.theme import ACCENT, ACCENT_HOVER, FG, MUTED, ease_in_out, font, icon_font, ICONS
from deskwidgets.win32 import HWND_TOPMOST, blit, make_layered, set_z, user32, work_areas

SUCCESS = (48, 209, 88)


class GlassBar:
    """Pílula com ícone, textos e botões. `parts`:
    ("icon", nome, cor), ("text", texto, negrito), ("muted", texto) e ("button", rótulo, função, principal).
    """
    H, PAD, GAP, SHADOW = 46, 16, 10, 14
    FADE_MS = 160

    def __init__(self, app):
        self.app = app
        self.win = None
        self.parts = []
        self.buttons = []       # (x0, y0, x1, y1, função) em pixels da janela
        self.hot = None
        self.pos = (0, 0)
        self.visible = False
        self._alpha = 0.0
        self._fade_job = None
        self._img = None

    # --- para as subclasses ---

    def anchor(self, w, h):
        """Canto superior esquerdo da janela (pixels da tela) para o tamanho dado."""
        raise NotImplementedError

    # --- janela ---

    def _window(self):
        if self.win is None:
            win = self.win = tk.Toplevel(self.app.root)
            win.overrideredirect(True)
            win.attributes("-alpha", 0.0)
            win.geometry("1x1+-100+-100")
            win.update()
            self.hwnd = user32.GetParent(win.winfo_id())
            make_layered(self.hwnd)
            win.bind("<Motion>", self._motion)
            win.bind("<Leave>", lambda _e: self._set_hot(None))
            win.bind("<ButtonRelease-1>", self._click)
            win.bind("<Escape>", lambda _e: self.on_escape())
        return self.win

    def on_escape(self):
        self.hide()

    def show(self, parts):
        self.parts = parts
        self.hot = None
        self._window()
        self._render()
        set_z(self.hwnd, HWND_TOPMOST)
        if not self.visible:
            self.visible = True
            self._fade(1.0)
        else:
            self._present()

    def hide(self):
        if self.visible:
            self.visible = False
            self._fade(0.0)

    def _render(self):
        s = self.app.s
        k = 2
        pad, gap, h, sh = self.PAD * s, self.GAP * s, round(self.H * s), round(self.SHADOW * s)
        bold, regular = font("seguisb.ttf", round(12.5 * s)), font("segoeui.ttf", round(12.5 * s))
        icons = icon_font(round(15 * s))
        # medidas de cada parte
        layout, x = [], pad
        for part in self.parts:
            kind = part[0]
            if kind == "icon":
                w = 18 * s if icons else 0
            elif kind in ("text", "muted"):
                w = (bold if kind == "text" and part[2] else regular).getlength(part[1])
            else:
                w = bold.getlength(part[1]) + 28 * s
            layout.append((part, x, w))
            x += w + gap
        w = round(x - gap + pad)
        W, H = w + 2 * sh, h + 2 * sh
        img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        # sombra suave embaixo da pílula
        shadow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        ImageDraw.Draw(shadow).rounded_rectangle((sh, sh + 4 * s, sh + w, sh + h + 4 * s), radius=h / 2,
                                                 fill=(0, 0, 0, 110))
        img.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(8 * s)))
        big = Image.new("RGBA", (w * k, h * k), (0, 0, 0, 0))
        d = ImageDraw.Draw(big)
        d.rounded_rectangle((0, 0, w * k - 1, h * k - 1), radius=h * k / 2, fill=(34, 34, 40, 246),
                            outline=(255, 255, 255, 40), width=max(1, round(s * k)))
        self.buttons = []
        bh = 30 * s
        for part, px, pw in layout:
            if part[0] == "button":
                primary = part[3]
                hot = self.hot == len(self.buttons)
                if primary:
                    fill = (ACCENT_HOVER if hot else ACCENT) + (255,)
                else:
                    fill = (255, 255, 255, 40 if hot else 22)
                y0 = (h - bh) / 2
                d.rounded_rectangle((px * k, y0 * k, (px + pw) * k, (y0 + bh) * k), radius=bh * k / 2, fill=fill)
                self.buttons.append((sh + px, sh + y0, sh + px + pw, sh + y0 + bh, part[2]))
        pill = big.resize((w, h), Image.LANCZOS)
        d = ImageDraw.Draw(pill)
        cy = h / 2
        for part, px, pw in layout:
            kind = part[0]
            if kind == "icon" and icons:
                d.text((px + pw / 2, cy), ICONS.get(part[1], part[1]), font=icons, fill=part[2] + (255,),
                       anchor="mm")
            elif kind == "text":
                f = bold if part[2] else regular
                d.text((round(px), round(cy)), part[1], font=f, fill=FG, anchor="lm")
            elif kind == "muted":
                d.text((round(px), round(cy)), part[1], font=regular, fill=MUTED, anchor="lm")
            elif kind == "button":
                d.text((px + pw / 2, cy), part[1], font=bold, fill=FG, anchor="mm")
        img.alpha_composite(pill, (sh, sh))
        self._img = img
        self.pos = self.anchor(W, H)

    def _present(self, offset=0):
        if self._img is not None:
            x, y = self.pos
            blit(self.hwnd, self._img, (x, y + offset), round(255 * self._alpha))

    def _fade(self, target):
        if self._fade_job:
            self.app.root.after_cancel(self._fade_job)
        start, t0 = self._alpha, time.perf_counter()
        duration = self.FADE_MS if self.app.animations else 0

        def step():
            t = min(1.0, (time.perf_counter() - t0) * 1000 / duration) if duration else 1.0
            self._alpha = start + (target - start) * ease_in_out(t)
            self._present(round((1 - self._alpha) * 10 * self.app.s))
            if t < 1:
                self._fade_job = self.app.root.after(16, step)
            else:
                self._fade_job = None
                if target == 0:  # escondida: janela vazia fora da tela
                    blit(self.hwnd, Image.new("RGBA", (1, 1), (0, 0, 0, 0)), (-10, -10))

        step()

    # --- mouse ---

    def _button_at(self, x, y):
        for i, (x0, y0, x1, y1, _fn) in enumerate(self.buttons):
            if x0 <= x < x1 and y0 <= y < y1:
                return i
        return None

    def _set_hot(self, index):
        if index != self.hot and self.visible:
            self.hot = index
            self.win.config(cursor="hand2" if index is not None else "")
            self._render()
            self._present()

    def _motion(self, e):
        self._set_hot(self._button_at(e.x, e.y))

    def _click(self, e):
        index = self._button_at(e.x, e.y)
        if index is not None and self.visible:
            callback = self.buttons[index][4]
            self.app.root.after(1, callback)


class Toast(GlassBar):
    """Aviso rápido na parte de baixo da tela, que some sozinho (o mouse em cima segura)."""
    SECONDS, ACTION_SECONDS = 3.5, 6

    def __init__(self, app):
        super().__init__(app)
        self._job = None

    def anchor(self, w, h):
        left, _top, right, bottom = work_areas()[0]
        return (left + right - w) // 2, bottom - h - round(18 * self.app.s)

    def notify(self, text, action_label=None, action=None, icon="check", color=SUCCESS, detail=None):
        parts = [("icon", icon, color), ("text", text, True)]
        if detail:
            parts.append(("muted", detail))
        if action:
            def run():
                self.hide()
                action()
            parts.append(("button", action_label or "Desfazer", run, False))
        self.show(parts)
        self._schedule(self.ACTION_SECONDS if action else self.SECONDS)

    def _schedule(self, seconds):
        if self._job:
            self.app.root.after_cancel(self._job)
        self._job = self.app.root.after(round(seconds * 1000), self._expire)

    def _expire(self):
        self._job = None
        px, py = self.app.root.winfo_pointerxy()
        x, y = self.pos
        if self._img and x <= px < x + self._img.width and y <= py < y + self._img.height:
            self._schedule(1.5)  # o mouse está em cima: espera mais um pouco
        else:
            self.hide()


class EditBar(GlassBar):
    """Barra no topo da tela durante o modo edição dos widgets."""

    def anchor(self, w, h):
        left, top, right, _bottom = work_areas()[0]
        return (left + right - w) // 2, top + round(10 * self.app.s)

    def open(self):
        self.show([
            ("icon", "edit", (255, 255, 255)),
            ("text", "Editando widgets", True),
            ("muted", "Arraste para mover  ·  −  fecha  ·  +  muda o tamanho"),
            ("button", "Adicionar widget", lambda: self.app.open_settings("widgets"), False),
            ("button", "Concluir", self.app.end_edit, True),
        ])
        self.win.focus_force()

    def on_escape(self):
        self.app.end_edit()
