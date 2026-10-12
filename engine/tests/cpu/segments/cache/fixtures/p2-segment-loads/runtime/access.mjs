import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const PC = 0x1000, SEG = 4524, SIZE = 4612, keyHigh = 0x5350414e;
const defaults = [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0];
const offsets = [28, 32, 36, 40, 44, 56], data = Buffer.from([0, 0, 0, 0, 3, 0, 0, 0, ...Array(24).fill(0x5a)]);
const counts = {contexts: 0, modules: 0, runs: 0, faults: 0, successes: 0, controls: 0};
const paths = ['engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/segments/cache.rs',
  'engine/src/cpu/dbt/wasm/segments/access.rs', 'engine/src/cpu/dbt/wasm/segments.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/strings.rs',
  'engine/src/cpu/dbt/wasm/simd.rs', 'engine/src/cpu/dbt/wasm/simd/direct.rs',
  'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs',
  'engine/src/cpu/dbt/wasm/atomic.rs', 'engine/tests/cpu/segments/cache/fixtures/p2-segment-loads/runtime/access.mjs'];
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
function word(c, address) {host(c, 'read16', [address]); return arena(c).readUInt32LE(120);}
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
  for (const i of imports.filter(i => i.module === 'ring3' && !i.name.startsWith('guard'))) ring3[i.name] = (...args) => {c.calls.push(i.name); return c.api[i.name](...args);};
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), bytes, {flag: 'wx'});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run,
    memory: imports.some(i => i.module === 'ring3' && !i.name.startsWith('guard')),
    cpu: imports.some(i => /^(cpu_|segment_load|stack_execute|simd_execute|x87_op)/.test(i.name)), instructionLength: instruction.length};
}
function setup(c, row) {
  const segments = Buffer.from(c.initial.subarray(SEG, SEG + 64));
  if (row.cache) {segments.writeUInt16LE(2, 4); defaults.forEach((v, i) => segments.writeUInt32LE(v >>> 0, offsets[i]));
    for (const [i, v] of Object.entries(row.cache)) segments.writeUInt32LE(v >>> 0, offsets[+i]);}
  if (row.fsBase !== undefined) segments.writeUInt32LE(row.fsBase >>> 0, 48);
  if (row.nullDs) segments.writeUInt16LE(0, 22);
  input(c, SEG, segments);
  const state = {r: [0x11223344, 3, 5, 0x5000, 0x8004, 0x5010, 0x5010, 0x5010], pc: PC, flags: row.flags ?? 0x244cd7};
  for (const [i, v] of Object.entries(row.registers ?? {})) state.r[+i] = v >>> 0;
  input(c, 0, record('R3ST', [...state.r, state.pc, state.flags], state.flags & ~0xcd7 ? 2 : 1, 56)); input(c, 96, words([row.cancel ?? 0])); return state;
}
function run(c, fn, row, state) {
  const before = arena(c), expected = Buffer.from(before), at = c.calls.length;
  const reason = row.reason ?? (row.fault ? row.ss ? 15 : 11 : 1), retired = row.retired ?? (row.fault ? 0 : 1);
  const next = {...state, r: [...state.r], pc: row.fault || retired === 0 ? PC : PC + fn.instructionLength};
  if (row.after) row.after(next);
  if (row.work !== undefined) expected.writeUInt32LE(row.work, 4604);
  expected.set(record('R3ST', [...next.r, next.pc, next.flags], next.flags & ~0xcd7 ? 2 : 1, 56));
  expected.set(record('R3EX', [reason, retired, 0, 0, 0, 0], reason === 16 ? 7 : reason === 11 || reason === 15 || fn.cpu ? 6 : row.exitVersion ?? (fn.memory ? 2 : 1)), 56);
  assert.equal(fn.run(c.base, c.base + 56, row.budget ?? 1, c.base + 96), 0, row.name);
  const after = arena(c), observed = c.calls.slice(at);
  if (row.fault || retired === 0 || !fn.memory) assert.equal(observed.length, 0, `${row.name}: authority precedes page helpers`);
  else {
    expected.set(after.subarray(100, 4236), 100);
    if (row.xmm0) expected.set(row.xmm0, 4364 + 32);
    assert.equal(observed.length, row.calls ?? 1, row.name);
  }
  assert.deepEqual(after, expected, `${c.owner}/${c.entries}/${row.name}: complete arena`);
  if (row.memoryByte) assert.equal(byte(c, ...row.memoryByte), row.memoryByte[1], row.name);
  if (row.memoryWord) assert.equal(word(c, row.memoryWord[0]), row.memoryWord[1], row.name);
  if (row.smc) {const saved = arena(c); assert.equal(fn.run(c.base, c.base + 56, 1, c.base + 96), 4); assert.deepEqual(arena(c), saved); c.stale = true; counts.controls++;}
  counts.runs++; counts[row.fault ? 'faults' : 'successes']++;
}

