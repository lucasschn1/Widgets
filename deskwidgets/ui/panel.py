"""Janelas desenhadas com o Pillow, no visual dos widgets (configurações, boas-vindas, perguntas).

Funciona no modo "imediato": a cada quadro a subclasse redesenha tudo em `draw(ui)`
chamando os controles (`ui.switch`, `ui.slider`, `ui.button`...), que desenham e, ao
mesmo tempo, registram as áreas clicáveis. O estado (o que está sob o mouse, apertado,
em foco, animando) fica no painel. Campos de texto são `tk.Entry` de verdade, colocados
por cima do desenho, para digitar com acentos, copiar, colar e selecionar normalmente.
"""

import time
import tkinter as tk

from PIL import Image, ImageDraw, ImageTk

from deskwidgets.theme import ACCENT, ACCENT_HOVER, DANGER, FAINT, FG, MUTED, ease_in_out, font, icon_font
from deskwidgets.win32 import style_window, user32, work_areas

K = 2                       # supersampling das formas (reduzido com box filter, que é rápido)
BASE = (24, 24, 30)         # fundo, misturado ao papel de parede desfocado (efeito "Mica")
BASE_AMOUNT = 0.84
FIELD = (40, 40, 48)        # campos de texto (opacos, para o tk.Entry por cima combinar)
FIELD_HOVER = (48, 48, 57)
LINE = (255, 255, 255, 26)  # bordas finas
SURFACE = (255, 255, 255, 11)
SURFACE_HOVER = (255, 255, 255, 20)
SURFACE_DOWN = (255, 255, 255, 7)


def hexcolor(rgb):
    return "#%02x%02x%02x" % tuple(rgb[:3])


def draw_text(d, x, y, text, f, fill, anchor="la"):
    """Texto com a âncora calculada à mão e a linha de base em pixel inteiro (fica nítido)."""
    horizontal, vertical = anchor
    if horizontal == "m":
        x -= f.getlength(text) / 2
    elif horizontal == "r":
        x -= f.getlength(text)
    if vertical == "a":
        y += f.getmetrics()[0]
    elif vertical == "m":
        _, top, _, bottom = f.getbbox(text or "X", anchor="ls")
        y -= (top + bottom) / 2
    d.text((round(x), round(y)), text, font=f, fill=fill, anchor="ls")


def fit(text, f, max_px):
    if max_px is None or f.getlength(text) <= max_px:
        return text
    while text and f.getlength(text + "…") > max_px:
        text = text[:-1]
    return text.rstrip(" ,-·|") + "…"


class Region:
    __slots__ = ("id", "box", "on_click", "on_drag", "on_release", "cursor")

    def __init__(self, rid, box, on_click=None, on_drag=None, on_release=None, cursor="hand2"):
        self.id, self.box = rid, box
        self.on_click, self.on_drag, self.on_release, self.cursor = on_click, on_drag, on_release, cursor


