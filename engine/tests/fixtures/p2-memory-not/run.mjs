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
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'compile_with_gates', 'compile_entries', 'store32'];
let keys = 0n, modules = 0, actualRuns = 0, ramWords = 0, snapshotGuards = 0;
const counts = {values_flags: 0, allowed_flag_patterns: 0, aliases: 0, addresses: 0, entry: 0, read_faults: 0, store_faults: 0, continuation_initial_calls: 0, continuation_resume_calls: 0, canonical_exits: 0, guard_rejections: 0, code_writes: 0, involutions: 0};
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

function compile(engine, bytes, blocks = [[0x1000, bytes.length]], entry = false) {
  if (bytes.length !== 0) upload(engine, 0x1000, bytes);
  refresh(engine);
  blocks.forEach(([entry, length], index) => {
    engine.view.setUint32(engine.base + 140 + index * 8, entry, true);
    engine.view.setUint32(engine.base + 144 + index * 8, length, true);
  });
  if (entry) {
    engine.view.setUint32(engine.base + 140, 0x1000, true);
    assert.equal(engine.api.compile_entries(1, 0), 0, 'actual entry-PC preparation with no guest data read');
  } else {
    assert.equal(engine.api.compile(blocks.length), 0, 'actual explicit memory NOT preparation');
  }
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
    {module: 'ring3', name: 'store32', kind: 'function'},
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

function run(engine, child, budget, expected, expectedExit, label, expectedHelper = undefined, continuation = undefined) {
  const before = arena(engine);
  assert.equal(child.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, `${label}: canonical runtime exit status`);
  actualRuns++; counts.canonical_exits++;
  if (continuation === 'initial') counts.continuation_initial_calls++;
  else if (continuation === 'resume') counts.continuation_resume_calls++;
  assert.deepEqual(readState(engine), expected, `${label}: complete CPU state`);
  const wanted = before.slice(), view = new DataView(wanted.buffer);
  expected.registers.forEach((register, index) => view.setUint32(16 + index * 4, register, true));
  view.setUint32(48, expected.eip, true); view.setUint32(52, expected.eflags, true);
  wanted.set(expectedExit, 56);
  if (expectedHelper !== undefined) wanted.set(expectedHelper, 100);
  assert.deepEqual(arena(engine), wanted, `${label}: complete 4236-byte arena, including state/exit/helper/cancel/transfer`);
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

const fixtureRoot = join(root, 'engine/tests/fixtures/p2-memory-not');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassemble = ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath];
writeFileSync(join(outputDir, 'integer.disassembly.txt'), execFileSync('xcrun', disassemble));
const object = readFileSync(objectPath), assembled = elfFixtures(object), registers = ['eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi'];
assert.equal(assembled.get('not').bytes.toString('hex'), 'f716', 'LLVM F7/2 NOT m32');
assert.equal(assembled.get('not_entry').bytes.toString('hex'), 'f716eb00', 'LLVM entry includes terminating short JMP');
assert.equal(assembled.get('not_prefix').bytes.toString('hex'), 'baffffffff83c201f716b999999999', 'LLVM arithmetic prefix/NOT/successor');
assert.equal(assembled.get('not_smc').bytes.toString('hex'), 'f716b9bbbbbbbb', 'LLVM NOT/code-word/successor');

function complement(value) {
  return Number(BigInt.asUintN(32, ~BigInt(value)));
}

function guarded(engine, child) {
  const before = arena(engine);
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'data NOT or rejected two-access instruction preserves code snapshot');
  snapshotGuards++;
  assert.deepEqual(arena(engine), before, 'successful code guard preserves complete arena');
}

function reject(engine, child, status, label) {
  const before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 20, 0xffffffff), status, label);
  actualRuns++; counts.guard_rejections++;
  assert.deepEqual(arena(engine), before, `${label}: complete arena unchanged`);
}

function sampleRam(engine, addresses) {
  return addresses.map(address => { ramWords++; return guestValue(engine, address); });
}

