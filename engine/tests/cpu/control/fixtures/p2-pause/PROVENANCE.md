This header-free polling leaf is self-authored. Routine tests consume its frozen
40-byte text directly; they do not invoke a compiler.

The source is `kernel.c` (SHA256
`8150ee0cbe5c2053d1c5c5112a1fd023c4a56ce9134aa7db9f93ab823b06f2c7`).
Two fresh builds used the unchanged i686 scalar profile and strict COFF extractor
from `p2-compiled-records/build.py`, SHA256
`fe426f202b48664a87357c766d39099b9d35b5d8217c2b114696164a0d468258`.
The disposable driver and full input/tool/command records are preserved in
`target/engine-pause-2026-10-11`; the copied first receipt is `receipt.json`.

Both original objects and extracted text agreed. The text SHA256 is
`2709e35459b460e6587fbbc0fc1428bac4ec3457d69e844e5d94482a305ed1b6`.
The original disassembly is `kernel.disasm.txt`. The object has one function,
zero timestamp, and no relocation or unresolved external. PAUSE appears unchanged
at offset `0x1a`. The 20 instructions form seven blocks beginning at offsets
`0, 0x10, 0x14, 0x1a, 0x1f, 0x21, 0x23`.

The compiler witness alone establishes neither engine admission nor execution.
The sibling actual-Wasm test separately executes this exact text with bounded
budgets, caller-owned stack arguments, canonical memory helpers, and a controlled
host write to the ready word between calls. It makes no concurrency, synchronization,
host-yield, timing, power, performance, or generic compiler compatibility claim.
