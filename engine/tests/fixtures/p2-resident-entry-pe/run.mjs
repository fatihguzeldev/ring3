import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-resident-entry-pe');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json')), oracle = JSON.parse(oracleBytes);
const programPath = join(root, oracle.reused_program.path), cpuOraclePath = join(root, oracle.reused_cpu_oracle.path);
const programBytes = readFileSync(programPath), cpuOracleBytes = readFileSync(cpuOraclePath);
assert.equal(hash(programBytes), oracle.reused_program.sha256);
assert.equal(hash(cpuOracleBytes), oracle.reused_cpu_oracle.sha256);
const cpuOracle = JSON.parse(cpuOracleBytes), selected = cpuOracle.cases.find(item => item.name === 'selected');
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
assert.equal(code.toString('hex'), cpuOracle.code_hex, 'unchanged pinned LLVM encoding');
const image = authoredPe(code);
assert.equal(hash(image), oracle.reused_pe_sha256, 'identical previously proved authored PE input');
writeFileSync(join(outputDir, 'imported-windows.exe'), image); artifacts['imported-windows.exe'] = hash(image);
writeFileSync(join(outputDir, 'program.S'), programBytes);
writeFileSync(join(outputDir, 'reused-cpu-oracle.json'), cpuOracleBytes);
writeFileSync(join(outputDir, 'runtime-oracle.json'), oracleBytes);
writeFileSync(join(outputDir, 'run.mjs'), readFileSync(import.meta.filename));
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', 'engine/tests/process_resident_entries_wasm.rs', 'engine/tests/fixtures/p2-resident-entry-pe/oracle.json', 'engine/tests/fixtures/p2-resident-entry-pe/run.mjs', oracle.reused_program.path, oracle.reused_cpu_oracle.path];
const sources = Object.fromEntries(sourcePaths.map(file => [file, hash(readFileSync(join(root, file)))]));
writeFileSync(join(outputDir, 'source-sha256.json'), JSON.stringify(sources, null, 2));
const engineBytes = readFileSync(enginePath);
writeFileSync(join(outputDir, 'engine.wasm'), engineBytes);
assert.ok(WebAssembly.validate(engineBytes));
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const names = ['open', 'close', 'arena_ptr', 'load_pe32_linked_at', 'map', 'protect', 'generation', 'module_len', 'compile_resident_entries', 'resident_module', 'find_resident', 'acknowledge_resident_installation', 'find_installed_resident', 'dispatcher_module', 'guard_dispatch_entry', 'guard_resident', 'read32', 'write32', 'store_resident32', 'capture_resident_call', 'complete_resident_windows_call'];
const instance = new WebAssembly.Instance(engineModule, {});
const api = Object.fromEntries(names.map(name => { const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); return [name, fn]; }));
assert.equal(api.compile_resident_entries.length, 2);
const key = 0xa342cdef89abcdefn;
const engine = {api, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n), memory: instance.exports.memory, units: [], installed: new Map()};
assert.ok(engine.memory instanceof WebAssembly.Memory);
assert.equal(api.open(oracle.pages, engine.low, engine.high), 0);
engine.base = api.arena_ptr() >>> 0; refresh(engine);
assert.ok(engine.base > 0 && engine.base + size <= engine.bytes.length);
engine.bytes.fill(0xa5, engine.base, engine.base + size);
cancel(0);
engine.table = new WebAssembly.Table({element: 'anyfunc', initial: oracle.table_slots, maximum: oracle.table_slots});

