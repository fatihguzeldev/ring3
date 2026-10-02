import assert from 'node:assert/strict';
import {execFileSync, spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, injectorPath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, Wasm injector, output directory and repository root');
const engineBytes = readFileSync(enginePath), injectorBytes = readFileSync(injectorPath);
assert.ok(WebAssembly.validate(engineBytes), 'actual engine validates');
assert.ok(WebAssembly.validate(injectorBytes), 'pure-Wasm helper injector validates');
const engineModule = new WebAssembly.Module(engineBytes), injectorModule = new WebAssembly.Module(injectorBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), [], 'engine has no JS imports');
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'store32'];
let keys = 0n, modules = 0, actualRuns = 0, injectedRuns = 0, resumes = 0;
const generatedHashes = [];

function hash(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function sortedImports(module) {
  return WebAssembly.Module.imports(module).sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
}

assert.deepEqual(sortedImports(injectorModule), [
  {module: 'env', name: 'memory', kind: 'memory'},
  {module: 'ring3', name: 'guard', kind: 'function'},
], 'injector imports only actual memory and actual Wasm guard');

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
  assert.equal(api.open(32, engine.low, engine.high), 0);
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

function compile(engine, bytes, blocks = [[0x1000, bytes.length]], expectedHelpers = []) {
  if (bytes.length !== 0) upload(engine, 0x1000, bytes);
  refresh(engine);
  blocks.forEach(([entry, length], index) => {
    engine.view.setUint32(engine.base + 140 + index * 8, entry, true);
    engine.view.setUint32(engine.base + 144 + index * 8, length, true);
  });
  assert.equal(engine.api.compile(blocks.length), 0, 'actual engine compiles checked stack region');
  const pointer = engine.api.module_ptr() >>> 0, length = engine.api.module_len() >>> 0;
  refresh(engine);
  assert.ok(pointer > 0 && length > 8 && pointer + length <= engine.bytes.length);
  const encoded = engine.bytes.slice(pointer, pointer + length);
  assert.ok(WebAssembly.validate(encoded));
  const module = new WebAssembly.Module(encoded);
  const expected = [
    {module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: 'guard', kind: 'function'},
    ...expectedHelpers.map(name => ({module: 'ring3', name, kind: 'function'})),
  ].sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sortedImports(module), expected, 'module imports exactly needed actual Wasm helpers');
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

function exit(reason, retired, detail = 0, address = 0, access = 0, length = reason === 5 ? 4 : 0) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3EX', 40, 2);
  [reason, retired, detail, address, access, length].forEach((field, index) => view.setUint32(16 + index * 4, field, true));
  return bytes;
}

function run(engine, child, budget, expected, expectedExit, label, expectedHelper = undefined, injected = false) {
  const before = arena(engine);
  assert.equal(child.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, `${label}: canonical runtime exit status`);
  if (injected) injectedRuns++; else actualRuns++;
  assert.deepEqual(readState(engine), expected, `${label}: complete CPU state`);
  const after = arena(engine);
  assert.deepEqual(after.slice(56, 96), expectedExit, `${label}: canonical v2 exit`);
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

const callBytes = [0xe8, 0xfb, 0, 0, 0], retBytes = [0xc3];
const nestedBytes = [0xb8, 1, 0, 0, 0, 0xe8, 1, 0, 0, 0, 0x90, 0x83, 0xc0, 2, 0xe8, 4, 0, 0, 0, 0x83, 0xc0, 4, 0xc3, 0x83, 0xc0, 8, 0xc3];
const recursiveBytes = [0xb8, 0, 0, 0, 0, 0xe8, 1, 0, 0, 0, 0x90, 0x83, 0xf9, 0, 0x74, 0x0b, 0x01, 0xc8, 0x83, 0xe9, 1, 0xe8, 0xf1, 0xff, 0xff, 0xff, 0xc3, 0xc3];
const arithmeticCall = [0xb8, 0xff, 0xff, 0xff, 0xff, 0x83, 0xc0, 1, 0xe8, 0xf3, 0, 0, 0];
const arithmeticRet = [0xb8, 0xff, 0xff, 0xff, 0xff, 0x83, 0xc0, 1, 0xc3];
const nestedBlocks = [[0x1000, 10], [0x100b, 8], [0x1013, 4], [0x1017, 4]];
const recursiveBlocks = [[0x1000, 10], [0x100b, 5], [0x1010, 10], [0x101a, 1], [0x101b, 1]];
const stackBase = 0x40000, stackPages = 20, stackTop = 0x54000;
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-stack');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(outputDir, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), assembled = elfFixtures(object);
const llvmFixtures = [
  {name: 'call_forward', bytes: callBytes}, {name: 'ret_plain', bytes: retBytes},
  {name: 'ret_imm_zero', bytes: [0xc2, 0, 0]}, {name: 'arithmetic_call', bytes: arithmeticCall},
  {name: 'arithmetic_ret', bytes: arithmeticRet}, {name: 'call_relative_five', bytes: [0xe8, 5, 0, 0, 0]},
  {name: 'nested', bytes: nestedBytes}, {name: 'recursive_sum', bytes: recursiveBytes},
];
for (const {name, bytes} of llvmFixtures) assert.deepEqual([...assembled.get(name).bytes], bytes, `LLVM independently assembled ${name}`);

function guarded(engine, child, label) {
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, label);
}

