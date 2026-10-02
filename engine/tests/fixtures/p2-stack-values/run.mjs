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

const registerNames = ['eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi'];
const registerForms = Array.from({length: 8}, (_, register) => [
  {name: `push_${register}`, bytes: [0x50 + register], register, push: true},
  {name: `push_modrm_${register}`, bytes: [0xff, 0xf0 + register], register, push: true, literalAlias: true},
  {name: `pop_${register}`, bytes: [0x58 + register], register, push: false},
  {name: `pop_modrm_${register}`, bytes: [0x8f, 0xc0 + register], register, push: false, literalAlias: true},
]).flat();
const immediateForms = [
  {name: 'imm8_negative128', bytes: [0x6a, 0x80], value: 0xffffff80},
  {name: 'imm8_negative1', bytes: [0x6a, 0xff], value: 0xffffffff},
  {name: 'imm8_zero', bytes: [0x6a, 0], value: 0},
  {name: 'imm8_positive127', bytes: [0x6a, 0x7f], value: 127},
  {name: 'imm32_negative129', bytes: [0x68, 0x7f, 0xff, 0xff, 0xff], value: 0xffffff7f},
  {name: 'imm32_high', bytes: [0x68, 0, 0, 0, 0x80], value: 0x80000000},
  {name: 'imm32_literal', bytes: [0x68, 0x78, 0x56, 0x34, 0x12], value: 0x12345678},
];
const cleanupForms = [0, 1, 4, 65535].map(cleanup => ({name: `cleanup_${cleanup}`, bytes: [0xc2, cleanup & 255, cleanup >>> 8], cleanup}));
const argumentsBytes = [0x51, 0x52, 0xe8, 1, 0, 0, 0, 0x90, 0x53, 0x8b, 0x5c, 0x24, 8, 0x8b, 0x44, 0x24, 12, 0x01, 0xd8, 0x5b, 0xc2, 8, 0];
const arithmeticPrefix = [0xb8, 0xff, 0xff, 0xff, 0xff, 0x83, 0xc0, 1];
const arithmeticForms = [
  {name: 'arithmetic_push', bytes: [...arithmeticPrefix, 0x50]},
  {name: 'arithmetic_pop_esp', bytes: [...arithmeticPrefix, 0x5c]},
  {name: 'arithmetic_pop_edi', bytes: [...arithmeticPrefix, 0x5f]},
  {name: 'arithmetic_cleanup', bytes: [...arithmeticPrefix, 0xc2, 0xff, 0xff]},
];
const smcBytes = [0x50, 0xb9, 0xaa, 0xaa, 0xaa, 0xaa];
const llvmFixtures = [...registerForms, ...immediateForms, ...cleanupForms, ...arithmeticForms, {name: 'arguments', bytes: argumentsBytes}, {name: 'smc_push', bytes: smcBytes}];
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-stack-values');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(outputDir, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), assembled = elfFixtures(object);
for (const fixture of llvmFixtures) {
  assert.deepEqual([...assembled.get(fixture.name).bytes], fixture.bytes, `LLVM fixture bytes ${fixture.name}`);
  if (fixture.literalAlias) {
    const start = disassembly.indexOf(`<${fixture.name}>:`);
    assert.ok(start >= 0);
    const end = disassembly.indexOf('\n\n', start);
    const text = disassembly.slice(start, end < 0 ? undefined : end);
    assert.match(text, new RegExp(`\\b${fixture.push ? 'push' : 'pop'}\\s+${registerNames[fixture.register]}\\b`), 'LLVM independently disassembles authored ModRM alias');
  }
}

function guarded(engine, child, label) {
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, label);
}

for (const fixture of registerForms) {
  const engine = fresh();
  assert.equal(engine.api.map(0x8000, 1, 3), 0);
  const child = compile(engine, Uint8Array.from(fixture.bytes), undefined, [fixture.push ? 'store32' : 'read32']);
  const start = state();
  start.registers[4] = fixture.push ? 0x9000 : 0x8ffc;
  word(engine, 0x8ffc, 0xfedcba98);
  const expected = copyState(start);
  if (fixture.push) expected.registers[4] = 0x8ffc;
  else {
    expected.registers[4] = 0x9000;
    expected.registers[fixture.register] = 0xfedcba98;
  }
  expected.eip += fixture.bytes.length;
  writeState(engine, start);
  run(engine, child, 1, expected, exit(1, 1), `${fixture.name}: exact alias and complete state`, helperRecord(0, fixture.push ? 0 : 0xfedcba98));
  assert.equal(guestValue(engine, 0x8ffc), fixture.push ? start.registers[fixture.register] : 0xfedcba98, `${fixture.name}: old source or read-only stack word`);
  guarded(engine, child, 'ordinary stack access preserves code stamps');
}

