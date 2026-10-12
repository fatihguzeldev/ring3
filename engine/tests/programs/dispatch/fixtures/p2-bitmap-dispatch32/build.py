#!/usr/bin/env python3
"""discover one original ready-bitmap dispatch with the unchanged i686 profile."""
import argparse
import ast
import hashlib
import importlib.util
import inspect
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[6]
DEPENDENCY = ROOT / "engine/tests/programs/records/fixtures/p2-compiled-records/build.py"
DEPENDENCY_SHA256 = "fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258"
SOURCE_SHA256 = "44243024fe2f431e55295772f250ba187f8d8e9cb94cf580941d18d6a3b6a974"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_dir", type=Path)
    args = parser.parse_args()
    dependency = DEPENDENCY.read_bytes()
    if sha(dependency) != DEPENDENCY_SHA256:
        raise RuntimeError("fixed compiler profile changed")
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("ring3_bitmap_dispatch_profile", DEPENDENCY)
    profile = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile)
    extractor_source = inspect.getsource(profile.extract_text)
    if extractor_source.count('"_ring3_reduce"') != 2:
        raise RuntimeError("unexpected fixed extractor symbol checks")
    adapted = extractor_source.replace("ring3_reduce", "ring3_take_ready_bit")
    namespace = dict(vars(profile))
    exec(compile(ast.parse(adapted), "<fixed-bitmap-dispatch-symbol-extractor>", "exec"), namespace)
    source = Path(__file__).resolve().with_name("kernel.c")
    builder = Path(__file__).resolve()
    source_before, builder_before = source.read_bytes(), builder.read_bytes()
    if sha(source_before) != SOURCE_SHA256:
        raise RuntimeError("original ready-bitmap dispatch source changed")
    compiler_before, objdump_before = profile.CLANG.read_bytes(), profile.OBJDUMP.read_bytes()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    (output / "kernel.c").write_bytes(source_before)
    commands = {
        "compiler_version": [str(profile.CLANG), "--version"],
        "objdump_version": [str(profile.OBJDUMP), "--version"],
        "compile": [str(profile.CLANG), *profile.FLAGS, "-c", "kernel.c", "-o", "kernel.obj"],
        "disassemble": [str(profile.OBJDUMP), "--disassemble", "--section=.text",
                        "--x86-asm-syntax=intel", "kernel.obj"],
    }
    compiler_version = profile.run(commands["compiler_version"], output, "compiler-version", profile.ENVIRONMENT)
    objdump_version = profile.run(commands["objdump_version"], output, "objdump-version", profile.ENVIRONMENT)
    profile.run(commands["compile"], output, "compile", profile.ENVIRONMENT)
    original_object = (output / "kernel.obj").read_bytes()
    text, coff = namespace["extract_text"](original_object)
    for section in coff["sections"]:
        if section["bytes"] and section["name"] not in (".text", ".debug$S"):
            raise RuntimeError("unexpected original non-code dependency")
    if coff["function"]["entry_offset"] != 0:
        raise RuntimeError("original function does not begin at the whole text entry")
    (output / "kernel.text.bin").write_bytes(text)
    disassembly = profile.run(commands["disassemble"], output, "disassemble", profile.ENVIRONMENT)
    (output / "kernel.disasm.txt").write_text(disassembly, encoding="utf-8")
    assert source.read_bytes() == source_before and builder.read_bytes() == builder_before
    assert profile.CLANG.read_bytes() == compiler_before and profile.OBJDUMP.read_bytes() == objdump_before
    assert DEPENDENCY.read_bytes() == dependency
    receipt = {
        "schema": "ring3.bitmap-dispatch-discovery.v1",
        "source": {"path": str(source), **profile.file_record(source)},
        "builder": {"path": str(builder), **profile.file_record(builder)},
        "dependency": {"path": str(DEPENDENCY), "sha256": DEPENDENCY_SHA256},
        "extractor_delta": "only fixed function symbol and diagnostic spelling ring3_reduce to ring3_take_ready_bit",
        "original_extractor_sha256": sha(extractor_source.encode()),
        "adapted_extractor_sha256": sha(adapted.encode()),
        "compiler": profile.tool_record(profile.CLANG, compiler_version),
        "objdump": profile.tool_record(profile.OBJDUMP, objdump_version),
        "commands": commands, "command_cwd": str(output), "environment": profile.ENVIRONMENT,
        "coff": coff,
        "source_intent": "header-free C11 ready-bitmap consumer: intersect pending with allowed, return sentinel 32 without calling ctz when empty, otherwise remove the least significant ready bit and return its zero-based slot",
        "artifact_policy": "retain the complete original object, whole extracted function text, and disassembly without rewriting; no opcode, size, or artifact hash assumed before discovery",
        "claim_ceiling": "producer discovery only; original emission, complete-function engine admission, and guest behavior require root observation; this plain private u32 bitmap makes no atomicity, concurrency, ordering, or performance claim",
        "artifacts": {f: profile.file_record(output / f) for f in ["kernel.c", "kernel.obj", "kernel.text.bin", "kernel.disasm.txt"]},
    }
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(output / "receipt.json")


if __name__ == "__main__":
    main()
