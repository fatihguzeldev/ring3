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
let keys = 0n, modules = 0, actualRuns = 0, injectedRuns = 0;
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

const forms = {
  push_ebx: [0xff, 0x33], push_esp: [0xff, 0x34, 0x24], push_esp_minus4: [0xff, 0x74, 0x24, 0xfc],
  push_sib: [0xff, 0x74, 0x8b, 0xf8], pop_ebx: [0x8f, 0x03], pop_esp: [0x8f, 0x04, 0x24],
  pop_esp_minus4: [0x8f, 0x44, 0x24, 0xfc], pop_sib: [0x8f, 0x44, 0x8b, 0x10],
  pop_esp_sib: [0x8f, 0x44, 0x8c, 8], pop_esp_minus8: [0x8f, 0x44, 0x24, 0xf8],
};
const transferBytes = [0xff, 0x36, 0xff, 0x33, 0xff, 0x73, 4, 0x8f, 0x07, 0x8f, 0x47, 4, 0x8f, 0x06, 0x8b, 0x07, 0x01, 0xe8];
const smcPush = [...forms.push_ebx, 0xb9, 0xbb, 0xbb, 0xbb, 0xbb], smcPop = [...forms.pop_ebx, 0xb9, 0xbb, 0xbb, 0xbb, 0xbb];
const llvmFixtures = [...Object.entries(forms).map(([name, bytes]) => ({name, bytes})), {name: 'transfer', bytes: transferBytes}, {name: 'smc_push', bytes: smcPush}, {name: 'smc_pop', bytes: smcPop}];
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-memory-stack');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(outputDir, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), assembled = elfFixtures(object);
for (const {name, bytes} of llvmFixtures) assert.deepEqual([...assembled.get(name).bytes], bytes, `independent LLVM memory-stack fixture ${name}`);

function guarded(engine, child, label) {
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, label);
}

function guestBytes(engine, address, length) {
  assert.equal(length % 4, 0);
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  for (let offset = 0; offset < length; offset += 4) view.setUint32(offset, guestValue(engine, address + offset), true);
  return bytes;
}

function literalMemory(engine) {
  for (const [address, pages] of [[0x4000, 2], [0x8000, 2], [0, 1], [0xfffff000, 1]]) assert.equal(engine.api.map(address, pages, 3), 0);
}

const value = 0xa1b2c3d4, untouched = 0x11223344;
const literalCases = [
  {name: 'PUSH other base', form: 'push_ebx', esp: 0x9000, regs: {3: 0x4000}, source: 0x4000, destination: 0x8ffc},
  {name: 'PUSH old ESP', form: 'push_esp', esp: 0x8ffc, regs: {}, source: 0x8ffc, destination: 0x8ff8},
  {name: 'PUSH exact source/destination overlap', form: 'push_esp_minus4', esp: 0x8ffc, regs: {}, source: 0x8ff8, destination: 0x8ff8},
  {name: 'PUSH SIB/displacement', form: 'push_sib', esp: 0x9000, regs: {3: 0x4000, 1: 4}, source: 0x4008, destination: 0x8ffc},
  {name: 'PUSH final source width', form: 'push_ebx', esp: 0x9000, regs: {3: 0xfffffffc}, source: 0xfffffffc, destination: 0x8ffc},
  {name: 'PUSH ESP arithmetic wrap uses old zero source', form: 'push_esp', esp: 0, regs: {}, source: 0, destination: 0xfffffffc},
  {name: 'POP other base', form: 'pop_ebx', esp: 0x8ffc, regs: {3: 0x4000}, source: 0x8ffc, destination: 0x4000},
  {name: 'POP pending ESP', form: 'pop_esp', esp: 0x8ff8, regs: {}, source: 0x8ff8, destination: 0x8ffc},
  {name: 'POP exact source/destination overlap', form: 'pop_esp_minus4', esp: 0x8ffc, regs: {}, source: 0x8ffc, destination: 0x8ffc},
  {name: 'POP non-ESP SIB/displacement', form: 'pop_sib', esp: 0x8ffc, regs: {3: 0x4000, 1: 3}, source: 0x8ffc, destination: 0x401c},
  {name: 'POP pending ESP with SIB index', form: 'pop_esp_sib', esp: 0x8fe0, regs: {1: 3}, source: 0x8fe0, destination: 0x8ff8},
  {name: 'POP pending ESP wraps to zero destination', form: 'pop_esp', esp: 0xfffffffc, regs: {}, source: 0xfffffffc, destination: 0},
  {name: 'POP pending ESP/displacement wraps to original final word', form: 'pop_esp_minus4', esp: 0xfffffffc, regs: {}, source: 0xfffffffc, destination: 0xfffffffc},
  {name: 'POP pending displacement reaches zero', form: 'pop_esp_minus8', esp: 4, regs: {}, source: 4, destination: 0},
];
for (const fixture of literalCases) {
  const engine = fresh();
  literalMemory(engine);
  word(engine, fixture.destination, untouched);
  word(engine, fixture.source, value);
  const bytes = forms[fixture.form], child = compile(engine, Uint8Array.from(bytes), undefined, ['read32', 'store32']);
  const start = state();
  start.registers[4] = fixture.esp;
  Object.entries(fixture.regs).forEach(([register, data]) => { start.registers[Number(register)] = data; });
  const expected = copyState(start), push = fixture.form.startsWith('push');
  expected.registers[4] = (fixture.esp + (push ? -4 : 4)) >>> 0;
  expected.eip += bytes.length;
  writeState(engine, start);
  run(engine, child, 1, expected, exit(1, 1), fixture.name, helperRecord(0));
  assert.equal(guestValue(engine, fixture.destination), value, 'destination uses captured first-read value');
  assert.equal(guestValue(engine, fixture.source), value, 'nonpartial source overlap preserves original word');
  guarded(engine, child, 'noncompiled page writes and source reads preserve code');
}

