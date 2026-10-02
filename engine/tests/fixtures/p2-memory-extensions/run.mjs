import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
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
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'compile_with_gates', 'generation', 'module_ptr', 'module_len', 'guard', 'read8', 'read16'];
let keys = 0n, modules = 0, actualRuns = 0, injectedRuns = 0, continuations = 0, helperProbes = 0;
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
  const key = 0x3080000000000000n + ++keys;
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

function narrow(width, tag = 0, value = 0, detail = 0, address = 0, access = 0) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3MH', 40, 2);
  [tag, value, detail, address, access, width].forEach((field, index) => view.setUint32(16 + index * 4, field, true));
  return bytes;
}

function compile(engine, bytes, blocks = [[0x1000, bytes.length]], expectedHelpers = [], gates = []) {
  if (bytes.length !== 0) upload(engine, 0x1000, bytes);
  refresh(engine);
  blocks.forEach(([entry, length], index) => {
    engine.view.setUint32(engine.base + 140 + index * 8, entry, true);
    engine.view.setUint32(engine.base + 144 + index * 8, length, true);
  });
  gates.forEach(([entry, id], index) => {
    engine.view.setUint32(engine.base + 140 + blocks.length * 8 + index * 8, entry, true);
    engine.view.setUint32(engine.base + 144 + blocks.length * 8 + index * 8, id, true);
  });
  assert.equal(gates.length ? engine.api.compile_with_gates(blocks.length, gates.length) : engine.api.compile(blocks.length), 0, 'actual engine compiles checked memory-extension region');
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
    ring3: {guard: engine.api.guard, read8: engine.api.read8, read16: engine.api.read16},
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

function exit(reason, retired, detail = 0, address = 0, access = 0, length = 0, version = 2) {
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
  assert.deepEqual(after.slice(56, 96), expectedExit, `${label}: canonical v2 exit`);
  assert.deepEqual(after.slice(96, 100), before.slice(96, 100), `${label}: cancellation unchanged`);
  assert.deepEqual(after.slice(140), before.slice(140), `${label}: transfer unchanged`);
  assert.deepEqual(after.slice(100, 140), expectedHelper ?? before.slice(100, 140), `${label}: helper result/preservation`);
}

function conversion(kind, value, width) {
  const modulus = 2n ** BigInt(width * 8), source = BigInt(value) % modulus;
  return Number(BigInt.asUintN(32, kind === 'sx' && source >= modulus / 2n ? source - modulus : source));
}
const forms = [
  {name: 'zx_byte_eax', hex: '0fb600', kind: 'zx', width: 1, destination: 0, regs: {0: 0x4000}, address: 0x4000},
  {name: 'sx_byte_ecx', hex: '0fbe4c73ff', kind: 'sx', width: 1, destination: 1, regs: {3: 0x4001, 6: 0}, address: 0x4000},
  {name: 'zx_word_edx', hex: '0fb7548a7f', kind: 'zx', width: 2, destination: 2, regs: {2: 0x3f81, 1: 0}, address: 0x4000},
  {name: 'sx_word_ebx', hex: '0fbf5cdb80', kind: 'sx', width: 2, destination: 3, regs: {3: 0x720}, address: 0x3fa0},
  {name: 'zx_byte_esp', hex: '0fb62424', kind: 'zx', width: 1, destination: 4, regs: {4: 0x4000}, address: 0x4000},
  {name: 'sx_byte_ebp', hex: '0fbe6c3d00', kind: 'sx', width: 1, destination: 5, regs: {5: 0x3ffe, 7: 2}, address: 0x4000},
  {name: 'zx_word_esi', hex: '0fb734b500400000', kind: 'zx', width: 2, destination: 6, regs: {6: 0}, address: 0x4000},
  {name: 'sx_word_edi', hex: '0fbf3d00400000', kind: 'sx', width: 2, destination: 7, regs: {}, address: 0x4000},
];
const generic = [
  {name: 'zx_byte', hex: '0fb606', kind: 'zx', width: 1}, {name: 'sx_byte', hex: '0fbe06', kind: 'sx', width: 1},
  {name: 'zx_word', hex: '0fb706', kind: 'zx', width: 2}, {name: 'sx_word', hex: '0fbf06', kind: 'sx', width: 2},
];
function elfFixtures(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]);
  assert.equal(view.getUint16(18, true), 3, 'LLVM i386 object');
  const sectionOffset = view.getUint32(32, true), sectionSize = view.getUint16(46, true), count = view.getUint16(48, true);
  const sections = Array.from({length: count}, (_, index) => {
    const at = sectionOffset + index * sectionSize;
    return {type: view.getUint32(at + 4, true), offset: view.getUint32(at + 16, true), size: view.getUint32(at + 20, true), link: view.getUint32(at + 24, true), info: view.getUint32(at + 28, true), stride: view.getUint32(at + 36, true)};
  });
  const table = sections.find(section => section.type === 2);
  assert.ok(table);
  const strings = sections[table.link];
  const symbols = new Map();
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    const startName = strings.offset + view.getUint32(at, true);
    const name = object.subarray(startName, object.indexOf(0, startName)).toString('utf8');
    const section = view.getUint16(at + 14, true), start = view.getUint32(at + 4, true), size = view.getUint32(at + 8, true);
    if (section > 0 && section < sections.length && size > 0) {
      const source = sections[section];
      assert.ok(start + size <= source.size);
      assert.ok(!sections.some(item => [4, 9].includes(item.type) && item.info === section && item.size > 0), 'authored code has no relocations');
      symbols.set(name, {offset: start, bytes: object.subarray(source.offset + start, source.offset + start + size)});
    }
  }
  return symbols;
}

