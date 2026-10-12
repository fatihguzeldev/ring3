# Mutable record leaf provenance

`kernel.c` is a header-free, self-authored scalar C leaf with the fixed cdecl
signature `u32 ring3_reduce(Record8B *records, u32 count, u32 seed)`. The PE caller
is authored separately. Routine tests consume frozen compiler text and do not
invoke a compiler.

The record layout matches the separate readonly fixture and has compile-time
size and offset assertions:

| Offset | Field | Type | Mutation |
| --- | --- | --- | --- |
| 0 | `signed_byte` | signed 8-bit | retained |
| 1 | `tag` | unsigned 8-bit | XOR with rotated accumulator's low byte |
| 2 | `signed_word` | signed 16-bit | retained |
| 4 | `weight` | unsigned 16-bit | add rotated accumulator's high WORD, modulo 65536 |
| 6 | `marker` | unsigned 16-bit | retained |

For each record, rotate the unsigned accumulator left by five, compute and store
the new tag and weight, then XOR the signed byte×signed word product for an odd
new tag or add it for an even new tag. Add `new_weight * new_tag` and continue.
The signed product fits `i32`; narrowing and accumulator arithmetic have defined
unsigned wraparound. Only bytes 1, 4 and 5 of each processed record may change.
The caller will execute two passes on the same live allocation, feeding the
first returned accumulator into the second pass without reinitializing records.

`build.py` imports the unchanged strict COFF extractor, tool paths, flags and
controlled environment from `../p2-compiled-records/build.py`. It requires that
dependency's exact SHA256
`fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258`.
Python bytecode output is disabled before importing it; the readonly fixture is
not modified. Both the adapter and imported builder are snapshotted and pinned
in each receipt alongside the source, compiler and objdump.

The inherited profile uses `i686-pc-windows-msvc`, freestanding C11 and `-O2`,
with builtins, stack protection, vectorization, SLP, loop unrolling, SSE, MMX,
unwind tables and incremental-linker compatibility disabled. Loop alignment is
one byte. The receipt records complete argv and environment, normalized hashes
of flags/environment/commands, and the original COFF/artifact hashes.

Build into a **new** output directory from the repository root:

```sh
python3 engine/tests/fixtures/p2-mutable-records/build.py target/mutable-records-rebuild
```

The extractor requires exactly the `_ring3_reduce` function, a single nonempty
`.text`, a zero timestamp and no relocations or unresolved externals. It retains
the complete section through its end, including any compiler padding. Source,
adapter, dependency and tool hashes must remain stable throughout the build.

The frozen compiler is Apple clang 21.0.0 (`clang-2100.1.1.101`), binary SHA256
`f30550eab15fdf5ab8c0dc54c52679711241e5d4b636b027e18c09fef531775d`.
Two fresh builds produced identical object, text and disassembly bytes. Receipts
match except for their output working directories. Independent raw COFF parsing
confirmed the zero timestamp, sole function, section bounds, absence of
relocations and exact extracted text; receipt and current input/tool hashes were
also verified.

| Frozen input or artifact | Bytes | SHA256 |
| --- | ---: | --- |
| `kernel.c` | 1700 | `b144696a2cb4fafc199beff9e1d13b921fa8cda115b4a74bf35275654ecf049b` |
| `build.py` | 5574 | `53a844cabb9ea0b29ba4bec0a28a34828b28120783c556a8b510ab185f7fef70` |
| imported readonly `build.py` | 10227 | `fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258` |
| `kernel.obj` (rebuild output) | 615 | `fb681debc143a8375b30a045bbdcc9247420a25242628f5794834de60f89f635` |
| `kernel.text.bin` | 101 | `600b2c46774458e4e3b1b96f032270388195c0afa65b563c9c63731623e48c5a` |
| `kernel.disasm.txt` | 2407 | `0661130734af67d3b4ee6750e2a6e44081d504fb96c239fb1a8d0f7fc15daa32` |
| `receipt.json` (first build) | 6228 | `5d4ba563452662df3fd5e0dd81c8433a99a6d36819c0506a8f74da8943216fe2` |

The original, unmodified text has entry offset `0`, exclusive end `101`, 40
reachable instructions and four block leaders: `0`, `0x10`, `0x12`, `0x60`.
Every byte is reachable. The emitted BYTE store is at `0x2a`, addressing
`[EBP + ESI*8 + 1]`; the WORD store at `0x2e` addresses
`[EBP + ESI*8 + 4]`. The function restores EBP, EBX, EDI and ESI before its cdecl
return. Replacement explicit/entry admission produced 14,030-byte Wasm modules;
resident explicit/entry admission produced 14,128-byte modules. All four preserved
the arena and stayed within the unchanged 8-block, 64-instruction, 65,536-byte
limits. These are compilation/admission results, not guest execution results.

The bank `target/engine-mutable-records-2026-10-11` preserves both build outputs
and the first admission probe's source, binary and failure. That probe reused a
single engine while attempting two overlapping resident owners, correctly
receiving `InstructionOverlap(4096)`. The corrected probe uses an independent
engine for each profile. Only probe isolation changed; C, builder, compiler text
and engine product code remained unchanged.

The intended evidence is one fixed-profile mutable scalar leaf in a self-authored
private PE. It does not establish general compiler/linker support, native Windows
execution, whole-leaf fault transactions, arbitrary executable compatibility or
a performance result. The readonly fixture's source and frozen artifacts remain
separate and unchanged.
