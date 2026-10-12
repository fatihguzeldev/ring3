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
  'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/abi/x86/state.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/segments.rs', 'engine/src/cpu/dbt/wasm/segments/access.rs', 'engine/src/cpu/x86/segments/cache.rs',
  'engine/tests/cpu/bits_flags/integration/fixtures/p2-scalar-bits/runtime/crc32.mjs', 'engine/tests/cpu/bits_flags/integration/wasm/scalar.rs',
];
const sources = () => Object.fromEntries(sourcePaths.map(path => [path, createHash('sha256').update(readFileSync(join(root, path))).digest('hex')]));
const sourcePins = sources();
const counts = {contexts: 0, modules: 0, runs: 0, register: 0, memory: 0, faults: 0, repairs: 0, permissions: 0, controls: 0, chains: 0, exhaustive: 0};
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((value, i) => b.writeUInt32LE(value >>> 0, 16 + i * 4)); return b;
}
const cpuRecord = s => record('R3ST', 56, [...s.registers, s.pc, s.flags], s.flags & ~0xcd7 ? 2 : 1);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, bytes) {new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);}
function upload(c, address, bytes) {input(c, 140, bytes); assert.equal(c.api.upload(address, bytes.length), 0);}
function seed(c, source = 0, address = DATA + 7) {
  const registers = Array.from({length: 8}, (_, i) => (0xa5101234 + i * 0x10001) >>> 0);
  registers[0] = 0xa5c31234; registers[2] = source >>> 0; registers[7] = address >>> 0;
  const state = {registers, pc: PC, flags: 0xcd7}; input(c, 0, cpuRecord(state)); input(c, 96, words([0])); return state;
}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, api: {}, memory: instance.exports.memory};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'discard_unacknowledged_resident', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident', 'read16', 'read32', 'store16', 'store32', 'store_resident16', 'store_resident32']) c.api[name] = instance.exports['ring3_abi_v1_' + name];
  c.keyLow = ++counts.contexts;
  assert.equal(c.api.open(8, c.keyLow, 0x43524332), 0); c.base = c.api.arena_ptr() >>> 0;
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xfffff000, 1, 3]]) assert.equal(c.api.map(...args), 0);
  return c;
}
function compile(c, instruction, memoryName) {
  if (c.previousResident) {
    input(c, 96, words([0]));
    assert.equal(c.api.discard_unacknowledged_resident(c.keyLow, 0x43524332, ...c.previousResident), 0);
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
  run.exitVersion = memoryName ? 2 : 1;
  return run;
}
function run(c, fn, state, {budget = 1, reason = 1, retired = 1, helper, fault, exitVersion = fn.exitVersion} = {}) {
  const expected = arena(c); expected.set(cpuRecord(state));
  expected.set(record('R3EX', 40, [reason, retired, fault?.detail ?? 0, fault?.address ?? 0, fault?.access ?? 0, fault?.length ?? 0], exitVersion), 56);
  if (helper) expected.set(record('R3MH', 40, helper.fields, helper.version), 100);
  assert.equal(fn(c.base, c.base + 56, budget, c.base + 96), 0);
  assert.deepEqual(arena(c), expected, 'complete CPU, exit, helper and unrelated numeric-state preservation'); counts.runs++;
}

const VALUES = [0, 1, 2, 0x7fffffff, 0x80000000, 0xffffffff, 0x12345678, 0xdeadbeef];
const SEEDS = [0, 0xffffffff, 0x12345678, 0x80000000];
const FLAGS = [2, 0xcd7, 0x4002, 0x40002, 0x200002, 0x244cd7];
function reflected(value, width) {
  let result = 0n;
  for (let bit = 0; bit < width; bit++) if (value & (1n << BigInt(bit))) result |= 1n << BigInt(width - 1 - bit);
  return result;
}
function crc32(initial, source, width) {
  // Intel's normal polynomial long division is independent of the emitted reflected update.
  const mask = (1n << BigInt(width)) - 1n;
  let dividend = (reflected(BigInt(source >>> 0) & mask, width) << 32n) ^ (reflected(BigInt(initial >>> 0), 32) << BigInt(width));
  for (let degree = width + 31; degree >= 32; degree--) if (dividend & (1n << BigInt(degree))) dividend ^= 0x11edc6f41n << BigInt(degree - 32);
  return Number(reflected(dividend, 32));
}
assert.equal(crc32(0, 0, 8), 0);
assert.equal(crc32(0, 1, 8), 0xf26b8303);
assert.equal(crc32(0xffffffff, 0xffffffff, 32), 0);
let standard = 0xffffffff;
for (const byte of Buffer.from('123456789')) standard = crc32(standard, byte, 8);
assert.equal((standard ^ 0xffffffff) >>> 0, 0xe3069283);

function encoded(width, destination, source, memory = false, reverseWord = false) {
  const prefixes = width === 16 ? (reverseWord ? [0xf2, 0x66] : [0x66, 0xf2]) : [0xf2];
  return [...prefixes, 0x0f, 0x38, width === 8 ? 0xf0 : 0xf1, (memory ? 0 : 0xc0) | (destination << 3) | source];
}
function sourceValue(state, width, source) {
  if (width === 8) return (state.registers[source & 3] >>> (source >= 4 ? 8 : 0)) & 255;
  return width === 16 ? state.registers[source] & 0xffff : state.registers[source];
}
function initial(c, width, destination, source, value, crc, flags, address) {
  const before = seed(c);
  before.registers[destination] = crc >>> 0;
  if (address === undefined) {
    if (width === 8) {
      const parent = source & 3, shift = source >= 4 ? 8 : 0;
      before.registers[parent] = ((before.registers[parent] & ~(255 << shift)) | ((value & 255) << shift)) >>> 0;
    } else if (width === 16) before.registers[source] = ((before.registers[source] & 0xffff0000) | (value & 0xffff)) >>> 0;
    else before.registers[source] = value >>> 0;
  } else before.registers[7] = address >>> 0;
  before.flags = flags; input(c, 0, cpuRecord(before)); return before;
}
function expected(before, width, destination, source, length, memoryValue) {
  const next = {...before, registers: [...before.registers], pc: PC + length};
  next.registers[destination] = crc32(before.registers[destination], memoryValue ?? sourceValue(before, width, source), width);
  return next;
}
function bytes(value, width) {const b = words([value]); return b.subarray(0, width / 8);}
function helper(width, value = 0, detail = 0, address = 0) {
  return {version: width === 32 ? 1 : 2, fields: [Number(detail !== 0), value, detail, address, detail ? 1 : 0, detail || width !== 32 ? width / 8 : 0]};
}
function stopped(c, fn) {
  const before = seed(c, 0, 0x8000); run(c, fn, before, {budget: 0, retired: 0}); input(c, 96, words([1])); run(c, fn, before, {reason: 2, retired: 0}); counts.controls += 2;
}
function readBytes(c, address, length) {
  const b = Buffer.alloc(length);
  for (let i = 0; i < length; i++) {assert.equal(c.api.read8((address + i) >>> 0), 0); b[i] = arena(c).readUInt32LE(120);}
  return b;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const width of [8, 16, 32]) {
    for (const destination of [0, 2, 7]) for (const source of width === 8 ? [0, 2, 4, 6, 7] : [0, 2, 7]) {
      const instruction = encoded(width, destination, source), fn = compile(c, instruction, null);
      for (const [index, value] of VALUES.entries()) for (const crc of SEEDS) {
        const before = initial(c, width, destination, source, value, crc, FLAGS[index % FLAGS.length]);
        run(c, fn, expected(before, width, destination, source, instruction.length)); counts.register++;
      }
      if (width === 8 && ((destination === 0 && source === 4) || (destination === 7 && source === 0))) {
        for (let value = 0; value < 256; value++) for (const crc of [0, 0xffffffff]) {
          const before = initial(c, width, destination, source, value, crc, 0xcd7);
          run(c, fn, expected(before, width, destination, source, instruction.length)); counts.register++; counts.exhaustive++;
        }
      }
      stopped(c, fn);
    }
    if (width === 16) {
      const instruction = encoded(width, 6, 6, false, true), fn = compile(c, instruction, null);
      for (const value of VALUES) {
        const before = initial(c, width, 6, 6, value, 0x12345678, 0x244cd7);
        run(c, fn, expected(before, width, 6, 6, instruction.length)); counts.register++;
      }
      stopped(c, fn);
    }
    for (const destination of [0, 7]) {
      const instruction = encoded(width, destination, 7, true), fn = compile(c, instruction, `read${width}`);
      for (const [index, value] of VALUES.entries()) {
        const address = index % 2 ? DATA + 4095 : 0x7000 - width / 8, operand = bytes(value, width), source = Number(BigInt(value) & ((1n << BigInt(width)) - 1n));
        upload(c, address, operand);
        const before = initial(c, width, destination, 7, 0, SEEDS[index % SEEDS.length], 0xcd7, address);
        run(c, fn, expected(before, width, destination, 7, instruction.length, source), {helper: helper(width, source)}); counts.memory++;
        assert.deepEqual(readBytes(c, address, width / 8), operand, 'CRC32 is read-only');
      }
      const address = width === 8 ? 0x7000 : 0x6fff, faultAddress = 0x7000;
      const before = initial(c, width, destination, 7, 0, 0xdeadbeef, 0xcd7, address);
      if (width !== 8) upload(c, address, Buffer.from([0xaa]));
      run(c, fn, before, {reason: 5, retired: 0, fault: {detail: 1, address: faultAddress, access: 1, length: width / 8}, helper: helper(width, 0, 1, faultAddress)}); counts.faults++;
      assert.equal(c.api.map(0x7000, 1, 3), 0); upload(c, address, bytes(0x80018001, width));
      const repaired = Number(0x80018001n & ((1n << BigInt(width)) - 1n));
      run(c, fn, expected(before, width, destination, 7, instruction.length, repaired), {helper: helper(width, repaired)}); counts.repairs++;
      assert.equal(c.api.unmap(0x7000, 1), 0);
      const protectedAddress = DATA + 7, protectedBefore = initial(c, width, destination, 7, 0, 0x12345678, 0xcd7, protectedAddress);
      upload(c, protectedAddress, bytes(0xffffffff, width)); assert.equal(c.api.protect(DATA, 1, 2), 0);
      run(c, fn, protectedBefore, {reason: 5, retired: 0, fault: {detail: 2, address: protectedAddress, access: 1, length: width / 8}, helper: helper(width, 0, 2, protectedAddress)}); counts.faults++; counts.permissions++;
      assert.equal(c.api.protect(DATA, 1, 3), 0);
      const allBits = Number((1n << BigInt(width)) - 1n);
      run(c, fn, expected(protectedBefore, width, destination, 7, instruction.length, allBits), {helper: helper(width, allBits)}); counts.repairs++;
      if (width !== 8) {
        const wrapped = initial(c, width, destination, 7, 0, 0x12345678, 0xcd7, 0xffffffff);
        // this profile rejects an overflowing logical segment span before calling the ram helper.
        run(c, fn, wrapped, {reason: 11, retired: 0, exitVersion: 6}); counts.faults++;
      }
      stopped(c, fn);
    }
  }
  const first = encoded(16, 0, 2), second = encoded(32, 1, 0), fn = compile(c, [...first, ...second], null);
  for (const flags of FLAGS) {
    const before = initial(c, 16, 0, 2, 0xbeef, 0x12345678, flags);
    const middle = expected(before, 16, 0, 2, first.length), next = expected(middle, 32, 1, 0, first.length + second.length);
    run(c, fn, next, {budget: 2, retired: 2}); counts.chains++;
  }
  assert.equal(c.api.close(), 0);
}
assert.equal(counts.contexts, 4); assert.equal(counts.modules, 164); assert.equal(counts.register, 8352); assert.equal(counts.memory, 192); assert.equal(counts.faults, 64); assert.equal(counts.repairs, 48); assert.equal(counts.permissions, 24); assert.equal(counts.controls, 320); assert.equal(counts.chains, 24); assert.equal(counts.exhaustive, 4096); assert.equal(counts.runs, 9000);
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed during execution');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, limits: ['finite IA-32 CRC32C semantics and ABI flags preservation on actual generated Wasm; no complete SSE4.2, hardware, browser or throughput claim']}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