for (const fixture of immediateForms) {
  const engine = fresh();
  assert.equal(engine.api.map(0x8000, 1, 3), 0);
  const child = compile(engine, Uint8Array.from(fixture.bytes), undefined, ['store32']);
  const start = state();
  start.registers[4] = 0x9000;
  const expected = copyState(start);
  expected.registers[4] = 0x8ffc;
  expected.eip += fixture.bytes.length;
  writeState(engine, start);
  run(engine, child, 1, expected, exit(1, 1), `${fixture.name}: decoded immediate bit pattern`, helperRecord(0));
  assert.equal(guestValue(engine, 0x8ffc), fixture.value);
}

for (const fixture of cleanupForms) {
  const engine = fresh();
  assert.equal(engine.api.map(0x8000, 1, 3), 0);
  const child = compile(engine, Uint8Array.from(fixture.bytes), undefined, ['read32']);
  const start = state();
  start.registers[4] = 0x8ffc;
  word(engine, 0x8ffc, 0xf1234567);
  const expected = copyState(start);
  expected.registers[4] = 0x9000 + fixture.cleanup;
  expected.eip = 0xf1234567;
  writeState(engine, start);
  run(engine, child, 2, expected, exit(3, 1), `${fixture.name}: unsigned byte cleanup without argument access`, helperRecord(0, 0xf1234567));
  assert.equal(guestValue(engine, 0x8ffc), 0xf1234567, 'RET cleanup does not write popped return word');
  if (fixture.cleanup === 65535) {
    assert.equal(engine.api.read32(0x9000), 0);
    assert.deepEqual(arena(engine).slice(100, 140), helperRecord(1, 0, 1, 0x9000, 1), 'skipped argument bytes are actually unmapped');
  }
}

for (const cleanup of [1, 65535]) {
  const engine = fresh();
  assert.equal(engine.api.map(0xfffff000, 1, 3), 0);
  const child = compile(engine, Uint8Array.from([0xc2, cleanup & 255, cleanup >>> 8]), undefined, ['read32']);
  const start = state();
  start.registers[4] = 0xfffffffc;
  word(engine, 0xfffffffc, 0xf1234567);
  const expected = copyState(start);
  expected.registers[4] = cleanup;
  expected.eip = 0xf1234567;
  writeState(engine, start);
  run(engine, child, 2, expected, exit(3, 1), `RET cleanup${cleanup} wraps ESP after valid final-width read`, helperRecord(0, 0xf1234567));
  assert.equal(guestValue(engine, 0xfffffffc), 0xf1234567);
}

for (const push of [true, false]) {
  for (const low of [true, false]) {
    const engine = fresh(), address = low ? 0 : 0xfffffffc;
    assert.equal(engine.api.map(low ? 0 : 0xfffff000, 1, 3), 0);
    const child = compile(engine, Uint8Array.from([push ? 0x50 : 0x5f]), undefined, [push ? 'store32' : 'read32']);
    const start = state();
    start.registers[4] = push ? (address + 4) >>> 0 : address;
    word(engine, address, 0x12345678);
    const expected = copyState(start);
    expected.registers[4] = push ? address : (address + 4) >>> 0;
    if (!push) expected.registers[7] = 0x12345678;
    expected.eip++;
    writeState(engine, start);
    run(engine, child, 1, expected, exit(1, 1), `${push ? 'PUSH' : 'POP'} valid access${address} with arithmetic ESP wrap`, helperRecord(0, push ? 0 : 0x12345678));
    assert.equal(guestValue(engine, address), push ? start.registers[0] : 0x12345678);
  }
}

