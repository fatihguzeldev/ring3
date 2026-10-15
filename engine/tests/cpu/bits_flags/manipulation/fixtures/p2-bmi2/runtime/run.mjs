import {pageFaultExit, pageFaultPacket} from '../../../../../../support/wasm/page-fault.mjs';
import {generatedImports, attachDetectionApi} from '../../../../../../support/wasm/binding.mjs';
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
  'engine/tests/cpu/bits_flags/manipulation/fixtures/p2-bmi2/runtime/run.mjs', 'engine/tests/support/wasm/binding.mjs', 'engine/tests/support/wasm/page-fault.mjs', 'engine/tests/cpu/bits_flags/manipulation/wasm/bmi2.rs',
];
sourcePaths.push(...["engine/src/abi/memory_helper/cpu.rs", "engine/src/cpu/dbt/wasm/memory/cpu_scalar.rs", "engine/src/cpu/dbt/wasm/emitter.rs", "engine/src/cpu/dbt/wasm/code_fetch.rs", "engine/src/process/memory/cpu_scalar.rs", "engine/src/process/memory/scalar/cpu_write.rs", "engine/src/process/wasm/memory/cpu_write.rs", "engine/src/abi/wasm/exports/memory/cpu_write.rs", "engine/tests/support/wasm/arena.mjs"].filter(path=>!sourcePaths.includes(path)));
const sources = () => Object.fromEntries(sourcePaths.map(path => [path, createHash('sha256').update(readFileSync(join(root, path))).digest('hex')]));
const sourcePins = sources();
const counts = {contexts: 0, modules: 0, runs: 0, register: 0, memory: 0, faults: 0, repairs: 0, permissions: 0, controls: 0};
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
 attachDetectionApi(c.api, instance.exports);
  c.keyLow = ++counts.contexts;
  for(const name of ["read16", "read32", "read8", "store16", "store32", "store_resident16", "store_resident32"]) {c.api["cpu_"+name]=instance.exports["ring3_abi_v1_cpu_"+name];assert.equal(c.api["cpu_"+name]?.length,name.startsWith("read")?1:name.includes("resident")?6:2,"cpu_"+name);}
 assert.equal(c.api.open(8, c.keyLow, 0x424d4932), 0); c.base = c.api.arena_ptr() >>> 0;
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xfffff000, 1, 3]]) assert.equal(c.api.map(...args), 0);
  return c;
}
function compile(c, instruction, memoryName) {
  if (c.previousResident) {
    input(c, 96, words([0]));
    assert.equal(c.api.discard_unacknowledged_resident(c.keyLow, 0x424d4932, ...c.previousResident), 0);
  }
  upload(c, PC, Buffer.from([...instruction, 0xeb, 0]));
  input(c, 140, words(c.entries ? [PC] : [PC, instruction.length + 2]));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  assert.equal(c.api[method](1, ...(c.entries ? [0] : [])), 0);
  const b = arena(c), pointer = c.owner === 'resident' ? b.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  if (c.owner === 'resident') c.previousResident = [b.readUInt32LE(148), b.readUInt32LE(152)];
  const length = c.owner === 'resident' ? b.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
  const expected = generatedImports({bound: true, resident: c.owner === 'resident', helpers: memoryName ? [memoryName] : []});
  assert.deepEqual(WebAssembly.Module.imports(module), expected, 'register forms have no memory helper import; each memory form has its one required helper');
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), bytes, {flag: 'wx'});
  const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run;
  run.exitVersion = memoryName ? 6 : 1;
  return run;
}
function run(c, fn, state, {budget = 1, reason = 1, retired = 1, helper, fault} = {}) {
  const expected = arena(c); expected.set(cpuRecord(state));
  expected.set(reason === 5 ? pageFaultExit({retired, errorCode: 4 | (fault.detail === 2 ? 1 : 0), address: fault.address, access: 1, length: fault.length}) : record('R3EX', 40, [reason, retired, fault?.detail ?? 0, fault?.address ?? 0, fault?.access ?? 0, fault?.length ?? 0], fn.exitVersion), 56);
  if (helper) expected.set(reason === 5 ? pageFaultPacket({errorCode: 4 | (fault.detail === 2 ? 1 : 0), address: fault.address, access: 1, length: fault.length}) : record('R3MH', 40, helper.fields, helper.version), 100);
  assert.equal(fn(c.base, c.base + 56, budget, c.base + 96), 0);
  assert.deepEqual(arena(c), expected, 'complete CPU, exit, helper and unrelated numeric-state preservation'); counts.runs++;
}
const FORMS = ['bzhi', 'pdep', 'pext', 'mulx', 'sarx', 'shlx', 'shrx', 'rorx'];
const VALUES = [0, 1, 2, 0x7fffffff, 0x80000000, 0xffffffff, 0xffff0001, 0x12345678];
const CONTROLS = [0, 1, 31, 32, 63, 64, 255, 0xffffffff];
const MASK = 0xffffffffn;
function encoded(form, destination, source, control, memory, w = false) {
  const pp = {bzhi: 0, pdep: 3, pext: 2, mulx: 3, sarx: 2, shlx: 1, shrx: 3, rorx: 3}[form];
  const opcode = ['bzhi', 'pdep', 'pext'].includes(form) ? 0xf5 : form === 'mulx' ? 0xf6 : form === 'rorx' ? 0xf0 : 0xf7;
  const bytes = [0xc4, form === 'rorx' ? 0xe3 : 0xe2, (Number(w) << 7) | ((~(form === 'rorx' ? 0 : control) & 15) << 3) | pp, opcode, (memory ? 0 : 0xc0) | (destination << 3) | source];
  if (form === 'rorx') bytes.push(control);
  return bytes;
}
function parallel(value, mask, deposit) {
  let result = 0n, compressed = 0n;
  for (let bit = 0n; bit < 32n; bit++) {
    if ((mask & (1n << bit)) === 0n) continue;
    const from = deposit ? compressed : bit, to = deposit ? bit : compressed;
    if (value & (1n << from)) result |= 1n << to;
    compressed++;
  }
  return result;
}
function result(form, source, control, flags) {
  const src = BigInt(source >>> 0), ctl = BigInt(control >>> 0), count = ctl & 31n;
  let value, nextFlags = flags;
  if (form === 'bzhi') {
    const index = Number(ctl & 255n);
    value = index >= 32 ? src : src & ((1n << BigInt(index)) - 1n);
    nextFlags = (flags & ~0x8d5) | Number(index >= 32) | (value === 0n ? 0x40 : 0) | ((value & 0x80000000n) ? 0x80 : 0);
  } else if (form === 'pdep' || form === 'pext') value = parallel(ctl, src, form === 'pdep');
  else if (form === 'shlx') value = (src << count) & MASK;
  else if (form === 'shrx') value = src >> count;
  else if (form === 'sarx') value = (BigInt.asIntN(32, src) >> count) & MASK;
  else if (form === 'rorx') value = ((src >> count) | (src << (32n - count))) & MASK;
  else throw new Error('MULX has a separate two-destination oracle');
  return {value: Number(value), flags: nextFlags};
}
assert.deepEqual(result('bzhi', 0xffffffff, 31, 0xcd7), {value: 0x7fffffff, flags: 0x402});
assert.deepEqual(result('bzhi', 0xffffffff, 32, 0xcd7), {value: 0xffffffff, flags: 0x483});
assert.deepEqual(result('bzhi', 0xffffffff, 256, 0xcd7), {value: 0, flags: 0x442});
assert.deepEqual(result('pdep', 0x80000001, 3, 0xcd7), {value: 0x80000001, flags: 0xcd7});
assert.deepEqual(result('pext', 0x80000001, 0x80000000, 0xcd7), {value: 2, flags: 0xcd7});
assert.deepEqual(result('sarx', 0x80000000, 31, 0xcd7), {value: 0xffffffff, flags: 0xcd7});
assert.deepEqual(result('rorx', 1, 255, 0xcd7), {value: 2, flags: 0xcd7});
function initial(c, form, source, control, value, ctl, memory, address = DATA + 7) {
  const before = seed(c, 0, address);
  if (form === 'mulx') before.registers[2] = ctl >>> 0;
  if (!memory) before.registers[source] = value >>> 0;
  if (form !== 'rorx' && form !== 'mulx') before.registers[control] = ctl >>> 0;
  if (memory) before.registers[7] = address;
  input(c, 0, cpuRecord(before));
  return before;
}
function expected(before, form, destination, source, control, memoryValue, length) {
  const next = {...before, registers: [...before.registers], pc: PC + length};
  const value = memoryValue ?? before.registers[source];
  if (form === 'mulx') {
    const product = BigInt(before.registers[2]) * BigInt(value);
    next.registers[control] = Number(product & MASK);
    next.registers[destination] = Number(product >> 32n);
  } else {
    const oracle = result(form, value, form === 'rorx' ? control : before.registers[control], before.flags);
    next.registers[destination] = oracle.value; next.flags = oracle.flags;
  }
  return next;
}
function vectors(form) {
  if (form === 'bzhi') return [0, 0xffffffff, 0x80000000].flatMap(value => [0, 1, 15, 31, 32, 33, 63, 127, 255, 0xffffff00, 0xffffff20, 256].map(control => [value, control]));
  if (form === 'pdep' || form === 'pext') return [0, 0xffffffff, 0x80000001, 0xaaaa5555, ...Array.from({length: 32}, (_, bit) => (2 ** bit) >>> 0)].flatMap(mask => [0, 1, 0xffffffff, 0xa5a55a5a].map(value => [mask, value]));
  if (form === 'mulx') return VALUES.flatMap(value => VALUES.map(control => [value, control]));
  return VALUES.map((value, i) => [value, CONTROLS[i]]);
}
function stopped(c, fn) {
  const before = seed(c, 0, 0x8000); run(c, fn, before, {budget: 0, retired: 0}); input(c, 96, words([1])); run(c, fn, before, {reason: 2, retired: 0}); counts.controls += 2;
}
function memoryFault(c, fn, before, address, reason, faultAddress) {
  run(c, fn, before, {reason: 5, retired: 0, fault: {detail: reason, address: faultAddress, access: 1, length: 4}, helper: {version: 10, fields: [1, 0, reason, faultAddress, 1, 4]}}); counts.faults++;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const form of FORMS) {
    const registerCases = [];
    if (form === 'rorx') {
      for (const destination of [0, 2]) for (const source of [3, destination]) for (const count of [0, 1, 31, 32, 255]) registerCases.push([destination, source, count, false]);
      registerCases.push([0, 3, 31, true]);
    } else {
      for (const destination of [0, 2, 7]) {
        for (const [source, control] of [[3, 1], [destination, 1], [3, destination], [destination, destination]]) registerCases.push([destination, source, control, false]);
        if (form === 'mulx') registerCases.push([destination, 2, 2, false]);
      }
      registerCases.push([0, 3, 1, true]);
    }
    for (const [destination, source, control, w] of registerCases) {
      const instruction = encoded(form, destination, source, control, false, w), fn = compile(c, instruction, null);
      for (const [value, ctl] of vectors(form)) {
        const before = initial(c, form, source, control, value, ctl, false);
        run(c, fn, expected(before, form, destination, source, control, undefined, instruction.length)); counts.register++;
      }
      stopped(c, fn);
    }
    for (const [destination, control, w] of [[0, form === 'rorx' ? 31 : 1, false], [7, form === 'rorx' ? 255 : 7, false], [0, form === 'rorx' ? 32 : form === 'mulx' ? 0 : 1, true]]) {
      const source = 7, instruction = encoded(form, destination, source, control, true, w), fn = compile(c, instruction, 'cpu_read32');
      for (let i = 0; i < VALUES.length; i++) {
        const address = i % 2 ? DATA + 4095 : DATA + 7, value = VALUES[i]; upload(c, address, words([value]));
        const before = initial(c, form, source, control, 0, CONTROLS[i], true, address);
        run(c, fn, expected(before, form, destination, source, control, value, instruction.length), {helper: {version: 10, fields: [0, value, 0, 0, 0, 4]}}); counts.memory++;
      }
      const address = 0x6fff, before = initial(c, form, source, control, 0, 33, true, address);
      upload(c, address, Buffer.from([0xaa])); memoryFault(c, fn, before, address, 1, 0x7000);
      assert.equal(c.api.map(0x7000, 1, 3), 0); upload(c, address, words([0x80018001]));
      run(c, fn, expected(before, form, destination, source, control, 0x80018001, instruction.length), {helper: {version: 10, fields: [0, 0x80018001, 0, 0, 0, 4]}}); counts.repairs++;
      assert.equal(c.api.unmap(0x7000, 1), 0);
      if (!w && destination === 0) {
        const protectedAddress = DATA + 7, protectedBefore = initial(c, form, source, control, 0, 31, true, protectedAddress);
        upload(c, protectedAddress, words([0xffffffff])); assert.equal(c.api.protect(DATA, 1, 2), 0);
        memoryFault(c, fn, protectedBefore, protectedAddress, 2, protectedAddress); counts.permissions++;
        assert.equal(c.api.protect(DATA, 1, 3), 0);
        run(c, fn, expected(protectedBefore, form, destination, source, control, 0xffffffff, instruction.length), {helper: {version: 10, fields: [0, 0xffffffff, 0, 0, 0, 4]}}); counts.repairs++;
      }
      stopped(c, fn);
    }
  }
  assert.equal(c.api.close(), 0);
}
assert.equal(counts.contexts, 4); assert.equal(counts.modules, 556); assert.equal(counts.register, 22864); assert.equal(counts.memory, 768); assert.equal(counts.faults, 128); assert.equal(counts.repairs, 128); assert.equal(counts.permissions, 32); assert.equal(counts.controls, 1112);
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed during execution');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, limits: ['finite 32-bit BMI2 semantics on actual generated Wasm; no universal ISA, hardware, browser or throughput claim']}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
