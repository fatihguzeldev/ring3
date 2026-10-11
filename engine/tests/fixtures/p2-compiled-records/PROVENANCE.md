# Compiled record leaf provenance

`kernel.c` is a header-free, self-authored scalar C leaf, compiled for
`i686-pc-windows-msvc`. Routine tests consume `kernel.text.bin`; they do not
invoke a compiler. The PE loader scaffold around this leaf is authored separately.

`Record8B` has compile-time size and offset assertions:

| Offset | Field | Type |
| --- | --- | --- |
| 0 | `signed_byte` | signed 8-bit |
| 1 | `tag` | unsigned 8-bit |
| 2 | `signed_word` | signed 16-bit |
| 4 | `weight` | unsigned 16-bit |
| 6 | `marker` | unsigned 16-bit, unused by the reduction |

Each record rotates the unsigned 32-bit accumulator left by five, XORs its
signed byte×signed word product for an odd tag or adds that product for an even
tag, then adds `weight * tag`. The signed product fits `i32`; conversions,
rotation and accumulation have defined unsigned wraparound. The function has no
globals, calls or external dependencies.

The frozen compiler is Apple clang 21.0.0 (`clang-2100.1.1.101`), binary SHA256
`f30550eab15fdf5ab8c0dc54c52679711241e5d4b636b027e18c09fef531775d`.
The fixed profile uses `-O2`, freestanding C11, no builtins, stack protector,
vectorization, SLP, unrolling, SSE, MMX or unwind tables. It additionally uses
`-mno-incremental-linker-compatible` and `-falign-loops=1`. `receipt.json` contains
the complete argv, compiler and objdump versions/hashes, controlled environment,
source/builder hashes, COFF metadata and artifact hashes.

Rebuild from the repository root into a **new** output directory:

```sh
python3 engine/tests/fixtures/p2-compiled-records/build.py target/compiled-records-rebuild
```

The builder snapshots the C source, emits `kernel.obj`, disassembles it and
extracts the entire uniquely identified `.text`. It rejects relocations,
unresolved externals, additional functions, nonzero timestamps and malformed
bounds. Source, builder and tool hashes must remain stable during the build.
Fresh output receipts record their own working directory; compare artifact
bytes/hashes when rebuilding in another directory.

| Frozen artifact | Bytes | SHA256 |
| --- | ---: | --- |
| `kernel.c` | 1539 | `168281c5b2e8f9aa18eb788fdf8beebf9e9aab90d38e3686a1b7ed0f6375aa8e` |
| `build.py` | 10227 | `fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258` |
| `kernel.obj` (rebuild output) | 588 | `02c4f3f29fc8fe80e07612d618711c12c6f307208dda62f289ad5fc9fce965e4` |
| `kernel.text.bin` | 74 | `8bb4f64cef988239a8042519e089027aa03cb938dcd8cc99de18124470cef5be` |
| `kernel.disasm.txt` | 1884 | `1d506ef39d26c74994bb3f23d725a18b1fe762f7b0c45c5dc8e3639ac4a9c00c` |
| `receipt.json` | 4770 | `a02bb3aca8f9ed2b8f1cd2374349c9b7908e7eff2d27392e40c964aa32e4888c` |

Two fresh builds reproduced the complete object, text and disassembly bytes.
`_ring3_reduce` starts at offset `0`; its recorded extent ends at exclusive
offset `74`, retaining the complete section. All 74 bytes are reachable: native
admission decoded 31 instructions with leaders `0`, `0x10`, `0x16`, `0x45`, and
produced a 12,168-byte bound Wasm module under the existing limits. The cdecl leaf
restores EBP, EBX, EDI and ESI and returns with caller-owned arguments intact.

The evidence bank `target/engine-compiled-records-2026-10-11` preserves the first
COFF timestamp refusal and its original 84-byte text. Independent admission of
that original text rejected offset `0x16` as `Unsupported(Segment)` for compiler
alignment padding `66 2e 0f 1f 84 00 00 00 00 00`. The fixed profile recompiles
the whole leaf with loop alignment disabled; no instruction bytes were patched
and no engine support was added. This is a deliberate compiler-profile limit.

This records one compiled scalar leaf's artifact admission. It does not establish
a compiler/linker-generated PE, arbitrary compiler-output acceptance, native
Windows execution, general executable compatibility or a performance result.
