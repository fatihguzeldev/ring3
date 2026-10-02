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
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'compile_with_gates', 'capture_call', 'complete_call', 'abandon_call', 'begin_callback', 'finish_callback', 'abort_callback', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'store32'];
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

const fixtureRoot = join(root, 'engine/tests/fixtures/p2-callbacks');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(outputDir, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), assembled = elfFixtures(object);
const callerBytes = [0x6a, 0xff, 0x6a, 0, 0xe8, 0xf7, 0, 0, 0, 0x90];
const callbackBytes = [0x8b, 0x44, 0x24, 4, 0x8b, 0x54, 0x24, 8, 0x01, 0xd0, 0x50, 0xe8, 0xf0, 0xfd, 0xff, 0xff];
const afterInnerBytes = [0x83, 0xc0, 1, 0xc2, 8, 0];
const smcBytes = [0x89, 0x07, 0xc2, 8, 0];
const llvmFixtures = [
  {name: 'outer_caller', offset: 0, bytes: callerBytes},
  {name: 'outer_gate', offset: 0x100, bytes: [0x0f, 0x0b]},
  {name: 'inner_gate', offset: 0x200, bytes: [0x0f, 0x0b]},
  {name: 'callback_return_gate', offset: 0x300, bytes: [0x0f, 0x0b]},
  {name: 'callback_entry', offset: 0x400, bytes: callbackBytes},
  {name: 'callback_after_inner', offset: 0x410, bytes: afterInnerBytes},
  {name: 'callback_smc', offset: 0x500, bytes: smcBytes},
];
for (const {name, offset, bytes} of llvmFixtures) {
  assert.deepEqual([...assembled.get(name).bytes], bytes, `independent LLVM callback fixture ${name}`);
  assert.equal(assembled.get(name).offset, offset);
}

const outerId = 101, innerId = 102, returnId = 103;
let captures = 0, callCompletions = 0, begins = 0, committedStaleBegins = 0;
let finishes = 0, aborts = 0, abandons = 0, rejectedOperations = 0;

function fail(engine, operation, status, label) {
  const before = arena(engine);
  assert.equal(operation(), status, label);
  rejectedOperations++;
  assert.deepEqual(arena(engine), before, `${label}: all arena bytes unchanged`);
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
  assert.deepEqual(after.slice(140, 252), callRecord(token, id, stopped, returnPc, values), 'literal single-use call record in shared token namespace');
  assert.deepEqual(after.slice(0, 140), before.slice(0, 140));
  assert.deepEqual(after.slice(252), before.slice(252));
}

function callbackRecord(test, phase, outcome = 0, result = 0) {
  const bytes = new Uint8Array(64), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3CB', 64);
  [2, 1, phase, outcome, test.entry, test.callbackState.registers[4], 0x1300, returnId, 2, result, test.child.generation, 0].forEach((value, index) => view.setUint32(16 + 4 * index, value, true));
  return bytes;
}

function setArguments(test, values) {
  refresh(test.engine);
  values.forEach((value, index) => test.engine.view.setUint32(test.engine.base + 140 + 4 * index, value, true));
}

function begin(test, values, status = 0) {
  const {engine, child, callbackState} = test;
  setArguments(test, values);
  const before = arena(engine);
  assert.equal(engine.api.begin_callback(engine.low, engine.high, child.generation, 1, test.entry, 0x1300, returnId, values.length), status, 'actual process callback begin');
  begins++;
  if (status === 17) committedStaleBegins++;
  assert.deepEqual(readState(engine), callbackState, 'begin changes only EIP/ESP from private outer state');
  const after = arena(engine);
  assert.deepEqual(after.slice(56, 96), exit(3, 0), 'begin publishes NeedCode v3 without retiring instructions');
  assert.deepEqual(after.slice(96, 140), before.slice(96, 140), 'begin preserves cancellation/helper');
  assert.deepEqual(after.slice(140, 204), callbackRecord(test, 1, status === 17 ? 1 : 0), 'complete independent Started R3CB64 record');
  assert.deepEqual(after.slice(204), before.slice(204));
  assert.equal(guestValue(engine, callbackState.registers[4]), 0x1300);
  values.forEach((value, index) => assert.equal(guestValue(engine, (callbackState.registers[4] + 4 * (index + 1)) >>> 0), value, 'ordered callback frame argument'));
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
  assert.deepEqual(arena(engine).slice(96), before.slice(96), 'call completion preserves helper/cancel/RAM input record');
  return expected;
}

