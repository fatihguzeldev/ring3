import {consumeCpuEvent} from '../../../../../../support/wasm/events.mjs';
import {ARENA_SIZE} from '../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const PC = 0x1000, SEG = 4524, SIZE = ARENA_SIZE, keyHigh = 0x5350414e;
const defaults = [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0];
const offsets = [28, 32, 36, 40, 44, 56], data = Buffer.from([0, 0, 0, 0, 3, 0, 0, 0, ...Array(24).fill(0x5a)]);
const counts = {contexts: 0, modules: 0, runs: 0, faults: 0, successes: 0, controls: 0};
const FP = 4236;
const paths = ['engine/tests/support/wasm/events.mjs', 'engine/tests/support/wasm/arena.mjs', 'engine/src/process/cpu/segments.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/process/cpu/x87.rs', 'engine/src/cpu/dbt/wasm/instructions/x87.rs', 'engine/src/cpu/x86/segments/cache.rs', 'engine/tests/cpu/x87/segment_access/wasm/cached.rs', 'engine/tests/cpu/x87/segment_access/fixtures/p2-x87-segment-access/runtime/run.mjs'];
const pins = () => Object.fromEntries(paths.map(p => [p, createHash('sha256').update(readFileSync(join(root, p))).digest('hex')])), sourcePins = pins();
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, fields, version = 1, size = 40) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8); b.set(words(fields), 16); return b;
}
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const input = (c, at, b) => new Uint8Array(c.memory.buffer).set(b, c.base + at);
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, b) {input(c, 140, b); host(c, 'upload', [address, b.length]);}
function byte(c, address) {host(c, 'read8', [address]); return arena(c).readUInt32LE(120);}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, memory: instance.exports.memory, api: {}, calls: []};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  c.key = ++counts.contexts; host(c, 'open', [12, c.key, keyHigh]); c.base = c.api.arena_ptr() >>> 0;
  for (const page of [PC, 0x5000, 0x8000, 0xf000]) host(c, 'map', [page, 1, 7]);
  c.initial = arena(c); return c;
}
function compile(c, instruction, name, terminal = false) {
  if (c.previous) {input(c, 96, words([0])); host(c, c.stale ? 'retire_stale_resident' : 'discard_unacknowledged_resident', [c.key, keyHigh, ...c.previous]); c.stale = false;}
  const code = Buffer.from(terminal ? instruction : [...instruction, 0xeb, 0]);
  upload(c, PC, code); input(c, 140, words(c.entries ? [PC] : [PC, code.length]));
  const compileName = c.owner === 'resident' ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile';
  assert.equal(c.api[compileName](1, ...(c.entries ? [0] : [])), 0, `${c.owner}/${c.entries}/${name}: compile`);
  const b = arena(c), pointer = c.owner === 'resident' ? b.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  if (c.owner === 'resident') c.previous = [b.readUInt32LE(148), b.readUInt32LE(152)];
  const length = c.owner === 'resident' ? b.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module), ring3 = {...c.api};
  for (const i of imports.filter(i => i.module === 'ring3' && !i.name.startsWith('guard') && i.name !== 'finish_exception_detection')) ring3[i.name] = (...args) => {c.calls.push(i.name); return c.api[i.name](...args);};
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), bytes, {flag: 'wx'});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run,
    memory: imports.some(i => i.module === 'ring3' && !i.name.startsWith('guard') && i.name !== 'finish_exception_detection'),
    cpu: imports.some(i => /^(cpu_|segment_load|stack_execute|simd_execute|x87_op)/.test(i.name)), instructionLength: instruction.length};
}
function setup(c, row) {
  const segments = Buffer.from(c.initial.subarray(SEG, SEG + 64));
  if (row.cache) {segments.writeUInt16LE(2, 4); defaults.forEach((v, i) => segments.writeUInt32LE(v >>> 0, offsets[i]));
    for (const [i, v] of Object.entries(row.cache)) segments.writeUInt32LE(v >>> 0, offsets[+i]);}
  if (row.fsBase !== undefined) segments.writeUInt32LE(row.fsBase >>> 0, 48);
  if (row.nullDs) segments.writeUInt16LE(0, 22);
  input(c, SEG, segments);
  input(c, FP, c.initial.subarray(FP, FP + 128));
  if (row.pending) {const f = arena(c).subarray(FP, FP + 128); f.writeUInt16LE(0x37e, 16); f.writeUInt16LE(1, 18); input(c, FP, f);}
  const state = {r: [0x11223344, 3, 5, 0x5000, 0x8004, 0x5010, 0x5010, 0x5010], pc: PC, flags: row.flags ?? 0x244cd7};
  for (const [i, v] of Object.entries(row.registers ?? {})) state.r[+i] = v >>> 0;
  input(c, 0, record('R3ST', [...state.r, state.pc, state.flags], state.flags & ~0xcd7 ? 2 : 1, 56)); input(c, 96, words([row.cancel ?? 0])); return state;
}
function run(c, fn, row, state) {
  const before = arena(c), expected = Buffer.from(before), at = c.calls.length;
  const reason = row.reason ?? (row.fault ? row.ss ? 15 : 11 : 1), retired = row.retired ?? (row.fault ? 0 : 1);
  const next = {...state, r: [...state.r], pc: retired ? PC + fn.instructionLength : PC};
  expected.set(record('R3ST', [...next.r, next.pc, next.flags], 2, 56));
  expected.set(record('R3EX', [reason, retired, row.mf ? 0x8081 : 0, 0, 0, 0], 6), 56);
  if (row.mf) expected.writeUInt16LE(0x8081, FP + 18);
  if (row.calls !== 0) expected.set(record('R3MH', [row.fault ? row.mf ? 4 : row.ss ? 7 : 3 : 0, row.fault ? 0 : state.flags, row.mf ? 0x8081 : 0, 0, 0, row.width], 7), 100);
  assert.equal(fn.run(c.base, c.base + 56, row.budget ?? 1, c.base + 96), 0, row.name);
  assert.deepEqual(arena(c), expected, `${c.owner}/${c.entries}/${row.name}: complete arena`);
  assert.equal(c.calls.length - at, row.calls ?? 1, row.name);
  consumeCpuEvent(c, SIZE);
  counts.runs++; counts[row.fault ? 'faults' : 'successes']++;
}
const rows = [
  ...[[[0xd9, 0x07], 4], [[0xdd, 0x07], 8], [[0xdb, 0x2f], 10], [[0xd9, 0x2f], 2],
      [[0x66, 0xd9, 0x27], 14], [[0xd9, 0x27], 28], [[0x66, 0xdd, 0x27], 94], [[0xdd, 0x27], 108]]
    .map(([code, width]) => ({name: `read-entire-${width}`, code, width, cache: {3: 0xf2300000 | (0x5010 + width - 2)}, fault: true})),
  ...[[[0xd9, 0x1f], 4], [[0xdd, 0x1f], 8], [[0xdb, 0x3f], 10], [[0xdf, 0x1f], 2],
      [[0x66, 0xd9, 0x37], 14], [[0xd9, 0x37], 28], [[0x66, 0xdd, 0x37], 94], [[0xdd, 0x37], 108]]
    .map(([code, width]) => ({name: `write-authority-${width}`, code, width, cache: {3: 0xf31fffff}, fault: true})),
  {name: 'default-SS-full80', code: [0xdb, 0x6d, 0], width: 10, cache: {2: 0xf2305018}, fault: true, ss: true},
  {name: 'explicit-SS-full32', code: [0x36, 0xd9, 0x07], width: 4, cache: {2: 0xf2305012}, fault: true, ss: true},
  {name: 'execute-only-CS-read', code: [0x2e, 0xd9, 0x07], width: 4, cache: {1: 0xf39fffff}, fault: true},
  {name: 'logical-FS-before-wrapping-base', code: [0x64, 0xdb, 0x2f], width: 10, registers: {7: 0x6010}, fsBase: 0xfffff000, cache: {4: 0xf2306018}, fault: true},
  {name: 'authority-before-unmapped-page', code: [0xd9, 0x1f], width: 4, registers: {7: 0x7000}, cache: {3: 0xf31fffff}, fault: true},
  {name: 'pending-MF-before-segment-authority', code: [0xdb, 0x2f], width: 10, cache: {3: 0xf2300000}, pending: true, mf: true, reason: 12, fault: true},
  {name: 'pending-MF-before-null-selector', code: [0xdb, 0x2f], width: 10, nullDs: true, pending: true, mf: true, reason: 12, fault: true},
  {name: 'nonwaiting-save-still-checks-authority', code: [0xd9, 0x37], width: 28, cache: {3: 0xf31fffff}, pending: true, fault: true},
  {name: 'readonly-load-control-at-exact-end', code: [0xd9, 0x2f], width: 2, cache: {3: 0xf2105011}},
  {name: 'logical-FS-load-control-at-exact-end', code: [0x64, 0xd9, 0x2f], width: 2, registers: {7: 0x6010}, fsBase: 0xfffff000, cache: {4: 0xf2306011}},
  {name: 'budget-precedes-waiting-and-segment', code: [0xdb, 0x2f], width: 10, cache: {3: 0xf2300000}, pending: true, budget: 0, retired: 0, calls: 0},
  {name: 'cancel-precedes-waiting-and-segment', code: [0xdb, 0x2f], width: 10, cache: {3: 0xf2300000}, pending: true, cancel: 1, reason: 2, retired: 0, calls: 0},
];
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const row of rows) {
    upload(c, 0x5010, Buffer.from([0x7f, 3, ...Array(106).fill(0x5a)]));
    const fn = compile(c, row.code, row.name), state = setup(c, row);
    run(c, fn, row, state);
  }
  host(c, 'close');
}
assert.equal(counts.runs, rows.length * 4); assert.equal(counts.contexts, 4);
assert.deepEqual(pins(), sourcePins, 'x87 access sources stayed fixed during proof');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, rows: rows.map(r => r.name), limits: [
  'finite waiting and nonwaiting x87 cached-segment rights, exact spans and MF priority proof; no hardware timing claim',
]}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
