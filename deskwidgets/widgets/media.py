"""Widget "Tocando agora": a mídia em reprodução no Windows, com controles."""

import asyncio
import io
import math
import random
import textwrap
import threading
import time

from PIL import Image, ImageDraw, ImageOps

from deskwidgets.theme import FG, MUTED, font, rounded_mask, translucent
from deskwidgets.ui.card import CardWidget
from deskwidgets.ui.drawing import draw_pause, draw_play, draw_skip
from deskwidgets.win32 import bring_to_front, change_volume, find_window, open_target


# ---------- widget: tocando agora ----------

BROWSERS = ("chrome", "msedge", "edge", "brave", "firefox", "opera", "vivaldi")
SOURCES = {
    "auto": "Automático (o que estiver tocando)",
    "spotify": "Spotify",
    "youtube": "YouTube (navegador)",
}
SOURCE_LABEL = {"spotify": "SPOTIFY", "youtube": "YOUTUBE", "outro": "MÍDIA", "auto": "MÍDIA"}
SOURCE_COLOR = {"spotify": (30, 215, 96, 255), "youtube": (255, 72, 72, 255)}


def media_kind(app_id):
    a = app_id.lower()
    if "spotify" in a:
        return "spotify"
    if any(b in a for b in BROWSERS):
        return "youtube"
    return "outro"


def clean_text(text):
    """Remove emojis (a fonte Segoe UI não tem, e eles virariam quadradinhos)."""
    kept = (ch for ch in text or "" if ord(ch) <= 0xFFFF and not 0x2600 <= ord(ch) <= 0x27BF
            and ch not in "\ufe0f\u200d")
    return " ".join("".join(kept).split())


def fmt_time(seconds):
    m, s = divmod(int(seconds), 60)
    h, m = divmod(m, 60)
    return f"{h}:{m:02}:{s:02}" if h else f"{m}:{s:02}"


class MediaWatcher(threading.Thread):
    """Lê a central de mídia do Windows (a mesma do controle de volume) numa thread própria.

    O resultado fica em `snapshot`, que a interface consulta periodicamente.
    """

    def __init__(self, get_pref):
        super().__init__(daemon=True)
        self.get_pref = get_pref
        self.snapshot = None
        self.stopped = False
        self.loop = None
        self._wake = None
        self._session = None
        self._art_key = None
        self._art = None

    def run(self):
        try:
            from winrt.windows.media.control import (
                GlobalSystemMediaTransportControlsSessionManager as Manager)
            from winrt.windows.storage.streams import Buffer, InputStreamOptions
        except ImportError:
            self.snapshot = {"empty": True, "pref": self.get_pref(), "error": "Pacote winrt não instalado"}
            return
        self._Buffer, self._ReadAhead = Buffer, InputStreamOptions.READ_AHEAD
        asyncio.run(self._main(Manager))

    async def _main(self, Manager):
        self.loop = asyncio.get_running_loop()
        self._wake = asyncio.Event()
        manager = None
        while not self.stopped:
            try:
                if manager is None:
                    manager = await Manager.request_async()
                self.snapshot = await self._read(manager)
            except Exception:  # a sessão pode sumir no meio da leitura; tenta de novo
                manager = None
                self._session = None
                self.snapshot = {"empty": True, "pref": self.get_pref()}
            self._wake.clear()
            try:
                await asyncio.wait_for(self._wake.wait(), 1)
            except asyncio.TimeoutError:
                pass

    def _choose(self, manager, pref):
        sessions = list(manager.get_sessions())
        if pref != "auto":
            sessions = [s for s in sessions if media_kind(s.source_app_user_model_id) == pref]
        if not sessions:
            return None
        playing = [s for s in sessions if int(s.get_playback_info().playback_status) == 4]
        if playing:
            return playing[0]
        current = manager.get_current_session()
        if current and any(s.source_app_user_model_id == current.source_app_user_model_id for s in sessions):
            return current
        return sessions[0]

    async def _read(self, manager):
        pref = self.get_pref()
        session = self._choose(manager, pref)
        self._session = session
        if session is None:
            return {"empty": True, "pref": pref}

        app_id = session.source_app_user_model_id
        info = session.get_playback_info()
        timeline = session.get_timeline_properties()
        props = await session.try_get_media_properties_async()

        title, artist = clean_text(props.title), clean_text(props.artist)
        key = (app_id, title, artist)
        if key != self._art_key or self._art is None:
            self._art_key, self._art = key, await self._read_art(props)

        updated = timeline.last_updated_time
        duration = (timeline.end_time - timeline.start_time).total_seconds()
        return {
            "empty": False,
            "pref": pref,
            "kind": media_kind(app_id),
            "title": title or "Sem título",
            "artist": artist,
            "playing": int(info.playback_status) == 4,
            "position": timeline.position.total_seconds(),
            "duration": max(0.0, duration),
            "updated": updated.timestamp() if updated.year > 2000 else None,
            "can_toggle": info.controls.is_play_pause_toggle_enabled,
            "can_prev": info.controls.is_previous_enabled,
            "can_next": info.controls.is_next_enabled,
            "art": self._art,
            "art_key": key if self._art is not None else None,
        }

    async def _read_art(self, props):
        if not props.thumbnail:
            return None
        try:
            stream = await props.thumbnail.open_read_async()
            size = stream.size
            buf = self._Buffer(size)
            await stream.read_async(buf, size, self._ReadAhead)
            return Image.open(io.BytesIO(bytes(buf))).convert("RGB")
        except Exception:
            return None

    def wake(self):
        if self.loop and self._wake:
            try:
                self.loop.call_soon_threadsafe(self._wake.set)
            except RuntimeError:  # a thread já terminou
                pass

    def command(self, name):
        """Envia play/pause, próxima ou anterior para a sessão mostrada."""
        session = self._session
        if self.loop is None or session is None:
            return

        async def run():
            try:
                await getattr(session, name)()
            except Exception:
                pass
            await asyncio.sleep(0.15)  # dá tempo do player atualizar o estado
            self._wake.set()

        asyncio.run_coroutine_threadsafe(run(), self.loop)

    def stop(self):
        self.stopped = True
        self.wake()