const fixtureRoot = join(root, 'engine/tests/fixtures/p2-memory-extensions');
const assembly = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(outputDir, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), assembled = elfFixtures(object), llvm = [];
function verify(name, offset, hex) {
  const fixture = assembled.get(name);
  assert.ok(fixture, `${name}: authored LLVM symbol`);
  assert.equal(fixture.offset, offset, `${name}: independent offset`);
  assert.equal(fixture.bytes.toString('hex'), hex, `${name}: independent encoding`);
  llvm.push({name, offset, hex});
}
let offset = 0;
for (const fixture of [...forms, ...generic]) { verify(fixture.name, offset, fixture.hex); offset += fixture.hex.length / 2; }
verify('prefix_fault', offset, '8d448bff0fbf1690'); offset += 8;
verify('scripted_byte', offset, 'ba785634120fbe06b999999999'); offset += 13;
verify('scripted_word', offset, 'ba785634120fbf06b999999999');
verify('composition', 0x200, '0fb6060fbe56010fbf5e0201d031d80fb77e04e9e8020000');
verify('composition_done', 0x500, '0f0b');
assert.equal(assembled.size, llvm.length, 'all nonempty LLVM symbols verified');

function valueBytes(width, value) { return Uint8Array.from(width === 1 ? [value] : [value % 256, Math.floor(value / 256)]); }
function fixtureEngine(fixture) {
  const engine = fresh();
  assert.equal(engine.api.map(0x3000, 2, 3), 0);
  const child = compile(engine, assembled.get(fixture.name).bytes, undefined, [fixture.width === 1 ? 'read8' : 'read16']);
  return {engine, child};
}
function memoryStart(fixture, flags = 0xcd7) {
  const start = state(0x1000, flags);
  for (const [register, value] of Object.entries(fixture.regs ?? {6: 0x4000})) start.registers[Number(register)] = value;
  return start;
}
for (const fixture of forms) {
  const {engine, child} = fixtureEngine(fixture), value = fixture.width === 1 ? 0x80 : 0x8001;
  upload(engine, fixture.address, valueBytes(fixture.width, value));
  for (const flags of [2, 0xcd7]) {
    const start = memoryStart(fixture, flags), expected = copyState(start);
    expected.registers[fixture.destination] = conversion(fixture.kind, value, fixture.width);
    expected.eip += fixture.hex.length / 2;
    writeState(engine, start);
    run(engine, child, 1, expected, exit(1, 1), `actual EA/alias/all-destinations ${fixture.name}/flags${flags}`, narrow(fixture.width, 0, value));
  }
}

for (const fixture of generic) {
  const {engine, child} = fixtureEngine(fixture);
  const edges = fixture.width === 1 ? [[0, 0], [1, 1], [0x7f, 0x7f], [0x80, 0xffffff80], [0xff, 0xffffffff]] : [[0, 0], [1, 1], [0x7fff, 0x7fff], [0x8000, 0xffff8000], [0xffff, 0xffffffff]];
  for (const [value, signed] of edges) {
    upload(engine, 0x4000, valueBytes(fixture.width, value));
    const literal = fixture.kind === 'zx' ? value : signed;
    assert.equal(conversion(fixture.kind, value, fixture.width), literal, 'independent literal/unsigned extraction agreement');
    for (const flags of [2, 0xcd7]) {
      const start = memoryStart(fixture, flags), expected = copyState(start);
      expected.registers[0] = literal;
      expected.eip += 3;
      writeState(engine, start);
      run(engine, child, 1, expected, exit(1, 1), `actual literal ${fixture.name}/${value}/flags${flags}`, narrow(fixture.width, 0, value));
    }
  }
}

