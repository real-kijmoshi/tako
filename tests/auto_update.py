"""Exercise scheduler CLI in an isolated home with a fake service manager."""
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest

BIN = Path(__file__).resolve().parents[1] / 'target/debug/tako'

class SchedulerTest(unittest.TestCase):
    def test_schedule_lifecycle_keeps_real_services_untouched(self):
        with tempfile.TemporaryDirectory(prefix='tako-schedule-') as tmp:
            home = Path(tmp)
            commands = home / 'bin'
            commands.mkdir()
            manager = commands / ('launchctl' if sys.platform == 'darwin' else 'systemctl')
            manager.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$HOME/calls"\nexit 0\n')
            manager.chmod(0o755)
            cfg = home / 'my config & settings'
            env = {**os.environ, 'HOME': str(home), 'XDG_CONFIG_HOME': str(home / '.config'), 'TAKO_HOME': str(cfg), 'PATH': str(commands) + os.pathsep + os.environ['PATH']}
            def run(*args):
                r = subprocess.run([str(BIN), 'auto-update', *args], env=env, capture_output=True, text=True, timeout=10)
                self.assertEqual(r.returncode, 0, r.stderr)
                return r.stdout
            self.assertIn('off', run('status'))
            self.assertIn('on (daily)', run('on'))
            self.assertIn('on (daily)', run('status'))
            if sys.platform == 'darwin':
                schedule = home / 'Library/LaunchAgents/dev.tako.update.plist'
                data = plistlib.loads(schedule.read_bytes())
                self.assertEqual(data['ProgramArguments'], [str(BIN.resolve()), 'update'])
                self.assertEqual(data['EnvironmentVariables']['TAKO_HOME'], str(cfg.resolve()))
                self.assertEqual(data['StartInterval'], 86400)
                self.assertNotIn('RunAtLoad', data)
            else:
                schedule = home / '.config/systemd/user/tako-update.timer'
                self.assertIn('OnCalendar=daily', schedule.read_text())
                self.assertIn(str(cfg.resolve()), (schedule.parent / 'tako-update.service').read_text())
            run('on')  # Re-enabling replaces the schedule instead of duplicating it.
            self.assertTrue(schedule.exists())
            self.assertIn('off', run('off'))
            self.assertFalse(schedule.exists())
            self.assertIn('off', run('status'))
            self.assertIn('off', run('off'))
            self.assertFalse((cfg / 'tako.pid').exists())
            self.assertNotIn(' restart', (home / 'calls').read_text())

    def test_invalid_option_does_not_install_a_schedule(self):
        r = subprocess.run([str(BIN), 'auto-update', 'wat'], capture_output=True, text=True)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn('usage:', r.stderr)

if __name__ == '__main__':
    unittest.main()
