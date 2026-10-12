#!/usr/bin/env python3
"""link one original freestanding C program with self-authored stdcall imports."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
PROFILE = Path(__file__).resolve().parents[7] / "engine/tests/programs/fixtures/p2-compiled-records/build.py"
PROFILE_SHA256 = "fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258"
SOURCE_SHA256 = "421b66212c3b8542694e6bdf2f3d5636378510557cea17f74642a30f1fa8ddb3"
DEFINITION_SHA256 = "514fd9eca2367fdbd6b21d54d530c185db32865d9ce60326091cc829d3b1c655"
IMAGE_SHA256 = "7c0451977a0818d5f4905a2b00f3a3bae63b9449ef22775ea003ebb0ca69c5d5"
LLD = Path("/Users/fatihguzel/.rustup/toolchains/1.97.1-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-lld")
NM = Path("/Library/Developer/CommandLineTools/usr/bin/llvm-nm")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_dir", type=Path)
    args = parser.parse_args()
    source = HERE / "entry.c"
    definition = HERE / "kernel32.def"
    inputs = [PROFILE, source, definition, Path(__file__).resolve()]
    before = {path: path.read_bytes() for path in inputs}
    for path, expected in [(PROFILE, PROFILE_SHA256), (source, SOURCE_SHA256),
                           (definition, DEFINITION_SHA256)]:
        if sha(before[path]) != expected:
            raise RuntimeError(f"retained producer input changed: {path.name}")
    spec = importlib.util.spec_from_file_location("ring3_c_imports_profile", PROFILE)
    profile = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile)
    tools = (profile.CLANG, LLD, profile.OBJDUMP, NM)
    tool_hashes = {path: sha(path.read_bytes()) for path in tools}
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    for name, path in [("entry.c", source), ("kernel32.def", definition)]:
        (output / name).write_bytes(before[path])
    commands = {
        "compiler_version": [str(profile.CLANG), "--version"],
        "linker_version": [str(LLD), "-flavor", "link", "--version"],
        "objdump_version": [str(profile.OBJDUMP), "--version"],
        "nm_version": [str(NM), "--version"],
        "compile": [str(profile.CLANG), *profile.FLAGS, "-c", "entry.c", "-o", "entry.obj"],
        "import_library": [str(LLD), "-flavor", "link", "/lib", "/machine:x86",
                           "/def:kernel32.def", "/out:kernel32.lib"],
        "entry_symbols": [str(NM), "--undefined-only", "entry.obj"],
        "entry_inspect": [str(profile.OBJDUMP), "--private-headers", "--section-headers",
                          "--reloc", "--syms", "--full-contents", "entry.obj"],
        "library_symbols": [str(NM), "--print-file-name", "kernel32.lib"],
        "link": [str(LLD), "-flavor", "link", "/machine:x86", "/nodefaultlib",
                 "/timestamp:0", "/fixed:no", "/dynamicbase", "/nxcompat",
                 "/base:0x400000", "/entry:ring3_entry", "/subsystem:console",
                 "/safeseh:no", "/out:program.exe", "/map:program.map", "entry.obj", "kernel32.lib"],
        "inspect": [str(profile.OBJDUMP), "--private-headers", "--section-headers",
                    "--full-contents", "program.exe"],
        "disassemble": [str(profile.OBJDUMP), "--disassemble", "--x86-asm-syntax=intel", "program.exe"],
    }
    versions = {name: profile.run(commands[name], output, name, profile.ENVIRONMENT)
                for name in ("compiler_version", "linker_version", "objdump_version", "nm_version")}
    profile.run(commands["compile"], output, "compile", profile.ENVIRONMENT)
    profile.run(commands["import_library"], output, "import_library", profile.ENVIRONMENT)
    for stem, name in [("entry_symbols", "entry.symbols.txt"),
                       ("entry_inspect", "entry.inspect.txt"),
                       ("library_symbols", "kernel32.symbols.txt")]:
        (output / name).write_text(profile.run(commands[stem], output, stem, profile.ENVIRONMENT), encoding="utf-8")
    undefined = {line.split()[-1] for line in (output / "entry.symbols.txt").read_text().splitlines() if line.strip()}
    if undefined != {"__imp__GetLastError@0", "__imp__SetLastError@4"}:
        raise RuntimeError(f"unexpected original C unresolved symbols: {sorted(undefined)}")
    profile.run(commands["link"], output, "link", profile.ENVIRONMENT)
    (output / "program.inspect.txt").write_text(
        profile.run(commands["inspect"], output, "inspect", profile.ENVIRONMENT), encoding="utf-8")
    (output / "program.disasm.txt").write_text(
        profile.run(commands["disassemble"], output, "disassemble", profile.ENVIRONMENT), encoding="utf-8")
    if sha((output / "program.exe").read_bytes()) != IMAGE_SHA256:
        raise RuntimeError("original observed linked PE changed; preserve output and inspect the difference")
    for path, data in before.items():
        if path.read_bytes() != data:
            raise RuntimeError(f"producer input changed during build: {path}")
    for path, digest in tool_hashes.items():
        if sha(path.read_bytes()) != digest:
            raise RuntimeError(f"producer tool changed during build: {path}")
    artifacts = ["entry.c", "kernel32.def", "entry.obj", "kernel32.lib", "entry.symbols.txt",
                 "entry.inspect.txt", "kernel32.symbols.txt", "program.exe", "program.map",
                 "program.inspect.txt", "program.disasm.txt"]
    receipt = {
        "schema": "ring3.c-imports.producer.v1",
        "inputs": [{"path": str(path), "sha256": sha(data), "bytes": len(data)}
                   for path, data in before.items()],
        "compiler": profile.tool_record(profile.CLANG, versions["compiler_version"]),
        "linker": profile.tool_record(LLD, versions["linker_version"]),
        "objdump": profile.tool_record(profile.OBJDUMP, versions["objdump_version"]),
        "nm": profile.tool_record(NM, versions["nm_version"]),
        "commands": commands, "command_cwd": str(output), "environment": profile.ENVIRONMENT,
        "compiler_profile_delta": {"remove": [], "append": []},
        "artifact_policy": "original clang C object and self-authored DEF import metadata linked by lld; EXPORTAS retains stdcall COFF symbols and requests undecorated DLL names; no source/object/library/image byte rewriting",
        "claim_ceiling": "one original linked PE producer with two imports; loading, rebasing, provider completion and generated-Wasm execution require separate checks",
        "artifacts": {name: profile.file_record(output / name) for name in artifacts},
    }
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(output / "receipt.json")


if __name__ == "__main__":
    main()
