import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = 4612;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = ['engine/src/abi/arena.rs', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/profile.rs',
  'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar_tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs',
  'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs', 'engine/src/memory/space.rs',
  'engine/tests/cpu/arithmetic/cases/native/cpu_decimal_adjust.rs', 'engine/tests/cpu/arithmetic/native.rs', 'engine/tests/support/cpu/prefix_admission.rs', 'engine/tests/cpu/arithmetic/native.rs', 'engine/tests/cpu/arithmetic/cases/wasm/cpu_ascii_adjust_wasm.rs', 'engine/tests/cpu/arithmetic/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/arithmetic/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/arithmetic/fixtures/p2-ascii-adjust/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs'];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [], observations = [], contexts = [];
const counts = {contexts: 0, runs: 0, retired: 0, exhaustive: 0, flag_inputs: 0, anchors: 0,
  repeats: 0, current_inputs: 0, controls: 0, retained: 0, consumers: 0, invalidated: 0, helper_calls: 0};
const allFlags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048]
  .reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(allFlags).size, 128); assert.ok(allFlags.every(f => (f & 2) && !(f & ~0xcd7)));
const modulo = (n, size) => (n % size + size) % size;
// independent byte carry/borrow arithmetic models AH separately from the AL correction.
function oracle(kind, ax, f) {
  const al = ax % 256, ah = Math.floor(ax / 256), low = al % 16 > 9 || Math.floor(f / 16) % 2 === 1;
  const direction = kind === 'AAA' ? 1 : -1, corrected = al + direction * 6 * low;
  const nextAh = modulo(ah + direction * low + Math.floor(corrected / 256), 256);
  return {result: nextAh * 256 + modulo(corrected, 16), flags: Math.floor(f / 1024) % 2 * 1024 + 2 + 17 * low};
}
const literals = [
  ['AAA', 0x0000, 2, 0x0000, 2], ['AAA', 0x0000, 0x12, 0x0106, 0x13],
  ['AAA', 0x00fa, 2, 0x0200, 0x13], ['AAA', 0xfffa, 2, 0x0100, 0x13],
  ['AAA', 0x7ffa, 2, 0x8100, 0x13], ['AAA', 0x80fa, 2, 0x8200, 0x13],
  ['AAA', 0x1295, 2, 0x1205, 2], ['AAA', 0x12f9, 3, 0x1209, 2],
  ['AAS', 0x0000, 0x12, 0xfe0a, 0x13], ['AAS', 0x0100, 0x12, 0xff0a, 0x13],
  ['AAS', 0x0006, 0x12, 0xff00, 0x13], ['AAS', 0x0005, 0x12, 0xfe0f, 0x13],
  ['AAS', 0x8000, 0x12, 0x7e0a, 0x13], ['AAS', 0x1200, 0x12, 0x100a, 0x13],
  ['AAS', 0x12f9, 3, 0x1209, 2], ['AAS', 0x12fa, 2, 0x1104, 0x13],
];
for (const [kind, ax, f, result, expectedFlags] of literals) {
  assert.deepEqual(oracle(kind, ax, f + 0xc00), {result, flags: expectedFlags + 0x400}, 'literal fullAX carry/borrow/undefined-flags anchor');
}
const main = [{kind: 'AAA', pc: 0x1000, bytes: Buffer.from([0x37, 0xeb, 0]), instructions: 2},
  {kind: 'AAS', pc: 0x1020, bytes: Buffer.from([0x3f, 0xeb, 0]), instructions: 2}];
const live = [{kind: 'AAA', pc: 0x1400, bytes: Buffer.from([0x00, 0xd8, 0x37, 0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0x80, 0xd1, 0, 0xeb, 0]), instructions: 6},
  {kind: 'AAS', pc: 0x1440, bytes: Buffer.from([0x28, 0xd8, 0x3f, 0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0x80, 0xd1, 0, 0xeb, 0]), instructions: 6}];
function aadOracle(ax, base, f) {
  const result = modulo(ax % 256 + Math.floor(ax / 256) * base, 256);
  const ones = [...result.toString(2)].filter(bit => bit === '1').length;
  return {result, flags: Math.floor(f / 1024) % 2 * 1024 + 2 + 4 * (ones % 2 === 0) + 64 * (result === 0) + 128 * (result >= 128)};
}
const aadLiterals = [[0, 0xff00, 0, 0x446], [0, 0xff80, 0x80, 0x482], [1, 0x0101, 2, 0x402],
  [10, 0x0205, 0x19, 0x402], [10, 0xfffa, 0xf0, 0x486], [255, 0xff01, 2, 0x402], [255, 0x0101, 0, 0x446],
  [128, 0x0101, 0x81, 0x486], [128, 0x8001, 1, 0x402], [0x66, 0x0101, 0x67, 0x402],
  [0x67, 0x0101, 0x68, 0x402], [0xf0, 0x0101, 0xf1, 0x482], [0xf2, 0x0101, 0xf3, 0x486],
  [0xf3, 0x0101, 0xf4, 0x482], [10, 0x0909, 0x63, 0x406], [16, 0x1000, 0, 0x446]];
