import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-image-startup');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json')), oracle = JSON.parse(oracleBytes);
const programPath = join(root, oracle.reused_program.path), codeOraclePath = join(root, oracle.reused_code_oracle.path);
const programBytes = readFileSync(programPath), codeOracleBytes = readFileSync(codeOraclePath);
assert.equal(hash(programBytes), oracle.reused_program.sha256);
assert.equal(hash(codeOracleBytes), oracle.reused_code_oracle.sha256);
const pinnedEncoding = JSON.parse(codeOracleBytes).code_hex;
const size = 4236, transfer = 140;
const artifacts = {}, observations = [], identities = [];

function authoredCode(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]);
  assert.equal(view.getUint16(18, true), 3);
  const offset = view.getUint32(32, true), stride = view.getUint16(46, true);
  const sections = Array.from({length: view.getUint16(48, true)}, (_, index) => {
    const at = offset + index * stride;
    return {type: view.getUint32(at + 4, true), offset: view.getUint32(at + 16, true), size: view.getUint32(at + 20, true), link: view.getUint32(at + 24, true), info: view.getUint32(at + 28, true), stride: view.getUint32(at + 36, true)};
  });
  const table = sections.find(section => section.type === 2);
  assert.ok(table);
  const strings = sections[table.link], programs = new Map();
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    const nameAt = strings.offset + view.getUint32(at, true);
    const name = object.subarray(nameAt, object.indexOf(0, nameAt)).toString('utf8');
    const section = view.getUint16(at + 14, true), start = view.getUint32(at + 4, true), length = view.getUint32(at + 8, true);
    if (section > 0 && section < sections.length && length > 0) {
      const source = sections[section];
      assert.ok(start + length <= source.size);
      assert.ok(!sections.some(item => [4, 9].includes(item.type) && item.info === section && item.size > 0));
      programs.set(name, object.subarray(source.offset + start, source.offset + start + length));
    }
  }
  assert.equal(programs.size, 1);
  return programs.get('imported_windows_sequence');
}

// literal input construction reproduces the pinned pe; it is not a runtime parser.
function authoredPe(code) {
  const bytes = Buffer.alloc(3072), view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  bytes.write('MZ'); view.setUint32(0x3c, 0x80, true); bytes.write('PE\0\0', 0x80);
  view.setUint16(0x84, 0x14c, true); view.setUint16(0x86, 4, true);
  view.setUint16(0x94, 224, true); view.setUint16(0x96, 0x102, true);
  const optional = 0x98;
  view.setUint16(optional, 0x10b, true);
  for (const [at, value] of [[4, 512], [8, 1536], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, 0x400000], [32, 4096], [36, 512], [56, 0x6000], [60, 1024], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16]]) view.setUint32(optional + at, value, true);
  view.setUint16(optional + 40, 4, true); view.setUint16(optional + 48, 4, true);
  view.setUint16(optional + 68, 3, true); view.setUint16(optional + 70, 0x100, true);
  for (const [index, rva, length] of [[1, 0x4000, 40], [5, 0x5000, 20], [12, 0x4050, 12]]) { view.setUint32(optional + 96 + index * 8, rva, true); view.setUint32(optional + 100 + index * 8, length, true); }
  [['.text', 57, 0x1000, 1024, 0x60000020], ['.data', 4, 0x3000, 1536, 0xc0000040], ['.imports', 144, 0x4000, 2048, 0x40000040], ['.fixups', 20, 0x5000, 2560, 0x40000040]].forEach(([name, virtualSize, rva, raw, flags], index) => {
    const at = 0x178 + index * 40; bytes.write(name, at);
    for (const [field, value] of [[8, virtualSize], [12, rva], [16, 512], [20, raw], [36, flags]]) view.setUint32(at + field, value, true);
  });
  bytes.fill(0xcc, 1024, 1536); bytes.set(code, 1024);
  [0x4040, 0, 0, 0x4060, 0x4050].forEach((value, index) => view.setUint32(2048 + index * 4, value, true));
  for (const at of [2048 + 0x40, 2048 + 0x50]) [0x4070, 0x4080, 0].forEach((value, index) => view.setUint32(at + index * 4, value, true));
  bytes.write('KeRnEl32.dLl', 2048 + 0x60, 'ascii');
  for (const [at, hint, name] of [[2048 + 0x70, 0x1234, 'GetLastError'], [2048 + 0x80, 0xffff, 'SetLastError']]) { view.setUint16(at, hint, true); bytes.write(name, at + 2, 'ascii'); }
  bytes.set(Buffer.from('001000001400000002300f3017301e3026302e30', 'hex'), 2560);
  return bytes;
}
const objectPath = join(outputDir, 'program.o'), assemble = ['-target', 'i386-unknown-linux-gnu', '-c', programPath, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
writeFileSync(join(outputDir, 'program.disassembly.txt'), execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]));
const code = authoredCode(readFileSync(objectPath));
assert.equal(code.toString('hex'), pinnedEncoding, 'unchanged pinned LLVM encoding only');
const image = authoredPe(code);
assert.equal(hash(image), oracle.reused_pe_sha256, 'identical previously proved authored PE input');
writeFileSync(join(outputDir, 'imported-windows.exe'), image); artifacts['imported-windows.exe'] = hash(image);
writeFileSync(join(outputDir, 'program.S'), programBytes);
writeFileSync(join(outputDir, 'reused-code-oracle.json'), codeOracleBytes);
writeFileSync(join(outputDir, 'startup-oracle.json'), oracleBytes);
writeFileSync(join(outputDir, 'run.mjs'), readFileSync(import.meta.filename));
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', 'engine/tests/process_image_startup_wasm.rs', 'engine/tests/fixtures/p2-image-startup/oracle.json', 'engine/tests/fixtures/p2-image-startup/run.mjs', oracle.reused_program.path, oracle.reused_code_oracle.path];
const sources = Object.fromEntries(sourcePaths.map(file => [file, hash(readFileSync(join(root, file)))]));
writeFileSync(join(outputDir, 'source-sha256.json'), JSON.stringify(sources, null, 2));
const engineBytes = readFileSync(enginePath);
writeFileSync(join(outputDir, 'engine.wasm'), engineBytes);
assert.ok(WebAssembly.validate(engineBytes));
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const names = ['open', 'close', 'arena_ptr', 'load_pe32_linked_at', 'start_loaded_image', 'generation', 'module_len', 'compile_resident_entries', 'resident_module', 'find_resident', 'acknowledge_resident_installation', 'find_installed_resident', 'dispatcher_module', 'guard_dispatch_entry', 'guard_resident', 'read32', 'store_resident32', 'capture_resident_call', 'complete_resident_windows_call'];
let engine;

