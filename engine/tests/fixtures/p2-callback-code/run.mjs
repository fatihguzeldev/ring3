import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const engineBytes = readFileSync(enginePath);
assert.ok(WebAssembly.validate(engineBytes), 'actual engine validates');
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), [], 'engine has no JS imports');
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'compile_with_gates', 'capture_call', 'complete_call', 'abandon_call', 'begin_callback', 'finish_callback', 'abort_callback', 'resume_callback_code', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'store32'];
let keys = 0n, modules = 0, actualRuns = 0;
const generatedHashes = [];

function hash(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function sortedImports(module) {
  return WebAssembly.Module.imports(module).sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
}

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
    const fn = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof fn, 'function', `actual export ${name}`);
    return [name, fn];
  }));
  const memory = instance.exports.memory;
  assert.ok(memory instanceof WebAssembly.Memory);
  const key = 0x8123456700000000n + ++keys;
  const engine = refresh({api, memory, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n)});
  assert.equal(api.open(12, engine.low, engine.high), 0);
  engine.base = api.arena_ptr() >>> 0;
  refresh(engine);
  assert.ok(engine.base > 0 && engine.base + 4236 <= engine.bytes.length);
  assert.equal(api.map(0x1000, 1, 7), 0);
  return engine;
}

function arena(engine) {
  return refresh(engine).bytes.slice(engine.base, engine.base + 4236);
}

function upload(engine, address, bytes) {
  assert.ok(bytes.length <= 4096);
  refresh(engine).bytes.set(bytes, engine.base + 140);
  assert.equal(engine.api.upload(address, bytes.length), 0, `upload ${address}`);
  refresh(engine);
}

function header(bytes, view, pointer, magic, length, version = 1) {
  bytes.set(Buffer.from(magic, 'ascii'), pointer);
  view.setUint16(pointer + 4, version, true);
  view.setUint16(pointer + 6, 1, true);
  view.setUint32(pointer + 8, length, true);
  view.setUint32(pointer + 12, 0, true);
}

function helperRecord(tag, value = 0, detail = 0, address = 0, access = 0, length = tag === 1 ? 4 : 0) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3MH', 40);
  [tag, value, detail, address, access, length].forEach((field, index) => view.setUint32(16 + index * 4, field, true));
  return bytes;
}

function guestValue(engine, address) {
  assert.equal(engine.api.read32(address), 0, 'actual guest observation helper');
  refresh(engine);
  assert.equal(engine.view.getUint32(engine.base + 116, true), 0, `guest observation ${address} succeeds`);
  return engine.view.getUint32(engine.base + 120, true);
}

function word(engine, address, value) {
  assert.equal(engine.api.write32(address, value), 0, `initialize guest word ${address}`);
  assert.deepEqual(arena(engine).slice(100, 140), helperRecord(0));
}

function descriptors(engine, blocks, gates = []) {
  refresh(engine);
  [...blocks, ...gates].forEach(([entry, value], index) => {
    engine.view.setUint32(engine.base + 140 + index * 8, entry, true);
    engine.view.setUint32(engine.base + 144 + index * 8, value, true);
  });
}

function compile(engine, blocks, gates, expectedHelpers = []) {
  descriptors(engine, blocks, gates);
  assert.equal(engine.api.compile_with_gates(blocks.length, gates.length), 0, 'actual engine compilation');
  return installedModule(engine, expectedHelpers);
}

function installedModule(engine, expectedHelpers = ['read32', 'store32']) {
  refresh(engine);
  const pointer = engine.api.module_ptr() >>> 0, length = engine.api.module_len() >>> 0;
  assert.ok(length > 8);
  const encoded = engine.bytes.slice(pointer, pointer + length);
  assert.ok(WebAssembly.validate(encoded));
  const module = new WebAssembly.Module(encoded);
  const expected = [
    {module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: 'guard', kind: 'function'},
    ...expectedHelpers.map(name => ({module: 'ring3', name, kind: 'function'})),
  ].sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sortedImports(module), expected, 'only actual memory/guard/instruction helpers; no hot gate import');
  const instance = new WebAssembly.Instance(module, {
    env: {memory: engine.memory},
    ring3: {guard: engine.api.guard, read32: engine.api.read32, store32: engine.api.store32},
  });
  modules++;
  generatedHashes.push(hash(encoded));
  writeFileSync(join(outputDir, `generated-${modules}.wasm`), encoded);
  return {module, run: instance.exports.run, generation: engine.api.generation() >>> 0};
}

