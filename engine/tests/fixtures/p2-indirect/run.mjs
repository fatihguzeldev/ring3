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

function exit(reason, retired, detail = 0, address = 0, access = 0, length = reason === 5 ? 4 : 0, version = 2) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3EX', 40, version);
  [reason, retired, detail, address, access, length].forEach((field, index) => view.setUint32(16 + index * 4, field, true));
  return bytes;
}

function run(engine, child, budget, expected, expectedExit, label, expectedHelper = undefined, injected = false) {
  const before = arena(engine);
  assert.equal(child.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, `${label}: canonical runtime exit status`);
  if (injected) injectedRuns++; else actualRuns++;
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

const forms = {
  call_eax: [0xff, 0xd0], call_esp: [0xff, 0xd4], call_esi: [0xff, 0xd6], call_ebx: [0xff, 0x13],
  call_esp_memory: [0xff, 0x14, 0x24], call_esp_minus4: [0xff, 0x54, 0x24, 0xfc], call_sib: [0xff, 0x54, 0x8b, 0xfc],
  jmp_eax: [0xff, 0xe0], jmp_esp: [0xff, 0xe4], jmp_ebx: [0xff, 0x23],
  jmp_sib: [0xff, 0x64, 0x8b, 0xfc], jmp_wrap: [0xff, 0x24, 0xb3],
};
const pointerBytes = [0xff, 0x13, 0x90, 0x01, 0xc8, 0xff, 0xe2, 0x29, 0xc8, 0xff, 0xe2, 0xc3];
const pointerBlocks = [[0x1000, 2], [0x1003, 4], [0x1007, 4], [0x100b, 1]];
const llvmFixtures = [...Object.entries(forms).map(([name, bytes]) => ({name, bytes})), {name: 'function_pointer', bytes: pointerBytes}];
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-indirect');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(outputDir, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), assembled = elfFixtures(object);
for (const {name, bytes} of llvmFixtures) assert.deepEqual([...assembled.get(name).bytes], bytes, `independent LLVM indirect fixture ${name}`);

function guarded(engine, child, label) {
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, label);
}

const target = 0xf1234567;
const callCases = [
  {name: 'CALL register absolute target', form: 'call_eax', esp: 0x9000, regs: {0: target}, target, stack: 0x8ffc},
  {name: 'CALL ESP captures old ESP', form: 'call_esp', esp: 0x9000, regs: {}, target: 0x9000, stack: 0x8ffc},
  {name: 'CALL memory absolute target survives helper overwrite', form: 'call_ebx', esp: 0x9000, regs: {3: 0x4000}, target, source: 0x4000, stack: 0x8ffc},
  {name: 'CALL [ESP] uses old ESP target word', form: 'call_esp_memory', esp: 0x8ffc, regs: {}, target, source: 0x8ffc, stack: 0x8ff8},
  {name: 'CALL [ESP-4] captures overlapping target before return push', form: 'call_esp_minus4', esp: 0x9000, regs: {}, target, source: 0x8ffc, stack: 0x8ffc},
  {name: 'CALL SIB old-register target', form: 'call_sib', esp: 0x9000, regs: {3: 0x4004, 1: 2}, target, source: 0x4008, stack: 0x8ffc},
  {name: 'CALL [ESP] wraps stack after valid old zero target read', form: 'call_esp_memory', esp: 0, regs: {}, target, source: 0, stack: 0xfffffffc},
];
for (const fixture of callCases) {
  const engine = fresh();
  for (const [address, pages] of [[0x4000, 1], [0x8000, 1], [0, 1], [0xfffff000, 1]]) assert.equal(engine.api.map(address, pages, 3), 0);
  if (fixture.source !== undefined) word(engine, fixture.source, fixture.target);
  const bytes = forms[fixture.form], memory = fixture.source !== undefined;
  const child = compile(engine, Uint8Array.from(bytes), undefined, memory ? ['read32', 'store32'] : ['store32']);
  const start = state();
  start.registers[4] = fixture.esp;
  Object.entries(fixture.regs).forEach(([register, value]) => { start.registers[Number(register)] = value; });
  const expected = copyState(start);
  expected.registers[4] = fixture.stack;
  expected.eip = fixture.target;
  writeState(engine, start);
  run(engine, child, 2, expected, exit(3, 1), fixture.name, helperRecord(0));
  assert.equal(guestValue(engine, fixture.stack), 0x1000 + bytes.length, 'CALL pushes next PC, never indirect target');
  if (memory && fixture.source !== fixture.stack) assert.equal(guestValue(engine, fixture.source), fixture.target, 'nonoverlapping function pointer remains unchanged');
  guarded(engine, child, 'data-target reads and return pushes preserve code');
  if (fixture.form === 'call_ebx') {
    writeState(engine, start);
    run(engine, child, 1, expected, exit(1, 1), 'unknown memory CALL target commits before budget1 exit', helperRecord(0));
    assert.equal(guestValue(engine, fixture.stack), 0x1000 + bytes.length);
    word(engine, fixture.source, 0x1100);
    word(engine, fixture.stack, 0x12345678);
    word(engine, fixture.stack - 4, 0x55667788);
    run(engine, child, 2, expected, exit(3, 0), 'unknown CALL resume needs code without target reread or repeated push', helperRecord(0));
    assert.equal(guestValue(engine, fixture.source), 0x1100, 'changed target pointer is not reread');
    assert.equal(guestValue(engine, fixture.stack), 0x12345678, 'completed return push is not repeated');
    assert.equal(guestValue(engine, fixture.stack - 4), 0x55667788, 'resume does not push at committed ESP');
  }
}

