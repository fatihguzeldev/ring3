#!/usr/bin/env python3
"""freeze one original clang/coff/lld pe32 image; routine tests use its bytes."""

import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess


CLANG = Path('/usr/bin/clang')
CLANG_BACKEND = Path('/Library/Developer/CommandLineTools/usr/bin/clang')
LLD = Path('/Users/fatihguzel/.rustup/toolchains/1.97.1-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-lld')
OBJDUMP = Path('/Library/Developer/CommandLineTools/usr/bin/llvm-objdump')
ENV = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'TZ': 'UTC', 'SOURCE_DATE_EPOCH': '0'}
TEXT = bytes.fromhex('b80700000083c023ebfe')


def pin(path):
    raw = path.read_bytes()
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def pe_metadata(raw):
    assert len(raw) >= 64 and raw[:2] == b'MZ'
    pe = struct.unpack_from('<I', raw, 60)[0]
    assert raw[pe:pe + 4] == b'PE\0\0'
    coff, optional = pe + 4, pe + 24
    assert struct.unpack_from('<HHI', raw, coff) == (0x14c, 1, 0)
    assert struct.unpack_from('<II', raw, coff + 8) == (0, 0)
    assert struct.unpack_from('<H', raw, coff + 16)[0] == 224
    assert struct.unpack_from('<H', raw, optional)[0] == 0x10b
    assert struct.unpack_from('<I', raw, optional + 16)[0] == 0x1000
    assert struct.unpack_from('<I', raw, optional + 28)[0] == 0x400000
    assert struct.unpack_from('<II', raw, optional + 32) == (4096, 512)
    assert struct.unpack_from('<II', raw, optional + 56) == (0x2000, 512)
    assert struct.unpack_from('<HH', raw, optional + 68) == (3, 0x8100)
    assert struct.unpack_from('<I', raw, optional + 92)[0] == 16
    assert raw[optional + 96:optional + 224] == bytes(128)
    section = optional + 224
    assert raw[section:section + 8] == b'.text\0\0\0'
    assert struct.unpack_from('<IIII', raw, section + 8) == (10, 0x1000, 512, 512)
    assert struct.unpack_from('<I', raw, section + 36)[0] == 0x60000020
    assert len(raw) == 1024 and raw[512:522] == TEXT
    return {'image_base': 0x400000, 'image_size': 0x2000, 'entry_point': 0x401000,
            'mapped_pages': 2, 'dll_characteristics': 0x8100, 'dll_characteristics_offset': optional + 70,
            'text_rva': 0x1000, 'text_raw_offset': 512, 'text_virtual_bytes': 10,
            'text_raw_bytes': 512, 'entry_offsets': [0, 8]}


def coff_text(raw):
    assert len(raw) >= 20
    machine, count, timestamp, symbols, symbol_count, optional, _ = struct.unpack_from('<HHIIIHH', raw)
    assert machine == 0x14c and timestamp == optional == 0 and 1 <= count <= 16
    assert 20 + 40 * count <= symbols and symbols + 18 * symbol_count + 4 <= len(raw)
    sections = []
    for i in range(count):
        name, _, _, size, pointer, reloc_at, lines_at, reloc_count, lines, flags = struct.unpack_from('<8sIIIIIIHHI', raw, 20 + i * 40)
        assert reloc_at == lines_at == reloc_count == lines == 0
        if size:
            assert 20 + 40 * count <= pointer and pointer + size <= symbols
        sections.append({'name': name.rstrip(b'\0').decode('ascii'), 'bytes': size,
                         'pointer': pointer, 'flags': flags})
    text = [s for s in sections if s['name'] == '.text']
    assert len(text) == 1 and [s for s in sections if s['flags'] & 0x20] == text
    text = text[0]
    value = raw[text['pointer']:text['pointer'] + text['bytes']]
    assert value == TEXT
    return value, {'machine': machine, 'timestamp': timestamp, 'relocations': 0, 'sections': sections}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new immutable build directory')
    output = parser.parse_args().output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).resolve().with_name('program.S')
    builder = Path(__file__).resolve()
    inputs = {'program.S': pin(source), 'build.py': pin(builder)}
    tools = {name: {'path': str(path), 'resolved_path': str(path.resolve(strict=True)), **pin(path)}
             for name, path in [('clang', CLANG), ('clang_backend', CLANG_BACKEND), ('lld', LLD), ('objdump', OBJDUMP)]}
    (output / 'program.S').write_bytes(source.read_bytes())
    (output / 'build.py').write_bytes(builder.read_bytes())
    commands = [
        [str(CLANG), '--version'], [str(LLD), '-flavor', 'link', '--version'], [str(OBJDUMP), '--version'],
        [str(CLANG), '--target=i686-pc-windows-msvc', '-c', '-mno-incremental-linker-compatible', 'program.S', '-o', 'program.obj'],
        [str(LLD), '-flavor', 'link', '/entry:entry', '/subsystem:console', '/machine:x86', '/nodefaultlib',
         '/base:0x400000', '/fixed', '/dynamicbase:no', '/nxcompat', '/safeseh:no', '/timestamp:0',
         '/out:program.exe', 'program.obj'],
        [str(OBJDUMP), '--private-headers', '--section-headers', '--disassemble', '--x86-asm-syntax=intel', 'program.exe'],
    ]
    assert all('tsaware' not in arg.lower() for command in commands for arg in command)
    versions = []
    for i, command in enumerate(commands):
        result = subprocess.run(command, cwd=output, env=ENV, capture_output=True)
        (output / f'command-{i}.stdout.log').write_bytes(result.stdout)
        (output / f'command-{i}.stderr.log').write_bytes(result.stderr)
        if result.returncode:
            raise RuntimeError(f'command{i} failed: {result.returncode}; original logs retained')
        if i < 3:
            versions.append(result.stdout.decode('utf8'))
        if i == 5:
            (output / 'program.disasm.txt').write_bytes(result.stdout)
    text, coff = coff_text((output / 'program.obj').read_bytes())
    (output / 'program.text.bin').write_bytes(text)
    image = (output / 'program.exe').read_bytes()
    metadata = pe_metadata(image)
    assert pin(source) == inputs['program.S'] and pin(builder) == inputs['build.py']
    for name, path in [('clang', CLANG), ('clang_backend', CLANG_BACKEND), ('lld', LLD), ('objdump', OBJDUMP)]:
        assert pin(path) == {k: tools[name][k] for k in ['bytes', 'sha256']}
    for name, version in zip(['clang', 'lld', 'objdump'], versions):
        tools[name]['version'] = version
    receipt = {'schema': 'ring3.pe32-tsaware.original.v1', 'inputs': inputs, 'tools': tools,
               'commands': commands, 'environment': ENV, 'command_cwd': str(output), 'coff': coff,
               'pe': metadata, 'original_linker_bytes_unmodified': True,
               'artifacts': {name: pin(output / name) for name in ['program.S', 'build.py', 'program.obj',
                             'program.exe', 'program.text.bin', 'program.disasm.txt']}}
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({'status': 'ok', 'output': str(output), 'image': receipt['artifacts']['program.exe']}))


if __name__ == '__main__':
    main()
