"""Junta os quadros gerados por `cargo test readme -- --ignored` nas animações do README.

    python packaging/readme_anim.py

Lê %TEMP%/widgets-readme/<nome>/frames.txt (arquivo e duração em ms por linha) e grava:
- docs/images/capa.gif: os seis widgets no pequeno, se mexendo (a capa do README)
- docs/images/musica.gif: o Tocando agora trocando de música (GIF, para tocar em qualquer lugar)
- docs/images/ilha.webp: o Tocando agora virando a ilha flutuante e voltando
- docs/images/tamanhos.webp: o Tempo mudando de tamanho
- docs/images/clawd.gif: o Clawd fazendo cada coisa (Sherlock, binóculo, prancheta, Natal...)
- docs/images/clawd-barra.gif: o Clawd em cima da barra de tarefas, com o balão
Precisa do Pillow (pip install pillow).
"""

import tempfile
from pathlib import Path

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parent.parent
FRAMES = Path(tempfile.gettempdir()) / "widgets-readme"


def load(name):
    folder = FRAMES / name
    frames, durations = [], []
    for line in (folder / "frames.txt").read_text().splitlines():
        file, ms = line.split()
        frames.append(Image.open(folder / file).convert("RGB"))
        durations.append(int(ms))
    return frames, durations


def save_delta_gif(frames, durations, path):
    """GIF com uma paleta só e, em cada quadro, transparente o que não mudou desde o anterior.

    Na capa quase tudo fica parado (só a chuva, os anéis, os números e a barra da música
    mexem), então cada quadro guarda só os pixels que mudaram e o arquivo fica pequeno. A
    paleta vem de uma amostra dos quadros; o pontilhado (dither) disfarça os degradês do vidro.
    """
    sample = frames[:: max(1, len(frames) // 8)]
    w, h = frames[0].size
    strip = Image.new("RGB", (w, h * len(sample)))
    for i, f in enumerate(sample):
        strip.paste(f, (0, h * i))
    # 255 cores: a última fica para o transparente
    # (a octree erra um pouco mais na média, mas quase não deixa pontinhos fora de lugar)
    palette = strip.quantize(colors=255, method=Image.Quantize.FASTOCTREE)
    clear = 255
    out = []
    prev_rgb, prev_idx = None, None
    for f in frames:
        idx = f.quantize(palette=palette, dither=Image.Dither.FLOYDSTEINBERG)
        assert idx.getextrema()[1] < clear, "a cor do transparente apareceu num quadro"
        if prev_rgb is None:
            out.append(idx)
        else:
            # mudou de verdade (o pontilhado sozinho não conta): pega os pixels do quadro novo
            changed = ImageChops.difference(f, prev_rgb).convert("L").point(lambda v: 255 if v > 0 else 0)
            # onde mudou, o índice novo; no resto, transparente
            frame = Image.new("P", f.size, clear)
            frame.putpalette(palette.getpalette())
            frame.paste(idx, (0, 0), changed)
            # o que fica na tela continua sendo o índice do quadro anterior
            idx = prev_idx.copy()
            idx.paste(frame, (0, 0), changed)
            out.append(frame)
        prev_rgb, prev_idx = f, idx
    for frame in out:
        frame.info["transparency"] = clear
    out[0].save(path, save_all=True, append_images=out[1:], duration=durations, loop=0, optimize=False, disposal=1, transparency=clear)


def main():
    cover, cover_ms = load("capa")
    save_delta_gif(cover, cover_ms, ROOT / "docs" / "images" / "capa.gif")
    for name, out in [("clawd", "clawd.gif"), ("barra", "clawd-barra.gif")]:
        frames, ms = load(name)
        save_delta_gif(frames, ms, ROOT / "docs" / "images" / out)
    music, music_ms = load("musica")
    # (uma paleta para o GIF todo: com o fundo escuro, uma por quadro deixava o arquivo com o dobro)
    save_delta_gif(music, music_ms, ROOT / "docs" / "images" / "musica.gif")
    island, island_ms = load("ilha")
    island[0].save(ROOT / "docs" / "images" / "ilha.webp", save_all=True, append_images=island[1:], duration=island_ms, loop=0, quality=86, method=6)
    sizes, sizes_ms = load("tamanhos")
    sizes[0].save(ROOT / "docs" / "images" / "tamanhos.webp", save_all=True, append_images=sizes[1:], duration=sizes_ms, loop=0, quality=88, method=6)
    for name in ("capa.gif", "musica.gif", "ilha.webp", "tamanhos.webp"):
        p = ROOT / "docs" / "images" / name
        print(f"{p}  ({p.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()
