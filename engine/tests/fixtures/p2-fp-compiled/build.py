#!/usr/bin/env python3
"""freeze two original freestanding i686 floating point functions."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from coff import extract_text

HERE = Path(__file__).resolve().parent
PROFILE = HERE.parent / "p2-compiled-records/build.py"
PROFILE_SHA256 = "fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kernel", choices=("x87", "sse2"))
    parser.add_argument("output_dir", type=Path)
    args = parser.parse_args()
    if hashlib.sha256(PROFILE.read_bytes()).hexdigest() != PROFILE_SHA256:
        raise RuntimeError("fixed compiler profile changed")
    spec = importlib.util.spec_from_file_location("ring3_fixed_i686_profile", PROFILE)
    profile = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile)
    removed = [] if args.kernel == "x87" else ["-mno-sse", "-mno-sse2"]
    appended = ["-mfpmath=387", "-mlong-double-80"] if args.kernel == "x87" else ["-msse2"]
    flags = [flag for flag in profile.FLAGS if flag not in removed] + appended
    inputs = [HERE / args.kernel / "kernel.c", Path(__file__).resolve(),
              HERE / "coff.py", PROFILE]
    before = {path: path.read_bytes() for path in inputs}
    tool_hashes = {path: profile.sha256(path.read_bytes())
                   for path in (profile.CLANG, profile.OBJDUMP)}
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    (output / "kernel.c").write_bytes(before[inputs[0]])
    commands = {
        "compiler_version": [str(profile.CLANG), "--version"],
        "objdump_version": [str(profile.OBJDUMP), "--version"],
        "compile": [str(profile.CLANG), *flags, "-c", "kernel.c", "-o", "kernel.obj"],
        "disassemble": [str(profile.OBJDUMP), "--disassemble", "--section=.text",
                        "--x86-asm-syntax=intel", "kernel.obj"],
    }
    versions = {}
    for name in ("compiler_version", "objdump_version", "compile"):
        versions[name] = profile.run(commands[name], output, name, profile.ENVIRONMENT)
    text, coff = extract_text((output / "kernel.obj").read_bytes())
    (output / "kernel.text.bin").write_bytes(text)
    disassembly = profile.run(commands["disassemble"], output, "disassemble", profile.ENVIRONMENT)
    (output / "kernel.disasm.txt").write_text(disassembly, encoding="utf-8")
    for path, data in before.items():
        if path.read_bytes() != data:
            raise RuntimeError(f"build input changed: {path}")
    for path, digest in tool_hashes.items():
        if profile.sha256(path.read_bytes()) != digest:
            raise RuntimeError(f"compiler tool changed: {path}")

    receipt = {
        "schema": "ring3.fp-compiled.v1",
        "kernel": args.kernel,
        "inputs": [{"path": str(path), **profile.file_record(path)} for path in inputs],
        "compiler": profile.tool_record(profile.CLANG, versions["compiler_version"]),
        "objdump": profile.tool_record(profile.OBJDUMP, versions["objdump_version"]),
        "commands": commands, "command_cwd": str(output), "environment": profile.ENVIRONMENT,
        "profile_delta": {"remove": removed, "append": appended},
        "coff": coff,
        "artifact_policy": "complete original object and whole unmodified .text; no opcode rewriting or linking; only a declared unreferenced __fltused COFF marker is admitted",
        "claim_ceiling": "producer provenance only; whole-function execution, cdecl state, numeric results and fault retry require the separate native and actual Wasm checks",
        "artifacts": {name: profile.file_record(output / name) for name in
                      ("kernel.c", "kernel.obj", "kernel.text.bin", "kernel.disasm.txt")},
    }
    path = output / "receipt.json"
    path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(path)


if __name__ == "__main__":
    main()
