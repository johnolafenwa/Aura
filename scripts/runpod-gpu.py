#!/usr/bin/env python3
"""Rent a RunPod NVIDIA pod, run commands on it over HTTPS, and always terminate it.

Cloud sessions have no SSH, so the pod starts a small command agent (AGENT_SOURCE
below) on an HTTP port. RunPod publishes that port at
https://<pod id>-<port>.proxy.runpod.net, and every request must carry a random
token made for that pod.

Authentication to the RunPod API: set RUNPOD_API_KEY, or leave it unset where an
egress proxy attaches the Authorization header for rest.runpod.io.

    scripts/runpod-gpu.py check                      # API reachable and authorized?
    scripts/runpod-gpu.py run --ref my-branch -- 'cargo test -p aura-device'
    scripts/runpod-gpu.py up                         # prints the pod id
    scripts/runpod-gpu.py exec POD -- nvidia-smi
    scripts/runpod-gpu.py put POD local.patch /workspace/local.patch
    scripts/runpod-gpu.py get POD /workspace/log.txt log.txt
    scripts/runpod-gpu.py down POD
    scripts/runpod-gpu.py reap                       # terminate every aura-gpu pod

A pod costs money from `up` until `down`. `run` terminates the pod in a finally
block, and the agent asks RunPod to remove its own pod after --max-minutes.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import secrets
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

API = 'https://rest.runpod.io/v1'
NAME_PREFIX = 'aura-gpu'
AGENT_PORT = 8000
DEFAULT_IMAGE = 'nvidia/cuda:12.4.1-devel-ubuntu22.04'
DEFAULT_GPUS = ['NVIDIA RTX A4000', 'NVIDIA RTX 4000 Ada Generation', 'NVIDIA RTX A5000', 'NVIDIA GeForce RTX 3090', 'NVIDIA L4']
REPOSITORY = 'https://github.com/johnolafenwa/Aura.git'
STATE_DIR = Path(os.environ.get('AURA_RUNPOD_STATE_DIR', Path.home() / '.cache' / 'aura' / 'runpod'))

AGENT_SOURCE = r'''
import hmac, json, os, subprocess, threading, time, urllib.parse, urllib.request, uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TOKEN = os.environ['AURA_AGENT_TOKEN']
ROOT = os.environ.get('AURA_AGENT_DIR', '/tmp/aura-agent')
os.makedirs(ROOT, exist_ok=True)
JOBS = {}
CHUNK = 256 * 1024


def remove_own_pod():
    time.sleep(float(os.environ.get('AURA_AGENT_MAX_SECONDS', '0')))
    pod, key = os.environ.get('RUNPOD_POD_ID'), os.environ.get('RUNPOD_API_KEY')
    if not pod:
        return
    try:
        subprocess.run(['runpodctl', 'remove', 'pod', pod], timeout=60)
    except Exception:
        pass
    if key:
        request = urllib.request.Request('https://rest.runpod.io/v1/pods/' + pod, method='DELETE', headers={'Authorization': 'Bearer ' + key})
        try:
            urllib.request.urlopen(request, timeout=60)
        except Exception:
            pass


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, status, body, kind='application/json'):
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(status)
        self.send_header('Content-Type', kind)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def route(self):
        if not hmac.compare_digest(self.headers.get('X-Aura-Token', ''), TOKEN):
            return self.reply(403, {'error': 'bad token'})
        url = urllib.parse.urlparse(self.path)
        query = dict(urllib.parse.parse_qsl(url.query))
        body = self.rfile.read(int(self.headers.get('Content-Length') or 0))
        parts = [part for part in url.path.split('/') if part]
        if self.command == 'GET' and parts == ['health']:
            return self.reply(200, {'ok': True})
        if self.command == 'POST' and parts == ['jobs']:
            spec = json.loads(body)
            job = uuid.uuid4().hex
            log = os.path.join(ROOT, job + '.log')
            env = dict(os.environ, **spec.get('env', {}))
            with open(log, 'wb') as out:
                process = subprocess.Popen(['bash', '-lc', spec['cmd']], cwd=spec.get('cwd') or None, env=env, stdin=subprocess.DEVNULL, stdout=out, stderr=subprocess.STDOUT, start_new_session=True)
            JOBS[job] = (process, log)
            return self.reply(201, {'id': job})
        if self.command == 'GET' and len(parts) == 2 and parts[0] == 'jobs' and parts[1] in JOBS:
            process, log = JOBS[parts[1]]
            code = process.poll()
            with open(log, 'rb') as source:
                source.seek(int(query.get('offset', 0)))
                data = source.read(CHUNK)
            done = code is not None and len(data) < CHUNK
            return self.reply(200, {'output': data.decode('utf-8', 'replace'), 'next_offset': int(query.get('offset', 0)) + len(data), 'done': done, 'exit_code': code})
        if self.command == 'DELETE' and len(parts) == 2 and parts[0] == 'jobs' and parts[1] in JOBS:
            process = JOBS[parts[1]][0]
            if process.poll() is None:
                os.killpg(process.pid, 9)
            return self.reply(200, {'ok': True})
        if self.command == 'PUT' and parts == ['files']:
            path = query['path']
            os.makedirs(os.path.dirname(path) or '.', exist_ok=True)
            with open(path, 'ab' if query.get('append') == '1' else 'wb') as target:
                target.write(body)
            if 'mode' in query:
                os.chmod(path, int(query['mode'], 8))
            return self.reply(200, {'bytes': len(body)})
        if self.command == 'GET' and parts == ['files']:
            with open(query['path'], 'rb') as source:
                source.seek(int(query.get('offset', 0)))
                return self.reply(200, source.read(int(query.get('length', CHUNK))), 'application/octet-stream')
        return self.reply(404, {'error': 'no such route'})

    def handle_request_safely(self):
        try:
            self.route()
        except Exception as error:
            self.reply(500, {'error': repr(error)})

    do_GET = do_POST = do_PUT = do_DELETE = handle_request_safely


if __name__ == '__main__':
    if float(os.environ.get('AURA_AGENT_MAX_SECONDS', '0')) > 0:
        threading.Thread(target=remove_own_pod, daemon=True).start()
    ThreadingHTTPServer((os.environ.get('AURA_AGENT_HOST', '0.0.0.0'), int(os.environ.get('AURA_AGENT_PORT', '8000'))), Handler).serve_forever()
'''

START_COMMAND = (
    '(command -v python3 >/dev/null || (apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq python3)) '
    '&& printf %s "$AURA_AGENT_B64" | base64 -d > /opt/aura_agent.py && exec python3 /opt/aura_agent.py'
)

# Brings a bare CUDA image to the point where `cargo` can build Aura at the given ref.
BOOTSTRAP = r'''
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq git curl ca-certificates build-essential pkg-config cmake xz-utils libzstd-dev libxml2-dev zlib1g-dev >/dev/null
command -v cargo >/dev/null || curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
grep -q cargo/env ~/.profile 2>/dev/null || echo '. "$HOME/.cargo/env"' >> ~/.profile
. "$HOME/.cargo/env"
mkdir -p /workspace && cd /workspace
[ -d Aura ] || git clone --quiet {repository} Aura
cd Aura && git fetch --quiet origin {ref} && git checkout --quiet FETCH_HEAD
[ ! -x scripts/install-llvm.sh ] || scripts/install-llvm.sh
nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv
git log --oneline -1
'''


class RunpodError(RuntimeError):
    pass


def http(method, url, body=None, headers=None, timeout=90):
    data = body if isinstance(body, (bytes, type(None))) else json.dumps(body).encode()
    # Cloudflare in front of RunPod refuses Python's default user agent (error 1010).
    request = urllib.request.Request(url, data=data, method=method, headers=dict({'User-Agent': 'aura-runpod-gpu/1'}, **(headers or {})))
    if data is not None and not isinstance(body, bytes):
        request.add_header('Content-Type', 'application/json')
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


def api(method, path, body=None):
    headers = {}
    if os.environ.get('RUNPOD_API_KEY'):
        headers['Authorization'] = 'Bearer ' + os.environ['RUNPOD_API_KEY']
    status, data = http(method, API + path, body, headers)
    if status == 401:
        raise RunpodError('RunPod rejected the API key (401). The stored value must be "Bearer <key>".')
    if status >= 400:
        raise RunpodError(f'RunPod {method} {path} failed with {status}: {data.decode("utf-8", "replace")[:500]}')
    return json.loads(data) if data else None


class Agent:
    def __init__(self, base_url, token):
        self.base_url, self.token = base_url.rstrip('/'), token

    def call(self, method, path, body=None, retries=5):
        for attempt in range(retries):
            try:
                status, data = http(method, self.base_url + path, body, {'X-Aura-Token': self.token})
            except (urllib.error.URLError, TimeoutError, ConnectionError) as error:
                status, data = 0, repr(error).encode()
            if status and status < 500:
                break
            time.sleep(min(2 ** attempt, 15))
        if status == 0 or status >= 400:
            raise RunpodError(f'agent {method} {path} failed with {status}: {data.decode("utf-8", "replace")[:300]}')
        return data

    def healthy(self):
        try:
            return json.loads(self.call('GET', '/health', retries=1)).get('ok') is True
        except (RunpodError, ValueError):
            return False

    def run(self, command, cwd=None, out=sys.stdout, poll_seconds=2.0):
        """Run a shell command, stream its output, and return its exit code."""
        job = json.loads(self.call('POST', '/jobs', {'cmd': command, 'cwd': cwd}))['id']
        offset = 0
        try:
            while True:
                state = json.loads(self.call('GET', f'/jobs/{job}?offset={offset}'))
                out.write(state['output'])
                out.flush()
                offset = state['next_offset']
                if state['done']:
                    return state['exit_code']
                if not state['output']:
                    time.sleep(poll_seconds)
        except KeyboardInterrupt:
            self.call('DELETE', f'/jobs/{job}')
            raise

    def put(self, local, remote, chunk=8 * 1024 * 1024):
        mode = oct(Path(local).stat().st_mode & 0o777)[2:]
        with open(local, 'rb') as source:
            first = True
            while True:
                data = source.read(chunk)
                if not data and not first:
                    break
                query = urllib.parse.urlencode({'path': remote, 'mode': mode, 'append': '0' if first else '1'})
                self.call('PUT', '/files?' + query, data)
                first = False

    def get(self, remote, local, chunk=8 * 1024 * 1024):
        offset = 0
        with open(local, 'wb') as target:
            while True:
                query = urllib.parse.urlencode({'path': remote, 'offset': offset, 'length': chunk})
                data = self.call('GET', '/files?' + query)
                target.write(data)
                offset += len(data)
                if len(data) < chunk:
                    return offset


def pod_request(name, image, gpus, disk_gb, max_minutes, token, cloud='SECURE'):
    return {
        'name': name,
        'imageName': image,
        'computeType': 'GPU',
        'cloudType': cloud,
        'gpuCount': 1,
        'gpuTypeIds': gpus,
        'gpuTypePriority': 'custom',
        'containerDiskInGb': disk_gb,
        'volumeInGb': 0,
        'ports': [f'{AGENT_PORT}/http'],
        'dockerStartCmd': ['bash', '-c', START_COMMAND],
        'env': {
            'AURA_AGENT_B64': base64.b64encode(AGENT_SOURCE.encode()).decode(),
            'AURA_AGENT_TOKEN': token,
            'AURA_AGENT_PORT': str(AGENT_PORT),
            'AURA_AGENT_MAX_SECONDS': str(max_minutes * 60),
        },
    }


def state_path(pod_id):
    return STATE_DIR / f'{pod_id}.json'


def agent_for(pod_id):
    path = state_path(pod_id)
    if not path.exists():
        raise RunpodError(f'no saved token for pod {pod_id}; it was not started from this machine')
    state = json.loads(path.read_text())
    return Agent(state['url'], state['token'])


def down(pod_id):
    api('DELETE', f'/pods/{pod_id}')
    state_path(pod_id).unlink(missing_ok=True)


def up(args, log=print):
    token = secrets.token_urlsafe(32)
    name = f'{NAME_PREFIX}-{time.strftime("%Y%m%d-%H%M%S", time.gmtime())}'
    pod = api('POST', '/pods', pod_request(name, args.image, args.gpu or DEFAULT_GPUS, args.disk_gb, args.max_minutes, token, args.cloud))
    pod_id = pod['id']
    try:
        STATE_DIR.mkdir(parents=True, exist_ok=True)
        url = f'https://{pod_id}-{AGENT_PORT}.proxy.runpod.net'
        path = state_path(pod_id)
        path.touch(mode=0o600)
        path.write_text(json.dumps({'url': url, 'token': token, 'name': name}))
        log(f'pod {pod_id} created ({pod.get("costPerHr")} credits/hour, {(pod.get("gpu") or {}).get("displayName") or "GPU pending"}); waiting for the agent', file=sys.stderr)
        agent = Agent(url, token)
        deadline = time.time() + args.start_timeout
        while not agent.healthy():
            if time.time() > deadline:
                raise RunpodError(f'the agent on pod {pod_id} did not answer within {args.start_timeout} s')
            time.sleep(10)
        return pod_id, agent
    except BaseException:
        down(pod_id)
        raise


def our_pods():
    return [pod for pod in api('GET', '/pods') or [] if (pod.get('name') or '').startswith(NAME_PREFIX)]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    commands = parser.add_subparsers(dest='command', required=True)

    def pod_options(sub):
        sub.add_argument('--gpu', action='append', help='RunPod GPU type id, tried in the order given (repeatable)')
        sub.add_argument('--image', default=DEFAULT_IMAGE)
        sub.add_argument('--cloud', default='SECURE', choices=['SECURE', 'COMMUNITY'])
        sub.add_argument('--disk-gb', type=int, default=60)
        sub.add_argument('--max-minutes', type=int, default=120, help='the pod removes itself after this long')
        sub.add_argument('--start-timeout', type=int, default=900)

    commands.add_parser('check')
    commands.add_parser('list')
    commands.add_parser('reap')
    pod_options(commands.add_parser('up'))
    run = commands.add_parser('run')
    pod_options(run)
    run.add_argument('--ref', default='main', help='branch, tag or commit of Aura to check out on the pod')
    run.add_argument('--no-bootstrap', action='store_true', help='skip installing the toolchain and cloning Aura')
    run.add_argument('--keep', action='store_true', help='leave the pod running afterwards (it still costs money)')
    run.add_argument('shell', nargs='+', help='command run in /workspace/Aura')
    execute = commands.add_parser('exec')
    execute.add_argument('pod')
    execute.add_argument('--cwd')
    execute.add_argument('shell', nargs='+')
    for name in ('put', 'get'):
        transfer = commands.add_parser(name)
        transfer.add_argument('pod')
        transfer.add_argument('source')
        transfer.add_argument('target')
    commands.add_parser('down').add_argument('pod')
    args = parser.parse_args(argv)

    try:
        if args.command == 'check':
            print(f'RunPod API authorized; {len(api("GET", "/pods") or [])} pod(s) on the account')
        elif args.command == 'list':
            for pod in our_pods():
                print(pod['id'], pod.get('name'), pod.get('desiredStatus'), pod.get('costPerHr'))
        elif args.command == 'reap':
            for pod in our_pods():
                down(pod['id'])
                print('terminated', pod['id'])
        elif args.command == 'up':
            print(up(args)[0])
        elif args.command == 'down':
            down(args.pod)
        elif args.command == 'exec':
            return agent_for(args.pod).run(' '.join(args.shell), args.cwd)
        elif args.command == 'put':
            agent_for(args.pod).put(args.source, args.target)
        elif args.command == 'get':
            agent_for(args.pod).get(args.source, args.target)
        elif args.command == 'run':
            pod_id, agent = up(args)
            try:
                if not args.no_bootstrap:
                    code = agent.run(BOOTSTRAP.format(repository=REPOSITORY, ref=args.ref))
                    if code != 0:
                        print(f'bootstrap failed with exit code {code}', file=sys.stderr)
                        return code
                return agent.run(' '.join(args.shell), None if args.no_bootstrap else '/workspace/Aura')
            finally:
                if args.keep:
                    print(f'pod {pod_id} is still running; stop it with: scripts/runpod-gpu.py down {pod_id}', file=sys.stderr)
                else:
                    down(pod_id)
                    print(f'pod {pod_id} terminated', file=sys.stderr)
    except RunpodError as error:
        print(f'error: {error}', file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main())
