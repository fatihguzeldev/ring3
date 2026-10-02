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
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'compile_with_gates', 'compile_entries', 'store32'];
let keys = 0n, modules = 0, actualRuns = 0, ramWords = 0, snapshotGuards = 0;
const counts = {values_flags: 0, aliases: 0, addresses: 0, entry: 0, read_faults: 0, store_faults: 0, explicit_same_byte_permission_faults: 0, continuation_initial_calls: 0, continuation_resume_calls: 0, canonical_exits: 0, guard_rejections: 0, code_writes: 0, explicit_same_byte_code_writes: 0, arrays: 0};
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
  assert.equal(api.map(0x1000, 2, 7), 0);
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
    assert.equal(engine.api.compile(blocks.length), 0, 'actual explicit writing memory binary preparation');
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

const fixtureRoot = join(root, 'engine/tests/fixtures/p2-memory-write');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassemble = ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath];
writeFileSync(join(outputDir, 'integer.disassembly.txt'), execFileSync('xcrun', disassemble));
const oraclePath = join(outputDir, 'oracle');
const compileOracle = ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', join(fixtureRoot, 'oracle.c'), '-o', oraclePath];
execFileSync('clang', compileOracle, {stdio: ['ignore', 'pipe', 'pipe']});
const object = readFileSync(objectPath), assembled = elfFixtures(object), ops = ['add', 'sub', 'and', 'or', 'xor'];
const registers = ['eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi'];
const opcodes = {add: 0x01, sub: 0x29, and: 0x21, or: 0x09, xor: 0x31};
const groups = {add: 0, sub: 5, and: 4, or: 1, xor: 6};
const forms = ops.flatMap(op => ['reg', 'imm32', 'imm8'].map(kind => ({op, kind, name: `${op}_${kind}`, register: 0, immediate: kind === 'reg' ? undefined : kind === 'imm32' ? 0x80000001 : 0xffffffff})));
const hexByte = value => value.toString(16).padStart(2, '0');
for (const form of forms) {
  const encoded = form.kind === 'reg' ? `${hexByte(opcodes[form.op])}06` : `${form.kind === 'imm32' ? '81' : '83'}${hexByte(groups[form.op] * 8 + 6)}${form.kind === 'imm32' ? '01000080' : 'ff'}`;
  assert.equal(assembled.get(form.name).bytes.toString('hex'), encoded, `LLVM ${form.name} exact m32/r32 or immediate direction`);
  assert.equal(assembled.get(`${form.name}_entry`).bytes.toString('hex'), `${encoded}eb00`, 'LLVM entry terminator');
  assert.equal(assembled.get(`${form.name}_prefix`).bytes.toString('hex'), `baffffffff83c201${encoded}b999999999`, 'LLVM prefix/operation/successor');
  assert.equal(assembled.get(`${form.name}_smc`).bytes.toString('hex'), `${encoded}b9bbbbbbbb`, 'LLVM operation/successor immediate');
}

function arithmetic(op, old, rhs, oldFlags) {
  const a = BigInt(old), b = BigInt(rhs);
  const raw = op === 'add' ? a + b : op === 'sub' ? a - b : op === 'and' ? a & b : op === 'or' ? a | b : a ^ b;
  const value = Number(BigInt.asUintN(32, raw));
  let flags = (oldFlags & 0x400) | 2, parity = 0;
  for (let byte = value & 255; byte !== 0; byte >>>= 1) parity += byte & 1;
  if (parity % 2 === 0) flags |= 4;
  if (value === 0) flags |= 0x40;
  if (value >= 0x80000000) flags |= 0x80;
  if (op === 'add' || op === 'sub') {
    if (op === 'add' ? raw > 0xffffffffn : a < b) flags |= 1;
    if (op === 'add' ? (old & 15) + (rhs & 15) > 15 : (old & 15) < (rhs & 15)) flags |= 0x10;
    const signed = op === 'add' ? BigInt.asIntN(32, a) + BigInt.asIntN(32, b) : BigInt.asIntN(32, a) - BigInt.asIntN(32, b);
    if (signed < -2147483648n || signed > 2147483647n) flags |= 0x800;
  }
  return {value, flags};
}

