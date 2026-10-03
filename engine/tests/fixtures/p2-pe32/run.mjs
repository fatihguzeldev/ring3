import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-pe32');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json'));
const oracle = JSON.parse(oracleBytes);
const size = 4236, transfer = 140, stack = 0x70000000;
const observations = [], artifacts = {};

function elfPrograms(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]);
  assert.equal(view.getUint16(18, true), 3, 'LLVM i386 object');
  const offset = view.getUint32(32, true), stride = view.getUint16(46, true);
  const sections = Array.from({length: view.getUint16(48, true)}, (_, index) => {
    const at = offset + index * stride;
    return {type: view.getUint32(at + 4, true), offset: view.getUint32(at + 16, true), size: view.getUint32(at + 20, true), link: view.getUint32(at + 24, true), info: view.getUint32(at + 28, true), stride: view.getUint32(at + 36, true)};
  });
  const table = sections.find(section => section.type === 2);
  assert.ok(table, 'authored ELF symbols');
  const strings = sections[table.link], programs = new Map();
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    const nameAt = strings.offset + view.getUint32(at, true);
    const name = object.subarray(nameAt, object.indexOf(0, nameAt)).toString('utf8');
    const section = view.getUint16(at + 14, true), start = view.getUint32(at + 4, true), length = view.getUint32(at + 8, true);
    if (section > 0 && section < sections.length && length > 0) {
      const source = sections[section];
      assert.ok(start + length <= source.size);
      assert.ok(!sections.some(item => [4, 9].includes(item.type) && item.info === section && item.size > 0), 'authored code has no relocations');
      programs.set(name, object.subarray(source.offset + start, source.offset + start + length));
    }
  }
  return programs;
}

// this creates authored files; execution below consumes only the Rust receipt.
function pe32(name, code) {
  const data = name === 'data_bss', expected = oracle[name].metadata;
  const bytes = Buffer.alloc(data ? 1536 : 1024), view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  bytes.write('MZ'); view.setUint32(0x3c, 0x80, true); bytes.write('PE\0\0', 0x80);
  view.setUint16(0x84, 0x14c, true); view.setUint16(0x86, data ? 2 : 1, true);
  view.setUint16(0x94, 224, true); view.setUint16(0x96, 0x103, true);
  const optional = 0x98;
  view.setUint16(optional, 0x10b, true);
  for (const [at, value] of [[4, 512], [8, data ? 512 : 0], [12, data ? 4 : 0], [16, 0x1000], [20, 0x1000], [24, data ? 0x3000 : 0], [28, expected.image_base], [32, 4096], [36, 512], [56, expected.image_size], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16]]) view.setUint32(optional + at, value, true);
  view.setUint16(optional + 40, 4, true); view.setUint16(optional + 48, 4, true);
  view.setUint16(optional + 68, 3, true); view.setUint16(optional + 70, 0x100, true);
  function section(at, title, virtualSize, rva, rawPointer, permissions) {
    bytes.write(title, at);
    for (const [offset, value] of [[8, virtualSize], [12, rva], [16, 512], [20, rawPointer], [36, permissions]]) view.setUint32(at + offset, value, true);
  }
  section(0x178, '.text', code.length, 0x1000, 512, 0x60000020);
  bytes.fill(0xcc, 512, 1024); bytes.set(code, 512);
  if (data) {
    section(0x1a0, '.data', 516, 0x3000, 1024, 0xc0000040);
    view.setUint32(1024, 37, true); view.setUint32(1532, 0xdeadbeef, true);
  }
  assert.ok(bytes.length <= 4096, 'each authored image fits the existing transfer');
  return bytes;
}

const assembly = join(fixtureRoot, 'programs.S'), objectPath = join(outputDir, 'programs.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
writeFileSync(join(outputDir, 'programs.disassembly.txt'), execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]));
const assembled = elfPrograms(readFileSync(objectPath)), images = new Map();
assert.equal(assembled.size, 2);
for (const name of ['data_bss', 'bounded_loop']) {
  const code = assembled.get(name);
  assert.equal(code.toString('hex'), oracle[name].code_hex, `${name}: independently frozen encoding`);
  const image = pe32(name, code);
  images.set(name, image); writeFileSync(join(outputDir, `${name}.exe`), image);
  artifacts[`${name}.exe`] = hash(image);
}

