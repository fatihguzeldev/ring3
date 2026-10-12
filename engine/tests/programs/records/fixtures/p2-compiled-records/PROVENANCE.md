# p2-compiled-records provenance

`kernel.c` is a self-authored scalar C11 reduction over eight-byte records. Compile-time assertions fix the signed-byte/tag/signed-word/weight/marker layout. It rotates an unsigned accumulator, combines the signed product according to tag parity and adds weight times tag with defined unsigned wraparound.

The fixed producer uses Apple clang/LLVM objdump with the freestanding scalar `i686-pc-windows-msvc` C11 profile. Exact tool identities, flags, environment, input hashes and artifact hashes are in `receipt.json`.

The ordinary COFF extractor retains the complete uniquely identified text and rejects relocations, unresolved externals, extra functions, nonzero timestamps and malformed bounds.

Retained inputs/artifacts: [kernel.c](kernel.c), [kernel.disasm.txt](kernel.disasm.txt), [kernel.text.bin](kernel.text.bin).

Reproduce from the repository root into a new output directory:

```sh
python3 engine/tests/programs/fixtures/p2-compiled-records/build.py target/p2-compiled-records-rebuild-new
```

Routine tests consume retained bytes without invoking a compiler. Receipt working directories and historical paths identify the original build; compare artifact hashes when reproducing elsewhere.
