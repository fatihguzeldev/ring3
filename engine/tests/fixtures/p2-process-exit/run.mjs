import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-process-exit');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json')), oracle = JSON.parse(oracleBytes);
assert.equal(hash(oracleBytes), '87b9865412e5bce7c35453af93fe080b9d7fdccb1bf5ea7b743923dbf5643bad');
const programBytes = readFileSync(join(fixtureRoot, 'program.S'));
assert.equal(hash(programBytes), '965a52f56696c934d64bc58acbe0a496b246983047c2171555c9f26f831d521a');
const size = 4236, transfer = 140;
const artifacts = {}, observations = [], identities = [];

function authoredText(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]);
  assert.equal(view.getUint16(18, true), 3);
  const offset = view.getUint32(32, true), stride = view.getUint16(46, true);
  const sections = Array.from({length: view.getUint16(48, true)}, (_, index) => {
    const at = offset + index * stride;
    return {name: view.getUint32(at, true), type: view.getUint32(at + 4, true), offset: view.getUint32(at + 16, true), size: view.getUint32(at + 20, true), link: view.getUint32(at + 24, true), info: view.getUint32(at + 28, true), stride: view.getUint32(at + 36, true)};
  });
  const sectionNames = sections[view.getUint16(50, true)];
  for (const section of sections) {
    const at = sectionNames.offset + section.name;
    section.name = object.subarray(at, object.indexOf(0, at)).toString('utf8');
  }
  const textIndex = sections.findIndex(section => section.name === '.text'), text = sections[textIndex];
  assert.ok(textIndex > 0);
  assert.ok(!sections.some(section => [4, 9].includes(section.type) && section.info === textIndex && section.size > 0));
  const code = object.subarray(text.offset, text.offset + text.size);
  const table = sections.find(section => section.type === 2);
  assert.ok(table);
  const names = sections[table.link], symbols = {};
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    const nameAt = names.offset + view.getUint32(at, true);
    const name = object.subarray(nameAt, object.indexOf(0, nameAt)).toString('utf8');
    const section = view.getUint16(at + 14, true), start = view.getUint32(at + 4, true), length = view.getUint32(at + 8, true);
    if (section === textIndex && length > 0) symbols[name] = {offset: start, size: length, hex: code.subarray(start, start + length).toString('hex')};
  }
  assert.deepEqual(symbols, oracle.program.symbols);
  assert.equal(code.length, oracle.program.text_length);
  assert.equal(code.toString('hex'), oracle.program.text_hex);
  assert.equal(hash(code), oracle.program.text_sha256);
  return code;
}

