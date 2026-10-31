"""Leitura e gravação da configuração, e os valores padrão das opções."""

import json

from deskwidgets.paths import CACHE, CONFIG
from deskwidgets.theme import BLUR, TINT_AMOUNT


def load_config(widget_keys):
    try:
        cfg = json.loads(CONFIG.read_text(encoding="utf-8-sig"))
    except (OSError, ValueError):
        cfg = {}
    if cfg and "sistema" not in cfg:  # formato antigo: só o widget de clima e sistema
        cfg = {"sistema": cfg}
    for key in widget_keys:
        cfg.setdefault(key, {})
    cfg.setdefault("app", {})  # opções gerais (ex.: posições travadas)
    return cfg


def save_config(cfg):
    CONFIG.write_text(json.dumps(cfg, ensure_ascii=False, indent=2), encoding="utf-8")


def load_cache():
    try:
        return json.loads(CACHE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def save_cache(cache):
    try:
        CACHE.write_text(json.dumps(cache, ensure_ascii=False), encoding="utf-8")
    except OSError:
        pass


# valores padrão das opções gerais (cfg["app"]); o que não foi mudado não é gravado
SETTINGS = {
    "desfoque": BLUR, "escurecimento": TINT_AMOUNT, "texto": 0.0,  # 0 = seguir o Windows
    "esmaecer": True, "opacidade_ociosa": 0.8, "animacoes": "auto",
    "expandir_hover": True, "atalhos_clique": True, "dicas": True, "economia_bateria": True,
    "atalho": "Ctrl+Alt+W", "atalho_pomodoro": "Ctrl+Alt+P",
    "notificacoes": {"cpu": True, "ram": True, "disco": True, "chuva": True, "github": True,
                     "pomodoro": True},
}
# opções que "Restaurar padrão" de cada página das configurações volta ao normal
APPEARANCE_KEYS = ("desfoque", "escurecimento", "texto", "esmaecer", "opacidade_ociosa", "animacoes")
BEHAVIOR_KEYS = ("expandir_hover", "atalhos_clique", "dicas", "economia_bateria")

ANIMATION_CHOICES = {"auto": "Seguir o Windows", "sempre": "Sempre", "nunca": "Nunca"}
NOTIFY_LABELS = {  # tipo: (título, descrição)
    "cpu": ("CPU alta", "Quando a CPU passa de 90% por 3 minutos"),
    "ram": ("Memória quase cheia", "Quando a memória passa de 90% por 3 minutos"),
    "disco": ("Disco quase cheio", "Quando o disco C: passa de 95% de uso"),
    "chuva": ("Chuva chegando", "Quando a previsão indica chuva na próxima hora"),
    "github": ("Revisão pedida no GitHub", "Quando alguém pede sua revisão num pull request"),
    "pomodoro": ("Pomodoro", "No fim de cada foco e de cada pausa"),
}