function state(eip = 0x1000, eflags = 0xcd7) {
  return {registers: [0x89abcdef, 0x13579bdf, 0x23456789, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde], eip, eflags};
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
  engine.bytes.fill(0x5a, engine.base + 100, engine.base + 140);
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

function exit(reason, retired, detail = 0, address = 0, access = 0, length = reason === 5 ? 4 : 0, version = 3) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3EX', 40, version);
  [reason, retired, detail, address, access, length].forEach((field, index) => view.setUint32(16 + index * 4, field, true));
  return bytes;
}

function run(engine, child, budget, expected, expectedExit, label, expectedHelper = undefined) {
  const before = arena(engine);
  assert.equal(child.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, `${label}: canonical runtime exit status`);
  actualRuns++;
  assert.deepEqual(readState(engine), expected, `${label}: complete CPU state`);
  const after = arena(engine);
  assert.deepEqual(after.slice(56, 96), expectedExit, `${label}: canonical exit`);
  assert.deepEqual(after.slice(96, 100), before.slice(96, 100), `${label}: cancellation unchanged`);
  assert.deepEqual(after.slice(140), before.slice(140), `${label}: transfer unchanged`);
  if (expectedHelper !== undefined) assert.deepEqual(after.slice(100, 140), expectedHelper, `${label}: canonical helper result`);
}


function elfFixtures(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]);
  assert.equal(view.getUint16(18, true), 3, 'independent i386 object');
  const sectionOffset = view.getUint32(32, true), sectionSize = view.getUint16(46, true), count = view.getUint16(48, true);
  const sections = Array.from({length: count}, (_, index) => {
    const offset = sectionOffset + index * sectionSize;
    return {type: view.getUint32(offset + 4, true), offset: view.getUint32(offset + 16, true), size: view.getUint32(offset + 20, true), link: view.getUint32(offset + 24, true), info: view.getUint32(offset + 28, true), stride: view.getUint32(offset + 36, true)};
  });
  const table = sections.find(section => section.type === 2);
  assert.ok(table, 'ELF symbol table');
  const strings = sections[table.link];
  const string = offset => {
    const start = strings.offset + offset;
    const end = object.indexOf(0, start);
    return object.subarray(start, end).toString('utf8');
  };
  const symbols = new Map();
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    const name = string(view.getUint32(at, true)), section = view.getUint16(at + 14, true);
    const start = view.getUint32(at + 4, true), size = view.getUint32(at + 8, true);
    if (section !== 0 && section < sections.length && size > 0) {
      const source = sections[section];
      assert.ok(start + size <= source.size);
      assert.ok(!sections.some(item => [4, 9].includes(item.type) && item.info === section && item.size > 0), 'authored text has no relocations');
      symbols.set(name, {offset: start, bytes: object.subarray(source.offset + start, source.offset + start + size)});
    }
  }
  return symbols;
}

const fixtureRoot = join(root, 'engine/tests/fixtures/p2-callback-code');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(outputDir, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), assembled = elfFixtures(object);
const callerBytes = [0x6a, 0xff, 0x6a, 0, 0xe8, 0xf7, 0, 0, 0, 0x90];
const callbackBytes = [0x8b, 0x44, 0x24, 4, 0x8b, 0x54, 0x24, 8, 0x01, 0xd0, 0xe8, 0xf1, 0x2b, 0, 0];
const afterCalleeBytes = [0x50, 0xe8, 0xeb, 0xfd, 0xff, 0xff];
const afterInnerBytes = [0x83, 0xc0, 1, 0xc2, 8, 0];
const calleeBytes = [0x83, 0xc0, 3, 0xc3];
const llvmFixtures = [
  {name: 'outer_caller', offset: 0, bytes: callerBytes},
  {name: 'outer_gate', offset: 0x100, bytes: [0x0f, 0x0b]},
  {name: 'inner_gate', offset: 0x200, bytes: [0x0f, 0x0b]},
  {name: 'callback_return_gate', offset: 0x300, bytes: [0x0f, 0x0b]},
  {name: 'callback_entry', offset: 0x400, bytes: callbackBytes},
  {name: 'callback_after_callee', offset: 0x40f, bytes: afterCalleeBytes},
  {name: 'callback_after_inner', offset: 0x415, bytes: afterInnerBytes},
  {name: 'absent_callee', offset: 0x3000, bytes: calleeBytes},
];
for (const {name, offset, bytes} of llvmFixtures) {
  assert.deepEqual([...assembled.get(name).bytes], bytes, `independent LLVM callback-code fixture ${name}`);
  assert.equal(assembled.get(name).offset, offset);
}