for (const push of [true, false]) {
  const engine = fresh();
  literalMemory(engine);
  const address = push ? 0x8ff8 : 0x8ffc;
  upload(engine, address, Uint8Array.from([1, 2, 3, 4, 5, 6, 7, 8]));
  const child = compile(engine, Uint8Array.from(push ? forms.push_ebx : forms.pop_ebx), undefined, ['read32', 'store32']);
  const start = state();
  start.registers[4] = push ? 0x9000 : 0x8ffc;
  start.registers[3] = push ? 0x8ffa : 0x8ffe;
  const expected = copyState(start);
  expected.registers[4] = push ? 0x8ffc : 0x9000;
  expected.eip = 0x1002;
  writeState(engine, start);
  run(engine, child, 1, expected, exit(1, 1), `${push ? 'PUSH' : 'POP'} partial unaligned overlap captures before writing`, helperRecord(0));
  assert.deepEqual(guestBytes(engine, address, 8), Uint8Array.from(push ? [1, 2, 3, 4, 3, 4, 5, 6] : [1, 2, 1, 2, 3, 4, 7, 8]));
}

const oracleSource = join(fixtureRoot, 'memory_stack_oracle.c'), oraclePath = join(outputDir, 'memory_stack_oracle');
const oracleCompile = ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', oracleSource, '-o', oraclePath];
execFileSync('clang', oracleCompile, {stdio: ['ignore', 'pipe', 'pipe']});
const nativeInputs = [
  [0x1234567, 0, 0xdeadbeef, 0, 0, 0x446],
  [0x89abcdef, 0xffffffff, 0x76543210, 1, 0, 0x457],
  [111, 0x7fffffff, 0xa1b2c3d4, 1, 0x80000000, 0xc96],
  [222, 0x80000000, 0x12345678, 0x80000000, 0, 0xc47],
  [333, 0x12345678, 0xfedcba98, 0x87654321, 0x99999999, 0x486],
];
for (const [first, second, saved, seed, sum, flags] of nativeInputs) {
  const native = JSON.parse(execFileSync(oraclePath, [first, second, saved, seed].map(String), {encoding: 'utf8'}));
  assert.deepEqual(native, {first: second, second: first, saved, sum}, 'native unsigned transfer/save/restore algorithm');
  assert.equal(sum, Number(BigInt.asUintN(32, BigInt(second) + BigInt(seed))));
  const engine = fresh();
  for (const address of [0x4000, 0x5000, 0x6000, 0x8000]) assert.equal(engine.api.map(address, 1, 3), 0);
  for (const [address, data] of [[0x4000, first], [0x4004, second], [0x5000, untouched], [0x5004, untouched], [0x6000, saved], [0x6004, untouched]]) word(engine, address, data);
  const child = compile(engine, Uint8Array.from(transferBytes), undefined, ['read32', 'store32']);
  const start = state();
  start.registers[3] = 0x4000;
  start.registers[4] = 0x9000;
  start.registers[5] = seed;
  start.registers[6] = 0x6000;
  start.registers[7] = 0x5000;
  const expected = copyState(start);
  expected.registers[0] = sum;
  expected.eip = 0x1012;
  expected.eflags = flags;
  writeState(engine, start);
  run(engine, child, 20, expected, exit(3, 8), `actual memory transfer/save/restore ${first}/${second}/${seed}`, helperRecord(0, second));
  for (const [address, data] of [[0x4000, first], [0x4004, second], [0x5000, second], [0x5004, first], [0x6000, saved], [0x6004, untouched], [0x8ffc, saved], [0x8ff8, first], [0x8ff4, second]]) assert.equal(guestValue(engine, address), data, 'native transfer matches actual RAM and preserves source/scratch neighbors');
}
for (const args of [[], ['1', '2', '3'], ['-1', '0', '0', '0'], ['4294967296', '0', '0', '0'], ['1x', '0', '0', '0'], ['1', ' 0', '0', '0'], ['1', '2', '3', '4', 'extra']]) {
  const invalid = spawnSync(oraclePath, args, {encoding: 'utf8'});
  assert.equal(invalid.status, 2, `native utility rejects ${JSON.stringify(args)}`);
  assert.equal(invalid.stdout, '');
}

