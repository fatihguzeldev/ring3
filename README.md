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
load-config common prefix; raw AMD64 exception function records; the fixed raw CLR header with uninterpreted nested coordinates;
resource root headers, raw entries and borrowed Unicode
name bytes; bounded acyclic resource directory graphs with shared table identities;
fixed raw resource data-entry records and their leaf references;
borrowed name bytes across each directory’s named-entry prefix;
conservative borrowed resource payload ranges with explicit empty views;
and delay-import descriptors, supported DLL names and lookup symbols. Certificate entries use file
offsets and preserve opaque bodies and padding without validating signatures.
These readers inspect bytes without loading modules, binding addresses or executing
the generated programs. Reader types and limits live with their [implementation](core/src/pe/).
The CLR reader preserves flags and the entry-point word without validating metadata,
tokens or runtime compatibility; declared header tails are not read.

`admit_ascii_source_paths` checks a materialized list of relative ASCII file-path
strings with explicit count, per-path byte, total byte and depth limits. It changes
backslash separators to slash, retains original case and supplies an ASCII-lowercase
comparison key. Duplicate and file/ancestor collisions are rejected; output strings
are owned. Keys and indices describe this lexical list, not filesystem containment,
content identity or Windows names. See the [ASCII path contract](core/src/source/paths.rs).

`parse_pe_header_prefix_batch` accepts an immutable list of already materialized
byte slices and explicit file-count, per-file byte and total-byte limits. It
checks every budget before reading prefixes, then returns owned results in input
order. A malformed prefix stays in its file's result. Repeated references count
toward the logical byte total each time; these limits do not cap process memory.
See the [batch API and example](core/src/pe/header_batch.rs).

`inspect_pe_declared_evidence` collects owned header-prefix, optional-header and
CLR-header evidence as three independent reader outcomes. Each selected field
retains its physical file offset and byte width; a later reader error preserves
earlier successful fields. Undeclared CLR slots, declared zero descriptors and
CLR-reader errors remain distinct. These declarations do not establish runtime
requirements, support or loadability. See the
[declared evidence API](core/src/pe/declared_evidence.rs).

`fingerprint_pe_declared_evidence` checks a caller byte limit, hashes every input
byte with SHA-256, and collects owned declared evidence from that same slice.
Admitted empty or malformed images retain their reader errors alongside the hash.
The digest includes uninspected and trailing bytes; it does not authenticate a
source or implement the PE Authenticode hash. See the
[content fingerprint API](core/src/pe/fingerprints.rs).

`describe_pe_architecture_declarations` accepts existing owned evidence and names
two COFF and four CLR flag bits while retaining raw values, byte coordinates and
independent outcomes. It reads no bytes and does not authenticate caller-supplied
fields or coordinates. Absent or failed CLR evidence has no bit observations;
required/preferred bits remain raw observations without a runtime verdict. See
the [declaration projection](core/src/pe/architecture_declarations.rs).

`parse_ascii_pe_source_headers` accepts paired path/content records, admits all
paths first, then checks content budgets and reads each PE header prefix. Owned
results keep each path with its original content result; malformed prefixes do
not stop later entries. Path and content byte totals remain separate. Pairings
come from the caller and do not establish file provenance or image loadability.
See the [source composition](core/src/source/pe_headers.rs).

`inspect_ascii_pe_source_evidence` keeps the same path-first admission and logical
content budgets, then associates every admitted path with all three declared PE
evidence outcomes. Each field offset belongs to its paired content; errors in one
file do not discard other admitted files. Shared admission completes before each
source goes directly to the independent evidence inspector. See the
[named evidence API](core/src/source/pe_evidence.rs).

`fingerprint_ascii_pe_source_evidence` applies the same complete path/content
admission, then binds each admitted path to its whole-input fingerprint result.
Algorithm-length refusals stay in their entries; aliases count and hash for every
occurrence. Paths are excluded from digests. See the
[named fingerprint API](core/src/source/pe_fingerprints.rs).

`inspect_ascii_pe_source_module_evidence` admits the full path/content list, then
keeps each owned path with its whole-input fingerprint and independent owned
static-import, delay-import and export results. The same family output caps apply
fresh to every occurrence; errors remain local to their entry or family. See the
[named module evidence API](core/src/source/pe_modules.rs).

