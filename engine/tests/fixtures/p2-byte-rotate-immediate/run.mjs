import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected current engine, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const {bytes: engineBytes, module: engineModule, sha256: engineSha256} = readEngine(enginePath);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, MASTER_COLD = PC + 0xf00, LOCAL_COLD = PC + 128;
const OWNERS = ['standalone', 'replacement', 'resident'], KINDS = ['left', 'right'], FLAGS = [2, 0xcd7];
const RAW = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 24, 31, 32, 33, 255];
const REG = [0xa1b2c3d4, 0x12345678, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const ALIASES = [['AL', 0, 1], ['CL', 1, 1], ['DL', 2, 1], ['BL', 3, 1], ['AH', 0, 256], ['CH', 1, 256], ['DH', 2, 256], ['BH', 3, 256]];
function getByte(registers, alias) {const [, parent, factor] = ALIASES[alias]; return Math.floor(registers[parent] / factor) % 256;}
function putByte(registers, alias, value) {
  assert.ok(value >= 0 && value <= 255 && Number.isInteger(value)); const [, parent, factor] = ALIASES[alias];
  const hex = registers[parent].toString(16).padStart(8, '0'), byte = value.toString(16).padStart(2, '0');
  registers[parent] = Number.parseInt(factor === 1 ? hex.slice(0, 6) + byte : hex.slice(0, 4) + byte + hex.slice(6), 16);
}
function rotate(kind, value, raw, incoming) {
  assert.ok(Number.isInteger(value) && value >= 0 && value <= 255 && Number.isInteger(raw) && raw >= 0 && raw <= 255);
  assert.ok((incoming & 2) !== 0 && (incoming & ~0xcd7) === 0); const q = raw % 32, r = q % 8;
  if (q === 0) return {value, flags: incoming, q, r, cf: incoming % 2, of: Math.floor(incoming / 0x800) % 2};
  let bits = value.toString(2).padStart(8, '0');
  for (let step = 0; step < q; step++) bits = kind === 'left' ? bits.slice(1) + bits[0] : bits[7] + bits.slice(0, 7);
  const result = Number.parseInt(bits, 2), cf = Number(bits[kind === 'left' ? 7 : 0]);
  const of = q === 1 ? Number(bits[0] !== (kind === 'left' ? String(cf) : bits[1])) : 0;
  const retained = [2, 4, 0x10, 0x40, 0x80, 0x400].reduce((sum, bit) => sum + Math.floor(incoming / bit) % 2 * bit, 0);
  return {value: result, flags: retained + cf + of * 0x800, q, r, cf, of};
}
const ANCHORS = [
  ['left', 0x81, 1, 2, 3, 0x803], ['left', 0x81, 9, 2, 3, 3],
  ['right', 0x80, 1, 2, 0x40, 0x802], ['right', 0x80, 9, 2, 0x40, 2],
  ['left', 0x80, 8, 0xcd7, 0x80, 0x4d6], ['right', 0x80, 24, 0xcd7, 0x80, 0x4d7],
  ['left', 0x81, 33, 2, 3, 0x803], ['left', 0x81, 32, 0xcd7, 0x81, 0xcd7],
  ['right', 0x80, 31, 2, 1, 2], ['right', 0x81, 255, 2, 3, 2],
  ['left', 0x40, 2, 2, 1, 3], ['right', 2, 2, 2, 0x80, 3],
  ['left', 0, 1, 0xcd7, 0, 0x4d6], ['right', 0, 8, 2, 0, 2],
];
for (const [kind, value, raw, incoming, expected, flags] of ANCHORS) {const result = rotate(kind, value, raw, incoming); assert.equal(result.value, expected); assert.equal(result.flags, flags);}
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4)); return bytes;};
const banks = [], masters = new Map(), currencyBanks = new Map();
function makeBank(name, group, kind = null, currency = null) {
  const bytes = Buffer.alloc(group === 'master' ? 4096 : 130, 0xcc), instructions = [], scans = [], groups = []; let at = 0;
  const append = code => {const pc = PC + at; bytes.set(code, at); instructions.push({pc, length: code.length, next_pc: pc + code.length}); at += code.length; return pc;};
  const tail = canary => {
    append([0x0f, 0x92, 0xc0]); append([0x0f, 0x90, 0xc2]); if (canary) append([0xbb, 0xef, 0xbe, 0xad, 0xde]);
    if (group === 'master') {const jump = Buffer.alloc(5); jump[0] = 0xe9; jump.writeInt32LE(MASTER_COLD - (PC + at + 5), 1); append(jump);}
    else append([0xeb, 128 - at - 2]);
  };
  if (group === 'master') for (let ordinal = 0; ordinal < RAW.length; ordinal++) {
    const specs = [], rows = [], raw = RAW[ordinal];
    for (let alias = 0; alias < 8; alias++) {
      const entry = PC + at, target = append([0xc0, 0xc0 + (kind === 'right' ? 8 : 0) + alias, raw]), next = PC + at;
      tail(false); specs.push([entry, PC + at - entry]); const scan = {kind, alias, raw, ordinal, target, next}; scans.push(scan); rows.push(scan);
    }
    groups.push({ordinal, raw, specs, scans: rows});
  }
  else for (const row of group === 'chain' ? [{kind: 'left', alias: 1, raw: 1, second_raw: 2}, {kind: 'right', alias: 5, raw: 9, second_raw: 2}] : [currency]) {
    const entry = PC + at, target = append([0xc0, 0xc0 + (row.kind === 'right' ? 8 : 0) + row.alias, row.raw]);
    const second = group === 'chain' ? append([0xc0, 0xc0 + (row.kind === 'right' ? 8 : 0) + row.alias, row.second_raw]) : null, next = PC + at;
    tail(true); scans.push({...row, target, second, next}); groups.push({ordinal: groups.length, raw: row.raw, specs: [[entry, PC + at - entry]], scans: [scans.at(-1)]});
  }
  const packed = at, cold = group === 'master' ? MASTER_COLD : LOCAL_COLD;
  assert.equal(packed, group === 'master' ? 1792 : group === 'chain' ? 38 : 16);
  assert.equal(instructions.length, group === 'master' ? 512 : group === 'chain' ? 12 : 5); bytes.set([0x0f, 0x0b], cold - PC);
  assert.deepEqual(bytes, readFileSync(join(output, `${name}.x86`)), 'independent Rust/JS authored bank bytes');
  const bank = {name, kind, group, bytes, packed, cold, groups, scans, instructions, specs: groups.flatMap(row => row.specs)}; banks.push(bank); return bank;
}
for (const kind of KINDS) masters.set(kind, makeBank(`master-${kind}`, 'master', kind));
const chainBank = makeBank('chain', 'chain');
const currencyCases = [{name: 'al-same', kind: 'left', alias: 0, raw: 9, value: 0x81, mutation_offset: 0, original: 0xc0, changed: 0xc0},
  {name: 'cl-immediate', kind: 'right', alias: 1, raw: 9, value: 0x80, mutation_offset: 2, original: 9, changed: 1},
  {name: 'ch-direction', kind: 'left', alias: 5, raw: 9, value: 0x81, mutation_offset: 1, original: 0xc5, changed: 0xcd}];