function result(start, form, old) {
  return arithmetic(form.op, old, form.immediate ?? start.registers[form.register], start.eflags);
}

function nextBinary(start, form, old, length) {
  const expected = copyState(start); expected.eip += length; expected.eflags = result(start, form, old).flags; return expected;
}

function guarded(engine, child) {
  const before = arena(engine);
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'data writes or rejected accesses preserve code snapshot');
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
  ['add', 0, 1, 1, 0x2], ['add', 0xffffffff, 1, 0, 0x57], ['add', 0x7fffffff, 1, 0x80000000, 0x896], ['add', 0x80000000, 0x80000000, 0, 0x847],
  ['sub', 0, 1, 0xffffffff, 0x97], ['sub', 1, 0, 1, 0x2], ['sub', 0x80000000, 1, 0x7fffffff, 0x816], ['sub', 0x7fffffff, 0xffffffff, 0x80000000, 0x887],
  ['and', 0xffffffff, 0, 0, 0x46], ['or', 0x80000000, 0, 0x80000000, 0x86], ['xor', 0x12345678, 0x12345678, 0, 0x46], ['xor', 0, 3, 3, 0x6],
];
for (const [op, old, rhs, value, flags] of literals) assert.deepEqual(arithmetic(op, old, rhs, 2), {value, flags}, 'BigInt math agrees with fixed literal flags and ordered SUB');
const pairs = [[0, 0], [0, 1], [1, 0], [0xffffffff, 1], [0x7fffffff, 1], [0x80000000, 1], [0x80000000, 0x80000000], [0, 0xffffffff], [0x10, 1], [0xf, 1], [0x12345678, 0x87654321], [0xaaaaaaaa, 0x55555555], [0x7fffffff, 0xffffffff]];
const values = [0, 1, 2, 3, 0xf, 0x10, 0x7fffffff, 0x80000000, 0xffffffff, 0x12345678];
const extraImmediates = ops.flatMap(op => [
  ...[['min', 0xffffff80, '80'], ['minus1', 0xffffffff, 'ff'], ['zero', 0, '00'], ['max', 127, '7f']].map(([suffix, immediate, hex]) => ({op, kind: 'imm8', name: `${op}_imm8_${suffix}`, immediate, hex: `83${hexByte(groups[op] * 8 + 6)}${hex}`})),
  ...[['max', 0x7fffffff, 'ffffff7f'], ['literal', 0x12345678, '78563412'], ['minus1', 0xffffffff, 'ffffffff']].map(([suffix, immediate, hex]) => ({op, kind: 'imm32', name: `${op}_imm32_${suffix}`, immediate, hex: `81${hexByte(groups[op] * 8 + 6)}${hex}`})),
]);
for (const form of forms.concat(extraImmediates)) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0);
  word(engine, 0x4000, 0x11223344); word(engine, 0x4008, 0x55667788); word(engine, 0x4ffc, 0xaabbccdd);
  const bytes = assembled.get(form.name).bytes, child = compile(engine, bytes);
  if (form.hex) assert.equal(bytes.toString('hex'), form.hex, 'LLVM literal full32 or sign-extended imm8 form');
  const inputs = form.kind === 'reg' ? pairs : (form.hex ? [0, 1, 0x80000000, 0xffffffff] : values).map(value => [value, undefined]);
  for (const [old, rhs] of inputs) for (const eflags of [2, 0xcd7]) {
    word(engine, 0x4004, old);
    const sampled = sampleRam(engine, [0x4000, 0x4008, 0x4ffc, 0x1000]);
    const start = state(0x1000, eflags); start.registers[6] = 0x4004;
    if (rhs !== undefined) start.registers[0] = rhs;
    writeState(engine, start);
    run(engine, child, 1, nextBinary(start, form, old, bytes.length), exit(1, 1), `${form.name}/${old}/${rhs}/${eflags} complete flags/GPRs`, helperRecord(0));
    assert.equal(sampleRam(engine, [0x4004])[0], result(start, form, old).value);
    assert.deepEqual(sampleRam(engine, [0x4000, 0x4008, 0x4ffc, 0x1000]), sampled, 'only destination changes; neighbors/tail/code preserved');
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
  const old = 0xfedcba98, form = {op, register};
  word(engine, address - 4, 0x11223344); word(engine, address, old); word(engine, address + 4, 0x55667788);
  const bytes = assembled.get(`${op}_${kind}_${name}`).bytes, child = compile(engine, bytes);
  assert.equal(bytes[0], opcodes[op]); assert.equal((bytes[1] >>> 3) & 7, register, 'LLVM source is old aliased GPR');
  if (kind === 'index') assert.equal((bytes[2] >>> 3) & 7, register, 'LLVM old index GPR');
  const sampled = sampleRam(engine, [address - 4, address + 4, 0x1000]);
  writeState(engine, start);
  run(engine, child, 1, nextBinary(start, form, old, bytes.length), exit(1, 1), `${op} old ${name}/${kind} source and EA`, helperRecord(0));
  assert.equal(sampleRam(engine, [address])[0], result(start, form, old).value);
  assert.deepEqual(sampleRam(engine, [address - 4, address + 4, 0x1000]), sampled);
  guarded(engine, child); counts.aliases++;
}