function abort(test) {
  const {engine} = test, before = arena(engine);
  assert.equal(engine.api.abort_callback(engine.low, engine.high, 2), 0, 'explicit callback forced unwind');
  aborts++;
  assert.deepEqual(arena(engine).slice(0, 96), test.outerRecords, 'abort restores byte-exact private outer state/exit');
  assert.deepEqual(arena(engine).slice(96), before.slice(96), 'abort preserves helper/cancel/transfer and guest effects');
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

function innerState(test, values) {
  const expected = copyState(test.callbackState);
  expected.registers[0] = Number(BigInt.asUintN(32, BigInt(values[0]) + BigInt(values[1])));
  expected.registers[2] = values[1];
  expected.registers[4] = (expected.registers[4] - 8) >>> 0;
  expected.eip = 0x1200;
  expected.eflags = addFlags(values[0], values[1], expected.eflags);
  return expected;
}

function prepare(stackTop = 0x9000, smc = false) {
  const engine = fresh();
  const stackPages = stackTop < 0x100 ? [0, 0xfffff000] : stackTop === 0x2010 ? [0x2000] : stackTop === 0x9014 ? [0x8000, 0x9000] : [0x8000];
  for (const address of stackPages) assert.equal(engine.api.map(address, 1, 3), 0);
  upload(engine, 0x1000, Uint8Array.from(callerBytes));
  for (const address of [0x1100, 0x1200, 0x1300]) upload(engine, address, Uint8Array.from([0x0f, 0x0b]));
  upload(engine, 0x1400, Uint8Array.from([...callbackBytes, ...afterInnerBytes]));
  upload(engine, 0x1500, Uint8Array.from(smcBytes));
  const specs = smc ? [[0x1000, 9], [0x1009, 1], [0x1100, 2], [0x1300, 2], [0x1500, 5]] : [[0x1000, 9], [0x1009, 1], [0x1100, 2], [0x1200, 2], [0x1300, 2], [0x1400, 16], [0x1410, 6]];
  const gates = smc ? [[0x1100, outerId], [0x1300, returnId]] : [[0x1100, outerId], [0x1200, innerId], [0x1300, returnId]];
  const child = compile(engine, specs, gates, ['read32', 'store32']);
  const start = state();
  start.registers[4] = stackTop;
  if (smc) start.registers[7] = 0x1300;
  const outer = copyState(start);
  outer.registers[4] = (stackTop - 12) >>> 0;
  outer.eip = 0x1100;
  writeState(engine, start);
  run(engine, child, 4, outer, exit(8, 3, outerId), 'actual authored outer CALL reaches numeric API gate', helperRecord(0));
  const callbackState = copyState(outer);
  callbackState.registers[4] = (outer.registers[4] - 12) >>> 0;
  callbackState.eip = smc ? 0x1500 : 0x1400;
  const test = {engine, child, start, outer, callbackState, entry: callbackState.eip, outerRecords: arena(engine).slice(0, 96)};
  capture(test, 1, outerId, outer, 0x1009, [0, 0xffffffff]);
  return test;
}

function outerCompleteAndResume(test, result) {
  const completed = complete(test, 1, test.outer, 2, 0x1009, result);
  const resumed = copyState(completed);
  resumed.eip = 0x100a;
  run(test.engine, test.child, 2, resumed, exit(3, 1), 'actual outer caller resumes once after process completion', arena(test.engine).slice(100, 140));
}

for (const [values, stackTop, singleStepRet] of [[[7, 9], 0x9000, true], [[0xffffffff, 1], 0x9000, false], [[7, 9], 16, true]]) {
  const test = prepare(stackTop), {engine, child} = test;
  begin(test, values);
  fail(engine, () => engine.api.compile(0), 12, 'active callback pins artifact even without inner pending');
  fail(engine, () => engine.api.begin_callback(engine.low, engine.high, child.generation, 1, 0x1400, 0x1300, returnId, 99), 12, 'callback-inside-callback precedes request validation');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, 1, 0), 12, 'suspended outer cannot complete during callback');
  fail(engine, () => engine.api.abandon_call(engine.low, engine.high, 1), 12, 'suspended outer cannot be abandoned during callback');
  const inner = innerState(test, values);
  run(engine, child, 6, inner, exit(8, 5, innerId), 'actual callback arithmetic/call yields ordinary inner API', helperRecord(0));
  capture(test, 3, innerId, inner, 0x1410, [inner.registers[0]]);
  fail(engine, () => child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 12, 'inner pending parks CPU before invalid pointer access');
  fail(engine, () => engine.api.finish_callback(engine.low, engine.high, child.generation, 2), 12, 'inner pending precedes callback finish stop validation');
  const innerResult = (inner.registers[0] ^ 0x80000000) >>> 0; // explicit host scalar choice, no provider.
  const resumed = complete(test, 3, inner, 1, 0x1410, innerResult);
  const added = copyState(resumed);
  added.registers[0] = (innerResult + 1) >>> 0;
  added.eip = 0x1413;
  added.eflags = addFlags(innerResult, 1, added.eflags);
  const returned = copyState(added);
  returned.registers[4] = test.outer.registers[4];
  returned.eip = 0x1300;
  if (singleStepRet) {
    run(engine, child, 1, added, exit(1, 1), 'callback ADD budget checkpoint', arena(engine).slice(100, 140));
    run(engine, child, 1, returned, exit(1, 1), 'real RET8 commits once then budget wins at return gate', helperRecord(0, 0x1300));
    run(engine, child, 1, returned, exit(8, 0, returnId), 'next positive run reaches return gate without repeated pop', helperRecord(0, 0x1300));
  } else run(engine, child, 3, returned, exit(8, 2, returnId), 'actual callback continuation and RET8 reach distinct return gate', helperRecord(0, 0x1300));
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 2, 0), 13, 'reserved callback return gate cannot be captured as ordinary API');
  word(engine, test.callbackState.registers[4], 0xf1234567);
  word(engine, test.outer.registers[4], 0xf1234567);
  refresh(engine).bytes.fill(0xcc, engine.base + 140, engine.base + 204);
  const beforeFinish = arena(engine);
  assert.equal(engine.api.finish_callback(engine.low, engine.high, child.generation, 2), 0);
  finishes++;
  assert.deepEqual(arena(engine).slice(0, 96), test.outerRecords, 'finish restores byte-exact outer Gate state/exit');
  assert.deepEqual(arena(engine).slice(96, 140), beforeFinish.slice(96, 140));
  assert.deepEqual(arena(engine).slice(140, 204), callbackRecord(test, 2, 0, returned.registers[0]), 'Returned record captures EAX before outer restoration');
  assert.deepEqual(arena(engine).slice(204), beforeFinish.slice(204));
  assert.equal(guestValue(engine, test.callbackState.registers[4]), 0xf1234567);
  assert.equal(guestValue(engine, test.outer.registers[4]), 0xf1234567);
  fail(engine, () => engine.api.finish_callback(engine.low, engine.high, child.generation, 2), 14, 'returned callback token cannot finish twice');
  fail(engine, () => engine.api.abort_callback(engine.low, engine.high, 2), 14, 'returned callback token cannot be aborted');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, 3, 0), 14, 'consumed inner token cannot replay against restored outer');
  outerCompleteAndResume(test, returned.registers[0]);
}

