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
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'compile_with_gates', 'compile_entries'];
let keys = 0n, modules = 0, actualRuns = 0, ramWords = 0, snapshotGuards = 0;
const counts = {flags: 0, aliases: 0, addresses: 0, entry: 0, read_faults: 0, continuation_initial_calls: 0, continuation_resume_calls: 0, canonical_exits: 0, guard_rejections: 0, scans: 0};
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
    assert.equal(engine.api.compile(blocks.length), 0, 'actual explicit memory comparison preparation');
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
  actualRuns++; counts.canonical_exits++;
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

const fixtureRoot = join(root, 'engine/tests/fixtures/p2-memory-compare');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassemble = ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath];
writeFileSync(join(outputDir, 'integer.disassembly.txt'), execFileSync('xcrun', disassemble));
const object = readFileSync(objectPath), assembled = elfFixtures(object);
const forms = [
  {name: 'cmp_reg', op: 'cmp', hex: '3906', register: 0},
  {name: 'cmp_imm32', op: 'cmp', hex: '813e01000080', immediate: 0x80000001},
  {name: 'cmp_imm8', op: 'cmp', hex: '833eff', immediate: 0xffffffff},
  {name: 'test_reg', op: 'test', hex: '8506', register: 0},
  {name: 'test_imm32', op: 'test', hex: 'f70601000080', immediate: 0x80000001},
];
const registers = ['eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi'];
for (const form of forms) assert.equal(assembled.get(form.name).bytes.toString('hex'), form.hex, `LLVM checked ${form.name} encoding`);

function guarded(engine, child) {
  const before = arena(engine);
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'read-only comparison preserves compiled code snapshot');
  snapshotGuards++;
  assert.deepEqual(arena(engine), before, 'successful code guard preserves complete arena');
}

function sampleRam(engine, addresses) {
  return addresses.map(address => { ramWords++; return guestValue(engine, address); });
}

function flags(op, lhs, rhs, oldFlags) {
  const a = BigInt(lhs), b = BigInt(rhs), raw = op === 'cmp' ? a - b : a & b;
  const value = Number(BigInt.asUintN(32, raw));
  let result = (oldFlags & 0x400) | 2, parity = 0;
  for (let byte = value & 255; byte !== 0; byte >>>= 1) parity += byte & 1;
  if (parity % 2 === 0) result |= 4;
  if (value === 0) result |= 0x40;
  if (value >= 0x80000000) result |= 0x80;
  if (op === 'cmp') {
    if (a < b) result |= 1;
    if ((a & 15n) < (b & 15n)) result |= 0x10;
    const signed = BigInt.asIntN(32, a) - BigInt.asIntN(32, b);
    if (signed < -2147483648n || signed > 2147483647n) result |= 0x800;
  }
  return result;
}

function expectedComparison(start, form, lhs, length, register = form.register) {
  const expected = copyState(start), rhs = form.immediate ?? start.registers[register];
  expected.eflags = flags(form.op, lhs, rhs, start.eflags);
  expected.eip += length;
  return expected;
}

