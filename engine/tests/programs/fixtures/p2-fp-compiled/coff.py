"""extract one whole original i386 function under the fp fixture's COFF policy."""

import struct


def extract_text(data):
    def span(offset, size, label):
        if offset < 0 or size < 0 or offset + size > len(data):
            raise ValueError(f"{label} lies outside the COFF object")
        return data[offset:offset + size]

    def name8(raw):
        name, separator, padding = raw.partition(b"\0")
        if separator and padding.strip(b"\0"):
            raise ValueError("nonzero padding in a short COFF name")
        if not name:
            raise ValueError("empty COFF name")
        return name.decode("ascii", errors="strict")

    machine, count, timestamp, symbols_at, symbol_count, optional, characteristics = (
        struct.unpack("<HHIIIHH", span(0, 20, "header"))
    )
    if machine != 0x14C or optional or characteristics or not 1 <= count <= 4:
        raise ValueError("expected a small ordinary i386 COFF object")
    if timestamp:
        raise ValueError("expected a reproducible zero timestamp")
    table_end = 20 + 40 * count
    span(20, 40 * count, "section table")
    if symbols_at < table_end or not symbol_count:
        raise ValueError("missing or overlapping symbol table")
    span(symbols_at, 18 * symbol_count, "symbol table")
    strings_at = symbols_at + 18 * symbol_count
    strings_size = struct.unpack("<I", span(strings_at, 4, "string table size"))[0]
    if strings_size < 4 or strings_at + strings_size != len(data):
        raise ValueError("invalid string table extent or trailing COFF data")
    strings = span(strings_at, strings_size, "string table")
    long_names = {}
    cursor = 4
    while cursor < len(strings):
        end = strings.find(b"\0", cursor)
        if end < 0 or end == cursor:
            raise ValueError("invalid string table name")
        long_names[cursor] = strings[cursor:end].decode("ascii", errors="strict")
        cursor = end + 1
    used_names = set()

    def symbol_name(raw):
        if raw[:4] != b"\0" * 4:
            return name8(raw)
        offset = struct.unpack("<I", raw[4:])[0]
        if offset not in long_names:
            raise ValueError("symbol name is not at a string table boundary")
        used_names.add(offset)
        return long_names[offset]

    sections = []
    raw_spans = []
    for index in range(count):
        fields = struct.unpack("<8sIIIIIIHHI", span(20 + 40 * index, 40, "section"))
        name = name8(fields[0])
        virtual_size, virtual_at, size, raw_at, reloc_at, lines_at, reloc, lines, flags = fields[1:]
        if name not in (".text", ".data", ".bss", ".debug$S"):
            raise ValueError(f"section {name} is outside this fixture")
        if any(section["name"] == name for section in sections):
            raise ValueError("duplicate section")
        if virtual_size or virtual_at or reloc_at or lines_at or reloc or lines:
            raise ValueError(f"unexpected virtual, relocation or line metadata in {name}")
        if name in (".data", ".bss") and size:
            raise ValueError("nonempty data dependencies are outside this fixture")
        if bool(flags & 0x20) != (name == ".text") or (name != ".text" and flags & 0x20000000):
            raise ValueError("only .text may contain executable code")
        if size:
            if raw_at < table_end or raw_at + size > symbols_at:
                raise ValueError("raw section overlaps COFF metadata")
            span(raw_at, size, "raw section")
            raw_spans.append((raw_at, raw_at + size))
        elif raw_at and not table_end <= raw_at <= symbols_at:
            raise ValueError("invalid empty section pointer")
        sections.append({"index": index + 1, "name": name, "bytes": size,
                         "raw_offset": raw_at, "characteristics": flags})
    cursor = table_end
    for start, end in sorted(raw_spans):
        if start != cursor:
            raise ValueError("overlapping raw sections or unexplained raw-data gap")
        cursor = end
    if cursor != symbols_at:
        raise ValueError("unexplained data before symbol table")
    texts = [section for section in sections if section["name"] == ".text"]
    if len(texts) != 1 or not texts[0]["bytes"]:
        raise ValueError("expected one nonempty .text section")
    text = texts[0]

    # only compiler identification records may occupy the discarded debug section.
    for section in sections:
        if section["name"] != ".debug$S" or not section["bytes"]:
            continue
        debug = span(section["raw_offset"], section["bytes"], "debug section")
        if len(debug) < 12 or struct.unpack_from("<II", debug) != (4, 0xF1):
            raise ValueError("unexpected CodeView debug metadata")
        size = struct.unpack_from("<I", debug, 8)[0]
        if 12 + size != len(debug):
            raise ValueError("unexpected trailing debug metadata")
        cursor = 12
        kinds = []
        while cursor < len(debug):
            if cursor + 4 > len(debug):
                raise ValueError("truncated debug record")
            length, kind = struct.unpack_from("<HH", debug, cursor)
            if length < 2 or cursor + 2 + length > len(debug):
                raise ValueError("invalid debug record extent")
            if kind not in (0x1101, 0x113C) or kind in kinds:
                raise ValueError("unexpected debug record dependency")
            fixed = 4 if kind == 0x1101 else 22
            payload = debug[cursor + 4:cursor + 2 + length]
            if len(payload) <= fixed:
                raise ValueError("truncated compiler identification payload")
            name = payload[fixed:]
            end = name.find(b"\0")
            if end < 0 or name[end:].strip(b"\0"):
                raise ValueError("unterminated compiler identification or nonzero padding")
            name[:end].decode("ascii", errors="strict")
            kinds.append(kind)
            cursor += 2 + length
        if kinds != [0x1101, 0x113C]:
            raise ValueError("missing compiler identification records")

    defined = []
    undefined = []
    section_definitions = set()
    metadata_names = set()
    index = 0
    while index < symbol_count:
        raw_name, value, section, kind, storage, auxiliary = struct.unpack(
            "<8sIhHBB", span(symbols_at + 18 * index, 18, "symbol")
        )
        if index + auxiliary >= symbol_count:
            raise ValueError("auxiliary symbols exceed symbol table")
        name = symbol_name(raw_name)
        if storage == 2 and section == 0:
            if name != "__fltused" or value or kind or auxiliary or undefined:
                raise ValueError(f"unresolved external symbol {name} is outside marker policy")
            undefined.append(name)
        elif storage == 2:
            if name != "_ring3_reduce" or section != text["index"] or value or kind != 0x20 or auxiliary:
                raise ValueError("unexpected defined external function")
            defined.append(name)
        elif storage == 3 and 1 <= section <= len(sections):
            owner = sections[section - 1]
            if name != owner["name"] or value or kind or auxiliary != 1 or section in section_definitions:
                raise ValueError("unexpected section symbol")
            aux = span(symbols_at + 18 * (index + 1), 18, "section auxiliary")
            length, reloc, lines, checksum, number, selection, reserved, high = struct.unpack("<IHHIHBBH", aux)
            if (length, reloc, lines, number, selection, reserved, high) != (owner["bytes"], 0, 0, section, 0, 0, 0):
                raise ValueError("section auxiliary disagrees with section")
            section_definitions.add(section)
        elif name == "@feat.00" and (value, section, kind, storage, auxiliary) == (1, -1, 0, 3, 0):
            if name in metadata_names:
                raise ValueError("duplicate feature metadata")
            metadata_names.add(name)
        elif name == ".file" and (value, section, kind, storage, auxiliary) == (0, -2, 0, 103, 1):
            if name in metadata_names or name8(span(symbols_at + 18 * (index + 1), 18, "file auxiliary")) != "kernel.c":
                raise ValueError("unexpected file metadata")
            metadata_names.add(name)
        else:
            raise ValueError(f"symbol {name} is outside this fixture")
        index += 1 + auxiliary
    if defined != ["_ring3_reduce"] or section_definitions != set(range(1, count + 1)):
        raise ValueError("expected one whole external function and canonical section symbols")
    if used_names != set(long_names):
        raise ValueError("unreferenced trailing string metadata")
    return span(text["raw_offset"], text["bytes"], ".text"), {
        "machine": machine, "timestamp": timestamp, "characteristics": characteristics,
        "sections": sections, "symbol_count": symbol_count,
        "external_symbols": defined, "undefined_external_symbols": undefined,
        "relocations": 0,
        "marker_policy": "only one optional undefined external __fltused with zero value/type/aux; all sections have zero relocations; no linking or byte rewriting",
        "function": {"symbol": "_ring3_reduce", "entry_offset": 0,
                     "extent_end_offset": text["bytes"],
                     "extent_basis": "sole function through original .text section end"},
    }
