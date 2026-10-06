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
const OWNERS = ['standalone', 'replacement', 'resident'], FLAGS = [2, 0xcd7], KINDS = ['left', 'right'];
const REG = [0x12345678, 0x23456789, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const VALUES = [0x80000001, 0x12345600, 0, 0xffffffff, 0xaaaaaaaa, 0x55555555, 0x80000000, 1];
const EDGES = [0, 32, 64, 128, 33, 65, 129, 255], EDGE_VALUES = [0x80000001, 0x80000001, 0x80000001, 0x80000001, 0, 0, 0x80000001, 0x80000001];
const ALIASES = [0x12345601, 0x89abcd02, 0x1234561f, 0x80000021];
const ANCHORS = [
  ['left', 0x80000001, 31, 2, 0xc0000000, 2], ['right', 0x80000001, 31, 2, 3, 2],
  ['right', 2, 2, 2, 0x80000000, 3], ['left', 0x40000000, 2, 0xcd7, 1, 0x4d7],
  ['left', 0x80000021, 33, 2, 0x43, 0x803], ['right', 0x80000001, 1, 2, 0xc0000000, 3],
  ['left', 0, 1, 2, 0, 2], ['right', 0, 1, 2, 0, 2],
];
function rotate(kind, value, raw, incoming) {
  assert.ok(Number.isInteger(value) && value >= 0 && value <= 0xffffffff);
  assert.ok(Number.isInteger(raw) && raw >= 0 && raw <= 255);
  assert.ok((incoming & 2) !== 0 && (incoming & ~0xcd7) === 0);
  const count = raw % 32;
  if (count === 0) return {value, flags: incoming, count, cf: incoming % 2, of: Math.floor(incoming / 0x800) % 2};
  const d = BigInt(value), power = 2n ** BigInt(count), wrap = 2n ** 32n;
  const wide = kind === 'left' ? d * power % wrap + d / (wrap / power) : d / power + d % power * (wrap / power);
  assert.ok(kind === 'left' || kind === 'right');
  const result = Number(wide), cf = kind === 'left' ? result % 2 : Math.floor(result / 2 ** 31);
  const of = count === 1 ? Number(Math.floor(result / 2 ** 31) !== (kind === 'left' ? cf : Math.floor(result / 2 ** 30) % 2)) : 0;
  const preserved = [2, 4, 0x10, 0x40, 0x80, 0x400].reduce((sum, bit) => sum + Math.floor(incoming / bit) % 2 * bit, 0);
  return {value: result, flags: preserved + cf + of * 0x800, count, cf, of};
}
for (const [kind, value, raw, incoming, expected, flags] of ANCHORS) {
  const result = rotate(kind, value, raw, incoming); assert.equal(result.value, expected); assert.equal(result.flags, flags);
}
for (const kind of KINDS) for (const raw of [0, 32, 64, 128, 224]) assert.deepEqual(rotate(kind, 0x80000001, raw, 0xcd7),
  {value: 0x80000001, flags: 0xcd7, count: 0, cf: 1, of: 1});
function lowByte(parent, value) {return Number.parseInt(parent.toString(16).padStart(8, '0').slice(0, 6) + value.toString(16).padStart(2, '0'), 16);}
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4)); return bytes;};
const banks = [], baseBanks = new Map(), edgeBanks = new Map(), currencyBanks = new Map();
function makeBank(name, kind, destination, group, forms) {
  const bytes = Buffer.alloc(130, 0xcc), specs = [], scans = [], instructions = []; let at = 0;
  const append = code => {const pc = PC + at; bytes.set(code, at); instructions.push({pc, length: code.length, next_pc: pc + code.length}); at += code.length; return pc;};
  if (group === 'base') {
    for (let raw = 0; raw < 32; raw++) {const target = append([0xc1, 0xc0 + (kind === 'right' ? 8 : 0) + destination, raw]);
      scans.push({kind, destination, mode: 'imm', raw, target, next: target + 3});}
    const target = append([0xd3, 0xc0 + (kind === 'right' ? 8 : 0) + destination]); scans.push({kind, destination, mode: 'cl', target, next: target + 2});
    append([0x0f, 0x92, 0xc0]); append([0x0f, 0x90, 0xc2]); append([0xeb, 128 - at - 2]); specs.push([PC, at]); assert.equal(at, 106);
  } else {
    for (const [index, form] of forms.entries()) {
      const entry = PC + at;
      if (group === 'currency') append([0xb8, 0x78, 0x56, 0x34, 0x12]);
      const target = append(form), mode = form[0] === 0xc1 ? 'imm' : 'cl';
      const scan = {kind, destination, mode, raw: mode === 'imm' ? form[2] : undefined, target, next: target + form.length, entry, canary: group === 'currency'};
      append([0x0f, 0x92, 0xc0]); append([0x0f, 0x90, 0xc2]); if (scan.canary) append([0xbb, 0xef, 0xbe, 0xad, 0xde]); append([0xeb, 128 - at - 2]);
      specs.push([entry, PC + at - entry]); scans.push({...scan, index});
    }
    assert.equal(at, group === 'edge' ? 88 : forms[0].length === 3 ? 21 : 20);
  }
  assert.ok(specs.length <= 8 && instructions.length <= 64); bytes.set([0x0f, 0x0b], 128);
  assert.deepEqual(readFileSync(join(output, `${name}.x86`)), bytes, 'independent Rust/JS authored bank bytes');
  const selected = {name, kind, destination, group, bytes, packed: at, instructions, specs, scans}; banks.push(selected); return selected;
}
for (const kind of KINDS) {
  for (let destination = 0; destination < 8; destination++) {const selected = makeBank(`base-${kind}-d${destination}`, kind, destination, 'base'); baseBanks.set(`${kind}/${destination}`, selected);}
  edgeBanks.set(kind, makeBank(`edge-${kind}-imm`, kind, 0, 'edge', EDGES.map(raw => [0xc1, kind === 'left' ? 0xc0 : 0xc8, raw])));
}
for (const name of ['opcode', 'direction', 'immediate']) currencyBanks.set(name,
  makeBank(`currency-${name}`, 'left', 6, 'currency', [name === 'immediate' ? [0xc1, 0xc6, 2] : [0xd3, 0xc6]]));
