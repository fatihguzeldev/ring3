# ring3

## Toolchains

- Rust/Cargo: `1.97.1`
- WebAssembly target: `wasm32-unknown-unknown`
- Node.js: `22.16.0`
- pnpm: `11.9.0`
- TypeScript: `5.9.3`

## Verification

```bash
pnpm install --frozen-lockfile
pnpm toolchain:check
pnpm format:rust
pnpm lint:rust
pnpm typecheck
pnpm build
```

## Static PE fixtures

The corpus builders require the pinned macOS Apple Clang/LLVM and Rust LLD binaries
recorded in `corpus/*.json`. They verify source and tool hashes, build each fixture
twice, and compare the output bytes without executing the guest programs.

```bash
pnpm corpus:test
pnpm corpus:verify
pnpm corpus:build
pnpm corpus:build:imports
pnpm corpus:build:ordinals
pnpm corpus:build:forwarders
pnpm corpus:build:pe32plus
```

`corpus:verify` builds all five fixture families twice and runs the 29 real-file
parser tests listed in `corpus/real-file-tests.json` against 12 fresh fixture paths.
It requires the pinned native Rust tools on `aarch64-apple-darwin`, compiles tests
offline into a fresh Cargo target, and refuses missing, extra or skipped cases.
`target/corpus-verification/run-*/verification` retains tool/source hashes, child
logs and fixture evidence; `verification.json` appears only after every case passes.

The named import builder produces self-authored PE32 and PE32+ EXE/DLL pairs under
a fresh `target/corpus-imports/run-*` directory. Its `fixtures.json` lists the four
environment variables used by the named import/export Rust integration tests;
`repeatability.json` links both build records. Generated binaries remain untracked.

The ordinal builder uses `target/corpus-ordinals/run-*` for the corresponding
ordinal-32768 EXE/DLL pairs. Its `fixtures.json` lists the four ordinal test paths.

The forwarder builder uses `target/corpus-forwarders/run-*` for sparse export DLLs
with named and ordinal target strings. Its `fixtures.json` lists both DLL test paths.

The PE32+ header builder uses `target/corpus-pe32plus/run-*` for the arithmetic
fixture with stack/heap header values above 32 bits. Its `fixtures.json` provides
`RING3_PE32PLUS_FIXTURE` for the header, section and RVA tests.