const literals = [
  ['cmp', 0, 0, 0x46], ['cmp', 0, 1, 0x97], ['cmp', 1, 0, 0x2],
  ['cmp', 0x80000000, 1, 0x816], ['cmp', 0x7fffffff, 0xffffffff, 0x887],
  ['cmp', 0xffffffff, 0x7fffffff, 0x86], ['cmp', 0x10, 1, 0x16],
  ['test', 0xffffffff, 0, 0x46], ['test', 0x80000000, 0xffffffff, 0x86],
  ['test', 1, 1, 0x2], ['test', 3, 3, 0x6],
];
for (const [op, lhs, rhs, expected] of literals) assert.equal(flags(op, lhs, rhs, 2), expected, 'independent mathematical flags agree with fixed literal anchors');
const operands = [[0, 0], [0, 1], [1, 0], [0xffffffff, 1], [0x7fffffff, 1], [0x80000000, 1], [0x80000000, 0x80000000], [0, 0xffffffff], [0x10, 1], [0xf, 1], [0x12345678, 0x87654321], [0xaaaaaaaa, 0x55555555], [0x7fffffff, 0xffffffff]];
for (const form of forms) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0);
  word(engine, 0x4000, 0x11223344); word(engine, 0x4008, 0x55667788); word(engine, 0x4ffc, 0xaabbccdd);
  const bytes = assembled.get(form.name).bytes, child = compile(engine, bytes);
  for (const [lhs, rhs] of operands) for (const initialFlags of [2, 0xcd7]) {
    assert.equal(engine.api.protect(0x4000, 1, 3), 0); word(engine, 0x4004, lhs);
    assert.equal(engine.api.protect(0x4000, 1, 1), 0);
    const ram = sampleRam(engine, [0x4000, 0x4004, 0x4008, 0x4ffc, 0x1000]);
    const start = state(0x1000, initialFlags); start.registers[0] = rhs; start.registers[6] = 0x4004;
    writeState(engine, start);
    run(engine, child, 1, expectedComparison(start, form, lhs, bytes.length), exit(1, 1), `${form.name} read-only flags/${lhs}/${rhs}/${initialFlags}`, helperRecord(0, lhs));
    assert.deepEqual(sampleRam(engine, [0x4000, 0x4004, 0x4008, 0x4ffc, 0x1000]), ram, 'read-only source, neighbors, tail and code RAM unchanged');
    guarded(engine, child); counts.flags++;
  }
}

const immediates = [
  {name: 'cmp_imm8_min', op: 'cmp', immediate: 0xffffff80, hex: '833e80'},
  {name: 'cmp_imm8_minus1', op: 'cmp', immediate: 0xffffffff, hex: '833eff'},
  {name: 'cmp_imm8_zero', op: 'cmp', immediate: 0, hex: '833e00'},
  {name: 'cmp_imm8_max', op: 'cmp', immediate: 127, hex: '833e7f'},
  {name: 'cmp_imm32_high', op: 'cmp', immediate: 0x80000000, hex: '813e00000080'},
  {name: 'cmp_imm32_max', op: 'cmp', immediate: 0x7fffffff, hex: '813effffff7f'},
  {name: 'cmp_imm32_literal', op: 'cmp', immediate: 0x12345678, hex: '813e78563412'},
  {name: 'cmp_imm32_minus1', op: 'cmp', immediate: 0xffffffff, hex: '813effffffff'},
  {name: 'test_imm32_zero', op: 'test', immediate: 0, hex: 'f70600000000'},
  {name: 'test_imm32_minus1', op: 'test', immediate: 0xffffffff, hex: 'f706ffffffff'},
  {name: 'test_imm32_high', op: 'test', immediate: 0x80000000, hex: 'f70600000080'},
  {name: 'test_imm32_max', op: 'test', immediate: 0x7fffffff, hex: 'f706ffffff7f'},
  {name: 'test_imm32_literal', op: 'test', immediate: 0x12345678, hex: 'f70678563412'},
];
for (const form of immediates) {
  const bytes = assembled.get(form.name).bytes;
  assert.equal(bytes.toString('hex'), form.hex, `LLVM immediate ${form.name}`);
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0);
  const child = compile(engine, bytes);
  for (const lhs of [0, 1, 0x7fffffff, 0x80000000, 0xffffffff, 0xffffff80]) for (const initialFlags of [2, 0xcd7]) {
    word(engine, 0x4000, lhs);
    const start = state(0x1000, initialFlags); start.registers[6] = 0x4000;
    writeState(engine, start);
    run(engine, child, 1, expectedComparison(start, form, lhs, bytes.length), exit(1, 1), `${form.name}/${lhs}/${initialFlags}`, helperRecord(0, lhs));
    assert.equal(guestValue(engine, 0x4000), lhs, 'immediate comparison preserves guest word'); ramWords++;
    guarded(engine, child); counts.flags++;
  }
}