// independent literal pe input construction; runtime entry authority comes from rust.
function authoredPe(code) {
  const bytes = Buffer.alloc(1536), view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  bytes.write('MZ'); view.setUint32(0x3c, 0x80, true); bytes.write('PE\0\0', 0x80);
  view.setUint16(0x84, 0x14c, true); view.setUint16(0x86, 2, true);
  view.setUint16(0x94, 224, true); view.setUint16(0x96, 0x103, true);
  const optional = 0x98;
  view.setUint16(optional, 0x10b, true);
  for (const [at, value] of [[4, 512], [8, 512], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, 0x400000], [32, 4096], [36, 512], [56, 0x4000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16]]) view.setUint32(optional + at, value, true);
  view.setUint16(optional + 40, 4, true); view.setUint16(optional + 48, 4, true);
  view.setUint16(optional + 68, 3, true); view.setUint16(optional + 70, 0x100, true);
  [['.text', 258, 0x1000, 512, 0x60000020], ['.data', 4, 0x3000, 1024, 0xc0000040]].forEach(([name, virtualSize, rva, raw, flags], index) => {
    const at = 0x178 + index * 40; bytes.write(name, at);
    for (const [field, value] of [[8, virtualSize], [12, rva], [16, 512], [20, raw], [36, flags]]) view.setUint32(at + field, value, true);
  });
  bytes.fill(0xcc, 512, 1024); bytes.set(code, 512);
  return bytes;
}
const assembly = join(fixtureRoot, 'program.S'), objectPath = join(outputDir, 'program.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
writeFileSync(join(outputDir, 'program.disassembly.txt'), execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]));
artifacts['program.o'] = hash(readFileSync(objectPath));
artifacts['program.disassembly.txt'] = hash(readFileSync(join(outputDir, 'program.disassembly.txt')));
const code = authoredText(readFileSync(objectPath)), image = authoredPe(code);
assert.equal(hash(image), oracle.pe.sha256);
writeFileSync(join(outputDir, 'exit.exe'), image); artifacts['exit.exe'] = hash(image);
writeFileSync(join(outputDir, 'program.x86'), code); artifacts['program.x86'] = hash(code);
writeFileSync(join(outputDir, 'program.S'), programBytes);
writeFileSync(join(outputDir, 'terminal-oracle.json'), oracleBytes);
writeFileSync(join(outputDir, 'run.mjs'), readFileSync(import.meta.filename));
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', 'engine/tests/process_exit_wasm.rs', 'engine/tests/fixtures/p2-process-exit/program.S', 'engine/tests/fixtures/p2-process-exit/oracle.json', 'engine/tests/fixtures/p2-process-exit/run.mjs'];
const sources = Object.fromEntries(sourcePaths.map(file => [file, hash(readFileSync(join(root, file)))]));
writeFileSync(join(outputDir, 'source-sha256.json'), JSON.stringify(sources, null, 2));
const engineBytes = readFileSync(enginePath);
writeFileSync(join(outputDir, 'engine.wasm'), engineBytes);
assert.ok(WebAssembly.validate(engineBytes));
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const names = ['open', 'close', 'arena_ptr', 'load_pe32', 'start_loaded_image', 'map', 'write32', 'compile_entries', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'resident_module', 'find_resident', 'acknowledge_resident_installation', 'find_installed_resident', 'dispatcher_module', 'guard', 'guard_dispatch_entry', 'guard_resident', 'read32', 'store32', 'store_resident32', 'capture_call', 'capture_resident_call', 'complete_call', 'complete_resident_call', 'complete_windows_call', 'complete_resident_windows_call'];
let engine, nextInstance = 0;
const bytesFromHex = hex => new Uint8Array(Buffer.from(hex, 'hex'));

