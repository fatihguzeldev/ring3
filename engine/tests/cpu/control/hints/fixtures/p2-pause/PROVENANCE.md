# p2-pause provenance

`kernel.c` is a self-authored, header-free polling leaf. Its complete original 40-byte compiler text includes PAUSE at offset0x1a. The relocation-free i386 COFF function has timestamp zero and no unresolved externals.

`receipt.json` records the Apple clang/LLVM objdump identities, fixed freestanding scalar i686 C11 profile, exact compile/disassemble argv, environment, input hashes and artifact hashes. The shared profile is `engine/tests/programs/fixtures/p2-compiled-records/build.py`.

Retained files: [kernel.c](kernel.c), [kernel.text.bin](kernel.text.bin), [kernel.disasm.txt](kernel.disasm.txt), [receipt.json](receipt.json). The original object is described by the receipt but is not retained in this directory.

There is no fixture-local builder. The pinned original driver is `target/engine-pause-2026-10-11/build-pause.py`, identified by the receipt's builder SHA-256. That driver is a local evidence artifact, not a tracked dependency. Reproduction requires recovering that matching driver or replaying the receipt's exact commands and whole-text extraction with its pinned shared profile. Routine tests consume retained text without compiling it.

PAUSE in this fixture does not establish host yielding, concurrency, timing or power behavior.