function nextNot(start, length) {
  const expected = copyState(start); expected.eip += length; return expected;
}

const literals = [[0, 0xffffffff], [1, 0xfffffffe], [0xffffffff, 0], [0x80000000, 0x7fffffff], [0x7fffffff, 0x80000000], [0x12345678, 0xedcba987], [0xaaaaaaaa, 0x55555555], [0x00ff00ff, 0xff00ff00]];
for (const [value, inverse] of literals) {
  assert.equal(complement(value), inverse, 'independent BigInt complement agrees with fixed literal');
  assert.equal(complement(inverse), value, 'independent involution anchor');
  for (let offset = 0; offset < 32; offset += 8) assert.notEqual((value >>> offset) & 255, (inverse >>> offset) & 255, 'NOT changes every byte');
}
{
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0);
  word(engine, 0x4000, 0x11223344); word(engine, 0x4008, 0x55667788); word(engine, 0x4ffc, 0xaabbccdd);
  const bytes = assembled.get('not').bytes, child = compile(engine, bytes);
  const flagBits = [1, 4, 0x10, 0x40, 0x80, 0x400, 0x800];
  const flagPatterns = Array.from({length: 128}, (_, pattern) => flagBits.reduce((value, bit, index) => value | (((pattern >>> index) & 1) ? bit : 0), 2));
  const cases = literals.flatMap(([value]) => [2, 0xcd7].map(eflags => ({value, eflags}))).concat(flagPatterns.map((eflags, index) => ({value: literals[index % literals.length][0], eflags})));
  assert.equal(new Set(flagPatterns).size, 128);
  for (const {value, eflags} of cases) {
    word(engine, 0x4004, value);
    const sampled = sampleRam(engine, [0x4000, 0x4008, 0x4ffc, 0x1000]);
    const start = state(0x1000, eflags); start.registers[6] = 0x4004;
    writeState(engine, start);
    run(engine, child, 1, nextNot(start, bytes.length), exit(1, 1), `NOT literal/allowed flags/${value}/${eflags}`, helperRecord(0));
    assert.equal(sampleRam(engine, [0x4004])[0], complement(value));
    assert.deepEqual(sampleRam(engine, [0x4000, 0x4008, 0x4ffc, 0x1000]), sampled, 'only the selected word changes; neighbors/page tail/code preserved');
    guarded(engine, child); counts.values_flags++;
  }
  counts.allowed_flag_patterns = flagPatterns.length;
}

for (const [register, name] of registers.entries()) for (const kind of ['base', 'sib', ...(register === 4 ? [] : ['index'])]) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0); assert.equal(engine.api.map(0xc000, 1, 3), 0);
  const start = state(); start.registers[register] = kind === 'index' ? 2 : 0x4004;
  if (kind === 'index' && register !== 3) start.registers[3] = 0x4000;
  if (kind === 'sib' && register === 4) start.registers[1] = 3;
  const address = kind === 'base' ? 0x4004 : kind === 'sib' ? register === 4 ? 0x400c : 0xc008 : register === 3 ? 6 : 0x4004;
  if (address === 6) assert.equal(engine.api.map(0, 1, 3), 0);
  const value = 0xfedcba98;
  word(engine, address - 4, 0x11223344); word(engine, address, value); word(engine, address + 4, 0x55667788);
  const bytes = assembled.get(`not_${kind}_${name}`).bytes, child = compile(engine, bytes);
  assert.equal(bytes[0], 0xf7); assert.equal((bytes[1] >>> 3) & 7, 2, 'LLVM NOT group2');
  if (kind === 'index') assert.equal((bytes[2] >>> 3) & 7, register, 'LLVM old index GPR');
  const sampled = sampleRam(engine, [address - 4, address + 4, 0x1000]);
  writeState(engine, start);
  run(engine, child, 1, nextNot(start, bytes.length), exit(1, 1), `NOT old ${name}/${kind} EA preserves all GPRs`, helperRecord(0));
  assert.equal(sampleRam(engine, [address])[0], complement(value));
  assert.deepEqual(sampleRam(engine, [address - 4, address + 4, 0x1000]), sampled);
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
for (const fixture of addressForms) {
  const engine = fresh();
  for (const [address, pages] of [[0, 1], [0x4000, 2], [0xfffff000, 1]]) assert.equal(engine.api.map(address, pages, 3), 0);
  const value = 0x90abcdef; word(engine, fixture.address, value);
  const start = state(); Object.entries(fixture.regs).forEach(([reg, data]) => { start.registers[Number(reg)] = data; });
  const bytes = assembled.get(fixture.form ? `not_${fixture.form}` : 'not').bytes, child = compile(engine, bytes);
  const sampled = sampleRam(engine, [0x1000]);
  writeState(engine, start);
  run(engine, child, 1, nextNot(start, bytes.length), exit(1, 1), `NOT ${fixture.form ?? 'page boundary'}/${fixture.address}`, helperRecord(0));
  assert.equal(sampleRam(engine, [fixture.address])[0], complement(value));
  assert.deepEqual(sampleRam(engine, [0x1000]), sampled);
  guarded(engine, child); counts.addresses++;
}

