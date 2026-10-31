"""Caminhos dos arquivos do app (programa, configuração, cache, fontes, papel de parede).

O programa pode rodar de dois jeitos: pelo código (`pythonw widget.pyw`) ou instalado,
como um Widgets.exe gerado pelo PyInstaller. Nos dois, as opções do usuário ficam em
%APPDATA%/Widgets, fora da pasta do programa (que é trocada a cada atualização).
"""

import os
import shutil
import sys
from pathlib import Path

FROZEN = getattr(sys, "frozen", False)  # rodando como Widgets.exe
ROOT = Path(sys.executable).resolve().parent if FROZEN else Path(__file__).resolve().parent.parent
ENTRY = ROOT / "widget.pyw"  # no modo código, é o que o Windows abre ao iniciar
DATA = Path(os.environ.get("APPDATA") or Path.home()) / "Widgets"
CONFIG = DATA / "config.json"
CACHE = DATA / "cache.json"
FONTS = Path(os.environ.get("WINDIR", r"C:\Windows")) / "Fonts"
TRANSCODED = Path(os.environ.get("APPDATA", "")) / "Microsoft" / "Windows" / "Themes" / "TranscodedWallpaper"


def prepare_data_dir():
    """Cria a pasta de dados e, na primeira vez, traz o config.json e o cache.json da versão antiga
    (que ficavam ao lado do widget.pyw)."""
    DATA.mkdir(parents=True, exist_ok=True)
    for name in ("config.json", "cache.json"):
        old, new = ROOT / name, DATA / name
        if old.exists() and not new.exists():
            try:
                shutil.copy2(old, new)
            except OSError:
                pass