for (const row of currencyCases) currencyBanks.set(row.name, makeBank(`currency-${row.name}`, 'currency', row.kind, row));
const plan = {regular_per_owner: 8 * 2 * 16 + 2 * 16 * 2 + 8 * 2 * 2 + 2 * 3 * 2 + 2 * 2,
  regular_targets: 368 * 3, chain_pairs: 2 * 2 * 3, chain_targets: 2 * 2 * 3 * 2, currency_targets: 3 * 2,
  contexts: 2 + 2 + 4 + 3 + 6, bound_contexts: 2 + 4 + 2 + 6, modules: 32 * 3 + 3 + 6 * 2,
  pure_modules: 32 + 1, bound_modules: 32 * 2 + 2 + 6 * 2, seeds: 368 * 3 + 12 + 6, regular_contexts: 8 + 3, bound_regular_contexts: 6 + 2,
  managed_page_rosters: 14};
plan.targets = plan.regular_targets + plan.chain_targets + plan.currency_targets;
plan.generated_calls = plan.regular_targets * 2 + plan.chain_pairs * 3 + plan.currency_targets * 6 + plan.regular_contexts * 4 + plan.bound_regular_contexts;
plan.retired = plan.regular_targets * 4 + plan.chain_pairs * 6 + plan.currency_targets * 5;
plan.raw_arenas = plan.targets * 2 + plan.currency_targets * 5;
plan.source_spans = 96 * 32 + 3 * 12 + 6 * 5 + 6 * 4;
plan.host_calls = 14 + 26 + 8 + 78 + 16 + 14; plan.host_requests = 26 + 78;
plan.cancel_inputs = plan.regular_contexts * 2 + plan.bound_regular_contexts + plan.currency_targets * 3;
plan.host_inputs = plan.seeds + plan.host_requests + 26 + plan.cancel_inputs;
plan.arena_checks = plan.contexts + (plan.host_calls - plan.bound_modules) * 2 + plan.bound_modules * 3 + plan.pure_modules
  + plan.host_requests + plan.seeds + plan.cancel_inputs + plan.generated_calls * 2 + plan.raw_arenas;