`inspect_pe_static_imports` retains owned static DLL and symbol metadata as two
independent descriptor/lookup results. Input admission precedes reading; complete
row and text admission precedes owned copying. Duplicate text counts for each
copy, and a lookup error preserves readable DLL declarations. These observations
outlive input bytes without selecting providers or inferring requirements. See
the [owned import evidence API](core/src/pe/imports/evidence.rs).

Related PE readers are grouped under [imports](core/src/pe/imports.rs),
[exports](core/src/pe/exports.rs), and [resources](core/src/pe/resources.rs).
Delay imports live inside the [imports family](core/src/pe/imports/delay.rs).
Each family keeps its readers and selection operations together; the public
`ring3_core` API remains available through the crate root.


`inspect_pe_exports` retains three independent owned directory, address and name
results after input release. Names keep their complete nested address table;
standalone and nested entries and text copies each count toward output limits.
Input admission precedes all readers; complete row and text admission precedes
new owned copies, while existing reader allocations occur earlier. All metadata,
target variants, raw forwarder spelling, absence and typed errors stay distinct.
Directory DLL-name pointers remain metadata; direct target content is not read.
See the [owned export evidence API](core/src/pe/exports/evidence.rs).

`lookup_pe_export_evidence` queries retained export metadata after image release.
It admits actual rows and text bytes in the required view, then validates every
count, positional index, ordinal and name-to-address reference before selection.
Name queries use nested addresses; ordinal queries use standalone addresses.
Unrelated views and reported totals are ignored. Results borrow evidence and
preserve opaque metadata; these checks do not establish PE validity, provenance,
cross-view coherence or allocation/time bounds. See the
[owned export query API](core/src/pe/exports/evidence_lookup.rs).


`inspect_pe_module_evidence` binds the whole-input fingerprint and declared,
static import, delay import and export evidence to one input slice. It retains
owned, independent family results even when another family exceeds its row/text
limits. Shared fingerprint input admission runs first; family output limits are
separate and do not cap total allocations. This is metadata evidence, not a
complete module report, provider resolution or authentication guarantee.
See the [same-input module evidence API](core/src/pe/module_evidence.rs).


`lookup_pe_export` selects metadata by exact name or full biased export ordinal
inside one supplied image. Name queries preserve every duplicate matching row;
ordinal queries use the address table without reading export names. The result
borrows name and forwarder text from the input and retains empty slots, raw RVAs
and unresolved forwarders. `PeExportLookup::new(bytes)` keeps an owner for repeated
queries on the same immutable image. Its `lookup` method retains name and address
reader results independently, including absence and errors; construction performs
no parsing. Results can outlive the owner, while the image must remain alive.
Mixed queries share one retained address table; name entries remain lazy. See the
[selection API](core/src/pe/exports/lookup.rs).

`decode_pe_forwarder_request` separates conventional `module.name` or
`module.#ordinal` text into borrowed request metadata under a caller byte limit.
It accepts visible ASCII with exactly one dot, preserves module/name spelling and
leading ordinal zeroes, and reports typed refusals with byte offsets. Multiple dots
and other unsupported syntax remain uninterpreted; refusal does not establish
Windows validity. This helper performs no module lookup, path normalization or
forwarder traversal. See the [request decoder](core/src/pe/exports/forwarder.rs).

`walk_pe_export_forwarders` follows one export query across an immutable image
list using caller-provided, source-specific routes. Source indices identify list
positions for that call; routes match exact module text. The caller supplies source,
route, hop, selection-row and aggregate text limits. The walk retains raw metadata
and ordered steps, stops at terminal selections, and rejects repeated source/EAT
entries even when query spelling changes. Errors return no partial walk. Results
own name queries and borrow raw text from images; fresh calls create fresh owners.
This does not discover DLLs, normalize names, bind targets or execute code. See the
[bounded walk contract](core/src/pe/exports/walk.rs).

