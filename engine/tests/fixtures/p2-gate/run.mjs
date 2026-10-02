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
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'compile_with_gates', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'store32'];
let keys = 0n, modules = 0, actualRuns = 0, rejectedRuns = 0;
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

function descriptors(engine, blocks, gates = []) {
  refresh(engine);
  [...blocks, ...gates].forEach(([entry, value], index) => {
    engine.view.setUint32(engine.base + 140 + index * 8, entry, true);
    engine.view.setUint32(engine.base + 144 + index * 8, value, true);
  });
}

function compile(engine, blocks, gates = [], expectedHelpers = [], legacy = false, gateCount = gates.length) {
  descriptors(engine, blocks, gates);
  assert.equal(legacy ? engine.api.compile(blocks.length) : engine.api.compile_with_gates(blocks.length, gateCount), 0, 'actual engine compilation');
  refresh(engine);
  const pointer = engine.api.module_ptr() >>> 0, length = engine.api.module_len() >>> 0;
  assert.ok(length > 8);
  const encoded = engine.bytes.slice(pointer, pointer + length);
  assert.ok(WebAssembly.validate(encoded));
  const module = new WebAssembly.Module(encoded);
  const expected = [
    {module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: 'guard', kind: 'function'},
    ...expectedHelpers.map(name => ({module: 'ring3', name, kind: 'function'})),
  ].sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sortedImports(module), expected, 'only actual memory/guard/instruction helpers; no hot gate import');
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

function exit(reason, retired, detail = 0, address = 0, access = 0, length = reason === 5 ? 4 : 0, version = 3) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3EX', 40, version);
  [reason, retired, detail, address, access, length].forEach((field, index) => view.setUint32(16 + index * 4, field, true));
  return bytes;
}

function run(engine, child, budget, expected, expectedExit, label, expectedHelper = undefined) {
  const before = arena(engine);
  assert.equal(child.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, `${label}: canonical runtime exit status`);
  actualRuns++;
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

const fixtureRoot = join(root, 'engine/tests/fixtures/p2-gate');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(outputDir, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), assembled = elfFixtures(object);
const marker = [0x0f, 0x0b];
const direct = [0xe8, 0xfb, 0, 0, 0, 0x83, 0xc0, 7, 0x90];
const indirect = [0xff, 0x13, 0x83, 0xc0, 7, 0x90];
const llvmFixtures = [
  {name: 'direct_caller', bytes: direct}, {name: 'gate_marker', bytes: marker},
  {name: 'indirect_caller', bytes: indirect}, {name: 'scripted_return', bytes: [0xc3]},
];
for (const {name, bytes} of llvmFixtures) assert.deepEqual([...assembled.get(name).bytes], bytes, `independent LLVM gate fixture ${name}`);
assert.equal(assembled.get('direct_caller').offset, 0);
assert.equal(assembled.get('gate_marker').offset, 0x100, 'LLVM direct CALL displacement reaches marker');

function reject(engine, child, status, label, pointers = [0xffffffff, 0xffffffff, 0xffffffff]) {
  const before = arena(engine);
  assert.equal(child.run(pointers[0], pointers[1], 10, pointers[2]), status, label);
  rejectedRuns++;
  assert.deepEqual(arena(engine), before, `${label}: rejects before state/exit/helper writes`);
}

function current(engine, child) {
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0);
}

