#!/usr/bin/env python3
"""build and freeze this one scalar i686 COFF leaf; routine tests use its bytes."""

import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess


CLANG = Path("/Library/Developer/CommandLineTools/usr/bin/clang")
OBJDUMP = Path("/Library/Developer/CommandLineTools/usr/bin/llvm-objdump")
FLAGS = [
    "--target=i686-pc-windows-msvc",
    "-std=c11",
    "-O2",
    "-ffreestanding",
    "-fno-builtin",
    "-fno-stack-protector",
    "-fno-vectorize",
    "-fno-slp-vectorize",
    "-fno-unroll-loops",
    "-falign-loops=1",
    "-mno-sse",
    "-mno-sse2",
    "-mno-mmx",
    "-mno-incremental-linker-compatible",
    "-fno-unwind-tables",
    "-fno-asynchronous-unwind-tables",
    "-fno-addrsig",
]
ENVIRONMENT = {
    "PATH": "/usr/bin:/bin",
    "LC_ALL": "C",
    "TZ": "UTC",
    "SOURCE_DATE_EPOCH": "0",
}


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def file_record(path):
    data = path.read_bytes()
    return {"file": path.name, "bytes": len(data), "sha256": sha256(data)}


def tool_record(path, version):
    return {
        "path": str(path),
        "resolved_path": str(path.resolve(strict=True)),
        "sha256": sha256(path.read_bytes()),
        "version": version,
    }


def run(argv, output, stem, environment):
    result = subprocess.run(argv, cwd=output, env=environment, capture_output=True)
    (output / f"{stem}.stdout.log").write_bytes(result.stdout)
    (output / f"{stem}.stderr.log").write_bytes(result.stderr)
    if result.returncode != 0:
        raise RuntimeError(f"{stem} failed with exit {result.returncode}; logs preserved")
    return result.stdout.decode("utf-8", errors="strict")


