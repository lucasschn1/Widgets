"""Widget de clima e sistema (CPU, memória e disco)."""

import math
import os
import shutil
import threading
import time
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from functools import lru_cache

from PIL import Image, ImageDraw, ImageFilter

from deskwidgets.theme import DIVIDER, MUTED, br
from deskwidgets.ui.card import CardWidget
from deskwidgets.ui.drawing import draw_ring
from deskwidgets.util import fetch_json, time_ago
from deskwidgets.win32 import cpu_times, gb, memory, open_target


# ---------- widget: clima e sistema ----------

WMO = {
    0: "Céu limpo",
    1: "Predominantemente limpo",
    2: "Parcialmente nublado",
    3: "Nublado",
    45: "Neblina",
    48: "Neblina com geada",
    51: "Garoa fraca",
    53: "Garoa",
    55: "Garoa forte",
    56: "Garoa congelante",
    57: "Garoa congelante",
    61: "Chuva fraca",
    63: "Chuva",
    65: "Chuva forte",
    66: "Chuva congelante",
    67: "Chuva congelante",
    71: "Neve fraca",
    73: "Neve",
    75: "Neve forte",
    77: "Grãos de neve",
    80: "Pancadas de chuva",
    81: "Pancadas de chuva",
    82: "Pancadas fortes",
    85: "Pancadas de neve",
    86: "Pancadas de neve",
    95: "Trovoada",
    96: "Trovoada com granizo",
    99: "Trovoada com granizo",
}


def weather_icon(code, is_day):
    """Nome do ícone (ver weather_art) para um código de tempo da Open-Meteo."""
    if code in (0, 1):
        return "sun" if is_day else "moon"
    if code == 2:
        return "partly_day" if is_day else "partly_night"
    if code == 3:
        return "clouds"
    if code in (45, 48):
        return "fog"
    if 51 <= code <= 57:
        return "drizzle"
    if 61 <= code <= 67 or 80 <= code <= 82:
        return "rain"
    if 71 <= code <= 77 or code in (85, 86):
        return "snow"
    if code >= 95:
        return "thunder"
    return "clouds"


# ---------- ícones do tempo ----------
# Desenhados com formas e gradientes num quadro de 100×100 unidades, em resolução
# maior, e reduzidos (bordas suaves). Cores no estilo dos ícones de tempo da Apple.

SUN_CORE = ((255, 226, 96), (255, 149, 0))
SUN_RAY = (255, 186, 10, 255)
MOON = ((255, 249, 224), (255, 210, 100))
CLOUD_LIGHT = ((255, 255, 255), (208, 220, 236))
CLOUD_GRAY = ((214, 222, 234), (150, 163, 184))
CLOUD_DARK = ((176, 186, 202), (100, 112, 134))
RAIN = ((110, 214, 255), (10, 132, 255))
BOLT = ((255, 232, 90), (255, 159, 10))
FOG_LINE = (214, 222, 234, 230)


class IconCanvas:
    """Tela para desenhar um ícone em unidades de 0 a 100."""
    K = 4

    def __init__(self, px):
        self.px = px
        self.u = px * self.K / 100
        self.img = Image.new("RGBA", (px * self.K, px * self.K), (0, 0, 0, 0))

    def mask(self):
        return Image.new("L", self.img.size, 0)

    def box(self, cx, cy, r):
        u = self.u
        return ((cx - r) * u, (cy - r) * u, (cx + r) * u, (cy + r) * u)

    def paint(self, mask, colors, shadow=True):
        """Pinta a forma da máscara com gradiente vertical (e uma sombra suave embaixo)."""
        top, bottom = colors
        if shadow:
            # sombra discreta, para a forma se destacar do vidro e das outras formas
            blur = mask.filter(ImageFilter.GaussianBlur(2.5 * self.u))
            dark = Image.new("RGBA", self.img.size, (0, 0, 0, 0))
            dark.putalpha(blur.point(lambda a: a * 0.3))
            self.img.alpha_composite(dark, (0, round(2 * self.u)))
        left, y0, right, y1 = mask.getbbox() or (0, 0, 1, 1)
        ramp = Image.new("L", self.img.size, 255)
        ramp.paste(0, (0, 0, self.img.width, y0))
        ramp.paste(Image.linear_gradient("L").resize((self.img.width, max(1, y1 - y0))), (0, y0))
        layer = Image.composite(Image.new("RGBA", self.img.size, bottom + (255,)),
                                Image.new("RGBA", self.img.size, top + (255,)), ramp)
        layer.putalpha(mask)
        self.img.alpha_composite(layer)

    def result(self):
        return self.img.resize((self.px, self.px), Image.LANCZOS)


