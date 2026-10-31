"""Base de todos os widgets: cartão de vidro com tamanhos, arraste, encaixe e animações."""

import math
import time
import tkinter as tk
from datetime import datetime, timezone

from PIL import Image, ImageDraw

from deskwidgets.theme import (
    ANIM_MS, BADGE_D, BADGE_FADE_MS, DRAG_START, EDGE_GAP, FG, FRAME_MS, HOT_FILL, HOVER_COLLAPSE_MS,
    HOVER_EXPAND_MS, MARGIN, PRESS_FILL, REORDER_MS, SIZES, SIZE_NAMES, SNAP_DIST, SS, STALE_DOT,
    STALE_XY, WIDGET_GAP, ease_in_out, feedback_shape, font,
)
from deskwidgets.util import time_ago
from deskwidgets.win32 import (
    HWND_BOTTOM, blit, make_layered, monitors, overlap, pin_to_desktop, set_z, user32, work_areas,
)


class CardWidget:
    """Cartão de vidro na área de trabalho, em até três tamanhos, com animações.

    As subclasses definem `key`, `title`, `layout(mode)`, `draw_shapes()` e
    `draw_content()`; opcionalmente `hit()`, `hot_rect()`, `tooltip()`, `on_wheel()`,
    `menu_items()` e `retry()`. `mode` é "p", "m" ou "g" (pequeno, médio ou grande).
    """
    key = ""
    title = ""
    description = ""
    icon = "widgets"  # ícone na barra lateral das configurações (ver theme.ICONS)
    sizes = SIZES

    def __init__(self, app):
        self.app = app
        self.cfg = app.cfg[self.key]
        mode = self.cfg.get("tamanho") or ("p" if self.cfg.get("compact") else "g")
        self.mode = mode if mode in self.sizes else self.sizes[-1]
        self.alive = True
        self.loading = False     # buscando dados: barra animada no topo do cartão
        self.error = None        # a última atualização falhou: ponto de aviso no canto
        self.updated = None      # quando os dados foram atualizados (timestamp)

        self.s = app.s           # escala da tela; depois vira a do monitor onde ele está
        self.margin = round(MARGIN * self.s)
        self._card = None        # conteúdo do cartão já desenhado (reaproveitado no hover)
        self._composed = None    # (conteúdo, posição, versão do fundo, cartão de vidro)
        self.hover = 0.0         # 0 a 1: mouse em cima (botão de tamanho e opacidade)
        self._hover_target = 0.0
        self._hover_job = None
        self._hot = None         # área clicável sob o mouse (x0, y0, x1, y1, raio), lógica
        self._pressing = False
        self._animating = False
        self._anim = None
        self._press = None       # (x, y, ação) do botão apertado, até virar clique ou arraste
        self._drag = None
        self._move_job = None
        self._slide_job = None
        self._slide_target = None
        self._loading_job = None
        self._temp = False       # aberto só enquanto o mouse está em cima
        self._expand_job = None
        self._watch_job = None
        self._outside_since = None
        self._rest = {}
        self._preview = None
        self._close_hot = False  # mouse sobre o "−" do modo edição
        self._stale = False      # mudou algo enquanto estava pausado ou escondido
        self.pos = (0, 0)        # canto superior esquerdo da janela na tela

        self.win = win = tk.Toplevel(app.root)
        win.title(self.title)
        win.overrideredirect(True)
        win.attributes("-alpha", 0.0)  # invisível até o primeiro desenho
        self._place()
        win.update()
        self.hwnd = user32.GetParent(win.winfo_id())
        make_layered(self.hwnd)
        self.owner = pin_to_desktop(self.hwnd)
        if app.focus_until:
            user32.ShowWindow(self.hwnd, 0)

        win.bind("<ButtonPress-1>", self._on_press)
        win.bind("<B1-Motion>", self._drag_move)
        win.bind("<ButtonRelease-1>", self._on_release)
        win.bind("<Double-Button-1>", self._on_double)
        win.bind("<Button-3>", self._show_menu)
        win.bind("<Enter>", self._on_enter)
        win.bind("<Leave>", self._on_leave)
        win.bind("<Motion>", self._on_motion)
        win.bind("<MouseWheel>", self._on_wheel)
        win.bind("<Destroy>", self._on_destroy)

    # --- miniaturas (galeria de widgets) ---

    @classmethod
    def preview(cls, app, mode, scale):
        """Cartão desenhado com dados de exemplo, sem criar a janela (para widgets ocultos)."""
        w = cls.__new__(cls)
        w.app, w.s, w.mode, w.alive = app, scale, mode, False
        w.cfg = dict(app.cfg.get(cls.key, {}))
        w.error, w.loading, w.updated, w._temp, w.hover = None, False, None, False, 0.0
        w.load_sample()
        return w.render_card(mode)

    def snapshot(self, mode, scale):
        """O cartão como está agora, em outro tamanho e escala (sem mexer na janela)."""
        old = self.s, self.mode
        self.s, self.mode = scale, mode
        try:
            return self.render_card(mode)
        finally:
            self.s, self.mode = old

    def load_sample(self):
        """Preenche o estado com dados de exemplo para preview() (as subclasses completam)."""

    # --- para as subclasses ---

    def layout(self, mode):
        raise NotImplementedError

    def draw_shapes(self, d, k, mode):
        """Formas desenhadas em resolução k vezes maior (suavizadas na redução)."""

    def draw_content(self, img, d, mode):
        """Texto e imagens, desenhados no tamanho final."""

    def overlay(self, img):
        """Camada desenhada a cada exibição por cima do cartão pronto (para animações leves)."""

    def hit(self, x, y):
        """Ação (função) da área clicável em (x, y), em pixels lógicos do cartão."""
        return None

    def hot_rect(self, x, y):
        """Área a destacar sob o mouse: (x0, y0, x1, y1, raio) em pixels lógicos, ou None."""
        return None

    def menu_items(self):
        """Itens próprios do menu (ver GlassMenu)."""
        return []

    def tooltip(self, x, y):
        """Texto da dica em (x, y), em pixels lógicos do cartão (uma linha por item)."""
        return None

    def on_wheel(self, x, y, steps):
        """Roda do mouse em (x, y); `steps` positivo é para cima."""

    def retry(self):
        """Tenta atualizar de novo (clique no aviso de erro)."""

    def on_rescale(self):
        """A escala mudou (o widget foi para um monitor com outra escala): limpa caches."""

    def default_pos(self, w, h):
        _, top, right, _ = work_areas()[0]
        edge, m = round(EDGE_GAP * self.s), self.margin
        return right - edge - (w - 2 * m) - m, top + edge - m

    # --- utilidades ---

    @property
    def compact(self):
        return self.mode == "p"

    def after(self, ms, fn):
        return self.app.root.after(ms, lambda: fn() if self.alive else None)

    def close(self):
        self.alive = False
        self.win.destroy()

    def save(self):
        self.app.save()

    def card_size(self, mode=None):
        lay = self.layout(mode or self.mode)
        return round(lay["w"] * self.s), round(lay["h"] * self.s)

    @property
    def size(self):
        """Tamanho da janela: o cartão mais a margem transparente."""
        w, h = self.card_size()
        return w + 2 * self.margin, h + 2 * self.margin

    def card_rect(self, pos=None, size=None):
        """Retângulo do cartão na tela (sem a margem transparente)."""
        x, y = pos or self.pos
        w, h = size or self.card_size()
        m = self.margin
        return x + m, y + m, x + m + w, y + m + h

    def below(self, other):
        """Posição logo abaixo de outro widget, encaixada e alinhada pela direita."""
        w, _ = self.card_size()
        _, _, right, bottom = other.card_rect()
        return right - w - self.margin, bottom + round(WIDGET_GAP * self.s) - self.margin

    def others(self):
        return [w for w in self.app.widgets.values() if w is not self and w.alive]

    def stack_below(self, rect=None):
        """Widgets empilhados embaixo deste (encaixados um no outro), em cadeia."""
        limit = (WIDGET_GAP + 8) * self.s
        stack, frontier = [], [rect or self.card_rect()]
        while frontier:
            top_rect = frontier.pop()
            for other in self.others():
                if other in stack:
                    continue
                other_rect = other.card_rect()
                if 0 <= other_rect[1] - top_rect[3] <= limit and overlap(top_rect, other_rect)[0] > 0:
                    stack.append(other)
                    frontier.append(other_rect)
        return stack

    def slide_to(self, pos, duration=ANIM_MS):
        """Move o widget até `pos` com animação (ao reorganizar e ao reordenar)."""
        pos = (round(pos[0]), round(pos[1]))
        if self._slide_job and self._slide_target == pos:
            return  # já está indo para lá
        if self._slide_job:
            self.app.root.after_cancel(self._slide_job)
            self._slide_job = None
        if self._card is None or self._animating or pos == self.pos:
            return
        self._slide_target = pos
        start, t0 = self.pos, time.perf_counter()
        duration = duration if self.app.animations else 0

        def step():
            t = min(1.0, (time.perf_counter() - t0) * 1000 / duration) if duration else 1.0
            e = ease_in_out(t)
            self.pos = tuple(round(a + (b - a) * e) for a, b in zip(start, pos))
            self.present(move=True)
            if t < 1:
                self._slide_job = self.after(FRAME_MS, step)
            else:
                self._slide_job = self._slide_target = None
                self._moved()

        step()

    def _moved(self):
        """Depois de mudar de lugar: avisa o Tk, guarda a posição e confere a escala."""
        x, y = self.pos
        self.win.geometry(f"+{x}+{y}")
        self.cfg["x"], self.cfg["y"] = x, y
        self.save()
        self.send_to_back()
        rect = self.card_rect()
        scale = self.app.scale_at(((rect[0] + rect[2]) // 2, (rect[1] + rect[3]) // 2))
        if abs(scale - self.s) > 0.01:
            self.set_scale(scale)

    def set_scale(self, scale):
        """Passa a desenhar na escala de outro monitor (cada um pode ter a sua)."""
        self.s = scale
        self.margin = round(MARGIN * scale)
        self._composed = None
        self.on_rescale()
        w, h = self.size
        self.win.geometry(f"{w}x{h}")
        if self._card is not None:
            self.refresh()

    def visible_on_screen(self, pos=None):
        """Se um pedaço razoável do cartão aparece em algum monitor."""
        rect, need = self.card_rect(pos), 60 * self.s
        return any(min(overlap(rect, screen)) >= need for screen in monitors())

    def ensure_visible(self):
        """Traz o widget para dentro da tela, se ele ficou fora (ex.: trocou o monitor)."""
        if self._animating or self.visible_on_screen():
            return
        left, top, right, bottom = work_areas()[0]
        w, h = self.size
        self.pos = (min(max(self.pos[0], left), right - w), min(max(self.pos[1], top), bottom - h))
        self.present(move=True)
        self._moved()

    # --- desenho ---

    def font_px(self, size):
        return round(size * self.s * self.app.text_scale)

    def text(self, draw, xy, text, face, size, fill=FG, anchor="la", max_w=None):
        f = font(face, self.font_px(size))
        if max_w is not None and f.getlength(text) > max_w * self.s:
            while text and f.getlength(text + "…") > max_w * self.s:
                text = text[:-1]
            text = text.rstrip(" ,-·|") + "…"

        # Calcula a âncora aqui e desenha a partir da linha de base em pixel inteiro:
        # posições fracionárias fazem o Pillow espalhar as letras e elas ficam borradas.
        x, y = xy[0] * self.s, xy[1] * self.s
        horizontal, vertical = anchor
        if horizontal == "m":
            x -= f.getlength(text) / 2
        elif horizontal == "r":
            x -= f.getlength(text)
        if vertical == "a":
            y += f.getmetrics()[0]
        elif vertical == "m":
            _, top, _, bottom = f.getbbox(text, anchor="ls")
            y -= (top + bottom) / 2
        draw.text((round(x), round(y)), text, font=f, fill=fill, anchor="ls")

    def wrap(self, text, face, size, max_w, max_lines):
        """Quebra o texto em até max_lines linhas; a última é cortada com "…" por text()."""
        f = font(face, self.font_px(size))
        limit = max_w * self.s
        words = text.split()
        lines, current = [], ""
        for i, word in enumerate(words):
            trial = f"{current} {word}".strip()
            if not current or f.getlength(trial) <= limit:
                current = trial
                continue
            lines.append(current)
            if len(lines) == max_lines - 1:
                current = " ".join(words[i:])
                break
            current = word
        if current:
            lines.append(current)
        return lines

    def picture(self, img, xy, pic):
        """Cola uma imagem RGBA com o canto superior esquerdo em xy (pixels lógicos)."""
        img.alpha_composite(pic, (round(xy[0] * self.s), round(xy[1] * self.s)))

    def render_card(self, mode=None):
        """Desenha o conteúdo do cartão sobre fundo transparente (o vidro entra em present())."""
        mode = mode or self.mode
        lay = self.layout(mode)
        k = self.s * SS
        big = Image.new("RGBA", (round(lay["w"] * k), round(lay["h"] * k)), (0, 0, 0, 0))
        self.draw_shapes(ImageDraw.Draw(big, "RGBA"), k, mode)
        img = big.resize(self.card_size(mode), Image.LANCZOS)
        self.draw_content(img, ImageDraw.Draw(img, "RGBA"), mode)
        return img

    def radius_px(self, mode=None):
        return self.layout(mode or self.mode)["radius"] * self.s

    def refresh(self):
        """Redesenha o cartão com os dados atuais e mostra na tela."""
        if not self.alive:
            return
        if self.app.suspended and self._card is not None:
            self._stale = True  # redesenha quando voltar a aparecer
            return
        self._stale = False
        self._card = self.render_card()
        if not self._animating:
            self.present()

    def opacity(self):
        """Opacidade da janela: os widgets esmaecem quando o mouse não está em cima."""
        idle = self.app.idle_opacity
        if self.app.peeking or self.app.editing or self._drag or self._temp or idle >= 1:
            return 255
        return round(255 * (idle + (1 - idle) * self.hover))

    def present(self, move=False):
        """Mostra o cartão já desenhado sobre o vidro, com a margem e os detalhes por cima.

        Barato: não redesenha o conteúdo. Com `move`, também leva a janela para self.pos.
        """
        card, m = self._card, self.margin
        if card is None or (self.app.suspended and not move):
            return
        backdrop = self.app.backdrop
        c = self._composed
        if c is None or c[0] is not card or c[1:3] != (self.pos, backdrop.version):
            x, y = self.pos
            glass = backdrop.glass(card, self.radius_px(), x + m, y + m)
            self._composed = (card, self.pos, backdrop.version, glass)
        img = Image.new("RGBA", (card.width + 2 * m, card.height + 2 * m), (0, 0, 0, 0))
        img.paste(self._composed[3], (m, m))
        self.overlay(img)
        self._draw_feedback(img)
        editing = self.app.editing
        if self.error and not editing:
            self._draw_stale_dot(img)
        if self.loading:
            self._draw_loading(img)
        if self.hover > 0 or editing:
            self._draw_badge(img)
        if editing:
            self._draw_close(img)
        blit(self.hwnd, img, self.pos if move else None, self.opacity())
        if self.loading and self._loading_job is None:
            self._loading_job = self.after(33, self._loading_tick)

    # --- detalhes por cima do cartão ---

    def _px_rect(self, rect):
        x0, y0, x1, y1, r = rect
        s, m = self.s, self.margin
        return round(m + x0 * s), round(m + y0 * s), round(m + x1 * s), round(m + y1 * s), r * s

    def _draw_feedback(self, img):
        """Destaque de vidro no que está sob o mouse, e mais escuro enquanto é apertado."""
        if not self._hot or self._drag:
            return
        x0, y0, x1, y1, r = self._px_rect(self._hot)
        if x1 - x0 < 2 or y1 - y0 < 2:
            return
        fill = PRESS_FILL if self._pressing else HOT_FILL
        img.alpha_composite(feedback_shape(x1 - x0, y1 - y0, round(r), fill), (x0, y0))

    def _draw_stale_dot(self, img):
        k, r = 3, 3.5 * self.s
        size = round(2 * r + 2)
        big = Image.new("RGBA", (size * k, size * k), (0, 0, 0, 0))
        ImageDraw.Draw(big).ellipse((k, k, (size - 1) * k, (size - 1) * k), fill=STALE_DOT)
        dot = big.resize((size, size), Image.LANCZOS)
        cx, cy = (self.margin + v * self.s for v in STALE_XY)
        img.alpha_composite(dot, (round(cx - size / 2), round(cy - size / 2)))

    def _on_stale_dot(self, x, y):
        return self.error and math.hypot(x - STALE_XY[0], y - STALE_XY[1]) <= 9

    def _draw_loading(self, img):
        """Brilho correndo ao longo do topo do cartão enquanto busca dados."""
        s, m = self.s, self.margin
        w = self._card.width
        r = round(self.radius_px())
        x0, x1 = m + r, m + w - r
        length = x1 - x0
        if length <= 0:
            return
        seg = max(8, length // 3)
        t = (time.perf_counter() * 0.9) % 1
        start = x0 - seg + round((length + seg) * t)
        thickness = max(2, round(2 * s))
        ramp = Image.new("L", (seg, 1))
        ramp.putdata([round(230 * math.sin(math.pi * i / (seg - 1))) for i in range(seg)])
        bar = Image.new("RGBA", (seg, thickness), (255, 255, 255, 0))
        bar.putalpha(ramp.resize((seg, thickness)))
        left, right = max(start, x0), min(start + seg, x1)
        if right > left:
            img.alpha_composite(bar.crop((left - start, 0, right - start, thickness)), (left, m + round(s)))

    def _loading_tick(self):
        self._loading_job = None
        if self.loading and not self._animating:
            self.present()
        elif not self.loading:
            self.present()  # tira o último quadro da barra

    # --- botão de tamanho ---

    def next_mode(self):
        i = self.sizes.index(self.mode)
        return self.sizes[(i + 1) % len(self.sizes)]

    def _badge_center(self):
        """Centro do botão: no canto superior direito, sobressaindo da borda do cartão."""
        card_w, _ = self.card_size()
        inset = 5 * self.s
        return self.margin + card_w - inset, self.margin + inset

    def _draw_badge(self, img):
        k = SS
        size = round((BADGE_D + 2) * self.s)
        big = Image.new("RGBA", (size * k, size * k), (0, 0, 0, 0))
        d = ImageDraw.Draw(big)
        r = BADGE_D / 2 * self.s * k
        c = size * k / 2
        d.ellipse((c - r, c - r, c + r, c + r), fill=(78, 78, 84, 255),
                  outline=(110, 110, 118, 255), width=max(1, round(self.s * k)))
        arm = 4.5 * self.s * k
        stroke = round(1.8 * self.s * k)
        d.line((c - arm, c, c + arm, c), fill=FG, width=stroke)
        # "+" quando o próximo tamanho é maior (ou para fixar aberto), "−" quando volta ao menor
        bigger = self._temp or self.sizes.index(self.next_mode()) > self.sizes.index(self.mode)
        if bigger:
            d.line((c, c - arm, c, c + arm), fill=FG, width=stroke)
        badge = big.resize((size, size), Image.LANCZOS)
        alpha = 1.0 if self.app.editing else self.hover
        badge.putalpha(badge.getchannel("A").point(lambda a: round(a * alpha)))
        cx, cy = self._badge_center()
        img.alpha_composite(badge, (round(cx - size / 2), round(cy - size / 2)))

    def _on_badge(self, x, y):
        if self.hover <= 0 and not self.app.editing:
            return False
        cx, cy = self._badge_center()
        return math.hypot(x - cx, y - cy) <= (BADGE_D / 2 + 2) * self.s

    # --- botão de fechar (modo edição) ---

    def _close_center(self):
        inset = 5 * self.s
        return self.margin + inset, self.margin + inset

    def _draw_close(self, img):
        k = SS
        size = round((BADGE_D + 2) * self.s)
        big = Image.new("RGBA", (size * k, size * k), (0, 0, 0, 0))
        d = ImageDraw.Draw(big)
        r = BADGE_D / 2 * self.s * k
        c = size * k / 2
        d.ellipse((c - r, c - r, c + r, c + r), fill=(255, 69, 58, 255) if self._close_hot else (92, 92, 100, 255),
                  outline=(120, 120, 128, 255), width=max(1, round(self.s * k)))
        arm = 4.5 * self.s * k
        d.line((c - arm, c, c + arm, c), fill=FG, width=round(1.8 * self.s * k))
        cx, cy = self._close_center()
        img.alpha_composite(big.resize((size, size), Image.LANCZOS), (round(cx - size / 2), round(cy - size / 2)))

    def _on_close(self, x, y):
        if not self.app.editing:
            return False
        cx, cy = self._close_center()
        return math.hypot(x - cx, y - cy) <= (BADGE_D / 2 + 2) * self.s

    def _close_action(self):
        self.app.close_widget(self.key)

    def _badge_action(self):
        if self._temp:
            self.pin_open()
        else:
            self.resize_to(self.next_mode())

    def _action_at(self, x, y):
        if self._animating or self.app.editing:  # no modo edição, o cartão todo serve para arrastar
            return None
        lx, ly = self._logical(x, y)
        if self._on_stale_dot(lx, ly):
            return self.retry
        return self.hit(lx, ly)

    # --- mouse em cima ---

    def _set_hover(self, on):
        self._hover_target = 1.0 if on else 0.0
        if self._hover_job is None and not self._animating and self._card is not None:
            self._hover_step()

    def _hover_step(self):
        step = FRAME_MS / BADGE_FADE_MS if self.app.animations else 1
        if self.hover < self._hover_target:
            self.hover = min(self._hover_target, self.hover + step)
        else:
            self.hover = max(self._hover_target, self.hover - step)
        self.present()
        if self.hover != self._hover_target:
            self._hover_job = self.after(FRAME_MS, self._hover_step)
        else:
            self._hover_job = None

    def _logical(self, x, y):
        return (x - self.margin) / self.s, (y - self.margin) / self.s

    def _on_enter(self, _e):
        self._set_hover(True)
        self._schedule_expand()

    def _on_motion(self, e):
        if self._drag:
            return
        close_hot = self._on_close(e.x, e.y)
        if close_hot != self._close_hot:
            self._close_hot = close_hot
            self.present()
        on_badge = self._on_badge(e.x, e.y) or close_hot
        action = None if on_badge else self._action_at(e.x, e.y)
        self.win.config(cursor="hand2" if on_badge or action else ("fleur" if self.app.editing else ""))
        lx, ly = self._logical(e.x, e.y)
        hot = None
        if action and not self._animating:
            hot = (STALE_XY[0] - 8, STALE_XY[1] - 8, STALE_XY[0] + 8, STALE_XY[1] + 8, 8) \
                if self._on_stale_dot(lx, ly) else self.hot_rect(lx, ly)
        if hot != self._hot:
            self._hot = hot
            self.present()
        tip = None
        if not self._animating and not on_badge and self.app.setting("dicas"):
            tip = self._status_tip() if self._on_stale_dot(lx, ly) else self.tooltip(lx, ly)
        self.app.tooltip.request(tip, e.x_root, e.y_root)
        self._schedule_expand()

    def _status_tip(self):
        when = ""
        if self.updated:
            when = "\nDados de " + time_ago(datetime.fromtimestamp(self.updated, timezone.utc))
        return f"{self.error}{when}\nClique para tentar de novo"

    def _on_leave(self, _e):
        self._set_hover(False)
        if self._close_hot:
            self._close_hot = False
            self.present()
        self.app.tooltip.hide()
        if self._expand_job:
            self.app.root.after_cancel(self._expand_job)
            self._expand_job = None
        if self._hot:
            self._hot = None
            self.present()

    def _on_wheel(self, e):
        if not self._animating and e.delta:
            self.app.tooltip.hide()
            self.on_wheel(*self._logical(e.x, e.y), e.delta / 120)

    def _on_destroy(self, e):
        # o Windows destrói a janela se o dono (a área de trabalho) sumir, como quando o
        # Explorer reinicia; nesse caso o widget é recriado em seguida
        if e.widget is self.win and self.alive:
            self.alive = False
            self.app.root.after(3000, lambda: self.app.revive(self.key))

    # --- abrir ao passar o mouse ---

    def _schedule_expand(self):
        if (self.mode != "p" or self._temp or self._animating or self._press or self._expand_job
                or self.app.editing or not self.app.setting("expandir_hover") or len(self.sizes) < 2):
            return
        self._expand_job = self.after(HOVER_EXPAND_MS, self._hover_expand)

    def _pointer_inside(self):
        px, py = self.win.winfo_pointerxy()
        x0, y0, x1, y1 = self.card_rect()
        return x0 <= px < x1 and y0 <= py < y1

    def _hover_expand(self):
        self._expand_job = None
        if self.mode != "p" or self._press or self._animating or not self._pointer_inside():
            return
        for other in self.others():  # este fica por cima dos vizinhos enquanto aberto
            other.send_to_back()
        self._temp = True
        self.resize_to(self.sizes[-1], reflow=False, temporary=True)
        self._outside_since = None
        self._watch_job = self.after(120, self._watch_pointer)

    def _watch_pointer(self):
        self._watch_job = None
        if not self._temp:
            return
        if self._pointer_inside() or self._press or self._animating or self.app.menu.open:
            self._outside_since = None
        elif self._outside_since is None:
            self._outside_since = time.perf_counter()
        elif (time.perf_counter() - self._outside_since) * 1000 >= HOVER_COLLAPSE_MS:
            self.resize_to("p", reflow=False, temporary=True)
            self._temp = False
            return
        self._watch_job = self.after(120, self._watch_pointer)

    def pin_open(self):
        """Mantém aberto o widget que só estava aberto por causa do mouse."""
        if not self._temp or self._animating:
            return
        self._temp = False
        small = self.card_rect(size=self.card_size("p"))
        delta = self.card_size()[1] - self.card_size("p")[1]
        for other in self.stack_below(small):
            other.slide_to((other.pos[0], other.pos[1] + delta))
        self.cfg["tamanho"] = self.mode
        self.cfg.pop("compact", None)
        self.save()
        self.present()

    # --- mudar de tamanho ---

    def set_mode(self, mode):
        if self._temp and mode == self.mode:
            self.pin_open()
            return
        if self._temp:
            self._temp = False
            self.mode = "p"  # o tamanho "de verdade" era o pequeno
        self.resize_to(mode)

    def resize_to(self, mode, reflow=True, temporary=False):
        if self._animating or mode == self.mode or mode not in self.sizes:
            return
        if self._hover_job:
            self.app.root.after_cancel(self._hover_job)
            self._hover_job = None
        followers = self.stack_below() if reflow else []
        # em alfa pré-multiplicado, para a transição entre os dois não passar por cinza
        old = self.render_card(self.mode).convert("RGBa")
        old_radius = self.radius_px()
        self.mode = mode
        new = self.render_card(mode).convert("RGBa")
        new_radius = self.radius_px()

        x, y = self.pos
        win_w = old.width + 2 * self.margin
        self._anim = {
            "old": old, "new": new, "r0": old_radius, "r1": new_radius,
            "x": x, "y": y, "start": time.perf_counter(), "save": not temporary,
            "duration": ANIM_MS if self.app.animations else 0,
            # na metade direita da tela, mantém a borda direita no lugar
            "right": x + win_w / 2 > self.win.winfo_screenwidth() / 2,
        }
        self._animating = True
        self._hot = None
        self.win.config(cursor="")
        self.app.tooltip.hide()
        # os widgets empilhados embaixo acompanham a mudança de altura, como no iPhone
        for other in followers:
            other.slide_to((other.pos[0], other.pos[1] + new.height - old.height))
        self._anim_step()

    def _anim_step(self):
        a = self._anim
        frame_start = time.perf_counter()
        t = min(1.0, (frame_start - a["start"]) * 1000 / a["duration"]) if a["duration"] else 1.0
        e = ease_in_out(t)
        old, new = a["old"], a["new"]
        w = round(old.width + (new.width - old.width) * e)
        h = round(old.height + (new.height - old.height) * e)

        # conteúdo: os dois estados presos no mesmo canto, um sumindo e o outro aparecendo
        layers = []
        for card in (old, new):
            layer = Image.new("RGBa", (w, h), (0, 0, 0, 0))
            layer.paste(card, (w - card.width if a["right"] else 0, 0))
            layers.append(layer)
        fade = ease_in_out(min(1.0, max(0.0, (t - 0.1) / 0.7)))
        content = Image.blend(layers[0], layers[1], fade).convert("RGBA")

        # vidro e cantos arredondados no tamanho e na posição deste quadro
        m = self.margin
        x = a["x"] + (old.width - w if a["right"] else 0)
        radius = a["r0"] + (a["r1"] - a["r0"]) * e
        img = Image.new("RGBA", (w + 2 * m, h + 2 * m), (0, 0, 0, 0))
        img.paste(self.app.backdrop.glass(content, radius, x + m, a["y"] + m), (m, m))
        blit(self.hwnd, img, (x, a["y"]), self.opacity())

        if t < 1:
            # desconta o tempo gasto desenhando, para manter ~60 quadros por segundo
            spent = (time.perf_counter() - frame_start) * 1000
            self.after(max(1, round(FRAME_MS - spent)), self._anim_step)
        else:
            self._anim_done(x, a["y"])

    def _anim_done(self, x, y):
        w, h = self.size
        self.pos = (x, y)
        self.win.geometry(f"{w}x{h}+{x}+{y}")
        if self._anim["save"]:
            self.cfg.update(tamanho=self.mode, x=x, y=y)
            self.cfg.pop("compact", None)
            self.save()
        self._animating = False
        # o mouse pode ter ficado fora do widget depois que ele encolheu
        self.hover = 1.0 if self._pointer_inside() else 0.0
        self._hover_target = self.hover
        self.refresh()
        if not self._temp:
            self.send_to_back()

    # --- janela ---

    def send_to_back(self):
        # atrás das outras janelas (mas acima do papel de parede, que é o dono)
        if not self.app.peeking:
            set_z(self.hwnd, HWND_BOTTOM)

    def _place(self):
        w, h = self.size
        x, y = self.cfg.get("x"), self.cfg.get("y")
        # aceita widgets um pouco fora da tela, desde que boa parte do cartão apareça
        if x is None or y is None or not self.visible_on_screen((x, y)):
            x, y = self.default_pos(w, h)
        self.pos = (x, y)
        rect = self.card_rect()
        scale = self.app.scale_at(((rect[0] + rect[2]) // 2, (rect[1] + rect[3]) // 2))
        if abs(scale - self.s) > 0.01:  # está num monitor com outra escala
            self.s, self.margin = scale, round(MARGIN * scale)
            w, h = self.size
        self.win.geometry(f"{w}x{h}+{x}+{y}")

    def _on_press(self, e):
        # só decide entre clique e arraste quando o mouse anda ou o botão é solto
        self._press = self._drag = None
        self.app.tooltip.hide()
        self.app.menu.close()
        if self._animating:
            return
        if self._on_close(e.x, e.y):
            action = self._close_action
        elif self._on_badge(e.x, e.y):
            action = self._badge_action
        else:
            action = self._action_at(e.x, e.y)
        self._press = (e.x_root, e.y_root, action)
        if action and self._hot:
            self._pressing = True
            self.present()

    def _on_release(self, e):
        press, self._press = self._press, None
        if self._pressing:
            self._pressing = False
            self.present()
        if self._drag:
            self._drag_end(e)
        elif press and press[2] and not self._animating:
            press[2]()

    def _on_double(self, e):
        if (not self._animating and not self._on_badge(e.x, e.y) and not self._on_close(e.x, e.y)
                and not self._action_at(e.x, e.y)):
            self._badge_action()

    def _drag_move(self, e):
        if self._press is None or self._animating or self._temp:
            return
        if self._drag is None:
            px, py, _ = self._press
            locked = self.app.locked and not self.app.editing
            if locked or math.hypot(e.x_root - px, e.y_root - py) < DRAG_START * self.s:
                return
            self._drag = (px - self.pos[0], py - self.pos[1])
            self._pressing = False
            self.win.config(cursor="fleur")
            self.app.tooltip.hide()
            self._drag_begin()
        pos = (e.x_root - self._drag[0], e.y_root - self._drag[1])
        free = e.state & 0x20000  # com Alt, solta o widget sem encaixar nem reordenar
        column = None if free else self._column_at(pos)
        targets, guides, self._preview = {}, {}, None
        if column:
            # sobre uma coluna: os outros abrem espaço para ele, como na tela do iPhone
            targets, self._preview = self._layout_column(column, pos)
        elif not free:
            pos, guides = self._snap(pos)
        self.app.guides.show(guides)
        for other in self.others():
            other.slide_to(targets.get(other, self._rest.get(other, other.pos)), REORDER_MS)
        self.pos = pos
        # o vidro muda com a posição: move e redesenha no mesmo passo, um quadro por vez
        if self._move_job is None:
            self._move_job = self.app.root.after_idle(self._move_now)

    def _drag_begin(self):
        """Guarda onde cada widget fica se este sair do lugar (a coluna dele fecha o espaço)."""
        followers = set(self.stack_below())
        shift = self.card_size()[1] + round(WIDGET_GAP * self.s)
        self._rest = {o: ((o.pos[0], o.pos[1] - shift) if o in followers else o.pos) for o in self.others()}
        self._origin = self.card_rect()

    def _column_at(self, pos):
        """Coluna de widgets sob o cartão arrastado: (esquerda, topo, membros) ou None."""
        rect = self.card_rect(pos)
        w, h = rect[2] - rect[0], rect[3] - rect[1]
        tol = 12 * self.s
        groups = []
        for other in self.others():
            if other not in self._rest:
                continue
            left = self._rest[other][0] + other.margin
            for group in groups:
                if abs(group[0] - left) <= tol:
                    group[1].append(other)
                    break
            else:
                groups.append([left, [other]])
        cy = (rect[1] + rect[3]) / 2
        for left, members in groups:
            col_w = max(o.card_size()[0] for o in members)
            if overlap(rect, (left, rect[1], left + col_w, rect[3]))[0] < 0.6 * min(w, col_w):
                continue
            tops = [self._rest[o][1] + o.margin for o in members]
            bottoms = [t + o.card_size()[1] for t, o in zip(tops, members)]
            top = min(tops)
            if abs(self._origin[0] - left) <= tol and self._origin[1] < top:
                top = self._origin[1]  # ele era o primeiro desta coluna
            if top - h / 2 <= cy <= max(bottoms) + h / 2:
                members.sort(key=lambda o: self._rest[o][1])
                return left, top, members
        return None

    def _layout_column(self, column, pos):
        """Posições da coluna com este widget inserido onde o mouse está."""
        left, top, members = column
        rect = self.card_rect(pos)
        cy = (rect[1] + rect[3]) / 2
        gap = round(WIDGET_GAP * self.s)
        index = sum(1 for o in members if self._rest[o][1] + o.margin + o.card_size()[1] / 2 < cy)
        targets, y, slot = {}, top, None
        for i, other in enumerate(members + [None]):
            if i == index:
                slot = (left - self.margin, y - self.margin)
                y += rect[3] - rect[1] + gap
            if other is None:
                break
            targets[other] = (left - other.margin, y - other.margin)
            y += other.card_size()[1] + gap
        return targets, slot

    def _snap(self, pos):
        """Encaixa a posição nas bordas da tela e nos outros widgets; devolve também as guias."""
        s, m = self.s, self.margin
        w, h = self.card_size()
        x, y = pos[0] + m, pos[1] + m  # canto do cartão
        gap, edge, reach = WIDGET_GAP * s, EDGE_GAP * s, SNAP_DIST * s
        # candidatos: (posição do canto do cartão, linha-guia ou None, retângulo de referência)
        xs, ys = [], []
        for area in work_areas():
            l, t, r, b = area
            xs += [(l + edge, l + edge, area), (r - edge - w, r - edge, area)]
            ys += [(t + edge, t + edge, area), (b - edge - h, b - edge, area)]
        for other in self.others():
            rect = l, t, r, b = other.card_rect()
            xs += [(l, l, rect), (r - w, r, rect), ((l + r - w) / 2, (l + r) / 2, rect)]
            ys += [(t, t, rect), (b - h, b, rect), ((t + b - h) / 2, (t + b) / 2, rect)]
            # lado a lado ou um embaixo do outro, com o espaço padrão (só se estiverem perto)
            if overlap((x, y - reach, x + w, y + h + reach), rect)[1] > 0:
                xs += [(r + gap, None, rect), (l - gap - w, None, rect)]
            if overlap((x - reach, y, x + w + reach, y + h), rect)[0] > 0:
                ys += [(b + gap, None, rect), (t - gap - h, None, rect)]

        def best(options, current):
            near = [o for o in options if abs(o[0] - current) <= reach]
            return min(near, key=lambda o: abs(o[0] - current)) if near else None

        bx, by = best(xs, x), best(ys, y)
        x = round(bx[0]) if bx else x
        y = round(by[0]) if by else y
        guides = {}
        if bx and bx[1] is not None:
            ref = bx[2]
            guides["v"] = (bx[1], min(ref[1], y), max(ref[3], y + h))
        if by and by[1] is not None:
            ref = by[2]
            guides["h"] = (by[1], min(ref[0], x), max(ref[2], x + w))
        return (x - m, y - m), guides

    def _move_now(self):
        self._move_job = None
        if self.alive and self._card is not None:
            self.present(move=True)

    def _drag_end(self, e):
        self._drag = None
        self.app.guides.hide()
        if self._move_job is not None:
            self.app.root.after_cancel(self._move_job)
            self._move_now()
        if self._preview:
            self.slide_to(self._preview, REORDER_MS)  # para o espaço aberto na coluna
            self._preview = None
        else:
            self._moved()
        self._rest = {}
        self._on_motion(e)  # volta o cursor e as dicas ao normal

    def menu_entries(self):
        sizes = [("radio", SIZE_NAMES[m], m == self.mode and not self._temp, lambda m=m: self.set_mode(m))
                 for m in self.sizes]
        entries = [("sub", "Tamanho", sizes)] if len(self.sizes) > 1 else []
        own = self.menu_items()
        if own:
            entries += [None] + own
        return entries + [None] + self.app.common_entries(self)

    def _show_menu(self, e):
        self.app.tooltip.hide()
        self.app.menu.popup(self.menu_entries(), e.x_root, e.y_root)
