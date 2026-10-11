# original relocatable PE32 fixture

The retained `i386.s`, object, executable and inspection output are unchanged
outputs of the existing self-authored relocation-fixture producer. They were
copied from the first of two fresh, byte-identical root-owned builds on
2026-10-11 at 12:17:32 UTC. No PE byte rewriting, section reconstruction or new
producer is involved. The historical `fixture.json` and fresh `evidence.json`
remain byte-exact, including `licenseStatus: pending`, `windowsOracle: not-run`
and `executed: false`. Those historical receipt fields do not describe the
separate Ring3 execution proof in `run.mjs`.

| retained file | bytes | SHA-256 |
| --- | ---: | --- |
| `i386.s` | 196 | `de14453ed6b7f5326f51d67acbcd55050987b1b7c314a099d9ee17ba27959365` |
| `relocations.obj` | 4483 | `19758a9c75c1c4f03d7c7f720225182af160d2545187bc5b71ff3c26022540b5` |
| `relocations.exe` | 6144 | `ca36f32c699b3e3200da8cc6d23fe7465f14e5df9fdedd425e9da9fb2a395818` |
| `inspection.txt` | 18414 | `fe5718b96e7dc68f72714245879516f7dcf91d71b08284fbd33f2ab3c7bc1340` |

The assembly was introduced by `fatihguzel` in commit
`891f87a9ad95e6437a84fdbeae126e1dc921f468` on 2026-09-08,
`build(corpus): add reproducible PE relocation fixtures`. It is byte-identical
to the source in commit `9d5b03c65854e9576cd42e26581e8d1bfb039c3b`,
which added the repository Apache-2.0 `LICENSE` and `NOTICE` on 2026-09-25.
The notice identifies Copyright 2026 Fatih Güzel. The assembly has no separate
file-level license header. This records the observed repository-license history
without upgrading the historical fixture's pending license field. The producer
and sources were removed in the prototype rebuild cleanup
`7f8471679293784ebbb8bb5acb04d345a50c5df9` on 2026-10-01; they remain recoverable
from history and the preserved corpus baseline. The fixture manifest moved
beside its source in `aa234942593a4083cea7ac811dcb36502ce3137e`.

`producer-source-pins.json` records the six exact recovered producer inputs:
`tools/corpus/build-relocations.mjs`, `tools/corpus/shared.mjs`,
`corpus/pe-base-relocations/i386.s`, `corpus/pe-base-relocations/amd64.s`,
`corpus/pe-base-relocations/fixture.json` and `.node-version`. The verified
recovery snapshot is commit `c1fbceedaa7baadbe47a0c13e1e6474b58a520f6`.
The unchanged CLI is invoked with no extra arguments from that recovered tree:

```sh
LC_ALL=C TZ=UTC /Users/fatihguzel/.nvm/versions/node/v22.16.0/bin/node tools/corpus/build-relocations.mjs
```

The root-owned fresh recovery is under
`target/engine-relocatable-pe-declarations-2026-10-11/producer-source/`.
Its first and second original outputs are under
`target/corpus-relocations/run-956fpk/{first,second}/i386/` within that tree.
The producer copies the source to `relocations.s`, pins all tool hashes and
versions, creates both fresh builds, and compares object, EXE, inspection and
receipt bytes. It also reproduces an unused AMD64 fixture; that is not PE32+
engine-admission evidence. `evidence.json` preserves the exact tool versions,
all argument arrays, and the `darwin/arm64`, Node 22.16.0, `LC_ALL=C`, `TZ=UTC`
environment. The i386 commands are:

```text
clang --target=i686-pc-windows-msvc -c relocations.s -o relocations.obj
rust-lld -flavor link /machine:x86 /nodefaultlib /timestamp:0 /fixed:no /dynamicbase /nxcompat /base:0x400000 /entry:entry /subsystem:console /out:relocations.exe relocations.obj /safeseh:no
llvm-objdump --private-headers --section-headers --full-contents relocations.exe
```

The resolved tools and SHA-256 values were:

| tool | resolved path | SHA-256 |
| --- | --- | --- |
| Apple clang 21.0.0 | `/Library/Developer/CommandLineTools/usr/bin/clang` | `f30550eab15fdf5ab8c0dc54c52679711241e5d4b636b027e18c09fef531775d` |
| LLD 22.1.6 | `/Users/fatihguzel/.rustup/toolchains/1.97.1-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-lld` | `8f5fe507df7232eac0a610c12a5d11f3202205235f710eb98c6c78df6d3f548d` |
| Apple LLVM 21.0.0 | `/Library/Developer/CommandLineTools/usr/bin/llvm-objdump` | `1dcc1432db4d828cf8cd71a84cb8d9bf63a7ca80b5eeec711a9e5f13a3f5f98e` |
| Node 22.16.0 | `/Users/fatihguzel/.nvm/versions/node/v22.16.0/bin/node` | `a45751fbfe88440bebff63cd44814e4ed6deb642bc3e3c4a14c4f8ae0ed9e019` |

The EXE has timestamp zero, I386/PE32 headers, preferred base `0x400000`, image
size `0x5000`, entry RVA `0x1000`, and DLL characteristics `0x8140`.
The original complete entry is `31 c0 c3` (`XOR EAX,EAX; RET`). The only data
directory is the 24-byte HIGHLOW directory at RVA `0x4000`. Its three source
RVAs `0x2008`, `0x200c`, `0x3010` initially hold `0x00402000`. Explicit bases
`0x400000`, `0x500000`, `0x300000` therefore yield pointers `0x402000`,
`0x502000`, `0x302000` respectively. The data anchor at RVA `0x2000` remains7.
All original headers, raw section bytes, zero-filled tails and relocation-table
bytes remain otherwise unchanged.

The mapped image comprises five pages: headers R, text RX, two data RW pages,
and a retained R `.reloc` page. `DYNAMIC_BASE` is accepted under the existing
explicit caller-selected placement policy; placement is not randomized.
`MEM_DISCARDABLE` is accepted while retaining the page for the image lifetime.
The original is a Relocated-profile consumer: Fixed still rejects its nonempty
relocation directory and Linked still rejects its absent imports.

The actual runner stages the original as 4096+2048 bytes and uses three explicit
bases with both replacement and resident compiled owners. Each path has a
separate authored caller at `0x8000`, a stack/output page at `0x9000`, one
initial CPU input after load/compile, and budgets1/2/2. Guest CALL enters the
original function, its natural RET returns zero, then the caller stores that
EAX to `0x9040`. No original image byte is written by the host or guest after
loading. Whole five-page image bytes, the 128-byte authored stack frame and
12-byte caller are checked before and after execution. Full4364-byte arena
assertions preserve dirty FP128 throughout; nondiagnostic actions retain full
before/after files. Public read32 diagnostics are separate from the eighteen
forwarded guest helpers. One capture per context uses three blocks and five
instructions under unchanged default limits. A closed `NO_ISOLATION` bit
sidecopy proves neutral staged refusal/retention/abort; the unmodified original
then proves invalid-base refusal and same-input valid-base retry.

This is one bounded self-authored import-free PE consumer. It provides no
Windows execution oracle, automatic ASLR, discard lifecycle, whole application,
hardware concurrency, performance, SDK, browser or game-support claim.