const arithmeticPrefix = [0xb8, 0xff, 0xff, 0xff, 0xff, 0x83, 0xc0, 1];
const faultCases = [
  {name: 'PUSH first source unmapped suppresses writable stack', push: true, esp: 0x4004, ea: 0x9000, access: 1, detail: 1, fault: 0x9000, map: [[0x4000, 1, 3]], words: [[0x4000, untouched]]},
  {name: 'PUSH first read wins when both accesses unmapped', push: true, esp: 0xa004, ea: 0x9000, access: 1, detail: 1, fault: 0x9000},
  {name: 'PUSH source read permission', push: true, esp: 0x8004, ea: 0x4000, access: 1, detail: 2, fault: 0x4000, map: [[0x4000, 1, 2], [0x8000, 1, 3]], words: [[0x4000, value], [0x8000, untouched]], observe: [[0x8000, untouched]]},
  {name: 'PUSH source cross-page permission', push: true, esp: 0x8004, ea: 0x4ffe, access: 1, detail: 2, fault: 0x5000, map: [[0x4000, 1, 3], [0x5000, 1, 2], [0x8000, 1, 3]], words: [[0x4ffc, value], [0x5000, value], [0x8000, untouched]], observe: [[0x4ffc, value], [0x8000, untouched]]},
  {name: 'PUSH source width overflow', push: true, esp: 0x8004, ea: 0xfffffffd, access: 1, detail: 3, fault: 0xfffffffd, map: [[0x8000, 1, 3]], words: [[0x8000, untouched]]},
  {name: 'PUSH second stack store unmapped', push: true, esp: 0x9004, ea: 0x4000, access: 2, detail: 1, fault: 0x9000, map: [[0x4000, 1, 3]], words: [[0x4000, value]]},
  {name: 'PUSH second stack write permission', push: true, esp: 0x8004, ea: 0x4000, access: 2, detail: 2, fault: 0x8000, map: [[0x4000, 1, 3], [0x8000, 1, 1]], words: [[0x4000, value], [0x8000, untouched]]},
  {name: 'PUSH second stack cross-page store', push: true, esp: 0x9002, ea: 0x4000, access: 2, detail: 2, fault: 0x9000, map: [[0x4000, 1, 3], [0x8000, 1, 3], [0x9000, 1, 1]], words: [[0x4000, value], [0x8ffc, value], [0x9000, untouched]]},
  {name: 'PUSH second stack width overflow', push: true, esp: 1, ea: 0x4000, access: 2, detail: 3, fault: 0xfffffffd, map: [[0x4000, 1, 3]], words: [[0x4000, value]]},
  {name: 'POP first stack read unmapped suppresses writable destination', push: false, esp: 0x9000, ea: 0x4000, access: 1, detail: 1, fault: 0x9000, map: [[0x4000, 1, 3]], words: [[0x4000, untouched]]},
  {name: 'POP first stack read permission', push: false, esp: 0x8000, ea: 0x4000, access: 1, detail: 2, fault: 0x8000, map: [[0x4000, 1, 3], [0x8000, 1, 2]], words: [[0x4000, untouched], [0x8000, value]], observe: [[0x4000, untouched]]},
  {name: 'POP second destination unmapped', push: false, esp: 0x8000, ea: 0x9000, access: 2, detail: 1, fault: 0x9000, map: [[0x8000, 1, 3]], words: [[0x8000, value]]},
  {name: 'POP pending ESP second destination unmapped', push: false, form: 'pop_esp', esp: 0x8ffc, ea: 0x4000, access: 2, detail: 1, fault: 0x9000, map: [[0x8000, 1, 3]], words: [[0x8ffc, value]]},
  {name: 'POP second destination write permission', push: false, esp: 0x8000, ea: 0x4000, access: 2, detail: 2, fault: 0x4000, map: [[0x4000, 1, 1], [0x8000, 1, 3]], words: [[0x4000, untouched], [0x8000, value]]},
  {name: 'POP second destination cross-page store', push: false, esp: 0x8000, ea: 0x4ffe, access: 2, detail: 2, fault: 0x5000, map: [[0x4000, 1, 3], [0x5000, 1, 1], [0x8000, 1, 3]], words: [[0x4ffc, value], [0x5000, untouched], [0x8000, value]]},
  {name: 'POP second destination width overflow', push: false, esp: 0x8000, ea: 0xfffffffd, access: 2, detail: 3, fault: 0xfffffffd, map: [[0x8000, 1, 3]], words: [[0x8000, value]]},
];
for (const fixture of faultCases) {
  const engine = fresh();
  for (const [address, pages] of fixture.map ?? []) assert.equal(engine.api.map(address, pages, 3), 0);
  for (const [address, data] of fixture.words ?? []) word(engine, address, data);
  for (const [address, pages, bits] of fixture.map ?? []) assert.equal(engine.api.protect(address, pages, bits), 0);
  const bytes = [...arithmeticPrefix, ...forms[fixture.form ?? (fixture.push ? 'push_ebx' : 'pop_ebx')], 0xb9, 0x99, 0x99, 0x99, 0x99];
  const child = compile(engine, Uint8Array.from(bytes), undefined, ['read32', 'store32']);
  const start = state();
  start.registers[3] = fixture.ea;
  start.registers[4] = fixture.esp;
  const expected = copyState(start);
  expected.registers[0] = 0;
  expected.eflags = 0x457;
  expected.eip = 0x1008;
  writeState(engine, start);
  run(engine, child, 20, expected, exit(5, 2, fixture.detail, fixture.fault, fixture.access), fixture.name, helperRecord(1, 0, fixture.detail, fixture.fault, fixture.access));
  for (const [address, data] of fixture.observe ?? fixture.words ?? []) assert.equal(guestValue(engine, address), data, 'failed memory stack instruction preserves readable source/destination bytes');
  guarded(engine, child, 'failed read or second store preserves code stamps');
}