const addressForms = [
  {form: 'absolute', address: 0x4000, regs: {}}, {form: 'no_base', address: 0x401c, regs: {1: 7}},
  {form: 'disp8_negative', address: 0x4000, regs: {3: 0x4080}}, {form: 'disp8_positive', address: 0x407f, regs: {3: 0x4000}},
  {form: 'disp32', address: 0x4345, regs: {3: 0xffff2000}}, {form: 'base_wrap', address: 4, regs: {3: 0xfffffffc}},
  {form: 'index_wrap', address: 0x4004, regs: {3: 0x4004, 1: 0x20000000}}, {form: 'unaligned', address: 0x4003, regs: {3: 0x4000, 1: 2}},
  {form: null, address: 0x4ffc, regs: {6: 0x4ffc}}, {form: null, address: 0x4ffe, regs: {6: 0x4ffe}}, {form: null, address: 0xfffffffc, regs: {6: 0xfffffffc}},
];
for (const form of forms) for (const fixture of addressForms) {
  const engine = fresh();
  for (const [address, pages] of [[0, 1], [0x4000, 2], [0xfffff000, 1]]) assert.equal(engine.api.map(address, pages, 3), 0);
  const old = 0x90abcdef; word(engine, fixture.address, old);
  const start = state(); Object.entries(fixture.regs).forEach(([reg, data]) => { start.registers[Number(reg)] = data; });
  const bytes = assembled.get(fixture.form ? `${form.name}_${fixture.form}` : form.name).bytes, child = compile(engine, bytes);
  const sampled = sampleRam(engine, [0x1000]);
  writeState(engine, start);
  run(engine, child, 1, nextBinary(start, form, old, bytes.length), exit(1, 1), `${form.name} ${fixture.form ?? 'page boundary'}/${fixture.address}`, helperRecord(0));
  assert.equal(sampleRam(engine, [fixture.address])[0], result(start, form, old).value);
  assert.deepEqual(sampleRam(engine, [0x1000]), sampled);
  guarded(engine, child); counts.addresses++;
}

