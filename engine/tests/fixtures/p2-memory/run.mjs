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
let keys = 0n, modules = 0, actualRuns = 0, injectedRuns = 0, resumes = 0, storeProbes = 0;
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

function compile(engine, bytes, blocks = [[0x1000, bytes.length]], expectedHelpers = []) {
  if (bytes.length !== 0) upload(engine, 0x1000, bytes);
  refresh(engine);
  blocks.forEach(([entry, length], index) => {
    engine.view.setUint32(engine.base + 140 + index * 8, entry, true);
    engine.view.setUint32(engine.base + 144 + index * 8, length, true);
  });
  assert.equal(engine.api.compile(blocks.length), 0, 'actual engine compiles checked RAM region');
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

const literals = [
  {name: 'load_ind', bytes: [0x8b, 0x03], regs: {3: 0x4000}, address: 0x4000},
  {name: 'store_ind', bytes: [0x89, 0x03], regs: {3: 0x4000}, address: 0x4000, source: 0},
  {name: 'load_alias', bytes: [0x8b, 0x00], regs: {0: 0x4000}, address: 0x4000},
  {name: 'store_alias', bytes: [0x89, 0x1b], regs: {3: 0x4000}, address: 0x4000, source: 3},
  {name: 'load_sib', bytes: [0x8b, 0x44, 0xb3, 0xfc], regs: {3: 0x4004, 6: 2}, address: 0x4008},
  {name: 'store_sib', bytes: [0x89, 0x54, 0x8b, 0x10], regs: {3: 0x4000, 1: 9}, address: 0x4034, source: 2},
  {name: 'load_no_base', bytes: [0x8b, 0x04, 0xb5, 0x10, 0x40, 0, 0], regs: {6: 2}, address: 0x4018},
  {name: 'load_esp', bytes: [0x8b, 0x04, 0x24], regs: {4: 0x400c}, address: 0x400c},
  {name: 'load_ebp', bytes: [0x8b, 0x45, 0], regs: {5: 0x4010}, address: 0x4010},
  {name: 'load_disp32', bytes: [0x8b, 0x83, 0, 0x10, 0, 0], regs: {3: 0x4000}, address: 0x5000},
  {name: 'load_absolute', bytes: [0xa1, 0x10, 0x40, 0, 0], regs: {}, address: 0x4010},
  {name: 'store_absolute', bytes: [0xa3, 0x14, 0x40, 0, 0], regs: {}, address: 0x4014, source: 0},
  {name: 'store_imm', bytes: [0xc7, 0x43, 4, 0xef, 0xbe, 0xad, 0xde], regs: {3: 0x4000}, address: 0x4004, value: 0xdeadbeef},
  {name: 'store_imm_sib', bytes: [0xc7, 0x44, 0x8b, 0x10, 0x78, 0x56, 0x34, 0x12], regs: {3: 0x4000, 1: 9}, address: 0x4034, value: 0x12345678},
  {name: 'load_wrap', bytes: [0x8b, 0x04, 0xb3], regs: {3: 0xfffffffc, 6: 5}, address: 0x10},
];
const sumBytes = [0xb8, 0, 0, 0, 0, 0x83, 0xf9, 0, 0x74, 0x0c, 0x8b, 0x16, 0x01, 0xd0, 0x83, 0xc6, 4, 0x83, 0xe9, 1, 0xeb, 0xef, 0x89, 0x07];
const smcBytes = [0xc7, 0x05, 0x0b, 0x10, 0, 0, 0x11, 0x22, 0x33, 0x44, 0xb9, 0xaa, 0xaa, 0xaa, 0xaa];

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

const fixtureRoot = join(root, 'engine/tests/fixtures/p2-memory');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(outputDir, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), assembled = elfFixtures(object);
for (const {name, bytes} of [...literals, {name: 'ram_sum', bytes: sumBytes}, {name: 'smc', bytes: smcBytes}]) {
  assert.deepEqual([...assembled.get(name).bytes], bytes, `LLVM independent literal fixture ${name}`);
}

for (const fixture of literals) {
  const engine = fresh();
  assert.equal(engine.api.map(0x4000, 2, 3), 0);
  assert.equal(engine.api.map(0, 1, 3), 0);
  const store = fixture.source !== undefined || fixture.value !== undefined;
  const loaded = 0xf1234567;
  word(engine, fixture.address, loaded);
  const child = compile(engine, Uint8Array.from(fixture.bytes), undefined, [store ? 'store32' : 'read32']);
  for (const flags of [2, 0x402, 0xcd7]) {
    const start = state(0x1000, flags);
    Object.entries(fixture.regs).forEach(([register, value]) => { start.registers[Number(register)] = value; });
    const expected = copyState(start);
    if (!store) expected.registers[0] = loaded;
    expected.eip += fixture.bytes.length;
    writeState(engine, start);
    run(engine, child, 1, expected, exit(1, 1), `literal ${fixture.name}/flags${flags}`, helperRecord(0, store ? 0 : loaded));
    if (store) assert.equal(guestValue(engine, fixture.address), fixture.value ?? start.registers[fixture.source], `${fixture.name}: store source uses old locals`);
    assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, `${fixture.name}: unrelated data store preserves code`);
  }
}