const outerId = 101, innerId = 102, returnId = 103;
const gates = [[0x1100, outerId], [0x1200, innerId], [0x1300, returnId]];
const oldBlocks = [[0x1000, 9], [0x1009, 1], [0x1100, 2], [0x1200, 2], [0x1300, 2], [0x1400, 15], [0x140f, 6], [0x1415, 6]];
// The consumed historical callback entry is intentionally omitted after install.
const newBlocks = oldBlocks.map(([entry, length]) => entry === 0x1400 ? [0x4000, 4] : [entry, length]);
let captures = 0, callCompletions = 0, begins = 0, installs = 0;
let finishes = 0, aborts = 0, abandons = 0, rejectedOperations = 0;

function fail(engine, operation, status, label) {
  const before = arena(engine), generation = engine.api.generation();
  const modulePointer = engine.api.module_ptr(), moduleLength = engine.api.module_len();
  assert.equal(operation(), status, label);
  rejectedOperations++;
  assert.deepEqual(arena(engine), before, `${label}: all arena bytes unchanged`);
  assert.equal(engine.api.generation(), generation, `${label}: generation not spent`);
  assert.equal(engine.api.module_ptr(), modulePointer, `${label}: artifact retained`);
  assert.equal(engine.api.module_len(), moduleLength, `${label}: artifact length retained`);
}

function callRecord(token, id, stopped, returnPc, values) {
  const bytes = new Uint8Array(112), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3CF', 112);
  [token, id, 2, values.length, stopped.eip, stopped.registers[4], returnPc, 0].forEach((value, index) => view.setUint32(16 + 4 * index, value, true));
  values.forEach((value, index) => view.setUint32(48 + 4 * index, value, true));
  return bytes;
}

function capture(test, token, id, stopped, returnPc, values) {
  const {engine, child} = test, before = arena(engine);
  assert.equal(engine.api.capture_call(engine.low, engine.high, child.generation, 2, values.length), 0);
  captures++;
  const after = arena(engine);
  assert.deepEqual(after.slice(140, 252), callRecord(token, id, stopped, returnPc, values), 'literal capture preserves shared outer1/callback2/inner3 token sequence');
  assert.deepEqual(after.slice(0, 140), before.slice(0, 140));
  assert.deepEqual(after.slice(252), before.slice(252));
}

function callbackRecord(test, generation, phase = 1, result = 0) {
  const bytes = new Uint8Array(64), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3CB', 64);
  [2, 1, phase, 0, 0x1400, test.callbackState.registers[4], 0x1300, returnId, 2, result, generation, 0].forEach((value, index) => view.setUint32(16 + 4 * index, value, true));
  return bytes;
}

function begin(test) {
  const {engine, child, values} = test;
  refresh(engine);
  values.forEach((value, index) => engine.view.setUint32(engine.base + 140 + 4 * index, value, true));
  const before = arena(engine);
  assert.equal(engine.api.begin_callback(engine.low, engine.high, child.generation, 1, 0x1400, 0x1300, returnId, 2), 0);
  begins++;
  assert.deepEqual(readState(engine), test.callbackState);
  const after = arena(engine);
  assert.deepEqual(after.slice(56, 96), exit(3, 0));
  assert.deepEqual(after.slice(96, 140), before.slice(96, 140));
  assert.deepEqual(after.slice(140, 204), callbackRecord(test, child.generation));
  assert.deepEqual(after.slice(204), before.slice(204));
}

function addFlags(left, right, old) {
  const wide = BigInt(left) + BigInt(right), value = Number(wide & 0xffffffffn);
  let flags = 2 | (old & 0x400);
  if (wide > 0xffffffffn) flags |= 1;
  if (([...Array(8).keys()].reduce((bits, bit) => bits + ((value >>> bit) & 1), 0) & 1) === 0) flags |= 4;
  if ((left & 15) + (right & 15) > 15) flags |= 0x10;
  if (value === 0) flags |= 0x40;
  if (value >= 0x80000000) flags |= 0x80;
  if ((~(left ^ right) & (left ^ value) & 0x80000000) !== 0) flags |= 0x800;
  return flags;
}

