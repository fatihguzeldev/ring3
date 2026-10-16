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
    inspection_path = repository / 'engine/tests/support/pe32/inspection.py'
    spec = importlib.util.spec_from_file_location('pe_inspection', inspection_path)
    inspection = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(inspection)
    compiler, linker = toolchain / 'bin/clang', toolchain / 'bin/lld-link'
    libraries = toolchain / 'i686-w64-mingw32/lib'
    common = [compiler, '-target', 'i686-w64-windows-gnu', '-c', '-Oz', '-ffreestanding', '-fno-builtin', '-fno-stack-protector', '-fno-unwind-tables', '-fno-asynchronous-unwind-tables', '-mno-sse', '-mno-sse2']
    commands = []

    def run(command):
        commands.append(list(map(str, command)))
        result = subprocess.run(commands[-1], cwd=output, capture_output=True, text=True)
        (output / f'command-{len(commands)}.log').write_text(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError(result.stdout + result.stderr)

    dlls = ['marshal-source', 'marshal-copy']
    definition = output / 'server.def'
    definition.write_text('EXPORTS\nDllGetClassObject=_DllGetClassObject@12\nDllCanUnloadNow=_DllCanUnloadNow@0\n')
    for role, name in enumerate(dlls):
        obj = output / (name + '.obj')
        run(common + [f'-DROLE={role}', here / 'server.c', '-o', obj])
        run([linker, '/dll', '/machine:x86', '/nodefaultlib', '/entry:DllMain@12', '/subsystem:windows', '/safeseh:no', '/timestamp:0', '/base:' + hex(0x500000 + role * 0x100000), '/def:' + str(definition), '/out:' + str(output / (name + '.dll')), obj, libraries / 'libkernel32.a'])
    name = 'marshaling-client'
    obj = output / (name + '.obj')
    run(common + [here / 'client.c', '-o', obj])
    run([linker, '/machine:x86', '/nodefaultlib', '/entry:entry', '/subsystem:console', '/safeseh:no', '/timestamp:0', '/out:' + str(output / (name + '.exe')), obj] + [libraries / ('lib' + library + '.a') for library in ['kernel32', 'ole32', 'advapi32']])
    artifacts = {name: inspection.inspect(output / name) for name in [*(dll + '.dll' for dll in dlls), 'marshaling-client.exe']}
    for name, artifact in artifacts.items():
        assert artifact['machine'] == 0x14c, name
        assert set(module.lower() for module in artifact['imports']) <= {'kernel32.dll', 'ole32.dll', 'advapi32.dll'}, artifact['imports']
    provenance = {
        'producerSources': {path.name: digest(path) for path in here.iterdir() if path.suffix in ['.c', '.h', '.py']},
        'producerDirectory': str(here.relative_to(repository)),
        'inspectionSource': str(inspection_path.relative_to(repository)),
        'inspectionSha256': digest(inspection_path),
        'compiler': subprocess.check_output([compiler, '--version'], text=True),
        'commands': commands,
        'artifacts': artifacts,
        'expectedExit': 0,
        'expectedFile': 'C:\\Temp\\com-marshaling.log',
        'recordLayout': {'bytes': 16, 'event': 0, 'thread': 4, 'role': 8, 'value': 12},
        'claim': 'self-authored freestanding PE32 custom IMarshal client/server with guest-defined immutable value packet, actual x86 COM header layout assertions and failure cleanup; no CRT implementation or proprietary input',
    }
    (output / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    print(json.dumps({'artifacts': str(output), 'hashes': {name: artifact['sha256'] for name, artifact in artifacts.items()}}))


if __name__ == '__main__':
    main()
