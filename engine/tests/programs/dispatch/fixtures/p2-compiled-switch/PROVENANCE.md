# p2-compiled-switch provenance

`kernel.c` is a self-authored scalar i686 C function, `u32 ring3_switch(volatile u32 *state, u32 selector, u32 salt)`. It applies one unsigned update selected from addition, XOR, rotate and multiply/add; the default subtracts the selector. It stores the updated state and returns it XOR salt.

The fixed producer uses Apple clang/LLVM objdump with the freestanding scalar `i686-pc-windows-msvc` C11 profile. It imports the pinned profile from `engine/tests/programs/records/fixtures/p2-compiled-records/build.py`. Exact tool identities, flags, environment, input hashes and artifact hashes are in `receipt.json`.

The purpose-specific COFF adapter resolves five DIR32 fields: one text-to-table reference and four table-to-text references. Raw sections remain in `kernel.text.raw.bin` and `kernel.rdata.raw.bin`; linked sections and `kernel.relocations.json` record only those DWORD substitutions at preferred base0x400000. Instruction opcodes and relative branches are retained. `check/coff.py` validates the fixed object layout and malformed inputs.

Retained inputs/artifacts: [kernel.c](kernel.c), [kernel.coff-relocations.txt](kernel.coff-relocations.txt), [kernel.disasm.txt](kernel.disasm.txt), [kernel.obj](kernel.obj), [kernel.rdata.bin](kernel.rdata.bin), [kernel.rdata.raw.bin](kernel.rdata.raw.bin), [kernel.relocations.json](kernel.relocations.json), [kernel.text.bin](kernel.text.bin), [kernel.text.raw.bin](kernel.text.raw.bin).

Reproduce from the repository root into a new output directory:

```sh
python3 engine/tests/programs/dispatch/fixtures/p2-compiled-switch/build.py target/p2-compiled-switch-rebuild-new
python3 engine/tests/programs/dispatch/fixtures/p2-compiled-switch/check/coff.py target/p2-compiled-switch-rebuild-new/kernel.obj
```

Routine tests consume retained bytes without invoking a compiler. Receipt working directories and historical paths identify the original build; compare artifact hashes when reproducing elsewhere.