plan.files = 1 + 6 + plan.modules + 6 + 6 + 1 + 1 + 1;
assert.deepEqual([plan.regular_per_owner, plan.targets, plan.contexts, plan.modules, plan.generated_calls, plan.retired, plan.raw_arenas, plan.files], [368, 1134, 17, 111, 2332, 4518, 2298, 133]);
assert.deepEqual([plan.host_calls, plan.host_inputs, plan.source_spans, plan.arena_checks], [156, 1300, 3162, 8676]);
const counts = {contexts: 0, modules: 0, seeds: 0, targets: 0, regular_targets: 0, chain_targets: 0, currency_targets: 0,
  left_targets: 0, right_targets: 0, zero_targets: 0, one_targets: 0, multi_targets: 0, generated_calls: 0, retired: 0,
  setb: 0, seto: 0, jumps: 0, canaries: 0, cancel_calls: 0, zero_budget_calls: 0, malformed_calls: 0, cold_calls: 0,
  direct_guard_controls: 0, stale_calls: 0, closed_calls: 0, arena_checks: 0, raw_arenas: 0, host_calls: 0};
const contexts = [], modules = [], targets = [], runs = [], mutations = [], hostInputs = [], hostCalls = [], artifacts = [], rawFrames = [], rawRecords = [], chainRows = [];
const caseCensus = new Set(), semanticInputs = new Set(), sourceSpans = [], managedContexts = [];
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
  'engine/tests/cpu_byte_rotate_one.rs', 'engine/tests/cpu_memory_byte_rotate_one.rs', 'engine/tests/cpu_byte_rotate_immediate_one.rs', 'engine/tests/cpu_rotate32.rs',
  'engine/tests/cpu_memory_byte_rotate.rs', 'engine/tests/cpu_byte_rotate_cl.rs', 'engine/tests/fixtures/p2-byte-rotate-immediate-one/run.mjs',
  'engine/tests/cpu_byte_rotate_immediate.rs', 'engine/tests/cpu_byte_rotate_immediate_wasm.rs', 'engine/tests/fixtures/p2-byte-rotate-immediate/run.mjs', 'engine/tests/fixtures/support/engine.mjs'];
assert.equal(new Set(sourcePaths).size, 15);
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
  if (name === 'close' && status === 0) ctx.guestPagesRetired = true;
  counts.host_calls++; hostCalls.push({ordinal: counts.host_calls, context: ctx.ordinal, name, args, status, before, after: snapshot(ctx)});}
function request(ctx, bytes, label) {assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER);
  hostInput(ctx, 'request', label, TRANSFER, bytes); check(ctx, 'explicit host request');}
function upload(ctx, address, bytes, label) {request(ctx, bytes, label); pure(ctx, 'upload', [address, bytes.length]);
  assert.ok(address >= PC && address + bytes.length <= PC + ctx.code.length); ctx.code.set(bytes, address - PC);
  hostInput(ctx, 'upload', label, null, bytes, {address});}
