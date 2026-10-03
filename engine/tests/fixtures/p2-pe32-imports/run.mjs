import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-pe32-imports');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json')), oracle = JSON.parse(oracleBytes);
const size = 4236, transfer = 140;
const artifacts = {}, observations = [], identities = [];

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
      assert.ok(!sections.some(item => [4, 9].includes(item.type) && item.info === section && item.size > 0), 'literal FF15/A3 operands have no ELF relocations');
      programs.set(name, object.subarray(source.offset + start, source.offset + start + length));
    }
  }
  assert.equal(programs.size, 1);
  return programs.get('imported_windows_sequence');
}

// this authored layout is input construction, never a runtime pe parser or linker.
function authoredPe(code) {
  const bytes = Buffer.alloc(oracle.file_size), view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  bytes.write('MZ'); view.setUint32(0x3c, 0x80, true); bytes.write('PE\0\0', 0x80);
  view.setUint16(0x84, 0x14c, true); view.setUint16(0x86, 4, true);
  view.setUint16(0x94, 224, true); view.setUint16(0x96, 0x102, true);
  const optional = 0x98;
  view.setUint16(optional, 0x10b, true);
  for (const [at, value] of [[4, 512], [8, 1536], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, oracle.preferred_base], [32, 4096], [36, 512], [56, oracle.image_size], [60, oracle.header_size], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16]]) view.setUint32(optional + at, value, true);
  view.setUint16(optional + 40, 4, true); view.setUint16(optional + 48, 4, true);
  view.setUint16(optional + 68, 3, true); view.setUint16(optional + 70, 0x100, true);
  for (const [index, directory] of [[1, oracle.import_directory], [5, oracle.relocation_directory], [12, oracle.iat_directory]]) directory.forEach((value, word) => view.setUint32(optional + 96 + index * 8 + word * 4, value, true));
  oracle.sections.forEach((section, index) => {
    const at = 0x178 + index * 40;
    bytes.write(section.name, at);
    for (const [field, value] of [[8, section.virtual_size], [12, section.rva], [16, 512], [20, section.raw_pointer], [36, section.flags]]) view.setUint32(at + field, value, true);
  });
  bytes.fill(0xcc, 1024, 1536); bytes.set(code, 1024);
  oracle.import_descriptor.forEach((value, index) => view.setUint32(2048 + index * 4, value, true));
  for (const at of [2048 + 0x40, 2048 + 0x50]) oracle.original_thunks.forEach((value, index) => view.setUint32(at + index * 4, value, true));
  bytes.write(oracle.module_name, 2048 + 0x60, 'ascii');
  for (const item of oracle.hint_names) {
    const at = 2048 + item.rva - 0x4000;
    view.setUint16(at, item.hint, true); bytes.write(item.name, at + 2, 'ascii');
  }
  bytes.set(Buffer.from(oracle.relocation_hex, 'hex'), 2560);
  assert.equal(bytes.length, 3072); assert.ok(bytes.length <= 4096);
  assert.equal(bytes.subarray(2560, 2580).toString('hex'), oracle.relocation_hex);
  return bytes;
}