{
  const engine = fresh();
  for (const address of [0x4000, 0x5000, 0x8000]) assert.equal(engine.api.map(address, 1, 3), 0);
  word(engine, 0x4000, value);
  word(engine, 0x5000, untouched);
  word(engine, 0x8000, untouched);
  const child = compile(engine, Uint8Array.from([...forms.push_ebx, 0x8f, 0x07]), undefined, ['read32', 'store32']);
  const start = state();
  start.registers[3] = 0x4000;
  start.registers[4] = 0x8004;
  start.registers[7] = 0x5000;
  writeState(engine, start);
  run(engine, child, 0, start, exit(1, 0), 'budget0 skips both memory-stack accesses', arena(engine).slice(100, 140));
  assert.equal(guestValue(engine, 0x8000), untouched);
  writeState(engine, start, 1);
  run(engine, child, 0, start, exit(2, 0), 'cancel outranks budget before two accesses', arena(engine).slice(100, 140));
  writeState(engine, start);
  const pushed = copyState(start);
  pushed.registers[4] = 0x8000;
  pushed.eip = 0x1002;
  run(engine, child, 1, pushed, exit(1, 1), 'budget1 commits both PUSH accesses atomically', helperRecord(0));
  assert.equal(guestValue(engine, 0x8000), value);
  word(engine, 0x4000, 0x55667788);
  word(engine, 0x8000, 0x99887766);
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 20, pushed, exit(2, 0), 'cancel between completed memory PUSH and POP', helperRecord(0));
  assert.equal(guestValue(engine, 0x5000), untouched);
  engine.view.setUint32(engine.base + 96, 0, true);
  const popped = copyState(start);
  popped.eip = 0x1004;
  run(engine, child, 1, popped, exit(1, 1), 'budget1 POP uses saved stack source without repeating PUSH', helperRecord(0));
  assert.equal(guestValue(engine, 0x5000), 0x99887766);
  word(engine, 0x8000, 0x12345678);
  word(engine, 0x5000, 0x87654321);
  run(engine, child, 2, popped, exit(3, 0), 'next resume repeats neither POP store nor ESP increment', helperRecord(0));
  assert.equal(guestValue(engine, 0x5000), 0x87654321);
  assert.equal(guestValue(engine, 0x4000), 0x55667788);
}