const oracleSource = join(fixtureRoot, 'stack_values_oracle.c'), oraclePath = join(outputDir, 'stack_values_oracle');
const oracleCompile = ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', oracleSource, '-o', oraclePath];
execFileSync('clang', oracleCompile, {stdio: ['ignore', 'pipe', 'pipe']});
const nativeInputs = [[0, 0, 0, 0x446], [0xffffffff, 1, 0, 0x457], [0x7fffffff, 1, 0x80000000, 0xc96], [0x80000000, 0x80000000, 0, 0xc47], [0x12345678, 0x87654321, 0x99999999, 0x486]];
for (const [first, second, sum, flags] of nativeInputs) {
  const native = JSON.parse(execFileSync(oraclePath, [String(first), String(second)], {encoding: 'utf8'}));
  assert.deepEqual(native, {sum}, 'independent native unsigned argument algorithm');
  assert.equal(sum, Number(BigInt.asUintN(32, BigInt(first) + BigInt(second))));
  const engine = fresh();
  assert.equal(engine.api.map(0x8000, 1, 3), 0);
  const child = compile(engine, Uint8Array.from(argumentsBytes), [[0x1000, 7], [0x1008, 15]], ['read32', 'store32']);
  const start = state();
  start.registers[1] = second;
  start.registers[2] = first;
  start.registers[4] = 0x9000;
  const expected = copyState(start);
  expected.registers[0] = sum;
  expected.eip = 0x1007;
  expected.eflags = flags;
  writeState(engine, start);
  run(engine, child, 20, expected, exit(3, 9), `pushed arguments, EBX save/restore and RET8/${first}/${second}`, helperRecord(0, 0x1007));
  assert.equal(guestValue(engine, 0x8ffc), second, 'second argument remains unchanged');
  assert.equal(guestValue(engine, 0x8ff8), first, 'first argument remains unchanged');
  assert.equal(guestValue(engine, 0x8ff4), 0x1007, 'exact caller continuation');
  assert.equal(guestValue(engine, 0x8ff0), start.registers[3], 'saved EBX stack value remains unchanged after POP');
}
for (const args of [[], ['1'], ['-1', '0'], ['4294967296', '0'], ['1x', '0'], ['1', ' 0'], ['1', '0', 'extra']]) {
  const result = spawnSync(oraclePath, args, {encoding: 'utf8'});
  assert.equal(result.status, 2, `native utility rejects ${JSON.stringify(args)}`);
  assert.equal(result.stdout, '');
}

{
  const engine = fresh();
  assert.equal(engine.api.map(0x8000, 1, 3), 0);
  const child = compile(engine, Uint8Array.from([0x50, 0x59, 0xc2, 4, 0]), undefined, ['read32', 'store32']);
  const start = state();
  start.registers[4] = 0x8004;
  word(engine, 0x8000, 0xaaaaaaaa);
  word(engine, 0x8004, 0x12345678);
  writeState(engine, start);
  run(engine, child, 0, start, exit(1, 0), 'budget0 skips PUSH', arena(engine).slice(100, 140));
  assert.equal(guestValue(engine, 0x8000), 0xaaaaaaaa);
  writeState(engine, start, 1);
  run(engine, child, 0, start, exit(2, 0), 'cancel outranks budget before PUSH', arena(engine).slice(100, 140));
  writeState(engine, start);
  const pushed = copyState(start);
  pushed.registers[4] = 0x8000;
  pushed.eip = 0x1001;
  run(engine, child, 1, pushed, exit(1, 1), 'one PUSH checkpoint', helperRecord(0));
  assert.equal(guestValue(engine, 0x8000), start.registers[0]);
  word(engine, 0x8000, 0x87654321);
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 10, pushed, exit(2, 0), 'cancel between PUSH and POP', helperRecord(0));
  engine.view.setUint32(engine.base + 96, 0, true);
  const popped = copyState(start);
  popped.registers[1] = 0x87654321;
  popped.eip = 0x1002;
  run(engine, child, 1, popped, exit(1, 1), 'one POP resume never repeats PUSH', helperRecord(0, 0x87654321));
  assert.equal(guestValue(engine, 0x8000), 0x87654321);
  const returned = copyState(popped);
  returned.registers[4] = 0x800c;
  returned.eip = 0x12345678;
  run(engine, child, 1, returned, exit(1, 1), 'one RET4 cleanup checkpoint', helperRecord(0, 0x12345678));
  run(engine, child, 2, returned, exit(3, 0), 'unknown target after RET resume never repeats cleanup', helperRecord(0, 0x12345678));
}