function refresh() { if (engine.buffer !== engine.memory.buffer) { engine.buffer = engine.memory.buffer; engine.bytes = new Uint8Array(engine.buffer); engine.view = new DataView(engine.buffer); } return engine; }
function arena() { return refresh().bytes.slice(engine.base, engine.base + size); }
function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true)); return bytes;
}
function words(fields) { const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer); fields.forEach((value, index) => view.setUint32(index * 4, value, true)); return bytes; }
function cpu(registers, pc) { return record('R3ST', 56, [...registers, pc, oracle.runtime.eflags]); }
function exit(reason, retired, detail = 0, version = 3) { return record('R3EX', 40, [reason, retired, detail, 0, 0, 0], version); }
function helper(value = 0) { return record('R3MH', 40, [0, value, 0, 0, 0, 0]); }
function residentRecord(unit) { return words([1, 24, unit.low, unit.high, unit.pointer, unit.length]); }
function installation(unit, slot) { return record('R3IN', 32, [unit.low, unit.high, slot, 0]); }
function operation(label, action, status = 0, patches = []) {
  const expected = arena(); for (const [offset, bytes] of patches) expected.set(bytes, offset);
  assert.equal(action(), status, `${engine.name}/${label}: status or value`); assert.deepEqual(arena(), expected, `${engine.name}/${label}: whole4236 arena`);
}
function cancel(value) { refresh().view.setUint32(engine.base + 96, value, true); }
function read(address, value, label) { operation(label, () => engine.api.read32(address), 0, [[100, helper(value)]]); }
function stack(values, label) { oracle.runtime.stack_addresses.forEach((address, index) => read(address, values[index], `${label}/word${index}`)); }
function entriesRequest(entries, gates = []) {
  const view = refresh().view;
  entries.forEach((pc, index) => view.setUint32(engine.base + transfer + index * 4, pc, true));
  gates.forEach(([pc, id], index) => { const offset = engine.base + transfer + entries.length * 4 + index * 8; view.setUint32(offset, pc, true); view.setUint32(offset + 4, id, true); });
}
function instantiate(unit, helpers) {
  refresh(); assert.ok(unit.pointer > 0 && unit.length > 8 && unit.pointer + unit.length <= engine.bytes.length);
  unit.bytes = engine.bytes.slice(unit.pointer, unit.pointer + unit.length);
  const module = new WebAssembly.Module(unit.bytes), imports = WebAssembly.Module.imports(module);
  const sort = rows => rows.sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sort(imports), sort([{module: 'env', name: 'memory', kind: 'memory'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))]));
  const allowed = new Set(['guard', 'guard_resident', 'store32', 'store_resident32']);
  const binding = Object.fromEntries(imports.filter(item => item.module === 'ring3').map(item => { assert.ok(allowed.has(item.name)); return [item.name, engine.api[item.name]]; }));
  unit.instance = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: binding}); unit.run = unit.instance.exports.run;
  assert.equal(typeof unit.run, 'function'); assert.equal(unit.run.length, 4);
  engine.units.push(unit);
  const filename = `${engine.name}-${unit.label}.wasm`; writeFileSync(join(outputDir, filename), unit.bytes); artifacts[filename] = hash(unit.bytes);
  identities.push({instance: engine.ordinal, owner: engine.owner, key: engine.key.toString(), label: unit.label, id: unit.id?.toString(), generation: unit.generation, pointer: unit.pointer, length: unit.length, module_sha256: hash(unit.bytes)});
  return unit;
}
function replacement(entries, gates) {
  entriesRequest(entries, gates);
  operation('replacement entry compiler publishes no transfer metadata', () => engine.api.compile_entries(entries.length, gates.length));
  assert.equal(engine.api.generation(), oracle.transport.replacement.generation);
  return instantiate({label: 'replacement', generation: engine.api.generation(), pointer: engine.api.module_ptr() >>> 0, length: engine.api.module_len() >>> 0, entries}, oracle.transport.replacement.helper_imports);
}
function resident(entries, gates, label, helpers) {
  entriesRequest(entries, gates);
  const before = arena(); assert.equal(engine.api.compile_resident_entries(entries.length, gates.length), 0);
  const after = arena(), view = new DataView(after.buffer, transfer, 24);
  const unit = {low: view.getUint32(8, true), high: view.getUint32(12, true), pointer: view.getUint32(16, true), length: view.getUint32(20, true), entries, label};
  unit.id = BigInt(unit.low) | (BigInt(unit.high) << 32n);
  assert.notEqual(unit.id, 0n); assert.ok(!engine.units.some(item => item.id === unit.id));
  const expected = before.slice(); expected.set(residentRecord(unit), transfer); assert.deepEqual(after, expected, 'raw resident24 metadata only');
  instantiate(unit, helpers);
  entries.forEach(pc => operation('logical resident ownership matches returned full id', () => engine.api.find_resident(pc), 0, [[transfer, residentRecord(unit)]]));
  return unit;
}
function install(unit, slot) {
  operation('compiled resident is not installed', () => engine.api.find_installed_resident(engine.low, engine.high, unit.entries[0]), 17);
  engine.table.set(slot, unit.run);
  operation('ack publishes only exact resident installation', () => engine.api.acknowledge_resident_installation(engine.low, engine.high, unit.low, unit.high, slot), 0, [[transfer, installation(unit, slot)]]);
  assert.equal(engine.table.get(slot), unit.run);
  unit.entries.forEach(pc => operation('installed current owner lookup', () => engine.api.find_installed_resident(engine.low, engine.high, pc), 0, [[transfer, installation(unit, slot)]]));
}
function dispatcher() {
  const before = arena(); assert.equal(engine.api.dispatcher_module(engine.low, engine.high), 0);
  const after = arena(), view = new DataView(after.buffer, transfer, 32), pointer = view.getUint32(24, true), length = view.getUint32(28, true);
  const expected = before.slice(); expected.set(record('R3DP', 32, [engine.low, engine.high, pointer, length]), transfer); assert.deepEqual(after, expected);
  refresh(); assert.ok(pointer > 0 && length > 8 && pointer + length <= engine.bytes.length);
  const bytes = engine.bytes.slice(pointer, pointer + length), module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'}, {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'}, {module: 'ring3', name: 'find_installed_resident', kind: 'function'}]);
  engine.dispatcher = {pointer, length, bytes, instance: new WebAssembly.Instance(module, {env: {memory: engine.memory, table: engine.table}, ring3: {guard_dispatch_entry: engine.api.guard_dispatch_entry, find_installed_resident: engine.api.find_installed_resident}})};
  engine.dispatcher.run = engine.dispatcher.instance.exports.run; assert.equal(engine.dispatcher.run.length, 4);
  const filename = `${engine.name}-returning-dispatcher.wasm`; writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes);
}
function setup(owner, name = owner) {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => { const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); return [name, fn]; }));
  assert.equal(api.complete_windows_call.length, 4); assert.equal(api.complete_resident_windows_call.length, 5);
  const ordinal = ++nextInstance, key = 0xa344cdef00000000n + BigInt(ordinal);
  engine = {owner, name, ordinal, api, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n), memory: instance.exports.memory, units: []};
  assert.ok(engine.memory instanceof WebAssembly.Memory);
  assert.equal(api.open(oracle.runtime.pages, engine.low, engine.high), 0);
  engine.base = api.arena_ptr() >>> 0; refresh(); assert.ok(engine.base > 0 && engine.base + size <= engine.bytes.length);
  const initialContext = arena().slice(0, 96);
  assert.deepEqual(initialContext.subarray(0, 56), cpu([0, 0, 0, 0, 0, 0, 0, 0], 0));
  assert.deepEqual(initialContext.subarray(56), exit(1, 0, 0, 1));
  engine.bytes.fill(0xa5, engine.base + 100, engine.base + size);
  refresh().bytes.set(image, engine.base + transfer);
  operation('fixed import-free Rust PE loader receipt', () => api.load_pe32(image.length), 0, [[transfer, record('R3PE', 32, [oracle.pe.preferred_base, oracle.pe.image_size, oracle.pe.entry_point, oracle.pe.mapped_pages])]]);
  const pe = new DataView(arena().buffer, transfer, 32);
  engine.entry = pe.getUint32(24, true);
  const gatePC = engine.entry + oracle.program.symbols.exit_gate.offset, canaryPC = engine.entry + oracle.program.symbols.exit_canary.offset;
  assert.equal(gatePC, oracle.runtime.gate_pc); assert.equal(canaryPC, oracle.runtime.return_canary);
  const gates = [[gatePC, oracle.runtime.gate_id]];
  if (owner === 'resident') {
    engine.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
    engine.gate = resident([gatePC], gates, 'gate-B', oracle.transport.resident.gate_helper_imports); install(engine.gate, 0);
    engine.caller = resident([engine.entry, canaryPC], [], 'caller-A-with-canary', oracle.transport.resident.caller_helper_imports); install(engine.caller, 1);
    dispatcher(); assert.equal(api.generation(), 0);
  } else {
    engine.caller = replacement([engine.entry, canaryPC, gatePC], gates); engine.gate = engine.caller;
  }
  assert.deepEqual(arena().subarray(0, 96), initialContext, 'precompiled owners retain the supported initial context');
  operation('engine startup owns all CPU and empty stack initialization', () => api.start_loaded_image(oracle.runtime.stack_base, oracle.runtime.stack_pages), 0, [[0, bytesFromHex(oracle.records.startup_state_hex)], [56, bytesFromHex(oracle.records.startup_exit_hex)]]);
  stack(oracle.runtime.stack_initial, 'empty stack before real guest CALL'); read(oracle.runtime.data_address, 0, 'canary data initially zero');
  return engine;
}
function run(target, budget, label, patches = [], status = 0, pointers = [engine.base, engine.base + 56, engine.base + 96]) {
  operation(label, () => target.run(pointers[0], pointers[1], budget, pointers[2]), status, patches);
}
function capture() {
  const owner = engine.gate;
  if (engine.owner === 'resident') {
    const view = refresh().view;
    assert.deepEqual([...engine.bytes.slice(engine.base + transfer, engine.base + transfer + 32)], [...installation(owner, 0)]);
    const low = view.getUint32(engine.base + transfer + 16, true), high = view.getUint32(engine.base + transfer + 20, true);
    operation('private resident Gate B capture', () => engine.api.capture_resident_call(engine.low, engine.high, low, high, 2, 1), 0, [[transfer, bytesFromHex(oracle.capture.frame_before_tamper_hex)]]);
  } else operation('private replacement capture', () => engine.api.capture_call(engine.low, engine.high, owner.generation, 2, 1), 0, [[transfer, bytesFromHex(oracle.capture.frame_before_tamper_hex)]]);
}
function provider(token = oracle.capture.token) {
  const owner = engine.gate;
  return engine.owner === 'resident'
    ? engine.api.complete_resident_windows_call(engine.low, engine.high, owner.low, owner.high, token)
    : engine.api.complete_windows_call(engine.low, engine.high, owner.generation, token);
}
function scalar() {
  const owner = engine.gate;
  return engine.owner === 'resident'
    ? engine.api.complete_resident_call(engine.low, engine.high, owner.low, owner.high, oracle.capture.token, 0)
    : engine.api.complete_call(engine.low, engine.high, owner.generation, oracle.capture.token, 0);
}
function retainedBytes() {
  refresh();
  for (const unit of engine.units) assert.deepEqual(engine.bytes.slice(unit.pointer, unit.pointer + unit.length), unit.bytes, 'retained module bytes/pointer remains exact');
  if (engine.dispatcher) {
    assert.deepEqual(engine.bytes.slice(engine.dispatcher.pointer, engine.dispatcher.pointer + engine.dispatcher.length), engine.dispatcher.bytes);
    assert.equal(engine.table.get(0), engine.gate.run); assert.equal(engine.table.get(1), engine.caller.run);
  }
}

