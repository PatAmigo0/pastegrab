"""Packs python-build-standalone's Windows Python, the wheels and yt-dlp into a few zip files.

On a spinning disk every file costs a seek, and loose .py files cost a compile on every start.
Bytecode in zips avoids both. Run it with a Linux CPython of the same minor version as the
Windows build, since bytecode is only valid for the version that wrote it.

Usage: python3.11 -I pack-python.py PBS_PYTHON_DIR WHEELS_DIR YT_DLP_FILE OUT_DIR
"""

import os
import py_compile
import shutil
import sys
import tempfile
import zipfile
from pathlib import Path

# Standard library packages yt-dlp never imports.
STDLIB_DROP = {
    "ensurepip", "idlelib", "lib2to3", "pydoc_data", "site-packages", "test", "tkinter",
    "turtledemo", "venv", "__pycache__",
}
STDLIB_DROP_FILES = {"turtle.py"}
DLLS_DROP = {"_tkinter.pyd", "tcl86t.dll", "tk86t.dll"}
# Kept as loose files: they load DLLs from their own folder or read data files by path.
KEEP_AS_FILES_DATA = {"certifi"}


def compile_to(src: Path, name: str, out: zipfile.ZipFile | Path) -> None:
    """Compiles one module to sourceless bytecode, stored as NAME.pyc."""
    with tempfile.TemporaryDirectory() as tmp:
        pyc = Path(tmp) / "m.pyc"
        py_compile.compile(
            str(src),
            cfile=str(pyc),
            dfile=name,
            doraise=True,
            invalidation_mode=py_compile.PycInvalidationMode.UNCHECKED_HASH,
        )
        target = name[:-3] + ".pyc"
        if isinstance(out, zipfile.ZipFile):
            out.write(pyc, target)
        else:
            dest = out / target
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(pyc, dest)


def add_tree(root: Path, out: zipfile.ZipFile | Path, skip_top: set[str] = frozenset()) -> int:
    """Adds every file under ROOT: .py as bytecode, data files as they are."""
    count = 0
    for path in sorted(root.rglob("*")):
        rel = path.relative_to(root)
        if path.is_dir() or rel.parts[0] in skip_top or "__pycache__" in rel.parts:
            continue
        if rel.parts[0].endswith(".dist-info") or rel.name in STDLIB_DROP_FILES:
            continue
        name = rel.as_posix()
        if path.suffix == ".py":
            compile_to(path, name, out)
        elif isinstance(out, zipfile.ZipFile):
            out.write(path, name)
        else:
            dest = out / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, dest)
        count += 1
    return count


def main() -> None:
    pbs, wheels, ytdlp, out = (Path(p) for p in sys.argv[1:5])
    py = out / "python"
    if out.exists():
        shutil.rmtree(out)
    py.mkdir(parents=True)

    for f in pbs.iterdir():
        if f.is_file() and f.suffix in {".exe", ".dll"}:
            shutil.copyfile(f, py / f.name)
    (py / "DLLs").mkdir()
    for f in (pbs / "DLLs").iterdir():
        if f.name not in DLLS_DROP:
            shutil.copyfile(f, py / "DLLs" / f.name)

    stdlib = py / "python311.zip"
    with zipfile.ZipFile(stdlib, "w", zipfile.ZIP_DEFLATED) as z:
        n = add_tree(pbs / "Lib", z, STDLIB_DROP)
    print(f"stdlib: {n} files, {stdlib.stat().st_size // 1024} KiB")

    # Packages with binary modules or data read by path stay loose; the rest go into one zip.
    site = py / "site-packages"
    site.mkdir()
    loose, packed = 0, 0
    with zipfile.ZipFile(py / "site.zip", "w", zipfile.ZIP_DEFLATED) as z:
        for top in sorted(wheels.iterdir()):
            if top.name.endswith(".dist-info"):
                continue
            binary = top.is_dir() and any(p.suffix in {".pyd", ".dll"} for p in top.rglob("*"))
            binary = binary or top.suffix == ".pyd" or top.name.endswith(".libs")
            if binary or top.name in KEEP_AS_FILES_DATA:
                if top.is_dir():
                    loose += add_tree(top, site / top.name)
                elif top.suffix == ".py":
                    compile_to(top, top.name, site)
                    loose += 1
                else:
                    shutil.copyfile(top, site / top.name)
                    loose += 1
            elif top.is_dir():
                for path in sorted(top.rglob("*")):
                    if path.is_file() and "__pycache__" not in path.parts:
                        name = path.relative_to(wheels).as_posix()
                        if path.suffix == ".py":
                            compile_to(path, name, z)
                        else:
                            z.write(path, name)
                        packed += 1
            elif top.suffix == ".py":
                compile_to(top, top.name, z)
                packed += 1
    print(f"site-packages: {loose} loose files, {packed} in site.zip")

    with tempfile.TemporaryDirectory() as tmp:
        with zipfile.ZipFile(ytdlp) as release:
            release.extractall(tmp)
        # The release starts from a __main__ at its root; `python -m yt_dlp` needs one in the package.
        entry = Path(tmp) / "yt_dlp" / "__main__.py"
        if not entry.exists():
            entry.write_text("import yt_dlp\n\nyt_dlp.main()\n", encoding="ascii")
        with zipfile.ZipFile(py / "ytdlp.zip", "w", zipfile.ZIP_DEFLATED) as z:
            n = add_tree(Path(tmp), z, {"__main__.py"})
    print(f"yt-dlp: {n} files, {(py / 'ytdlp.zip').stat().st_size // 1024} KiB")

    # The ._pth file is the whole module search path; Python also runs isolated with it.
    (py / "python311._pth").write_text(
        "python311.zip\r\nDLLs\r\nsite-packages\r\nsite.zip\r\nytdlp.zip\r\n", encoding="ascii"
    )
    files = sum(1 for p in out.rglob("*") if p.is_file())
    size = sum(p.stat().st_size for p in out.rglob("*") if p.is_file())
    print(f"total: {files} files, {size // (1024 * 1024)} MiB")


if __name__ == "__main__":
    os.umask(0o022)
    main()
