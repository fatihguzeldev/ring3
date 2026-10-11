# Fixed compiler switch and jump table

`kernel.c` is one header-free scalar i686 C function:
`u32 ring3_switch(volatile u32 *state, u32 selector, u32 salt)`.
It loads the original state once, applies one unsigned operation, writes the
result once, and returns the result XOR salt. The cases are `+7`, XOR
`0xa5a55a5a`, rotate left five bits, and `*3+1`; the default subtracts the
selector. Unsigned arithmetic wraps modulo 2^32. Runtime callers provide a
mapped, aligned DWORD and the ordinary three-argument cdecl frame.

The source SHA256 is
`5b4f0b8c7f3f4ad03e0d12782eeb7fbf9354c89f5cbaa7db0b72da0afe9353fd`.
The first two original compiler outputs were byte-identical 715-byte COFF
objects, SHA256
`c257d84de2faef79bec68e95a1fbfbbd05738b2ee7f2854fe3fcc4491f344d23`.
They contain a 59-byte function, a 16-byte four-entry jump table, five DIR32
relocations, and no unresolved external symbols. The original instruction
at text offset `0x14` is an actual memory-indirect JMP. The table selects text
offsets `0x1b`, `0x2b`, `0x20`, and `0x25`, in selector order. The default begins
at `0x33`, and the common store/return suffix begins at `0x35`.

Native admission of that original linked text covered all 59 bytes as 22
instructions and eight leaders: `0`, `0x14`, `0x1b`, `0x20`, `0x25`, `0x2b`,
`0x33`, `0x35`. Four bound preparation profiles admitted it with unchanged
8-block/64-instruction/65536-byte limits. Its caller belongs to a separate
capture. These are finite artifact/admission observations, not a general
compiler-output or execution-performance claim.

## Rebuild

From the repository root, use a fresh output directory:

```sh
python3 engine/tests/fixtures/p2-compiled-switch/build.py target/compiled-switch-rebuild-new
python3 engine/tests/fixtures/p2-compiled-switch/check_coff.py target/compiled-switch-rebuild-new/kernel.obj
```

The adapter imports the unchanged compiler/tool utilities from
`../p2-compiled-records/build.py`, whose required SHA256 is
`fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258`.
It does not invoke or weaken that builder's strict no-relocation extractor.
The fixed profile targets `i686-pc-windows-msvc`, C11, `-O2`, freestanding,
without builtins, stack protector, vectorization, SLP, unrolling, SSE/SSE2,
MMX, unwind tables or addrsig. It retains `-falign-loops=1` and
`-mno-incremental-linker-compatible`; the COFF timestamp must remain zero.
The controlled environment contains only fixed PATH, `LC_ALL=C`, `TZ=UTC`
and `SOURCE_DATE_EPOCH=0`.

`receipt.json` records source, builder and dependency snapshots; compiler and
objdump paths, versions and binary hashes; exact argv, working directory,
flags and environment; normalized profile hashes; COFF sections/symbols;
and artifact byte counts and SHA256 hashes. Tool and input hashes are checked
again before linked artifacts are published. Routine engine tests consume
frozen artifacts and do not invoke a compiler.
`check_coff.py` applies reproducible malformed controls to the original
object through the same `extract_switch` entry point, without compiling it.

## Bounded linking

The preferred PE base is `0x400000`, text RVA `0x2000`, and table RVA `0x4000`.
`kernel.text.raw.bin` and `kernel.rdata.raw.bin` retain original section bytes.
`kernel.text.bin` and `kernel.rdata.bin` contain only the five resolved DWORD
fields at the preferred base. The original-object disassembly is retained as
`kernel.disasm.txt`; its zero absolute JMP displacement is the original COFF
addend, not the linked address.

The text DIR32 field at `0x17` references the primary `.rdata` section symbol.
The four table fields at offsets `0`, `4`, `8`, `12` reference the primary
`.text` section symbol. Their original DWORD values are section-relative
addends. The adapter resolves each as preferred base + target section RVA +
symbol value + addend and records the result in `kernel.relocations.json`.
The corresponding PE HIGHLOW RVAs are `0x2017`, `0x4000`, `0x4004`, `0x4008`,
`0x400c`. PE relocation at another image base applies the same delta to all
five fields; instruction opcodes and relative branches remain unchanged.

The parser admits the observed five-section order and exact characteristics,
14 COFF symbol slots including auxiliaries, one external function at text
offset zero, and exactly these five ordered type-6 fields. Empty `.data` and
`.bss`, plus 100 bytes of nonloaded `.debug$S` compiler metadata, are validated
but never loaded or accepted as relocation targets. COMDAT, mutable static
data, other section layouts, undefined externals, DIR32NB, REL32, auxiliary
relocation targets, overlapping spans/fields, invalid symbol names and
out-of-range addends fail closed. Section auxiliaries must agree with the
section's size and relocation count. Checksums in those discarded auxiliary
records are retained COFF metadata, not a substitute for artifact SHA256.
The four table addends must be exactly `0x1b`, `0x2b`, `0x20`, `0x25` in
selector order. Bounds and distinctness alone cannot establish instruction
boundaries: a malformed control moved the first target from `0x1b` to
`0x1c`, inside ADD. This fixed-artifact check rejects that change without
adding a generic instruction decoder to the COFF parser.

This uses the official [Microsoft PE/COFF specification](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format):
COFF symbol indices include 18-byte auxiliary records; i386 DIR32 is a
32-bit virtual address, distinct from DIR32NB's RVA; PE HIGHLOW adds the
image relocation delta to a DWORD. This is a fixed fixture linker, without
a system linker, external libraries or general COFF support.

The implementation and runtime evidence are bounded to this authored C/PE,
the admitted table and default engine capacities. It does not establish
general compiler, Windows, browser, game, concurrency or performance support.

## Execution evidence

The actual Wasm campaign used preferred `0x400000` and relocated `0x500000`
images with replacement and resident ownership: four contexts, 108 generated
runs, 544 retired instructions and four real ExitProcess completions. The
forwarded canonical helper trace contains 196 read32 and 200 store32 calls,
including eight table-permission faults; 320 public diagnostic reads are
separate. After the first call's initial salt, each later call consumes the
preceding returned EAX as salt and the current state in RAM, without host
CPU reseeding. The stopped indirect JMP
preserves its CPU and retires zero instructions; preceding function work is
already committed. Its retry repairs only the table page's Read permission.

Twenty executed children and two resident dispatchers were saved and checked
physically. Replacement uses seven finite captures per context; resident
uses three children and a dispatcher with eight table slots. The largest
child has eight entries, 56 instructions and 31117 Wasm bytes, within the
unchanged limits. Every generated run compares the complete 4364-byte arena,
including dirty valid FP state. RAM assertions cover the authored state,
outputs, stack, table and canaries; they do not cover all guest RAM.

The evidence bank preserves the first malformed-table acceptance and its
exact-target repair. It also preserves test-only corrections for the loaded
image's `into_parts()` API, the mandatory two-byte gate descriptor and a
nested module name caught by Clippy. The first actual execution stopped at
an expected-packet error: a permission fault's exit detail is `2`, rather
than the fixture's omitted-field default `0`. The only runtime repair added
that expected detail; compiler bytes, engine and counters were unchanged.
The corrected result SHA256 is
`755d2bd6ec9c3c834e48460a7ffdd645c3fe33a1ef732bbf8650b85f3594c46d`.
This execution-evidence section was added after that run; it is a prose-only
delta from the documentation hash in the recorded source snapshot.