for (const push of [true, false]) {
  const engine = fresh();
  assert.equal(engine.api.map(push ? 0x4000 : 0x8000, 1, 3), 0);
  word(engine, push ? 0x4000 : 0x8000, value);
  const child = compile(engine, Uint8Array.from(push ? smcPush : smcPop), undefined, ['read32', 'store32']);
  const start = state();
  start.registers[3] = push ? 0x4000 : 0x1003;
  start.registers[4] = push ? 0x1007 : 0x8000;
  const committed = copyState(start);
  committed.registers[4] = push ? 0x1003 : 0x8004;
  committed.eip = 0x1002;
  writeState(engine, start);
  run(engine, child, 1, committed, exit(6, 1), `${push ? 'PUSH' : 'POP'} memory SMC commits full instruction before successor/budget`, helperRecord(0));
  assert.equal(guestValue(engine, 0x1003), value, 'committed store patches exact successor immediate');
  const before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 10, 0xffffffff), 4, 'stale two-access module rejects before generated read');
  actualRuns++;
  assert.deepEqual(arena(engine), before);
  const replacement = compile(engine, new Uint8Array(), [[0x1000, 7]], ['read32', 'store32']);
  const expected = copyState(committed);
  expected.registers[1] = value;
  expected.eip = 0x1007;
  writeState(engine, committed);
  run(engine, replacement, 2, expected, exit(3, 1), 'recompiled successor skips committed memory stack accesses', arena(engine).slice(100, 140));
  assert.equal(guestValue(engine, push ? 0x4000 : 0x8000), value, 'SMC source remains unchanged');
}

for (const push of [true, false]) {
  const engine = fresh();
  word(engine, 0x1ff0, value);
  const bytes = push ? forms.push_esp_minus4 : forms.pop_esp_minus4;
  const child = compile(engine, Uint8Array.from(bytes), undefined, ['read32', 'store32']);
  const start = state();
  start.registers[4] = push ? 0x1ff4 : 0x1ff0;
  const expected = copyState(start);
  expected.registers[4] = push ? 0x1ff0 : 0x1ff4;
  expected.eip = 0x1004;
  writeState(engine, start);
  run(engine, child, 1, expected, exit(6, 1), `${push ? 'PUSH' : 'POP'} identical overlapping code-page write conservatively invalidates`, helperRecord(0));
  assert.equal(guestValue(engine, 0x1ff0), value);
  assert.equal(engine.api.module_ptr(), 0);
}

