# original locked word compare-exchange consumer

`kernel.c` is a header-free, self-authored C11 function. It invokes one relaxed
strong compare-exchange on `_Atomic(unsigned short)`, takes a caller-owned
expected-word pointer, and returns the builtin Boolean as `unsigned char`.
The source asserts the byte and WORD widths. The compiler implements the
conditional expected-output store and both natural return paths.

Two fresh root discovery builds produced identical original source, object,
complete text and Intel disassembly. The retained function is 34 bytes and
12 instructions, including `66 F0 0F B1 0E` at offset 17: a locked WORD
compare-exchange with CX as desired source. Its leaders are offsets 0, 27
and 30. Mismatch executes 12 instructions and the ordinary expected-word
store; match skips that store and executes 11. Both include the natural RET.
No instruction, branch, prefix, store or compiler padding is rewritten.

| original artifact | bytes | SHA-256 |
| --- | ---: | --- |
| `kernel.c` | 356 | `aedc9d309421eaefff3b4e89708eefae49dda388f0097f3665f41575310719d8` |
| `kernel.obj` | 558 | `cf946fbef8619d09836540238093766e085b68897cbde6210e031467bf2ddcb1` |
| `kernel.text.bin` | 34 | `0f48660c339c99d6f159dc2ca66b7ec9061ce665e707fa4223d1b0e949044bcc` |
| `kernel.disasm.txt` | 841 | `20469809ce92dfc43fb6fa3a6a1617b9f80aba6b224ba8f01d88c473e3f632dc` |

`build.py` promotes the observed object and whole text to strict checks. It
imports the unchanged `p2-compiled-records/build.py` profile with SHA-256
`fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258`.
The only change to its ordinary relocation-free i386 COFF extractor is the
fixed function symbol and diagnostic spelling `ring3_reduce` to
`ring3_compare_exchange`. The object has zero timestamp, one defined external
function, no unresolved symbols or relocations, `.text` 34 bytes, empty `.data`
and `.bss`, and 100 bytes of nonruntime `.debug$S` metadata.

The fixed producer uses Apple clang and LLVM objdump from Command Line Tools
with the retained freestanding scalar i686 C11 profile. Its receipt records
exact tool paths, versions and hashes, argument arrays, environment, source,
builder and shared-profile hashes, COFF inventory and artifact hashes. Root
owns formal compilation and comparison against the two discovery outputs.
Routine fixtures consume the frozen artifacts without invoking a compiler.
Discovery, frozen baseline refusal, formal repeatability and observed runtime
results remain in `target/engine-locked-cmpxchg16-scout-2026-10-11/`.

The whole original function is compiled in all four bound flat32 profiles.
Entry compilation supplies all three original leaders explicitly. A separate
authored caller makes two natural cdecl calls using the same cell and expected
pointers. The first mismatch updates expected from 3456 to A7B6. The second
call consumes that guest-produced expected WORD and replaces the cell with
D3E4. The caller explicitly applies MOVZX to AL after each return before
combining false and true into result 2. This is necessary: the original loads
D3E4 into ECX, writes only CL for SETE, then moves ECX to EAX, so natural raw
EAX can be D300 or D301. Only AL is the C function's Boolean return contract.

The intended actual proof retains dirty FP128 and full 4364-byte arena
comparisons, canonical forwarded helper packets and selected RAM canaries.
WORD endpoint FFFFFFFE is valid; FFFFFFFF overflows. Split accesses validate
both read bytes before write permission or mutation. The locked cell helper
reuses the strict V6 40-byte WORD exchange record with the old WORD retained.
Mismatch performs a mandatory identity write, requiring write permission and
invalidating consumed executable code. The compiler's later expected-output
store is outside that cell transaction and may fault independently.

This is a finite synchronous private-flat32 consumer with unshared memory and
direct canonical helpers. It does not establish hardware bus locking, C11
interthread ordering, arbitrary host-wrapper atomicity, whole RAM, performance,
Windows or application compatibility, SDK, browser or game support. This file
is frozen before actual execution; observed counts and any repairs belong to
the saved result and evidence bank.
