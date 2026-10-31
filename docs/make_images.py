"""Gera as imagens do README (docs/images) com o próprio código dos widgets.

    python docs/make_images.py

Os widgets são desenhados com dados de exemplo sobre um papel de parede abstrato gerado
aqui mesmo, então as imagens não mostram nada pessoal e podem ser refeitas quando o
visual mudar. Uma janela de configurações aparece por um instante durante a geração.
"""

import random
import sys
import time
import tkinter as tk
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from types import SimpleNamespace

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs" / "images"
sys.path.insert(0, str(ROOT))

from deskwidgets.backdrop import Backdrop, extend_edges  # noqa: E402
from deskwidgets.config import SETTINGS  # noqa: E402
from deskwidgets.theme import FG, MUTED, SATURATION, font, icon_font  # noqa: E402
from deskwidgets.widgets import WIDGET_TYPES  # noqa: E402

S = 1.5                 # escala dos desenhos (imagens nítidas em telas de alta densidade)
SCREEN = (2400, 1500)   # a "tela" falsa onde tudo acontece
OFFSET = 300            # quanto a tela falsa sobra antes da tela de verdade (janelas de configurações)


# ---------- papel de parede e vidro ----------

def wallpaper(size, seed=7):
    """Fundo abstrato: manchas de cor bem desfocadas sobre azul-escuro."""
    w, h = size
    small = Image.new("RGB", (w // 4, h // 4), (12, 16, 34))
    d = ImageDraw.Draw(small)
    rnd = random.Random(seed)
    blobs = [((10, 132, 255), 0.30, 0.28, 0.42), ((191, 90, 242), 0.72, 0.30, 0.40),
             ((255, 118, 82), 0.80, 0.82, 0.36), ((48, 209, 160), 0.18, 0.86, 0.30),
             ((255, 200, 90), 0.52, 0.62, 0.20), ((90, 80, 255), 0.45, 0.05, 0.30)]
    for color, fx, fy, fr in blobs:
        cx, cy, r = fx * small.width, fy * small.height, fr * small.width
        cx += rnd.uniform(-20, 20)
        d.ellipse((cx - r, cy - r * 0.8, cx + r, cy + r * 0.8), fill=color)
    small = small.filter(ImageFilter.GaussianBlur(small.width / 9))
    big = small.resize(size, Image.BICUBIC)
    grain = Image.effect_noise(size, 6).convert("RGB")  # um pouco de granulado tira o "degradê de computador"
    return Image.blend(big, grain, 0.035)


class FakeBackdrop(Backdrop):
    """O vidro dos widgets, mas sobre o papel de parede gerado em vez do de verdade."""

    def __init__(self, app, wall):
        self.app, self.s, self.version, self.origin = app, S, 1, (0, 0)
        D = self.DOWN
        small = wall.resize((wall.width // D, wall.height // D), Image.LANCZOS)
        from PIL import ImageEnhance
        small = ImageEnhance.Color(small).enhance(SATURATION)
        self.sharp = extend_edges(small, self.PAD)
        blurred = small.filter(ImageFilter.GaussianBlur(app.setting("desfoque") * S / D))
        self.image = extend_edges(blurred, self.PAD)
        self.gray = self.image.convert("L")

    def check(self):
        return False


# ---------- um app de mentira, só com o que o desenho precisa ----------

class FakeApp:
    def __init__(self, wall, root=None):
        self.cfg = {key: {} for key in WIDGET_TYPES}
        self.cfg["app"] = {"desfoque": 22, "escurecimento": 0.34}
        self.cache, self.widgets = {}, {}
        self.text_scale, self.animations, self.saving, self.s = 1.0, True, False, S
        self.locked, self.editing, self.peeking = False, False, False
        self.hotkey_errors, self.update, self.update_status = {}, None, "Você tem a versão mais recente"
        self.settings_window = self.welcome_window = None
        self.root = root
        self.tray = SimpleNamespace(hotkey="Ctrl+Alt+W", hotkeys={"peek": "Ctrl+Alt+W", "pomodoro": "Ctrl+Alt+P"},
                                    pause_hotkeys=lambda on: None, ok=True)
        self.backdrop = FakeBackdrop(self, wall)

    def setting(self, name):
        value = self.cfg["app"].get(name)
        return SETTINGS[name] if value is None else value

    def notify_enabled(self, kind):
        return kind != "cpu"

    def scale_at(self, _point):
        return S

    def changed(self):
        pass

    def save(self):
        pass

    def __getattr__(self, name):  # ações (botões) que as capturas nunca chamam
        return lambda *a, **k: None


# ---------- widgets com dados de exemplo ----------

def album_art(px):
    """Capa de disco inventada: degradê com um "sol" e linhas de horizonte."""
    img = Image.new("RGB", (px, px))
    d = ImageDraw.Draw(img)
    for y in range(px):
        t = y / px
        d.line((0, y, px, y), fill=(round(40 + 200 * t), round(30 + 40 * t), round(120 - 40 * t)))
    r = px * 0.26
    d.ellipse((px / 2 - r, px * 0.55 - r, px / 2 + r, px * 0.55 + r), fill=(255, 196, 92))
    for i in range(6):
        y = px * (0.62 + i * 0.065)
        d.rectangle((0, y, px, y + px * 0.018 * (i + 1) * 0.6), fill=(60, 20, 90))
    return img


def make_widget(app, key, mode):
    """Instância sem janela (como as miniaturas da galeria), com dados bonitos de exemplo."""
    cls = WIDGET_TYPES[key]
    w = cls.__new__(cls)
    w.app, w.s, w.mode, w.alive = app, S, mode, True
    w.cfg = app.cfg[key]
    w.error, w.loading, w.updated, w._temp, w.hover = None, False, None, False, 0.0
    w.load_sample()
    if key == "midia":
        w.media.update(title="Sunset Drive", artist="Neon Avenue  ·  Night Shift", art=album_art(600),
                       art_key="exemplo", position=96.0, duration=231.0)
    elif key == "sistema":
        w.weather = {"place": "Florianópolis, Santa Catarina", "temp": "24°", "icon": "sun", "desc": "Céu limpo",
                     "minmax": "Máx. 27°   Mín. 18°", "details": "Sensação 25°  ·  Umidade 61%  ·  Vento 12 km/h",
                     "hours": [["Agora", "sun", "24°"], ["15h", "sun", "26°"], ["16h", "clouds", "26°"],
                               ["17h", "clouds", "25°"], ["18h", "drizzle", "22°"], ["19h", "moon", "21°"]]}
    elif key == "github":
        rnd = random.Random(3)
        today = date.today()
        days = []
        for i in range(370, -1, -1):
            day = today - timedelta(days=i)
            busy = 0.25 if day.weekday() >= 5 else 0.7
            count = rnd.choice([0, 1, 2, 3, 5, 8]) if rnd.random() < busy else 0
            if i < 12:  # uma sequência de 12 dias até hoje
                count = max(count, rnd.choice([2, 3, 5]))
            days.append((day, min(4, (count + 1) // 2), count))
        now = datetime.now(timezone.utc)
        rows = [{"icon": icon, "title": title, "info": info, "when": now - timedelta(minutes=m), "url": ""}
                for icon, title, info, m in (
                    ("commit", "Anima o botão de tamanho", "widgets  ·  main", 12),
                    ("pr_merged", "Galeria de widgets nas configurações", "widgets  ·  #18", 95),
                    ("pr_open", "Modo escuro no instalador", "widgets  ·  #21", 260),
                    ("star", "Deu estrela", "python/cpython", 1500),
                    ("issue", "Widget de calendário", "widgets  ·  #22", 2900),
                    ("commit", "Corrige o encaixe entre monitores", "widgets  ·  main", 4400))]
        w.data = {"user": "voce", "days": days, "total": sum(c for _, _, c in days), "rows": rows}
    elif key == "pomodoro":
        app.cfg[key]["hoje"] = {"data": today_iso(), "pomodoros": 3, "minutos": 75}
        w.phase, w.cycle, w.total = "foco", 2, 1500.0
        w.remaining, w.end = 1500.0, time.time() + 13 * 60 + 27
    return w


def today_iso():
    return date.today().isoformat()


def glass_card(app, widget, mode, pos):
    content = widget.render_card(mode)
    return app.backdrop.glass(content, widget.layout(mode)["radius"] * S, *pos)


def scene(app, wall, placements, box):
    """Recorte `box` da tela falsa com os widgets nas posições dadas (em pixels da tela)."""
    img = wall.crop(box).convert("RGBA")
    for widget, mode, (x, y) in placements:
        card = glass_card(app, widget, mode, (x, y))
        shadow = Image.new("RGBA", img.size, (0, 0, 0, 0))
        ImageDraw.Draw(shadow).rounded_rectangle(
            (x - box[0], y - box[1] + 10, x - box[0] + card.width, y - box[1] + card.height + 10),
            radius=24 * S, fill=(0, 0, 0, 70))
        img.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(18)))
        img.alpha_composite(card, (x - box[0], y - box[1]))
    return img


def card_size(widget, mode):
    lay = widget.layout(mode)
    return round(lay["w"] * S), round(lay["h"] * S)


# ---------- imagens ----------

def hero(app, wall):
    """A área de trabalho com os quatro widgets."""
    gap, edge = round(12 * S), round(28 * S)
    W, H = 1260, 1110
    box = (SCREEN[0] - W, 0, SCREEN[0], H)
    right = SCREEN[0] - edge
    cols = [[("sistema", "g"), ("midia", "g"), ("pomodoro", "m")], [("github", "g")]]
    placements = []
    for col in cols:
        y = edge
        width = 0
        for key, mode in col:
            w = make_widget(app, key, mode)
            cw, ch = card_size(w, mode)
            placements.append((w, mode, (right - cw, y)))
            y += ch + gap
            width = max(width, cw)
        right -= width + gap
    img = scene(app, wall, placements, box)
    img.convert("RGB").save(OUT / "desktop.png", optimize=True)


def sizes(app, wall):
    """Os três tamanhos de dois widgets, lado a lado."""
    rows = []
    for key in ("pomodoro", "sistema"):
        placements, x = [], 60
        widgets = []
        for mode in ("p", "m", "g"):
            w = make_widget(app, key, mode)
            widgets.append((w, mode))
        tallest = max(card_size(w, m)[1] for w, m in widgets)
        for w, mode in widgets:
            cw, ch = card_size(w, mode)
            placements.append((w, mode, (x, 60 + (tallest - ch) // 2)))
            x += cw + 50
        rows.append(scene(app, wall, placements, (0, 0, x + 10, tallest + 120)))
    W = max(r.width for r in rows)
    out = Image.new("RGB", (W, sum(r.height for r in rows)))
    y = 0
    for r in rows:
        out.paste(r.convert("RGB").resize((W, round(r.height * W / r.width))) if r.width != W else r.convert("RGB"),
                  (0, y))
        y += r.height
    out.save(OUT / "tamanhos.png", optimize=True)


def save_animation(frames, durations, path, scale=0.8):
    """Animação em WebP: cores completas (o GIF, com 256 cores, apagava o amarelo do sol) e arquivo menor.
    O GitHub mostra WebP animado no README como se fosse um GIF."""
    size = (round(frames[0].width * scale), round(frames[0].height * scale))
    frames = [f.convert("RGB").resize(size, Image.LANCZOS) for f in frames]
    frames[0].save(path, save_all=True, append_images=frames[1:], duration=durations, loop=0, quality=88,
                   method=6)


def resize_gif(app, wall):
    """A animação de mudar de tamanho (pequeno → médio → grande → pequeno), como no app."""
    from deskwidgets.theme import ease_in_out
    key = "sistema"
    states = ["p", "m", "g", "p"]
    w = make_widget(app, key, "g")
    big_w, big_h = card_size(w, "g")
    margin = 50
    x0, y0 = 300, 200                      # canto superior esquerdo do cartão na tela falsa
    box = (x0 - margin, y0 - margin, x0 + big_w + margin, y0 + big_h + margin)
    base = wall.crop(box).convert("RGBA")
    frames, durations = [], []

    def frame(content, radius):
        img = base.copy()
        glass = app.backdrop.glass(content, radius, x0, y0)
        shadow = Image.new("RGBA", img.size, (0, 0, 0, 0))
        ImageDraw.Draw(shadow).rounded_rectangle((margin, margin + 10, margin + glass.width, margin + glass.height + 10),
                                                 radius=radius, fill=(0, 0, 0, 70))
        img.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(16)))
        img.alpha_composite(glass, (margin, margin))
        return img

    for a, b in zip(states, states[1:]):
        old = w.render_card(a).convert("RGBa")
        new = w.render_card(b).convert("RGBa")
        r0, r1 = w.layout(a)["radius"] * S, w.layout(b)["radius"] * S
        frames.append(frame(old.convert("RGBA"), r0))
        durations.append(1100)
        steps = 14
        for i in range(1, steps + 1):
            t = i / steps
            e = ease_in_out(t)
            cw = round(old.width + (new.width - old.width) * e)
            ch = round(old.height + (new.height - old.height) * e)
            layers = []
            for card in (old, new):
                layer = Image.new("RGBa", (cw, ch), (0, 0, 0, 0))
                layer.paste(card, (0, 0))
                layers.append(layer)
            fade = ease_in_out(min(1.0, max(0.0, (t - 0.1) / 0.7)))
            content = Image.blend(layers[0], layers[1], fade).convert("RGBA")
            frames.append(frame(content, r0 + (r1 - r0) * e))
            durations.append(24)
    save_animation(frames, durations, OUT / "tamanhos.webp")


def pomodoro_gif(app, wall):
    """O Pomodoro correndo (acelerado): o anel esvazia e o relógio segue."""
    w = make_widget(app, "pomodoro", "g")
    cw, ch = card_size(w, "g")
    margin = 50
    x0, y0 = 1500, 300
    box = (x0 - margin, y0 - margin, x0 + cw + margin, y0 + ch + margin)
    frames = []
    start = 25 * 60
    for i in range(48):
        left = start - i * 31
        w.end = time.time() + left
        frames.append(scene(app, wall, [(w, "g", (x0, y0))], box))
    save_animation(frames, [110] * len(frames), OUT / "pomodoro.webp")


def window_shot(panel, wall, name):
    """A janela (desenho do painel + barra de título do Windows 11) sobre o papel de parede."""
    panel.render()
    frame = panel.last_frame.convert("RGBA")
    x, y = panel._client_origin()
    x, y = x + OFFSET, y + OFFSET  # nas coordenadas do papel de parede
    title_h = round(32 * S)
    margin = 70
    W, H = frame.width, frame.height + title_h
    box = (x - margin, y - title_h - margin, x + W + margin, y + frame.height + margin)
    img = wall.crop(box).convert("RGBA")
    shadow = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle((margin, margin + 16, margin + W, margin + H + 16), radius=18,
                                             fill=(0, 0, 0, 140))
    img.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(26)))
    window = Image.new("RGBA", (W, H))
    caption = frame.crop((0, 0, W, 6)).resize((1, 1), Image.BOX).getpixel((0, 0))
    window.paste(caption[:3] + (255,), (0, 0, W, title_h))
    window.paste(frame, (0, title_h))
    d = ImageDraw.Draw(window)
    d.text((round(14 * S), title_h / 2), panel.TITLE, font=font("segoeui.ttf", round(12 * S)), fill=FG, anchor="lm")
    icons = icon_font(round(10 * S))
    if icons:
        for i, glyph in enumerate(("", "", "")):  # fechar, maximizar, minimizar
            d.text((W - round((23 + 46 * i) * S), title_h / 2), glyph, font=icons, fill=MUTED, anchor="mm")
    mask = Image.new("L", (W, H), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, W - 1, H - 1), radius=round(8 * S), fill=255)
    window.putalpha(mask)
    ImageDraw.Draw(window).rounded_rectangle((0, 0, W - 1, H - 1), radius=round(8 * S),
                                             outline=(255, 255, 255, 40), width=1)
    img.alpha_composite(window, (margin, margin))
    img.convert("RGB").save(OUT / f"{name}.png", optimize=True)


def windows(wall):
    """Configurações e boas-vindas, abertas por um instante com o app de mentira."""
    from deskwidgets.ui.onboarding import Welcome
    from deskwidgets.ui.settings import SettingsWindow
    root = tk.Tk()
    root.withdraw()
    app = FakeApp(wall, root)
    # a janela abre na tela de verdade: a tela falsa começa OFFSET pixels antes, para sobrar fundo em volta
    app.backdrop.origin = (-OFFSET, -OFFSET)
    for key in WIDGET_TYPES:
        app.widgets[key] = make_widget(app, key, {"sistema": "g", "midia": "g", "github": "p", "pomodoro": "m"}[key])
    app.cfg["sistema"].update(city="Florianópolis", place="Florianópolis, Santa Catarina")
    app.cfg["github"]["usuario"] = "voce"
    app.cfg["app"]["layouts"] = {"Trabalho": {k: {"visible": True} for k in WIDGET_TYPES},
                                 "Jogos": {"midia": {"visible": True}}}

    settings = SettingsWindow(app, "aparencia")
    root.update()
    for page, name in (("aparencia", "config-aparencia"), ("widgets", "config-galeria"),
                       ("pomodoro", "config-pomodoro")):
        settings.go(page)
        for _ in range(3):  # deixa as animações (indicador da barra lateral) chegarem ao fim
            settings.render()
            root.update()
            time.sleep(0.25)
        window_shot(settings, wall, name)
    settings.close()

    welcome = Welcome(app)
    root.update()
    welcome.render()
    time.sleep(0.3)
    window_shot(welcome, wall, "boas-vindas")
    welcome.close()
    root.destroy()


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    wall = wallpaper(SCREEN)
    app = FakeApp(wall)
    for old in OUT.glob("*.gif"):
        old.unlink()
    steps = [("desktop.png", lambda: hero(app, wall)), ("tamanhos.png", lambda: sizes(app, wall)),
             ("tamanhos.webp", lambda: resize_gif(app, wall)), ("pomodoro.webp", lambda: pomodoro_gif(app, wall)),
             ("janelas", lambda: windows(wall))]
    for name, step in steps:
        t = time.perf_counter()
        step()
        print(f"{name:14} {time.perf_counter() - t:5.1f} s")
    for path in sorted(OUT.iterdir()):
        print(f"  {path.name:24} {path.stat().st_size / 1e6:5.2f} MB")


if __name__ == "__main__":
    main()