for (const owner of oracle.runtime.cases) {
  setup(owner);
  const primary = engine.dispatcher ?? engine.caller, base = engine.base, memory = engine.memory;
  const gatePatches = [[0, bytesFromHex(oracle.records.gate_state_hex)], [56, bytesFromHex(oracle.records.gate_exit_hex)], [100, helper()]];
  if (owner === 'resident') gatePatches.push([transfer, installation(engine.gate, 0)]);
  run(primary, oracle.runtime.budget, 'actual MOV/PUSH/CALL and zero-retirement Gate', gatePatches);
  stack(oracle.runtime.stack_at_gate, 'real return and exit argument'); read(oracle.runtime.data_address, 0, 'precompiled return canary has not run');
  capture();
  // explicit negative input tampering challenges the saved private argument only.
  refresh().view.setUint32(engine.base + transfer + 48, oracle.capture.diagnostic_arg_tamper, true);
  operation('mutable guest argument differs from the captured private argument', () => engine.api.write32(0x8ffc, oracle.capture.guest_arg_tamper), 0, [[100, helper()]]);
  operation('reserved ExitProcess cannot return via generic scalar completion', scalar, oracle.negative_controls.scalar_reserved_ID_refusal);
  cancel(1); operation('cancelled terminal completion retains pending frame and retry', provider, oracle.negative_controls.cancel_completion); cancel(0);
  operation('ExitProcess owns fresh v4 receipt and does not return', provider, 0, [[56, bytesFromHex(oracle.records.terminal_exit_hex)]]);
  assert.deepEqual(arena().subarray(0, 56), bytesFromHex(oracle.records.gate_state_hex));
  const terminalArena = arena(), terminalFile = `${owner}-terminal-arena.bin`;
  writeFileSync(join(outputDir, terminalFile), terminalArena); artifacts[terminalFile] = hash(terminalArena);
  stack(oracle.capture.stack_after_tamper, 'terminal completion leaves captured stack words intact'); read(oracle.runtime.data_address, 0, 'terminal never runs return store');
  operation('terminal replay wins before wrong token', () => provider(0), 21);
  operation('terminal scalar replay is inert', scalar, 21);
  operation('terminal precedes malformed stack-map input', () => engine.api.map(1, 0, 99), 21);
  operation('terminal prevents direct guest write and helper publication', () => engine.api.write32(oracle.runtime.data_address, 0xdeadbeef), 21);
  operation('terminal precedes invalid replacement compiler counts', () => engine.api.compile_entries(0, 9), 21);
  operation('terminal precedes invalid resident compiler counts', () => engine.api.compile_resident_entries(0, 9), 21);
  operation('terminal precedes invalid startup input', () => engine.api.start_loaded_image(1, 0), 21);
  operation('terminal precedes malformed loaded-image request', () => engine.api.load_pe32(0), 21);
  operation('value getter module_ptr remains zero on liveguard failure', () => engine.api.module_ptr(), 0);
  operation('value getter module_len remains zero on liveguard failure', () => engine.api.module_len(), 0);
  operation('status-bearing dispatcher metadata is refused', () => engine.api.dispatcher_module(0, 0), 21);
  if (owner === 'resident') operation('status-bearing resident metadata is refused', () => engine.api.resident_module(0, 0), 21);
  operation('existing open allocation remains occupied', () => engine.api.open(0, 0, 0), 6);
  assert.equal(engine.api.arena_ptr(), base); assert.equal(engine.api.generation(), owner === 'resident' ? 0 : 1);
  assert.equal(engine.memory, memory); retainedBytes();
  observations.push({owner, phase: 'terminal', prefix_retired: 3, fresh_terminal_retired: 0, exit_code: oracle.runtime.exit_code, state_hex: Buffer.from(terminalArena.subarray(0, 56)).toString('hex'), exit_hex: Buffer.from(terminalArena.subarray(56, 96)).toString('hex'), diagnostic_argument: oracle.capture.diagnostic_arg_tamper, guest_argument: oracle.capture.guest_arg_tamper, data: 0});
  // explicit postterminal arena restoration cannot change the private terminal latch.
  refresh().bytes.set(cpu(oracle.canary_restore.registers, oracle.canary_restore.eip), engine.base);
  engine.bytes.set(bytesFromHex(oracle.canary_restore.exit_hex), engine.base + 56);
  for (const target of new Set([engine.caller, engine.gate, engine.dispatcher].filter(Boolean))) {
    run(target, oracle.runtime.budget, 'retained executable canary is blocked after valid CPU/Exit restoration', [], 21);
    cancel(1); run(target, 0, 'terminal wins before cancellation and zero budget', [], 21); cancel(0);
    run(target, 0, 'terminal wins before malformed pointers', [], 21, [0xffffffff, 0xffffffff, 0xffffffff]);
  }
  retainedBytes(); read(oracle.runtime.data_address, 0, 'restored supported canary still cannot store');
  stack(oracle.capture.stack_after_tamper, 'rejected generated runs leave guest words intact');
  operation('close preserves the arena tombstone', () => engine.api.close());
  run(engine.caller, oracle.runtime.budget, 'Closed replaces terminal guard for retained caller', [], 5);
  operation('Closed precedes invalid provider owner and token', () => provider(0), 5);
  operation('Closed precedes diagnostic guest read', () => engine.api.read32(oracle.runtime.data_address), 5);
}