const highId = 0xfedcba98;
{
  const engine = fresh();
  upload(engine, 0x1000, Uint8Array.from(marker));
  const child = compile(engine, [[0x1000, 2]], [[0x1000, highId]]);
  const start = state();
  start.registers[4] = 0xffffffff; // Deliberately unmapped: numeric stop does not read a return word.
  writeState(engine, start, 1);
  const untouchedHelper = arena(engine).slice(100, 140);
  run(engine, child, 0, start, exit(2, 0), 'cancel outranks zero budget and gate', untouchedHelper);
  refresh(engine).view.setUint32(engine.base + 96, 0, true);
  run(engine, child, 0, start, exit(1, 0), 'zero budget outranks gate', untouchedHelper);
  run(engine, child, 1, start, exit(8, 0, highId), 'high numeric ID gate has no retired instruction or stack access', untouchedHelper);
  run(engine, child, 100, start, exit(8, 0, highId), 'repeated gate is identical without automatic return', untouchedHelper);
  const interior = copyState(start);
  interior.eip++;
  writeState(engine, interior);
  run(engine, child, 1, interior, exit(3, 0), 'marker second byte has no inherited gate binding', untouchedHelper);
  reject(engine, child, 1, 'current artifact requires exact arena pointers');
  writeState(engine, start);
  refresh(engine).view.setUint32(engine.base + 52, 0, true);
  reject(engine, child, 2, 'state preflight precedes gate', [engine.base, engine.base + 56, engine.base + 96]);

  const other = fresh();
  upload(other, 0x1000, Uint8Array.from(marker));
  compile(other, [[0x1000, 2]], [[0x1000, highId]]);
  const rebound = new WebAssembly.Instance(child.module, {env: {memory: other.memory}, ring3: {guard: other.api.guard}}).exports;
  writeState(other, start);
  reject(other, rebound, 3, 'different instance key rejects before gate memory reads', [other.base, other.base + 56, other.base + 96]);
}

{
  const engine = fresh();
  const ids = [1, 7, 255, 65536, 0x80000000, 0xffffffff, 0x12345678, 0xabcdef01];
  const blocks = ids.map((_, index) => [0x1000 + index * 0x10, 2]);
  const gates = blocks.map(([entry], index) => [entry, ids[index]]);
  blocks.forEach(([entry]) => upload(engine, entry, Uint8Array.from(marker)));
  const child = compile(engine, blocks, gates);
  for (const [entry, id] of gates) {
    const start = state(entry);
    start.registers[4] = 0xffffffff;
    writeState(engine, start);
    run(engine, child, 1, start, exit(8, 0, id), `transfer gate${id} including last eighth pair`, arena(engine).slice(100, 140));
  }
}

{
  const engine = fresh();
  upload(engine, 0x1000, Uint8Array.from([0xb8, 5, 0, 0, 0, 0x90]));
  const start = state(), expected = copyState(start);
  expected.registers[0] = 5;
  expected.eip = 0x1006;
  for (const legacy of [true, false]) {
    // Invalid trailing gate data must be ignored by both zero-gate entry points.
    const child = compile(engine, [[0x1000, 6]], [[0x1000, 0]], [], legacy, 0);
    writeState(engine, start);
    run(engine, child, 3, expected, exit(3, 2, 0, 0, 0, 0, 1), `${legacy ? 'legacy' : 'zero-gate'} helper-free module retains exit v1`, arena(engine).slice(100, 140));
  }
}

let rejectedCompiles = 0;
{
  const engine = fresh();
  for (const entry of [0x1000, 0x1010]) upload(engine, entry, Uint8Array.from(marker));
  upload(engine, 0x1020, Uint8Array.from([0x90, 0x90]));
  const child = compile(engine, [[0x1000, 2]], [[0x1000, 99]]);
  const pointer = engine.api.module_ptr(), length = engine.api.module_len();
  const encoded = refresh(engine).bytes.slice(pointer, pointer + length);
  const cases = [
    {name: 'block count zero', blocks: [[0x1000, 2]], gates: [], counts: [0, 0], status: 7},
    {name: 'block count above bound', blocks: [[0x1000, 2]], gates: [], counts: [9, 0], status: 7},
    {name: 'gate count exceeds blocks', blocks: [[0x1000, 2]], gates: [], counts: [1, 2], status: 7},
    {name: 'gate count above bound', blocks: [[0x1000, 2]], gates: [], counts: [1, 9], status: 7},
    {name: 'zero gate ID', blocks: [[0x1000, 2]], gates: [[0x1000, 0]], status: 10},
    {name: 'duplicate gate PC', blocks: [[0x1000, 2], [0x1010, 2]], gates: [[0x1000, 1], [0x1000, 2]], status: 10},
    {name: 'duplicate gate ID', blocks: [[0x1000, 2], [0x1010, 2]], gates: [[0x1000, 1], [0x1010, 1]], status: 10},
    {name: 'gate missing declared block', blocks: [[0x1000, 2]], gates: [[0x1010, 1]], status: 10},
    {name: 'gate wrong declared length', blocks: [[0x1000, 1]], gates: [[0x1000, 1]], status: 10},
    {name: 'gate wrong fetched marker', blocks: [[0x1020, 2]], gates: [[0x1020, 1]], status: 10},
    {name: 'unregistered UD2 remains unsupported', blocks: [[0x1000, 2]], gates: [], status: 10},
  ];
  for (const fixture of cases) {
    descriptors(engine, fixture.blocks, fixture.gates);
    const before = arena(engine), counts = fixture.counts ?? [fixture.blocks.length, fixture.gates.length];
    assert.equal(engine.api.compile_with_gates(...counts), fixture.status, fixture.name);
    rejectedCompiles++;
    assert.deepEqual(arena(engine), before, `${fixture.name}: failed request retains arena`);
    assert.equal(engine.api.generation(), child.generation);
    assert.equal(engine.api.module_ptr(), pointer);
    assert.equal(engine.api.module_len(), length);
    assert.deepEqual(refresh(engine).bytes.slice(pointer, pointer + length), encoded);
    current(engine, child);
  }
  const start = state();
  writeState(engine, start);
  run(engine, child, 1, start, exit(8, 0, 99), 'failed gate compile retains installed binding', arena(engine).slice(100, 140));
}