{
  const engine = fresh(), bytes = assembled.get('not_entry').bytes, child = compile(engine, bytes, undefined, true);
  const start = state(); start.registers[6] = 0x9000;
  writeState(engine, start);
  run(engine, child, 1, start, exit(5, 0, 1, 0x9000, 1), 'entry prepared with unmapped data faults only on actual read', helperRecord(1, 0, 1, 0x9000, 1), 'initial');
  assert.equal(engine.api.map(0x9000, 1, 3), 0); word(engine, 0x9000, 0x80000000);
  const repaired = nextNot(start, assembled.get('not').bytes.length);
  run(engine, child, 1, repaired, exit(1, 1), 'entry mapping repair resumes same NOT artifact', helperRecord(0), 'resume');
  assert.equal(sampleRam(engine, [0x9000])[0], 0x7fffffff);
  guarded(engine, child); counts.entry += 2;
}

const faults = [
  {label: 'unmapped first read', address: 0x9000, detail: 1, fault: 0x9000, access: 1},
  {label: 'write-only destination fails first read', address: 0x4000, detail: 2, fault: 0x4000, access: 1, maps: [[0x4000, 1, 2]], observe: []},
  {label: 'page-final first read missing next page', address: 0x4fff, detail: 1, fault: 0x5000, access: 1, maps: [[0x4000, 1, 3]]},
  {label: 'cross-page first read permission', address: 0x4ffe, detail: 2, fault: 0x5000, access: 1, maps: [[0x4000, 1, 3], [0x5000, 1, 2]], observe: [0x4ffc]},
  {label: 'nonwrapping width overflow at final three bytes', address: 0xfffffffd, detail: 3, fault: 0xfffffffd, access: 1, maps: [[0xfffff000, 1, 3]]},
  {label: 'nonwrapping width overflow at final byte', address: 0xffffffff, detail: 3, fault: 0xffffffff, access: 1, maps: [[0xfffff000, 1, 3]]},
  {label: 'read-only destination fails second store', address: 0x4000, detail: 2, fault: 0x4000, access: 2, maps: [[0x4000, 1, 1]]},
  {label: 'cross-page second store cannot partially write first page', address: 0x4ffe, detail: 2, fault: 0x5000, access: 2, maps: [[0x4000, 1, 3], [0x5000, 1, 1]]},
  {label: 'readable cross-page span has denied write at first byte', address: 0x4ffe, detail: 2, fault: 0x4ffe, access: 2, maps: [[0x4000, 1, 1], [0x5000, 1, 3]]},
];
for (const fixture of faults) {
  const engine = fresh(), words = [];
  for (const [address, pages] of fixture.maps ?? []) { assert.equal(engine.api.map(address, pages, 3), 0); words.push([address === 0x4000 ? 0x4ffc : address, 0x11223344]); }
  if (fixture.address === 0x4000) words.push([0x4000, 0xa1b2c3d4]);
  for (const [address, value] of words) word(engine, address, value);
  for (const [address, pages, permissions] of fixture.maps ?? []) assert.equal(engine.api.protect(address, pages, permissions), 0);
  const sampledAddresses = fixture.observe ?? words.map(([address]) => address);
  const bytes = assembled.get('not_prefix').bytes, child = compile(engine, bytes);
  const sampled = sampleRam(engine, sampledAddresses.concat([0x1000]));
  const start = state(); start.registers[6] = fixture.address;
  const stopped = copyState(start); stopped.registers[2] = 0; stopped.eflags = 0x457; stopped.eip = 0x1008;
  writeState(engine, start);
  run(engine, child, 20, stopped, exit(5, 2, fixture.detail, fixture.fault, fixture.access), `prefix/NOT/${fixture.label}`, helperRecord(1, 0, fixture.detail, fixture.fault, fixture.access));
  assert.deepEqual(sampleRam(engine, sampledAddresses.concat([0x1000])), sampled, 'failed read or Store4 preserves every readable destination word and code');
  guarded(engine, child); if (fixture.access === 1) counts.read_faults++; else counts.store_faults++;
}