function refresh(e) { if (e.buffer !== e.memory.buffer) { e.buffer = e.memory.buffer; e.bytes = new Uint8Array(e.buffer); e.view = new DataView(e.buffer); } return e; }
function arena(e) { return refresh(e).bytes.slice(e.base, e.base + size); }
function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true)); return bytes;
}
function words(fields) { const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer); fields.forEach((value, index) => view.setUint32(index * 4, value, true)); return bytes; }
function cpu(registers, pc) { return record('R3ST', 56, [...registers, pc, oracle.startup.eflags]); }
function exit(reason, retired, detail = 0) { return record('R3EX', 40, [reason, retired, detail, 0, 0, 0], 3); }
function helper(value = 0) { return record('R3MH', 40, [0, value, 0, 0, 0, 0]); }
function residentRecord(unit) { return words([1, 24, unit.low, unit.high, unit.pointer, unit.length]); }
function installation(unit, slot) { return record('R3IN', 32, [unit.low, unit.high, slot, 0]); }
function operation(label, action, status = 0, patches = []) {
  const expected = arena(engine); for (const [offset, bytes] of patches) expected.set(bytes, offset);
  assert.equal(action(), status, `${label}: status`); assert.deepEqual(arena(engine), expected, `${label}: whole4236 arena`);
}
function cancel(value) { refresh(engine).view.setUint32(engine.base + 96, value, true); }
function read(address, value, label) { operation(label, () => api.read32(address), 0, [[100, helper(value)]]); }
function stack(values, label) { oracle.startup.stack_addresses.forEach((address, index) => read(address, values[index], `${label}: stack${index}`)); }
function entriesRequest(entries, gates = []) {
  const view = refresh(engine).view;
  entries.forEach((pc, index) => view.setUint32(engine.base + transfer + index * 4, pc, true));
  gates.forEach(([pc, id], index) => { const offset = engine.base + transfer + entries.length * 4 + index * 8; view.setUint32(offset, pc, true); view.setUint32(offset + 4, id, true); });
}
function getUnit(unit, label) {
  operation(label, () => api.resident_module(unit.low, unit.high), 0, [[transfer, residentRecord(unit)]]);
  refresh(engine); assert.deepEqual(engine.bytes.slice(unit.pointer, unit.pointer + unit.length), unit.bytes, 'retained immutable module bytes/pointer');
}
function compile(entries, gates, label, helpers) {
  entriesRequest(entries, gates);
  const before = arena(engine); assert.equal(api.compile_resident_entries(entries.length, gates.length), 0);
  const after = arena(engine), view = new DataView(after.buffer, transfer, 24);
  const unit = {low: view.getUint32(8, true), high: view.getUint32(12, true), pointer: view.getUint32(16, true), length: view.getUint32(20, true), entries: [...entries], label};
  unit.id = BigInt(unit.low) | (BigInt(unit.high) << 32n);
  assert.notEqual(unit.id, 0n); assert.ok(!engine.units.some(item => item.id === unit.id));
  const expected = before.slice(); expected.set(residentRecord(unit), transfer); assert.deepEqual(after, expected, `${label}: only resident24 metadata`);
  refresh(engine); assert.ok(unit.pointer > 0 && unit.length > 8 && unit.pointer + unit.length <= engine.bytes.length);
  unit.bytes = engine.bytes.slice(unit.pointer, unit.pointer + unit.length);
  const module = new WebAssembly.Module(unit.bytes), imports = WebAssembly.Module.imports(module);
  const sort = rows => rows.sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sort(imports), sort([{module: 'env', name: 'memory', kind: 'memory'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))]), 'exact independently frozen required helpers');
  const allowed = new Set(['guard_resident', 'read32', 'store_resident32']);
  const binding = Object.fromEntries(imports.filter(item => item.module === 'ring3').map(item => { assert.ok(allowed.has(item.name)); return [item.name, api[item.name]]; }));
  unit.instance = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: binding}); unit.run = unit.instance.exports.run;
  assert.equal(typeof unit.run, 'function'); assert.equal(unit.run.length, 4);
  engine.units.push(unit);
  const filename = `resident-${engine.units.length}-${label}.wasm`; writeFileSync(join(outputDir, filename), unit.bytes); artifacts[filename] = hash(unit.bytes);
  identities.push({label, entries, id: unit.id.toString(), pointer: unit.pointer, length: unit.length, module_sha256: hash(unit.bytes)});
  entries.forEach(pc => operation('logical resident lookup matches returned owner', () => api.find_resident(pc), 0, [[transfer, residentRecord(unit)]]));
  assert.equal(api.generation(), 0); assert.equal(api.module_len(), 0);
  return unit;
}
function install(unit, slot, status = 0) {
  const pc = unit.entries[0];
  operation('compiled unit is not installed until acknowledgement', () => api.find_installed_resident(engine.low, engine.high, pc), 17);
  engine.table.set(slot, unit.run);
  operation('ordinary installation acknowledgement', () => api.acknowledge_resident_installation(engine.low, engine.high, unit.low, unit.high, slot), status, status === 0 ? [[transfer, installation(unit, slot)]] : []);
  if (status !== 0) { engine.table.set(slot, null); assert.equal(engine.table.get(slot), null); return; }
  engine.installed.set(unit.id, slot); assert.equal(engine.table.get(slot), unit.run);
  unit.entries.forEach(entry => operation('installed finder publishes exact current binding', () => api.find_installed_resident(engine.low, engine.high, entry), 0, [[transfer, installation(unit, slot)]]));
}
function dispatcher() {
  const before = arena(engine); assert.equal(api.dispatcher_module(engine.low, engine.high), 0);
  const after = arena(engine), view = new DataView(after.buffer, transfer, 32), pointer = view.getUint32(24, true), length = view.getUint32(28, true);
  const expected = before.slice(); expected.set(record('R3DP', 32, [engine.low, engine.high, pointer, length]), transfer); assert.deepEqual(after, expected);
  refresh(engine); assert.ok(pointer > 0 && length > 8 && pointer + length <= engine.bytes.length);
  const bytes = engine.bytes.slice(pointer, pointer + length);
  if (engine.dispatcherBytes) { assert.equal(pointer, engine.dispatcherPointer); assert.deepEqual(bytes, engine.dispatcherBytes); return; }
  const module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'}, {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'}, {module: 'ring3', name: 'find_installed_resident', kind: 'function'}]);
  engine.dispatcher = new WebAssembly.Instance(module, {env: {memory: engine.memory, table: engine.table}, ring3: {guard_dispatch_entry: api.guard_dispatch_entry, find_installed_resident: api.find_installed_resident}});
  assert.equal(engine.dispatcher.exports.run.length, 4);
  engine.dispatcherBytes = bytes; engine.dispatcherPointer = pointer;
  writeFileSync(join(outputDir, 'returning-dispatcher.wasm'), bytes); artifacts['returning-dispatcher.wasm'] = hash(bytes);
}
function dispatch(budget, label, patches, status = 0) { operation(label, () => engine.dispatcher.exports.run(engine.base, engine.base + 56, budget, engine.base + 96), status, patches); }
function observedNeedCodePC() {
  const view = refresh(engine).view;
  assert.equal(view.getUint32(engine.base + 56, true), 0x58453352);
  assert.equal(view.getUint32(engine.base + 60, true), 0x10003);
  assert.equal(view.getUint32(engine.base + 72, true), 3);
  return view.getUint32(engine.base + 48, true);
}
function lookupOwner() {
  const view = refresh(engine).view;
  assert.equal(view.getUint32(engine.base + transfer, true), 0x4e493352);
  assert.equal(view.getUint32(engine.base + transfer + 4, true), 0x10001);
  assert.equal(view.getUint32(engine.base + transfer + 8, true), 32);
  return {low: view.getUint32(engine.base + transfer + 16, true), high: view.getUint32(engine.base + transfer + 20, true), slot: view.getUint32(engine.base + transfer + 24, true)};
}