let scriptedCompletions = 0;
for (const memory of [false, true]) {
  const engine = fresh(), bytes = memory ? indirect : direct, callLength = memory ? 2 : 5;
  for (const address of [0x4000, 0x8000, 0x9000]) assert.equal(engine.api.map(address, 1, 3), 0);
  word(engine, 0x4000, 0x1100);
  word(engine, 0x9000, 0x10203040);
  upload(engine, 0x1000, Uint8Array.from(bytes));
  upload(engine, 0x1100, Uint8Array.from(marker));
  upload(engine, 0x1200, Uint8Array.from([0xc3]));
  const blocks = [[0x1000, callLength], [0x1000 + callLength, 4], [0x1100, 2], [0x1200, 1]];
  const child = compile(engine, blocks, [[0x1100, highId]], ['read32', 'store32']);
  const start = state();
  start.registers[3] = 0x4000;
  start.registers[4] = 0x9000;
  const called = copyState(start);
  called.registers[4] = 0x8ffc;
  called.eip = 0x1100;
  writeState(engine, start);
  run(engine, child, 2, called, exit(8, 1, highId), `${memory ? 'memory indirect' : 'direct'} CALL commits before numeric stop`, helperRecord(0));
  assert.equal(guestValue(engine, 0x8ffc), 0x1000 + callLength, 'gate retains actual pushed return word');
  assert.equal(guestValue(engine, 0x9000), 0x10203040, 'gate leaves caller argument word unchanged');
  writeState(engine, start);
  run(engine, child, 1, called, exit(1, 1), 'CALL budget1 stops at gate before gate detection', helperRecord(0));
  word(engine, 0x4000, 0x1200);
  run(engine, child, 1, called, exit(8, 0, highId), 'next positive run yields gate without another CALL', helperRecord(0));
  run(engine, child, 1, called, exit(8, 0, highId), 'repeated CALL gate stop does not return automatically', helperRecord(0));
  assert.equal(guestValue(engine, 0x8ffc), 0x1000 + callLength);

  // Scripted process-owner policy only: observe words, choose a result/resume PC,
  // then execute the existing RET and caller. This is not a provider or ABI marshaller.
  const returnPc = guestValue(engine, called.registers[4]);
  const argument = guestValue(engine, 0x9000);
  assert.equal(returnPc, 0x1000 + callLength);
  assert.equal(argument, 0x10203040);
  const completed = copyState(called);
  completed.registers[0] = argument + 1;
  completed.eip = 0x1200;
  const returned = copyState(start);
  returned.registers[0] = 0x10203048;
  returned.eip = 0x1000 + bytes.length;
  returned.eflags = 0x406; // ADD low byte 0x41+7=0x48: PF, fixed 0x2, preserved DF.
  writeState(engine, completed);
  run(engine, child, 4, returned, exit(3, 3), 'scripted process completion then guest RET/caller', helperRecord(0, returnPc));
  scriptedCompletions++;
  assert.equal(guestValue(engine, 0x9000), argument);
  assert.equal(guestValue(engine, 0x8ffc), returnPc);
  current(engine, child);
}