{
  const engine = fresh();
  assert.equal(engine.api.map(0x8000, 1, 3), 0);
  const child = compile(engine, Uint8Array.from(nestedBytes), nestedBlocks, ['read32', 'store32']);
  const start = state();
  start.registers[4] = 0x9000;
  const expected = copyState(start);
  expected.registers[0] = 15;
  expected.eflags = 0x406;
  expected.eip = 0x100a;
  writeState(engine, start);
  run(engine, child, 20, expected, exit(3, 8), 'nested calls return to literal caller continuation', helperRecord(0, 0x100a));
  assert.equal(guestValue(engine, 0x8ffc), 0x100a, 'outer call pushed its exact next EIP');
  assert.equal(guestValue(engine, 0x8ff8), 0x1013, 'inner call pushed its exact next EIP');
}

const oracleSource = join(fixtureRoot, 'stack_oracle.c'), oraclePath = join(outputDir, 'stack_oracle');
const oracleCompile = ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', oracleSource, '-o', oraclePath];
execFileSync('clang', oracleCompile, {stdio: ['ignore', 'pipe', 'pipe']});
const nativeInputs = [0, 1, 2, 31, 4097, 20000];
function recursiveFixture(count) {
  const engine = fresh();
  assert.equal(engine.api.map(stackBase, stackPages, 3), 0);
  const child = compile(engine, Uint8Array.from(recursiveBytes), recursiveBlocks, ['read32', 'store32']);
  const start = state();
  start.registers[1] = count;
  start.registers[4] = stackTop;
  const expected = copyState(start);
  expected.registers[0] = Number(BigInt.asUintN(32, BigInt(count) * BigInt(count + 1) / 2n));
  expected.registers[1] = 0;
  expected.eip = 0x100a;
  expected.eflags = 0x446;
  return {engine, child, start, expected};
}
for (const count of nativeInputs) {
  const native = JSON.parse(execFileSync(oraclePath, [String(count)], {encoding: 'utf8'}));
  const {engine, child, start, expected} = recursiveFixture(count);
  assert.deepEqual(native, {sum: expected.registers[0], remaining: 0, retired: 6 * count + 5}, 'native unsigned algorithm agrees with independent math');
  writeState(engine, start);
  run(engine, child, native.retired + 10, expected, exit(3, native.retired), `guest recursive algorithm/native ${count}`, helperRecord(0, 0x100a));
  assert.equal(guestValue(engine, stackTop - 4), 0x100a);
  if (count !== 0) assert.equal(guestValue(engine, stackTop - 4 * (count + 1)), 0x101a, 'deepest frame stores recursive continuation');
}
for (const args of [[], ['-1'], ['20001'], ['1x'], [' 1'], ['1', 'extra']]) {
  const invalid = spawnSync(oraclePath, args, {encoding: 'utf8'});
  assert.equal(invalid.status, 2, `native utility rejects ${JSON.stringify(args)}`);
  assert.equal(invalid.stdout, '');
}

{
  const count = 20000;
  const {engine, child, start, expected} = recursiveFixture(count);
  writeState(engine, start);
  const deepest = copyState(expected);
  deepest.registers[4] = stackTop - 4 * (count + 1);
  deepest.eip = 0x100b;
  run(engine, child, 5 * count + 2, deepest, exit(1, 5 * count + 2), '20001 guest frames checkpoint without host recursion', helperRecord(0));
  assert.equal(guestValue(engine, deepest.registers[4]), 0x101a, 'observed deepest return word');
  assert.equal(guestValue(engine, stackTop - 4), 0x100a, 'observed top caller return word');
  assert.equal(guestValue(engine, deepest.registers[4] + 4), 0x101a, 'observed adjacent deep frame');
  run(engine, child, count + 10, expected, exit(3, count + 3), '20001 guest frames unwind precisely', helperRecord(0, 0x100a));
}