for (const [base, ax, result, f] of aadLiterals) assert.deepEqual(aadOracle(ax, base, 0xcd7), {result, flags: f}, 'literal AAD original-byte/base0/flags anchor');
const aadBanks = Array.from({length: 5}, (_, index) => {
  const first = index * 63, count = Math.min(63, 256 - first), pc = 0x1800 + index * 256;
  const rows = Array.from({length: count}, (_, i) => ({base: first + i, pc: pc + i * 2}));
  return {name: `aad-${index}`, rows, blocks: [{pc, bytes: Buffer.from([...rows.flatMap(r => [0xd5, r.base]), 0xeb, 0]), instructions: count + 1}]};
});
assert.equal(aadBanks.flatMap(b => b.rows).length, 256);
const aadLive = [[0x1e00, 9, 10, 0x63, 0x406], [0x1e40, 1, 255, 0, 0x446]].map(([pc, byte, base, result, f]) =>
  ({pc, byte, base, result, flags: f, instructions: 9, bytes: Buffer.from([0xb4, byte, 0xb0, byte, 0xd5, base,
    0x0f, 0x94, 0xc2, 0x0f, 0x9a, 0xc6, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0x80, 0xd1, 0, 0xeb, 0])}));
// repeated subtraction derives the quotient/remainder from original AL independently of wasm division.
function aamOracle(ax, base, f) {
  if (base === 0) return null;
  let remainder = ax % 256, quotient = 0;
  while (remainder >= base) {remainder -= base; quotient++;}
  const ones = [...remainder.toString(2)].filter(bit => bit === '1').length;
  return {result: quotient * 256 + remainder, flags: Math.floor(f / 1024) % 2 * 1024 + 2 + 4 * (ones % 2 === 0) + 64 * (remainder === 0) + 128 * (remainder >= 128)};
}
const aamLiterals = [[0, 0, null, null], [0, 0xffff, null, null], [1, 0xffff, 0xff00, 0x446],
  [2, 0x80ff, 0x7f01, 0x402], [3, 0x00ff, 0x5500, 0x446], [10, 0xffff, 0x1905, 0x406],
  [10, 0x8081, 0x0c09, 0x406], [128, 0xffff, 0x017f, 0x402], [128, 0x0080, 0x0100, 0x446],
  [255, 0x80ff, 0x0100, 0x446], [255, 0xff80, 0x0080, 0x482], [0x66, 0x00ff, 0x0233, 0x406],
  [0x67, 0x00ff, 0x0231, 0x402], [0xf0, 0x00ff, 0x010f, 0x406], [0xf2, 0x00ff, 0x010d, 0x402], [0xf3, 0x00ff, 0x010c, 0x406]];
