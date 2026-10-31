"""Boas-vindas em 3 passos: escolher os widgets, personalizar e conhecer os atalhos."""

from deskwidgets.startup import set_startup, startup_enabled
from deskwidgets.theme import ACCENT, FAINT, FG, MUTED
from deskwidgets.ui.gallery import TIPS, widget_thumbnail
from deskwidgets.ui.panel import Panel
from deskwidgets.ui.prompt import CitySuggest
from deskwidgets.widgets import WIDGET_TYPES

STEPS = [
    ("Escolha seus widgets", "Clique para marcar os que você quer na área de trabalho. Dá para mudar depois."),
    ("Deixe com a sua cara", "Algumas informações para os widgets mostrarem o que importa para você."),
    ("Tudo pronto!", "Alguns atalhos para aproveitar melhor."),
]


class Welcome(Panel):
    TITLE = "Bem-vindo aos widgets"
    W, H = 760, 580

    def __init__(self, app):
        self.step = 0
        self.chosen = {key: key in app.widgets for key in WIDGET_TYPES}
        if not any(self.chosen.values()):
            self.chosen = dict.fromkeys(WIDGET_TYPES, True)
        system, github = app.cfg["sistema"], app.cfg["github"]
        self.city_text = system.get("place") or system.get("city") or ""
        self.city_place = None
        self.user = github.get("usuario", "")
        self.startup = startup_enabled() or not app.cfg["app"].get("dicas_vistas")
        self.city = CitySuggest(self)
        self.thumbs = {}
        super().__init__(app)

    def on_close(self):
        if self.app.welcome_window is self:
            self.app.welcome_window = None
        if not self.app.cfg["app"].get("dicas_vistas"):
            self.app.cfg["app"]["dicas_vistas"] = True
            self.app.save()

    # --- desenho ---

    def draw(self, ui):
        W, H = self.W, self.H
        # pontos dos passos
        for i in range(len(STEPS)):
            cx = W / 2 + (i - 1) * 22
            active = i == self.step
            width = ui.anim(f"dot{i}", 22.0 if active else 8.0, 220)
            ui.rect((cx - width / 2, 22, cx + width / 2, 30), 4,
                    fill=ACCENT + (255,) if i <= self.step else (255, 255, 255, 50))
        title, subtitle = STEPS[self.step]
        ui.text(W / 2, 62, title, 24, FG, "seguisb.ttf", anchor="ma")
        ui.text(W / 2, 100, subtitle, 12.5, MUTED, anchor="ma", max_w=W - 120)
        [self.step_widgets, self.step_personal, self.step_tips][self.step](ui, 56, 140, W - 112)

        # rodapé
        y = H - 64
        ui.rect((0, y - 12, W, y - 11), 0, fill=(255, 255, 255, 14))
        last = self.step == len(STEPS) - 1
        nw = ui.button("proximo", W - 40, y, "Começar a usar" if last else "Continuar",
                       self.finish if last else self.next, "primary", align="r", w=150, h=36)
        if self.step:
            ui.button("voltar", W - 40 - nw - 10, y, "Voltar", self.back, align="r", w=100, h=36)
        if not last:
            ui.button("pular", 40, y, "Pular", self.close, kind="ghost", h=36)

    def step_widgets(self, ui, x, y, w):
        gap = 16
        tile_w, tile_h = (w - gap) / 2, 172
        for i, (key, cls) in enumerate(WIDGET_TYPES.items()):
            tx = x + (i % 2) * (tile_w + gap)
            ty = y + (i // 2) * (tile_h + gap)
            on = self.chosen[key]
            rid = "tile:" + key
            hot = ui.hot(rid)
            ui.rect((tx, ty, tx + tile_w, ty + tile_h), 12,
                    fill=(255, 255, 255, 18 if hot else 11),
                    outline=ACCENT + (255,) if on else (255, 255, 255, 22), width=2 if on else 1)
            stage_h = 112
            thumb = self._thumb(key, ui, tx + 10, ty + 10, tile_w - 20, stage_h)
            if thumb is not None:
                ui.image(thumb, tx + 10 + (tile_w - 20 - thumb.width / self.s) / 2,
                         ty + 10 + (stage_h - thumb.height / self.s) / 2)
            ui.text(tx + 16, ty + stage_h + 26, cls.title, 13.5, FG, "seguisb.ttf", anchor="lm", max_w=tile_w - 60)
            ui.text(tx + 16, ty + stage_h + 46, cls.description, 11, MUTED, anchor="lm", max_w=tile_w - 32)
            # marca de escolhido
            cx, cy = tx + tile_w - 22, ty + stage_h + 26
            if on:
                ui.circle(cx, cy, 10, fill=ACCENT + (255,))
                ui.icon(cx, cy, "check", 11, FG)
            else:
                ui.circle(cx, cy, 10, outline=(255, 255, 255, 120), width=1.3)
            ui.region(rid, (tx, ty, tx + tile_w, ty + tile_h), on_click=lambda k=key: self._toggle(k))

    def _thumb(self, key, ui, x, y, w, h):
        if key not in self.thumbs:
            s = self.s
            ox, oy = self._client_origin()
            try:
                self.thumbs[key] = widget_thumbnail(self.app, key, s * 0.6, (w - 16) * s, (h - 12) * s,
                                                    (ox + round(x * s), oy + round(y * s)))
            except Exception:
                self.thumbs[key] = None
        return self.thumbs[key]

    def _toggle(self, key):
        self.chosen[key] = not self.chosen[key]
        self.request()

    def step_personal(self, ui, x, y, w):
        label_w = 200
        fx = x + label_w
        fw = w - label_w

        ui.text(x, y + 18, "Cidade do clima", 13, FG, "seguisb.ttf", anchor="lm")
        ui.text(x, y + 38, "Para a previsão do tempo", 11, MUTED, anchor="lm")
        ui.entry("cidade", (fx, y, fx + fw, y + 36), self.city_text, self._city_commit, "Ex.: São Paulo",
                 on_type=self._city_typed, on_key=self._city_key)
        sug_y = y + 40
        y += 74
        ui.text(x, y + 18, "Usuário do GitHub", 13, FG, "seguisb.ttf", anchor="lm")
        ui.text(x, y + 38, "Opcional", 11, MUTED, anchor="lm")
        ui.entry("usuario", (fx, y, fx + fw, y + 36), self.user, lambda t: setattr(self, "user", t.strip()),
                 "seu-usuario")
        y += 74
        ui.text(x, y + 11, "Iniciar com o Windows", 13, FG, "seguisb.ttf", anchor="lm")
        ui.text(x, y + 31, "Os widgets abrem sozinhos quando você entra", 11, MUTED, anchor="lm")
        ui.switch("iniciar", fx, y, self.startup, lambda on: (setattr(self, "startup", on), self.request()))
        if not self.chosen.get("sistema"):
            ui.text(fx, y + 60, "O widget de clima não foi escolhido; a cidade fica guardada para depois.", 11,
                    FAINT)
        ui.overlay()
        self.city.draw(ui, fx, sug_y, fw, self._city_pick)

    def _city_typed(self, text):
        self.city_text, self.city_place = text, None
        self.city.typed(text)

    def _city_commit(self, text):
        self.city_text = text.strip()

    def _city_key(self, keysym):
        return self.city.key(keysym, self._city_pick)

    def _city_pick(self, place):
        self.city_text, self.city_place = place["label"], place
        self.set_entry("cidade", place["label"])
        self.city.clear()
        self.canvas.focus_set()

    def step_tips(self, ui, x, y, w):
        hotkey = self.app.tray.hotkey
        pomodoro = self.app.tray.hotkeys.get("pomodoro")
        items = []
        if hotkey:
            items.append((hotkey, "Mostra os widgets na frente das janelas"))
        if pomodoro:
            items.append((pomodoro, "Começa ou pausa o Pomodoro"))
        items += [(title, desc) for title, desc in TIPS[:4]]
        for title, desc in items:
            if "+" in title:  # atalho de teclado: teclas desenhadas
                kx = x
                for part in title.split("+"):
                    tw = ui.text_width(part, 11.5, "seguisb.ttf") + 16
                    ui.rect((kx, y, kx + tw, y + 24), 5, fill=(255, 255, 255, 22), outline=(255, 255, 255, 40))
                    ui.text(kx + tw / 2, y + 12, part, 11.5, FG, "seguisb.ttf", anchor="mm")
                    kx += tw + 5
            else:
                ui.text(x, y + 12, title, 13, FG, "seguisb.ttf", anchor="lm")
            lines = ui.wrap(desc, 12, w - 190)[:2]
            for j, line in enumerate(lines):
                ui.text(x + 190, y + 4 + j * 17, line, 12, MUTED)
            y += 26 + 17 * max(1, len(lines))

    # --- passos ---

    def next(self):
        if self.step == 0 and not any(self.chosen.values()):
            self.app.toast("Escolha pelo menos um widget", icon="close", color=(255, 184, 0))
            return
        self.step += 1
        self.canvas.focus_set()
        self.request()

    def back(self):
        self.step = max(0, self.step - 1)
        self.city.clear()
        self.canvas.focus_set()
        self.request()

    def finish(self):
        app = self.app
        for key, on in self.chosen.items():
            if on != (key in app.widgets):
                app.set_visible(key, on)
        city = self.city_text.strip()
        system_cfg = app.cfg["sistema"]
        if city and city != (system_cfg.get("place") or system_cfg.get("city")):
            system = app.widgets.get("sistema")
            if system:
                system.set_city(city, self.city_place)
            elif self.city_place:
                p = self.city_place
                system_cfg.update(city=p["label"], lat=p["lat"], lon=p["lon"], place=p["label"])
            else:
                system_cfg.update(city=city, lat=None, lon=None, place=None)
        user = self.user.strip().lstrip("@")
        if user and user != app.cfg["github"].get("usuario"):
            github = app.widgets.get("github")
            if github:
                github.set_user(user)
            else:
                app.cfg["github"]["usuario"] = user
        try:
            set_startup(self.startup)
        except OSError:
            pass
        app.cfg["app"]["dicas_vistas"] = True
        app.save()
        self.close()
        app.toast("Tudo pronto!", detail="Clique direito num widget para ver as opções")
