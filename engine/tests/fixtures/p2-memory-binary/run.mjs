import assert from 'node:assert/strict';
import {execFileSync, spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const engineBytes = readFileSync(enginePath);
assert.ok(WebAssembly.validate(engineBytes), 'actual engine validates');
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), [], 'engine has no JS imports');
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'compile_with_gates'];
let keys = 0n, modules = 0, actualRuns = 0, ramWords = 0;
const counts = {flags: 0, aliases: 0, addresses: 0, read_faults: 0, continuation_scenario_runs: 0, actual_continuation_calls: 0, lifecycle: 0, folds: 0};
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

function compile(engine, bytes, blocks = [[0x1000, bytes.length]]) {
  if (bytes.length !== 0) upload(engine, 0x1000, bytes);
  refresh(engine);
  blocks.forEach(([entry, length], index) => {
    engine.view.setUint32(engine.base + 140 + index * 8, entry, true);
    engine.view.setUint32(engine.base + 144 + index * 8, length, true);
  });
  assert.equal(engine.api.compile(blocks.length), 0, 'actual engine compiles checked memory binary region');
  const pointer = engine.api.module_ptr() >>> 0, length = engine.api.module_len() >>> 0;
  refresh(engine);
  assert.ok(pointer > 0 && length > 8 && pointer + length <= engine.bytes.length);
  const encoded = engine.bytes.slice(pointer, pointer + length);
  assert.ok(WebAssembly.validate(encoded));
  const module = new WebAssembly.Module(encoded);
  const expected = [
    {module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: 'guard', kind: 'function'},
    {module: 'ring3', name: 'read32', kind: 'function'},
  ].sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sortedImports(module), expected, 'module imports exactly needed actual Wasm helpers');
  const instance = new WebAssembly.Instance(module, {
    env: {memory: engine.memory},
    ring3: {guard: engine.api.guard, read32: engine.api.read32},
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

function run(engine, child, budget, expected, expectedExit, label, expectedHelper = undefined) {
  const before = arena(engine);
  assert.equal(child.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, `${label}: canonical runtime exit status`);
  actualRuns++;
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

const fixtureRoot = join(root, 'engine/tests/fixtures/p2-memory-binary');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassemble = ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath];
writeFileSync(join(outputDir, 'integer.disassembly.txt'), execFileSync('xcrun', disassemble));
const object = readFileSync(objectPath), assembled = elfFixtures(object);
const ops = ['add', 'sub', 'cmp', 'and', 'or', 'xor'];
const registers = ['eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi'];
for (const [op, opcode] of [['add', 0x03], ['sub', 0x2b], ['cmp', 0x3b], ['and', 0x23], ['or', 0x0b], ['xor', 0x33]]) {
  assert.deepEqual([...assembled.get(op).bytes], [opcode, 0x06], `LLVM memory-source ${op}`);
}

function guarded(engine, child) {
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'memory-source operations preserve compiled code snapshot');
}

function sampleRam(engine, addresses) {
  return addresses.map(address => { ramWords++; return guestValue(engine, address); });
}

function result(op, lhs, rhs, oldFlags) {
  const a = BigInt(lhs), b = BigInt(rhs), mask = 0xffffffffn;
  const raw = op === 'add' ? a + b : op === 'sub' || op === 'cmp' ? a - b : op === 'and' ? a & b : op === 'or' ? a | b : a ^ b;
  const value = Number(raw & mask);
  let flags = (oldFlags & 0x400) | 2;
  let parity = 0;
  for (let byte = value & 255; byte !== 0; byte >>>= 1) parity += byte & 1;
  if (parity % 2 === 0) flags |= 4;
  if (value === 0) flags |= 0x40;
  if (value >= 0x80000000) flags |= 0x80;
  if (['add', 'sub', 'cmp'].includes(op)) {
    const adding = op === 'add';
    if (adding ? raw > mask : a < b) flags |= 1;
    if (adding ? (a & 15n) + (b & 15n) > 15n : (a & 15n) < (b & 15n)) flags |= 0x10;
    const signedA = BigInt.asIntN(32, a), signedB = BigInt.asIntN(32, b);
    const signed = adding ? signedA + signedB : signedA - signedB;
    if (signed < -2147483648n || signed > 2147483647n) flags |= 0x800;
  }
  return {value: op === 'cmp' ? lhs : value, flags};
}

function expectedBinary(start, op, rhs, register, length) {
  const expected = copyState(start), next = result(op, start.registers[register], rhs, start.eflags);
  expected.registers[register] = next.value;
  expected.eflags = next.flags;
  expected.eip += length;
  return expected;
}

const literals = [
  ['add', 0, 1, 1, 0x2], ['add', 0xffffffff, 1, 0, 0x57], ['add', 0x7fffffff, 1, 0x80000000, 0x896],
  ['add', 0x80000000, 0x80000000, 0, 0x847], ['sub', 0, 1, 0xffffffff, 0x97],
  ['sub', 0x80000000, 1, 0x7fffffff, 0x816], ['cmp', 0x7fffffff, 0xffffffff, 0x7fffffff, 0x887],
  ['and', 0xffffffff, 0, 0, 0x46], ['or', 0x80000000, 0, 0x80000000, 0x86],
  ['xor', 0x12345678, 0x12345678, 0, 0x46],
];
for (const [op, lhs, rhs, value, flags] of literals) assert.deepEqual(result(op, lhs, rhs, 2), {value, flags}, 'independent BigInt oracle agrees with literal result and flag cases');
const operands = [[0, 0], [0, 1], [0xffffffff, 1], [0x7fffffff, 1], [0x80000000, 1], [0x80000000, 0x80000000], [0, 0xffffffff], [0x10, 1], [0xf, 1], [0x12345678, 0x87654321], [0xaaaaaaaa, 0x55555555], [0xff, 0x101], [0x7fffffff, 0xffffffff]];
for (const op of ops) {
  const engine = fresh();
  assert.equal(engine.api.map(0x4000, 1, 3), 0);
  word(engine, 0x4000, 0x11223344); word(engine, 0x4008, 0x55667788); word(engine, 0x4ffc, 0xaabbccdd);
  const bytes = assembled.get(op).bytes, child = compile(engine, bytes);
  for (const [lhs, rhs] of operands) for (const flags of [2, 0xcd7]) {
    word(engine, 0x4004, rhs);
    const ram = sampleRam(engine, [0x4000, 0x4004, 0x4008, 0x4ffc, 0x1000]);
    const start = state(0x1000, flags); start.registers[0] = lhs; start.registers[6] = 0x4004;
    writeState(engine, start);
    run(engine, child, 1, expectedBinary(start, op, rhs, 0, bytes.length), exit(1, 1), `${op} memory flags/${lhs}/${rhs}/${flags}`, helperRecord(0, rhs));
    assert.deepEqual(sampleRam(engine, [0x4000, 0x4004, 0x4008, 0x4ffc, 0x1000]), ram, 'source, neighboring words, page tail and code RAM unchanged');
    guarded(engine, child); counts.flags++;
  }
}

for (const op of ops) for (const [register, reg] of registers.entries()) for (const sib of [false, true]) {
  const engine = fresh();
  assert.equal(engine.api.map(0x4000, 1, 3), 0); assert.equal(engine.api.map(0xc000, 1, 3), 0);
  const start = state(); start.registers[register] = 0x4004;
  if (sib && register === 4) start.registers[1] = 3;
  const address = !sib ? 0x4004 : register === 4 ? 0x400c : 0xc008;
  const rhs = 0xfedcba98;
  word(engine, address - 4, 0x11223344); word(engine, address, rhs); word(engine, address + 4, 0x55667788);
  const bytes = assembled.get(`${op}_${sib ? 'sib' : 'base'}_${reg}`).bytes, child = compile(engine, bytes);
  const ram = sampleRam(engine, [address - 4, address, address + 4]);
  writeState(engine, start);
  run(engine, child, 1, expectedBinary(start, op, rhs, register, bytes.length), exit(1, 1), `${op} old ${reg} ${sib ? 'SIB' : 'base'} alias`, helperRecord(0, rhs));
  assert.deepEqual(sampleRam(engine, [address - 4, address, address + 4]), ram);
  guarded(engine, child); counts.aliases++;
}

const addressForms = [
  {form: 'absolute', address: 0x4000, regs: {}},
  {form: 'no_base', address: 0x401c, regs: {1: 7}},
  {form: 'disp8_negative', address: 0x4000, regs: {3: 0x4080}},
  {form: 'disp8_positive', address: 0x407f, regs: {3: 0x4000}},
  {form: 'disp32', address: 0x4345, regs: {3: 0xffff2000}},
  {form: 'base_wrap', address: 4, regs: {3: 0xfffffffc}},
  {form: 'index_wrap', address: 0x4004, regs: {3: 0x4004, 1: 0x20000000}},
  {form: 'unaligned', address: 0x4003, regs: {3: 0x4000, 1: 2}},
  {form: null, address: 0x4ffc, regs: {6: 0x4ffc}},
  {form: null, address: 0x4ffe, regs: {6: 0x4ffe}},
  {form: null, address: 0xfffffffc, regs: {6: 0xfffffffc}},
];
for (const op of ops) for (const fixture of addressForms) {
  const engine = fresh();
  for (const [address, pages] of [[0, 1], [0x4000, 2], [0xfffff000, 1]]) assert.equal(engine.api.map(address, pages, 3), 0);
  const rhs = 0x90abcdef; word(engine, fixture.address, rhs);
  const start = state(); start.registers[0] = 0x76543210;
  Object.entries(fixture.regs).forEach(([reg, value]) => { start.registers[Number(reg)] = value; });
  const bytes = assembled.get(fixture.form ? `${op}_${fixture.form}` : op).bytes, child = compile(engine, bytes);
  const ram = sampleRam(engine, [fixture.address]);
  writeState(engine, start);
  run(engine, child, 1, expectedBinary(start, op, rhs, 0, bytes.length), exit(1, 1), `${op} ${fixture.form ?? 'page boundary'}/${fixture.address}`, helperRecord(0, rhs));
  assert.deepEqual(sampleRam(engine, [fixture.address]), ram);
  guarded(engine, child); counts.addresses++;
}

const faults = [
  {label: 'unmapped', address: 0x9000, detail: 1, fault: 0x9000},
  {label: 'write-only page', address: 0x4000, detail: 2, fault: 0x4000, maps: [[0x4000, 1, 2]]},
  {label: 'page-final Read4 missing next page', address: 0x4fff, detail: 1, fault: 0x5000, maps: [[0x4000, 1, 3]]},
  {label: 'cross-page Read4 permission', address: 0x4ffe, detail: 2, fault: 0x5000, maps: [[0x4000, 1, 3], [0x5000, 1, 2]]},
  {label: 'width overflow at final three bytes', address: 0xfffffffd, detail: 3, fault: 0xfffffffd, maps: [[0xfffff000, 1, 3]]},
  {label: 'width overflow at final byte', address: 0xffffffff, detail: 3, fault: 0xffffffff, maps: [[0xfffff000, 1, 3]]},
];
for (const op of ops) for (const fixture of faults) {
  const engine = fresh();
  for (const [address, pages] of fixture.maps ?? []) { assert.equal(engine.api.map(address, pages, 3), 0); word(engine, address, 0x11223344); }
  for (const [address, pages, permissions] of fixture.maps ?? []) assert.equal(engine.api.protect(address, pages, permissions), 0);
  const bytes = assembled.get(`${op}_prefix`).bytes, child = compile(engine, bytes);
  const start = state(); start.registers[6] = fixture.address;
  const stopped = copyState(start); stopped.registers[2] = 0; stopped.eflags = 0x457; stopped.eip = 0x1008;
  writeState(engine, start);
  run(engine, child, 20, stopped, exit(5, 2, fixture.detail, fixture.fault, 1), `${op} committed prefix then ${fixture.label}`, helperRecord(1, 0, fixture.detail, fixture.fault, 1));
  guarded(engine, child); counts.read_faults++;
}

for (const op of ops) {
  const engine = fresh(), bytes = assembled.get(`${op}_prefix`).bytes, child = compile(engine, bytes);
  const start = state(); start.registers[6] = 0x9000;
  writeState(engine, start);
  run(engine, child, 0, start, exit(1, 0), `${op} zero budget before prefix/read`, arena(engine).slice(100, 140)); counts.continuation_scenario_runs++;
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 0, start, exit(2, 0), `${op} cancel outranks zero budget`, arena(engine).slice(100, 140)); counts.continuation_scenario_runs++; counts.actual_continuation_calls++;
  engine.view.setUint32(engine.base + 96, 0, true);
  const stopped = copyState(start); stopped.registers[2] = 0; stopped.eflags = 0x457; stopped.eip = 0x1008;
  run(engine, child, 20, stopped, exit(5, 2, 1, 0x9000, 1), `${op} initial precise read fault`, helperRecord(1, 0, 1, 0x9000, 1)); counts.continuation_scenario_runs++; counts.actual_continuation_calls++;
  run(engine, child, 1, stopped, exit(5, 0, 1, 0x9000, 1), `${op} unrepaired retry does not repeat prefix`, helperRecord(1, 0, 1, 0x9000, 1)); counts.continuation_scenario_runs++; counts.actual_continuation_calls++;
  assert.equal(engine.api.map(0x9000, 1, 3), 0); word(engine, 0x9000, 0x89abcdef);
  const repaired = expectedBinary(stopped, op, 0x89abcdef, 0, assembled.get(op).bytes.length);
  run(engine, child, 1, repaired, exit(1, 1), `${op} repair resumes faulting read only`, helperRecord(0, 0x89abcdef)); counts.continuation_scenario_runs++; counts.actual_continuation_calls++;
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 20, repaired, exit(2, 0), `${op} cancel between memory operation and successor`, helperRecord(0, 0x89abcdef)); counts.continuation_scenario_runs++; counts.actual_continuation_calls++;
  engine.view.setUint32(engine.base + 96, 0, true);
  const next = copyState(repaired); next.registers[1] = 0x99999999; next.eip = 0x1000 + bytes.length;
  run(engine, child, 1, next, exit(1, 1), `${op} interior successor after repaired read`, helperRecord(0, 0x89abcdef)); counts.continuation_scenario_runs++; counts.actual_continuation_calls++;
  run(engine, child, 1, next, exit(3, 0), `${op} unknown continuation retires no extra operation`, helperRecord(0, 0x89abcdef)); counts.continuation_scenario_runs++; counts.actual_continuation_calls++;
  guarded(engine, child);
}

