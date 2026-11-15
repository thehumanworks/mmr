#!/usr/bin/env python3
"""Run the Buck2 target against synthetic data only; record warm timing."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('--buck2', required=True)
parser.add_argument('--tool-path', required=True)
parser.add_argument('--evidence', required=True)
args = parser.parse_args()
base = [args.buck2, 'run', '-c', 'experiment.tool_path=' + args.tool_path, '//:mmr', '--']
project = Path(__file__).resolve().parent
evidence = Path(args.evidence).resolve()
evidence.mkdir(parents=True, exist_ok=True)
results = {}
with tempfile.TemporaryDirectory(prefix='mmr-buck2-fixture-') as temporary:
    fixture = Path(temporary)
    sessions = fixture / '.codex/sessions'
    sessions.mkdir(parents=True)
    rows = [
        {'type': 'session_meta', 'timestamp': '2025-01-02T00:00:00', 'payload': {'id': 'buck2-synthetic', 'cwd': '/tmp/buck2-project', 'model_provider': 'openai'}},
        {'type': 'event_msg', 'timestamp': '2025-01-02T00:00:01', 'payload': {'type': 'user_message', 'message': 'synthetic Buck2 source build'}},
        {'type': 'response_item', 'timestamp': '2025-01-02T00:00:02', 'payload': {'role': 'assistant', 'content': [{'type': 'output_text', 'text': 'synthetic response'}]}},
    ]
    (sessions / 'fixture.jsonl').write_text(''.join(json.dumps(row) + '\n' for row in rows))
    environment = os.environ.copy()
    environment['HOME'] = str(fixture)
    environment.pop('XDG_CONFIG_HOME', None)
    environment.pop('CODEX_HOME', None)
    for name, command in [('initial-version', ['--version']), ('warm-version', ['--version']), ('synthetic-read', ['--source', 'codex', 'read', 'session', 'buck2-synthetic'])]:
        start = time.monotonic()
        result = subprocess.run(base + command, cwd=project, env=environment, capture_output=True, text=True)
        results[name] = {'exit_code': result.returncode, 'seconds': time.monotonic() - start}
        (evidence / (name + '.stdout')).write_text(result.stdout)
        (evidence / (name + '.stderr')).write_text(result.stderr)
        if result.returncode:
            raise SystemExit(result.stderr)
        if name == 'synthetic-read':
            data = json.loads(result.stdout)
            assert data['total_messages'] == 2, data
            assert data['messages'][0]['content'] == 'synthetic Buck2 source build', data
        else:
            assert result.stdout.strip() == 'mmr 0.2.0', result.stdout
(evidence / 'verification.json').write_text(json.dumps(results, indent=2) + '\n')
print(json.dumps(results, indent=2))