{
  const {engine, child, start, expected} = recursiveFixture(31);
  writeState(engine, start);
  let total = 0, localResumes = 0;
  while (true) {
    assert.equal(child.run(engine.base, engine.base + 56, 7, engine.base + 96), 0);
    actualRuns++;
    resumes++;
    localResumes++;
    refresh(engine);
    const reason = engine.view.getUint32(engine.base + 72, true), retired = engine.view.getUint32(engine.base + 76, true);
    assert.ok(reason === 1 || reason === 3);
    assert.ok(retired <= 7);
    if (reason === 1) assert.equal(retired, 7);
    assert.deepEqual(arena(engine).slice(56, 96), exit(reason, retired), 'guest stack resume canonical exit');
    total += retired;
    if (reason === 3) break;
    assert.ok(localResumes < 40);
  }
  assert.equal(total, 6 * 31 + 5, 'resumes retire each guest CALL/RET once');
  assert.deepEqual(readState(engine), expected, 'resumes restore every guest frame and full CPU state');
}

{
  const engine = fresh();
  assert.equal(engine.api.map(0x8000, 1, 3), 0);
  upload(engine, 0x1100, Uint8Array.from(retBytes));
  const child = compile(engine, Uint8Array.from(callBytes), [[0x1000, 5], [0x1100, 1]], ['read32', 'store32']);
  const start = state();
  start.registers[4] = 0x9000;
  word(engine, 0x8ffc, 0xaaaaaaaa);
  writeState(engine, start);
  run(engine, child, 0, start, exit(1, 0), 'budget0 skips CALL push', arena(engine).slice(100, 140));
  assert.equal(guestValue(engine, 0x8ffc), 0xaaaaaaaa);
  writeState(engine, start, 1);
  run(engine, child, 0, start, exit(2, 0), 'cancel outranks budget before CALL push', arena(engine).slice(100, 140));
  assert.equal(guestValue(engine, 0x8ffc), 0xaaaaaaaa);
  writeState(engine, start);
  const called = copyState(start);
  called.registers[4] = 0x8ffc;
  called.eip = 0x1100;
  run(engine, child, 1, called, exit(1, 1), 'CALL commit checkpoint', helperRecord(0));
  assert.equal(guestValue(engine, 0x8ffc), 0x1005);
  word(engine, 0x8ffc, 0xf1234567);
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 10, called, exit(2, 0), 'cancel between CALL and RET', helperRecord(0));
  engine.view.setUint32(engine.base + 96, 0, true);
  const returned = copyState(start);
  returned.eip = 0xf1234567;
  run(engine, child, 1, returned, exit(1, 1), 'RET budget1 commits the pop before checking the unknown target', helperRecord(0, 0xf1234567));
  run(engine, child, 2, returned, exit(3, 0), 'RET checkpoint resumes at the returned PC without popping again', helperRecord(0, 0xf1234567));
  assert.equal(guestValue(engine, 0x8ffc), 0xf1234567, 'resuming RET did not repeat CALL push');
}

{
  const engine = fresh();
  assert.equal(engine.api.map(0x8000, 1, 3), 0);
  const child = compile(engine, Uint8Array.from(callBytes), undefined, ['store32']);
  const start = state();
  start.registers[4] = 0x9000;
  const expected = copyState(start);
  expected.registers[4] = 0x8ffc;
  expected.eip = 0x1100;
  writeState(engine, start);
  run(engine, child, 2, expected, exit(3, 1), 'unknown CALL target is NeedCode after successful push', helperRecord(0));
  assert.equal(guestValue(engine, 0x8ffc), 0x1005);
}