for (const memory of [false, true]) {
  const engine = fresh();
  if (memory) assert.equal(engine.api.map(0x4000, 1, 3), 0);
  assert.equal(engine.api.map(0x8000, 1, 3), 0);
  word(engine, 0x8ffc, 0x11223344);
  upload(engine, 0x1100, Uint8Array.from([0xb9, 0x78, 0x56, 0x34, 0x12, 0x90]));
  const child = compile(engine, Uint8Array.from(memory ? forms.jmp_ebx : forms.jmp_eax), [[0x1000, 2], [0x1100, 6]], memory ? ['read32'] : []);
  const version = memory ? 2 : 1;
  for (const [destination, retired, next, ecx] of [[0x1100, 3, 0x1106, 0x12345678], [0x1105, 2, 0x1106, null], [target, 1, target, null]]) {
    const start = state();
    start.registers[4] = 0x9000;
    if (memory) {
      start.registers[3] = 0x4000;
      word(engine, 0x4000, destination);
    } else start.registers[0] = destination;
    const expected = copyState(start);
    expected.eip = next;
    if (ecx !== null) expected.registers[1] = ecx;
    writeState(engine, start);
    const beforeHelper = arena(engine).slice(100, 140);
    run(engine, child, retired + 1, expected, exit(3, retired, 0, 0, 0, 0, version), `${memory ? 'memory/v2' : 'register/v1'} JMP target${destination}`, memory ? helperRecord(0, destination) : beforeHelper);
    assert.equal(guestValue(engine, 0x8ffc), 0x11223344, 'JMP does not push a return word');
    guarded(engine, child, 'mutable data target does not invalidate code');
  }
  const start = state();
  start.registers[4] = 0x9000;
  if (memory) {
    start.registers[3] = 0x4000;
    word(engine, 0x4000, 0x1100);
  } else start.registers[0] = 0x1100;
  writeState(engine, start);
  const jumped = copyState(start);
  jumped.eip = 0x1100;
  run(engine, child, 1, jumped, exit(1, 1, 0, 0, 0, 0, version), 'budget1 commits indirect JMP before callee', memory ? helperRecord(0, 0x1100) : arena(engine).slice(100, 140));
  if (memory) word(engine, 0x4000, target);
  const expected = copyState(jumped);
  expected.eip = 0x1106;
  expected.registers[1] = 0x12345678;
  run(engine, child, 3, expected, exit(3, 2, 0, 0, 0, 0, version), 'JMP resume skips changed target and executes interior continuation', arena(engine).slice(100, 140));
}

