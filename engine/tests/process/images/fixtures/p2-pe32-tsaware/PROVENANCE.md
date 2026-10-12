# p2-pe32-tsaware provenance

`program.S` is self-authored: it sets EAX to7, adds35 and remains in a two-byte self-jump. Clang assembles the complete ten-byte text; rust-lld links the original PE32 executable. The linker receives no terminal-server-aware switch; its default `DllCharacteristics=0x8100` is retained without header reconstruction.

`build.py` fixes tools, argv and environment, validates the produced image and records input/tool/artifact hashes in `receipt.json`. Retained files: [program.S](program.S), [program.obj](program.obj), [program.exe](program.exe), [program.text.bin](program.text.bin), [program.disasm.txt](program.disasm.txt), [receipt.json](receipt.json).

Reproduce from the repository root into a new directory:

```sh
python3 engine/tests/process/images/fixtures/p2-pe32-tsaware/build.py target/p2-pe32-tsaware-rebuild-new
```

Routine tests consume the original executable without compiling/linking. Receipt paths and working directories identify the original build; compare artifact hashes when reproducing elsewhere. This declarative PE flag fixture does not implement terminal-server sessions or Windows startup.