for (const op of ['cmp', 'test']) for (const [register, name] of registers.entries()) for (const sib of [false, true]) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0); assert.equal(engine.api.map(0xc000, 1, 3), 0);
  const start = state(); start.registers[register] = 0x4004;
  if (sib && register === 4) start.registers[1] = 3;
  const address = !sib ? 0x4004 : register === 4 ? 0x400c : 0xc008, lhs = 0xfedcba98;
  word(engine, address - 4, 0x11223344); word(engine, address, lhs); word(engine, address + 4, 0x55667788);
  const bytes = assembled.get(`${op}_${sib ? 'sib' : 'base'}_${name}`).bytes, child = compile(engine, bytes);
  assert.equal(bytes[0], op === 'cmp' ? 0x39 : 0x85); assert.equal((bytes[1] >>> 3) & 7, register, 'LLVM source GPR');
  const ram = sampleRam(engine, [address - 4, address, address + 4, 0x1000]);
  writeState(engine, start);
  run(engine, child, 1, expectedComparison(start, {op, register}, lhs, bytes.length), exit(1, 1), `${op} old ${name} ${sib ? 'SIB' : 'base'} source/EA alias`, helperRecord(0, lhs));
  assert.deepEqual(sampleRam(engine, [address - 4, address, address + 4, 0x1000]), ram);
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
for (const form of forms) for (const fixture of addressForms) {
  const engine = fresh();
  for (const [address, pages] of [[0, 1], [0x4000, 2], [0xfffff000, 1]]) assert.equal(engine.api.map(address, pages, 3), 0);
  const lhs = 0x90abcdef; word(engine, fixture.address, lhs);
  const start = state(); start.registers[0] = 0x76543210;
  Object.entries(fixture.regs).forEach(([reg, value]) => { start.registers[Number(reg)] = value; });
  const bytes = assembled.get(fixture.form ? `${form.name}_${fixture.form}` : form.name).bytes, child = compile(engine, bytes);
  const ram = sampleRam(engine, [fixture.address, 0x1000]);
  writeState(engine, start);
  run(engine, child, 1, expectedComparison(start, form, lhs, bytes.length), exit(1, 1), `${form.name} ${fixture.form ?? 'page boundary'}/${fixture.address}`, helperRecord(0, lhs));
  assert.deepEqual(sampleRam(engine, [fixture.address, 0x1000]), ram);
  guarded(engine, child); counts.addresses++;
}

for (const form of forms) {
  const engine = fresh(), bytes = assembled.get(`${form.name}_entry`).bytes, child = compile(engine, bytes, undefined, true);
  const start = state(); start.registers[6] = 0x9000;
  writeState(engine, start);
  run(engine, child, 1, start, exit(5, 0, 1, 0x9000, 1), `${form.name} entry prepared with unmapped data faults only at runtime`, helperRecord(1, 0, 1, 0x9000, 1));
  assert.equal(engine.api.map(0x9000, 1, 3), 0); word(engine, 0x9000, 0x80000000);
  const repaired = expectedComparison(start, form, 0x80000000, assembled.get(form.name).bytes.length);
  run(engine, child, 1, repaired, exit(1, 1), `${form.name} entry mapping repair keeps prepared artifact`, helperRecord(0, 0x80000000));
  guarded(engine, child); counts.entry += 2;
}

