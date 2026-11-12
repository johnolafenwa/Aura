import argparse
import importlib.util
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('runpod_gpu', Path(__file__).with_name('runpod-gpu.py'))
runpod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runpod)


class AgentTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        source = Path(cls.directory.name) / 'agent.py'
        source.write_text(runpod.AGENT_SOURCE)
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0))
            port = probe.getsockname()[1]
        env = dict(os.environ, AURA_AGENT_TOKEN='secret', AURA_AGENT_PORT=str(port), AURA_AGENT_HOST='127.0.0.1', AURA_AGENT_DIR=cls.directory.name)
        cls.process = subprocess.Popen([sys.executable, str(source)], env=env)
        cls.url = f'http://127.0.0.1:{port}'
        cls.agent = runpod.Agent(cls.url, 'secret')
        deadline = time.time() + 20
        with mock.patch.dict(os.environ, {'no_proxy': '127.0.0.1', 'NO_PROXY': '127.0.0.1'}):
            while not cls.agent.healthy():
                if time.time() > deadline:
                    raise RuntimeError('agent did not start')
                time.sleep(0.1)

    @classmethod
    def tearDownClass(cls):
        cls.process.kill()
        cls.process.wait()
        cls.directory.cleanup()

    def setUp(self):
        patch = mock.patch.dict(os.environ, {'no_proxy': '127.0.0.1', 'NO_PROXY': '127.0.0.1'})
        patch.start()
        self.addCleanup(patch.stop)

    def test_runs_a_command_and_reports_output_and_exit_code(self):
        out = io.StringIO()
        code = self.agent.run('echo one; echo two >&2; exit 7', out=out, poll_seconds=0.05)
        self.assertEqual(code, 7)
        self.assertEqual(out.getvalue(), 'one\ntwo\n')

    def test_streams_output_larger_than_one_chunk(self):
        out = io.StringIO()
        code = self.agent.run('head -c 700000 /dev/zero | tr "\\0" x', out=out, poll_seconds=0.05)
        self.assertEqual(code, 0)
        self.assertEqual(len(out.getvalue()), 700000)

    def test_runs_in_the_requested_directory(self):
        out = io.StringIO()
        self.assertEqual(self.agent.run('pwd', cwd=self.directory.name, out=out, poll_seconds=0.05), 0)
        self.assertEqual(Path(out.getvalue().strip()).resolve(), Path(self.directory.name).resolve())

    def test_files_round_trip_in_several_chunks(self):
        local = Path(self.directory.name) / 'local.bin'
        local.write_bytes(bytes(range(256)) * 40)
        local.chmod(0o750)
        remote = Path(self.directory.name) / 'nested' / 'remote.bin'
        self.agent.put(local, str(remote), chunk=1000)
        self.assertEqual(remote.read_bytes(), local.read_bytes())
        self.assertEqual(remote.stat().st_mode & 0o777, 0o750)
        back = Path(self.directory.name) / 'back.bin'
        self.assertEqual(self.agent.get(str(remote), back, chunk=1000), 10240)
        self.assertEqual(back.read_bytes(), local.read_bytes())

    def test_rejects_a_wrong_token(self):
        stranger = runpod.Agent(self.url, 'wrong')
        self.assertFalse(stranger.healthy())
        with self.assertRaisesRegex(runpod.RunpodError, '403'):
            stranger.call('POST', '/jobs', {'cmd': 'true'})


class ClientTests(unittest.TestCase):
    def options(self, **changes):
        values = dict(image='image', gpu=None, cloud='SECURE', disk_gb=60, max_minutes=30, start_timeout=0)
        return argparse.Namespace(**dict(values, **changes))

    def test_pod_request_starts_the_agent_with_a_lifetime(self):
        request = runpod.pod_request('aura-gpu-x', 'image', ['NVIDIA L4'], 60, 30, 'token')
        self.assertEqual(request['ports'], ['8000/http'])
        self.assertEqual(request['gpuTypeIds'], ['NVIDIA L4'])
        self.assertEqual(request['env']['AURA_AGENT_MAX_SECONDS'], '1800')
        self.assertEqual(request['env']['AURA_AGENT_TOKEN'], 'token')
        self.assertIn('aura_agent.py', request['dockerStartCmd'][-1])

    def test_api_explains_a_rejected_key(self):
        with mock.patch.object(runpod, 'http', return_value=(401, b'')):
            with self.assertRaisesRegex(runpod.RunpodError, 'Bearer <key>'):
                runpod.api('GET', '/pods')

    def test_up_terminates_the_pod_when_the_agent_never_answers(self):
        calls = []

        def fake_api(method, path, body=None):
            calls.append((method, path))
            return {'id': 'pod1', 'costPerHr': 0.2}

        with tempfile.TemporaryDirectory() as directory, mock.patch.object(runpod, 'STATE_DIR', Path(directory)), mock.patch.object(runpod, 'api', fake_api), mock.patch.object(runpod.Agent, 'healthy', return_value=False), mock.patch.object(runpod.time, 'sleep'):
            with self.assertRaisesRegex(runpod.RunpodError, 'did not answer'):
                runpod.up(self.options(), log=lambda *a, **k: None)
            self.assertEqual(list(Path(directory).iterdir()), [])
        self.assertEqual(calls, [('POST', '/pods'), ('DELETE', '/pods/pod1')])

    def test_run_terminates_the_pod_even_when_the_command_fails(self):
        agent = mock.Mock()
        agent.run.side_effect = [0, 3]
        with mock.patch.object(runpod, 'up', return_value=('pod1', agent)), mock.patch.object(runpod, 'down') as down, mock.patch('sys.stderr', io.StringIO()):
            self.assertEqual(runpod.main(['run', '--ref', 'branch', '--', 'cargo', 'test']), 3)
        down.assert_called_once_with('pod1')
        self.assertIn('branch', agent.run.call_args_list[0].args[0])
        self.assertEqual(agent.run.call_args_list[1].args, ('cargo test', '/workspace/Aura'))

    def test_run_terminates_the_pod_when_the_command_raises(self):
        agent = mock.Mock()
        agent.run.side_effect = KeyboardInterrupt
        with mock.patch.object(runpod, 'up', return_value=('pod1', agent)), mock.patch.object(runpod, 'down') as down, mock.patch('sys.stderr', io.StringIO()):
            with self.assertRaises(KeyboardInterrupt):
                runpod.main(['run', '--no-bootstrap', '--', 'nvidia-smi'])
        down.assert_called_once_with('pod1')

    def test_reap_only_terminates_pods_this_script_named(self):
        pods = [{'id': 'a', 'name': 'aura-gpu-1'}, {'id': 'b', 'name': 'someone-else'}]
        with mock.patch.object(runpod, 'api', return_value=pods), mock.patch.object(runpod, 'down') as down, mock.patch('builtins.print'):
            self.assertEqual(runpod.main(['reap']), 0)
        down.assert_called_once_with('a')


if __name__ == '__main__':
    unittest.main()
