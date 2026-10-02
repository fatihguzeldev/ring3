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
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'compile_with_gates', 'capture_call', 'complete_call', 'abandon_call', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'store32'];
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

const fixtureRoot = join(root, 'engine/tests/fixtures/p2-process-call');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(outputDir, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), assembled = elfFixtures(object);
const args = [0, 0xffffffff, 0x80000000, 0x7fffffff, 0x01020304, 0x11223344, 0x55aa55aa, 0xdeadbeef, 42, 7, 0xffffff80, 0x80, 0xabcdef01, 0xfedcba98, 0x12345678, 0xcafebabe];
// literal independently authored opcode forms; no iced encoder or DBT-derived bytes.
const pushes = [[0x6a, 0], [0x6a, 0xff], [0x68, 0, 0, 0, 0x80], [0x68, 0xff, 0xff, 0xff, 0x7f], [0x68, 4, 3, 2, 1], [0x68, 0x44, 0x33, 0x22, 0x11], [0x68, 0xaa, 0x55, 0xaa, 0x55], [0x68, 0xef, 0xbe, 0xad, 0xde], [0x6a, 42], [0x6a, 7], [0x6a, 0x80], [0x68, 0x80, 0, 0, 0], [0x68, 1, 0xef, 0xcd, 0xab], [0x68, 0x98, 0xba, 0xdc, 0xfe], [0x68, 0x78, 0x56, 0x34, 0x12], [0x68, 0xbe, 0xba, 0xfe, 0xca]];
const conventionNames = {1: 'cdecl', 2: 'stdcall', 3: 'thiscall'};
const programs = new Map(), llvmFixtures = [];
for (const tag of [1, 2, 3]) for (const count of [0, 2, 16]) {
  const name = `${conventionNames[tag]}_${count}`;
  const prefix = [...(tag === 3 ? [0xb9, 0x67, 0x45, 0x23, 0xf1] : []), ...pushes.slice(0, count).reverse().flat()];
  const call = tag === 1 ? [0xe8, 0x100 - prefix.length - 5, 0, 0, 0] : tag === 2 ? [0xff, 0x13] : [0xff, 0xd6];
  const callLength = prefix.length + call.length;
  const bytes = [...prefix, ...call, ...(tag === 1 && count > 0 ? [0x83, 0xc4, count * 4] : []), 0x90];
  assert.deepEqual([...assembled.get(name).bytes], bytes, `independent LLVM ${name}`);
  assert.deepEqual([...assembled.get(`${name}_gate`).bytes], [0x0f, 0x0b]);
  assert.equal(assembled.get(`${name}_gate`).offset - assembled.get(name).offset, 0x100, 'local direct CALL target at marker');
  programs.set(name, {tag, count, bytes, callLength});
  llvmFixtures.push(name, `${name}_gate`);
}

const gateId = 0xfedcba98;
let captures = 0, completions = 0, abandons = 0, rejectedOperations = 0;

function fail(engine, operation, status, label) {
  const before = arena(engine);
  assert.equal(operation(), status, label);
  rejectedOperations++;
  assert.deepEqual(arena(engine), before, `${label}: all arena bytes unchanged`);
}

function frame(token, tag, values, stopped, returnPc) {
  const bytes = new Uint8Array(112), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3CF', 112);
  [token, gateId, tag, values.length, stopped.eip, stopped.registers[4], returnPc, tag === 3 ? stopped.registers[1] : 0].forEach((value, index) => view.setUint32(16 + 4 * index, value, true));
  values.forEach((value, index) => view.setUint32(48 + 4 * index, value, true));
  return bytes;
}