for (const secondStore of [false, true]) {
  const engine = fresh(), bytes = assembled.get('not_prefix').bytes;
  assert.equal(engine.api.map(0x4000, 1, 3), 0); word(engine, 0x4000, 0xa1b2c3d4);
  if (secondStore) { assert.equal(engine.api.map(0x9000, 1, 3), 0); word(engine, 0x9000, 0x11223344); assert.equal(engine.api.protect(0x9000, 1, 1), 0); }
  const child = compile(engine, bytes), start = state(); start.registers[6] = 0x9000;
  const access = secondStore ? 2 : 1, detail = secondStore ? 2 : 1;
  writeState(engine, start);
  run(engine, child, 0, start, exit(1, 0), 'NOT zero budget before either access', undefined, 'initial');
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 0, start, exit(2, 0), 'NOT cancel outranks zero budget', undefined, 'resume');
  engine.view.setUint32(engine.base + 96, 0, true);
  const moved = copyState(start); moved.registers[2] = 0xffffffff; moved.eip = 0x1005;
  run(engine, child, 1, moved, exit(1, 1), 'NOT one budget retires prefix MOV only', undefined, 'resume');
  const stopped = copyState(start); stopped.registers[2] = 0; stopped.eflags = 0x457; stopped.eip = 0x1008;
  run(engine, child, 1, stopped, exit(1, 1), 'NOT one budget retires prefix ADD only', undefined, 'resume');
  run(engine, child, 0, stopped, exit(1, 0), 'zero budget at NOT preserves prefix flags and both-access boundary', undefined, 'resume');
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 20, stopped, exit(2, 0), 'cancel at NOT precedes either faulting access', undefined, 'resume');
  engine.view.setUint32(engine.base + 96, 0, true);
  run(engine, child, 20, stopped, exit(5, 0, detail, 0x9000, access), `actual continued NOT ${secondStore ? 'second store' : 'first read'} fault`, helperRecord(1, 0, detail, 0x9000, access), 'resume');
  run(engine, child, 1, stopped, exit(5, 0, detail, 0x9000, access), 'unrepaired NOT retry does not repeat prefix or mutate RAM', helperRecord(1, 0, detail, 0x9000, access), 'resume');
  if (secondStore) { assert.equal(sampleRam(engine, [0x9000])[0], 0x11223344); assert.equal(engine.api.protect(0x9000, 1, 3), 0); }
  else assert.equal(engine.api.map(0x9000, 1, 3), 0);
  word(engine, 0x9000, 0x89abcdef);
  const repaired = nextNot(stopped, assembled.get('not').bytes.length);
  run(engine, child, 1, repaired, exit(1, 1), 'repair resumes NOT and re-reads current RAM before complement/store', helperRecord(0), 'resume');
  assert.equal(sampleRam(engine, [0x9000])[0], 0x76543210, 'retry uses repaired current RAM, not captured value from failed store');
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 20, repaired, exit(2, 0), 'cancel between completed NOT and successor', undefined, 'resume');
  engine.view.setUint32(engine.base + 96, 0, true);
  const next = copyState(repaired); next.registers[1] = 0x99999999; next.eip = 0x1000 + bytes.length;
  run(engine, child, 1, next, exit(1, 1), 'interior successor does not repeat completed NOT', undefined, 'resume');
  run(engine, child, 1, next, exit(3, 0), 'unknown continuation retires no extra operation', undefined, 'resume');
  assert.deepEqual(sampleRam(engine, [0x9000, 0x4000]), [0x76543210, 0xa1b2c3d4]);
  guarded(engine, child);
}

