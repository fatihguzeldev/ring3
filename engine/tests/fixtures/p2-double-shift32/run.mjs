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
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = PC + 128;
const OWNERS = ['replacement', 'resident'], FLAGS = [2, 0xcd7], EDGE_COUNTS = [0, 32, 64, 128, 2, 31, 33, 255];
const REG = [0x12345678, 0x80000021, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const ENCODINGS = [{kind: 'shld', mode: 'imm', opcode: 0xa4}, {kind: 'shld', mode: 'cl', opcode: 0xa5},
  {kind: 'shrd', mode: 'imm', opcode: 0xac}, {kind: 'shrd', mode: 'cl', opcode: 0xad}];
const ANCHORS = [
  {kind: 'shld', destination: 0x80000000, source: 0, value: 0, flags: 0x847},
  {kind: 'shld', destination: 0, source: 0x80000000, value: 1, flags: 2},
  {kind: 'shld', destination: 0x40000000, source: 0, value: 0x80000000, flags: 0x886},
  {kind: 'shld', destination: 0xffffffff, source: 0, value: 0xfffffffe, flags: 0x83},
  {kind: 'shrd', destination: 0, source: 1, value: 0x80000000, flags: 0x886},
  {kind: 'shrd', destination: 0x80000000, source: 0, value: 0x40000000, flags: 0x806},
  {kind: 'shrd', destination: 1, source: 0, value: 0, flags: 0x47},
  {kind: 'shrd', destination: 0x80000001, source: 1, value: 0xc0000000, flags: 0x87},
  {kind: 'shld', destination: 0, source: 0, value: 0, flags: 0x46, self: true},
  {kind: 'shrd', destination: 0, source: 0, value: 0, flags: 0x46, self: true},
];
function bits(value) {return value.toString(2).padStart(32, '0').split('').map(Number);}
function doubleShift(kind, destination, source, raw, incoming) {
  assert.ok(FLAGS.includes(incoming) || (incoming & 2) !== 0 && (incoming & ~0xcd7) === 0);
  const count = raw % 32, original = bits(destination), sourceBits = bits(source), result = [...original];
  if (count === 0) return {value: destination, flags: incoming, count, carry: incoming & 1};
  let carry;
  for (let step = 0; step < count; step++) {
    if (kind === 'shld') {carry = result.shift(); result.push(sourceBits[step]);}
    else {assert.equal(kind, 'shrd'); carry = result.pop(); result.unshift(sourceBits[31 - step]);}
  }
  const value = Number.parseInt(result.join(''), 2), parity = result.slice(24).reduce((sum, bit) => sum + bit, 0) % 2 === 0;
  const flags = 2 + (incoming & 0x400) + carry + Number(parity) * 4 + Number(result.every(bit => bit === 0)) * 0x40
    + result[0] * 0x80 + (count === 1 ? (original[0] ^ result[0]) * 0x800 : 0);
  if (destination === source) {
    const rotation = kind === 'shld' ? original.slice(count).concat(original.slice(0, count))
      : original.slice(32 - count).concat(original.slice(0, 32 - count));
    assert.deepEqual(result, rotation, 'self-source rotation result, with double-shift flags');
  }
  return {value, flags, count, carry};
}
for (const anchor of ANCHORS) for (const incoming of FLAGS) {
  const result = doubleShift(anchor.kind, anchor.destination, anchor.source, 1, incoming);
  assert.equal(result.value, anchor.value, 'literal double-shift result'); assert.equal(result.flags, anchor.flags + (incoming & 0x400), 'literal double-shift FLAGS');
}
function lowByte(parent, value) {const hex = parent.toString(16).padStart(8, '0'); return Number.parseInt(hex.slice(0, 6) + value.toString(16).padStart(2, '0'), 16);}
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
function bank(name, forms, descriptors, packed, instructions, canary = false) {
  const bytes = Buffer.alloc(130, 0xcc), scans = [], specs = []; let offset = 0;
  for (const [index, form] of forms.entries()) {
    const tail = [0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, ...(canary ? [0xbb, 0xef, 0xbe, 0xad, 0xde] : [])];
    const length = form.length + tail.length + 2, entry = PC + offset;
    bytes.set([...form, ...tail, 0xeb, 128 - offset - length], offset);
    specs.push([entry, length]); scans.push({...descriptors[index], entry, target: entry + (canary ? 5 : 0), next: entry + form.length, canary}); offset += length;
  }
  assert.equal(offset, packed); assert.ok(specs.length <= 8 && instructions <= 64); bytes.set([0x0f, 0x0b], 128);
  assert.deepEqual(readFileSync(join(output, `${name}.x86`)), bytes, 'independent Rust/JS bank bytes');
  return {name, bytes, scans, specs, instructions, packed};
}
const banks = [], normal = new Map(), immediateEdges = new Map();
for (const encoding of ENCODINGS) for (let source = 0; source < 8; source++) {
  const name = `normal-${encoding.kind}-${encoding.mode}-s${source}`, forms = [], scans = [];
  for (let destination = 0; destination < 8; destination++) {
    forms.push([0x0f, encoding.opcode, 0xc0 + source * 8 + destination, ...(encoding.mode === 'imm' ? [1] : [])]);
    scans.push({...encoding, source, destination, raw: encoding.mode === 'imm' ? 1 : undefined});
  }
  const selected = bank(name, forms, scans, encoding.mode === 'imm' ? 96 : 88, 32); banks.push(selected); normal.set(name, selected);
}
for (const encoding of ENCODINGS.filter(row => row.mode === 'imm')) {
  const selected = bank(`edge-${encoding.kind}-imm`, EDGE_COUNTS.map(raw => [0x0f, encoding.opcode, 0xfe, raw]),
    EDGE_COUNTS.map(raw => ({...encoding, source: 7, destination: 6, raw})), 96, 32);
  banks.push(selected); immediateEdges.set(encoding.kind, selected);
}
const clDescriptors = ENCODINGS.filter(row => row.mode === 'cl').flatMap(encoding =>
  [{...encoding, source: 1, destination: 3}, {...encoding, source: 7, destination: 1}]);
const clEdges = bank('edge-cl', clDescriptors.map(row => [0x0f, row.opcode, 0xc0 + row.source * 8 + row.destination]), clDescriptors, 44, 16);
banks.push(clEdges);
const currencyBank = bank('currency', [[0xbe, 0x78, 0x56, 0x34, 0x12, 0x0f, 0xac, 0xfe, 1]],
  [{kind: 'shrd', mode: 'imm', opcode: 0xac, source: 7, destination: 6, raw: 1}], 22, 6, true); banks.push(currencyBank);
assert.equal(banks.length, 36);
const formulas = {matrix_targets: OWNERS.length * ENCODINGS.length * 8 * 8 * FLAGS.length,
  immediate_edge_targets: OWNERS.length * 2 * EDGE_COUNTS.length * FLAGS.length,
  cl_edge_targets: OWNERS.length * clDescriptors.length * EDGE_COUNTS.length * FLAGS.length,
  literal_targets: OWNERS.length * ANCHORS.length * 2 * FLAGS.length, currency_targets: OWNERS.length * 3};
const regularTargets = formulas.matrix_targets + formulas.immediate_edge_targets + formulas.cl_edge_targets + formulas.literal_targets;
const targetCount = regularTargets + formulas.currency_targets, pauseCases = OWNERS.length * 2 * 2 * 2 * FLAGS.length;
const regularContexts = OWNERS.length * (32 + 3), plannedContexts = regularContexts + formulas.currency_targets;
const plannedModules = regularContexts + formulas.currency_targets * 2, plannedCalls = regularTargets * 3 + pauseCases * 2 + regularContexts + formulas.currency_targets * 8;
const plannedRetired = regularTargets * 4 + formulas.currency_targets * 6, plannedRaw = targetCount + formulas.currency_targets * 4;
const plannedFiles = 1 + banks.length + plannedModules + 1 + 1 + 1;
assert.deepEqual([regularTargets, targetCount, pauseCases, plannedContexts, plannedModules, plannedCalls, plannedRetired, plannedRaw, plannedFiles], [1296, 1302, 32, 76, 82, 4070, 5220, 1326, 122]);
const counts = {contexts: 0, modules: 0, seeds: 0, targets: 0, matrix_targets: 0, immediate_edge_targets: 0, cl_edge_targets: 0, literal_targets: 0, currency_targets: 0,
  shld_targets: 0, shrd_targets: 0, imm_targets: 0, cl_targets: 0, zero_targets: 0, self_targets: 0, ecx_source_targets: 0, ecx_destination_targets: 0,
  cl_source_count_aliases: 0, cl_destination_count_aliases: 0, cl_triple_aliases: 0, generated_calls: 0, retired: 0, setb: 0, seto: 0, jumps: 0,
  mov_prefixes: 0, canaries: 0, cancel_calls: 0, zero_budget_calls: 0, stale_calls: 0, closed_calls: 0, arena_checks: 0, raw_arenas: 0};
const contexts = [], modules = [], targets = [], runs = [], mutations = [], hostInputs = [], artifacts = [], rawFrames = [], rawRecords = [];
const pairCensus = new Map(), rawCountCensus = new Map();
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
  chapter_separator_policy: 'complete chapters and internal PDF PAGE separators, including separating line31648; excludes successor chapter'};
