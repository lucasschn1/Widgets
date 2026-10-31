"""O aplicativo: cria os widgets e coordena configurações, menus, avisos e modos."""

import copy
import ctypes
import queue
import shutil
import time
import tkinter as tk
from ctypes import wintypes

from deskwidgets import updates
from deskwidgets.backdrop import Backdrop
from deskwidgets.config import SETTINGS, load_cache, load_config, save_cache, save_config
from deskwidgets.paths import prepare_data_dir
from deskwidgets.startup import refresh_startup
from deskwidgets.theme import ANIM_MS, EDGE_GAP, PEEK_IDLE_MS, WIDGET_GAP
from deskwidgets.tray import Tray
from deskwidgets.ui.menu import GlassMenu
from deskwidgets.ui.overlay import Guides, Tooltip
from deskwidgets.ui.toast import EditBar, Toast
from deskwidgets.widgets import WIDGET_TYPES
from deskwidgets.widgets.media import MediaWidget
from deskwidgets.win32 import (
    HWND_NOTOPMOST, HWND_TOPMOST, SYSTEM_POWER_STATUS, area_for, cpu_times, desktop_window, gb, kernel32,
    memory, open_target, overlap, pin_to_desktop, registry, set_z, shell32, user32,
)

HOTKEY_SETTINGS = {"peek": "atalho", "pomodoro": "atalho_pomodoro"}  # atalho global: opção no cfg["app"]
HOTKEY_LABELS = {"peek": "Mostrar widgets na frente", "pomodoro": "Começar ou pausar o Pomodoro"}