{
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0); word(engine, 0x4000, 0x89abcdef);
  const child = compile(engine, assembled.get('not').bytes), start = state(); start.registers[6] = 0x4000;
  writeState(engine, start);
  assert.equal(engine.api.protect(0x1000, 1, 5), 0);
  reject(engine, child, 4, 'stale NOT guard precedes invalid pointers and either guest access');
  assert.equal(engine.api.close(), 0);
  reject(engine, child, 5, 'closed NOT guard precedes invalid pointers and either guest access');
}

for (const immediate of [true, false]) {
  const engine = fresh(), bytes = assembled.get('not_smc').bytes;
  if (!immediate) word(engine, 0x1ff0, 0x12345678);
  const child = compile(engine, bytes), start = state(); start.registers[6] = immediate ? 0x1003 : 0x1ff0;
  const original = immediate ? 0xbbbbbbbb : 0x12345678, changed = complement(original);
  const sampled = sampleRam(engine, [0x1000]), committed = nextNot(start, assembled.get('not').bytes.length);
  writeState(engine, start);
  run(engine, child, 1, committed, exit(6, 1), `NOT ${immediate ? 'successor immediate' : 'code-page data word'} commits before invalidation/successor/budget`, helperRecord(0), 'initial');
  assert.equal(sampleRam(engine, [start.registers[6]])[0], changed);
  if (!immediate) assert.deepEqual(sampleRam(engine, [0x1000]), sampled, 'page-granular stamp changes even when instruction bytes are elsewhere');
  reject(engine, child, 4, 'old NOT module rejects after its successful code word write');
  const replacement = compile(engine, new Uint8Array(), [[0x1000, bytes.length]]);
  const next = copyState(committed); next.registers[1] = immediate ? changed : 0xbbbbbbbb; next.eip = 0x1000 + bytes.length;
  run(engine, replacement, 1, next, exit(1, 1), 'recompiled interior MOV uses current code and skips completed NOT without host CPU writes', undefined, 'resume');
  run(engine, replacement, 2, next, exit(3, 0), 'recompiled unknown continuation repeats neither NOT nor successor', undefined, 'resume');
  assert.equal(sampleRam(engine, [start.registers[6]])[0], changed, 'recompiled continuation does not restore the complemented word');
  guarded(engine, replacement); counts.code_writes++;
}