def icon_sun(c, cx, cy, r):
    u = c.u
    d = ImageDraw.Draw(c.img)
    for i in range(8):
        a = math.radians(i * 45)
        x0, y0 = cx + math.cos(a) * r * 1.42, cy + math.sin(a) * r * 1.42
        x1, y1 = cx + math.cos(a) * r * 1.78, cy + math.sin(a) * r * 1.78
        w = r * 0.24
        d.line((x0 * u, y0 * u, x1 * u, y1 * u), fill=SUN_RAY, width=round(w * u))
        for x, y in ((x0, y0), (x1, y1)):
            d.ellipse(c.box(x, y, w / 2), fill=SUN_RAY)
    m = c.mask()
    ImageDraw.Draw(m).ellipse(c.box(cx, cy, r), fill=255)
    c.paint(m, SUN_CORE, shadow=False)


def icon_moon(c, cx, cy, r):
    m = c.mask()
    d = ImageDraw.Draw(m)
    d.ellipse(c.box(cx, cy, r), fill=255)
    d.ellipse(c.box(cx + r * 0.52, cy - r * 0.38, r * 0.84), fill=0)
    c.paint(m, MOON, shadow=False)


def icon_cloud(c, x0, y0, w, colors):
    """Nuvem de largura w com o canto superior esquerdo em (x0, y0); altura = w / 1.6."""
    h = w / 1.6
    u = c.u
    m = c.mask()
    d = ImageDraw.Draw(m)
    base_top = y0 + h * 0.46
    d.rounded_rectangle((x0 * u, base_top * u, (x0 + w) * u, (y0 + h) * u),
                        radius=(y0 + h - base_top) / 2 * u, fill=255)
    d.ellipse(c.box(x0 + w * 0.31, y0 + h * 0.60, h * 0.30), fill=255)
    d.ellipse(c.box(x0 + w * 0.58, y0 + h * 0.42, h * 0.42), fill=255)
    c.paint(m, colors)


def icon_drops(c, xs, y, length, width, colors, slant=0.3):
    m = c.mask()
    d = ImageDraw.Draw(m)
    u = c.u
    for x in xs:
        x1, y1 = x - length * slant, y + length
        d.line((x * u, y * u, x1 * u, y1 * u), fill=255, width=round(width * u))
        for px, py in ((x, y), (x1, y1)):
            d.ellipse(c.box(px, py, width / 2), fill=255)
    c.paint(m, colors, shadow=False)


def icon_flakes(c, points, r):
    d = ImageDraw.Draw(c.img)
    u = c.u
    for cx, cy in points:
        for i in range(3):
            a = math.radians(90 + i * 60)
            dx, dy = math.cos(a) * r, math.sin(a) * r
            d.line(((cx - dx) * u, (cy - dy) * u, (cx + dx) * u, (cy + dy) * u),
                   fill=(255, 255, 255, 255), width=round(r * 0.42 * u))
        d.ellipse(c.box(cx, cy, r * 0.3), fill=(255, 255, 255, 255))


def icon_bolt(c):
    m = c.mask()
    pts = [(56, 56), (38, 80), (50, 80), (43, 98), (66, 70), (54, 70), (63, 56)]
    ImageDraw.Draw(m).polygon([(x * c.u, y * c.u) for x, y in pts], fill=255)
    c.paint(m, BOLT, shadow=False)


