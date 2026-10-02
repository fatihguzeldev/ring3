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
const counts = {values_flags: 0, aliases: 0, addresses: 0, entry: 0, read_faults: 0, store_faults: 0, same_byte_permission_faults: 0, continuation_initial_calls: 0, continuation_resume_calls: 0, canonical_exits: 0, guard_rejections: 0, code_writes: 0, same_byte_code_writes: 0, counters: 0};
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
    assert.equal(engine.api.compile(blocks.length), 0, 'actual explicit memory unary preparation');
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

const fixtureRoot = join(root, 'engine/tests/fixtures/p2-memory-unary');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassemble = ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath];
writeFileSync(join(outputDir, 'integer.disassembly.txt'), execFileSync('xcrun', disassemble));
const object = readFileSync(objectPath), assembled = elfFixtures(object), ops = ['inc', 'dec', 'neg'];
const registers = ['eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi'];
const encodings = {inc: 'ff06', dec: 'ff0e', neg: 'f71e'};
for (const op of ops) {
  assert.equal(assembled.get(op).bytes.toString('hex'), encodings[op], `LLVM ${op} m32`);
  assert.equal(assembled.get(`${op}_entry`).bytes.toString('hex'), `${encodings[op]}eb00`, 'LLVM entry terminating short JMP');
  assert.equal(assembled.get(`${op}_prefix`).bytes.toString('hex'), `baffffffff83c201${encodings[op]}b999999999`, 'LLVM arithmetic prefix/unary/successor');
  assert.equal(assembled.get(`${op}_smc`).bytes.toString('hex'), `${encodings[op]}b9bbbbbbbb`, 'LLVM unary/code-word/successor');
}
assert.equal(assembled.get('counter_loop').bytes.toString('hex'), 'ff0e75fc', 'LLVM DEC memory/JNZ loop');

function arithmetic(op, old, oldFlags) {
  const a = BigInt(old), raw = op === 'inc' ? a + 1n : op === 'dec' ? a - 1n : -a;
  const value = Number(BigInt.asUintN(32, raw));
  let flags = (oldFlags & (op === 'neg' ? 0x400 : 0x401)) | 2, parity = 0;
  for (let byte = value & 255; byte !== 0; byte >>>= 1) parity += byte & 1;
  if (parity % 2 === 0) flags |= 4;
  if (value === 0) flags |= 0x40;
  if (value >= 0x80000000) flags |= 0x80;
  const nibble = old & 15;
  if (op === 'inc' ? nibble === 15 : op === 'dec' ? nibble === 0 : nibble !== 0) flags |= 0x10;
  if (op === 'neg' && old !== 0) flags |= 1;
  const signed = BigInt.asIntN(32, a), signedResult = op === 'inc' ? signed + 1n : op === 'dec' ? signed - 1n : -signed;
  if (signedResult < -2147483648n || signedResult > 2147483647n) flags |= 0x800;
  return {value, flags};
}

function nextUnary(start, op, value, length) {
  const expected = copyState(start); expected.eip += length; expected.eflags = arithmetic(op, value, start.eflags).flags; return expected;
}

function guarded(engine, child) {
  const before = arena(engine);
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'data unary or rejected two-access instruction preserves code snapshot');
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

const literals = [
  ['inc', 0, 1, 0x2], ['inc', 0xffffffff, 0, 0x56], ['inc', 0x7fffffff, 0x80000000, 0x896], ['inc', 0xf, 0x10, 0x12],
  ['dec', 0, 0xffffffff, 0x96], ['dec', 1, 0, 0x46], ['dec', 0x80000000, 0x7fffffff, 0x816], ['dec', 0x10, 0xf, 0x16],
  ['neg', 0, 0, 0x46], ['neg', 1, 0xffffffff, 0x97], ['neg', 0x80000000, 0x80000000, 0x887], ['neg', 0xffffffff, 1, 0x13], ['neg', 0x10, 0xfffffff0, 0x87],
];
for (const [op, old, value, flags] of literals) assert.deepEqual(arithmetic(op, old, 2), {value, flags}, 'independent BigInt flags agree with fixed literal result/flag anchors');
const values = [0, 1, 2, 3, 0xf, 0x10, 0xff, 0x7fffffff, 0x80000000, 0xffffffff, 0xffffff80, 0x12345678];
const oldFlagPatterns = [2, 3, 0x402, 0x403, 0xcd7];
for (const op of ops) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0);
  word(engine, 0x4000, 0x11223344); word(engine, 0x4008, 0x55667788); word(engine, 0x4ffc, 0xaabbccdd);
  const bytes = assembled.get(op).bytes, child = compile(engine, bytes);
  for (const value of values) for (const eflags of oldFlagPatterns) {
    word(engine, 0x4004, value);
    const sampled = sampleRam(engine, [0x4000, 0x4008, 0x4ffc, 0x1000]);
    const start = state(0x1000, eflags); start.registers[6] = 0x4004;
    writeState(engine, start);
    run(engine, child, 1, nextUnary(start, op, value, bytes.length), exit(1, 1), `${op} literal/CF/DF flags/${value}/${eflags}`, helperRecord(0));
    assert.equal(sampleRam(engine, [0x4004])[0], arithmetic(op, value, eflags).value);
    assert.deepEqual(sampleRam(engine, [0x4000, 0x4008, 0x4ffc, 0x1000]), sampled, 'only selected word may change; neighbors/page tail/code preserved');
    guarded(engine, child); counts.values_flags++;
  }
}