const arrays = [[0], [0xffffffff], [0, 1, 0x7fffffff, 0x80000000, 0x12345678, 0xaaaaaaaa, 0x55555555, 0xff00ff00], [0x80000001, 0xffffff80, 0x00000080, 0xffff0000, 0x00ffffff]];
let invertedWords = 0, involutionRetired = 0;
for (const values of arrays) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0);
  values.forEach((value, index) => { word(engine, 0x4000 + index * 4, value); });
  word(engine, 0x4000 + values.length * 4, 0x11223344); word(engine, 0x4ffc, 0x55667788);
  const names = ['invert_loop', 'invert_finish', 'reset_loop', 'restore_loop', 'restore_finish'];
  const blocks = names.map(name => { const fixture = assembled.get(name); upload(engine, 0x1000 + fixture.offset, fixture.bytes); return [0x1000 + fixture.offset, fixture.bytes.length]; });
  const child = compile(engine, new Uint8Array(), blocks), start = state(0x1600);
  start.registers[1] = values.length; start.registers[5] = values.length; start.registers[6] = 0x4000; start.registers[7] = 0x4000;
  const afterFirst = copyState(start); afterFirst.registers[1] = 0; afterFirst.registers[6] += values.length * 4; afterFirst.eip = 0x1000 + assembled.get('invert_finish').offset; afterFirst.eflags = 0x446;
  const sentinels = [0x4000 + values.length * 4, 0x4ffc, 0x1600], sampled = sampleRam(engine, sentinels);
  const addresses = values.map((_, index) => 0x4000 + index * 4), inverse = values.map(complement);
  writeState(engine, start);
  run(engine, child, values.length * 4, afterFirst, exit(1, values.length * 4), `resident array inversion/${values.length}`, helperRecord(0), 'initial');
  assert.deepEqual(sampleRam(engine, addresses), inverse, 'first authored pass matches independent BigInt complement for every word');
  assert.deepEqual(sampleRam(engine, sentinels), sampled);
  const restored = copyState(afterFirst); restored.eip = 0x1700;
  const remaining = values.length * 4 + 5;
  run(engine, child, remaining + 1, restored, exit(3, remaining), `resident involution resumes through guest reset and second loop/${values.length}`, helperRecord(0), 'resume');
  assert.deepEqual(sampleRam(engine, addresses), values, 'second authored pass restores every original word');
  assert.deepEqual(sampleRam(engine, sentinels), sampled);
  guarded(engine, child); counts.involutions++; invertedWords += values.length; involutionRetired += values.length * 8 + 5;
}
assert.equal(actualRuns, counts.canonical_exits + counts.guard_rejections, 'actual generated calls equal canonical exits plus guard rejections');
assert.equal(counts.continuation_initial_calls, 9); assert.equal(counts.continuation_resume_calls, 31);
const provenance = {
  counts, generated_modules: modules, actual_generated_run_calls: actualRuns, snapshot_guard_calls: snapshotGuards, ram_words_observed: ramWords,
  inversion: {array_cases: arrays.length, words_per_pass: invertedWords, retired: involutionRetired},
  engine_sha256: hash(engineBytes), generated_set_sha256: hash(generatedHashes.join('\n')), generated_module_sha256: generatedHashes,
  sources: Object.fromEntries(['integer.S', 'run.mjs'].map(name => [name, hash(readFileSync(join(fixtureRoot, name)))]).concat([['dbt_memory_not_wasm.rs', hash(readFileSync(join(root, 'engine/tests/dbt_memory_not_wasm.rs')))]])),
  artifacts: {object_sha256: hash(object)},
  tools: {node: process.version, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).trim(), llvm_objdump: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], assemble: ['clang', ...assemble], disassemble: ['xcrun', ...disassemble]},
  llvm_fixtures: [...assembled].map(([name, {offset, bytes}]) => ({name, offset, hex: bytes.toString('hex')})),
  claim: 'actual engine embedded flat32 NOT m32 F7/2 through existing checked Read4 then Store4: complement with literal/BigInt oracle; all GPRs and all128 allowed flags preserved; full4236 arena; old-GPR EA and precise first-read/second-store faults; atomic cross-page Store4 rejection; actual prefix/budget/cancel/current-RAM retry and interior/unknown continuations without host CPU patches; real code-word NOT commits once before invalidation and recompiled continuation skips it; resident array inversion/involution. NOT changes every destination byte. No hardware RMW fault, LOCK concurrency, browser, game or performance claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, generated_module_sha256: generatedHashes.length, llvm_fixtures: provenance.llvm_fixtures.length, artifacts: outputDir}));