const faults = [
  {label: 'unmapped', address: 0x9000, detail: 1, fault: 0x9000},
  {label: 'write-only page', address: 0x4000, detail: 2, fault: 0x4000, maps: [[0x4000, 1, 2]]},
  {label: 'page-final Read4 missing next page', address: 0x4fff, detail: 1, fault: 0x5000, maps: [[0x4000, 1, 3]]},
  {label: 'cross-page Read4 permission', address: 0x4ffe, detail: 2, fault: 0x5000, maps: [[0x4000, 1, 3], [0x5000, 1, 2]]},
  {label: 'width overflow at final three bytes', address: 0xfffffffd, detail: 3, fault: 0xfffffffd, maps: [[0xfffff000, 1, 3]]},
  {label: 'width overflow at final byte', address: 0xffffffff, detail: 3, fault: 0xffffffff, maps: [[0xfffff000, 1, 3]]},
];
for (const form of forms) for (const fixture of faults) {
  const engine = fresh();
  for (const [address, pages] of fixture.maps ?? []) { assert.equal(engine.api.map(address, pages, 3), 0); word(engine, address, 0x11223344); }
  for (const [address, pages, permissions] of fixture.maps ?? []) assert.equal(engine.api.protect(address, pages, permissions), 0);
  const bytes = assembled.get(`${form.name}_prefix`).bytes, child = compile(engine, bytes);
  const start = state(); start.registers[6] = fixture.address;
  const stopped = copyState(start); stopped.registers[2] = 0; stopped.eflags = 0x457; stopped.eip = 0x1008;
  writeState(engine, start);
  run(engine, child, 20, stopped, exit(5, 2, fixture.detail, fixture.fault, 1), `${form.name} committed prefix then ${fixture.label}`, helperRecord(1, 0, fixture.detail, fixture.fault, 1));
  guarded(engine, child); counts.read_faults++;
}

for (const form of forms) {
  const engine = fresh(), bytes = assembled.get(`${form.name}_prefix`).bytes, child = compile(engine, bytes);
  const start = state(); start.registers[6] = 0x9000;
  writeState(engine, start);
  run(engine, child, 0, start, exit(1, 0), `${form.name} zero budget before prefix/read`); counts.continuation_initial_calls++;
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 0, start, exit(2, 0), `${form.name} cancel outranks zero budget`); counts.continuation_resume_calls++;
  engine.view.setUint32(engine.base + 96, 0, true);
  const moved = copyState(start); moved.registers[2] = 0xffffffff; moved.eip = 0x1005;
  run(engine, child, 1, moved, exit(1, 1), `${form.name} one budget retires prefix MOV only`); counts.continuation_resume_calls++;
  const stopped = copyState(start); stopped.registers[2] = 0; stopped.eflags = 0x457; stopped.eip = 0x1008;
  run(engine, child, 1, stopped, exit(1, 1), `${form.name} one budget retires prefix ADD only`); counts.continuation_resume_calls++;
  run(engine, child, 0, stopped, exit(1, 0), `${form.name} zero budget before comparison preserves prefix flags`); counts.continuation_resume_calls++;
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 20, stopped, exit(2, 0), `${form.name} cancel at comparison outranks the unmapped read`); counts.continuation_resume_calls++;
  engine.view.setUint32(engine.base + 96, 0, true);
  run(engine, child, 20, stopped, exit(5, 0, 1, 0x9000, 1), `${form.name} precise read fault from actual prefix continuation`, helperRecord(1, 0, 1, 0x9000, 1)); counts.continuation_resume_calls++;
  run(engine, child, 1, stopped, exit(5, 0, 1, 0x9000, 1), `${form.name} unrepaired retry preserves prefix flags`, helperRecord(1, 0, 1, 0x9000, 1)); counts.continuation_resume_calls++;
  assert.equal(engine.api.map(0x9000, 1, 3), 0); word(engine, 0x9000, 0x89abcdef);
  const repaired = expectedComparison(stopped, form, 0x89abcdef, assembled.get(form.name).bytes.length);
  run(engine, child, 1, repaired, exit(1, 1), `${form.name} repair resumes faulting read only`, helperRecord(0, 0x89abcdef)); counts.continuation_resume_calls++;
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 20, repaired, exit(2, 0), `${form.name} cancel between read and successor`, helperRecord(0, 0x89abcdef)); counts.continuation_resume_calls++;
  engine.view.setUint32(engine.base + 96, 0, true);
  const next = copyState(repaired); next.registers[1] = 0x99999999; next.eip = 0x1000 + bytes.length;
  run(engine, child, 1, next, exit(1, 1), `${form.name} interior successor after repaired read`, helperRecord(0, 0x89abcdef)); counts.continuation_resume_calls++;
  run(engine, child, 1, next, exit(3, 0), `${form.name} unknown continuation retires no extra operation`, helperRecord(0, 0x89abcdef)); counts.continuation_resume_calls++;
  guarded(engine, child);
}