function prepare(values = [7, 9]) {
  const engine = fresh();
  assert.equal(engine.api.map(0x4000, 1, 7), 0);
  assert.equal(engine.api.map(0x8000, 1, 3), 0);
  upload(engine, 0x1000, Uint8Array.from(callerBytes));
  for (const address of [0x1100, 0x1200, 0x1300]) upload(engine, address, Uint8Array.from([0x0f, 0x0b]));
  upload(engine, 0x1400, Uint8Array.from([...callbackBytes, ...afterCalleeBytes, ...afterInnerBytes]));
  upload(engine, 0x4000, Uint8Array.from(calleeBytes));
  const child = compile(engine, oldBlocks, gates, ['read32', 'store32']);
  const start = state();
  start.registers[4] = 0x9000;
  const outer = copyState(start);
  outer.registers[4] = 0x8ff4;
  outer.eip = 0x1100;
  writeState(engine, start); // The only host CPU-state initialization in this instance.
  run(engine, child, 4, outer, exit(8, 3, outerId), 'actual outer PUSH/PUSH/CALL reaches gate', helperRecord(0));
  const callbackState = copyState(outer);
  callbackState.registers[4] = 0x8fe8;
  callbackState.eip = 0x1400;
  const test = {engine, child, values, start, outer, callbackState, outerRecords: arena(engine).slice(0, 96)};
  capture(test, 1, outerId, outer, 0x1009, [0, 0xffffffff]);
  begin(test);
  return test;
}

function stopAtAbsentCallee(test, singleStepCall = false) {
  const {engine, child, values} = test;
  const arithmetic = copyState(test.callbackState);
  arithmetic.registers[0] = Number(BigInt.asUintN(32, BigInt(values[0]) + BigInt(values[1])));
  arithmetic.registers[2] = values[1];
  arithmetic.eip = 0x140a;
  arithmetic.eflags = addFlags(values[0], values[1], arithmetic.eflags);
  const stopped = copyState(arithmetic);
  stopped.registers[4] -= 4;
  stopped.eip = 0x4000;
  if (singleStepCall) {
    run(engine, child, 3, arithmetic, exit(1, 3), 'callback arithmetic budget checkpoint before missing CALL', helperRecord(0, values[1]));
    descriptors(engine, newBlocks, gates);
    fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, child.generation, 2, 0, 0), 7, 'counts precede non-NeedCode stop validation');
    fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, child.generation, 2, 8, 3), 13, 'Budget stop cannot install replacement');
    run(engine, child, 1, stopped, exit(1, 1), 'missing CALL writes one return then budget wins at committed callee PC', helperRecord(0));
    assert.equal(guestValue(engine, stopped.registers[4]), 0x140f);
    run(engine, child, 1, stopped, exit(3, 0), 'positive resume NeedCode does not repeat committed CALL push', helperRecord(0, 0x140f));
  } else run(engine, child, 5, stopped, exit(3, 4), 'callback real CALL commits before absent-callee NeedCode', helperRecord(0));
  test.stopped = stopped;
  return stopped;
}

function checkedStack(test) {
  const {engine, callbackState, outer, stopped, values} = test;
  const words = [[outer.registers[4], 0x1009], [outer.registers[4] + 4, 0], [outer.registers[4] + 8, 0xffffffff], [callbackState.registers[4], 0x1300], [callbackState.registers[4] + 4, values[0]], [callbackState.registers[4] + 8, values[1]], [stopped.registers[4], 0x140f]];
  for (const [address, value] of words) assert.equal(guestValue(engine, address), value, 'literal checked guest stack word');
  return words;
}

function install(test) {
  const {engine, child} = test;
  const words = checkedStack(test);
  descriptors(engine, newBlocks, gates);
  const before = arena(engine), oldGeneration = child.generation;
  assert.equal(engine.api.resume_callback_code(engine.low, engine.high, oldGeneration, 2, 8, 3), 0, 'actual synchronous callback replacement');
  installs++;
  const generation = engine.api.generation() >>> 0;
  assert.equal(generation, oldGeneration + 1, 'exactly one generation spent');
  const after = arena(engine);
  assert.deepEqual(after.slice(0, 140), before.slice(0, 140), 'installation preserves complete CPU/retired exit/cancel/helper records');
  assert.deepEqual(after.slice(140, 204), callbackRecord(test, generation), 'updated Started64 preserves historical entry absent from replacement');
  assert.deepEqual(after.slice(204), before.slice(204), 'transfer outside output64 preserved');
  for (const [address, value] of words) assert.equal(guestValue(engine, address), value, 'installation performs no guest stack write');
  test.oldChild = child;
  test.child = installedModule(engine);
  assert.equal(test.child.generation, generation);
  assert.deepEqual(readState(engine), test.stopped);
}