for (const op of ops) for (const [register, name] of registers.entries()) for (const kind of ['base', 'sib', ...(register === 4 ? [] : ['index'])]) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0); assert.equal(engine.api.map(0xc000, 1, 3), 0);
  const start = state(); start.registers[register] = kind === 'index' ? 2 : 0x4004;
  if (kind === 'index' && register !== 3) start.registers[3] = 0x4000;
  if (kind === 'sib' && register === 4) start.registers[1] = 3;
  const address = kind === 'base' ? 0x4004 : kind === 'sib' ? register === 4 ? 0x400c : 0xc008 : register === 3 ? 6 : 0x4004;
  if (address === 6) assert.equal(engine.api.map(0, 1, 3), 0);
  const value = 0xfedcba98;
  word(engine, address - 4, 0x11223344); word(engine, address, value); word(engine, address + 4, 0x55667788);
  const bytes = assembled.get(`${op}_${kind}_${name}`).bytes, child = compile(engine, bytes);
  assert.equal(bytes[0], op === 'neg' ? 0xf7 : 0xff); assert.equal((bytes[1] >>> 3) & 7, op === 'inc' ? 0 : op === 'dec' ? 1 : 3, 'LLVM exact unary opcode group');
  if (kind === 'index') assert.equal((bytes[2] >>> 3) & 7, register, 'LLVM old index GPR');
  const sampled = sampleRam(engine, [address - 4, address + 4, 0x1000]);
  writeState(engine, start);
  run(engine, child, 1, nextUnary(start, op, value, bytes.length), exit(1, 1), `${op} old ${name}/${kind} EA preserves all GPRs`, helperRecord(0));
  assert.equal(sampleRam(engine, [address])[0], arithmetic(op, value, start.eflags).value);
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
for (const op of ops) for (const fixture of addressForms) {
  const engine = fresh();
  for (const [address, pages] of [[0, 1], [0x4000, 2], [0xfffff000, 1]]) assert.equal(engine.api.map(address, pages, 3), 0);
  const value = 0x90abcdef; word(engine, fixture.address, value);
  const start = state(); Object.entries(fixture.regs).forEach(([reg, data]) => { start.registers[Number(reg)] = data; });
  const bytes = assembled.get(fixture.form ? `${op}_${fixture.form}` : op).bytes, child = compile(engine, bytes);
  const sampled = sampleRam(engine, [0x1000]);
  writeState(engine, start);
  run(engine, child, 1, nextUnary(start, op, value, bytes.length), exit(1, 1), `${op} ${fixture.form ?? 'page boundary'}/${fixture.address}`, helperRecord(0));
  assert.equal(sampleRam(engine, [fixture.address])[0], arithmetic(op, value, start.eflags).value);
  assert.deepEqual(sampleRam(engine, [0x1000]), sampled);
  guarded(engine, child); counts.addresses++;
}