function capture(engine, child, tag, values, stopped, returnPc, token = 1) {
  const before = arena(engine);
  assert.equal(engine.api.capture_call(engine.low, engine.high, child.generation, tag, values.length), 0, 'actual process capture');
  captures++;
  const after = arena(engine);
  assert.deepEqual(after.slice(0, 140), before.slice(0, 140), 'capture leaves full CPU/exit/cancel/helper unchanged');
  assert.deepEqual(after.slice(140, 252), frame(token, tag, values, stopped, returnPc), 'complete independent R3CF112 golden record, unused args zero');
  assert.deepEqual(after.slice(252), before.slice(252), 'capture changes only 112 transfer bytes');
  return token;
}

function complete(engine, child, tag, count, stopped, returnPc, token, result) {
  const before = arena(engine), expected = copyState(stopped);
  expected.registers[0] = result;
  expected.registers[4] = (stopped.registers[4] + 4 + (tag === 1 ? 0 : count * 4)) >>> 0;
  expected.eip = returnPc;
  assert.equal(engine.api.complete_call(engine.low, engine.high, child.generation, token, result), 0, 'actual process completion only');
  completions++;
  assert.deepEqual(readState(engine), expected, 'completion exact result/cleanup/captured return; other GPR/flags preserved');
  const after = arena(engine);
  assert.deepEqual(after.slice(56, 96), exit(3, 0), 'completion consumes Gate with canonical NeedCode v3 and no guest retirement');
  assert.deepEqual(after.slice(96), before.slice(96), 'completion leaves cancel/helper/transfer unchanged');
  return expected;
}

function abandon(engine, token) {
  const before = arena(engine);
  assert.equal(engine.api.abandon_call(engine.low, engine.high, token), 0, 'explicit process abandon');
  abandons++;
  assert.deepEqual(arena(engine), before, 'abandon leaves all bytes unchanged');
}

function prepare(tag, count, stackTop = 0x9000) {
  const engine = fresh(), program = programs.get(`${conventionNames[tag]}_${count}`);
  assert.equal(engine.api.map(0x4000, 1, 3), 0);
  for (const address of stackTop < 0x100 ? [0, 0xfffff000] : stackTop === 0x8004 ? [0x7000, 0x8000] : [0x8000]) assert.equal(engine.api.map(address, 1, 3), 0);
  word(engine, 0x4000, 0x1100);
  upload(engine, 0x1000, Uint8Array.from(program.bytes));
  upload(engine, 0x1100, Uint8Array.from([0x0f, 0x0b]));
  const returnPc = 0x1000 + program.callLength;
  const child = compile(engine, [[0x1000, program.callLength], [returnPc, program.bytes.length - program.callLength], [0x1100, 2]], [[0x1100, gateId]], tag === 2 ? ['read32', 'store32'] : ['store32']);
  const start = state();
  start.registers[3] = 0x4000;
  start.registers[4] = stackTop;
  start.registers[6] = 0x1100;
  const stopped = copyState(start);
  stopped.registers[4] = (stackTop - count * 4 - 4) >>> 0;
  stopped.eip = 0x1100;
  if (tag === 3) stopped.registers[1] = 0xf1234567;
  writeState(engine, start);
  const retired = count + 1 + (tag === 3 ? 1 : 0);
  run(engine, child, retired + 1, stopped, exit(8, retired, gateId), `${conventionNames[tag]}${count} authored CALL reaches gate`, helperRecord(0));
  return {engine, child, program, returnPc, start, stopped};
}

// independent BigInt ADD oracle for the cdecl caller's actual ADD ESP, imm8.
// the completion itself preserves flags and retires no guest instruction.
function addFlags(left, right, old) {
  const wide = BigInt(left) + BigInt(right), value = Number(wide & 0xffffffffn);
  let flags = 2 | (old & 0x400);
  if (wide > 0xffffffffn) flags |= 1;
  if (([...Array(8).keys()].reduce((bits, bit) => bits + ((value >>> bit) & 1), 0) & 1) === 0) flags |= 4;
  if (((left & 15) + (right & 15)) > 15) flags |= 0x10;
  if (value === 0) flags |= 0x40;
  if (value >= 0x80000000) flags |= 0x80;
  if ((~(left ^ right) & (left ^ value) & 0x80000000) !== 0) flags |= 0x800;
  return flags;
}