function fresh(owner, selected, label, writable = false) {
  const ordinal = ++counts.contexts, ctx = {owner, ordinal, selected, label, low: ordinal, high: 0x4252494d,
    expected: Buffer.alloc(SIZE), code: Buffer.from(selected.bytes), units: []};
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(exit(1, 0), 56); ctx.expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100);
  if (owner === 'standalone') {ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; refresh(ctx).bytes.set(ctx.expected, ctx.base); check(ctx, 'independent standalone initialized arena');}
  else {
    const instance = new WebAssembly.Instance(engineModule, {}), api = {}; ctx.memory = instance.exports.memory; ctx.api = api;
    const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, upload: 2, compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
    for (const [name, arity] of Object.entries(arities)) {api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);}
    assert.equal(api.open(1, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0; refresh(ctx); assert.ok(ctx.base > 0 && ctx.base + SIZE <= ctx.bytes.length);
    check(ctx, 'independent arena after open/getter group'); pure(ctx, 'map', [PC, 1, 7]); upload(ctx, PC, selected.bytes, 'authored bank'); if (!writable) pure(ctx, 'protect', [PC, 1, 5]);
    ctx.codePermissions = writable ? 7 : 5; managedContexts.push(ctx);
  }
  contexts.push({context: ordinal, owner, bank: selected.name, label, key: owner === 'standalone' ? null : [ctx.low, ctx.high], base: ctx.base,
    page_limit: owner === 'standalone' ? null : 1, code_permissions: owner === 'standalone' ? null : writable ? 7 : 5}); return ctx;
}
function physical(ctx, unit) {refresh(ctx); assert.ok(unit.pointer > 0 && unit.pointer + unit.length <= ctx.bytes.length); const bytes = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.deepEqual(bytes, unit.bytes, 'complete physical live module allocation retained'); return {pointer: unit.pointer, bytes: unit.length, sha256: hash(bytes)};}
function compile(ctx, phase, specs = ctx.selected.specs, scan = null) {
  let binding = {}, bytes, file;
  assert.ok(specs.length > 0 && specs.length <= 8);
  if (ctx.owner === 'standalone') {file = `standalone-${ctx.selected.name}${ctx.selected.group === 'master' ? `-${phase}` : ''}.wasm`; bytes = readFileSync(join(output, file));}
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
  assert.ok(ctx.owner !== 'resident' || ctx.units.length <= 8, 'resident unit budget is per fresh engine context');
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
function target(ctx, unit, scan, pc, group, caseKey) {
  const original = cpu(ctx), offset = pc - PC, encoded = ctx.code.subarray(offset, offset + 3); assert.equal(original.pc, pc); assert.equal(encoded[0], 0xc0);
  assert.equal(encoded[1], 0xc0 + (scan.kind === 'right' ? 8 : 0) + scan.alias); const raw = encoded[2], oldByte = getByte(original.registers, scan.alias);
  const result = rotate(scan.kind, oldByte, raw, original.flags), registers = [...original.registers]; putByte(registers, scan.alias, result.value);
  const beforeArena = saveArena(ctx, `${caseKey}: before target`, unit), observed = run(ctx, unit, 1, 'complete target before any consumer', {registers, pc: pc + 3, flags: result.flags, retired: 1});
  const afterArena = saveArena(ctx, `${caseKey}: after target before any consumer`, unit); counts.targets++; counts[group]++; counts[`${scan.kind}_targets`]++;
  counts[result.q === 0 ? 'zero_targets' : result.q === 1 ? 'one_targets' : 'multi_targets']++;
  const key = `${ctx.owner}/${caseKey}`; assert.ok(!caseCensus.has(key)); caseCensus.add(key); semanticInputs.add([scan.kind, scan.alias, oldByte, raw, original.flags].join('/'));
  const row = {context: ctx.ordinal, owner: ctx.owner, module: unit.file, bank: ctx.selected.name, group, case_key: caseKey, kind: scan.kind, alias: scan.alias,
    encoded_target_hex: encoded.toString('hex'), raw_count: raw, masked_count: result.q, distance: result.r, old_cl: original.registers[1] % 256, old_byte: oldByte,
    target_pc: pc, next_pc: pc + 3, input_registers: original.registers, input_flags: original.flags, expected: result, registers_after: registers,
    before: observed.before, after: observed.after, run_ordinal: observed.ordinal, before_arena: beforeArena, after_arena: afterArena,
    snapshot_phase: 'complete physical before/after C0 target before SETcc; consecutive chain has no intervening reseed or consumer'};
  targets.push(row); return row;
}
function consumers(ctx, unit, scan, canary = false) {
  const current = cpu(ctx), registers = [...current.registers]; putByte(registers, 0, current.flags % 2); putByte(registers, 2, Math.floor(current.flags / 0x800) % 2);
  if (canary) registers[3] = 0xdeadbeef;
  const row = run(ctx, unit, canary ? 5 : 4, canary ? 'current SETB/SETO/MOV/JMP tail only' : 'SETB/SETO/nearJMP consume current FLAGS',
    {registers, pc: ctx.selected.cold, reason: 3, retired: canary ? 4 : 3});
  counts.setb++; counts.seto++; counts.jumps++; if (canary) counts.canaries++; return row;
}
function initial(alias, value) {const registers = [...REG]; putByte(registers, alias, value); return registers;}
function regular(ctx, unit, scan, value, flags, group) {
  seed(ctx, initial(scan.alias, value), scan.target, flags);
  target(ctx, unit, scan, scan.target, 'regular_targets', `${group}/${scan.kind}/g${scan.ordinal}/a${scan.alias}/f${flags}`); consumers(ctx, unit, scan);
}
function closed(ctx, unit, label) {
  const before = arena(ctx); let retained;
  const observed = run(ctx, unit, 0, label, {}, 5, true, () => {const after = arena(ctx); assert.deepEqual(after, before, 'known arena allocation retained immediately after closed guard');
    retained = {pointer: ctx.base, bytes: SIZE, sha256: hash(after), note: 'retained EngineInstance arena from known pointer; guest page owners/code publication were closed'};});
  counts.closed_calls++; return {run_ordinal: observed.ordinal, retained_arena: retained};
}
function controls(ctx, unit) {
  run(ctx, unit, 0, 'valid current module zero budget'); counts.zero_budget_calls++; cancel(ctx, 1);
  run(ctx, unit, 0, 'pending cancel wins over zero budget', {reason: 2}); counts.cancel_calls++;
  run(ctx, unit, 0, 'invalid State pointer range wins over cancel/budget', {}, 1, true); counts.malformed_calls++; cancel(ctx, 0);
  run(ctx, unit, 1, 'valid current cold EIP returns NeedCode', {reason: 3}); counts.cold_calls++;
  if (ctx.api) {
    for (const role of ['key', 'identity']) {const low = ctx.low ^ Number(role === 'key');
      const name = ctx.owner === 'replacement' ? 'guard' : 'guard_resident', args = ctx.owner === 'replacement' ? [low, ctx.high, unit.generation + Number(role === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96]
        : [low, ctx.high, unit.low ^ Number(role === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96]; pure(ctx, name, args, 3); counts.direct_guard_controls++;}
    pure(ctx, 'close', []); cancel(ctx, 1); ctx.closed = closed(ctx, unit, 'closed current child before pointers/cancel/budget');
  }
}
for (const owner of OWNERS) for (const kind of KINDS) {
  const selected = masters.get(kind), batches = owner === 'resident' ? [[0, 8], [8, 16]] : [[0, 16]];
  for (const [begin, end] of batches) {
    const ctx = fresh(owner, selected, `368-case owner matrix/${kind}/groups${begin}-${end - 1}`); let last;
    for (let ordinal = begin; ordinal < end; ordinal++) {
      const group = selected.groups[ordinal], unit = compile(ctx, `g${ordinal.toString().padStart(2, '0')}`, group.specs); last = unit;
      for (const scan of group.scans) {
        const parity = (ordinal + scan.alias + Number(kind === 'right')) % 2, flags = FLAGS[parity];
        regular(ctx, unit, scan, kind === 'left' ? 0x81 : 0x80, flags, 'basis');
        if (scan.alias === 0) for (const other of FLAGS) regular(ctx, unit, scan, kind === 'left' ? 0x80 : 0x81, other, 'opposite-al');
        if ([0, 1].includes(group.raw)) regular(ctx, unit, scan, kind === 'left' ? 0x81 : 0x80, FLAGS[1 - parity], 'other-flags');
        if (scan.alias === 0 && [0, 1, 8].includes(group.raw)) for (const other of FLAGS) regular(ctx, unit, scan, 0, other, 'zero-al');
        if (scan.alias === 0 && group.raw === 2) for (const other of FLAGS) regular(ctx, unit, scan, kind === 'left' ? 0x40 : 2, other, 'cf1-q2');
      }
    }
    controls(ctx, last);
  }
}
for (const owner of OWNERS) {
  const ctx = fresh(owner, chainBank, 'two consecutive instruction-owned counts'), unit = compile(ctx, 'initial');
  for (const scan of chainBank.scans) for (const flags of FLAGS) {
    const registers = [...REG]; registers[1] = scan.kind === 'left' ? 0x12345681 : 0x13578078;
    seed(ctx, registers, scan.target, flags); const key = `chain/${scan.kind}/a${scan.alias}/f${flags}`;
    const first = target(ctx, unit, scan, scan.target, 'chain_targets', `${key}/first`), second = target(ctx, unit, scan, scan.second, 'chain_targets', `${key}/second`);
    assert.equal(second.before.state_hex, first.after.state_hex); assert.equal(second.before.helper_hex, first.after.helper_hex);
    assert.deepEqual([first.expected.value, second.expected.value], scan.kind === 'left' ? [3, 0x0c] : [0x40, 0x10]);
    assert.deepEqual([first.raw_count, second.raw_count], scan.kind === 'left' ? [1, 2] : [9, 2]);
    const tail = consumers(ctx, unit, scan, true); chainRows.push({context: ctx.ordinal, alias: scan.alias, kind: scan.kind, input_flags: flags,
      first_case: first.case_key, second_case: second.case_key, first_run: first.run_ordinal, second_run: second.run_ordinal, tail_run: tail.ordinal,
      no_intervening_write: 'two consecutive budget1 C0 calls; no consumer/CPU/helper/EIP reseed between them'});
  }
  controls(ctx, unit);
}
for (const owner of ['replacement', 'resident']) for (const mutation of currencyCases) {
  const selected = currencyBanks.get(mutation.name), ctx = fresh(owner, selected, `host-${mutation.name}`, true), old = compile(ctx, 'initial'), scan = selected.scans[0];
  seed(ctx, initial(mutation.alias, mutation.value), scan.target, 0xcd7);
  const row = target(ctx, old, scan, scan.target, 'currency_targets', `currency/${mutation.name}`);
  const beforeOld = physical(ctx, old); assert.equal(ctx.code[mutation.mutation_offset], mutation.original);
  upload(ctx, PC + mutation.mutation_offset, Buffer.from([mutation.changed]), `sole consumed target upload/${mutation.name}`); cancel(ctx, 1); let retainedOld;
  const staleOld = run(ctx, old, 0, 'stale original before invalid pointers/cancel/budget', {}, 4, true,
    () => {retainedOld = physical(ctx, old); assert.deepEqual(retainedOld, beforeOld);}); counts.stale_calls++;
  const oldStaleArena = saveArena(ctx, 'currency after original stale', old); cancel(ctx, 0); const committed = snapshot(ctx);
  ctx.currentCodeArtifact = artifact(`current-${ctx.ordinal}.x86`, ctx.code); ctx.tailArtifact = artifact(`tail-${ctx.ordinal}.x86`, ctx.code.subarray(3, 16));
  const current = compile(ctx, 'current-tail', [[scan.next, 13]], scan), published = snapshot(ctx);
  for (const field of ['state_hex', 'exit_hex', 'helper_hex']) assert.equal(published[field], committed[field], `fresh compile preserves ${field}`);
  if (owner === 'replacement') assert.notEqual(current.generation, old.generation); else assert.notDeepEqual([current.low, current.high], [old.low, old.high]);
  const originalReplay = rotate(mutation.kind, row.expected.value, mutation.raw, row.expected.flags);
  const mutatedOperand = ctx.code[1], currentReplay = rotate((mutatedOperand & 8) ? 'right' : 'left', row.expected.value, ctx.code[2], row.expected.flags);
  assert.notEqual(originalReplay.value, row.expected.value); assert.notEqual(currentReplay.value, row.expected.value);
  assert.equal(currentReplay.value, mutation.name === 'al-same' ? 6 : mutation.name === 'cl-immediate' ? 0x20 : 0x81);
  const tail = consumers(ctx, current, scan, true), completedArena = saveArena(ctx, 'currency after only current tail', current);
  const expectedRegisters = [...row.registers_after]; putByte(expectedRegisters, 0, row.expected.cf); putByte(expectedRegisters, 2, row.expected.of); expectedRegisters[3] = 0xdeadbeef;
  assert.deepEqual(cpu(ctx).registers, expectedRegisters); assert.equal(snapshot(ctx).helper_hex, Buffer.alloc(40, 0x5a).toString('hex'));
  const beforeTail = physical(ctx, current); assert.equal(ctx.code[3], 0x0f); upload(ctx, scan.next, Buffer.from([0x0f]), 'same consumed tail byte'); cancel(ctx, 1); let retainedTail;
  const staleTail = run(ctx, current, 0, 'stale current tail before invalid pointers/cancel/budget', {}, 4, true,
    () => {retainedTail = physical(ctx, current); assert.deepEqual(retainedTail, beforeTail);}); counts.stale_calls++;
  const tailStaleArena = saveArena(ctx, 'currency after current stale', current); pure(ctx, 'close', []); const closedRows = [];
  for (const unit of [old, current]) {const close = closed(ctx, unit, 'closed retained child before stale/pointer/cancel/budget'); close.arena = saveArena(ctx, 'currency after closed child', unit); closedRows.push(close);}
  mutations.push({context: ctx.ordinal, case: mutation.name, original_module: old.file, current_module: current.file, address: PC + mutation.mutation_offset,
    offset: mutation.mutation_offset, original: mutation.original, changed: mutation.changed, causation: 'one same/changing consumed target host upload before first stale; no permission mutation',
    target_run_ordinal: row.run_ordinal, stale_old_run_ordinal: staleOld.ordinal, old_stale_arena: oldStaleArena, retained_old_after_stale: retainedOld,
    current_tail_run_ordinal: tail.ordinal, completed_tail_arena: completedArena, stale_tail_run_ordinal: staleTail.ordinal, tail_stale_arena: tailStaleArena,
    retained_tail_after_stale: retainedTail, closed: closedRows, current_code_artifact: ctx.currentCodeArtifact, tail_artifact: ctx.tailArtifact, tail_entry: scan.next, tail_bytes: 13,
    no_replay: {original_target: originalReplay, current_head_target: currentReplay, authority: 'independent mathematical counterfactuals; no replay executed; consumed three-byte C0 head has no earlier MOV prefix'}});
}
assert.equal(caseCensus.size, plan.targets);
for (const group of ['regular_targets', 'chain_targets', 'currency_targets']) assert.equal(counts[group], plan[group]);
for (const name of ['targets', 'contexts', 'modules', 'seeds', 'generated_calls', 'retired', 'raw_arenas', 'host_calls', 'arena_checks']) assert.equal(counts[name], plan[name], `source-derived ${name}`);
assert.deepEqual([counts.left_targets, counts.right_targets, counts.zero_targets, counts.one_targets, counts.multi_targets], [568, 566, 180, 186, 768]);
assert.deepEqual([counts.setb, counts.seto, counts.jumps, counts.canaries], [1122, 1122, 1122, 18]);
assert.deepEqual([counts.cancel_calls, counts.zero_budget_calls, counts.malformed_calls, counts.cold_calls, counts.direct_guard_controls, counts.stale_calls, counts.closed_calls], [11, 11, 11, 11, 16, 12, 20]);
assert.deepEqual([hostInputs.length, sourceSpans.length], [plan.host_inputs, plan.source_spans]);
const savedArenas = artifact('arena-snapshots.bin', Buffer.concat(rawFrames)); assert.equal(savedArenas.bytes, plan.raw_arenas * SIZE);
const sourcePinsAfter = sourceIdentities(); assert.deepEqual(sourcePinsAfter, sourcePins); assert.equal(hash(readFileSync(enginePath)), engineSha256);
const ownerCounts = Object.fromEntries(OWNERS.map(owner => [owner, targets.filter(row => row.owner === owner).length])); assert.deepEqual(ownerCounts, {standalone: 376, replacement: 379, resident: 379});
const managedPageRosters = managedContexts.map(ctx => {
  assert.equal(ctx.guestPagesRetired, true, 'all managed guest page owners were closed'); const modeledPage = Buffer.alloc(4096); modeledPage.set(ctx.code);
  return {context: ctx.ordinal, owner: ctx.owner, page_limit: 1, final_state: 'retired-by-successful-close', current_mapped_pages: [],
    retired_pages: [{address: PC, bytes: 4096, permissions_before_close: ctx.codePermissions,
      modeled_image: {bytes: modeledPage.length, hex: modeledPage.toString('hex'), sha256: hash(modeledPage)},
      known_uploaded_extent: ctx.code.length, upload_input_ordinals: hostInputs.filter(row => row.context === ctx.ordinal && row.kind === 'upload').map(row => row.ordinal)}],
    authority: 'model from successful zero-initialized map plus exact successful host uploads; no physical guest-RAM readback; retained EngineInstance arena is separate'};
});
assert.equal(managedPageRosters.length, plan.managed_page_rosters);
const result = {status: 'ok', profile: 'finite C0 register-byte immediate rotates: native-Rust standalone Node plus actual-engine-Wasm replacement/resident',
  command: [process.execPath, process.argv[1], enginePath, output, root], environment: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch},
  engine: {path: enginePath, bytes: engineBytes.length, sha256: engineSha256, caller_sha256: process.env.RING3_ENGINE_SHA256 ?? null}, source,
  source_pins: sourcePins, source_pins_after: sourcePinsAfter, selected_input_census: {repo_paths: 15, engine: 1, total: 16}, plan, counts,
  corpus: {regular_cases_per_owner: 368, owner_target_counts: ownerCounts, pure_native_node_targets: 376, actual_engine_compiled_bound_targets: 758,
    case_keys: caseCensus.size, semantic_input_keys: semanticInputs.size, semantic_key: 'direction/alias/oldbyte/rawImmediate/FLAGS, owner omitted; purpose-group scalar repetitions allowed',
    raw_counts: RAW, basis_bytes: {left: 0x81, right: 0x80}, opposite_al: {left: 0x80, right: 0x81}, flags: FLAGS, old_cl_sentinel: 120,
    registers: REG, aliases: ALIASES, literal_anchors: ANCHORS},
  banks: banks.map(({name, kind, group, bytes, packed, cold, instructions, specs, scans, groups, artifact}) => ({name, kind, group, bytes: bytes.length, packed, cold, instructions, specs, scans, groups, artifact})),
  contexts, managed_page_rosters: managedPageRosters, modules, targets, runs, chain_pairs: chainRows, mutations, host_inputs: hostInputs, host_calls: hostCalls,
  raw_arena_records: rawRecords, raw_arena_artifact: savedArenas, artifacts,
  evidence_limits: {oracle: 'eight-character repeated bit sequence, literal anchors and hexadecimal parent-byte replacement; independent checker uses integer division/remainder and physical opcode decode',
    owners: '33 standalone modules are native Rust wrapper output executed by Node in three independent memory arenas;758 managed targets are compiled by frozen engine Wasm',
    flags: 'FLAGS2/cd7 finite samples only; no128FLAGSdomain claim; q0 preserves all accepted bits; OF0 above1 is selected undefined-flag policy',
    chains: 'two consecutive budget1 C0 targets with separate complete frames; consumers only after second; second instruction immediate is independently decoded',
    raw_frames: 'all1134 target before/after full4236B plus30 currency frames saved; other run/control states are metadata and live assertions',
    temporal: 'stale full live module callbacks and closed known retained-arena callbacks directly follow status assertion before post-call check/snapshot; saved files do not certify timing',
    code: 'six authored banks/current code/tails and host inputs are physical files;14 managed4096B page rosters/images are map/upload models retired by close, no physical Read32 RAM dump',
    host_schedule: '156 recorded map/upload/protect/compiler/guard/close calls;14 opens and getter groups covered by initialization checks; three pure initializations separate',
    scope: 'no memory/store/exit6/helper/local/cap/FLAGSvalidation expansion, wholeWasmbodycertificate, fullCI/bootstrap/SDK/browser/game/performance claim'},
  pre_execution_findings: [{label: 'SCOUT-UPLOAD-COUNT', note: 'immutable source scout used+20B oldJS increase; accepted contract fixes eight register contexts times five one-byte increases to+40B; no new fixture or executed failure'},
    {label: 'SCOUT-CHAIN-JUMP', note: 'immutable scout used E9 in a chain prose line; accepted local19B chains use shortEB, matrix14B rows use E9 to reach mastercold0xf00; no executed failure'},
    {label: 'CLOSED-ARENA-LEAD', note: 'source-falsified: close retires guest memory and publication but EngineInstance retains arena; compare known4236B allocation without open-pointer getter claim'}]};
assert.equal(artifacts.length + 1, plan.files); writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), [...artifacts.map(row => row.path), 'result.json'].sort(), 'exact physical output census');
const resultBytes = readFileSync(join(output, 'result.json')); console.log(JSON.stringify({status: 'ok', engine_sha256: engineSha256, result_sha256: hash(resultBytes),
  targets: counts.targets, owner_targets: ownerCounts, contexts: counts.contexts, modules: counts.modules, generated_calls: counts.generated_calls, retired: counts.retired,
  raw_arenas: counts.raw_arenas, files: plan.files, output}));