for (const [address, permissions, label] of [[0x4ffe, 3, 'unaligned cross-page'], [0xfffffffc, 3, 'last four guest bytes']]) {
  const engine = fresh();
  assert.equal(engine.api.map(address & 0xfffff000, address === 0x4ffe ? 2 : 1, permissions), 0);
  word(engine, address, 0xaabbccdd);
  const child = compile(engine, Uint8Array.from([0x89, 0x03, 0x8b, 0x13]), undefined, ['read32', 'store32']);
  const start = state();
  start.registers[0] = 0x80706050;
  start.registers[3] = address;
  const expected = copyState(start);
  expected.registers[2] = start.registers[0];
  expected.eip = 0x1004;
  writeState(engine, start);
  run(engine, child, 3, expected, exit(3, 2), label, helperRecord(0, start.registers[0]));
  assert.equal(guestValue(engine, address), start.registers[0]);
}

const faultCases = [
  {name: 'unmapped', address: 0x8000, detail: 1, fault: 0x8000},
  {name: 'cross-page unmapped', address: 0x4ffe, detail: 1, fault: 0x5000, map: [[0x4000, 1, 3]]},
  {name: 'read permission', address: 0x4000, detail: 2, fault: 0x4000, map: [[0x4000, 1, 2]], readOnly: true},
  {name: 'write permission', address: 0x4000, detail: 2, fault: 0x4000, map: [[0x4000, 1, 1]], storeOnly: true},
  {name: 'second-page read permission', address: 0x4ffe, detail: 2, fault: 0x5000, map: [[0x4000, 1, 3], [0x5000, 1, 2]], readOnly: true},
  {name: 'second-page write permission', address: 0x4ffe, detail: 2, fault: 0x5000, map: [[0x4000, 1, 3], [0x5000, 1, 1]], storeOnly: true},
  {name: 'address overflow', address: 0xfffffffd, detail: 3, fault: 0xfffffffd, map: [[0xfffff000, 1, 3]]},
];
for (const fault of faultCases) {
  for (const store of [false, true]) {
    if ((store && fault.readOnly) || (!store && fault.storeOnly)) continue;
    const engine = fresh();
    assert.equal(engine.api.map(0x6000, 1, 3), 0);
    word(engine, 0x6000, 0x10203040);
    for (const [address, pages] of fault.map ?? []) assert.equal(engine.api.map(address, pages, 3), 0);
    if (fault.map?.some(([address]) => address === 0x4000)) {
      word(engine, 0x4000, 0x11223344);
      word(engine, 0x4ffc, 0xa1b2c3d4);
    }
    if (fault.map?.some(([address]) => address === 0x5000)) word(engine, 0x5000, 0x55667788);
    if (fault.address >= 0xfffff000) word(engine, 0xfffffffc, 0xa1b2c3d4);
    for (const [address, pages, bits] of fault.map ?? []) assert.equal(engine.api.protect(address, pages, bits), 0);
    const bytes = Uint8Array.from([0xbe, 0x78, 0x56, 0x34, 0x12, 0x89, 0x07, store ? 0x89 : 0x8b, store ? 0x03 : 0x13, 0xb9, 0x99, 0x99, 0x99, 0x99]);
    const child = compile(engine, bytes, undefined, store ? ['store32'] : ['read32', 'store32']);
    const start = state();
    start.registers[0] = 0xfedcba98;
    start.registers[3] = fault.address;
    start.registers[7] = 0x6000;
    const expected = copyState(start);
    expected.registers[6] = 0x12345678;
    expected.eip = 0x1007;
    writeState(engine, start);
    const access = store ? 2 : 1;
    run(engine, child, 20, expected, exit(5, 2, fault.detail, fault.fault, access), `${fault.name}/${store ? 'store' : 'load'}`, helperRecord(1, 0, fault.detail, fault.fault, access));
    assert.equal(guestValue(engine, 0x6000), start.registers[0], 'prior committed data store survives later fault');
    if (store && fault.name === 'write permission') assert.equal(guestValue(engine, 0x4000), 0x11223344, 'write-permission fault preserves the exact attempted word');
    if (store && fault.name === 'second-page write permission') assert.equal(guestValue(engine, 0x5000), 0x55667788, 'cross-page permission fault preserves second-page bytes');
    if (fault.map?.some(([address, , bits]) => address === 0x4000 && (bits & 1))) assert.equal(guestValue(engine, 0x4ffc), 0xa1b2c3d4, 'faulting store preserves first-page bytes');
    if (fault.address >= 0xfffff000) assert.equal(guestValue(engine, 0xfffffffc), 0xa1b2c3d4, 'overflow store preserves final guest bytes');
    assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'fault leaves artifact current');
  }
}