const assembly = join(fixtureRoot, 'program.S'), objectPath = join(outputDir, 'program.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
writeFileSync(join(outputDir, 'program.disassembly.txt'), execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]));
const code = authoredCode(readFileSync(objectPath));
assert.equal(code.toString('hex'), oracle.code_hex, 'LLVM matches frozen independent FF15/A3 encoding');
writeFileSync(join(outputDir, 'imported-windows.x86'), code); artifacts['imported-windows.x86'] = hash(code);
const image = authoredPe(code);
writeFileSync(join(outputDir, 'imported-windows.exe'), image); artifacts['imported-windows.exe'] = hash(image);
writeFileSync(join(outputDir, 'authored-oracle.json'), oracleBytes);
writeFileSync(join(outputDir, 'program.S'), readFileSync(assembly));
writeFileSync(join(outputDir, 'run.mjs'), readFileSync(import.meta.filename));
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', 'engine/tests/process_pe32_imports_wasm.rs', 'engine/tests/fixtures/p2-pe32-imports/program.S', 'engine/tests/fixtures/p2-pe32-imports/oracle.json', 'engine/tests/fixtures/p2-pe32-imports/run.mjs'];
const sources = Object.fromEntries(sourcePaths.map(file => [file, hash(readFileSync(join(root, file)))]));
writeFileSync(join(outputDir, 'source-sha256.json'), JSON.stringify(sources, null, 2));
const engineBytes = readFileSync(enginePath);
writeFileSync(join(outputDir, 'engine.wasm'), engineBytes);
assert.ok(WebAssembly.validate(engineBytes));
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), [], 'Rust PE linker and provider need no host imports');
const names = ['open', 'close', 'arena_ptr', 'load_pe32', 'load_pe32_at', 'load_pe32_linked_at', 'map', 'unmap', 'compile_entries', 'compile_with_gates', 'compile_resident_with_gates', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident', 'read32', 'write32', 'store32', 'store_resident32', 'capture_call', 'capture_resident_call', 'complete_windows_call', 'complete_resident_windows_call'];
let nextInstance = 0;

function refresh(engine) {
  if (engine.buffer !== engine.memory.buffer) {
    engine.buffer = engine.memory.buffer;
    engine.bytes = new Uint8Array(engine.buffer); engine.view = new DataView(engine.buffer);
  }
  return engine;
}
function arena(engine) { return refresh(engine).bytes.slice(engine.base, engine.base + size); }
function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true));
  return bytes;
}
function words(fields) {
  const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer);
  fields.forEach((value, index) => view.setUint32(index * 4, value, true));
  return bytes;
}
function cpu(registers, eip) { return record('R3ST', 56, [...registers, eip, oracle.eflags]); }
function exit(reason, retired, detail = 0, version = 3) { return record('R3EX', 40, [reason, retired, detail, 0, 0, 0], version); }
function helper(value = 0) { return record('R3MH', 40, [0, value, 0, 0, 0, 0]); }
function operation(engine, label, action, status = 0, patches = []) {
  const expected = arena(engine);
  for (const [offset, bytes] of patches) expected.set(bytes, offset);
  assert.equal(action(), status, `${label}: status`);
  assert.deepEqual(arena(engine), expected, `${label}: whole4236 State/Exit/helper/cancel/transfer arena`);
}
function fresh(pages = 8) {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => {
    const fn = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof fn, 'function', name); return [name, fn];
  }));
  assert.equal(api.load_pe32_linked_at.length, 3);
  assert.equal(api.complete_windows_call.length, 4); assert.equal(api.complete_resident_windows_call.length, 5);
  const ordinal = ++nextInstance, key = 0xa3410000c0000000n + BigInt(ordinal);
  const engine = {api, ordinal, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n), memory: instance.exports.memory};
  assert.ok(engine.memory instanceof WebAssembly.Memory);
  assert.equal(api.open(pages, engine.low, engine.high), 0);
  engine.base = api.arena_ptr() >>> 0; refresh(engine);
  assert.ok(engine.base > 0 && engine.base + size <= engine.bytes.length);
  engine.bytes.fill(0xa5, engine.base, engine.base + size);
  return engine;
}
function request(engine, bytes = image) { refresh(engine).bytes.set(bytes, engine.base + transfer); }
function load(engine, example) {
  request(engine);
  const metadata = [example.base, oracle.image_size, example.entry, oracle.mapped_pages, oracle.gate_base, 2, 0, 0, ...oracle.gates.flat()];
  operation(engine, `${example.name}: Rust loads, relocates and links`, () => engine.api.load_pe32_linked_at(image.length, example.base, oracle.gate_base), 0, [[transfer, record('R3LI', 64, metadata)]]);
  const view = new DataView(arena(engine).buffer, transfer, 64);
  const receipt = {image_base: view.getUint32(16, true), image_size: view.getUint32(20, true), entry_point: view.getUint32(24, true), mapped_pages: view.getUint32(28, true), gate_base: view.getUint32(32, true), gate_count: view.getUint32(36, true), gates: Array.from({length: view.getUint32(36, true)}, (_, index) => [view.getUint32(48 + index * 8, true), view.getUint32(52 + index * 8, true)])};
  assert.deepEqual(receipt.gates, oracle.gates);
  assert.equal(hash(image), artifacts['imported-windows.exe'], 'immutable caller PE bytes retained');
  assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_len(), 0);
  return receipt;
}
function read(engine, address, value, label) { operation(engine, label, () => engine.api.read32(address), 0, [[100, helper(value)]]); }
function fault(engine, address, access, detail, label) {
  const action = access === 1 ? () => engine.api.read32(address) : () => engine.api.write32(address, 0x11223344);
  operation(engine, label, action, 0, [[100, record('R3MH', 40, [1, 0, detail, address, access, 4])]]);
}
function stack(engine, values, label) { oracle.stack_addresses.forEach((address, index) => read(engine, address, values[index], `${label}: stack${index}`)); }
function rejectExecute(engine, address, label) {
  refresh(engine).view.setUint32(engine.base + transfer, address, true);
  operation(engine, label, () => engine.api.compile_entries(1, 0), 10);
  assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_len(), 0);
}
function inspectImage(engine, example, receipt) {
  const base = receipt.image_base;
  read(engine, base + 0xb4, oracle.preferred_base, 'header retains preferred ImageBase');
  read(engine, base + 0xa8, 0x1000, 'header retains entry RVA');
  oracle.target_rvas.forEach((rva, index) => read(engine, base + rva, example.patched_words[index], `literal HIGHLOW operand${index}`));
  for (const [label, offset, length] of [['descriptor and terminal', 0, 40], ['ILT', 0x40, 12], ['module string', 0x60, 16], ['Get Hint/name', 0x70, 16], ['Set Hint/name', 0x80, 16]]) {
    for (let at = 0; at < length; at += 4) read(engine, base + 0x4000 + offset + at, image.readUInt32LE(2048 + offset + at), `immutable ${label}+${at}`);
  }
  for (let offset = 0; offset < 20; offset += 4) read(engine, base + 0x5000 + offset, image.readUInt32LE(2560 + offset), `original relocation directory+${offset}`);
  receipt.gates.forEach(([pc], index) => {
    read(engine, base + 0x4050 + index * 4, oracle.gates[index][0], `Rust-linked IAT slot${index}`);
    read(engine, pc, 0x00000b0f, `engine-owned UD2 stub${index}`);
    read(engine, pc + 4, 0, `zero stub tail${index}`);
    fault(engine, pc, 2, 2, `final gate RX${index}`);
  });
  read(engine, base + 0x4058, 0, 'IAT zero terminal retained');
  read(engine, base + 0x3000, oracle.initial_data, 'initialized writable data before guest');
  for (const [address, label] of [[base, 'header R'], [receipt.entry_point, 'code RX'], [base + 0x4050, 'IAT R'], [base + 0x5000, 'relocation R']]) fault(engine, address, 2, 2, label);
  rejectExecute(engine, base + 0x4050, 'readonly IAT has no execute permission');
  rejectExecute(engine, base + 0x3000, 'writable data has no execute permission');
  if (example.base !== oracle.preferred_base) {
    fault(engine, oracle.preferred_base + 0x1000, 1, 1, 'old preferred code is unmapped');
    fault(engine, oracle.preferred_base + 0x4050, 1, 1, 'old preferred IAT is unmapped');
  }
}
function descriptors(engine, blocks, gates) {
  const view = refresh(engine).view;
  [...blocks, ...gates].forEach(([pc, value], index) => {
    view.setUint32(engine.base + transfer + index * 8, pc, true);
    view.setUint32(engine.base + transfer + index * 8 + 4, value, true);
  });
}
function child(engine, label, blocks, gates, caller, residentId) {
  descriptors(engine, blocks, gates);
  let bytes, low, high;
  if (residentId !== undefined) {
    const before = arena(engine);
    assert.equal(engine.api.compile_resident_with_gates(blocks.length, gates.length), 0);
    const after = arena(engine), metadata = new DataView(after.buffer, transfer, 24);
    low = metadata.getUint32(8, true); high = metadata.getUint32(12, true);
    assert.equal(BigInt(low) | (BigInt(high) << 32n), BigInt(residentId));
    const pointer = metadata.getUint32(16, true), length = metadata.getUint32(20, true);
    const expected = before.slice(); expected.set(words([1, 24, low, high, pointer, length]), transfer);
    assert.deepEqual(after, expected, `${label}: only resident metadata24 published`);
    refresh(engine); assert.ok(pointer > 0 && length > 8 && pointer + length <= engine.bytes.length);
    bytes = engine.bytes.slice(pointer, pointer + length);
    identities.push({instance: engine.ordinal, key: engine.key.toString(), unit: residentId, label, pointer, length});
  } else {
    operation(engine, label, () => engine.api.compile_with_gates(blocks.length, gates.length));
    assert.equal(engine.api.generation(), 1);
    const pointer = engine.api.module_ptr() >>> 0, length = engine.api.module_len() >>> 0;
    refresh(engine); assert.ok(pointer > 0 && length > 8 && pointer + length <= engine.bytes.length);
    bytes = engine.bytes.slice(pointer, pointer + length);
    identities.push({instance: engine.ordinal, key: engine.key.toString(), generation: 1, label, pointer, length});
  }
  assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes), guard = residentId === undefined ? 'guard' : 'guard_resident';
  const imports = [guard, ...(caller ? ['read32', residentId === undefined ? 'store32' : 'store_resident32'] : [])];
  const sort = rows => rows.sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sort(WebAssembly.Module.imports(module)), sort([{module: 'env', name: 'memory', kind: 'memory'}, ...imports.map(name => ({module: 'ring3', name, kind: 'function'}))]), `${label}: exact indirect-CALL/read/store imports`);
  const run = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: Object.fromEntries(imports.map(name => [name, engine.api[name]]))}).exports.run;
  assert.equal(typeof run, 'function'); assert.equal(run.length, 4);
  const filename = `engine-${engine.ordinal}-${label}.wasm`;
  writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes);
  return {run, low, high};
}
function setup(example) {
  const engine = fresh(), receipt = load(engine, example);
  inspectImage(engine, example, receipt);
  operation(engine, 'caller explicitly maps stack', () => engine.api.map(0x8000, 2, 3));
  oracle.stack_addresses.forEach((address, index) => operation(engine, 'initial stack sentinel', () => engine.api.write32(address, oracle.initial_stack_values[index]), 0, [[100, helper()]]));
  const blocks = oracle.caller_blocks.map(([offset, length]) => [receipt.entry_point + offset, length]);
  const gates = receipt.gates, gateBlocks = gates.map(([pc]) => [pc, 2]);
  let caller, gateOwner;
  if (example.owner === 'resident') {
    gateOwner = child(engine, 'gate-B', gateBlocks, gates, false, 1);
    caller = child(engine, 'caller-A', blocks, [], true, 2);
    assert.equal(engine.api.generation(), 0);
  } else {
    caller = child(engine, 'replacement', [...blocks, ...gateBlocks], gates, true);
    gateOwner = caller;
  }
  // the sole host cpu initialization; all later calls/returns/stores are guest or rust-owned.
  refresh(engine).bytes.set(cpu(oracle.startup_registers, receipt.entry_point), engine.base);
  engine.bytes.fill(0, engine.base + 96, engine.base + 100);
  return {engine, receipt, example, caller, gateOwner};
}
function dispatch(ctx, stage, expected) {
  const {engine, receipt, example, caller, gateOwner} = ctx, resident = example.owner === 'resident';
  const [pc, id] = receipt.gates[stage.gate_index];
  operation(engine, `${example.name}: real CALL [IAT] ${stage.api}`, () => caller.run(engine.base, engine.base + 56, oracle.budget, engine.base + 96), 0, [
    [0, cpu(stage.stop_registers, pc)], [56, exit(resident ? 3 : 8, stage.retired, resident ? 0 : id, resident ? 2 : 3)], [100, helper()],
  ]);
  if (resident) operation(engine, 'Gate B dispatch retires0', () => gateOwner.run(engine.base, engine.base + 56, oracle.budget, engine.base + 96), 0, [[56, exit(8, 0, id)]]);
  stack(engine, expected.stack_values, 'real indirect CALL/PUSH stack');
  read(engine, receipt.image_base + 0x3000, stage.data_after_dispatch, 'actual A3 guest data store ordering');
}
function capture(ctx, stage, expected, token) {
  const {engine, receipt, example, gateOwner} = ctx, [pc, id] = receipt.gates[stage.gate_index];
  const action = example.owner === 'resident'
    ? () => engine.api.capture_resident_call(engine.low, engine.high, gateOwner.low, gateOwner.high, 2, stage.args.length)
    : () => engine.api.capture_call(engine.low, engine.high, 1, 2, stage.args.length);
  const captured = record('R3CF', 112, [token, id, 2, stage.args.length, pc, stage.stop_registers[4], expected.return_pc, 0, ...Array.from({length: 16}, (_, index) => stage.args[index] ?? 0)]);
  operation(engine, 'capture private actual IAT call', action, 0, [[transfer, captured]]);
}
function provider(ctx, token) {
  const {engine, example, gateOwner} = ctx;
  return example.owner === 'resident'
    ? engine.api.complete_resident_windows_call(engine.low, engine.high, gateOwner.low, gateOwner.high, token)
    : engine.api.complete_windows_call(engine.low, engine.high, 1, token);
}
for (const example of oracle.cases) {
  const ctx = setup(example), {engine, receipt, caller} = ctx;
  let retired = 0;
  for (let index = 0; index < oracle.stages.length; index++) {
    const stage = oracle.stages[index], expected = example.stages[index], token = index + 1;
    dispatch(ctx, stage, expected); retired += stage.retired;
    capture(ctx, stage, expected, token);
    operation(engine, 'Rust-owned Windows completion without host scalar', () => provider(ctx, token), 0, [[0, cpu(stage.returned_registers, expected.return_pc)], [56, exit(3, 0)]]);
    stack(engine, expected.stack_values, 'provider completion preserves guest stack');
    operation(engine, 'consumed import-call token cannot replay', () => provider(ctx, token), 14);
    observations.push({name: example.name, owner: example.owner, api: stage.api, token, retired: stage.retired, last_error_after: stage.last_error_after, guest_data: stage.data_after_dispatch, return_pc: expected.return_pc});
  }
  operation(engine, 'actual final LEA/JMP caller continuation', () => caller.run(engine.base, engine.base + 56, oracle.budget, engine.base + 96), 0, [[0, cpu(oracle.final.registers, example.done)], [56, exit(oracle.final.reason, oracle.final.retired, 0, example.owner === 'resident' ? 2 : 3)]]);
  retired += oracle.final.retired; assert.equal(retired, oracle.total_retired);
  stack(engine, example.final_stack_values, 'final unchanged stack neighbors');
  read(engine, receipt.image_base + 0x3000, oracle.final_data, 'full-u32 value written by guest survives Set0/Get0');
  receipt.gates.forEach(([pc], index) => read(engine, receipt.image_base + 0x4050 + index * 4, oracle.gates[index][0], 'IAT pointer survives calls'));
  observations.push({name: example.name, receipt, final_registers: oracle.final.registers, final_eip: example.done, final_data: oracle.final_data, total_retired: retired});
  request(engine);
  operation(engine, 'loaded image cannot be linked twice', () => engine.api.load_pe32_linked_at(image.length, example.base, oracle.gate_base), 7);
  operation(engine, 'explicit unmap of loader gate page', () => engine.api.unmap(receipt.gate_base, 1));
  request(engine);
  operation(engine, 'load latch survives unmap', () => engine.api.load_pe32_linked_at(image.length, example.base, oracle.gate_base), 7);
  operation(engine, 'close preserves full arena', () => engine.api.close());
  operation(engine, 'Closed before invalid transfer/base/gate', () => engine.api.load_pe32_linked_at(4097, 0, 0), 5);
}