function complete(test, token, stopped, count, returnPc, result) {
  const {engine, child} = test, before = arena(engine), expected = copyState(stopped);
  expected.registers[0] = result;
  expected.registers[4] = (stopped.registers[4] + 4 + count * 4) >>> 0;
  expected.eip = returnPc;
  assert.equal(engine.api.complete_call(engine.low, engine.high, child.generation, token, result), 0);
  callCompletions++;
  assert.deepEqual(readState(engine), expected);
  assert.deepEqual(arena(engine).slice(56, 96), exit(3, 0));
  assert.deepEqual(arena(engine).slice(96), before.slice(96));
  return expected;
}

for (const [values, singleStepCall] of [[[7, 9], false], [[0xffffffff, 1], true]]) {
  const test = prepare(values), {engine} = test;
  stopAtAbsentCallee(test, singleStepCall);
  const oldGeneration = test.child.generation;
  if (!singleStepCall) {
    descriptors(engine, newBlocks, gates);
    fail(engine, () => engine.api.resume_callback_code(engine.low ^ 1, engine.high, oldGeneration, 2, 0, 0), 3, 'wrong session key precedes malformed counts');
    fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, oldGeneration + 1, 2, 8, 3), 3, 'wrong artifact generation rejects before installation');
    fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, oldGeneration, 1, 8, 3), 14, 'outer token does not authorize callback replacement');
    fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, oldGeneration, 0, 8, 3), 14, 'zero callback token cannot authorize replacement');
    fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, oldGeneration, 2, 0, 0), 7, 'empty replacement rejected atomically');
    fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, oldGeneration, 2, 9, 3), 7, 'replacement block limit enforced before descriptors');
    fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, oldGeneration, 2, 8, 9), 7, 'gate count cannot exceed block count');
    refresh(engine).view.setUint32(engine.base + 96, 1, true);
    fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, oldGeneration, 2, 8, 3), 16, 'cancelled callback replacement is atomic');
    engine.view.setUint32(engine.base + 96, 0, true);
    const missingTarget = newBlocks.map(([entry, length]) => entry === 0x4000 ? [0x4003, 1] : [entry, length]);
    descriptors(engine, missingTarget, gates);
    fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, oldGeneration, 2, 8, 3), 7, 'valid compiled RET region missing stopped ordinary PC cannot install');
    for (const changedId of [outerId, returnId]) {
      const changedGates = gates.map(([entry, id]) => [entry, id === changedId ? id + 100 : id]);
      descriptors(engine, newBlocks, changedGates);
      fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, oldGeneration, 2, 8, 3), 7, 'replacement must retain each private outer/return Gate binding');
    }
    const malformed = newBlocks.map(([entry, length]) => entry === 0x4000 ? [entry, 0] : [entry, length]);
    descriptors(engine, malformed, gates);
    fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, oldGeneration, 2, 8, 3), 10, 'staged compilation failure retains old artifact and continuation');
  }
  install(test);
  const {child, oldChild} = test;
  fail(engine, () => oldChild.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 3, 'old generated binding rejects before malformed pointers');
  fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, oldGeneration, 2, 8, 3), 3, 'old-generation installation request cannot replay');
  const inner = copyState(test.callbackState);
  inner.registers[0] = (test.stopped.registers[0] + 3) >>> 0;
  inner.registers[2] = values[1];
  inner.registers[4] -= 8;
  inner.eip = 0x1200;
  inner.eflags = addFlags(test.stopped.registers[0], 3, test.stopped.eflags);
  run(engine, child, 5, inner, exit(8, 4, innerId), 'new callee ADD/real RET resumes callback PUSH/CALL exactly once', helperRecord(0));
  capture(test, 3, innerId, inner, 0x1415, [inner.registers[0]]);
  fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, child.generation, 2, 0, 0), 12, 'active inner pending precedes replacement counts');
  fail(engine, () => engine.api.compile(0), 12, 'ordinary compile remains Busy across migrated callback');
  const scalar = (inner.registers[0] ^ 0x80000000) >>> 0; // Host unsigned result choice, no provider.
  const resumed = complete(test, 3, inner, 1, 0x1415, scalar);
  const added = copyState(resumed);
  added.registers[0] = (scalar + 1) >>> 0;
  added.eip = 0x1418;
  added.eflags = addFlags(scalar, 1, added.eflags);
  const returned = copyState(added);
  returned.registers[4] = test.outer.registers[4];
  returned.eip = 0x1300;
  if (!singleStepCall) {
    run(engine, child, 1, added, exit(1, 1), 'callback post-inner ADD checkpoint', arena(engine).slice(100, 140));
    run(engine, child, 1, returned, exit(1, 1), 'migrated real RET8 commits before return-gate budget', helperRecord(0, 0x1300));
    run(engine, child, 1, returned, exit(8, 0, returnId), 'return gate observes committed RET8 without another pop', helperRecord(0, 0x1300));
  } else run(engine, child, 3, returned, exit(8, 2, returnId), 'migrated callback reaches distinct return gate through real RET8', helperRecord(0, 0x1300));
  const beforeFinish = arena(engine);
  assert.equal(engine.api.finish_callback(engine.low, engine.high, child.generation, 2), 0);
  finishes++;
  assert.deepEqual(arena(engine).slice(0, 96), test.outerRecords, 'finish restores exact original outer CPU/exit after migration');
  assert.deepEqual(arena(engine).slice(96, 140), beforeFinish.slice(96, 140));
  assert.deepEqual(arena(engine).slice(140, 204), callbackRecord(test, child.generation, 2, returned.registers[0]));
  assert.deepEqual(arena(engine).slice(204), beforeFinish.slice(204));
  fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, child.generation, 2, 8, 3), 14, 'consumed callback cannot replay replacement');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, oldGeneration, 1, 0), 3, 'old generation cannot complete migrated private outer frame');
  const completed = complete(test, 1, test.outer, 2, 0x1009, returned.registers[0]);
  const caller = copyState(completed);
  caller.eip = 0x100a;
  run(engine, child, 2, caller, exit(3, 1), 'original outer caller resumes under installed generation', arena(engine).slice(100, 140));
}