for (const [base, ax, result, f] of aamLiterals) assert.deepEqual(aamOracle(ax, base, 0xcd7), result === null ? null : {result, flags: f}, 'literal original-AL quotient/remainder/flags or base0 divide error');
const aamBanks = aadBanks.map((bank, index) => ({name: `aam-${index}`, rows: bank.rows, blocks: bank.blocks.map(b => {
  const bytes = Buffer.from(b.bytes); for (let i = 0; i < bytes.length - 2; i += 2) bytes[i] = 0xd4;
  return {...b, bytes, arithmetic: true};
})}));
const aamLive = [{pc: 0x1e00, instructions: 8, arithmetic: true, bytes: Buffer.from([0xf6, 0xe3, 0xd4, 10,
  0x0f, 0x94, 0xc2, 0x0f, 0x9a, 0xc6, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0x80, 0xd1, 0, 0xeb, 0])},
  {pc: 0x1e40, instructions: 3, arithmetic: true, bytes: Buffer.from([0x00, 0xd8, 0xd4, 0, 0xeb, 0])}];
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(1, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = s => record('R3ST', 56, [...s.registers, s.pc, s.flags]);
const exit = (reason, retired, version = 1) => {const b = record('R3EX', 40, [reason, retired, 0, 0, 0, 0]); b.writeUInt16LE(version, 4); return b;};
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, bytes) {
  const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'declared input preserves the rest of the full arena');
}
function host(c, name, args) {
  const expected = arena(c); assert.equal(c.api[name](...args), 0, name);
  if (name.startsWith('compile_resident')) {const b = arena(c); assert.equal(b.readUInt32LE(140), 1); assert.equal(b.readUInt32LE(144), 24); expected.set(b.subarray(140, 164), 140);}
  assert.deepEqual(arena(c), expected, 'host full arena effect: ' + name);
}
function upload(c, pc, bytes) {input(c, 140, bytes); host(c, 'upload', [pc, bytes.length]);}
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
const parents = n => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + modulo(0x7e00 + n * (17 + i), 65536));
const setByte = (parent, byte) => Math.floor(parent / 256) * 256 + byte;
const setWord = (parent, word) => Math.floor(parent / 65536) * 65536 + word;
function seed(c, pc, f = 0xcd7, r = parents(0)) {
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags: f})); b.set(exit(3, 0), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function adjusted(s, kind, nextPc) {
  const expected = oracle(kind, s.registers[0] % 65536, s.flags), r = [...s.registers];
  r[0] = setWord(r[0], expected.result); return {registers: r, pc: nextPc, flags: expected.flags};
}
function aadAdjusted(s, base, nextPc) {
  const expected = aadOracle(s.registers[0] % 65536, base, s.flags), r = [...s.registers];
  r[0] = setWord(r[0], expected.result); return {registers: r, pc: nextPc, flags: expected.flags};
}
function run(c, unit, budget, next, label, {reason = 1, retired = 1, status = 0, category = label, base = null} = {}) {
  const before = cpu(c), expected = arena(c);
  if (status === 0) {expected.set(state(next)); expected.set(exit(reason, retired, unit.exitVersion), 56);}
  const observedStatus = unit.run(c.base, c.base + 56, budget, c.base + 96);
  assert.equal(observedStatus, status, label); assert.deepEqual(arena(c), expected, 'full4612/GPR/AX-high16/FLAGS/helper/dirtyFP: ' + label);
  const after = arena(c), accepted = observedStatus === 0;
  observations.push({context: c.id, category, label, input_pc: before.pc, input_ax: before.registers[0] % 65536, input_flags: before.flags,
    expected_status: status, observed_status: observedStatus, expected_reason: status === 0 ? reason : null,
    observed_reason: accepted ? after.readUInt32LE(72) : null, expected_retired: status === 0 ? retired : null,
    observed_retired: accepted ? after.readUInt32LE(76) : null, expected_pc: status === 0 ? next.pc : null,
    observed_pc: accepted ? after.readUInt32LE(48) : null, expected_flags: status === 0 ? next.flags : null,
    observed_flags: accepted ? after.readUInt32LE(52) : null, expected_ax: status === 0 ? next.registers[0] % 65536 : null,
    observed_ax: accepted ? after.readUInt32LE(16) % 65536 : null, helper_trace: [], ...(base === null ? {} : {base}), ...(unit.exitVersion === 5 ?
      {expected_exit_version: status === 0 ? 5 : null, observed_exit_version: accepted ? after.readUInt16LE(60) : null} : {})});
  counts.runs++; if (accepted) counts.retired += after.readUInt32LE(76);
}
function compile(c, name, blocks) {
  const instructions = blocks.reduce((sum, b) => sum + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8 && instructions <= 64);
  assert.ok(blocks.reduce((sum, b) => sum + b.bytes.length, 0) <= 65536); assert.ok(++c.captures <= 6);
  let bytes, binding = {};
  if (c.owner === 'standalone') bytes = readFileSync(join(output, `standalone-${c.entries ? 'entries' : 'extent'}-${name}.wasm`));
  else {
    input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
    const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
    host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]); const b = arena(c);
    binding = c.owner === 'resident' ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
      : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
    bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length));
  }
  assert.ok(bytes.length <= 65536); const module = new WebAssembly.Module(bytes), guard = c.owner === 'resident' ? 'guard_resident' : 'guard';
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...(c.owner === 'standalone' ? [] : [{module: 'ring3', name: guard, kind: 'function'}])];
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  const exitVersion = blocks.some(b => b.arithmetic === true) ? 5 : 1;
  modules.push({context: c.id, owner: c.owner, entries: c.entries, name, blocks: blocks.length, instructions, file, sha256: hash(bytes), imports, exit_version: exitVersion, ...binding});
  return {exitVersion, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ...(c.api ? {ring3: c.api} : {})}).exports.run};
}
function open(owner, entries, code = [...main, ...live]) {
  const c = {id: ++counts.contexts, owner, entries, captures: 0};
  if (owner === 'standalone') {c.memory = new WebAssembly.Memory({initial: 1}); c.base = 128; input(c, 0, initial); return c;}
  const instance = new WebAssembly.Instance(engine.module, {}); c.memory = instance.exports.memory; c.api = {};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'protect', 'upload', 'read8', 'compile', 'compile_entries',
    'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident']) c.api[name] = instance.exports['ring3_abi_v1_' + name];
  assert.equal(c.api.open(2, c.id, 0x41414153), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const page of [0x1000, 0x7000]) host(c, 'map', [page, 1, 7]);
  for (const b of code) upload(c, b.pc, b.bytes);
  upload(c, 0x7010, Buffer.from([0x55, 0xa5, 0x5a, 0xaa])); host(c, 'protect', [0x7000, 1, 1]); return c;
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), unit = compile(c, 'main', main);
  for (const row of main) {
    for (let a = 0; a < 256; a++) for (let af = 0; af < 2; af++) for (const ah of [0, 255]) {
      const f = 0x8c6 + af * 16 + a % 2 + Math.floor(a / 2) % 2 * 1024, r = parents(a + af * 256 + ah); r[0] = setWord(r[0], ah * 256 + a);
      seed(c, row.pc, f, r); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), row.kind, {category: 'exhaustive'}); counts.exhaustive++;
    }
    for (const [i, f] of allFlags.entries()) {
      const pair = Math.floor(i / 2), a = [0, 5, 6, 9, 10, 0x95, 0x99, 0x9a, 0xfa, 0xff][pair % 10];
      const ah = [0, 255, 0x7f, 0x80][pair % 4], r = parents(pair); r[0] = setWord(r[0], ah * 256 + a);
      seed(c, row.pc, f, r); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), row.kind, {category: 'all128FLAGS'}); counts.flag_inputs++;
    }
    for (const [kind, ax, f, result, expectedFlags] of literals.filter(l => l[0] === row.kind)) {
      const r = parents(ax); r[0] = setWord(r[0], ax); seed(c, row.pc, f + 0xc00, r);
      const next = cpu(c); next.pc++; next.registers[0] = setWord(next.registers[0], result); next.flags = expectedFlags + 0x400;
      run(c, unit, 1, next, kind, {category: 'literal-anchor'}); counts.anchors++;
    }
    const r = parents(37); r[0] = setWord(r[0], row.kind === 'AAA' ? 0x0095 : 0x0000); seed(c, row.pc, 0xc12, r);
    for (let i = 0; i < 2; i++) {input(c, 0, state({...cpu(c), pc: row.pc})); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), 'only PC rewound; returned AX/AF/CF are current'); counts.repeats++;}
    const current = cpu(c); current.pc = row.pc; current.flags = 0x813; current.registers = parents(731); current.registers[0] = setWord(current.registers[0], 0x8006);
    input(c, 0, state(current)); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), 'explicit current AX/high16/FLAGS/allparents'); counts.current_inputs++;
    seed(c, row.pc); run(c, unit, 0, cpu(c), 'zero budget neutral', {retired: 0});
    input(c, 96, words([1])); run(c, unit, 1, cpu(c), 'cancel neutral', {reason: 2, retired: 0});
    run(c, unit, 0, cpu(c), 'cancel precedes zero budget', {reason: 2, retired: 0}); input(c, 96, words([0]));
    run(c, unit, 3, adjusted(cpu(c), row.kind, row.pc + 3), 'adjust and JMP retire before cold lookup', {reason: 3, retired: 2});
    run(c, unit, 1, cpu(c), 'cold entry neutral', {reason: 3, retired: 0}); run(c, unit, 0, cpu(c), 'budget before cold lookup', {retired: 0}); counts.controls += 6;
  }
  const continuation = compile(c, 'live', live); seed(c, main[0].pc);
  if (owner === 'replacement') run(c, unit, 1, cpu(c), 'old replacement generation neutral InvalidArtifact3', {status: 3, retired: 0});
  else run(c, unit, 1, adjusted(cpu(c), 'AAA', main[0].pc + 1), 'older unit remains usable'); counts.retained++;
  for (const row of live) {
    const r = parents(0); r[0] = setWord(r[0], row.kind === 'AAA' ? 0x0009 : 0x0003); r[3] = setByte(r[3], row.kind === 'AAA' ? 0x09 : 0x07); r[1] = setByte(r[1], 0);
    seed(c, row.pc, 0x402, r);
    let next = cpu(c); next.pc += 2; next.registers[0] = setByte(next.registers[0], row.kind === 'AAA' ? 0x12 : 0xfc); next.flags = row.kind === 'AAA' ? 0x416 : 0x497;
    run(c, continuation, 1, next, 'live ' + row.kind + ' ordinary byte producer');
    next = cpu(c); next.pc++; next.registers[0] = setWord(next.registers[0], row.kind === 'AAA' ? 0x0108 : 0xff06); next.flags = 0x413;
    run(c, continuation, 1, next, 'live ' + row.kind + ' consumes producer AL/AF and clears four undefined flags');
    next = cpu(c); next.pc += 6; next.registers[2] = Math.floor(next.registers[2] / 65536) * 65536 + 1;
    run(c, continuation, 2, next, 'live SETB/SETO observe adjusted carry/cleared overflow', {retired: 2});
    next = cpu(c); next.pc += 3; next.registers[1] = setByte(next.registers[1], 1); next.flags = 0x402;
    run(c, continuation, 1, next, 'live ADC consumes adjusted carry without CPU reseeding');
    run(c, continuation, 2, {...cpu(c), pc: row.pc + 14}, 'live cold tail preserves returned state', {reason: 3}); counts.consumers += 5;
  }
  if (c.api) {
    for (const [i, value] of [0x55, 0xa5, 0x5a, 0xaa].entries()) {
      const expected = arena(c), helper = record('R3MH', 40, [0, value, 0, 0, 0, 1]); helper.writeUInt16LE(2, 4); expected.set(helper, 100);
      assert.equal(c.api.read8(0x7010 + i), 0); assert.deepEqual(arena(c), expected, 'selected readonly RAM canaries unchanged');
    }
    upload(c, main[0].pc, main[0].bytes.subarray(0, 1));
    run(c, continuation, 1, cpu(c), 'same-byte RWX source mutation neutral CodeInvalidated4', {status: 4, retired: 0}); counts.invalidated++;
    assert.equal(c.api.close(), 0);
  }
  contexts.push({id: c.id, owner, entries, captures: c.captures, maximum_blocks: 2, maximum_instructions: 12, maximum_mapped_pages: c.api ? 2 : null});
}
assert.deepEqual(counts, {contexts: 6, runs: 14098, retired: 14056, exhaustive: 12288, flag_inputs: 1536, anchors: 96,
  repeats: 24, current_inputs: 12, controls: 72, retained: 6, consumers: 60, invalidated: 4, helper_calls: 0}); assert.equal(modules.length, 12);
