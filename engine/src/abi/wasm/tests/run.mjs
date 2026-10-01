import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {existsSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, probePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, helper probe, output directory and repo root');
const engineBytes = readFileSync(enginePath);
const probeBytes = readFileSync(probePath);
assert.ok(WebAssembly.validate(engineBytes), 'actual engine validation');
assert.ok(WebAssembly.validate(probeBytes), 'helper probe validation');
const engineModule = new WebAssembly.Module(engineBytes);
const probeModule = new WebAssembly.Module(probeBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), [], 'engine has no JS imports');
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32'];
const baselineBytes = Uint8Array.from([0xb8, 0, 0, 0, 0, 0x83, 0xf9, 0, 0x74, 7, 0x01, 0xc8, 0x83, 0xe9, 1, 0xeb, 0xf4]);
const baselineBlocks = [[0x1000, 5], [0x1005, 5], [0x100a, 7]];
let keys = 0n, calls = 0, modules = 0;
const generatedHashes = [];

function hash(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function imports(module) {
  return WebAssembly.Module.imports(module).sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
}
assert.deepEqual(imports(probeModule), [
  {module: 'env', name: 'memory', kind: 'memory'},
  {module: 'ring3', name: 'guard', kind: 'function'},
  {module: 'ring3', name: 'read32', kind: 'function'},
  {module: 'ring3', name: 'write32', kind: 'function'},
], 'probe function imports');

function refresh(engine) {
  if (engine.buffer !== engine.memory.buffer) {
    engine.buffer = engine.memory.buffer;
    engine.bytes = new Uint8Array(engine.buffer);
    engine.view = new DataView(engine.buffer);
  }
  return engine;
}

function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => {
    const value = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof value, 'function', `actual export ${name}`);
    return [name, value];
  }));
  const memory = instance.exports.memory;
  assert.ok(memory instanceof WebAssembly.Memory, 'engine owns actual exported memory');
  const key = 0xfedcba9800000000n + ++keys;
  const engine = refresh({instance, api, memory, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n), base: 0});
  engine.probe = new WebAssembly.Instance(probeModule, {
    env: {memory},
    ring3: {guard: api.guard, read32: api.read32, write32: api.write32},
  }).exports;
  return engine;
}

function arena(engine) {
  refresh(engine);
  return engine.bytes.slice(engine.base, engine.base + 4236);
}

function open(engine, pages = 8) {
  assert.equal(engine.api.open(pages, engine.low, engine.high), 0, 'open engine context');
  engine.base = engine.api.arena_ptr() >>> 0;
  refresh(engine);
  assert.ok(engine.base > 0 && engine.base + 4236 <= engine.bytes.length, 'fixed arena bounds');
  assert.equal(Buffer.from(engine.bytes.subarray(engine.base, engine.base + 4)).toString('ascii'), 'R3ST');
}

function transfer(engine, bytes) {
  refresh(engine).bytes.set(bytes, engine.base + 140);
}

function upload(engine, address, bytes) {
  transfer(engine, bytes);
  assert.equal(engine.api.upload(address, bytes.length), 0, 'upload authored guest bytes');
  refresh(engine);
}

function descriptors(engine, blocks) {
  refresh(engine);
  blocks.forEach(([entry, length], index) => {
    engine.view.setUint32(engine.base + 140 + index * 8, entry, true);
    engine.view.setUint32(engine.base + 144 + index * 8, length, true);
  });
}

function installed(engine) {
  const pointer = engine.api.module_ptr() >>> 0, length = engine.api.module_len() >>> 0;
  refresh(engine);
  assert.ok(pointer > 0 && length > 8 && pointer + length <= engine.bytes.length, 'checked artifact getters');
  const bytes = engine.bytes.slice(pointer, pointer + length);
  assert.ok(WebAssembly.validate(bytes), 'engine-generated module validation');
  const module = new WebAssembly.Module(bytes);
  assert.deepEqual(imports(module), [
    {module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: 'guard', kind: 'function'},
  ], 'generated module direct engine guard import');
  const instance = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: {guard: engine.api.guard}});
  const generation = engine.api.generation() >>> 0;
  assert.ok(generation > 0);
  modules++;
  generatedHashes.push(hash(bytes));
  writeFileSync(join(outputDir, `generated-${modules}.wasm`), bytes);
  return {bytes, module, run: instance.exports.run, generation};
}

function compile(engine, blocks = baselineBlocks) {
  descriptors(engine, blocks);
  assert.equal(engine.api.compile(blocks.length), 0, 'compile embedded region');
  return installed(engine);
}