for (const op of ops) {
  const engine = fresh(), bytes = assembled.get(`${op}_entry`).bytes, child = compile(engine, bytes, undefined, true);
  const start = state(); start.registers[6] = 0x9000;
  writeState(engine, start);
  run(engine, child, 1, start, exit(5, 0, 1, 0x9000, 1), `${op} entry prepared with unmapped data faults only at runtime`, helperRecord(1, 0, 1, 0x9000, 1), 'initial');
  assert.equal(engine.api.map(0x9000, 1, 3), 0); word(engine, 0x9000, 0x80000000);
  const repaired = nextUnary(start, op, 0x80000000, assembled.get(op).bytes.length);
  run(engine, child, 1, repaired, exit(1, 1), `${op} entry mapping repair resumes same artifact`, helperRecord(0), 'resume');
  assert.equal(sampleRam(engine, [0x9000])[0], arithmetic(op, 0x80000000, start.eflags).value);
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
const faultCases = ops.flatMap(op => faults.map(fixture => ({op, ...fixture}))).concat([0, 0x80000000].map(value => ({op: 'neg', label: `same-byte NEG/${value} still requires writable destination`, address: 0x4000, detail: 2, fault: 0x4000, access: 2, maps: [[0x4000, 1, 1]], value, sameByte: true})));
for (const fixture of faultCases) {
  const engine = fresh(), words = [];
  for (const [address, pages] of fixture.maps ?? []) { assert.equal(engine.api.map(address, pages, 3), 0); words.push([address === 0x4000 ? 0x4ffc : address, 0x11223344]); }
  if (fixture.address === 0x4000) words.push([0x4000, fixture.value ?? 0xa1b2c3d4]);
  for (const [address, value] of words) word(engine, address, value);
  for (const [address, pages, permissions] of fixture.maps ?? []) assert.equal(engine.api.protect(address, pages, permissions), 0);
  const sampledAddresses = fixture.observe ?? words.map(([address]) => address);
  const bytes = assembled.get(`${fixture.op}_prefix`).bytes, child = compile(engine, bytes), sampled = sampleRam(engine, sampledAddresses.concat([0x1000]));
  const start = state(); start.registers[6] = fixture.address;
  const stopped = copyState(start); stopped.registers[2] = 0; stopped.eflags = 0x457; stopped.eip = 0x1008;
  writeState(engine, start);
  run(engine, child, 20, stopped, exit(5, 2, fixture.detail, fixture.fault, fixture.access), `prefix/${fixture.op}/${fixture.label} preserves prefix flags on either failure`, helperRecord(1, 0, fixture.detail, fixture.fault, fixture.access));
  assert.deepEqual(sampleRam(engine, sampledAddresses.concat([0x1000])), sampled, 'failed read or Store4 preserves every readable destination word and code');
  guarded(engine, child); if (fixture.access === 1) counts.read_faults++; else counts.store_faults++;
  if (fixture.sameByte) counts.same_byte_permission_faults++;
}

for (const op of ops) for (const secondStore of [false, true]) {
  const engine = fresh(), bytes = assembled.get(`${op}_prefix`).bytes;
  assert.equal(engine.api.map(0x4000, 1, 3), 0); word(engine, 0x4000, 0xa1b2c3d4);
  if (secondStore) { assert.equal(engine.api.map(0x9000, 1, 3), 0); word(engine, 0x9000, 0x11223344); assert.equal(engine.api.protect(0x9000, 1, 1), 0); }
  const child = compile(engine, bytes), start = state(); start.registers[6] = 0x9000;
  const access = secondStore ? 2 : 1, detail = secondStore ? 2 : 1;
  writeState(engine, start);
  run(engine, child, 0, start, exit(1, 0), `${op} zero budget before either access`, undefined, 'initial');
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 0, start, exit(2, 0), `${op} cancel outranks zero budget`, undefined, 'resume');
  engine.view.setUint32(engine.base + 96, 0, true);
  const moved = copyState(start); moved.registers[2] = 0xffffffff; moved.eip = 0x1005;
  run(engine, child, 1, moved, exit(1, 1), `${op} one budget retires prefix MOV only`, undefined, 'resume');
  const stopped = copyState(start); stopped.registers[2] = 0; stopped.eflags = 0x457; stopped.eip = 0x1008;
  run(engine, child, 1, stopped, exit(1, 1), `${op} one budget retires prefix ADD only`, undefined, 'resume');
  run(engine, child, 0, stopped, exit(1, 0), `${op} zero budget at unary preserves prefix flags and both-access boundary`, undefined, 'resume');
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 20, stopped, exit(2, 0), `${op} cancel at unary precedes either faulting access`, undefined, 'resume');
  engine.view.setUint32(engine.base + 96, 0, true);
  run(engine, child, 20, stopped, exit(5, 0, detail, 0x9000, access), `actual continued ${op} ${secondStore ? 'second store' : 'first read'} fault keeps old flags`, helperRecord(1, 0, detail, 0x9000, access), 'resume');
  run(engine, child, 1, stopped, exit(5, 0, detail, 0x9000, access), `${op} unrepaired retry repeats neither prefix nor flags commit`, helperRecord(1, 0, detail, 0x9000, access), 'resume');
  if (secondStore) { assert.equal(sampleRam(engine, [0x9000])[0], 0x11223344); assert.equal(engine.api.protect(0x9000, 1, 3), 0); }
  else assert.equal(engine.api.map(0x9000, 1, 3), 0);
  word(engine, 0x9000, 0x89abcdef);
  const repaired = nextUnary(stopped, op, 0x89abcdef, assembled.get(op).bytes.length);
  run(engine, child, 1, repaired, exit(1, 1), `${op} repaired retry re-reads current RAM before store/flags`, helperRecord(0), 'resume');
  const changed = arithmetic(op, 0x89abcdef, stopped.eflags).value;
  assert.equal(sampleRam(engine, [0x9000])[0], changed, 'retry uses current RAM, not captured value from failed store');
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 20, repaired, exit(2, 0), `${op} cancel between completed unary and successor`, undefined, 'resume');
  engine.view.setUint32(engine.base + 96, 0, true);
  const next = copyState(repaired); next.registers[1] = 0x99999999; next.eip = 0x1000 + bytes.length;
  run(engine, child, 1, next, exit(1, 1), `${op} interior successor does not repeat completed unary`, undefined, 'resume');
  run(engine, child, 1, next, exit(3, 0), `${op} unknown continuation retires no extra operation`, undefined, 'resume');
  assert.deepEqual(sampleRam(engine, [0x9000, 0x4000]), [changed, 0xa1b2c3d4]);
  guarded(engine, child);
}

