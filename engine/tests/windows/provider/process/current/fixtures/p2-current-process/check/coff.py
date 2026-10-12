#!/usr/bin/env python3
"""check the original caller's four imported iat fields and malformed coff controls."""

import hashlib
import json
from pathlib import Path
import runpy
import struct
import sys

sys.dont_write_bytecode = True


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: check/coff.py BUILD_OUTPUT/kernel.obj")
    original = Path(sys.argv[1]).read_bytes()
    if hashlib.sha256(original).hexdigest() != "7b4d5e9fa4a83d695741fa9431dcce922f84e0bbd746ff13205cc0bee591e10b":
        raise ValueError("expected the frozen original fixed-profile object")
    extract = runpy.run_path(str(Path(__file__).resolve().parent.parent / "build.py"))["extract_current_process"]
    text, coff = extract(original)
    assert len(text) == 67 and coff["function"]["entry_offset"] == 0
    assert [(row["source_offset"], row["target_symbol"], row["iat_rva"]) for row in coff["relocations"]] == [
        (4, "__imp__GetCurrentProcess@0", 0x3180),
        (17, "__imp__SetLastError@4", 0x3184),
        (28, "__imp__FlushInstructionCache@12", 0x3188),
        (36, "__imp__GetLastError@0", 0x318C)]
    expected = bytearray(original[180:247])
    for offset, address in [(4, 0x403180), (17, 0x403184), (28, 0x403188), (36, 0x40318C)]:
        struct.pack_into("<I", expected, offset, address)
    assert text == expected
    assert len({row["target_symbol"] for row in coff["relocations"]}) == 4
    assert [row["pe_highlow_rva"] for row in coff["relocations"]] == [0x2004, 0x2011, 0x201C, 0x2024]
    cases = [("truncated-header", original[:19]), ("truncated-symbols", original[:650]),
             ("trailing-bytes", original + b"x")]

    def word(label, offset, value, width=4):
        data = bytearray(original)
        data[offset:offset + width] = value.to_bytes(width, "little")
        cases.append((label, bytes(data)))

    for label, offset, value, width in [
        ("wrong-machine", 0, 0x8664, 2), ("wrong-section-count", 2, 5, 2),
        ("timestamp", 4, 1, 4), ("optional-header", 16, 1, 2),
        ("characteristics", 18, 1, 2), ("symbols-overlap-raw", 8, 300, 4),
        ("symbol-count", 12, 17, 4), ("string-size-overflow", 675, 0xFFFFFFFF, 4),
        ("text-size", 36, 66, 4), ("text-pointer-overflow", 40, 0xFFFFFFFF, 4),
        ("reloc-span-overlap-text", 44, 180, 4), ("reloc-count", 52, 3, 2),
        ("line-pointer", 48, 180, 4), ("line-count", 54, 1, 2),
        ("code-comdat", 56, 0x60501020, 4), ("debug-writable", 176, 0xC2300040, 4),
        ("debug-overlap-text", 160, 180, 4), ("data-not-empty", 76, 1, 4),
        ("section-aux-size", 405, 66, 4), ("section-aux-relocs", 409, 3, 2),
        ("aux-overrun", 387 + 14 * 18 + 17, 2, 1),
        ("function-entry", 387 + 9 * 18 + 8, 1, 4),
        ("function-not-code", 387 + 9 * 18 + 12, 0, 2),
        ("import-defined", 387 + 10 * 18 + 12, 1, 2),
        ("import-addend-value", 387 + 10 * 18 + 8, 1, 4),
        ("import-not-external", 387 + 10 * 18 + 16, 3, 1),
        ("import-type", 387 + 10 * 18 + 14, 0x20, 2),
        ("import-aux", 387 + 10 * 18 + 17, 1, 1),
        ("string-offset-overflow", 387 + 10 * 18 + 4, 0xFFFFFFFF, 4),
        ("string-non-ascii", 675 + 81, 0xFF, 1),
        ("relocation-aux-target", 251, 1, 4), ("relocation-section-target", 251, 0, 4),
        ("relocation-symbol-overflow", 251, 16, 4), ("relocation-wrong-import", 251, 11, 4),
        ("dir32nb", 255, 7, 2), ("rel32", 255, 0x14, 2),
        ("field-overflow", 247, 64, 4), ("duplicate-field", 257, 4, 4),
        ("overlapping-field", 257, 6, 4), ("nonzero-addend", 184, 1, 4),
        ("call-opcode-change", 182, 0xFE, 1), ("nonrelocated-code-change", 180, 0x90, 1),
    ]:
        word(label, offset, value, width)
    for label, data in cases:
        try:
            extract(data)
        except ValueError:
            continue
        raise AssertionError(f"malformed object admitted: {label}")
    print(json.dumps({"status": "PASS", "original_bytes": len(original),
                      "linked_text_bytes": len(text), "malformed_cases": len(cases),
                      "linked_text_sha256": hashlib.sha256(text).hexdigest()}, sort_keys=True))


if __name__ == "__main__":
    main()