const injectedCases = [
  {name: 'first read VersionExhausted suppresses store', readRecord: helperRecord(2, 0, 1), reason: 7, detail: 1, stores: 0},
  {name: 'first read protocol suppresses store', readRecord: helperRecord(3), reason: 7, detail: 2, stores: 0},
  {name: 'first read closed status suppresses store', readRecord: helperRecord(0), readStatus: 5, reason: 7, detail: 3, stores: 0},
  {name: 'first read completion11 suppresses store', readRecord: helperRecord(0), readStatus: 11, reason: 7, detail: 3, stores: 0},
  {name: 'first read canonical fault suppresses store', firstFault: true, reason: 5, detail: 1, stores: 0},
  {name: 'second store VersionExhausted preserves old ESP', storeRecord: helperRecord(2, 0, 1), reason: 7, detail: 1, stores: 1},
  {name: 'second store protocol preserves old ESP', storeRecord: helperRecord(3), reason: 7, detail: 2, stores: 1},
  {name: 'second store closed status preserves old ESP', storeRecord: helperRecord(0), storeStatus: 5, reason: 7, detail: 3, stores: 1},
  {name: 'second store canonical fault preserves old ESP', secondFault: true, reason: 5, detail: 1, stores: 1},
  {name: 'second store completion11 contradiction preserves old ESP', storeRecord: helperRecord(2, 0, 1), storeStatus: 11, reason: 7, detail: 2, stores: 1},
  {name: 'second store invalidation commits both-access instruction', storeRecord: helperRecord(0), storeStatus: 11, reason: 6, detail: 0, stores: 1, commit: true},
];
for (const push of [true, false]) {
  const engine = fresh();
  const actual = compile(engine, Uint8Array.from([...arithmeticPrefix, ...(push ? forms.push_ebx : forms.pop_ebx), 0xb9, 0x99, 0x99, 0x99, 0x99]), undefined, ['read32', 'store32']);
  const injector = new WebAssembly.Instance(injectorModule, {env: {memory: engine.memory}, ring3: {guard: engine.api.guard}}).exports;
  const injected = new WebAssembly.Instance(actual.module, {env: {memory: engine.memory}, ring3: {guard: injector.guard, read32: injector.read32, store32: injector.store32}}).exports;
  const readAddress = push ? 0x4000 : 0x9000, storeAddress = push ? 0x9000 : 0x4000;
  for (const fixture of injectedCases) {
    const start = state();
    start.registers[3] = 0x4000;
    start.registers[4] = push ? 0x9004 : 0x9000;
    const expected = copyState(start);
    expected.registers[0] = 0;
    expected.eflags = 0x457;
    expected.eip = fixture.commit ? 0x100a : 0x1008;
    if (fixture.commit) expected.registers[4] = push ? 0x9000 : 0x9004;
    const readRecord = fixture.firstFault ? helperRecord(1, 0, 1, readAddress, 1) : fixture.readRecord ?? helperRecord(0, value);
    const storeRecord = fixture.secondFault ? helperRecord(1, 0, 1, storeAddress, 2) : fixture.storeRecord ?? helperRecord(0);
    writeState(engine, start);
    refresh(engine).bytes.set(readRecord, engine.base + 140);
    engine.bytes.set(storeRecord, engine.base + 180);
    injector.configure(engine.base + 100, engine.base + 140, engine.base + 180, fixture.readStatus ?? 0, fixture.storeStatus ?? 0);
    const faultAddress = fixture.firstFault ? readAddress : fixture.secondFault ? storeAddress : 0;
    const access = fixture.firstFault ? 1 : fixture.secondFault ? 2 : 0;
    run(engine, injected, 20, expected, exit(fixture.reason, fixture.commit ? 3 : 2, fixture.detail, faultAddress, access), `synthetic ${push ? 'PUSH' : 'POP'}/${fixture.name}`, undefined, true);
    assert.equal(injector.read_calls(), 1, 'one first read');
    assert.equal(injector.store_calls(), fixture.stores, 'first failure suppresses second store');
    assert.equal(injector.order(), fixture.stores ? 12 : 1, 'read occurs strictly before store');
    assert.equal(injector.read_address() >>> 0, readAddress, 'first helper receives exact source EA');
    if (fixture.stores) {
      assert.equal(injector.store_address() >>> 0, storeAddress, 'second helper receives exact destination EA');
      assert.equal(injector.store_value() >>> 0, value, 'second helper receives captured value despite changed helper record');
    }
    guarded(engine, actual, 'synthetic branch does not change actual guest code');
  }
}

const provenance = {
  engine_sha256: hash(engineBytes), injector_sha256: hash(injectorBytes), generated_modules: modules,
  actual_engine_runs: actualRuns, synthetic_helper_runs: injectedRuns,
  native_algorithm_cases: nativeInputs.length, native_invalid_inputs: 7,
  generated_set_sha256: hash(generatedHashes.join('\n')),
  sources: {assembly_sha256: hash(readFileSync(assembly)), native_c_sha256: hash(readFileSync(oracleSource))},
  artifacts: {object_sha256: hash(object), native_binary_sha256: hash(readFileSync(oraclePath))},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], native_compile: ['clang', ...oracleCompile]},
  llvm_fixtures: llvmFixtures.map(({name}) => ({name, offset: assembled.get(name).offset, hex: assembled.get(name).bytes.toString('hex')})),
  claim: 'actual flat32 memory PUSH/POP with ordered read/store, deferred ESP, overlap, precise faults and SMC; synthetic helpers prove sequence/failure/parameter branches only; native C proves unsigned transfer/save/restore, not x86 hardware flags/counters',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, artifacts: outputDir}));
