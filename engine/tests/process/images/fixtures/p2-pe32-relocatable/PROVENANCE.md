# p2-pe32-relocatable provenance

`i386.s` is self-authored source introduced in commit `891f87a9ad95e6437a84fdbeae126e1dc921f468`. The retained object, PE32 executable and inspection output are unmodified producer outputs. `producer-source-pins.json` identifies the exact six source/producer inputs recoverable byte-for-byte from source revision `9d5b03c65854e9576cd42e26581e8d1bfb039c3b`.

The repository Apache-2.0 LICENSE/NOTICE appeared in commit `9d5b03c65854e9576cd42e26581e8d1bfb039c3b` with byte-identical assembly and Copyright2026 Fatih Güzel. The assembly has no separate license header. The historical manifest's `licenseStatus: pending` is preserved; this provenance does not upgrade that recorded status.

Retained files: [i386.s](i386.s), [relocations.obj](relocations.obj), [relocations.exe](relocations.exe), [inspection.txt](inspection.txt), [fixture.json](fixture.json), [evidence.json](evidence.json), [producer-source-pins.json](producer-source-pins.json).

The producer is recoverable from the pinned Git snapshot, not from the current source tree. In that recovered tree, with Node22.16.0 and the exact clang/rust-lld/llvm-objdump identities in `evidence.json`, run:

```sh
LC_ALL=C TZ=UTC node tools/corpus/build-relocations.mjs
```

The producer performs two builds and compares artifacts. It also emits an unused AMD64 fixture; this directory retains only the i386 inputs/output. `evidence.json` contains the exact command arrays, environment, tool versions and hashes.

The PE32 image has preferred base0x400000, image size0x5000, entry RVA0x1000 and `DllCharacteristics=0x8140`. Its complete entry is XOR EAX,EAX; RET. The HIGHLOW directory at RVA0x4000 identifies DWORD fields at RVAs0x2008,0x200c,0x3010. Historical `executed: false` and `windowsOracle: not-run` describe the producer receipt; they do not report current Ring3 execution.