for (const fixture of [
  {name: 'JMP ESP helper-free absolute target', form: 'jmp_esp', regs: {4: 0x9000}, version: 1, target: 0x9000},
  {name: 'JMP SIB target read', form: 'jmp_sib', regs: {3: 0x4004, 1: 2}, source: 0x4008, version: 2, target},
  {name: 'JMP wrapping EA target read', form: 'jmp_wrap', regs: {3: 0xfffffffc, 6: 5}, source: 0x10, version: 2, target},
  {name: 'JMP final source word', form: 'jmp_ebx', regs: {3: 0xfffffffc}, source: 0xfffffffc, version: 2, target},
]) {
  const engine = fresh();
  for (const address of [0x4000, 0, 0xfffff000]) assert.equal(engine.api.map(address, 1, 3), 0);
  if (fixture.source !== undefined) word(engine, fixture.source, fixture.target);
  const child = compile(engine, Uint8Array.from(forms[fixture.form]), undefined, fixture.version === 1 ? [] : ['read32']);
  const start = state();
  Object.entries(fixture.regs).forEach(([register, value]) => { start.registers[Number(register)] = value; });
  const expected = copyState(start);
  expected.eip = fixture.target;
  writeState(engine, start);
  run(engine, child, 2, expected, exit(3, 1, 0, 0, 0, 0, fixture.version), fixture.name, fixture.version === 1 ? arena(engine).slice(100, 140) : helperRecord(0, fixture.target));
}

const oracleSource = join(fixtureRoot, 'indirect_oracle.c'), oraclePath = join(outputDir, 'indirect_oracle');
const oracleCompile = ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', oracleSource, '-o', oraclePath];
execFileSync('clang', oracleCompile, {stdio: ['ignore', 'pipe', 'pipe']});
const nativeInputs = [[0, 0, 0, 0, 0x446], [0, 0xffffffff, 1, 0, 0x457], [0, 0x7fffffff, 1, 0x80000000, 0xc96], [1, 0, 1, 0xffffffff, 0x497], [1, 0x80000000, 1, 0x7fffffff, 0xc16], [1, 0x7fffffff, 0xffffffff, 0x80000000, 0xc87]];
{
  const engine = fresh();
  for (const address of [0x4000, 0x8000]) assert.equal(engine.api.map(address, 1, 3), 0);
  const child = compile(engine, Uint8Array.from(pointerBytes), pointerBlocks, ['read32', 'store32']);
  const originalPointer = engine.api.module_ptr(), originalLength = engine.api.module_len();
  for (const [kind, left, right, result, flags] of nativeInputs) {
    const native = JSON.parse(execFileSync(oraclePath, [kind, left, right].map(String), {encoding: 'utf8'}));
    assert.deepEqual(native, {result}, 'native unsigned mutable-function-pointer algorithm');
    assert.equal(result, Number(BigInt.asUintN(32, kind === 0 ? BigInt(left) + BigInt(right) : BigInt(left) - BigInt(right))));
    word(engine, 0x4000, kind === 0 ? 0x1003 : 0x1007);
    guarded(engine, child, 'changing RAM callee leaves artifact current');
    assert.equal(engine.api.module_ptr(), originalPointer, 'mutable pointer does not recompile');
    assert.equal(engine.api.module_len(), originalLength);
    assert.equal(engine.api.generation(), child.generation);
    const start = state();
    start.registers[0] = left;
    start.registers[1] = right;
    start.registers[2] = 0x100b;
    start.registers[3] = 0x4000;
    start.registers[4] = 0x9000;
    const expected = copyState(start);
    expected.registers[0] = result;
    expected.eip = 0x1002;
    expected.eflags = flags;
    writeState(engine, start);
    run(engine, child, 10, expected, exit(3, 4), `RAM CALL + register tail-JMP/RET ${kind}/${left}/${right}`, helperRecord(0, 0x1002));
    assert.equal(guestValue(engine, 0x8ffc), 0x1002, 'algorithm pushes caller next PC');
  }
}
for (const args of [[], ['0', '1'], ['2', '0', '0'], ['0', '-1', '0'], ['0', '4294967296', '0'], ['0', '1x', '0'], ['0', '1', '0', 'extra']]) {
  const invalid = spawnSync(oraclePath, args, {encoding: 'utf8'});
  assert.equal(invalid.status, 2, `native utility rejects ${JSON.stringify(args)}`);
  assert.equal(invalid.stdout, '');
}

