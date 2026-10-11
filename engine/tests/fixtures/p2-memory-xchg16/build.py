#!/usr/bin/env python3
"""freeze the fixed freestanding i686 C11 exchange leaf and compiler profile."""
import argparse
import ast
import hashlib
import importlib.util
import inspect
import json
from pathlib import Path
import sys

DEPENDENCY = Path(__file__).resolve().parent.parent / "p2-compiled-records" / "build.py"
DEPENDENCY_SHA256 = "fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258"

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
    spec = importlib.util.spec_from_file_location("ring3_exchange_word_profile", DEPENDENCY)
    profile = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile)
    extractor_source = inspect.getsource(profile.extract_text)
    if extractor_source.count('"_ring3_reduce"') != 2:
        raise RuntimeError("unexpected fixed extractor symbol checks")
    adapted = extractor_source.replace("ring3_reduce", "ring3_exchange_word")
    namespace = dict(vars(profile))
    exec(compile(ast.parse(adapted), "<fixed-exchange-symbol-extractor>", "exec"), namespace)
    source = Path(__file__).with_name("kernel.c")
    builder = Path(__file__)
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source_before, builder_before = source.read_bytes(), builder.read_bytes()
    compiler_before, objdump_before = profile.CLANG.read_bytes(), profile.OBJDUMP.read_bytes()
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
    text, coff = namespace["extract_text"]((output / "kernel.obj").read_bytes())
    if text.hex() != "8b4424040fb74c24086687080fb7c1c3":
        raise RuntimeError("original five-instruction word exchange leaf changed")
    original_object = (output / "kernel.obj").read_bytes()
    if len(original_object) != 537 or sha(original_object) != "82456689c709bf29ae1eda146e2b45b69257cfb56919c4baf9e935f7746395aa":
        raise RuntimeError("original fixed word exchange object changed")
    (output / "kernel.text.bin").write_bytes(text)
    disassembly = profile.run(commands["disassemble"], output, "disassemble", profile.ENVIRONMENT)
    (output / "kernel.disasm.txt").write_text(disassembly)
    assert source.read_bytes() == source_before and builder.read_bytes() == builder_before
    assert profile.CLANG.read_bytes() == compiler_before and profile.OBJDUMP.read_bytes() == objdump_before
    assert DEPENDENCY.read_bytes() == dependency
    receipt = {
        "schema": "ring3.word-memory-exchange-leaf.v1",
        "source": {"path": str(source.resolve()), **profile.file_record(source)},
        "builder": {"path": str(builder.resolve()), **profile.file_record(builder)},
        "dependency": {"path": str(DEPENDENCY), "sha256": DEPENDENCY_SHA256},
        "extractor_delta": "exact existing ordinary relocation-free i386 COFF parser; only fixed function symbol and diagnostic spelling ring3_reduce to ring3_exchange_word",
        "original_extractor_sha256": sha(extractor_source.encode()),
        "adapted_extractor_sha256": sha(adapted.encode()),
        "compiler": profile.tool_record(profile.CLANG, compiler_version),
        "objdump": profile.tool_record(profile.OBJDUMP, objdump_version),
        "commands": commands, "command_cwd": str(output), "environment": profile.ENVIRONMENT,
        "coff": coff,
        "contract": "one C11 seq_cst exchange builtin on one word, returning old unsigned word zeroextended; fixture proves finite synchronous engine execution, not C11 multithread ordering",
        "artifacts": {f: profile.file_record(output / f) for f in ["kernel.c", "kernel.obj", "kernel.text.bin", "kernel.disasm.txt"]},
    }
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(output / "receipt.json")

if __name__ == "__main__":
    main()
