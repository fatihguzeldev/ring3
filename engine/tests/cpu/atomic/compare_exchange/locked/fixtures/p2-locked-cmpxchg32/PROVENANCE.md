# p2-locked-cmpxchg32 provenance

`kernel.c` is a self-authored C11 compare-exchange leaf with a natural Boolean return and a failure-only store to the caller-owned expected DWORD. The complete original 38-byte compiler function is retained.

The fixed producer uses Apple clang/LLVM objdump with the freestanding scalar `i686-pc-windows-msvc` C11 profile. It imports the pinned profile from `engine/tests/programs/fixtures/p2-compiled-records/build.py`. Exact tool identities, flags, environment, input hashes and artifact hashes are in `receipt.json`.

The ordinary COFF extractor retains the complete function text and rejects relocations, unresolved externals and malformed layouts. Adapters change the fixed function-symbol check without rewriting compiler instructions.

Retained inputs/artifacts: [kernel.c](kernel.c), [kernel.disasm.txt](kernel.disasm.txt), [kernel.obj](kernel.obj), [kernel.text.bin](kernel.text.bin).

Reproduce from the repository root into a new output directory:

```sh
python3 engine/tests/cpu/atomic/fixtures/p2-locked-cmpxchg32/build.py target/p2-locked-cmpxchg32-rebuild-new
```

Routine tests consume retained bytes without invoking a compiler. Receipt working directories and historical paths identify the original build; compare artifact hashes when reproducing elsewhere.

The C atomic source is a compiler witness; it does not establish concurrent C11 ordering or hardware bus-lock behavior.