class Ctx:
    """Superfície de desenho de um quadro (ou de um pedaço dele, como a área rolável)."""

    def __init__(self, panel, size, origin=(0, 0), scroll=0):
        self.p, self.s = panel, panel.s
        self.w, self.h = size
        self.ox, self.oy = origin   # onde esta superfície fica na janela (pixels lógicos)
        self.dy = -scroll
        self.layers = [([], [])]    # (formas, textos e imagens), uma camada por cima da outra

    # --- desenho ---

    def overlay(self):
        """O que vier depois fica por cima de tudo (ex.: a lista de sugestões)."""
        self.layers.append(([], []))

    def shape(self, fn):
        """`fn(d, f, dy)` desenha em resolução f = escala × K, com o deslocamento da rolagem."""
        self.layers[-1][0].append(fn)

    def rect(self, box, radius=0, fill=None, outline=None, width=1):
        x0, y0, x1, y1 = box

        def fn(d, f, dy):
            d.rounded_rectangle((x0 * f, (y0 + dy) * f, x1 * f, (y1 + dy) * f), radius=radius * f, fill=fill,
                                outline=outline, width=max(1, round(width * f)) if outline else 0)
        self.shape(fn)

    def circle(self, cx, cy, r, fill=None, outline=None, width=1):
        def fn(d, f, dy):
            d.ellipse(((cx - r) * f, (cy - r + dy) * f, (cx + r) * f, (cy + r + dy) * f), fill=fill,
                      outline=outline, width=max(1, round(width * f)) if outline else 0)
        self.shape(fn)

    def line(self, points, fill, width=1):
        def fn(d, f, dy):
            d.line([(x * f, (y + dy) * f) for x, y in points], fill=fill, width=max(1, round(width * f)),
                   joint="curve")
        self.shape(fn)

    def text(self, x, y, text, size=13, color=FG, face="segoeui.ttf", anchor="la", max_w=None):
        f = font(face, max(1, round(size * self.s * self.p.app.text_scale)))
        text = fit(text, f, None if max_w is None else max_w * self.s)
        self.layers[-1][1].append(("text", x, y, text, f, color, anchor))
        return f.getlength(text) / self.s

    def text_width(self, text, size=13, face="segoeui.ttf"):
        return font(face, max(1, round(size * self.s * self.p.app.text_scale))).getlength(text) / self.s

    def icon(self, x, y, name_or_char, size=16, color=FG, anchor="mm"):
        from deskwidgets.theme import ICONS
        f = icon_font(max(1, round(size * self.s)))
        if f is None:
            return
        self.layers[-1][1].append(("text", x, y, ICONS.get(name_or_char, name_or_char), f, color, anchor))

    def image(self, img, x, y):
        """Cola uma imagem RGBA (já no tamanho final, em pixels) com o canto em (x, y) lógicos."""
        self.layers[-1][1].append(("image", x, y, img))

    def wrap(self, text, size, max_w, face="segoeui.ttf"):
        f = font(face, max(1, round(size * self.s * self.p.app.text_scale)))
        lines, line = [], ""
        for word in text.split():
            trial = f"{line} {word}".strip()
            if line and f.getlength(trial) > max_w * self.s:
                lines.append(line)
                line = word
            else:
                line = trial
        return lines + ([line] if line else [])

    def render(self, background):
        """Desenha tudo sobre `background` (opaco, do tamanho desta superfície) e devolve a imagem RGB.

        Desenhar direto no fundo opaco faz as formas translúcidas se somarem certo (numa camada
        transparente o Pillow troca o alfa em vez de compor, e um destaque "abriria um buraco").
        """
        s = self.s
        out = background.convert("RGB")
        dy = self.dy
        for shapes, ops in self.layers:
            if shapes:
                # ampliar sem suavizar e reduzir de novo devolve os mesmos pixels onde nada foi desenhado
                big = out.resize((out.width * K, out.height * K), Image.NEAREST)
                d = ImageDraw.Draw(big, "RGBA")
                for fn in shapes:
                    fn(d, s * K, dy)
                out = big.reduce(K)
            if ops:
                d = ImageDraw.Draw(out, "RGBA")
                for op in ops:
                    if op[0] == "text":
                        _, x, y, text, f, color, anchor = op
                        draw_text(d, x * s, (y + dy) * s, text, f, color, anchor)
                    else:
                        _, x, y, img = op
                        pos = (round(x * s), round((y + dy) * s))
                        out.paste(img, pos, img if img.mode == "RGBA" else None)
        return out

    # --- interação ---

    def region(self, rid, box, **handlers):
        """Área clicável; `box` nas coordenadas desta superfície (antes da rolagem)."""
        x0, y0, x1, y1 = box
        wx0, wy0 = x0 + self.ox, y0 + self.dy + self.oy
        wx1, wy1 = x1 + self.ox, y1 + self.dy + self.oy
        # recorta na superfície (o que rolou para fora não recebe cliques)
        cx0, cy0 = max(wx0, self.ox), max(wy0, self.oy)
        cx1, cy1 = min(wx1, self.ox + self.w), min(wy1, self.oy + self.h)
        if cx1 > cx0 and cy1 > cy0:
            self.p.regions.append(Region(rid, (cx0, cy0, cx1, cy1), **handlers))

    def to_window_x(self, x):
        return x + self.ox

    def hot(self, rid):
        return self.p.hover == rid

    def down(self, rid):
        return self.p.pressed == rid and self.p.hover == rid

    def anim(self, rid, target, ms=170):
        return self.p.anim(rid, target, ms)

    # --- controles ---

    def switch(self, rid, x, y, on, on_change, area=None):
        """Interruptor (40×22) com o canto superior esquerdo em (x, y); `area` amplia o clique."""
        w, h = 40, 22
        t = self.anim(rid, 1.0 if on else 0.0)
        hot = self.hot(rid)
        off_fill = (255, 255, 255, 36 if hot else 22)
        track = tuple(round(a + (b - a) * t) for a, b in zip(off_fill, ACCENT_HOVER + (255,) if hot else ACCENT + (255,)))
        self.rect((x, y, x + w, y + h), h / 2, fill=track,
                  outline=(255, 255, 255, round(110 * (1 - t))) if t < 1 else None, width=1)
        r = 7 + 1.5 * t + (0.8 if self.down(rid) else 0)
        cx = x + h / 2 + (w - h) * t
        self.circle(cx, y + h / 2, r, fill=(255, 255, 255, 255) if t > 0.5 else (235, 235, 240, 230))
        self.region(rid, area or (x - 4, y - 4, x + w + 4, y + h + 4), on_click=lambda: on_change(not on))

    def slider(self, rid, x, y, w, value, lo, hi, on_change, step=1, on_release=None):
        """Slider com a trilha centrada em y; chama on_change(valor) enquanto arrasta."""
        frac = 0 if hi == lo else (value - lo) / (hi - lo)
        frac = min(max(frac, 0), 1)
        self.rect((x, y - 2, x + w, y + 2), 2, fill=(255, 255, 255, 48))
        if frac > 0:
            self.rect((x, y - 2, x + w * frac, y + 2), 2, fill=ACCENT + (255,))
        hot, down = self.hot(rid), self.down(rid) or self.p.dragging == rid
        cx = x + w * frac
        self.circle(cx, y, 10, fill=(70, 70, 80, 255), outline=(255, 255, 255, 40))
        self.circle(cx, y, 5.5 if down else (7 if hot else 6),
                    fill=(ACCENT_HOVER if hot else ACCENT) + (255,))
        wx = self.to_window_x(x)

        def drag(px, _py):
            v = lo + min(max((px - wx) / w, 0), 1) * (hi - lo)
            v = round(v / step) * step
            v = round(v, 4) if isinstance(step, float) else int(v)
            if v != value:
                on_change(v)

        self.region(rid, (x - 10, y - 13, x + w + 10, y + 13), on_drag=drag, on_release=on_release)

    def segmented(self, rid, x, y, options, current, on_change, align="l", h=32, size=12):
        """Botões segmentados; `options` = [(valor, rótulo)]. Devolve a largura."""
        pad = 14
        widths = [self.text_width(label, size) + 2 * pad for _, label in options]
        total = sum(widths) + 4
        if align == "r":
            x -= total
        self.rect((x, y, x + total, y + h), 8, fill=(255, 255, 255, 14), outline=LINE)
        index = next((i for i, (v, _) in enumerate(options) if v == current), 0)
        target = 2 + sum(widths[:index])
        pos = self.anim(rid + ":pos", target, 200)
        width = self.anim(rid + ":w", widths[index], 200)
        self.rect((x + pos, y + 2, x + pos + width, y + h - 2), 6, fill=(255, 255, 255, 46))
        cx = x + 2
        for (value, label), seg_w in zip(options, widths):
            sid = f"{rid}:{value}"
            if self.hot(sid) and value != current:
                self.rect((cx, y + 2, cx + seg_w, y + h - 2), 6, fill=(255, 255, 255, 14))
            self.text(cx + seg_w / 2, y + h / 2, label, size, FG if value == current else MUTED,
                      "seguisb.ttf" if value == current else "segoeui.ttf", anchor="mm")
            self.region(sid, (cx, y, cx + seg_w, y + h), on_click=lambda v=value: on_change(v))
            cx += seg_w
        return total

    def button(self, rid, x, y, label, on_click, kind="secondary", w=None, h=32, icon=None, align="l",
               enabled=True, size=12.5):
        """Botão: "primary" (azul), "secondary", "danger" ou "ghost". Devolve a largura."""
        iw = 22 if icon else 0
        w = w or self.text_width(label, size, "seguisb.ttf") + 32 + iw
        if align == "r":
            x -= w
        elif align == "m":
            x -= w / 2
        hot, down = self.hot(rid) and enabled, self.down(rid) and enabled
        if kind == "primary":
            fill = (ACCENT_HOVER if hot and not down else ACCENT) + (255 if enabled else 110,)
            outline, color = None, FG
        elif kind == "danger":
            fill = (DANGER + (60,)) if hot else (DANGER + (34,))
            outline, color = DANGER + (120,), (255, 140, 130, 255)
        elif kind == "ghost":
            fill = SURFACE_HOVER if hot else None
            outline, color = None, FG
        else:
            fill = SURFACE_DOWN if down else (SURFACE_HOVER if hot else (255, 255, 255, 16))
            outline, color = LINE, FG
        if not enabled:
            color = FAINT
        self.rect((x, y, x + w, y + h), 7, fill=fill, outline=outline)
        tx = x + w / 2 + iw / 2
        if icon:
            label_w = self.text_width(label, size, "seguisb.ttf")
            self.icon(tx - label_w / 2 - 12, y + h / 2 + (1 if down else 0), icon, 13, color)
        self.text(tx, y + h / 2 + (1 if down else 0), label, size, color, "seguisb.ttf", anchor="mm")
        if enabled:
            self.region(rid, (x, y, x + w, y + h), on_click=on_click)
        return w

    def entry(self, rid, box, value, on_commit, placeholder="", on_type=None, on_key=None, error=False):
        """Campo de texto (um tk.Entry de verdade por cima do desenho)."""
        x0, y0, x1, y1 = box
        focused = self.p.focus == rid
        hot = self.hot(rid)
        self.rect(box, 7, fill=(FIELD_HOVER if hot and not focused else FIELD) + (255,),
                  outline=DANGER + (200,) if error else (255, 255, 255, 40 if focused else 26))
        if focused:
            self.rect((x0 + 1, y1 - 2.5, x1 - 1, y1 - 0.5), 1.2, fill=ACCENT + (255,))
        self.region(rid, box, on_click=lambda: self.p.focus_entry(rid), cursor="xterm")
        inner = (x0 + 11, y0 + 4, x1 - 11, y1 - 5)
        self.p.place_entry(self, rid, inner, value, on_commit, placeholder, on_type, on_key,
                           (FIELD_HOVER if hot and not focused else FIELD))

    def hotkey(self, rid, box, label, on_record, error=None):
        """Campo de atalho: mostra as teclas; clique e aperte a combinação nova."""
        x0, y0, x1, y1 = box
        recording = self.p.recording == rid
        hot = self.hot(rid)
        self.rect(box, 7, fill=(FIELD_HOVER if hot else FIELD) + (255,),
                  outline=ACCENT + (255,) if recording else (DANGER + (200,) if error else LINE))
        cy = (y0 + y1) / 2
        if recording:
            self.text(x0 + 12, cy, "Pressione o atalho…  (Esc cancela, Backspace remove)", 11.5, FG, anchor="lm",
                      max_w=x1 - x0 - 24)
        elif label:
            x = x0 + 8
            for part in label.split("+"):
                tw = self.text_width(part, 11.5, "seguisb.ttf") + 16
                self.rect((x, cy - 11, x + tw, cy + 11), 5, fill=(255, 255, 255, 22), outline=(255, 255, 255, 34))
                self.text(x + tw / 2, cy, part, 11.5, FG, "seguisb.ttf", anchor="mm")
                x += tw + 5
        else:
            self.text(x0 + 12, cy, "Nenhum  ·  clique para definir", 11.5, FAINT, anchor="lm")
        self.region(rid, box, on_click=lambda: self.p.start_recording(rid, on_record))

    def card(self, box, radius=10):
        self.rect(box, radius, fill=SURFACE, outline=(255, 255, 255, 16))

    def divider(self, x0, x1, y):
        self.rect((x0, y, x1, y + 1), 0, fill=(255, 255, 255, 18))