for (const op of ops) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0); word(engine, 0x4000, 0x89abcdef);
  const child = compile(engine, assembled.get(op).bytes), start = state(); start.registers[6] = 0x4000;
  writeState(engine, start);
  assert.equal(engine.api.protect(0x1000, 1, 5), 0);
  reject(engine, child, 4, `stale ${op} guard precedes invalid pointers and either guest access`);
  assert.equal(engine.api.close(), 0);
  reject(engine, child, 5, `closed ${op} guard precedes invalid pointers and either guest access`);
}

const smcCases = ops.flatMap(op => [true, false].map(immediate => ({op, immediate, value: immediate ? 0xbbbbbbbb : 0x12345678}))).concat([0, 0x80000000].map(value => ({op: 'neg', immediate: false, value, sameByte: true})));
for (const {op, immediate, value, sameByte} of smcCases) {
  const engine = fresh(), bytes = assembled.get(`${op}_smc`).bytes;
  if (!immediate) word(engine, 0x1ff0, value);
  const child = compile(engine, bytes), start = state(); start.registers[6] = immediate ? 0x1003 : 0x1ff0;
  const changed = arithmetic(op, value, start.eflags).value, sampled = sampleRam(engine, [0x1000, 0x1007]);
  const committed = nextUnary(start, op, value, assembled.get(op).bytes.length);
  writeState(engine, start);
  run(engine, child, 1, committed, exit(6, 1), `${op} ${sameByte ? 'same-byte ' : ''}${immediate ? 'successor immediate' : 'code-page word'} commits flags/RAM/retirement before invalidation/successor/budget`, helperRecord(0), 'initial');
  assert.equal(sampleRam(engine, [start.registers[6]])[0], changed);
  const after = sampleRam(engine, [0x1000, 0x1007]);
  assert.equal(after[0] & 0xffffff, sampled[0] & 0xffffff, 'old unary/MOV opcode bytes stay unchanged'); assert.equal(after[1], sampled[1], 'bytes following rewritten immediate stay unchanged');
  if (!immediate) assert.deepEqual(after, sampled, 'page-granular code stamp changes while instruction bytes remain identical');
  if (sameByte) { assert.equal(changed, value, 'NEG fixed-point output equals original RAM'); counts.same_byte_code_writes++; }
  reject(engine, child, 4, `old ${op} module rejects after committed code Store4`);
  const replacement = compile(engine, new Uint8Array(), [[0x1000, bytes.length]]);
  const next = copyState(committed); next.registers[1] = immediate ? changed : 0xbbbbbbbb; next.eip = 0x1000 + bytes.length;
  run(engine, replacement, 1, next, exit(1, 1), `${op} recompiled interior MOV skips completed memory/flags operation without host CPU writes`, undefined, 'resume');
  run(engine, replacement, 2, next, exit(3, 0), `${op} recompiled unknown continuation repeats neither operation`, undefined, 'resume');
  assert.equal(sampleRam(engine, [start.registers[6]])[0], changed, 'recompiled continuation keeps once-committed destination RAM');
  guarded(engine, replacement); counts.code_writes++;
}