{
  const engine = fresh();
  for (const address of [0x4000, 0x8000]) assert.equal(engine.api.map(address, 1, 3), 0);
  word(engine, 0x4000, 0x1100);
  word(engine, 0x8ffc, 0x11223344);
  upload(engine, 0x1100, Uint8Array.from([0xc3]));
  const child = compile(engine, Uint8Array.from(forms.call_ebx), [[0x1000, 2], [0x1100, 1]], ['read32', 'store32']);
  const start = state();
  start.registers[3] = 0x4000;
  start.registers[4] = 0x9000;
  writeState(engine, start);
  run(engine, child, 0, start, exit(1, 0), 'budget0 skips target read and return push', arena(engine).slice(100, 140));
  assert.equal(guestValue(engine, 0x8ffc), 0x11223344);
  writeState(engine, start, 1);
  run(engine, child, 0, start, exit(2, 0), 'cancel outranks budget before indirect CALL', arena(engine).slice(100, 140));
  writeState(engine, start);
  const called = copyState(start);
  called.registers[4] = 0x8ffc;
  called.eip = 0x1100;
  run(engine, child, 1, called, exit(1, 1), 'indirect CALL budget1 checkpoint', helperRecord(0));
  word(engine, 0x4000, target);
  word(engine, 0x8ffc, 0x12345678);
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 20, called, exit(2, 0), 'cancel after completed indirect CALL', helperRecord(0));
  engine.view.setUint32(engine.base + 96, 0, true);
  const returned = copyState(start);
  returned.eip = 0x12345678;
  run(engine, child, 2, returned, exit(3, 1), 'CALL resume skips target reread/push and RET reads current stack', helperRecord(0, 0x12345678));
  assert.equal(guestValue(engine, 0x8ffc), 0x12345678);
}

const arithmeticPrefix = [0xb8, 0xff, 0xff, 0xff, 0xff, 0x83, 0xc0, 1];
const faultCases = [
  {name: 'CALL target unmapped suppresses writable return stack', form: 'call_ebx', esp: 0x8004, ea: 0x9000, access: 1, detail: 1, fault: 0x9000, map: [[0x8000, 1, 3]], words: [[0x8000, 0x11223344]]},
  {name: 'CALL target read permission', form: 'call_ebx', esp: 0x8004, ea: 0x4000, access: 1, detail: 2, fault: 0x4000, map: [[0x4000, 1, 2], [0x8000, 1, 3]], words: [[0x4000, target], [0x8000, 0x11223344]], observe: [[0x8000, 0x11223344]]},
  {name: 'CALL target cross-page permission', form: 'call_ebx', esp: 0x8004, ea: 0x4ffe, access: 1, detail: 2, fault: 0x5000, map: [[0x4000, 1, 3], [0x5000, 1, 2], [0x8000, 1, 3]], words: [[0x4ffc, target], [0x5000, target], [0x8000, 0x11223344]], observe: [[0x4ffc, target], [0x8000, 0x11223344]]},
  {name: 'CALL target width overflow', form: 'call_ebx', esp: 0x8004, ea: 0xfffffffd, access: 1, detail: 3, fault: 0xfffffffd, map: [[0x8000, 1, 3]], words: [[0x8000, 0x11223344]]},
  {name: 'CALL second return store unmapped', form: 'call_ebx', esp: 0x9004, ea: 0x4000, access: 2, detail: 1, fault: 0x9000, map: [[0x4000, 1, 3]], words: [[0x4000, target]]},
  {name: 'CALL second return write permission', form: 'call_ebx', esp: 0x8004, ea: 0x4000, access: 2, detail: 2, fault: 0x8000, map: [[0x4000, 1, 3], [0x8000, 1, 1]], words: [[0x4000, target], [0x8000, 0x11223344]]},
  {name: 'CALL second return cross-page store', form: 'call_ebx', esp: 0x9002, ea: 0x4000, access: 2, detail: 2, fault: 0x9000, map: [[0x4000, 1, 3], [0x8000, 1, 3], [0x9000, 1, 1]], words: [[0x4000, target], [0x8ffc, target], [0x9000, 0x11223344]]},
  {name: 'CALL second return width overflow', form: 'call_ebx', esp: 1, ea: 0x4000, access: 2, detail: 3, fault: 0xfffffffd, map: [[0x4000, 1, 3]], words: [[0x4000, target]]},
  {name: 'register CALL stack permission preserves captured source', form: 'call_esi', esp: 0x8004, access: 2, detail: 2, fault: 0x8000, map: [[0x8000, 1, 1]], words: [[0x8000, 0x11223344]]},
  {name: 'memory JMP target fault has no stack operation', form: 'jmp_ebx', esp: 0x9000, ea: 0x4000, access: 1, detail: 1, fault: 0x4000},
];
for (const fixture of faultCases) {
  const engine = fresh();
  for (const [address, pages] of fixture.map ?? []) assert.equal(engine.api.map(address, pages, 3), 0);
  for (const [address, value] of fixture.words ?? []) word(engine, address, value);
  for (const [address, pages, bits] of fixture.map ?? []) assert.equal(engine.api.protect(address, pages, bits), 0);
  const helpers = fixture.form === 'call_esi' ? ['store32'] : fixture.form === 'jmp_ebx' ? ['read32'] : ['read32', 'store32'];
  const child = compile(engine, Uint8Array.from([...arithmeticPrefix, ...forms[fixture.form]]), undefined, helpers);
  const start = state();
  start.registers[3] = fixture.ea ?? 0x4000;
  start.registers[4] = fixture.esp;
  start.registers[6] = target;
  const expected = copyState(start);
  expected.registers[0] = 0;
  expected.eflags = 0x457;
  expected.eip = 0x1008;
  writeState(engine, start);
  run(engine, child, 20, expected, exit(5, 2, fixture.detail, fixture.fault, fixture.access), fixture.name, helperRecord(1, 0, fixture.detail, fixture.fault, fixture.access));
  for (const [address, value] of fixture.observe ?? fixture.words ?? []) assert.equal(guestValue(engine, address), value, 'failed indirect transfer preserves readable target/stack bytes');
  guarded(engine, child, 'failed indirect transfer leaves code current');
}

