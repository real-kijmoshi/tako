"""Dashboard integration checks. Run after `cargo build`: python3 tests/dashboard.py."""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import Request, urlopen
from urllib.error import HTTPError

ROOT = Path(__file__).resolve().parents[1]

class Provider(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, status, value):
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.send_header('x-ratelimit-remaining-tokens', '800')
        self.send_header('x-ratelimit-limit-tokens', '1000')
        self.send_header('x-ratelimit-reset-tokens', '30s')
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self.reply(200, {'data': [{'id': 'first'}, {'id': 'second'}]})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if body.get('stream') and body['messages'][-1]['content'] == 'slow':
            # A reply that takes a while, so the live board can watch it stream.
            time.sleep(.4)
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            for _ in range(8):
                self.wfile.write(('data: ' + json.dumps({'choices': [{'delta': {'content': 'word ' * 20}}]}) + '\n\n').encode())
                self.wfile.flush()
                time.sleep(.2)
            self.wfile.write(b'data: [DONE]\n\n')
            return
        if body.get('stream'):
            events = [
                {'choices': [{'delta': {'content': 'stream reply'}}]},
                {'choices': [], 'usage': {'prompt_tokens': 21, 'completion_tokens': 4}},
            ]
            data = ''.join('data: ' + json.dumps(v) + '\n\n' for v in events) + 'data: [DONE]\n\n'
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(data.encode())))
            self.end_headers()
            for offset in range(0, len(data), 7):
                self.wfile.write(data[offset:offset + 7].encode())
                self.wfile.flush()
            return
        if body['model'] == 'first':
            self.reply(429, {'error': {'message': 'rate limited'}})
        else:
            self.reply(200, {'usage': {'prompt_tokens': 12, 'completion_tokens': 5}, 'id': 'test', 'object': 'chat.completion', 'choices': [
                {'index': 0, 'message': {'role': 'assistant', 'content': 'Hello from fallback'}, 'finish_reason': 'stop'}]})