const boundary = new Map();
for (const fixture of generic) {
  const engine = fresh();
  assert.equal(engine.api.map(0x4000, 1, 3), 0);
  assert.equal(engine.api.map(0xfffff000, 1, 3), 0);
  upload(engine, 0x4ffe, Uint8Array.from([0, 0x80]));
  upload(engine, 0xfffffffe, Uint8Array.from([0, 0x80]));
  assert.equal(engine.api.protect(0x4000, 1, 1), 0);
  assert.equal(engine.api.protect(0xfffff000, 1, 1), 0);
  const child = compile(engine, assembled.get(fixture.name).bytes, undefined, [fixture.width === 1 ? 'read8' : 'read16']);
  boundary.set(fixture.name, {engine, child});
  for (const address of fixture.width === 1 ? [0x4fff, 0xffffffff] : [0x4ffe, 0xfffffffe]) {
    const value = fixture.width === 1 ? 0x80 : 0x8000;
    const start = state(); start.registers[6] = address;
    const expected = copyState(start); expected.registers[0] = conversion(fixture.kind, value, fixture.width); expected.eip += 3;
    writeState(engine, start);
    run(engine, child, 1, expected, exit(1, 1), `actual no-overfetch ${fixture.name}/${address}`, narrow(fixture.width, 0, value));
  }
}
const crossing = boundary.get('sx_word');
for (const [address, detail, denied] of [[0x4fff, 1, 0x5000], [0xffffffff, 3, 0xffffffff]]) {
  const start = state(); start.registers[6] = address; writeState(crossing.engine, start);
  run(crossing.engine, crossing.child, 10, start, exit(5, 0, detail, denied, 1, 2), `actual narrow fault ${address}`, narrow(2, 1, 0, detail, denied, 1));
}
assert.equal(crossing.engine.api.map(0x5000, 1, 2), 0);
upload(crossing.engine, 0x5000, Uint8Array.from([1]));
const deniedStart = state(); deniedStart.registers[6] = 0x4fff;
writeState(crossing.engine, deniedStart);
run(crossing.engine, crossing.child, 10, deniedStart, exit(5, 0, 2, 0x5000, 1, 2), 'actual word crossing into write-only page', narrow(2, 1, 0, 2, 0x5000, 1));
assert.equal(crossing.engine.api.protect(0x5000, 1, 1), 0);
const crossingSuccess = copyState(deniedStart); crossingSuccess.registers[0] = 0x180; crossingSuccess.eip += 3;
run(crossing.engine, crossing.child, 1, crossingSuccess, exit(1, 1), 'actual repaired word resumes same EA once across page', narrow(2, 0, 0x180));
continuations++;

{
  const fixture = forms[1], {engine, child} = fixtureEngine(fixture);
  assert.equal(engine.api.map(0, 1, 3), 0); upload(engine, 0, Uint8Array.from([0x80]));
  const start = state(); start.registers[3] = 0xffffffff; start.registers[6] = 1;
  const expected = copyState(start); expected.registers[1] = 0xffffff80; expected.eip += 5;
  writeState(engine, start);
  run(engine, child, 1, expected, exit(1, 1), 'actual old-EA modulo wrap before narrow request', narrow(1, 0, 0x80));
}