refresh(engine).bytes.set(image, engine.base + transfer);
const linkedFields = [oracle.actual_base, 0x6000, 0x501000, 6, oracle.gate_base, 2, 0, 0, 0x60000000, 0x10001, 0x60000010, 0x10002];
operation('same Rust linked PE loader and R3LI64', () => api.load_pe32_linked_at(image.length, oracle.actual_base, oracle.gate_base), 0, [[transfer, record('R3LI', 64, linkedFields)]]);
const linked = new DataView(arena(engine).buffer, transfer, 64);
const receipt = {image_base: linked.getUint32(16, true), entry_point: linked.getUint32(24, true), gates: Array.from({length: linked.getUint32(36, true)}, (_, index) => [linked.getUint32(48 + index * 8, true), linked.getUint32(52 + index * 8, true)])};
assert.equal(receipt.entry_point, selected.entry);
receipt.gates.forEach(([pc], index) => { read(receipt.image_base + 0x4050 + index * 4, pc, 'IAT already bound by Rust'); read(pc, 0x00000b0f, 'RX stub already authored by Rust'); });
operation('invalid count before any new publication', () => api.compile_resident_entries(0, 9), 7);
const gate = compile(receipt.gates.map(([pc]) => pc), receipt.gates, 'gate-B', oracle.module_helpers.gate);
install(gate, oracle.installation.gate_slot);
dispatcher();
entriesRequest([receipt.gates[0][0]]);
operation('UD2 is unsupported as an ordinary seed', () => api.compile_resident_entries(1, 0), 10);
getUnit(gate, 'failed ordinary decode retains gate bytes and owner');
operation('explicit startup stack map', () => api.map(0x8000, 2, 3));
oracle.startup.stack_addresses.forEach((address, index) => operation('initial stack sentinel', () => api.write32(address, oracle.startup.stack_values[index]), 0, [[100, helper()]]));
cancel(1);
let caller = compile([receipt.entry_point], [], 'caller-1', oracle.module_helpers.callers[0]);
install(caller, oracle.installation.caller_slots[0], 16);
// the sole host cpu initialization; later compilation only reads canonical state eip.
refresh(engine).bytes.set(cpu(oracle.startup.registers, receipt.entry_point), engine.base);
dispatch(cpuOracle.budget, 'cancelled dispatcher stops before lookup', [[56, exit(2, 0)]]);
cancel(0);
getUnit(caller, 'ack retry reuses the prepared unit without compilation');
install(caller, oracle.installation.caller_slots[0]);
entriesRequest([receipt.entry_point]);
operation('current executable instruction overlap rejects publication', () => api.compile_resident_entries(1, 0), 7);
getUnit(caller, 'overlap rejection retains caller bytes and owner');
dispatcher();
const stable = [engine.memory, engine.table, engine.dispatcher];
let retired = 0;
for (let index = 0; index < cpuOracle.stages.length; index++) {
  if (index !== 0) {
    dispatch(cpuOracle.budget - retired, 'real cold continuation miss retires0', [[56, exit(3, 0)]]);
    const seed = observedNeedCodePC();
    assert.equal(seed, selected.stages[index - 1].return_pc, 'pinned return pc is assertion-only');
    caller = compile([seed], [], `caller-${index + 1}`, oracle.module_helpers.callers[index]);
    install(caller, oracle.installation.caller_slots[index]); dispatcher();
  }
  const stage = cpuOracle.stages[index], expected = selected.stages[index];
  const wantedLookup = installation(gate, oracle.installation.gate_slot);
  dispatch(cpuOracle.budget - retired, 'actual returning dispatcher reaches IAT provider gate', [[0, cpu(stage.stop_registers, receipt.gates[stage.gate_index][0])], [56, exit(8, stage.retired, stage.id)], [100, helper()], [transfer, wantedLookup]]);
  retired += stage.retired;
  const owner = lookupOwner(); assert.deepEqual(owner, {low: gate.low, high: gate.high, slot: 0});
  stack(expected.stack_values, 'real indirect CALL/PUSH stack');
  read(receipt.image_base + 0x3000, stage.data_after_dispatch, 'guest A3 store order');
  const observedId = refresh(engine).view.getUint32(engine.base + 80, true);
  assert.ok(receipt.gates.some(([, id]) => id === observedId));
  const stackWords = observedId === 0x10002 ? 1 : 0, token = index + 1;
  const captured = record('R3CF', 112, [token, stage.id, 2, stage.args.length, receipt.gates[stage.gate_index][0], stage.stop_registers[4], expected.return_pc, 0, ...Array.from({length: 16}, (_, arg) => stage.args[arg] ?? 0)]);
  operation('existing capture privately owns the observed Gate B call', () => api.capture_resident_call(engine.low, engine.high, owner.low, owner.high, 2, stackWords), 0, [[transfer, captured]]);
  if (index === 0) {
    operation('pending precedes invalid ordinary entry counts', () => api.compile_resident_entries(0, 9), 12);
    operation('pending prevents ordinary installation publication', () => api.acknowledge_resident_installation(engine.low, engine.high, owner.low, owner.high, 0), 12);
    dispatch(cpuOracle.budget, 'pending dispatcher stays parked', [], 12);
    operation('full high owner limb matters', () => api.complete_resident_windows_call(engine.low, engine.high, owner.low, owner.high ^ 1, token), 3);
    operation('full high key limb matters', () => api.complete_resident_windows_call(engine.low, engine.high ^ 1, owner.low, owner.high, token), 3);
    operation('live caller is not the private pending gate owner', () => api.complete_resident_windows_call(engine.low, engine.high, caller.low, caller.high, token), 14);
  }
  operation('Rust Windows completion keeps its frozen private return', () => api.complete_resident_windows_call(engine.low, engine.high, owner.low, owner.high, token), 0, [[0, cpu(stage.returned_registers, expected.return_pc)], [56, exit(3, 0)]]);
  operation('consumed provider token cannot replay', () => api.complete_resident_windows_call(engine.low, engine.high, owner.low, owner.high, token), 14);
  stack(expected.stack_values, 'completion writes no guest stack');
  observations.push({phase: 'positive', gate_id: observedId, token, caller_id: caller.id.toString(), gate_owner_id: gate.id.toString(), retired: stage.retired, guest_data: stage.data_after_dispatch});
  [engine.memory, engine.table, engine.dispatcher].forEach((reference, index) => assert.equal(reference, stable[index]));
}
dispatch(cpuOracle.budget - retired, 'final caller is still undiscovered', [[56, exit(3, 0)]]);
const finalSeed = observedNeedCodePC();
assert.equal(finalSeed, selected.stages.at(-1).return_pc, 'final caller seed comes from actual state');
caller = compile([finalSeed], [], 'caller-6', oracle.module_helpers.callers[5]);
install(caller, oracle.installation.caller_slots[5]); dispatcher();
dispatch(cpuOracle.budget - retired, 'actual final LEA/JMP and missing UD2 transport', [[0, cpu(cpuOracle.final.registers, selected.done)], [56, exit(3, 2)], [transfer, installation(caller, 6)]]);
retired += 2; assert.equal(retired, oracle.fresh_retirement_only.total);
assert.equal(observedNeedCodePC(), selected.done, 'terminal is asserted, never used as a compilation seed');
assert.equal(engine.units.length, oracle.live_units);
stack(selected.final_stack_values, 'final stack neighbors');
read(receipt.image_base + 0x3000, cpuOracle.final_data, 'guest data remains full-u32 after Set0/Get0');
engine.units.forEach(unit => { getUnit(unit, 'all seven positive owners remain current'); unit.entries.forEach(pc => operation('retained installed owner remains exact', () => api.find_installed_resident(engine.low, engine.high, pc), 0, [[transfer, installation(unit, engine.installed.get(unit.id))]])); });
observations.push({phase: 'positive_final', receipt, retired, registers: cpuOracle.final.registers, eip: selected.done, data: cpuOracle.final_data, live_units: engine.units.length});

