"""One bounded release-build/Node campaign. All evidence paths must be fresh."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[4]
BUILD = ['cargo', 'build', '--locked', '--release', '-p', 'ring3-engine', '--target', 'wasm32-unknown-unknown']
GLOBAL_SECONDS = 1200
CHILD_SECONDS = 180


def pin(path):
    data = Path(path).read_bytes()
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def write_json(path, data):
    temporary = path.with_suffix(path.suffix + '.tmp')
    temporary.write_text(json.dumps(data, indent=2) + '\n')
    temporary.replace(path)


def inspect(command):
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=10, check=False)
        return {'command': command, 'exit_code': result.returncode, 'stdout': result.stdout.strip(), 'stderr': result.stderr.strip()}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {'command': command, 'unavailable': type(error).__name__}


def bounded(command, output, prefix, deadline, cap, env=None):
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        return {'command': command, 'exit_code': None, 'timed_out': True, 'not_started': True, 'ns': '0'}
    seconds = min(cap, remaining)
    cleanup_seconds = min(2, seconds / 2)
    execution_seconds = seconds - cleanup_seconds
    started = time.monotonic_ns()
    with (output / f'{prefix}.stdout.log').open('xb') as stdout, (output / f'{prefix}.stderr.log').open('xb') as stderr:
        process = subprocess.Popen(command, cwd=ROOT, stdout=stdout, stderr=stderr, env=env, start_new_session=True)
        timed_out = False
        try:
            process.wait(timeout=execution_seconds)
        except subprocess.TimeoutExpired:
            timed_out = True
            # Only this supervisor-created process group is terminated.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait(timeout=min(cleanup_seconds, max(0.001, deadline - time.monotonic())))
    return {'command': command, 'exit_code': process.returncode, 'timed_out': timed_out, 'limit_seconds': seconds, 'execution_limit_seconds': execution_seconds, 'cleanup_reserve_seconds': cleanup_seconds, 'ns': str(time.monotonic_ns() - started)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--campaign', required=True, type=Path)
    parser.add_argument('--freeze', required=True, type=Path)
    parser.add_argument('--node', default=shutil.which('node'))
    args = parser.parse_args()
    assert args.node and Path(args.node).is_file(), 'Node executable is required'
    freeze = json.loads(args.freeze.read_text())
    assert freeze['status'] == 'source-reviewed', 'source reviews and census must precede execution'
    sources = freeze['source_pins']
    assert sources and all(pin(ROOT / path) == expected for path, expected in sources.items()), 'frozen source drift'
    assert len(freeze['reviews']) == 3 and all(row['ack'] is True for row in freeze['reviews'])
    # Prevent inherited runtime/compiler injection; do not dump environment values.
    flags = ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTC', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_LINKER', 'NODE_OPTIONS']
    flags += sorted(name for name in os.environ if name.startswith('CARGO_PROFILE_RELEASE_') or name == 'CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS')
    assert not [name for name in flags if os.environ.get(name)], 'unexpected build/runtime override; preserve and reconcile before starting'
    cargo_home = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo')).expanduser()
    config_paths = {parent / '.cargo' / name for parent in [ROOT, *ROOT.parents] for name in ['config', 'config.toml']}
    config_paths.update(cargo_home / name for name in ['config', 'config.toml'])
    config_state = {str(path): 'absent' if not path.exists() else pin(path) for path in sorted(config_paths)}
    assert all(value == 'absent' for value in config_state.values()), 'Cargo config exists; reconcile its code-generation settings before this default-profile campaign'
    output = args.campaign.resolve()
    output.mkdir(parents=True, exist_ok=False)
    node = str(Path(args.node).resolve())
    metadata = {
        'python': sys.version, 'node': inspect([node, '--version']), 'rustc': inspect(['rustc', '-Vv']),
        'cargo': inspect(['cargo', '-V']), 'os': platform.platform(), 'machine': platform.machine(),
        'cpu_count': os.cpu_count(), 'load_average': list(os.getloadavg()),
        'cpu': inspect(['sysctl', '-n', 'machdep.cpu.brand_string']),
        'memory_bytes': inspect(['sysctl', '-n', 'hw.memsize']),
        'power': inspect(['pmset', '-g', 'batt']),
        'power_mode': inspect(['pmset', '-g', 'custom']),
        'compiler_runtime_overrides': {name: 'unset-or-empty' for name in flags},
        'cargo_config_state': config_state,
        'cache_policy': 'existing incremental/build caches retained; construction-cold is not process-cold',
        'profile': 'Cargo existing default release; no custom workspace profile',
    }
    artifact = ROOT / 'target/wasm32-unknown-unknown/release/ring3_engine.wasm'
    metadata['prior_artifact'] = pin(artifact) if artifact.exists() else None
    record = {'schema_version': 1, 'status': 'running', 'source_pins': sources, 'freeze': pin(args.freeze), 'metadata': metadata, 'children': [], 'limits': {'global_seconds': GLOBAL_SECONDS, 'child_seconds': CHILD_SECONDS}}
    write_json(output / 'campaign.json', record)
    # The global budget begins before the one build, after read-only preflight.
    started = time.monotonic_ns()
    deadline = time.monotonic() + GLOBAL_SECONDS
    try:
        record['build'] = bounded(BUILD, output, 'build', deadline, GLOBAL_SECONDS)
        write_json(output / 'campaign.json', record)
        assert record['build']['exit_code'] == 0 and not record['build']['timed_out'], 'release build incomplete'
        assert all(pin(ROOT / path) == expected for path, expected in sources.items()), 'source drift during build'
        record['engine'] = {'path': str(artifact), **pin(artifact)}
        write_json(output / 'campaign.json', record)
        runner = ROOT / 'engine/tests/fixtures/resident-phases/run.mjs'
        for index in range(3):
            child_output = output / f'process-{index}'
            command = [node, '--expose-gc', str(runner), str(artifact), str(child_output), str(index)]
            env = dict(os.environ)
            env['RING3_ENGINE_SHA256'] = record['engine']['sha256']
            # A file, not an environment dump, supplies the exact frozen pins.
            env['RING3_PHASE_CAMPAIGN'] = str(output / 'campaign.json')
            outcome = bounded(command, output, f'process-{index}', deadline, CHILD_SECONDS, env)
            child = {'process_index': index, 'result_file': f'process-{index}/result.json', 'status': 'incomplete', **outcome}
            record['children'].append(child)
            result_path = output / child['result_file']
            if result_path.exists():
                result = json.loads(result_path.read_text())
                child['result_pin'] = pin(result_path)
                if outcome['exit_code'] == 0 and not outcome['timed_out'] and result['status'] == 'complete':
                    assert result['engine'] == {key: record['engine'][key] for key in ['sha256', 'bytes']}
                    expected_pins = [{'path': path, **value} for path, value in sorted(sources.items())]
                    assert result['source_pins_before'] == expected_pins
                    assert result['source_pins_after'] == expected_pins
                    child['status'] = 'complete'
            write_json(output / 'campaign.json', record)
            assert child['status'] == 'complete', f'process {index} incomplete; no adaptive retry'
            assert pin(artifact) == {key: record['engine'][key] for key in ['sha256', 'bytes']}, 'artifact drift'
            assert all(pin(ROOT / path) == expected for path, expected in sources.items()), 'source drift'
        record['status'] = 'complete'
    except (AssertionError, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        record['status'] = 'incomplete'
        record['failure'] = {'type': type(error).__name__, 'message': str(error)}
    finally:
        record['wall_ns'] = str(time.monotonic_ns() - started)
        record['remaining_seconds'] = max(0, deadline - time.monotonic())
        write_json(output / 'campaign.json', record)
    print(json.dumps({'status': record['status'], 'campaign': str(output), 'children': len(record['children']), 'wall_ns': record['wall_ns']}))
    return 0 if record['status'] == 'complete' else 1


if __name__ == '__main__':
    raise SystemExit(main())