function callerResume(test, completed) {
  const {engine, child, program, start} = test, expected = copyState(completed);
  expected.eip = 0x1000 + program.bytes.length;
  const cleanup = program.tag === 1 && program.count > 0;
  if (cleanup) {
    expected.registers[4] = start.registers[4];
    expected.eflags = addFlags(completed.registers[4], program.count * 4, completed.eflags);
  }
  run(engine, child, 3, expected, exit(3, cleanup ? 2 : 1), 'actual caller resumes once after process completion', arena(engine).slice(100, 140));
}

for (const tag of [1, 2, 3]) for (const count of [0, 2, 16]) {
  const test = prepare(tag, count), {engine, child, stopped, returnPc} = test;
  const values = args.slice(0, count), token = capture(engine, child, tag, values, stopped, returnPc);
  fail(engine, () => child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 12, 'pending CPU guard rejects before invalid pointers');
  const result = Number(BigInt.asUintN(32, 0x80000000n + BigInt(values[0] ?? 0) + BigInt(values.at(-1) ?? 0) + BigInt(tag === 3 ? 0xf1234567 : 0)));
  // mutable transfer/RAM are not the privately captured frame. completion must
  // use the original return/arguments, with no guest reads or return-word writes.
  word(engine, stopped.registers[4], 0xf1234567);
  if (count > 0) word(engine, (stopped.registers[4] + 4) >>> 0, 0x55667788);
  refresh(engine).bytes.fill(0xcc, engine.base + 140, engine.base + 252);
  const completed = complete(engine, child, tag, count, stopped, returnPc, token, result);
  assert.equal(guestValue(engine, stopped.registers[4]), 0xf1234567, 'completion does not pop/write current return RAM');
  if (count > 0) assert.equal(guestValue(engine, (stopped.registers[4] + 4) >>> 0), 0x55667788);
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, token, 0), 14, 'single-use completion rejects replay');
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, tag, count), 13, 'consumed Gate cannot be recaptured before a fresh CPU stop');
  callerResume(test, completed);
}

{
  const test = prepare(1, 2), {engine, child, stopped, returnPc} = test;
  const token = capture(engine, child, 1, args.slice(0, 2), stopped, returnPc);
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 0, 99), 12, 'pending precedes malformed tag/count');
  fail(engine, () => engine.api.compile(0), 12, 'pending legacy compile rejects before descriptor counts');
  fail(engine, () => engine.api.compile_with_gates(99, 99), 12, 'pending gated compile rejects before descriptor reads');
  fail(engine, () => engine.api.capture_call(engine.low ^ 1, engine.high, child.generation, 0, 99), 3, 'session key precedes pending and malformed request');
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation + 1, 0, 99), 3, 'generation precedes pending and malformed request');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, token + 1, 1), 14, 'wrong token preserves pending frame');
  const saved = arena(engine).slice(0, 96);
  refresh(engine).view.setUint32(engine.base + 76, 1234, true);
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, token, 1), 15, 'changed saved exit rejects completion');
  engine.bytes.set(saved, engine.base);
  engine.view.setUint32(engine.base + 16, 0x11223344, true);
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, token, 1), 15, 'changed saved CPU state rejects completion');
  engine.bytes.set(saved, engine.base);
  engine.view.setUint32(engine.base + 96, 1, true);
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, token, 1), 16, 'cancelled completion preserves pending');
  engine.view.setUint32(engine.base + 96, 0, true);
  fail(engine, () => engine.api.abandon_call(engine.low ^ 1, engine.high, token + 1), 3, 'abandon key precedes bad token');
  abandon(engine, token);
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, token, 1), 14, 'abandoned token cannot complete');
  const renewed = capture(engine, child, 1, args.slice(0, 2), stopped, returnPc, 2);
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, token, 1), 14, 'old token does not consume renewed pending frame');
  const completed = complete(engine, child, 1, 2, stopped, returnPc, renewed, 0x80000000);
  callerResume(test, completed);
}