const faultCases = [
  {name: 'CALL unmapped', call: true, address: 0x8000, detail: 1, fault: 0x8000},
  {name: 'CALL write permission', call: true, address: 0x4000, detail: 2, fault: 0x4000, map: [[0x4000, 1, 1]]},
  {name: 'CALL cross-page permission', call: true, address: 0x4ffe, detail: 2, fault: 0x5000, map: [[0x4000, 1, 3], [0x5000, 1, 1]]},
  {name: 'CALL width overflow after ESP wrap', call: true, address: 0xfffffffd, detail: 3, fault: 0xfffffffd},
  {name: 'RET unmapped', call: false, address: 0x8000, detail: 1, fault: 0x8000},
  {name: 'RET read permission', call: false, address: 0x4000, detail: 2, fault: 0x4000, map: [[0x4000, 1, 2]]},
  {name: 'RET cross-page permission', call: false, address: 0x4ffe, detail: 2, fault: 0x5000, map: [[0x4000, 1, 3], [0x5000, 1, 2]]},
  {name: 'RET width overflow', call: false, address: 0xfffffffd, detail: 3, fault: 0xfffffffd},
];
for (const fault of faultCases) {
  const engine = fresh();
  for (const [address, pages] of fault.map ?? []) assert.equal(engine.api.map(address, pages, 3), 0);
  if (fault.map?.some(([address]) => address === 0x4000)) {
    word(engine, 0x4000, 0x11223344);
    word(engine, 0x4ffc, 0xa1b2c3d4);
  }
  if (fault.map?.some(([address]) => address === 0x5000)) word(engine, 0x5000, 0x55667788);
  for (const [address, pages, bits] of fault.map ?? []) assert.equal(engine.api.protect(address, pages, bits), 0);
  upload(engine, 0x1100, Uint8Array.from([0xb9, 0x99, 0x99, 0x99, 0x99]));
  const bytes = fault.call ? arithmeticCall : arithmeticRet;
  const child = compile(engine, Uint8Array.from(bytes), [[0x1000, bytes.length], [0x1100, 5]], [fault.call ? 'store32' : 'read32']);
  const start = state();
  start.registers[4] = fault.call ? (fault.address + 4) >>> 0 : fault.address;
  const expected = copyState(start);
  expected.registers[0] = 0;
  expected.eflags = 0x457;
  expected.eip = 0x1008;
  writeState(engine, start);
  const access = fault.call ? 2 : 1;
  run(engine, child, 20, expected, exit(5, 2, fault.detail, fault.fault, access), `${fault.name}: failed frame preserves ESP and computed flags`, helperRecord(1, 0, fault.detail, fault.fault, access));
  if (fault.call && fault.map) {
    assert.equal(guestValue(engine, 0x4000), 0x11223344, 'failed CALL preserves attempted writable/readable word');
    assert.equal(guestValue(engine, 0x4ffc), 0xa1b2c3d4, 'failed CALL preserves first-page tail');
    if (fault.map.some(([address]) => address === 0x5000)) assert.equal(guestValue(engine, 0x5000), 0x55667788, 'failed CALL preserves second-page head');
  }
  guarded(engine, child, 'failed stack helper preserves code stamps');
}

for (const call of [true, false]) {
  for (const low of [false, true]) {
    const engine = fresh();
    const address = low ? 0 : 0xfffffffc;
    assert.equal(engine.api.map(low ? 0 : 0xfffff000, 1, 3), 0);
    const child = compile(engine, Uint8Array.from(call ? callBytes : retBytes), undefined, [call ? 'store32' : 'read32']);
    const start = state();
    start.registers[4] = call ? (address + 4) >>> 0 : address;
    if (!call) word(engine, address, 0xf1234567);
    const expected = copyState(start);
    expected.registers[4] = call ? address : (address + 4) >>> 0;
    expected.eip = call ? 0x1100 : 0xf1234567;
    writeState(engine, start);
    run(engine, child, 2, expected, exit(3, 1), `${call ? 'CALL' : 'RET'} ${low ? 'zero-page' : 'high-page'} ESP wrap succeeds with nonwrapping width`, helperRecord(0, call ? 0 : 0xf1234567));
    if (call) assert.equal(guestValue(engine, address), 0x1005);
  }
}

{
  const engine = fresh();
  assert.equal(engine.api.map(0xfffff000, 1, 7), 0);
  assert.equal(engine.api.map(0x4000, 1, 3), 0);
  upload(engine, 0xfffffffb, Uint8Array.from([0xe8, 5, 0, 0, 0]));
  const child = compile(engine, new Uint8Array(), [[0xfffffffb, 5]], ['store32']);
  const start = state(0xfffffffb);
  start.registers[4] = 0x4004;
  const expected = copyState(start);
  expected.registers[4] = 0x4000;
  expected.eip = 5;
  writeState(engine, start);
  run(engine, child, 2, expected, exit(3, 1), 'CALL return address and direct target wrap at guest PC end', helperRecord(0));
  assert.equal(guestValue(engine, 0x4000), 0, 'CALL pushed wrapping next EIP');
}