// capture authored expectations and source fingerprints before engine runtime.
writeFileSync(join(outputDir, 'authored-oracle.json'), oracleBytes);
writeFileSync(join(outputDir, 'programs.S'), readFileSync(assembly));
writeFileSync(join(outputDir, 'run.mjs'), readFileSync(import.meta.filename));
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', 'engine/tests/process_pe32_wasm.rs', 'engine/tests/fixtures/p2-pe32/programs.S', 'engine/tests/fixtures/p2-pe32/oracle.json', 'engine/tests/fixtures/p2-pe32/run.mjs'];
const sources = Object.fromEntries(sourcePaths.map(file => [file, hash(readFileSync(join(root, file)))]));
writeFileSync(join(outputDir, 'source-sha256.json'), JSON.stringify(sources, null, 2));
const engineBytes = readFileSync(enginePath);
writeFileSync(join(outputDir, 'engine.wasm'), engineBytes);
assert.ok(WebAssembly.validate(engineBytes), 'actual engine validates');
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), [], 'actual engine has no host imports');
const names = ['open', 'close', 'arena_ptr', 'load_pe32', 'map', 'compile_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'store32'];
let nextKey = 0n;

function refresh(engine) {
  if (engine.buffer !== engine.memory.buffer) {
    engine.buffer = engine.memory.buffer;
    engine.bytes = new Uint8Array(engine.buffer); engine.view = new DataView(engine.buffer);
  }
  return engine;
}
function fresh(pages = 8) {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => {
    const fn = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof fn, 'function', `actual export ${name}`);
    return [name, fn];
  }));
  assert.equal(api.load_pe32.length, 1);
  const memory = instance.exports.memory, key = 0xa338000000000000n + ++nextKey;
  assert.ok(memory instanceof WebAssembly.Memory);
  assert.equal(api.open(pages, Number(key & 0xffffffffn), Number(key >> 32n)), 0);
  const engine = refresh({api, memory, base: api.arena_ptr() >>> 0});
  assert.ok(engine.base > 0 && engine.base + size <= engine.bytes.length);
  engine.bytes.fill(0xa5, engine.base, engine.base + size);
  return engine;
}
function arena(engine) { return refresh(engine).bytes.slice(engine.base, engine.base + size); }
function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true));
  return bytes;
}
function operation(engine, label, action, status = 0, patches = []) {
  const expected = arena(engine);
  for (const [offset, bytes] of patches) expected.set(bytes, offset);
  assert.equal(action(), status, `${label}: status`);
  assert.deepEqual(arena(engine), expected, `${label}: complete arena including State/Exit/helper/cancel/transfer`);
}
function request(engine, bytes) { refresh(engine).bytes.set(bytes, engine.base + transfer); }
function load(engine, name) {
  const image = images.get(name), metadata = oracle[name].metadata;
  request(engine, image);
  operation(engine, `${name} image load`, () => engine.api.load_pe32(image.length), 0, [[transfer, record('R3PE', 32, Object.values(metadata))]]);
  const view = new DataView(arena(engine).buffer, transfer, 32);
  const receipt = {image_base: view.getUint32(16, true), image_size: view.getUint32(20, true), entry_point: view.getUint32(24, true), mapped_pages: view.getUint32(28, true)};
  assert.deepEqual(receipt, metadata, `${name}: Rust metadata receipt`);
  assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_len(), 0);
  return receipt;
}
function read(engine, address, value, label) {
  operation(engine, label, () => engine.api.read32(address), 0, [[100, record('R3MH', 40, [0, value, 0, 0, 0, 0])]]);
}
function fault(engine, address, access, detail, label) {
  const call = access === 1 ? () => engine.api.read32(address) : () => engine.api.write32(address, 0x11223344);
  operation(engine, label, call, 0, [[100, record('R3MH', 40, [1, 0, detail, address, access, 4])]]);
}
function descriptors(engine, entries) {
  const view = refresh(engine).view;
  entries.forEach((entry, index) => view.setUint32(engine.base + transfer + index * 4, entry, true));
}
function rejectExecute(engine, address, label) {
  descriptors(engine, [address]);
  operation(engine, label, () => engine.api.compile_entries(1, 0), 10);
  assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_len(), 0);
}
function compile(engine, name, receipt) {
  // extra seeds come from authored source labels, never PE-header inspection.
  descriptors(engine, oracle[name].entry_offsets.map(offset => receipt.entry_point + offset));
  operation(engine, `${name} compile from receipt entry`, () => engine.api.compile_entries(oracle[name].entry_offsets.length, 0));
  assert.equal(engine.api.generation(), 1);
  const pointer = engine.api.module_ptr() >>> 0, length = engine.api.module_len() >>> 0;
  refresh(engine); assert.ok(length > 8 && pointer + length <= engine.bytes.length);
  const bytes = engine.bytes.slice(pointer, pointer + length), module = new WebAssembly.Module(bytes);
  const helpers = name === 'data_bss' ? ['read32', 'store32'] : [];
  const expectedImports = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard', kind: 'function'}, ...helpers.map(helper => ({module: 'ring3', name: helper, kind: 'function'}))];
  const sort = imports => imports.sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sort(WebAssembly.Module.imports(module)), sort(expectedImports), `${name}: exact needed imports`);
  const ring3 = {guard: engine.api.guard};
  for (const helper of helpers) ring3[helper] = engine.api[helper];
  const run = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3}).exports.run;
  assert.equal(typeof run, 'function'); assert.equal(run.length, 4);
  writeFileSync(join(outputDir, `${name}.wasm`), bytes); artifacts[`${name}.wasm`] = hash(bytes);
  return run;
}
function startup(engine, receipt) {
  operation(engine, 'caller explicitly maps stack', () => engine.api.map(stack, 1, 3));
  const before = arena(engine);
  before.set(record('R3ST', 56, [...oracle.startup.registers, receipt.entry_point, oracle.startup.eflags]), 0);
  before.fill(0, 96, 100);
  // the sole host CPU initialization; every later state change is emitted x86.
  refresh(engine).bytes.set(before, engine.base);
}
function runProgram(engine, name, run) {
  let retired = 0;
  for (const expected of oracle[name].runs) {
    const patches = [[0, record('R3ST', 56, [...expected.registers, expected.eip, expected.eflags])], [56, record('R3EX', 40, [expected.reason, expected.retired, 0, 0, 0, 0], expected.exit_version)]];
    if (name === 'data_bss') patches.push([100, record('R3MH', 40, [0, 0, 0, 0, 0, 0])]);
    operation(engine, `${name} budget ${expected.budget}`, () => run(engine.base, engine.base + 56, expected.budget, engine.base + 96), 0, patches);
    retired += expected.retired;
    observations.push({program: name, budget: expected.budget, registers: expected.registers, eip: expected.eip, eflags: expected.eflags, reason: expected.reason, retired: expected.retired});
  }
  assert.equal(retired, oracle[name].total_retired);
}