assert.equal(banks.length, 21);
const plan = {corpus_cases: 1024 + 64 + 16, matrix_targets: 1024 * 3, edge_targets: 64 * 3, alias_targets: 16 * 3, currency_targets: 3 * 2,
  arithmetic_contexts: (16 + 2) * 3, currency_contexts: 3 * 2, arithmetic_modules: (16 + 2) * 3, currency_modules: 3 * 2 * 2,
  pause_cases: 2 * 2 * 2 * 3, compiler_refusals: 2 * 2, valid_controls: (16 + 2) * 3, direct_guard_controls: (16 + 2) * 2 * 2};
plan.targets = plan.matrix_targets + plan.edge_targets + plan.alias_targets + plan.currency_targets;
plan.contexts = plan.arithmetic_contexts + plan.currency_contexts; plan.modules = plan.arithmetic_modules + plan.currency_modules;
plan.consumer_rows = 512 * 3 + plan.edge_targets + plan.alias_targets;
plan.closed_calls = (16 + 2) * 2 + plan.currency_modules;
plan.generated_calls = plan.targets + plan.consumer_rows * 3 + plan.pause_cases * 2 + plan.valid_controls * 3 + plan.compiler_refusals + plan.closed_calls + plan.currency_targets * 4;
plan.retired = plan.targets + plan.consumer_rows * 3 + plan.currency_targets * 5;
plan.raw_arenas = plan.targets * 2 + plan.currency_targets * 7;
plan.files = 1 + banks.length + plan.modules + 1 + plan.currency_targets * 2 + 1 + 1;
plan.bound_contexts = (16 + 2) * 2 + plan.currency_contexts; plan.bound_modules = (16 + 2) * 2 + plan.currency_modules;
plan.cancel_field_inputs = plan.pause_cases * 2 + plan.valid_controls * 2 + (16 + 2) * 2 + plan.currency_targets * 3;
plan.host_requests = plan.bound_contexts + plan.bound_modules + plan.currency_targets * 2 + plan.compiler_refusals;
plan.host_uploads = plan.bound_contexts + plan.currency_targets * 2;
plan.host_inputs = plan.targets + plan.cancel_field_inputs + plan.host_requests + plan.host_uploads;
plan.host_calls = plan.bound_contexts * 2 + (16 + 2) * 2 + plan.bound_modules + plan.currency_targets * 2 + plan.compiler_refusals + plan.direct_guard_controls + plan.bound_contexts;
plan.source_spans = 16 * 3 * 36 + 2 * 3 * 32 + plan.currency_targets * 6 + plan.currency_targets * 4;
plan.arena_checks = plan.contexts + plan.host_requests + plan.host_calls * 2 + plan.modules + plan.targets + plan.cancel_field_inputs + plan.generated_calls * 2 + plan.raw_arenas + plan.compiler_refusals;
assert.deepEqual([plan.targets, plan.contexts, plan.modules, plan.generated_calls, plan.retired, plan.raw_arenas, plan.files], [3318, 60, 66, 8932, 8676, 6678, 103]);
assert.deepEqual([plan.host_inputs, plan.host_calls, plan.source_spans, plan.arena_checks], [3688, 298, 1980, 28902]);
const counts = {contexts: 0, modules: 0, seeds: 0, targets: 0, matrix_targets: 0, edge_targets: 0, alias_targets: 0, currency_targets: 0,
  left_targets: 0, right_targets: 0, imm_targets: 0, cl_targets: 0, zero_targets: 0, one_targets: 0, multi_targets: 0,
  generated_calls: 0, retired: 0, setb: 0, seto: 0, jumps: 0, mov_prefixes: 0, canaries: 0, cancel_calls: 0, zero_budget_calls: 0,
  malformed_calls: 0, compiler_refusals: 0, direct_guard_controls: 0, stale_calls: 0, closed_calls: 0, arena_checks: 0, raw_arenas: 0, host_calls: 0};
const contexts = [], modules = [], targets = [], runs = [], mutations = [], hostInputs = [], hostCalls = [], artifacts = [], rawFrames = [], rawRecords = [];
const caseCensus = new Set(), semanticInputs = new Set(), sourceSpans = [];
function artifact(path, bytes, write = true) {assert.ok(!artifacts.some(row => row.path === path)); if (write) writeFileSync(join(output, path), bytes, {flag: 'wx'});
  const row = {path, bytes: bytes.length, sha256: hash(bytes)}; artifacts.push(row); return row;}