for (const op of ops) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0); word(engine, 0x4000, 0x89abcdef);
  const child = compile(engine, assembled.get(op).bytes), start = state(); start.registers[6] = 0x4000;
  writeState(engine, start);
  assert.equal(engine.api.protect(0x1000, 1, 5), 0);
  let before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 4, `${op} stale guard precedes invalid pointers`); actualRuns++; counts.lifecycle++;
  assert.deepEqual(arena(engine), before, 'stale rejection preserves complete arena');
  assert.equal(engine.api.close(), 0); before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 5, `${op} closed guard precedes invalid pointers`); actualRuns++; counts.lifecycle++;
  assert.deepEqual(arena(engine), before, 'closed rejection preserves complete arena');
}

const oracleSource = join(fixtureRoot, 'binary_oracle.c'), oraclePath = join(outputDir, 'binary_oracle');
const oracleCompile = ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', oracleSource, '-o', oraclePath];
execFileSync('clang', oracleCompile, {stdio: ['ignore', 'pipe', 'pipe']});
const foldInputs = [
  {seed: 0, rows: [[0, 0, 0xffffffff, 0, 0, 0]]},
  {seed: 0xffffffff, rows: [[1, 0, 0xffffffff, 0, 1, 0]]},
  {seed: 0x80000000, rows: [[0x80000000, 0xabcdef01, 0xf0f0f0f0, 0x01010101, 0xffffffff, 0xffffffff], [0xffffffff, 0x55555555, 0xffffffff, 0, 0x80000000, 1]]},
  {seed: 0x12345678, rows: [[0x9abcdef0, 0x77777777, 0xffffffff, 1, 2, 0x80000000], [7, 3, 0xffff, 0x10000, 0xffffffff, 0xffffffff], [0x80000000, 0xffffffff, 0xaaaaaaaa, 0x55555555, 0x7fffffff, 0]]},
  {seed: 0, rows: [[0, 0, 0, 0, 0, 1], [1, 0, 0xffffffff, 0, 0, 1], [0xffffffff, 0, 0xffffffff, 0, 0, 1], [0, 0, 0xffffffff, 0, 0, 0], [1, 1, 0xffffffff, 0, 0, 1], [0x80000000, 0, 0xffffffff, 0, 0, 0xffffffff]]},
];
let belowBranches = 0, aboveBranches = 0, foldRetired = 0;
for (const {seed, rows} of foldInputs) {
  const native = JSON.parse(execFileSync(oraclePath, [seed, ...rows.flat()].map(String), {encoding: 'utf8'}));
  let value = BigInt(seed), branchSum = 0, lastBranch = 0, below = 0, above = 0;
  for (const [add, xor, and, or, sub, cmp] of rows) {
    value = BigInt.asUintN(32, value + BigInt(add)); value ^= BigInt(xor); value &= BigInt(and); value |= BigInt(or); value = BigInt.asUintN(32, value - BigInt(sub));
    lastBranch = value < BigInt(cmp) ? 2 : 1; branchSum += lastBranch;
    if (lastBranch === 2) below++; else above++;
  }
  assert.deepEqual(native, {value: Number(value), branch_sum: branchSum, last_branch: lastBranch, below, above}, 'native unsigned C and independent BigInt array fold agree');
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0);
  rows.flat().forEach((wordValue, index) => { word(engine, 0x4000 + index * 4, wordValue); });
  word(engine, 0x4000 + rows.length * 24, 0x11223344); word(engine, 0x4ffc, 0x55667788);
  const names = ['fold_loop', 'fold_above', 'fold_below', 'fold_joined', 'fold_finish'];
  const blocks = names.map(name => { const fixture = assembled.get(name); upload(engine, 0x1000 + fixture.offset, fixture.bytes); return [0x1000 + fixture.offset, fixture.bytes.length]; });
  const child = compile(engine, new Uint8Array(), blocks), start = state(0x1600);
  start.registers[0] = seed; start.registers[1] = rows.length; start.registers[3] = 0; start.registers[6] = 0x4000;
  const expected = copyState(start); expected.registers[0] = native.value; expected.registers[1] = 0; expected.registers[2] = native.last_branch; expected.registers[3] = native.branch_sum; expected.registers[6] += rows.length * 24; expected.eip = 0x1700; expected.eflags = 0x446;
  const sampled = [...rows.flat().keys()].map(index => 0x4000 + index * 4).concat([0x4000 + rows.length * 24, 0x4ffc]);
  const ram = sampleRam(engine, sampled);
  writeState(engine, start);
  const retired = rows.length * 13 + 1;
  run(engine, child, 200, expected, exit(3, retired), `native C resident fold/${seed}/${rows.length}`, helperRecord(0, rows.at(-1)[5]));
  assert.deepEqual(sampleRam(engine, sampled), ram, 'fold preserves every input word and both sentinels');
  guarded(engine, child); counts.folds++; belowBranches += below; aboveBranches += above; foldRetired += retired;
}
assert.ok(belowBranches > 0 && aboveBranches > 0, 'resident fold takes both memory CMP/JB branches');
const invalidOracleInputs = [[], ['0'], ['0', ...Array(5).fill('0')], ['0', ...Array(7).fill('0')], ['-1', ...Array(6).fill('0')], ['4294967296', ...Array(6).fill('0')], ['0', '1x', ...Array(5).fill('0')], ['0', ' 0', ...Array(5).fill('0')], ['0', ...Array(42).fill('0')]];
for (const args of invalidOracleInputs) {
  const rejected = spawnSync(oraclePath, args, {encoding: 'utf8'});
  assert.equal(rejected.status, 2, `native utility rejects ${JSON.stringify(args)}`); assert.equal(rejected.stdout, '');
}
const provenance = {
  counts, generated_modules: modules, actual_engine_runs: actualRuns, ram_words_observed: ramWords,
  native_algorithm_cases: foldInputs.length, native_invalid_inputs: invalidOracleInputs.length,
  fold: {below_branches: belowBranches, above_branches: aboveBranches, retired: foldRetired},
  engine_sha256: hash(engineBytes), generated_set_sha256: hash(generatedHashes.join('\n')),
  sources: Object.fromEntries(['integer.S', 'binary_oracle.c', 'run.mjs'].map(name => [name, hash(readFileSync(join(fixtureRoot, name)))]).concat([['dbt_memory_binary_wasm.rs', hash(readFileSync(join(root, 'engine/tests/dbt_memory_binary_wasm.rs')))]])),
  artifacts: {object_sha256: hash(object), native_binary_sha256: hash(readFileSync(oraclePath))},
  tools: {node: process.version, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).trim(), llvm_objdump: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).trim()},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', ...disassemble], native_compile: ['clang', ...oracleCompile]},
  llvm_fixtures: [...assembled].map(([name, {offset, bytes}]) => ({name, offset, hex: bytes.toString('hex')})),
  claim: 'actual engine embedded flat32 ADD/SUB/CMP/AND/OR/XOR r32,m32, exact Read4 faults, complete arena/state and sampled RAM purity, old register address aliases, retry/resume/lifecycle, and resident unsigned C fold with both CMP/JB paths. AF is deterministically cleared for logical operations. Native C proves unsigned algorithm, not hardware x86 flags. No TEST, memory destinations, browser, title compatibility or performance claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, llvm_fixtures: provenance.llvm_fixtures.length, artifacts: outputDir}));
