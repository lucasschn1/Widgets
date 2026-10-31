"""Fundo de vidro: o papel de parede desfocado atrás dos cartões."""

import math
from functools import lru_cache
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageEnhance, ImageFilter, ImageStat

from deskwidgets.paths import TRANSCODED
from deskwidgets.theme import EDGE, SATURATION, SHEEN, TINT, rounded_mask
from deskwidgets.win32 import monitors, registry, user32


def extend_edges(img, pad):
    """Aumenta a imagem repetindo as bordas, para widgets que saem um pouco da tela."""
    w, h = img.size
    out = Image.new(img.mode, (w + 2 * pad, h + 2 * pad))
    out.paste(img, (pad, pad))
    out.paste(img.crop((0, 0, w, 1)).resize((w, pad)), (pad, 0))
    out.paste(img.crop((0, h - 1, w, h)).resize((w, pad)), (pad, pad + h))
    full = h + 2 * pad
    out.paste(out.crop((pad, 0, pad + 1, full)).resize((pad, full)), (0, 0))
    out.paste(out.crop((pad + w - 1, 0, pad + w, full)).resize((pad, full)), (pad + w, 0))
    return out


@lru_cache(maxsize=8)
def glass_finish(w, h, radius, line):
    """Reflexo no topo e borda clara do vidro (camada branca translúcida) e a máscara dos cantos."""
    k = 2
    edge = Image.new("L", (w * k, h * k), 0)
    ImageDraw.Draw(edge).rounded_rectangle((0, 0, w * k - 1, h * k - 1), radius=radius * k,
                                           outline=255, width=line * k)
    edge = edge.resize((w, h), Image.LANCZOS)
    ramp = Image.linear_gradient("L").resize((w, h))  # 0 no topo, 255 na base
    top, bottom = EDGE
    edge = ImageChops.multiply(edge, ramp.point(lambda v: round(255 * (top + (bottom - top) * v / 255))))
    sheen = ramp.point(lambda v: round(255 * SHEEN * max(0.0, 1 - v / 255 / 0.6)))
    shine = Image.new("RGBA", (w, h), (255, 255, 255, 0))
    shine.putalpha(ImageChops.screen(edge, sheen))
    return shine, rounded_mask(w, h, radius)


