#!/usr/bin/env python3
"""Build the portable PC client without third-party dependencies."""
from pathlib import Path
import io
import zipfile

root = Path(__file__).resolve().parents[1]
out = root / 'dist' / 'tako-code.pyz'
out.parent.mkdir(exist_ok=True)
stream = io.BytesIO()
with zipfile.ZipFile(stream, 'w', zipfile.ZIP_DEFLATED) as archive:
    info = zipfile.ZipInfo('__main__.py', (2026, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    archive.writestr(info, (root / 'code/tako_code.py').read_bytes())
out.write_bytes(b'#!/usr/bin/env python3\n' + stream.getvalue())
out.chmod(0o755)
print(out)