function directRead(engine, width, address, record, status = 0) {
  const before = arena(engine);
  assert.equal(engine.api[width === 1 ? 'read8' : 'read16'](address), status, `actual direct read${width * 8}/${address}`);
  helperProbes++;
  const after = arena(engine);
  assert.deepEqual(after.slice(0, 100), before.slice(0, 100), 'direct narrow read preserves CPU/exit/cancel');
  assert.deepEqual(after.slice(140), before.slice(140), 'direct narrow read preserves transfer');
  assert.deepEqual(after.slice(100, 140), status === 0 ? record : before.slice(100, 140), 'direct narrow helper canonical output/lifecycle purity');
}
{
  const {engine, child} = boundary.get('zx_word');
  for (const [width, address, record] of [[1, 0x4fff, narrow(1, 0, 0x80)], [2, 0x4ffe, narrow(2, 0, 0x8000)], [1, 0xffffffff, narrow(1, 0, 0x80)], [2, 0xfffffffe, narrow(2, 0, 0x8000)], [2, 0xffffffff, narrow(2, 1, 0, 3, 0xffffffff, 1)], [2, 0x4fff, narrow(2, 1, 0, 1, 0x5000, 1)]]) directRead(engine, width, address, record);
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'direct reads preserve compiled snapshot');
  upload(engine, 0x1000, assembled.get('zx_word').bytes);
  let before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 4, 'stale narrow module guard precedes malformed pointers'); actualRuns++;
  assert.deepEqual(arena(engine), before, 'stale guard rejection preserves complete arena');
  directRead(engine, 1, 0x4fff, narrow(1, 0, 0x80));
  assert.equal(engine.api.close(), 0);
  for (const width of [1, 2]) directRead(engine, width, 0x4fff, undefined, 5);
  before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 5, 'closed narrow module guard precedes pointers'); actualRuns++;
  assert.deepEqual(arena(engine), before);
}

{
  const engine = fresh(), bytes = assembled.get('prefix_fault').bytes;
  const child = compile(engine, bytes, undefined, ['read16']);
  const start = state(); start.registers[3] = 0xfffffff8; start.registers[1] = 3; start.registers[6] = 0x9000;
  const stopped = copyState(start); stopped.registers[0] = 3; stopped.eip = 0x1004;
  writeState(engine, start);
  run(engine, child, 0, start, exit(1, 0), 'budget0 before narrow helper/prefix', arena(engine).slice(100, 140));
  run(engine, child, 10, stopped, exit(5, 1, 1, 0x9000, 1, 2), 'committed LEA prefix survives precise word fault', narrow(2, 1, 0, 1, 0x9000, 1)); continuations++;
  refresh(engine).view.setUint32(engine.base + 96, 1, true);
  run(engine, child, 0, stopped, exit(2, 0), 'cancel outranks zero budget at failed extension', narrow(2, 1, 0, 1, 0x9000, 1)); continuations++;
  engine.view.setUint32(engine.base + 96, 0, true);
  run(engine, child, 1, stopped, exit(5, 0, 1, 0x9000, 1, 2), 'unrepaired fault retry retires no prefix again', narrow(2, 1, 0, 1, 0x9000, 1)); continuations++;
  assert.equal(engine.api.map(0x9000, 1, 3), 0); upload(engine, 0x9000, Uint8Array.from([0, 0x80]));
  const repaired = copyState(stopped); repaired.registers[2] = 0xffff8000; repaired.eip = 0x1007;
  run(engine, child, 1, repaired, exit(1, 1), 'budget1 repaired extension commits destination once', narrow(2, 0, 0x8000)); continuations++;
  const next = copyState(repaired); next.eip++;
  run(engine, child, 1, next, exit(1, 1), 'interior successor resumes after extension', narrow(2, 0, 0x8000)); continuations++;
  run(engine, child, 1, next, exit(3, 0), 'unknown continuation retires no extra instruction', narrow(2, 0, 0x8000)); continuations++;
}

{
  const engine = fresh(); assert.equal(engine.api.map(0x4000, 1, 3), 0);
  upload(engine, 0x1200, assembled.get('composition').bytes); upload(engine, 0x1500, assembled.get('composition_done').bytes);
  const child = compile(engine, new Uint8Array(), [[0x1200, 24], [0x1500, 2]], ['read8', 'read16'], [[0x1500, 308]]);
  for (const [byte, signedByte, word, tail, literal, flags] of [[0, 0, 0, 0, 0, 0x446], [255, 128, 0x80ff, 0xffff, 0xffff8080, 0x482], [128, 255, 0xff80, 0x8000, 0xffffffff, 0x486], [255, 127, 0x7fff, 0x7fff, 0x7e81, 0x406]]) {
    const convertedByte = BigInt(signedByte >= 128 ? signedByte - 256 : signedByte), convertedWord = BigInt(word >= 32768 ? word - 65536 : word);
    const result = Number(BigInt.asUintN(32, BigInt(byte) + convertedByte) ^ BigInt.asUintN(32, convertedWord));
    assert.equal(result, literal, 'resident composition independent arithmetic/literal agreement');
    upload(engine, 0x4000, Uint8Array.from([byte, signedByte, ...valueBytes(2, word), ...valueBytes(2, tail)]));
    const start = state(0x1200); start.registers[6] = 0x4000;
    const expected = copyState(start); expected.registers[0] = literal; expected.registers[2] = Number(BigInt.asUintN(32, convertedByte)); expected.registers[3] = Number(BigInt.asUintN(32, convertedWord)); expected.registers[7] = tail; expected.eflags = flags; expected.eip = 0x1500;
    writeState(engine, start);
    run(engine, child, 8, expected, exit(8, 7, 308, 0, 0, 0, 3), `actual resident narrow input composition ${byte}/${signedByte}/${word}`, narrow(2, 0, tail));
  }
}

