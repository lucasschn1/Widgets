"""Pergunta de vidro (um campo de texto), aberta junto do widget; substitui as caixas do Windows 95.

Também tem a busca de cidades com sugestões enquanto se digita, usada aqui e nas configurações.
"""

import threading

from deskwidgets.theme import ACCENT, FG, MUTED
from deskwidgets.ui.panel import Panel
from deskwidgets.win32 import work_areas


class CitySuggest:
    """Sugestões de cidades enquanto se digita (busca em segundo plano, depois de uma pausa)."""
    DELAY_MS, ROW_H = 350, 38

    def __init__(self, panel):
        self.panel = panel
        self.results = []
        self.index = -1
        self.query = ""
        self.busy = False
        self._job = None
        self._seq = 0
        self._result = None

    def typed(self, text):
        self.query = text.strip()
        if self._job:
            self.panel.win.after_cancel(self._job)
            self._job = None
        if len(self.query) < 2:
            self.clear()
            return
        self._job = self.panel.win.after(self.DELAY_MS, self._search)

    def clear(self):
        if self.results or self.busy:
            self.results, self.index, self.busy = [], -1, False
            self._seq += 1
            self.panel.request()

    def _search(self):
        from deskwidgets.widgets.system import search_cities
        self._job = None
        self._seq += 1
        seq, query = self._seq, self.query
        self.busy = True
        self.panel.request()

        def work():
            try:
                found = search_cities(query)
            except Exception:
                found = []
            self._result = (seq, found)

        threading.Thread(target=work, daemon=True).start()
        self.panel.win.after(120, self._poll)

    def _poll(self):
        if not self.panel.alive:
            return
        if self._result is None:
            self.panel.win.after(120, self._poll)
            return
        seq, found = self._result
        self._result = None
        if seq == self._seq:
            self.results, self.index, self.busy = found, -1, False
            self.panel.request()

    def key(self, keysym, on_pick):
        """Setas escolhem, Enter confirma e Esc fecha a lista; devolve True se usou a tecla."""
        if not self.results:
            return False
        if keysym in ("Down", "Up"):
            step = 1 if keysym == "Down" else -1
            self.index = (self.index + step) % len(self.results)
            self.panel.request()
            return True
        if keysym == "Return" and self.index >= 0:
            on_pick(self.results[self.index])
            return True
        if keysym == "Escape":
            self.clear()
            return True
        return False

    def height(self):
        return len(self.results) * self.ROW_H + 8 if self.results else 0

    def draw(self, ui, x, y, w, on_pick):
        """Lista de sugestões com o canto em (x, y); devolve a altura usada."""
        if not self.results:
            if self.busy:
                ui.text(x + w - 12, y - 22, "buscando…", 11, MUTED, anchor="rm")
            return 0
        h = self.height()
        ui.rect((x, y, x + w, y + h), 8, fill=(46, 46, 55, 255), outline=(255, 255, 255, 30))
        for i, place in enumerate(self.results):
            top = y + 4 + i * self.ROW_H
            rid = f"cidade:{i}"
            if ui.hot(rid) or i == self.index:
                ui.rect((x + 4, top, x + w - 4, top + self.ROW_H), 6, fill=(255, 255, 255, 22))
            ui.icon(x + 22, top + self.ROW_H / 2, "location", 13, ACCENT + (255,))
            ui.text(x + 40, top + 7, place["name"], 12.5, FG, "seguisb.ttf", max_w=w - 56)
            ui.text(x + 40, top + 23, place["detail"], 10.5, MUTED, max_w=w - 56)
            ui.region(rid, (x + 4, top, x + w - 4, top + self.ROW_H), on_click=lambda p=place: on_pick(p))
        return h