for (const memory of [true, false]) {
  const engine = fresh(), callee = memory ? 0x1100 : 0x1008;
  if (memory) word(engine, 0x1ff0, callee);
  upload(engine, callee, Uint8Array.from([0xc3]));
  const bytes = memory ? forms.call_esp_minus4 : forms.call_esp;
  const child = compile(engine, Uint8Array.from(bytes), [[0x1000, bytes.length], [callee, 1]], ['read32', 'store32']);
  const start = state();
  start.registers[4] = memory ? 0x1ff4 : callee;
  const called = copyState(start);
  called.registers[4] -= 4;
  called.eip = callee;
  writeState(engine, start);
  run(engine, child, 1, called, exit(6, 1), `${memory ? 'overlapping memory' : 'old ESP register'} CALL SMC commits captured target`, helperRecord(0));
  assert.equal(guestValue(engine, called.registers[4]), 0x1000 + bytes.length, 'SMC return push overwrites target/nearby code with return PC');
  const before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 10, 0xffffffff), 4, 'stale indirect artifact rejects before target read');
  actualRuns++;
  assert.deepEqual(arena(engine), before);
  const replacement = compile(engine, new Uint8Array(), [[callee, 1]], ['read32']);
  const returned = copyState(start);
  returned.eip = 0x1000 + bytes.length;
  writeState(engine, called);
  run(engine, replacement, 2, returned, exit(3, 1), 'recompiled captured callee RET pops committed return without another CALL', helperRecord(0, returned.eip));
}