{
  const test = prepare(), {engine, child} = test;
  stopAtAbsentCallee(test);
  word(engine, 0x1000, 0x006aff6a); // Same bytes still invalidate the current code snapshot.
  descriptors(engine, newBlocks, gates);
  fail(engine, () => engine.api.resume_callback_code(engine.low, engine.high, child.generation, 2, 0, 0), 4, 'old stale code precedes malformed replacement counts');
  fail(engine, () => child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 4, 'old stale artifact still rejects before pointers');
  const before = arena(engine);
  assert.equal(engine.api.abort_callback(engine.low, engine.high, 2), 0, 'stale migration stays explicit abort-only recovery');
  aborts++;
  assert.deepEqual(arena(engine).slice(0, 96), test.outerRecords);
  assert.deepEqual(arena(engine).slice(96), before.slice(96));
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, 1, 0), 4, 'abort preserves stale outer artifact limitation');
  const beforeAbandon = arena(engine);
  assert.equal(engine.api.abandon_call(engine.low, engine.high, 1), 0);
  abandons++;
  assert.deepEqual(arena(engine), beforeAbandon);
  assert.equal(guestValue(engine, 0x1000), 0x006aff6a);
}

const provenance = {
  engine_sha256: hash(engineBytes), generated_modules: modules, actual_engine_runs: actualRuns,
  successful_captures: captures, successful_call_completions: callCompletions,
  successful_callback_begins: begins, successful_code_installations: installs,
  successful_callback_finishes: finishes, explicit_callback_aborts: aborts, explicit_outer_abandons: abandons,
  rejected_operations: rejectedOperations, synthetic_helper_runs: 0,
  generated_set_sha256: hash(generatedHashes.join('\n')),
  sources: {assembly_sha256: hash(readFileSync(assembly)), node_sha256: hash(readFileSync(join(fixtureRoot, 'run.mjs')))},
  artifacts: {object_sha256: hash(object)},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]},
  llvm_fixtures: llvmFixtures.map(({name}) => ({name, offset: assembled.get(name).offset, hex: assembled.get(name).bytes.toString('hex')})),
  claim: 'bounded explicit synchronous callback NeedCode code installation with migrated private outer generation and actual guest callee/RET continuation; host unsigned scalar result choice only, no automatic discovery, async installation, stale/SMC migration, provider, scheduler, game, performance or full P2-V0 claim; final-stage CPU/snapshot races and generation limits are native/private evidence',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, artifacts: outputDir}));
