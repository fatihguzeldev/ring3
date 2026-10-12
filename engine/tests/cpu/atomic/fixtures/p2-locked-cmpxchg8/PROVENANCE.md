# original locked byte compare-exchange consumer

`kernel.c` is a header-free, self-authored C11 function. It invokes one relaxed
strong compare-exchange on `_Atomic(unsigned char)`, takes a caller-owned
expected-byte pointer, and returns the builtin's Boolean result as `unsigned
int`. The compiler implements the conditional failure-output store and both
return paths. The source is not an instruction-byte template.

The root ran two fresh discovery builds with the existing scalar i686 profile.
They produced identical original source, object, complete text and Intel
disassembly. The retained whole function contains 34 bytes and 14 instructions,
including `LOCK CMPXCHG byte ptr [ESI],AH` at offset 17. Its leaders are offsets
0, 26 and 28. The failure path executes the expected-byte store and retires
14 instructions; the success path skips that store and retires 13. Neither
path drops the compiler's natural RET.

| original artifact | bytes | SHA-256 |
| --- | ---: | --- |
| `kernel.c` | 353 | `f28564ae660a89c902bd4415772970cad9e43b046f23a7e2a33c8e4b349af3af` |
| `kernel.obj` | 558 | `1a53454f6350eeff6ff8da18243950133e7f82e86033c5aa67de96c58e839e3d` |
| `kernel.text.bin` | 34 | `1a0c6a5c52530f6aa5b2781a5dab5bff28f2d8330ca257a7b478de669a93115f` |
| `kernel.disasm.txt` | 978 | `1b1c23518673dbe7f6a9c4142892488ee542bd0f2f710a8f0b1097b918c75650` |

`build.py` promotes those observed object and text bytes to strict checks. It
imports the unchanged `p2-compiled-records/build.py` profile, SHA-256
`fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258`,
and reuses its ordinary i386 COFF extractor. The only extractor substitution
is the fixed symbol and diagnostic spelling `ring3_reduce` to
`ring3_compare_exchange`. It keeps the entire original `.text` without
rewriting or removing LOCK, branches, expected-output stores, or padding.
The strict object has timestamp zero, one defined external function, no
unresolved symbols and no relocations. Its sections are `.text` 34 bytes,
empty `.data` and `.bss`, and 100 bytes of nonruntime `.debug$S` metadata.

The producer runs Apple clang and LLVM objdump from the Command Line Tools
installation using the unchanged `--target=i686-pc-windows-msvc`, C11, `-O2`,
freestanding scalar profile. `receipt.json` records exact argument arrays,
resolved tool paths, versions and hashes, source and builder hashes, environment,
COFF inventory and artifact hashes. Fresh formal builds are root-owned. Routine
tests consume their retained artifacts and do not invoke a compiler. The
discovery, baseline refusal, formal repeatability, actual results and repairs
are recorded separately in
`target/engine-locked-cmpxchg8-scout-2026-10-11/`.

The baseline engine rejected the exact original LOCK instruction and all four
bound compilations of the complete function. The same frozen admission body
checks every neighboring instruction and preserves the full 4364-byte arena.
Entry compilation supplies all three original leaders explicitly; the entry
finder does not follow an omitted conditional target.

The separate authored caller makes two natural cdecl calls to the same original
function. Its first mismatch updates the live expected byte and returns zero;
the second call consumes that expected-byte update, succeeds, and returns one.
The caller also consumes both Boolean results. Original and authored bytes are
identified separately. The function's failure-output store is an ordinary
byte store after the locked cell transaction and can fault independently.
No whole-function transaction or rollback is claimed.

The intended actual proof uses four bound profiles, canonical forwarded helper
packets, dirty FP128 and full 4364-byte arena assertions, plus explicitly
selected RAM frames. BYTE addresses include `FFFFFFFF`; there is no BYTE
cross-page or overflow case. A single checked cell transaction reuses the
existing V5 40-byte checked-byte exchange packet; its mandatory mismatch write
still requires write permission and can invalidate consumed executable code.

This establishes a finite synchronous private-flat32 consumer with unshared
memory and direct canonical helpers. It does not establish hardware bus locks,
C11 interthread ordering, arbitrary host-wrapper atomicity, whole-process RAM,
performance, Windows or application compatibility, SDK, browser or game support.
Observed actual counts belong to the saved result and evidence bank rather
than postexecution changes to this provenance file.
