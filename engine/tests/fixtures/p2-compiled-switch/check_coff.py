#!/usr/bin/env python3
"""check this fixture's exact original COFF object and independent malformed cases."""

import hashlib
from pathlib import Path
import runpy
import struct
import sys

sys.dont_write_bytecode = True


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: check_coff.py BUILD_OUTPUT/kernel.obj")
    original = Path(sys.argv[1]).read_bytes()
    expected = "c257d84de2faef79bec68e95a1fbfbbd05738b2ee7f2854fe3fcc4491f344d23"
    if hashlib.sha256(original).hexdigest() != expected:
        raise ValueError("expected the frozen fixed-profile original object")
    adapter = runpy.run_path(str(Path(__file__).with_name("build.py")))
    extract = adapter["extract_switch"]
    text, table, _ = extract(original)
    assert len(text) == 59 and len(table) == 16
    assert struct.unpack_from("<I", text, 23)[0] == 0x404000
    assert struct.unpack("<4I", table) == (0x40201B, 0x40202B, 0x402020, 0x402025)
    assert hashlib.sha256(text).hexdigest() == "62fea086adcfbea3941cc241f8eec89d3f660cdfcfc36563a1276717715051f7"
    assert hashlib.sha256(table).hexdigest() == "3b90c5c3e6b86c5d71982e8919a6d3824c2226234cacaf899f7d6484aba63cc9"

    cases = []

    def word(label, offset, value, width=4):
        data = bytearray(original)
        data[offset:offset + width] = value.to_bytes(width, "little")
        cases.append((label, bytes(data)))

    cases.extend((("truncated-header", original[:19]),
                  ("truncated-symbols", original[:600]),
                  ("trailing-data", original + b"x")))
    word("wrong-machine", 0, 0x8664, 2)
    word("nonzero-timestamp", 4, 1)
    word("optional-header", 16, 1, 2)
    word("section-table-overflow", 2, 0xFFFF, 2)
    word("symbol-table-overlaps-debug", 8, 420)
    word("string-table-overflow", 697, 0xFFFFFFFF)
    word("text-raw-extent-overflow", 20 + 16, 0xFFFFFFFF)
    word("rdata-overlaps-text", 20 + 3 * 40 + 20, 220)
    word("rdata-relocations-overlap-text", 20 + 3 * 40 + 24, 230)
    word("rdata-relocation-count", 20 + 3 * 40 + 32, 5, 2)
    word("writable-table", 20 + 3 * 40 + 36, 0xC0300040)
    word("comdat-code", 20 + 36, 0x60501020)
    word("aux-length-mismatch", 445 + 18, 58)
    word("aux-relocation-count", 445 + 18 + 4, 2, 2)
    word("aux-overrun", 445 + 12 * 18 + 17, 2, 1)
    word("undefined-function", 445 + 11 * 18 + 12, 0, 2)
    word("function-entry-outside-text", 445 + 11 * 18 + 8, 59)
    word("section-symbol-nonzero-value", 445 + 6 * 18 + 8, 1)
    word("text-relocation-aux-reference", 279 + 4, 7)
    word("text-relocation-symbol-overflow", 279 + 4, 14)
    word("text-relocation-debug-target", 279 + 4, 8)
    word("unsupported-dir32nb", 279 + 8, 7, 2)
    word("unsupported-rel32", 279 + 8, 0x14, 2)
    word("text-field-overflows-section", 279, 58)
    word("table-field-overflows-section", 305, 14)
    word("overlapping-table-fields", 315, 2)
    word("duplicate-table-field", 315, 0)
    word("wrong-table-target-section", 305 + 4, 6)
    word("table-addend-outside-code", 289, 59)
    word("table-addend-overflow", 289, 0xFFFFFFFF)
    word("table-addend-inside-instruction", 289, 28)
    word("unexpected-text-addend", 220 + 23, 1)

    for label, data in cases:
        try:
            extract(data)
        except ValueError:
            continue
        raise AssertionError(f"malformed object admitted: {label}")
    print(f"PASS: original linked text/table and {len(cases)} malformed COFF cases")


if __name__ == "__main__":
    main()
