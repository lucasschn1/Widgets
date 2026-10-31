"""Iniciar com o Windows (chave Run do registro)."""

import os
import sys
import winreg
from pathlib import Path

from deskwidgets.paths import ENTRY, FROZEN
from deskwidgets.win32 import registry

STARTUP_LNK = (Path(os.environ.get("APPDATA", "")) / "Microsoft" / "Windows" / "Start Menu" / "Programs"
               / "Startup" / "Widget Clima e Sistema.lnk")
RUN_KEY, RUN_NAME = r"Software\Microsoft\Windows\CurrentVersion\Run", "DesktopWidgets"


def launch_command():
    """O comando que abre o app: o Widgets.exe instalado, ou o pythonw com o widget.pyw."""
    if FROZEN:
        return f'"{Path(sys.executable).resolve()}"'
    exe = Path(sys.executable)
    pythonw = exe.with_name("pythonw.exe") if exe.with_name("pythonw.exe").exists() else exe
    return f'"{pythonw}" "{ENTRY}"'


def startup_enabled():
    return STARTUP_LNK.exists() or bool(registry(RUN_KEY, RUN_NAME))


def set_startup(on):
    if on:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY, 0, winreg.KEY_SET_VALUE) as key:
            winreg.SetValueEx(key, RUN_NAME, 0, winreg.REG_SZ, launch_command())
        return
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY, 0, winreg.KEY_SET_VALUE) as key:
            winreg.DeleteValue(key, RUN_NAME)
    except OSError:
        pass
    try:
        STARTUP_LNK.unlink()  # o atalho antigo na pasta Inicializar
    except OSError:
        pass


def refresh_startup():
    """Se a inicialização está ligada mas aponta para outro lugar (ex.: passou do código para o
    Widgets.exe, ou o app mudou de pasta), corrige o caminho."""
    current = registry(RUN_KEY, RUN_NAME)
    if (current and current != launch_command()) or STARTUP_LNK.exists():
        try:
            set_startup(True)
            STARTUP_LNK.unlink(missing_ok=True)
        except OSError:
            pass
