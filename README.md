# ring3

Ring3 is a compatibility-runtime project. The current implementation is a Rust
core that reads Windows executable (PE) metadata, with self-authored test files
and tools that verify the results. Guest execution and TypeScript host integration
remain future work.

## Repository map

| Location | Purpose |
| --- | --- |
| [core/src/](core/src/) | Rust readers, address types and PE metadata types. |
| [core/tests/](core/tests/) | Rust tests for valid files, malformed input and reader limits. |
| [runtime/](runtime/) | TypeScript host package; currently a package skeleton. |
| [corpus/](corpus/) | Small test-program sources and their expected properties. |
| [tools/corpus/](tools/corpus/) | Fixture builders, verification runner and their colocated tests. |
| [tools/verify-toolchain.mjs](tools/verify-toolchain.mjs) | Node.js and package-manager version checks. |
| `target/` | Generated Rust builds, EXE/DLL samples and test output; ignored by Git. |

The [core exports](core/src/lib.rs) cover PE32/PE32+ headers, sections and file
ranges; static import/export metadata; raw base-relocation blocks; fixed TLS
directories; raw certificate tables and entry metadata; raw debug directory
metadata and borrowed payload file ranges independent of their RVA fields; the supported size-bearing
load-config common prefix; resource root headers, raw entries and borrowed Unicode
name bytes; bounded acyclic resource directory graphs with shared table identities;
fixed raw resource data-entry records and their leaf references;
borrowed name bytes across each directory’s named-entry prefix;
conservative borrowed resource payload ranges with explicit empty views;
and delay-import descriptors, supported DLL names and lookup symbols. Certificate entries use file
offsets and preserve opaque bodies and padding without validating signatures.
These readers inspect bytes without loading modules, binding addresses or executing
the generated programs. Reader types and limits live with their [implementation](core/src/pe/).

## Fixture corpus

A fixture is a small input with known properties, used to test a reader. Each
family keeps its source files and `fixture.json` together:

```text
corpus/
  pe-named-imports/
    fixture.json
    imports.c
    probe.c
  ...
  real-file-tests.json
```

`fixture.json` records source paths, expected metadata and pinned source/tool/output
identities. It is maintained test input, not a run log. The builders read it,
compile the source twice and verify the resulting bytes. Generated EXE/DLL files
and per-run evidence go under `target/`; they are not committed.

| Family | Input being checked | Build command |
| --- | --- | --- |
| [pe32-arithmetic](corpus/pe32-arithmetic/) | Minimal 32-bit EXE. | `pnpm corpus:build` |
| [pe32plus-arithmetic](corpus/pe32plus-arithmetic/) | Minimal 64-bit EXE with wide header values. | `pnpm corpus:build:pe32plus` |
| [pe-named-imports](corpus/pe-named-imports/) | EXE/DLL pairs with symbols imported by name. | `pnpm corpus:build:imports` |
| [pe-ordinal-imports](corpus/pe-ordinal-imports/) | EXE/DLL pairs with symbols imported by number. | `pnpm corpus:build:ordinals` |
| [pe-forwarders](corpus/pe-forwarders/) | DLL exports that refer to another module's symbols. | `pnpm corpus:build:forwarders` |
| [pe-base-relocations](corpus/pe-base-relocations/) | Files containing relocation block metadata. | `pnpm corpus:build:relocations` |
| [pe-tls](corpus/pe-tls/) | Files containing fixed thread-local storage directories. | `pnpm corpus:build:tls` |
| [pe-delay-imports](corpus/pe-delay-imports/) | Delay-import metadata with named symbols. | `pnpm corpus:build:delay` |
| [pe-delay-ordinals](corpus/pe-delay-ordinals/) | Delay-import metadata with ordinal `32768`. | `pnpm corpus:build:delay-ordinals` |
| [pe-resources](corpus/pe-resources/) | Linked resource roots and raw Unicode name bytes. | `pnpm corpus:build:resources` |

Builders require the pinned macOS Apple Clang/LLVM and Rust LLD binaries recorded
in each manifest. Tool identity checks intentionally fail when those binaries
change; review and update the pins when changing the fixture toolchain.

## Development

Use Rust/Cargo `1.97.1`, the `wasm32-unknown-unknown` target, Node.js `22.16.0`,
pnpm `11.9.0` and TypeScript `5.9.3`.

```bash
pnpm install --frozen-lockfile
pnpm toolchain:check
pnpm format:rust
pnpm lint:rust
pnpm test:rust
pnpm typecheck
pnpm build
```

`test:rust` runs the regular Rust tests and documentation tests. Tests that require
generated EXE/DLL files run through the corpus verifier below.
The regular suite includes a fixed 3,074-input mutation campaign across the PE
readers, checking repeated results and input preservation. This finite campaign
does not replace semantic tests or coverage-guided fuzzing.

## Corpus verification

```bash
pnpm corpus:test
pnpm corpus:verify
```

`corpus:test` runs the producer and verification guard tests, including changed
inputs, tool failures and output-directory ownership checks.

`corpus:verify` builds all ten fixture families twice and runs all 63 tests listed
in [real-file-tests.json](corpus/real-file-tests.json) against 22 fresh file paths,
including named and ordinal delay-import lookups, linked resource directory graphs
and fixed resource data-entry records.
Certificate-entry tests append synthetic records to generated PE files in memory;
the source fixtures remain unchanged and are not cryptographically signed.

The verifier requires pinned native Rust tools on `aarch64-apple-darwin`, compiles
the Rust tests offline into a fresh Cargo target, and rejects missing, extra or
skipped cases. Its `verification.json` is written only after every check passes.
The command prints the evidence directory and fixture paths needed for inspection.

Generated output is temporary. Preserve required run evidence in the Ring3
Obsidian bank before removing completed run directories and copied workspaces.
Keep source files and manifests in Git; keep research and task history in the bank.