IDLE_FPS = 15
IDLE_FRAMES = 60  # 4 segundos em loop
IDLE_GRADIENT = {
    "auto": ((10, 132, 255), (191, 90, 242)),
    "spotify": ((30, 215, 96), (20, 170, 200)),
    "youtube": ((255, 72, 72), (255, 150, 60)),
}
FUNNY = [
    ("Silêncio total…", "O DJ saiu pra tomar um café"),
    ("Nada tocando", "Seus fones estão com saudade de você"),
    ("Palco vazio", "Bora dar play em alguma coisa?"),
    ("Shhh…", "A playlist está tirando um cochilo"),
    ("Modo biblioteca", "Só dá pra ouvir o ventilador do PC"),
    ("Cadê a trilha sonora?", "Todo momento merece uma"),
    ("A música parou", "Mas a vibe continua"),
    ("Tudo quieto por aqui", "O vizinho agradece"),
    ("Intervalo", "Até os músicos precisam descansar"),
    ("Sem som no momento", "Que tal aquela música que gruda?"),
]


def idle_frames(w, h, colors, bars, with_z):
    """Quadros de um equalizador "cochilando", com zzz subindo. Desenhados uma vez só."""
    k = 2
    W, H = w * k, h * k
    bar = W / (bars * 1.9)
    step = (W - bar) / (bars - 1)
    center = (bars - 1) / 2
    frames = []
    for f in range(IDLE_FRAMES):
        p = f / IDLE_FRAMES
        img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        for i in range(bars):
            # ondas com número inteiro de ciclos por loop, para emendar sem salto
            envelope = 0.35 + 0.65 * math.exp(-((i - center) / (bars * 0.32)) ** 2)
            wave = 0.5 + 0.5 * math.sin(2 * math.pi * (2 * p - i / bars * 1.5))
            drift = 0.5 + 0.5 * math.sin(2 * math.pi * (p + i * 0.21))
            level = 0.12 + 0.6 * envelope * (0.7 * wave + 0.3 * drift)
            bh = max(bar, H * 0.78 * level)
            t = i / (bars - 1)
            color = tuple(round(a + (b - a) * t) for a, b in zip(*colors)) + (255,)
            x = i * step
            d.rounded_rectangle((x, H / 2 - bh / 2, x + bar, H / 2 + bh / 2), radius=bar / 2, fill=color)
        if with_z:
            for j in range(3):
                q = (p + j / 3) % 1
                size = round((11 + 9 * q) * k * h / 70)
                alpha = round(220 * math.sin(math.pi * q))
                # sobem por cima das últimas barras (que são as mais baixas), balançando de leve
                zx = W * (0.85 + 0.1 * q + 0.01 * math.sin(2 * math.pi * (q * 2)))
                zy = H * (0.4 - 0.34 * q)
                d.text((zx, zy), "z", font=font("seguisb.ttf", max(1, size)),
                       fill=(220, 220, 228, alpha), anchor="mm")
        frames.append(img.resize((w, h), Image.LANCZOS))
    return frames


