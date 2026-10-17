import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--toolchain', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    toolchain = args.toolchain.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    here = Path(__file__).resolve().parent
    repository = here.parents[6]
    inspector = repository / 'engine/tests/support/pe32/inspection.py'
    spec = importlib.util.spec_from_file_location('pe_inspection', inspector)
    inspection = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(inspection)
    compiler, linker = toolchain / 'bin/clang', toolchain / 'bin/lld-link'
    libraries = toolchain / 'i686-w64-mingw32/lib'
    obj, exe = output / 'dialogs.obj', output / 'dialogs-authored.exe'
    commands = [
        [compiler, '-target', 'i686-w64-windows-gnu', '-c', '-Oz', '-ffreestanding', '-fno-builtin', '-fno-stack-protector', '-fno-unwind-tables', '-fno-asynchronous-unwind-tables', '-mno-sse', '-mno-sse2', here / 'client.c', '-o', obj],
        [linker, '/machine:x86', '/nodefaultlib', '/entry:entry', '/subsystem:console', '/safeseh:no', '/timestamp:0', '/out:' + str(exe), obj] + [libraries / ('lib' + library + '.a') for library in ['kernel32', 'user32', 'gdi32']],
    ]
    for index, command in enumerate(commands, 1):
        result = subprocess.run(list(map(str, command)), cwd=output, capture_output=True, text=True)
        (output / f'command-{index}.log').write_text(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError(result.stdout + result.stderr)
    artifact = inspection.inspect(exe)
    assert artifact['machine'] == 0x14c
    assert set(name.lower() for name in artifact['imports']) <= {'kernel32.dll', 'user32.dll', 'gdi32.dll'}
    provenance = {
        'producerDirectory': str(here.relative_to(repository)),
        'producerSources': {path.name: digest(path) for path in here.iterdir() if path.suffix in ['.c', '.py']},
        'inspectionSource': str(inspector.relative_to(repository)),
        'inspectionSha256': digest(inspector),
        'compiler': subprocess.check_output([compiler, '--version'], text=True),
        'commands': [list(map(str, command)) for command in commands],
        'artifacts': {exe.name: artifact},
        'expectedExit': 0,
        'expectedFile': 'C:\\Temp\\dialogs.log',
        'recordLayout': {'bytes': 16, 'event': 0, 'scenario': 4, 'message': 8, 'value': 12},
        'claim': 'self-authored freestanding PE32 standard/extended ANSI/Unicode modeless/modal dialogs with real child procedures, fonts, nested creation, posted modal command and ordered destruction; no CRT or proprietary input',
    }
    (output / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    print(json.dumps({'artifacts': str(output), 'hashes': {exe.name: artifact['sha256']}}))


if __name__ == '__main__':
    main()