function setup(engine) {
  assert.equal(engine.api.map(0x1000, 1, 7), 0);
  upload(engine, 0x1000, baselineBytes);
  assert.equal(engine.api.map(0x4000, 2, 7), 0);
  assert.equal(engine.probe.write32(0x4000, 0x11223344), 0);
  assert.equal(engine.probe.write32(0x4004, 0xaabbccdd), 0);
}

function header(bytes, view, pointer, magic, length) {
  bytes.set(Buffer.from(magic, 'ascii'), pointer);
  view.setUint16(pointer + 4, 1, true);
  view.setUint16(pointer + 6, 1, true);
  view.setUint32(pointer + 8, length, true);
  view.setUint32(pointer + 12, 0, true);
}

function state(input = 10, eip = 0x1000) {
  return {registers: [0x89abcdef, input, 0x23456789, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde], eip, eflags: 0xcd7};
}

function copyState(value) {
  return {...value, registers: [...value.registers]};
}

function writeState(engine, value, cancel = 0) {
  refresh(engine);
  header(engine.bytes, engine.view, engine.base, 'R3ST', 56);
  value.registers.forEach((register, index) => engine.view.setUint32(engine.base + 16 + index * 4, register, true));
  engine.view.setUint32(engine.base + 48, value.eip, true);
  engine.view.setUint32(engine.base + 52, value.eflags, true);
  engine.view.setUint32(engine.base + 96, cancel, true);
  engine.bytes.fill(0xa5, engine.base + 56, engine.base + 96);
}

function readState(engine) {
  refresh(engine);
  assert.deepEqual(engine.bytes.slice(engine.base, engine.base + 16), Uint8Array.from([0x52, 0x33, 0x53, 0x54, 1, 0, 1, 0, 56, 0, 0, 0, 0, 0, 0, 0]));
  return {
    registers: Array.from({length: 8}, (_, index) => engine.view.getUint32(engine.base + 16 + index * 4, true)),
    eip: engine.view.getUint32(engine.base + 48, true),
    eflags: engine.view.getUint32(engine.base + 52, true),
  };
}

function exit(reason, retired) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3EX', 40);
  view.setUint32(16, reason, true);
  view.setUint32(20, retired, true);
  return bytes;
}

function run(engine, child, budget, expected, reason, retired, label) {
  const untouched = arena(engine).slice(96);
  assert.equal(child.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, `${label}: host status`);
  calls++;
  assert.deepEqual(readState(engine), expected, `${label}: full state`);
  assert.deepEqual(arena(engine).slice(56, 96), exit(reason, retired), `${label}: canonical exit`);
  assert.deepEqual(arena(engine).slice(96), untouched, `${label}: cancel/helper/transfer unchanged`);
}

function helperRecord(tag, value = 0, detail = 0, address = 0, access = 0) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3MH', 40);
  [tag, value, detail, address, access, tag === 1 ? 4 : 0].forEach((field, index) => view.setUint32(16 + index * 4, field, true));
  return bytes;
}

function helper(engine, name, address, value, expected) {
  const code = name === 'read32' ? engine.probe.read32(address) : engine.probe.write32(address, value);
  assert.equal(code, 0, `actual Wasm helper ${name}/${address}`);
  calls++;
  assert.deepEqual(arena(engine).slice(100, 140), expected, `${name}/${address}: canonical helper result`);
}

function guestValue(engine, address) {
  assert.equal(engine.probe.read32(address), 0);
  refresh(engine);
  assert.equal(engine.view.getUint32(engine.base + 116, true), 0, 'guest observation succeeds');
  return engine.view.getUint32(engine.base + 120, true);
}

function rejected(engine, child, expectedStatus, label, badPointers = false, observeGuest = true) {
  const beforeGuest = observeGuest ? [guestValue(engine, 0x4000), guestValue(engine, 0x4004)] : null;
  const before = arena(engine);
  const pointers = badPointers ? [0xffffffff, 0xffffffff, 0xffffffff] : [engine.base, engine.base + 56, engine.base + 96];
  assert.equal(child.run(pointers[0], pointers[1], 100, pointers[2]), expectedStatus, `${label}: rejected before generated memory access`);
  calls++;
  assert.deepEqual(arena(engine), before, `${label}: arena unchanged`);
  if (observeGuest) assert.deepEqual([guestValue(engine, 0x4000), guestValue(engine, 0x4004)], beforeGuest, `${label}: guest words unchanged`);
}

