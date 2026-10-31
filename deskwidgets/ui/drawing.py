"""Formas e ícones reaproveitados pelos widgets (anéis, play, pausa...)."""

import math

from deskwidgets.theme import translucent


def draw_ring(d, cx, cy, diameter, width, frac, color):
    """Anel de progresso estilo "anéis de atividade", com pontas arredondadas."""
    rc = diameter / 2 - width / 2  # raio da linha central do anel
    box = (cx - rc - width / 2, cy - rc - width / 2, cx + rc + width / 2, cy + rc + width / 2)
    d.ellipse(box, outline=translucent(color, 0.28), width=round(width))
    if frac <= 0:
        return
    end = -90 + 360 * min(frac, 1)
    d.arc(box, -90, end, fill=color + (255,), width=round(width))
    for angle in (-90, end):
        px = cx + rc * math.cos(math.radians(angle))
        py = cy + rc * math.sin(math.radians(angle))
        d.ellipse((px - width / 2, py - width / 2, px + width / 2, py + width / 2), fill=color + (255,))


def rounded_triangle(d, pts, r, fill):
    """Triângulo com cantos arredondados, no estilo dos ícones da Apple."""
    (ax, ay), (bx, by), (cx, cy) = pts
    a = math.dist((bx, by), (cx, cy))
    b = math.dist((ax, ay), (cx, cy))
    c = math.dist((ax, ay), (bx, by))
    perimeter = a + b + c
    ix = (a * ax + b * bx + c * cx) / perimeter
    iy = (a * ay + b * by + c * cy) / perimeter
    inradius = abs((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)) / perimeter
    # encolhe o triângulo em r a partir do incentro e depois o contorna com traço redondo:
    # as bordas voltam ao lugar original e as pontas ficam arredondadas
    f = max(0.0, (inradius - r) / inradius)
    sp = [(ix + (x - ix) * f, iy + (y - iy) * f) for x, y in pts]
    d.polygon(sp, fill=fill)
    for i in range(3):
        d.line([sp[i], sp[(i + 1) % 3]], fill=fill, width=max(1, round(2 * r)))
    for x, y in sp:
        d.ellipse((x - r, y - r, x + r, y + r), fill=fill)


def draw_play(d, cx, cy, size, color):
    w, h = size * 0.9, size
    # um triângulo centrado pela caixa parece deslocado para a esquerda;
    # empurrar um pouco para a direita o centraliza visualmente
    x0 = cx - w / 2 + w * 0.09
    rounded_triangle(d, [(x0, cy - h / 2), (x0, cy + h / 2), (x0 + w, cy)], size * 0.08, color)


def draw_pause(d, cx, cy, size, color):
    bar, gap, h = size * 0.3, size * 0.26, size * 0.92
    for x in (cx - gap / 2 - bar, cx + gap / 2):
        d.rounded_rectangle((x, cy - h / 2, x + bar, cy + h / 2), radius=bar * 0.35, fill=color)


def draw_skip(d, cx, cy, size, color, forward):
    """Dois triângulos (⏩ / ⏪) centrados em cx."""
    w, h = size * 0.52, size * 0.74
    sign = 1 if forward else -1
    for base in (cx - sign * w, cx):
        rounded_triangle(d, [(base, cy - h / 2), (base, cy + h / 2), (base + sign * w, cy)],
                         size * 0.07, color)


def draw_restart(d, cx, cy, size, color):
    """Seta circular (↺), aberta no topo, com a ponta apontando para a esquerda."""
    r, w = size * 0.4, max(1, round(size * 0.13))
    start, end = -70, 215  # graus do Pillow: a partir das 3 horas, no sentido horário
    d.arc((cx - r, cy - r, cx + r, cy + r), start, end, fill=color, width=w)
    ex, ey = cx + r * math.cos(math.radians(end)), cy + r * math.sin(math.radians(end))
    d.ellipse((ex - w / 2, ey - w / 2, ex + w / 2, ey + w / 2), fill=color)
    a = math.radians(start)
    px, py = cx + r * math.cos(a), cy + r * math.sin(a)
    tx, ty = math.sin(a), -math.cos(a)  # tangente no sentido anti-horário
    nx, ny = math.cos(a), math.sin(a)   # direção radial
    head = size * 0.36
    rounded_triangle(d, [(px + tx * head * 0.7, py + ty * head * 0.7),
                         (px - tx * head * 0.3 + nx * head * 0.55, py - ty * head * 0.3 + ny * head * 0.55),
                         (px - tx * head * 0.3 - nx * head * 0.55, py - ty * head * 0.3 - ny * head * 0.55)],
                     size * 0.05, color)