for (const store of [false, true]) {
  const engine = fresh();
  const bytes = Uint8Array.from([0xb8, 0xff, 0xff, 0xff, 0xff, 0x83, 0xc0, 1, store ? 0x89 : 0x8b, store ? 0x03 : 0x13, 0xb9, 0x99, 0x99, 0x99, 0x99]);
  const child = compile(engine, bytes, undefined, [store ? 'store32' : 'read32']);
  const start = state();
  start.registers[3] = 0x9000;
  const expected = copyState(start);
  expected.registers[0] = 0;
  expected.eflags = 0x457;
  expected.eip = 0x1008;
  writeState(engine, start);
  const access = store ? 2 : 1;
  run(engine, child, 20, expected, exit(5, 2, 1, 0x9000, access), `computed ADD flags survive ${store ? 'store' : 'load'} fault`, helperRecord(1, 0, 1, 0x9000, access));
}

const oracleSource = join(fixtureRoot, 'ram_oracle.c'), oraclePath = join(outputDir, 'ram_oracle');
const oracleCompile = ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', oracleSource, '-o', oraclePath];
execFileSync('clang', oracleCompile, {stdio: ['ignore', 'pipe', 'pipe']});
const nativeInputs = [[0, 0], [1, 0xffffffff], [2, 0x80000000], [7, 123], [31, 0x89abcdef], [128, 0x10203040]];
function inputValues(count, seed) {
  return Array.from({length: count}, (_, index) => Number(BigInt.asUintN(32, (BigInt(seed) ^ BigInt.asUintN(32, 0x9e3779b9n * BigInt(index + 1))) + BigInt(index))));
}
function sumFixture(count, seed) {
  const engine = fresh();
  assert.equal(engine.api.map(0x4000, 1, 3), 0);
  assert.equal(engine.api.map(0x6000, 1, 3), 0);
  const values = inputValues(count, seed), input = new Uint8Array(count * 4), view = new DataView(input.buffer);
  values.forEach((value, index) => view.setUint32(index * 4, value, true));
  upload(engine, 0x4000, input);
  word(engine, 0x6000, 0xaaaaaaaa);
  const child = compile(engine, Uint8Array.from(sumBytes), [[0x1000, 5], [0x1005, 5], [0x100a, 12], [0x1016, 2]], ['read32', 'store32']);
  const start = state();
  start.registers[1] = count;
  start.registers[6] = 0x4000;
  start.registers[7] = 0x6000;
  return {engine, child, start, values};
}
for (const [count, seed] of nativeInputs) {
  const native = JSON.parse(execFileSync(oraclePath, [String(count), String(seed)], {encoding: 'utf8'}));
  const {engine, child, start, values} = sumFixture(count, seed);
  const sum = Number(BigInt.asUintN(32, values.reduce((total, value) => total + BigInt(value), 0n)));
  assert.deepEqual(native, {sum, last: values.at(-1) ?? 0, remaining: 0, retired: 7 * count + 4}, 'independent native C algorithm agrees with input data');
  const expected = copyState(start);
  expected.registers[0] = native.sum;
  expected.registers[1] = 0;
  if (count !== 0) expected.registers[2] = native.last;
  expected.registers[6] += 4 * count;
  expected.eip = 0x1018;
  expected.eflags = 0x446;
  writeState(engine, start);
  run(engine, child, native.retired + 10, expected, exit(3, native.retired), `actual RAM/native algorithm ${count}/${seed}`, helperRecord(0));
  assert.equal(guestValue(engine, 0x6000), native.sum, 'native result stored in actual guest RAM');
}
for (const args of [[], ['1'], ['-1', '0'], ['257', '0'], ['1x', '0'], ['1', '4294967296'], ['1', ' 0'], ['1', '0', 'extra']]) {
  const result = spawnSync(oraclePath, args, {encoding: 'utf8'});
  assert.equal(result.status, 2, `native utility rejects ${JSON.stringify(args)}`);
  assert.equal(result.stdout, '');
}