{
  const test = prepare(0x9014), {engine, child} = test;
  word(engine, 0x8ffc, 0x11223344);
  assert.equal(engine.api.protect(0x9000, 1, 1), 0);
  setArguments(test, [7, 9]);
  fail(engine, () => engine.api.begin_callback(engine.low, engine.high, child.generation, 1, 0x1400, 0x1300, returnId, 2), 8, 'later argument write failure preserves all callback publication bytes');
  assert.equal(guestValue(engine, 0x8ffc), 0x11223344, 'preflight failure leaves writable return-word prefix unchanged');
  assert.equal(guestValue(engine, test.outer.registers[4]), 0x1009, 'failed callback setup preserves actual outer return word');
  fail(engine, () => child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 12, 'failed setup retains parked outer authority');
  assert.equal(engine.api.protect(0x9000, 1, 3), 0);
  begin(test, [7, 9]); // callback token remains 2 after the failed setup.
  abort(test);
  assert.equal(guestValue(engine, 0x8ffc), 0x1300, 'abort retains committed callback return word');
  assert.equal(guestValue(engine, 0x9000), 7, 'abort retains committed callback argument');
  outerCompleteAndResume(test, 0x80000000);
}

{
  const test = prepare(), {engine, child} = test, values = [7, 9];
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  setArguments(test, values);
  fail(engine, () => engine.api.begin_callback(engine.low, engine.high, child.generation, 1, 0x1400, 0x1300, returnId, 2), 16, 'cancelled setup is atomic and retains outer authority');
  engine.view.setUint32(engine.base + 96, 0, true);
  begin(test, values);
  const loaded = copyState(test.callbackState);
  loaded.registers[0] = 7;
  loaded.registers[2] = 9;
  loaded.eip = 0x1408;
  run(engine, child, 2, loaded, exit(1, 2), 'callback runs two real loads before cancellation', helperRecord(0, 9));
  engine.view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 100, loaded, exit(2, 0), 'cancellation parks callback at precise instruction without retirement', helperRecord(0, 9));
  engine.view.setUint32(engine.base + 96, 0, true);
  const inner = innerState(test, values);
  run(engine, child, 4, inner, exit(8, 3, innerId), 'cleared cancellation resumes remaining callback instructions once', helperRecord(0));
  capture(test, 3, innerId, inner, 0x1410, [16]);
  engine.view.setUint32(engine.base + 96, 1, true);
  abort(test); // forced unwind discards the active inner token and ignores cancellation.
  assert.equal(guestValue(engine, inner.registers[4]), 0x1410, 'abort preserves inner guest CALL side effect');
  assert.equal(guestValue(engine, (inner.registers[4] + 4) >>> 0), 16, 'abort preserves inner guest pushed value');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, 3, 0), 14, 'aborted inner authority cannot replay');
  fail(engine, () => engine.api.finish_callback(engine.low, engine.high, child.generation, 2), 14, 'aborted callback cannot finish');
  fail(engine, () => engine.api.abort_callback(engine.low, engine.high, 2), 14, 'abort token is single use');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, 1, 0), 16, 'restored outer completion still respects preserved cancellation');
  engine.view.setUint32(engine.base + 96, 0, true);
  outerCompleteAndResume(test, 0xcafebabe);
}