const asciiCounts = {...counts}, aadCounts = {all_bases: 0, byte_inputs: 0, flag_inputs: 0, anchors: 0,
  repeats: 0, current_inputs: 0, controls: 0, retained: 0, consumers: 0, invalidated: 0};
for (const owner of ['standalone', 'replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries, [...aadBanks.flatMap(b => b.blocks), ...aadLive]); let firstUnit;
  for (const bank of aadBanks) {
    const unit = compile(c, bank.name, bank.blocks); if (!firstUnit) firstUnit = unit;
    for (const row of bank.rows) {
      for (const ax of [0x0000, 0x0101, 0x80ff, 0xffff]) {
        const r = parents(ax + row.base); r[0] = setWord(r[0], ax); seed(c, row.pc, 0xcd7, r);
        run(c, unit, 1, aadAdjusted(cpu(c), row.base, row.pc + 2), 'all unsigned bases at selected AX', {category: 'aad-all-bases', base: row.base}); aadCounts.all_bases++;
      }
      for (const [ah, base] of [[255, 0], [0x7f, 10], [0x80, 128], [255, 255]].filter(pair => pair[1] === row.base)) {
        for (let al = 0; al < 256; al++) {
          const r = parents(al + ah); r[0] = setWord(r[0], ah * 256 + al); seed(c, row.pc, 0xcd7, r);
          run(c, unit, 1, aadAdjusted(cpu(c), base, row.pc + 2), 'all AL at selected AH/base', {category: 'aad-byte-input', base}); aadCounts.byte_inputs++;
        }
      }
      if (row.base === 10) for (const f of allFlags) {
        const r = parents(0); r[0] = setWord(r[0], 0x1234); seed(c, row.pc, f, r);
        run(c, unit, 1, aadAdjusted(cpu(c), row.base, row.pc + 2), 'all128 FLAGS with identical AX/base', {category: 'aad-all128FLAGS', base: row.base}); aadCounts.flag_inputs++;
      }
      for (const [base, ax, result, f] of aadLiterals.filter(a => a[0] === row.base)) {
        const r = parents(ax); r[0] = setWord(r[0], ax); seed(c, row.pc, 0xcd7, r);
        const next = cpu(c); next.pc += 2; next.registers[0] = setWord(next.registers[0], result); next.flags = f;
        run(c, unit, 1, next, 'literal AAD original bytes/base/undefined flags', {category: 'aad-literal-anchor', base}); aadCounts.anchors++;
      }
      if (row.base === 10) {
        const r = parents(37); r[0] = setWord(r[0], 0x1234); seed(c, row.pc, 0xcd7, r);
        for (let i = 0; i < 2; i++) {
          input(c, 0, state({...cpu(c), pc: row.pc}));
          run(c, unit, 1, aadAdjusted(cpu(c), row.base, row.pc + 2), 'AAD repeat rewinds only PC; returned AH0 is current', {category: 'aad-repeat', base: row.base}); aadCounts.repeats++;
        }
      }
      if (row.base === 255) {
        const current = cpu(c); current.pc = row.pc; current.flags = 0x813; current.registers = parents(731); current.registers[0] = 0xc9008006;
        input(c, 0, state(current)); run(c, unit, 1, aadAdjusted(cpu(c), row.base, row.pc + 2), 'AAD explicit current old bytes/high16/FLAGS', {category: 'aad-current-input', base: row.base}); aadCounts.current_inputs++;
      }
    }
    if (bank === aadBanks[0]) {
      const row = bank.rows.at(-1); seed(c, row.pc);
      run(c, unit, 0, cpu(c), 'AAD zero budget neutral', {category: 'aad-control', retired: 0, base: row.base});
      input(c, 96, words([1])); run(c, unit, 1, cpu(c), 'AAD cancel neutral', {category: 'aad-control', reason: 2, retired: 0, base: row.base});
      run(c, unit, 0, cpu(c), 'AAD cancel before zero budget', {category: 'aad-control', reason: 2, retired: 0, base: row.base}); input(c, 96, words([0]));
      run(c, unit, 3, aadAdjusted(cpu(c), row.base, row.pc + 4), 'last AAD and JMP retire before cold lookup', {category: 'aad-control', reason: 3, retired: 2, base: row.base});
      run(c, unit, 1, cpu(c), 'AAD cold entry neutral', {category: 'aad-control', reason: 3, retired: 0});
      run(c, unit, 0, cpu(c), 'AAD budget before cold lookup', {category: 'aad-control', retired: 0}); aadCounts.controls += 6;
    }
  }
  const continuation = compile(c, 'aad-live', aadLive); seed(c, aadBanks[0].rows[0].pc);
  if (owner === 'replacement') run(c, firstUnit, 1, cpu(c), 'AAD old replacement generation neutral InvalidArtifact3', {category: 'aad-retained', status: 3, retired: 0});
  else run(c, firstUnit, 1, aadAdjusted(cpu(c), 0, aadBanks[0].rows[0].pc + 2), 'AAD older unit remains usable', {category: 'aad-retained', base: 0}); aadCounts.retained++;
  for (const row of aadLive) {
    const r = parents(0); r[1] = setByte(r[1], 0); seed(c, row.pc, 0xcd7, r);
    let next = cpu(c); next.pc += 4; next.registers[0] = setWord(next.registers[0], row.byte * 257);
    run(c, continuation, 2, next, 'live MOV AH/AL produces both AAD inputs', {category: 'aad-consumer', retired: 2});
    next = cpu(c); next.pc += 2; next.registers[0] = setWord(next.registers[0], row.result); next.flags = row.flags;
    run(c, continuation, 1, next, 'live AAD consumes original AH/AL and clears AH/CF/AF/OF', {category: 'aad-consumer', base: row.base});
    next = cpu(c); next.pc += 12; next.registers[2] = setWord(next.registers[2], row.result === 0 ? 0x0101 : 0x0100); next.registers[3] = setWord(next.registers[3], 0);
    run(c, continuation, 4, next, 'live SETZ/SETP/SETB/SETO consumes returned flags', {category: 'aad-consumer', retired: 4});
    next = cpu(c); next.pc += 3; next.registers[1] = setByte(next.registers[1], 0); next.flags = 0x446;
    run(c, continuation, 1, next, 'live ADC consumes cleared carry without CPU reseeding', {category: 'aad-consumer'});
    run(c, continuation, 2, {...cpu(c), pc: row.pc + row.bytes.length}, 'live AAD cold tail preserves returned state', {category: 'aad-consumer', reason: 3}); aadCounts.consumers += 5;
  }
  if (c.api) {
    for (const [i, value] of [0x55, 0xa5, 0x5a, 0xaa].entries()) {
      const expected = arena(c), helper = record('R3MH', 40, [0, value, 0, 0, 0, 1]); helper.writeUInt16LE(2, 4); expected.set(helper, 100);
      assert.equal(c.api.read8(0x7010 + i), 0); assert.deepEqual(arena(c), expected, 'AAD selected readonly RAM canaries unchanged');
    }
    upload(c, aadBanks[0].blocks[0].pc, Buffer.from([0xd5]));
    run(c, continuation, 1, cpu(c), 'AAD same-byte RWX source mutation neutral CodeInvalidated4', {category: 'aad-invalidated', status: 4, retired: 0}); aadCounts.invalidated++;
    assert.equal(c.api.close(), 0);
  }
  assert.equal(c.captures, 6);
  contexts.push({id: c.id, owner, entries, purpose: 'AAD', captures: c.captures, maximum_blocks: 2, maximum_instructions: 64, maximum_mapped_pages: c.api ? 2 : null});
}
assert.deepEqual(aadCounts, {all_bases: 6144, byte_inputs: 6144, flag_inputs: 768, anchors: 96,
  repeats: 12, current_inputs: 6, controls: 36, retained: 6, consumers: 60, invalidated: 4});
aadCounts.contexts = counts.contexts - asciiCounts.contexts; aadCounts.runs = counts.runs - asciiCounts.runs;
aadCounts.retired = counts.retired - asciiCounts.retired; aadCounts.modules = modules.length - 12; aadCounts.helper_calls = 0;
assert.equal(aadCounts.contexts, 6); assert.equal(aadCounts.runs, 13276); assert.equal(aadCounts.retired, 13294); assert.equal(aadCounts.modules, 36);
assert.equal(counts.contexts, 12); assert.equal(counts.runs, 27374); assert.equal(counts.retired, 27350); assert.equal(modules.length, 48);
const previousCounts = {...counts}, aamCounts = {all_bases: 0, byte_inputs: 0, flag_inputs: 0, anchors: 0,
  repeats: 0, current_inputs: 0, controls: 0, retained: 0, consumers: 0, zero_controls: 0, invalidated: 0, divide_errors: 0};
function aamStep(c, unit, row, label, category) {
  const before = cpu(c), expected = aamOracle(before.registers[0] % 65536, row.base, before.flags);
  const next = {...before, registers: [...before.registers]};
  if (expected !== null) {next.pc += 2; next.registers[0] = setWord(next.registers[0], expected.result); next.flags = expected.flags;}
  run(c, unit, 1, next, label, {category, base: row.base, reason: expected === null ? 10 : 1, retired: expected === null ? 0 : 1});
  if (expected === null) aamCounts.divide_errors++;
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries, [...aamBanks.flatMap(b => b.blocks), ...aamLive]); let firstUnit;
  for (const bank of aamBanks) {
    const unit = compile(c, bank.name, bank.blocks); if (!firstUnit) firstUnit = unit;
    for (const row of bank.rows) {
      for (const ax of [0x0000, 0xff00, 0x007f, 0xffff]) {
        const r = parents(ax + row.base); r[0] = setWord(r[0], ax); seed(c, row.pc, 0xcd7, r);
        aamStep(c, unit, row, 'AAM all bases at selected AL/ignored AH', 'aam-all-bases'); aamCounts.all_bases++;
      }
      for (const [ah, base] of [[255, 1], [0x7f, 10], [0x80, 128], [255, 255]].filter(pair => pair[1] === row.base)) {
        for (let al = 0; al < 256; al++) {
          const r = parents(al + ah); r[0] = setWord(r[0], ah * 256 + al); seed(c, row.pc, 0xcd7, r);
          aamStep(c, unit, row, 'AAM all AL at selected positive bases/poisoned AH', 'aam-byte-input'); aamCounts.byte_inputs++;
        }
      }
      if (row.base === 0 || row.base === 10) for (const f of allFlags) {
        const r = parents(0); r[0] = setWord(r[0], 0x1234); seed(c, row.pc, f, r);
        aamStep(c, unit, row, 'AAM all128 FLAGS on positive and zero bases', 'aam-all128FLAGS'); aamCounts.flag_inputs++;
      }
      for (const [base, ax, result, f] of aamLiterals.filter(a => a[0] === row.base)) {
        const r = parents(ax); r[0] = setWord(r[0], ax); seed(c, row.pc, 0xcd7, r); const next = cpu(c);
        if (result !== null) {next.pc += 2; next.registers[0] = setWord(next.registers[0], result); next.flags = f;}
        run(c, unit, 1, next, 'literal AAM quotient/remainder or zero-base error', {category: 'aam-literal-anchor', base, reason: result === null ? 10 : 1, retired: result === null ? 0 : 1});
        aamCounts.anchors++; if (result === null) aamCounts.divide_errors++;
      }
      if (row.base === 10) {
        const r = parents(37); r[0] = setWord(r[0], 0x8081); seed(c, row.pc, 0xcd7, r);
        for (let i = 0; i < 2; i++) {
          input(c, 0, state({...cpu(c), pc: row.pc})); aamStep(c, unit, row, 'AAM repeat rewinds only PC; returned AL is current', 'aam-repeat'); aamCounts.repeats++;
        }
      }
      if (row.base === 255) {
        const current = cpu(c); current.pc = row.pc; current.flags = 0x813; current.registers = parents(731); current.registers[0] = 0xc90080ff;
        input(c, 0, state(current)); aamStep(c, unit, row, 'AAM explicit current AL/poisoned AH/high16/FLAGS', 'aam-current-input'); aamCounts.current_inputs++;
      }
    }
    if (bank === aamBanks[0]) {
      const row = bank.rows.at(-1); seed(c, row.pc);
      run(c, unit, 0, cpu(c), 'AAM zero budget emits V5', {category: 'aam-control', retired: 0});
      input(c, 96, words([1])); run(c, unit, 1, cpu(c), 'AAM cancel emits V5', {category: 'aam-control', reason: 2, retired: 0});
      run(c, unit, 0, cpu(c), 'AAM cancel precedes zero budget', {category: 'aam-control', reason: 2, retired: 0}); input(c, 96, words([0]));
      const expected = aamOracle(cpu(c).registers[0] % 65536, row.base, cpu(c).flags), next = cpu(c);
      next.pc = row.pc + 4; next.registers[0] = setWord(next.registers[0], expected.result); next.flags = expected.flags;
      run(c, unit, 3, next, 'last AAM and JMP retire before cold lookup', {category: 'aam-control', reason: 3, retired: 2, base: row.base});
      run(c, unit, 1, cpu(c), 'AAM cold emits V5', {category: 'aam-control', reason: 3, retired: 0});
      run(c, unit, 0, cpu(c), 'AAM budget precedes cold lookup', {category: 'aam-control', retired: 0}); aamCounts.controls += 6;
    }
  }
  const continuation = compile(c, 'aam-live', aamLive), retainedRow = aamBanks[0].rows[10]; seed(c, retainedRow.pc);
  if (owner === 'replacement') run(c, firstUnit, 1, cpu(c), 'AAM old replacement generation neutral3', {category: 'aam-retained', status: 3, retired: 0});
  else aamStep(c, firstUnit, retainedRow, 'AAM older unit remains usable', 'aam-retained'); aamCounts.retained++;
  const positive = aamLive[0], r = parents(0); r[0] = setWord(r[0], 0xff11); r[3] = setByte(r[3], 17); r[1] = setByte(r[1], 0); seed(c, positive.pc, 0xcd7, r);
  let next = cpu(c); next.pc += 2; next.registers[0] = setWord(next.registers[0], 0x0121); next.flags = 0xc03;
  run(c, continuation, 1, next, 'live BYTE MUL17x17 returns full AX and overflow', {category: 'aam-consumer'});
  next = cpu(c); next.pc += 2; next.registers[0] = setWord(next.registers[0], 0x0303); next.flags = 0x406;
  run(c, continuation, 1, next, 'live AAM10 consumes AL21 only, ignoring returned AH1', {category: 'aam-consumer', base: 10});
  next = cpu(c); next.pc += 12; next.registers[2] = setWord(next.registers[2], 0x0100); next.registers[3] = setWord(next.registers[3], 0);
  run(c, continuation, 4, next, 'live SETZ/SETP/SETB/SETO consumes AAM flags', {category: 'aam-consumer', retired: 4});
  next = cpu(c); next.pc += 3; next.registers[1] = setByte(next.registers[1], 0); next.flags = 0x446;
  run(c, continuation, 1, next, 'live ADC consumes cleared AAM carry', {category: 'aam-consumer'});
  run(c, continuation, 2, {...cpu(c), pc: positive.pc + positive.bytes.length}, 'live positive AAM cold tail', {category: 'aam-consumer', reason: 3}); aamCounts.consumers += 5;
  const faulting = aamLive[1], faultRegisters = parents(0); faultRegisters[0] = setWord(faultRegisters[0], 0x8009); faultRegisters[3] = setByte(faultRegisters[3], 9); seed(c, faulting.pc, 0xcd7, faultRegisters);
  next = cpu(c); next.pc += 2; next.registers[0] = setWord(next.registers[0], 0x8012); next.flags = 0x416;
  run(c, continuation, 2, next, 'live ADD commits before AAM0 stops at faultPC', {category: 'aam-fault-consumer', base: 0, reason: 10, retired: 1});
  run(c, continuation, 1, cpu(c), 'unchanged AAM0 retry has no retirement/publication', {category: 'aam-fault-consumer', base: 0, reason: 10, retired: 0});
  const current = cpu(c); current.registers = parents(731); current.registers[0] = 0xc900ff00; current.flags = 0x813; input(c, 0, state(current));
  run(c, continuation, 1, cpu(c), 'explicit new AL/AH/high16/FLAGS cannot repair base0', {category: 'aam-fault-consumer', base: 0, reason: 10, retired: 0}); aamCounts.consumers += 3; aamCounts.divide_errors += 3;
  run(c, continuation, 0, cpu(c), 'budget0 precedes AAM0 divide error', {category: 'aam-zero-control', base: 0, retired: 0});
  input(c, 96, words([1])); run(c, continuation, 1, cpu(c), 'cancel precedes AAM0 divide error', {category: 'aam-zero-control', base: 0, reason: 2, retired: 0});
  run(c, continuation, 0, cpu(c), 'cancel precedes both AAM0 and budget0', {category: 'aam-zero-control', base: 0, reason: 2, retired: 0}); input(c, 96, words([0])); aamCounts.zero_controls += 3;
  if (c.api) {
    for (const [i, value] of [0x55, 0xa5, 0x5a, 0xaa].entries()) {
      const expected = arena(c), helper = record('R3MH', 40, [0, value, 0, 0, 0, 1]); helper.writeUInt16LE(2, 4); expected.set(helper, 100);
      assert.equal(c.api.read8(0x7010 + i), 0); assert.deepEqual(arena(c), expected, 'AAM selected readonly RAM canaries unchanged');
    }
    upload(c, aamBanks[0].blocks[0].pc, Buffer.from([0xd4]));
    run(c, continuation, 1, cpu(c), 'AAM same-byte RWX source mutation neutral4', {category: 'aam-invalidated', status: 4, retired: 0}); aamCounts.invalidated++;
    assert.equal(c.api.close(), 0);
  }
  assert.equal(c.captures, 6);
  contexts.push({id: c.id, owner, entries, purpose: 'AAM', captures: c.captures, maximum_blocks: 2, maximum_instructions: 64, maximum_mapped_pages: c.api ? 2 : null});
}
assert.deepEqual(aamCounts, {all_bases: 6144, byte_inputs: 6144, flag_inputs: 1536, anchors: 96,
  repeats: 12, current_inputs: 6, controls: 36, retained: 6, consumers: 48, zero_controls: 18, invalidated: 4, divide_errors: 822});