const resumed = sumFixture(31, 0x89abcdef);
writeState(resumed.engine, resumed.start);
let retiredTotal = 0;
while (true) {
  assert.equal(resumed.child.run(resumed.engine.base, resumed.engine.base + 56, 3, resumed.engine.base + 96), 0);
  actualRuns++;
  resumes++;
  refresh(resumed.engine);
  const reason = resumed.engine.view.getUint32(resumed.engine.base + 72, true), retired = resumed.engine.view.getUint32(resumed.engine.base + 76, true);
  assert.ok(reason === 1 || reason === 3);
  assert.ok(retired <= 3);
  if (reason === 1) assert.equal(retired, 3);
  assert.deepEqual(arena(resumed.engine).slice(56, 96), exit(reason, retired), 'RAM resume canonical v2 exit');
  retiredTotal += retired;
  if (reason === 3) break;
  assert.ok(resumes < 100, 'finite RAM loop terminates');
}
const resumedExpected = copyState(resumed.start);
resumedExpected.registers[0] = Number(BigInt.asUintN(32, resumed.values.reduce((sum, value) => sum + BigInt(value), 0n)));
resumedExpected.registers[1] = 0;
resumedExpected.registers[2] = resumed.values.at(-1);
resumedExpected.registers[6] += 31 * 4;
resumedExpected.eip = 0x1018;
resumedExpected.eflags = 0x446;
assert.deepEqual(readState(resumed.engine), resumedExpected, 'RAM resumes preserve full final state');
assert.equal(retiredTotal, 7 * 31 + 4, 'RAM resumes retire each instruction exactly once');
assert.equal(guestValue(resumed.engine, 0x6000), resumedExpected.registers[0]);

{
  const engine = fresh();
  assert.equal(engine.api.map(0x4000, 1, 3), 0);
  word(engine, 0x4000, 0x11111111);
  const child = compile(engine, Uint8Array.from([0x89, 0x03, 0x8b, 0x13]), undefined, ['read32', 'store32']);
  const start = state();
  start.registers[0] = 0x22222222;
  start.registers[3] = 0x4000;
  writeState(engine, start);
  const untouchedHelper = arena(engine).slice(100, 140);
  run(engine, child, 0, start, exit(1, 0), 'budget0 before memory access', untouchedHelper);
  assert.equal(guestValue(engine, 0x4000), 0x11111111);
  writeState(engine, start, 1);
  run(engine, child, 0, start, exit(2, 0), 'cancel outranks budget before memory access', arena(engine).slice(100, 140));
  assert.equal(guestValue(engine, 0x4000), 0x11111111);
  writeState(engine, start);
  const afterStore = copyState(start);
  afterStore.eip = 0x1002;
  run(engine, child, 1, afterStore, exit(1, 1), 'store checkpoint', helperRecord(0));
  word(engine, 0x4000, 0x33333333);
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 10, afterStore, exit(2, 0), 'cancel between store and load', helperRecord(0));
  engine.view.setUint32(engine.base + 96, 0, true);
  const finished = copyState(afterStore);
  finished.registers[2] = 0x33333333;
  finished.eip = 0x1004;
  run(engine, child, 2, finished, exit(3, 1), 'resume skips already committed store', helperRecord(0, 0x33333333));
  assert.equal(guestValue(engine, 0x4000), 0x33333333, 'resume did not repeat store');
}