{
  const test = prepare(0x9000, true), {engine, child} = test;
  begin(test, [7, 9]);
  const changed = copyState(test.callbackState);
  changed.eip = 0x1502;
  run(engine, child, 1, changed, exit(6, 1), 'actual callback code write commits once then invalidates before RET', helperRecord(0));
  assert.equal(guestValue(engine, 0x1300), test.start.registers[0], 'committed code write remains visible');
  fail(engine, () => child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 4, 'stale code rejection precedes callback/pointer checks');
  fail(engine, () => engine.api.finish_callback(engine.low, engine.high, child.generation, 2), 4, 'finish rejects stale pinned artifact');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, 1, 0), 4, 'stale code precedes suspended outer busy status');
  fail(engine, () => engine.api.compile(0), 12, 'active stale callback still pins compilation');
  abort(test);
  assert.equal(guestValue(engine, 0x1300), test.start.registers[0], 'forced abort does not roll back guest code');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, 1, 0), 4, 'outer completion remains stale after abort');
  const before = arena(engine);
  assert.equal(engine.api.abandon_call(engine.low, engine.high, 1), 0, 'explicit stale outer abandon releases authority');
  abandons++;
  assert.deepEqual(arena(engine), before);
}

{
  // all real outer PUSH/CALL writes land in data page 0x2000. callback setup
  // then crosses into the admitted code page without touching instruction bytes.
  const test = prepare(0x2010), {engine, child} = test;
  assert.equal(test.outer.registers[4], 0x2004);
  assert.equal(test.callbackState.registers[4], 0x1ff8);
  begin(test, [7, 9], 17);
  assert.equal(guestValue(engine, 0x1300), 0x00000b0f, 'gate bytes remain identical despite code-page version change');
  fail(engine, () => child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 4, 'committed setup status17 immediately rejects old code artifact');
  fail(engine, () => engine.api.finish_callback(engine.low, engine.high, child.generation, 2), 4, 'committed stale callback remains explicit abortable authority');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, 1, 0), 4, 'committed setup cannot masquerade as failed begin');
  abort(test);
  assert.equal(guestValue(engine, 0x1ff8), 0x1300);
  assert.equal(guestValue(engine, 0x1ffc), 7);
  assert.equal(guestValue(engine, 0x2000), 9, 'abort preserves all committed setup words');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, 1, 0), 4, 'restored outer remains pinned to invalidated generation');
  const before = arena(engine);
  assert.equal(engine.api.abandon_call(engine.low, engine.high, 1), 0);
  abandons++;
  assert.deepEqual(arena(engine), before);
}

const provenance = {
  engine_sha256: hash(engineBytes), generated_modules: modules, actual_engine_runs: actualRuns,
  successful_captures: captures, successful_call_completions: callCompletions,
  successful_callback_begins: begins, committed_stale_begins: committedStaleBegins,
  successful_callback_finishes: finishes, explicit_callback_aborts: aborts, explicit_outer_abandons: abandons,
  rejected_operations: rejectedOperations, synthetic_helper_runs: 0,
  generated_set_sha256: hash(generatedHashes.join('\n')),
  sources: {assembly_sha256: hash(readFileSync(assembly)), node_sha256: hash(readFileSync(join(fixtureRoot, 'run.mjs')))}, artifacts: {object_sha256: hash(object)},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]},
  llvm_fixtures: llvmFixtures.map(({name}) => ({name, offset: assembled.get(name).offset, hex: assembled.get(name).bytes.toString('hex')})),
  claim: 'one bounded actual guest callback with ordinary inner API reentry, real RET8, process-owned outer restoration and explicit abort; host unsigned scalar result choice only, no provider, scheduler, arbitrary nesting, generation migration, game, performance or full P2-V0 claim; setup Memory8 is actual atomic status evidence, exact denied-address precedence is native evidence',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, artifacts: outputDir}));
