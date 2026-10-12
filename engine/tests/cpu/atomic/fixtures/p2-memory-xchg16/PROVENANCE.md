# word memory exchange proof

## original compiler artifact

`kernel.c` is the original306-byte header-free C11 source, SHA256 `933adfdf3c98b4238a2915addf2563871a4322142af58aad4f94cbf97e166337`. It requests an unsigned-short atomic exchange and returns the old word zeroextended to unsigned32. The second actual guest call consumes the first returned EAX without a host CPU reseed.

Root compiled this exact source twice in fresh directories using the pinned i686 freestanding scalar profile. Both original objects were identical537-byte ordinary i386 COFF, SHA256 `82456689c709bf29ae1eda146e2b45b69257cfb56919c4baf9e935f7746395aa`. Their original relocation-free text is16 bytes, SHA256 `a9d8a9a9dbe0da4761697e968ddea5966a2d1db9f97112e9c0b43f641ce44fea`:

```
8b4424040fb74c24086687080fb7c1c3
```

The five original instructions load the pointer with read32, load the low desired argument word with read16/MOVZX, exchange CX with `[EAX]` at offset9, zeroextend returned CX to EAX, then naturally RET using read32. No instruction or object relocation was rewritten. The exact original two-build evidence and baseline unsupported-opcode admission evidence are retained in `target/engine-word-exchange-scout-2026-10-11` and `target/engine-memory-xchg16-2026-10-11`.

`build.py` reuses the unchanged strict ordinary-COFF extractor/compiler profile from `p2-compiled-records/build.py`, SHA256 `fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258`. Its extractor adaptation changes only the fixed symbol from `ring3_reduce` to `ring3_exchange_word`; the builder additionally fails closed if original text or object identity changes. The formal builder replay matched the source/object/text of both original builds. All three builds used Intel-syntax `--disassemble --section=.text --x86-asm-syntax=intel`; their405-byte disassemblies are byte-identical, SHA256 `0ac556713f75dda48d8ba5c3b7beae6b5b6a76b64aa9f9b9fd6060b0e74cb572`. The5385-byte `receipt.json`, SHA256 `d8dbe487e06e0a9c00ee20d5cef6ee88bc9012d5e8b7da994b894ca068aee007`, records exact tool hashes, flags, environment, extractor hashes and file identities. Routine fixture execution reads these frozen artifacts and needs no compiler.

Root rebuild command, only when explicitly authorized:

```
python3 engine/tests/fixtures/p2-memory-xchg16/build.py target/memory-word-exchange-compiler-rebuild
```

## finite runtime contract

The runner targets exact single66 WORD memory87 exchange in four bound profiles. Its arithmetic low16/own-high16 oracle covers AX/CX/DX/BX/SP/BP/SI/DI while preserving original full effective addresses and complete FLAGS. It compares the entire4364-byte arena including dirty128-byte FP state on every generated call, and checks selected32-byte RAM windows and selected stack/code bytes separately. Persisted final/compiler milestone arenas are a finite subset of those runtime assertions. Public diagnostic reads are separate from forwarded guest helper observations.

The canonical exchange uses one synchronous exclusive engine borrow, full two-byte Read then Write preflight, one version advance even for identity, two committed little-endian bytes and returned old word. FFFFFFFE is a valid two-byte span; FFFFFFFF is a Read AddressOverflow before mapping/permission checks and cannot wrap a write. A later-page Read denial wins over an earlier-page Write denial. Map-only repairs expose known zeroed new pages; protection-only repairs retain original RAM. Explicit current-parent/EA/FLAGS changes are separate from repair-only retries.

The new40-byte R3MH version6 keeps old versions/layouts unchanged. Read16 uses existing V2 and DWORD stack read/store uses existing V1. Synthetic protocol/infrastructure packets never forward or commit a real exchange. They include the unrepresentable nonoverflow fault addressFFFFFFFF from originalEAFFFFFFFE, out-of-span/wrong-width/old-value/status errors, and valid infrastructure records. Native tests supply actual exhausted-counter evidence; synthetic records do not exhaust a real counter.

Changed and identity consumed-code exchanges publish the returned word/PC/one retirement before reason6, then reject the old owner4 before a fresh untouched suffix capture. Later replacement generation3 is a different rejection. The compiler proof's two real calls retain stack/callee-saved parents and observe old word BEEF→5678 with cell5678→BEEF; the desired argument's upper16 bits are ignored by the original compiler's WORD load. Module blocks/instructions/bytes, resident limits and arena remain unchanged. Final actual counters and physical artifacts belong to the saved result, not this source artifact description.

The [Intel XCHG reference, printed6-28/29](https://cdrdv2-public.intel.com/789589/334569-sdm-vol-2d.pdf) supplies the instruction reference. This finite private-flat32 proof establishes no hardware bus locking, shared memory, cross-thread/C11 ordering, fences, whole-RAM dump, explicit LOCK/prefix aliases, performance gain, browser/game/SDK/graphics compatibility or hosted CI status. Local task: [R3-541](https://linear.app/ring3-dev/issue/R3-541/execute-compiler-word-exchanges-in-one-memory-transaction).