{
  const test = prepare(2, 2), {engine, child, stopped, returnPc} = test;
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 0, 2), 7, 'bad convention tag');
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 2, 17), 7, 'stack word count above sixteen');
  const saved = arena(engine).slice(0, 96);
  refresh(engine).view.setUint16(engine.base + 4, 2, true);
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 0, 99), 7, 'bad request precedes malformed state');
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 2, 2), 13, 'malformed state is not a capturable stop');
  engine.bytes.set(saved, engine.base);
  engine.view.setUint32(engine.base + 48, 0x1101, true);
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 2, 2), 13, 'gate second byte is not exact bound PC');
  engine.bytes.set(saved, engine.base);
  engine.view.setUint32(engine.base + 80, 7, true);
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 2, 2), 13, 'unbound numeric ID is not a capturable stop');
  engine.bytes.set(saved, engine.base);
  engine.view.setUint32(engine.base + 96, 1, true);
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 2, 2), 16, 'capture cancellation precedes guest reads');
  engine.view.setUint32(engine.base + 96, 0, true);
  assert.equal(engine.api.protect(0x8000, 1, 2), 0);
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 2, 2), 8, 'return word Read4 permission failure is atomic');
  assert.equal(engine.api.protect(0x8000, 1, 3), 0);
  const token = capture(engine, child, 2, args.slice(0, 2), stopped, returnPc);
  abandon(engine, token); // Failed requests consumed neither token nor output.
}

{
  const test = prepare(2, 16, 0x8004), {engine, child, stopped, returnPc} = test;
  assert.equal(stopped.registers[4], 0x7fc0);
  assert.equal(engine.api.protect(0x8000, 1, 2), 0);
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 2, 16), 8, 'last argument Read4 denied after accessible return/first fifteen');
  assert.equal(engine.api.protect(0x8000, 1, 3), 0);
  const token = capture(engine, child, 2, args, stopped, returnPc);
  assert.equal(engine.api.unmap(0x7000, 2), 0);
  const completed = complete(engine, child, 2, 16, stopped, returnPc, token, 0xffffffff);
  callerResume(test, completed); // Completion succeeds without rereading unmapped captured RAM.
}

{
  const test = prepare(1, 2, 8), {engine, child, stopped, returnPc} = test;
  assert.equal(stopped.registers[4], 0xfffffffc);
  assert.equal(guestValue(engine, 0xfffffffc), returnPc);
  assert.equal(guestValue(engine, 0), args[0]);
  assert.equal(guestValue(engine, 4), args[1]);
  const token = capture(engine, child, 1, args.slice(0, 2), stopped, returnPc);
  const completed = complete(engine, child, 1, 2, stopped, returnPc, token, 0x80000000);
  assert.equal(completed.registers[4], 0, 'cdecl return wraps ESP before caller cleanup');
  callerResume(test, completed);
  assert.equal(readState(engine).registers[4], 8);
  assert.equal(readState(engine).eflags, 0x402, 'literal ADD ESP 0+8 preserves DF and clears arithmetic flags');
}

{
  const test = prepare(2, 0, 0), {engine, child, stopped, returnPc} = test;
  assert.equal(engine.api.protect(0, 1, 4), 0, 'hypothetical first argument page cannot be read');
  const token = capture(engine, child, 2, [], stopped, returnPc);
  const completed = complete(engine, child, 2, 0, stopped, returnPc, token, 0x80000000);
  assert.equal(completed.registers[4], 0, 'zero-argument return at final word wraps ESP to zero');
  callerResume(test, completed);
}