class MediaWidget(CardWidget):
    key = "midia"
    title = "Tocando agora"
    description = "A música ou o vídeo tocando no Windows, com capa e controles."
    icon = "midia"

    W, H, RADIUS = 320, 196, 26
    ART, ART_RADIUS, ART_XY = 76, 14, (18, 18)
    TEXT_X = 18 + 76 + 14
    TEXT_W = W - TEXT_X - 18
    BAR_Y = 114
    CTRL_Y = 162
    PLAY_D = 44
    IDLE_BOX = (18, 110, W - 36, 70)  # x, y, largura, altura do equalizador

    COMPACT_W, COMPACT_H, COMPACT_RADIUS = W, 64, 22
    C_ART, C_ART_RADIUS, C_ART_XY = 40, 9, (12, 12)
    C_TEXT_X = 12 + 40 + 10
    C_TEXT_W = COMPACT_W - C_TEXT_X - 92
    C_PLAY_D = 32
    C_IDLE_BOX = (20, 20, 24, 24)  # mini equalizador dentro do quadrado da capa

    # médio: capa, título e artista, controles e uma barra de progresso fina
    MEDIUM_H, M_RADIUS = 102, 24
    M_ART, M_ART_RADIUS, M_ART_XY = 74, 12, (14, 14)
    M_TEXT_X = 14 + 74 + 14
    M_TEXT_W = W - M_TEXT_X - 16
    M_CTRL_Y, M_PLAY_D = 66, 32
    M_BAR_Y = 92
    M_IDLE_BOX = (14 + 17, 14 + 17, 40, 40)

    POLL_MS = 250
    MESSAGE_EVERY_MS = 12000

    def __init__(self, app):
        self.media = None
        self._sig = None
        self._art_cache = None
        self._idle_cache = {}
        self._msg = random.randrange(len(FUNNY))
        cfg = app.cfg[self.key]
        # a thread começa antes de self.cfg existir, então lê direto do dicionário
        self.watcher = MediaWatcher(lambda: cfg.get("fonte", "auto"))
        self.watcher.start()
        super().__init__(app)
        self.refresh()
        self.send_to_back()
        self.after(self.POLL_MS, self._poll)
        self.after(1000 // IDLE_FPS, self._idle_tick)
        self.after(self.MESSAGE_EVERY_MS, self._next_message)

    def load_sample(self):
        self._art_cache, self._idle_cache, self._msg = None, {}, 0
        self.media = {"kind": "spotify", "title": "Nome da música", "artist": "Artista  ·  Álbum",
                      "playing": True, "art": None, "art_key": None, "can_toggle": True, "can_prev": True,
                      "can_next": True, "position": 74.0, "duration": 213.0, "updated": None,
                      "pref": "auto", "empty": False}

    def set_source(self, value):
        self._set_source(value)

    def default_pos(self, w, h):
        # logo abaixo do widget de clima e sistema, encaixado nele
        system = self.app.widgets.get("sistema")
        return self.below(system) if system else super().default_pos(w, h)

    def close(self):
        self.watcher.stop()
        super().close()

    def on_rescale(self):
        self._idle_cache.clear()
        self._art_cache = None

    def layout(self, mode):
        if mode == "p":
            return {"w": self.COMPACT_W, "h": self.COMPACT_H, "radius": self.COMPACT_RADIUS}
        if mode == "m":
            return {"w": self.W, "h": self.MEDIUM_H, "radius": self.M_RADIUS}
        return {"w": self.W, "h": self.H, "radius": self.RADIUS}

    def _art_geom(self, mode=None):
        """((x, y), lado, raio) da capa no tamanho dado."""
        mode = mode or self.mode
        if mode == "p":
            return self.C_ART_XY, self.C_ART, self.C_ART_RADIUS
        if mode == "m":
            return self.M_ART_XY, self.M_ART, self.M_ART_RADIUS
        return self.ART_XY, self.ART, self.ART_RADIUS

    # --- estado ---

    def _has_media(self):
        return bool(self.media) and not self.media.get("empty")

    def _pref(self):
        return (self.media or {}).get("pref") or self.cfg.get("fonte", "auto")

    def _position(self):
        m = self.media
        pos = m["position"]
        if m["playing"] and m["updated"]:
            pos += time.time() - m["updated"]
        return max(0.0, min(pos, m["duration"])) if m["duration"] else pos

    def _signature(self):
        m = self.media
        if not self._has_media():
            return ("vazio", self._pref(), m and m.get("error"))
        second = int(self._position()) if m["playing"] and m["duration"] else None
        return (m["kind"], m["title"], m["artist"], m["playing"], m["art_key"],
                m["can_toggle"], m["can_prev"], m["can_next"], m["position"], second)

    def _poll(self):
        self.media = self.watcher.snapshot
        sig = self._signature()
        if sig != self._sig:
            self._sig = sig
            self.refresh()
        self.after(self.POLL_MS * (3 if self.app.saving else 1), self._poll)

    def _next_message(self):
        self._msg = random.choice([i for i in range(len(FUNNY)) if i != self._msg])
        if not self._has_media():
            self.refresh()
        self.after(self.MESSAGE_EVERY_MS, self._next_message)

    # --- animação de "nada tocando" ---

    def _idle_box(self, mode=None):
        mode = mode or self.mode
        return {"p": self.C_IDLE_BOX, "m": self.M_IDLE_BOX, "g": self.IDLE_BOX}[mode]

    def _idle_frames(self, mode):
        pref = self._pref()
        key = (mode, pref)
        if key not in self._idle_cache:
            _, _, w, h = self._idle_box(mode)
            self._idle_cache[key] = idle_frames(
                round(w * self.s), round(h * self.s), IDLE_GRADIENT[pref],
                bars={"p": 4, "m": 5, "g": 23}[mode], with_z=mode == "g")
        return self._idle_cache[key]

    def overlay(self, img):
        if self._has_media() or self._animating:
            return
        frames = self._idle_frames(self.mode)
        # na bateria, com a economia ligada, o equalizador fica parado
        index = 0 if self.app.saving else int(time.perf_counter() * IDLE_FPS) % len(frames)
        x, y, _, _ = self._idle_box()
        img.alpha_composite(frames[index], (self.margin + round(x * self.s), self.margin + round(y * self.s)))

    def _idle_tick(self):
        if not self._has_media() and not self._animating and self._card is not None and not self.app.saving:
            self.present()
        self.after(1000 // IDLE_FPS, self._idle_tick)

    # --- controles ---

    def _controls(self, mode=None):
        """(nome, x, y, tamanho, habilitado, comando, em_círculo) de cada botão, em pixels lógicos."""
        if not self._has_media():
            return []
        mode = mode or self.mode
        m = self.media
        toggle = "pause" if m["playing"] else "play"
        if mode == "p":
            y = self.COMPACT_H / 2
            return [
                (toggle, self.COMPACT_W - 60, y, self.C_PLAY_D, m["can_toggle"],
                 "try_toggle_play_pause_async", True),
                ("next", self.COMPACT_W - 22, y, 12, m["can_next"], "try_skip_next_async", False),
            ]
        if mode == "m":
            x, y = self.M_TEXT_X, self.M_CTRL_Y
            return [
                ("prev", x + 14, y, 15, m["can_prev"], "try_skip_previous_async", False),
                (toggle, x + 62, y, self.M_PLAY_D, m["can_toggle"], "try_toggle_play_pause_async", True),
                ("next", x + 110, y, 15, m["can_next"], "try_skip_next_async", False),
            ]
        cx, y = self.W / 2, self.CTRL_Y
        return [
            ("prev", cx - 72, y, 18, m["can_prev"], "try_skip_previous_async", False),
            (toggle, cx, y, self.PLAY_D, m["can_toggle"], "try_toggle_play_pause_async", True),
            ("next", cx + 72, y, 18, m["can_next"], "try_skip_next_async", False),
        ]

    @staticmethod
    def _reach(size, circle):
        return size / 2 + 2 if circle else max(size, 14) * 0.9

    def _control_at(self, x, y):
        for control in self._controls():
            _name, cx, cy, size, _enabled, _command, circle = control
            if math.hypot(x - cx, y - cy) <= self._reach(size, circle):
                return control
        return None

    def _on_art(self, x, y):
        (ax, ay), art, _ = self._art_geom()
        return self._has_media() and ax <= x <= ax + art and ay <= y <= ay + art

    def _on_bar(self, x, y):
        m = self.media
        if not (self._has_media() and m["duration"]) or self.mode == "p":
            return False
        if self.mode == "m":
            return x >= self.M_TEXT_X - 4 and self.M_BAR_Y - 8 <= y <= self.MEDIUM_H
        return self.BAR_Y - 8 <= y <= self.BAR_Y + 26

    def hit(self, x, y):
        control = self._control_at(x, y)
        if control and control[4]:
            return lambda c=control[5]: self.watcher.command(c)
        if self._on_art(x, y) and self.app.setting("atalhos_clique"):
            return self._open_player
        return None

    def hot_rect(self, x, y):
        control = self._control_at(x, y)
        if control:
            _name, cx, cy, size, _enabled, _command, circle = control
            r = self._reach(size, circle) + 2
            return cx - r, cy - r, cx + r, cy + r, r
        if self._on_art(x, y):
            (ax, ay), art, radius = self._art_geom()
            return ax, ay, ax + art, ay + art, radius
        return None

    def _open_player(self):
        """Traz para a frente a janela do que está tocando (Spotify ou a aba do navegador)."""
        m = self.media
        hwnd = find_window(m["title"][:40]) if m["title"] else None
        if hwnd:
            bring_to_front(hwnd)
        elif m["kind"] == "spotify":
            open_target("spotify:")

    def tooltip(self, x, y):
        if not self._has_media():
            return None
        m = self.media
        control = self._control_at(x, y)
        if control:
            return {"prev": "Anterior", "next": "Próxima", "play": "Tocar", "pause": "Pausar"}[control[0]]
        song = "\n".join(textwrap.wrap(m["title"], 48)[:3])
        if m["artist"]:
            song += "\n\n" + m["artist"]
        if self._on_art(x, y):
            if not self.app.setting("atalhos_clique"):
                return song
            return song + ("\n" if m["artist"] else "\n\n") + "Clique para abrir o player"
        if self._on_bar(x, y):
            return f"{fmt_time(self._position())} de {fmt_time(m['duration'])}"
        if self.mode == "p":
            text_area = self.C_TEXT_X <= x <= self.C_TEXT_X + self.C_TEXT_W
        elif self.mode == "m":
            text_area = x >= self.M_TEXT_X and y < self.M_CTRL_Y - 18
        else:
            text_area = x >= self.TEXT_X and y < self.BAR_Y - 6
        return song if text_area else None

    def on_wheel(self, x, y, steps):
        change_volume(steps)

    def menu_items(self):
        pref = self.cfg.get("fonte", "auto")
        sources = [("radio", label, value == pref, lambda v=value: self._set_source(v))
                   for value, label in SOURCES.items()]
        return [("sub", "Mostrar", sources)]

    def _set_source(self, value):
        self.cfg["fonte"] = value
        self.save()
        self.watcher.wake()

    # --- desenho ---

    def draw_shapes(self, d, k, mode):
        (x, y), art, radius = self._art_geom(mode)
        if not (self._has_media() and self.media["art"] is not None):
            d.rounded_rectangle((x * k, y * k, (x + art) * k, (y + art) * k), radius=radius * k,
                                fill=translucent((255, 255, 255), 0.12))

        dim = translucent((255, 255, 255), 0.3)
        for name, cx, cy, size, enabled, _command, circle in self._controls(mode):
            cx, cy, size = cx * k, cy * k, size * k
            if circle:
                r = size / 2
                d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=FG if enabled else dim)
                glyph, color = size * 0.38, (28, 28, 30, 255)
            else:
                glyph, color = size, FG if enabled else dim
            if name == "play":
                draw_play(d, cx, cy, glyph, color)
            elif name == "pause":
                draw_pause(d, cx, cy, glyph, color)
            else:
                draw_skip(d, cx, cy, glyph, color, forward=name == "next")

        if mode != "p" and self._has_media() and self.media["duration"]:
            frac = self._position() / self.media["duration"]
            if mode == "m":
                x0, x1, y, h = self.M_TEXT_X * k, (self.W - 16) * k, self.M_BAR_Y * k, 3 * k
            else:
                x0, x1, y, h = 18 * k, (self.W - 18) * k, self.BAR_Y * k, 4 * k
            d.rounded_rectangle((x0, y, x1, y + h), radius=h / 2, fill=translucent((255, 255, 255), 0.22))
            if frac > 0:
                d.rounded_rectangle((x0, y, max(x0 + h, x0 + (x1 - x0) * frac), y + h),
                                    radius=h / 2, fill=(235, 235, 240, 255))

    def _paste_art(self, img, mode):
        m = self.media
        if not (self._has_media() and m["art"] is not None):
            return False
        xy, size, radius = self._art_geom(mode)
        px = round(size * self.s)
        key = (m["art_key"], px)
        if not self._art_cache or self._art_cache[0] != key:
            pic = ImageOps.fit(m["art"], (px, px), Image.LANCZOS).convert("RGBA")
            pic.putalpha(rounded_mask(px, px, radius * self.s))
            self._art_cache = (key, pic)
        img.alpha_composite(self._art_cache[1], (round(xy[0] * self.s), round(xy[1] * self.s)))
        return True

    def _note(self, d, mode, glyph_size):
        """Nota musical no lugar da capa, quando há música mas não há imagem."""
        (x, y), size, _ = self._art_geom(mode)
        self.text(d, (x + size / 2, y + size / 2), "♪", "seguisym.ttf", glyph_size, MUTED, anchor="mm")

    def draw_content(self, img, d, mode):
        m = self.media if self._has_media() else None
        error = (self.media or {}).get("error")
        funny_title, funny_sub = FUNNY[self._msg]
        if error:
            funny_sub = error

        if mode == "p":
            x = self.C_TEXT_X
            if m:
                if not self._paste_art(img, mode):
                    self._note(d, mode, 18)
                self.text(d, (x, 14), m["title"], "seguisb.ttf", 12, max_w=self.C_TEXT_W)
                self.text(d, (x, 33), m["artist"], "segoeui.ttf", 10.5, MUTED, max_w=self.C_TEXT_W)
            else:
                w = self.COMPACT_W - x - 18
                self.text(d, (x, 14), funny_title, "seguisb.ttf", 12, max_w=w)
                self.text(d, (x, 33), funny_sub, "segoeui.ttf", 10.5, MUTED, max_w=w)
            return

        if mode == "m":
            x, w = self.M_TEXT_X, self.M_TEXT_W
            if m:
                if not self._paste_art(img, mode):
                    self._note(d, mode, 28)
                self.text(d, (x, 13), m["title"], "seguisb.ttf", 13, max_w=w)
                self.text(d, (x, 32), m["artist"], "segoeui.ttf", 11, MUTED, max_w=w)
            else:
                self.text(d, (x, 22), funny_title, "seguisb.ttf", 13, max_w=w)
                for i, line in enumerate(self.wrap(funny_sub, "segoeui.ttf", 11, w, 2)):
                    self.text(d, (x, 42 + 16 * i), line, "segoeui.ttf", 11, MUTED, max_w=w)
            return

        x, w = self.TEXT_X, self.TEXT_W
        kind = m["kind"] if m else self._pref()
        self.text(d, (x, 19), SOURCE_LABEL[kind], "seguisb.ttf", 10, SOURCE_COLOR.get(kind, MUTED))
        if m:
            if not self._paste_art(img, mode):
                self._note(d, mode, 32)
            lines = self.wrap(m["title"], "seguisb.ttf", 14, w, 2)
            for i, line in enumerate(lines):
                self.text(d, (x, 35 + 19 * i), line, "seguisb.ttf", 14, max_w=w)
            self.text(d, (x, 37 + 19 * len(lines)), m["artist"], "segoeui.ttf", 12, MUTED, max_w=w)
            if m["duration"]:
                pos = self._position()
                ty = self.BAR_Y + 10
                self.text(d, (18, ty), fmt_time(pos), "segoeui.ttf", 10, MUTED)
                self.text(d, (self.W - 18, ty), "-" + fmt_time(m["duration"] - pos),
                          "segoeui.ttf", 10, MUTED, anchor="ra")
        else:
            self._note(d, mode, 32)
            self.text(d, (x, 35), funny_title, "seguisb.ttf", 14, max_w=w)
            for i, line in enumerate(self.wrap(funny_sub, "segoeui.ttf", 12, w, 2)):
                self.text(d, (x, 57 + 17 * i), line, "segoeui.ttf", 12, MUTED, max_w=w)
