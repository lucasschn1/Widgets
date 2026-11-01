"""Junta os quadros gerados por `cargo test readme -- --ignored` nas animações do README.

    python packaging/readme_anim.py

Lê %TEMP%/widgets-readme/<nome>/frames.txt (arquivo e duração em ms por linha) e grava:
- docs/images/musica.gif: o Tocando agora trocando de música (GIF, para tocar em qualquer lugar)
- docs/images/tamanhos.webp: o Tempo mudando de tamanho
Precisa do Pillow (pip install pillow).
"""

import os
import tempfile
from pathlib import Path

from PIL import Image

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


def save_gif(frames, durations, path):
    # cada quadro com a sua paleta: as músicas têm cores muito diferentes entre si, e uma paleta
    # só para todas deixaria os degradês granulados
    quantized = [f.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.FLOYDSTEINBERG) for f in frames]
    quantized[0].save(path, save_all=True, append_images=quantized[1:], duration=durations, loop=0, optimize=True, disposal=1)


def main():
    music, music_ms = load("musica")
    save_gif(music, music_ms, ROOT / "docs" / "images" / "musica.gif")
    sizes, sizes_ms = load("tamanhos")
    sizes[0].save(ROOT / "docs" / "images" / "tamanhos.webp", save_all=True, append_images=sizes[1:], duration=sizes_ms, loop=0, quality=88, method=6)
    for name in ("musica.gif", "tamanhos.webp"):
        p = ROOT / "docs" / "images" / name
        print(f"{p}  ({p.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()
