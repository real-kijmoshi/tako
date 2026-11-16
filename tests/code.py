"""PC worker, Git integration and hosted free-only gateway regression checks."""
import importlib.util
import json
import hashlib
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import Request, urlopen
from urllib.error import HTTPError

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('tako_code', ROOT / 'code/tako_code.py')
code = importlib.util.module_from_spec(spec)
spec.loader.exec_module(code)

class FakeApi:
    c = {'runner': 'test-pc'}
    def __init__(self):
        self.events = []
    def redact(self, value):
        return value.replace('test-secret', '[redacted]')
    def runner(self, action, **value):
        self.events.append(value)
        return {'ok': True}
    def complete(self, messages, tools=None, reasoning=False):
        if not tools:
            if messages[0]['role'] == 'system':
                return {'content': 'Reviewed actual patch and test results.'}
            return {'content': json.dumps([{'title': 'Update ' + f, 'instructions': 'Edit only ' + f} for f in ['a.txt', 'b.txt']])}
        filename = 'a.txt' if 'Edit only a.txt' in messages[1]['content'] else 'b.txt'
        count = sum(m['role'] == 'tool' for m in messages)
        if count >= 2:
            return {'content': 'Updated ' + filename}
        name = 'read_file' if count == 0 else 'write_file'
        args = {'path': filename}
        if count:
            args['content'] = 'updated ' + filename + '\n'
        return {'content': None, 'tool_calls': [{'id': 'call-' + str(count), 'type': 'function', 'function': {'name': name, 'arguments': json.dumps(args)}}]}