for (const name of ['data_bss', 'bounded_loop']) {
  const engine = fresh(), receipt = load(engine, name), base = receipt.image_base;
  read(engine, base, 0x5a4d, `${name}: mapped DOS header`);
  fault(engine, base, 2, 2, `${name}: headers are read-only`);
  fault(engine, receipt.entry_point, 2, 2, `${name}: text is read-only`);
  read(engine, receipt.entry_point + 256, 0xcccccccc, `${name}: entire raw padding copied`);
  fault(engine, base + 0x2000, 1, 1, `${name}: image gap unmapped`);
  rejectExecute(engine, base, `${name}: headers cannot execute`);
  if (name === 'data_bss') {
    read(engine, oracle[name].initialized_word.address, 37, 'initialized data before guest');
    read(engine, oracle[name].bss_word.address, 0, 'BSS zero before guest');
    read(engine, base + 0x31fc, 0xdeadbeef, 'initialized raw end before BSS');
    read(engine, base + 0x3204, 0, 'mapped section tail zero');
    fault(engine, base + 0x4000, 1, 1, 'trailing image page unmapped');
    rejectExecute(engine, base + 0x3000, 'RW data cannot execute');
  }
  const run = compile(engine, name, receipt);
  startup(engine, receipt); runProgram(engine, name, run);
  if (name === 'data_bss') {
    read(engine, oracle[name].initialized_word.address, 37, 'initialized data retained');
    read(engine, oracle[name].bss_word.address, 42, 'guest wrote loader-zeroed BSS');
    read(engine, base + 0x3204, 0, 'guest store preserves adjacent zero tail');
  }
  request(engine, images.get(name));
  operation(engine, `${name}: load replay rejected`, () => engine.api.load_pe32(images.get(name).length), 7);
  operation(engine, `${name}: close preserves arena`, () => engine.api.close());
  operation(engine, `${name}: closed wins oversized length`, () => engine.api.load_pe32(4097), 5);
}