{
  const engine = fresh();
  const child = compile(engine, Uint8Array.from(smcBytes), undefined, ['store32']);
  const start = state(), afterStore = copyState(start);
  afterStore.eip = 0x100a;
  writeState(engine, start);
  run(engine, child, 1, afterStore, exit(6, 1), 'active SMC commits store and exits before old next instruction', helperRecord(0));
  assert.equal(guestValue(engine, 0x100b), 0x44332211, 'SMC changed actual future instruction immediate');
  assert.equal(engine.api.module_ptr(), 0);
  assert.equal(engine.api.module_len(), 0);
  const before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 10, 0xffffffff), 4, 'retained stale module rejects before pointer/state reads');
  actualRuns++;
  assert.deepEqual(arena(engine), before, 'stale entry preserves complete arena');
  const replacement = compile(engine, new Uint8Array(), [[0x1000, 15]], ['store32']);
  assert.equal(replacement.generation, child.generation + 1);
  const expected = copyState(afterStore);
  expected.registers[1] = 0x44332211;
  expected.eip = 0x100f;
  writeState(engine, afterStore);
  run(engine, replacement, 2, expected, exit(3, 1), 'recompiled interior resume executes patched immediate', arena(engine).slice(100, 140));
}

for (const identical of [false, true]) {
  const engine = fresh();
  word(engine, 0x1ff0, 0x11223344);
  const child = compile(engine, Uint8Array.from([0x89, 0x03]), undefined, ['store32']);
  const start = state();
  start.registers[0] = identical ? 0x11223344 : 0x55667788;
  start.registers[3] = 0x1ff0;
  const expected = copyState(start);
  expected.eip = 0x1002;
  writeState(engine, start);
  run(engine, child, 1, expected, exit(6, 1), `conservative code-page store/identical${identical}`, helperRecord(0));
  assert.equal(guestValue(engine, 0x1ff0), start.registers[0]);
}

{
  const engine = fresh();
  word(engine, 0x1ffc, 0x11223344);
  const child = compile(engine, Uint8Array.from([0x89, 0x03]), undefined, ['store32']);
  const start = state();
  start.registers[0] = 0xffffffff;
  start.registers[3] = 0x1ffe;
  writeState(engine, start);
  run(engine, child, 5, start, exit(5, 0, 1, 0x2000, 2), 'faulting store on code page does not invalidate', helperRecord(1, 0, 1, 0x2000, 2));
  assert.equal(guestValue(engine, 0x1ffc), 0x11223344);
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0);
}

{
  const engine = fresh();
  assert.equal(engine.api.map(0x4000, 1, 3), 0);
  word(engine, 0x4000, 0x11223344);
  let before = arena(engine);
  assert.equal(engine.api.store32(0x4000, 0xffffffff), 3, 'CPU store requires installed artifact before writing');
  storeProbes++;
  assert.deepEqual(arena(engine), before);
  assert.equal(guestValue(engine, 0x4000), 0x11223344);
  const child = compile(engine, Uint8Array.from([0x8b, 0x03]), undefined, ['read32']);
  assert.equal(engine.api.store32(0x4000, 0xaabbccdd), 0, 'CPU data store completes without invalidation');
  storeProbes++;
  assert.deepEqual(arena(engine).slice(100, 140), helperRecord(0));
  assert.equal(guestValue(engine, 0x4000), 0xaabbccdd);
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0);
  assert.equal(engine.api.store32(0x1ff0, 0x44332211), 11, 'CPU code store explicitly reports committed invalidation');
  storeProbes++;
  assert.deepEqual(arena(engine).slice(100, 140), helperRecord(0));
  assert.equal(guestValue(engine, 0x1ff0), 0x44332211);
  before = arena(engine);
  assert.equal(engine.api.store32(0x4000, 0xffffffff), 4, 'stale CPU store rejects before writing');
  storeProbes++;
  assert.deepEqual(arena(engine), before);
  assert.equal(guestValue(engine, 0x4000), 0xaabbccdd);
  assert.equal(engine.api.close(), 0);
  before = arena(engine);
  assert.equal(engine.api.store32(0x4000, 0xffffffff), 5, 'closed CPU store rejects without touching retained arena');
  storeProbes++;
  assert.deepEqual(arena(engine), before);
}

