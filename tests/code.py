"""PC worker, Git integration and hosted free-only gateway regression checks."""
import importlib.util
import json
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
        for path in ['../outside', str(self.folder / 'absolute'), '.env', '.env.staging', '.git/config', 'cert.pem']:
            with self.assertRaises(code.CodeError): worker.execute('write_file', {'path': path, 'content': 'x'})
        (self.repo / 'link').symlink_to(self.folder)
        with self.assertRaises(code.CodeError): worker.execute('read_file', {'path': 'link/outside'})
        with self.assertRaisesRegex(code.CodeError, 'Read this file'): worker.execute('write_file', {'path': 'a.txt', 'content': 'x'})
        worker.execute('read_file', {'path': 'a.txt'})
        worker.execute('write_file', {'path': 'a.txt', 'content': 'allowed'})
        self.assertEqual((self.repo / 'a.txt').read_text(), 'allowed')
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
    def log_message(self, *args): pass
    def do_POST(self):
        data = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.seen.append(data['model'])
        if data.get('stream') and not self.fail:
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
        self.request('/api/action', {'action': 'provider.remove', 'provider': 'kilo'})
        Provider.seen.clear()
        self.assertEqual(self.request('/code/v1/chat/completions', body)[0], 503)
        self.assertFalse(Provider.seen, 'No upstream request when only paid routes remain')

if __name__ == '__main__': unittest.main()
