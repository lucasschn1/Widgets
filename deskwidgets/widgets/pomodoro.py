"""Widget de relógio com timer Pomodoro."""

import math
import time
from datetime import date

from deskwidgets.theme import DIVIDER, EDGE_GAP, FG, MUTED, WIDGET_GAP, translucent
from deskwidgets.ui.card import CardWidget
from deskwidgets.ui.drawing import draw_pause, draw_play, draw_restart, draw_ring, draw_skip
from deskwidgets.util import MONTHS_LONG, WEEKDAYS_LONG
from deskwidgets.win32 import area_for, open_target


# ---------- widget: relógio pomodoro ----------

PHASES = {  # nome, cor do anel
    "foco": ("Foco", (255, 99, 88)),
    "pausa": ("Pausa", (48, 209, 88)),
    "longa": ("Pausa longa", (10, 132, 255)),
}
POMODORO_DEFAULTS = {"foco": 25, "pausa": 5, "longa": 15}
DURATION_CHOICES = {"foco": (15, 20, 25, 30, 40, 45, 50, 60, 90), "pausa": (3, 5, 10, 15),
                    "longa": (10, 15, 20, 30)}


def fmt_timer(seconds):
    seconds = max(0, math.ceil(seconds))
    h, rest = divmod(seconds, 3600)
    m, s = divmod(rest, 60)
    return f"{h}:{m:02d}:{s:02d}" if h else f"{m:02d}:{s:02d}"


def fmt_minutes(minutes):
    h, m = divmod(round(minutes), 60)
    if not h:
        return f"{m} min"
    return f"{h}h{m:02d}" if m else f"{h}h"


