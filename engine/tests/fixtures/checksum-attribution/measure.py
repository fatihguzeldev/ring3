"""Bounded checksum campaign against an already verified release; never builds."""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[4]
SHARED = ROOT / 'engine/tests/fixtures/resident-phases/measure.py'
if sys.flags.optimize or not sys.flags.ignore_environment:
    raise RuntimeError('use python3 -E without -O')
if hashlib.sha256(SHARED.read_bytes()).hexdigest() != '061693a57bf1ee4309c427f17a602e3fc901508870bbabacdbfe87ae8ca188d9':
    raise RuntimeError('reviewed supervisor source drift')
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('resident_phase_supervisor', SHARED)
shared = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shared)
pin, write_json, inspect, bounded = shared.pin, shared.write_json, shared.inspect, shared.bounded
GLOBAL_SECONDS, CHILD_SECONDS = 600, 60
ENGINE_SHA = '4d5e42c9e4adbcb774827f0a6c99687e5c9aed5b4a5ed34c8348bb4b62cf1bf2'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--campaign', required=True, type=Path)
    parser.add_argument('--freeze', required=True, type=Path)
    parser.add_argument('--node', default=shutil.which('node'))
    args = parser.parse_args()
    assert sys.flags.optimize == 0 and sys.flags.ignore_environment, 'use python3 -E without -O'
    assert args.node and Path(args.node).is_file(), 'Node executable missing'
    assert not os.environ.get('NODE_OPTIONS'), 'inherited Node runtime injection'
    freeze = json.loads(args.freeze.read_text())
    assert freeze['status'] == 'source-reviewed'
    sources = freeze['source_pins']
    assert sources and all(pin(ROOT / p) == expected for p, expected in sources.items()), 'source drift'
    assert len(freeze['reviews']) == 3 and all(row['ack'] is True for row in freeze['reviews'])
    artifact = ROOT / 'target/wasm32-unknown-unknown/release/ring3_engine.wasm'
    assert freeze['engine'] == {'bytes': 586988, 'sha256': ENGINE_SHA}
    assert pin(artifact) == freeze['engine'], 'existing release artifact drift'
    prior_path = ROOT / 'target/resident-phase-session/campaign-1/campaign.json'
    assert pin(prior_path) == freeze['prior_campaign'], 'release provenance drift'
    prior = json.loads(prior_path.read_text())
    assert prior['status'] == 'complete' and prior['build']['exit_code'] == 0
    assert prior['engine'] == {'path': str(artifact), **freeze['engine']}
    output = args.campaign.resolve()
    output.mkdir(parents=True, exist_ok=False)
    node = str(Path(args.node).resolve())
    record = {
        'schema_version': 1, 'status': 'running', 'source_pins': sources,
        'freeze': pin(args.freeze), 'engine': {'path': str(artifact), **pin(artifact)},
        'prior_campaign': {'path': str(prior_path), **pin(prior_path)},
        'build_policy': 'reuse prior pinned release; no build command',
        'metadata': {
            'python': sys.version, 'node': inspect([node, '--version']),
            'os': platform.platform(), 'machine': platform.machine(), 'cpu_count': os.cpu_count(),
            'cpu': inspect(['sysctl', '-n', 'machdep.cpu.brand_string']),
            'load_average': list(os.getloadavg()), 'power': inspect(['pmset', '-g', 'batt']),
            'power_mode': inspect(['pmset', '-g', 'custom']),
            'release_build_metadata': prior['metadata'],
            'cache_policy': 'existing caches retained; warm authored kernel, no cold-start claim',
        },
        'children': [], 'limits': {'global_seconds': GLOBAL_SECONDS, 'child_seconds': CHILD_SECONDS},
    }
    write_json(output / 'campaign.json', record)
    started = time.monotonic_ns()
    deadline = time.monotonic() + GLOBAL_SECONDS
    try:
        for index in range(3):
            runner = ROOT / 'engine/tests/fixtures/checksum-attribution/run.mjs'
            command = [node, '--expose-gc', str(runner), str(artifact), str(output / f'process-{index}'), str(index)]
            env = dict(os.environ)
            env['RING3_ENGINE_SHA256'] = ENGINE_SHA
            env['RING3_CHECKSUM_CAMPAIGN'] = str(output / 'campaign.json')
            outcome = bounded(command, output, f'process-{index}', deadline, CHILD_SECONDS, env)
            child = {'process_index': index, 'result_file': f'process-{index}/result.json', 'status': 'incomplete', **outcome}
            record['children'].append(child)
            write_json(output / 'campaign.json', record)
            result_path = output / child['result_file']
            if result_path.exists():
                result = json.loads(result_path.read_text())
                child['result_pin'] = pin(result_path)
                expected_pins = [{'path': p, **v} for p, v in sorted(sources.items())]
                if outcome['exit_code'] == 0 and not outcome['timed_out'] and result['status'] == 'complete':
                    assert result['engine'] == freeze['engine']
                    assert result['source_pins_before'] == result['source_pins_after'] == expected_pins
                    child['status'] = 'complete'
            write_json(output / 'campaign.json', record)
            assert child['status'] == 'complete', f'process {index} incomplete; retain, no adaptive retry'
            assert pin(artifact) == freeze['engine'], 'artifact drift'
            assert all(pin(ROOT / p) == expected for p, expected in sources.items()), 'source drift'
        record['status'] = 'complete'
    except (AssertionError, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        record['status'] = 'incomplete'
        record['failure'] = {'type': type(error).__name__, 'message': str(error)}
    finally:
        record['wall_ns'] = str(time.monotonic_ns() - started)
        record['remaining_seconds'] = max(0, deadline - time.monotonic())
        write_json(output / 'campaign.json', record)
    print(json.dumps({'status': record['status'], 'children': len(record['children']), 'wall_ns': record['wall_ns']}))
    return 0 if record['status'] == 'complete' else 1


if __name__ == '__main__':
    raise SystemExit(main())
