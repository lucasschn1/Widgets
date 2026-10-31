"""Menu de contexto no estilo de vidro dos widgets."""

import tkinter as tk
from types import SimpleNamespace

from PIL import Image, ImageDraw

from deskwidgets.theme import FG, MUTED, feedback_shape, font
from deskwidgets.win32 import HWND_TOPMOST, area_for, blit, make_layered, set_z, user32


class GlassMenu:
    """Menu no estilo dos widgets (vidro escuro, cantos arredondados), com submenus.

    Itens: ("cmd", rótulo, função[, atalho]), ("check", rótulo, marcado, função),
    ("radio", rótulo, escolhido, função), ("sub", rótulo, itens) e None (separador).
    """
    ITEM_H, SEP_H, PAD, MIN_W = 30, 9, 6, 210
    BG = (36, 36, 42, 244)

    def __init__(self, app):
        self.app = app
        self.levels = []
        self._watch_job = None
        self._armed = False

    @property
    def open(self):
        return bool(self.levels)

    def popup(self, entries, x, y):
        self.close()
        self._armed = False  # o botão que abriu o menu ainda pode estar apertado
        self._open_level(entries, x, y)
        self._watch_job = self.app.root.after(40, self._watch)

    def close(self, depth=0):
        while len(self.levels) > depth:
            self.levels.pop().win.destroy()
        if not self.levels and self._watch_job:
            self.app.root.after_cancel(self._watch_job)
            self._watch_job = None

    def _open_level(self, entries, x, y, parent=None):
        s = self.app.s
        f, fa = font("segoeui.ttf", round(12 * s)), font("segoeui.ttf", round(11 * s))
        pad, ih, sh = round(self.PAD * s), round(self.ITEM_H * s), round(self.SEP_H * s)
        left_col, right_col = round(30 * s), round(30 * s)
        widths = [f.getlength(e[1]) + (fa.getlength(e[3]) + 24 * s if e[0] == "cmd" and len(e) > 3 and e[3] else 0)
                  for e in entries if e]
        w = max(round(self.MIN_W * s), round(left_col + max(widths, default=0) + right_col))
        rects, top = [], pad
        for e in entries:
            height = sh if e is None else ih
            rects.append((pad, top, w - pad, top + height))
            top += height
        h = top + pad
        area = area_for((x, y, x + 1, y + 1))
        if parent:  # submenu ao lado do item, para a direita se couber
            px0, py0, px1, _ = parent
            x, y = px1 - round(4 * s), py0 - pad
            if x + w > area[2]:
                x = px0 - w + round(4 * s)
        elif x + w > area[2]:
            x -= w
        if y + h > area[3]:
            y = (y - h) if not parent else area[3] - h
        x, y = max(area[0], x), max(area[1], y)

        win = tk.Toplevel(self.app.root)
        win.overrideredirect(True)
        win.attributes("-alpha", 0.0)
        win.geometry(f"1x1+{x}+{y}")
        win.update()
        level = SimpleNamespace(win=win, hwnd=user32.GetParent(win.winfo_id()), entries=entries,
                                rects=rects, pos=(x, y), size=(w, h), hot=None,
                                fonts=(f, fa), cols=(left_col, right_col))
        make_layered(level.hwnd)
        set_z(level.hwnd, HWND_TOPMOST)
        win.bind("<Motion>", lambda e, lv=level: self._motion(lv, e.x, e.y))
        win.bind("<ButtonRelease-1>", lambda e, lv=level: self._click(lv, e.x, e.y))
        for key in ("<Escape>", "<Up>", "<Down>", "<Return>", "<Left>", "<Right>"):
            win.bind(key, self._key)
        self.levels.append(level)
        self._draw(level)
        if not parent:
            win.focus_force()
            user32.SetForegroundWindow(level.hwnd)

    def _draw(self, level):
        s = self.app.s
        w, h = level.size
        k = 2
        big = Image.new("RGBA", (w * k, h * k), (0, 0, 0, 0))
        ImageDraw.Draw(big).rounded_rectangle((0, 0, w * k - 1, h * k - 1), radius=12 * s * k, fill=self.BG,
                                              outline=(255, 255, 255, 34), width=max(1, round(s * k)))
        img = big.resize((w, h), Image.LANCZOS)
        d = ImageDraw.Draw(img, "RGBA")
        f, fa = level.fonts
        left_col, right_col = level.cols
        for i, (entry, (x0, y0, x1, y1)) in enumerate(zip(level.entries, level.rects)):
            if entry is None:
                ym = (y0 + y1) // 2
                d.line((x0 + 8 * s, ym, x1 - 8 * s, ym), fill=(66, 66, 74, 244), width=max(1, round(s)))
                continue
            if i == level.hot:
                img.alpha_composite(feedback_shape(x1 - x0, y1 - y0, round(7 * s), (255, 255, 255, 30)), (x0, y0))
            kind, label = entry[0], entry[1]
            ym = (y0 + y1) / 2
            d.text((x0 + left_col - round(2 * s), ym), label, font=f, fill=FG, anchor="lm")
            cx = x0 + left_col / 2
            if kind == "check" and entry[2]:
                d.line([(cx - 5 * s, ym), (cx - 1.5 * s, ym + 3.5 * s), (cx + 5 * s, ym - 4 * s)],
                       fill=FG, width=max(1, round(1.6 * s)), joint="curve")
            elif kind == "radio" and entry[2]:
                r = 3.2 * s
                d.ellipse((cx - r, ym - r, cx + r, ym + r), fill=FG)
            elif kind == "sub":
                ax = x1 - right_col / 2
                d.line([(ax - 2 * s, ym - 4.5 * s), (ax + 2.5 * s, ym), (ax - 2 * s, ym + 4.5 * s)],
                       fill=(176, 176, 184, 255), width=max(1, round(1.5 * s)), joint="curve")
            elif kind == "cmd" and len(entry) > 3 and entry[3]:
                d.text((x1 - 10 * s, ym), entry[3], font=fa, fill=MUTED, anchor="rm")
        blit(level.hwnd, img, level.pos)

    def _index_at(self, level, x, y):
        for i, (entry, (x0, y0, x1, y1)) in enumerate(zip(level.entries, level.rects)):
            if entry is not None and x0 <= x < x1 and y0 <= y < y1:
                return i
        return None

    def _set_hot(self, level, index):
        depth = self.levels.index(level)
        if index == level.hot:
            return
        level.hot = index
        self.close(depth + 1)
        self._draw(level)
        if index is not None and level.entries[index][0] == "sub":
            x0, y0, x1, y1 = level.rects[index]
            lx, ly = level.pos
            self._open_level(level.entries[index][2], 0, 0, parent=(lx + x0, ly + y0, lx + x1, ly + y1))

    def _motion(self, level, x, y):
        if level in self.levels:
            index = self._index_at(level, x, y)
            if index is not None or len(self.levels) == self.levels.index(level) + 1:
                self._set_hot(level, index)

    def _click(self, level, x, y):
        if level not in self.levels:
            return
        index = self._index_at(level, x, y)
        if index is not None:
            self._activate(level, index)

    def _activate(self, level, index):
        entry = level.entries[index]
        if entry[0] == "sub":
            self._set_hot(level, index)
            return
        callback = entry[2] if entry[0] == "cmd" else entry[3]
        self.close()
        self.app.root.after(1, callback)

    def _key(self, e):
        if not self.levels:
            return
        level = self.levels[-1]
        items = [i for i, entry in enumerate(level.entries) if entry is not None]
        if e.keysym == "Escape":
            self.close()
        elif e.keysym in ("Up", "Down"):
            pos = items.index(level.hot) if level.hot in items else -1
            pos = (pos + (1 if e.keysym == "Down" else -1)) % len(items)
            level.hot = items[pos]
            self._draw(level)
        elif e.keysym in ("Return", "Right") and level.hot is not None:
            entry = level.entries[level.hot]
            if entry[0] == "sub":
                hot, level.hot = level.hot, None
                self._set_hot(level, hot)
                sub = self.levels[-1]
                sub.hot = next(i for i, x in enumerate(sub.entries) if x is not None)
                self._draw(sub)
            elif e.keysym == "Return":
                self._activate(level, level.hot)
        elif e.keysym == "Left" and len(self.levels) > 1:
            self.close(len(self.levels) - 1)

    def _watch(self):
        """Fecha o menu ao clicar fora dele."""
        self._watch_job = None
        if not self.levels:
            return
        pressed = any(user32.GetAsyncKeyState(vk) & 0x8000 for vk in (0x01, 0x02, 0x04))
        if not pressed:
            self._armed = True
        elif self._armed:
            px, py = self.app.root.winfo_pointerxy()
            inside = any(lv.pos[0] <= px < lv.pos[0] + lv.size[0] and lv.pos[1] <= py < lv.pos[1] + lv.size[1]
                         for lv in self.levels)
            if not inside:
                self.close()
                return
        self._watch_job = self.app.root.after(40, self._watch)