`lookup_pe_export_batch` adds explicit query-count and total selection-row limits
to an ordered query list. Each selected target costs one logical row; ambiguity
costs every matching row. Other outcomes and per-query parse errors cost zero,
while still counting as queries. Budget refusal returns no partial batch. Each
query may allocate its own result before the row check, so this is not a byte or
process-memory cap. `PeExportLookup::lookup_batch` applies the same rules across repeated batches on
one image owner. Each batch starts fresh query/row limits and reuses retained
address/name results. Count refusal and empty batches leave tables untouched;
row refusal may retain tables for later calls. Results borrow the image and may
outlive the owner. See the [export batch API](core/src/pe/exports/batch.rs).

`parse_pe_amd64_exception_functions` reads at most 4096 raw 12-byte records
from exception directory slot 3 in AMD64 PE32+ files. It preserves record order,
zero entries, duplicates and all three RVA fields without following function or
unwind targets. The directory RVA must be four-byte aligned; its complete table
must have one conservative file-backed range. An absent slot is accepted for any
otherwise accepted image. The result owns scalar metadata. This is not unwind
validation, exception execution, or an input or peak-memory budget; see the
[exception reader API](core/src/pe/amd64_exceptions.rs) for error precedence.

`parse_pe_amd64_unwind_info_v1` reads one explicit, four-byte aligned RVA in an
AMD64 PE32+ file, independently of exception-directory membership. The owned
result separates raw code slots from odd-count padding and preserves frame,
prolog and fixed handler/chain coordinates. The entire envelope must occupy one
conservative file-backed range; its declared slot count bounds it to 528 bytes.
Only version 1 and structural flag values 0–4 are admitted. Opcode meaning,
frame validity, handler data and target contents remain unchecked. No target
is followed and no unwinding is performed. See the
[v1 unwind API](core/src/pe/amd64_exceptions/unwind.rs) for fields and error order.

`lookup_pe_import_exports` matches one descriptor's ordered import symbols against
an explicitly supplied provider image. It validates all importer lookups first,
then selects the descriptor by its zero-based index and runs one export batch.
Import hints remain metadata; names use exact matching and ordinals keep all
16 bits. Provider errors stay aligned with the corresponding import entries.
Query and selection-row limits apply after import parsing; they do not bound
input bytes or temporary allocations. Each image owns the lifetime of its own
borrowed text. This is static metadata matching without module discovery,
architecture compatibility checks or address binding. See the
[import/export API](core/src/pe/imports/export_batch.rs).
`lookup_pe_import_exports_with_provider` accepts an existing `PeExportLookup`
so calls for multiple importers or descriptors can reuse one caller-selected
provider's tables. Each call has fresh query/row limits, and returned metadata
can outlive the owner while retaining each input image's independent lifetime.

`inspect_pe_delay_imports` retains three independent owned results for the raw
delay table, DLL names and lookup symbols. Each keeps absence, present-empty
table metadata and typed errors distinct. Input admission precedes the readers;
complete row and text admission precedes new text/conversion copies. Existing
reader allocations occur earlier, and the already-owned raw table moves into
the result. Repeated text counts per occurrence; these limits do not cap process
memory. Exact metadata survives input release, including readable raw/name
results when later stages fail. See the
[owned delay evidence API](core/src/pe/imports/delay/evidence.rs).

`lookup_pe_delay_import_exports` applies the same explicit-provider matching to
one delay-import descriptor. It validates the complete delay table, DLL names
and lookup entries before selection. Unsupported attributes and unavailable INTs
retain the raw reader's errors; INTs never fall back to IAT bytes. Each result
keeps the selected delay metadata and its ordered provider selections, with
independent image lifetimes and the same query/row budget scope. See the
[delay import/export API](core/src/pe/imports/delay/export_batch.rs).
`lookup_pe_delay_import_exports_with_provider` accepts a retained `PeExportLookup`
with the same independent image lifetimes and per-call budgets as the static
counterpart. Complete delay validation still precedes provider matching.


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
compile the source twice and verify the resulting bytes. Debug fixture PDB sidecars
are checked as whole files; their symbols are not decoded. Generated EXE/DLL files
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
| [pe-amd64-exceptions](corpus/pe-amd64-exceptions/) | AMD64 DLLs with two raw exception function records and their v1 unwind metadata, or an absent table. | `pnpm corpus:build:exceptions` |
| [pe-debug-payloads](corpus/pe-debug-payloads/) | Linked raw debug metadata, opaque CodeView bytes and empty REPRO records. | `pnpm corpus:build:debug` |
| [pe-managed](corpus/pe-managed/) | Unpatched self-authored managed PE32 and PE32+ files with raw CLR headers. | `pnpm corpus:build:managed` |

