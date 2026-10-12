#!/usr/bin/env python3
"""freeze this one scalar switch and its bounded coff dir32 jump table."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys


DEPENDENCY = Path(__file__).resolve().parents[5] / "engine/tests/programs/fixtures/p2-compiled-records/build.py"
DEPENDENCY_SHA256 = "fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258"
PREFERRED_BASE = 0x400000
SECTION_RVAS = {".text": 0x2000, ".rdata": 0x4000}
SECTION_FLAGS = {
    ".text": 0x60500020,
    ".data": 0xC0300040,
    ".bss": 0xC0300080,
    ".rdata": 0x40300040,
    ".debug$S": 0x42300040,
}
SOURCE = "https://learn.microsoft.com/en-us/windows/win32/debug/pe-format"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def json_sha256(value):
    return sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode())


def load_profile():
    data = DEPENDENCY.read_bytes()
    if sha256(data) != DEPENDENCY_SHA256:
        raise RuntimeError("readonly builder differs from the frozen compiler profile")
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("ring3_switch_profile", DEPENDENCY)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load the fixed compiler profile")
    profile = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile)
    if DEPENDENCY.read_bytes() != data:
        raise RuntimeError("compiler profile changed during import")
    return profile


def extract_switch(data):
    """link only the observed section-symbol dir32 fields at the preferred base."""
    def span(offset, size, label):
        if offset < 0 or size < 0 or offset + size > len(data):
            raise ValueError(f"{label} lies outside the COFF object")
        return data[offset:offset + size]

    machine, count, timestamp, symbols_at, symbol_count, optional, characteristics = (
        struct.unpack("<HHIIIHH", span(0, 20, "COFF header"))
    )
    if (machine, count, timestamp, optional, characteristics) != (0x14C, 5, 0, 0, 0):
        raise ValueError("expected the fixed ordinary reproducible i386 COFF profile")
    section_end = 20 + count * 40
    span(20, count * 40, "section table")
    if symbols_at < section_end or symbol_count != 14:
        raise ValueError("unexpected or overlapping symbol table")
    span(symbols_at, symbol_count * 18, "symbol table")
    strings_at = symbols_at + symbol_count * 18
    strings_size = struct.unpack("<I", span(strings_at, 4, "string table size"))[0]
    if strings_size < 4 or strings_at + strings_size != len(data):
        raise ValueError("invalid string table or trailing COFF data")
    strings = span(strings_at, strings_size, "string table")

    sections = []
    occupied = []
    for index, expected_name in enumerate(SECTION_FLAGS, 1):
        raw_name, physical, virtual, size, raw_at, reloc_at, line_at, reloc_count, lines, flags = (
            struct.unpack("<8sIIIIIIHHI", span(20 + (index - 1) * 40, 40, "section"))
        )
        if raw_name != expected_name.encode().ljust(8, b"\0"):
            raise ValueError("unexpected section name or section order")
        if physical != 0 or virtual != 0 or flags != SECTION_FLAGS[expected_name]:
            raise ValueError("unsupported section flags, COMDAT or address fields")
        expected_relocations = {".text": 1, ".rdata": 4}.get(expected_name, 0)
        if reloc_count != expected_relocations or line_at != 0 or lines != 0:
            raise ValueError("unexpected relocation count or line records")
        if bool(reloc_at) != bool(reloc_count):
            raise ValueError("unexpected relocation pointer")
        if expected_name == ".text" and not 27 <= size <= 4096:
            raise ValueError("expected one small nonempty text section")
        if expected_name == ".rdata" and size != 16:
            raise ValueError("expected one four-DWORD readonly jump table")
        if expected_name in (".data", ".bss") and size != 0:
            raise ValueError("mutable or uninitialized static data is outside this fixture")
        if expected_name == ".debug$S" and size != 100:
            raise ValueError("unexpected discarded compiler metadata extent")
        if not size and raw_at != 0 and not section_end <= raw_at <= symbols_at:
            raise ValueError("empty section pointer lies outside section data")
        for at, length, kind in [(raw_at, size, "raw"), (reloc_at, reloc_count * 10, "relocations")]:
            if not length:
                continue
            if at < section_end or at + length > symbols_at:
                raise ValueError(f"{expected_name} {kind} overlaps COFF metadata")
            span(at, length, f"{expected_name} {kind}")
            if any(at < end and start < at + length for start, end in occupied):
                raise ValueError("overlapping raw or relocation spans")
            occupied.append((at, at + length))
        section = {"index": index, "name": expected_name, "bytes": size,
                   "raw_offset": raw_at, "relocations_offset": reloc_at,
                   "relocation_count": reloc_count, "characteristics": flags,
                   "loaded": expected_name in SECTION_RVAS}
        section["raw_sha256"] = sha256(span(raw_at, size, "section bytes"))
        sections.append(section)
    by_name = {section["name"]: section for section in sections}

    def symbol_name(raw):
        if raw[:4] == b"\0" * 4:
            offset = struct.unpack("<I", raw[4:])[0]
            if not 4 <= offset < len(strings):
                raise ValueError("symbol name offset lies outside string table")
            end = strings.find(b"\0", offset)
            if end < 0:
                raise ValueError("unterminated symbol name")
            return strings[offset:end].decode("ascii")
        name, _, padding = raw.partition(b"\0")
        if any(padding):
            raise ValueError("nonzero short symbol name padding")
        return name.decode("ascii")

    symbols = {}
    names = set()
    index = 0
    while index < symbol_count:
        raw_name, value, section, kind, storage, auxiliaries = struct.unpack(
            "<8sIhHBB", span(symbols_at + index * 18, 18, "symbol")
        )
        if index + auxiliaries >= symbol_count:
            raise ValueError("auxiliary symbols exceed the symbol table")
        name = symbol_name(raw_name)
        if name in names:
            raise ValueError("duplicate primary symbol")
        names.add(name)
        if name in by_name:
            item = by_name[name]
            if (value, section, kind, storage, auxiliaries) != (0, item["index"], 0, 3, 1):
                raise ValueError("unexpected section symbol")
            length, relocations, lines, checksum, number, selection, unused = struct.unpack(
                "<IHHIHB3s", span(symbols_at + (index + 1) * 18, 18, "section auxiliary")
            )
            if (length, relocations, lines, number, selection, unused) != (
                item["bytes"], item["relocation_count"], 0, item["index"], 0, b"\0" * 3
            ):
                raise ValueError("section auxiliary disagrees with its section")
        elif name == "_ring3_switch":
            if (value, section, kind, storage, auxiliaries) != (0, 1, 0x20, 2, 0):
                raise ValueError("unexpected function entry or external definition")
        elif name == "@feat.00":
            if (value, section, kind, storage, auxiliaries) != (1, -1, 0, 3, 0):
                raise ValueError("unexpected compiler feature symbol")
        elif name == ".file":
            if (value, section, kind, storage, auxiliaries) != (0, -2, 0, 103, 1):
                raise ValueError("unexpected file symbol")
            if span(symbols_at + (index + 1) * 18, 18, "file auxiliary") != b"kernel.c\0".ljust(18, b"\0"):
                raise ValueError("unexpected source file auxiliary")
        else:
            raise ValueError("undefined or unsupported primary symbol")
        symbols[index] = {"index": index, "name": name, "value": value,
                          "section": section, "type": kind, "storage": storage,
                          "auxiliaries": auxiliaries}
        index += 1 + auxiliaries
    if names != {*SECTION_FLAGS, "_ring3_switch", "@feat.00", ".file"}:
        raise ValueError("missing required section, function or compiler symbol")

    linked = {name: bytearray(span(by_name[name]["raw_offset"], by_name[name]["bytes"], name))
              for name in SECTION_RVAS}
    relocations = []
    for name, fields, target_name in [(".text", [23], ".rdata"), (".rdata", [0, 4, 8, 12], ".text")]:
        section = by_name[name]
        raw = span(section["raw_offset"], section["bytes"], name)
        for ordinal in range(section["relocation_count"]):
            at, symbol_index, kind = struct.unpack(
                "<IIH", span(section["relocations_offset"] + ordinal * 10, 10, "relocation")
            )
            if kind != 6 or at != fields[ordinal] or at + 4 > len(raw):
                raise ValueError("unsupported, duplicate or out-of-range DIR32 field")
            symbol = symbols.get(symbol_index)
            if symbol is None or symbol["name"] != target_name:
                raise ValueError("relocation must target the admitted primary section symbol")
            addend = struct.unpack_from("<I", raw, at)[0]
            target_offset = symbol["value"] + addend
            if target_offset >= by_name[target_name]["bytes"] or (name == ".text" and target_offset != 0):
                raise ValueError("relocation addend lies outside its admitted target")
            value = PREFERRED_BASE + SECTION_RVAS[target_name] + target_offset
            if value > 0xFFFFFFFF:
                raise ValueError("linked DIR32 VA overflows a DWORD")
            struct.pack_into("<I", linked[name], at, value)
            relocations.append({"source_section": name, "source_offset": at,
                                "source_rva": SECTION_RVAS[name] + at,
                                "target_symbol_index": symbol_index, "target_symbol": target_name,
                                "target_section_rva": SECTION_RVAS[target_name],
                                "symbol_value": symbol["value"], "raw_addend": addend,
                                "target_offset": target_offset, "linked_va": value,
                                "pe_type": 3, "pe_highlow_rva": SECTION_RVAS[name] + at})
    targets = [row["target_offset"] for row in relocations if row["source_section"] == ".rdata"]
    if targets != [27, 43, 32, 37]:
        raise ValueError("expected the original selector-ordered instruction-start targets")
    coff = {
        "machine": machine, "timestamp": timestamp, "characteristics": characteristics,
        "sections": sections, "symbol_count": symbol_count, "symbols": list(symbols.values()),
        "external_symbols": ["_ring3_switch"], "relocations": relocations,
        "layout": {"preferred_base": PREFERRED_BASE, "section_rvas": SECTION_RVAS},
        "function": {"symbol": "_ring3_switch", "entry_offset": 0,
                     "extent_end_offset": by_name[".text"]["bytes"],
                     "extent_basis": "single function through the complete original text section"},
        "table": {"entries": 4, "entry_bytes": 4, "target_offsets": targets},
        "discarded_metadata": ["empty .data", "empty .bss", "nonloaded .debug$S compiler metadata"],
        "specification": SOURCE,
    }
    return bytes(linked[".text"]), bytes(linked[".rdata"]), coff


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_dir", type=Path, help="new directory for original and linked artifacts")
    args = parser.parse_args()
    profile = load_profile()
    source, builder = Path(__file__).resolve().with_name("kernel.c"), Path(__file__).resolve()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    snapshots = {"source": (source, "kernel.c"), "builder": (builder, "build.py"),
                 "dependency": (DEPENDENCY, "compiled-records-build.py")}
    inputs = {}
    for name, (path, filename) in snapshots.items():
        data = path.read_bytes()
        inputs[name] = {"path": str(path), "file": path.name, "bytes": len(data), "sha256": sha256(data)}
        (output / filename).write_bytes(data)
    if inputs["dependency"]["sha256"] != DEPENDENCY_SHA256:
        raise RuntimeError("compiler profile changed before its input snapshot")
    compiler_hash, objdump_hash = sha256(profile.CLANG.read_bytes()), sha256(profile.OBJDUMP.read_bytes())
    flags, environment = list(profile.FLAGS), dict(profile.ENVIRONMENT)
    commands = {
        "compiler_version": [str(profile.CLANG), "--version"],
        "objdump_version": [str(profile.OBJDUMP), "--version"],
        "compile": [str(profile.CLANG), *flags, "-c", "kernel.c", "-o", "kernel.obj"],
        "disassemble": [str(profile.OBJDUMP), "--disassemble", "--section=.text",
                        "--x86-asm-syntax=intel", "kernel.obj"],
        "relocations": [str(profile.OBJDUMP), "-r", "kernel.obj"],
    }
    compiler_version = profile.run(commands["compiler_version"], output, "compiler-version", environment)
    objdump_version = profile.run(commands["objdump_version"], output, "objdump-version", environment)
    profile.run(commands["compile"], output, "compile", environment)
    original = (output / "kernel.obj").read_bytes()
    text, table, coff = extract_switch(original)
    disassembly = profile.run(commands["disassemble"], output, "disassemble", environment)
    relocation_dump = profile.run(commands["relocations"], output, "relocations", environment)
    artifacts = {"kernel.text.bin": text, "kernel.rdata.bin": table,
                 "kernel.disasm.txt": disassembly.encode(), "kernel.coff-relocations.txt": relocation_dump.encode(),
                 "kernel.relocations.json": (json.dumps({"layout": coff["layout"],
                    "relocations": coff["relocations"], "table": coff["table"]}, indent=2, sort_keys=True) + "\n").encode()}
    for section in coff["sections"]:
        if section["loaded"]:
            artifacts[f"kernel{section['name']}.raw.bin"] = original[
                section["raw_offset"]:section["raw_offset"] + section["bytes"]]
    compiler = profile.tool_record(profile.CLANG, compiler_version)
    objdump = profile.tool_record(profile.OBJDUMP, objdump_version)
    if compiler["sha256"] != compiler_hash or objdump["sha256"] != objdump_hash:
        raise RuntimeError("a compiler tool changed during the build")
    for name, (path, filename) in snapshots.items():
        for observed in [path.read_bytes(), (output / filename).read_bytes()]:
            if len(observed) != inputs[name]["bytes"] or sha256(observed) != inputs[name]["sha256"]:
                raise RuntimeError(f"{name} changed during the build")
    for filename, data in artifacts.items():
        (output / filename).write_bytes(data)
    all_files = ["kernel.obj", *artifacts, *(filename for _, filename in snapshots.values())]
    receipt = {"schema": "ring3.compiled-switch.v1", **inputs,
               "compiler": compiler, "objdump": objdump, "commands": commands,
               "command_cwd": str(output), "flags": flags, "environment": environment,
               "profile": {"flags_sha256": json_sha256(flags),
                           "environment_sha256": json_sha256(environment),
                           "commands_sha256": json_sha256(commands),
                           "hash_encoding": "utf8 JSON, sorted keys, compact separators"},
               "coff": coff, "artifacts": {name: profile.file_record(output / name) for name in all_files}}
    receipt_path = output / "receipt.json"
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(receipt_path)


if __name__ == "__main__":
    main()
