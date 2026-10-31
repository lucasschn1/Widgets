"""Miniaturas dos widgets (galeria das configurações e boas-vindas)."""

from PIL import Image

from deskwidgets.widgets import WIDGET_TYPES

TIPS = [  # (título, explicação): dicas de uso, nas configurações e nas boas-vindas
    ("Mover", "Arraste de qualquer lugar. Os widgets se encaixam uns nos outros e nas bordas da tela; "
              "segure Alt para soltar livre."),
    ("Reordenar", "Arraste um widget para dentro de uma coluna: os outros abrem espaço."),
    ("Tamanho", "Botão no canto (aparece com o mouse) ou dois cliques: pequeno, médio e grande."),
    ("Espiar", "Pare o mouse sobre um widget pequeno para abri-lo por um instante."),
    ("Roda do mouse", "Volume no Tocando agora; rola as atividades do GitHub; ±1 minuto no Pomodoro."),
    ("Menu", "Clique direito num widget ou no ícone da bandeja: layouts, modo foco, edição e mais."),
]


def widget_thumbnail(app, key, scale, max_w, max_h, screen_xy, mode="m"):
    """O widget `key` em vidro, cabendo em max_w × max_h pixels (vivo se estiver aberto)."""
    cls = WIDGET_TYPES[key]
    live = app.widgets.get(key)
    if live is not None and live.alive:
        content = live.snapshot(mode, scale)
        radius = live.layout(mode)["radius"] * scale
    else:
        content = cls.preview(app, mode, scale)
        radius = 24 * scale
    f = min(1.0, max_w / content.width, max_h / content.height)
    if f < 1:
        content = content.resize((max(1, round(content.width * f)), max(1, round(content.height * f))),
                                 Image.LANCZOS)
        radius *= f
    return app.backdrop.glass(content, radius, *screen_xy)