const malformed = Buffer.from(image), unsupported = Buffer.from(image), alias = Buffer.from(image);
malformed.writeUInt32LE(1, 2048 + 20);
unsupported[2048 + 0x72] = 'g'.charCodeAt(0);
alias.writeUInt32LE(12, 0x98 + 96 + 5 * 8 + 4);
alias.writeUInt32LE(0x4000, 2560); alias.writeUInt32LE(12, 2564); alias.writeUInt16LE(0x3050, 2568); alias.writeUInt16LE(0, 2570);
for (const [label, pages, bytes, length, base, gate, status, initialize] of [
  ['malformed_terminal', 8, malformed, malformed.length, 0x500000, oracle.gate_base, 19],
  ['unsupported_name', 8, unsupported, unsupported.length, 0x500000, oracle.gate_base, 20],
  ['relocation_iat_alias', 8, alias, alias.length, 0x500000, oracle.gate_base, 19],
  ['gate_in_image_gap', 8, image, image.length, 0x500000, 0x502000, 19],
  ['capacity_includes_gate_page', 5, image, image.length, 0x500000, oracle.gate_base, 8],
  ['non_pristine', 8, image, image.length, 0x500000, oracle.gate_base, 7, engine => {
    operation(engine, 'preexisting mapping', () => engine.api.map(0x8000, 1, 3));
    operation(engine, 'preexisting RAM sentinel', () => engine.api.write32(0x8000, 0x12345678), 0, [[100, helper()]]);
  }],
  ['oversized_transfer', 8, image, 4097, 0x500000, oracle.gate_base, 7],
  ['closed', 8, image, 4097, 0, 0, 5, engine => operation(engine, 'close pristine engine', () => engine.api.close())],
]) {
  const engine = fresh(pages);
  if (initialize) initialize(engine);
  request(engine, bytes);
  operation(engine, `rejected ${label}`, () => engine.api.load_pe32_linked_at(length, base, gate), status);
  assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_len(), 0);
  if (label === 'non_pristine') read(engine, 0x8000, 0x12345678, 'failed load retains mapped RAM');
  if (!['closed', 'non_pristine'].includes(label)) {
    fault(engine, 0x501000, 1, 1, 'failed candidate publishes no image mapping');
    fault(engine, oracle.gate_base, 1, 1, 'failed candidate publishes no gate mapping');
  }
  if (pages === 8 && !['closed', 'non_pristine'].includes(label)) {
    const receipt = load(engine, oracle.cases[1]);
    read(engine, receipt.image_base + 0x4050, oracle.gates[0][0], 'same instance accepts valid follow-on link');
    read(engine, receipt.image_base + 0x1002, 0x504050, 'valid follow-on retains selected-base relocation');
  }
  observations.push({rejection: label, status});
}
const legacy = fresh();
for (const [name, action] of [['fixed legacy load', () => legacy.api.load_pe32(image.length)], ['selected legacy load', () => legacy.api.load_pe32_at(image.length, 0x500000)]]) {
  request(legacy);
  operation(legacy, `${name} retains import refusal`, action, 20);
}
load(legacy, oracle.cases[1]);