function refresh(e = engine) { if (e.buffer !== e.memory.buffer) { e.buffer = e.memory.buffer; e.bytes = new Uint8Array(e.buffer); e.view = new DataView(e.buffer); } return e; }
function arena() { return refresh().bytes.slice(engine.base, engine.base + size); }
function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true)); return bytes;
}
function words(fields) { const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer); fields.forEach((value, index) => view.setUint32(index * 4, value, true)); return bytes; }
function cpu(registers, pc) { return record('R3ST', 56, [...registers, pc, oracle.eflags]); }
function exit(reason, retired, detail = 0, version = 3) { return record('R3EX', 40, [reason, retired, detail, 0, 0, 0], version); }
function helper(value = 0) { return record('R3MH', 40, [0, value, 0, 0, 0, 0]); }
function unmapped(address) { return record('R3MH', 40, [1, 0, 1, address, 1, 4]); }
function residentRecord(unit) { return words([1, 24, unit.low, unit.high, unit.pointer, unit.length]); }
function installation(unit, slot) { return record('R3IN', 32, [unit.low, unit.high, slot, 0]); }
function operation(label, action, status = 0, patches = []) {
  const expected = arena(); for (const [offset, bytes] of patches) expected.set(bytes, offset);
  assert.equal(action(), status, `${engine.case.name}/${label}: status`); assert.deepEqual(arena(), expected, `${engine.case.name}/${label}: whole4236 arena`);
}
function cancel(value) { refresh().view.setUint32(engine.base + 96, value, true); }
function read(address, value, label) { operation(label, () => engine.api.read32(address), 0, [[100, helper(value)]]); }
function absent(address, label) { operation(label, () => engine.api.read32(address), 0, [[100, unmapped(address)]]); }
function stack(values, label) { engine.case.stack_addresses.forEach((address, index) => read(address, values[index], `${label}/word${index}`)); }
function entriesRequest(entries, gates = []) {
  const view = refresh().view;
  entries.forEach((pc, index) => view.setUint32(engine.base + transfer + index * 4, pc, true));
  gates.forEach(([pc, id], index) => { const offset = engine.base + transfer + entries.length * 4 + index * 8; view.setUint32(offset, pc, true); view.setUint32(offset + 4, id, true); });
}
function getUnit(unit) {
  operation('retained generated bytes and full owner', () => engine.api.resident_module(unit.low, unit.high), 0, [[transfer, residentRecord(unit)]]);
  refresh(); assert.deepEqual(engine.bytes.slice(unit.pointer, unit.pointer + unit.length), unit.bytes);
}
function compile(entries, gates, label, helpers) {
  entriesRequest(entries, gates);
  const before = arena(); assert.equal(engine.api.compile_resident_entries(entries.length, gates.length), 0);
  const after = arena(), view = new DataView(after.buffer, transfer, 24);
  const unit = {low: view.getUint32(8, true), high: view.getUint32(12, true), pointer: view.getUint32(16, true), length: view.getUint32(20, true), entries: [...entries], label};
  unit.id = BigInt(unit.low) | (BigInt(unit.high) << 32n);
  assert.notEqual(unit.id, 0n); assert.ok(!engine.units.some(item => item.id === unit.id));
  const expected = before.slice(); expected.set(residentRecord(unit), transfer); assert.deepEqual(after, expected, 'only resident24 metadata');
  refresh(); assert.ok(unit.pointer > 0 && unit.length > 8 && unit.pointer + unit.length <= engine.bytes.length);
  unit.bytes = engine.bytes.slice(unit.pointer, unit.pointer + unit.length);
  const module = new WebAssembly.Module(unit.bytes), imports = WebAssembly.Module.imports(module);
  const sort = rows => rows.sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sort(imports), sort([{module: 'env', name: 'memory', kind: 'memory'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))]));
  const allowed = new Set(['guard_resident', 'read32', 'store_resident32']);
  const binding = Object.fromEntries(imports.filter(item => item.module === 'ring3').map(item => { assert.ok(allowed.has(item.name)); return [item.name, engine.api[item.name]]; }));
  unit.instance = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: binding}); unit.run = unit.instance.exports.run;
  assert.equal(typeof unit.run, 'function'); assert.equal(unit.run.length, 4);
  engine.units.push(unit);
  const filename = `${engine.case.name}-resident-${engine.units.length}-${label}.wasm`; writeFileSync(join(outputDir, filename), unit.bytes); artifacts[filename] = hash(unit.bytes);
  identities.push({case: engine.case.name, key: engine.key.toString(), label, entries, id: unit.id.toString(), pointer: unit.pointer, length: unit.length, module_sha256: hash(unit.bytes)});
  entries.forEach(pc => operation('logical finder matches returned owner', () => engine.api.find_resident(pc), 0, [[transfer, residentRecord(unit)]]));
  assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_len(), 0);
  return unit;
}
function install(unit, slot) {
  operation('compiled owner is not installed', () => engine.api.find_installed_resident(engine.low, engine.high, unit.entries[0]), 17);
  engine.table.set(slot, unit.run);
  operation('installation acknowledgement', () => engine.api.acknowledge_resident_installation(engine.low, engine.high, unit.low, unit.high, slot), 0, [[transfer, installation(unit, slot)]]);
  engine.installed.set(unit.id, slot); assert.equal(engine.table.get(slot), unit.run);
  unit.entries.forEach(pc => operation('finder exposes exact installed owner', () => engine.api.find_installed_resident(engine.low, engine.high, pc), 0, [[transfer, installation(unit, slot)]]));
}
function dispatcher() {
  const before = arena(); assert.equal(engine.api.dispatcher_module(engine.low, engine.high), 0);
  const after = arena(), view = new DataView(after.buffer, transfer, 32), pointer = view.getUint32(24, true), length = view.getUint32(28, true);
  const expected = before.slice(); expected.set(record('R3DP', 32, [engine.low, engine.high, pointer, length]), transfer); assert.deepEqual(after, expected);
  refresh(); assert.ok(pointer > 0 && length > 8 && pointer + length <= engine.bytes.length);
  const bytes = engine.bytes.slice(pointer, pointer + length);
  if (engine.dispatcherBytes) { assert.equal(pointer, engine.dispatcherPointer); assert.deepEqual(bytes, engine.dispatcherBytes); return; }
  const module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'}, {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'}, {module: 'ring3', name: 'find_installed_resident', kind: 'function'}]);
  engine.dispatcher = new WebAssembly.Instance(module, {env: {memory: engine.memory, table: engine.table}, ring3: {guard_dispatch_entry: engine.api.guard_dispatch_entry, find_installed_resident: engine.api.find_installed_resident}});
  assert.equal(engine.dispatcher.exports.run.length, 4);
  engine.dispatcherBytes = bytes; engine.dispatcherPointer = pointer;
  const filename = `${engine.case.name}-returning-dispatcher.wasm`; writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes);
}
function dispatch(budget, label, patches, status = 0) { operation(label, () => engine.dispatcher.exports.run(engine.base, engine.base + 56, budget, engine.base + 96), status, patches); }
function observedNeedCodePC() {
  const view = refresh().view;
  assert.equal(view.getUint32(engine.base + 56, true), 0x58453352);
  assert.equal(view.getUint32(engine.base + 60, true), 0x10003);
  assert.equal(view.getUint32(engine.base + 72, true), 3);
  return view.getUint32(engine.base + 48, true);
}
function lookupOwner() {
  const view = refresh().view;
  assert.equal(view.getUint32(engine.base + transfer, true), 0x4e493352);
  assert.equal(view.getUint32(engine.base + transfer + 4, true), 0x10001);
  assert.equal(view.getUint32(engine.base + transfer + 8, true), 32);
  return {low: view.getUint32(engine.base + transfer + 16, true), high: view.getUint32(engine.base + transfer + 20, true), slot: view.getUint32(engine.base + transfer + 24, true)};
}