const rows = [
  ...[[[0x88, 0x07], 1], [[0x66, 0x89, 0x07], 2], [[0x89, 0x07], 4]].map(([code, width]) => ({name: `readonly-store-${width}`, code, cache: {3: 0xf31fffff}, fault: true})),
  ...[
    ['AND', 0x21, 2, 0, 0x46], ['OR', 0x09, 2, 3, 6], ['XOR', 0x31, 2, 3, 6],
    ['ADD', 0x01, 2, 3, 6], ['SUB', 0x29, 2, 0xfffd, 0x93],
    ['ADC', 0x11, 3, 4, 2], ['SBB', 0x19, 3, 0xfffc, 0x97],
  ].map(([name, opcode, flags, value, resultFlags]) => ({name: `word-RMW-${name}-retains-checked-address`,
    code: [0x66, opcode, 0x0f], flags, calls: 2, memoryWord: [0x5010, value], after: s => {s.flags = resultFlags;}})),
  {name: 'word-RMW-write-authority-before-read', code: [0x66, 0x09, 0x0f], cache: {3: 0xf31fffff}, fault: true},
  {name: 'word-RMW-entire-span-before-read', code: [0x66, 0x09, 0x0f], cache: {3: 0xf2305010}, fault: true},
  {name: 'word-RMW-exact-end', code: [0x66, 0x09, 0x0f], flags: 2, cache: {3: 0xf2305011}, calls: 2,
    memoryWord: [0x5010, 3], after: s => {s.flags = 6;}},
  ...[[[0x8a, 0x07], 0x500f], [[0x66, 0x8b, 0x07], 0x5010], [[0x8b, 0x07], 0x5012]].map(([code, limit]) => ({name: `read-limit-${code.length}-${limit}`, code, cache: {3: 0xf2300000 | limit}, fault: true})),
  {name: 'default-SS', code: [0x8b, 0x45, 0], cache: {2: 0xf2305012}, fault: true, ss: true},
  {name: 'SIB-base-EBP-selects-SS', code: [0x8b, 0x44, 0x05, 0], registers: {0: 0}, cache: {2: 0xf2305012}, fault: true, ss: true},
  {name: 'SIB-index-EBP-keeps-DS', code: [0x8b, 0x04, 0x2f], registers: {5: 0}, cache: {3: 0xf2305012}, fault: true},
  {name: 'address16-BP-SI-selects-SS', code: [0x67, 0x8b, 0x02], registers: {5: 0x5000, 6: 0x10}, cache: {2: 0xf2305012}, fault: true, ss: true},
  {name: 'DS-override-wins-over-EBP', code: [0x3e, 0x8b, 0x45, 0], cache: {3: 0xf2305012}, fault: true},
  {name: 'FS-checks-logical-before-wrapping-base', code: [0x64, 0x8b, 0x07], registers: {7: 0x6010}, fsBase: 0xfffff000, cache: {4: 0xf2306012}, fault: true},
  {name: 'v1-CS-write', code: [0x2e, 0x89, 0x07], fault: true},
  {name: 'v2-CS-write', code: [0x2e, 0x89, 0x07], cache: {1: defaults[1]}, fault: true},
  {name: 'read-execute-only-CS', code: [0x2e, 0x8b, 0x07], cache: {1: 0xf39fffff}, fault: true},
  {name: 'write-readable-code-through-DS', code: [0x89, 0x07], cache: {3: defaults[1]}, fault: true},
  {name: 'RMW-write-authority-before-unmapped-read', code: [0x83, 0x0f, 0], registers: {7: 0x7000}, cache: {3: 0xf31fffff}, fault: true},
  {name: 'locked-nochange-write-authority', code: [0xf0, 0x83, 0x0f, 0], cache: {3: 0xf31fffff}, fault: true},
  {name: 'CMPXCHG-nochange-write-authority', code: [0x0f, 0xb1, 0x0f], cache: {3: 0xf31fffff}, fault: true},
  {name: 'zero-count-shift-write-authority', code: [0xd3, 0x27], registers: {1: 0}, cache: {3: 0xf31fffff}, fault: true},
  {name: 'BT-adjusted-offset-limit', code: [0x0f, 0xa3, 0x0f], registers: {1: 32}, cache: {3: 0xf2305013}, fault: true},
  {name: 'locked-BTS-adjusted-offset-limit', code: [0xf0, 0x0f, 0xab, 0x0f], registers: {1: 32}, cache: {3: 0xf2305013}, fault: true},
  {name: 'XLAT-selected-byte-limit', code: [0xd7], registers: {0: 0x10}, cache: {3: 0xf230500f}, fault: true},
  ...[[0xf2, 0x0f, 0x38, 0xf1, 0x07], [0xf3, 0x0f, 0xb8, 0x07], [0x0f, 0x38, 0xf0, 0x07]].map((code, i) => ({name: `scalar-family-${i}`, code, cache: {3: 0xf2305012}, fault: true})),
  {name: 'direct-SIMD-full16-limit', code: [0x0f, 0x10, 0x07], cache: {3: 0xf230501e}, fault: true},
  {name: 'helper-SIMD-full16-limit', code: [0x0f, 0x58, 0x07], cache: {3: 0xf230501e}, fault: true},
  {name: 'SIMD-write-readonly', code: [0x0f, 0x11, 0x07], cache: {3: 0xf31fffff}, fault: true},
  {name: 'packed-string-zero-length-still-full16', code: [0x66, 0x0f, 0x3a, 0x61, 0x07, 0], registers: {0: 0, 2: 0}, cache: {3: 0xf230501e}, fault: true},
  {name: 'BOUND-full8-limit', code: [0x62, 0x07], cache: {3: 0xf2305016}, fault: true},
  {name: 'far-pointer-full6-limit', code: [0xc5, 0x07], cache: {3: 0xf2305014}, fault: true},
  {name: 'PUSH-SS-limit', code: [0x50], cache: {2: 0xf2307fff}, fault: true, ss: true},
  {name: 'POP-SS-limit', code: [0x58], registers: {4: 0x8000}, cache: {2: 0xf2308002}, fault: true, ss: true},
  {name: 'RET-SS-limit', terminal: true, code: [0xc3], registers: {4: 0x8000}, cache: {2: 0xf2308002}, fault: true, ss: true},
  {name: 'PUSH-memory-checks-SS-before-source-page', code: [0xff, 0x37], registers: {7: 0x7000}, cache: {2: 0xf2307fff}, fault: true, ss: true},
  {name: 'POP-memory-checks-destination-before-stack-page', code: [0x8f, 0x07], registers: {4: 0x7000}, cache: {3: 0xf31fffff}, fault: true},
  {name: 'CALL-memory-checks-SS-before-target-page', terminal: true, code: [0xff, 0x17], registers: {7: 0x7000}, cache: {2: 0xf2307fff}, fault: true, ss: true},
  {name: 'MOVSD-checks-ES-before-source-page', code: [0xa5], registers: {6: 0x7000}, cache: {0: 0xf31fffff}, fault: true},
  {name: 'MOVSD-readonly-ES', code: [0xa5], cache: {0: 0xf31fffff}, fault: true},
  {name: 'expand-down-lower-end-exclusive', code: [0x8a, 0x07], registers: {7: 0xfff}, cache: {3: 0xf0700fff}, fault: true},
  {name: 'expand-down-B16-full-word-end', code: [0x66, 0x8b, 0x07], registers: {7: 0xffff}, cache: {3: 0xf0700fff}, fault: true},
  {name: 'deterministic-full4GiB-overflow-policy', code: [0x8b, 0x07], registers: {7: 0xffffffff}, fault: true},
  {name: 'CLFLUSH-code-limit', code: [0x2e, 0x0f, 0xae, 0x3f], cache: {1: 0xf290500f}, fault: true},
  {name: 'normal-read-exact-end', code: [0x8b, 0x07], cache: {3: 0xf2305013}, after: s => {s.r[0] = 0;}},
  {name: 'readonly-data-read', code: [0x8b, 0x07], cache: {3: 0xf31fffff}, after: s => {s.r[0] = 0;}},
  {name: 'FS-logical-exact-end', code: [0x64, 0x8b, 0x07], registers: {7: 0x6010}, fsBase: 0xfffff000, cache: {4: 0xf2306013}, after: s => {s.r[0] = 0;}},
  {name: 'expand-down-B16-exact-word-end', code: [0x66, 0x8b, 0x07], registers: {7: 0xfffe}, cache: {3: 0xf0700fff}, after: s => {s.r[0] &= 0xffff0000;}},
  {name: 'SIMD-full16-exact-end', code: [0x0f, 0x10, 0x07], cache: {3: 0xf230501f}, xmm0: data.subarray(0, 16)},
  {name: 'CLFLUSH-execute-only-code', code: [0x2e, 0x0f, 0xae, 0x3f], cache: {1: 0xf39fffff}},
  {name: 'zero-count-REP-skips-null-segments', work: 1, code: [0xf3, 0xa5], registers: {1: 0}, nullDs: true, calls: 0},
  {name: 'LEA-skips-null-segment', code: [0x8d, 0x07], nullDs: true, after: s => {s.r[0] = 0x5010;}},
  {name: 'prefetch-skips-segment-limit', exitVersion: 6, calls: 0, code: [0x0f, 0x18, 0x07], cache: {3: 0xf2300000}},
  {name: 'budget-zero-precedes-access', code: [0x89, 0x07], cache: {3: 0xf31fffff}, budget: 0, retired: 0},
  {name: 'cancel-precedes-access', code: [0x89, 0x07], cache: {3: 0xf31fffff}, cancel: 1, reason: 2, retired: 0},
  {name: 'committed-code-write-retirement', code: [0xc6, 0x07, 0x41], registers: {7: PC + 3}, reason: 6, smc: true, memoryByte: [PC + 3, 0x41]},
  {name: 'committed-code-write-TF-priority', code: [0xc6, 0x07, 0x41], registers: {7: PC + 3}, flags: 0x244dd7, reason: 16, smc: true, memoryByte: [PC + 3, 0x41]},
];
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const row of rows) {
    upload(c, 0x5010, data); const fn = compile(c, row.code, row.name, row.terminal), state = setup(c, row);
    run(c, fn, row, state);
  }
  host(c, 'close');
}
assert.equal(counts.runs, rows.length * 4); assert.equal(counts.contexts, 4);
assert.deepEqual(pins(), sourcePins, 'segment authority sources stayed fixed during proof');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, rows: rows.map(r => r.name), limits: [
  'finite ordinary scalar/SIMD/string/stack authority proof; x87 waiting, compound stack, masked/state-image selected spans and CS execution remain separate integration seams',
  'full4GiB logical overflow is Ring3 deterministic policy permitted by Intel implementation-specific behavior; no hardware timing or throughput claim',
]}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