for (const form of forms) {
  const engine = fresh(), bytes = assembled.get(`${form.name}_entry`).bytes, child = compile(engine, bytes, undefined, true);
  const start = state(); start.registers[6] = 0x9000;
  writeState(engine, start);
  run(engine, child, 1, start, exit(5, 0, 1, 0x9000, 1), `${form.name} entry prepares without data reads`, helperRecord(1, 0, 1, 0x9000, 1), 'initial');
  assert.equal(engine.api.map(0x9000, 1, 3), 0); word(engine, 0x9000, 0x80000000);
  run(engine, child, 1, nextBinary(start, form, 0x80000000, assembled.get(form.name).bytes.length), exit(1, 1), `${form.name} entry mapping repair`, helperRecord(0), 'resume');
  assert.equal(sampleRam(engine, [0x9000])[0], result(start, form, 0x80000000).value);
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
  {label: 'cross-page store denied at first byte', address: 0x4ffe, detail: 2, fault: 0x4ffe, access: 2, maps: [[0x4000, 1, 1], [0x5000, 1, 3]]},
];
const faultCases = forms.flatMap(form => faults.filter(fixture => form.kind === 'reg' || [0, 6].includes(faults.indexOf(fixture))).map(fixture => ({form, ...fixture}))).concat(forms.filter(form => form.kind === 'reg').map(form => ({form, ...faults[6], sameByte: true})));
for (const fixture of faultCases) {
  const engine = fresh(), words = [];
  for (const [address, pages] of fixture.maps ?? []) { assert.equal(engine.api.map(address, pages, 3), 0); words.push([address === 0x4000 ? 0x4ffc : address, 0x11223344]); }
  if (fixture.address === 0x4000) words.push([0x4000, 0xa1b2c3d4]);
  for (const [address, value] of words) word(engine, address, value);
  for (const [address, pages, permissions] of fixture.maps ?? []) assert.equal(engine.api.protect(address, pages, permissions), 0);
  const sampledAddresses = fixture.observe ?? words.map(([address]) => address);
  const bytes = assembled.get(`${fixture.form.name}_prefix`).bytes, child = compile(engine, bytes), sampled = sampleRam(engine, sampledAddresses.concat([0x1000]));
  const start = state(); start.registers[6] = fixture.address;
  if (fixture.sameByte) start.registers[0] = fixture.form.op === 'and' ? 0xffffffff : 0;
  const stopped = copyState(start); stopped.registers[2] = 0; stopped.eflags = 0x457; stopped.eip = 0x1008;
  if (fixture.sameByte) assert.equal(result(stopped, fixture.form, 0xa1b2c3d4).value, 0xa1b2c3d4, 'identity operation still requires Store4 permission');
  writeState(engine, start);
  run(engine, child, 20, stopped, exit(5, 2, fixture.detail, fixture.fault, fixture.access), `${fixture.form.name}/${fixture.label} preserves prefix flags`, helperRecord(1, 0, fixture.detail, fixture.fault, fixture.access));
  assert.deepEqual(sampleRam(engine, sampledAddresses.concat([0x1000])), sampled, 'failed access preserves readable RAM/code, including rejected cross-page Store4');
  guarded(engine, child); if (fixture.access === 1) counts.read_faults++; else counts.store_faults++;
  if (fixture.sameByte) counts.explicit_same_byte_permission_faults++;
}

