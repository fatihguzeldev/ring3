# p2-bitmap-dispatch32 provenance

`kernel.c` is a self-authored, header-free C11 leaf that intersects pending bits with an allowed mask, clears the lowest eligible bit, and returns its index. With no eligible bit it returns32 without changing pending. A nonzero guard keeps `__builtin_ctz` defined.

The fixed producer uses Apple clang/LLVM objdump with the freestanding scalar `i686-pc-windows-msvc` C11 profile. It imports the pinned profile from `engine/tests/programs/fixtures/p2-compiled-records/build.py`. Exact tool identities, flags, environment, input hashes and artifact hashes are in `receipt.json`.

The ordinary COFF extractor retains the complete function text and rejects relocations, unresolved externals and malformed layouts. Adapters change the fixed function-symbol check without rewriting compiler instructions.

Retained inputs/artifacts: [kernel.c](kernel.c), [kernel.disasm.txt](kernel.disasm.txt), [kernel.obj](kernel.obj), [kernel.text.bin](kernel.text.bin).

Reproduce from the repository root into a new output directory:

```sh
python3 engine/tests/programs/fixtures/p2-bitmap-dispatch32/build.py target/p2-bitmap-dispatch32-rebuild-new
```

Routine tests consume retained bytes without invoking a compiler. Receipt working directories and historical paths identify the original build; compare artifact hashes when reproducing elsewhere.