artifact('engine.wasm', engineBytes); for (const selected of banks) selected.artifact = artifact(`${selected.name}.x86`, selected.bytes, false);
const source = {provenance: 'physically pinned offline Intel253667-093US September2026; no fresh edition/download claim',
  prerequisite_policy: 'ignored immutable local PDF/fulltext/extract required before guest; no bootstrap/fullCI claim',
  pdf_path: 'target/r3-rotate-scout/intel-253667-093-vol2b.pdf', pdf_bytes: 6915682, pdf_sha256: 'a261998ace8e07f624bf3e2486bb6cfe950fb8be8d0dc9631bcecb4c5ec953e4',
  full_text_path: 'target/p2-memory-binary-spec/253667-093-sdm-vol-2b.txt', full_text_bytes: 1537522, full_text_sha256: 'f5e6dc689d41655d64792512bfe8add56fc4ac97203aab5c1377a72bac291a35',
  extract_path: 'target/r3-rotate-scout/intel-093-rotate-pages.txt', extract_bytes: 11074, extract_sha256: '7a0e03302580204f99e8283af389f84ec082701807f1a9615a028af4d6f849d0',
  chapter_lines: [27174, 27413], chapter_bytes: 10932, chapter_sha256: 'f502898d90ce6ea7a6a513d1c8761e507af8ad92fac581e1240ea413b78aa74d',
  distinction: 'full_text is entire Vol2B; extract is same edition five pages; saved chapter includes complete rotate chapter, excludes successor RCPPS'};
for (const kind of ['pdf', 'full_text', 'extract']) {const bytes = readFileSync(join(root, source[`${kind}_path`])); assert.equal(bytes.length, source[`${kind}_bytes`]); assert.equal(hash(bytes), source[`${kind}_sha256`]);}
const fullTextLines = readFileSync(join(root, source.full_text_path), 'utf8').split('\n');
const chapter = Buffer.from(fullTextLines.slice(source.chapter_lines[0] - 1, source.chapter_lines[1]).join('\n') + '\n');
assert.equal(chapter.length, source.chapter_bytes); assert.equal(hash(chapter), source.chapter_sha256); source.saved_chapter = artifact('intel-rotate.txt', chapter);
const sourcePaths = ['engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer.rs',
  'engine/tests/cpu_rotate_one.rs', 'engine/tests/cpu_rotate_immediate_one.rs', 'engine/tests/fixtures/p2-rotate-immediate-one/run.mjs',
  'engine/tests/cpu_rotate32.rs', 'engine/tests/cpu_rotate32_wasm.rs', 'engine/tests/fixtures/p2-rotate32/run.mjs', 'engine/tests/fixtures/support/engine.mjs'];
