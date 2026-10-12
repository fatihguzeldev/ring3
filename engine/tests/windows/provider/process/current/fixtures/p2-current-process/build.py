#!/usr/bin/env python3
"""freeze one original i686 caller and its four explicit imported iat fields."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys


DEPENDENCY = Path(__file__).resolve().parents[8] / "engine/tests/programs/records/fixtures/p2-compiled-records/build.py"
DEPENDENCY_SHA256 = "fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258"
PREFERRED_BASE = 0x400000
TEXT_RVA = 0x2000
SECTION_FLAGS = {".text": 0x60500020, ".data": 0xC0300040,
                 ".bss": 0xC0300080, ".debug$S": 0x42300040}
IAT_FIELDS = [(4, "__imp__GetCurrentProcess@0", "GetCurrentProcess", 0x3180),
              (17, "__imp__SetLastError@4", "SetLastError", 0x3184),
              (28, "__imp__FlushInstructionCache@12", "FlushInstructionCache", 0x3188),
              (36, "__imp__GetLastError@0", "GetLastError", 0x318C)]
ORIGINAL_TEXT = bytes.fromhex(
    "5756ff150000000089c668efcdab89ff15000000006aff6a0056ff1500000000"
    "89c7ff1500000000f7d683f70109f735efcdab8931c909f8b82a0000000f45c15e5fc3")
SPECIFICATION = "https://learn.microsoft.com/en-us/windows/win32/debug/pe-format"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def json_sha256(value):
    return sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode())


def load_profile():
    data = DEPENDENCY.read_bytes()
    if sha256(data) != DEPENDENCY_SHA256:
        raise RuntimeError("readonly compiler profile differs from its frozen source")
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("ring3_current_process_profile", DEPENDENCY)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load the fixed compiler profile")
    profile = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile)
    if DEPENDENCY.read_bytes() != data:
        raise RuntimeError("compiler profile changed during import")
    return profile


def extract_current_process(data):
    """resolve only the four observed external iat dir32 fields, without code rewriting."""
    def span(offset, size, label):
        if offset < 0 or size < 0 or offset + size > len(data):
            raise ValueError(f"{label} lies outside the COFF object")
        return data[offset:offset + size]

    machine, count, timestamp, symbols_at, symbol_count, optional, characteristics = (
        struct.unpack("<HHIIIHH", span(0, 20, "COFF header")))
    if (machine, count, timestamp, optional, characteristics) != (0x14C, 4, 0, 0, 0):
        raise ValueError("expected the fixed reproducible ordinary i386 COFF profile")
    section_end = 20 + count * 40
    span(20, count * 40, "section table")
    if symbol_count != 16 or symbols_at < section_end:
        raise ValueError("unexpected or overlapping symbol table")
    span(symbols_at, symbol_count * 18, "symbol table")
    strings_at = symbols_at + symbol_count * 18
    strings_size = struct.unpack("<I", span(strings_at, 4, "string length"))[0]
    if strings_size < 4 or strings_at + strings_size != len(data):
        raise ValueError("invalid string table or trailing COFF bytes")
    strings = span(strings_at, strings_size, "string table")
    sections, occupied = [], []
    for index, name in enumerate(SECTION_FLAGS, 1):
        fields = struct.unpack("<8sIIIIIIHHI", span(20 + (index - 1) * 40, 40, "section"))
        raw_name, physical, virtual, size, raw_at, reloc_at, line_at, reloc_count, lines, flags = fields
        if raw_name != name.encode().ljust(8, b"\0"):
            raise ValueError("unexpected section name or order")
        if (physical, virtual, flags) != (0, 0, SECTION_FLAGS[name]):
            raise ValueError("unsupported section address, characteristics or COMDAT")
        expected_size = {".text": 67, ".debug$S": 100}.get(name, 0)
        if size != expected_size or reloc_count != (4 if name == ".text" else 0):
            raise ValueError("unexpected section extent or relocation count")
        if lines != 0 or line_at != 0 or bool(reloc_at) != bool(reloc_count):
            raise ValueError("unexpected line record or relocation pointer")
        if not size and raw_at != 0 and not section_end <= raw_at <= symbols_at:
            raise ValueError("empty section pointer lies outside section data")
        for at, length in [(raw_at, size), (reloc_at, reloc_count * 10)]:
            if not length:
                continue
            if at < section_end or at + length > symbols_at:
                raise ValueError("raw or relocation span overlaps COFF metadata")
            span(at, length, "section data")
            if any(at < end and start < at + length for start, end in occupied):
                raise ValueError("overlapping raw or relocation spans")
            occupied.append((at, at + length))
        sections.append({"index": index, "name": name, "bytes": size, "raw_offset": raw_at,
                         "relocations_offset": reloc_at, "relocation_count": reloc_count,
                         "characteristics": flags, "loaded": name == ".text",
                         "raw_sha256": sha256(span(raw_at, size, "section bytes"))})
    by_name = {section["name"]: section for section in sections}

    def symbol_name(raw):
        if raw[:4] == b"\0" * 4:
            at = struct.unpack("<I", raw[4:])[0]
            if not 4 <= at < len(strings):
                raise ValueError("symbol name offset lies outside the string table")
            end = strings.find(b"\0", at)
            if end < 0:
                raise ValueError("unterminated symbol name")
            try:
                return strings[at:end].decode("ascii")
            except UnicodeDecodeError as error:
                raise ValueError("non-ASCII symbol") from error
        name, _, padding = raw.partition(b"\0")
        if any(padding):
            raise ValueError("nonzero short symbol name padding")
        try:
            return name.decode("ascii")
        except UnicodeDecodeError as error:
            raise ValueError("non-ASCII symbol") from error

    symbols, names, index = {}, set(), 0
    imports = {item[1] for item in IAT_FIELDS}
    while index < symbol_count:
        raw, value, section, kind, storage, aux = struct.unpack(
            "<8sIhHBB", span(symbols_at + index * 18, 18, "symbol"))
        name = symbol_name(raw)
        if index + aux >= symbol_count or name in names:
            raise ValueError("duplicate symbol or auxiliary extent overflow")
        names.add(name)
        shape = (value, section, kind, storage, aux)
        if name in by_name:
            item = by_name[name]
            if shape != (0, item["index"], 0, 3, 1):
                raise ValueError("unexpected section symbol")
            length, relocs, lines, checksum, number, selection, unused = struct.unpack(
                "<IHHIHB3s", span(symbols_at + (index + 1) * 18, 18, "section auxiliary"))
            if (length, relocs, lines, number, selection, unused) != (
                item["bytes"], item["relocation_count"], 0, item["index"], 0, b"\0" * 3):
                raise ValueError("section auxiliary disagrees with its section")
        elif name == "_ring3_current_process":
            if shape != (0, 1, 0x20, 2, 0):
                raise ValueError("unexpected function symbol or entry")
        elif name in imports:
            if shape != (0, 0, 0, 2, 0):
                raise ValueError("import must be an undefined external data symbol")
        elif name == "@feat.00":
            if shape != (1, -1, 0, 3, 0):
                raise ValueError("unexpected compiler feature symbol")
        elif name == ".file":
            if shape != (0, -2, 0, 103, 1) or span(
                    symbols_at + (index + 1) * 18, 18, "file auxiliary") != b"kernel.c\0".ljust(18, b"\0"):
                raise ValueError("unexpected source file symbol")
        else:
            raise ValueError("unsupported or unresolved external symbol")
        symbols[index] = {"index": index, "name": name, "value": value, "section": section,
                          "type": kind, "storage": storage, "auxiliaries": aux}
        index += 1 + aux
    if names != {*SECTION_FLAGS, *imports, "_ring3_current_process", "@feat.00", ".file"}:
        raise ValueError("missing required compiler, entry or import symbol")
    section = by_name[".text"]
    raw = span(section["raw_offset"], section["bytes"], "original text")
    linked, relocations = bytearray(raw), []
    for ordinal, (offset, expected_symbol, api, iat_rva) in enumerate(IAT_FIELDS):
        at, symbol_index, kind = struct.unpack("<IIH", span(
            section["relocations_offset"] + ordinal * 10, 10, "relocation"))
        symbol = symbols.get(symbol_index)
        if kind != 6 or at != offset or at + 4 > len(raw):
            raise ValueError("unexpected, overlapping or non-DIR32 field")
        if symbol is None or symbol["name"] != expected_symbol:
            raise ValueError("relocation must name its exact primary imported IAT symbol")
        addend = struct.unpack_from("<I", raw, at)[0]
        if addend != 0 or raw[at - 2:at] != b"\xff\x15":
            raise ValueError("unexpected IAT addend or indirect call encoding")
        value = PREFERRED_BASE + iat_rva
        struct.pack_into("<I", linked, at, value)
        relocations.append({"source_section": ".text", "source_offset": at,
                            "source_rva": TEXT_RVA + at, "target_symbol_index": symbol_index,
                            "target_symbol": expected_symbol, "api": api, "iat_rva": iat_rva,
                            "raw_addend": addend, "linked_va": value,
                            "pe_type": 3, "pe_highlow_rva": TEXT_RVA + at})
    if raw != ORIGINAL_TEXT:
        raise ValueError("original instructions differ from the admitted fixed compiler artifact")
    coff = {"machine": machine, "timestamp": timestamp, "characteristics": characteristics,
            "sections": sections, "symbol_count": symbol_count, "symbols": list(symbols.values()),
            "imports": [row[2] for row in IAT_FIELDS], "relocations": relocations,
            "layout": {"preferred_base": PREFERRED_BASE, "text_rva": TEXT_RVA},
            "function": {"symbol": "_ring3_current_process", "entry_offset": 0,
                         "extent_end_offset": len(raw), "extent_basis": "complete original text section"},
            "discarded_metadata": ["empty .data", "empty .bss", "nonloaded .debug$S"],
            "specification": SPECIFICATION}
    return bytes(linked), coff


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_dir", type=Path, help="new directory for original and linked artifacts")
    args = parser.parse_args()
    profile = load_profile()
    source, builder = Path(__file__).with_name("kernel.c").resolve(), Path(__file__).resolve()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    snapshots = {"source": (source, "kernel.c"), "builder": (builder, "build.py"),
                 "checker": (builder.with_name("check_coff.py"), "check_coff.py"),
                 "dependency": (DEPENDENCY, "compiled-records-build.py")}
    inputs = {}
    for name, (path, filename) in snapshots.items():
        data = path.read_bytes()
        inputs[name] = {"path": str(path), "bytes": len(data), "sha256": sha256(data)}
        (output / filename).write_bytes(data)
    if inputs["dependency"]["sha256"] != DEPENDENCY_SHA256:
        raise RuntimeError("compiler profile changed before snapshot")
    tool_hashes = [sha256(path.read_bytes()) for path in [profile.CLANG, profile.OBJDUMP]]
    flags, environment = list(profile.FLAGS), dict(profile.ENVIRONMENT)
    commands = {"compiler_version": [str(profile.CLANG), "--version"],
                "objdump_version": [str(profile.OBJDUMP), "--version"],
                "compile": [str(profile.CLANG), *flags, "-c", "kernel.c", "-o", "kernel.obj"],
                "disassemble": [str(profile.OBJDUMP), "-d", "-r", "kernel.obj"]}
    compiler_version = profile.run(commands["compiler_version"], output, "compiler-version", environment)
    objdump_version = profile.run(commands["objdump_version"], output, "objdump-version", environment)
    profile.run(commands["compile"], output, "compile", environment)
    original = (output / "kernel.obj").read_bytes()
    text, coff = extract_current_process(original)
    disassembly = profile.run(commands["disassemble"], output, "disassemble", environment)
    raw = original[coff["sections"][0]["raw_offset"]:coff["sections"][0]["raw_offset"] + len(text)]
    artifacts = {"kernel.text.bin": text, "kernel.text.raw.bin": raw,
                 "kernel.disasm.txt": disassembly.encode(),
                 "kernel.relocations.json": (json.dumps({"layout": coff["layout"],
                    "relocations": coff["relocations"]}, indent=2, sort_keys=True) + "\n").encode()}
    compiler, objdump = profile.tool_record(profile.CLANG, compiler_version), profile.tool_record(profile.OBJDUMP, objdump_version)
    if [compiler["sha256"], objdump["sha256"]] != tool_hashes:
        raise RuntimeError("a tool changed during the build")
    for name, (path, filename) in snapshots.items():
        for data in [path.read_bytes(), (output / filename).read_bytes()]:
            if len(data) != inputs[name]["bytes"] or sha256(data) != inputs[name]["sha256"]:
                raise RuntimeError(f"{name} changed during the build")
    for name, data in artifacts.items():
        (output / name).write_bytes(data)
    names = ["kernel.obj", *artifacts, *(filename for _, filename in snapshots.values())]
    receipt = {"schema": "ring3.compiled-current-process.v1", **inputs,
               "compiler": compiler, "objdump": objdump, "commands": commands,
               "command_cwd": str(output), "flags": flags, "environment": environment,
               "profile": {"flags_sha256": json_sha256(flags),
                           "environment_sha256": json_sha256(environment),
                           "commands_sha256": json_sha256(commands)},
               "coff": coff, "artifacts": {name: profile.file_record(output / name) for name in names}}
    receipt_path = output / "receipt.json"
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(receipt_path)


if __name__ == "__main__":
    main()