def extract_text(data):
    def checked_span(offset, size, label):
        if offset < 0 or size < 0 or offset + size > len(data):
            raise ValueError(f"{label} lies outside the COFF object")
        return data[offset : offset + size]

    header = checked_span(0, 20, "COFF header")
    machine, count, timestamp, symbols_at, symbol_count, optional_size, characteristics = (
        struct.unpack("<HHIIIHH", header)
    )
    if machine != 0x14C or optional_size != 0 or not 1 <= count <= 16:
        raise ValueError("expected a small ordinary i386 COFF object")
    if timestamp != 0:
        raise ValueError("expected reproducible zero COFF timestamp")
    section_table_end = 20 + 40 * count
    checked_span(20, 40 * count, "section table")
    if symbols_at < section_table_end or symbol_count == 0:
        raise ValueError("missing or overlapping symbol table")
    checked_span(symbols_at, 18 * symbol_count, "symbol table")
    strings_at = symbols_at + 18 * symbol_count
    strings_size = struct.unpack("<I", checked_span(strings_at, 4, "string table size"))[0]
    if strings_size < 4:
        raise ValueError("invalid COFF string table size")
    strings = checked_span(strings_at, strings_size, "string table")
    if strings_at + strings_size != len(data):
        raise ValueError("unexpected trailing COFF data")

    sections = []
    raw_spans = []
    for index in range(count):
        fields = struct.unpack("<8sIIIIIIHHI", checked_span(20 + 40 * index, 40, "section"))
        name = fields[0].rstrip(b"\0").decode("ascii", errors="strict")
        size, raw_at, relocations_at, lines_at, relocations, lines, flags = fields[3:]
        if name.startswith("/"):
            raise ValueError("long section names are outside this fixture")
        if relocations != 0 or lines != 0:
            raise ValueError(f"unexpected relocation or line records in {name}")
        if relocations_at != 0 or lines_at != 0:
            raise ValueError(f"unexpected relocation or line pointer in {name}")
        if size:
            if raw_at < section_table_end or raw_at + size > symbols_at:
                raise ValueError(f"raw section {name} overlaps COFF metadata")
            checked_span(raw_at, size, f"raw section {name}")
            for start, end in raw_spans:
                if raw_at < end and start < raw_at + size:
                    raise ValueError("overlapping raw sections")
            raw_spans.append((raw_at, raw_at + size))
        if name == ".drectve" and size:
            raise ValueError("linker directives are outside this leaf contract")
        sections.append({"index": index + 1, "name": name, "bytes": size,
                         "raw_offset": raw_at, "characteristics": flags})
    text_sections = [section for section in sections if section["name"] == ".text"]
    code_sections = [section for section in sections if section["characteristics"] & 0x20]
    if len(text_sections) != 1 or code_sections != text_sections or not text_sections[0]["bytes"]:
        raise ValueError("expected one nonempty .text code section")
    text = text_sections[0]

    def symbol_name(raw):
        if raw[:4] != b"\0" * 4:
            return raw.rstrip(b"\0").decode("ascii", errors="strict")
        offset = struct.unpack("<I", raw[4:])[0]
        if not 4 <= offset < len(strings):
            raise ValueError("symbol name offset outside string table")
        end = strings.find(b"\0", offset)
        if end == -1:
            raise ValueError("unterminated symbol name")
        return strings[offset:end].decode("ascii", errors="strict")

    functions = []
    external_symbols = []
    index = 0
    while index < symbol_count:
        raw_name, value, section, kind, storage, auxiliary_count = struct.unpack(
            "<8sIhHBB", checked_span(symbols_at + 18 * index, 18, "symbol")
        )
        if index + auxiliary_count >= symbol_count:
            raise ValueError("auxiliary symbols exceed symbol table")
        name = symbol_name(raw_name)
        if storage == 2:
            if section == 0:
                raise ValueError(f"unresolved external symbol {name}")
            external_symbols.append(name)
        if section == text["index"] and (kind & 0x30) == 0x20:
            functions.append({"symbol": name, "entry_offset": value, "storage": storage})
        index += 1 + auxiliary_count
    if external_symbols != ["_ring3_reduce"] or len(functions) != 1:
        raise ValueError("expected only the ring3_reduce external function")
    function = functions[0]
    if function["symbol"] != "_ring3_reduce" or function["storage"] != 2:
        raise ValueError("unexpected .text function symbol")
    if not 0 <= function["entry_offset"] < text["bytes"]:
        raise ValueError("function entry lies outside .text")
    function["extent_end_offset"] = text["bytes"]
    function["extent_basis"] = "single function through section end, including compiler padding"
    del function["storage"]
    return checked_span(text["raw_offset"], text["bytes"], ".text"), {
        "machine": machine, "timestamp": timestamp, "characteristics": characteristics,
        "sections": sections, "symbol_count": symbol_count,
        "external_symbols": external_symbols, "relocations": 0, "function": function,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_dir", type=Path, help="new directory for frozen artifacts")
    args = parser.parse_args()
    source = Path(__file__).resolve().with_name("kernel.c")
    builder = Path(__file__).resolve()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source_record = {"path": str(source), **file_record(source)}
    builder_record = {"path": str(builder), **file_record(builder)}
    compiler_hash = sha256(CLANG.read_bytes())
    objdump_hash = sha256(OBJDUMP.read_bytes())
    (output / "kernel.c").write_bytes(source.read_bytes())
    if file_record(output / "kernel.c")["sha256"] != source_record["sha256"]:
        raise RuntimeError("source changed while taking its input snapshot")
    commands = {
        "compiler_version": [str(CLANG), "--version"],
        "objdump_version": [str(OBJDUMP), "--version"],
        "compile": [str(CLANG), *FLAGS, "-c", "kernel.c", "-o", "kernel.obj"],
        "disassemble": [str(OBJDUMP), "--disassemble", "--section=.text",
                        "--x86-asm-syntax=intel", "kernel.obj"],
    }
    compiler_version = run(commands["compiler_version"], output, "compiler-version", ENVIRONMENT)
    objdump_version = run(commands["objdump_version"], output, "objdump-version", ENVIRONMENT)
    run(commands["compile"], output, "compile", ENVIRONMENT)
    object_path = output / "kernel.obj"
    text, coff = extract_text(object_path.read_bytes())
    (output / "kernel.text.bin").write_bytes(text)
    disassembly = run(commands["disassemble"], output, "disassemble", ENVIRONMENT)
    (output / "kernel.disasm.txt").write_text(disassembly, encoding="utf-8")
    compiler = tool_record(CLANG, compiler_version)
    objdump = tool_record(OBJDUMP, objdump_version)
    if compiler["sha256"] != compiler_hash or objdump["sha256"] != objdump_hash:
        raise RuntimeError("a compiler tool changed during the build")
    if file_record(source) != {key: source_record[key] for key in ("file", "bytes", "sha256")}:
        raise RuntimeError("source changed during the build")
    if file_record(builder) != {key: builder_record[key] for key in ("file", "bytes", "sha256")}:
        raise RuntimeError("builder changed during the build")
    receipt = {
        "schema": "ring3.compiled-records.v1",
        "source": source_record,
        "builder": builder_record,
        "compiler": compiler,
        "objdump": objdump,
        "commands": commands, "command_cwd": str(output), "environment": ENVIRONMENT,
        "record": {"bytes": 8, "offsets": {"signed_byte": 0, "tag": 1,
                    "signed_word": 2, "weight": 4, "marker": 6}},
        "coff": coff,
        "artifacts": {name: file_record(output / filename) for name, filename in [
            ("source_snapshot", "kernel.c"),
            ("object", "kernel.obj"), ("text", "kernel.text.bin"),
            ("disassembly", "kernel.disasm.txt")
        ]},
    }
    receipt_path = output / "receipt.json"
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(receipt_path)


if __name__ == "__main__":
    main()