// a fresh instance with forged terminal output remains active without private publication.
setup('replacement', 'forged-terminal-output');
refresh().bytes.set(bytesFromHex(oracle.records.terminal_exit_hex), engine.base + 56);
const forged = oracle.forged_terminal_without_private_latch;
run(engine.caller, forged.budget, 'forged terminal bytes do not create process ownership', [[0, cpu(forged.registers_after_MOV, forged.eip_after_MOV)], [56, bytesFromHex(forged.exit_hex_after_MOV)]]);
stack(oracle.runtime.stack_initial, 'one MOV does not synthesize or write stack'); read(oracle.runtime.data_address, 0, 'fresh forged-output control retains canary zero');
observations.push({owner: 'fresh-forged-terminal', phase: 'diagnostic_only', status: 0, retired: 1, registers: forged.registers_after_MOV, eip: forged.eip_after_MOV, data: 0});
operation('fresh nonterminal close', () => engine.api.close());
const provenance = {
  engine_sha256: hash(engineBytes), oracle_sha256: hash(oracleBytes), program_sha256: hash(programBytes), text_sha256: hash(code), pe_sha256: hash(image), artifacts, sources, identities, observations,
  command: [process.execPath, process.argv[1], enginePath, outputDir, root], assembly_command: ['clang', ...assemble],
  tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0]},
  claim: 'a separately authored fixed import-free PE uses engine-owned empty-stack startup and real MOV/PUSH/CALL to a numeric ExitProcess Gate. Replacement and resident plus returning dispatcher paths each retire3, capture a private Stdcall1 frame and publish fresh terminal v4/retired0/full-u32 code without caller return, ESP cleanup or State/RAM modification. Diagnostic CF and guest argument tampering are explicit negative inputs; cancel16 and scalar7 retain the same-token pending owner for private-code retry. A supported store canary was compiled before startup and never runs, even after documented postterminal raw CPU/Exit restoration; retained caller/Gate/dispatcher reject21 before valid or malformed pointer/cancel/budget inputs. Retained raw module bytes, allocation metadata and postmortem bounded reads survive until close5; live metadata status APIs reject21 and legacy value pointers/lengths stay0. A separate fresh replacement executes one MOV despite forged terminal Exit bytes, proving private latch authority. Native tests own full RAM/owner/version/latch/LastError/codec/linker rejection matrix; actual word observations are bounded. Host Memory/Table provenance remains trusted. No named ExitProcess import linking, broad Windows cleanup/threads/handles/startup, reset/scheduler, SDK/browser/performance/game/full P2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({status: 'ok', engine_sha256: provenance.engine_sha256, oracle_sha256: provenance.oracle_sha256, cases: oracle.runtime.cases, prefix_retired_per_case: 3, terminal_retired_per_case: 0, forged_output_retired: 1, output: outputDir}));
