import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const {bytes: engineBytes, module: engineModule, sha256: engineSha256} = readEngine(enginePath);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = PC + 128, KEEP = 0x3000, DATA = 0x4000, SECOND = 0x5000, TOP = 0xfffff000, WORD = 0x4010;
const OWNERS = ['replacement', 'resident'], FLAGS = [2, 0xcd7], KINDS = ['shld', 'shrd'], MODES = ['imm', 'cl'];
const REG = [0x12345678, 0x80000021, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const EDGE = [0, 32, 64, 128, 2, 31, 33, 255];
const ANCHORS = [
  ['shld', 'imm', 0x80000000, 0, 0, 0x847], ['shld', 'cl', 0, 0x80000000, 1, 2],
  ['shld', 'imm', 0x40000000, 0, 0x80000000, 0x886], ['shld', 'cl', 0xffffffff, 0, 0xfffffffe, 0x83],
  ['shrd', 'imm', 0, 1, 0x80000000, 0x886], ['shrd', 'cl', 0x80000000, 0, 0x40000000, 0x806],
  ['shrd', 'imm', 1, 0, 0, 0x47], ['shrd', 'cl', 0x80000001, 1, 0xc0000000, 0x87],
];
function doubleShift(kind, destination, source, raw, incoming) {
  const count = raw % 32;
  if (count === 0) return {value: destination, flags: incoming, carry: incoming % 2, count};
  const bits = value => value.toString(2).padStart(32, '0').split('').map(Number);
  const result = bits(destination), sourceBits = bits(source), oldSign = result[0]; let carry;
  for (let step = 0; step < count; step++) {
    if (kind === 'shld') {carry = result.shift(); result.push(sourceBits[step]);}
    else {carry = result.pop(); result.unshift(sourceBits[31 - step]);}
  }
  const value = Number.parseInt(result.join(''), 2), parity = result.slice(24).reduce((sum, bit) => sum + bit, 0) % 2 === 0;
  const flags = 2 + (incoming & 0x400) + carry + Number(parity) * 4 + Number(value === 0) * 0x40
    + result[0] * 0x80 + Number(count === 1 && oldSign !== result[0]) * 0x800;
  if (destination === source) {
    const old = bits(destination), rotated = kind === 'shld' ? [...old.slice(count), ...old.slice(0, count)] : [...old.slice(32 - count), ...old.slice(0, 32 - count)];
    assert.equal(value, Number.parseInt(rotated.join(''), 2), 'self-source rotate result with double-shift flags');
  }
  return {value, flags, carry, count};
}
for (const [kind, , destination, source, value, flags] of ANCHORS) {
  assert.deepEqual(doubleShift(kind, destination, source, 1, 2), {value, flags, carry: flags % 2, count: 1}, 'literal count-one anchor');
}
assert.deepEqual(doubleShift('shrd', 0x80000001, 0x123456ff, 255, 2), {value: 0x2468adff, flags: 6, carry: 0, count: 31});
assert.deepEqual(doubleShift('shld', 0x80000001, 0x123456ff, 255, 2), {value: 0x891a2b7f, flags: 0x82, carry: 0, count: 31});
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
function bank(name, kind, mode, forms, descriptors, packed, instructions) {
  const bytes = Buffer.alloc(130, 0xcc), scans = [], specs = []; let offset = 0;
  for (const [index, form] of forms.entries()) {
    const descriptor = descriptors[index], tail = [0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, ...(descriptor.canary ? [0xbb, 0xef, 0xbe, 0xad, 0xde] : [])];
    const length = form.length + tail.length + 2, entry = PC + offset;
    bytes.set([...form, ...tail, 0xeb, 128 - offset - length], offset);
    scans.push({entry, target: entry + (descriptor.prefix ?? 0), next: entry + form.length, kind, mode, ...descriptor});
    specs.push([entry, length]); offset += length;
  }
  assert.equal(offset, packed); assert.ok(specs.length <= 8 && instructions <= 64); bytes.set([0x0f, 0x0b], 128);
  assert.deepEqual(readFileSync(join(output, `${name}.x86`)), bytes, 'independent Rust/JS physical x86 bank');
  return {name, kind, mode, bytes, packed_bytes: packed, physical_bytes: bytes.length, specs, scans, instructions};
}
const banks = [];
for (const kind of KINDS) {
  const immediate = kind === 'shld' ? 0xa4 : 0xac, cl = kind === 'shld' ? 0xa5 : 0xad;
  for (const mode of MODES) {
    const normal = Array.from({length: 8}, (_, source) => [0x0f, mode === 'imm' ? immediate : cl, source * 8 + 5, 0x10, 0x40, 0, 0, ...(mode === 'imm' ? [1] : [])]);
    banks.push(bank(`normal-${kind}-${mode}`, kind, mode, normal, Array.from({length: 8}, (_, source) => ({source, raw: 1})), mode === 'imm' ? 128 : 120, 32));
    const edge = EDGE.map(raw => mode === 'imm' ? [0x0f, immediate, 0x37, raw] : [0x0f, cl, 0x0f]);
    banks.push(bank(`edge-${kind}-${mode}`, kind, mode, edge, EDGE.map(raw => ({source: mode === 'imm' ? 6 : 1, raw})), mode === 'imm' ? 96 : 88, 32));
  }
  const forms = [[0x0f, cl, 0], [0x0f, immediate, 0x24, 0x24, 1], [0x0f, immediate, 0x6d, 0, 1],
    [0x0f, cl, 0x4c, 0x8f, 0xf0], [0x0f, immediate, 0x1c, 0x9b, 1],
    [0x83, 0xc3, 1, 0x0f, kind === 'shld' ? immediate : cl, 0x0f, ...(kind === 'shld' ? [32] : [])],
    [0xbe, ...(kind === 'shld' ? [0x78, 0x56, 0x34, 0x12] : [1, 0, 0, 0]), 0x0f, immediate, 0x0f, kind === 'shld' ? 32 : 1]];
  const descriptors = [{source: 0, mode: 'cl', raw: 1}, {source: 4, mode: 'imm', raw: 1}, {source: 5, mode: 'imm', raw: 1},
    {source: 1, mode: 'cl', raw: 1}, {source: 3, mode: 'imm', raw: 1},
    {source: 1, mode: kind === 'shld' ? 'imm' : 'cl', raw: kind === 'shld' ? 32 : 33, prefix: 3},
    {source: 1, mode: 'imm', raw: kind === 'shld' ? 32 : 1, prefix: 5, canary: true}];
  const selected = bank(`special-${kind}`, kind, 'mixed', forms, descriptors, kind === 'shld' ? 100 : 99, 31);
  assert.deepEqual(selected.scans.map(scan => scan.entry - PC), [0, 11, 24, 37, 50, 63, kind === 'shld' ? 78 : 77]); banks.push(selected);
}
const faultShapes = [
  {name: 'cross-read-unmapped', detail: 1, fault: SECOND, access: 1, missing: SECOND},
  {name: 'cross-read-denied', detail: 2, fault: SECOND, access: 1, denied: SECOND, permissions: 2},
  {name: 'cross-first-read-only', detail: 2, fault: 0x4ffe, access: 2, denied: DATA, permissions: 1},
  {name: 'cross-second-read-only', detail: 2, fault: SECOND, access: 2, denied: SECOND, permissions: 1},
];
const formulas = {matrix_targets: OWNERS.length * KINDS.length * MODES.length * 8 * FLAGS.length,
  edge_targets: OWNERS.length * KINDS.length * MODES.length * EDGE.length * FLAGS.length, literal_targets: OWNERS.length * ANCHORS.length * FLAGS.length,
  alias_targets: OWNERS.length * KINDS.length * 5 * FLAGS.length, range_targets: OWNERS.length * KINDS.length * 2 * FLAGS.length,
  repaired_targets: OWNERS.length * KINDS.length * faultShapes.length * FLAGS.length, smc_targets: OWNERS.length * KINDS.length};
const plannedTargets = Object.values(formulas).reduce((total, value) => total + value, 0), regularTargets = plannedTargets - formulas.smc_targets;
const overflowCases = OWNERS.length * KINDS.length * FLAGS.length, faultCalls = 2 * (formulas.repaired_targets + overflowCases);
const normalContexts = OWNERS.length * KINDS.length * MODES.length, regularContexts = normalContexts * 2 + OWNERS.length * KINDS.length;
const planned = {targets: plannedTargets, regular_targets: regularTargets, overflow_cases: overflowCases, contexts: regularContexts + formulas.smc_targets,
  modules: regularContexts + 2 * formulas.smc_targets, seeds: plannedTargets + overflowCases,
  generated_calls: regularTargets * 4 + faultCalls + normalContexts * 4 + regularContexts + formulas.smc_targets * 7,
  retired: regularTargets * 4 + formulas.repaired_targets + overflowCases + formulas.smc_targets * 6,
  raw_arenas: plannedTargets + faultCalls + formulas.smc_targets * 4,
  pages: normalContexts * 8 * 2 + OWNERS.length * KINDS.length * 132 + formulas.smc_targets * 8,
  source_spans: normalContexts * 8 * 2 + OWNERS.length * KINDS.length * 7 + formulas.smc_targets * 8,
  transfer_requests: 540, host_inputs: 956, arena_checks: 1416496};
planned.raw_bytes = planned.raw_arenas * SIZE; planned.diagnostic_read32_calls = planned.pages * 1024 + planned.targets;
planned.files = planned.pages + planned.modules + banks.length + 4;
assert.deepEqual([planned.targets, planned.regular_targets, planned.overflow_cases, planned.contexts, planned.modules, planned.seeds, planned.generated_calls,
  planned.retired, planned.raw_arenas, planned.raw_bytes, planned.pages, planned.source_spans, planned.diagnostic_read32_calls, planned.files],
  [380, 376, 8, 24, 28, 388, 1664, 1568, 476, 2016336, 688, 188, 704892, 730]);
const counts = {contexts: 0, modules: 0, seeds: 0, targets: 0, matrix_targets: 0, edge_targets: 0, literal_targets: 0, alias_targets: 0, range_targets: 0,
  repaired_targets: 0, smc_targets: 0, shld_targets: 0, shrd_targets: 0, imm_targets: 0, cl_targets: 0, zero_targets: 0, count_one_targets: 0,
  multiple_count_targets: 0, generated_calls: 0, retired: 0, raw_arenas: 0, arena_checks: 0, pages: 0, diagnostic_read32_calls: 0, target_read32_calls: 0,
  transfer_requests: 0, setb: 0, seto: 0, jumps: 0, canaries: 0, retired_add_prefixes: 0, retired_mov_prefixes: 0, overflow_cases: 0, fault_calls: 0,
  malformed_calls: 0, cancel_calls: 0, zero_budget_calls: 0, stale_calls: 0, closed_calls: 0, permission_only_repairs: 0, changed_data_repairs: 0,
  restored_map_repairs: 0, same_value_code_stores: 0, changing_code_stores: 0};
const contexts = [], modules = [], targets = [], faults = [], codeStores = [], pagesRead = [], runs = [], hostInputs = [], artifacts = [], rawFrames = [], rawRecords = [];
function artifact(path, bytes, write = true) {
  assert.ok(!artifacts.some(row => row.path === path)); if (write) writeFileSync(join(output, path), bytes, {flag: 'wx'});
  const row = {path, bytes: bytes.length, sha256: hash(bytes)}; artifacts.push(row); return row;
}
artifact('engine.wasm', engineBytes); for (const selected of banks) artifact(`${selected.name}.x86`, selected.bytes, false);
const source = {provenance: 'existing physically pinned local Intel primary; no fresh download or latest-edition claim',
  prerequisite_policy: 'ignored immutable local PDF/full text required offline before guest execution; no bootstrap or clean-checkout/fullCI claim',
  pdf_path: 'target/p2-memory-binary-spec/253667-093-sdm-vol-2b.pdf', pdf_bytes: 6915682, pdf_sha256: 'a261998ace8e07f624bf3e2486bb6cfe950fb8be8d0dc9631bcecb4c5ec953e4',
  full_text_path: 'target/p2-memory-binary-spec/253667-093-sdm-vol-2b.txt', full_text_bytes: 1537522, full_text_sha256: 'f5e6dc689d41655d64792512bfe8add56fc4ac97203aab5c1377a72bac291a35',
  chapter_lines: [31544, 31751], chapter_bytes: 10703, chapter_sha256: 'dd24e252d5d4c230107617c86ac281c110c390f835b9daae36fcf42c6617603c',
  chapters: [{name: 'SHLD', lines: [31544, 31647], bytes: 5307, sha256: '25c456314acbf78cb431300e191e189f9bffe4093a7de41bb261914c295a11f9'},
    {name: 'SHRD', lines: [31649, 31751], bytes: 5383, sha256: '5a8af8f3bbb51273e06f00de43466b6d8389c6c8956abb8acd66d7e24a5a6e32'}],
  chapter_separator_policy: 'complete chapters and internal PDF PAGE separators, including line31648; excludes successor chapter'};
for (const kind of ['pdf', 'full_text']) {const bytes = readFileSync(join(root, source[`${kind}_path`])); assert.equal(bytes.length, source[`${kind}_bytes`]); assert.equal(hash(bytes), source[`${kind}_sha256`]);}
const fullTextLines = readFileSync(join(root, source.full_text_path), 'utf8').split('\n');
const chapterBytes = (start, end) => Buffer.from(fullTextLines.slice(start - 1, end).join('\n') + '\n');
for (const chapter of source.chapters) {const bytes = chapterBytes(...chapter.lines); assert.equal(bytes.length, chapter.bytes); assert.equal(hash(bytes), chapter.sha256);}
const chapter = chapterBytes(...source.chapter_lines); assert.equal(chapter.length, source.chapter_bytes); assert.equal(hash(chapter), source.chapter_sha256);
source.saved_chapter = artifact('intel-memory-double-shift.txt', chapter);
const sourcePaths = ['engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/integer.rs', 'engine/tests/cpu_double_shift32.rs',
  'engine/tests/cpu_memory_double_shift32.rs', 'engine/tests/cpu_memory_double_shift32_wasm.rs',
  'engine/tests/fixtures/p2-memory-double-shift32/run.mjs', 'engine/tests/fixtures/support/engine.mjs'];
assert.equal(new Set(sourcePaths).size, 10);
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
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
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
function upload(ctx, address, bytes, purpose) {request(ctx, bytes, `request: ${purpose}`); pure(ctx, () => ctx.api.upload(address, bytes.length), purpose); patch(ctx, address, bytes);}
function map(ctx, address, bytes = pattern, permissions = 3) {pure(ctx, () => ctx.api.map(address, 1, permissions), 'map page'); ctx.pages.set(address, Buffer.alloc(4096)); ctx.permissions.set(address, permissions); upload(ctx, address, bytes, 'page input');}
function unmap(ctx, address) {pure(ctx, () => ctx.api.unmap(address, 1), 'unmap page'); assert.ok(ctx.pages.delete(address)); ctx.permissions.delete(address);}
function protect(ctx, address, permissions) {pure(ctx, () => ctx.api.protect(address, 1, permissions), 'permission change'); ctx.permissions.set(address, permissions);}
function fresh(owner, selected, label, writable = false) {
  const instance = new WebAssembly.Instance(engineModule, {}), api = {}, ordinal = ++counts.contexts;
  const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, unmap: 2, protect: 3, upload: 2, read32: 1,
    compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store32: 2, store_resident32: 6};
  for (const [name, arity] of Object.entries(arities)) {api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);}
  const ctx = {owner, ordinal, selected, label, api, memory: instance.exports.memory, low: ordinal, high: 0x4d445332,
    expected: Buffer.alloc(SIZE), pages: new Map(), permissions: new Map(), units: []};
  assert.ok(ctx.memory instanceof WebAssembly.Memory); assert.equal(api.open(4, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  refresh(ctx); assert.ok(ctx.base > 0 && ctx.base + SIZE <= ctx.bytes.length);
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(exit(1, 1, 0), 56); ctx.expected.set(helper(), 100); check(ctx, 'independently initialized arena');
  const codePage = Buffer.alloc(4096); codePage.set(selected.bytes); map(ctx, PC, codePage, 7); if (!writable) protect(ctx, PC, 5);
  for (const address of [KEEP, DATA, SECOND]) map(ctx, address);
  contexts.push({owner, context: ordinal, label, bank: selected.name, key: [ctx.low, ctx.high], base: ctx.base, page_limit: 4, code_permissions: writable ? 7 : 5}); return ctx;
}
const identity = (ctx, unit) => ctx.owner === 'replacement' ? {generation: unit.generation} : {low: unit.low, high: unit.high};
function physical(ctx, unit) {refresh(ctx); assert.ok(unit.pointer > 0 && unit.pointer + unit.length <= ctx.bytes.length); const bytes = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.deepEqual(bytes, unit.bytes, 'whole live module allocation retained'); return {pointer: unit.pointer, bytes: unit.length, sha256: hash(bytes)};}
function compile(ctx, phase = 'initial', tail) {
  const specs = tail ? [[tail.next, 13]] : ctx.selected.specs, memory = !tail;
  request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat()), `compile ${phase} input`); let binding;
  if (ctx.owner === 'replacement') {pure(ctx, () => ctx.api.compile_entries(specs.length, 0), 'entry compiler'); binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);}
  else {check(ctx, 'before resident compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
    binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
    assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident receipt only');
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
function pages(ctx, label) {
  for (const [address, expected] of ctx.pages) {
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
  return [registers[0], registers[4], registers[5], (registers[7] + registers[1] * 4 - 16) >>> 0, (registers[3] * 5) >>> 0][scan.alias];
}
function target(ctx, unit, scan, group, label, reason = 1) {
  const original = cpu(ctx), address = addressFor(scan, original.registers), raw = scan.mode === 'imm' ? scan.raw : original.registers[1] % 256;
  assert.equal(original.pc, scan.target); const oldDestination = wordAt(ctx, address), oldSource = original.registers[scan.source], expected = doubleShift(scan.kind, oldDestination, oldSource, raw, original.flags);
  patch(ctx, address, words([expected.value])); const call = run(ctx, unit, 1, label, {pc: scan.next, flags: expected.flags, reason, retired: 1, helperBytes: helper()});
  const saved = saveArena(ctx, `${label}-target`, unit); counts.targets++; counts[group]++; counts[`${scan.kind}_targets`]++; counts[`${scan.mode}_targets`]++;
  counts[expected.count === 0 ? 'zero_targets' : expected.count === 1 ? 'count_one_targets' : 'multiple_count_targets']++;
  const row = {ordinal: counts.targets, context: ctx.ordinal, module: unit.file, group, label, scan, address, old_destination: oldDestination, old_source: oldSource,
    raw_count: raw, masked_count: expected.count, incoming_flags: original.flags, expected, call: call.ordinal, before: call.before, first_target: call.after, raw_arena: saved,
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
function preflight(ctx, unit, label) {
  cancel(ctx, 1); run(ctx, unit, 0, `${label}-malformed-before-cancel`, {}, 1, true); counts.malformed_calls++;
  run(ctx, unit, 0, `${label}-cancel-before-budget`, {reason: 2}); run(ctx, unit, 1, `${label}-cancel`, {reason: 2}); counts.cancel_calls += 2;
  cancel(ctx, 0); run(ctx, unit, 0, `${label}-budget-zero`, {}); counts.zero_budget_calls++;
}
function finish(ctx, unit) {for (const wrong of ['key', 'identity']) pure(ctx, () => guard(ctx, unit, wrong), `wrong ${wrong}`, 3);
  pure(ctx, () => ctx.api.close(), 'close regular process'); run(ctx, unit, 0, 'closed-before-malformed', {}, 5, true); counts.closed_calls++;}

function normal(owner, selected) {
  const ctx = fresh(owner, selected, 'all source parents and literal anchors'), unit = compile(ctx); pages(ctx, 'initial');
  for (const scan of selected.scans) for (const flags of FLAGS) {
    const registers = [...REG], authored = {...scan, address_kind: 'absolute'}, label = `matrix-${scan.source}-${flags.toString(16)}`;
    upload(ctx, WORD, words([0x80000001]), 'matrix operand'); seed(ctx, registers, scan.target, flags, label);
    if (scan.source === 0 && flags === 2) preflight(ctx, unit, label);
    const row = target(ctx, unit, authored, 'matrix_targets', label); targetRam(ctx, row); consumers(ctx, unit, row);
  }
  for (const [kind, mode, destination, source, value, anchorFlags] of ANCHORS.filter(anchor => anchor[0] === selected.kind && anchor[1] === selected.mode)) for (const flags of FLAGS) {
    const sourceIndex = mode === 'imm' ? 1 : 6, scan = {...selected.scans[sourceIndex], address_kind: 'absolute'}, registers = [...REG];
    if (mode === 'cl') registers[1] = 0x12345601; registers[sourceIndex] = source;
    const label = `literal-${destination.toString(16)}-${source.toString(16)}-${flags.toString(16)}`;
    upload(ctx, WORD, words([destination]), 'literal operand'); seed(ctx, registers, scan.target, flags, label);
    const row = target(ctx, unit, scan, 'literal_targets', label); assert.equal(row.expected.value, value); assert.equal(row.expected.flags, anchorFlags + (flags & 0x400));
    targetRam(ctx, row); consumers(ctx, unit, row);
  }
  pages(ctx, 'final-normal'); finish(ctx, unit);
}
function edge(owner, selected) {
  const ctx = fresh(owner, selected, 'masked count edges and complete ECX source'), unit = compile(ctx); pages(ctx, 'initial');
  for (const scan of selected.scans) for (const flags of FLAGS) {
    const registers = [...REG], authored = {...scan, address_kind: 'edi'}, label = `edge-${scan.raw}-${flags.toString(16)}`;
    registers[7] = WORD; if (scan.mode === 'imm') registers[6] = 0x12345678; else registers[1] = 0x12345600 + scan.raw;
    upload(ctx, WORD, words([0x80000001]), 'edge operand'); seed(ctx, registers, scan.target, flags, label);
    const row = target(ctx, unit, authored, 'edge_targets', label);
    if (scan.raw % 32 === 0) {assert.equal(row.expected.value, 0x80000001); assert.equal(row.expected.flags, flags); assert.notEqual(row.old_source, row.old_destination);}
    if (scan.mode === 'cl' && scan.raw === 255) {
      assert.equal(row.old_source, 0x123456ff); assert.equal(row.expected.value, scan.kind === 'shld' ? 0x891a2b7f : 0x2468adff);
      const wrong = doubleShift(scan.kind, row.old_destination, 255, 255, flags); assert.notEqual(wrong.value, row.expected.value);
      row.full_source_discriminator = {authority: 'hypothetical mathematical wrong path; not executed', cl_only_value: wrong.value,
        expected_cl_only_literal: scan.kind === 'shld' ? 0x8000007f : 0x1ff}; assert.equal(wrong.value, row.full_source_discriminator.expected_cl_only_literal);
    }
    targetRam(ctx, row); consumers(ctx, unit, row);
  }
  pages(ctx, 'final-edge'); finish(ctx, unit);
}
function faultFrame(ctx, unit, scan, shape, registers, prefixFlags, first, label) {
  const failure = {registers, pc: scan.target, flags: prefixFlags, reason: 5, retired: Number(first), detail: shape.detail, fault: shape.fault,
    access: shape.access, helperBytes: helper(0, shape.detail, shape.fault, shape.access)};
  const call = run(ctx, unit, first ? 2 : 1, label, failure); counts.fault_calls++; if (first) counts.retired_add_prefixes++;
  const raw = saveArena(ctx, label, unit); pages(ctx, label); return {call, raw};
}
function special(owner, selected) {
  const ctx = fresh(owner, selected, 'old EA aliases, DWORD endpoints and precise faults'), unit = compile(ctx); pages(ctx, 'initial');
  for (let form = 0; form < 5; form++) for (const flags of FLAGS) {
    const scan = {...selected.scans[form], alias: form}, registers = [...REG];
    if (form === 0) registers[0] = WORD; if (form === 1) registers[4] = WORD; if (form === 2) registers[5] = WORD;
    if (form === 3) {registers[1] = 0x80000021; registers[7] = 0x3f9c;} if (form === 4) registers[3] = 0xcd0;
    assert.equal(addressFor(scan, registers), WORD); const destination = form === 3 ? registers[1] : 0x80000001, label = `alias-${form}-${flags.toString(16)}`;
    upload(ctx, WORD, words([destination]), 'alias operand'); seed(ctx, registers, scan.target, flags, label);
    const row = target(ctx, unit, scan, 'alias_targets', label); targetRam(ctx, row); consumers(ctx, unit, row);
  }
  pages(ctx, 'after-aliases');
  const rangeScan = {...selected.scans[1], alias: 1};
  for (const flags of FLAGS) {
    const registers = [...REG]; registers[4] = 0x4ffe; const label = `crossing-${flags.toString(16)}`;
    upload(ctx, 0x4ffe, words([0x80000001]), 'crossing DWORD operand'); seed(ctx, registers, rangeScan.target, flags, label);
    const row = target(ctx, unit, rangeScan, 'range_targets', label); targetRam(ctx, row); consumers(ctx, unit, row);
  }
  pages(ctx, 'after-crossing'); unmap(ctx, SECOND); map(ctx, TOP);
  for (const flags of FLAGS) {
    const registers = [...REG]; registers[4] = 0xfffffffc; const label = `final-dword-${flags.toString(16)}`;
    upload(ctx, 0xfffffffc, words([0x80000001]), 'complete final DWORD operand'); seed(ctx, registers, rangeScan.target, flags, label);
    const row = target(ctx, unit, rangeScan, 'range_targets', label); targetRam(ctx, row); consumers(ctx, unit, row);
  }
  pages(ctx, 'after-final-dword'); const scan = {...selected.scans[5], address_kind: 'edi'};
  for (const seedFlags of FLAGS) {
    const registers = [...REG]; registers[1] = selected.kind === 'shld' ? 0x80000000 : 0x80000021; registers[3] = 0xffffffff; registers[7] = 0xfffffffd;
    const label = `overflow-${seedFlags.toString(16)}`; seed(ctx, registers, scan.entry, seedFlags, label); registers[3] = 0;
    const prefixFlags = 0x57 + (seedFlags & 0x400), shape = {name: 'nonwrapping-read-overflow', detail: 3, fault: 0xfffffffd, access: 1};
    const first = faultFrame(ctx, unit, scan, shape, registers, prefixFlags, true, `${label}-first`);
    const retry = faultFrame(ctx, unit, scan, shape, registers, prefixFlags, false, `${label}-retry`); counts.overflow_cases++;
    faults.push({context: ctx.ordinal, module: unit.file, ...shape, address: 0xfffffffd, width: 4, seed_flags: seedFlags, prefix_flags: prefixFlags, first, retry,
      repair_mode: 'none; complete address range overflows and permission changes cannot repair it', successful_target: null});
  }
  unmap(ctx, TOP); map(ctx, SECOND); pages(ctx, 'restored-four-pages');
  for (const shape of faultShapes) for (const seedFlags of FLAGS) {
    protect(ctx, DATA, 3); protect(ctx, SECOND, 3); upload(ctx, 0x4ffe, words([0x7fffffff]), 'fault original operand');
    if (shape.missing !== undefined) unmap(ctx, SECOND); if (shape.denied !== undefined) protect(ctx, shape.denied, shape.permissions);
    const registers = [...REG]; registers[1] = selected.kind === 'shld' ? 0x80000000 : 0x80000021; registers[3] = 0xffffffff; registers[7] = 0x4ffe;
    const label = `${shape.name}-${seedFlags.toString(16)}`; seed(ctx, registers, scan.entry, seedFlags, label); registers[3] = 0;
    const prefixFlags = 0x57 + (seedFlags & 0x400), first = faultFrame(ctx, unit, scan, shape, registers, prefixFlags, true, `${label}-first`);
    const retry = faultFrame(ctx, unit, scan, shape, registers, prefixFlags, false, `${label}-retry`);
    const changed = seedFlags === 2, repairedValue = changed ? selected.kind === 'shld' ? 0x12345678 : 0xffffffff : 0x7fffffff;
    let repairMode;
    if (shape.missing !== undefined) {
      map(ctx, SECOND); upload(ctx, 0x4ffe, words([repairedValue]), 'map and DWORD data restoration'); repairMode = 'map+data-restoration';
      if (changed) counts.changed_data_repairs++; else counts.restored_map_repairs++;
    } else {
      protect(ctx, shape.denied, 3); repairMode = changed ? 'permission+changed-data' : 'permission-only';
      if (changed) {upload(ctx, 0x4ffe, words([repairedValue]), 'permission and changed operand repair'); counts.changed_data_repairs++;} else counts.permission_only_repairs++;
    }
    pure(ctx, () => guard(ctx, unit), 'access/data-only repair retains original owner'); assert.equal(wordAt(ctx, 0x4ffe), repairedValue);
    const row = target(ctx, unit, scan, 'repaired_targets', `${label}-repaired`);
    if (selected.kind === 'shld') {assert.equal(row.expected.value, repairedValue); assert.equal(row.expected.flags, prefixFlags);}
    else {assert.equal(row.expected.value, changed ? 0xffffffff : 0xbfffffff); assert.equal(row.expected.flags, changed ? 0x87 : 0xc87);}
    targetRam(ctx, row); consumers(ctx, unit, row); pages(ctx, `${label}-repaired`);
    faults.push({context: ctx.ordinal, module: unit.file, ...shape, address: 0x4ffe, width: 4, seed_flags: seedFlags, prefix_flags: prefixFlags,
      original_operand: 0x7fffffff, first, retry, repair_mode: repairMode, required_access_restoration: shape.missing !== undefined ? 'map SECOND' : `protect ${shape.denied.toString(16)} RW`,
      repair_value: repairedValue, changed_operand: changed, successful_target: row.ordinal, no_reseed: 'same original child/CPU; only access/data repair and typed diagnostics'});
  }
  pages(ctx, 'final-special'); finish(ctx, unit);
}
function smc(owner, selected) {
  const changing = selected.kind === 'shrd', ctx = fresh(owner, selected, changing ? 'changing consumed MOV immediate' : 'equal count-zero consumed MOV immediate', true);
  const old = compile(ctx), scan = {...selected.scans[6], address_kind: 'edi'}; pages(ctx, 'initial-smc');
  for (const wrong of ['key', 'identity']) pure(ctx, () => guard(ctx, old, wrong), `SMC wrong ${wrong}`, 3);
  const registers = [...REG]; registers[1] = changing ? 1 : 0x87654321; registers[7] = scan.entry + 1;
  seed(ctx, registers, scan.entry, 0xcd7, 'one SMC head/target/tail seed'); registers[6] = changing ? 1 : 0x12345678;
  const mov = run(ctx, old, 1, 'consumed MOV head retires once', {registers, pc: scan.target, retired: 1}); counts.retired_mov_prefixes++;
  const movArena = saveArena(ctx, 'smc-prefix-MOV', old), beforeOld = physical(ctx, old), row = target(ctx, old, scan, 'smc_targets', 'consumed-MOV-store', 6);
  assert.equal(row.old_destination, changing ? 1 : 0x12345678); assert.equal(row.expected.value, changing ? 0x80000000 : 0x12345678);
  assert.equal(row.expected.flags, changing ? 0xc87 : 0xcd7); counts[changing ? 'changing_code_stores' : 'same_value_code_stores']++;
  let retainedOld; cancel(ctx, 1); const staleOld = run(ctx, old, 0, 'stale old before malformed cancelled zero-budget call', {}, 4, true, () => {
    retainedOld = physical(ctx, old); assert.deepEqual(retainedOld, beforeOld, 'immediate whole old allocation before post-call arena checks/snapshots/getters/diagnostics/save/publication');
  }); counts.stale_calls++;
  const staleOldArena = saveArena(ctx, 'smc-old-stale', old); cancel(ctx, 0); const committed = snapshot(ctx), current = compile(ctx, 'current-tail', scan);
  assert.notDeepEqual(identity(ctx, current), identity(ctx, old)); assert.deepEqual(core(snapshot(ctx)), core(committed), 'compile retains State/Exit/helper while transfer request/receipt changes are independently modeled');
  assert.equal(current.version, 1); const tailRegisters = [...cpu(ctx).registers];
  tailRegisters[0] = replaceLowByte(tailRegisters[0], row.expected.flags % 2); tailRegisters[2] = replaceLowByte(tailRegisters[2], Number((row.expected.flags & 0x800) !== 0)); tailRegisters[3] = 0xdeadbeef;
  const tail = run(ctx, current, 5, 'pure current tail without head/target replay', {registers: tailRegisters, pc: COLD, reason: 3, retired: 4});
  counts.setb++; counts.seto++; counts.canaries++; counts.jumps++; const tailArena = saveArena(ctx, 'smc-current-tail', current);
  assert.equal(cpu(ctx).registers[6], changing ? 1 : 0x12345678); assert.equal(cpu(ctx).registers[1], changing ? 1 : 0x87654321);
  assert.deepEqual(Buffer.from(snapshot(ctx).helper_hex, 'hex'), helper(), 'Store4 success helper retained through guard-only continuation');
  targetRam(ctx, row); pages(ctx, 'after-current-tail');
  const beforeTail = physical(ctx, current), sameByte = ctx.pages.get(PC)[scan.next - PC]; assert.equal(sameByte, 0x0f);
  upload(ctx, scan.next, Buffer.from([sameByte]), 'same consumed-tail byte'); cancel(ctx, 1);
  let retainedTail; const staleTail = run(ctx, current, 0, 'stale fresh tail before malformed cancelled zero-budget call', {}, 4, true, () => {
    retainedTail = physical(ctx, current); assert.deepEqual(retainedTail, beforeTail, 'immediate whole fresh allocation before post-call arena checks/snapshots/getters/diagnostics/save/close');
  }); counts.stale_calls++;
  const staleTailArena = saveArena(ctx, 'smc-current-stale', current); pure(ctx, () => ctx.api.close(), 'close pending-cancel stale owners');
  for (const unit of [old, current]) {run(ctx, unit, 0, 'closed before stale malformed cancelled zero-budget call', {}, 5, true); counts.closed_calls++;}
  const replay = changing ? doubleShift('shrd', row.expected.value, row.old_source, row.raw_count, row.expected.flags) : null;
  if (replay) {assert.equal(replay.value, 0xc0000000); assert.equal(replay.flags, 0x486);}
  codeStores.push({context: ctx.ordinal, changing, address: registers[7], consumed_instruction: 'MOV ESI, imm32', mov, mov_arena: movArena, target: row.ordinal,
    store_status: {value: 11, authority: 'source-inferred; direct managed helper import is not intercepted'}, observed_helper_success: true, observed_exit: 6, observed_retired: 1,
    old_identity: identity(ctx, old), current_identity: identity(ctx, current), stale_old: staleOld, old_stale_arena: staleOldArena, retained_old_after_stale: retainedOld,
    current_tail: tail, current_tail_arena: tailArena, stale_tail: staleTail, current_stale_arena: staleTailArena, retained_tail_after_stale: retainedTail,
    fresh_tail_exit_version: 1, ram_observation_phase: 'after pure tail; target raw arena/helper saved before any diagnostic',
    replay_discriminator: changing ? {authority: 'hypothetical independent mathematical wrong paths; never executed', target_replay: replay,
      replayed_head_esi: 0x80000000, retained_esi: 1} : {authority: 'zero arithmetic is idempotent; current-tail entry/source/retirement and preserved CPU prove continuation, no arithmetic-value discriminator'}});
}
for (const owner of OWNERS) {
  for (const selected of banks.filter(bank => bank.name.startsWith('normal-'))) normal(owner, selected);
  for (const selected of banks.filter(bank => bank.name.startsWith('edge-'))) edge(owner, selected);
  for (const selected of banks.filter(bank => bank.name.startsWith('special-'))) special(owner, selected);
  for (const selected of banks.filter(bank => bank.name.startsWith('special-'))) smc(owner, selected);
}
for (const [name, value] of Object.entries(formulas)) assert.equal(counts[name], value, `observed ${name}`);
for (const name of ['targets', 'contexts', 'modules', 'seeds', 'generated_calls', 'retired', 'raw_arenas', 'pages', 'diagnostic_read32_calls', 'transfer_requests', 'arena_checks', 'overflow_cases']) assert.equal(counts[name], planned[name], `observed ${name}`);
assert.equal(counts.target_read32_calls, planned.targets); assert.equal(runs.length, planned.generated_calls); assert.equal(hostInputs.length, planned.host_inputs);
assert.equal(counts.retired_add_prefixes, formulas.repaired_targets + overflowCases); assert.equal(counts.fault_calls, faultCalls);
assert.deepEqual([counts.shld_targets, counts.shrd_targets, counts.imm_targets, counts.cl_targets, counts.zero_targets, counts.count_one_targets, counts.multiple_count_targets], [190, 190, 204, 176, 82, 250, 48]);
assert.deepEqual([counts.setb, counts.seto, counts.jumps, counts.canaries, counts.retired_mov_prefixes, counts.malformed_calls, counts.cancel_calls, counts.zero_budget_calls, counts.stale_calls, counts.closed_calls], [380, 380, 380, 4, 4, 8, 16, 8, 8, 28]);
assert.deepEqual([counts.permission_only_repairs, counts.changed_data_repairs, counts.restored_map_repairs, counts.same_value_code_stores, counts.changing_code_stores], [12, 16, 4, 2, 2]);
assert.equal(modules.reduce((total, row) => total + row.source_spans.length, 0), planned.source_spans);
assert.equal(runs.reduce((total, row) => total + row.retired, 0), planned.retired);
const raw = Buffer.concat(rawFrames); assert.equal(raw.length, planned.raw_bytes); const rawArtifact = artifact('arena-snapshots.bin', raw);
assert.deepEqual(sourceIdentities(), sourcePins, 'selected ten sources unchanged before/after actual calls'); assert.equal(hash(readFileSync(enginePath)), engineSha256, 'actual engine input unchanged');
const names = [...artifacts.map(row => row.path), 'result.json']; assert.equal(new Set(names).size, names.length); assert.equal(names.length, planned.files);
const result = {status: 'ok', engine_sha256: engineSha256, engine_bytes: engineBytes.length,
  engine_input_policy: 'readEngine reads current bytes once; optional caller RING3_ENGINE_SHA256 is verified; saved bytes and before/after current input are pinned', tools: {node: process.version, v8: process.versions.v8},
  counts, formulas, planned, contexts, modules, targets, faults, code_stores: codeStores, pages: pagesRead, generated_runs: runs, host_inputs: hostInputs,
  raw_arena_records: rawRecords, raw_arena_artifact: rawArtifact, source, source_pins_before: sourcePins, source_pins_after: sourceIdentities(),
  oracle: {anchors: ANCHORS, flag_seeds: FLAGS, register_sentinels: REG, edge_raw_counts: EDGE, pattern_sha256: hash(pattern), arena_bytes: SIZE,
    banks: banks.map(selected => ({name: selected.name, kind: selected.kind, mode: selected.mode, packed_bytes: selected.packed_bytes, physical_bytes: selected.physical_bytes,
      hex: selected.bytes.toString('hex'), sha256: hash(selected.bytes), specs: selected.specs, scans: selected.scans, instructions: selected.instructions})),
    arithmetic: 'repeated one-bit sequence transfer using independent binary digits and last removed destination bit; parity from eight binary digits; count-one old/new sign XOR; masked-zero preserves full admitted FLAGS/DWORD',
    flags_policy: 'accepted current mask0xcd7 with mandatory0x2; AF at nonzero and OF above1 deliberately zero under Ring3 policy, no physical CPU undefined-bit claim',
    checkpoint_policy: 'complete live4236 arena around every generated run and map/unmap/protect/upload/guard/close/Read32; explicit host writes checked after mutation; compiler request/receipt/getter/link groups checked; initialization checked after open/getter group; raw target/fault/SMC frames saved before diagnostic helper changes',
    ram_policy: '376 normal/edge/literal/alias/range/repaired actual Read32 observations after raw target before SETcc; four SMC Read32 observations after pure continuation; all688 page files are aligned typed Read32 observations, not direct raw managed RAM access',
    continuation_policy: 'access restoration plus declared optional operand repair at same original child/CPU; no CPU/helper reseed; normal diagnostic Read32 helper persists into flag consumers, SMC Store4 helper persists through fresh guard-only tail before diagnostics',
    live_evidence_limit: 'saved raw frames/pages/modules do not reproduce temporal immediate whole live allocation checks or every unsaved checkpoint',
    inherited_pre_execution_lessons: ['regular JMP uses budget2 for one retirement then NeedCode; fresh four-instruction tail budget5', 'compile can change transfer request/receipt while State/Exit/helper stay committed'],
    accepted_contract_literal_correction: {label: 'LIT-MDS-A7', incorrect_historical_shrd: 0x246addff, corrected_shrd: 0x2468adff, authority: 'root pre-execution numeric falsification; source bit-sequence literal and no corpus/count change; no guest/product failure'},
    retention_order_correction: {label: 'LIVE-MDS-A8', historical_driver_path: 'target/r3-memory-double-shift32-driver-before-retention-order-fix.mjs',
      historical_driver_bytes: 53812, historical_driver_sha256: '450514b51342dd59d12905155c5ac31e537b26ce8b2014a1ffa606c621727df8',
      authority: 'root pre-use fixture ordering review; move only two SMC stale allocation comparisons into immediate post-unit.run callback before post-call arena checks/snapshots; no corpus/count change and no executed failure/product bug'}},
  artifacts, artifact_census: {modules: planned.modules, program_banks: banks.length, pages: planned.pages, raw_arenas_files: 1, engine_files: 1, chapter_files: 1, result_files: 1, total: names.length, names},
  claim: 'finite prefix-free flat32 memory SHLD/SHRD imm8/CL through current managed replacement/resident owners; all eight source GPRs and selected masked edges/aliases/literal flags; precise checked Read4/Write4 faults with prior ADD retirement, unchanged retry and declared access/data repair without reseed; complete first-target CPU/Exit/helper/arena before consumers and actual typed RAM observations; same-value masked-zero and changing consumed-MOV stores retire once with observed exit6/helper success, raw11 source-inferred; stale4 immediate full live allocations, distinct pure current tail without replay, same-byte tail invalidation and closed5 priority; no hardware undefined bits/bus/fault-cycle/atomicity/concurrency, broad EA/ISA/PE/Windows/SDK/browser/game/performance/fullCI/clean-checkout or remaining Wasm-body certification claim'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), names.sort(), 'exact physical artifact census');
console.log(JSON.stringify({status: 'ok', output: join(output, 'result.json'), sha256: hash(resultBytes), counts, planned}));
