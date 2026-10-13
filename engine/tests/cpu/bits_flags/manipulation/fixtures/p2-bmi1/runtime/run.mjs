import {ARENA_SIZE} from '../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const engine = readEngine(enginePath), PC = 0x1000, DATA = 0x5000, SIZE = ARENA_SIZE;
const sourcePaths = [
  'engine/src/cpu/x86/bits.rs', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/bit_manipulation.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/mod.rs',
  'engine/src/cpu/dbt/wasm/integer/bit_manipulation.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/tests/cpu/bits_flags/manipulation/fixtures/p2-bmi1/runtime/run.mjs', 'engine/tests/cpu/bits_flags/manipulation/wasm/bmi1.rs',
];
sourcePaths.push(...["engine/src/abi/memory_helper/cpu.rs", "engine/src/cpu/dbt/wasm/memory/cpu_scalar.rs", "engine/src/cpu/dbt/wasm/emitter.rs", "engine/src/cpu/dbt/wasm/code_fetch.rs", "engine/src/process/memory/cpu_scalar.rs", "engine/src/process/memory/scalar/cpu_write.rs", "engine/src/process/wasm/memory/cpu_write.rs", "engine/src/abi/wasm/exports/memory/cpu_write.rs", "engine/tests/support/wasm/arena.mjs"].filter(path=>!sourcePaths.includes(path)));
const sources = () => Object.fromEntries(sourcePaths.map(path => [path, createHash('sha256').update(readFileSync(join(root, path))).digest('hex')]));
const sourcePins = sources();
const counts = {contexts: 0, modules: 0, runs: 0, register: 0, memory: 0, faults: 0, repairs: 0, controls: 0};
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
  for(const name of ["read16", "read32", "read8", "store16", "store32", "store_resident16", "store_resident32"]) {c.api["cpu_"+name]=instance.exports["ring3_abi_v1_cpu_"+name];assert.equal(c.api["cpu_"+name]?.length,name.startsWith("read")?1:name.includes("resident")?6:2,"cpu_"+name);}
 assert.equal(c.api.open(8, c.keyLow, 0x424d4931), 0); c.base = c.api.arena_ptr() >>> 0;
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xfffff000, 1, 3]]) assert.equal(c.api.map(...args), 0);
  return c;
}
function compile(c, instruction, memoryName) {
  if (c.previousResident) {
    input(c, 96, words([0]));
    assert.equal(c.api.discard_unacknowledged_resident(c.keyLow, 0x424d4931, ...c.previousResident), 0);
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
  if (memoryName) expected.push({module: 'ring3', name: memoryName, kind: 'function'});
  assert.deepEqual(WebAssembly.Module.imports(module), expected, 'register forms have no memory helper import; each memory form has its one required helper');
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), bytes, {flag: 'wx'});
  const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run;
  run.exitVersion = memoryName ? 6 : 1;
  return run;
}
function run(c, fn, state, {budget = 1, reason = 1, retired = 1, helper, fault} = {}) {
  const expected = arena(c); expected.set(cpuRecord(state));
  expected.set(record('R3EX', 40, [reason, retired, fault?.detail ?? 0, fault?.address ?? 0, fault?.access ?? 0, fault?.length ?? 0], fn.exitVersion), 56);
  if (helper) expected.set(record('R3MH', 40, helper.fields, helper.version), 100);
  assert.equal(fn(c.base, c.base + 56, budget, c.base + 96), 0);
  assert.deepEqual(arena(c), expected, 'complete CPU, exit, helper and unrelated numeric-state preservation'); counts.runs++;
}
const FORMS = ['andn', 'bextr', 'blsi', 'blsmsk', 'blsr'];
const VALUES = [0, 1, 2, 0x80000000, 0xffffffff, 0x12345678, 0x80018001, 0xaaaa5555];
const CONTROLS = [0, 0x0100, 0x011f, 0x2000, 0x2020, 0x21ff, 0xffff, 0xabcd0810];
const MASK = 0xffffffffn;
function encoded(form, destination, source, control, memory, w) {
  const unary = form.startsWith('bls');
  const opcode = form === 'andn' ? 0xf2 : form === 'bextr' ? 0xf7 : 0xf3;
  const group = form === 'blsi' ? 3 : form === 'blsmsk' ? 2 : 1;
  return [0xc4, 0xe2, (Number(w) << 7) | ((~(unary ? destination : control) & 15) << 3), opcode, (memory ? 0 : 0xc0) | ((unary ? group : destination) << 3) | source];
}
function result(form, source, control, flags) {
  const src = BigInt(source >>> 0), ctl = BigInt(control >>> 0);
  let value;
  if (form === 'andn') value = (~ctl & src) & MASK;
  else if (form === 'bextr') {
    const start = Number(ctl & 255n), length = Number((ctl >> 8n) & 255n);
    value = start >= 32 ? 0n : (src >> BigInt(start)) & ((1n << BigInt(length)) - 1n);
  } else if (form === 'blsi') value = (-src & src) & MASK;
  else if (form === 'blsmsk') value = ((src - 1n) ^ src) & MASK;
  else value = ((src - 1n) & src) & MASK;
  let nextFlags = flags & ~0x8d5;
  if (form !== 'blsmsk' && value === 0n) nextFlags |= 0x40;
  if (form !== 'bextr' && (value & 0x80000000n)) nextFlags |= 0x80;
  if ((form === 'blsi' && src !== 0n) || ((form === 'blsmsk' || form === 'blsr') && src === 0n)) nextFlags |= 1;
  return {value: Number(value), flags: nextFlags};
}
assert.deepEqual(result('blsi', 0, 0, 0xcd7), {value: 0, flags: 0x442});
assert.deepEqual(result('blsi', 0x80000000, 0, 0xcd7), {value: 0x80000000, flags: 0x483});
assert.deepEqual(result('blsmsk', 0, 0, 0xcd7), {value: 0xffffffff, flags: 0x483});
assert.deepEqual(result('bextr', 0xffffffff, 0x2000, 0xcd7), {value: 0xffffffff, flags: 0x402});
assert.deepEqual(result('bextr', 0xffffffff, 0xff20, 0xcd7), {value: 0, flags: 0x442});
assert.deepEqual(result('bextr', 0xffffffff, 0x021f, 0xcd7), {value: 1, flags: 0x402});
function initial(c, source, control, value, ctl, memory, address) {
  const before = seed(c, 0, address);
  if (!memory) before.registers[source] = value >>> 0;
  if (control !== null) before.registers[control] = ctl >>> 0;
  input(c, 0, cpuRecord(before));
  return before;
}
function expected(before, form, destination, source, control, memoryValue, length) {
  const oracle = result(form, memoryValue ?? before.registers[source], control === null ? 0 : before.registers[control], before.flags);
  const next = {...before, registers: [...before.registers], pc: PC + length, flags: oracle.flags};
  next.registers[destination] = oracle.value;
  return next;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const form of FORMS) {
    const ternary = form === 'andn' || form === 'bextr';
    for (let destination = 0; destination < 8; destination++) for (const w of [false, true]) {
      const aliases = ternary ? [[2, 1], [destination, 1], [2, destination], [destination, destination]] : [[2, null], [destination, null]];
      for (const [source, control] of aliases) {
        const instruction = encoded(form, destination, source, control ?? 0, false, w);
        const fn = compile(c, instruction, null);
        const values = ternary ? VALUES : [...VALUES, ...Array.from({length: 32}, (_, bit) => (2 ** bit) >>> 0)];
        for (let i = 0; i < values.length; i++) {
          const before = initial(c, source, control, values[i], CONTROLS[i % CONTROLS.length], false, DATA + 7);
          run(c, fn, expected(before, form, destination, source, control, undefined, instruction.length)); counts.register++;
        }
        const stopped = seed(c); run(c, fn, stopped, {budget: 0, retired: 0}); input(c, 96, words([1])); run(c, fn, stopped, {reason: 2, retired: 0}); counts.controls += 2;
      }
      const source = 7, control = ternary ? destination : null;
      const instruction = encoded(form, destination, source, control ?? 0, true, w), fn = compile(c, instruction, 'cpu_read32');
      for (let i = 0; i < VALUES.length; i++) {
        const address = i % 2 ? DATA + 4095 : DATA + 7, value = VALUES[i];
        upload(c, address, words([value]));
        const before = initial(c, source, control, 0, CONTROLS[i], true, address);
        // A control register may alias the memory base. Its actual value defines both operands.
        if (control === source) {
          before.registers[source] = address; input(c, 0, cpuRecord(before));
        }
        run(c, fn, expected(before, form, destination, source, control, value, instruction.length), {helper: {version: 10, fields: [0, value, 0, 0, 0, 4]}}); counts.memory++;
      }
      const address = 0x6fff, before = initial(c, source, control, 0, address, true, address);
      before.registers[source] = address; input(c, 0, cpuRecord(before));
      upload(c, address, Buffer.from([0xaa]));
      run(c, fn, before, {reason: 5, retired: 0, fault: {detail: 1, address: 0x7000, access: 1, length: 4}, helper: {version: 10, fields: [1, 0, 1, 0x7000, 1, 4]}}); counts.faults++;
      assert.equal(c.api.map(0x7000, 1, 3), 0); upload(c, address, words([0x80018001]));
      run(c, fn, expected(before, form, destination, source, control, 0x80018001, instruction.length), {helper: {version: 10, fields: [0, 0x80018001, 0, 0, 0, 4]}}); counts.repairs++;
      assert.equal(c.api.unmap(0x7000, 1), 0);
      const stopped = seed(c, 0, 0x8000); run(c, fn, stopped, {budget: 0, retired: 0}); input(c, 96, words([1])); run(c, fn, stopped, {reason: 2, retired: 0}); counts.controls += 2;
    }
    const instruction = [0x67, ...encoded(form, 0, 7, 1, true, false)];
    const fn = compile(c, instruction, 'cpu_read32');
    for (const value of [0, 0x80000000, 0xffffffff]) {
      upload(c, DATA + 7, words([value]));
      const before = seed(c); before.registers[3] = 0x12345007; before.registers[1] = 0x2000;
      input(c, 0, cpuRecord(before));
      run(c, fn, expected(before, form, 0, 3, ternary ? 1 : null, value, instruction.length), {helper: {version: 10, fields: [0, value, 0, 0, 0, 4]}}); counts.memory++;
    }
    const stopped = seed(c); run(c, fn, stopped, {budget: 0, retired: 0}); input(c, 96, words([1])); run(c, fn, stopped, {reason: 2, retired: 0}); counts.controls += 2;
  }
  assert.equal(c.api.close(), 0);
}
assert.equal(counts.contexts, 4); assert.equal(counts.modules, 1236); assert.equal(counts.register, 19456); assert.equal(counts.memory, 2620); assert.equal(counts.faults, 320); assert.equal(counts.repairs, 320); assert.equal(counts.controls, 2472);
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed during execution');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, limits: ['finite 32-bit BMI1 semantics on actual generated Wasm; no universal ISA, hardware, browser or throughput claim']}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
