#!/usr/bin/env python3
"""Build a standalone tako-code executable (no Python needed to run it).

Requires PyInstaller in the building Python. Writes dist/tako-code-<os>-<arch>[.exe]
and prints its path. Executables are built natively; PyInstaller cannot cross-compile.
"""
from pathlib import Path
import importlib.util
import platform
import shutil
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
system = {"darwin": "macos", "linux": "linux", "win32": "windows"}[sys.platform]
machine = platform.machine().lower()
arch = {"arm64": "aarch64", "aarch64": "aarch64", "x86_64": "x86_64", "amd64": "x86_64"}[machine]
name = f"tako-code-{system}-{arch}"
exe = ".exe" if system == "windows" else ""

with tempfile.TemporaryDirectory(prefix="tako-code-build-") as work:
    # certifi is a fallback for HTTPS on systems without a standard certificate bundle.
    extra = ["--hidden-import", "certifi", "--collect-data", "certifi"] if importlib.util.find_spec("certifi") else []
    subprocess.run([sys.executable, "-m", "PyInstaller", "--onefile", "--clean", "--noconfirm", *extra,
                    "--name", name, "--distpath", str(Path(work) / "out"),
                    "--workpath", str(Path(work) / "build"), "--specpath", work,
                    str(root / "code" / "tako_code.py")], check=True)
    out = root / "dist" / (name + exe)
    out.parent.mkdir(exist_ok=True)
    shutil.copy2(Path(work) / "out" / (name + exe), out)
print(out)
