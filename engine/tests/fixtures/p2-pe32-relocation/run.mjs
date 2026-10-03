import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-pe32-relocation');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json')), oracle = JSON.parse(oracleBytes);
const size = 4236, transfer = 140, stack = 0x70000000;
const observations = [], artifacts = {};

function authoredCode(object) {
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
      assert.ok(!sections.some(item => [4, 9].includes(item.type) && item.info === section && item.size > 0), 'authored code has no ELF relocations');
      programs.set(name, object.subarray(source.offset + start, source.offset + start + length));
    }
  }
  assert.equal(programs.size, 1);
  return programs.get('relocated_data');
}

// fixture construction is independent of the runtime loader and its receipt.
function authoredPe(code) {
  const bytes = Buffer.alloc(2048), view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  bytes.write('MZ'); view.setUint32(0x3c, 0x80, true); bytes.write('PE\0\0', 0x80);
  view.setUint16(0x84, 0x14c, true); view.setUint16(0x86, 3, true);
  view.setUint16(0x94, 224, true); view.setUint16(0x96, 0x102, true);
  const optional = 0x98;
  view.setUint16(optional, 0x10b, true);
  for (const [at, value] of [[4, 512], [8, 1024], [12, 4], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, oracle.preferred_base], [32, 4096], [36, 512], [56, oracle.image_size], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16], [136, 0x4000], [140, 28]]) view.setUint32(optional + at, value, true);
  view.setUint16(optional + 40, 4, true); view.setUint16(optional + 48, 4, true);
  view.setUint16(optional + 68, 3, true); view.setUint16(optional + 70, 0x100, true);
  function section(at, name, virtualSize, rva, rawPointer, flags) {
    bytes.write(name, at);
    for (const [offset, value] of [[8, virtualSize], [12, rva], [16, 512], [20, rawPointer], [36, flags]]) view.setUint32(at + offset, value, true);
  }
  section(0x178, '.text', code.length, 0x1000, 512, 0x60000020);
  section(0x1a0, '.data', 516, 0x3000, 1024, 0xc0000040);
  section(0x1c8, '.fixups', 28, 0x4000, 1536, 0x40000040);
  bytes.fill(0xcc, 512, 1024); bytes.set(code, 512);
  view.setUint32(1024, 0x00403004, true); view.setUint32(1028, 37, true);
  bytes.set(Buffer.from(oracle.directory_hex, 'hex'), 1536);
  assert.equal(bytes.subarray(1536, 1564).toString('hex'), oracle.directory_hex);
  assert.ok(bytes.length <= 4096);
  return bytes;
}