// after the positive programme, identical code writes deliberately stale old units.
const firstCaller = engine.units[1];
operation('negative stale setup permits an explicit code write', () => api.protect(receipt.entry_point, 1, 7));
operation('identical first code word changes content version', () => api.write32(receipt.entry_point, code.readUInt32LE(0)), 0, [[100, helper()]]);
operation('negative stale setup restores RX', () => api.protect(receipt.entry_point, 1, 5));
operation('stale old module getter rejects without publication', () => api.resident_module(firstCaller.low, firstCaller.high), 4);
operation('stale old installed lookup rejects', () => api.find_installed_resident(engine.low, engine.high, receipt.entry_point), 4);
operation('retained old child guard rejects before CPU effects', () => firstCaller.run(engine.base, engine.base + 56, cpuOracle.budget, engine.base + 96), 4);
getUnit(gate, 'separate gate page and its private owner remain current');
const fresh = compile([receipt.entry_point], [], 'fresh-stale-control', oracle.module_helpers.callers[0]);
engine.table.set(1, fresh.run);
operation('stale occupied slot cannot be reused by a fresh unit', () => api.acknowledge_resident_installation(engine.low, engine.high, fresh.low, fresh.high, 1), 7);
engine.table.set(1, firstCaller.run);
assert.equal(engine.table.get(1), firstCaller.run);
install(fresh, 7);
entriesRequest([0xffffffff]);
operation('full registry capacity precedes invalid seed decoding', () => api.compile_resident_entries(1, 0), 18);
getUnit(fresh, 'capacity failure retains the fresh admitted owner');
getUnit(gate, 'capacity failure retains the independent gate owner');
dispatcher();
read(receipt.image_base + 0x3000, cpuOracle.final_data, 'negative compiler controls retain final guest data');
observations.push({phase: 'negative_stale_capacity', stale_id: firstCaller.id.toString(), current_id: fresh.id.toString(), retained_gate_id: gate.id.toString(), fresh_slot: 7, capacity_status: 18});
operation('close preserves full arena', () => api.close());
operation('Closed precedes invalid entry counts', () => api.compile_resident_entries(0, 9), 5);
operation('Closed before invalid installer identity and slot', () => api.acknowledge_resident_installation(0, 0, 0, 0, 99), 5);
operation('closed retained generated child remains inert', () => fresh.run(engine.base, engine.base + 56, cpuOracle.budget, engine.base + 96), 5);
const provenance = {
  engine_sha256: hash(engineBytes), runtime_oracle_sha256: hash(oracleBytes), reused_program_sha256: hash(programBytes), reused_cpu_oracle_sha256: hash(cpuOracleBytes), reused_pe_sha256: hash(image), artifacts, sources, identities, observations,
  command: [process.execPath, process.argv[1], enginePath, outputDir, root], assembly_command: ['clang', ...assemble],
  tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0]},
  claim: 'one pinned linked PE uses new ordinary resident entry discovery for receipt-derived Gate B and initial entry, then five caller continuations seeded only by canonical observed NeedCode EIP. Seven positive resident units installed in trusted same-memory/table slots execute through the unchanged returning dispatcher and engine-owned Get/Set provider; no caller BlockSpec lengths, preseeded runtime return map, scalar result, IAT/stub patch or poststartup CPU patch. Reused independent LLVM/CPU/stack/data oracle and separately frozen v3 transport produce14 retirements/EAX7/dataf1234567. Compiler cancellation success, ack16/same-unit retry, zero-retirement Cancelled/cold misses, private pending/full-u64 owners, failure preservation, stale occupied slot retention and an eighth load-only fresh-unit capacity control are explicit. Identical code write/protect after positive completion is deliberate stale-input setup, not positive execution. Native tests own full registry/RAM/ID-reservation proof; actual words are bounded observations. No callback discovery, graph/cache/optimizer/auto executor, slot reuse, startup ownership, ExitProcess, SDK/browser/performance/game/full P2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({status: 'ok', engine_sha256: provenance.engine_sha256, runtime_oracle_sha256: provenance.runtime_oracle_sha256, positive_units: oracle.live_units, positive_retired: retired, base: oracle.actual_base, output: outputDir}));
