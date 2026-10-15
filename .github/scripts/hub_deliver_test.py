"""Local HTTP fixtures only: Access, token handling, one empty POST, redirects, exact commit."""
import contextlib
import http.server
import json
import threading
import unittest
import hub_deliver

COMMIT = 'a' * 40

class Tests(unittest.TestCase):
    def fixture(self, access=404, post=200, commit=COMMIT, redirect=False):
        calls = []
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *args): pass
            def do_GET(self):
                self.server.owner.assertEqual(self.headers['User-Agent'], 'Trommi-Deploy/1.0')
                calls.append(('GET', self.path))
                code = 200 if self.path == '/healthz' else (302 if redirect else access)
                self.send_response(code)
                if redirect: self.send_header('Location', '/secret-leak')
                self.end_headers()
                if self.path == '/healthz': self.wfile.write(json.dumps({'ok': True, 'commit': commit, 'protocol_version': 1}).encode())
            def do_POST(self):
                self.server.owner.assertEqual(self.headers['User-Agent'], 'Trommi-Deploy/1.0')
                calls.append(('POST', self.path))
                self.server.owner.assertEqual(self.headers['Authorization'], 'Bearer fixture-token')
                self.server.owner.assertEqual(self.headers['CF-Access-Client-Id'], 'fixture-id')
                self.server.owner.assertEqual(self.headers['CF-Access-Client-Secret'], 'fixture-secret')
                self.server.owner.assertEqual(int(self.headers.get('Content-Length', '0')), 0)
                self.send_response(post); self.end_headers()
                self.wfile.write(b'{"target":"trommi","status":"deployed"}')
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        server.owner = self
        thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
        self.addCleanup(server.server_close); self.addCleanup(server.shutdown)
        base = f'http://127.0.0.1:{server.server_port}'
        def run(): hub_deliver.deploy(base + '/deploy', {'Authorization':'Bearer fixture-token','CF-Access-Client-Id':'fixture-id','CF-Access-Client-Secret':'fixture-secret'}, COMMIT, base + '/healthz', tries=2, pause=lambda _: None)
        return run, calls

    def test_success_is_one_empty_post_and_public_exact_commit(self):
        run, calls = self.fixture(); run()
        self.assertEqual(calls, [('GET','/deploy'),('POST','/deploy'),('GET','/healthz')])

    def test_access_403_never_calls_deploy(self):
        run, calls = self.fixture(access=403)
        with self.assertRaisesRegex(RuntimeError, 'Access'): run()
        self.assertEqual(calls, [('GET','/deploy')])

    def test_bad_app_token_is_separate_from_access(self):
        run, calls = self.fixture(post=401)
        with self.assertRaisesRegex(RuntimeError, 'app token'): run()
        self.assertEqual(sum(method == 'POST' for method, _ in calls), 1)

    def test_failed_deploy_is_not_retried(self):
        run, calls = self.fixture(post=502)
        with self.assertRaisesRegex(RuntimeError, 'recovery journal'): run()
        self.assertEqual(sum(method == 'POST' for method, _ in calls), 1)

    def test_old_public_commit_is_not_a_success(self):
        run, _ = self.fixture(commit='b' * 40)
        with self.assertRaisesRegex(RuntimeError, 'expected commit'): run()

    def test_headers_never_follow_redirect(self):
        run, calls = self.fixture(redirect=True)
        with self.assertRaisesRegex(RuntimeError, 'preflight'): run()
        self.assertEqual(calls, [('GET','/deploy')])

if __name__ == '__main__': unittest.main()
