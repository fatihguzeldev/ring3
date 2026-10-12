# DWORD memory exchange proof

The original fixture is a freestanding C11 leaf with one `__c11_atomic_exchange` using `__ATOMIC_SEQ_CST`. It uses no SDK, system header, library, proprietary payload, or PE loader extension.

## Frozen compiler artifact

`build.py` reuses the fixed i686 profile and ordinary relocation-free i386 COFF extractor from `../p2-compiled-records/build.py` (SHA256 `fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258`). Only the fixed function symbol and diagnostic spelling change from ring3_reduce to ring3_exchange. The extractor is not generalized.

Two independent builds produced identical source, object, text and disassembly. The receipt preserves the exact compiler/objdump paths, executable hashes, versions, flags, environment, working directory, parser adaptation and artifact hashes. Receipt bytes differ between builds because their working directories differ.

| artifact | bytes | SHA256 |
| --- | ---: | --- |
| kernel.c | 207 | 7634476e3cc83834817057bb68edf32cdc9b8e3f1998a683ba5e6be32bcd7750 |
| kernel.obj | 527 | b6f4f6a58f41e34e76e3878ad7cf1380aa459baec74abcdecb288709ad1ce826 |
| kernel.text.bin | 11 | 66fb4a5aaae183d35069621ea1b973ad550f0ce9516415542c74b98f4374c6db |

Text `8b4c24048b4424088701c3` has one entry, no relocation, and four instructions: MOV ECX,[ESP+4]; MOV EAX,[ESP+8]; XCHG [ECX],EAX; RET. The XCHG is the unprefixed `87 01` at leaf offset8. Matching baseline bound compilation rejected that original instruction at0x2008 before product implementation.

Reproduce in a fresh output directory:

```sh
python3 engine/tests/fixtures/p2-memory-xchg32/build.py target/memory-exchange-compiler-rebuild
```

## Execution contract

The four bound profiles admit exact unprefixed DWORD memory XCHG. Each operation invokes one canonical exchange helper. The helper holds one exclusive mutable engine registry borrow through owner admission, complete Read then Write preflight, one version update, existing validated copy-in, R3MH publication, and currentness check. It makes no outgoing callback.

The old DWORD is published to the source register only after a valid successful helper packet. FLAGS are preserved. A consumed executable page is invalidated even by an identical write; successful old-value publication and one retirement precede the existing SMC exit. Access/version failures preserve memory state and expose zero helper VALUE.

The frozen leaf runs at0x2000 with a natural cdecl caller at0x3000 and stack at0x7000. First replacement12345678 returns89ABCDEF; the caller pushes that live returned EAX as the second replacement. The second call returns12345678 and restores the cell to89ABCDEF. There is no host CPU/RAM reseed inside this chain.

The actual runner checks full4364-byte arena equality, a dirty128-byte x87 owner, observed CPU/exit/helper packets and selected RAM canaries. Synthetic malformed-helper tests do not forward or commit an engine transaction; rejection does not establish rollback of a malicious host that already wrote memory.

## Sources and ceilings

[Intel SDM volume2D, XCHG, printed6-28/29](https://cdrdv2-public.intel.com/789589/334569-sdm-vol-2d.pdf) specifies memory XCHG locking without an explicit LOCK prefix and unchanged FLAGS. [Clang C11 atomics](https://clang.llvm.org/docs/LanguageExtensions.html#c11-atomic-operations) documents the builtin used by this original source.

This proof covers a synchronous canonical transaction over private flat32 engine memory and finite selected execution cases. It establishes no cross-thread/shared-memory ordering, native bus lock, fences, complete C11 concurrency, hardware fault priority/completeness, whole-RAM state, performance gain, browser/game/SDK/graphics compatibility, or hosted CI status. Explicit LOCK, BYTE/WORD memory XCHG and prefix aliases remain closed. Existing block/instruction/module/resident limits remain unchanged.

Local evidence bank: `target/engine-memory-xchg32-2026-10-11`. Linear: [R3-536](https://linear.app/ring3-dev/issue/R3-536/support-single-transaction-dword-memory-xchg).