{
  const engine = fresh();
  upload(engine, 0x1100, Uint8Array.from(retBytes));
  const child = compile(engine, Uint8Array.from(callBytes), [[0x1000, 5], [0x1100, 1]], ['read32', 'store32']);
  const start = state();
  start.registers[4] = 0x1008;
  const expected = copyState(start);
  expected.registers[4] = 0x1004;
  expected.eip = 0x1100;
  writeState(engine, start);
  run(engine, child, 1, expected, exit(6, 1), 'CALL code-page push commits ESP and target before SMC exit', helperRecord(0));
  assert.equal(guestValue(engine, 0x1004), 0x1005, 'CALL committed exact return word into guest code');
  const before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 10, 0xffffffff), 4, 'stale stack module rejected before generated access');
  actualRuns++;
  assert.deepEqual(arena(engine), before);
  const replacement = compile(engine, new Uint8Array(), [[0x1100, 1]], ['read32']);
  const returned = copyState(expected);
  returned.registers[4] = 0x1008;
  returned.eip = 0x1005;
  writeState(engine, expected);
  run(engine, replacement, 2, returned, exit(3, 1), 'recompiled callee RET pops committed SMC return word', helperRecord(0, 0x1005));
}

{
  const engine = fresh();
  upload(engine, 0x1000, Uint8Array.from([0xff, 0x34, 0x24]));
  refresh(engine).view.setUint32(engine.base + 140, 0x1000, true);
  engine.view.setUint32(engine.base + 144, 3, true);
  assert.equal(engine.api.compile(1), 10, 'PUSH [ESP] memory operand stays outside the single-access stack profile');
}

const injectedCases = [
  {name: 'VersionExhausted', status: 0, record: helperRecord(2, 0, 1), detail: 1},
  {name: 'OtherReturned', status: 0, record: helperRecord(2, 0, 2), detail: 3},
  {name: 'bad helper tag', status: 0, record: helperRecord(3), detail: 2},
  {name: 'closed helper', status: 5, record: helperRecord(0), detail: 3},
  {name: 'completion11 non-success', status: 11, record: helperRecord(2, 0, 1), detail: 2, callOnly: true},
  {name: 'RET completion11 rejected', status: 11, record: helperRecord(0), detail: 3, retOnly: true},
];
for (const call of [true, false]) {
  const engine = fresh();
  const bytes = call ? arithmeticCall : arithmeticRet;
  const actual = compile(engine, Uint8Array.from(bytes), undefined, [call ? 'store32' : 'read32']);
  const injector = new WebAssembly.Instance(injectorModule, {env: {memory: engine.memory}, ring3: {guard: engine.api.guard}}).exports;
  const injected = new WebAssembly.Instance(actual.module, {env: {memory: engine.memory}, ring3: {guard: injector.guard, read32: injector.read32, store32: injector.store32}}).exports;
  for (const fixture of injectedCases) {
    if ((call && fixture.retOnly) || (!call && fixture.callOnly)) continue;
    const start = state();
    start.registers[4] = call ? 0x9004 : 0x9000;
    const expected = copyState(start);
    expected.registers[0] = 0;
    expected.eflags = 0x457;
    expected.eip = 0x1008;
    writeState(engine, start);
    refresh(engine).bytes.set(fixture.record, engine.base + 140);
    injector.configure(engine.base + 100, engine.base + 140, fixture.status);
    run(engine, injected, 20, expected, exit(7, 2, fixture.detail), `synthetic ${call ? 'CALL' : 'RET'} ${fixture.name}: frame uncommitted`, undefined, true);
    assert.equal(injector.calls(), 1, 'failed stack instruction calls helper once');
    guarded(engine, actual, 'synthetic helper does not change actual guest code');
  }
}

const provenance = {
  engine_sha256: hash(engineBytes), injector_sha256: hash(injectorBytes), generated_modules: modules,
  actual_engine_runs: actualRuns, synthetic_helper_runs: injectedRuns, resumes,
  native_algorithm_cases: nativeInputs.length, native_invalid_inputs: 6, maximum_guest_frames: 20001,
  generated_set_sha256: hash(generatedHashes.join('\n')),
  sources: {assembly_sha256: hash(readFileSync(assembly)), native_c_sha256: hash(readFileSync(oracleSource))},
  artifacts: {object_sha256: hash(object), native_binary_sha256: hash(readFileSync(oraclePath))},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], native_compile: ['clang', ...oracleCompile]},
  llvm_fixtures: llvmFixtures.map(({name}) => ({name, offset: assembled.get(name).offset, hex: assembled.get(name).bytes.toString('hex')})),
  claim: 'actual engine E8 CALL/C3 RET with precise guest stack faults, bounded dispatcher recursion and CALL SMC; synthetic helpers prove emitter error branches only; native C proves unsigned algorithm, not x86 hardware flags or counters',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, artifacts: outputDir}));
