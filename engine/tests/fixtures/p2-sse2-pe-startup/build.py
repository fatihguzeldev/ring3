#!/usr/bin/env python3
"""link one original SSE2 kernel into a freestanding relocatable PE32 program."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
PROFILE = HERE.parent / "p2-compiled-records/build.py"
PROFILE_SHA256 = "fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258"
KERNEL = HERE.parent / "p2-fp-compiled/sse2/kernel.obj"
KERNEL_SHA256 = "b072459ef7d463c448abb4ca90066e3957cf8c44b3011138d1c186d66dee23f5"
KERNEL_TEXT = HERE.parent / "p2-fp-compiled/sse2/kernel.text.bin"
KERNEL_TEXT_SHA256 = "08fad7ecb51ad9026481bf5dbe347b26ef413fc292d4b27599cc6f26a62b243d"
SOURCE_SHA256 = "654539e5e8ccf22137ab93b56daa5749c942e3d66f2c782fc99642a846c21815"
IMAGE_SHA256 = "5598e22ba00a83ca377aec59187dd67853f1e57698b08fac87114a27f31455a8"
LLD = Path("/Users/fatihguzel/.rustup/toolchains/1.97.1-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-lld")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_dir", type=Path)
    args = parser.parse_args()
    source = HERE / "entry.c"
    inputs = [PROFILE, KERNEL, KERNEL_TEXT, source, Path(__file__).resolve()]
    before = {path: path.read_bytes() for path in inputs}
    for path, expected in [(PROFILE, PROFILE_SHA256), (KERNEL, KERNEL_SHA256),
                           (KERNEL_TEXT, KERNEL_TEXT_SHA256), (source, SOURCE_SHA256)]:
        if sha(before[path]) != expected:
            raise RuntimeError(f"retained producer input changed: {path.name}")
    spec = importlib.util.spec_from_file_location("ring3_sse2_pe_profile", PROFILE)
    profile = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile)
    flags = [flag for flag in profile.FLAGS if flag not in ("-mno-sse", "-mno-sse2")] + ["-msse2"]
    tools = (profile.CLANG, LLD, profile.OBJDUMP)
    tool_hashes = {path: sha(path.read_bytes()) for path in tools}
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    for name, path in [("entry.c", source), ("kernel.obj", KERNEL), ("kernel.text.bin", KERNEL_TEXT)]:
        (output / name).write_bytes(before[path])
    commands = {
        "compiler_version": [str(profile.CLANG), "--version"],
        "linker_version": [str(LLD), "-flavor", "link", "--version"],
        "objdump_version": [str(profile.OBJDUMP), "--version"],
        "compile": [str(profile.CLANG), *flags, "-c", "entry.c", "-o", "entry.obj"],
        "link": [str(LLD), "-flavor", "link", "/machine:x86", "/nodefaultlib",
                 "/timestamp:0", "/fixed:no", "/dynamicbase", "/nxcompat",
                 "/base:0x400000", "/entry:ring3_entry", "/subsystem:console",
                 "/safeseh:no", "/out:program.exe", "/map:program.map", "entry.obj", "kernel.obj"],
        "inspect": [str(profile.OBJDUMP), "--private-headers", "--section-headers",
                    "--full-contents", "program.exe"],
        "disassemble": [str(profile.OBJDUMP), "--disassemble", "--x86-asm-syntax=intel", "program.exe"],
    }
    versions = {name: profile.run(commands[name], output, name, profile.ENVIRONMENT)
                for name in ("compiler_version", "linker_version", "objdump_version")}
    profile.run(commands["compile"], output, "compile", profile.ENVIRONMENT)
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
    artifacts = ["entry.c", "entry.obj", "kernel.obj", "kernel.text.bin", "program.exe",
                 "program.map", "program.inspect.txt", "program.disasm.txt"]
    receipt = {
        "schema": "ring3.sse2-pe-startup.producer.v1",
        "inputs": [{"path": str(path), "sha256": sha(data), "bytes": len(data)}
                   for path, data in before.items()],
        "compiler": profile.tool_record(profile.CLANG, versions["compiler_version"]),
        "linker": profile.tool_record(LLD, versions["linker_version"]),
        "objdump": profile.tool_record(profile.OBJDUMP, versions["objdump_version"]),
        "commands": commands, "command_cwd": str(output), "environment": profile.ENVIRONMENT,
        "compiler_profile_delta": {"remove": ["-mno-sse", "-mno-sse2"], "append": ["-msse2"]},
        "artifact_policy": "original clang entry object plus unchanged retained kernel object linked by lld; no source/object/image byte rewriting; __fltused is defined by entry.c",
        "claim_ceiling": "one original linked PE producer; loading, rebasing, startup and generated-Wasm execution require separate checks",
        "artifacts": {name: profile.file_record(output / name) for name in artifacts},
    }
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(output / "receipt.json")


if __name__ == "__main__":
    main()
