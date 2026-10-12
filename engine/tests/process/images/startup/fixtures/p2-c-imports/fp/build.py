#!/usr/bin/env python3
"""link the retained SSE2 kernel and self-authored stdcall imports into PE32."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
FIXTURES = HERE.parent.parent
PROFILE = Path(__file__).resolve().parents[8] / "engine/tests/programs/records/fixtures/p2-compiled-records/build.py"
PROFILE_SHA256 = "fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258"
KERNEL = Path(__file__).resolve().parents[8] / "engine/tests/programs/numeric/fixtures/p2-fp-compiled/sse2/kernel.obj"
KERNEL_SHA256 = "b072459ef7d463c448abb4ca90066e3957cf8c44b3011138d1c186d66dee23f5"
KERNEL_TEXT = Path(__file__).resolve().parents[8] / "engine/tests/programs/numeric/fixtures/p2-fp-compiled/sse2/kernel.text.bin"
KERNEL_TEXT_SHA256 = "08fad7ecb51ad9026481bf5dbe347b26ef413fc292d4b27599cc6f26a62b243d"
DEFINITION = HERE.parent / "kernel32.def"
DEFINITION_SHA256 = "514fd9eca2367fdbd6b21d54d530c185db32865d9ce60326091cc829d3b1c655"
LIBRARY = HERE.parent / "kernel32.lib"
LIBRARY_SHA256 = "04357cbd08941a86c69c7fe7348b05267363d23876870bac9f121e175361effd"
SOURCE_SHA256 = "2ebd510c4d013ecfa54bb5bf91bda8dc1c674f7a0317e3441904e58a4118fc6e"
LLD = Path("/Users/fatihguzel/.rustup/toolchains/1.97.1-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-lld")
NM = Path("/Library/Developer/CommandLineTools/usr/bin/llvm-nm")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_dir", type=Path)
    args = parser.parse_args()
    source = HERE / "entry.c"
    inputs = [PROFILE, KERNEL, KERNEL_TEXT, DEFINITION, LIBRARY, source,
              Path(__file__).resolve()]
    before = {path: path.read_bytes() for path in inputs}
    for path, expected in [(PROFILE, PROFILE_SHA256), (KERNEL, KERNEL_SHA256),
                           (KERNEL_TEXT, KERNEL_TEXT_SHA256),
                           (DEFINITION, DEFINITION_SHA256), (LIBRARY, LIBRARY_SHA256),
                           (source, SOURCE_SHA256)]:
        if sha(before[path]) != expected:
            raise RuntimeError(f"retained producer input changed: {path}")
    spec = importlib.util.spec_from_file_location("ring3_fp_imports_profile", PROFILE)
    profile = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile)
    flags = [flag for flag in profile.FLAGS if flag not in ("-mno-sse", "-mno-sse2")] + ["-msse2"]
    tools = (profile.CLANG, LLD, profile.OBJDUMP, NM)
    tool_hashes = {path: sha(path.read_bytes()) for path in tools}
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    for name, path in [("entry.c", source), ("kernel.obj", KERNEL),
                       ("kernel.text.bin", KERNEL_TEXT), ("kernel32.def", DEFINITION),
                       ("kernel32.lib", LIBRARY)]:
        (output / name).write_bytes(before[path])
    commands = {
        "compiler_version": [str(profile.CLANG), "--version"],
        "linker_version": [str(LLD), "-flavor", "link", "--version"],
        "objdump_version": [str(profile.OBJDUMP), "--version"],
        "nm_version": [str(NM), "--version"],
        "compile": [str(profile.CLANG), *flags, "-c", "entry.c", "-o", "entry.obj"],
        "entry_symbols": [str(NM), "--undefined-only", "entry.obj"],
        "entry_inspect": [str(profile.OBJDUMP), "--private-headers", "--section-headers",
                          "--reloc", "--syms", "--full-contents", "entry.obj"],
        "library_symbols": [str(NM), "--print-file-name", "kernel32.lib"],
        "link": [str(LLD), "-flavor", "link", "/machine:x86", "/nodefaultlib",
                 "/timestamp:0", "/fixed:no", "/dynamicbase", "/nxcompat",
                 "/base:0x400000", "/entry:ring3_entry", "/subsystem:console",
                 "/safeseh:no", "/out:program.exe", "/map:program.map", "entry.obj",
                 "kernel.obj", "kernel32.lib"],
        "inspect": [str(profile.OBJDUMP), "--private-headers", "--section-headers",
                    "--full-contents", "program.exe"],
        "disassemble": [str(profile.OBJDUMP), "--disassemble", "--x86-asm-syntax=intel", "program.exe"],
    }
    versions = {name: profile.run(commands[name], output, name, profile.ENVIRONMENT)
                for name in ("compiler_version", "linker_version", "objdump_version", "nm_version")}
    profile.run(commands["compile"], output, "compile", profile.ENVIRONMENT)
    for stem, name in [("entry_symbols", "entry.symbols.txt"),
                       ("entry_inspect", "entry.inspect.txt"),
                       ("library_symbols", "kernel32.symbols.txt")]:
        (output / name).write_text(profile.run(commands[stem], output, stem, profile.ENVIRONMENT), encoding="utf-8")
    undefined = {line.split()[-1] for line in (output / "entry.symbols.txt").read_text().splitlines() if line.strip()}
    if undefined != {"_ring3_reduce", "__imp__GetLastError@0", "__imp__SetLastError@4"}:
        raise RuntimeError(f"unexpected original C unresolved symbols: {sorted(undefined)}")
    profile.run(commands["link"], output, "link", profile.ENVIRONMENT)
    (output / "program.inspect.txt").write_text(
        profile.run(commands["inspect"], output, "inspect", profile.ENVIRONMENT), encoding="utf-8")
    (output / "program.disasm.txt").write_text(
        profile.run(commands["disassemble"], output, "disassemble", profile.ENVIRONMENT), encoding="utf-8")
    for path, data in before.items():
        if path.read_bytes() != data:
            raise RuntimeError(f"producer input changed during build: {path}")
    for path, digest in tool_hashes.items():
        if sha(path.read_bytes()) != digest:
            raise RuntimeError(f"producer tool changed during build: {path}")
    artifacts = ["entry.c", "entry.obj", "kernel.obj", "kernel.text.bin", "kernel32.def",
                 "kernel32.lib", "entry.symbols.txt", "entry.inspect.txt", "kernel32.symbols.txt",
                 "program.exe", "program.map", "program.inspect.txt", "program.disasm.txt"]
    receipt = {
        "schema": "ring3.fp-imports.producer.v1",
        "inputs": [{"path": str(path), "sha256": sha(data), "bytes": len(data)}
                   for path, data in before.items()],
        "compiler": profile.tool_record(profile.CLANG, versions["compiler_version"]),
        "linker": profile.tool_record(LLD, versions["linker_version"]),
        "objdump": profile.tool_record(profile.OBJDUMP, versions["objdump_version"]),
        "nm": profile.tool_record(NM, versions["nm_version"]),
        "commands": commands, "command_cwd": str(output), "environment": profile.ENVIRONMENT,
        "compiler_profile_delta": {"remove": ["-mno-sse", "-mno-sse2"], "append": ["-msse2"]},
        "artifact_policy": "original clang C entry plus unchanged retained SSE2 kernel and parent DEF/import library linked by lld; no import-library rebuild or source/object/library/image byte rewriting; __fltused is defined by entry.c",
        "claim_ceiling": "one original linked PE combining two original SSE2 calls with two Win32 imports; loading, rebasing, suspension, provider completion and generated-Wasm execution require separate checks",
        "artifacts": {name: profile.file_record(output / name) for name in artifacts},
    }
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(output / "receipt.json")


if __name__ == "__main__":
    main()