class PomodoroWidget(CardWidget):
    key = "pomodoro"
    title = "Pomodoro"
    description = "Relógio com timer de foco e pausas, ciclos e o total do dia."
    icon = "pomodoro"

    W, PAD = 320, 20
    COMPACT_H, COMPACT_RADIUS = 64, 22
    C_RING = (34, 32, 40, 5)            # cx, cy, diâmetro, espessura
    MEDIUM_H, M_RADIUS = 150, 24
    M_RING = (75, 75, 110, 8)
    M_COL_X = 150                       # coluna da direita (relógio e botões)
    M_CTRL_Y, M_PLAY_D = 118, 34
    HEIGHT, RADIUS = 340, 26
    G_RING = (160, 174, 140, 10)
    G_DOTS_Y = 262
    G_CTRL_Y, G_PLAY_D = 298, 42

    MIN_S, MAX_S = 60, 180 * 60

    def __init__(self, app):
        cfg = app.cfg[self.key]
        cfg.setdefault("tamanho", "m")
        self._load_state(cfg)
        self._tick_job = None
        super().__init__(app)
        self._tick()
        self.send_to_back()

    def _load_state(self, cfg):
        state = cfg.get("estado", {})
        self.phase = state.get("fase") if state.get("fase") in PHASES else "foco"
        self.cycle = int(state.get("ciclo", 0))
        self.total = float(state.get("total") or self._duration(self.phase, cfg) * 60)
        self.remaining = float(state.get("restante", self.total))
        self.end = state.get("fim")  # horário em que a fase acaba, enquanto está correndo

    def load_sample(self):
        self._load_state(self.cfg)
        self.cfg.setdefault("hoje", {"data": date.today().isoformat(), "pomodoros": 3, "minutos": 75})

    def set_duration(self, phase, minutes):
        self._set_duration(phase, minutes)

    def set_every(self, n):
        self._set_every(n)

    def default_pos(self, w, h):
        # embaixo do widget mais baixo; se não couber, numa coluna nova à esquerda
        others = self.others()
        if not others:
            return super().default_pos(w, h)
        lowest = max(others, key=lambda o: o.card_rect()[3])
        x, y = self.below(lowest)
        _, top, _, bottom = area_for(lowest.card_rect())
        m, s = self.margin, self.s
        if y + h - m <= bottom - EDGE_GAP * s:
            return x, y
        left = min(o.card_rect()[0] for o in others)
        return round(left - WIDGET_GAP * s - (w - 2 * m) - m), round(top + EDGE_GAP * s - m)

    # --- estado ---

    def _duration(self, phase, cfg=None):
        return (cfg or self.cfg).get("duracoes", {}).get(phase, POMODORO_DEFAULTS[phase])

    @property
    def every(self):
        return self.cfg.get("longa_a_cada", 4)

    @property
    def running(self):
        return self.end is not None

    def left(self):
        return max(0.0, self.end - time.time()) if self.running else self.remaining

    def started(self):
        return self.running or self.remaining < self.total - 0.5

    def _today(self):
        today = date.today().isoformat()
        stats = self.cfg.get("hoje")
        if not stats or stats.get("data") != today:
            stats = self.cfg["hoje"] = {"data": today, "pomodoros": 0, "minutos": 0}
        return stats

    def _store(self):
        self.cfg["estado"] = {"fase": self.phase, "ciclo": self.cycle, "total": self.total,
                              "restante": self.left(), "fim": self.end}
        self.save()

    def _set_phase(self, phase):
        self.phase = phase
        self.total = self.remaining = self._duration(phase) * 60.0
        self.end = None

    def _next_phase(self):
        if self.phase == "foco":
            return "longa" if self.cycle >= self.every else "pausa"
        return "foco"

    # --- ações ---

    def toggle(self):
        if self.running:
            self.remaining, self.end = self.left(), None
        else:
            self.end = time.time() + self.remaining
        self._store()
        self._tick()

    def reset(self):
        self._set_phase(self.phase)
        self._store()
        self._tick()

    def skip(self):
        """Vai para a próxima fase sem contar a atual como concluída."""
        if self.phase == "longa":
            self.cycle = 0
        self._set_phase(self._next_phase())
        self._store()
        self._tick()

    def reset_day(self):
        self.cfg.pop("hoje", None)
        self.cycle = 0
        self._store()
        self.refresh()

    def _complete(self, late=False):
        finished = self.phase
        if finished == "foco":
            stats = self._today()
            stats["pomodoros"] += 1
            stats["minutos"] += round(self.total / 60)
            self.cycle += 1
        nxt = self._next_phase()
        if finished == "longa":
            self.cycle = 0
        self._set_phase(nxt)
        auto = self.cfg.get("auto", False) and not late
        if auto:
            self.end = time.time() + self.total
        self._store()
        if late:
            return
        if finished == "foco":
            n = self._today()["pomodoros"]
            title = "Pomodoro concluído 🍅"
            text = f"{n}º do dia. Hora de uma {PHASES[nxt][0].lower()} de {fmt_minutes(self.total / 60)}."
        else:
            title = "Fim da pausa"
            text = f"Bora focar por {fmt_minutes(self.total / 60)}."
        if not auto:
            text += " Clique para começar."
        self.app.notify("pomodoro", title, text, action=None if auto else self._start_if_idle)

    def _start_if_idle(self):
        if self.alive and not self.running:
            self.toggle()

    def _tick(self):
        """Confere o fim da fase e redesenha: a cada segundo correndo, a cada minuto parado."""
        if self._tick_job:
            self.app.root.after_cancel(self._tick_job)
        if self.running and time.time() >= self.end:
            self._complete(late=time.time() - self.end > 60)  # o PC dormiu ou o app estava fechado
        self.refresh()
        now = time.time()
        if self.running:
            delay = (self.end - now) % 1 or 1  # vira o segundo junto com o timer
        else:
            delay = 60 - now % 60  # só o relógio muda
        self._tick_job = self.after(max(50, round(delay * 1000) + 15), self._tick)

    def on_wheel(self, x, y, steps):
        """Roda do mouse: um minuto a mais ou a menos (parado no início, muda a duração da fase)."""
        delta = 60 * (round(steps) or (1 if steps > 0 else -1))
        rem = self.left()
        if not self.started():
            self.total = self.remaining = float(min(max(rem + delta, self.MIN_S), self.MAX_S))
            self.cfg.setdefault("duracoes", {})[self.phase] = round(self.total / 60)
        else:
            new = min(max(rem + delta, min(rem, self.MIN_S)), self.MAX_S)
            change = new - rem
            self.total = max(self.total + change, new, self.MIN_S)
            if self.running:
                self.end += change
            else:
                self.remaining = new
        self._store()
        self._tick()

    def _set_duration(self, phase, minutes):
        self.cfg.setdefault("duracoes", {})[phase] = minutes
        if phase == self.phase and not self.started():
            self._set_phase(phase)
        self._store()
        self.refresh()

    def _set_every(self, n):
        self.cfg["longa_a_cada"] = n
        self.cycle = min(self.cycle, n)
        self._store()
        self.refresh()

    def _toggle_auto(self):
        self.cfg["auto"] = not self.cfg.get("auto", False)
        self.save()

    def menu_items(self):
        durations = []
        for phase, choices in DURATION_CHOICES.items():
            current = self._duration(phase)
            options = sorted(set(choices) | {current})
            durations.append(("sub", PHASES[phase][0], [
                ("radio", fmt_minutes(m), m == current, lambda p=phase, m=m: self._set_duration(p, m))
                for m in options]))
        durations.append(("sub", "Pausa longa a cada", [
            ("radio", f"{n} pomodoros", n == self.every, lambda n=n: self._set_every(n)) for n in range(2, 7)]))
        nxt = PHASES[self._next_phase()][0].lower()
        return [("cmd", "Pausar" if self.running else ("Continuar" if self.started() else "Começar"),
                 self.toggle),
                ("cmd", "Reiniciar esta fase", self.reset),
                ("cmd", f"Pular para {'a ' if nxt != 'foco' else 'o '}{nxt}", self.skip),
                ("sub", "Durações", durations),
                ("check", "Começar a próxima fase sozinho", self.cfg.get("auto", False), self._toggle_auto),
                ("cmd", "Zerar o dia", self.reset_day)]

    # --- geometria ---

    def layout(self, mode):
        if mode == "p":
            return {"w": self.W, "h": self.COMPACT_H, "radius": self.COMPACT_RADIUS, "ring": self.C_RING}
        if mode == "m":
            return {"w": self.W, "h": self.MEDIUM_H, "radius": self.M_RADIUS, "ring": self.M_RING}
        return {"w": self.W, "h": self.HEIGHT, "radius": self.RADIUS, "ring": self.G_RING}

    def _controls(self, mode=None):
        """(nome, x, y, tamanho, ação, em_círculo) de cada botão, em pixels lógicos."""
        mode = mode or self.mode
        toggle = "pause" if self.running else "play"
        if mode == "m":
            cx, y, d = (self.M_COL_X + self.W - 16) / 2, self.M_CTRL_Y, self.M_PLAY_D
            gap = 52
        elif mode == "g":
            cx, y, d = self.W / 2, self.G_CTRL_Y, self.G_PLAY_D
            gap = 70
        else:
            return []
        return [("restart", cx - gap, y, d * 0.5, self.reset, False),
                (toggle, cx, y, d, self.toggle, True),
                ("skip", cx + gap, y, d * 0.42, self.skip, False)]

    def _control_at(self, x, y):
        for control in self._controls():
            _name, cx, cy, size, _action, circle = control
            reach = size / 2 + 2 if circle else max(size, 14) * 0.9
            if math.hypot(x - cx, y - cy) <= reach:
                return control
        return None

    def _on_ring(self, x, y):
        if self.mode == "p":
            return 6 <= x <= 168 and 6 <= y <= self.COMPACT_H - 6
        cx, cy, diameter, _ = self.layout(self.mode)["ring"]
        return math.hypot(x - cx, y - cy) <= diameter / 2 + 2

    def _clock_rect(self):
        if self.mode == "p":
            return self.W - 108, 8, self.W - 8, self.COMPACT_H - 8
        if self.mode == "m":
            return self.M_COL_X - 6, 8, self.W - 8, 72
        return self.PAD - 6, 30, 150, 86

    def _on_clock(self, x, y):
        x0, y0, x1, y1 = self._clock_rect()
        return x0 <= x <= x1 and y0 <= y <= y1

    # --- cliques, destaques e dicas ---

    def hit(self, x, y):
        control = self._control_at(x, y)
        if control:
            return control[4]
        if self._on_ring(x, y):
            return self.toggle
        if self._on_clock(x, y) and self.app.setting("atalhos_clique"):
            return lambda: open_target("ms-clock:")
        return None

    def hot_rect(self, x, y):
        control = self._control_at(x, y)
        if control:
            _name, cx, cy, size, _action, circle = control
            r = size / 2 + 4 if circle else max(size, 14) * 0.9
            return cx - r, cy - r, cx + r, cy + r, r
        if self._on_ring(x, y):
            if self.mode == "p":
                return 6, 6, 168, self.COMPACT_H - 6, 14
            cx, cy, diameter, _ = self.layout(self.mode)["ring"]
            r = diameter / 2 + 5
            return cx - r, cy - r, cx + r, cy + r, r
        if self._on_clock(x, y):
            return self._clock_rect() + (12,)
        return None

    def tooltip(self, x, y):
        control = self._control_at(x, y)
        if control:
            name = control[0]
            if name == "restart":
                return "Reiniciar esta fase"
            if name == "skip":
                return f"Pular para: {PHASES[self._next_phase()][0].lower()}"
            return "Pausar" if self.running else "Começar"
        if self._on_ring(x, y):
            label = PHASES[self.phase][0]
            lines = [f"{label}  ·  {fmt_timer(self.left())} restantes"]
            if self.running:
                lines.append("Termina às " + time.strftime("%H:%M", time.localtime(self.end)))
            if self.phase == "foco":
                lines.append(f"Pomodoro {min(self.cycle + 1, self.every)} de {self.every} até a pausa longa")
            lines.append("Clique para " + ("pausar" if self.running else "começar"))
            lines.append("Roda do mouse: ±1 minuto")
            return "\n".join(lines)
        if self._on_clock(x, y):
            today = date.today()
            stats = self._today()
            lines = [f"{WEEKDAYS_LONG[today.weekday()].capitalize()}, {today.day} de {MONTHS_LONG[today.month - 1]}",
                     f"Hoje: {stats['pomodoros']} pomodoros  ·  {fmt_minutes(stats['minutos'])} de foco"]
            if self.app.setting("atalhos_clique"):
                lines.append("Clique para abrir o Relógio do Windows")
            return "\n".join(lines)
        return None

    # --- desenho ---

    def _frac(self):
        return self.left() / self.total if self.total else 0

    def draw_shapes(self, d, k, mode):
        color = PHASES[self.phase][1]
        cx, cy, diameter, width = self.layout(mode)["ring"]
        draw_ring(d, cx * k, cy * k, diameter * k, width * k, self._frac(), color)
        if mode == "p":  # botão de play/pausa dentro do anel
            glyph = 11 * k
            if self.running:
                draw_pause(d, cx * k, cy * k, glyph, FG)
            else:
                draw_play(d, cx * k, cy * k, glyph, FG)
            return

        # um ponto por pomodoro do ciclo até a pausa longa
        n, spacing, r = self.every, 14, 3.5
        if mode == "m":
            x0, y = self.M_COL_X + r, 84
        else:
            x0, y = self.W / 2 - (n - 1) * spacing / 2, self.G_DOTS_Y
        focus = PHASES["foco"][1]
        for i in range(n):
            x = x0 + i * spacing
            box = ((x - r) * k, (y - r) * k, (x + r) * k, (y + r) * k)
            if i < self.cycle:
                d.ellipse(box, fill=focus + (255,))
            elif i == self.cycle and self.phase == "foco":
                d.ellipse(box, outline=focus + (255,), width=round(1.5 * k))
            else:
                d.ellipse(box, fill=translucent((255, 255, 255), 0.22))

        for name, bx, by, size, _action, circle in self._controls(mode):
            bx, by, size = bx * k, by * k, size * k
            if circle:
                r = size / 2
                d.ellipse((bx - r, by - r, bx + r, by + r), fill=FG)
                glyph, glyph_color = size * 0.38, (28, 28, 30, 255)
            else:
                glyph, glyph_color = size, FG
            if name == "play":
                draw_play(d, bx, by, glyph, glyph_color)
            elif name == "pause":
                draw_pause(d, bx, by, glyph, glyph_color)
            elif name == "restart":
                draw_restart(d, bx, by, glyph, glyph_color)
            else:
                draw_skip(d, bx, by, glyph, glyph_color, forward=True)

    def _status(self):
        label = PHASES[self.phase][0]
        if self.started() and not self.running:
            return label + " · pausado"
        return label

    def draw_content(self, img, d, mode):
        today = date.today()
        clock = time.strftime("%H:%M")
        color = PHASES[self.phase][1] + (255,)
        stats = self._today()
        cx, cy, diameter, _ = self.layout(mode)["ring"]
        timer = fmt_timer(self.left())

        if mode == "p":
            self.text(d, (64, cy), timer, "segoeuil.ttf", 26, anchor="lm")
            right = self.W - 16
            self.text(d, (right, 13), clock, "seguisb.ttf", 15, anchor="ra")
            status = self._status()
            if self.phase == "foco":
                status += f" · {min(self.cycle + 1, self.every)}/{self.every}"
            self.text(d, (right, 36), status, "segoeui.ttf", 10.5, MUTED, anchor="ra", max_w=104)
            return

        paused = self.started() and not self.running
        digits = MUTED if paused else FG
        label = PHASES[self.phase][0]

        if mode == "m":
            if paused:
                self.text(d, (cx, cy - 27), "PAUSADO", "seguisb.ttf", 8.5, MUTED, anchor="mm")
            self.text(d, (cx, cy - 4), timer, "segoeuil.ttf", 25 if len(timer) <= 5 else 19, digits, anchor="mm")
            self.text(d, (cx, cy + 20), label, "seguisb.ttf", 10.5, color, anchor="mm", max_w=diameter - 24)
            x = self.M_COL_X
            self.text(d, (x - 2, 10), clock, "segoeuil.ttf", 32)
            short = f"{WEEKDAYS_LONG[today.weekday()][:3]}, {today.day} de {MONTHS_LONG[today.month - 1][:3]}"
            self.text(d, (x, 52), short, "segoeui.ttf", 11, MUTED)
            self.text(d, (self.W - 16, 84), f"hoje: {stats['pomodoros']}", "segoeui.ttf", 10.5, MUTED,
                      anchor="rm")
            return

        W, PAD = self.W, self.PAD
        self.text(d, (PAD, 19), "POMODORO", "seguisb.ttf", 10, PHASES["foco"][1] + (255,))
        self.text(d, (W - PAD, 19), f"{WEEKDAYS_LONG[today.weekday()]}, {today.day} de {MONTHS_LONG[today.month - 1]}",
                  "segoeui.ttf", 10.5, MUTED, anchor="ra", max_w=190)
        self.text(d, (PAD - 2, 32), clock, "segoeuil.ttf", 40)
        n = stats["pomodoros"]
        self.text(d, (W - PAD, 46), f"{n} pomodoro{'s' if n != 1 else ''} hoje", "seguisb.ttf", 13, anchor="ra")
        self.text(d, (W - PAD, 66), f"{fmt_minutes(stats['minutos'])} de foco", "segoeui.ttf", 11, MUTED,
                  anchor="ra")
        y = round(92 * self.s)
        d.line((PAD * self.s, y, (W - PAD) * self.s, y), fill=DIVIDER, width=max(1, round(self.s)))
        if paused:
            self.text(d, (cx, cy - 34), "PAUSADO", "seguisb.ttf", 9, MUTED, anchor="mm")
        self.text(d, (cx, cy - 6), timer, "segoeuil.ttf", 34 if len(timer) <= 5 else 26, digits, anchor="mm")
        self.text(d, (cx, cy + 24), label, "seguisb.ttf", 11.5, color, anchor="mm", max_w=diameter - 30)