for (const form of forms) for (const secondStore of [false, true]) {
  const engine = fresh(), bytes = assembled.get(`${form.name}_prefix`).bytes;
  assert.equal(engine.api.map(0x4000, 1, 3), 0); word(engine, 0x4000, 0xa1b2c3d4);
  if (secondStore) { assert.equal(engine.api.map(0x9000, 1, 3), 0); word(engine, 0x9000, 0x11223344); assert.equal(engine.api.protect(0x9000, 1, 1), 0); }
  const child = compile(engine, bytes), start = state(); start.registers[6] = 0x9000;
  const access = secondStore ? 2 : 1, detail = secondStore ? 2 : 1;
  writeState(engine, start);
  run(engine, child, 0, start, exit(1, 0), `${form.name} zero budget before accesses`, undefined, 'initial');
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 0, start, exit(2, 0), `${form.name} cancel outranks zero budget`, undefined, 'resume');
  engine.view.setUint32(engine.base + 96, 0, true);
  const moved = copyState(start); moved.registers[2] = 0xffffffff; moved.eip = 0x1005;
  run(engine, child, 1, moved, exit(1, 1), `${form.name} prefix MOV only`, undefined, 'resume');
  const stopped = copyState(start); stopped.registers[2] = 0; stopped.eflags = 0x457; stopped.eip = 0x1008;
  run(engine, child, 1, stopped, exit(1, 1), `${form.name} prefix ADD only`, undefined, 'resume');
  run(engine, child, 0, stopped, exit(1, 0), `${form.name} zero budget at two-access boundary`, undefined, 'resume');
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 20, stopped, exit(2, 0), `${form.name} cancel before access/fault`, undefined, 'resume');
  engine.view.setUint32(engine.base + 96, 0, true);
  run(engine, child, 20, stopped, exit(5, 0, detail, 0x9000, access), `${form.name} continued ${secondStore ? 'Store4' : 'Read4'} fault`, helperRecord(1, 0, detail, 0x9000, access), 'resume');
  run(engine, child, 1, stopped, exit(5, 0, detail, 0x9000, access), `${form.name} unrepaired retry`, helperRecord(1, 0, detail, 0x9000, access), 'resume');
  if (secondStore) { assert.equal(sampleRam(engine, [0x9000])[0], 0x11223344); assert.equal(engine.api.protect(0x9000, 1, 3), 0); }
  else assert.equal(engine.api.map(0x9000, 1, 3), 0);
  word(engine, 0x9000, 0x89abcdef);
  const repaired = nextBinary(stopped, form, 0x89abcdef, assembled.get(form.name).bytes.length);
  run(engine, child, 1, repaired, exit(1, 1), `${form.name} repaired retry reads current RAM`, helperRecord(0), 'resume');
  const changed = result(stopped, form, 0x89abcdef).value;
  assert.equal(sampleRam(engine, [0x9000])[0], changed, 'current RAM determines retry result/flags');
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 20, repaired, exit(2, 0), `${form.name} cancel before successor`, undefined, 'resume');
  engine.view.setUint32(engine.base + 96, 0, true);
  const next = copyState(repaired); next.registers[1] = 0x99999999; next.eip = 0x1000 + bytes.length;
  run(engine, child, 1, next, exit(1, 1), `${form.name} interior successor skips completed store`, undefined, 'resume');
  run(engine, child, 1, next, exit(3, 0), `${form.name} unknown continuation`, undefined, 'resume');
  assert.deepEqual(sampleRam(engine, [0x9000, 0x4000]), [changed, 0xa1b2c3d4]); guarded(engine, child);
}

for (const form of forms) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0); word(engine, 0x4000, 0x89abcdef);
  const child = compile(engine, assembled.get(form.name).bytes), start = state(); start.registers[6] = 0x4000;
  writeState(engine, start); assert.equal(engine.api.protect(0x1000, 1, 5), 0);
  reject(engine, child, 4, `stale ${form.name} guard precedes invalid pointers/accesses`);
  assert.equal(engine.api.close(), 0); reject(engine, child, 5, `closed ${form.name} guard precedes invalid pointers/accesses`);
}

const smcCases = forms.map(form => ({form, immediate: true})).concat(forms.filter(form => form.kind === 'reg').map(form => ({form, immediate: false, sameByte: true})));
for (const {form, immediate, sameByte} of smcCases) {
  const engine = fresh(), bytes = assembled.get(`${form.name}_smc`).bytes, length = assembled.get(form.name).bytes.length;
  const old = immediate ? 0xbbbbbbbb : 0x12345678, address = immediate ? 0x1000 + length + 1 : 0x1ff0;
  if (!immediate) word(engine, address, old);
  const child = compile(engine, bytes), start = state(); start.registers[6] = address;
  if (sameByte) start.registers[0] = form.op === 'and' ? 0xffffffff : 0;
  const changed = result(start, form, old).value, sampled = sampleRam(engine, [0x1000, address + 4]);
  const committed = nextBinary(start, form, old, length);
  writeState(engine, start);
  run(engine, child, 1, committed, exit(6, 1), `${form.name} ${sameByte ? 'same-byte' : 'rewritten successor'} commits RAM/flags before invalidation/budget`, helperRecord(0), 'initial');
  assert.equal(sampleRam(engine, [address])[0], changed);
  const after = sampleRam(engine, [0x1000, address + 4]);
  const mask = length + 1 >= 4 ? 0xffffffff : 0xffffff;
  assert.equal(after[0] & mask, sampled[0] & mask, 'operation and successor opcode bytes preserved'); assert.equal(after[1], sampled[1], 'bytes following destination preserved');
  if (sameByte) { assert.equal(changed, old); assert.deepEqual(after, sampled); counts.explicit_same_byte_code_writes++; }
  reject(engine, child, 4, `old ${form.name} artifact rejects after code Store4`);
  const replacement = compile(engine, new Uint8Array(), [[0x1000, bytes.length]]);
  const next = copyState(committed); next.registers[1] = immediate ? changed : 0xbbbbbbbb; next.eip = 0x1000 + bytes.length;
  run(engine, replacement, 1, next, exit(1, 1), `${form.name} recompiled interior successor skips completed operation without host CPU writes`, undefined, 'resume');
  run(engine, replacement, 2, next, exit(3, 0), `${form.name} recompiled unknown continuation`, undefined, 'resume');
  assert.equal(sampleRam(engine, [address])[0], changed); guarded(engine, replacement); counts.code_writes++;
}