@lru_cache(maxsize=32)
def weather_art(name, px):
    """Ícone do tempo `name` com px × px pixels."""
    c = IconCanvas(px)
    if name == "sun":
        icon_sun(c, 50, 50, 24)
    elif name == "moon":
        icon_moon(c, 50, 50, 34)
    elif name == "partly_day":
        icon_sun(c, 37, 37, 16)
        icon_cloud(c, 20, 42, 74, CLOUD_LIGHT)
    elif name == "partly_night":
        icon_moon(c, 38, 36, 22)
        icon_cloud(c, 20, 42, 74, CLOUD_LIGHT)
    elif name == "clouds":
        icon_cloud(c, 34, 18, 60, CLOUD_GRAY)
        icon_cloud(c, 6, 34, 76, CLOUD_LIGHT)
    elif name == "fog":
        icon_cloud(c, 12, 10, 76, CLOUD_GRAY)
        d = ImageDraw.Draw(c.img)
        for y, x0, x1 in ((70, 14, 78), (82, 24, 88), (94, 18, 66)):
            w = 6
            d.rounded_rectangle((x0 * c.u, (y - w / 2) * c.u, x1 * c.u, (y + w / 2) * c.u),
                                radius=w / 2 * c.u, fill=FOG_LINE)
    elif name == "drizzle":
        icon_cloud(c, 10, 8, 80, CLOUD_GRAY)
        icon_drops(c, (34, 52, 70), 76, 7, 6, RAIN)
    elif name == "rain":
        icon_cloud(c, 10, 6, 80, CLOUD_GRAY)
        icon_drops(c, (32, 50, 68), 70, 20, 6, RAIN)
    elif name == "snow":
        icon_cloud(c, 10, 6, 80, CLOUD_GRAY)
        icon_flakes(c, ((30, 78), (50, 90), (70, 78)), 7)
    elif name == "thunder":
        icon_cloud(c, 10, 4, 80, CLOUD_DARK)
        icon_bolt(c)
    return c.result()


def search_cities(query, count=6):
    """Cidades que combinam com o texto: [{"name", "detail", "label", "lat", "lon"}]."""
    name = query.split(",")[0].strip()
    if len(name) < 2:
        return []
    url = "https://geocoding-api.open-meteo.com/v1/search?" + urllib.parse.urlencode(
        {"name": name, "count": count, "language": "pt", "format": "json"}
    )
    out = []
    for place in fetch_json(url).get("results") or []:
        label = place["name"] + (f", {place['admin1']}" if place.get("admin1") else "")
        detail = ", ".join(p for p in (place.get("admin1"), place.get("country")) if p)
        out.append({"name": place["name"], "detail": detail, "label": label,
                    "lat": place["latitude"], "lon": place["longitude"]})
    return out


def geocode(query):
    results = search_cities(query, count=1)
    if not results:
        raise LookupError(f"Cidade não encontrada: {query.split(',')[0].strip()}")
    place = results[0]
    return place["lat"], place["lon"], place["label"]


def forecast(lat, lon):
    url = "https://api.open-meteo.com/v1/forecast?" + urllib.parse.urlencode({
        "latitude": lat,
        "longitude": lon,
        "current": "temperature_2m,apparent_temperature,relative_humidity_2m,"
                   "weather_code,wind_speed_10m,is_day,precipitation",
        "hourly": "temperature_2m,weather_code,is_day",
        "forecast_hours": 8,
        "minutely_15": "precipitation",  # para avisar que vai chover
        "forecast_minutely_15": 6,
        "daily": "temperature_2m_max,temperature_2m_min",
        "timezone": "auto",
        "forecast_days": 1,
    })
    return fetch_json(url)


RAIN_CODES = set(range(51, 68)) | set(range(80, 83)) | {95, 96, 99}


