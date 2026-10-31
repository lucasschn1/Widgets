"""Atualizações pelo GitHub: compara a versão instalada com o último Release e atualiza com um clique."""

import subprocess
import threading
import webbrowser

from deskwidgets import REPO, __version__
from deskwidgets.paths import FROZEN
from deskwidgets.util import fetch_json

RELEASES = f"https://github.com/{REPO}/releases"
INSTALL_SCRIPT = f"https://raw.githubusercontent.com/{REPO}/main/install.ps1"


def parse_version(text):
    """"v2.1.0" -> (2, 1, 0); partes que não são números contam como 0."""
    parts = []
    for piece in text.strip().lstrip("vV").split("."):
        digits = "".join(ch for ch in piece if ch.isdigit())
        parts.append(int(digits) if digits else 0)
    return tuple(parts + [0] * (3 - len(parts)))


def latest_release():
    """{"version", "url", "notes"} do último Release publicado (levanta erro sem conexão)."""
    data = fetch_json(f"https://api.github.com/repos/{REPO}/releases/latest")
    return {"version": data["tag_name"].lstrip("vV"), "url": data["html_url"], "notes": data.get("body") or ""}


def is_newer(version):
    return parse_version(version) > parse_version(__version__)


def check_async(callback):
    """Procura atualização numa thread; `callback(release_ou_None, erro_ou_None)` roda nessa thread."""
    def work():
        try:
            release = latest_release()
            callback(release if is_newer(release["version"]) else None, None)
        except Exception as exc:  # sem internet, sem Releases ainda, limite da API...
            callback(None, exc)
    threading.Thread(target=work, daemon=True).start()


def install_update():
    """Instalado (Widgets.exe): roda o install.ps1, que fecha o app, troca os arquivos e abre de novo.
    Rodando pelo código: só abre a página de Releases."""
    if not FROZEN:
        webbrowser.open(RELEASES)
        return
    command = f"irm {INSTALL_SCRIPT} | iex"
    subprocess.Popen(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden",
                      "-Command", command], creationflags=0x08000000)  # CREATE_NO_WINDOW