class Client(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='tako-code-test-')
        self.addCleanup(self.tmp.cleanup)
        self.folder = Path(self.tmp.name)
        self.home_patch = patch.object(code, 'HOME', self.folder / 'client')
        self.home_patch.start(); self.addCleanup(self.home_patch.stop)
        self.repo = self.folder / 'repo'; self.repo.mkdir()
        subprocess.run(['git', 'init', '-q', str(self.repo)], check=True)
        for name in ['a.txt', 'b.txt']:
            (self.repo / name).write_text('original\n')
        code.git(self.repo, 'add', '.')
        code.git(self.repo, 'commit', '-qm', 'Initial')
        self.base = code.git(self.repo, 'rev-parse', 'HEAD').stdout.strip()
        self.api = FakeApi()
        self.job = {'id': '0123456789abcdef', 'engine': 'builtin', 'workers_requested': 2, 'task': 'Update both files'}
        self.project = {'path': str(self.repo), 'test_commands': [[sys.executable, '-c', 'from pathlib import Path; assert Path("a.txt").read_text()=="updated a.txt\\n"; assert Path("b.txt").read_text()=="updated b.txt\\n"']]}
    def swarm(self):
        swarm = code.Swarm(self.api, self.job, self.project, threading.Event())
        swarm.run()
        return swarm
    def test_two_real_workers_integrate_test_and_apply_while_runner_connected(self):
        swarm = self.swarm()
        self.assertEqual(swarm.state['phase'], 'ready', swarm.state.get('error'))
        self.assertEqual(len(swarm.state['workers']), 2)
        self.assertEqual({w['phase'] for w in swarm.state['workers']}, {'done'})
        self.assertEqual(swarm.state['tests'][0]['exit_code'], 0)
        self.assertEqual((self.repo / 'a.txt').read_text(), 'original\n')
        self.assertEqual(code.git(self.repo, 'rev-parse', 'HEAD').stdout.strip(), self.base)
        self.assertIn('+updated b.txt', swarm.state['diff'])
        with patch.object(code, 'config', return_value={}), patch.object(code, 'Api', return_value=self.api), code.runner_lock():
            code.main(['apply', self.job['id']])
        self.assertEqual((self.repo / 'a.txt').read_text(), 'updated a.txt\n')
        self.assertEqual(code.load(swarm.folder / 'state.json')['phase'], 'applied')
    def test_failed_configured_tests_block_apply(self):
        self.project['test_commands'] = [[sys.executable, '-c', 'raise SystemExit(3)']]
        swarm = self.swarm()
        self.assertEqual(swarm.state['phase'], 'needs_attention')
        self.assertEqual(swarm.state['tests'][0]['exit_code'], 3)
        with self.assertRaises(code.CodeError): code.apply(self.job['id'])
    def test_dirty_checkout_and_moved_head_are_preserved(self):
        (self.repo / 'a.txt').write_text('local work\n')
        swarm = self.swarm()
        self.assertEqual(swarm.state['phase'], 'failed')
        self.assertFalse(swarm.state['workers'])
        self.assertEqual((self.repo / 'a.txt').read_text(), 'local work\n')
        code.git(self.repo, 'restore', 'a.txt')
        swarm = self.swarm()
        code.git(self.repo, 'commit', '--allow-empty', '-qm', 'User moved branch')
        with self.assertRaisesRegex(code.CodeError, 'branch moved'): code.apply(self.job['id'])
    def test_file_tools_enforce_boundaries_and_read_before_write(self):
        worker = code.Builtin(self.api, self.repo, threading.Event(), lambda _: None)
        for path in ['../outside', str(self.folder / 'absolute'), '.env', '.env.staging', '.git/config', 'cert.pem',
                     '.GIT/config', '.Git', '.ENV', '.git./config', '.git /config', 'GIT~1/config', 'Cert.PEM', 'a.txt:stream', '.g‌it/config', '.﻿env', '.ｇｉｔ/config']:
            with self.assertRaises(code.CodeError): worker.execute('write_file', {'path': path, 'content': 'x'})
        (self.repo / 'link').symlink_to(self.folder)
        with self.assertRaises(code.CodeError): worker.execute('read_file', {'path': 'link/outside'})
        with self.assertRaisesRegex(code.CodeError, 'Read this file'): worker.execute('write_file', {'path': 'a.txt', 'content': 'x'})
        worker.execute('read_file', {'path': 'a.txt'})
        worker.execute('write_file', {'path': 'a.txt', 'content': 'allowed'})
        self.assertEqual((self.repo / 'a.txt').read_text(), 'allowed')
    def test_activity_turns_agent_json_into_short_status(self):
        claude = json.dumps({'type': 'assistant', 'message': {'content': [{'type': 'tool_use', 'name': 'Edit', 'input': {'file_path': '/tmp/w/worker-1/src/app.py'}}]}})
        codex = json.dumps({'type': 'item.started', 'item': {'type': 'command_execution', 'command': 'npm test'}})
        opencode = json.dumps({'type': 'tool_use', 'part': {'type': 'tool', 'tool': 'write', 'state': {'input': {'filePath': '/x/y/z.py'}}}})
        self.assertEqual(code.activity(claude), 'Edit src/app.py')
        self.assertEqual(code.activity(codex), '$ npm test')
        self.assertEqual(code.activity(opencode), 'write y/z.py')
        self.assertEqual(code.activity('read_file a.txt'), 'read_file a.txt')
        self.assertIsNone(code.activity(json.dumps({'type': 'system', 'subtype': 'init'})))
        self.assertIsNone(code.activity(''))
    def test_untrusted_text_cannot_drive_the_terminal(self):
        evil = 'ok\x1b]52;c;cGF3bmVk\x07\x1b[2J\x9b31m\rdone'
        for text in [code.activity(evil), code.activity(json.dumps({'type': 'item.completed', 'item': {'type': 'agent_message', 'text': evil}})),
                     code.clip(evil, 200), code.first_line(evil)]:
            self.assertFalse(any(ch in text for ch in '\x1b\x07\x9b\r'), repr(text))
        lines = code.board({'task': evil, 'engine': 'claude', 'workers': [{'id': 1, 'title': evil, 'phase': 'failed', 'error': evil}]}, {}, [evil], time.monotonic())
        self.assertTrue(all('\x1b]' not in l and '\x07' not in l and '\x1b[2J' not in l for l in lines), lines)
        (code.HOME / 'jobs' / 'abc').mkdir(parents=True)
        (code.HOME / 'jobs' / 'abc' / 'changes.patch').write_text('+line\x1b]52;c;eA==\x07\n')
        with patch('builtins.print') as shown:
            code.show_diff('abc')
        printed = ''.join(str(c.args[0]) for c in shown.call_args_list)
        self.assertNotIn('\x07', printed)
        self.assertNotIn('\x1b]', printed, 'escape introducer removed; leftover text is inert')
    def test_board_fits_the_terminal(self):
        state = {'task': 'x' * 300, 'engine': 'claude', 'phase': 'working', 'workers': [
            {'id': 1, 'title': 'A very long worker title that goes on and on', 'phase': 'working'},
            {'id': 2, 'title': 'Short', 'phase': 'done'}, {'id': 3, 'title': 'Broken', 'phase': 'failed', 'error': 'e' * 200}]}
        for width in (40, 80, 200):
            lines = code.board(state, {1: 'Edit ' + 'y' * 300}, ['merging'], time.monotonic(), 3, width)
            plain = [__import__('re').sub(r'\x1b\[[0-9;]*m', '', l) for l in lines]
            self.assertTrue(all(len(l) < max(40, width) for l in plain), [len(l) for l in plain])
            self.assertTrue(any('Claude Code × 3' in l for l in plain))
    def test_current_directory_becomes_the_project(self):
        c = {'url': 'https://x', 'key': 'k'}
        with patch.object(code.Path, 'cwd', return_value=self.repo):
            pid = code.pick_project(c)
        self.assertEqual(c['projects'][pid]['path'], str(self.repo.resolve()))
        self.assertEqual(c['last_project'], pid)
        with patch.object(code.Path, 'cwd', return_value=self.folder):
            self.assertEqual(code.pick_project(c), pid, 'outside a repo, reuse the last project')
        code.add_project(c, self.repo, [['npm', 'test']])
        self.assertEqual(c['projects'][pid]['test_commands'], [['npm', 'test']])
        code.add_project(c, self.repo)
        self.assertEqual(c['projects'][pid]['test_commands'], [['npm', 'test']], 'tests survive re-registering')
    def test_new_folders_and_fresh_repos_are_set_up_after_asking(self):
        asked = []
        def yes(q): asked.append(q); return True
        plain = self.folder / 'plain'; plain.mkdir(); (plain / 'app.py').write_text('print(1)\n')
        with patch('builtins.print'):
            self.assertIsNone(code.prepare_repo(plain, lambda q: False), 'declining changes nothing')
            self.assertFalse((plain / '.git').exists())
            c = {'url': 'https://x', 'key': 'k'}
            with patch.object(code.Path, 'cwd', return_value=plain):
                pid = code.pick_project(c, yes)
        self.assertEqual(len(asked), 2, 'asks to git init, then to make the first commit')
        self.assertIn('app.py', asked[1])
        self.assertEqual(c['projects'][pid]['path'], str(plain.resolve()))
        self.assertEqual(code.git(plain, 'ls-files').stdout.split(), ['app.py'])
        self.assertEqual(code.git(plain, 'status', '--porcelain').stdout, '', 'clean, so a swarm can start')
        empty = self.folder / 'empty'; empty.mkdir(); code.git(empty, 'init')  # like `git init .`
        with patch('builtins.print'):
            self.assertEqual(code.prepare_repo(empty, yes), empty.resolve())
        self.assertEqual(code.git(empty, 'rev-list', '--count', 'HEAD').stdout.strip(), '1')
        asked.clear()
        self.assertEqual(code.prepare_repo(self.repo, yes), self.repo.resolve())
        self.assertEqual(asked, [], 'a normal repository needs no questions')
    def test_self_update_installs_only_verified_releases(self):
        good = b'new tako-code build'
        files = {}
        class Releases(BaseHTTPRequestHandler):
            def log_message(self, *a): pass
            def do_HEAD(self): self.do_GET()
            def do_GET(self):
                if self.path == '/latest':
                    self.send_response(302); self.send_header('Location', '/tag/v9.9.9'); self.end_headers(); return
                body = b'release page' if self.path.startswith('/tag/') else files.get(self.path)
                if body is None:
                    self.send_response(404); self.end_headers(); return
                self.send_response(200); self.send_header('Content-Length', str(len(body))); self.end_headers()
                if self.command == 'GET': self.wfile.write(body)
        server = ThreadingHTTPServer(('127.0.0.1', 0), Releases)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        asset = code.asset_name()
        exe = self.folder / 'bin' / ('tako-code.exe' if os.name == 'nt' else 'tako-code'); exe.parent.mkdir(); exe.write_bytes(b'old build')
        try:
            with patch.object(code, 'RELEASES', f'http://127.0.0.1:{server.server_port}'), patch.object(code.sys, 'frozen', True, create=True), \
                 patch.object(code.sys, 'executable', str(exe)):
                files['/download/v9.9.9/' + asset] = good
                files['/download/v9.9.9/SHA256SUMS'] = (hashlib.sha256(b'something else').hexdigest() + '  ' + asset + '\n').encode()
                u = code.Update()
                with self.assertRaises(code.CodeError):
                    u.check(force=True)
                self.assertIsNone(u.install())
                self.assertEqual(exe.read_bytes(), b'old build', 'a mismatched download is never installed')
                files['/download/v9.9.9/SHA256SUMS'] = (hashlib.sha256(good).hexdigest() + '  ' + asset + '\n').encode()
                u = code.Update()
                self.assertEqual(u.check(force=True), 'v9.9.9')
                self.assertEqual(exe.read_bytes(), b'old build', 'nothing changes until the swap')
                self.assertEqual(u.install(), 'v9.9.9')
                self.assertEqual(exe.read_bytes(), good)
                self.assertIsNone(code.Update().check(), 'checks at most every few hours')
                with patch.object(code, 'VERSION', '9.9.9'), patch('builtins.print') as shown:
                    code.announce_update(); code.announce_update()
                self.assertEqual(len(shown.call_args_list), 1, 'says "Updated" once')
        finally:
            server.shutdown(); server.server_close()
    def test_https_finds_trusted_roots_when_python_cannot(self):
        # Release executables carry the build machine's certificate paths, which do not exist here.
        import ssl
        broken = ssl.DefaultVerifyPaths('/nonexistent/cert.pem', '/nonexistent/certs', '', '', '', '')
        with patch.object(code, '_TLS', None), patch.object(ssl, 'get_default_verify_paths', return_value=broken), \
             patch.object(ssl.SSLContext, 'set_default_verify_paths', lambda self: None):
            ctx = code.tls()
        if os.name != 'nt' and any(os.path.isfile(f) for f in code.CA_FILES):
            self.assertGreater(ctx.cert_store_stats()['x509_ca'], 0)
        self.assertEqual(ctx.verify_mode, ssl.CERT_REQUIRED)
        self.assertTrue(ctx.check_hostname)
    def test_agent_cli_errors_are_explained(self):
        r = code.failure_reason
        self.assertIn('API Error: 524', r(json.dumps({'type': 'result', 'subtype': 'success', 'is_error': True, 'result': 'API Error: 524 timeout'})))
        self.assertEqual(r(json.dumps({'type': 'result', 'subtype': 'error_max_turns', 'num_turns': 60, 'is_error': True})), 'hit its 60 turn limit before finishing')
        self.assertEqual(r(json.dumps({'type': 'turn.failed', 'error': {'message': 'stream disconnected'}})), 'stream disconnected')
        self.assertEqual(r(json.dumps({'type': 'error', 'error': {'name': 'APIError', 'data': {'message': 'rate limited'}}})), 'rate limited')
        self.assertEqual(r('Error: Claude Code on Windows requires git-bash'), 'Error: Claude Code on Windows requires git-bash')
        self.assertIsNone(r(json.dumps({'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': 'fixing the error'}]}})))
        self.assertIsNone(r(json.dumps({'type': 'result', 'subtype': 'success', 'is_error': False, 'result': 'done'})))
        # A real (fake) CLI that fails: the worker error carries its words, not just an exit code.
        bin_dir = self.folder / 'bin'; bin_dir.mkdir()
        fake = bin_dir / 'claude'
        fake.write_text('#!/bin/sh\necho \'{"type":"result","subtype":"success","is_error":true,"result":"API Error: 429 rate limited"}\'\nexit 1\n'); fake.chmod(0o755)
        if os.name != 'nt':
            with patch.dict(os.environ, {'PATH': str(bin_dir) + os.pathsep + os.environ['PATH']}), self.assertRaises(code.Transient) as failed:
                code.external('claude', code.Api({'url': 'https://example.com', 'key': 'k'}), self.repo, 'task', threading.Event(), lambda m: None)
            self.assertEqual(str(failed.exception), 'Claude Code stopped: API Error: 429 rate limited')
    def test_one_failing_agent_does_not_sink_the_swarm(self):
        self.job = {**self.job, 'engine': 'claude', 'workers_requested': 4}
        self.project = {**self.project, 'test_commands': []}
        tasks = [{'title': f'Part {n}', 'instructions': f'part {n}'} for n in range(1, 5)]
        calls = {}
        def fake_external(engine, api, root, prompt, cancel, log):
            part = prompt.split('Your assigned task: part ')[1][0]
            calls[part] = calls.get(part, 0) + 1
            if part == '1':
                (root / 'one.txt').write_text('1\n')
            elif part == '2' and calls[part] == 1:
                raise code.Transient('Claude Code stopped: API Error: 529 overloaded')
            elif part == '2':
                (root / 'two.txt').write_text('2\n')
            elif part == '3':
                (root / 'three.txt').write_text('half\n')
                raise code.CodeError('Claude Code stopped: hit its 60 turn limit before finishing')
            else:
                raise code.CodeError('Claude Code stopped: invalid request')
        with patch.object(code.Swarm, 'plan', lambda self: tasks), patch.object(code, 'external', fake_external), \
             patch.object(threading.Event, 'wait', lambda self, timeout=None: self.is_set()):
            swarm = self.swarm()
        self.assertEqual(swarm.state['phase'], 'ready', swarm.state.get('error'))
        phases = {w['id']: w['phase'] for w in swarm.state['workers']}
        self.assertEqual(phases, {1: 'done', 2: 'done', 3: 'partial', 4: 'failed'})
        self.assertEqual(calls['2'], 2, 'a temporary API error is retried once')
        self.assertEqual(calls['4'], 1, 'a real error is not retried')
        self.assertIn('invalid request', [w for w in swarm.state['workers'] if w['id'] == 4][0]['error'])
        diff = (swarm.folder / 'changes.patch').read_text()
        for name in ['one.txt', 'two.txt', 'three.txt']:
            self.assertIn(name, diff)
    def test_job_ids_from_vps_cannot_name_paths(self):
        for bad in ['../../etc', '0123456789ABCDEF', '0123456789abcde/', '']:
            with self.assertRaises(code.CodeError):
                code.Swarm(self.api, {**self.job, 'id': bad}, self.project, threading.Event())
        self.assertFalse((self.folder / 'etc').exists())
    def test_url_credentials_and_cancel(self):
        for url in ['http://example.com', 'https://user:pass@example.com', 'https://example.com?key=secret']:
            with self.assertRaises(code.CodeError): code.validate_url(url)
        self.assertEqual(code.validate_url('https://example.com/v1/'), 'https://example.com')
        api = code.Api({'url': 'https://example.com', 'key': 'secret-key', 'runner_token': 'runner-secret'})
        self.assertEqual(api.redact('secret-key runner-secret'), '[redacted] [redacted]')
        with self.assertRaises(code.CodeError):
            code.SameOriginRedirect().redirect_request(Request('https://example.com/x'), None, 302, '', {}, 'https://other.example/x')
        cancelled = threading.Event(); cancelled.set()
        with self.assertRaises(code.Cancelled): code.Builtin(self.api, self.repo, cancelled, lambda _: None).run('Do work')
    def test_disconnected_events_retry_and_redact(self):
        swarm = code.Swarm(self.api, self.job, self.project, threading.Event())
        with patch.object(self.api, 'runner', side_effect=code.CodeError('offline')):
            swarm.event('test-secret', phase='working')
            self.assertEqual(len(swarm.pending), 1)
        swarm.flush()
        self.assertFalse(swarm.pending)
        self.assertEqual(self.api.events[-1]['event']['message'], '[redacted]')