{
  const engine = fresh();
  upload(engine, 0x1000, Uint8Array.from(direct.slice(0, 5)));
  upload(engine, 0x1100, Uint8Array.from(marker));
  const child = compile(engine, [[0x1000, 5], [0x1100, 2]], [[0x1100, highId]], ['store32']);
  const start = state();
  start.registers[4] = 0x9004;
  writeState(engine, start);
  run(engine, child, 2, start, exit(5, 0, 1, 0x9000, 2), 'gate-bearing direct CALL fault remains precise exit v3', helperRecord(1, 0, 1, 0x9000, 2));
  start.registers[4] = 0x1104;
  const committed = copyState(start);
  committed.registers[4] = 0x1100;
  committed.eip = 0x1100;
  writeState(engine, start);
  run(engine, child, 1, committed, exit(6, 1), 'CALL writes gate marker and exits CodeInvalidated before gate', helperRecord(0));
  assert.equal(guestValue(engine, 0x1100), 0x1005);
  reject(engine, child, 4, 'CALL-invalidated gate rejects before bad pointer/state access');
}

for (const mutation of ['same-byte', 'changed-byte', 'protect', 'unmap', 'replacement', 'close']) {
  const engine = fresh();
  upload(engine, 0x1000, Uint8Array.from(marker));
  const child = compile(engine, [[0x1000, 2]], [[0x1000, highId]]);
  writeState(engine, state());
  let status = 4;
  if (mutation === 'same-byte') upload(engine, 0x1000, Uint8Array.from(marker));
  else if (mutation === 'changed-byte') upload(engine, 0x1000, Uint8Array.from([0x0f, 0x90]));
  else if (mutation === 'protect') assert.equal(engine.api.protect(0x1000, 1, 7), 0);
  else if (mutation === 'unmap') assert.equal(engine.api.unmap(0x1000, 1), 0);
  else if (mutation === 'replacement') {
    const replacement = compile(engine, [[0x1000, 2]], [[0x1000, 7]]);
    writeState(engine, state());
    run(engine, replacement, 1, state(), exit(8, 0, 7), 'same marker bytes accept a new immutable numeric binding', arena(engine).slice(100, 140));
    status = 3;
  } else {
    assert.equal(engine.api.close(), 0);
    status = 5;
  }
  reject(engine, child, status, `${mutation} gate artifact rejects before invalid pointers`);
}

{
  const engine = fresh();
  assert.equal(engine.api.map(0x2000, 1, 7), 0);
  upload(engine, 0x1fff, Uint8Array.from(marker));
  assert.equal(engine.api.protect(0x1000, 2, 4), 0, 'gate accepts checked Execute without Read');
  const child = compile(engine, [[0x1fff, 2]], [[0x1fff, 0xffffffff]]);
  const start = state(0x1fff);
  start.registers[4] = 0xffffffff;
  writeState(engine, start);
  run(engine, child, 1, start, exit(8, 0, 0xffffffff), 'cross-page execute-only numeric marker', arena(engine).slice(100, 140));
  assert.equal(engine.api.protect(0x2000, 1, 4), 0);
  reject(engine, child, 4, 'second marker page stamp invalidates gate before any access');
}

const provenance = {
  engine_sha256: hash(engineBytes), generated_modules: modules,
  actual_engine_runs: actualRuns, entry_guard_rejections: rejectedRuns, rejected_compiles: rejectedCompiles,
  scripted_process_completions: scriptedCompletions, synthetic_helper_runs: 0,
  generated_set_sha256: hash(generatedHashes.join('\n')),
  sources: {assembly_sha256: hash(readFileSync(assembly))}, artifacts: {object_sha256: hash(object)},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]},
  llvm_fixtures: llvmFixtures.map(({name}) => ({name, offset: assembled.get(name).offset, hex: assembled.get(name).bytes.toString('hex')})),
  claim: 'actual artifact-bound numeric CPU yield, precise CALL state and entry guards; two completion policies are scripted by the fixture process owner, not implemented Windows providers, calling conventions, callbacks or marshalling',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, artifacts: outputDir}));
