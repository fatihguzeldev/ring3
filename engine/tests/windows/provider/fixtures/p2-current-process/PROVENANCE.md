# Original C current-process caller

`kernel.c` is a header-free i686 C function with four dllimport stdcall
declarations. It saves the real `GetCurrentProcess()` result, sets LastError
to `0x89abcdef`, calls `FlushInstructionCache(saved_handle, NULL, 0xffffffff)`,
and reads LastError. It returns 42 only when the handle is `0xffffffff`, Flush
returns one, and LastError remains unchanged. The authored PE entry calls this
function and passes its natural returned EAX to named `ExitProcess`.

The fixed compiler profile is imported from the unchanged
`../p2-compiled-records/build.py`, required SHA256
`fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258`.
It uses `i686-pc-windows-msvc`, C11, `-O2`, freestanding scalar code without
builtins, stack protector, vectorization, SLP, unrolling, SSE/SSE2, MMX,
unwind tables or addrsig. It retains `-falign-loops=1` and
`-mno-incremental-linker-compatible`, a zero COFF timestamp and the controlled
PATH/locale/timezone/SOURCE_DATE_EPOCH environment. No compiler runs in routine
engine tests.

Rebuild from the repository root into a fresh directory:

```sh
python3 engine/tests/fixtures/p2-current-process/build.py target/current-process-rebuild-new
python3 engine/tests/fixtures/p2-current-process/check_coff.py target/current-process-rebuild-new/kernel.obj
```

The first two original compiler builds were identical 805-byte objects,
SHA256 `7b4d5e9fa4a83d695741fa9431dcce922f84e0bbd746ff13205cc0bee591e10b`.
The complete 67-byte function has 23 instructions, including CMOVNE and four
memory-indirect CALLs. Root admitted its five original blocks at offsets
`0`, `8`, `21`, `32`, `40` on the matching baseline engine; this artifact did
not require a new CPU opcode. The original raw text SHA256 is
`957a7b5deafb448e973efd9b18f8235856f2c522ac0b7cb3da20aeb5278299c0`.

The purpose-specific adapter validates the ordinary four-section COFF layout,
16 symbol slots including auxiliaries, exact function entry and four undefined
external IAT symbols. Empty `.data`/`.bss` and 100 bytes of nonloaded `.debug$S`
metadata are validated and discarded. Only these type-6 `IMAGE_REL_I386_DIR32`
fields are resolved:

| Text field | External symbol | Authored IAT RVA |
| --- | --- | --- |
| `4` | `__imp__GetCurrentProcess@0` | `0x3180` |
| `17` | `__imp__SetLastError@4` | `0x3184` |
| `28` | `__imp__FlushInstructionCache@12` | `0x3188` |
| `36` | `__imp__GetLastError@0` | `0x318c` |

Each original DWORD addend is zero. Linking replaces those four DWORDs with
preferred base `0x400000` plus the listed IAT RVA; every other instruction byte
must remain identical to the original artifact. Text RVA is `0x2000`, so the
corresponding PE HIGHLOW entries are `0x2004`, `0x2011`, `0x201c`, `0x2024`.
The authored caller adds one HIGHLOW entry at `0x100a` for ExitProcess.
The linked text SHA256 is
`372189564f9ae99ced2d328844f779200e2e2c7332d350372fc4e39571bda24d`.
This follows the [Microsoft PE/COFF format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format)
for external symbols, auxiliary symbol indices, DIR32 virtual addresses and
PE HIGHLOW relocation. It is a fixed fixture adapter, without a system linker
or arbitrary external-library support.

`receipt.json` records source/builder/checker/dependency snapshots, tool binary
hashes and versions, argv, environment and artifact hashes. Inputs and tools
are checked again before publication. The companion checker verifies the
original linked result and 45 malformed objects covering bounds, overlap,
symbols/auxiliaries, external shape, relocation type/target/position/addend and
instruction-byte changes. Raw object, raw text, linked text, disassembly and
relocation manifest remain separate artifacts.

The actual runner is authored for preferred-base bytes loaded at `0x500000`
under replacement and resident ownership. Five staged captures per owner keep
each child within three blocks and 14 instructions including its gate, with
the unchanged eight-slot resident table and 65536-byte module ceiling. It
observes the natural C return separately before ExitProcess, compares all
4364 arena bytes including dirty valid FP, and saves chronological captured
provider frames and completed arenas. No CPU reseed or host guest-RAM mutation
occurs after startup. Its expected finite ledger is 16 generated calls,
56 retired instructions, ten provider completions and 42 canonical helpers;
runtime success remains an execution check, rather than a source assertion.

A draft receipt copied the earlier one-text-page fixture's mapped-page count
five. Source review established two text pages plus header/data/fixups and
one gate page, so the expectation was corrected to six before execution.
The stack is a separate seventh mapped page. Evidence preserves the draft
via an exact one-line inverse and identifies that reconstruction explicitly.

This fixture does not establish real OS handles, hardware instruction-cache
or thread behavior, general compiler/Windows/application compatibility, or
performance improvement. RAM evidence covers selected stack, IAT and canaries.