// load-only controls verify packed metadata and zero unused stubs; the two-api
// programme remains the sole execution oracle.
for (const [name, thunk, pc, id, directoryPresent] of [
  ['Get-only absent directory12', 0x4070, 0x60000000, 0x10001, false],
  ['Set-only exact directory12', 0x4080, 0x60000010, 0x10002, true],
]) {
  const bytes = Buffer.from(image);
  for (const raw of [2048 + 0x40, 2048 + 0x50]) { bytes.writeUInt32LE(thunk, raw); bytes.writeUInt32LE(0, raw + 4); }
  bytes.writeUInt32LE(directoryPresent ? 0x4050 : 0, 0x98 + 96 + 12 * 8);
  bytes.writeUInt32LE(directoryPresent ? 8 : 0, 0x98 + 96 + 12 * 8 + 4);
  const filename = directoryPresent ? 'set-only.exe' : 'get-only.exe';
  writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes);
  const engine = fresh(); request(engine, bytes);
  const fields = [0x500000, oracle.image_size, 0x501000, 6, oracle.gate_base, 1, 0, 0, pc, id, 0, 0];
  operation(engine, name, () => engine.api.load_pe32_linked_at(bytes.length, 0x500000, oracle.gate_base), 0, [[transfer, record('R3LI', 64, fields)]]);
  read(engine, 0x504050, pc, 'single imported API binds its fixed provider slot');
  read(engine, 0x504054, 0, 'single imported API retains IAT terminator');
  read(engine, pc, 0x00000b0f, 'single used stub contains UD2');
  read(engine, pc === oracle.gate_base ? oracle.gate_base + 16 : oracle.gate_base, 0, 'unused provider slot remains zero');
  fault(engine, pc, 2, 2, 'single provider page finalized RX');
  assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_len(), 0);
  operation(engine, 'close load-only control', () => engine.api.close());
  observations.push({load_only: name, gate_count: 1, gate: [pc, id], unused_metadata: [0, 0]});
}

