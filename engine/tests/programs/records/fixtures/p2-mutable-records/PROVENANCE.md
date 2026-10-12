# p2-mutable-records provenance

`kernel.c` is a self-authored scalar C11 reduction over eight-byte records. It retains the signed fields and marker, XORs the tag with the rotated accumulator low byte and adds its high WORD to weight. Only record bytes1,4,5 mutate; narrowing and accumulator arithmetic use defined unsigned wraparound.

The fixed producer uses Apple clang/LLVM objdump with the freestanding scalar `i686-pc-windows-msvc` C11 profile. It imports the pinned profile from `engine/tests/programs/fixtures/p2-compiled-records/build.py`. Exact tool identities, flags, environment, input hashes and artifact hashes are in `receipt.json`.

The producer reuses the strict COFF extractor unchanged. It retains the complete function text and rejects relocations, unresolved externals and malformed layouts without rewriting compiler instructions.

Retained inputs/artifacts: [kernel.c](kernel.c), [kernel.disasm.txt](kernel.disasm.txt), [kernel.text.bin](kernel.text.bin).

Reproduce from the repository root into a new output directory:

```sh
python3 engine/tests/programs/fixtures/p2-mutable-records/build.py target/p2-mutable-records-rebuild-new
```

Routine tests consume retained bytes without invoking a compiler. Receipt working directories and historical paths identify the original build; compare artifact hashes when reproducing elsewhere.
