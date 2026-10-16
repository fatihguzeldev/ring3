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
    repository = here.parents[5]
    inspection_path = repository / 'engine/tests/support/pe32/inspection.py'
    inspection_spec = importlib.util.spec_from_file_location('pe_inspection', inspection_path)
    inspection = importlib.util.module_from_spec(inspection_spec)
    inspection_spec.loader.exec_module(inspection)
    commands = []

    def run(command):
        result = subprocess.run(list(map(str, command)), cwd=output, capture_output=True, text=True)
        commands.append(list(map(str, command)))
        (output / f'command-{len(commands)}.log').write_text(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError(result.stdout + result.stderr)

    compiler = toolchain / 'bin/clang'
    linker = toolchain / 'bin/lld-link'
    common = [compiler, '-target', 'i686-w64-windows-gnu', '-c', '-Oz', '-ffreestanding', '-fno-builtin', '-fno-stack-protector', '-fno-unwind-tables', '-fno-asynchronous-unwind-tables', '-mno-sse', '-mno-sse2']
    libraries = toolchain / 'i686-w64-mingw32/lib'
    for name, macro, base in [('com-server', None, '0x500000'), ('com-no-unload', 'NO_CAN_UNLOAD', '0x600000'), ('com-failed', 'FAIL_ATTACH', '0x700000')]:
        obj = output / (name + '.obj')
        run(common + (['-D' + macro] if macro else []) + [here / 'server.c', '-o', obj])
        definition = output / (name + '.def')
        definition.write_text('EXPORTS\nDllGetClassObject=_DllGetClassObject@12\n' + ('' if macro == 'NO_CAN_UNLOAD' else 'DllCanUnloadNow=_DllCanUnloadNow@0\n'))
        run([linker, '/dll', '/machine:x86', '/nodefaultlib', '/entry:DllMain@12', '/subsystem:windows', '/safeseh:no', '/timestamp:0', '/base:' + base, '/def:' + str(definition), '/out:' + str(output / (name + '.dll')), obj, libraries / 'libkernel32.a'])
    obj = output / 'com-client.obj'
    run(common + [here / 'client.c', '-o', obj])
    run([linker, '/machine:x86', '/nodefaultlib', '/entry:entry', '/subsystem:console', '/safeseh:no', '/timestamp:0', '/out:' + str(output / 'com-client.exe'), obj] + [libraries / ('lib' + name + '.a') for name in ['kernel32', 'ole32', 'advapi32']])
    artifacts = {name: inspection.inspect(output / name) for name in ['com-server.dll', 'com-no-unload.dll', 'com-failed.dll', 'com-client.exe']}
    for name, artifact in artifacts.items():
        assert artifact['machine'] == 0x14c, name
        assert set(module.lower() for module in artifact['imports']) <= {'kernel32.dll', 'ole32.dll', 'advapi32.dll'}, artifact['imports']
    assert {'DllGetClassObject', 'DllCanUnloadNow'} <= set(artifacts['com-server.dll']['exports'])
    assert artifacts['com-no-unload.dll']['exports'] == ['DllGetClassObject']
    provenance = {'producerSources': {path.name: digest(path) for path in here.iterdir() if path.suffix in ['.c', '.h', '.py']}, 'inspectionSource': str(inspection_path.relative_to(repository)), 'inspectionSha256': digest(inspection_path), 'compiler': subprocess.check_output([compiler, '--version'], text=True), 'commands': commands, 'artifacts': artifacts, 'expectedExit': 0, 'expectedFile': 'C:\\Temp\\com-activation.log', 'expectedBytes': 'AGqflcccirBrufUDAGcfGcfGcfrUDAGUDNGcfrn', 'claim': 'self-authored freestanding PE32 COM client/server; no CRT implementation or proprietary binary input'}
    (output / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    print(json.dumps({'artifacts': str(output), 'hashes': {name: artifact['sha256'] for name, artifact in artifacts.items()}}))


if __name__ == '__main__':
    main()