class App:
    BACKDROP_EVERY_MS = 5000  # de quanto em quanto tempo ver se o papel de parede mudou
    FULLSCREEN_EVERY_MS = 2000
    EVENTS_EVERY_MS = 100
    POWER_EVERY_MS = 30000
    ALERTS_EVERY_MS = 10000
    UPDATE_FIRST_MS = 20 * 1000          # primeira busca por atualização, logo depois de abrir
    UPDATE_EVERY_MS = 6 * 60 * 60 * 1000  # e depois a cada 6 horas

    def __init__(self):
        try:
            # cada monitor com a sua escala (os widgets se redesenham ao mudar de monitor)
            ctypes.windll.shcore.SetProcessDpiAwareness(2)
        except (AttributeError, OSError):
            pass
        self.cfg = load_config(WIDGET_TYPES)
        self.save()  # grava já no formato atual, caso tenha vindo do antigo
        self.cache = load_cache()
        self.root = tk.Tk()
        self.root.withdraw()
        self.s = self.scale_at((0, 0))
        self.paused = False      # pausado enquanto há um programa em tela cheia
        self.peeking = False     # widgets mostrados na frente das janelas
        self.editing = False     # modo edição: fechar, redimensionar e arrastar à vontade
        self.saving = False      # economia de energia (na bateria)
        self.focus_until = None  # modo foco: widgets escondidos até esse horário (ou "sempre")
        self.settings_window = self.welcome_window = None
        self.hotkey_errors = {}  # atalho que outro programa já usa: nome -> rótulo pedido
        self.update = None       # Release mais novo que a versão instalada (ou None)
        self.update_status = "Ainda não procurou"
        self._update_result = None
        self._peek_job = self._focus_job = None
        self._notified = {}      # última notificação de cada tipo (para não repetir)
        self._high_since = {}
        self._alert_cpu = cpu_times()
        self._notification_action = None
        self._read_preferences()
        self.backdrop = Backdrop(self)
        self.guides = Guides(self.root, self.s)
        self.tooltip = Tooltip(self.root, self.s)
        self.menu = GlassMenu(self)
        self.toasts = Toast(self)
        self.edit_bar = EditBar(self)

        self.events = queue.Queue()
        self.tray = Tray(self.events, {name: self.setting(opt) for name, opt in HOTKEY_SETTINGS.items()})
        self.tray.start()
        self.tray.ready.wait(3)

        visible = {key: self.cfg[key].get("visible", True) for key in WIDGET_TYPES}
        if not self.tray.ok and not any(visible.values()):
            visible = dict.fromkeys(visible, True)  # sem bandeja, não pode ficar sem nada na tela
        self.widgets = {}
        for key, cls in WIDGET_TYPES.items():
            if visible[key]:
                self.widgets[key] = cls(self)

        self._power_tick()
        self.root.after(self.BACKDROP_EVERY_MS, self._check_backdrop)
        self.root.after(self.FULLSCREEN_EVERY_MS, self._check_fullscreen)
        self.root.after(self.EVENTS_EVERY_MS, self._pump)
        self.root.after(self.ALERTS_EVERY_MS, self._check_alerts)
        self.root.after(1500, self._first_run)
        self.root.after(self.UPDATE_FIRST_MS, self.check_updates)

    def _first_run(self):
        if not self.cfg["app"].get("dicas_vistas"):
            self.open_welcome()
        wanted = self.setting("atalho")
        if self.tray.ok and wanted and self.tray.hotkey != wanted:
            chosen = self.tray.hotkey or "nenhum"
            self.toast(f"{wanted} já é usado por outro programa", detail=f"Atalho atual: {chosen}",
                       icon="atalhos", color=(255, 184, 0), action_label="Trocar",
                       action=lambda: self.open_settings("atalhos"))

    # --- configurações ---

    def setting(self, name):
        value = self.cfg["app"].get(name)
        return SETTINGS[name] if value is None else value

    def set_setting(self, name, value, apply=True):
        """Grava uma opção geral e aplica na hora (as configurações não têm botão "Salvar")."""
        if value == SETTINGS.get(name):
            self.cfg["app"].pop(name, None)  # no padrão: não precisa gravar
        else:
            self.cfg["app"][name] = value
        self.save()
        if apply:
            glass_only = name in ("desfoque", "escurecimento", "esmaecer", "opacidade_ociosa")
            self.apply_settings(rebuild=name in ("desfoque", "escurecimento"), redraw=not glass_only)

    def notify_enabled(self, kind):
        return bool({**SETTINGS["notificacoes"], **self.setting("notificacoes")}.get(kind))

    def set_notify(self, kind, on):
        current = {**SETTINGS["notificacoes"], **self.setting("notificacoes")}
        current[kind] = on
        self.cfg["app"]["notificacoes"] = current
        self.save()

    def _read_preferences(self):
        """Valores que dependem das configurações e do Windows (texto, animações, opacidade)."""
        text = self.setting("texto")
        if not text:  # segue o "Tamanho do texto" da acessibilidade do Windows, até 120%
            try:
                text = int(registry(r"Software\Microsoft\Accessibility", "TextScaleFactor") or 100) / 100
            except ValueError:
                text = 1.0
        self.text_scale = min(max(text, 0.9), 1.2)
        mode = self.setting("animacoes")
        if mode == "auto":
            enabled = wintypes.BOOL(True)
            user32.SystemParametersInfoW(0x1042, 0, ctypes.byref(enabled), 0)  # SPI_GETCLIENTAREAANIMATION
            self.animations = bool(enabled.value)
        else:
            self.animations = mode == "sempre"
        self.idle_opacity = self.setting("opacidade_ociosa") if self.setting("esmaecer") else 1.0

    def apply_settings(self, rebuild=False, redraw=True):
        """Aplica as opções: `rebuild` refaz o vidro; sem `redraw`, só reexibe (mais leve)."""
        self._read_preferences()
        if rebuild:
            self.backdrop.rebuild()
        self._check_power()
        for widget in self._alive():
            if redraw:
                widget.refresh()
            else:
                widget.present()

    def open_settings(self, page=None):
        from deskwidgets.ui.settings import SettingsWindow
        if self.settings_window and self.settings_window.alive:
            if page:
                self.settings_window.go(page)
            self.settings_window.win.deiconify()
            self.settings_window.win.lift()
            self.settings_window.win.focus_force()
        else:
            self.settings_window = SettingsWindow(self, page)

    def changed(self):
        """Algo mudou fora das configurações (ex.: widget fechado): elas se redesenham."""
        if self.settings_window and self.settings_window.alive:
            self.settings_window.changed()

    def open_welcome(self):
        from deskwidgets.ui.onboarding import Welcome
        if self.welcome_window and self.welcome_window.alive:
            self.welcome_window.win.lift()
            self.welcome_window.win.focus_force()
        else:
            self.welcome_window = Welcome(self)

    def set_hotkey(self, name, label):
        """Troca um atalho global; se outro programa já usar, avisa (evento "hotkey_set")."""
        self.set_setting(HOTKEY_SETTINGS[name], label, apply=False)
        self.hotkey_errors.pop(name, None)
        self.tray.set_hotkey(name, label)

    def reset_all_settings(self):
        """Volta todas as opções gerais ao padrão (com "Desfazer")."""
        keep = {k: v for k, v in self.cfg["app"].items() if k in ("layouts", "dicas_vistas", "travado")}
        old = copy.deepcopy(self.cfg["app"])
        self.cfg["app"] = keep
        self._after_reset()

        def undo():
            self.cfg["app"] = old
            self._after_reset()

        self.toast("Configurações restauradas", action=undo)

    def _after_reset(self):
        self.save()
        for name, opt in HOTKEY_SETTINGS.items():
            self.tray.set_hotkey(name, self.setting(opt))
        self.apply_settings(rebuild=True)
        self.changed()

    def save(self):
        save_config(self.cfg)

    def save_cache(self):
        save_cache(self.cache)

    @property
    def locked(self):
        return bool(self.cfg["app"].get("travado"))

    def set_locked(self, on):
        self.cfg["app"]["travado"] = on
        self.save()
        self.changed()

    @property
    def suspended(self):
        """Não vale a pena redesenhar: tela cheia ou modo foco."""
        return self.paused or bool(self.focus_until)

    def scale_at(self, point):
        """Escala (1 = 96 dpi) do monitor onde está o ponto."""
        monitor = user32.MonitorFromPoint(wintypes.POINT(*point), 2)  # o mais próximo
        dpi_x, dpi_y = wintypes.UINT(96), wintypes.UINT(96)
        try:
            ctypes.windll.shcore.GetDpiForMonitor(monitor, 0, ctypes.byref(dpi_x), ctypes.byref(dpi_y))
        except (AttributeError, OSError):
            pass
        return dpi_x.value / 96

    def _alive(self):
        return [w for w in self.widgets.values() if w.alive]

    # --- avisos e perguntas ---

    def toast(self, text, action_label=None, action=None, icon="check", color=(48, 209, 88), detail=None):
        """Aviso rápido na parte de baixo da tela, com um botão opcional (ex.: "Desfazer")."""
        self.toasts.notify(text, action_label, action, icon, color, detail)

    def prompt(self, title, message, on_done, initial="", anchor=None, cities=False, placeholder=""):
        """Pergunta de vidro com um campo de texto; `on_done(texto, cidade_escolhida)` ao confirmar."""
        from deskwidgets.ui.prompt import Prompt
        Prompt(self, title, message, initial=initial, on_done=on_done, anchor=anchor, cities=cities,
               placeholder=placeholder)

    def notify(self, kind, title, text, cooldown=0, action=None):
        if not self.notify_enabled(kind):
            return
        now = time.time()
        if now - self._notified.get(kind, 0) < cooldown:
            return
        self._notified[kind] = now
        self._notification_action = action
        self.tray.notify(title, text)

    # --- atualizações ---

    def check_updates(self, manual=False):
        """Procura uma versão nova no GitHub (sozinho a cada 6 h, ou pelo botão em Sobre)."""
        if not manual:
            self.root.after(self.UPDATE_EVERY_MS, self.check_updates)
        self.update_status = "Procurando…"
        self.changed()

        def done(release, error):
            self._update_result = (release, error, manual)
        updates.check_async(done)
        self.root.after(300, self._update_poll)

    def _update_poll(self):
        if self._update_result is None:
            self.root.after(300, self._update_poll)
            return
        release, error, manual = self._update_result
        self._update_result = None
        when = time.strftime("%H:%M")
        if error:
            self.update_status = f"Não deu para verificar ({when})"
            if manual:
                self.toast("Não deu para procurar atualizações", detail="Verifique a conexão", icon="close",
                           color=(255, 184, 0))
        elif release:
            self.update = release
            self.update_status = f"Versão {release['version']} disponível"
            if manual or self.cache.get("update_notified") != release["version"]:
                self.cache["update_notified"] = release["version"]
                self.save_cache()
                self.toast(f"Versão {release['version']} disponível", action_label="Atualizar",
                           action=self.install_update, icon="refresh", color=(10, 132, 255))
        else:
            self.update = None
            self.update_status = f"Você tem a versão mais recente (verificado às {when})"
            if manual:
                self.toast("Você já tem a versão mais recente")
        self.changed()

    def install_update(self):
        if updates.FROZEN:
            self.toast("Atualizando…", detail="Os widgets fecham e abrem de novo sozinhos", icon="refresh",
                       color=(10, 132, 255))
        updates.install_update()

    # --- verificações periódicas ---

    def _check_backdrop(self):
        changed = self.backdrop.check()  # inclui mudança de monitores e resolução
        progman = desktop_window()
        animations = self.animations
        self._read_preferences()  # o usuário pode ter mudado as animações ou o texto no Windows
        for widget in self._alive():
            if progman and widget.owner != progman:  # o Explorer reiniciou
                widget.owner = pin_to_desktop(widget.hwnd)
            if changed and widget._card is not None and not widget._animating:
                widget.present()
                widget.ensure_visible()
        if animations != self.animations:
            self.apply_settings()
        self.root.after(self.BACKDROP_EVERY_MS, self._check_backdrop)

    def _check_fullscreen(self):
        # jogo, vídeo em tela cheia ou apresentação: os widgets nem aparecem, então
        # param de redesenhar até o programa sair da tela cheia
        state = ctypes.c_int()
        busy = shell32.SHQueryUserNotificationState(ctypes.byref(state)) == 0 and state.value in (2, 3, 4)
        if busy != self.paused:
            self.paused = busy
            if not self.suspended:
                self._wake_all()
        self.root.after(self.FULLSCREEN_EVERY_MS, self._check_fullscreen)

    def _wake_all(self):
        for widget in self._alive():
            if widget._stale:
                widget.refresh()
            else:
                widget.present()

    def _check_power(self):
        """Na bateria (com a economia ligada), atualiza menos e para as animações contínuas."""
        status = SYSTEM_POWER_STATUS()
        on_battery = bool(kernel32.GetSystemPowerStatus(ctypes.byref(status))) and status.ACLineStatus == 0
        self.saving = on_battery and self.setting("economia_bateria")

    def _power_tick(self):
        self._check_power()
        self.root.after(self.POWER_EVERY_MS, self._power_tick)

    def _check_alerts(self):
        """Notifica CPU ou memória altas por alguns minutos e disco quase cheio."""
        now = time.time()
        idle, total = cpu_times()
        last_idle, last_total = self._alert_cpu
        self._alert_cpu = (idle, total)
        cpu = 1 - (idle - last_idle) / (total - last_total) if total > last_total else 0
        used, total_mem = memory()
        for kind, value, text in (
                ("cpu", cpu, "A CPU está acima de 90% há 3 minutos."),
                ("ram", used / total_mem, "A memória está acima de 90% há 3 minutos.")):
            if value >= 0.9 and self.notify_enabled(kind):
                since = self._high_since.setdefault(kind, now)
                if now - since >= 180:
                    title = "CPU alta" if kind == "cpu" else "Memória quase cheia"
                    self.notify(kind, title, text, cooldown=1800, action=lambda: open_target("taskmgr.exe"))
            else:
                self._high_since.pop(kind, None)
        disk = shutil.disk_usage("C:\\")
        if disk.used / disk.total >= 0.95:
            self.notify("disco", "Disco quase cheio",
                        f"O disco C: está com {round(disk.used / disk.total * 100)}% de uso "
                        f"({gb(disk.free):.0f} GB livres).",
                        cooldown=86400, action=lambda: open_target("ms-settings:storagesense"))
        self.root.after(self.ALERTS_EVERY_MS, self._check_alerts)

    def _pump(self):
        """Atende os pedidos da bandeja e dos atalhos de teclado."""
        while True:
            try:
                kind, value = self.events.get_nowait()
            except queue.Empty:
                break
            if kind == "peek" or (kind == "hotkey" and value == "peek"):
                self.toggle_peek()
            elif kind == "hotkey" and value == "pomodoro":
                self.toggle_pomodoro()
            elif kind == "tray_menu":
                pt = wintypes.POINT()
                user32.GetCursorPos(ctypes.byref(pt))
                self.menu.popup(self.common_entries(None), pt.x, pt.y)
            elif kind == "notification" and self._notification_action:
                self._notification_action()
            elif kind == "other_instance":
                self.toast("Os widgets já estão abertos", detail="Clique direito num widget ou no ícone da bandeja",
                           icon="widgets", color=(10, 132, 255))
                if not self.peeking and not self.editing:
                    self.toggle_peek()
            elif kind == "hotkey_set":
                name, registered = value
                wanted = self.setting(HOTKEY_SETTINGS[name])
                if wanted and registered != wanted:
                    self.hotkey_errors[name] = wanted
                    self.toast(f"{wanted} já é usado por outro programa", detail="Escolha outra combinação",
                               icon="atalhos", color=(255, 184, 0))
                self.changed()
        self.root.after(self.EVENTS_EVERY_MS, self._pump)

    # --- menu ---

    def common_entries(self, widget):
        """Itens do menu que valem para todos os widgets (também é o menu da bandeja)."""
        widgets = [("check", cls.title, key in self.widgets,
                    lambda k=key: self.set_visible(k, k not in self.widgets))
                   for key, cls in WIDGET_TYPES.items()]
        widgets += [None, ("cmd", "Galeria de widgets…", lambda: self.open_settings("widgets"))]
        layouts = self.cfg["app"].get("layouts", {})
        layout_items = [("cmd", name, lambda n=name: self.apply_layout(n)) for name in layouts]
        if layout_items:
            layout_items.append(None)
        layout_items.append(("cmd", "Salvar layout atual…", self.save_layout))
        if layouts:
            layout_items.append(("sub", "Excluir", [("cmd", name, lambda n=name: self.delete_layout(n))
                                                    for name in layouts]))
        if self.focus_until:
            until = "" if self.focus_until == "sempre" else f" (até {time.strftime('%H:%M', time.localtime(self.focus_until))})"
            focus = [("cmd", "Sair do modo foco" + until, self.end_focus)]
        else:
            focus = [("cmd", label, lambda m=minutes: self.start_focus(m))
                     for label, minutes in (("30 minutos", 30), ("1 hora", 60), ("2 horas", 120),
                                            ("Até eu desligar", None))]
        entries = [("sub", "Widgets", widgets), ("sub", "Layouts", layout_items), None,
                   ("cmd", "Editar widgets", self.start_edit if not self.editing else self.end_edit),
                   ("cmd", "Mostrar na frente", self.toggle_peek, self.tray.hotkey),
                   ("sub", "Modo foco", focus),
                   ("cmd", "Organizar widgets", self.arrange),
                   ("check", "Travar posições", self.locked, lambda: self.set_locked(not self.locked)),
                   None,
                   ("cmd", "Configurações…", self.open_settings),
                   ("cmd", "Atalhos e dicas", lambda: self.open_settings("atalhos")),
                   None]
        if widget:
            entries.append(("cmd", "Fechar este widget", lambda: self.close_widget(widget.key)))
        entries.append(("cmd", "Sair", self.quit))
        return entries

    # --- layouts ---

    def save_layout(self):
        self.prompt("Salvar layout", "Dê um nome para a posição atual dos widgets (ex.: Trabalho, Jogos).",
                    lambda name, _p: self._save_layout(name.strip()), placeholder="Nome do layout")

    def _save_layout(self, name):
        if not name:
            return
        snapshot = {}
        for key in WIDGET_TYPES:
            widget = self.widgets.get(key)
            if widget and widget.alive:
                mode = "p" if widget._temp else widget.mode
                snapshot[key] = {"visible": True, "x": widget.pos[0], "y": widget.pos[1], "tamanho": mode}
            else:
                snapshot[key] = {"visible": False}
        self.cfg["app"].setdefault("layouts", {})[name] = snapshot
        self.save()
        self.toast(f"Layout \"{name}\" salvo", detail="Aplique pelo menu Layouts")
        self.changed()

    def delete_layout(self, name):
        layouts = self.cfg["app"].get("layouts", {})
        old = layouts.pop(name, None)
        self.save()
        self.changed()
        if old is not None:
            def undo():
                self.cfg["app"].setdefault("layouts", {})[name] = old
                self.save()
                self.changed()
            self.toast(f"Layout \"{name}\" excluído", action=undo, icon="close", color=(255, 255, 255))

    def apply_layout(self, name):
        snapshot = self.cfg["app"].get("layouts", {}).get(name)
        if not snapshot:
            return
        for key, state in snapshot.items():
            if key not in WIDGET_TYPES:
                continue
            if not state.get("visible"):
                self.set_visible(key, False, reflow=False)
                continue
            widget = self.widgets.get(key)
            if widget is None:
                self.cfg[key].update(x=state["x"], y=state["y"], tamanho=state["tamanho"])
                self.set_visible(key, True, place=False)
                continue
            if widget._temp:
                widget._temp = False
                widget.mode = "p"
            if widget.mode != state["tamanho"]:
                widget.resize_to(state["tamanho"], reflow=False)
                self.root.after(ANIM_MS + 40, lambda w=widget, s=state: w.slide_to((s["x"], s["y"])))
            else:
                widget.slide_to((state["x"], state["y"]))
        self.toast(f"Layout \"{name}\" aplicado")

    # --- modo foco ---

    def start_focus(self, minutes):
        """Esconde os widgets por um tempo (ou até desligar), sem fechar nada."""
        self.end_peek()
        self.end_edit()
        self.focus_until = time.time() + minutes * 60 if minutes else "sempre"
        for widget in self._alive():
            user32.ShowWindow(widget.hwnd, 0)
        if minutes:
            self._focus_job = self.root.after(minutes * 60 * 1000, self.end_focus)
            self.tray.set_tip(f"Modo foco até {time.strftime('%H:%M', time.localtime(self.focus_until))}")
        else:
            self.tray.set_tip("Modo foco ligado")
        self.toast("Modo foco ligado", detail="Os widgets voltam ao sair pelo ícone da bandeja", icon="pin",
                   color=(191, 90, 242))

    def end_focus(self):
        if self._focus_job:
            self.root.after_cancel(self._focus_job)
            self._focus_job = None
        self.focus_until = None
        self.tray.set_tip(Tray.TIP)
        for widget in self._alive():
            user32.ShowWindow(widget.hwnd, 8)  # SW_SHOWNA
            widget.send_to_back()
        self._wake_all()

    # --- modo edição ---

    def start_edit(self):
        """Todos os widgets na frente, com "−" para fechar e o botão de tamanho sempre visível."""
        if self.editing:
            return
        if self.focus_until:
            self.end_focus()
        self.end_peek()
        self.editing = True
        for widget in self._alive():
            set_z(widget.hwnd, HWND_TOPMOST)
            widget.present()
        self.edit_bar.open()

    def end_edit(self):
        if not self.editing:
            return
        self.editing = False
        self.edit_bar.hide()
        for widget in self._alive():
            set_z(widget.hwnd, HWND_NOTOPMOST)
            widget._close_hot = False
            widget.send_to_back()
            widget.present()

    # --- ações ---

    def toggle_pomodoro(self):
        """Atalho global do Pomodoro: começa ou pausa (e abre o widget, se estiver oculto)."""
        from deskwidgets.widgets.pomodoro import PHASES, fmt_timer
        if "pomodoro" not in self.widgets:
            self.set_visible("pomodoro", True)
        widget = self.widgets["pomodoro"]
        widget.toggle()
        phase, color = PHASES[widget.phase]
        detail = f"{phase}  ·  {fmt_timer(widget.left())} restantes"
        if widget.running:
            self.toast("Pomodoro rodando", detail=detail, icon="pomodoro", color=color)
        else:
            self.toast("Pomodoro pausado", detail=detail, icon="pomodoro", color=(255, 255, 255))

    def toggle_peek(self):
        """Mostra os widgets por cima das janelas, até chamar de novo ou o mouse sair deles."""
        if self.editing:
            return
        if self.focus_until:
            self.end_focus()
        if self.peeking:
            self.end_peek()
            return
        self.peeking = True
        for widget in self._alive():
            set_z(widget.hwnd, HWND_TOPMOST)
            widget.present()  # opacidade total enquanto estão na frente
        self._peek_seen = time.perf_counter()
        self._peek_job = self.root.after(250, self._peek_check)

    def _peek_check(self):
        if any(w._hover_target > 0 for w in self._alive()) or self.menu.open:
            self._peek_seen = time.perf_counter()
        if (time.perf_counter() - self._peek_seen) * 1000 > PEEK_IDLE_MS:
            self.end_peek()
        else:
            self._peek_job = self.root.after(250, self._peek_check)

    def end_peek(self):
        if self._peek_job:
            self.root.after_cancel(self._peek_job)
            self._peek_job = None
        if not self.peeking:
            return
        self.peeking = False
        for widget in self._alive():
            set_z(widget.hwnd, HWND_NOTOPMOST)
            widget.send_to_back()
            widget.present()

    def arrange(self):
        """Empilha os widgets em colunas a partir da direita da tela, com o espaço padrão.

        Quando o próximo não cabe mais na altura da tela, começa outra coluna à esquerda.
        """
        widgets = sorted((w for w in self._alive() if w._card is not None and not w._temp),
                         key=lambda w: (-w.card_rect()[2], w.card_rect()[1]))
        if not widgets:
            return
        _, top, right, bottom = area_for(widgets[0].card_rect())
        edge, gap = EDGE_GAP * self.s, WIDGET_GAP * self.s
        y, column_w = top + edge, 0
        right -= edge
        for widget in widgets:
            w, h = widget.card_size()
            if column_w and y + h > bottom - edge:  # não cabe: próxima coluna
                right -= column_w + gap
                y, column_w = top + edge, 0
            widget.slide_to((round(right - w - widget.margin), round(y - widget.margin)))
            y += h + gap
            column_w = max(column_w, w)

    def close_widget(self, key):
        """Fecha um widget com aviso e "Desfazer" (que o devolve ao lugar, com os vizinhos)."""
        widget = self.widgets.get(key)
        if widget is None:
            return
        followers = [(o, o.pos) for o in widget.stack_below()]
        pos, mode = widget.pos, ("p" if widget._temp else widget.mode)
        if not self.set_visible(key, False):
            return

        def undo():
            if key in self.widgets:
                return
            for other, old_pos in followers:
                if other.alive:
                    other.slide_to(old_pos)
            self.cfg[key].update(x=pos[0], y=pos[1], tamanho=mode)
            self.set_visible(key, True, place=False)
            if self.editing:
                new = self.widgets[key]
                set_z(new.hwnd, HWND_TOPMOST)

        self.toast(f"{WIDGET_TYPES[key].title} fechado", action=undo, icon="close", color=(255, 255, 255))

    def set_visible(self, key, on, reflow=True, place=True):
        """Mostra ou esconde um widget; devolve False se não deu (o último, sem bandeja)."""
        if not on and not self.tray.ok and set(self.widgets) == {key}:
            # sem a bandeja, fechar o último widget encerraria o programa
            self.toast("Não dá para fechar o último widget sem o ícone na bandeja", icon="close",
                       color=(255, 184, 0))
            return False
        self.cfg[key]["visible"] = on
        self.save()
        if on and key not in self.widgets:
            widget = self.widgets[key] = WIDGET_TYPES[key](self)
            # se a posição guardada ficou ocupada, vai para baixo do último widget
            others = widget.others()
            if place and others and any(min(overlap(widget.card_rect(), o.card_rect())) > 0 for o in others):
                widget.slide_to(widget.below(max(others, key=lambda o: o.card_rect()[3])))
            if self.editing or self.peeking:
                set_z(widget.hwnd, HWND_TOPMOST)
        elif not on and key in self.widgets:
            widget = self.widgets.pop(key)
            # os widgets encaixados embaixo sobem para ocupar o lugar
            followers = widget.stack_below() if reflow else []
            shift = widget.card_size()[1] + round(WIDGET_GAP * self.s)
            widget.close()
            for other in followers:
                other.slide_to((other.pos[0], other.pos[1] - shift))
        self.changed()
        return True

    def revive(self, key):
        """Recria um widget cuja janela o Windows destruiu (ex.: o Explorer reiniciou)."""
        old = self.widgets.get(key)
        if old is None or old.alive:
            return
        if not desktop_window():  # o Explorer ainda está voltando
            self.root.after(2000, lambda: self.revive(key))
            return
        if isinstance(old, MediaWidget):
            old.watcher.stop()
        self.widgets[key] = WIDGET_TYPES[key](self)

    def quit(self):
        self.menu.close()
        self.tray.stop()
        for widget in self.widgets.values():
            widget.alive = False
            if isinstance(widget, MediaWidget):
                widget.watcher.stop()
        self.root.destroy()

    def run(self):
        self.root.mainloop()


MUTEX_NAME = "Local\\DesktopWidgets-lucasschn1"
_mutex = None  # fica vivo enquanto o app roda


def already_running():
    """Se já há um app aberto, pede para ele se mostrar e devolve True."""
    global _mutex
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CreateMutexW.restype = wintypes.HANDLE
    k32.CreateMutexW.argtypes = [ctypes.c_void_p, wintypes.BOOL, wintypes.LPCWSTR]
    _mutex = k32.CreateMutexW(None, False, MUTEX_NAME)
    if ctypes.get_last_error() != 183:  # ERROR_ALREADY_EXISTS
        return False
    hwnd = user32.FindWindowW(Tray.CLASS_NAME, None)
    if hwnd:
        user32.PostMessageW(hwnd, Tray.WM_OTHER_INSTANCE, 0, 0)
    return True


def main():
    if already_running():
        return
    prepare_data_dir()
    refresh_startup()
    App().run()