class Backdrop:
    """Papel de parede desfocado, que serve de fundo de vidro para os widgets.

    Os widgets ficam sempre atrás das outras janelas, então o que há atrás deles é o
    papel de parede: basta mostrar o pedaço dele que fica sob cada cartão. A tela
    inteira é desfocada uma vez só, em escala reduzida; recortar depois é barato.
    """
    DOWN = 4   # redução da imagem desfocada
    PAD = 64   # borda extra, em pixels reduzidos
    DESKTOP, COLORS = r"Control Panel\Desktop", r"Control Panel\Colors"

    def __init__(self, app):
        self.app = app
        self.s = app.s
        self.sig = None
        self.version = 0
        self.check()

    def rebuild(self):
        """Refaz o fundo (depois de mudar o desfoque ou o escurecimento)."""
        self.sig = None
        self.check()

    def _signature(self):
        try:
            mtime = TRANSCODED.stat().st_mtime
        except OSError:
            mtime = None
        return (registry(self.DESKTOP, "WallPaper"), registry(self.DESKTOP, "WallpaperStyle"),
                registry(self.DESKTOP, "TileWallpaper"), registry(self.COLORS, "Background"),
                mtime, monitors())

    def check(self):
        """Refaz o fundo se o papel de parede ou os monitores mudaram; diz se refez."""
        sig = self._signature()
        if sig == self.sig:
            return False
        self.sig = sig
        self._build()
        self.version += 1
        return True

    def _build(self):
        path, style, tile, color, _mtime, screens = self.sig
        if not screens:
            screens = ((0, 0, user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)),)
        left, top = min(r[0] for r in screens), min(r[1] for r in screens)
        right, bottom = max(r[2] for r in screens), max(r[3] for r in screens)
        self.origin = (left, top)
        try:
            bg = tuple(int(v) for v in color.split())[:3]
        except ValueError:
            bg = ()
        D = self.DOWN
        canvas = Image.new("RGB", (math.ceil((right - left) / D), math.ceil((bottom - top) / D)),
                           bg if len(bg) == 3 else (0, 0, 0))
        picture = self._load(path)
        if picture:
            # "Estender" (22) cobre todos os monitores com uma imagem só
            for area in [(left, top, right, bottom)] if style == "22" else screens:
                self._paint(canvas, picture, area, style, tile == "1")
        canvas = ImageEnhance.Color(canvas).enhance(SATURATION)
        self.sharp = extend_edges(canvas, self.PAD)  # sem desfoque: para a prévia nas configurações
        canvas = canvas.filter(ImageFilter.GaussianBlur(self.app.setting("desfoque") * self.s / D))
        self.image = extend_edges(canvas, self.PAD)
        self.gray = self.image.convert("L")  # para medir o quanto o fundo é claro

    @staticmethod
    def _load(path):
        if not path:  # cor sólida, sem imagem
            return None
        for file in (TRANSCODED, Path(path)):  # a cópia do Windows é a imagem que está na tela
            try:
                with Image.open(file) as img:
                    return img.convert("RGB")
            except (OSError, ValueError):
                pass
        return None

    def _paint(self, canvas, picture, area, style, tile):
        """Desenha o papel de parede numa área da tela, como o Windows faz em cada modo."""
        D = self.DOWN
        l, t, r, b = area
        x0, y0 = round((l - self.origin[0]) / D), round((t - self.origin[1]) / D)
        aw, ah = max(1, round((r - l) / D)), max(1, round((b - t) / D))
        pw, ph = picture.width / D, picture.height / D  # tamanho original, já reduzido
        region = canvas.crop((x0, y0, x0 + aw, y0 + ah))
        if tile:  # lado a lado
            piece = picture.resize((max(1, round(pw)), max(1, round(ph))), Image.LANCZOS)
            for ty in range(0, ah, piece.height):
                for tx in range(0, aw, piece.width):
                    region.paste(piece, (tx, ty))
        else:
            if style == "2":  # esticar
                w, h = aw, ah
            elif style in ("6", "10", "22"):  # ajustar / preencher / estender
                scale = (min if style == "6" else max)(aw / pw, ah / ph)
                w, h = pw * scale, ph * scale
            else:  # centralizar, no tamanho original
                w, h = pw, ph
            w, h = max(1, round(w)), max(1, round(h))
            region.paste(picture.resize((w, h), Image.LANCZOS), ((aw - w) // 2, (ah - h) // 2))
        canvas.paste(region, (x0, y0))

    def glass(self, content, radius, x, y):
        """Cartão de vidro sob `content` (RGBA), com o canto superior esquerdo em (x, y) na tela."""
        w, h = content.size
        D, P = self.DOWN, self.PAD
        bw, bh = w / D, h / D
        bx = min(max((x - self.origin[0]) / D + P, 0), self.image.width - bw)
        by = min(max((y - self.origin[1]) / D + P, 0), self.image.height - bh)
        base = self.image.resize((w, h), Image.BILINEAR, box=(bx, by, bx + bw, by + bh))
        # contraste automático: quanto mais claro o fundo, mais o vidro escurece, para o
        # texto branco continuar legível
        light = ImageStat.Stat(self.gray.crop((round(bx), round(by), round(bx + bw), round(by + bh)))).mean[0] / 255
        amount = min(0.85, max(0.12, self.app.setting("escurecimento") + (light - 0.3) * 0.8))
        img = Image.blend(base, Image.new("RGB", (w, h), TINT), amount).convert("RGBA")
        shine, mask = glass_finish(w, h, round(radius, 1), max(1, round(self.s)))
        img.alpha_composite(shine)
        img.alpha_composite(content)
        img.putalpha(mask)
        return img

    def _box(self, x, y, w, h):
        """Caixa (na imagem reduzida) correspondente a um retângulo da tela."""
        D, P = self.DOWN, self.PAD
        bw, bh = w / D, h / D
        bx = min(max((x - self.origin[0]) / D + P, 0), self.image.width - bw)
        by = min(max((y - self.origin[1]) / D + P, 0), self.image.height - bh)
        return bx, by, bx + bw, by + bh

    def region(self, x, y, w, h, sharp=False):
        """O papel de parede (desfocado, ou nítido com `sharp`) sob um retângulo da tela, em RGB."""
        source = self.sharp if sharp else self.image
        return source.resize((max(1, w), max(1, h)), Image.BILINEAR, box=self._box(x, y, w, h))

    def glass_with(self, content, radius, x, y, blur, tint):
        """Como glass(), mas com outro desfoque e escurecimento (prévia ao vivo das configurações)."""
        w, h = content.size
        D = self.DOWN
        box = self._box(x, y, w, h)
        pad = 3 * blur * self.s / D + 2
        outer = (max(0, round(box[0] - pad)), max(0, round(box[1] - pad)),
                 min(self.sharp.width, round(box[2] + pad)), min(self.sharp.height, round(box[3] + pad)))
        piece = self.sharp.crop(outer).filter(ImageFilter.GaussianBlur(blur * self.s / D))
        inner = (box[0] - outer[0], box[1] - outer[1], box[2] - outer[0], box[3] - outer[1])
        base = piece.resize((w, h), Image.BILINEAR, box=inner)
        light = ImageStat.Stat(base.convert("L")).mean[0] / 255
        amount = min(0.85, max(0.12, tint + (light - 0.3) * 0.8))
        img = Image.blend(base, Image.new("RGB", (w, h), TINT), amount).convert("RGBA")
        shine, mask = glass_finish(w, h, round(radius, 1), max(1, round(self.s)))
        img.alpha_composite(shine)
        img.alpha_composite(content)
        img.putalpha(mask)
        return img