const engine = fresh();
assert.equal(engine.api.arena_ptr(), 0);
assert.equal(engine.api.module_ptr(), 0);
assert.equal(engine.api.module_len(), 0);
assert.equal(engine.api.generation(), 0);
assert.equal(engine.probe.guard(engine.low, engine.high, 1, 0, 0, 0), 5, 'uninitialized guard');
assert.equal(engine.probe.read32(0xffffffff), 5, 'uninitialized read helper');
assert.equal(engine.probe.write32(0xffffffff, 0xffffffff), 5, 'uninitialized write helper');
assert.equal(engine.api.open(8, 0, 0), 7, 'zero key rejected');
for (const pages of [0, 4097, 0xffffffff]) assert.equal(engine.api.open(pages, engine.low, engine.high), 7, `invalid resident pages ${pages}`);
assert.equal(engine.api.arena_ptr(), 0, 'failed init remains uninitialized');
open(engine);
const fixedPointer = engine.base;
const beforeDuplicate = arena(engine);
assert.equal(engine.api.open(8, engine.low, engine.high), 6, 'create once');
assert.deepEqual(arena(engine), beforeDuplicate, 'duplicate open preserves arena');
for (const bits of [8, 0x80000000, 0xffffffff]) assert.equal(engine.api.map(0x8000, 1, bits), 7, `invalid permission bits ${bits}`);
assert.equal(engine.api.map(1, 1, 7), 8, 'invalid page range typed memory status');
assert.equal(engine.api.upload(0x1000, 4097), 7, 'transfer overflow rejected');
setup(engine);
let child = compile(engine);
assert.equal(engine.api.arena_ptr() >>> 0, fixedPointer, 'arena survives compile allocations');
assert.equal(engine.probe.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'actual Wasm guard accepts live artifact');

