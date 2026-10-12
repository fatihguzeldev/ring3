# byte memory exchange proof

## original compiler artifact

`kernel.c` is the original299-byte header-free C11 source, SHA256 `b59f2f4b9c2ebf1ef624bea5b39a40992ad3178b1abf564ed1a4afc5d8645fcc`. It requests an unsigned-char atomic exchange and returns the old byte zeroextended to unsigned32. The second actual guest call consumes the first returned EAX without a host CPU reseed.

Root compiled this exact source twice in fresh directories using the pinned i686 freestanding scalar profile. Both original objects were identical536-byte ordinary i386 COFF, SHA256 `861642c2cd191585e659d838e4f585dbdf64624982e987b946de9fa8da7d23de`. Their original relocation-free text is15 bytes, SHA256 `bff846b26485bf378e247d0680815eaaa1e46e628c32e2dd7f966c9167570663`:

```
8b4424040fb64c240886080fb6c1c3
```

The five original instructions load the pointer with read32, load the low desired argument byte with read8/MOVZX, exchange CL with `[EAX]` at offset9, zeroextend returned CL to EAX, then naturally RET using read32. No instruction or object relocation was rewritten. The exact original two-build evidence and baseline unsupported-opcode admission evidence are retained in `target/engine-memory-xchg8-2026-10-11`.

`build.py` reuses the unchanged strict ordinary-COFF extractor/compiler profile from `p2-compiled-records/build.py`, SHA256 `fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258`. Its extractor adaptation changes only the fixed symbol from `ring3_reduce` to `ring3_exchange_byte`; the builder additionally fails closed if original text or object identity changes. The formal builder replay matched the source/object/text of both original builds. The inherited Intel-syntax disassembly differs only in presentation from the original `-d -r` disassembly. The committed405-byte disassembly SHA256 is `d10b7f482e00fecb40b6db5b026d91fd60b59e39577711145855e88be514ac36`. `receipt.json` records exact tool hashes, flags, environment, extractor hashes and file identities. Routine fixture execution reads these frozen artifacts and needs no compiler.

Root rebuild command, only when explicitly authorized:

```
python3 engine/tests/fixtures/p2-memory-xchg8/build.py target/memory-byte-exchange-compiler-rebuild
```

## finite runtime contract

The new runner targets exact unprefixed86 memory exchange in four bound profiles. Its arithmetic byte-lane oracle covers AL/CL/DL/BL/AH/CH/DH/BH while preserving all other24 parent bits, full original effective addresses and complete FLAGS. It compares the entire4364-byte arena including dirty128-byte FP state on every generated call, and checks selected32-byte RAM windows and selected stack/code bytes separately. Public diagnostic reads are separate from forwarded guest helper observations.

The canonical exchange uses one synchronous exclusive engine borrow, one-byte Read then Write preflight, one version advance even for identity, one committed byte and returned old byte. All u32 addresses including FFFFFFFF are valid width1 addresses. Real faults are Read Unmapped/Permission or Write Permission; no byte span overflow or cross-page access is invented. The new40-byte R3MH version5 keeps old versions/layouts unchanged. Read8 uses existing V2 and DWORD stack read/store uses existing V1. Synthetic protocol/infrastructure packets never forward or commit a real exchange.

Changed and identity consumed-code exchanges must publish the returned byte/PC/one retirement before reason6, then reject the old owner4 before a fresh suffix capture. Later replacement generation3 is a different rejection. The compiler proof's two real calls retain stack/callee-saved parents and observe old byte EF→78 with cell78→EF; the desired argument's upper24 bits are ignored by the original compiler's byte load. Module blocks/instructions/bytes, resident limits and arena remain unchanged. Final actual counters and physical artifacts belong to the saved result, not this source artifact description.

The [Intel XCHG reference, printed6-28/29](https://cdrdv2-public.intel.com/789589/334569-sdm-vol-2d.pdf) supplies the instruction reference. This finite private-flat32 proof establishes no hardware bus locking, shared memory, cross-thread/C11 ordering, fences, whole-RAM dump, explicit LOCK/WORD/prefix aliases, performance gain, browser/game/SDK/graphics compatibility or hosted CI status. Local task: [R3-540](https://linear.app/ring3-dev/issue/R3-540/execute-compiler-byte-exchanges-in-one-memory-transaction).
