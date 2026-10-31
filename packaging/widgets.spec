# Receita do PyInstaller para o Widgets.exe (modo pasta: abre mais rápido e o antivírus estranha menos).
# Use pelo packaging/build.py, que também gera o ícone e o .zip.
import re
from pathlib import Path

from PyInstaller.utils.hooks import collect_data_files, collect_dynamic_libs, collect_submodules
from PyInstaller.utils.win32.versioninfo import (
    FixedFileInfo, StringFileInfo, StringStruct, StringTable, VarFileInfo, VarStruct, VSVersionInfo,
)

ROOT = Path(SPECPATH).parent
init = (ROOT / "deskwidgets" / "__init__.py").read_text(encoding="utf-8")
VERSION = re.search(r'__version__ = "([^"]+)"', init).group(1)
NUMBERS = tuple(int(n) for n in VERSION.split(".")[:3]) + (0,)

version_info = VSVersionInfo(
    ffi=FixedFileInfo(filevers=NUMBERS, prodvers=NUMBERS),
    kids=[
        StringFileInfo([StringTable("041604B0", [
            StringStruct("CompanyName", "lucasschn1"),
            StringStruct("FileDescription", "Widgets da área de trabalho"),
            StringStruct("FileVersion", VERSION),
            StringStruct("InternalName", "Widgets"),
            StringStruct("OriginalFilename", "Widgets.exe"),
            StringStruct("ProductName", "Widgets"),
            StringStruct("ProductVersion", VERSION),
        ])]),
        VarFileInfo([VarStruct("Translation", [0x0416, 1200])]),
    ],
)

# o winrt (widget "Tocando agora") é importado dentro de uma função: o PyInstaller não o acharia sozinho
hidden = collect_submodules("winrt") + collect_submodules("deskwidgets")
binaries = collect_dynamic_libs("winrt")
datas = collect_data_files("winrt")

a = Analysis(
    [str(ROOT / "widget.pyw")],
    pathex=[str(ROOT)],
    binaries=binaries,
    datas=datas,
    hiddenimports=hidden,
    excludes=["unittest", "pydoc", "doctest", "pdb", "lib2to3", "tkinter.test", "test"],
    noarchive=False,
)
pyz = PYZ(a.pure)
exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="Widgets",
    console=False,          # sem janela de terminal
    icon=str(ROOT / "packaging" / "Widgets.ico"),
    version=version_info,
    upx=False,              # UPX costuma disparar falsos positivos de antivírus
)
coll = COLLECT(exe, a.binaries, a.datas, name="Widgets", upx=False)