class Dashboard(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory(prefix='tako-dashboard-')
        cls.home = Path(cls.tmp.name)
        cls.upstream = ThreadingHTTPServer(('127.0.0.1', 0), Provider)
        threading.Thread(target=cls.upstream.serve_forever, daemon=True).start()
        with socket.socket() as s:
            s.bind(('127.0.0.1', 0))
            port = s.getsockname()[1]
        cls.base = f'http://127.0.0.1:{port}'
        (cls.home / 'config.toml').write_text(f'listen = "127.0.0.1:{port}"\napi_key = "router-secret"\n')
        (cls.home / 'tako.log').write_text('key router-secret must be redacted\n')
        cls.proc = subprocess.Popen([str(ROOT / 'target/debug/tako'), 'run'], env={**os.environ, 'TAKO_HOME': str(cls.home)}, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(100):
            try:
                urlopen(cls.base + '/health', timeout=1).close()
                break
            except OSError:
                time.sleep(.05)
        else:
            raise RuntimeError('Router did not start')

    @classmethod
    def tearDownClass(cls):
        cls.proc.terminate()
        cls.proc.wait(timeout=5)
        cls.upstream.shutdown()
        cls.upstream.server_close()
        cls.tmp.cleanup()

    def request(self, path, value=None, headers=None):
        req = Request(self.base + path, data=json.dumps(value).encode() if value is not None else None,
                      headers=headers if headers is not None else {'x-tako': '1', 'Content-Type': 'application/json'})
        try:
            with urlopen(req) as r:
                return r.status, json.load(r)
        except HTTPError as e:
            with e:
                return e.code, json.load(e)

    def test_live_requests_show_while_in_flight(self):
        self.assertEqual(self.request('/api/action', {'action': 'provider.add', 'provider': 'slowmock', 'base_url': f'http://127.0.0.1:{self.upstream.server_port}/v1', 'key': 'slow-secret'})[0], 200)
        req = Request(self.base + '/v1/chat/completions', data=json.dumps({'model': 'slowmock/second', 'stream': True,
            'messages': [{'role': 'user', 'content': 'slow'}]}).encode(),
            headers={'Content-Type': 'application/json', 'Authorization': 'Bearer router-secret'})
        done = threading.Event()
        threading.Thread(target=lambda: (urlopen(req).read(), done.set()), daemon=True).start()
        phases, outs = set(), []
        while not done.wait(.05):
            for f in self.request('/status')[1]['inflight']:
                if f['model'] == 'slowmock/second':
                    phases.add(f['phase'])
                    self.assertEqual(f['target'], 'slowmock/second')
                    self.assertEqual(f['attempt'], 1)
                    outs.append(f.get('out', 0))
        self.assertEqual(phases, {'waiting', 'streaming'})
        self.assertGreater(max(outs), 0, 'output tokens should count up while streaming')
        self.assertEqual(outs, sorted(outs))
        metrics = self.request('/status')[1]
        self.assertEqual(metrics['inflight'], [])
        self.assertGreater(metrics['now'], 0)
        self.assertRegex(metrics['ui'], r'^[0-9a-f]{16}$')
        self.assertEqual(self.request('/status')[1]['ui'], metrics['ui'])
        self.request('/api/action', {'action': 'provider.remove', 'provider': 'slowmock'})

    def test_smart_routes_pick_and_fall_back(self):
        self.assertEqual(self.request('/api/action', {'action': 'provider.add', 'provider': 'routemock', 'base_url': f'http://127.0.0.1:{self.upstream.server_port}/v1', 'key': 'route-secret'})[0], 200)
        self.assertEqual(self.request('/api/action', {'action': 'combo.save', 'name': 'routing', 'targets': ['routemock/first', 'routemock/second']})[0], 200)
        models = [m['id'] for m in self.request('/v1/models', headers={'Authorization': 'Bearer router-secret'})[1]['data']]
        self.assertIn('any:auto', models)
        self.assertIn('any:coding', models)
        overview = self.request('/api/overview')[1]
        pool = next(p for p in overview['routes'] if p['pool'] == 'any')
        self.assertEqual({t['name'] for t in pool['tasks']} >= {'any:auto', 'any:fast'}, True)
        # routemock/first answers 429, so the route falls through to routemock/second
        status, chat = self.request('/api/action', {'action': 'chat', 'model': 'any:auto', 'messages': [{'role': 'user', 'content': 'hello'}]})
        self.assertEqual(status, 200, chat)
        self.assertEqual(chat['reply'], 'Hello from fallback')
        recent = [e for e in self.request('/status')[1]['recent'] if e['target'].startswith('routemock/')]
        self.assertEqual(recent[-1]['model'], 'any:auto → fast')
        self.assertEqual(self.request('/api/action', {'action': 'combo.default', 'name': 'free:auto'})[0], 200)
        self.assertEqual(self.request('/api/overview')[1]['default'], 'free:auto')
        self.assertEqual(self.request('/api/action', {'action': 'combo.default', 'name': 'routing'})[0], 200)
        self.request('/api/action', {'action': 'combo.delete', 'name': 'routing'})
        self.request('/api/action', {'action': 'provider.remove', 'provider': 'routemock'})

    def test_dashboard_workflow(self):
        self.assertEqual(self.request('/api/action', {'action': 'combo.delete', 'name': 'anything'}, {})[0], 403)
        self.assertEqual(self.request('/api/catalog', headers={'Host': 'evil.example'})[0], 403)
        self.assertEqual(self.request('/api/action', {'action': 'provider.add', 'provider': 'mock', 'base_url': f'http://127.0.0.1:{self.upstream.server_port}/v1', 'key': 'provider-secret'})[0], 200)
        self.assertEqual(len(self.request('/api/action', {'action': 'models', 'provider': 'mock'})[1]['models']), 2)
        self.assertEqual(self.request('/api/action', {'action': 'combo.save', 'name': 'test', 'targets': ['missing/model']})[0], 400)
        self.assertEqual(self.request('/api/action', {'action': 'combo.save', 'name': 'test', 'targets': ['mock/first', 'mock/second']})[0], 200)
        self.assertEqual(self.request('/api/action', {'action': 'combo.default', 'name': 'test'})[0], 200)
        status, chat = self.request('/api/action', {'action': 'chat', 'model': 'test', 'messages': [{'role': 'user', 'content': 'hello'}]})
        self.assertEqual(status, 200)
        self.assertEqual(chat['reply'], 'Hello from fallback')
        metrics = self.request('/status')[1]
        self.assertEqual(metrics['requests']['rescued'], 1)
        self.assertTrue(metrics['cooling'])
        self.assertEqual(metrics['tokens']['input_tokens'], 12)
        self.assertEqual(metrics['tokens']['output_tokens'], 5)
        self.assertEqual(metrics['tokens']['total_tokens'], 17)
        self.assertEqual(metrics['tokens']['estimated_requests'], 0)
        self.assertEqual(metrics['token_targets']['mock/second']['total_tokens'], 17)
        self.assertEqual(metrics['recent'][-1]['tokens']['total_tokens'], 17)
        limit = next(q for key, q in metrics['token_limits'].items() if key.startswith('mock/second'))
        self.assertEqual((limit['remaining'], limit['limit'], limit['reset']), (800, 1000, '30s'))
        self.assertNotIn('provider-secret', json.dumps(metrics))
        # Exercise both passthrough streaming and OpenAI -> Anthropic conversion.
        for path in ['/v1/chat/completions', '/v1/messages']:
            req = Request(self.base + path, data=json.dumps({'model': 'mock/second', 'stream': True,
                'max_tokens': 100, 'messages': [{'role': 'user', 'content': 'hello'}]}).encode(),
                headers={'Content-Type': 'application/json', 'Authorization': 'Bearer router-secret'})
            with urlopen(req) as response:
                self.assertIn(b'stream reply', response.read())
        metrics = self.request('/status')[1]
        self.assertEqual(metrics['tokens']['total_tokens'], 67)
        self.assertEqual(metrics['tokens']['input_tokens'], 54)
        self.assertEqual(metrics['tokens']['output_tokens'], 13)
        self.assertEqual(metrics['tokens']['estimated_requests'], 0)
        self.assertGreater(self.request('/api/cooldowns/clear', {})[1]['cleared'], 0)
        self.request('/api/action', {'action': 'provider.key', 'provider': 'mock', 'key': 'rotated-secret'})
        overview = self.request('/api/overview')[1]
        self.assertEqual(overview['providers']['mock']['keys'], 2)
        self.assertNotIn('provider-secret', json.dumps(overview))
        self.assertNotIn('router-secret', json.dumps(self.request('/api/logs')[1]))
        code, exported = self.request('/api/action', {'action':'settings.export'})
        self.assertEqual(code,200)
        bundle=exported['bundle']
        self.assertIn('provider-secret',bundle['settings'])
        self.assertNotIn('router-secret',bundle['settings'])
        self.assertNotIn('listen',bundle['settings'])
        self.assertEqual(self.request('/api/action', {'action':'settings.preview','bundle':bundle})[1]['providers'],1)
        before=(self.home/'config.toml').read_bytes()
        self.assertEqual(self.request('/api/action', {'action':'settings.import','bundle':{'format':'tako-settings','version':1,'settings':'[combos]\ninvalid=["missing/model"]'}})[0],400)
        self.assertEqual((self.home/'config.toml').read_bytes(),before)
        self.assertEqual(self.request('/api/action', {'action': 'provider.remove', 'provider': 'mock'})[0], 200)
        self.assertEqual(self.request('/api/action', {'action':'settings.import','bundle':bundle})[0],200)
        self.assertEqual(self.request('/api/overview')[1]['providers']['mock']['keys'],2)
        self.assertIn('router-secret',(self.home/'config.toml').read_text())
        self.assertEqual(self.request('/api/action', {'action': 'provider.remove', 'provider': 'mock'})[0], 200)
        overview = self.request('/api/overview')[1]
        self.assertFalse(overview['providers'])
        self.assertFalse(overview['combos'])
        self.assertIsNone(overview['default'])

if __name__ == '__main__':
    unittest.main()
