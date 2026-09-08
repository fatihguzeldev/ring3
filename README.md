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

## Raw delay-import metadata

`parse_pe_delay_import_descriptors` reads at most 128 raw descriptors in either
PE width and retains the all-zero terminator coordinates. Missing directories and
present empty tables remain distinct. Attribute and address words stay unclassified;
DLL names, lookup/IAT targets and delayed loading are outside this reader.

`parse_pe_delay_import_names` pairs those descriptors with exact borrowed ASCII DLL
names when attributes equal `1`. Other attribute values return an explicit error.
Name scans include the NUL terminator in their 1024-byte per-name and 65,536-byte
total limits; names are preserved without normalization or module resolution.

`parse_pe_delay_import_lookups` validates all supported DLL names before reading
explicit INT entries. It returns borrowed symbol names with hints or 16-bit ordinals,
using the static lookup reader's encoding, backing and scan limits. Missing INTs
return an error; IAT fallback, module resolution and delayed execution are excluded.

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
pnpm corpus:build:relocations
pnpm corpus:build:tls
pnpm corpus:build:delay
```

`corpus:verify` builds all eight fixture families twice and runs the 39 real-file
parser tests listed in `corpus/real-file-tests.json` against 18 fresh fixture paths.
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

The relocation builder uses `target/corpus-relocations/run-*` for PE32 and PE32+
images with two base-relocation blocks. Its `fixtures.json` lists both paths for
the raw block metadata tests; no relocations or guest code are executed.

The TLS builder uses `target/corpus-tls/run-*` for PE32 and PE32+ images with
fixed TLS directories. Its `fixtures.json` lists both paths for the raw directory
metadata tests; no TLS targets or callbacks are executed.

The delay builder uses `target/corpus-delay/run-*` for self-authored DLL/import-library
and delay-importing EXE pairs. Its `fixtures.json` supplies both EXE paths for raw
descriptor, supported DLL-name and named lookup tests. The link-only helper and generated PE
programs are never executed.