class SystemWidget(CardWidget):
    key = "sistema"
    title = "Clima e Sistema"
    description = "Tempo agora e nas próximas horas, com CPU, memória e disco."
    icon = "clima"

    WIDTH, HEIGHT, PAD, RADIUS = 320, 318, 20, 26
    RING_D, RING_W = 72, 8
    ICON, COMPACT_ICON = 54, 34
    COMPACT_W, COMPACT_H, COMPACT_RADIUS = WIDTH, 64, 22
    MEDIUM_H = 150  # pequeno + as próximas horas
    HOURS = 6
    C_DIVIDER_X = 136  # entre o tempo e os anéis
    COMPACT_RING_D, COMPACT_RING_W = 40, 5
    RING_COLORS = {"CPU": (10, 132, 255), "RAM": (191, 90, 242), "Disco": (48, 209, 88)}
    RING_FULL = (255, 69, 58)

    WEATHER_EVERY_MS = 15 * 60 * 1000
    WEATHER_RETRY_MS = 2 * 60 * 1000
    SYSTEM_EVERY_MS = 2000

    def __init__(self, app):
        cached = app.cache.get("weather")
        # abre com o último clima guardado, enquanto busca o atual
        self.weather = cached if cached and cached.get("place") == (app.cfg[self.key].get("place")
                                                                  or app.cfg[self.key].get("city")) else None
        self.weather_msg = "Carregando…"
        self.metrics = []
        self._weather_result = None
        self._weather_busy = False
        self._weather_job = None
        self._last_cpu = cpu_times()
        super().__init__(app)
        self.updated = app.cache.get("weather_time") if self.weather else None

        self.update_system()
        self.send_to_back()
        if not self.cfg.get("city"):
            self.weather_msg = "Clique para escolher a cidade"
            self.refresh()
        else:
            self.update_weather()

    def load_sample(self):
        cached = self.app.cache.get("weather")
        self.weather = cached or {
            "place": "São Paulo, São Paulo", "temp": "24°", "icon": "sun", "desc": "Céu limpo",
            "minmax": "Máx. 27°   Mín. 17°", "details": "Sensação 25°  ·  Umidade 58%  ·  Vento 9 km/h",
            "hours": [["Agora", "sun", "24°"], ["15h", "sun", "26°"], ["16h", "clouds", "26°"],
                      ["17h", "clouds", "25°"], ["18h", "clouds", "23°"], ["19h", "moon", "21°"]]}
        self.weather_msg = ""
        self.metrics = [("CPU", 0.14, "1,7 / 12 núcleos"), ("RAM", 0.46, "7,4 / 16 GB"),
                        ("Disco", 0.58, "160 / 276 GB")]

    def layout(self, mode):
        if mode in ("p", "m"):
            return {
                "w": self.COMPACT_W, "h": self.COMPACT_H if mode == "p" else self.MEDIUM_H,
                "radius": self.COMPACT_RADIUS,
                "ring_d": self.COMPACT_RING_D, "ring_w": self.COMPACT_RING_W,
                # anéis encostados na direita, com a mesma margem do cartão
                "rings": [(self.COMPACT_W - 36 - 54 * (2 - i), self.COMPACT_H / 2) for i in range(3)],
            }
        col = (self.WIDTH - 2 * self.PAD) / 3
        return {
            "w": self.WIDTH, "h": self.HEIGHT, "radius": self.RADIUS,
            "ring_d": self.RING_D, "ring_w": self.RING_W,
            "rings": [(self.PAD + col * (i + 0.5), 188 + self.RING_D / 2) for i in range(3)],
        }

    def draw_shapes(self, d, k, mode):
        lay = self.layout(mode)
        for (cx, cy), (name, frac, _detail) in zip(lay["rings"], self.metrics):
            color = self.RING_FULL if frac >= 0.9 else self.RING_COLORS[name]
            draw_ring(d, cx * k, cy * k, lay["ring_d"] * k, lay["ring_w"] * k, frac, color)

    def draw_content(self, img, d, mode):
        lay = self.layout(mode)
        if mode == "g":
            self._content_full(img, d, lay)
            return
        self._content_compact(img, d, lay)
        if mode == "m":
            self._content_hours(img, d)

    def _content_compact(self, img, d, lay):
        cy = self.COMPACT_H / 2
        wx = self.weather
        x = 16
        if wx:
            # centralizado pela caixa do ícone, que é desenhado já centrado nela
            size = self.COMPACT_ICON
            self.picture(img, (14, cy - size / 2), weather_art(wx["icon"], round(size * self.s)))
            x = 14 + size + 6
        self.text(d, (x, cy), wx["temp"] if wx else "--°", "segoeuil.ttf", 26, anchor="lm")

        x = round(self.C_DIVIDER_X * self.s)
        d.line((x, 18 * self.s, x, (self.COMPACT_H - 18) * self.s), fill=DIVIDER, width=max(1, round(self.s)))

        for (rx, ry), (_name, frac, _detail) in zip(lay["rings"], self.metrics):
            self.text(d, (rx, ry), f"{round(frac * 100)}%", "seguisb.ttf", 10, anchor="mm")

    def _hour_columns(self):
        col = (self.COMPACT_W - 32) / self.HOURS
        return [16 + col * (i + 0.5) for i in range(self.HOURS)]

    def _content_hours(self, img, d):
        """Faixa com a previsão das próximas horas (tamanho médio)."""
        W = self.COMPACT_W
        y = round(self.COMPACT_H * self.s)
        d.line((self.PAD * self.s, y, (W - self.PAD) * self.s, y), fill=DIVIDER, width=max(1, round(self.s)))
        hours = (self.weather or {}).get("hours")
        if not hours:
            msg = self.weather_msg if not self.weather else "Previsão por hora indisponível"
            self.text(d, (W / 2, (self.COMPACT_H + self.MEDIUM_H) / 2), msg, "segoeui.ttf", 12, MUTED,
                      anchor="mm", max_w=W - 40)
            return
        icon = 28
        for cx, (label, name, temp) in zip(self._hour_columns(), hours):
            self.text(d, (cx, self.COMPACT_H + 10), label, "segoeui.ttf", 10.5, MUTED, anchor="ma")
            self.picture(img, (cx - icon / 2, self.COMPACT_H + 30), weather_art(name, round(icon * self.s)))
            self.text(d, (cx, self.COMPACT_H + 64), temp, "seguisb.ttf", 12, anchor="ma")

    def _content_full(self, img, d, lay):
        W, PAD = self.WIDTH, self.PAD
        inner = W - 2 * PAD

        # clima
        wx = self.weather
        city = (wx or {}).get("place") or self.cfg.get("place") or self.cfg.get("city") or "Clima"
        self.text(d, (PAD, 18), city, "seguisb.ttf", 14, max_w=inner - self.ICON - 4)
        self.text(d, (PAD - 2, 34), wx["temp"] if wx else "--°", "segoeuil.ttf", 48)
        if wx:
            size = self.ICON
            self.picture(img, (W - PAD - size + 4, 12), weather_art(wx["icon"], round(size * self.s)))
            self.text(d, (PAD, 100), wx["desc"], "seguisb.ttf", 13, max_w=inner)
            self.text(d, (PAD, 120), wx["minmax"], "segoeui.ttf", 12, MUTED)
            self.text(d, (PAD, 145), wx["details"], "segoeui.ttf", 11, MUTED, max_w=inner)
        else:
            self.text(d, (PAD, 100), self.weather_msg, "seguisb.ttf", 13, max_w=inner)

        y = round(172 * self.s)
        d.line((PAD * self.s, y, (W - PAD) * self.s, y), fill=DIVIDER, width=max(1, round(self.s)))

        # sistema
        for (cx, cy), (name, frac, detail) in zip(lay["rings"], self.metrics):
            self.text(d, (cx, cy), f"{round(frac * 100)}%", "seguisb.ttf", 15, anchor="mm")
            self.text(d, (cx, cy + self.RING_D / 2 + 10), name, "seguisb.ttf", 12, anchor="ma")
            self.text(d, (cx, cy + self.RING_D / 2 + 27), detail, "segoeui.ttf", 11, MUTED, anchor="ma")

    def menu_items(self):
        return [("cmd", "Atualizar clima", self.update_weather),
                ("cmd", "Mudar cidade…", self.ask_city)]

    # --- cliques, destaques e dicas ---

    def _ring_at(self, x, y):
        lay = self.layout(self.mode)
        for (cx, cy), metric in zip(lay["rings"], self.metrics):
            if math.hypot(x - cx, y - cy) <= lay["ring_d"] / 2 + 2:
                return (cx, cy, lay["ring_d"] / 2) + tuple(metric)
        return None

    def _weather_rect(self):
        """Área da temperatura e do ícone do tempo (x0, y0, x1, y1)."""
        if self.mode in ("p", "m"):
            return 8, 8, self.C_DIVIDER_X - 6, self.COMPACT_H - 8
        return self.PAD - 6, 34, self.PAD + 112, 94

    def _on_weather(self, x, y):
        x0, y0, x1, y1 = self._weather_rect()
        on_icon = (self.mode == "g" and self.WIDTH - self.PAD - self.ICON <= x <= self.WIDTH - self.PAD + 4
                   and 12 <= y <= 12 + self.ICON)
        return (x0 <= x <= x1 and y0 <= y <= y1) or on_icon

    def _on_hours(self, x, y):
        return self.mode == "m" and self.COMPACT_H + 4 <= y <= self.MEDIUM_H - 4 and 8 <= x <= self.COMPACT_W - 8

    def hit(self, x, y):
        shortcuts = self.app.setting("atalhos_clique")
        ring = self._ring_at(x, y)
        if ring and shortcuts:
            target = "ms-settings:storagesense" if ring[3] == "Disco" else "taskmgr.exe"
            return lambda: open_target(target)
        if self._on_weather(x, y) or self._on_hours(x, y):
            if not self.cfg.get("city"):
                return self.ask_city
            if not self.weather:
                return self.update_weather if self.error else None
            return self._open_forecast if shortcuts else None
        return None

    def hot_rect(self, x, y):
        ring = self._ring_at(x, y)
        if ring:
            cx, cy, r = ring[:3]
            return cx - r - 4, cy - r - 4, cx + r + 4, cy + r + 4, r + 4
        if self._on_hours(x, y):
            return 8, self.COMPACT_H + 4, self.COMPACT_W - 8, self.MEDIUM_H - 6, 14
        if self._on_weather(x, y):
            if self.mode == "g" and x > self.PAD + 112:
                s = self.ICON
                return self.WIDTH - self.PAD - s, 10, self.WIDTH - self.PAD + 4, 14 + s, 14
            return self._weather_rect() + (12,)
        return None

    def _open_forecast(self):
        place = self.cfg.get("place") or self.cfg.get("city") or ""
        open_target("https://www.google.com/search?q=" + urllib.parse.quote(f"previsão do tempo {place}"))

    def tooltip(self, x, y):
        ring = self._ring_at(x, y)
        if ring:
            name, frac, detail = ring[3:]
            action = ("as configurações de armazenamento" if name == "Disco"
                      else "o Gerenciador de Tarefas")
            lines = [f"{name}  ·  {round(frac * 100)}%", detail]
            if self.app.setting("atalhos_clique"):
                lines.append(f"Clique para abrir {action}")
            return "\n".join(lines)
        wx = self.weather
        if self._on_hours(x, y):
            return "Próximas horas\nClique para ver a previsão completa" if wx else None
        if self._on_weather(x, y):
            if not wx:
                return "Clique para tentar de novo" if self.error else None
            lines = [f"{wx['desc']}  ·  {wx['temp']}", wx["minmax"]]
            if self.mode != "g":
                lines.append(wx["details"])
            if self.updated:
                lines.append("Atualizado " + time_ago(datetime.fromtimestamp(self.updated, timezone.utc)))
            if self.app.setting("atalhos_clique"):
                lines.append("Clique para ver a previsão")
            return "\n".join(lines)
        return None

    def retry(self):
        self.update_weather()

    # --- atualizações ---

    def update_system(self):
        idle, total = cpu_times()
        last_idle, last_total = self._last_cpu
        self._last_cpu = (idle, total)
        d_total = total - last_total
        cpu = 1 - (idle - last_idle) / d_total if d_total else 0
        cpu = min(max(cpu, 0), 1)
        cores = os.cpu_count() or 1

        used, total_mem = memory()
        disk = shutil.disk_usage("C:\\")

        self.metrics = [
            ("CPU", cpu, f"{br(cpu * cores)} / {cores} núcleos"),
            ("RAM", used / total_mem, f"{br(gb(used))} / {gb(total_mem):.0f} GB"),
            ("Disco", disk.used / disk.total, f"{gb(disk.used):.0f} / {gb(disk.total):.0f} GB"),
        ]
        self.refresh()
        # na bateria, com a economia ligada, atualiza com menos frequência
        self.after(self.SYSTEM_EVERY_MS * (3 if self.app.saving else 1), self.update_system)

    def ask_city(self):
        self.app.prompt("Cidade do clima", "Digite o nome da cidade e escolha na lista.",
                        initial=self.cfg.get("city", ""), anchor=self.card_rect(), cities=True,
                        placeholder="Ex.: São Paulo", on_done=lambda text, place: self.set_city(text, place))

    def set_city(self, city, place=None):
        """Troca a cidade; com `place` (escolhida na lista de sugestões) não precisa procurar de novo."""
        if place:
            self.cfg.update(city=place["label"], lat=place["lat"], lon=place["lon"], place=place["label"])
        else:
            self.cfg.update(city=city, lat=None, lon=None, place=None)
        self.save()
        self.weather = None
        self.weather_msg = "Atualizando…"
        self.refresh()
        self.update_weather()

    def update_weather(self):
        if self._weather_job:
            self.app.root.after_cancel(self._weather_job)
            self._weather_job = None
        if self._weather_busy or not self.cfg.get("city"):
            return
        self._weather_busy = self.loading = True
        self.present()
        cfg = dict(self.cfg)
        threading.Thread(target=self._weather_worker, args=(cfg,), daemon=True).start()
        self.after(200, self._weather_poll)

    def _weather_worker(self, cfg):
        try:
            if cfg.get("lat") is None:
                cfg["lat"], cfg["lon"], cfg["place"] = geocode(cfg["city"])
            self._weather_result = (cfg, forecast(cfg["lat"], cfg["lon"]), None)
        except Exception as exc:  # rede, cidade inválida etc.
            self._weather_result = (cfg, None, exc)

    def _weather_poll(self):
        if self._weather_result is None:
            self.after(200, self._weather_poll)
            return
        cfg, data, error = self._weather_result
        self._weather_result = None
        self._weather_busy = self.loading = False

        if error:
            if isinstance(error, LookupError):
                self.weather, self.weather_msg, self.error = None, str(error), None
                delay = self.WEATHER_EVERY_MS
            else:
                # sem conexão: mantém o último clima na tela, com o aviso no canto
                self.weather_msg = "Sem conexão. Clique para tentar de novo"
                self.error = "Sem conexão com o serviço do clima"
                delay = self.WEATHER_RETRY_MS
            self.refresh()
            self._weather_job = self.after(delay, self.update_weather)
            return

        for key in ("city", "lat", "lon", "place"):
            self.cfg[key] = cfg.get(key)
        self.save()

        cur, daily = data["current"], data["daily"]
        self.weather = {
            "place": cfg.get("place") or cfg.get("city", ""),
            "temp": f"{round(cur['temperature_2m'])}°",
            "icon": weather_icon(cur["weather_code"], cur.get("is_day", 1)),
            "desc": WMO.get(cur["weather_code"], "—"),
            "minmax": f"Máx. {round(daily['temperature_2m_max'][0])}°   "
                      f"Mín. {round(daily['temperature_2m_min'][0])}°",
            "details": f"Sensação {round(cur['apparent_temperature'])}°  ·  "
                       f"Umidade {cur['relative_humidity_2m']}%  ·  "
                       f"Vento {round(cur['wind_speed_10m'])} km/h",
            "hours": self._hours(data),
        }
        self.error = None
        self.updated = time.time()
        self.app.cache.update(weather=self.weather, weather_time=self.updated)
        self.app.save_cache()
        self._check_rain(data)
        self.refresh()
        self._weather_job = self.after(self.WEATHER_EVERY_MS, self.update_weather)

    def _hours(self, data):
        hourly = data.get("hourly") or {}
        rows = list(zip(hourly.get("time", []), hourly.get("temperature_2m", []),
                        hourly.get("weather_code", []), hourly.get("is_day", [])))
        hours = []
        for i, (stamp, temp, code, is_day) in enumerate(rows[:self.HOURS]):
            label = "Agora" if i == 0 else f"{int(stamp[11:13])}h"
            hours.append((label, weather_icon(code, is_day), f"{round(temp)}°"))
        return hours

    def _check_rain(self, data):
        """Avisa quando vai começar a chover na próxima hora (e ainda não está chovendo)."""
        if not self.app.notify_enabled("chuva"):
            return
        cur = data["current"]
        if cur.get("weather_code") in RAIN_CODES or (cur.get("precipitation") or 0) > 0:
            return
        steps = data.get("minutely_15") or {}
        now = datetime.fromisoformat(cur["time"])
        for stamp, amount in zip(steps.get("time", []), steps.get("precipitation", [])):
            minutes = (datetime.fromisoformat(stamp) - now).total_seconds() / 60
            if 0 < minutes <= 60 and (amount or 0) >= 0.2:
                place = self.weather["place"].split(",")[0]
                self.app.notify("chuva", "Chuva chegando",
                                f"Deve começar a chover em cerca de {round(minutes)} min em {place}.",
                                cooldown=3 * 3600, action=self._open_forecast)
                return
