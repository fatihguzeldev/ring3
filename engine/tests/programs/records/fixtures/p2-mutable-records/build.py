#!/usr/bin/env python3
"""freeze this mutable scalar leaf using the unchanged readonly coff gate."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys


DEPENDENCY = Path(__file__).resolve().parents[6] / "engine/tests/programs/records/fixtures/p2-compiled-records/build.py"
DEPENDENCY_SHA256 = "fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258"


def load_profile():
    dependency_bytes = DEPENDENCY.read_bytes()
    if hashlib.sha256(dependency_bytes).hexdigest() != DEPENDENCY_SHA256:
        raise RuntimeError("readonly builder dependency differs from the frozen profile")
    # importing the profile must not add files to the readonly fixture.
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("ring3_readonly_record_profile", DEPENDENCY)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load the fixed readonly builder dependency")
    profile = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile)
    if DEPENDENCY.read_bytes() != dependency_bytes:
        raise RuntimeError("readonly builder dependency changed during import")
    return profile


def json_sha256(value):
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_dir", type=Path, help="new directory for frozen artifacts")
    args = parser.parse_args()
    profile = load_profile()
    source = Path(__file__).resolve().with_name("kernel.c")
    builder = Path(__file__).resolve()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    inputs = {
        "source": {"path": str(source), **profile.file_record(source)},
        "builder": {"path": str(builder), **profile.file_record(builder)},
        "dependency": {"path": str(DEPENDENCY), **profile.file_record(DEPENDENCY)},
    }
    snapshots = {
        "source": (source, "kernel.c"),
        "builder": (builder, "build.py"),
        "dependency": (DEPENDENCY, "compiled-records-build.py"),
    }
    for name, (path, filename) in snapshots.items():
        (output / filename).write_bytes(path.read_bytes())
        if profile.file_record(output / filename)["sha256"] != inputs[name]["sha256"]:
            raise RuntimeError(f"{name} changed while taking its input snapshot")
    compiler_hash = profile.sha256(profile.CLANG.read_bytes())
    objdump_hash = profile.sha256(profile.OBJDUMP.read_bytes())
    flags = list(profile.FLAGS)
    environment = dict(profile.ENVIRONMENT)
    commands = {
        "compiler_version": [str(profile.CLANG), "--version"],
        "objdump_version": [str(profile.OBJDUMP), "--version"],
        "compile": [str(profile.CLANG), *flags, "-c", "kernel.c", "-o", "kernel.obj"],
        "disassemble": [str(profile.OBJDUMP), "--disassemble", "--section=.text",
                        "--x86-asm-syntax=intel", "kernel.obj"],
    }
    compiler_version = profile.run(commands["compiler_version"], output,
                                   "compiler-version", environment)
    objdump_version = profile.run(commands["objdump_version"], output,
                                  "objdump-version", environment)
    profile.run(commands["compile"], output, "compile", environment)
    text, coff = profile.extract_text((output / "kernel.obj").read_bytes())
    (output / "kernel.text.bin").write_bytes(text)
    disassembly = profile.run(commands["disassemble"], output, "disassemble", environment)
    (output / "kernel.disasm.txt").write_text(disassembly, encoding="utf-8")
    compiler = profile.tool_record(profile.CLANG, compiler_version)
    objdump = profile.tool_record(profile.OBJDUMP, objdump_version)
    if compiler["sha256"] != compiler_hash or objdump["sha256"] != objdump_hash:
        raise RuntimeError("a compiler tool changed during the build")
    for name, (path, _) in snapshots.items():
        expected = {key: inputs[name][key] for key in ("file", "bytes", "sha256")}
        if profile.file_record(path) != expected:
            raise RuntimeError(f"{name} changed during the build")
    receipt = {
        "schema": "ring3.mutable-records.v1",
        **inputs,
        "compiler": compiler,
        "objdump": objdump,
        "commands": commands,
        "command_cwd": str(output),
        "environment": environment,
        "profile": {
            "flags": flags,
            "flags_sha256": json_sha256(flags),
            "environment_sha256": json_sha256(environment),
            "commands_sha256": json_sha256(commands),
            "hash_encoding": "utf8 JSON, sorted keys, compact separators",
        },
        "record": {
            "bytes": 8,
            "offsets": {"signed_byte": 0, "tag": 1, "signed_word": 2,
                        "weight": 4, "marker": 6},
            "mutable_offsets": [1, 4, 5],
        },
        "coff": coff,
        "artifacts": {name: profile.file_record(output / filename) for name, filename in [
            ("source_snapshot", "kernel.c"), ("builder_snapshot", "build.py"),
            ("dependency_snapshot", "compiled-records-build.py"),
            ("object", "kernel.obj"), ("text", "kernel.text.bin"),
            ("disassembly", "kernel.disasm.txt"),
        ]},
    }
    receipt_path = output / "receipt.json"
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(receipt_path)


if __name__ == "__main__":
    main()