assert.equal(new Set(sourcePaths).size, 11);
const sourceIdentities = () => Object.fromEntries(sourcePaths.map(path => {const bytes = readFileSync(join(root, path)); return [path, {bytes: bytes.length, sha256: hash(bytes)}];}));
const sourcePins = sourceIdentities();
function profile(bytes, owner) {
  let at = 8; assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  const uleb = () => {let value = 0, scale = 1; for (let i = 0; i < 5; i++) {assert.ok(at < bytes.length); const byte = bytes[at++]; value += (byte & 127) * scale;
    if (!(byte & 128)) {assert.ok(value <= 0xffffffff); return value;} scale *= 128;} assert.fail('invalid bounded unsigned LEB');};
  const text = () => {const n = uleb(); assert.ok(at + n <= bytes.length); const value = bytes.subarray(at, at + n).toString('utf8'); at += n; return value;};
  const sections = new Map(); while (at < bytes.length) {const id = bytes[at++], n = uleb(); assert.ok(at + n <= bytes.length && !sections.has(id)); sections.set(id, [at, at + n]); at += n;}
  assert.deepEqual([...sections.keys()], [1, 2, 3, 7, 10]); const section = id => {at = sections.get(id)[0];}, ended = id => assert.equal(at, sections.get(id)[1]);
  section(1); const types = []; for (let i = 0, n = uleb(); i < n; i++) {assert.equal(bytes[at++], 0x60); const parameters = [], results = [];
    for (let j = 0, n = uleb(); j < n; j++) parameters.push(bytes[at++]); for (let j = 0, n = uleb(); j < n; j++) results.push(bytes[at++]); types.push({parameters, results});} ended(1);
  const standalone = owner === 'standalone'; assert.deepEqual(types, [{parameters: Array(4).fill(0x7f), results: [0x7f]},
    ...(standalone ? [] : [{parameters: Array(owner === 'replacement' ? 6 : 7).fill(0x7f), results: [0x7f]}])]);
  section(2); assert.equal(uleb(), standalone ? 1 : 2); assert.equal(text(), 'env'); assert.equal(text(), 'memory'); assert.equal(bytes[at++], 2); assert.equal(uleb(), 0); assert.equal(uleb(), 1);
  if (!standalone) {assert.equal(text(), 'ring3'); assert.equal(text(), owner === 'replacement' ? 'guard' : 'guard_resident'); assert.equal(bytes[at++], 0); assert.equal(uleb(), 1);} ended(2);
  section(3); assert.equal(uleb(), 1); assert.equal(uleb(), 0); ended(3);
  section(7); assert.equal(uleb(), 1); assert.equal(text(), 'run'); assert.equal(bytes[at++], 0); assert.equal(uleb(), standalone ? 0 : 1); ended(7);
  section(10); assert.equal(uleb(), 1); const bodyBytes = uleb(), end = at + bodyBytes; assert.equal(end, sections.get(10)[1]); const locals = [];
  for (let i = 0, n = uleb(); i < n; i++) locals.push([uleb(), bytes[at++]]); assert.deepEqual(locals, [[16, 0x7f], [1, 0x7e]]); assert.ok(at < end); assert.equal(bytes[end - 1], 0x0b);
  return {types, locals, body_bytes: bodyBytes, sections: [...sections.keys()]};
}
function record(magic, size, fields) {const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(1, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4)); return bytes;}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0]);
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.ordinal}/${label}: complete live arena`); counts.arena_checks++;}
function snapshot(ctx) {const bytes = arena(ctx); return {state_hex: bytes.subarray(0, 56).toString('hex'), exit_hex: bytes.subarray(56, 96).toString('hex'),
  helper_hex: bytes.subarray(100, 140).toString('hex'), whole_arena_sha256: hash(bytes)};}
function cpu(ctx) {return {registers: Array.from({length: 8}, (_, i) => ctx.expected.readUInt32LE(16 + i * 4)), pc: ctx.expected.readUInt32LE(48), flags: ctx.expected.readUInt32LE(52)};}
function saveArena(ctx, label, unit) {check(ctx, label); const bytes = arena(ctx), row = {context: ctx.ordinal, module: unit.file, label, path: 'arena-snapshots.bin',
  offset: rawFrames.length * SIZE, bytes: SIZE, sha256: hash(bytes)}; rawFrames.push(bytes); rawRecords.push(row); counts.raw_arenas++; return row;}
function hostInput(ctx, kind, label, offset, bytes, extra = {}) {hostInputs.push({ordinal: hostInputs.length + 1, context: ctx.ordinal, kind, label,
  arena_offset: offset, bytes: bytes.length, hex: bytes.toString('hex'), sha256: hash(bytes), ...extra});}
function pure(ctx, name, args, status = 0) {check(ctx, `before host ${name}`); const before = snapshot(ctx); assert.equal(ctx.api[name](...args), status, name); check(ctx, `host ${name}`);
  counts.host_calls++; hostCalls.push({ordinal: counts.host_calls, context: ctx.ordinal, name, args, status, before, after: snapshot(ctx)});}
function request(ctx, bytes, label) {assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER);
  hostInput(ctx, 'request', label, TRANSFER, bytes); check(ctx, 'explicit host request');}
function upload(ctx, address, bytes, label) {request(ctx, bytes, label); pure(ctx, 'upload', [address, bytes.length]);
  assert.ok(address >= PC && address + bytes.length <= PC + ctx.code.length); ctx.code.set(bytes, address - PC);
  hostInput(ctx, 'upload', label, null, bytes, {address});}
function fresh(owner, selected, label, writable = false) {
  const ordinal = ++counts.contexts, ctx = {owner, ordinal, selected, label, low: ordinal, high: 0x524f5432,
    expected: Buffer.alloc(SIZE), code: Buffer.from(selected.bytes), units: []};
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(exit(1, 0), 56); ctx.expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100);
  if (owner === 'standalone') {ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; refresh(ctx).bytes.set(ctx.expected, ctx.base); check(ctx, 'independent standalone initialized arena');}
  else {
    const instance = new WebAssembly.Instance(engineModule, {}), api = {}; ctx.memory = instance.exports.memory; ctx.api = api;
    const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, upload: 2, compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
    for (const [name, arity] of Object.entries(arities)) {api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);}
    assert.equal(api.open(1, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0; refresh(ctx); assert.ok(ctx.base > 0 && ctx.base + SIZE <= ctx.bytes.length);
    check(ctx, 'independent arena after open/getter group'); pure(ctx, 'map', [PC, 1, 7]); upload(ctx, PC, selected.bytes, 'authored bank'); if (!writable) pure(ctx, 'protect', [PC, 1, 5]);
  }
  contexts.push({context: ordinal, owner, bank: selected.name, label, key: owner === 'standalone' ? null : [ctx.low, ctx.high], base: ctx.base,
    page_limit: owner === 'standalone' ? null : 1, code_permissions: owner === 'standalone' ? null : writable ? 7 : 5}); return ctx;
}
function physical(ctx, unit) {refresh(ctx); assert.ok(unit.pointer > 0 && unit.pointer + unit.length <= ctx.bytes.length); const bytes = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.deepEqual(bytes, unit.bytes, 'complete physical live module allocation retained'); return {pointer: unit.pointer, bytes: unit.length, sha256: hash(bytes)};}
function compile(ctx, phase = 'initial', scan) {
  const specs = scan ? [[scan.next, 13]] : ctx.selected.specs; let binding = {}, bytes, file;
  if (ctx.owner === 'standalone') {assert.equal(phase, 'initial'); file = `standalone-${ctx.selected.name}.wasm`; bytes = readFileSync(join(output, file));}
  else {
    request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat()), `compile ${phase}`); check(ctx, 'before compiler group'); const before = snapshot(ctx);
    const name = ctx.owner === 'replacement' ? 'compile_entries' : 'compile_resident', args = ctx.owner === 'replacement' ? [specs.length, 0] : [specs.length]; assert.equal(ctx.api[name](...args), 0);
    if (ctx.owner === 'replacement') {binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);}
    else {refresh(ctx); binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, i) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + i * 4, true)]));
      assert.ok(binding.low || binding.high); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER);
      assert.equal(ctx.api.generation(), 0); assert.equal(ctx.api.module_ptr(), 0); assert.equal(ctx.api.module_len(), 0);}
    check(ctx, 'compiler receipt/getter group'); counts.host_calls++; hostCalls.push({ordinal: counts.host_calls, context: ctx.ordinal, name, args, status: 0, before, after: snapshot(ctx), binding});
    assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
    bytes = Buffer.from(refresh(ctx).bytes.subarray(binding.pointer, binding.pointer + binding.length)); file = `${ctx.owner}-${ctx.ordinal}-${phase}.wasm`;
  }
  assert.ok(bytes.length > 8 && bytes.length <= 65536 && WebAssembly.validate(bytes)); const module = new WebAssembly.Module(bytes), guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...(ctx.owner === 'standalone' ? [] : [{module: 'ring3', name: guard, kind: 'function'}])];
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]); const abi = profile(bytes, ctx.owner);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.api ? {ring3: {[guard]: ctx.api[guard]}} : {})}); assert.equal(child.exports.run.length, 4);
  const saved = artifact(file, bytes, ctx.owner !== 'standalone'), unit = {...binding, bytes, file, run: child.exports.run}; ctx.units.push(unit); counts.modules++;
  const codeArtifact = scan ? ctx.currentCodeArtifact : ctx.selected.artifact, spans = ctx.selected.instructions.filter(row => specs.some(([entry, length]) => row.pc >= entry && row.pc < entry + length)).map(row => {
    const bytes = ctx.code.subarray(row.pc - PC, row.pc - PC + row.length), span = {...row, hex: bytes.toString('hex'), sha256: hash(bytes), file: codeArtifact.path, offset: row.pc - PC}; sourceSpans.push(span); return span;});
  modules.push({context: ctx.ordinal, owner: ctx.owner, bank: ctx.selected.name, phase, file, ...binding, specs, instructions: spans.length,
    imports, exports: [{name: 'run', kind: 'function'}], run_arity: 4, exit_version: 1, profile: abi, artifact: saved, code_artifact: codeArtifact, tail_artifact: scan ? ctx.tailArtifact : null, source_spans: spans});
  check(ctx, 'module validation/instantiation leaves arena unchanged'); return unit;
}
function seed(ctx, registers, pc, flags) {assert.ok(FLAGS.includes(flags)); ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96); ctx.expected.fill(0x5a, 100, 140);
  const bytes = Buffer.from(ctx.expected.subarray(0, 140)); refresh(ctx).bytes.set(bytes, ctx.base); hostInput(ctx, 'seed', 'independent case seed', 0, bytes); check(ctx, 'one independent case seed'); counts.seeds++;}
function cancel(ctx, value) {const bytes = words([value]); ctx.expected.set(bytes, 96); refresh(ctx).bytes.set(bytes, ctx.base + 96); hostInput(ctx, 'cancel', 'explicit cancellation field', 96, bytes); check(ctx, 'cancellation field only');}
function run(ctx, unit, budget, label, outcome = {}, status = 0, malformed = false, retention) {
  check(ctx, `before ${label}`); const before = snapshot(ctx), current = cpu(ctx), {registers = current.registers, pc = current.pc, flags = current.flags, reason = 1, retired = 0} = outcome;
  if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);}
  const pointers = malformed ? [0xffffffff, ctx.base + 56, budget, ctx.base + 96] : [ctx.base, ctx.base + 56, budget, ctx.base + 96];
  assert.equal(unit.run(...pointers), status, label); if (retention) retention(); check(ctx, label);
  counts.generated_calls++; counts.retired += status ? 0 : retired;
  const row = {ordinal: counts.generated_calls, context: ctx.ordinal, module: unit.file, label, budget, status, pointers, reason: status ? null : reason,
    retired: status ? 0 : retired, before, after: snapshot(ctx)}; runs.push(row); return row;
}
function target(ctx, unit, scan, group, caseKey, alias = false) {
  const original = cpu(ctx), raw = scan.mode === 'imm' ? scan.raw : original.registers[1] % 256; assert.equal(original.pc, scan.target);
  const result = rotate(scan.kind, original.registers[scan.destination], raw, original.flags), registers = [...original.registers]; registers[scan.destination] = result.value;
  const beforeArena = saveArena(ctx, `${caseKey}: before target`, unit), observed = run(ctx, unit, 1, 'first full target before AL/DL consumers', {registers, pc: scan.next, flags: result.flags, retired: 1});
  const afterArena = saveArena(ctx, `${caseKey}: after target before consumers`, unit); counts.targets++; counts[group]++; counts[`${scan.kind}_targets`]++; counts[`${scan.mode}_targets`]++;
  counts[result.count === 0 ? 'zero_targets' : result.count === 1 ? 'one_targets' : 'multi_targets']++;
  const semantic = [scan.kind, scan.mode, scan.destination, original.registers[scan.destination], raw, original.flags].join('/'); semanticInputs.add(semantic);
  const key = `${ctx.owner}/${caseKey}`; assert.ok(!caseCensus.has(key)); caseCensus.add(key);
  const row = {context: ctx.ordinal, module: unit.file, bank: ctx.selected.name, group, case_key: caseKey, kind: scan.kind, mode: scan.mode, destination: scan.destination,
    raw_count: raw, masked_count: result.count, target_pc: scan.target, next_pc: scan.next, input_registers: original.registers, input_flags: original.flags,
    expected: result, registers_after: registers, before: observed.before, after: observed.after, run_ordinal: observed.ordinal, before_arena: beforeArena, after_arena: afterArena,
    snapshot_phase: 'complete target before SETB AL / SETO DL'};
  if (alias) {const late = result.value % 32, lateResult = rotate(scan.kind, original.registers[1], late, original.flags), clOnly = rotate(scan.kind, raw, raw, original.flags);
    assert.notEqual(late, result.count); assert.notEqual(lateResult.value, result.value); assert.notEqual(clOnly.value, result.value);
    row.alias = {old_ecx: original.registers[1], old_cl: raw, late_count: late, late_count_result: lateResult, cl_only_operand_result: clOnly, basis: 'two separate mathematical wrong paths; no wrong-path guest replay'};}
  targets.push(row); return row;
}
function consumers(ctx, unit, scan) {const current = cpu(ctx), registers = [...current.registers]; registers[0] = lowByte(registers[0], current.flags % 2);
  run(ctx, unit, 1, 'SETB AL consumes live CF', {registers, pc: scan.next + 3, retired: 1}); counts.setb++;
  registers[2] = lowByte(registers[2], Math.floor(current.flags / 0x800) % 2); run(ctx, unit, 1, 'SETO DL consumes live OF', {registers, pc: scan.next + 6, retired: 1}); counts.seto++;
  run(ctx, unit, 2, 'JMP with spare budget reaches cold lookup', {registers, pc: COLD, reason: 3, retired: 1}); counts.jumps++;}
function regular(ctx, unit, scan, registers, flags, group, key, consume, pause = false, alias = false) {
  seed(ctx, registers, scan.target, flags);
  if (pause) {cancel(ctx, 1); run(ctx, unit, 0, 'cancel beats zero budget before target', {reason: 2}); counts.cancel_calls++;
    cancel(ctx, 0); run(ctx, unit, 0, 'zero budget alone before same target'); counts.zero_budget_calls++;}
  const row = target(ctx, unit, scan, group, key, alias); if (consume) consumers(ctx, unit, scan); return row;
}
function compilerRefusal(ctx, unit) {
  const metadata = [ctx.api.generation(), ctx.api.module_ptr(), ctx.api.module_len()], retained = physical(ctx, unit);
  request(ctx, words(ctx.owner === 'replacement' ? [COLD] : [COLD, 2]), 'unsupported existing cold UD2 request');
  pure(ctx, ctx.owner === 'replacement' ? 'compile_entries' : 'compile_resident', ctx.owner === 'replacement' ? [1, 0] : [1], 10); counts.compiler_refusals++;
  assert.deepEqual([ctx.api.generation(), ctx.api.module_ptr(), ctx.api.module_len()], metadata); assert.deepEqual(physical(ctx, unit), retained);
  check(ctx, 'failed compiler preserves owner/getter group'); run(ctx, unit, 0, 'prior published child remains live after compiler refusal');
}
function controls(ctx, unit) {
  run(ctx, unit, 0, 'zero budget current module'); counts.zero_budget_calls++; cancel(ctx, 1);
  run(ctx, unit, 0, 'pending cancel wins over zero budget', {reason: 2}); counts.cancel_calls++;
  run(ctx, unit, 0, 'invalid pointer range wins over pending cancel', {}, 1, true); counts.malformed_calls++; cancel(ctx, 0);
  if (ctx.api) {
    for (const role of ['key', 'identity']) {const low = ctx.low ^ Number(role === 'key');
      const name = ctx.owner === 'replacement' ? 'guard' : 'guard_resident', args = ctx.owner === 'replacement' ? [low, ctx.high, unit.generation + Number(role === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96]
        : [low, ctx.high, unit.low ^ Number(role === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96]; pure(ctx, name, args, 3); counts.direct_guard_controls++;}
    pure(ctx, 'close', []); cancel(ctx, 1); run(ctx, unit, 0, 'closed bound child before pointers/cancel/budget', {}, 5, true); counts.closed_calls++;
  }
}
for (const owner of OWNERS) {
  for (const selected of baseBanks.values()) {
    const ctx = fresh(owner, selected, 'finite full masked-count matrix'), unit = compile(ctx), direction = KINDS.indexOf(selected.kind);
    for (const [form, mode] of ['imm', 'cl'].entries()) for (let raw = 0; raw < 32; raw++) {
      const registers = [...REG], destination = selected.destination, flags = (direction + destination + raw + form) % 2 === 0 ? 2 : 0xcd7;
      if (mode === 'cl') registers[1] = destination === 1 ? 0x12345600 + raw : 0xa1b2c300 + raw;
      if (!(mode === 'cl' && destination === 1)) registers[destination] = VALUES[destination];
      const scan = mode === 'imm' ? selected.scans[raw] : selected.scans[32], pause = destination === 6 && [0, 31].includes(raw);
      regular(ctx, unit, scan, registers, flags, 'matrix_targets', `matrix/${selected.kind}/${mode}/d${destination}/raw${raw}`, mode === 'cl', pause);
    }
    if (selected.destination === 0) for (const [index, raw] of EDGES.entries()) for (const flags of FLAGS) {
      const registers = [...REG]; registers[0] = EDGE_VALUES[index]; registers[1] = 0xa1b2c300 + raw;
      regular(ctx, unit, selected.scans[32], registers, flags, 'edge_targets', `edge/${selected.kind}/cl/d0/raw${raw}/f${flags}`, true);
    }
    if (selected.destination === 1) for (const [index, value] of ALIASES.entries()) for (const flags of FLAGS) {
      const registers = [...REG]; registers[1] = value;
      regular(ctx, unit, selected.scans[32], registers, flags, 'alias_targets', `alias/${selected.kind}/cl/d1/case${index}/f${flags}`, true, false, true);
    }
    if (ctx.api && selected.destination === 0) compilerRefusal(ctx, unit); controls(ctx, unit);
  }
  for (const selected of edgeBanks.values()) {
    const ctx = fresh(owner, selected, 'raw immediate boundaries with direct live consumers'), unit = compile(ctx);
    for (const [index, scan] of selected.scans.entries()) for (const flags of FLAGS) {const registers = [...REG]; registers[0] = EDGE_VALUES[index]; registers[1] = 0x556677a5;
      regular(ctx, unit, scan, registers, flags, 'edge_targets', `edge/${selected.kind}/imm/d0/raw${scan.raw}/f${flags}`, true);}
    controls(ctx, unit);
  }
}
const mutationCases = [{name: 'opcode', offset: 5, original: 0xd3, changed: 0xd1, replay_kind: 'left', replay_count: 1, replay_value: 0xc},
  {name: 'direction', offset: 6, original: 0xc6, changed: 0xce, replay_kind: 'right', replay_count: 2, replay_value: 0x80000001},
  {name: 'immediate', offset: 7, original: 2, changed: 34, replay_kind: 'left', replay_count: 34, replay_value: 0x18}];
for (const owner of OWNERS.filter(owner => owner !== 'standalone')) for (const mutation of mutationCases) {
  const selected = currencyBanks.get(mutation.name), ctx = fresh(owner, selected, `consumed-${mutation.name}`, true), old = compile(ctx), scan = selected.scans[0], registers = [...REG];
  registers[1] = 0x12345602; registers[6] = 0x80000001; seed(ctx, registers, PC, 0xcd7);
  const movBefore = saveArena(ctx, 'currency before MOV header', old); registers[0] = 0x12345678;
  const mov = run(ctx, old, 1, 'MOV header retires before target', {registers, pc: scan.target, retired: 1}); counts.mov_prefixes++;
  const movAfter = saveArena(ctx, 'currency after MOV header', old), row = target(ctx, old, scan, 'currency_targets', `currency/${mutation.name}`);
  assert.deepEqual([row.expected.value, row.expected.flags], [6, 0x4d6]); assert.equal(ctx.code[mutation.offset], mutation.original);
  const beforeOld = physical(ctx, old); upload(ctx, PC + mutation.offset, Buffer.from([mutation.changed]), `consumed-${mutation.name}`); cancel(ctx, 1); let retainedOld;
  const staleOld = run(ctx, old, 0, 'stale original before pointer/cancel/budget', {}, 4, true,
    () => {retainedOld = physical(ctx, old); assert.deepEqual(retainedOld, beforeOld);}); counts.stale_calls++;
  const oldStaleArena = saveArena(ctx, 'currency after old stale', old); cancel(ctx, 0); const committed = snapshot(ctx);
  ctx.currentCodeArtifact = artifact(`current-${ctx.ordinal}.x86`, ctx.code); ctx.tailArtifact = artifact(`tail-${ctx.ordinal}.x86`, ctx.code.subarray(scan.next - PC, scan.next - PC + 13));
  const current = compile(ctx, 'current-tail', scan), published = snapshot(ctx);
  for (const field of ['state_hex', 'exit_hex', 'helper_hex']) assert.equal(published[field], committed[field], `fresh compilation preserves ${field}`);
  if (owner === 'replacement') assert.notEqual(current.generation, old.generation); else assert.notDeepEqual([current.low, current.high], [old.low, old.high]);
  const now = cpu(ctx), after = [...now.registers]; after[0] = lowByte(after[0], now.flags % 2); after[2] = lowByte(after[2], Math.floor(now.flags / 0x800) % 2); after[3] = 0xdeadbeef;
  const tail = run(ctx, current, 5, 'only current SETB/SETO/MOV/JMP tail executes', {registers: after, pc: COLD, reason: 3, retired: 4}); counts.setb++; counts.seto++; counts.canaries++; counts.jumps++;
  assert.equal(cpu(ctx).registers[6], 6); assert.equal(cpu(ctx).registers[1], 0x12345602); assert.equal(snapshot(ctx).helper_hex, Buffer.alloc(40, 0x5a).toString('hex'));
  const completedArena = saveArena(ctx, 'currency after current tail', current), replay = rotate(mutation.replay_kind, row.expected.value, mutation.replay_count, row.expected.flags);
  assert.equal(replay.value, mutation.replay_value); assert.notEqual(replay.value, 6);
  const beforeTail = physical(ctx, current); assert.equal(ctx.code[scan.next - PC], 0x0f); upload(ctx, scan.next, Buffer.from([0x0f]), 'same consumed tail byte'); cancel(ctx, 1); let retainedTail;
  const staleTail = run(ctx, current, 0, 'stale current tail before pointer/cancel/budget', {}, 4, true,
    () => {retainedTail = physical(ctx, current); assert.deepEqual(retainedTail, beforeTail);}); counts.stale_calls++;
  const tailStaleArena = saveArena(ctx, 'currency after current stale', current); pure(ctx, 'close', []); const closed = [];
  for (const unit of [old, current]) {const observed = run(ctx, unit, 0, 'closed retained child before stale/pointer/cancel/budget', {}, 5, true); counts.closed_calls++;
    closed.push({run_ordinal: observed.ordinal, arena: saveArena(ctx, 'currency after closed child', unit)});}
  mutations.push({context: ctx.ordinal, case: mutation.name, original_module: old.file, current_module: current.file, address: PC + mutation.offset, offset: mutation.offset,
    original: mutation.original, changed: mutation.changed, mov_run_ordinal: mov.ordinal, target_run_ordinal: row.run_ordinal, mov_before_arena: movBefore, mov_after_arena: movAfter,
    stale_old_run_ordinal: staleOld.ordinal, old_stale_arena: oldStaleArena, retained_old_after_stale: retainedOld, current_tail_run_ordinal: tail.ordinal, completed_tail_arena: completedArena,
    stale_tail_run_ordinal: staleTail.ordinal, tail_stale_arena: tailStaleArena, retained_tail_after_stale: retainedTail, closed, current_code_artifact: ctx.currentCodeArtifact,
    tail_artifact: ctx.tailArtifact, tail_entry: scan.next, tail_bytes: 13, no_replay: {retained_esi: 6, old_ecx: 0x12345602, hypothetical_current_target: replay,
      authority: 'independent numeric counterfactual; no target replay executed; unchanged header MOV alone is not a distinguishing scalar'}});
}
assert.equal(caseCensus.size, plan.targets); for (const group of ['matrix_targets', 'edge_targets', 'alias_targets', 'currency_targets']) assert.equal(counts[group], plan[group]);
assert.deepEqual([counts.targets, counts.contexts, counts.modules, counts.seeds, counts.generated_calls, counts.retired, counts.raw_arenas],
  [plan.targets, plan.contexts, plan.modules, plan.targets, plan.generated_calls, plan.retired, plan.raw_arenas]);
assert.deepEqual([counts.left_targets, counts.right_targets, counts.imm_targets, counts.cl_targets, counts.zero_targets, counts.one_targets, counts.multi_targets], [1662, 1656, 1634, 1684, 192, 192, 2934]);
assert.deepEqual([counts.setb, counts.seto, counts.jumps, counts.mov_prefixes, counts.canaries, counts.cancel_calls, counts.zero_budget_calls, counts.malformed_calls,
  counts.compiler_refusals, counts.direct_guard_controls, counts.stale_calls, counts.closed_calls], [1782, 1782, 1782, 6, 6, 78, 78, 54, 4, 72, 12, 48]);
assert.deepEqual([hostInputs.length, counts.host_calls, sourceSpans.length, counts.arena_checks], [plan.host_inputs, plan.host_calls, plan.source_spans, plan.arena_checks]);
const savedArenas = artifact('arena-snapshots.bin', Buffer.concat(rawFrames)); assert.equal(savedArenas.bytes, plan.raw_arenas * SIZE);
const sourcePinsAfter = sourceIdentities(); assert.deepEqual(sourcePinsAfter, sourcePins, 'source bytes unchanged'); assert.equal(hash(readFileSync(enginePath)), engineSha256, 'engine bytes unchanged');
const result = {status: 'ok', profile: 'finite prefix-free register ROL/ROR32 imm8+CL through standalone/replacement/resident actual modules',
  command: [process.execPath, process.argv[1], enginePath, output, root], environment: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch},
  engine: {path: enginePath, bytes: engineBytes.length, sha256: engineSha256, caller_sha256: process.env.RING3_ENGINE_SHA256 ?? null}, source,
  source_pins: sourcePins, source_pins_after: sourcePinsAfter, selected_input_census: {repo_paths: 11, engine: 1, total: 12}, plan, counts,
  corpus: {declared_owner_free_cases: plan.corpus_cases, executed_arithmetic_rows: 3312, case_keys: caseCensus.size, semantic_input_keys: semanticInputs.size,
    semantic_key: 'direction/form/destination/oldDWORD/rawcount/FLAGS; owner omitted; purpose-group repetitions are allowed and not padded with GPR changes',
    values: VALUES, registers: REG, flags: FLAGS, edge_counts: EDGES, edge_values: EDGE_VALUES, alias_ecx: ALIASES, literal_anchors: ANCHORS},
  banks: banks.map(({name, kind, destination, group, bytes, packed, instructions, specs, scans, artifact}) => ({name, kind, destination, group, bytes: bytes.length, packed, instructions, specs, scans, artifact})),
  contexts, modules, targets, runs, mutations, host_inputs: hostInputs, host_calls: hostCalls, raw_arena_records: rawRecords, raw_arena_artifact: savedArenas, artifacts,
  evidence_limits: {oracle: 'BigInt divmod/multiply and scalar bit extraction; root independent checker uses separate bit sequence/literals, not expected metadata',
    flags: 'masked0 preserves all valid FLAGS; nonzero preserves SF/ZF/AF/PF/DF/fixed0x2; OF0 above1 is Ring3 undefined-flag policy, not processor guarantee',
    consumers: 'all CL, explicit edge and alias rows plus six current tails consume live CF/OF; chained base C1 rows stop immediately after target and next independent case may reseed',
    raw_frames: 'all3318 target before/after arenas plus seven additional selected currency arenas per context physically saved; other arenas checked live and represented by metadata/digests',
    temporal: 'immediate stale allocation retention callbacks precede post-run arena checks/snapshots/getters/save/publication; saved artifacts do not independently replay this live ordering',
    source: 'authored/current x86 bank files and exact successful upload inputs saved; managed code RAM not physically reread',
    host_schedule: 'host_calls counts the298 recorded map/protect/upload/compiler/guard/close actions; initialization open/getter reads are separate reviewed groups; host_inputs explicitly records seed/request/cancel/upload bytes',
    scope: 'finite declared case roster, no full operand/FLAGS Cartesian, fullCI/bootstrap/browser/SDK/game/performance/memory/carry/narrow/x64 claim'},
  pre_execution_findings: [{label: 'ROOT-RR-A1', note: 'initial k31 ROL80000001 CF/FLAGS literal corrected before any model/guest execution; preserved root scalar first pass is separate evidence'},
    {label: 'CONTRACT-RR-A2', note: 'legacy raw native admission classifier directly528/496; new raw register admission not hidden behind word prefixes'},
    {label: 'CONTRACT-RR-A3', note: '1104 means declared purpose-group case identities, not pairwise distinct arithmetic inputs'},
    {label: 'CONTRACT-RR-A4', note: 'invalid pointer/range/overlap raw1 differs from valid-range State header/FLAGS raw2; current raw1 control explicitly passes invalid State pointer'},
    {label: 'PLAN-RR-A6', note: 'provisional call formula8926 missed six current-tail calls; provisional file literal102 missed one declared artifact; source-reviewed before any execution, fixed8932calls/103files without corpus changes',
      initial_driver_sha256: 'f2b2898d5a729f1af7d9afbf6b3e8b45fae73254facb3487c57691d8d45b3d17', preserved_path: 'target/r3-register-rotate32-actual-before-census-fix.mjs'}]};
assert.equal(artifacts.length + 1, plan.files); writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), [...artifacts.map(row => row.path), 'result.json'].sort(), 'exact physical output census');
const resultBytes = readFileSync(join(output, 'result.json')); console.log(JSON.stringify({status: 'ok', engine_sha256: engineSha256, result_sha256: hash(resultBytes),
  targets: counts.targets, contexts: counts.contexts, modules: counts.modules, generated_calls: counts.generated_calls, retired: counts.retired,
  raw_arenas: counts.raw_arenas, files: plan.files, output}));