const provenance = {
  engine_sha256: hash(engineBytes), oracle_sha256: hash(oracleBytes), artifacts, sources, identities, observations,
  command: [process.execPath, process.argv[1], enginePath, outputDir, root], assembly_command: ['clang', ...assemble],
  tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0]},
  claim: 'independently authored PE32 named GetLastError/SetLastError imports and six HIGHLOW operands executed at preferred replacement and selected resident caller-A/Gate-B bases through actual emitted Wasm. Rust alone writes IAT and RX UD2 gate page; host supplies only loader arguments and follows Rust R3LI entry/gates. Five real FF15 indirect CALLs, private engine provider completions and actual A3 guest store reach literal frozen CPU/stack/data/Exit oracles with14 retirements each. No host scalar, IAT/stub writes, name resolution, runtime PE parser or poststartup CPU patch. Whole4236arena errors, unmapped failed candidates, valid retry, total capacity including gate page, strict old APIs, load latch after unmap and Closed. Claim is bounded unbound single-DLL named imports, existing HIGHLOW and synchronous single-context providers; no arbitrary DLL/ordinal/forwarder/delay/bound/Windows startup/TLS/TEB/multithread/callback provider, SDK/browser/game/full P2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({status: 'ok', engine_sha256: provenance.engine_sha256, oracle_sha256: provenance.oracle_sha256, owners: oracle.cases.map(example => example.owner), bases: oracle.cases.map(example => example.base), retired_each: oracle.total_retired, output: outputDir}));