{
  const engine = fresh();
  assert.equal(engine.api.map(0x8000, 1, 3), 0);
  const child = compile(engine, Uint8Array.from([0x5c, 0x50]), undefined, ['read32', 'store32']);
  const start = state();
  start.registers[4] = 0x8000;
  word(engine, 0x8000, 0x8008);
  word(engine, 0x8004, 0xaaaaaaaa);
  writeState(engine, start);
  const popped = copyState(start);
  popped.registers[4] = 0x8008;
  popped.eip = 0x1001;
  run(engine, child, 1, popped, exit(1, 1), 'POP ESP loaded value wins increment at budget boundary', helperRecord(0, 0x8008));
  const pushed = copyState(popped);
  pushed.registers[4] = 0x8004;
  pushed.eip = 0x1002;
  run(engine, child, 1, pushed, exit(1, 1), 'PUSH resumes using committed POP ESP value', helperRecord(0));
  assert.equal(guestValue(engine, 0x8000), 0x8008, 'POP source word remains unchanged');
  assert.equal(guestValue(engine, 0x8004), start.registers[0], 'next PUSH uses loaded ESP');
}

const faultCases = [
  {name: 'PUSH unmapped', operation: 'push', address: 0x9000, detail: 1, fault: 0x9000},
  {name: 'PUSH write permission', operation: 'push', address: 0x4000, detail: 2, fault: 0x4000, map: [[0x4000, 1, 1]]},
  {name: 'PUSH cross-page permission', operation: 'push', address: 0x4ffe, detail: 2, fault: 0x5000, map: [[0x4000, 1, 3], [0x5000, 1, 1]]},
  {name: 'PUSH width overflow', operation: 'push', address: 0xfffffffd, detail: 3, fault: 0xfffffffd},
  {name: 'POP ESP unmapped', operation: 'pop_esp', address: 0x9000, detail: 1, fault: 0x9000},
  {name: 'POP ESP read permission', operation: 'pop_esp', address: 0x4000, detail: 2, fault: 0x4000, map: [[0x4000, 1, 2]]},
  {name: 'POP EDI cross-page permission', operation: 'pop_edi', address: 0x4ffe, detail: 2, fault: 0x5000, map: [[0x4000, 1, 3], [0x5000, 1, 2]]},
  {name: 'POP ESP width overflow', operation: 'pop_esp', address: 0xfffffffd, detail: 3, fault: 0xfffffffd},
  {name: 'RET65535 width overflow before cleanup', operation: 'cleanup', address: 0xfffffffd, detail: 3, fault: 0xfffffffd},
];
function operationBytes(operation) {
  return operation === 'push' ? [0x50] : operation === 'pop_esp' ? [0x5c] : operation === 'pop_edi' ? [0x5f] : [0xc2, 0xff, 0xff];
}
for (const fault of faultCases) {
  const engine = fresh();
  for (const [address, pages] of fault.map ?? []) assert.equal(engine.api.map(address, pages, 3), 0);
  if (fault.map?.some(([address]) => address === 0x4000)) {
    word(engine, 0x4000, 0x11223344);
    word(engine, 0x4ffc, 0xa1b2c3d4);
  }
  if (fault.map?.some(([address]) => address === 0x5000)) word(engine, 0x5000, 0x55667788);
  for (const [address, pages, bits] of fault.map ?? []) assert.equal(engine.api.protect(address, pages, bits), 0);
  const push = fault.operation === 'push';
  const bytes = [...arithmeticPrefix, ...operationBytes(fault.operation), ...(fault.operation === 'cleanup' ? [] : [0xb9, 0x99, 0x99, 0x99, 0x99])];
  const child = compile(engine, Uint8Array.from(bytes), undefined, [push ? 'store32' : 'read32']);
  const start = state();
  start.registers[4] = push ? (fault.address + 4) >>> 0 : fault.address;
  const expected = copyState(start);
  expected.registers[0] = 0;
  expected.eflags = 0x457;
  expected.eip = 0x1008;
  writeState(engine, start);
  const access = push ? 2 : 1;
  run(engine, child, 20, expected, exit(5, 2, fault.detail, fault.fault, access), `${fault.name}: flags and every precommit effect preserved`, helperRecord(1, 0, fault.detail, fault.fault, access));
  if (push && fault.map) {
    assert.equal(guestValue(engine, 0x4000), 0x11223344, 'PUSH fault preserves attempted word');
    assert.equal(guestValue(engine, 0x4ffc), 0xa1b2c3d4, 'PUSH fault preserves first-page tail');
    if (fault.map.some(([address]) => address === 0x5000)) assert.equal(guestValue(engine, 0x5000), 0x55667788, 'PUSH fault preserves second-page head');
  }
  guarded(engine, child, 'failed stack value instruction preserves code stamps');
}

