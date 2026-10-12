import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const engine = readEngine(enginePath), PC = 0x1000, DATA = 0x5000, SIZE = 4612;
const sourcePaths = [
  'engine/src/cpu/x86/bits.rs', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/scalar_bits.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/mod.rs',
  'engine/src/cpu/dbt/wasm/integer/scalar_bits.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/tests/cpu/bits_flags/count/fixtures/p2-scalar-bits/runtime/run.mjs', 'engine/tests/cpu/bits_flags/count/wasm/runtime.rs',
];
const sources = () => Object.fromEntries(sourcePaths.map(path => [path, createHash('sha256').update(readFileSync(join(root, path))).digest('hex')]));
const sourcePins = sources();
const counts = {contexts: 0, modules: 0, runs: 0, counts: 0, swaps: 0, faults: 0, repairs: 0, controls: 0};
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((value, i) => b.writeUInt32LE(value >>> 0, 16 + i * 4)); return b;
}
const cpuRecord = s => record('R3ST', 56, [...s.registers, s.pc, s.flags]);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, bytes) {new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);}
function upload(c, address, bytes) {input(c, 140, bytes); assert.equal(c.api.upload(address, bytes.length), 0);}
function cpu(c) {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};}
function seed(c, source = 0, address = DATA + 7) {
  const registers = Array.from({length: 8}, (_, i) => (0xa5101234 + i * 0x10001) >>> 0);
  registers[0] = 0xa5c31234; registers[2] = source >>> 0; registers[7] = address >>> 0;
  const state = {registers, pc: PC, flags: 0xcd7}; input(c, 0, cpuRecord(state)); input(c, 96, words([0])); return state;
}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, api: {}, memory: instance.exports.memory};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'discard_unacknowledged_resident', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident', 'read16', 'read32', 'store16', 'store32', 'store_resident16', 'store_resident32']) c.api[name] = instance.exports['ring3_abi_v1_' + name];
  c.keyLow = ++counts.contexts;
  assert.equal(c.api.open(8, c.keyLow, 0x53424954), 0); c.base = c.api.arena_ptr() >>> 0;
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xfffff000, 1, 3]]) assert.equal(c.api.map(...args), 0);
  return c;
}
function compile(c, instruction, memoryName, store = false) {
  if (c.previousResident) {
    input(c, 96, words([0]));
    assert.equal(c.api.discard_unacknowledged_resident(c.keyLow, 0x53424954, ...c.previousResident), 0);
  }
  upload(c, PC, Buffer.from([...instruction, 0xeb, 0]));
  input(c, 140, words(c.entries ? [PC] : [PC, instruction.length + 2]));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  assert.equal(c.api[method](1, ...(c.entries ? [0] : [])), 0);
  const b = arena(c), pointer = c.owner === 'resident' ? b.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  if (c.owner === 'resident') c.previousResident = [b.readUInt32LE(148), b.readUInt32LE(152)];
  const length = c.owner === 'resident' ? b.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
  const expected = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: c.owner === 'resident' ? 'guard_resident' : 'guard', kind: 'function'}];
  if (memoryName) expected.push({module: 'ring3', name: store && c.owner === 'resident' ? memoryName.replace('store', 'store_resident') : memoryName, kind: 'function'});
  assert.deepEqual(WebAssembly.Module.imports(module), expected, 'register forms have no memory helper import; each memory form has its one required helper');
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), bytes, {flag: 'wx'});
  const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run;
  run.exitVersion = memoryName ? 2 : 1;
  return run;
}
function run(c, fn, state, {budget = 1, reason = 1, retired = 1, helper, fault} = {}) {
  const expected = arena(c); expected.set(cpuRecord(state));
  expected.set(record('R3EX', 40, [reason, retired, fault?.detail ?? 0, fault?.address ?? 0, fault?.access ?? 0, fault?.length ?? 0], fn.exitVersion), 56);
  if (helper) expected.set(record('R3MH', 40, helper.fields, helper.version), 100);
  assert.equal(fn(c.base, c.base + 56, budget, c.base + 96), 0);
  assert.deepEqual(arena(c), expected, 'complete CPU, exit, helper and unrelated numeric-state preservation'); counts.runs++;
}
function encoded(value, width) {const b = Buffer.alloc(width / 8); if (width === 16) b.writeUInt16LE(value & 0xffff); else b.writeUInt32LE(value >>> 0); return b;}
function readBytes(c, address, length) {const b = Buffer.alloc(length); for (let i = 0; i < length; i++) {assert.equal(c.api.read8((address + i) >>> 0), 0); b[i] = arena(c).readUInt32LE(120);} return b;}
const merged = (old, value, width) => width === 16 ? ((old & 0xffff0000) | value) >>> 0 : value >>> 0;
function counted(kind, value, width) {
  const bits = (width === 16 ? value & 0xffff : value >>> 0).toString(2).padStart(width, '0');
  const zero = !bits.includes('1');
  const count = kind === 'population' ? bits.replaceAll('0', '').length : zero ? width : kind === 'leading' ? bits.indexOf('1') : width - 1 - bits.lastIndexOf('1');
  return {count, flags: 0x402 + (kind === 'population' ? Number(zero) * 0x40 : Number(zero) + Number(count === 0) * 0x40)};
}
function swapped(value, width) {return parseInt(encoded(value, width).toString('hex'), 16) >>> 0;}
assert.deepEqual(counted('trailing', 0, 16), {count: 16, flags: 0x403});
assert.deepEqual(counted('leading', 0x8000, 16), {count: 0, flags: 0x442});
assert.deepEqual(counted('population', 0xffffffff, 32), {count: 32, flags: 0x402});
assert.equal(swapped(0x12345678, 32), 0x78563412);
assert.equal(swapped(0x1234, 16), 0x3412);

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const [kind, opcode] of [['trailing', 0xbc], ['leading', 0xbd], ['population', 0xb8]]) for (const width of [16, 32]) for (const operand of ['register', 'alias', 'memory']) {
    const instruction = [...(width === 16 ? [0x66] : []), 0xf3, 0x0f, opcode, operand === 'memory' ? 0x07 : operand === 'alias' ? 0xc0 : 0xc2];
    const fn = compile(c, instruction, operand === 'memory' ? `read${width}` : null);
    for (const source of [0, 1, 2, 0x100, 0x8000, 0x80000000, 0xffff0000, 0x80018001, 0xffffffff]) {
      const address = source === 0x100 ? DATA + 4095 : DATA + 7;
      if (operand === 'memory') upload(c, address, encoded(source, width));
      const before = seed(c, source, address); if (operand === 'alias') {before.registers[0] = source >>> 0; input(c, 16, words([source]));}
      const oracle = counted(kind, source, width), next = {...before, registers: [...before.registers], pc: PC + instruction.length, flags: oracle.flags};
      next.registers[0] = merged(before.registers[0], oracle.count, width);
      run(c, fn, next, {helper: operand === 'memory' ? {version: width === 16 ? 2 : 1, fields: [0, width === 16 ? source & 0xffff : source, 0, 0, 0, width === 16 ? 2 : 0]} : undefined}); counts.counts++;
      if (operand === 'memory') assert.deepEqual(readBytes(c, address, width / 8), encoded(source, width));
    }
    if (operand === 'memory') {
      const address = 0x6fff, before = seed(c, 0, address), fault = {detail: 1, address: 0x7000, access: 1, length: width / 8};
      upload(c, address, Buffer.from([0]));
      run(c, fn, before, {reason: 5, retired: 0, fault, helper: {version: width === 16 ? 2 : 1, fields: [1, 0, 1, 0x7000, 1, width / 8]}}); counts.faults++;
      assert.equal(c.api.map(0x7000, 1, 3), 0); upload(c, address, encoded(0x8001, width));
      const oracle = counted(kind, 0x8001, width), next = {...before, registers: [...before.registers], pc: PC + instruction.length, flags: oracle.flags}; next.registers[0] = merged(before.registers[0], oracle.count, width);
      run(c, fn, next, {helper: {version: width === 16 ? 2 : 1, fields: [0, 0x8001, 0, 0, 0, width === 16 ? 2 : 0]}}); counts.repairs++;
      assert.equal(c.api.unmap(0x7000, 1), 0);
    }
    const stopped = seed(c, 0, 0x8000); run(c, fn, stopped, {budget: 0, retired: 0}); input(c, 96, words([1])); run(c, fn, stopped, {reason: 2, retired: 0}); counts.controls += 2;
  }
  for (const width of [16, 32]) for (const store of [false, true]) {
    const instruction = [...(width === 16 ? [0x66] : []), 0x0f, 0x38, store ? 0xf1 : 0xf0, 0x07];
    const fn = compile(c, instruction, `${store ? 'store' : 'read'}${width}`, store);
    for (const source of [0, 0x12345678, 0x80018001, 0xffffffff]) {
      const address = DATA + 4095; upload(c, address - 1, Buffer.from([0xaa, ...encoded(store ? 0 : source, width), 0x55]));
      const before = seed(c, 0, address); if (store) {before.registers[0] = source >>> 0; input(c, 16, words([source]));}
      const value = swapped(source, width), next = {...before, registers: [...before.registers], pc: PC + instruction.length};
      if (!store) next.registers[0] = merged(before.registers[0], value, width);
      run(c, fn, next, {helper: {version: store ? width === 16 ? 4 : 1 : width === 16 ? 2 : 1, fields: [0, store ? 0 : width === 16 ? source & 0xffff : source, 0, 0, 0, width === 16 ? 2 : 0]}});
      assert.deepEqual(readBytes(c, address - 1, width / 8 + 2), Buffer.from([0xaa, ...encoded(store ? value : source, width), 0x55])); counts.swaps++;
    }
    const address = 0x6fff, before = seed(c, 0, address), fault = {detail: 1, address: 0x7000, access: store ? 2 : 1, length: width / 8}; upload(c, address, Buffer.from([0xaa]));
    run(c, fn, before, {reason: 5, retired: 0, fault, helper: {version: store ? width === 16 ? 4 : 1 : width === 16 ? 2 : 1, fields: [1, 0, 1, 0x7000, fault.access, width / 8]}}); counts.faults++;
    assert.deepEqual(readBytes(c, address, 1), Buffer.from([0xaa]), 'failed MOVBE store leaves first page unchanged');
    assert.equal(c.api.map(0x7000, 1, 3), 0); upload(c, address, encoded(0x1234, width));
    const next = {...before, registers: [...before.registers], pc: PC + instruction.length}; if (!store) next.registers[0] = merged(before.registers[0], swapped(0x1234, width), width);
    run(c, fn, next, {helper: {version: store ? width === 16 ? 4 : 1 : width === 16 ? 2 : 1, fields: [0, store ? 0 : 0x1234, 0, 0, 0, width === 16 ? 2 : 0]}}); counts.repairs++;
    assert.equal(c.api.unmap(0x7000, 1), 0);
  }
  assert.equal(c.api.close(), 0);
}
assert.equal(counts.counts, 648); assert.equal(counts.swaps, 64); assert.equal(counts.faults, 40); assert.equal(counts.repairs, 40); assert.equal(counts.controls, 144);
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed during execution');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, limits: ['finite word/dword scalar count and MOVBE semantics on actual generated Wasm; no universal ISA, hardware, browser or throughput claim']}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