Native fixture builders require the pinned macOS Apple Clang/LLVM and Rust LLD binaries recorded
in each manifest. Tool identity checks intentionally fail when those binaries
change; review and update the pins when changing the fixture toolchain.
Debug fixture repeatability also depends on the pinned local tool installation
paths retained inside PDB sidecars. Object timestamps are zero; the PE/debug
timestamps are stable content-derived values.

Managed fixtures require the existing macOS arm64 .NET SDK `10.0.401` recorded in
[their manifest](corpus/pe-managed/fixture.json). Set `RING3_DOTNET_ROOT` to its
extracted root before running `corpus:build:managed`, `corpus:test` or
`corpus:verify`.
The producer verifies pinned host, compiler, runtime and reference bytes, then
invokes the compiler directly with scoped CLI/temp directories. It performs no
SDK download, package restore or generated PE execution. Reproducibility is
limited to the pinned local toolchain; the output PE timestamps are nonzero.


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
The regular suite includes a fixed 6,148-input mutation campaign across 28 raw PE
readers, including present and absent AMD64 exception tables and v1 unwind metadata
at one explicit RVA. It checks repeated results, input preservation and owned
unwind record fields, with one synthetic handler seed. This finite campaign
does not replace semantic tests or coverage-guided fuzzing.

## Corpus verification

```bash
pnpm corpus:test
pnpm corpus:verify
```

`corpus:test` runs the producer and verification guard tests, including changed
inputs, tool failures and output-directory ownership checks.

`corpus:verify` builds the registered fixture families twice and runs the tests listed
in [real-file-tests.json](corpus/real-file-tests.json) against fresh file paths,
including named and ordinal delay-import lookups and explicit EXE/provider pairs,
explicit forwarder/ordinal-provider walks with named-miss and ordinal-selected terminals,
paired ASCII source/header results and admission refusals across both PE widths,
linked resource directory graphs,
fixed resource data-entry records and unpatched managed CLR headers. Four direct
declared-evidence cases retain complete owned fields and physical byte ranges
from unpatched PE32/PE32+ and managed files, including declared zero CLR
descriptors and present CLR headers. Two named-evidence groups retain mixed
compiled pairings, aliases, reordered records, empty-content errors and exact
path-before-content admission refusals. Four direct architecture-declaration
cases retain exact raw fields and named COFF/CLR bit observations from the same
unpatched native and managed files.
Four direct content-fingerprint cases retain exact whole-file SHA-256 digests
and owned declared evidence from these same unpatched files.
Two named-fingerprint groups retain owned mixed and aliased compiled results,
reordered path associations and exact path-before-content admission refusals.
Two owned static-import groups retain compiled named/ordinal metadata after input
release and exact input, row and text budget refusals across both PE widths.
Two owned delay-import groups retain all three metadata tables and named/ordinal
entries after input release, with the same exact budget order and operands.
Two owned export groups retain complete named, ordinal-only and sparse-forwarder
metadata after input release, including nested address tables and exact budget
refusal operands.
Certificate-entry tests append synthetic records to generated PE files in memory;
the source fixtures remain unchanged and are not cryptographically signed.
Two module-evidence groups retain whole-file fingerprints and complete owned
static/delay/export observations across six compiled inputs, including input
release and independent family output-refusal operands.
Two named-module groups retain mixed path/content pairings, aliases and owned
results across the same six inputs, with exact whole-list admission and shared
per-family output-refusal operands.
Two owned-query groups use the existing named, ordinal-only and sparse-forwarder
DLLs to retain complete selections after image release, exact query-view row/text
caps and independent reader refusals, separately from collection output totals.

The verifier requires pinned native Rust tools on `aarch64-apple-darwin`, compiles
the Rust tests offline into a fresh Cargo target, and rejects missing, extra or
skipped cases. Its `verification.json` is written only after every check passes.
The command prints the evidence directory and fixture paths needed for inspection.

Generated output is temporary. Preserve required run evidence in the Ring3
Obsidian bank before removing completed run directories and copied workspaces.
Keep source files and manifests in Git; keep research and task history in the bank.