class Panel:
    """Janela comum (com foco e teclado) desenhada com o Pillow. As subclasses fazem `draw(ui)`."""
    TITLE = "Widgets"
    W, H = 760, 560
    FRAMELESS = False

    def __init__(self, app, pos=None, size=None):
        self.app = app
        self.alive = True
        self.W, self.H = size or (self.W, self.H)
        self.regions = []
        self.hover = self.pressed = self.dragging = self.focus = self.recording = None
        self._on_record = None
        self.anims = {}
        self.entries = {}            # id: (tk.Entry, item do canvas)
        self._entry_info = {}        # id: callbacks e placeholder do último quadro
        self._seen = set()
        self._job = self._move_job = None
        self._bg = self._bg_key = None
        self._photo = None
        self._caption = None

        win = self.win = tk.Toplevel(app.root)
        win.title(self.TITLE)
        win.withdraw()
        if self.FRAMELESS:
            win.overrideredirect(True)
        win.resizable(False, False)
        self.s = app.scale_at(pos or self._center_point())
        w, h = round(self.W * self.s), round(self.H * self.s)
        x, y = pos or self._centered(w, h)
        win.geometry(f"{w}x{h}+{x}+{y}")
        self.canvas = tk.Canvas(win, width=w, height=h, highlightthickness=0, bd=0, bg=hexcolor(BASE))
        self.canvas.pack(fill="both", expand=True)
        self._image_item = self.canvas.create_image(0, 0, anchor="nw")
        win.protocol("WM_DELETE_WINDOW", self.close)
        c = self.canvas
        c.bind("<Motion>", self._motion)
        c.bind("<ButtonPress-1>", self._press)
        c.bind("<B1-Motion>", self._motion)
        c.bind("<ButtonRelease-1>", self._release)
        c.bind("<Leave>", self._leave)
        win.bind("<MouseWheel>", self._wheel)
        win.bind("<KeyPress>", self._key)
        win.bind("<Configure>", self._configure)
        self.render()
        win.deiconify()
        win.update_idletasks()
        self.hwnd = user32.GetParent(win.winfo_id())
        style_window(self.hwnd, caption=self._caption, border=(60, 60, 70) if self.FRAMELESS else None)
        win.attributes("-topmost", True)  # o Windows nem sempre deixa trazer para a frente
        win.after(300, lambda: self.alive and win.attributes("-topmost", False))
        win.focus_force()
        user32.SetForegroundWindow(self.hwnd)

    # --- para as subclasses ---

    def draw(self, ui):
        raise NotImplementedError

    def on_wheel(self, steps):
        """Roda do mouse (para rolar a página)."""

    def on_close(self):
        """A janela vai fechar."""

    # --- posição e fundo ---

    def _center_point(self):
        left, top, right, bottom = work_areas()[0]
        return (left + right) // 2, (top + bottom) // 2

    def _centered(self, w, h):
        left, top, right, bottom = work_areas()[0]
        return (left + right - w) // 2, max(top, (top + bottom - h) // 2 - round(20 * self.s))

    def _client_origin(self):
        try:
            return self.win.winfo_rootx(), self.win.winfo_rooty()
        except tk.TclError:
            return 0, 0

    def background(self):
        """Papel de parede desfocado sob a janela, bem escurecido (como o Mica do Windows 11)."""
        w, h = round(self.W * self.s), round(self.H * self.s)
        x, y = self._client_origin()
        key = (x, y, w, h, self.app.backdrop.version)
        if key != self._bg_key:
            region = self.app.backdrop.region(x, y, w, h)
            self._bg = Image.blend(region, Image.new("RGB", (w, h), BASE), BASE_AMOUNT).convert("RGBA")
            self._bg_key = key
            top = self._bg.crop((0, 0, w, max(1, round(6 * self.s)))).resize((1, 1), Image.BOX).getpixel((0, 0))
            self._caption = top[:3]
        return self._bg

    def _configure(self, e):
        if e.widget is self.win and self._move_job is None:
            self._move_job = self.win.after(60, self._moved)

    def _moved(self):
        self._move_job = None
        if self.alive and self._client_origin() + (round(self.W * self.s), round(self.H * self.s)) != (self._bg_key or ())[:4]:
            self.render()
            if self._caption:
                style_window(self.hwnd, caption=self._caption)

    def resize(self, w, h):
        self.W, self.H = w, h
        pw, ph = round(w * self.s), round(h * self.s)
        x, y = self.win.winfo_x(), self.win.winfo_y()
        self.win.geometry(f"{pw}x{ph}+{x}+{y}")
        self.canvas.config(width=pw, height=ph)
        self.request()

    # --- quadros ---

    def request(self):
        """Pede um quadro novo (vários pedidos seguidos viram um só)."""
        if self._job is None and self.alive:
            self._job = self.win.after(1, self._frame)

    def _frame(self):
        self._job = None
        if self.alive:
            self.render()

    def render(self):
        self.regions = []
        self._seen = set()
        self._anim_running = False
        ui = Ctx(self, (self.W, self.H))
        self.draw(ui)
        frame = ui.render(self.background())
        self.last_frame = frame  # (útil para testes e capturas)
        for rid, (entry, item) in self.entries.items():
            if rid not in self._seen:
                self.canvas.itemconfigure(item, state="hidden")
        self._photo = ImageTk.PhotoImage(frame)
        self.canvas.itemconfigure(self._image_item, image=self._photo)
        if self._anim_running:
            self._job = self.win.after(16, self._frame)

    def anim(self, rid, target, ms):
        """Valor animado de `rid` indo até `target` (com aceleração suave)."""
        now = time.perf_counter()
        state = self.anims.get(rid)
        if state is None:
            self.anims[rid] = (target, target, now, ms)
            return target
        start, end, t0, dur = state
        t = min(1.0, (now - t0) * 1000 / dur) if dur else 1.0
        current = start + (end - start) * ease_in_out(t)
        if target != end:
            self.anims[rid] = (current, target, now, ms if self.app.animations else 0)
            self._anim_running = True
            return current
        if t < 1:
            self._anim_running = True
        return current

    # --- mouse e teclado ---

    def _logical(self, e):
        return e.x / self.s, e.y / self.s

    def _hit(self, x, y):
        for region in reversed(self.regions):
            x0, y0, x1, y1 = region.box
            if x0 <= x < x1 and y0 <= y < y1:
                return region
        return None

    def _region(self, rid):
        return next((r for r in self.regions if r.id == rid), None)

    def _motion(self, e):
        x, y = self._logical(e)
        if self.dragging:
            region = self._region(self.dragging)
            if region and region.on_drag:
                region.on_drag(x, y)
            return
        region = self._hit(x, y)
        rid = region.id if region else None
        self.canvas.config(cursor=region.cursor if region else "")
        if rid != self.hover:
            self.hover = rid
            self.request()

    def _press(self, e):
        x, y = self._logical(e)
        region = self._hit(x, y)
        if self.recording and (not region or region.id != self.recording):
            self.stop_recording()
        if not region or region.cursor != "xterm":
            self.canvas.focus_set()  # tira o foco dos campos (e eles salvam)
        if not region:
            return
        self.pressed = region.id
        if region.on_drag:
            self.dragging = region.id
            region.on_drag(x, y)
        self.request()

    def _release(self, e):
        x, y = self._logical(e)
        pressed, dragging = self.pressed, self.dragging
        self.pressed = self.dragging = None
        region = self._region(dragging or pressed) if (dragging or pressed) else None
        if dragging and region and region.on_release:
            region.on_release()
        elif pressed:
            hit = self._hit(x, y)
            if hit and hit.id == pressed and hit.on_click:
                hit.on_click()
        self._motion(e)
        self.request()

    def _leave(self, _e):
        if self.hover and not self.dragging:
            self.hover = None
            self.request()

    def _wheel(self, e):
        if e.delta:
            self.on_wheel(e.delta / 120)

    def _key(self, e):
        if self.recording:
            self._record_key(e)
            return "break"
        if e.keysym == "Escape" and self.focus is None:
            self.close()
        return None

    # --- campos de texto ---

    def place_entry(self, ui, rid, inner, value, on_commit, placeholder, on_type, on_key, bg):
        """Coloca (ou cria) o tk.Entry do campo `rid` sobre o desenho do quadro."""
        x0, y0, x1, y1 = inner
        wx0, wy0 = ui.ox + x0, ui.oy + y0 + ui.dy
        wx1, wy1 = ui.ox + x1, ui.oy + y1 + ui.dy
        visible = wy0 >= ui.oy and wy1 <= ui.oy + ui.h
        self._entry_info[rid] = (on_commit, placeholder, on_type, on_key)
        s = self.s
        if rid not in self.entries:
            size = -max(1, round(12.5 * s * self.app.text_scale))
            entry = tk.Entry(self.canvas, bd=0, relief="flat", highlightthickness=0, font=("Segoe UI", size),
                             fg="#f2f2f5", insertbackground="#ffffff", selectbackground=hexcolor(ACCENT),
                             selectforeground="#ffffff", insertwidth=max(1, round(s)))
            item = self.canvas.create_window(0, 0, window=entry, anchor="nw")
            self.entries[rid] = (entry, item)
            entry._placeholder = False
            self._set_entry_text(rid, value)
            entry.bind("<FocusIn>", lambda _e, r=rid: self._entry_focus(r, True))
            entry.bind("<FocusOut>", lambda _e, r=rid: self._entry_focus(r, False))
            entry.bind("<Return>", lambda _e, r=rid: self._entry_return(r))
            entry.bind("<KeyRelease>", lambda e, r=rid: self._entry_typed(r, e))
            entry.bind("<KeyPress>", lambda e, r=rid: self._entry_keypress(r, e))
        entry, item = self.entries[rid]
        entry.config(bg=hexcolor(bg), readonlybackground=hexcolor(bg))
        self.canvas.coords(item, round(wx0 * s), round(wy0 * s))
        self.canvas.itemconfigure(item, width=round((wx1 - wx0) * s), height=round((wy1 - wy0) * s),
                                  state="normal" if visible else "hidden")
        self._seen.add(rid)

    def entry_text(self, rid):
        entry = self.entries.get(rid, (None,))[0]
        if entry is None or entry._placeholder:
            return ""
        return entry.get()

    def _set_entry_text(self, rid, value):
        entry = self.entries[rid][0]
        entry.delete(0, "end")
        placeholder = self._entry_info.get(rid, (None, ""))[1]
        if value or self.focus == rid:
            entry.insert(0, value or "")
            entry.config(fg="#f2f2f5")
            entry._placeholder = False
        else:
            entry.insert(0, placeholder)
            entry.config(fg="#8a8a96")
            entry._placeholder = True

    def set_entry(self, rid, value):
        """Troca o texto de um campo (ex.: depois de "Restaurar padrão")."""
        if rid in self.entries:
            self._set_entry_text(rid, value)

    def focus_entry(self, rid):
        if rid in self.entries:
            self.entries[rid][0].focus_set()

    def _entry_focus(self, rid, on):
        entry = self.entries[rid][0]
        if on:
            self.focus = rid
            if entry._placeholder:
                entry.delete(0, "end")
                entry.config(fg="#f2f2f5")
                entry._placeholder = False
        else:
            if self.focus == rid:
                self.focus = None
            self._commit(rid)
            if not entry.get():
                self._set_entry_text(rid, "")
        self.request()

    def _commit(self, rid):
        on_commit = self._entry_info.get(rid, (None,))[0]
        if on_commit:
            on_commit(self.entry_text(rid))

    def _entry_return(self, rid):
        on_key = self._entry_info.get(rid, (None, None, None, None))[3]
        if on_key and on_key("Return"):
            return "break"
        self.canvas.focus_set()  # salva pelo FocusOut
        return "break"

    def _entry_keypress(self, rid, e):
        on_key = self._entry_info.get(rid, (None, None, None, None))[3]
        if e.keysym in ("Up", "Down", "Escape", "Tab") and on_key and on_key(e.keysym):
            return "break"
        if e.keysym == "Escape":
            self.canvas.focus_set()
            if self.FRAMELESS:
                self.close()
            return "break"
        return None

    def _entry_typed(self, rid, e):
        if e.keysym in ("Return", "Escape", "Up", "Down", "Tab"):
            return
        on_type = self._entry_info.get(rid, (None, None, None, None))[2]
        if on_type:
            on_type(self.entry_text(rid))

    # --- gravar atalho ---

    def start_recording(self, rid, on_record):
        self.recording, self._on_record = rid, on_record
        self.canvas.focus_set()
        self.app.tray.pause_hotkeys(True)  # senão o atalho atual é engolido pelo Windows
        self.request()

    def stop_recording(self):
        if self.recording:
            self.recording = self._on_record = None
            self.app.tray.pause_hotkeys(False)
            self.request()

    def _record_key(self, e):
        from deskwidgets.tray import hotkey_from_event
        if e.keysym == "Escape":
            self.stop_recording()
            return
        callback = self._on_record
        if e.keysym in ("BackSpace", "Delete"):
            self.stop_recording()
            callback("")
            return
        label = hotkey_from_event(e)
        if label:
            self.stop_recording()
            callback(label)

    # --- fechar ---

    def close(self):
        if not self.alive:
            return
        if self.focus:
            self._commit(self.focus)
        self.stop_recording()
        self.on_close()
        self.alive = False
        self.win.destroy()