let arrayRetired = 0, zeroBranches = 0, nonzeroBranches = 0;
const arrayCases = [
  {parameters: [0, 0, 0xffffffff, 0, 0], values: [0]},
  {parameters: [1, 1, 0xffffffff, 0, 0], values: [1, 0xffffffff]},
  {parameters: [0xffffffff, 1, 0xffffffff, 0, 0], values: [2, 0, 1, 0x80000000]},
  {parameters: [0x7fffffff, 0x80000000, 0xffff, 0x80, 0x80], values: [1, 0x12345678, 0xffffffff, 0x80000000, 0, 0x7fffffff, 0x10001, 0x10000]},
  {parameters: [0x12345678, 0x87654321, 0x55555555, 0x80000000, 0x80000000], values: [0, 0xffffffff, 0xaaaaaaaa]},
];
const invalidOracleArguments = [[], ['0', '0', '0', '0', '0'], ['0', '0', '0', '0', '0', ''], ['0', '0', '0', '0', '0', '4294967296'], ['0', '0', '0', '0', '0', '-1'], ['0', '0', '0', '0', '0', ' 1'], Array(14).fill('0')];
for (const args of invalidOracleArguments) { const rejected = spawnSync(oraclePath, args, {encoding: 'utf8'}); assert.equal(rejected.status, 2); assert.equal(rejected.stdout, ''); }
for (const {parameters, values} of arrayCases) {
  const native = JSON.parse(execFileSync(oraclePath, parameters.concat(values).map(String), {encoding: 'utf8'}));
  const independent = values.map(old => Number(BigInt.asUintN(32, ((BigInt.asUintN(32, BigInt(old) + BigInt(parameters[0]) - BigInt(parameters[1])) & BigInt(parameters[2])) | BigInt(parameters[3])) ^ BigInt(parameters[4]))));
  assert.deepEqual(native.values, independent, 'independent unsigned C and BigInt array outputs');
  assert.deepEqual(native.markers, independent.map(value => value === 0 ? 1 : 2));
  assert.equal(native.zero, independent.filter(value => value === 0).length); assert.equal(native.nonzero, values.length - native.zero);
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0);
  values.forEach((value, index) => { word(engine, 0x4000 + index * 8, value); word(engine, 0x4004 + index * 8, 0); });
  const sentinel = 0x4000 + values.length * 8; word(engine, sentinel, 0x11223344); word(engine, 0x4ffc, 0x55667788);
  const blocks = ['array_loop', 'array_nonzero', 'array_zero', 'array_joined', 'array_finish'].map(name => { const fixture = assembled.get(name); upload(engine, 0x1000 + fixture.offset, fixture.bytes); return [0x1000 + fixture.offset, fixture.bytes.length]; });
  const child = compile(engine, new Uint8Array(), blocks), start = state(0x2000);
  [0, 3, 2, 5, 7].forEach((reg, index) => { start.registers[reg] = parameters[index]; }); start.registers[1] = values.length; start.registers[6] = 0x4000;
  const sampled = sampleRam(engine, [sentinel, 0x4ffc, 0x2000]);
  let partial = values[0]; for (let index = 0; index < 4; index++) partial = arithmetic(ops[index], partial, parameters[index], start.eflags).value;
  const first = copyState(start); first.eflags = arithmetic('xor', partial, parameters[4], start.eflags).flags;
  first.eip = 0x1000 + assembled.get(independent[0] === 0 ? 'array_zero' : 'array_nonzero').offset;
  writeState(engine, start);
  run(engine, child, 6, first, exit(1, 6), 'resident array pauses after five writing kinds and memory-flag JZ', helperRecord(0), 'initial');
  assert.deepEqual(sampleRam(engine, values.flatMap((_, index) => [0x4000 + index * 8, 0x4004 + index * 8])), values.flatMap((value, index) => [index === 0 ? independent[0] : value, 0]));
  const complete = copyState(start); complete.registers[1] = 0; complete.registers[6] = sentinel; complete.eip = 0x2100; complete.eflags = 0x446;
  const retired = values.length * 11 + 1;
  run(engine, child, retired - 5, complete, exit(3, retired - 6), 'resident array consumes flags/branches through finish without intermediate host CPU writes', helperRecord(0), 'resume');
  assert.deepEqual(sampleRam(engine, values.flatMap((_, index) => [0x4000 + index * 8, 0x4004 + index * 8])), native.values.flatMap((value, index) => [value, native.markers[index]]));
  assert.deepEqual(sampleRam(engine, [sentinel, 0x4ffc, 0x2000]), sampled);
  guarded(engine, child); counts.arrays++; arrayRetired += retired; zeroBranches += native.zero; nonzeroBranches += native.nonzero;
}
assert.ok(zeroBranches > 0 && nonzeroBranches > 0, 'resident JZ takes both branches using writing memory flags');
assert.equal(actualRuns, counts.canonical_exits + counts.guard_rejections, 'generated calls equal canonical exits plus guard rejects');
assert.equal(counts.continuation_initial_calls, forms.length + forms.length * 2 + smcCases.length + arrayCases.length);
assert.equal(counts.continuation_resume_calls, forms.length + forms.length * 2 * 11 + smcCases.length * 2 + arrayCases.length);
const provenance = {
  counts, generated_modules: modules, actual_generated_run_calls: actualRuns, snapshot_guard_calls: snapshotGuards, ram_words_observed: ramWords,
  array: {cases: arrayCases.length, rows: zeroBranches + nonzeroBranches, zero_branches: zeroBranches, nonzero_branches: nonzeroBranches, retired: arrayRetired},
  engine_sha256: hash(engineBytes), generated_set_sha256: hash(generatedHashes.join('\n')), generated_module_sha256: generatedHashes,
  sources: Object.fromEntries(['integer.S', 'run.mjs', 'oracle.c'].map(name => [name, hash(readFileSync(join(fixtureRoot, name)))]).concat([['dbt_memory_write_wasm.rs', hash(readFileSync(join(root, 'engine/tests/dbt_memory_write_wasm.rs')))]])),
  artifacts: {object_sha256: hash(object), native_oracle_sha256: hash(readFileSync(oraclePath))},
  tools: {node: process.version, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).trim(), llvm_objdump: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], assemble: ['clang', ...assemble], disassemble: ['xcrun', ...disassemble], compile_oracle: ['clang', ...compileOracle]},
  llvm_fixtures: [...assembled].map(([name, {offset, bytes}]) => ({name, offset, hex: bytes.toString('hex')})),
  claim: 'actual engine embedded flat32 ADD/SUB/AND/OR/XOR m32,r32/imm32/sign-extended imm8 through validated Read4 then Store4 then flags/retirement; independent unsigned BigInt/literal flag math and C/BigInt resident array/JZ oracle; unchanged GPRs/full4236 arena; old EA/source aliases and precise read/store/atomic-cross-page faults; prefix/budget/cancel/current-RAM repair/interior/unknown/lifecycle continuations without host CPU patches; real regular and identity same-byte code Store4 commits once before invalidation/recompiled successor. No hardware RMW fault/LOCK/concurrency/browser/game/performance claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, generated_module_sha256: generatedHashes.length, llvm_fixtures: provenance.llvm_fixtures.length, artifacts: outputDir}));
