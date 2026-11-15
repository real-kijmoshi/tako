"""Portable settings tests: two hosts, privacy, backups, and failed imports."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

BIN = Path(__file__).resolve().parents[1] / 'target/debug/tako'
SOURCE = '''listen = "0.0.0.0:9000"
api_key = "SOURCE-HOST-SECRET" # SOURCE-PRIVATE-COMMENT
cooldown = 42
default = "daily"
[providers.remote]
type = "openai"
base_url = "https://example.com/v1"
keys = ["PROVIDER-SECRET", "env:TAKO_TRANSFER_UNSET", "env:TAKO_TRANSFER_DEFINED"]
[providers.remote.headers]
X-Custom = "PROVIDER-HEADER-SECRET"
[combos]
daily = ["remote/model"]
[aliases]
"claude-*" = "daily"
'''
DEST = '''listen = "127.0.0.1:23456"
api_key = "DEST-HOST-SECRET"
[providers.previous]
type = "anthropic"
keys = ["previous-provider-secret"]
'''

class SettingsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='tako-transfer-')
        self.root = Path(self.tmp.name)
        self.a, self.b = self.root/'source', self.root/'destination'
        for home, config in [(self.a, SOURCE), (self.b, DEST)]:
            home.mkdir()
            (home/'config.toml').write_text(config)
        (self.b/'auth.json').write_text('{"password":"destination-auth-sentinel"}')
        self.export = self.root/'settings.json'
    def tearDown(self): self.tmp.cleanup()
    def cli(self, home, *args, success=True):
        env = {**os.environ,'TAKO_HOME':str(home),'TAKO_TRANSFER_DEFINED':'RESOLVED-ENV-SECRET'}
        env.pop('TAKO_TRANSFER_UNSET', None)
        result = subprocess.run([str(BIN),*map(str,args)], env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return result
    def do_export(self): self.cli(self.a,'export',self.export)
    def test_round_trip_preserves_security_and_backs_up_destination(self):
        self.do_export()
        text = self.export.read_text()
        self.assertIn('PROVIDER-SECRET',text)
        self.assertIn('PROVIDER-HEADER-SECRET',text)
        self.assertIn('env:TAKO_TRANSFER_UNSET',text)
        self.assertIn('env:TAKO_TRANSFER_DEFINED',text)
        self.assertNotIn('RESOLVED-ENV-SECRET',text)
        self.assertNotIn('SOURCE-HOST-SECRET',text)
        self.assertNotIn('SOURCE-PRIVATE-COMMENT',text)
        self.assertNotIn('0.0.0.0:9000',text)
        self.assertEqual(self.export.stat().st_mode & 0o777,0o600)
        credentials=(self.b/'auth.json').read_bytes()
        self.cli(self.b,'import',self.export)
        config=tomllib.loads((self.b/'config.toml').read_text())
        self.assertEqual(config['listen'],'127.0.0.1:23456')
        self.assertEqual(config['api_key'],'DEST-HOST-SECRET')
        self.assertEqual(config['cooldown'],42)
        self.assertEqual(config['combos']['daily'],['remote/model'])
        self.assertEqual(config['aliases']['claude-*'],'daily')
        self.assertEqual(set(config['providers']),{'remote'})
        self.assertEqual((self.b/'auth.json').read_bytes(),credentials)
        backup, = list(self.b.glob('config.before-import-*.toml'))
        self.assertEqual(backup.read_text(),DEST)
        self.assertEqual(backup.stat().st_mode & 0o777,0o600)
        self.assertEqual((self.b/'config.toml').stat().st_mode & 0o777,0o600)
        self.assertEqual(list(self.b.glob('*.tmp')),[])
        self.cli(self.b,'check')
    def test_rejected_imports_leave_settings_and_credentials_unchanged(self):
        invalid = [
            {'format':'tako-settings','version':2,'settings':''},
            {'format':'tako-settings','version':1,'settings':'listen="0.0.0.0:80"'},
            {'format':'tako-settings','version':1,'settings':'[combos]\nbroken=["missing/model"]'},
            {'format':'tako-settings','version':1,'settings':'[providers.bad]\ntype="openai"\nkeys="INPUT-SECRET"'},
        ]
        for value in invalid:
            self.export.write_text(json.dumps(value))
            result=self.cli(self.b,'import',self.export,success=False)
            self.assertNotIn('INPUT-SECRET',result.stderr)
            self.assertEqual((self.b/'config.toml').read_text(),DEST)
            self.assertEqual(list(self.b.glob('config.before-import-*')),[])
        self.export.write_text('bad JSON with INPUT-SECRET')
        result=self.cli(self.b,'import',self.export,success=False)
        self.assertNotIn('INPUT-SECRET',result.stderr)
        self.export.write_text('x'*(256*1024+1))
        self.cli(self.b,'import',self.export,success=False)
    def test_export_refuses_overwrite_and_preserves_existing_file(self):
        self.export.write_text('keep-me')
        self.cli(self.a,'export',self.export,success=False)
        self.assertEqual(self.export.read_text(),'keep-me')
    def test_import_into_fresh_install(self):
        self.do_export()
        fresh=self.root/'fresh'
        self.cli(fresh,'import',self.export)
        self.cli(fresh,'check')
        self.assertFalse((fresh/'auth.json').exists())
        self.assertFalse((fresh/'tako.pid').exists())

if __name__ == '__main__': unittest.main()