class Provider(BaseHTTPRequestHandler):
    seen = []
    fail = False
    delay = 0          # seconds before answering, like a slow free model
    json_only = False  # answer with plain JSON even when a stream was requested
    tool = False       # answer with a tool call
    def log_message(self, *args): pass
    def do_POST(self):
        data = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.seen.append(data['model'])
        time.sleep(self.delay)
        if self.tool and not self.fail:
            value = {'id': 'fixture', 'object': 'chat.completion', 'choices': [{'index': 0, 'finish_reason': 'tool_calls', 'message': {'role': 'assistant', 'content': None,
                     'tool_calls': [{'id': 'call_9', 'type': 'function', 'function': {'name': 'write_file', 'arguments': '{"path": "a.py"}'}}]}}]}
            self.send_response(200); self.send_header('Content-Type', 'application/json'); self.end_headers(); self.wfile.write(json.dumps(value).encode()); return
        if data.get('stream') and not self.fail and not self.json_only:
            events = [{'id':'fixture','model':data['model'],'choices':[{'index':0,'delta':{'role':'assistant','content':'Free response'},'finish_reason':None}]}, {'choices':[{'index':0,'delta':{},'finish_reason':'stop'}],'usage':{'prompt_tokens':10,'completion_tokens':2}}]
            body = ''.join('data: '+json.dumps(event)+'\n\n' for event in events)+'data: [DONE]\n\n'
            self.send_response(200); self.send_header('Content-Type','text/event-stream'); self.send_header('Content-Length',str(len(body.encode()))); self.end_headers(); self.wfile.write(body.encode()); return
        value = {'error': {'message': 'Free quota exhausted'}} if self.fail else {'id': 'fixture', 'object': 'chat.completion', 'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': 'Free response'}, 'finish_reason': 'stop'}]}
        self.send_response(429 if self.fail else 200)
        self.send_header('Content-Type', 'application/json'); self.end_headers()
        self.wfile.write(json.dumps(value).encode())