for (const [caseIndex, test] of oracle.cases.entries()) {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => { const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); return [name, fn]; }));
  assert.equal(api.start_loaded_image.length, 2);
  const key = caseIndex === 0 ? 0xa343cdef89abcdefn : 0xb343987676543210n;
  engine = {case: test, api, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n), memory: instance.exports.memory, units: [], installed: new Map()};
  assert.ok(engine.memory instanceof WebAssembly.Memory);
  assert.equal(api.open(test.pages, engine.low, engine.high), 0);
  engine.base = api.arena_ptr() >>> 0; refresh();
  assert.ok(engine.base > 0 && engine.base + size <= engine.bytes.length);
  const initial = arena();
  assert.deepEqual(initial.subarray(0, 56), cpu(oracle.startup.before_context.State_registers, oracle.startup.before_context.State_EIP));
  assert.deepEqual(initial.subarray(56, 96), exit(1, 0, 0, 1));
  engine.bytes.fill(0xa5, engine.base + 100, engine.base + size);
  engine.table = new WebAssembly.Table({element: 'anyfunc', initial: oracle.table_slots, maximum: oracle.table_slots});
  refresh().bytes.set(image, engine.base + transfer);
  const linkedFields = [oracle.actual_base, 0x6000, oracle.entry_point, 6, oracle.gate_base, 2, 0, 0, 0x60000000, 0x10001, 0x60000010, 0x10002];
  operation('Rust linked PE and unchanged initial context', () => api.load_pe32_linked_at(image.length, oracle.actual_base, oracle.gate_base), 0, [[transfer, record('R3LI', 64, linkedFields)]]);
  const linked = new DataView(arena().buffer, transfer, 64);
  const receipt = {image_base: linked.getUint32(16, true), entry_point: linked.getUint32(24, true), gates: Array.from({length: linked.getUint32(36, true)}, (_, index) => [linked.getUint32(48 + index * 8, true), linked.getUint32(52 + index * 8, true)])};
  receipt.gates.forEach(([pc], index) => { read(receipt.image_base + 0x4050 + index * 4, pc, 'Rust bound IAT'); read(pc, 0x00000b0f, 'Rust RX gate stub'); });
  const gate = compile(receipt.gates.map(([pc]) => pc), receipt.gates, 'gate', oracle.transport.gate_module_helpers);
  install(gate, oracle.transport.gate_slot);
  let caller = compile([receipt.entry_point], [], 'caller-1', oracle.transport.caller_module_helpers[0]);
  install(caller, oracle.transport.caller_slots[0]); dispatcher();
  assert.deepEqual(arena().subarray(0, 96), initial.subarray(0, 96), 'metadata operations retain supported fresh context');
  const stable = [engine.memory, engine.table, engine.dispatcher, gate.instance, caller.instance];
  if (test.name === 'low') {
    operation('prestart capacity preserves owners and retry', () => api.start_loaded_image(test.stack_base, 3), oracle.errors.prestart_capacity);
    absent(test.stack_base, 'capacity rejection maps no first page');
    operation('whole private PE gap is reserved from stack', () => api.start_loaded_image(0x502000, 1), oracle.errors.prestart_PE_gap_overlap);
    absent(0x502000, 'rejected PE gap remains unmapped');
    cancel(1);
    operation('prestart cancellation preserves latch and retry', () => api.start_loaded_image(test.stack_base, test.stack_pages), oracle.errors.prestart_cancel);
    absent(test.stack_base, 'cancel rejection maps no stack');
    cancel(0);
  }
  // expected bytes are observations only; the host never writes state or exit.
  operation('engine owns default CPU and empty stack publication', () => api.start_loaded_image(test.stack_base, test.stack_pages), 0, [[0, cpu(test.startup_registers, oracle.entry_point)], [56, exit(3, 0)]]);
  assert.equal(observedNeedCodePC(), receipt.entry_point, 'startup entry is private loaded-image authority');
  operation('successful startup cannot repeat', () => api.start_loaded_image(test.stack_base, test.stack_pages), oracle.errors.one_shot);
  stack(test.initial_stack_values, 'empty stack before guest CALL'); read(test.lower_stack_word, 0, 'lower configured stack word');
  absent(test.one_past_probe, 'one-past-end or wrapped low page remains unmapped');
  entriesRequest([test.stack_base]);
  operation('RW stack is not executable code', () => api.compile_resident_entries(1, 0), oracle.errors.stack_nonexecute_compile);
  getUnit(gate); getUnit(caller); dispatcher();
  assert.equal(engine.table.get(0), gate.run); assert.equal(engine.table.get(1), caller.run);
  [engine.memory, engine.table, engine.dispatcher, gate.instance, caller.instance].forEach((value, index) => assert.equal(value, stable[index]));
  observations.push({case: test.name, phase: 'startup', entry: observedNeedCodePC(), registers: test.startup_registers, flags: oracle.eflags, stack_base: test.stack_base, stack_pages: test.stack_pages, preserved_precompiled_ids: [gate.id.toString(), caller.id.toString()]});
  let retired = 0;
  for (let index = 0; index < oracle.stages.length; index++) {
    if (index !== 0) {
      dispatch(oracle.budget - retired, 'actual cold continuation miss', [[56, exit(3, 0)]]);
      const seed = observedNeedCodePC();
      assert.equal(seed, oracle.stages[index - 1].return_pc, 'expected return PC only asserts the observed seed');
      caller = compile([seed], [], `caller-${index + 1}`, oracle.transport.caller_module_helpers[index]);
      install(caller, oracle.transport.caller_slots[index]); dispatcher();
    }
    const stage = oracle.stages[index], expected = test.stages[index];
    dispatch(oracle.budget - retired, 'real IAT CALL/PUSH reaches provider gate', [[0, cpu(expected.stop_registers, receipt.gates[stage.gate_index][0])], [56, exit(8, stage.retired, stage.id)], [100, helper()], [transfer, installation(gate, 0)]]);
    retired += stage.retired;
    const owner = lookupOwner(); assert.deepEqual(owner, {low: gate.low, high: gate.high, slot: 0});
    stack(expected.stack_values, 'guest stack at real gate'); read(receipt.image_base + 0x3000, stage.data_after_dispatch, 'guest store order');
    const observedId = refresh().view.getUint32(engine.base + 80, true);
    assert.ok(receipt.gates.some(([, id]) => id === observedId));
    const stackWords = observedId === 0x10002 ? 1 : 0, token = index + 1;
    const captured = record('R3CF', 112, [token, stage.id, 2, stage.args.length, receipt.gates[stage.gate_index][0], expected.stop_registers[4], stage.return_pc, 0, ...Array.from({length: 16}, (_, arg) => stage.args[arg] ?? 0)]);
    operation('private engine capture owns the observed provider frame', () => api.capture_resident_call(engine.low, engine.high, owner.low, owner.high, 2, stackWords), 0, [[transfer, captured]]);
    if (index === 0 && test.name === 'low') operation('pending owner wins before startup latch and invalid parameters', () => api.start_loaded_image(1, 0), oracle.errors.pending);
    operation('engine Windows completion returns privately with no scalar', () => api.complete_resident_windows_call(engine.low, engine.high, owner.low, owner.high, token), 0, [[0, cpu(expected.returned_registers, stage.return_pc)], [56, exit(3, 0)]]);
    operation('provider frame cannot replay', () => api.complete_resident_windows_call(engine.low, engine.high, owner.low, owner.high, token), 14);
    stack(expected.stack_values, 'Stdcall cleanup leaves guest words intact');
    observations.push({case: test.name, phase: 'provider', id: observedId, token, caller_id: caller.id.toString(), gate_owner_id: gate.id.toString(), retired: stage.retired, entry_esp: expected.stop_registers[4], returned_esp: expected.returned_registers[4], guest_data: stage.data_after_dispatch});
    [engine.memory, engine.table, engine.dispatcher].forEach((value, index) => assert.equal(value, stable[index]));
  }
  dispatch(oracle.budget - retired, 'final caller is still cold', [[56, exit(3, 0)]]);
  const finalSeed = observedNeedCodePC(); assert.equal(finalSeed, oracle.stages.at(-1).return_pc);
  caller = compile([finalSeed], [], 'caller-6', oracle.transport.caller_module_helpers[5]);
  install(caller, oracle.transport.caller_slots[5]); dispatcher();
  dispatch(oracle.budget - retired, 'final guest LEA/JMP and missing UD2 transport', [[0, cpu(test.final_registers, oracle.final.eip)], [56, exit(3, oracle.final.retired)], [transfer, installation(caller, 6)]]);
  retired += oracle.final.retired; assert.equal(retired, oracle.final.total_retired);
  assert.equal(observedNeedCodePC(), oracle.final.eip, 'terminal UD2 is never used as a compile seed');
  assert.equal(engine.units.length, oracle.positive_units_per_case);
  stack(test.final_stack_values, 'final unchanged guest neighbors'); read(test.lower_stack_word, 0, 'unused lower stack stays zero');
  absent(test.one_past_probe, 'no low-page alias or extra top stack mapping'); read(receipt.image_base + 0x3000, oracle.final.data, 'final full-u32 guest data');
  engine.units.forEach(unit => { getUnit(unit); unit.entries.forEach(pc => operation('all seven installed owners remain current', () => api.find_installed_resident(engine.low, engine.high, pc), 0, [[transfer, installation(unit, engine.installed.get(unit.id))]])); });
  dispatcher();
  operation('startup remains one-shot after execution', () => api.start_loaded_image(1, 0), oracle.errors.one_shot);
  observations.push({case: test.name, phase: 'final', retired, registers: test.final_registers, flags: oracle.eflags, eip: oracle.final.eip, data: oracle.final.data, live_units: engine.units.length});
  operation('close keeps the arena tombstone', () => api.close());
  operation('Closed precedes invalid startup parameters', () => api.start_loaded_image(1, 0), oracle.errors.Closed);
  operation('closed retained generated owner is inert', () => gate.run(engine.base, engine.base + 56, oracle.budget, engine.base + 96), oracle.errors.Closed);
}
const provenance = {
  engine_sha256: hash(engineBytes), startup_oracle_sha256: hash(oracleBytes), reused_program_sha256: hash(programBytes), reused_code_oracle_sha256: hash(codeOracleBytes), reused_pe_sha256: hash(image), artifacts, sources, identities, observations,
  command: [process.execPath, process.argv[1], enginePath, outputDir, root], assembly_command: ['clang', ...assemble],
  tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0]},
  claim: 'two fresh engine instances start the identical pinned authored linked PE with new independently frozen all-zero CPU/flags2/empty-stack literals. Startup owns the only State/Exit initialization and stack mapping; the host has no map/write32/upload bindings and never patches CPU/Exit. Already installed Gate/first caller, full-u64 IDs, generated bytes/pointers and returning dispatcher survive first96-only startup. Low stack8000/2 publishes ESPa000; upper stackfffff000/1 publishes ESP0, real guest CALL/PUSH uses last valid width4 words and private Stdcall cleanup returns0 without a mapped low page. Both real IAT/provider/guest-store/resident paths retire14 and produce EAX7/dataf1234567 with flags2. Continuation seeds are canonical observed NeedCode EIP; expected return PCs are assertions only. Proportional prestart capacity/PE-gap/cancel failures and same-owner retry, RW/nonX/boundary observations, pending/one-shot/Closed and retained owners are explicit. Native tests own full RAM/version/latch/history matrix; actual word observations are bounded. Host Memory/Table/function provenance remains trusted. No Windows entry conventions, synthetic return, argv/TLS/CRT/PEB/TEB/SEH, process exit, autorun/scheduler/graph/cache/optimizer, SDK/browser/performance/game/full P2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({status: 'ok', engine_sha256: provenance.engine_sha256, startup_oracle_sha256: provenance.startup_oracle_sha256, cases: oracle.cases.map(test => test.name), retired_per_case: oracle.final.total_retired, output: outputDir}));
