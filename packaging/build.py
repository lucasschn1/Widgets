"""Gera o Widgets.exe (pasta dist/Widgets) e o dist/Widgets-win64.zip.

    python packaging/build.py

Precisa das dependências de requirements.txt e de packaging/requirements-build.txt.
O instalador (Widgets-Setup.exe) é gerado depois, pelo Inno Setup, com packaging/Widgets.iss.
"""

import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PACKAGING = ROOT / "packaging"
DIST = ROOT / "dist"
sys.path.insert(0, str(ROOT))


def make_icon():
    """Widgets.ico com o mesmo desenho do ícone da bandeja, em vários tamanhos."""
    from deskwidgets.tray import tray_icon_image
    icon = tray_icon_image(256)
    icon.save(PACKAGING / "Widgets.ico", sizes=[(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48),
                                                (64, 64), (128, 128), (256, 256)])


def pyinstaller():
    subprocess.run([sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean",
                    "--distpath", str(DIST), "--workpath", str(ROOT / "build"),
                    str(PACKAGING / "widgets.spec")], check=True, cwd=ROOT)


def make_zip():
    target = DIST / "Widgets-win64.zip"
    target.unlink(missing_ok=True)
    folder = DIST / "Widgets"
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for path in sorted(folder.rglob("*")):
            if path.is_file():
                z.write(path, Path("Widgets") / path.relative_to(folder))
    return target


if __name__ == "__main__":
    shutil.rmtree(DIST / "Widgets", ignore_errors=True)
    make_icon()
    pyinstaller()
    zip_path = make_zip()
    size = sum(p.stat().st_size for p in (DIST / "Widgets").rglob("*") if p.is_file())
    print(f"\nPronto: {DIST / 'Widgets' / 'Widgets.exe'}  ({size / 1e6:.0f} MB)")
    print(f"        {zip_path}  ({zip_path.stat().st_size / 1e6:.0f} MB)")