function changed(record, offset, value, size = 4) {
  const result = record.slice(), view = new DataView(result.buffer);
  if (size === 2) view.setUint16(offset, value, true);
  else view.setUint32(offset, value, true);
  return result;
}
function scriptedCases(width) {
  const success = narrow(width, 0, width === 1 ? 255 : 65535), fault = narrow(width, 1, 0, 1, 0x9000, 1), infrastructure = narrow(width, 2, 0, 1);
  const protocol = exit(7, 1, 2), rejected = exit(7, 1, 3);
  const cases = [
    {label: 'canonical success exact unsigned maximum', record: success, success: true},
    {label: 'canonical unmapped request byte', record: fault, expected: exit(5, 1, 1, 0x9000, 1, width)},
    {label: 'canonical infrastructure version exhaustion', record: infrastructure, expected: exit(7, 1, 1)},
    {label: 'canonical other memory infrastructure', record: narrow(width, 2, 0, 2), expected: rejected},
    ...[[0, 0, 4, 'magic'], [4, 1, 2, 'version'], [6, 2, 2, 'profile'], [8, 39, 4, 'length'], [12, 1, 4, 'reserved']].map(([offset, value, size, label]) => ({label: `invalid v2 ${label}`, record: changed(success, offset, value, size)})),
    ...[[success, 'success'], [fault, 'fault'], [infrastructure, 'infrastructure']].map(([record, label]) => ({label: `wrong requested width on ${label}`, record: changed(record, 36, width === 1 ? 2 : 1)})),
    {label: 'legacy width4 is excluded from narrow result', record: changed(success, 36, 4)},
    {label: 'value exceeds requested width', record: changed(success, 20, width === 1 ? 256 : 65536)},
    ...[[24, 'detail'], [28, 'fault byte'], [32, 'access']].map(([offset, label]) => ({label: `nonzero success ${label}`, record: changed(success, offset, 1)})),
    {label: 'unknown result tag', record: changed(success, 16, 3)},
    {label: 'fault value must be zero', record: changed(fault, 20, 1)},
    {label: 'unknown fault detail', record: changed(fault, 24, 4)},
    {label: 'fault access must be read', record: changed(fault, 32, 2)},
    {label: 'fault byte precedes request', record: changed(fault, 28, 0x8fff)},
    {label: 'fault byte is after requested span', record: changed(fault, 28, 0x9000 + width)},
    {label: 'false request overflow', record: narrow(width, 1, 0, 3, 0x9000, 1)},
    ...[[20, 'value'], [28, 'fault byte'], [32, 'access']].map(([offset, label]) => ({label: `nonzero infrastructure ${label}`, record: changed(infrastructure, offset, 1)})),
    {label: 'unknown infrastructure detail', record: changed(infrastructure, 24, 3)},
    {label: 'read lifecycle rejection is terminal infrastructure', status: 5, record: success, expected: rejected},
    {label: 'read infrastructure host rejection', status: 9, record: success, expected: rejected},
    {label: 'store committed status11 is rejected before parsing malformed read result', status: 11, record: changed(success, 0, 0), expected: rejected},
  ];
  if (width === 2) cases.push(
    {label: 'terminal fault byte lies in nonoverflowing request', address: 0xfffffffe, record: narrow(2, 1, 0, 1, 0xffffffff, 1), expected: exit(5, 1, 1, 0xffffffff, 1, 2)},
    {label: 'true request overflow', address: 0xffffffff, record: narrow(2, 1, 0, 3, 0xffffffff, 1), expected: exit(5, 1, 3, 0xffffffff, 1, 2)},
    {label: 'overflowing request cannot report success', address: 0xffffffff, record: narrow(2, 0, 0)},
    {label: 'overflowing request cannot report unmapped', address: 0xffffffff, record: narrow(2, 1, 0, 1, 0xffffffff, 1)},
    {label: 'overflow fault byte must equal original request', address: 0xffffffff, record: narrow(2, 1, 0, 3, 0xfffffffe, 1)},
  );
  return cases.map(test => ({status: 0, address: 0x9000, expected: protocol, ...test}));
}

