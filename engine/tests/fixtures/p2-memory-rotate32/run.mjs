import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected current engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const {bytes: engineBytes, module: engineModule, sha256: engineSha256} = readEngine(enginePath);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = PC + 128, KEEP = 0x3000, DATA = 0x4000, SECOND = 0x5000, TOP = 0xfffff000, WORD = 0x4010;
const OWNERS = ['replacement', 'resident'], FLAGS = [2, 0xcd7], KINDS = ['rol', 'ror'], RAW = [0, 1, 2, 31, 32, 33, 255], VALUES = [0x80000001, 0x12345678];
const REG = [0x12345678, 0x87654321, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const ANCHORS = [['rol', 0x80000001, 1, 3, 0x803], ['ror', 0x80000001, 1, 0xc0000000, 3],
  ['rol', 0x80000001, 31, 0xc0000000, 2], ['ror', 0x80000001, 31, 3, 2],
  ['rol', 0x40000000, 2, 1, 3], ['ror', 2, 2, 0x80000000, 3], ['rol', 0, 16, 0, 2], ['ror', 0xffffffff, 2, 0xffffffff, 3]];
function rotate(kind, destination, raw, incoming) {
  const count = raw % 32;
  if (count === 0) return {value: destination, flags: incoming, carry: incoming % 2, count};
  const bits = destination.toString(2).padStart(32, '0').split('').map(Number);
  for (let step = 0; step < count; step++) {if (kind === 'rol') bits.push(bits.shift()); else bits.unshift(bits.pop());}
  const value = Number.parseInt(bits.join(''), 2), carry = kind === 'rol' ? bits[31] : bits[0];
  const overflow = count === 1 ? (kind === 'rol' ? bits[0] ^ carry : bits[0] ^ bits[1]) : 0;
  return {value, flags: (incoming & 0x4d6) + carry + overflow * 0x800, carry, count};
}
for (const [kind, old, raw, value, flags] of ANCHORS) assert.deepEqual(rotate(kind, old, raw, 2), {value, flags, carry: flags % 2, count: raw % 32}, 'independent literal rotate anchor');
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
function bank(name, kind, forms, descriptors, packed, instructions, entries) {
  const bytes = Buffer.alloc(130, 0xcc), scans = [], specs = []; let offset = 0;
  for (const [index, form] of forms.entries()) {
    const descriptor = descriptors[index], tail = [0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, ...(descriptor.canary ? [0xbb, 0xef, 0xbe, 0xad, 0xde] : [])];
    const length = form.length + tail.length + 2, entry = PC + offset;
    bytes.set([...form, ...tail, 0xeb, 128 - offset - length], offset);
    scans.push({entry, target: entry + (descriptor.prefix ?? 0), next: entry + form.length, kind, ...descriptor}); specs.push([entry, length]); offset += length;
  }
  assert.equal(offset, packed); assert.deepEqual(scans.map(scan => scan.entry - PC), entries);
  assert.ok(specs.length <= 8 && instructions <= 64); bytes.set([0x0f, 0x0b], 128);
  assert.deepEqual(readFileSync(join(output, `${name}.x86`)), bytes, 'independently authored Rust/JS whole130B bank');
  return {name, kind, bytes, packed_bytes: packed, physical_bytes: bytes.length, specs, scans, instructions};
}
const banks = [];
for (const kind of KINDS) {
  const extension = kind === 'rol' ? 0 : 8;
  const normal = RAW.map(raw => [0xc1, 5 | extension, 0x10, 0x40, 0, 0, raw]); normal.push([0xd3, 5 | extension, 0x10, 0x40, 0, 0]);
  banks.push(bank(`normal-${kind}`, kind, normal, [...RAW.map(raw => ({mode: 'imm', raw, address_kind: 'absolute'})), {mode: 'cl', address_kind: 'absolute'}], 119, 32, [0, 15, 30, 45, 60, 75, 90, 105]));
  const target = kind === 'rol' ? [0xc1, 7, 32] : [0xd3, 0x0f];
  const forms = [[0xd3, 1 | extension], [0xd3, 0x44 | extension, 0x8f, 0xf0], [0xd3, 4 | extension, 0x89],
    [0xd3, 4 | extension, 0x24], [0xd3, 0x45 | extension, 0], [0x83, 0xc3, 1, ...target], [0xbe, 1, 0, 0, 0x80, ...target]];
  const descriptors = Array.from({length: 5}, (_, alias) => ({mode: 'cl', alias, address_kind: 'alias'}));
  descriptors.push({mode: kind === 'rol' ? 'imm' : 'cl', raw: kind === 'rol' ? 32 : 2, prefix: 3, address_kind: 'edi'},
    {mode: kind === 'rol' ? 'imm' : 'cl', raw: kind === 'rol' ? 32 : 2, prefix: 5, canary: true, address_kind: 'edi'});
  banks.push(bank(`special-${kind}`, kind, forms, descriptors, kind === 'rol' ? 90 : 88, 31, [0, 10, 22, 33, 44, 55, kind === 'rol' ? 69 : 68]));
}
const faultShapes = [
  {name: 'cross-read-unmapped', detail: 1, fault: SECOND, access: 1, missing: SECOND},
  {name: 'cross-read-denied', detail: 2, fault: SECOND, access: 1, denied: SECOND, permissions: 2},
  {name: 'cross-first-read-only', detail: 2, fault: 0x4ffe, access: 2, denied: DATA, permissions: 1},
  {name: 'cross-second-read-only', detail: 2, fault: SECOND, access: 2, denied: SECOND, permissions: 1},
];
const formulas = {normal_targets: OWNERS.length * KINDS.length * VALUES.length * RAW.length * 2 * FLAGS.length,
  alias_targets: OWNERS.length * KINDS.length * 5 * FLAGS.length, range_targets: OWNERS.length * KINDS.length * 2 * FLAGS.length,
  repaired_targets: OWNERS.length * KINDS.length * faultShapes.length * FLAGS.length, smc_targets: OWNERS.length * KINDS.length};
const plannedTargets = Object.values(formulas).reduce((sum, value) => sum + value, 0), regularTargets = plannedTargets - formulas.smc_targets;
const overflowCases = OWNERS.length * KINDS.length * FLAGS.length, faultCalls = (formulas.repaired_targets + overflowCases) * 2;
const normalContexts = OWNERS.length * KINDS.length, regularContexts = normalContexts * 2;
const planned = {targets: plannedTargets, regular_targets: regularTargets, overflow_cases: overflowCases, contexts: regularContexts + formulas.smc_targets,
  modules: regularContexts + formulas.smc_targets * 2, seeds: plannedTargets + overflowCases,
  generated_calls: regularTargets * 4 + faultCalls + normalContexts * 3 + regularContexts + formulas.smc_targets * 7,
  retired: regularTargets * 4 + formulas.repaired_targets + overflowCases + formulas.smc_targets * 6,
  raw_arenas: plannedTargets * 2 + faultCalls + formulas.smc_targets * 6 + normalContexts * 3 + regularContexts,
  pages: normalContexts * 6 + OWNERS.length * KINDS.length * 120 + formulas.smc_targets * 6,
  source_spans: normalContexts * 8 + OWNERS.length * KINDS.length * 7 + formulas.smc_targets * 8,
  maps: 56, unmaps: 16, protects: 152, uploads: 380, uploaded_bytes: 197924, host_calls: 700, transfer_requests: 396, host_inputs: 740, arena_checks: 1087644};
planned.raw_bytes = planned.raw_arenas * SIZE; planned.diagnostic_read32_calls = planned.pages * 1024 + planned.targets;
planned.files = planned.pages + planned.modules + banks.length + formulas.smc_targets * 2 + 4;
assert.deepEqual([planned.targets, planned.regular_targets, planned.overflow_cases, planned.contexts, planned.modules, planned.seeds, planned.generated_calls,
  planned.retired, planned.raw_arenas, planned.raw_bytes, planned.pages, planned.source_spans, planned.diagnostic_read32_calls, planned.files],
  [316, 312, 8, 12, 16, 324, 1376, 1312, 756, 3202416, 528, 92, 540988, 560]);
const counts = {contexts: 0, modules: 0, seeds: 0, targets: 0, normal_targets: 0, alias_targets: 0, range_targets: 0, repaired_targets: 0, smc_targets: 0,
  rol_targets: 0, ror_targets: 0, imm_targets: 0, cl_targets: 0, zero_targets: 0, count_one_targets: 0, multiple_count_targets: 0,
  generated_calls: 0, retired: 0, raw_arenas: 0, arena_checks: 0, pages: 0, diagnostic_read32_calls: 0, target_read32_calls: 0, maps: 0, unmaps: 0,
  protects: 0, uploads: 0, uploaded_bytes: 0, host_calls: 0, transfer_requests: 0, setb: 0, seto: 0, jumps: 0, canaries: 0, retired_add_prefixes: 0,
  retired_mov_prefixes: 0, overflow_cases: 0, fault_calls: 0, malformed_calls: 0, cancel_calls: 0, zero_budget_calls: 0, stale_calls: 0, closed_calls: 0,
  permission_only_repairs: 0, changed_data_repairs: 0, map_only_repairs: 0, same_value_code_stores: 0, changing_code_stores: 0};
const contexts = [], modules = [], targets = [], faults = [], codeStores = [], pagesRead = [], runs = [], hostInputs = [], hostCalls = [], artifacts = [], rawFrames = [], rawRecords = [];
function artifact(path, bytes, write = true) {assert.ok(!artifacts.some(row => row.path === path)); if (write) writeFileSync(join(output, path), bytes, {flag: 'wx'});
  const row = {path, bytes: bytes.length, sha256: hash(bytes)}; artifacts.push(row); return row;}
artifact('engine.wasm', engineBytes); for (const selected of banks) artifact(`${selected.name}.x86`, selected.bytes, false);
const source = {provenance: 'physically pinned offline Intel253667-093US September2026; no fresh edition/download claim',
  prerequisite_policy: 'ignored immutable local PDF/fulltext/extract required before guest; no bootstrap/clean-checkout/fullCI claim',
  pdf_path: 'target/r3-rotate-scout/intel-253667-093-vol2b.pdf', pdf_bytes: 6915682, pdf_sha256: 'a261998ace8e07f624bf3e2486bb6cfe950fb8be8d0dc9631bcecb4c5ec953e4',
  full_text_path: 'target/p2-memory-binary-spec/253667-093-sdm-vol-2b.txt', full_text_bytes: 1537522, full_text_sha256: 'f5e6dc689d41655d64792512bfe8add56fc4ac97203aab5c1377a72bac291a35',
  extract_path: 'target/r3-rotate-scout/intel-093-rotate-pages.txt', extract_bytes: 11074, extract_sha256: '7a0e03302580204f99e8283af389f84ec082701807f1a9615a028af4d6f849d0',
  chapter_lines: [27174, 27413], chapter_bytes: 10932, chapter_sha256: 'f502898d90ce6ea7a6a513d1c8761e507af8ad92fac581e1240ea413b78aa74d',
  distinction: 'fulltext/extract/chapter are different extents of the same edition; complete chapter excludes successor RCPPS'};
for (const kind of ['pdf', 'full_text', 'extract']) {const bytes = readFileSync(join(root, source[`${kind}_path`])); assert.equal(bytes.length, source[`${kind}_bytes`]); assert.equal(hash(bytes), source[`${kind}_sha256`]);}
const fullTextLines = readFileSync(join(root, source.full_text_path), 'utf8').split('\n');
const chapter = Buffer.from(fullTextLines.slice(source.chapter_lines[0] - 1, source.chapter_lines[1]).join('\n') + '\n');
assert.equal(chapter.length, source.chapter_bytes); assert.equal(hash(chapter), source.chapter_sha256); source.saved_chapter = artifact('intel-memory-rotate.txt', chapter);
const sourcePaths = ['engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/memory.rs',
  'engine/src/cpu/dbt/wasm/integer.rs', 'engine/tests/cpu_memory_rotate_one.rs', 'engine/tests/cpu_rotate_immediate_one.rs', 'engine/tests/cpu_rotate32.rs',
  'engine/tests/fixtures/p2-rotate-immediate-one/run.mjs', 'engine/tests/cpu_memory_rotate32.rs', 'engine/tests/cpu_memory_rotate32_wasm.rs',
  'engine/tests/fixtures/p2-memory-rotate32/run.mjs', 'engine/tests/cpu_rotate32_wasm.rs', 'engine/tests/fixtures/p2-rotate32/run.mjs',
  'engine/tests/cpu_rotate_immediate_one_wasm.rs', 'engine/tests/fixtures/support/engine.mjs'];
assert.equal(new Set(sourcePaths).size, 16);
const sourceIdentities = () => Object.fromEntries(sourcePaths.map(path => {const bytes = readFileSync(join(root, path)); return [path, {bytes: bytes.length, sha256: hash(bytes)}];}));
const sourcePins = sourceIdentities();

function profile(bytes, owner, memory, ctx, binding) {
  let at = 8; assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  const uleb = () => {let value = 0, scale = 1; for (let index = 0; index < 5; index++) {assert.ok(at < bytes.length); const byte = bytes[at++]; value += (byte & 127) * scale;
    if (!(byte & 128)) {assert.ok(value <= 0xffffffff); return value;} scale *= 128;} assert.fail('invalid bounded unsigned LEB');};
  const sleb = () => {let value = 0, scale = 1, byte; for (let index = 0; index < 5; index++) {assert.ok(at < bytes.length); byte = bytes[at++]; value += (byte & 127) * scale;
    scale *= 128; if (!(byte & 128)) return (byte & 64 ? value - scale : value) >>> 0;} assert.fail('invalid bounded signed LEB');};
  const text = () => {const length = uleb(); assert.ok(at + length <= bytes.length); const value = bytes.subarray(at, at + length).toString('utf8'); at += length; return value;};
  const sections = new Map();
  while (at < bytes.length) {const id = bytes[at++], length = uleb(), start = at; assert.ok(start + length <= bytes.length && !sections.has(id)); sections.set(id, [start, start + length]); at += length;}
  assert.deepEqual([...sections.keys()], [1, 2, 3, 7, 10]);
  const section = id => {at = sections.get(id)[0];}, ended = id => assert.equal(at, sections.get(id)[1]);
  section(1); const types = []; for (let index = 0, length = uleb(); index < length; index++) {assert.equal(bytes[at++], 0x60); const parameters = [];
    for (let index = 0, length = uleb(); index < length; index++) parameters.push(bytes[at++]); const results = []; for (let index = 0, length = uleb(); index < length; index++) results.push(bytes[at++]); types.push({parameters, results});} ended(1);
  const expectedTypes = [{parameters: Array(4).fill(0x7f), results: [0x7f]}, {parameters: Array(owner === 'replacement' ? 6 : 7).fill(0x7f), results: [0x7f]}];
  if (memory) expectedTypes.push({parameters: [0x7f], results: [0x7f]}, {parameters: Array(owner === 'replacement' ? 2 : 6).fill(0x7f), results: [0x7f]}); assert.deepEqual(types, expectedTypes);
  const names = [owner === 'replacement' ? 'guard' : 'guard_resident', ...(memory ? ['read32', owner === 'replacement' ? 'store32' : 'store_resident32'] : [])];
  section(2); assert.equal(uleb(), names.length + 1); assert.equal(text(), 'env'); assert.equal(text(), 'memory'); assert.equal(bytes[at++], 2); assert.equal(uleb(), 0); assert.equal(uleb(), 1);
  for (const [index, name] of names.entries()) {assert.equal(text(), 'ring3'); assert.equal(text(), name); assert.equal(bytes[at++], 0); assert.equal(uleb(), index + 1);} ended(2);
  section(3); assert.equal(uleb(), 1); assert.equal(uleb(), 0); ended(3);
  section(7); assert.equal(uleb(), 1); assert.equal(text(), 'run'); assert.equal(bytes[at++], 0); assert.equal(uleb(), names.length); ended(7);
  section(10); assert.equal(uleb(), 1); const bodyLength = uleb(), bodyEnd = at + bodyLength; assert.equal(bodyEnd, sections.get(10)[1]);
  const locals = []; for (let index = 0, length = uleb(); index < length; index++) locals.push([uleb(), bytes[at++]]); assert.deepEqual(locals, [[16, 0x7f], [1, 0x7e], ...(memory ? [[6, 0x7f]] : [])]);
  const guardConstants = [ctx.low, ctx.high, ...(owner === 'replacement' ? [binding.generation] : [binding.low, binding.high])];
  for (const value of guardConstants) {assert.equal(bytes[at++], 0x41); assert.equal(sleb(), value >>> 0);}
  for (const parameter of [0, 1, 3]) {assert.equal(bytes[at++], 0x20); assert.equal(uleb(), parameter);} assert.equal(bytes[at++], 0x10); assert.equal(uleb(), 0);
  assert.equal(bytes[bodyEnd - 1], 0x0b); return {types, locals, guard_constants: guardConstants, body_bytes: bodyLength, sections: [...sections.keys()], body_claim: 'guard prefix/types/locals only; remaining body not independently certified'};
}

function record(magic, version, size, fields) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 1, 56, [...registers, pc, flags]);
const exit = (version, reason, retired, detail = 0, address = 0, access = 0) => record('R3EX', version, 40, [reason, retired, detail, address, access, access ? 4 : 0]);
const helper = (value = 0, detail = 0, address = 0, access = 0) => record('R3MH', 1, 40, [access ? 1 : 0, value, detail, address, access, access ? 4 : 0]);
const pattern = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1));
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.ordinal}/${label}: complete live arena`); counts.arena_checks++;}
function snapshot(ctx) {const bytes = arena(ctx); return {state_hex: bytes.subarray(0, 56).toString('hex'), exit_hex: bytes.subarray(56, 96).toString('hex'),
  helper_hex: bytes.subarray(100, 140).toString('hex'), whole_arena_sha256: hash(bytes)};}
function core(snapshot) {return {state_hex: snapshot.state_hex, exit_hex: snapshot.exit_hex, helper_hex: snapshot.helper_hex};}
function cpu(ctx) {return {registers: Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), pc: ctx.expected.readUInt32LE(48), flags: ctx.expected.readUInt32LE(52)};}
function saveArena(ctx, label, unit) {check(ctx, label); const bytes = arena(ctx), row = {context: ctx.ordinal, module: unit.file, label,
  path: 'arena-snapshots.bin', offset: rawFrames.length * SIZE, bytes: SIZE, sha256: hash(bytes)}; rawFrames.push(bytes); rawRecords.push(row); counts.raw_arenas++; return row;}
function noteHost(ctx, label, status, before, input = null) {hostCalls.push({ordinal: ++counts.host_calls, context: ctx.ordinal, label, status, input, before, after: snapshot(ctx)});}
function pure(ctx, action, label, status = 0, input = null) {check(ctx, `before ${label}`); const before = snapshot(ctx); assert.equal(action(), status, label); check(ctx, label); noteHost(ctx, label, status, before, input);}
function hostWrite(ctx, offset, bytes, purpose, category) {
  const before = snapshot(ctx); refresh(ctx).bytes.set(bytes, ctx.base + offset); ctx.expected.set(bytes, offset); check(ctx, purpose);
  hostInputs.push({ordinal: hostInputs.length + 1, context: ctx.ordinal, category, purpose, arena_offset: offset, bytes: bytes.length,
    hex: bytes.toString('hex'), sha256: hash(bytes), before, after: snapshot(ctx)});
}
function request(ctx, bytes, purpose) {assert.ok(bytes.length <= 4096); hostWrite(ctx, TRANSFER, bytes, purpose, 'transfer'); counts.transfer_requests++;}
function patch(ctx, address, bytes) {
  assert.ok(address >= 0 && address + bytes.length <= 4294967296);
  for (const [index, value] of bytes.entries()) {const at = address + index, base = Math.floor(at / 4096) * 4096; assert.ok(ctx.pages.has(base)); ctx.pages.get(base)[at % 4096] = value;}
}
function upload(ctx, address, bytes, purpose) {request(ctx, bytes, `request: ${purpose}`); pure(ctx, () => ctx.api.upload(address, bytes.length), purpose, 0, {operation: 'upload', address, bytes: bytes.length, transfer_input: hostInputs.length}); patch(ctx, address, bytes); counts.uploads++; counts.uploaded_bytes += bytes.length;}
function map(ctx, address, bytes = pattern, permissions = 3) {pure(ctx, () => ctx.api.map(address, 1, permissions), 'map page', 0, {operation: 'map', address, pages: 1, permissions}); ctx.pages.set(address, Buffer.alloc(4096)); ctx.permissions.set(address, permissions); counts.maps++; if (bytes !== null) upload(ctx, address, bytes, 'page input');}
function unmap(ctx, address) {pure(ctx, () => ctx.api.unmap(address, 1), 'unmap page', 0, {operation: 'unmap', address, pages: 1}); assert.ok(ctx.pages.delete(address)); ctx.permissions.delete(address); counts.unmaps++;}
function protect(ctx, address, permissions) {pure(ctx, () => ctx.api.protect(address, 1, permissions), 'permission change', 0, {operation: 'protect', address, pages: 1, permissions}); ctx.permissions.set(address, permissions); counts.protects++;}
function fresh(owner, selected, label, writable = false, special = false) {
  const instance = new WebAssembly.Instance(engineModule, {}), api = {}, ordinal = ++counts.contexts;
  const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, unmap: 2, protect: 3, upload: 2, read32: 1,
    compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store32: 2, store_resident32: 6};
  for (const [name, arity] of Object.entries(arities)) {api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);}
  const ctx = {owner, ordinal, selected, label, api, memory: instance.exports.memory, low: ordinal, high: 0x4d525433,
    expected: Buffer.alloc(SIZE), pages: new Map(), permissions: new Map(), units: []};
  assert.ok(ctx.memory instanceof WebAssembly.Memory); assert.equal(api.open(special ? 4 : 3, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  refresh(ctx); assert.ok(ctx.base > 0 && ctx.base + SIZE <= ctx.bytes.length);
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(exit(1, 1, 0), 56); ctx.expected.set(helper(), 100); check(ctx, 'independently initialized arena'); noteHost(ctx, 'open/arena getter initialization', 0, null, {operation: 'open', page_limit: special ? 4 : 3, key: [ctx.low, ctx.high]});
  const codePage = Buffer.alloc(4096); codePage.set(selected.bytes); map(ctx, PC, codePage, 7); if (!writable) protect(ctx, PC, 5);
  for (const address of [KEEP, DATA, ...(special ? [SECOND] : [])]) map(ctx, address);
  contexts.push({owner, context: ordinal, label, bank: selected.name, key: [ctx.low, ctx.high], base: ctx.base, page_limit: special ? 4 : 3, code_permissions: writable ? 7 : 5}); return ctx;
}
const identity = (ctx, unit) => ctx.owner === 'replacement' ? {generation: unit.generation} : {low: unit.low, high: unit.high};
function physical(ctx, unit) {refresh(ctx); assert.ok(unit.pointer > 0 && unit.pointer + unit.length <= ctx.bytes.length); const bytes = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.deepEqual(bytes, unit.bytes, 'whole live module allocation retained'); return {pointer: unit.pointer, bytes: unit.length, sha256: hash(bytes)};}
function compile(ctx, phase = 'initial', tail) {
  const specs = tail ? [[tail.next, 13]] : ctx.selected.specs, memory = !tail;
  request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat()), `compile ${phase} input`); let binding;
  if (ctx.owner === 'replacement') {pure(ctx, () => ctx.api.compile_entries(specs.length, 0), 'entry compiler', 0, {operation: 'compile_entries', count: specs.length, gates: 0, specs}); binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);}
  else {check(ctx, 'before resident compiler'); const before = snapshot(ctx); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
    binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
    assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident receipt only'); noteHost(ctx, 'resident compiler/receipt', 0, before, {operation: 'compile_resident', count: specs.length, specs});
    assert.equal(ctx.api.generation(), 0); assert.equal(ctx.api.module_ptr(), 0); assert.equal(ctx.api.module_len(), 0);}
  check(ctx, 'compiler publication/getter group'); refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
  const bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)); assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes), guardName = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  const names = [guardName, ...(memory ? ['read32', ctx.owner === 'replacement' ? 'store32' : 'store_resident32'] : [])];
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const abi = profile(bytes, ctx.owner, memory, ctx, binding), child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: Object.fromEntries(names.map(name => [name, ctx.api[name]]))});
  assert.equal(child.exports.run.length, 4); check(ctx, 'child linking preserves arena');
  const file = `${ctx.owner}-${ctx.ordinal}-${phase}.wasm`, saved = artifact(file, bytes), unit = {...binding, bytes, file, run: child.exports.run, version: memory ? 2 : 1};
  ctx.units.push(unit); counts.modules++; modules.push({owner: ctx.owner, context: ctx.ordinal, phase, bank: ctx.selected.name, specs, instructions: tail ? 4 : ctx.selected.instructions,
    ...binding, exit_version: unit.version, identity: identity(ctx, unit), imports, exports: [{name: 'run', kind: 'function'}], run_arity: 4, profile: abi, artifact: saved,
    declared_code_page_sha256: hash(ctx.pages.get(PC)), code_page_authority: 'host/model at compile; physical typed page snapshots saved at declared checkpoints',
    source_spans: specs.map(([entry, length]) => {const sourceBytes = ctx.pages.get(PC).subarray(entry - PC, entry - PC + length); return {entry, bytes: length, hex: sourceBytes.toString('hex'), sha256: hash(sourceBytes), bank_offset: entry - PC};})}); return unit;
}
function seed(ctx, registers, pc, flags, purpose) {
  assert.ok(FLAGS.includes(flags)); const bytes = Buffer.alloc(140); bytes.set(state(registers, pc, flags)); bytes.set(exit(2, 3, 0), 56); bytes.set(helper(0xdecafbad), 100);
  hostWrite(ctx, 0, bytes, purpose, 'case-seed'); counts.seeds++;
}
function cancel(ctx, value) {hostWrite(ctx, 96, words([value]), 'explicit cancellation field', 'cancel');}
function run(ctx, unit, budget, label, outcome = {}, status = 0, malformed = false, retention) {
  check(ctx, `before ${label}`); const before = snapshot(ctx), current = cpu(ctx), {registers = current.registers, pc = current.pc, flags = current.flags,
    reason = 1, retired = 0, detail = 0, fault = 0, access = 0, helperBytes} = outcome;
  if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(unit.version, reason, retired, detail, fault, access), 56); if (helperBytes !== undefined) ctx.expected.set(helperBytes, 100);}
  assert.equal(unit.run(malformed ? 1 : ctx.base, ctx.base + 56, budget, ctx.base + 96), status, label); retention?.(); check(ctx, label);
  counts.generated_calls++; counts.retired += status ? 0 : retired;
  const row = {ordinal: counts.generated_calls, context: ctx.ordinal, module: unit.file, label, budget, status, malformed_state_pointer: malformed ? 1 : null,
    reason: status ? null : reason, retired: status ? 0 : retired, before, after: snapshot(ctx)}; runs.push(row); return row;
}
function guard(ctx, unit, wrong) {return ctx.owner === 'replacement'
  ? ctx.api.guard(ctx.low ^ Number(wrong === 'key'), ctx.high, unit.generation + Number(wrong === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96)
  : ctx.api.guard_resident(ctx.low ^ Number(wrong === 'key'), ctx.high, unit.low ^ Number(wrong === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);}
function wordAt(ctx, address) {const bytes = Buffer.alloc(4); for (let index = 0; index < 4; index++) {const at = address + index; bytes[index] = ctx.pages.get(Math.floor(at / 4096) * 4096)[at % 4096];} return bytes.readUInt32LE();}
function observeWord(ctx, address, value) {check(ctx, 'before typed Read32 diagnostic'); assert.equal(ctx.api.read32(address), 0); ctx.expected.set(helper(value), 100); check(ctx, 'typed Read32 changes helper only'); counts.diagnostic_read32_calls++;
  return {value: refresh(ctx).view.getUint32(ctx.base + 120, true), helper_hex: snapshot(ctx).helper_hex};}
function pages(ctx, label, addresses = [...ctx.pages.keys()]) {
  for (const address of addresses) {const expected = ctx.pages.get(address); assert.ok(expected);
    const permissions = ctx.permissions.get(address), actual = Buffer.alloc(4096); if (!(permissions & 1)) protect(ctx, address, permissions | 1);
    for (let offset = 0; offset < 4096; offset += 4) actual.writeUInt32LE(observeWord(ctx, address + offset, expected.readUInt32LE(offset)).value, offset);
    assert.deepEqual(actual, expected, 'entire mapped page read through aligned typed Read32'); if (!(permissions & 1)) protect(ctx, address, permissions);
    const file = `page-${ctx.ordinal}-${counts.pages}-${label}-${address.toString(16)}.bin`, saved = artifact(file, actual); counts.pages++;
    pagesRead.push({context: ctx.ordinal, label, address, permissions, expected_sha256: hash(expected), artifact: saved, diagnostic_read32_calls: 1024, final_helper_hex: snapshot(ctx).helper_hex,
      observation: 'typed aligned Read32; temporary read permission restored when required'});
  }
}
function addressFor(scan, registers) {
  if (scan.address_kind === 'absolute') return WORD;
  if (scan.address_kind === 'edi') return registers[7];
  return [registers[1], (registers[7] + registers[1] * 4 - 16) >>> 0, (registers[1] * 5) >>> 0, registers[4], registers[5]][scan.alias];
}
function target(ctx, unit, scan, group, label, reason = 1) {
  const original = cpu(ctx), address = addressFor(scan, original.registers), raw = scan.mode === 'imm' ? scan.raw : original.registers[1] % 256;
  assert.equal(original.pc, scan.target); const oldDestination = wordAt(ctx, address), expected = rotate(scan.kind, oldDestination, raw, original.flags);
  const rawBefore = saveArena(ctx, `${label}-before-target`, unit); patch(ctx, address, words([expected.value]));
  const call = run(ctx, unit, 1, label, {pc: scan.next, flags: expected.flags, reason, retired: 1, helperBytes: helper()});
  const saved = saveArena(ctx, `${label}-target`, unit); counts.targets++; counts[group]++; counts[`${scan.kind}_targets`]++; counts[`${scan.mode}_targets`]++;
  counts[expected.count === 0 ? 'zero_targets' : expected.count === 1 ? 'count_one_targets' : 'multiple_count_targets']++;
  const row = {ordinal: counts.targets, context: ctx.ordinal, module: unit.file, group, label, scan, address, old_destination: oldDestination,
    old_ecx: original.registers[1], raw_count: raw, masked_count: expected.count, incoming_flags: original.flags, expected,
    call: call.ordinal, before: call.before, first_target: call.after, raw_before: rawBefore, raw_arena: saved,
    ram_observation_phase: group === 'smc_targets' ? 'after pure current-tail continuation' : 'after target raw frame and before SETB/SETO'}; targets.push(row); return row;
}
function targetRam(ctx, row) {row.ram_readback = {...observeWord(ctx, row.address, row.expected.value), phase: row.ram_observation_phase, after: snapshot(ctx)}; counts.target_read32_calls++;}
const replaceLowByte = (parent, value) => Number.parseInt(parent.toString(16).padStart(8, '0').slice(0, 6) + value.toString(16).padStart(2, '0'), 16);
function consumers(ctx, unit, row) {
  let current = cpu(ctx), registers = [...current.registers]; registers[0] = replaceLowByte(registers[0], current.flags % 2);
  run(ctx, unit, 1, `${row.label}-setb`, {registers, pc: current.pc + 3, retired: 1}); counts.setb++;
  current = cpu(ctx); registers = [...current.registers]; registers[2] = replaceLowByte(registers[2], Number((current.flags & 0x800) !== 0));
  run(ctx, unit, 1, `${row.label}-seto`, {registers, pc: current.pc + 3, retired: 1}); counts.seto++;
  run(ctx, unit, 2, `${row.label}-jmp`, {pc: COLD, reason: 3, retired: 1}); counts.jumps++;
}
function preflight(ctx, unit) {
  cancel(ctx, 1); run(ctx, unit, 0, 'current malformed pointer before cancel/budget0', {}, 1, true); saveArena(ctx, 'current-malformed', unit); counts.malformed_calls++;
  run(ctx, unit, 0, 'current cancellation before budget0', {reason: 2}); saveArena(ctx, 'current-cancel-budget0', unit); counts.cancel_calls++;
  cancel(ctx, 0); run(ctx, unit, 0, 'current budget0 without cancellation', {}); saveArena(ctx, 'current-budget0', unit); counts.zero_budget_calls++;
}
function finish(ctx, unit) {
  for (const wrong of ['key', 'identity']) pure(ctx, () => guard(ctx, unit, wrong), `wrong ${wrong}`, 3);
  pure(ctx, () => ctx.api.close(), 'close regular process'); run(ctx, unit, 0, 'closed before malformed pointer', {}, 5, true); saveArena(ctx, 'regular-closed', unit); counts.closed_calls++;
}
function normal(owner, selected) {
  const ctx = fresh(owner, selected, 'selected immediate/old CL counts and flags preservation'), unit = compile(ctx); pages(ctx, 'initial-normal');
  let controlled = false;
  for (const mode of ['imm', 'cl']) for (const raw of RAW) for (const old of VALUES) for (const flags of FLAGS) {
    const scan = selected.scans[mode === 'imm' ? RAW.indexOf(raw) : 7], registers = [...REG], label = `${mode}-${raw}-${old.toString(16)}-${flags.toString(16)}`;
    registers[1] = mode === 'cl' ? 0xa1b2c300 + raw : REG[1];
    upload(ctx, WORD, words([old]), 'normal DWORD operand'); seed(ctx, registers, scan.target, flags, label);
    if (!controlled) {preflight(ctx, unit); controlled = true;}
    const row = target(ctx, unit, scan, 'normal_targets', label); if (raw % 32 === 0) {assert.equal(row.expected.value, old); assert.equal(row.expected.flags, flags);}
    targetRam(ctx, row); consumers(ctx, unit, row);
  }
  pages(ctx, 'final-normal'); finish(ctx, unit);
}
function faultFrame(ctx, unit, scan, shape, registers, prefixFlags, first, label) {
  const failure = {registers, pc: scan.target, flags: prefixFlags, reason: 5, retired: Number(first), detail: shape.detail, fault: shape.fault,
    access: shape.access, helperBytes: helper(0, shape.detail, shape.fault, shape.access)};
  const call = run(ctx, unit, first ? 2 : 1, label, failure); counts.fault_calls++; if (first) counts.retired_add_prefixes++;
  const raw = saveArena(ctx, label, unit), firstPages = pagesRead.length; pages(ctx, label);
  return {call, raw, page_refs: pagesRead.slice(firstPages).map(page => page.artifact.path), currently_mapped: [...ctx.pages.keys()]};
}
function special(owner, selected) {
  const ctx = fresh(owner, selected, 'full ECX EA/count aliases, DWORD endpoints and precise faults', false, true), unit = compile(ctx); pages(ctx, 'initial-special');
  for (let form = 0; form < 5; form++) for (const flags of FLAGS) {
    const scan = selected.scans[form], registers = [...REG], label = `alias-${form}-${flags.toString(16)}`;
    if (form === 0) registers[1] = WORD;
    if (form === 1) {registers[1] = 0x00010002; registers[7] = 0xfffc4018;}
    if (form === 2) registers[1] = 0xcd0;
    if (form === 3) {registers[4] = WORD; registers[1] = 0x12345602;}
    if (form === 4) {registers[5] = WORD; registers[1] = 0x12345620;}
    const old = [0, selected.kind === 'rol' ? 0x40000000 : 2, 0x80000001, 0xffffffff, 0x12345678][form];
    assert.equal(addressFor(scan, registers), WORD); upload(ctx, WORD, words([old]), 'four-byte alias operand only'); seed(ctx, registers, scan.target, flags, label);
    const row = target(ctx, unit, scan, 'alias_targets', label);
    if (form < 3) {const wrongRegisters = [...registers]; wrongRegisters[1] %= 256; const wrong = addressFor(scan, wrongRegisters);
      assert.equal(wrong, [0x10, 0xfffc4010, 0x410][form]); assert.notEqual(wrong, WORD); assert.ok(!ctx.pages.has(Math.floor(wrong / 4096) * 4096));
      row.full_parent_ea_discriminator = {hypothetical_cl_only_ea: wrong, correct_ea: WORD, authority: 'independent declared old-EA math; wrong path never executed'};
    }
    if (form === 0) assert.deepEqual([row.expected.value, row.expected.flags], [0, flags === 2 ? 2 : 0x4d6]);
    if (form === 1 || form === 3) assert.equal(row.expected.flags, flags === 2 ? 3 : 0x4d7);
    if (form === 2) assert.equal(row.expected.value, 0x18000);
    if (form === 4) assert.deepEqual([row.expected.value, row.expected.flags], [old, flags]);
    targetRam(ctx, row); consumers(ctx, unit, row);
  }
  pages(ctx, 'after-aliases'); const rangeScan = selected.scans[3];
  for (const flags of FLAGS) {
    const registers = [...REG]; registers[4] = 0x4ffe; registers[1] = 0x12345602; const label = `crossing-${flags.toString(16)}`;
    upload(ctx, 0x4ffe, words([0x80000001]), 'crossing DWORD operand'); seed(ctx, registers, rangeScan.target, flags, label);
    const row = target(ctx, unit, rangeScan, 'range_targets', label); assert.equal(row.expected.value, selected.kind === 'rol' ? 6 : 0x60000000);
    assert.equal(row.expected.flags, flags === 2 ? 2 : 0x4d6); targetRam(ctx, row); consumers(ctx, unit, row);
  }
  pages(ctx, 'after-crossing'); unmap(ctx, SECOND); map(ctx, TOP); pages(ctx, 'before-final-dword');
  for (const flags of FLAGS) {
    const registers = [...REG]; registers[4] = 0xfffffffc; registers[1] = 0x12345602; const label = `final-dword-${flags.toString(16)}`;
    upload(ctx, 0xfffffffc, words([0x80000001]), 'complete final DWORD operand'); seed(ctx, registers, rangeScan.target, flags, label);
    const row = target(ctx, unit, rangeScan, 'range_targets', label); assert.equal(row.expected.value, selected.kind === 'rol' ? 6 : 0x60000000);
    targetRam(ctx, row); consumers(ctx, unit, row);
  }
  pages(ctx, 'after-final-dword'); const scan = selected.scans[5];
  for (const seedFlags of FLAGS) {
    const registers = [...REG]; registers[1] = 0x12345602; registers[3] = 0x7fffffff; registers[7] = 0xfffffffd;
    const label = `overflow-${seedFlags.toString(16)}`; seed(ctx, registers, scan.entry, seedFlags, label); registers[3] = 0x80000000;
    const prefixFlags = 0x896 + (seedFlags & 0x400), shape = {name: 'nonwrapping-read-overflow', detail: 3, fault: 0xfffffffd, access: 1};
    const first = faultFrame(ctx, unit, scan, shape, registers, prefixFlags, true, `${label}-first`);
    const retry = faultFrame(ctx, unit, scan, shape, registers, prefixFlags, false, `${label}-retry`); counts.overflow_cases++;
    faults.push({context: ctx.ordinal, module: unit.file, ...shape, address: 0xfffffffd, width: 4, seed_flags: seedFlags, prefix_flags: prefixFlags, first, retry,
      repair_mode: 'none; entire address range permanently overflows', successful_target: null});
  }
  unmap(ctx, TOP); map(ctx, SECOND); pages(ctx, 'restored-four-pages');
  for (const shape of faultShapes) for (const seedFlags of FLAGS) {
    protect(ctx, DATA, 3); protect(ctx, SECOND, 3); upload(ctx, 0x4ffe, words([0x7fffffff]), 'fault original operand');
    if (shape.missing !== undefined) unmap(ctx, SECOND); if (shape.denied !== undefined) protect(ctx, shape.denied, shape.permissions);
    const registers = [...REG]; registers[1] = 0x12345602; registers[3] = 0x7fffffff; registers[7] = 0x4ffe;
    const label = `${shape.name}-${seedFlags.toString(16)}`; seed(ctx, registers, scan.entry, seedFlags, label); registers[3] = 0x80000000;
    const prefixFlags = 0x896 + (seedFlags & 0x400), first = faultFrame(ctx, unit, scan, shape, registers, prefixFlags, true, `${label}-first`);
    const retry = faultFrame(ctx, unit, scan, shape, registers, prefixFlags, false, `${label}-retry`);
    const changed = seedFlags === 2, repairedValue = changed ? 0x12345678 : shape.missing !== undefined ? 0x0000ffff : 0x7fffffff; let repairMode;
    if (shape.missing !== undefined) {map(ctx, SECOND, null); repairMode = changed ? 'map+changed-data' : 'map-only-zeroed-SECOND';}
    else {protect(ctx, shape.denied, 3); repairMode = changed ? 'permission+changed-data' : 'permission-only';}
    if (changed) {upload(ctx, 0x4ffe, words([repairedValue]), 'declared changed DWORD after access restoration'); counts.changed_data_repairs++;}
    else if (shape.missing !== undefined) counts.map_only_repairs++; else counts.permission_only_repairs++;
    pure(ctx, () => guard(ctx, unit), 'access/data-only repair retains original owner'); assert.equal(wordAt(ctx, 0x4ffe), repairedValue);
    const row = target(ctx, unit, scan, 'repaired_targets', `${label}-repaired`);
    const expectedValue = selected.kind === 'rol' ? repairedValue : changed ? 0x048d159e : shape.missing !== undefined ? 0xc0003fff : 0xdfffffff;
    const expectedFlags = selected.kind === 'rol' ? prefixFlags : changed ? 0x96 : 0x497;
    assert.deepEqual([row.expected.value, row.expected.flags], [expectedValue, expectedFlags]);
    targetRam(ctx, row); consumers(ctx, unit, row); pages(ctx, `${label}-repaired`, [DATA, SECOND]);
    faults.push({context: ctx.ordinal, module: unit.file, ...shape, address: 0x4ffe, width: 4, seed_flags: seedFlags, prefix_flags: prefixFlags,
      original_operand: 0x7fffffff, first, retry, repair_mode: repairMode, required_access_restoration: shape.missing !== undefined ? 'map zero SECOND' : `protect ${shape.denied.toString(16)} RW`,
      repair_value: repairedValue, changed_operand: changed, successful_target: row.ordinal, no_reseed: 'same original child/CPU; only access/data repair and diagnostic helper writes'});
  }
  pages(ctx, 'final-special'); finish(ctx, unit);
}
function smc(owner, selected) {
  const changing = selected.kind === 'ror', ctx = fresh(owner, selected, changing ? 'changing consumed MOV immediate' : 'same masked-zero consumed MOV immediate', true);
  const old = compile(ctx), scan = selected.scans[6]; pages(ctx, 'initial-smc');
  for (const wrong of ['key', 'identity']) pure(ctx, () => guard(ctx, old, wrong), `SMC wrong ${wrong}`, 3);
  const registers = [...REG]; registers[1] = 0x12345602; registers[7] = scan.entry + 1;
  seed(ctx, registers, scan.entry, 0xcd7, 'one SMC head/target/tail seed'); registers[6] = 0x80000001;
  const mov = run(ctx, old, 1, 'consumed MOV head retires once', {registers, pc: scan.target, retired: 1}); counts.retired_mov_prefixes++;
  const movArena = saveArena(ctx, 'smc-prefix-MOV', old), beforeOld = physical(ctx, old), row = target(ctx, old, scan, 'smc_targets', 'consumed-MOV-store', 6);
  assert.deepEqual([row.old_destination, row.expected.value, row.expected.flags], [0x80000001, changing ? 0x60000000 : 0x80000001, changing ? 0x4d6 : 0xcd7]);
  counts[changing ? 'changing_code_stores' : 'same_value_code_stores']++;
  let retainedOld; cancel(ctx, 1); const staleOld = run(ctx, old, 0, 'stale old before malformed cancelled budget0', {}, 4, true, () => {
    retainedOld = physical(ctx, old); assert.deepEqual(retainedOld, beforeOld, 'immediate full old allocation before post-run arena check/snapshot/getter/diagnostic/save/publication');
  }); counts.stale_calls++;
  const staleOldArena = saveArena(ctx, 'smc-old-stale', old); cancel(ctx, 0);
  const currentBank = artifact(`current-${ctx.ordinal}.x86`, Buffer.from(ctx.pages.get(PC).subarray(0, 130))), tailBytes = ctx.pages.get(PC).subarray(scan.next - PC, scan.next - PC + 13);
  const tailInput = artifact(`current-tail-${ctx.ordinal}.x86`, Buffer.from(tailBytes)); assert.equal(tailBytes.length, 13);
  const committed = snapshot(ctx), current = compile(ctx, 'current-tail', scan);
  assert.notDeepEqual(identity(ctx, current), identity(ctx, old)); assert.deepEqual(core(snapshot(ctx)), core(committed), 'compile preserves State/Exit/helper; transfer/receipt changes modeled separately');
  assert.equal(current.version, 1); const tailRegisters = [...cpu(ctx).registers];
  tailRegisters[0] = replaceLowByte(tailRegisters[0], row.expected.flags % 2); tailRegisters[2] = replaceLowByte(tailRegisters[2], Number((row.expected.flags & 0x800) !== 0)); tailRegisters[3] = 0xdeadbeef;
  const tail = run(ctx, current, 5, 'pure current tail without target/head replay', {registers: tailRegisters, pc: COLD, reason: 3, retired: 4});
  counts.setb++; counts.seto++; counts.canaries++; counts.jumps++; const tailArena = saveArena(ctx, 'smc-current-tail', current);
  assert.equal(cpu(ctx).registers[6], 0x80000001); assert.equal(cpu(ctx).registers[1], 0x12345602); assert.deepEqual(Buffer.from(snapshot(ctx).helper_hex, 'hex'), helper(), 'Store4 success persists through guard-only tail');
  targetRam(ctx, row); pages(ctx, 'after-current-tail');
  const beforeTail = physical(ctx, current), sameByte = ctx.pages.get(PC)[scan.next - PC]; assert.equal(sameByte, 0x0f); assert.equal(ctx.permissions.get(PC), 7);
  upload(ctx, scan.next, Buffer.from([sameByte]), 'same consumed-tail byte while CODE remains RWX'); cancel(ctx, 1);
  let retainedTail; const staleTail = run(ctx, current, 0, 'stale fresh tail before malformed cancelled budget0', {}, 4, true, () => {
    retainedTail = physical(ctx, current); assert.deepEqual(retainedTail, beforeTail, 'immediate full fresh allocation before post-run arena check/snapshot/getter/diagnostic/save/close');
  }); counts.stale_calls++;
  const staleTailArena = saveArena(ctx, 'smc-current-stale', current); pure(ctx, () => ctx.api.close(), 'close pending-cancel stale owners');
  for (const unit of [old, current]) {run(ctx, unit, 0, 'closed before stale malformed cancelled budget0', {}, 5, true); saveArena(ctx, `smc-closed-${unit.file}`, unit); counts.closed_calls++;}
  const replay = changing ? rotate('ror', row.expected.value, row.raw_count, row.expected.flags) : null;
  if (replay) assert.deepEqual([replay.value, replay.flags], [0x18000000, 0x4d6]);
  codeStores.push({context: ctx.ordinal, changing, address: registers[7], consumed_instruction: 'MOV ESI,80000001', mov, mov_arena: movArena, target: row.ordinal,
    current_bank: currentBank, current_tail_input: tailInput, store_status: {value: 11, authority: 'source-inferred; direct managed helper not intercepted'}, observed_helper_success: true, observed_exit: 6, observed_retired: 1,
    old_identity: identity(ctx, old), current_identity: identity(ctx, current), stale_old: staleOld, old_stale_arena: staleOldArena, retained_old_after_stale: retainedOld,
    current_tail: tail, current_tail_arena: tailArena, stale_tail: staleTail, current_stale_arena: staleTailArena, retained_tail_after_stale: retainedTail,
    fresh_tail_exit_version: 1, ram_observation_phase: 'after pure tail; full target raw/helper before diagnostic', code_permissions_through_tail_upload: 7,
    replay_discriminator: changing ? {authority: 'hypothetical independent mathematical wrong paths; never executed', target_replay: replay, replayed_head_esi: 0x60000000, retained_esi: 0x80000001}
      : {authority: 'zero arithmetic idempotent; entry/retirement/current physical tail and retained CPU establish continuation, no scalar replay witness'}});
}
for (const owner of OWNERS) {
  for (const selected of banks.filter(bank => bank.name.startsWith('normal-'))) normal(owner, selected);
  for (const selected of banks.filter(bank => bank.name.startsWith('special-'))) special(owner, selected);
  for (const selected of banks.filter(bank => bank.name.startsWith('special-'))) smc(owner, selected);
}
for (const [name, value] of Object.entries(formulas)) assert.equal(counts[name], value, `observed ${name}`);
for (const name of ['targets', 'contexts', 'modules', 'seeds', 'generated_calls', 'retired', 'raw_arenas', 'pages', 'diagnostic_read32_calls', 'transfer_requests',
  'arena_checks', 'overflow_cases', 'maps', 'unmaps', 'protects', 'uploads', 'uploaded_bytes', 'host_calls']) assert.equal(counts[name], planned[name], `observed ${name}`);
assert.equal(counts.target_read32_calls, planned.targets); assert.equal(runs.length, planned.generated_calls); assert.equal(hostInputs.length, planned.host_inputs); assert.equal(hostCalls.length, planned.host_calls);
assert.equal(counts.retired_add_prefixes, formulas.repaired_targets + overflowCases); assert.equal(counts.fault_calls, faultCalls);
assert.deepEqual([counts.rol_targets, counts.ror_targets, counts.imm_targets, counts.cl_targets, counts.zero_targets, counts.count_one_targets, counts.multiple_count_targets], [158, 158, 130, 186, 90, 64, 162]);
assert.deepEqual([counts.setb, counts.seto, counts.jumps, counts.canaries, counts.retired_mov_prefixes, counts.malformed_calls, counts.cancel_calls, counts.zero_budget_calls, counts.stale_calls, counts.closed_calls], [316, 316, 316, 4, 4, 4, 4, 4, 8, 16]);
assert.deepEqual([counts.permission_only_repairs, counts.changed_data_repairs, counts.map_only_repairs, counts.same_value_code_stores, counts.changing_code_stores], [12, 16, 4, 2, 2]);
assert.equal(modules.reduce((sum, row) => sum + row.source_spans.length, 0), planned.source_spans); assert.equal(runs.reduce((sum, row) => sum + row.retired, 0), planned.retired);
const raw = Buffer.concat(rawFrames); assert.equal(raw.length, planned.raw_bytes); const rawArtifact = artifact('arena-snapshots.bin', raw);
assert.deepEqual(sourceIdentities(), sourcePins, 'selected16 current sources unchanged before/after guest'); assert.equal(hash(readFileSync(enginePath)), engineSha256, 'current engine unchanged');
const names = [...artifacts.map(row => row.path), 'result.json']; assert.equal(new Set(names).size, names.length); assert.equal(names.length, planned.files);
const result = {status: 'ok', engine_sha256: engineSha256, engine_bytes: engineBytes.length,
  engine_input_policy: 'readEngine reads current bytes once; optional caller SHA verified; saved input and before/after current file pinned', tools: {node: process.version, v8: process.versions.v8},
  counts, formulas, planned, contexts, modules, targets, faults, code_stores: codeStores, pages: pagesRead, generated_runs: runs, host_inputs: hostInputs, host_calls: hostCalls,
  raw_arena_records: rawRecords, raw_arena_artifact: rawArtifact, source, source_pins_before: sourcePins, source_pins_after: sourceIdentities(),
  oracle: {anchors: ANCHORS, flag_seeds: FLAGS, register_sentinels: REG, selected_raw_counts: RAW, normal_values: VALUES, pattern_sha256: hash(pattern), arena_bytes: SIZE,
    banks: banks.map(selected => ({name: selected.name, kind: selected.kind, packed_bytes: selected.packed_bytes, physical_bytes: selected.physical_bytes,
      hex: selected.bytes.toString('hex'), sha256: hash(selected.bytes), specs: selected.specs, scans: selected.scans, instructions: selected.instructions})),
    arithmetic: 'independent repeated32 binary digits with displaced bit recirculation; CF last result end bit; OF only at count1; no emitter bit-shift/mask formula reused',
    flags_policy: 'valid mask0xcd7 mandatory0x2; zero entire FLAGS retained; nonzero only CF/OF changed, undefined multi OF explicitly0; no hardware undefined-bit claim',
    checkpoint_policy: 'complete live4236 around generated run, modeled map/unmap/protect/upload/guard/close and typed Read32; host writes checked after mutation; compiler request/receipt/getter/link and initialization groups explicit',
    ram_policy: '312 actual typed Read32 after target raw before SETcc;4 SMC after pure tail;528 aligned typed fullpage snapshots; no direct managed RAM pointer read',
    host_call_policy: '700 non-diagnostic owner/memory/compiler calls including initialized open group;540988 typed Read32 calls separately counted, page loops live-asserted and physicalpages saved rather than expanded per-word metadata',
    continuation_policy: 'same original child/CPU through first fault/retry/access+optionaldata repair; no CPU/helper reseed; ordinary diagnostic ReadSuccess legitimately persists through consumers; SMC Store4 success persists through pure tail',
    live_evidence_limit: 'saved fullframes/pages/modules do not reproduce live retention callback timing or every unsaved checkpoint; remaining Wasm body not certified',
    inherited_pre_execution_lessons: ['JMP budget2 yields one retirement then NeedCode; pure4-instruction tail budget5', 'compile preserves State/Exit/helper while transfer/receipt changes', 'live allocation callback before post-run arena check/snapshot', 'SMC CODE stays7; same-byte upload advances version without protection confound'],
    pre_use_census_correction: {label: 'F8', initial_driver_path: 'target/r3-memory-rotate32-driver-first-authored.mjs', initial_driver_bytes: 52937,
      initial_driver_sha256: 'a002328f17ef356f7cb254ed6b64ff5691bcc8d0b246ed733904019e11069f72',
      initial: {uploads: 480, host_calls: 900, transfer_requests: 496, host_inputs: 840, arena_checks: 1088144},
      corrected: {uploads: 380, host_calls: 800, transfer_requests: 396, host_inputs: 740, arena_checks: 1087844},
      historical_scope: 'pre-use intermediate correction; host_calls and arena_checks remained wrong and were superseded by F9 after first actual failure',
      authority: 'independent lifecycle whole-source review and author rederivation before first syntax/guest; aggregate metadata only, no corpus/runtime/product failure'},
    first_actual_census_failure: {label: 'F9', initial_driver_path: 'target/r3-memory-rotate32-before-actual-host-fix/engine/tests/fixtures/p2-memory-rotate32/run.mjs',
      initial_driver_bytes: 54086, initial_driver_sha256: '887caeedc084ac1ee955d26b701779b0fdfcc6f058988f6e75347594d8de2d37',
      failure_capture: {path: 'target/r3-memory-rotate32-actual-result.json', bytes: 1977, sha256: '8ab5cad17eec72d5be9ea7bc5664346b0818bba07c2b7f0c059d9fa4435c5e6b'},
      partial_manifest: {path: 'target/r3-memory-rotate32-actual-first-failure.json', sha256: '5221bda82489ab4de9064d4910d210398da3431d20336aacf0cc7eb968172710'},
      first_cargo_exit: 101, planned_before: {host_calls: 800, arena_checks: 1087844}, observed_first: {arena_checks: 1087644},
      corrected: {host_calls: 700, arena_checks: 1087644}, host_sum: '56map+16unmap+152protect+380upload+16compile+56guard+12close+12open=700',
      arena_sum: '12+2*(700-12)+32+740+2752+756+1081976=1087644',
      failed_folder: 'target/p2-memory-rotate32-fixtures/wasm-68612-1791334314608061000', partial_files: 558, unwritten_artifacts: ['arena-snapshots.bin', 'result.json'],
      authority: 'first actual101 exposed aggregate assertion failure; all prior800/check1087844 static gates superseded for these totals; no saved raw/result join for failed folder; count/provenance correction only, no numeric guest or product defect inferred'},
    prospective_fault_witness_fix: {label: 'RMM-F2', original_prefix_flags: [0x57, 0x457], corrected_prefix_flags: [0x896, 0xc96], successful_ror_flags: [0x97, 0x497],
      provenance: 'initial wrong idea existed only in pre-source message/tooltrace; corrected before first plan/source/use; no product or guest failure'}},
  artifacts, artifact_census: {modules: planned.modules, initial_program_banks: banks.length, current_banks: 4, pure_tail_inputs: 4, pages: planned.pages,
    raw_arena_files: 1, engine_files: 1, chapter_files: 1, result_files: 1, total: names.length, names},
  claim: 'finite prefix-free flat32 memory ROL/ROR C1/D3 through actual managed replacement/resident; selected raw counts only, full ECX EA/count aliases, exact first-target CPU/FLAGS/helper/fullarena and typed RAM; unconditional checked Read4/Store4 including zero, precise crossing/overflow faults with prior ADD retirement, unchanged retry and declared permission/map/data repair without reseed; same/changing consumed MOV stores exit6/retire1/helper success with raw11 source-inferred; synchronous immediate live old/tail allocation retention, pure current13Btail without CPU repair/replay, same-byte consumed-tail stale4 and closed5 priority; no MMIO/hardware bus/fault-cycle/atomicity/concurrency, broad ISA/SDK/browser/game/performance/fullCI/clean-checkout or remaining Wasm body proof'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), names.sort(), 'exact physical artifact census');
console.log(JSON.stringify({status: 'ok', output: join(output, 'result.json'), sha256: hash(resultBytes), counts, planned}));