aamCounts.contexts = counts.contexts - previousCounts.contexts; aamCounts.runs = counts.runs - previousCounts.runs;
aamCounts.retired = counts.retired - previousCounts.retired; aamCounts.modules = modules.length - 48; aamCounts.helper_calls = 0;
assert.equal(aamCounts.contexts, 6); assert.equal(aamCounts.runs, 14050); assert.equal(aamCounts.retired, 13204); assert.equal(aamCounts.modules, 36);
assert.equal(counts.contexts, 18); assert.equal(counts.runs, 41424); assert.equal(counts.retired, 40554); assert.equal(modules.length, 84);
assert.deepEqual(sources(), sourcePins);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, ascii_counts: asciiCounts, aad_counts: aadCounts, aam_counts: aamCounts, contexts, modules, observations,
  literal_anchors: literals, aad_literal_anchors: aadLiterals, aam_literal_anchors: aamLiterals, aam_banks: aamBanks.map(b => ({name: b.name, bases: b.rows.map(r => r.base), instructions: b.blocks[0].instructions})), aad_banks: aadBanks.map(b => ({name: b.name, bases: b.rows.map(r => r.base), instructions: b.blocks[0].instructions})),
  limits: ['finite exact37/3F/D5imm8/D4imm8 pure six-profile proof; original AAA/AAS checkpoint14098calls/14056retired/12captures and first uploads/order preserved',
    'AAA/AAS exhaustiveAL256×AF2×AH00/FF plus all128 admittedFLAGS at selected thresholds; independent original-AH and AL-correction carry/borrow oracle and literal fullAX wrap',
    'AAA/AAS CF=AF=adjustmentcondition and SF/ZF/PF/OF clear; AAD derivesSZP from lowbyte and clearsCF/AF/OF; undefined-flag clearing is Ring3 policy, DF/bit1 retained',
    'AAD all256 bases at four selectedAX and exhaustiveAL at four selectedAH/base pairs, all128FLAGS with identicalAX/base; not exhaustivewholeAX×allbases',
    'AAD unsigned u8 identity is native typedIR evidence: signed-vs-unsigned multiplication is congruent modulo256 and not discriminated by runtime results',
    'full4612 arena equality preserves ownEAXhigh16/allotherGPR/helper/dirtyFP128; selected bound readonlyRAMcanaries, no guest-memoryoperand/arbitrary-hostrollback/wholeRAMsnapshot claim',
    'live byteADD/SUB→AAA/AAS→SETB/SETO→ADC and MOVAH/AL→AAD→SETZ/SETP/SETB/SETO→ADC have no CPU reseeding; observed per-row status/reason/retirement/PC/FLAGS/AX persisted, neutral3/4 observations null',
    'oldmain2blocks4instructions/live2blocks12instructions and2captures/context; AAD five1blockbanks64/64/64/64/5instructions plus2block18instructionlive, exactly6captures/context',
    'allcode/protections before captures; default8/64/65536/sixcapture unchanged; unbound savedmodules lack runtime sourceguard; replacement generation3 and same-byteRWX source CodeInvalidated4 are neutral',
    'AAM extends six contexts with five1block64/64/64/64/5 banks and2block11instructionlive, exactly6captures; previous48modules and27374observation prefix preserved for matching-parent comparison',
    'AAM all256bases at selectedAX and exhaustiveAL at fourpositivebases; all128FLAGS on positivebase10 and base0, repeated-subtraction quotient/remainder oracle uses originalAL only; not exhaustiveAL×allbases×FLAGS',
    'AAM units always emit existingV5 including positive-only/budget/cancel/cold; base0 reason10 preservesCPU/FLAGS and faultPC without retiring AAM or trapping, priorproducer effects/retirement retained',
    'live BYTE MUL17x17→AX0121→AAM10 usesAL21only→0303/406→SETcc→ADC446; ADD→AAM0, unchanged/current retries and budget/cancel precedence are separate observed cases',
    'empty guest-memory-helper trace is structural importabsence, not guard-calltelemetry; no hardwarecompleteness/prefixedaliases/performance/SDK/browser/game/dispatcherexpansion/hostedCI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