class Prompt(Panel):
    """Pergunta com um campo de texto. `on_done(texto, cidade_escolhida_ou_None)` só é chamado ao confirmar."""
    FRAMELESS = True
    W = 380

    def __init__(self, app, title, message, initial="", on_done=None, anchor=None, cities=False,
                 placeholder="", ok_label="Salvar"):
        self.title, self.message, self.value = title, message, initial
        self.on_done, self.ok_label, self.placeholder = on_done, ok_label, placeholder
        self.suggest = CitySuggest(self) if cities else None
        self.picked = None
        self._rows = None
        size = (self.W, self._height())
        super().__init__(app, pos=self._position(app, anchor, size), size=size)
        self.win.bind("<FocusOut>", lambda _e: self.win.after(200, self._check_focus))
        self.win.after(50, self._focus_field)

    def _focus_field(self):
        if self.alive:
            self.focus_entry("valor")
            entry = self.entries["valor"][0]
            entry.select_range(0, "end")
            entry.icursor("end")

    def _position(self, app, anchor, size):
        s = app.scale_at(((anchor[0] + anchor[2]) // 2, (anchor[1] + anchor[3]) // 2) if anchor else (0, 0))
        w, h = round(size[0] * s), round((size[1] + 6 * CitySuggest.ROW_H) * s)
        if not anchor:
            left, top, right, bottom = work_areas()[0]
            return (left + right - w) // 2, (top + bottom - h) // 2
        x0, y0, x1, y1 = anchor
        area = next((a for a in work_areas() if a[0] <= x0 < a[2] and a[1] <= y0 < a[3]), work_areas()[0])
        gap = round(10 * s)
        x = min(max(x1 - w, area[0] + gap), area[2] - w - gap)
        y = y1 + gap if y1 + gap + h <= area[3] else max(area[1] + gap, y0 - gap - round(size[1] * s))
        return x, y

    def _height(self):
        lines = 2 if len(self.message) > 52 else 1
        return 150 + 17 * lines + (self.suggest.height() + 8 if self.suggest and self.suggest.results else 0)

    def _check_focus(self):
        if not self.alive:
            return
        try:
            focused = self.win.focus_get()
        except (KeyError, Exception):
            focused = None
        if focused is None and not self.pressed:
            self.close()  # clicou fora: cancela

    # --- desenho ---

    def draw(self, ui):
        W = self.W
        h = self._height()
        if abs(h - self.H) > 0.5:
            self.resize(W, h)
            return
        ui.text(20, 18, self.title, 15, FG, "seguisb.ttf", max_w=W - 40)
        y = 44
        for line in ui.wrap(self.message, 12, W - 40)[:2]:
            ui.text(20, y, line, 12, MUTED)
            y += 17
        y += 10
        ui.entry("valor", (20, y, W - 20, y + 36), self.value, self._commit, self.placeholder,
                 on_type=self._typed, on_key=self._key_in_field)
        y += 36
        if self.suggest:
            if self.suggest.results:
                y += 6
            y += self.suggest.draw(ui, 20, y, W - 40, self._pick)
        by = self.H - 52
        ok_w = ui.button("ok", W - 20, by, self.ok_label, self._confirm, kind="primary", align="r", w=96)
        ui.button("cancelar", W - 20 - ok_w - 8, by, "Cancelar", self.close, align="r", w=96)

    # --- campo ---

    def _commit(self, text):
        self.value = text

    def _typed(self, text):
        self.value = text
        self.picked = None
        if self.suggest:
            self.suggest.typed(text)

    def _key_in_field(self, keysym):
        if self.suggest and self.suggest.key(keysym, self._pick):
            return True
        if keysym == "Return":
            self._confirm()
            return True
        return False

    def _pick(self, place):
        self.picked = place
        self.value = place["label"]
        self.set_entry("valor", place["label"])
        self.suggest.clear()
        self._confirm()

    def _confirm(self):
        text = self.entry_text("valor").strip()
        callback, picked = self.on_done, self.picked
        if picked and text != picked["label"]:
            picked = None  # editou depois de escolher
        self.on_done = None
        self.close()
        if callback and text:
            callback(text, picked)
