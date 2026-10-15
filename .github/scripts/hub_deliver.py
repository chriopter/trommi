#!/usr/bin/env python3
"""An empty authenticated webhook, then proof of the public hub's exact commit. Never echo headers/bodies."""
import json
import os
import time
import urllib.error
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None  # Credentials must not follow redirects.


def request(url, headers, method, timeout):
    req = urllib.request.Request(url, headers={**headers, 'User-Agent': 'Trommi-Deploy/1.0'}, method=method)
    try:
        with urllib.request.build_opener(NoRedirect).open(req, timeout=timeout) as r:
            return r.status, r.read(65537)
    except urllib.error.HTTPError as e:
        status = e.code
        e.close()
        return status, b''


def preflight(url, headers):
    status, _ = request(url, headers, 'GET', 15)
    if status == 403:
        raise RuntimeError('Cloudflare Access refused the service-token headers (HTTP 403); webhook was not called')
    if status not in (404, 405):
        raise RuntimeError(f'Access preflight did not reach YoloDeploy (HTTP {status}); webhook was not called')
    print('Cloudflare Access preflight passed; YoloDeploy route reached.', flush=True)


def deploy(url, headers, commit, health='https://hub.trommi.com/healthz', tries=36, pause=time.sleep):
    preflight(url, headers)
    status, body = request(url, headers, 'POST', 300)
    if status == 403:
        raise RuntimeError('POST refused by Cloudflare Access (HTTP 403)')
    if status == 401:
        raise RuntimeError('Access passed but YoloDeploy rejected this app token (HTTP 401)')
    if status != 200:
        raise RuntimeError(f'YoloDeploy did not deploy (HTTP {status}); inspect its recovery journal before retrying')
    if len(body) > 65536:
        raise RuntimeError('YoloDeploy returned an oversized result')
    try:
        answer = json.loads(body)
    except (ValueError, UnicodeError) as e:
        raise RuntimeError('YoloDeploy returned no valid bounded JSON result') from e
    if answer.get('target') != 'trommi' or answer.get('status') != 'deployed':
        raise RuntimeError('YoloDeploy did not confirm the fixed Trommi target')
    for _ in range(tries):
        try:
            status, body = request(health, {}, 'GET', 10)
            answer = json.loads(body)
            if status == 200 and answer.get('ok') is True and answer.get('commit') == commit and type(answer.get('protocol_version')) is int:
                print(f'Public hub healthy at expected commit {commit}.', flush=True)
                return
        except (ValueError, UnicodeError, OSError):
            pass
        pause(5)
    raise RuntimeError('Public hub did not report the expected commit; deployment is not confirmed')


def main():
    if os.environ.get('GITHUB_REF') != 'refs/heads/main':
        raise RuntimeError('Only main may deploy')
    url = os.environ['DEPLOY_WEBHOOK_URL']
    if url != 'https://yolodeploy.app/deploy':
        raise RuntimeError('Unexpected deployment endpoint')
    headers = {name: os.environ[key] for name, key in [
        ('CF-Access-Client-Id', 'CF_ACCESS_CLIENT_ID'),
        ('CF-Access-Client-Secret', 'CF_ACCESS_CLIENT_SECRET'),
        ('Authorization', 'DEPLOY_TOKEN')]}
    if not all(headers.values()):
        raise RuntimeError('A deployment credential is missing')
    headers['Authorization'] = 'Bearer ' + headers['Authorization']
    if '--preflight-only' in __import__('sys').argv:
        preflight(url, headers)
        return
    deploy(url, headers, os.environ['GITHUB_SHA'])


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError) as error:
        # Never print request objects, response bodies or exception URLs.
        print('::error::' + (str(error) if isinstance(error, RuntimeError) else 'Deployment network failure'))
        raise SystemExit(1)