const injectedCases = [
  {name: 'memory CALL first-read infra suppresses return store', form: 'call_ebx', readRecord: helperRecord(2, 0, 1), reason: 7, detail: 1, reads: 1, stores: 0},
  {name: 'memory CALL second-store infra retains old ESP/PC', form: 'call_ebx', storeRecord: helperRecord(2, 0, 1), reason: 7, detail: 1, reads: 1, stores: 1},
  {name: 'memory CALL second-store protocol retains old ESP/PC', form: 'call_ebx', storeRecord: helperRecord(3), reason: 7, detail: 2, reads: 1, stores: 1},
  {name: 'memory CALL success retains target despite helper overwrite', form: 'call_ebx', reason: 3, detail: 0, reads: 1, stores: 1, commit: true},
  {name: 'memory CALL completion11 commits captured target', form: 'call_ebx', storeStatus: 11, reason: 6, detail: 0, reads: 1, stores: 1, commit: true},
  {name: 'memory CALL completion11 contradiction retains old ESP/PC', form: 'call_ebx', storeRecord: helperRecord(2, 0, 1), storeStatus: 11, reason: 7, detail: 2, reads: 1, stores: 1},
  {name: 'register CALL infra retains old target register', form: 'call_esi', storeRecord: helperRecord(2, 0, 1), reason: 7, detail: 1, reads: 0, stores: 1},
  {name: 'register CALL success captures target before return store', form: 'call_esi', reason: 3, detail: 0, reads: 0, stores: 1, commit: true},
  {name: 'register CALL completion11 captures target', form: 'call_esi', storeStatus: 11, reason: 6, detail: 0, reads: 0, stores: 1, commit: true},
  {name: 'memory JMP infra performs no return store', form: 'jmp_ebx', readRecord: helperRecord(2, 0, 1), reason: 7, detail: 1, reads: 1, stores: 0},
  {name: 'memory JMP protocol performs no return store', form: 'jmp_ebx', readRecord: helperRecord(3), reason: 7, detail: 2, reads: 1, stores: 0},
];
for (const fixture of injectedCases) {
  const engine = fresh();
  const helpers = fixture.form === 'call_esi' ? ['store32'] : fixture.form === 'jmp_ebx' ? ['read32'] : ['read32', 'store32'];
  const actual = compile(engine, Uint8Array.from([...arithmeticPrefix, ...forms[fixture.form]]), undefined, helpers);
  const injector = new WebAssembly.Instance(injectorModule, {env: {memory: engine.memory}, ring3: {guard: engine.api.guard}}).exports;
  const injected = new WebAssembly.Instance(actual.module, {env: {memory: engine.memory}, ring3: {guard: injector.guard, read32: injector.read32, store32: injector.store32}}).exports;
  const start = state();
  start.registers[3] = 0x4000;
  start.registers[4] = 0x9000;
  start.registers[6] = target;
  const expected = copyState(start);
  expected.registers[0] = 0;
  expected.eflags = 0x457;
  expected.eip = fixture.commit ? target : 0x1008;
  if (fixture.commit && fixture.form !== 'jmp_ebx') expected.registers[4] = 0x8ffc;
  writeState(engine, start);
  refresh(engine).bytes.set(fixture.readRecord ?? helperRecord(0, target), engine.base + 140);
  engine.bytes.set(fixture.storeRecord ?? helperRecord(0), engine.base + 180);
  injector.configure(engine.base + 100, engine.base + 140, engine.base + 180, 0, fixture.storeStatus ?? 0);
  run(engine, injected, 20, expected, exit(fixture.reason, fixture.commit ? 3 : 2, fixture.detail), `synthetic ${fixture.name}`, undefined, true);
  assert.equal(injector.read_calls(), fixture.reads);
  assert.equal(injector.store_calls(), fixture.stores);
  assert.equal(injector.order(), fixture.reads ? (fixture.stores ? 12 : 1) : 2, 'captured target precedes return push');
  if (fixture.reads) assert.equal(injector.read_address() >>> 0, 0x4000);
  if (fixture.stores) {
    assert.equal(injector.store_address() >>> 0, 0x8ffc);
    assert.equal(injector.store_value() >>> 0, 0x100a, 'second helper receives immediate return PC rather than target RESULT');
  }
  guarded(engine, actual, 'synthetic helper does not invalidate actual artifact');
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
  claim: 'actual embedded near indirect CALL/JMP, mutable RAM target, resident tail dispatch, precise faults and SMC; synthetic helpers prove sequence/target/return-PC branches only; native C proves unsigned arithmetic selection, not x86 hardware flags/counters',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, artifacts: outputDir}));