for (const form of forms) {
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0); word(engine, 0x4000, 0x89abcdef);
  const child = compile(engine, assembled.get(form.name).bytes), start = state(); start.registers[6] = 0x4000;
  writeState(engine, start);
  assert.equal(engine.api.protect(0x1000, 1, 5), 0);
  let before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 4, `${form.name} stale guard precedes invalid pointers`); actualRuns++; counts.guard_rejections++;
  assert.deepEqual(arena(engine), before, 'stale rejection preserves complete arena');
  assert.equal(engine.api.close(), 0); before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 5, `${form.name} closed guard precedes invalid pointers`); actualRuns++; counts.guard_rejections++;
  assert.deepEqual(arena(engine), before, 'closed rejection preserves complete arena');
}

const oracleSource = join(fixtureRoot, 'compare_oracle.c'), oraclePath = join(outputDir, 'compare_oracle');
const oracleCompile = ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', oracleSource, '-o', oraclePath];
execFileSync('clang', oracleCompile, {stdio: ['ignore', 'pipe', 'pipe']});
const scanInputs = [
  {threshold: 0, mask: 1, rows: [[0, 1]]},
  {threshold: 1, mask: 0xffffffff, rows: [[0, 0xffffffff]]},
  {threshold: 0, mask: 0, rows: [[0xffffffff, 0xffffffff]]},
  {threshold: 0x80000000, mask: 0x80000001, rows: [[0x7fffffff, 1], [0x80000000, 0x80000000], [0xffffffff, 2], [0x80000001, 1]]},
  {threshold: 0xffffffff, mask: 0xffffffff, rows: [[0, 1], [0xffffffff, 0], [0xfffffffe, 3], [0xffffffff, 0xffffffff]]},
  {threshold: 0x12345678, mask: 0xaaaaaaaa, rows: [[0x12345677, 0xffffffff], [0x12345678, 0x55555555], [0x12345679, 2], [0xffffffff, 0x80000000], [0x80000000, 0], [0x12345678, 0xaaaaaaaa], [1, 1], [0x7fffffff, 3], [0xffffffff, 4], [0x12345678, 8], [0x12345678, 0xffffffff], [0xffffffff, 0]]},
];
let belowBranches = 0, zeroBranches = 0, acceptedBranches = 0, scanRetired = 0;
for (const {threshold, mask, rows} of scanInputs) {
  const native = JSON.parse(execFileSync(oraclePath, [threshold, mask, ...rows.flat()].map(String), {encoding: 'utf8'}));
  let accepted = 0, below = 0, zero = 0, lastRead = 0;
  for (const [value, bits] of rows) {
    lastRead = value;
    if (BigInt(value) < BigInt(threshold)) below++;
    else { lastRead = bits; if ((BigInt(bits) & BigInt(mask)) === 0n) zero++; else accepted++; }
  }
  assert.deepEqual(native, {accepted, below, zero, last_read: lastRead}, 'defined unsigned C and independent BigInt predicate agree');
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0);
  rows.flat().forEach((value, index) => { word(engine, 0x4000 + index * 4, value); });
  word(engine, 0x4000 + rows.length * 8, 0x11223344); word(engine, 0x4ffc, 0x55667788);
  assert.equal(engine.api.protect(0x4000, 1, 1), 0);
  const names = ['scan_compare', 'scan_test', 'scan_accept', 'scan_skip', 'scan_joined', 'scan_finish'];
  const blocks = names.map(name => { const fixture = assembled.get(name); upload(engine, 0x1000 + fixture.offset, fixture.bytes); return [0x1000 + fixture.offset, fixture.bytes.length]; });
  const child = compile(engine, new Uint8Array(), blocks), start = state(0x1600);
  start.registers[0] = threshold; start.registers[1] = rows.length; start.registers[2] = 0; start.registers[3] = mask; start.registers[6] = 0x4000;
  const expected = copyState(start); expected.registers[1] = 0; expected.registers[2] = native.accepted; expected.registers[6] += rows.length * 8; expected.eip = 0x1700; expected.eflags = 0x446;
  const sampled = [...rows.flat().keys()].map(index => 0x4000 + index * 4).concat([0x4000 + rows.length * 8, 0x4ffc, 0x1600]);
  const ram = sampleRam(engine, sampled), retired = below * 6 + zero * 8 + accepted * 9 + 1;
  writeState(engine, start);
  run(engine, child, 200, expected, exit(3, retired), `resident unsigned C read-only predicate/${threshold}/${mask}/${rows.length}`, helperRecord(0, native.last_read));
  assert.deepEqual(sampleRam(engine, sampled), ram, 'predicate preserves every input word, sentinels and code');
  guarded(engine, child); counts.scans++; belowBranches += below; zeroBranches += zero; acceptedBranches += accepted; scanRetired += retired;
}
assert.ok(belowBranches > 0 && zeroBranches > 0 && acceptedBranches > 0, 'actual CMP/JB and TEST/JZ take both predicate branches');
const invalidOracleInputs = [[], ['0', '0'], ['0', '0', '0'], ['0', '0', ...Array(3).fill('0')], ['-1', '0', '0', '0'], ['4294967296', '0', '0', '0'], ['0', '1x', '0', '0'], ['0', ' 0', '0', '0'], ['0', '0', ...Array(26).fill('0')]];
for (const args of invalidOracleInputs) {
  const rejected = spawnSync(oraclePath, args, {encoding: 'utf8'});
  assert.equal(rejected.status, 2, `native utility rejects ${JSON.stringify(args)}`); assert.equal(rejected.stdout, '');
}
assert.equal(actualRuns, counts.canonical_exits + counts.guard_rejections, 'actual generated run calls equal canonical exits plus guard rejections');
assert.equal(counts.continuation_initial_calls, forms.length); assert.equal(counts.continuation_resume_calls, forms.length * 11);
const provenance = {
  counts, generated_modules: modules, actual_generated_run_calls: actualRuns, snapshot_guard_calls: snapshotGuards, ram_words_observed: ramWords,
  native_algorithm_cases: scanInputs.length, native_invalid_inputs: invalidOracleInputs.length,
  predicate: {below_branches: belowBranches, zero_branches: zeroBranches, accepted_branches: acceptedBranches, retired: scanRetired},
  engine_sha256: hash(engineBytes), generated_set_sha256: hash(generatedHashes.join('\n')), generated_module_sha256: generatedHashes,
  sources: Object.fromEntries(['integer.S', 'compare_oracle.c', 'run.mjs'].map(name => [name, hash(readFileSync(join(fixtureRoot, name)))]).concat([['dbt_memory_compare_wasm.rs', hash(readFileSync(join(root, 'engine/tests/dbt_memory_compare_wasm.rs')))]])),
  artifacts: {object_sha256: hash(object), native_binary_sha256: hash(readFileSync(oraclePath))},
  tools: {node: process.version, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).trim(), llvm_objdump: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], assemble: ['clang', ...assemble], disassemble: ['xcrun', ...disassemble], native_compile: ['clang', ...oracleCompile]},
  llvm_fixtures: [...assembled].map(([name, {offset, bytes}]) => ({name, offset, hex: bytes.toString('hex')})),
  claim: 'actual engine embedded flat32 CMP 39/r, 81/7 imm32, 83/7 sign-imm8 and TEST 85/r, F7/0 imm32; memory LHS/source RHS flags with literal and mathematical anchors; preserved GPRs and full 4236-byte arena; no store import; read-only pages and sampled RAM/code purity; precise Read4 and continuation/lifecycle proof; independent unsigned C/BigInt resident predicate consuming both CMP/JB and TEST/JZ. TEST AF is deterministically zero by existing policy. No hardware, writing memory ISA, browser, title compatibility or performance claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, generated_module_sha256: generatedHashes.length, llvm_fixtures: provenance.llvm_fixtures.length, artifacts: outputDir}));