const assembly = join(fixtureRoot, 'program.S'), objectPath = join(outputDir, 'program.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
writeFileSync(join(outputDir, 'program.disassembly.txt'), execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]));
const code = authoredCode(readFileSync(objectPath));
assert.equal(code.toString('hex'), oracle.code_hex, 'LLVM matches independently frozen code');
const image = authoredPe(code);
writeFileSync(join(outputDir, 'relocated-data.exe'), image); artifacts['relocated-data.exe'] = hash(image);
writeFileSync(join(outputDir, 'authored-oracle.json'), oracleBytes);
writeFileSync(join(outputDir, 'program.S'), readFileSync(assembly));
writeFileSync(join(outputDir, 'run.mjs'), readFileSync(import.meta.filename));
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', 'engine/tests/process_pe32_relocation_wasm.rs', 'engine/tests/fixtures/p2-pe32-relocation/program.S', 'engine/tests/fixtures/p2-pe32-relocation/oracle.json', 'engine/tests/fixtures/p2-pe32-relocation/run.mjs'];
const sources = Object.fromEntries(sourcePaths.map(file => [file, hash(readFileSync(join(root, file)))]));
writeFileSync(join(outputDir, 'source-sha256.json'), JSON.stringify(sources, null, 2));
const engineBytes = readFileSync(enginePath);
writeFileSync(join(outputDir, 'engine.wasm'), engineBytes);
assert.ok(WebAssembly.validate(engineBytes), 'actual engine validates');
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), [], 'actual engine has no host imports');
const names = ['open', 'close', 'arena_ptr', 'load_pe32', 'load_pe32_at', 'map', 'compile_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'store32'];
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
  assert.equal(api.load_pe32.length, 1); assert.equal(api.load_pe32_at.length, 2);
  const memory = instance.exports.memory, key = 0xa339000000000000n + ++nextKey;
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
  assert.deepEqual(arena(engine), expected, `${label}: whole arena including State/Exit/helper/cancel/transfer`);
}
function request(engine, bytes = image) { refresh(engine).bytes.set(bytes, engine.base + transfer); }
function load(engine, example) {
  request(engine);
  const metadata = [example.base, oracle.image_size, example.entry, oracle.mapped_pages];
  operation(engine, `${example.name}: load at requested base`, () => engine.api.load_pe32_at(image.length, example.base), 0, [[transfer, record('R3PE', 32, metadata)]]);
  const view = new DataView(arena(engine).buffer, transfer, 32);
  const receipt = {image_base: view.getUint32(16, true), image_size: view.getUint32(20, true), entry_point: view.getUint32(24, true), mapped_pages: view.getUint32(28, true)};
  assert.deepEqual(Object.values(receipt), metadata);
  assert.equal(hash(image), artifacts['relocated-data.exe'], 'caller image bytes remain original');
  assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_len(), 0);
  return receipt;
}
function read(engine, address, value, label) {
  operation(engine, label, () => engine.api.read32(address), 0, [[100, record('R3MH', 40, [0, value, 0, 0, 0, 0])]]);
}
function fault(engine, address, access, detail, label) {
  const action = access === 1 ? () => engine.api.read32(address) : () => engine.api.write32(address, 0x11223344);
  operation(engine, label, action, 0, [[100, record('R3MH', 40, [1, 0, detail, address, access, 4])]]);
}
function entry(engine, address) { refresh(engine).view.setUint32(engine.base + transfer, address, true); }
function rejectExecute(engine, address, label) {
  entry(engine, address);
  operation(engine, label, () => engine.api.compile_entries(1, 0), 10);
  assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_len(), 0);
}
function compile(engine, example, receipt) {
  entry(engine, receipt.entry_point);
  operation(engine, `${example.name}: compile only returned entry`, () => engine.api.compile_entries(1, 0));
  assert.equal(engine.api.generation(), 1);
  const pointer = engine.api.module_ptr() >>> 0, length = engine.api.module_len() >>> 0;
  refresh(engine); assert.ok(length > 8 && pointer + length <= engine.bytes.length);
  const bytes = engine.bytes.slice(pointer, pointer + length), module = new WebAssembly.Module(bytes);
  const sort = imports => imports.sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sort(WebAssembly.Module.imports(module)), sort([{module: 'env', name: 'memory', kind: 'memory'}, ...['guard', 'read32', 'store32'].map(name => ({module: 'ring3', name, kind: 'function'}))]), 'only exact memory/guard/instruction imports');
  const run = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: {guard: engine.api.guard, read32: engine.api.read32, store32: engine.api.store32}}).exports.run;
  assert.equal(typeof run, 'function'); assert.equal(run.length, 4);
  writeFileSync(join(outputDir, `${example.name}.wasm`), bytes); artifacts[`${example.name}.wasm`] = hash(bytes);
  return run;
}
function startup(engine, receipt) {
  operation(engine, 'caller explicitly maps stack', () => engine.api.map(stack, 1, 3));
  // the only host CPU initialization; later CPU movement is emitted x86.
  refresh(engine).bytes.set(record('R3ST', 56, [...oracle.startup_registers, receipt.entry_point, oracle.startup_eflags]), engine.base);
  engine.bytes.fill(0, engine.base + 96, engine.base + 100);
}

for (const example of oracle.cases) {
  const engine = fresh();
  request(engine);
  operation(engine, `${example.name}: unchanged legacy directory refusal`, () => engine.api.load_pe32(image.length), oracle.abi_errors.legacy_nonzero_directory);
  const receipt = load(engine, example), base = receipt.image_base;
  read(engine, base + 0xb4, oracle.preferred_base, `${example.name}: original header retains preferred ImageBase`);
  for (let index = 0; index < oracle.target_rvas.length; index++) read(engine, base + oracle.target_rvas[index], example.patched_words[index], `${example.name}: literal HIGHLOW word ${index}`);
  read(engine, base + 0x3004, oracle.initialized_value, `${example.name}: initialized data before guest`);
  read(engine, base + 0x3200, oracle.initial_bss, `${example.name}: zero BSS before guest`);
  read(engine, base + 0x4000, 0x1000, `${example.name}: relocation directory retains original page RVA`);
  fault(engine, base, 2, 2, `${example.name}: header R`);
  fault(engine, receipt.entry_point + 2, 2, 2, `${example.name}: fixed RX code is read-only`);
  fault(engine, base + 0x4000, 2, 2, `${example.name}: ordinary R relocation section is read-only`);
  fault(engine, base + 0x2000, 1, 1, `${example.name}: image gap unmapped`);
  fault(engine, base + 0x5000, 1, 1, `${example.name}: trailing image page unmapped`);
  if (example.base !== oracle.preferred_base) for (const offset of [0, 0x1000, 0x3000]) fault(engine, oracle.preferred_base + offset, 1, 1, `${example.name}: preferred address ${offset} unmapped`);
  rejectExecute(engine, base, `${example.name}: headers cannot execute`);
  rejectExecute(engine, base + 0x3000, `${example.name}: RW data cannot execute`);
  const run = compile(engine, example, receipt);
  startup(engine, receipt);
  operation(engine, `${example.name}: actual emitted guest execution`, () => run(engine.base, engine.base + 56, oracle.budget, engine.base + 96), 0, [
    [0, record('R3ST', 56, [...example.registers, example.done, example.eflags])],
    [56, record('R3EX', 40, [oracle.reason, oracle.retired, 0, 0, 0, 0], oracle.exit_version)],
    [100, record('R3MH', 40, [0, 0, 0, 0, 0, 0])],
  ]);
  read(engine, base + 0x3004, oracle.initialized_value, `${example.name}: initialized value retained`);
  read(engine, base + 0x3200, oracle.final_bss, `${example.name}: emitted guest store proves final RW data`);
  read(engine, base + 0x3204, 0, `${example.name}: adjacent zero tail retained`);
  for (let index = 0; index < oracle.target_rvas.length; index++) read(engine, base + oracle.target_rvas[index], example.patched_words[index], `${example.name}: fixup word retained after guest`);
  observations.push({name: example.name, receipt, words: example.patched_words, registers: example.registers, eip: example.done, eflags: example.eflags, reason: oracle.reason, retired: oracle.retired});
  request(engine);
  operation(engine, `${example.name}: loaded image replay rejected`, () => engine.api.load_pe32_at(image.length, example.base), 7);
  operation(engine, `${example.name}: close preserves arena`, () => engine.api.close());
  operation(engine, `${example.name}: Closed first with invalid length/base`, () => engine.api.load_pe32_at(4097, 0), 5);
}