let counterRetired = 0, takenBranches = 0, completedBranches = 0;
const counterValues = [1, 2, 17, 256];
for (const value of counterValues) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0);
  word(engine, 0x4000, 0x11223344); word(engine, 0x4004, value); word(engine, 0x4008, 0x55667788); word(engine, 0x4ffc, 0xaabbccdd);
  const names = ['counter_loop', 'counter_finish'];
  const blocks = names.map(name => { const fixture = assembled.get(name); upload(engine, 0x1000 + fixture.offset, fixture.bytes); return [0x1000 + fixture.offset, fixture.bytes.length]; });
  const child = compile(engine, new Uint8Array(), blocks), start = state(0x1600); start.registers[6] = 0x4004;
  const sampled = sampleRam(engine, [0x4000, 0x4008, 0x4ffc, 0x1600]);
  writeState(engine, start);
  const afterFirst = nextUnary(start, 'dec', value, assembled.get('dec').bytes.length);
  run(engine, child, 1, afterFirst, exit(1, 1), `resident DEC counter/${value} pauses with new memory flags before JNZ`, helperRecord(0), 'initial');
  assert.equal(sampleRam(engine, [0x4004])[0], Number(BigInt(value) - 1n));
  const complete = copyState(start); complete.eip = 0x1700; complete.eflags = 0x447;
  run(engine, child, value * 2 + 1, complete, exit(3, value * 2), `resident DEC/JNZ counter/${value} consumes memory flags until zero`, helperRecord(0), 'resume');
  assert.equal(sampleRam(engine, [0x4004])[0], 0, 'independent positive integer countdown finishes at zero');
  assert.deepEqual(sampleRam(engine, [0x4000, 0x4008, 0x4ffc, 0x1600]), sampled, 'counter changes only selected word');
  guarded(engine, child); counts.counters++; counterRetired += value * 2 + 1; takenBranches += value - 1; completedBranches++;
}
assert.ok(takenBranches > 0 && completedBranches > 0, 'DEC memory flags exercise taken and completed JNZ branches');
assert.equal(actualRuns, counts.canonical_exits + counts.guard_rejections, 'actual generated calls equal canonical exits plus guard rejections');
assert.equal(counts.continuation_initial_calls, 21); assert.equal(counts.continuation_resume_calls, 89);
const provenance = {
  counts, generated_modules: modules, actual_generated_run_calls: actualRuns, snapshot_guard_calls: snapshotGuards, ram_words_observed: ramWords,
  counter: {cases: counterValues.length, taken_branches: takenBranches, completed_branches: completedBranches, retired: counterRetired},
  engine_sha256: hash(engineBytes), generated_set_sha256: hash(generatedHashes.join('\n')), generated_module_sha256: generatedHashes,
  sources: Object.fromEntries(['integer.S', 'run.mjs'].map(name => [name, hash(readFileSync(join(fixtureRoot, name)))]).concat([['dbt_memory_unary_wasm.rs', hash(readFileSync(join(root, 'engine/tests/dbt_memory_unary_wasm.rs')))]])),
  artifacts: {object_sha256: hash(object)},
  tools: {node: process.version, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).trim(), llvm_objdump: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], assemble: ['clang', ...assemble], disassemble: ['xcrun', ...disassemble]},
  llvm_fixtures: [...assembled].map(([name, {offset, bytes}]) => ({name, offset, hex: bytes.toString('hex')})),
  claim: 'actual engine embedded flat32 INC m32 FF/0, DEC m32 FF/1 and NEG m32 F7/3 through validated Read4/Store4 then flags/retirement commit; literal and independent BigInt flags including INC/DEC CF/DF and NEG zero/minimum; unchanged GPRs/full4236 arena; precise old-EA/read/store/atomic-cross-page faults; prefix/budget/cancel/current-RAM repair/interior/unknown/lifecycle continuations without host CPU patches; real code writes and NEG fixed-point same-byte Store4 invalidation retire flags/RAM once before recompiled successor; resident DEC memory/JNZ counter. No writing binary memory/general ISA/hardware RMW fault/LOCK concurrency/browser/game/performance claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, generated_module_sha256: generatedHashes.length, llvm_fixtures: provenance.llvm_fixtures.length, artifacts: outputDir}));
