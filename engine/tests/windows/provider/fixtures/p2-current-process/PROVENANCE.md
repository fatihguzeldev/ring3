# p2-current-process provenance

`kernel.c` is a self-authored i686 C function with four dllimport stdcall declarations. It saves GetCurrentProcess, sets LastError, passes the saved handle to FlushInstructionCache and reads LastError. It returns42 when the handle/result/error checks succeed; the PE caller is separately authored.

The fixed producer uses Apple clang/LLVM objdump with the freestanding scalar `i686-pc-windows-msvc` C11 profile. It imports the pinned profile from `engine/tests/programs/fixtures/p2-compiled-records/build.py`. Exact tool identities, flags, environment, input hashes and artifact hashes are in `receipt.json`.

The purpose-specific COFF adapter resolves only the four declared DIR32 IAT fields. `kernel.text.raw.bin` retains original text; `kernel.text.bin` and `kernel.relocations.json` record the declared linked DWORD substitutions. Other instruction bytes are retained. `check_coff.py` validates the fixed object layout and malformed inputs.

Retained inputs/artifacts: [kernel.c](kernel.c), [kernel.disasm.txt](kernel.disasm.txt), [kernel.obj](kernel.obj), [kernel.relocations.json](kernel.relocations.json), [kernel.text.bin](kernel.text.bin), [kernel.text.raw.bin](kernel.text.raw.bin).

Reproduce from the repository root into a new output directory:

```sh
python3 engine/tests/windows/provider/fixtures/p2-current-process/build.py target/p2-current-process-rebuild-new
python3 engine/tests/windows/provider/fixtures/p2-current-process/check_coff.py target/p2-current-process-rebuild-new/kernel.obj
```

Routine tests consume retained bytes without invoking a compiler. Receipt working directories and historical paths identify the original build; compare artifact hashes when reproducing elsewhere.