const source = join(root, 'engine/src/cpu/dbt/wasm/fixtures/baseline_oracle.c');
const sourceHash = hash(readFileSync(source));
let oracle = join(root, 'target/p2-baseline-fixtures/baseline_oracle'), reused = false;
const provenancePath = join(root, 'target/p2-baseline-fixtures/provenance.json');
if (existsSync(provenancePath) && existsSync(oracle)) {
  const provenance = JSON.parse(readFileSync(provenancePath, 'utf8'));
  reused = provenance.sha256?.['engine/src/cpu/dbt/wasm/fixtures/baseline_oracle.c'] === sourceHash && provenance.sha256?.['target/p2-baseline-fixtures/baseline_oracle'] === hash(readFileSync(oracle));
}
if (!reused) {
  oracle = join(outputDir, 'baseline_oracle');
  execFileSync('clang', ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', source, '-o', oracle], {stdio: ['ignore', 'pipe', 'pipe']});
}
for (const input of [0, 1, 2, 10, 100, 1000]) {
  const native = JSON.parse(execFileSync(oracle, [String(input)], {encoding: 'utf8'}));
  assert.deepEqual(native, {sum: input * (input + 1) / 2, remaining: 0, retired: 5 * input + 3}, `independent native/math oracle ${input}`);
  const start = state(input), expected = copyState(start);
  expected.registers[0] = native.sum;
  expected.registers[1] = 0;
  expected.eip = 0x1011;
  expected.eflags = 0x446;
  writeState(engine, start);
  run(engine, child, native.retired + 10, expected, 3, native.retired, `engine-owned baseline ${input}`);
}
const start = state(10), checkpoint = copyState(start);
checkpoint.registers[0] = 0;
checkpoint.eip = 0x1005;
writeState(engine, start);
run(engine, child, 0, start, 1, 0, 'zero budget');
run(engine, child, 1, checkpoint, 1, 1, 'first instruction checkpoint');
refresh(engine).view.setUint32(engine.base + 96, 0xffffffff, true);
run(engine, child, 0, checkpoint, 2, 0, 'cancellation priority between resumes');
engine.view.setUint32(engine.base + 96, 0, true);
const finished = copyState(start);
finished.registers[0] = 55;
finished.registers[1] = 0;
finished.eip = 0x1011;
finished.eflags = 0x446;
run(engine, child, 100, finished, 3, 52, 'resume after cancellation cleared');
const unknown = state(3, 0x12345678);
writeState(engine, unknown);
run(engine, child, 10, unknown, 3, 0, 'unknown PC NeedCode');

writeState(engine, state(1000));
let retiredTotal = 0, resumes = 0;
while (true) {
  assert.equal(child.run(engine.base, engine.base + 56, 13, engine.base + 96), 0);
  calls++;
  resumes++;
  refresh(engine);
  const reason = engine.view.getUint32(engine.base + 72, true), retired = engine.view.getUint32(engine.base + 76, true);
  assert.deepEqual(arena(engine).slice(56, 96), exit(reason, retired), 'resumed canonical exit');
  assert.ok(reason === 1 || reason === 3);
  assert.ok(retired <= 13);
  if (reason === 1) assert.equal(retired, 13);
  retiredTotal += retired;
  if (reason === 3) break;
  assert.ok(resumes < 500, 'bounded loop eventually finishes');
}
const longExpected = state(1000);
longExpected.registers[0] = 500500;
longExpected.registers[1] = 0;
longExpected.eip = 0x1011;
longExpected.eflags = 0x446;
assert.deepEqual(readState(engine), longExpected, 'resumed full state');
assert.equal(retiredTotal, 5003, 'resumed exact retirement');

for (const [stateOffset, exitOffset, cancelOffset] of [[1, 56, 96], [0, 57, 96], [0, 56, 97], [0xffffffff, 56, 96]]) {
  const before = arena(engine);
  const pointer = stateOffset === 0xffffffff ? stateOffset : engine.base + stateOffset;
  assert.equal(child.run(pointer, engine.base + exitOffset, 1, engine.base + cancelOffset), 1, 'exact arena pointers required');
  calls++;
  assert.deepEqual(arena(engine), before, 'wrong pointers cannot mutate arena');
}
for (const [offset, width, value] of [[0, 1, 0], [4, 2, 2], [6, 2, 2], [8, 4, 55], [12, 4, 1], [52, 4, 0], [52, 4, 0xa], [52, 4, 0x80000002]]) {
  writeState(engine, state(3));
  refresh(engine);
  if (width === 1) engine.view.setUint8(engine.base + offset, value);
  else if (width === 2) engine.view.setUint16(engine.base + offset, value, true);
  else engine.view.setUint32(engine.base + offset, value, true);
  const before = arena(engine);
  assert.equal(child.run(engine.base, engine.base + 56, 1, engine.base + 96), 2, 'invalid state rejected');
  calls++;
  assert.deepEqual(arena(engine), before, 'invalid state cannot mutate arena');
}
writeState(engine, state(3));
const beforeGrowth = engine.memory.buffer, oldView = new Uint8Array(beforeGrowth);
engine.memory.grow(1);
assert.equal(oldView.byteLength, 0, 'old buffer view detached on growth');
refresh(engine);
assert.equal(engine.api.arena_ptr() >>> 0, fixedPointer, 'fixed arena pointer after memory growth');
assert.equal(engine.probe.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'same actual memory object after growth');

helper(engine, 'write32', 0x4ffe, 0x44332211, helperRecord(0));
helper(engine, 'read32', 0x4ffe, undefined, helperRecord(0, 0x44332211));
assert.equal(engine.api.protect(0x5000, 1, 5), 0);
helper(engine, 'write32', 0x4ffe, 0xffffffff, helperRecord(1, 0, 2, 0x5000, 2));
helper(engine, 'read32', 0x4ffe, undefined, helperRecord(0, 0x44332211));
assert.equal(engine.api.map(0x6000, 1, 3), 0);
helper(engine, 'write32', 0x6ffc, 0x44332211, helperRecord(0));
helper(engine, 'write32', 0x6ffe, 0xffffffff, helperRecord(1, 0, 1, 0x7000, 2));
helper(engine, 'read32', 0x6ffc, undefined, helperRecord(0, 0x44332211));
helper(engine, 'read32', 0x6ffe, undefined, helperRecord(1, 0, 1, 0x7000, 1));
assert.equal(engine.api.protect(0x6000, 1, 4), 0);
helper(engine, 'read32', 0x6000, undefined, helperRecord(1, 0, 2, 0x6000, 1));
assert.equal(engine.api.map(0xfffff000, 1, 3), 0);
helper(engine, 'write32', 0xfffffffc, 0xffffffff, helperRecord(0));
helper(engine, 'read32', 0xfffffffc, undefined, helperRecord(0, 0xffffffff));
helper(engine, 'read32', 0xfffffffd, undefined, helperRecord(1, 0, 3, 0xfffffffd, 1));
helper(engine, 'write32', 0xfffffffd, 123, helperRecord(1, 0, 3, 0xfffffffd, 2));
helper(engine, 'write32', 0x1ffe, 123, helperRecord(1, 0, 1, 0x2000, 2));
assert.equal(engine.probe.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'failed code-page write and unrelated data writes preserve code versions');

descriptors(engine, [[0x1000, 4]]);
const beforeFailure = arena(engine), generation = child.generation;
assert.equal(engine.api.compile(1), 10, 'failed compile typed status');
assert.deepEqual(arena(engine), beforeFailure, 'failed compile arena unchanged');
assert.equal(engine.api.generation(), generation, 'failed compile keeps generation');
assert.equal(engine.probe.guard(engine.low, engine.high, generation, engine.base, engine.base + 56, engine.base + 96), 0, 'failed compile keeps installed artifact');
for (const count of [0, 9, 0xffffffff]) assert.equal(engine.api.compile(count), 7, `invalid compile count ${count}`);
const old = child;
child = compile(engine);
assert.equal(child.generation, old.generation + 1, 'same code replacement advances generation');
rejected(engine, old, 3, 'old generation', true);
assert.equal(engine.probe.guard(engine.low ^ 1, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 3, 'wrong key');
const beforeHighKey = arena(engine);
assert.equal(engine.probe.guard(engine.low, engine.high ^ 1, child.generation, engine.base, engine.base + 56, engine.base + 96), 3, 'wrong high key word');
assert.deepEqual(arena(engine), beforeHighKey, 'wrong high key word preserves arena');

for (const mutation of ['write', 'protect', 'remap']) {
  const previous = child;
  if (mutation === 'write') helper(engine, 'write32', 0x1000, 0xb8, helperRecord(0));
  else if (mutation === 'protect') assert.equal(engine.api.protect(0x1000, 1, 7), 0);
  else {
    assert.equal(engine.api.unmap(0x1000, 1), 0);
    assert.equal(engine.api.map(0x1000, 1, 7), 0);
    upload(engine, 0x1000, baselineBytes);
  }
  assert.equal(engine.api.module_ptr(), 0, `${mutation}: unavailable artifact pointer`);
  assert.equal(engine.api.module_len(), 0, `${mutation}: unavailable artifact length`);
  rejected(engine, previous, 4, `code ${mutation}`, true);
  helper(engine, 'read32', 0x1000, undefined, helperRecord(0, 0xb8));
  child = compile(engine);
}

const second = fresh();
open(second);
setup(second);
const secondChild = compile(second);
assert.notEqual(second.key, engine.key, 'host assigns distinct nonzero keys');
const rebound = new WebAssembly.Instance(old.module, {env: {memory: second.memory}, ring3: {guard: second.api.guard}});
rejected(second, {run: rebound.exports.run}, 3, 'cross-instance same-generation wrong key', true);
assert.equal(secondChild.generation, old.generation, 'cross-instance rejection is not a generation difference');

assert.equal(engine.api.close(), 0, 'close engine context');
assert.equal(engine.api.arena_ptr(), 0);
assert.equal(engine.api.generation(), 0);
assert.equal(engine.api.module_ptr(), 0);
assert.equal(engine.api.module_len(), 0);
rejected(engine, child, 5, 'closed context', true, false);
const closedArena = arena(engine);
assert.equal(engine.probe.read32(0x4000), 5, 'closed read helper');
assert.equal(engine.probe.write32(0x4000, 0), 5, 'closed write helper');
assert.equal(engine.api.map(0x8000, 1, 3), 5, 'closed map');
assert.equal(engine.api.upload(0x1000, 1), 5, 'closed upload');
assert.equal(engine.api.compile(1), 5, 'closed compile');
assert.equal(engine.api.open(8, engine.low, engine.high), 6, 'closed instance never reopens');
assert.deepEqual(arena(engine), closedArena, 'closed operations preserve retained arena');

const neverOpened = fresh();
assert.equal(neverOpened.api.close(), 0, 'close before open creates tombstone');
assert.equal(neverOpened.api.close(), 0, 'close before open is idempotent');
assert.equal(neverOpened.api.open(8, neverOpened.low, neverOpened.high), 6, 'closed uninitialized instance cannot open');
for (const getter of ['arena_ptr', 'generation', 'module_ptr', 'module_len']) {
  assert.equal(neverOpened.api[getter](), 0, `closed uninitialized ${getter} unavailable`);
}
assert.equal(neverOpened.probe.guard(neverOpened.low, neverOpened.high, 1, 0, 0, 0), 5, 'closed uninitialized guard');
assert.equal(neverOpened.probe.read32(0xffffffff), 5, 'closed uninitialized read helper');
assert.equal(neverOpened.probe.write32(0xffffffff, 0xffffffff), 5, 'closed uninitialized write helper');

console.log(JSON.stringify({engine_sha256: hash(engineBytes), probe_sha256: hash(probeBytes), generated_modules: modules, calls, resumes, native_cases: 6, oracle_reused: reused, generated_set_sha256: hash(generatedHashes.join('\n')), artifacts: outputDir}));