const malformed = Buffer.from(image), unsupported = Buffer.from(image);
malformed.writeUInt32LE(0, 1540); unsupported.writeUInt16LE(0x8002, 1544);
for (const [label, pages, bytes, length, base, status, setup] of [
  ['malformed_block', 8, malformed, malformed.length, 0x500000, oracle.abi_errors.malformed],
  ['invalid_base', 8, image, image.length, 0, oracle.abi_errors.malformed],
  ['unsupported_type', 8, unsupported, unsupported.length, 0x500000, oracle.abi_errors.unsupported],
  ['capacity', 3, image, image.length, 0x500000, oracle.abi_errors.capacity],
  ['non_pristine', 8, image, image.length, 0x500000, oracle.abi_errors.non_pristine, engine => {
    operation(engine, 'preexisting RAM mapping', () => engine.api.map(stack, 1, 3));
    operation(engine, 'preexisting RAM word', () => engine.api.write32(stack, 0x12345678), 0, [[100, record('R3MH', 40, [0, 0, 0, 0, 0, 0])]]);
  }],
  ['oversized_transfer', 8, image, 4097, 0x500000, oracle.abi_errors.oversized_transfer],
  ['closed', 8, image, image.length, 0x500000, oracle.abi_errors.closed, engine => operation(engine, 'close fresh engine', () => engine.api.close())],
]) {
  const engine = fresh(pages);
  if (setup) setup(engine);
  request(engine, bytes);
  operation(engine, `rejected ${label} selected-base load`, () => engine.api.load_pe32_at(length, base), status);
  if (label === 'non_pristine') read(engine, stack, 0x12345678, 'failed publication retains preexisting RAM');
  if (['malformed_block', 'invalid_base', 'unsupported_type', 'oversized_transfer'].includes(label)) {
    const receipt = load(engine, oracle.cases[1]);
    read(engine, receipt.image_base + 0x3000, 0x503004, `valid relocated follow-on after ${label}`);
  }
  observations.push({rejection: label, status});
}

const provenance = {
  engine_sha256: hash(engineBytes), oracle_sha256: hash(oracleBytes), artifacts, sources, observations,
  command: [process.execPath, process.argv[1], enginePath, outputDir, root], assembly_command: ['clang', ...assemble],
  tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0]},
  claim: 'one independently authored I386 HIGHLOW PE32 executed at preferred/up/down bases through actual emitted Wasm from the Rust-returned entry. Frozen literal instruction/fixup/register/data/Exit/retirement oracles precede product implementation; LLVM source verifies exact code. Host creates files but does not parse PE for execution, compute expected fixups from product values or patch CPU after its explicit startup. RX absolute operands and initialized data pointer are fixed by Rust; preferred addresses are unmapped in rebased instances. Entire arena sentinels cover old refusal, new receipt, malformed/unsupported/capacity/non-pristine/Closed and valid follow-on. No other relocation kinds, imports/provider, DLL, ASLR, TLS/CRT/Windows startup, SDK/browser/game/full P2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({status: 'ok', engine_sha256: provenance.engine_sha256, oracle_sha256: provenance.oracle_sha256, bases: oracle.cases.map(example => example.base), retired_each: oracle.retired, output: outputDir}));