class Gateway(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory(prefix='tako-code-api-')
        cls.home = Path(cls.tmp.name)
        cls.upstream = ThreadingHTTPServer(('127.0.0.1', 0), Provider)
        threading.Thread(target=cls.upstream.serve_forever, daemon=True).start()
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0)); port = sock.getsockname()[1]
        cls.base = f'http://127.0.0.1:{port}'
        (cls.home / 'config.toml').write_text(f'''listen="127.0.0.1:{port}"
api_key="router-secret"
default="paid-route"
[providers.paid]
type="openai"
base_url="http://127.0.0.1:{cls.upstream.server_port}/v1"
[providers.kilo]
type="openai"
base_url="http://127.0.0.1:{cls.upstream.server_port}/v1"
[combos]
paid-route=["paid/billed-model"]
[aliases]
"free:coding"="paid-route"
''')
        cls.proc = subprocess.Popen([str(ROOT / 'target/debug/tako'), 'run'], env={**os.environ, 'TAKO_HOME': str(cls.home)}, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        for _ in range(100):
            try:
                urlopen(cls.base + '/health', timeout=1).close(); break
            except OSError: time.sleep(.05)
        else:
            cls.proc.terminate()
            raise RuntimeError(cls.proc.communicate(timeout=5)[1].decode())
    @classmethod
    def tearDownClass(cls):
        cls.proc.terminate(); cls.proc.communicate(timeout=5)
        cls.upstream.shutdown(); cls.upstream.server_close(); cls.tmp.cleanup()
    def request(self, path, value=None, headers=None):
        if headers is None: headers = {'x-tako': '1', 'Authorization': 'Bearer router-secret', 'Content-Type': 'application/json'}
        request = Request(self.base + path, data=json.dumps(value).encode() if value is not None else None, headers=headers)
        try:
            with urlopen(request, timeout=10) as response: return response.status, json.load(response)
        except HTTPError as e:
            with e: return e.code, json.load(e)
    def test_streaming_keeps_slow_free_models_alive(self):
        # Proxies like Cloudflare drop a request after ~100 s without a byte (HTTP 524).
        for provider in ['paid', 'kilo']:
            self.request('/api/action', {'action': 'provider.add', 'provider': provider, 'base_url': f'http://127.0.0.1:{self.upstream.server_port}/v1', 'key': 'fixture-key'})
        api = code.Api({'url': self.base, 'key': 'router-secret'})
        try:
            Provider.delay, Provider.json_only = 2, True
            started = time.monotonic()
            req = Request(self.base + '/code/v1/chat/completions', data=json.dumps({'model': 'x', 'stream': True, 'messages': [{'role': 'user', 'content': 'hi'}]}).encode(),
                          headers={'Authorization': 'Bearer router-secret', 'Content-Type': 'application/json'})
            with urlopen(req, timeout=10) as stream:
                self.assertEqual(stream.headers['Content-Type'], 'text/event-stream')
                self.assertEqual(stream.readline(), b': keepalive\n')
                self.assertLess(time.monotonic() - started, 1.5, 'answered before the model finished')
                rest = stream.read().decode()
            self.assertIn('Free response', rest)
            self.assertTrue(rest.rstrip().endswith('data: [DONE]'))
            self.assertEqual(api.complete([{'role': 'user', 'content': 'hi'}])['content'], 'Free response')
            Provider.delay, Provider.tool = 0, True
            message = api.complete([{'role': 'user', 'content': 'hi'}], [{'type': 'function', 'function': {'name': 'write_file', 'parameters': {'type': 'object'}}}])
            self.assertEqual(message['tool_calls'][0]['id'], 'call_9')
            self.assertEqual(json.loads(message['tool_calls'][0]['function']['arguments']), {'path': 'a.py'})
            # Claude Code reads Anthropic events; a tool call must arrive as a tool_use block.
            req = Request(self.base + '/code/v1/messages', data=json.dumps({'model': 'claude-x', 'max_tokens': 50, 'stream': True, 'messages': [{'role': 'user', 'content': 'hi'}]}).encode(),
                          headers={'x-api-key': 'router-secret', 'Content-Type': 'application/json', 'anthropic-version': '2023-06-01'})
            with urlopen(req, timeout=10) as stream:
                body = stream.read().decode()
            names = [l[7:] for l in body.splitlines() if l.startswith('event: ')]
            self.assertEqual(names[0], 'message_start'); self.assertEqual(names[-1], 'message_stop')
            self.assertIn('"type":"tool_use"', body.replace(' ', ''))
            self.assertIn('input_json_delta', body)
            # Errors become stream error events; tako-code retries them, then gives up clearly.
            Provider.tool, Provider.fail = False, True
            with patch.object(code.Api, 'RETRY_DELAYS', (0,)), self.assertRaises(code.Transient) as failed:
                api.complete([{'role': 'user', 'content': 'hi'}])
            self.assertIn('Free model failed', str(failed.exception))
        finally:
            Provider.delay, Provider.json_only, Provider.tool, Provider.fail = 0, False, False, False
    def test_auth_runner_ownership_and_strict_free_routing(self):
        for path, body in [('/v1/code/capabilities', None), ('/v1/code/register', {'runner': 'pc'}), ('/code/v1/models', None), ('/code/v1/chat/completions', {'model': 'paid/model'})]:
            self.assertEqual(self.request(path, body, {})[0], 401, path)
        self.assertEqual(self.request('/api/code/action', {'action': 'create'}, {})[0], 403)
        self.assertTrue(self.request('/v1/code/capabilities')[1]['free_gateway'])
        pc = {'runner': 'pc', 'name': 'Local PC', 'projects': [{'id': 'app', 'name': 'App'}], 'engines': ['builtin']}
        pc['runner_token'] = self.request('/v1/code/register', pc)[1]['runner_token']
        self.assertEqual(self.request('/v1/code/register', pc)[0], 200)
        self.assertEqual(self.request('/v1/code/poll', {**pc, 'runner_token': 'bad'})[0], 400)
        job = self.request('/api/code/action', {'action': 'create', 'runner': 'pc', 'project': 'app', 'engine': 'builtin', 'workers': 2, 'task': '<script>literal task</script>'})[1]
        self.assertEqual(self.request('/v1/code/poll', pc)[1]['job']['id'], job['id'])
        self.assertEqual(self.request('/v1/code/poll', pc)[1]['job'], None)
        self.assertEqual(self.request('/v1/code/event', {**pc, 'job': job['id'], 'event': {'phase': 'ready', 'diff_truncated': True}})[0], 200)
        self.assertTrue(self.request('/api/code/action', {'action': 'job', 'id': job['id']})[1]['diff_truncated'])
        self.assertNotIn('secret_hash', json.dumps(self.request('/api/code')[1]))
        with urlopen(self.base + '/downloads/tako-code.py') as download:
            self.assertEqual(download.read(), (ROOT / 'code/tako_code.py').read_bytes())
        # A paid default and an alias named free:coding must not override Code policy.
        for provider in ['paid', 'kilo']:
            self.assertEqual(self.request('/api/action', {'action': 'provider.add', 'provider': provider, 'base_url': f'http://127.0.0.1:{self.upstream.server_port}/v1', 'key': 'fixture-key'})[0], 200)
        self.assertEqual(self.request('/api/action', {'action': 'combo.save', 'name': 'paid-route', 'targets': ['paid/billed-model']})[0], 200)
        self.assertEqual(self.request('/api/action', {'action': 'combo.default', 'name': 'paid-route'})[0], 200)
        body = {'model': 'paid/billed-model', 'messages': [{'role': 'user', 'content': 'hello'}], 'max_tokens': 20}
        for model in ['paid/billed-model', 'free:coding', 'unknown']:
            Provider.seen.clear()
            status, reply = self.request('/code/v1/chat/completions', {**body, 'model': model})
            self.assertEqual(status, 200, reply)
            self.assertTrue(Provider.seen)
            self.assertTrue(all(model.endswith(':free') or model == 'kilo-auto/free' for model in Provider.seen), Provider.seen)
        # Codex speaks the Responses API; it must use the same free-only routing.
        Provider.seen.clear()
        responses = {'model': 'gpt-5-codex', 'instructions': 'Be brief', 'input': [{'type': 'message', 'role': 'user', 'content': [{'type': 'input_text', 'text': 'hi'}]}], 'tools': [{'type': 'web_search'}]}
        status, reply = self.request('/code/v1/responses', responses)
        self.assertEqual(status, 200, reply)
        self.assertEqual(reply['object'], 'response')
        self.assertEqual(reply['output'][0]['content'][0]['text'], 'Free response')
        self.assertTrue(Provider.seen and all(m.endswith(':free') or m == 'kilo-auto/free' for m in Provider.seen), Provider.seen)
        request = Request(self.base + '/code/v1/responses', data=json.dumps({**responses, 'stream': True}).encode(), headers={'Authorization': 'Bearer router-secret', 'Content-Type': 'application/json'})
        with urlopen(request, timeout=10) as stream:
            events = [line[7:] for line in stream.read().decode().splitlines() if line.startswith('event: ')]
        self.assertEqual(events[0], 'response.created')
        self.assertEqual(events[-1], 'response.completed')
        self.assertIn('response.output_text.delta', events)
        self.assertEqual(self.request('/code/v1/responses', {'model': 'x'})[0], 400)
        Provider.fail = True; Provider.seen.clear()
        status, reply = self.request('/code/v1/responses', responses)
        self.assertGreaterEqual(status, 400, reply)
        status, reply = self.request('/code/v1/chat/completions', body)
        self.assertGreaterEqual(status, 400, reply)
        self.assertTrue(all(model.endswith(':free') or model == 'kilo-auto/free' for model in Provider.seen), Provider.seen)
        Provider.fail = False
        # A task typed in the terminal UI becomes a VPS job the web panel can see.
        with tempfile.TemporaryDirectory() as folder, patch.object(code, 'HOME', Path(folder) / 'client'):
            repo = Path(folder) / 'repo'; repo.mkdir()
            subprocess.run(['git', 'init', '-q', str(repo)], check=True)
            (repo / 'a.txt').write_text('a\n'); code.git(repo, 'add', '.'); code.git(repo, 'commit', '-qm', 'init')
            c = {'url': self.base, 'key': 'router-secret', 'name': 'Terminal PC', 'runner': 'term-pc'}
            pid = code.add_project(c, repo)
            runner = code.Runner(c); runner.connect()
            with patch.object(code.Swarm, 'run', lambda swarm: swarm.event('Planned', phase='failed', error='stopped for the test')):
                swarm = runner.submit(pid, 'builtin', 2, 'Typed in the terminal')
                swarm.thread.join(10)
            job = self.request('/api/code/action', {'action': 'job', 'id': swarm.job['id']})[1]
            self.assertEqual((job['task'], job['source'], job['phase']), ('Typed in the terminal', 'terminal', 'failed'))
            self.assertIsNone(runner.tick())
            self.assertIsNone(runner.active, 'a finished swarm frees the runner')
        self.request('/api/action', {'action': 'provider.remove', 'provider': 'kilo'})
        Provider.seen.clear()
        self.assertEqual(self.request('/code/v1/chat/completions', body)[0], 503)
        self.assertFalse(Provider.seen, 'No upstream request when only paid routes remain')

if __name__ == '__main__': unittest.main()