for (const kind of ['pdf', 'full_text']) {const bytes = readFileSync(join(root, source[`${kind}_path`])); assert.equal(bytes.length, source[`${kind}_bytes`]); assert.equal(hash(bytes), source[`${kind}_sha256`]);}
const fullTextLines = readFileSync(join(root, source.full_text_path), 'utf8').split('\n');
const chapterBytes = (start, end) => Buffer.from(fullTextLines.slice(start - 1, end).join('\n') + '\n');
for (const chapter of source.chapters) {const bytes = chapterBytes(...chapter.lines); assert.equal(bytes.length, chapter.bytes); assert.equal(hash(bytes), chapter.sha256);}
const chapter = chapterBytes(...source.chapter_lines); assert.equal(chapter.length, source.chapter_bytes); assert.equal(hash(chapter), source.chapter_sha256);
source.saved_chapter = artifact('intel-double-shift.txt', chapter);
const sourcePaths = ['engine/tests/cpu_double_shift32.rs', 'engine/tests/cpu_double_shift32_wasm.rs', 'engine/tests/fixtures/p2-double-shift32/run.mjs',
  'engine/tests/fixtures/support/engine.mjs', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer.rs', 'engine/tests/cpu_shift.rs'];
assert.equal(new Set(sourcePaths).size, 9);
const sourceIdentities = () => Object.fromEntries(sourcePaths.map(path => {const bytes = readFileSync(join(root, path)); return [path, {bytes: bytes.length, sha256: hash(bytes)}];}));
const sourcePins = sourceIdentities();

function profile(bytes, owner) {
  let at = 8; assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  const uleb = () => {let value = 0, scale = 1; for (let i = 0; i < 5; i++) {assert.ok(at < bytes.length); const byte = bytes[at++]; value += (byte & 127) * scale;
    if (!(byte & 128)) {assert.ok(value <= 0xffffffff); return value;} scale *= 128;} assert.fail('invalid bounded unsigned LEB');};
  const text = () => {const length = uleb(); assert.ok(at + length <= bytes.length); const value = bytes.subarray(at, at + length).toString('utf8'); at += length; return value;};
  const sections = new Map();
  while (at < bytes.length) {const id = bytes[at++], length = uleb(), start = at; assert.ok(start + length <= bytes.length); assert.ok(!sections.has(id)); sections.set(id, [start, start + length]); at += length;}
  assert.deepEqual([...sections.keys()], [1, 2, 3, 7, 10]);
  const section = id => {at = sections.get(id)[0];}; const ended = id => assert.equal(at, sections.get(id)[1]);
  section(1); const types = []; for (let i = 0, n = uleb(); i < n; i++) {assert.equal(bytes[at++], 0x60); const parameters = [];
    for (let j = 0, n = uleb(); j < n; j++) parameters.push(bytes[at++]); const results = []; for (let j = 0, n = uleb(); j < n; j++) results.push(bytes[at++]); types.push({parameters, results});} ended(1);
  assert.deepEqual(types, [{parameters: Array(4).fill(0x7f), results: [0x7f]}, {parameters: Array(owner === 'replacement' ? 6 : 7).fill(0x7f), results: [0x7f]}]);
  section(2); assert.equal(uleb(), 2); assert.equal(text(), 'env'); assert.equal(text(), 'memory'); assert.equal(bytes[at++], 2); assert.equal(uleb(), 0); assert.equal(uleb(), 1);
  assert.equal(text(), 'ring3'); assert.equal(text(), owner === 'replacement' ? 'guard' : 'guard_resident'); assert.equal(bytes[at++], 0); assert.equal(uleb(), 1); ended(2);
  section(3); assert.equal(uleb(), 1); assert.equal(uleb(), 0); ended(3);
  section(7); assert.equal(uleb(), 1); assert.equal(text(), 'run'); assert.equal(bytes[at++], 0); assert.equal(uleb(), 1); ended(7);
  section(10); assert.equal(uleb(), 1); const bodyLength = uleb(), bodyEnd = at + bodyLength; assert.equal(bodyEnd, sections.get(10)[1]);
  const locals = []; for (let i = 0, n = uleb(); i < n; i++) locals.push([uleb(), bytes[at++]]); assert.deepEqual(locals, [[16, 0x7f], [1, 0x7e]]);
  assert.ok(at < bodyEnd); assert.equal(bytes[bodyEnd - 1], 0x0b); return {types, locals, body_bytes: bodyLength, sections: [...sections.keys()]};
}
function record(magic, version, size, fields) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 1, 56, [...registers, pc, flags]);
const exit = (reason, retired) => record('R3EX', 1, 40, [reason, retired, 0, 0, 0, 0]);
const initialHelper = () => record('R3MH', 1, 40, [0, 0, 0, 0, 0, 0]);
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.ordinal}/${label}: complete live arena`); counts.arena_checks++;}
function snapshot(ctx) {const bytes = arena(ctx); return {state_hex: bytes.subarray(0, 56).toString('hex'), exit_hex: bytes.subarray(56, 96).toString('hex'),
  helper_hex: bytes.subarray(100, 140).toString('hex'), whole_arena_sha256: hash(bytes)};}
function cpu(ctx) {return {registers: Array.from({length: 8}, (_, i) => ctx.expected.readUInt32LE(16 + i * 4)), pc: ctx.expected.readUInt32LE(48), flags: ctx.expected.readUInt32LE(52)};}
function saveArena(ctx, label, unit) {check(ctx, label); const bytes = arena(ctx), row = {context: ctx.ordinal, module: unit.file, label,
  path: 'arena-snapshots.bin', offset: rawFrames.length * SIZE, bytes: SIZE, sha256: hash(bytes)}; rawFrames.push(bytes); rawRecords.push(row); counts.raw_arenas++; return row;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'explicit host request');}
function upload(ctx, address, bytes, purpose) {request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), purpose);
  assert.ok(address >= PC && address + bytes.length <= PC + ctx.code.length); ctx.code.set(bytes, address - PC);
  hostInputs.push({context: ctx.ordinal, purpose, address, bytes: bytes.length, hex: bytes.toString('hex'), sha256: hash(bytes)});}
function fresh(owner, selected, label, writable = false) {
  const instance = new WebAssembly.Instance(engineModule, {}), api = {}, ordinal = ++counts.contexts;
  const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, upload: 2, compile_entries: 2, compile_resident: 1,
    generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
  for (const [name, arity] of Object.entries(arities)) {api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);}
  const ctx = {owner, ordinal, selected, label, api, memory: instance.exports.memory, low: ordinal, high: 0x44534832,
    expected: Buffer.alloc(SIZE), code: Buffer.alloc(selected.bytes.length), units: []};
  assert.ok(ctx.memory instanceof WebAssembly.Memory); assert.equal(api.open(1, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  refresh(ctx); assert.ok(ctx.base > 0 && ctx.base + SIZE <= ctx.bytes.length);
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(exit(1, 0), 56); ctx.expected.set(initialHelper(), 100); check(ctx, 'independently initialized arena');
  pure(ctx, () => api.map(PC, 1, 7), 'one code page'); upload(ctx, PC, selected.bytes, 'authored bank');
  if (!writable) pure(ctx, () => api.protect(PC, 1, 5), 'normal RX code');
  contexts.push({context: ordinal, owner, label, bank: selected.name, key: [ctx.low, ctx.high], base: ctx.base, page_limit: 1, code_permissions: writable ? 7 : 5}); return ctx;
}
const identity = (ctx, unit) => ctx.owner === 'replacement' ? {generation: unit.generation} : {low: unit.low, high: unit.high};
function physical(ctx, unit) {refresh(ctx); assert.ok(unit.pointer + unit.length <= ctx.bytes.length); const bytes = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.deepEqual(bytes, unit.bytes, 'whole physical live module allocation retained'); return {pointer: unit.pointer, bytes: unit.length, sha256: hash(bytes)};}
function compile(ctx, phase = 'initial', tail) {
  const specs = tail ? [[tail.next, 13]] : ctx.selected.specs; request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat())); let binding;
  if (ctx.owner === 'replacement') {pure(ctx, () => ctx.api.compile_entries(specs.length, 0), 'entry compiler'); binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);}
  else {check(ctx, 'before resident compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
    binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, i) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + i * 4, true)]));
    assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident receipt only');
    assert.equal(ctx.api.generation(), 0); assert.equal(ctx.api.module_ptr(), 0); assert.equal(ctx.api.module_len(), 0);}
  check(ctx, 'compiler publication/getter group'); refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
  const bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)); assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes), guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: guard, kind: 'function'}];
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const abi = profile(bytes, ctx.owner), child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: {[guard]: ctx.api[guard]}}); assert.equal(child.exports.run.length, 4);
  const file = `${ctx.owner}-${ctx.ordinal}-${phase}.wasm`, saved = artifact(file, bytes), unit = {...binding, bytes, file, run: child.exports.run}; ctx.units.push(unit); counts.modules++;
  modules.push({context: ctx.ordinal, owner: ctx.owner, bank: ctx.selected.name, phase, specs, instructions: tail ? 4 : ctx.selected.instructions, ...binding,
    identity: identity(ctx, unit), exit_version: 1, imports, exports: [{name: 'run', kind: 'function'}], run_arity: 4, profile: abi, artifact: saved,
    source_spans: specs.map(([entry, length]) => {const bytes = ctx.code.subarray(entry - PC, entry - PC + length); return {entry, bytes: length, hex: bytes.toString('hex'), sha256: hash(bytes), bank_path: `${ctx.selected.name}.x86`, bank_offset: entry - PC};})});
  return unit;
}
function seed(ctx, registers, pc, flags) {assert.ok(FLAGS.includes(flags)); ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96); ctx.expected.fill(0x5a, 100, 140);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 140), ctx.base); check(ctx, 'one independent initial case seed'); counts.seeds++;}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true); check(ctx, 'explicit cancellation field');}
function run(ctx, unit, budget, label, outcome = {}, status = 0, malformed = false) {
  check(ctx, `before ${label}`); const before = snapshot(ctx), current = cpu(ctx), {registers = current.registers, pc = current.pc, flags = current.flags, reason = 1, retired = 0} = outcome;
  if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);}
  assert.equal(unit.run(malformed ? 1 : ctx.base, ctx.base + 56, budget, ctx.base + 96), status, label); check(ctx, label);
  counts.generated_calls++; counts.retired += status ? 0 : retired;
  const row = {ordinal: counts.generated_calls, context: ctx.ordinal, module: unit.file, label, budget, status, malformed_state_pointer: malformed ? 1 : null,
    reason: status ? null : reason, retired: status ? 0 : retired, before, after: snapshot(ctx)}; runs.push(row); return row;
}
function target(ctx, unit, scan, group, label) {
  const original = cpu(ctx), raw = scan.mode === 'imm' ? scan.raw : original.registers[1] % 256;
  assert.equal(original.pc, scan.target); const oldDestination = original.registers[scan.destination], oldSource = original.registers[scan.source];
  const result = doubleShift(scan.kind, oldDestination, oldSource, raw, original.flags), registers = [...original.registers]; registers[scan.destination] = result.value;
  const observed = run(ctx, unit, 1, label, {registers, pc: scan.next, flags: result.flags, retired: 1});
  const rawArena = saveArena(ctx, `${group}: immediate target before AL/DL consumers`, unit);
  counts.targets++; counts[group]++; counts[`${scan.kind}_targets`]++; counts[`${scan.mode}_targets`]++;
  counts.zero_targets += Number(result.count === 0); counts.self_targets += Number(scan.destination === scan.source);
  counts.ecx_source_targets += Number(scan.source === 1); counts.ecx_destination_targets += Number(scan.destination === 1);
  counts.cl_source_count_aliases += Number(scan.mode === 'cl' && scan.source === 1); counts.cl_destination_count_aliases += Number(scan.mode === 'cl' && scan.destination === 1);
  counts.cl_triple_aliases += Number(scan.mode === 'cl' && scan.source === 1 && scan.destination === 1);
  rawCountCensus.set(raw, (rawCountCensus.get(raw) ?? 0) + 1);
  const row = {context: ctx.ordinal, module: unit.file, bank: ctx.selected.name, group, label, kind: scan.kind, mode: scan.mode, opcode: scan.opcode,
    source: scan.source, destination: scan.destination, target_pc: scan.target, next_pc: scan.next, raw_count: raw, masked_count: result.count,
    old_source: oldSource, old_destination: oldDestination, seed_flags: original.flags, result: result.value, flags: result.flags,
    registers_before: original.registers, registers_after: registers, snapshot_phase: 'immediately after target, before SETB AL / SETO DL', observed, raw_arena: rawArena};
  targets.push(row); return row;
}
function consumers(ctx, unit, scan) {
  const current = cpu(ctx), registers = [...current.registers]; registers[0] = lowByte(registers[0], current.flags & 1); registers[2] = lowByte(registers[2], Number(Boolean(current.flags & 0x800)));
  run(ctx, unit, 2, 'SETB AL / SETO DL read live flags without reseed', {registers, pc: scan.next + 6, flags: current.flags, retired: 2}); counts.setb++; counts.seto++;
  if (scan.canary) registers[3] = 0xdeadbeef;
  run(ctx, unit, scan.canary ? 3 : 2, 'terminal JMP reaches unowned cold lookup with spare budget', {registers, pc: COLD, flags: current.flags, retired: scan.canary ? 2 : 1, reason: 3});
  counts.jumps++; counts.canaries += Number(scan.canary);
}
function regularCase(ctx, unit, scan, registers, flags, group, label, pause = false) {
  seed(ctx, registers, scan.entry, flags); const row = target(ctx, unit, scan, group, label);
  if (pause) {cancel(ctx, 1); run(ctx, unit, 4, 'cancel after target before consumers', {reason: 2}); counts.cancel_calls++;
    cancel(ctx, 0); run(ctx, unit, 0, 'zero budget after target before consumers'); counts.zero_budget_calls++;}
  consumers(ctx, unit, scan); return row;
}
function close(ctx, unit) {pure(ctx, () => ctx.api.close(), 'close completed context'); run(ctx, unit, 1, 'retained closed child before malformed state', {}, 5, true); counts.closed_calls++;}
for (const owner of OWNERS) {
  for (const selected of normal.values()) {
    const ctx = fresh(owner, selected, 'complete count1 source/destination pair row'), unit = compile(ctx);
    for (const scan of selected.scans) for (const flags of FLAGS) {
      regularCase(ctx, unit, scan, [...REG], flags, 'matrix_targets', 'all register pairs use old source/destination/count');
      const key = `${owner}/${scan.kind}/${scan.mode}/${flags}/${scan.source}/${scan.destination}`; assert.ok(!pairCensus.has(key)); pairCensus.set(key, 1);
    }
    if (selected.scans[0].source === 6) for (const anchor of ANCHORS.filter(row => row.kind === selected.scans[0].kind)) for (const flags of FLAGS) {
      const registers = [...REG]; registers[1] = 0x80000001; registers[6] = anchor.source;
      const destination = anchor.self ? 6 : 7; registers[destination] = anchor.destination;
      const row = regularCase(ctx, unit, selected.scans[destination], registers, flags, 'literal_targets', 'frozen literal result and double-shift flags');
      assert.equal(row.result, anchor.value); assert.equal(row.flags, anchor.flags + (flags & 0x400)); row.literal_anchor = anchor;
    }
    close(ctx, unit);
  }
  for (const selected of immediateEdges.values()) {
    const ctx = fresh(owner, selected, 'raw immediate masked-count edges'), unit = compile(ctx);
    for (const scan of selected.scans) for (const flags of FLAGS) {
      const row = regularCase(ctx, unit, scan, [...REG], flags, 'immediate_edge_targets', 'raw immediate preserved independently of masked count', [0, 33].includes(scan.raw));
      if (row.masked_count === 0) {assert.notEqual(row.old_source, 0); assert.notEqual((row.old_source | row.old_destination) >>> 0, row.old_destination, 'immediate zero-count source-OR discriminator');}
    }
    close(ctx, unit);
  }
  {
    const ctx = fresh(owner, clEdges, 'old CL as source or destination parent'), unit = compile(ctx);
    for (const scan of clEdges.scans) for (const raw of EDGE_COUNTS) for (const flags of FLAGS) {
      const registers = [...REG]; registers[1] = 0x80000000 + raw; registers[3] = 0x40000100; registers[7] = 0x12345678;
      const row = regularCase(ctx, unit, scan, registers, flags, 'cl_edge_targets', 'old source/count or destination/count alias', scan.destination === 1 && [0, 33].includes(raw));
      assert.equal(row.raw_count, raw); if (row.masked_count === 0) {assert.notEqual(row.old_source, 0); assert.notEqual((row.old_source | row.old_destination) >>> 0, row.old_destination, 'zero-count source-OR discriminator');}
    }
    close(ctx, unit);
  }
}
const mutationCases = [{name: 'consumed-opcode', offset: 6, original: 0xac, changed: 0xa4, replay_kind: 'shld', replay_count: 1, replay_source: 1},
  {name: 'consumed-immediate', offset: 8, original: 1, changed: 2, replay_kind: 'shrd', replay_count: 2, replay_source: 1},
  {name: 'consumed-ModRM', offset: 7, original: 0xfe, changed: 0xde, replay_kind: 'shrd', replay_count: 1, replay_source: 2}];
for (const owner of OWNERS) for (const mutation of mutationCases) {
  const ctx = fresh(owner, currencyBank, mutation.name, true), old = compile(ctx), scan = currencyBank.scans[0], registers = [...REG];
  registers[6] = 0xded00d00; registers[7] = 1; registers[3] = 2; seed(ctx, registers, scan.entry, 0xcd7); registers[6] = 0x12345678;
  const mov = run(ctx, old, 1, 'consumed MOV prefix retires before target', {registers, pc: scan.target, flags: 0xcd7, retired: 1}); counts.mov_prefixes++;
  const movArena = saveArena(ctx, 'currency MOV prefix', old), beforeOld = physical(ctx, old);
  const row = target(ctx, old, scan, 'currency_targets', 'SHRD target commits once before consumed-byte mutation');
  assert.equal(row.result, 0x891a2b3c); assert.equal(row.flags, 0xc86); assert.equal(ctx.code[mutation.offset], mutation.original);
  upload(ctx, PC + mutation.offset, Buffer.from([mutation.changed]), mutation.name); cancel(ctx, 1);
  const staleOld = run(ctx, old, 4, 'stale old child before malformed state and cancellation', {}, 4, true); counts.stale_calls++;
  const afterOld = physical(ctx, old); assert.deepEqual(afterOld, beforeOld, 'old full allocation immediately after stale before publication');
  const oldStaleArena = saveArena(ctx, 'currency old stale', old); cancel(ctx, 0); const committed = snapshot(ctx), current = compile(ctx, 'current-tail', scan);
  assert.notDeepEqual(identity(ctx, current), identity(ctx, old)); const published = snapshot(ctx);
  for (const field of ['state_hex', 'exit_hex', 'helper_hex']) assert.equal(published[field], committed[field], `fresh tail publication preserves ${field}`);
  consumers(ctx, current, scan); assert.equal(cpu(ctx).registers[6], 0x891a2b3c); assert.equal(cpu(ctx).flags, 0xc86);
  assert.equal(cpu(ctx).registers[0], 0x12345600); assert.equal(cpu(ctx).registers[2], 0x34567801); assert.equal(cpu(ctx).registers[3], 0xdeadbeef);
  assert.equal(snapshot(ctx).helper_hex, Buffer.alloc(40, 0x5a).toString('hex')); const tailArena = saveArena(ctx, 'currency completed current tail', current);
  const hypotheticalTarget = doubleShift(mutation.replay_kind, row.result, mutation.replay_source, mutation.replay_count, row.flags);
  const hypotheticalHead = doubleShift(mutation.replay_kind, 0x12345678, mutation.replay_source, mutation.replay_count, 0xcd7);
  assert.notEqual(hypotheticalTarget.value, row.result, 'hypothetical changed target replay changes retained ESI'); assert.notEqual(hypotheticalHead.value, row.result, 'hypothetical changed whole-head replay changes retained ESI');
  const beforeTail = physical(ctx, current); assert.equal(ctx.code[scan.next - PC], 0x0f); upload(ctx, scan.next, Buffer.from([0x0f]), 'same consumed-tail opcode'); cancel(ctx, 1);
  const staleTail = run(ctx, current, 4, 'stale current tail before malformed state and cancellation', {}, 4, true); counts.stale_calls++;
  const afterTail = physical(ctx, current); assert.deepEqual(afterTail, beforeTail, 'fresh tail full allocation immediately after stale before close');
  const tailStaleArena = saveArena(ctx, 'currency current tail stale', current); cancel(ctx, 0); pure(ctx, () => ctx.api.close(), 'close both stale owners');
  for (const unit of [old, current]) {run(ctx, unit, 1, 'closed child before stale/malformed state', {}, 5, true); counts.closed_calls++;}
  mutations.push({context: ctx.ordinal, case: mutation.name, offset: mutation.offset, address: PC + mutation.offset, original: mutation.original, changed: mutation.changed,
    currency_authority: 'successful host upload to a truly consumed byte; raw code memory not physically reread', mov, target: row, old_identity: identity(ctx, old), fresh_identity: identity(ctx, current),
    mov_arena: movArena, stale_old: staleOld, old_stale_arena: oldStaleArena, retained_old_after_stale: afterOld, completed_tail_arena: tailArena,
    stale_tail: staleTail, tail_stale_arena: tailStaleArena, retained_tail_after_stale: afterTail, pure_tail_entry: scan.next, pure_tail_bytes: 13,
    no_replay: {basis: 'hypothetical changed target/head re-execution from explicit operands; no replay executed', retained_esi: row.result, target_replay: hypotheticalTarget, whole_head_replay: hypotheticalHead}});
}
assert.equal(pairCensus.size, formulas.matrix_targets); for (const owner of OWNERS) for (const encoding of ENCODINGS) for (const flags of FLAGS) for (let source = 0; source < 8; source++) for (let destination = 0; destination < 8; destination++)
  assert.equal(pairCensus.get(`${owner}/${encoding.kind}/${encoding.mode}/${flags}/${source}/${destination}`), 1, 'exact pair roster');
for (const [group, planned] of Object.entries(formulas)) assert.equal(counts[group], planned, group);
assert.deepEqual([counts.targets, counts.contexts, counts.modules, counts.seeds, counts.generated_calls, counts.retired, counts.raw_arenas], [targetCount, plannedContexts, plannedModules, targetCount, plannedCalls, plannedRetired, plannedRaw]);
assert.deepEqual([counts.shld_targets, counts.shrd_targets, counts.imm_targets, counts.cl_targets, counts.zero_targets, counts.self_targets], [648, 654, 622, 680, 96, 144]);
assert.deepEqual([counts.ecx_source_targets, counts.ecx_destination_targets, counts.cl_source_count_aliases, counts.cl_destination_count_aliases, counts.cl_triple_aliases], [192, 192, 128, 128, 8]);
assert.deepEqual([counts.setb, counts.seto, counts.jumps, counts.mov_prefixes, counts.canaries, counts.cancel_calls, counts.zero_budget_calls, counts.stale_calls, counts.closed_calls], [targetCount, targetCount, targetCount, 6, 6, pauseCases, pauseCases, 12, plannedModules]);
assert.deepEqual(Object.fromEntries([...rawCountCensus].sort(([a], [b]) => a - b)), {0: 24, 1: 598, 2: 24, 31: 24, 32: 24, 33: 536, 64: 24, 128: 24, 255: 24});
const savedArenas = artifact('arena-snapshots.bin', Buffer.concat(rawFrames)); assert.equal(savedArenas.bytes, plannedRaw * SIZE);
assert.deepEqual(sourceIdentities(), sourcePins, 'source bytes unchanged'); assert.equal(hash(readFileSync(enginePath)), engineSha256, 'actual engine input unchanged');
const result = {status: 'ok', profile: 'finite prefix-free register SHLD/SHRD32 imm8+CL, direct bound replacement/resident execution',
  command: [process.execPath, process.argv[1], enginePath, output, root], environment: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch},
  engine_sha256: engineSha256, engine_bytes: engineBytes.length, caller_engine_sha256: process.env.RING3_ENGINE_SHA256 ?? null, source,
  selected_source_identities: sourcePins, identity_census: {selected_sources: 9, engine: 1, total: 10},
  oracle: {authority: 'independent repeated MSB-first bit-vector steps, frozen literal anchors and self rotation identity; no emitted complementary-shift formulas or engine output input',
    anchors: ANCHORS, register_sentinels: REG, flag_seeds: FLAGS, count_edges: EDGE_COUNTS, accepted_flags: '0xcd7 with mandatory0x2; accepted outside08d5 DF400 and0x2 preserved',
    undefined_flags: 'nonzero AF0 and OF0 for count>1 are declared Ring3 choices, not Intel-defined processor values', mutation_cases: mutationCases,
    checkpoint_policy: 'complete live arena compared around every generated run and explicit upload/map/protect/close; compile request/receipt/getter groups checked at publication and initialization after open/getter group; every immediate target physically saved before AL/DL consumers; selected currency checkpoints physically saved; immediate complete live old/current module allocation comparisons after stale before other operations',
    zero_count_policy: 'all GPRs and entire accepted dirty FLAGS unchanged; instruction still retires once and advances EIP',
    seeding_policy: 'one explicit State/Exit/opaque-helper seed per independent case in a reused bank context; no positive CPU/CL/FLAGS/EIP/helper reseed between target, pause or resumed consumers/current tail',
    register_only_policy: 'one code page, no data page/read/store import/RAM readback/page dump; native owns unbound standalone ABI/locals/export currency'},
  prospective_formulas: {...formulas, regular_targets: regularTargets, pause_cases: pauseCases, contexts: plannedContexts, modules: plannedModules,
    generated_calls: plannedCalls, retired: plannedRetired, raw_arenas: plannedRaw, files: plannedFiles}, counts,
  pair_census: {rows: pairCensus.size, expected: formulas.matrix_targets, each_exactly_once: true}, raw_count_census: Object.fromEntries(rawCountCensus),
  banks: banks.map(selected => ({name: selected.name, bytes: selected.bytes.length, packed: selected.packed, specs: selected.specs, instructions: selected.instructions, scans: selected.scans})),
  contexts, modules, targets, runs, mutations, host_inputs: hostInputs, raw_arena_records: rawRecords, raw_arena_artifact: savedArenas, artifacts,
  pre_execution_finding: {label: 'PLAN-DS-A1', initial_plan_sha256: '1d54db32ab8fe6b77957385f3861c5125a047588b1fa0d410069e1dc9939b382',
    finding: 'exact final instruction budgets would stop for Budget1 before cold lookup', fixed_budgets: {regular_jmp: 2, currency_mov_jmp: 3}, scope: 'plan corrected before first actual execution; corpus/call/retirement counts unchanged'},
  pre_execution_evidence_finding: {label: 'EVID-DS-A2', initial_driver_sha256: '42716d7e4a954f89ed887ead4ecd6a0657730c72b1e73cdd19ac4613824e105c',
    finding: 'whole-arena snapshot hash cannot stay equal across legitimate fresh compile transfer request/receipt changes',
    fix: 'compare state_hex/exit_hex/helper_hex only for committed publication; existing compiler complete arena request/receipt checks retained',
    scope: 'static source finding fixed before first actual execution; no executed fixture failure; corpus/call/retirement counts unchanged'},
  evidence_limits: {raw_frames: 'all immediate targets and four additional frames per currency context physically saved; other complete arenas checked live and represented by metadata/digests',
    temporal: 'live allocation retention ordering, pre/post-call timing, arities and unsaved continuation arenas rely on reviewed driver assertions, not independent replay of saved files',
    hypothetical: 'changed head/target replay discriminators are mathematical counterfactuals; no replay executed', code_memory: 'authored banks and exact mutation inputs saved; managed code memory not physically reread'},
  claim: 'finite actual bound register SHLD/SHRD32: all4x64 source/destination pairs at count1 with FLAGS2/cd7, old ECX/source/count/self aliases, independent literal result/CF/OF/PF/ZF/SF and self-rotation-result/double-flag anchors, raw0/32/64/128 masked-zero exact preservation and raw2/31/33/255 edges, full first-target CPU before live SETB/SETO and same-CPU cancel/budget continuation; consumed opcode/immediate/ModRM upload stale4 with immediate whole live allocation checks, pure13-byte fresh current tail without reseed/replay, same-byte tail invalidation and retained closed5. Physical target/module/program joins and declared live-only limits; deterministic undefined flag policy, intentional offline immutable primary inputs. No atomicity/concurrency/memory/word/x64/prefix/SDK/browser/CI/bootstrap/game/performance claim.'};
assert.equal(artifacts.length + 1, plannedFiles); writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), [...artifacts.map(row => row.path), 'result.json'].sort(), 'exact physical output census');
const resultBytes = readFileSync(join(output, 'result.json')); console.log(JSON.stringify({status: 'ok', engine_sha256: engineSha256, result_sha256: hash(resultBytes), targets: counts.targets,
  contexts: counts.contexts, modules: counts.modules, generated_calls: counts.generated_calls, retired: counts.retired, raw_arenas: counts.raw_arenas, files: plannedFiles, output}));