function altered(record, offset, value, width = 4) {
  const bytes = record.slice(), view = new DataView(bytes.buffer);
  if (width === 1) view.setUint8(offset, value);
  else if (width === 2) view.setUint16(offset, value, true);
  else view.setUint32(offset, value, true);
  return bytes;
}
const injectionCases = [
  {name: 'version exhaustion', record: helperRecord(2, 0, 1), detail: 1},
  {name: 'other returned infrastructure', record: helperRecord(2, 0, 2), detail: 3},
  {name: 'unknown tag', record: helperRecord(3), detail: 2},
  {name: 'unknown infrastructure detail', record: helperRecord(2, 0, 3), detail: 2},
  {name: 'success detail', record: helperRecord(0, 0, 1), detail: 2},
  {name: 'success address', record: helperRecord(0, 0, 0, 0x9000), detail: 2},
  {name: 'success access', record: helperRecord(0, 0, 0, 0, 1), detail: 2},
  {name: 'success length', record: helperRecord(0, 0, 0, 0, 0, 4), detail: 2},
  {name: 'fault value', record: helperRecord(1, 1, 1, 0x9000, 1), detail: 2, loadOnly: true},
  {name: 'fault detail', record: helperRecord(1, 0, 4, 0x9000, 1), detail: 2, loadOnly: true},
  {name: 'fault wrong access', record: helperRecord(1, 0, 1, 0x9000, 2), detail: 2, loadOnly: true},
  {name: 'fault wrong width', record: helperRecord(1, 0, 1, 0x9000, 1, 2), detail: 2, loadOnly: true},
  {name: 'fault outside access', record: helperRecord(1, 0, 1, 0x9004, 1), detail: 2, loadOnly: true},
  {name: 'fault before access', record: helperRecord(1, 0, 1, 0x8fff, 1), detail: 2, loadOnly: true},
  {name: 'false overflow', record: helperRecord(1, 0, 3, 0x9000, 1), detail: 2, loadOnly: true},
  {name: 'reported nonoverflow fault wraps canonical width', record: helperRecord(1, 0, 1, 0xffffffff, 1), address: 0xfffffffc, detail: 2, loadOnly: true},
  {name: 'infra value', record: helperRecord(2, 1, 1), detail: 2},
  {name: 'infra fault fields', record: helperRecord(2, 0, 1, 0x9000, 1, 4), detail: 2},
  {name: 'closed helper status', record: helperRecord(0), status: 5, detail: 3},
  {name: 'reentrant helper status', record: helperRecord(0), status: 9, detail: 3},
  {name: 'missing artifact helper status', record: helperRecord(0), status: 3, detail: 3},
  {name: 'stale helper status', record: helperRecord(0), status: 4, detail: 3},
  {name: 'unknown helper status', record: helperRecord(0), status: 12, detail: 3},
  {name: 'read completion11 rejected', record: helperRecord(0), status: 11, detail: 3, loadOnly: true},
  {name: 'store success value', record: helperRecord(0, 1), detail: 2, storeOnly: true},
  {name: 'store11 fault contradiction', record: helperRecord(1, 0, 1, 0x9000, 2), status: 11, detail: 2, storeOnly: true},
  {name: 'store11 infrastructure contradiction', record: helperRecord(2, 0, 1), status: 11, detail: 2, storeOnly: true},
];
for (const [offset, width, value, name] of [[0, 1, 0, 'magic'], [4, 2, 2, 'version'], [6, 2, 2, 'profile'], [8, 4, 39, 'length'], [12, 4, 1, 'reserved']]) {
  injectionCases.push({name: `header ${name}`, record: altered(helperRecord(0), offset, value, width), detail: 2});
}
for (const store of [false, true]) {
  const engine = fresh();
  const bytes = Uint8Array.from([0xbe, 0x78, 0x56, 0x34, 0x12, store ? 0x89 : 0x8b, store ? 0x03 : 0x13, 0xb9, 0x99, 0x99, 0x99, 0x99]);
  const actual = compile(engine, bytes, undefined, [store ? 'store32' : 'read32']);
  const injector = new WebAssembly.Instance(injectorModule, {env: {memory: engine.memory}, ring3: {guard: engine.api.guard}}).exports;
  const injected = new WebAssembly.Instance(actual.module, {env: {memory: engine.memory}, ring3: {guard: injector.guard, read32: injector.read32, store32: injector.store32}}).exports;
  for (const fixture of injectionCases) {
    if ((store && fixture.loadOnly) || (!store && fixture.storeOnly)) continue;
    const start = state();
    start.registers[3] = fixture.address ?? 0x9000;
    const expected = copyState(start);
    expected.registers[6] = 0x12345678;
    expected.eip = 0x1005;
    writeState(engine, start);
    refresh(engine).bytes.set(fixture.record, engine.base + 140);
    injector.configure(engine.base + 100, engine.base + 140, fixture.status ?? 0);
    run(engine, injected, 10, expected, exit(7, 1, fixture.detail), `synthetic Wasm helper branch/${store ? 'store' : 'load'}/${fixture.name}`, undefined, true);
    assert.equal(injector.calls(), 1, 'synthetic helper invoked once after committed prefix');
    assert.equal(engine.api.guard(engine.low, engine.high, actual.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'injector did not mutate actual guest code');
  }
  const start = state();
  start.registers[3] = 0x9000;
  writeState(engine, start);
  injector.configure(engine.base + 100, engine.base + 140, 0);
  run(engine, injected, 0, start, exit(1, 0), 'synthetic helper skipped on budget0', arena(engine).slice(100, 140), true);
  assert.equal(injector.calls(), 0);
}

{
  const engine = fresh();
  const bytes = Uint8Array.from([0xb8, 0xff, 0xff, 0xff, 0xff, 0x83, 0xc0, 1, 0x8b, 0x13, 0xb9, 0x99, 0x99, 0x99, 0x99]);
  const actual = compile(engine, bytes, undefined, ['read32']);
  const injector = new WebAssembly.Instance(injectorModule, {env: {memory: engine.memory}, ring3: {guard: engine.api.guard}}).exports;
  const injected = new WebAssembly.Instance(actual.module, {env: {memory: engine.memory}, ring3: {guard: injector.guard, read32: injector.read32}}).exports;
  const start = state();
  start.registers[3] = 0x9000;
  const expected = copyState(start);
  expected.registers[0] = 0;
  expected.eflags = 0x457;
  expected.eip = 0x1008;
  writeState(engine, start);
  const record = helperRecord(2, 0, 1);
  refresh(engine).bytes.set(record, engine.base + 140);
  injector.configure(engine.base + 100, engine.base + 140, 0);
  run(engine, injected, 20, expected, exit(7, 2, 1), 'synthetic VersionExhausted preserves computed ADD flags', record, true);
  assert.equal(injector.calls(), 1);
}

const provenance = {
  engine_sha256: hash(engineBytes), injector_sha256: hash(injectorBytes), generated_modules: modules,
  actual_engine_runs: actualRuns, synthetic_helper_runs: injectedRuns, store_probes: storeProbes,
  resumes, native_algorithm_cases: nativeInputs.length, native_invalid_inputs: 8,
  generated_set_sha256: hash(generatedHashes.join('\n')),
  sources: {assembly_sha256: hash(readFileSync(assembly)), native_c_sha256: hash(readFileSync(oracleSource))},
  artifacts: {object_sha256: hash(object), native_binary_sha256: hash(readFileSync(oraclePath))},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], native_compile: ['clang', ...oracleCompile]},
  llvm_fixtures: [...literals, {name: 'ram_sum', bytes: sumBytes}, {name: 'smc', bytes: smcBytes}].map(({name}) => ({name, offset: assembled.get(name).offset, hex: assembled.get(name).bytes.toString('hex')})),
  claim: 'actual engine RAM MOV32, precise faults and active SMC; synthetic helper runs prove emitter error branches only; native C proves the RAM algorithm, not x86 hardware flags or counters',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, artifacts: outputDir}));