const valid = images.get('data_bss');
const malformed = valid.subarray(0, 63), unsupported = Buffer.from(valid);
unsupported.writeUInt16LE(0x8664, 0x84);
for (const [label, pages, bytes, length, status, setup] of [
  ['malformed', 8, malformed, malformed.length, oracle.abi_errors.malformed],
  ['unsupported', 8, unsupported, unsupported.length, oracle.abi_errors.unsupported],
  ['capacity', 2, valid, valid.length, oracle.abi_errors.capacity],
  ['non_pristine', 8, valid, valid.length, oracle.abi_errors.non_pristine, engine => {
    operation(engine, 'preexisting RAM mapping', () => engine.api.map(stack, 1, 3));
    operation(engine, 'preexisting RAM word', () => engine.api.write32(stack, 0x12345678), 0, [[100, record('R3MH', 40, [0, 0, 0, 0, 0, 0])]]);
  }],
  ['oversized_transfer', 8, valid, 4097, oracle.abi_errors.oversized_transfer],
  ['closed', 8, valid, valid.length, oracle.abi_errors.closed, engine => operation(engine, 'close fresh engine', () => engine.api.close())],
]) {
  const engine = fresh(pages);
  if (setup) setup(engine);
  request(engine, bytes);
  operation(engine, `rejected ${label} load`, () => engine.api.load_pe32(length), status);
  if (label === 'non_pristine') read(engine, stack, 0x12345678, 'rejected load retains preexisting RAM');
  if (['malformed', 'unsupported', 'capacity', 'oversized_transfer'].includes(label)) {
    const name = label === 'capacity' ? 'bounded_loop' : 'data_bss';
    load(engine, name);
    read(engine, oracle[name].metadata.image_base, 0x5a4d, `valid follow-on after ${label}`);
  }
  observations.push({rejection: label, status});
}

const provenance = {
  engine_sha256: hash(engineBytes), oracle_sha256: hash(oracleBytes), artifacts, sources, observations,
  command: [process.execPath, process.argv[1], enginePath, outputDir, root], assembly_command: ['clang', ...assemble],
  tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0]},
  claim: 'two independently authored fixed-base import-free PE32 programs loaded by Rust and executed through actual emitted Wasm from returned entry. Literal CPU/Exit/data/retirement oracles precede product implementation; LLVM source-derived code verifies exact bytes. JS creates PE fixtures but never parses PE for execution. Caller explicitly maps stack and writes CPU once; no CPU patches after startup. Complete arena sentinels cover success receipt and all error returns. No imports, relocation, TLS, CRT, Windows startup, process Exit, arbitrary PE, browser, SDK, game or full P2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({status: 'ok', engine_sha256: provenance.engine_sha256, oracle_sha256: provenance.oracle_sha256, programs: ['data_bss', 'bounded_loop'], retired: [5, 12], output: outputDir}));
