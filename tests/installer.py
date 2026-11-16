"""Exercise the real installer with isolated, deterministic release downloads."""
import hashlib
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class Installer(unittest.TestCase):
    def install(self, mode, script='install.sh', name='tako'):
        with tempfile.TemporaryDirectory(prefix='tako-install-test-') as home:
            home = Path(home)
            commands, dest = home / 'commands', home / 'destination'
            commands.mkdir(); dest.mkdir()
            old = b'#!/bin/sh\necho old-installation\n'
            (dest / name).write_bytes(old)
            payload = b'#!/bin/sh\necho "tako 0.1.0"\n'
            (home / 'payload').write_bytes(payload)
            fake = commands / 'curl'
            fake.write_text(f'#!{sys.executable}\n' + '''import hashlib, os, pathlib, platform, sys
args = sys.argv[1:]
output = pathlib.Path(args[args.index('-o') + 1])
url = args[args.index('-o') - 1]
asset = os.environ['NAME'] + '-' + ('macos' if platform.system() == 'Darwin' else 'linux') + '-' + ('aarch64' if platform.machine() in ('arm64', 'aarch64') else 'x86_64')
payload = pathlib.Path(os.environ['FIXTURE']).read_bytes()
mode = os.environ['MODE']
if url.endswith('SHA256SUMS'):
    line = hashlib.sha256(payload).hexdigest() + '  ' + asset + '\\n'
    output.write_text('' if mode == 'missing' else line * (2 if mode == 'duplicate' else 1))
else:
    output.write_bytes(b'corrupted' if mode == 'corrupt' else payload)
''')
            fake.chmod(0o755)
            result = subprocess.run(['sh', str(ROOT / script)], env={**os.environ, 'NAME': name, 'PATH': str(commands) + os.pathsep + os.environ['PATH'], 'TAKO_INSTALL_DIR': str(dest), 'TAKO_VERSION': 'v0.1.0', 'FIXTURE': str(home / 'payload'), 'MODE': mode}, capture_output=True, text=True, errors="replace", timeout=10)
            self.assertEqual(list(dest.glob('.tako*-install.*')), [])
            if mode == 'valid':
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((dest / name).read_bytes(), payload)
                self.assertIn('tako 0.1.0', result.stdout)
            else:
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual((dest / name).read_bytes(), old)

    def test_verified_install(self): self.install('valid')
    def test_corrupt_download_keeps_current_install(self): self.install('corrupt')
    def test_missing_checksum_keeps_current_install(self): self.install('missing')
    def test_ambiguous_checksum_keeps_current_install(self): self.install('duplicate')

    # Tako Code's installer must give the same guarantees for the tako-code executable.
    def test_code_verified_install(self): self.install('valid', 'install-code.sh', 'tako-code')
    def test_code_corrupt_download_keeps_current_install(self): self.install('corrupt', 'install-code.sh', 'tako-code')
    def test_code_missing_checksum_keeps_current_install(self): self.install('missing', 'install-code.sh', 'tako-code')
    def test_code_ambiguous_checksum_keeps_current_install(self): self.install('duplicate', 'install-code.sh', 'tako-code')


if __name__ == '__main__':
    unittest.main()
