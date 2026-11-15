"""Security integration checks: cargo build && python3 tests/auth.py."""
import hashlib
import json
import os
import pty
import select
import socket
import subprocess
import tempfile
import termios
import time
import unittest
import threading
from http.server import ThreadingHTTPServer
from dashboard import Provider
from pathlib import Path
from urllib.request import Request, urlopen
from urllib.error import HTTPError

BIN = Path(__file__).resolve().parents[1] / 'target/debug/tako'
PASSWORD = 'test-admin-password-123'


def password_setup(env):
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(str(BIN), [str(BIN), 'password'], env)
    output = b''
    prompts = 0
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        ready, _, _ = select.select([fd], [], [], .1)
        if ready:
            try:
                output += os.read(fd, 4096)
            except OSError:
                break
            if prompts == 0 and b'characters):' in output:
                while termios.tcgetattr(fd)[3] & termios.ECHO: time.sleep(.01)
                os.write(fd, (PASSWORD + '\n').encode()); prompts = 1
            elif prompts == 1 and b'Confirm password:' in output:
                while termios.tcgetattr(fd)[3] & termios.ECHO: time.sleep(.01)
                os.write(fd, (PASSWORD + '\n').encode()); prompts = 2
    os.close(fd)
    _, status = os.waitpid(pid, 0)
    assert os.waitstatus_to_exitcode(status) == 0, output.decode()
    assert PASSWORD.encode() not in output, 'Password echoed to terminal'


class AuthTest(unittest.TestCase):
    def test_security(self):
        with tempfile.TemporaryDirectory() as home:
            env = {**os.environ, 'TAKO_HOME': home}
            password_setup(env)
            credential_path = Path(home) / 'auth.json'
            self.assertEqual(credential_path.stat().st_mode & 0o777, 0o600)
            self.assertNotIn(PASSWORD, credential_path.read_text())
            with socket.socket() as sock:
                sock.bind(('127.0.0.1', 0)); port = sock.getsockname()[1]
            (Path(home) / 'config.toml').write_text(f'listen = "127.0.0.1:{port}"\n')
            proc = subprocess.Popen([str(BIN), 'run'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            base = f'http://127.0.0.1:{port}'
            def request(path, value=None, cookie=None, key=None, origin=None, host=None):
                headers = {'x-tako': '1', 'Content-Type': 'application/json'}
                if cookie: headers['Cookie'] = cookie
                if key: headers['Authorization'] = 'Bearer ' + key
                if origin: headers['Origin'] = origin
                if host: headers['Host'] = host
                try:
                    response = urlopen(Request(base + path, data=None if value is None else json.dumps(value).encode(), headers=headers), timeout=5)
                except HTTPError as error:
                    response = error
                with response:
                    body = response.read().decode()
                    try: body = json.loads(body)
                    except ValueError: pass
                    return response.code, body, response.headers
            try:
                for _ in range(100):
                    try: request('/health'); break
                    except OSError: time.sleep(.05)
                self.assertIn('Admin password', request('/ui')[1])
                self.assertEqual(request('/api/overview')[0], 401)
                self.assertEqual(request('/status')[0], 401)
                self.assertEqual(request('/v1/models')[0], 401)
                self.assertEqual(request('/api/login', {'password': PASSWORD}, origin='https://evil.example')[0], 403)
                self.assertEqual(request('/api/login', {'password': 'wrong'})[0], 401)
                code, _, headers = request('/api/login', {'password': PASSWORD}, origin='https://router.example', host='router.example')
                self.assertEqual(code, 200)
                self.assertIn('Secure', headers['Set-Cookie'])
                cookie = headers['Set-Cookie'].split(';')[0]
                self.assertEqual(request('/api/overview', cookie=cookie, host='router.example')[0], 200)
                self.assertEqual(request('/status', cookie=cookie)[0], 200)
                code, body, _ = request('/api/action', {'action': 'key.create', 'name': 'Laptop'}, cookie=cookie)
                self.assertEqual(code, 200)
                key = body['key']
                self.assertNotIn(key, credential_path.read_text())
                self.assertIn(hashlib.sha256(key.encode()).hexdigest(), credential_path.read_text())
                self.assertEqual(request('/v1/models', key=key)[0], 200)
                self.assertEqual(request('/api/overview', key=key)[0], 401)
                for operation in ['settings.export','settings.preview','settings.import']:
                    self.assertEqual(request('/api/action', {'action':operation}, key=key)[0],401)
                code, exported, _ = request('/api/action', {'action':'settings.export'}, cookie=cookie)
                self.assertEqual(code,200)
                self.assertEqual(exported['bundle']['format'],'tako-settings')
                self.assertNotIn(key,json.dumps(exported))
                self.assertNotIn('password',exported['bundle']['settings'])
                self.assertEqual(request('/api/action', {'action':'settings.export'}, cookie=cookie, origin='https://evil.example')[0],403)
                upstream = ThreadingHTTPServer(('127.0.0.1', 0), Provider)
                threading.Thread(target=upstream.serve_forever, daemon=True).start()
                try:
                    self.assertEqual(request('/api/action', {'action': 'provider.add', 'provider': 'mock', 'base_url': f'http://127.0.0.1:{upstream.server_port}/v1'}, cookie=cookie)[0], 200)
                    code, reply, _ = request('/api/action', {'action': 'chat', 'model': 'mock/second', 'messages': [{'role': 'user', 'content': 'hello'}]}, cookie=cookie)
                    self.assertEqual(code, 200)
                    self.assertEqual(reply['reply'], 'Hello from fallback')
                    status = subprocess.run([str(BIN), 'status'], env=env, capture_output=True, text=True, timeout=5)
                    self.assertIn('tako is running', status.stdout)
                finally:
                    upstream.shutdown(); upstream.server_close()
                keys = request('/api/security', cookie=cookie)[1]['keys']
                self.assertNotIn(key, json.dumps(keys))
                request('/api/action', {'action': 'key.revoke', 'id': keys[0]['id']}, cookie=cookie)
                self.assertEqual(request('/v1/models', key=key)[0], 401)
                request('/api/logout', {}, cookie=cookie)
                self.assertEqual(request('/api/security', cookie=cookie)[0], 401)
                _, _, headers = request('/api/login', {'password': PASSWORD})
                cookie = headers['Set-Cookie'].split(';')[0]
                password_setup(env)
                self.assertEqual(request('/api/security', cookie=cookie)[0], 401)
                for _ in range(5): request('/api/login', {'password': 'wrong'})
                self.assertIn('Too many', request('/api/login', {'password': PASSWORD})[1]['error']['message'])
            finally:
                proc.terminate(); proc.wait(timeout=5)
            credential_path.unlink()
            (Path(home) / 'config.toml').write_text('listen = "0.0.0.0:20129"\n')
            result = subprocess.run([str(BIN), 'run'], env=env, capture_output=True, timeout=5)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'Public listeners require', result.stderr)


if __name__ == '__main__':
    unittest.main()