{
  const engine = fresh();
  const child = compile(engine, Uint8Array.from(smcBytes), undefined, ['store32']);
  const start = state();
  start.registers[0] = 0x11223344;
  start.registers[4] = 0x1006;
  const pushed = copyState(start);
  pushed.registers[4] = 0x1002;
  pushed.eip = 0x1001;
  writeState(engine, start);
  run(engine, child, 1, pushed, exit(6, 1), 'PUSH SMC commits stack and next EIP before successor/budget', helperRecord(0));
  assert.equal(guestValue(engine, 0x1002), 0x11223344, 'PUSH patched exact next MOV immediate');
  const before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 10, 0xffffffff), 4, 'stale PUSH artifact rejected before reads');
  actualRuns++;
  assert.deepEqual(arena(engine), before);
  const replacement = compile(engine, new Uint8Array(), [[0x1000, 6]], ['store32']);
  const expected = copyState(pushed);
  expected.registers[1] = 0x11223344;
  expected.eip = 0x1006;
  writeState(engine, pushed);
  run(engine, replacement, 2, expected, exit(3, 1), 'recompiled successor resumes without repeating PUSH', arena(engine).slice(100, 140));
  assert.equal(guestValue(engine, 0x1002), 0x11223344);
}

const injectedCases = [
  {name: 'VersionExhausted', status: 0, record: helperRecord(2, 0, 1), detail: 1},
  {name: 'bad helper tag', status: 0, record: helperRecord(3), detail: 2},
  {name: 'closed helper', status: 5, record: helperRecord(0), detail: 3},
];
for (const operation of ['push', 'pop_esp', 'cleanup']) {
  const engine = fresh(), push = operation === 'push';
  const actual = compile(engine, Uint8Array.from([...arithmeticPrefix, ...operationBytes(operation)]), undefined, [push ? 'store32' : 'read32']);
  const injector = new WebAssembly.Instance(injectorModule, {env: {memory: engine.memory}, ring3: {guard: engine.api.guard}}).exports;
  const injected = new WebAssembly.Instance(actual.module, {env: {memory: engine.memory}, ring3: {guard: injector.guard, read32: injector.read32, store32: injector.store32}}).exports;
  const cases = [...injectedCases];
  if (push) cases.push({name: 'completion11 non-success', status: 11, record: helperRecord(2, 0, 1), detail: 2});
  if (operation === 'pop_esp') cases.push({name: 'read completion11 rejected', status: 11, record: helperRecord(0), detail: 3});
  for (const fixture of cases) {
    const start = state();
    start.registers[4] = push ? 0x9004 : 0x9000;
    const expected = copyState(start);
    expected.registers[0] = 0;
    expected.eflags = 0x457;
    expected.eip = 0x1008;
    writeState(engine, start);
    refresh(engine).bytes.set(fixture.record, engine.base + 140);
    injector.configure(engine.base + 100, engine.base + 140, fixture.status);
    run(engine, injected, 20, expected, exit(7, 2, fixture.detail), `synthetic ${operation}/${fixture.name}: uncommitted stack effects`, undefined, true);
    assert.equal(injector.calls(), 1);
    guarded(engine, actual, 'synthetic helper preserves actual artifact stamps');
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
  llvm_fixtures: llvmFixtures.map(fixture => ({name: fixture.name, offset: assembled.get(fixture.name).offset, hex: assembled.get(fixture.name).bytes.toString('hex'), source: fixture.literalAlias ? 'authored opcode alias independently disassembled by LLVM' : 'authored Intel assembly independently assembled by LLVM'})),
  claim: 'actual flat32 register/immediate PUSH, register POP and near RET cleanup; synthetic helper runs prove emitter failure branches only; native C proves unsigned argument algorithm, not x86 hardware flags/counters or calling-convention policy',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, artifacts: outputDir}));
