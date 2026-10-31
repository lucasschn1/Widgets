"""Configurações: barra lateral com páginas, controles próprios, prévia ao vivo e aplicação imediata.

Cada página é um método `page_<id>(ui, x, w)` que desenha de cima para baixo e devolve a
altura usada (para a rolagem). Toda mudança vale na hora; não há "Salvar" nem "Cancelar".
"""

import copy
from datetime import date


from deskwidgets.config import APPEARANCE_KEYS, BEHAVIOR_KEYS, NOTIFY_LABELS
from deskwidgets import __version__
from deskwidgets.paths import DATA, FROZEN
from deskwidgets.startup import set_startup, startup_enabled
from deskwidgets.theme import ACCENT, ANIM_MS, FAINT, FG, MUTED, SIZE_NAMES, rounded_mask
from deskwidgets.tray import tray_icon_image
from deskwidgets.ui.gallery import TIPS, widget_thumbnail
from deskwidgets.ui.panel import Ctx, Panel
from deskwidgets.ui.prompt import CitySuggest
from deskwidgets.widgets import WIDGET_TYPES
from deskwidgets.win32 import open_target

SIDEBAR, TOP = 240, 78
ANIMATION_SHORT = {"auto": "Seguir o Windows", "sempre": "Sempre", "nunca": "Nunca"}
SOURCE_SHORT = {"auto": "Automático", "spotify": "Spotify", "youtube": "YouTube"}
PHASE_RANGES = {"foco": ("Foco", 5, 90, 5), "pausa": ("Pausa curta", 1, 30, 1), "longa": ("Pausa longa", 5, 60, 5)}