const scriptedEvidence = [];
for (const width of [1, 2]) {
  const engine = fresh(), actual = compile(engine, assembled.get(width === 1 ? 'scripted_byte' : 'scripted_word').bytes, undefined, [width === 1 ? 'read8' : 'read16']);
  const injector = new WebAssembly.Instance(injectorModule, {env: {memory: engine.memory}, ring3: {guard: engine.api.guard}});
  const injected = new WebAssembly.Instance(actual.module, {env: {memory: engine.memory}, ring3: {guard: injector.exports.guard, read8: injector.exports.read8, read16: injector.exports.read16}});
  const child = {...actual, run: injected.exports.run};
  const cases = scriptedCases(width);
  for (const test of cases) {
    const start = state(); start.registers[6] = test.address;
    const expected = copyState(start); expected.registers[2] = 0x12345678; expected.eip = 0x1005;
    if (test.success) { expected.registers[0] = 0xffffffff; expected.eip = 0x1008; }
    writeState(engine, start);
    engine.bytes.set(test.record, engine.base + 140);
    injector.exports.configure(engine.base + 100, engine.base + 140, test.status);
    const copied = test.status === 0 || test.status === 11;
    run(engine, child, test.success ? 2 : 20, expected, test.success ? exit(1, 2) : test.expected, `pure-Wasm scripted read${width * 8}: ${test.label}`, copied ? test.record : undefined, true);
    assert.equal(injector.exports.calls(), 1, 'scripted helper receives one request');
    assert.equal(injector.exports.address() >>> 0, test.address, 'scripted helper receives old effective address');
    assert.equal(engine.api.guard(engine.low, engine.high, actual.generation, engine.base, engine.base + 56, engine.base + 96), 0, 'scripted helper does not change actual artifact snapshot');
    scriptedEvidence.push({width, label: test.label, status: test.status, address: test.address});
  }
  for (const cancel of [0, 1]) {
    const start = state(); start.registers[6] = 0x9000;
    writeState(engine, start, cancel);
    injector.exports.configure(engine.base + 100, engine.base + 140, 0);
    run(engine, child, 0, start, exit(cancel ? 2 : 1, 0), `pure-Wasm scripted read${width * 8}: cancel/budget before helper`, undefined, true);
    assert.equal(injector.exports.calls(), 0, 'pre-instruction stop does not call helper');
  }
}

const provenance = {
  claim: 'Own embedded baseline Wasm memory-source MOVZX/MOVSX32 with actual engine read8/read16 and precise width1/2 faults; scripted pure-Wasm helper protocol evidence is separate. No host helper callbacks, intermediate host CPU edits, hardware flags, performance or full P2-V0 claim.',
  generated_modules: modules, actual_engine_cpu_calls: actualRuns, actual_engine_direct_helper_calls: helperProbes, continuation_calls: continuations, pure_wasm_scripted_cpu_calls: injectedRuns,
  llvm_symbols: llvm, scripted_cases: scriptedEvidence,
  engine_sha256: hash(engineBytes), helper_injector_sha256: hash(injectorBytes), module_set_sha256: hash(generatedHashes.join('\n')),
  sha256: Object.fromEntries(['integer.S', 'run.mjs'].map(name => [name, hash(readFileSync(join(fixtureRoot, name)))]).concat([['dbt_memory_extensions_wasm.rs', hash(readFileSync(join(root, 'engine/tests/dbt_memory_extensions_wasm.rs')))], ['integer.o', hash(object)]])),
  versions: {clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0], objdump: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).split('\n')[0], node: process.version},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]},
  generated_sha256: generatedHashes,
};
writeFileSync(join(outputDir, 'provenance.json'), `${JSON.stringify(provenance, null, 2)}\n`);
console.log(JSON.stringify({modules, actual_engine_cpu_calls: actualRuns, actual_engine_direct_helper_calls: helperProbes, continuation_calls: continuations, pure_wasm_scripted_cpu_calls: injectedRuns, llvm_symbols: llvm.length, engine_sha256: provenance.engine_sha256, module_set_sha256: provenance.module_set_sha256, artifacts: outputDir}));

