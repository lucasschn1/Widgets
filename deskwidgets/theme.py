"""Medidas, cores, fontes e animações compartilhadas por todo o visual."""

from functools import lru_cache

from PIL import Image, ImageDraw, ImageFont

from deskwidgets.paths import FONTS


# medidas em pixels "lógicos" (96 dpi); multiplicadas pela escala da tela
SS = 3  # supersampling das formas, para bordas suaves
MARGIN = 8  # área transparente em volta do cartão, para o botão sobressair na borda
BADGE_D = 20
ANIM_MS = 300
BADGE_FADE_MS = 120
FRAME_MS = 16
WIDGET_GAP = 12  # espaço entre widgets encaixados
EDGE_GAP = 24    # espaço até a borda da tela, ao encaixar nela
SNAP_DIST = 12   # a que distância o encaixe "puxa" o widget
DRAG_START = 4   # quanto o mouse precisa andar para um clique virar arraste
TIP_DELAY_MS = 450
PEEK_IDLE_MS = 5000  # "mostrar na frente" volta ao normal depois disso sem o mouse em cima

# vidro: o papel de parede atrás do cartão, desfocado, mais saturado e escurecido
BLUR = 26            # raio do desfoque, em pixels lógicos (ajustável nas configurações)
SATURATION = 1.6
TINT = (18, 18, 24)
TINT_AMOUNT = 0.40   # escurecimento base; sobe sozinho em papéis de parede claros
SHEEN = 0.10         # reflexo branco no topo do vidro
EDGE = (0.45, 0.10)  # opacidade da borda clara, no topo e na base


def translucent(rgb, amount):
    """Cor translúcida, que deixa o vidro aparecer por trás."""
    return tuple(rgb) + (round(255 * amount),)


DIVIDER = translucent((255, 255, 255), 0.16)
FG = (250, 250, 252, 255)
MUTED = (255, 255, 255, 170)
FAINT = (255, 255, 255, 110)
ACCENT = (10, 132, 255)          # azul do sistema, nos botões principais e interruptores ligados
ACCENT_HOVER = (64, 156, 255)
DANGER = (255, 69, 58)


def br(number, decimals=1):
    return f"{number:.{decimals}f}".replace(".", ",")


def ease_in_out(t):
    return 4 * t ** 3 if t < 0.5 else 1 - (-2 * t + 2) ** 3 / 2


@lru_cache(maxsize=None)
def font(name, size):
    return ImageFont.truetype(str(FONTS / name), size)


@lru_cache(maxsize=None)
def icon_font(size):
    """Fonte de ícones do Windows (Segoe Fluent Icons no 11, Segoe MDL2 Assets no 10)."""
    for name in ("SegoeIcons.ttf", "segmdl2.ttf"):
        if (FONTS / name).exists():
            return ImageFont.truetype(str(FONTS / name), size)
    return None


# códigos dos ícones nas fontes de ícones do Windows
ICONS = {
    "geral": "", "aparencia": "", "widgets": "", "clima": "", "midia": "",
    "github": "", "pomodoro": "", "avisos": "", "atalhos": "", "sobre": "",
    "add": "", "check": "", "close": "", "edit": "", "folder": "",
    "refresh": "", "search": "", "undo": "", "pin": "", "location": "",
}


def rounded_mask(w, h, radius):
    """Máscara de retângulo arredondado com bordas suaves."""
    big = Image.new("L", (w * 2, h * 2), 0)
    ImageDraw.Draw(big).rounded_rectangle((0, 0, w * 2 - 1, h * 2 - 1), radius=radius * 2, fill=255)
    return big.resize((w, h), Image.LANCZOS)


# ---------- base dos widgets ----------

SIZES = ("p", "m", "g")
SIZE_NAMES = {"p": "Pequeno", "m": "Médio", "g": "Grande"}
HOT_FILL = (255, 255, 255, 42)    # destaque do que está sob o mouse
PRESS_FILL = (0, 0, 0, 80)        # o que está sendo apertado "afunda"
STALE_DOT = (255, 184, 0, 255)    # aviso de dados desatualizados (sem conexão)
STALE_XY = (12, 12)               # posição do aviso, em pixels lógicos do cartão
HOVER_EXPAND_MS = 650             # mouse parado em cima de um widget pequeno: ele abre
HOVER_COLLAPSE_MS = 350           # e fecha de novo pouco depois de o mouse sair
REORDER_MS = 180


@lru_cache(maxsize=32)
def feedback_shape(w, h, radius, fill):
    """Forma arredondada suave usada no destaque de hover e de clique."""
    k = 3
    big = Image.new("RGBA", (w * k, h * k), (0, 0, 0, 0))
    ImageDraw.Draw(big).rounded_rectangle((0, 0, w * k - 1, h * k - 1), radius=radius * k, fill=fill)
    return big.resize((w, h), Image.LANCZOS)