class SettingsWindow(Panel):
    TITLE = "Configurações dos widgets"
    W, H = 900, 640

    def __init__(self, app, page=None):
        self.page = page or "geral"
        self.scrolls = {}
        self.content_h = 0
        self.thumbs = {}
        self.city = CitySuggest(self)
        self._throttled = {}
        self._preview = None      # (chave, imagem) da prévia da página Aparência
        self._preview_content = None
        super().__init__(app)

    # --- navegação ---

    def nav_items(self):
        items = [("geral", "Geral", "geral"), ("aparencia", "Aparência", "aparencia"),
                 ("widgets", "Galeria de widgets", "widgets"), ("-", "SEUS WIDGETS", None)]
        items += [(key, cls.title, cls.icon) for key, cls in WIDGET_TYPES.items()]
        items += [("-", "", None), ("avisos", "Notificações", "avisos"), ("atalhos", "Atalhos e dicas", "atalhos"),
                  ("sobre", "Sobre", "sobre")]
        return items

    def go(self, page):
        if page != self.page:
            self.page = page
            self.city.clear()
            self.canvas.focus_set()
            self._preview_content = None
            if page == "widgets":
                self.thumbs.clear()
            self.request()

    def changed(self):
        self.thumbs.clear()
        self._preview_content = None
        self.request()

    def on_wheel(self, steps):
        view = self.H - TOP
        top = max(0, self.content_h - view)
        new = min(max(self.scrolls.get(self.page, 0) - steps * 64, 0), top)
        if new != self.scrolls.get(self.page, 0):
            self.scrolls[self.page] = new
            self.city.clear()
            self.request()

    def on_close(self):
        if self.app.settings_window is self:
            self.app.settings_window = None

    def _throttle(self, key, fn, ms=140):
        """Roda `fn` já, e depois no máximo uma vez a cada `ms` (para sliders que mexem nos widgets)."""
        if key in self._throttled:
            self._throttled[key] = fn
            return
        fn()
        self._throttled[key] = None

        def flush():
            pending = self._throttled.pop(key, None)
            if pending and self.alive:
                self._throttle(key, pending, ms)
        self.win.after(ms, flush)

    # --- desenho ---

    def draw(self, ui):
        W, H = self.W, self.H
        # barra lateral
        ui.rect((0, 0, SIDEBAR, H), 0, fill=(255, 255, 255, 7))
        ui.rect((SIDEBAR, 0, SIDEBAR + 1, H), 0, fill=(255, 255, 255, 16))
        icon = tray_icon_image(round(30 * self.s))
        ui.image(icon, 22, 22)
        ui.text(62, 22, "Widgets", 15, FG, "seguisb.ttf")
        ui.text(62, 41, "Configurações", 11, MUTED)
        y = 82
        selected_y = None
        for page, label, icon_name in self.nav_items():
            if page == "-":
                if label:
                    ui.text(26, y + 12, label, 9.5, FAINT, "seguisb.ttf")
                    y += 30
                else:
                    ui.rect((20, y + 8, SIDEBAR - 20, y + 9), 0, fill=(255, 255, 255, 16))
                    y += 17
                continue
            rid = "nav:" + page
            active = page == self.page
            if active:
                selected_y = y
                ui.rect((12, y, SIDEBAR - 12, y + 36), 7, fill=(255, 255, 255, 22))
            elif ui.hot(rid):
                ui.rect((12, y, SIDEBAR - 12, y + 36), 7, fill=(255, 255, 255, 11))
            hidden = page in WIDGET_TYPES and page not in self.app.widgets
            color = FG if not hidden else FAINT
            ui.icon(36, y + 18, icon_name, 15, color)
            ui.text(58, y + 18, label, 12.5, color, "seguisb.ttf" if active else "segoeui.ttf", anchor="lm",
                    max_w=SIDEBAR - 80)
            if hidden:
                ui.text(SIDEBAR - 24, y + 18, "oculto", 10, FAINT, anchor="rm")
            ui.region(rid, (12, y, SIDEBAR - 12, y + 36), on_click=lambda p=page: self.go(p))
            y += 38
        if selected_y is not None:  # indicador azul que desliza até a página escolhida
            iy = ui.anim("nav-indicator", selected_y, 220)
            ui.rect((12, iy + 10, 15, iy + 26), 1.5, fill=ACCENT + (255,))

        # título da página
        title, subtitle = self.page_title()
        ui.text(SIDEBAR + 32, 24, title, 22, FG, "seguisb.ttf")
        if subtitle:
            ui.text(SIDEBAR + 32, 54, subtitle, 11.5, MUTED, max_w=W - SIDEBAR - 64)

        # conteúdo rolável
        view_h = H - TOP
        scroll = self.scrolls.get(self.page, 0)
        left = SIDEBAR + 1  # a linha da barra lateral fica de fora
        content = Ctx(self, (W - left, view_h), origin=(left, TOP), scroll=scroll)
        x, w = 32, W - SIDEBAR - 64
        height = getattr(self, "page_" + (self.page if self.page not in WIDGET_TYPES else "widget"))(content, x, w)
        self.content_h = height + 32
        top = max(0, self.content_h - view_h)
        if scroll > top:
            self.scrolls[self.page] = top
            self.request()
        s = self.s
        x0, y0 = round(left * s), round(TOP * s)
        backdrop = self.background().crop((x0, y0, x0 + round((W - left) * s), y0 + round(view_h * s)))
        ui.image(content.render(backdrop), left, TOP)
        if self.content_h > view_h:  # barra de rolagem fininha
            track = view_h - 16
            thumb = max(30, track * view_h / self.content_h)
            ty = TOP + 8 + (track - thumb) * min(scroll, top) / top
            ui.rect((W - 8, ty, W - 5, ty + thumb), 1.5, fill=(255, 255, 255, 60))
        if scroll > 0:  # sombra no topo quando rolou
            ui.rect((SIDEBAR + 1, TOP, W, TOP + 1), 0, fill=(255, 255, 255, 20))

    def page_title(self):
        if self.page in WIDGET_TYPES:
            cls = WIDGET_TYPES[self.page]
            return cls.title, cls.description
        return {
            "geral": ("Geral", "Como os widgets se comportam no dia a dia."),
            "aparencia": ("Aparência", "Vidro, texto e animações. Tudo muda na hora."),
            "widgets": ("Galeria de widgets", "Escolha quais widgets ficam na sua área de trabalho."),
            "avisos": ("Notificações", "Quando os widgets podem chamar sua atenção."),
            "atalhos": ("Atalhos e dicas", "Atalhos de teclado globais e gestos do mouse."),
            "sobre": ("Sobre", None),
        }[self.page]

    # --- blocos das páginas ---

    def section(self, ui, x, y, title):
        ui.text(x + 2, y, title, 13, FG, "seguisb.ttf")
        return y + 28

    def rows(self, ui, x, y, w, rows):
        """Cartão com linhas (título, descrição, controle); o controle recebe (ui, direita, centro_y, caixa)."""
        rows = [r for r in rows if r]
        widths, lines = [], []
        for _title, desc, control in rows:
            text_w = w - 36 - (getattr(control, "width", w * 0.44) if control else 0)
            widths.append(text_w)
            # descrição em até 2 linhas (a segunda é cortada com "…" se ainda não couber)
            full = ui.wrap(desc, 11.5, text_w) if desc else []
            lines.append(full if len(full) <= 2 else [full[0], " ".join(full[1:])])
        heights = [(66 + 17 * (len(ls) - 1)) if ls else 54 for ls in lines]
        ui.card((x, y, x + w, y + sum(heights)))
        top = y
        for i, ((title, desc, control), h, text_w, desc_lines) in enumerate(zip(rows, heights, widths, lines)):
            box = (x, top, x + w, top + h)
            if i:
                ui.divider(x + 16, x + w - 16, top)
            if control:
                control(ui, x + w - 18, top + h / 2, box)
            if desc:
                ui.text(x + 18, top + 15, title, 13, FG, max_w=text_w)
                for j, line in enumerate(desc_lines):
                    ui.text(x + 18, top + 36 + 17 * j, line, 11.5, MUTED, max_w=text_w)
            else:
                ui.text(x + 18, top + h / 2, title, 13, FG, anchor="lm", max_w=text_w)
            top += h
        return top + 26

    def sw(self, rid, on, on_change):
        def control(ui, rx, cy, box):
            if ui.hot(rid):
                ui.rect((box[0] + 1, box[1] + 1, box[2] - 1, box[3] - 1), 9, fill=(255, 255, 255, 7))
            ui.switch(rid, rx - 40, cy - 11, on, on_change, area=box)
        control.width = 70  # espaço que o controle ocupa (o texto da linha usa o resto)
        return control

    def sl(self, rid, value, lo, hi, on_change, fmt, step=1, on_release=None, width=210):
        def control(ui, rx, cy, _box):
            ui.text(rx, cy, fmt(value), 12, MUTED, anchor="rm")
            ui.slider(rid, rx - 64 - width, cy, width, value, lo, hi, on_change, step, on_release)
        return control

    def seg(self, rid, options, current, on_change):
        def control(ui, rx, cy, _box):
            ui.segmented(rid, rx, cy - 16, options, current, on_change, align="r")
        return control

    def btn(self, rid, label, on_click, kind="secondary", icon=None):
        def control(ui, rx, cy, _box):
            ui.button(rid, rx, cy - 16, label, on_click, kind, align="r", icon=icon)
        return control

    def field(self, rid, value, on_commit, placeholder="", width=270, **kw):
        def control(ui, rx, cy, _box):
            ui.entry(rid, (rx - width, cy - 17, rx, cy + 17), value, on_commit, placeholder, **kw)
        return control

    def hk(self, name):
        from deskwidgets.app import HOTKEY_SETTINGS
        label = self.app.setting(HOTKEY_SETTINGS[name])
        error = self.app.hotkey_errors.get(name)

        def control(ui, rx, cy, _box):
            ui.hotkey("atalho:" + name, (rx - 280, cy - 17, rx, cy + 17), label,
                      lambda value: self._set_hotkey(name, value), error=error)
        return control

    def _set_hotkey(self, name, value):
        self.app.set_hotkey(name, value)
        if value:
            self.app.toast(f"Atalho {value} definido")
        self.request()

    def restore(self, ui, x, y, w, rid, fn):
        ui.button(rid, x + w, y - 10, "Restaurar padrão", fn, kind="ghost", align="r", icon="undo", size=12)
        return y + 30

    def toggle_setting(self, name):
        return lambda on: (self.app.set_setting(name, on), self.request())

    # --- Geral ---

    def page_geral(self, ui, x, w):
        app = self.app
        y = self.section(ui, x, 4, "Comportamento")
        y = self.rows(ui, x, y, w, [
            ("Iniciar com o Windows", "Abre os widgets quando você entra no Windows",
             self.sw("iniciar", startup_enabled(), self._set_startup)),
            ("Travar posições", "Evita arrastar sem querer (o modo edição continua liberado)",
             self.sw("travado", app.locked, lambda on: app.set_locked(on))),
            ("Abrir widget pequeno com o mouse", "Parar o mouse sobre um widget pequeno o abre por um instante",
             self.sw("expandir_hover", app.setting("expandir_hover"), self.toggle_setting("expandir_hover"))),
            ("Cliques abrem programas e sites", "Gerenciador de Tarefas, previsão do tempo, GitHub…",
             self.sw("atalhos_clique", app.setting("atalhos_clique"), self.toggle_setting("atalhos_clique"))),
            ("Dicas ao passar o mouse", "Explicações curtas sobre o que está sob o mouse",
             self.sw("dicas", app.setting("dicas"), self.toggle_setting("dicas"))),
            ("Economizar energia na bateria", "Atualiza menos e pausa as animações contínuas",
             self.sw("economia_bateria", app.setting("economia_bateria"), self.toggle_setting("economia_bateria"))),
        ])
        y = self.section(ui, x, y, "Organizar")
        y = self.rows(ui, x, y, w, [
            ("Modo edição", "Feche, redimensione e mova todos os widgets de uma vez",
             self.btn("editar", "Editar widgets", app.start_edit, "primary", icon="edit")),
            ("Organizar automaticamente", "Empilha os widgets em colunas na direita da tela",
             self.btn("organizar", "Organizar", app.arrange)),
        ])
        y = self.section(ui, x, y, "Layouts")
        layouts = app.cfg["app"].get("layouts", {})
        rows = []
        for name, snap in layouts.items():
            count = sum(1 for state in snap.values() if state.get("visible"))
            rows.append((name, f"{count} widget{'s' if count != 1 else ''}", self._layout_buttons(name)))
        rows.append(("Salvar layout atual", "Guarda a posição e o tamanho de cada widget",
                     self.btn("salvar_layout", "Salvar…", app.save_layout, icon="add")))
        y = self.rows(ui, x, y, w, rows)
        return self.restore(ui, x, y, w, "restaurar_geral", self._restore_behavior)

    def _layout_buttons(self, name):
        def control(ui, rx, cy, _box):
            bw = ui.button("excluir:" + name, rx, cy - 16, "Excluir", lambda: self.app.delete_layout(name),
                           align="r")
            ui.button("aplicar:" + name, rx - bw - 8, cy - 16, "Aplicar", lambda: self.app.apply_layout(name),
                      align="r")
        return control

    def _set_startup(self, on):
        try:
            set_startup(on)
        except OSError:
            self.app.toast("Não deu para mudar a inicialização", icon="close", color=(255, 184, 0))
        self.request()

    def _restore_behavior(self):
        self._restore_keys(BEHAVIOR_KEYS, "Comportamento restaurado")

    def _restore_keys(self, keys, message):
        cfg = self.app.cfg["app"]
        old = {k: cfg[k] for k in keys if k in cfg}
        if not old:
            self.app.toast("Já está tudo no padrão")
            return
        for k in keys:
            cfg.pop(k, None)
        self.app.save()
        self.app.apply_settings(rebuild=True)
        self.request()

        def undo():
            cfg.update(old)
            self.app.save()
            self.app.apply_settings(rebuild=True)
            self.request()
        self.app.toast(message, action=undo)

    # --- Aparência ---

    def page_aparencia(self, ui, x, w):
        app = self.app
        blur, tint = app.setting("desfoque"), app.setting("escurecimento")
        y = 4
        self._draw_preview(ui, x, y, w, 150, blur, tint)
        y += 150 + 26
        y = self.section(ui, x, y, "Vidro")
        y = self.rows(ui, x, y, w, [
            ("Desfoque", "Quanto o papel de parede fica borrado atrás dos widgets",
             self.sl("desfoque", blur, 8, 40, lambda v: self._live("desfoque", v), lambda v: f"{v} px")),
            ("Escurecimento", "Deixa o texto legível mesmo em papéis de parede claros",
             self.sl("escurecimento", round(tint * 100), 15, 70, lambda v: self._live("escurecimento", v / 100),
                     lambda v: f"{v}%")),
        ])
        following = not app.setting("texto")
        y = self.section(ui, x, y, "Texto e transparência")
        y = self.rows(ui, x, y, w, [
            ("Seguir o tamanho de texto do Windows", "Configurações → Acessibilidade → Tamanho do texto",
             self.sw("texto_auto", following, self._set_follow_text)),
            None if following else (
                "Tamanho do texto", None,
                self.sl("texto", round(app.text_scale * 100), 90, 120, self._set_text_size, lambda v: f"{v}%",
                        on_release=lambda: app.apply_settings())),
            ("Esmaecer sem o mouse", "Os widgets ficam mais transparentes até o mouse passar por cima",
             self.sw("esmaecer", app.setting("esmaecer"), self.toggle_setting("esmaecer"))),
            None if not app.setting("esmaecer") else (
                "Opacidade sem o mouse", None,
                self.sl("opacidade_ociosa", round(app.setting("opacidade_ociosa") * 100), 40, 95,
                        lambda v: self._live("opacidade_ociosa", v / 100), lambda v: f"{v}%")),
        ])
        y = self.section(ui, x, y, "Movimento")
        y = self.rows(ui, x, y, w, [
            ("Animações", "Ao mudar de tamanho, reorganizar e abrir menus",
             self.seg("animacoes", list(ANIMATION_SHORT.items()), app.setting("animacoes"),
                      lambda v: (app.set_setting("animacoes", v), self.request()))),
        ])
        return self.restore(ui, x, y, w, "restaurar_aparencia",
                            lambda: self._restore_keys(APPEARANCE_KEYS, "Aparência restaurada"))

    def _live(self, name, value):
        """Slider de aparência: a prévia muda já e os widgets acompanham (no máximo ~7 vezes por segundo)."""
        self.app.cfg["app"][name] = value
        self.request()
        self._throttle(name, lambda: self.app.set_setting(name, value))

    def _set_follow_text(self, follow):
        self.app.set_setting("texto", 0.0 if follow else round(self.app.text_scale, 2))
        self.request()

    def _set_text_size(self, v):
        self.app.cfg["app"]["texto"] = v / 100
        self.app.save()
        self.app._read_preferences()
        self.request()

    def _draw_preview(self, ui, x, y, w, h, blur, tint):
        """Um widget de verdade sobre o papel de parede, com o vidro das opções atuais."""
        s = self.s
        ox, oy = self._client_origin()
        sx = ox + round((ui.ox + x) * s)
        sy = oy + round((ui.oy + y + ui.dy) * s)
        pw, ph = round(w * s), round(h * s)
        if self._preview_content is None:
            key = next((k for k in ("pomodoro", "sistema", "midia", "github") if k in self.app.widgets), "sistema")
            live = self.app.widgets.get(key)
            content = live.snapshot("p", s) if live else WIDGET_TYPES[key].preview(self.app, "p", s)
            self._preview_content = content
        content = self._preview_content
        cache_key = (sx, sy, pw, ph, blur, tint, self.app.backdrop.version, id(content))
        if not self._preview or self._preview[0] != cache_key:
            stage = self.app.backdrop.region(sx, sy, pw, ph, sharp=True).convert("RGBA")
            cx = sx + (pw - content.width) // 2
            cy = sy + (ph - content.height) // 2
            glass = self.app.backdrop.glass_with(content, 22 * s, cx, cy, blur, tint)
            stage.alpha_composite(glass, (cx - sx, cy - sy))
            stage.putalpha(rounded_mask(pw, ph, 12 * s))
            self._preview = (cache_key, stage)
        ui.image(self._preview[1], x, y)
        ui.rect((x, y, x + w, y + h), 12, outline=(255, 255, 255, 30))
        ui.text(x + 14, y + h - 14, "Prévia ao vivo", 10.5, (255, 255, 255, 200), "seguisb.ttf", anchor="ls")

    # --- Galeria ---

    def page_widgets(self, ui, x, w):
        app = self.app
        bw = ui.button("editar", x, 4, "Editar widgets", app.start_edit, "primary", icon="edit")
        ui.button("organizar", x + bw + 8, 4, "Organizar", app.arrange, icon="refresh")
        ui.text(x + w, 20, "Clique direito num widget para mais opções", 11, MUTED, anchor="rm")
        y = 56
        gap = 16
        tile_w, tile_h = (w - gap) / 2, 262
        stage_h = 146
        for i, (key, cls) in enumerate(WIDGET_TYPES.items()):
            tx = x + (i % 2) * (tile_w + gap)
            ty = y + (i // 2) * (tile_h + gap)
            visible = key in app.widgets
            ui.rect((tx, ty, tx + tile_w, ty + tile_h), 12, fill=(255, 255, 255, 11),
                    outline=ACCENT + (150,) if visible else (255, 255, 255, 16), width=1.2 if visible else 1)
            ui.rect((tx + 8, ty + 8, tx + tile_w - 8, ty + 8 + stage_h), 9, fill=(0, 0, 0, 34))
            thumb = self._thumbnail(key, ui, tx + 8, ty + 8, tile_w - 16, stage_h)
            if thumb is not None:
                ui.image(thumb, tx + 8 + (tile_w - 16 - thumb.width / self.s) / 2,
                         ty + 8 + (stage_h - thumb.height / self.s) / 2)
            ny = ty + stage_h + 22
            ui.text(tx + 16, ny, cls.title, 14, FG, "seguisb.ttf", anchor="lm", max_w=tile_w - 90)
            ui.switch("ver:" + key, tx + tile_w - 56, ny - 11, visible,
                      lambda on, k=key: self._set_visible(k, on))
            for j, line in enumerate(ui.wrap(cls.description, 11.5, tile_w - 32)[:2]):
                ui.text(tx + 16, ny + 18 + j * 17, line, 11.5, MUTED)
            ui.button("opcoes:" + key, tx + 10, ty + tile_h - 40, "Opções", lambda k=key: self.go(k), kind="ghost",
                      h=30, size=12)
        rows = (len(WIDGET_TYPES) + 1) // 2
        return y + rows * (tile_h + gap)

    def _thumbnail(self, key, ui, x, y, w, h):
        visible = key in self.app.widgets
        cached = self.thumbs.get(key)
        if cached is None or cached[0] != visible:
            s = self.s
            ox, oy = self._client_origin()
            screen = (ox + round((ui.ox + x) * s), oy + round((ui.oy + y) * s))
            try:
                img = widget_thumbnail(self.app, key, s * 0.72, (w - 24) * s, (h - 20) * s, screen)
            except Exception:
                img = None  # uma miniatura com problema não pode travar as configurações
            self.thumbs[key] = cached = (visible, img)
        return cached[1]

    def _set_visible(self, key, on):
        if self.app.set_visible(key, on) and not on:
            self.app.toast(f"{WIDGET_TYPES[key].title} oculto", action=lambda: self.app.set_visible(key, True),
                           icon="close", color=(255, 255, 255))
        self.request()

    # --- páginas de cada widget ---

    def page_widget(self, ui, x, w):
        key = self.page
        widget = self.app.widgets.get(key)
        cfg = self.app.cfg[key]
        y = 4
        if widget is None:
            ui.rect((x, y, x + w, y + 56), 10, fill=(255, 184, 0, 22), outline=(255, 184, 0, 90))
            ui.text(x + 18, y + 28, "Este widget está oculto.", 12.5, FG, "seguisb.ttf", anchor="lm")
            ui.button("mostrar", x + w - 14, y + 12, "Mostrar na tela", lambda: self._set_visible(key, True),
                      "primary", align="r")
            y += 56 + 22
        current = (widget.mode if widget else cfg.get("tamanho", "g"))
        cls = WIDGET_TYPES[key]
        sizes = [(m, SIZE_NAMES[m]) for m in cls.sizes]
        y = self.section(ui, x, y, "Visual")
        y = self.rows(ui, x, y, w, [
            ("Tamanho", "Também dá para mudar com o botão no canto ou com dois cliques",
             self.seg("tamanho:" + key, sizes, current, lambda m: self._set_size(key, m))),
        ])
        return getattr(self, "widget_" + key)(ui, x, y, w, widget, cfg)

    def _set_size(self, key, mode):
        widget = self.app.widgets.get(key)
        if widget:
            widget.set_mode(mode)
            self.win.after(ANIM_MS + 60, self.request)
        else:
            self.app.cfg[key]["tamanho"] = mode
            self.app.save()
        self.request()

    def widget_sistema(self, ui, x, y, w, widget, cfg):
        app = self.app
        y = self.section(ui, x, y, "Clima")
        field_box = {}

        def city_control(ui_, rx, cy, _box):
            field_box["box"] = (rx - 300, cy + 20, 300)
            ui_.entry("cidade", (rx - 300, cy - 17, rx, cy + 17), cfg.get("place") or cfg.get("city", ""),
                      lambda _t: None, "Digite e escolha na lista", on_type=self.city.typed, on_key=self._city_key)

        place = cfg.get("place") or cfg.get("city")
        y = self.rows(ui, x, y, w, [
            ("Cidade", f"Agora: {place}" if place else "Nenhuma cidade escolhida", city_control),
            ("Atualizar agora", "O clima atualiza sozinho a cada 15 minutos",
             self.btn("atualizar_clima", "Atualizar", widget.update_weather if widget else None, icon="refresh")
             if widget else None),
            ("Avisar quando for chover", NOTIFY_LABELS["chuva"][1],
             self.sw("aviso:chuva", app.notify_enabled("chuva"), lambda on: self._notify("chuva", on))),
        ])
        if "box" in field_box:  # sugestões por cima do resto da página
            fx, fy, fw = field_box["box"]
            ui.overlay()
            self.city.draw(ui, fx, fy + 4, fw, self._pick_city)
        return y

    def _city_key(self, keysym):
        if self.city.key(keysym, self._pick_city):
            return True
        if keysym == "Return":
            text = self.entry_text("cidade").strip()
            if text:
                self._apply_city(text, None)
            return False  # deixa o Enter tirar o foco do campo
        return False

    def _pick_city(self, place):
        self.set_entry("cidade", place["label"])
        self.city.clear()
        self.canvas.focus_set()
        self._apply_city(place["label"], place)

    def _apply_city(self, text, place):
        system = self.app.widgets.get("sistema")
        if system:
            system.set_city(text, place)
        elif place:
            self.app.cfg["sistema"].update(city=place["label"], lat=place["lat"], lon=place["lon"],
                                           place=place["label"])
        else:
            self.app.cfg["sistema"].update(city=text, lat=None, lon=None, place=None)
        self.app.save()
        self.app.toast(f"Cidade: {place['label'] if place else text}", icon="location", color=ACCENT)
        self.request()

    def _notify(self, kind, on):
        self.app.set_notify(kind, on)
        self.request()

    def widget_midia(self, ui, x, y, w, widget, cfg):
        current = cfg.get("fonte", "auto")

        def set_source(value):
            if widget:
                widget.set_source(value)
            else:
                cfg["fonte"] = value
                self.app.save()
            self.request()

        y = self.section(ui, x, y, "Reprodução")
        return self.rows(ui, x, y, w, [
            ("Mostrar", "Qual player o widget acompanha quando há mais de um tocando",
             self.seg("fonte", list(SOURCE_SHORT.items()), current, set_source)),
            ("Roda do mouse", "Sobre o widget, ajusta o volume do Windows", None),
        ])

    def widget_github(self, ui, x, y, w, widget, cfg):
        app = self.app

        def set_user(text):
            user = text.strip().lstrip("@")
            if not user or user == cfg.get("usuario"):
                return
            if widget:
                widget.set_user(user)
            else:
                cfg["usuario"] = user
                app.save()
            app.toast(f"GitHub: @{user}")
            self.request()

        y = self.section(ui, x, y, "Conta")
        y = self.rows(ui, x, y, w, [
            ("Usuário", "Com o GitHub CLI (gh) conectado, entram também os repositórios privados",
             self.field("usuario", cfg.get("usuario", ""), set_user, "seu-usuario")),
            ("Atualizar agora", "As atividades atualizam sozinhas a cada 10 minutos",
             self.btn("atualizar_github", "Atualizar", widget.update_data, icon="refresh") if widget else None),
            ("Avisar pedidos de revisão", NOTIFY_LABELS["github"][1],
             self.sw("aviso:github", app.notify_enabled("github"), lambda on: self._notify("github", on))),
        ])
        return y

    def widget_pomodoro(self, ui, x, y, w, widget, cfg):
        from deskwidgets.widgets.pomodoro import POMODORO_DEFAULTS, fmt_minutes
        app = self.app
        durations = {**POMODORO_DEFAULTS, **cfg.get("duracoes", {})}

        def set_duration(phase, minutes):
            if widget:
                self._throttle("dur:" + phase, lambda: widget.set_duration(phase, minutes))
            cfg.setdefault("duracoes", {})[phase] = minutes
            app.save()
            self.request()

        def set_every(n):
            if widget:
                widget.set_every(n)
            else:
                cfg["longa_a_cada"] = n
                app.save()
            self.request()

        rows = []
        for phase, (label, lo, hi, step) in PHASE_RANGES.items():
            rows.append((label, None, self.sl("dur:" + phase, durations[phase], lo, hi,
                                              lambda v, p=phase: set_duration(p, v),
                                              lambda v: fmt_minutes(v), step=step)))
        rows.append(("Pausa longa a cada", None,
                     self.seg("longa_a_cada", [(n, f"{n}") for n in range(2, 7)], cfg.get("longa_a_cada", 4),
                              set_every)))
        y = self.section(ui, x, y, "Durações")
        y = self.rows(ui, x, y, w, rows)

        def set_auto(on):
            cfg["auto"] = on
            app.save()
            self.request()

        y = self.section(ui, x, y, "Comportamento")
        y = self.rows(ui, x, y, w, [
            ("Começar a próxima fase sozinho", "Sem precisar clicar quando o foco ou a pausa acaba",
             self.sw("auto", cfg.get("auto", False), set_auto)),
            ("Avisar no fim de cada fase", NOTIFY_LABELS["pomodoro"][1],
             self.sw("aviso:pomodoro", app.notify_enabled("pomodoro"), lambda on: self._notify("pomodoro", on))),
            ("Atalho global", "Começa ou pausa de qualquer programa", self.hk("pomodoro")),
        ])
        stats = cfg.get("hoje") or {}
        if stats.get("data") != date.today().isoformat():
            stats = {"pomodoros": 0, "minutos": 0}
        n = stats["pomodoros"]
        y = self.section(ui, x, y, "Hoje")
        return self.rows(ui, x, y, w, [
            (f"{n} pomodoro{'s' if n != 1 else ''}  ·  {fmt_minutes(stats['minutos'])} de foco",
             "A contagem recomeça sozinha à meia-noite",
             self.btn("zerar", "Zerar o dia", lambda: self._reset_day(widget, cfg)) if n else None),
        ])

    def _reset_day(self, widget, cfg):
        old_today, old_state = copy.deepcopy(cfg.get("hoje")), copy.deepcopy(cfg.get("estado"))
        if widget:
            widget.reset_day()
        else:
            cfg.pop("hoje", None)
            self.app.save()
        self.request()

        def undo():
            cfg["hoje"] = old_today
            if old_state:
                cfg["estado"] = old_state
            if widget and widget.alive:
                widget.cycle = int((old_state or {}).get("ciclo", 0))
                widget.refresh()
            self.app.save()
            self.request()
        self.app.toast("Dia zerado", action=undo)

    # --- Notificações ---

    def page_avisos(self, ui, x, w):
        app = self.app
        rows = [(title, desc, self.sw("aviso:" + kind, app.notify_enabled(kind),
                                      lambda on, k=kind: self._notify(k, on)))
                for kind, (title, desc) in NOTIFY_LABELS.items()]
        y = self.section(ui, x, 4, "Avisar quando")
        y = self.rows(ui, x, y, w, rows)
        y = self.rows(ui, x, y, w, [
            ("Testar", "Mostra uma notificação de exemplo agora",
             self.btn("testar", "Enviar teste",
                      lambda: app.tray.notify("Tudo certo", "As notificações dos widgets estão funcionando."))),
        ])

        def restore():
            old = copy.deepcopy(app.cfg["app"].get("notificacoes"))
            app.cfg["app"].pop("notificacoes", None)
            app.save()
            self.request()

            def undo():
                if old is not None:
                    app.cfg["app"]["notificacoes"] = old
                    app.save()
                    self.request()
            app.toast("Notificações restauradas", action=undo)
        return self.restore(ui, x, y, w, "restaurar_avisos", restore)

    # --- Atalhos ---

    def page_atalhos(self, ui, x, w):
        from deskwidgets.app import HOTKEY_LABELS
        y = self.section(ui, x, 4, "Teclado (funcionam em qualquer programa)")
        rows = []
        for name, label in HOTKEY_LABELS.items():
            error = self.app.hotkey_errors.get(name)
            desc = f"{error} já é usado por outro programa" if error else "Clique no campo e aperte a combinação"
            rows.append((label, desc, self.hk(name)))
        y = self.rows(ui, x, y, w, rows)
        y = self.section(ui, x, y, "Mouse")
        return self.rows(ui, x, y, w, [(title, desc, None) for title, desc in TIPS])

    # --- Sobre ---

    def page_sobre(self, ui, x, w):
        app = self.app
        icon = tray_icon_image(round(56 * self.s))
        ui.image(icon, x, 4)
        ui.text(x + 74, 12, "Widgets da área de trabalho", 18, FG, "seguisb.ttf")
        kind = "instalado" if FROZEN else "rodando pelo código"
        ui.text(x + 74, 40, f"Versão {__version__} ({kind})  ·  Python, Tkinter e Pillow", 12, MUTED)
        y = 90
        if app.update:
            update = self.btn("atualizar", f"Atualizar para {app.update['version']}", app.install_update,
                              "primary", icon="refresh")
        else:
            update = self.btn("procurar", "Procurar agora", lambda: app.check_updates(manual=True), icon="refresh")
        y = self.rows(ui, x, y, w, [
            ("Atualizações", app.update_status, update),
        ])
        y = self.rows(ui, x, y, w, [
            ("Boas-vindas", "Rever a apresentação e escolher os widgets de novo",
             self.btn("boas_vindas", "Abrir", app.open_welcome)),
            ("Pasta das configurações", str(DATA), self.btn("pasta", "Abrir pasta",
                                                            lambda: open_target(str(DATA)), icon="folder")),
            ("Restaurar todas as configurações", "Aparência, comportamento, atalhos e notificações (layouts ficam)",
             self.btn("restaurar_tudo", "Restaurar", app.reset_all_settings, kind="danger")),
        ])
        return y
