import {generatedImports, attachDetectionApi} from '../../../../../../../support/wasm/binding.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const engine = readEngine(enginePath), PC = 0x1000, DATA = 0x5000, SIZE = 4612;
const sourcePaths = [
  'engine/src/cpu/x86/decode/cache_timestamp.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/ir.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/cache_flush.rs', 'engine/src/cpu/dbt/wasm/integer_system.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/atomic.rs', 'engine/src/cpu/dbt/wasm/cpu_result.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/cpu/dbt/wasm/segments.rs', 'engine/src/process/cpu/integer.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs', 'engine/src/abi/cpu_helper.rs',
  'engine/tests/cpu/profile/environment/ordering/native.rs', 'engine/tests/cpu/profile/environment/ordering/wasm.rs', 'engine/tests/cpu/profile/environment/ordering/fixtures/p2-cache-timestamp/runtime/run.mjs', 'engine/tests/support/wasm/binding.mjs',
];
const sources = () => Object.fromEntries(sourcePaths.map(path => [path, createHash('sha256').update(readFileSync(join(root, path))).digest('hex')]));
const sourcePins = sources();
const counts = {contexts: 0, modules: 0, runs: 0, flushes: 0, timestamps: 0, faults: 0, repairs: 0, controls: 0, ordering: 0};
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
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, api: {}, memory: instance.exports.memory, calls: [], tsc: 0};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
 attachDetectionApi(c.api, instance.exports);
  c.keyLow = ++counts.contexts;
  assert.equal(c.api.open(8, c.keyLow, 0x43545343), 0); c.base = c.api.arena_ptr() >>> 0;
  assert.equal(c.api.configure_cpu_clock(1_000_000_000, 0, 0, 0), 0);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xfffff000, 1, 3]]) assert.equal(c.api.map(...args), 0);
  return c;
}
function compile(c, instruction, helpers) {
  if (c.previousResident) {
    input(c, 96, words([0]));
    assert.equal(c.api.discard_unacknowledged_resident(c.keyLow, 0x43545343, ...c.previousResident), 0);
  }
  upload(c, PC, Buffer.from([...instruction, 0xeb, 0]));
  input(c, 140, words(c.entries ? [PC] : [PC, instruction.length + 2]));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  assert.equal(c.api[method](1, ...(c.entries ? [0] : [])), 0);
  const b = arena(c), pointer = c.owner === 'resident' ? b.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  if (c.owner === 'resident') c.previousResident = [b.readUInt32LE(148), b.readUInt32LE(152)];
  c.binding = c.owner === 'resident' ? [c.keyLow, 0x43545343, ...c.previousResident] : [];
  const length = c.owner === 'resident' ? b.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
  const bound = name => c.owner === 'resident' ? (name === 'cpu_integer' ? 'cpu_integer_resident' : name === 'store32' ? 'store_resident32' : name) : name;
  const imports = WebAssembly.Module.imports(module);
  assert.deepEqual(imports.map(row => `${row.module}:${row.name}:${row.kind}`).sort(), generatedImports({resident: c.owner === 'resident', helpers: [...helpers.map((name) => bound(name))]}).map(item => `${item.module}:${item.name}:${item.kind}`).sort());
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), bytes, {flag: 'wx'});
  const ring3 = {...c.api};
  for (const name of helpers.map(bound)) ring3[name] = (...args) => {
    const before = arena(c), status = c.api[name](...args), after = arena(c);
    const allowed = Buffer.from(before); allowed.set(after.subarray(100, 140), 100);
    if (name.startsWith('cpu_integer')) allowed.set(after.subarray(140, 164), 140);
    assert.deepEqual(after, allowed, 'helper changes only its result packet and timestamp payload');
    c.calls.push({name, args: args.map(value => value >>> 0), status, input: Array.from({length: 6}, (_, index) => before.readUInt32LE(140 + index * 4))}); return status;
  };
  const fn = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run;
  fn.exitVersion = helpers.includes('cpu_integer') ? 6 : 2; return fn;
}
function run(c, fn, state, {budget = 1, reason = 1, retired = 1, helper, payload, fault, calls = []} = {}) {
  const expected = arena(c), start = c.calls.length; expected.set(cpuRecord(state));
  const exitVersion = reason === 11 ? 6 : fn.exitVersion;
  expected.set(record('R3EX', 40, [reason, retired, fault?.detail ?? 0, fault?.address ?? 0, fault?.access ?? 0, fault?.length ?? 0], exitVersion), 56);
  if (helper) expected.set(record('R3MH', 40, helper.fields, helper.version), 100);
  if (payload) expected.set(words(payload), 140);
  assert.equal(fn(c.base, c.base + 56, budget, c.base + 96), 0);
  assert.deepEqual(arena(c), expected, 'complete CPU, exit, helper, payload and unrelated arena preservation');
  const observed = c.calls.slice(start); assert.deepEqual(observed.map(row => row.name.replace('_resident', '')), calls);
  assert.ok(observed.every(row => row.status === 0)); counts.runs++;
}
const successByte = value => ({version: 2, fields: [0, value, 0, 0, 0, 1]});
const faultByte = (detail, address) => ({version: 2, fields: [1, 0, detail, address, 1, 1]});
const timestampPacket = {version: 7, fields: [0, 0, 0, 0, 0, 0]};
function next(before, length) {return {...before, registers: [...before.registers], pc: PC + length};}
function stopped(c, fn) {
  const before = seed(c, 0, 0x8000); run(c, fn, before, {budget: 0, retired: 0}); input(c, 96, words([1])); run(c, fn, before, {reason: 2, retired: 0}); counts.controls += 2;
}
function timestamp(c, fn, before, pc, auxiliary, {budget = 1, retired = 1, prefixCalls = [], payloadBefore = before} = {}) {
  assert.equal(c.api.observe_cpu_clock(c.tsc, 0), 0);
  const state = {...before, registers: [...before.registers], pc}; state.registers[0] = c.tsc; state.registers[2] = 0; if (auxiliary) state.registers[1] = 0;
  const payload = [arena(c).readUInt32LE(140), payloadBefore.flags, state.registers[0], state.registers[2], payloadBefore.registers[3], auxiliary ? 0 : payloadBefore.registers[1]];
  run(c, fn, state, {budget, retired, helper: timestampPacket, payload, calls: [...prefixCalls, 'cpu_integer']});
  assert.equal(c.calls.at(-1).args.at(-2), auxiliary ? 0x20001 : 0x20000); counts.timestamps++; return state;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), flush = [0x0f, 0xae, 0x3f], fn = compile(c, flush, ['read8']);
  for (const address of [DATA + 7, 0x6fff, 0xffffffff, PC + 7]) {
    if (address !== PC + 7) upload(c, address, Buffer.from([0xa7]));
    const value = address === PC + 7 ? 0 : 0xa7, before = seed(c, 0xdeadbeef, address), generation = c.api.generation();
    before.flags = 0x244cd7; input(c, 0, cpuRecord(before));
    run(c, fn, next(before, flush.length), {helper: successByte(value), calls: ['read8']}); counts.flushes++;
    assert.equal(c.api.generation(), generation, 'flush must not change code identity');
    input(c, 0, cpuRecord(before));
    run(c, fn, next(before, flush.length), {helper: successByte(value), calls: ['read8']}); counts.flushes++;
  }
  const faulted = seed(c, 0xdeadbeef, 0x7000);
  run(c, fn, faulted, {reason: 5, retired: 0, helper: faultByte(1, 0x7000), fault: {detail: 1, address: 0x7000, access: 1, length: 1}, calls: ['read8']}); counts.faults++;
  assert.equal(c.api.map(0x7000, 1, 3), 0); upload(c, 0x7000, Buffer.from([0x5a]));
  run(c, fn, next(faulted, flush.length), {helper: successByte(0x5a), calls: ['read8']}); counts.repairs++;
  assert.equal(c.api.unmap(0x7000, 1), 0);
  const denied = seed(c, 0, DATA + 7); assert.equal(c.api.protect(DATA, 1, 4), 0);
  run(c, fn, denied, {reason: 5, retired: 0, helper: faultByte(2, DATA + 7), fault: {detail: 2, address: DATA + 7, access: 1, length: 1}, calls: ['read8']}); counts.faults++;
  assert.equal(c.api.protect(DATA, 1, 3), 0);
  run(c, fn, next(denied, flush.length), {helper: successByte(0xa7), calls: ['read8']}); counts.repairs++;
  stopped(c, fn);

  const csCode = [0x2e, ...flush], cs = compile(c, csCode, ['read8']);
  assert.equal(c.api.configure_segment_descriptor(0x43, 0, 3, 1, 2), 0);
  const segments = Buffer.from(arena(c).subarray(4524, 4588)), executeOnly = Buffer.from(segments); executeOnly.writeUInt16LE(0x43, 18); input(c, 4524, executeOnly);
  const csBefore = seed(c, 0, DATA + 7); run(c, cs, next(csBefore, csCode.length), {helper: successByte(0xa7), calls: ['read8']}); counts.flushes++;
  input(c, 4524, segments);
  const fsCode = [0x64, 0x67, 0x0f, 0xae, 0x38], fs = compile(c, fsCode, ['read8']);
  assert.equal(c.api.set_segment_base(4, DATA), 0); upload(c, DATA + 0x11, Buffer.from([0x81]));
  const fsBefore = seed(c); fsBefore.registers[3] = 0xfffffff0; fsBefore.registers[6] = 0x21; input(c, 0, cpuRecord(fsBefore));
  run(c, fs, next(fsBefore, fsCode.length), {helper: successByte(0x81), calls: ['read8']}); counts.flushes++;
  assert.equal(c.api[c.owner === 'resident' ? 'segment_load_resident' : 'segment_load'](...c.binding, 4, 0), 0); input(c, 0, cpuRecord(fsBefore));
  run(c, fs, fsBefore, {reason: 11, retired: 0}); counts.faults++;
  assert.equal(c.api[c.owner === 'resident' ? 'segment_load_resident' : 'segment_load'](...c.binding, 4, 0x3b), 0);

  for (const prefix of [[], [0x66], [0x67], [0x2e]]) {
    const instruction = [...prefix, 0x0f, 0x01, 0xf9], rdtscp = compile(c, instruction, ['cpu_integer']);
    stopped(c, rdtscp);
    for (const flags of [2, 0xcd7, 0x244cd7]) {
      const before = seed(c); before.flags = flags; input(c, 0, cpuRecord(before)); timestamp(c, rdtscp, before, PC + instruction.length, true);
    }
  }
  const chainCode = [0x0f, 0x31, 0x0f, 0x01, 0xf9, 0x0f, 0x31], chain = compile(c, chainCode, ['cpu_integer']);
  let current = seed(c); current = timestamp(c, chain, current, PC + 2, false); current = timestamp(c, chain, current, PC + 5, true); timestamp(c, chain, current, PC + 7, false); counts.ordering++;

  const loadCode = [0x8b, 0x17, 0x0f, 0x01, 0xf9], load = compile(c, loadCode, ['read32', 'cpu_integer']);
  const loadBefore = seed(c, 0, 0x6fff); upload(c, 0x6fff, Buffer.from([0xaa]));
  run(c, load, loadBefore, {budget: 2, reason: 5, retired: 0, fault: {detail: 1, address: 0x7000, access: 1, length: 4}, helper: {version: 1, fields: [1, 0, 1, 0x7000, 1, 4]}, calls: ['read32']}); counts.faults++;
  assert.equal(c.api.map(0x7000, 1, 3), 0); upload(c, 0x6fff, words([0x12345678]));
  const loaded = {...loadBefore, registers: [...loadBefore.registers]}; loaded.registers[2] = 0x12345678;
  timestamp(c, load, loadBefore, PC + loadCode.length, true, {budget: 2, retired: 2, prefixCalls: ['read32'], payloadBefore: loaded}); counts.repairs++; counts.ordering++;
  assert.equal(c.calls.at(-1).input[3], 0x12345678, 'prior load publishes EDX before RDTSCP samples the counter');
  assert.equal(c.api.unmap(0x7000, 1), 0);

  const storeCode = [0x89, 0x07, ...flush, 0x0f, 0x01, 0xf9], store = compile(c, storeCode, ['store32', 'read8', 'cpu_integer']);
  const stored = seed(c, 0, DATA + 7); stored.registers[0] = 0x12345678; input(c, 0, cpuRecord(stored));
  timestamp(c, store, stored, PC + storeCode.length, true, {budget: 3, retired: 3, prefixCalls: ['store32', 'read8']}); counts.ordering++;
  assert.equal(c.api.read32(DATA + 7), 0); assert.equal(arena(c).readUInt32LE(120), 0x12345678);
  assert.equal(c.api.close(), 0);
}
assert.equal(counts.contexts, 4); assert.equal(counts.modules, 40); assert.equal(counts.runs, 172); assert.equal(counts.flushes, 40); assert.equal(counts.timestamps, 68); assert.equal(counts.faults, 16); assert.equal(counts.repairs, 12); assert.equal(counts.controls, 40); assert.equal(counts.ordering, 12);
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed during execution');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, limits: ['coherent synchronous guest RAM and ordered virtual timestamp events for one virtual CPU; no physical cache, host clock, multicore or throughput claim']}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