for (const [esp, count] of [[0xfffffffd, 0], [0xfffffff9, 1]]) {
  const engine = fresh();
  assert.equal(engine.api.map(0xfffff000, 1, 3), 0);
  if (count) word(engine, esp, 0x12345678);
  upload(engine, 0x1100, Uint8Array.from([0x0f, 0x0b]));
  const child = compile(engine, [[0x1100, 2]], [[0x1100, gateId]]);
  const stopped = state(0x1100);
  stopped.registers[4] = esp;
  writeState(engine, stopped);
  run(engine, child, 1, stopped, exit(8, 0, gateId), 'preset stack pointer reaches actual helper-free numeric gate', arena(engine).slice(100, 140));
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 2, count), 8, `${count ? 'argument' : 'return'} width4 overflow capture failure is atomic`);
}

{
  const test = prepare(3, 0), {engine, child, stopped, returnPc} = test;
  const token = capture(engine, child, 3, [], stopped, returnPc);
  assert.equal(engine.api.protect(0x1000, 1, 7), 0);
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  fail(engine, () => child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 4, 'stale code precedes pending and bad pointers');
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 0, 99), 4, 'stale code precedes pending and invalid request');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, token + 1, 1), 4, 'stale code precedes token and cancellation');
  abandon(engine, token); // Intentionally works despite stale code and cancellation.
  const replacement = compile(engine, [[0x1100, 2]], [[0x1100, gateId]]);
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 3, 0), 3, 'old generation cannot capture after replacement');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, token, 1), 3, 'old generation precedes missing token');
  fail(engine, () => engine.api.capture_call(engine.low ^ 1, engine.high, replacement.generation, 3, 0), 3, 'current generation with wrong session key');
}

{
  const test = prepare(3, 2), {engine, child, stopped} = test;
  const unknown = 0xf1234567;
  word(engine, stopped.registers[4], unknown);
  const token = capture(engine, child, 3, args.slice(0, 2), stopped, unknown);
  const completed = complete(engine, child, 3, 2, stopped, unknown, token, 0xffffffff);
  run(engine, child, 2, completed, exit(3, 0), 'unknown captured return succeeds then CPU NeedCode without another pop', arena(engine).slice(100, 140));

  // a fresh Gate stop permits another parked frame; close consumes it permanently.
  writeState(engine, stopped);
  run(engine, child, 1, stopped, exit(8, 0, gateId), 'fresh actual gate observation after completion');
  const pending = capture(engine, child, 3, args.slice(0, 2), stopped, unknown, 2);
  assert.equal(engine.api.close(), 0);
  fail(engine, () => child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 5, 'closed context precedes pending/pointer checks');
  fail(engine, () => engine.api.capture_call(engine.low, engine.high, child.generation, 0, 99), 5, 'closed context precedes malformed capture');
  fail(engine, () => engine.api.complete_call(engine.low, engine.high, child.generation, pending, 1), 5, 'closed pending token cannot complete');
  fail(engine, () => engine.api.abandon_call(engine.low, engine.high, pending), 5, 'closed pending token cannot be abandoned');
}

const provenance = {
  engine_sha256: hash(engineBytes), generated_modules: modules, actual_engine_runs: actualRuns,
  successful_captures: captures, successful_completions: completions, explicit_abandons: abandons,
  rejected_operations: rejectedOperations, synthetic_helper_runs: 0,
  generated_set_sha256: hash(generatedHashes.join('\n')),
  sources: {assembly_sha256: hash(readFileSync(assembly))}, artifacts: {object_sha256: hash(object)},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]},
  llvm_fixtures: llvmFixtures.map(name => ({name, offset: assembled.get(name).offset, hex: assembled.get(name).bytes.toString('hex')})),
  claim: 'actual checked integer capture/single-use process completion and authored guest caller continuation; host scalar result selection only, no provider/pointee/Windows catalogue/callback/scheduler claim; Wasm Memory8 proof is atomic status only, exact denied-address precedence is native evidence',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, artifacts: outputDir}));
