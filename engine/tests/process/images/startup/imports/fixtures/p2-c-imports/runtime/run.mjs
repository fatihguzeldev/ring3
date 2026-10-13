import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'expected engine, output directory and repository root');
const fixture = 'engine/tests/process/images/startup/imports/fixtures/p2-c-imports';
const SIZE = ARENA_SIZE, T = 140, STACK = 0x8000, STACK_TOP = 0x9000;
const KEY_LOW = 0x43494d50, KEY_HIGH = 0x4f525453;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engine = readEngine(enginePath);
const initial = readFileSync(join(output, 'initial-arena.bin'));
const image = readFileSync(join(root, fixture, 'program.exe'));
const oracleBytes = readFileSync(join(root, fixture, 'oracle.json'));
const oracle = JSON.parse(oracleBytes);
assert.equal(initial.length, SIZE);
assert.equal(oracle.schema, 'ring3.c-imports.v1');
assert.equal(image.length, oracle.image.bytes);
assert.equal(hash(image), oracle.image.sha256);
const originalText = Buffer.from(oracle.original_text.hex, 'hex');
assert.equal(hash(originalText), oracle.original_text.sha256);
assert.deepEqual(image.subarray(oracle.original_text.file_offset,
  oracle.original_text.file_offset + originalText.length), originalText);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
assert.deepEqual(oracle.caller_entry_rvas, [0x1000, 0x100f, 0x1017, 0x1020, 0x1022]);
assert.deepEqual(oracle.expected_output_words, [0xf1234567, 0, 0x52494e47]);

const artifacts = {}, contexts = [], modules = [], observations = [], helpers = [], providers = [], diagnostics = [];
const counts = {contexts: 0, modules: 0, dispatcher_modules: 0, runs: 0, retired: 0,
  helper_calls: 0, read32: 0, store32: 0, cold_exits: 0, captures: 0,
  provider_completions: 0, wrong_token_refusals: 0, replay_refusals: 0, diagnostic_words: 0};
function save(name, bytes) {
  writeFileSync(join(output, name), bytes, {flag: 'wx'});
  artifacts[name] = {bytes: bytes.length, sha256: hash(bytes)};
  return name;
}
const sourcePaths = [
  `${fixture}/runtime/run.mjs`, `${fixture}/oracle.json`, `${fixture}/entry.c`, `${fixture}/kernel32.def`,
  `${fixture}/build.py`, `${fixture}/program.exe`, `${fixture}/entry.obj`, `${fixture}/kernel32.lib`,
  `${fixture}/receipt.json`, 'engine/tests/process/images/startup/imports/integer/native.rs', 'engine/tests/process/images/native.rs', 'engine/tests/process/images/startup/imports/integer/wasm.rs', 'engine/tests/process/images/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/support/wasm/engine.mjs', 'engine/src/process/loading/input.rs', 'engine/src/process/loading/mod.rs', 'engine/src/process/mod.rs',
  'engine/src/process/loading/startup.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/execution/installation.rs',
  'engine/src/process/calls/call.rs', 'engine/src/process/calls/context.rs', 'engine/src/process/calls/mod.rs', 'engine/src/process/windows/calls.rs', 'engine/src/process/windows/mod.rs', 'engine/src/windows/provider.rs',
  'engine/src/windows/calling_convention.rs', 'engine/src/cpu/dbt/wasm/dispatcher.rs',
];
const sourceHashes = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcesBefore = sourceHashes();
save('engine.wasm', engine.bytes);
save('input-program.exe', image);
save('oracle.json', oracleBytes);

function word(value) {
  const bytes = Buffer.alloc(4);
  bytes.writeUInt32LE(value >>> 0);
  return bytes;
}
const words = values => Buffer.concat(values.map(word));
function record(magic, version, length, values) {
  const bytes = Buffer.alloc(length);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(length, 8);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4));
  return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 1, 56, [...registers, pc, flags]);
const exit = (version, reason, retired, detail = 0) => record('R3EX', version, 40, [reason, retired, detail, 0, 0, 0]);
const cpuPacket = (value = 0) => record('R3MH', 10, 40, [0, value, 0, 0, 0, 4]);
const packet = (value = 0) => record('R3MH', 1, 40, [0, value, 0, 0, 0, 0]);
const residentRecord = unit => words([1, 24, unit.low, unit.high, unit.pointer, unit.length]);
const installation = unit => record('R3IN', 1, 32, [unit.low, unit.high, unit.slot, 0]);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, bytes) {
  const expected = arena(c);
  expected.set(bytes, T);
  new Uint8Array(c.memory.buffer, c.base + T, bytes.length).set(bytes);
  assert.deepEqual(arena(c), expected, 'only declared transfer input changes');
}
function host(c, label, action, patches = [], status = 0) {
  const expected = arena(c);
  for (const [offset, bytes] of patches) expected.set(bytes, offset);
  assert.equal(action(), status, label);
  assert.deepEqual(arena(c), expected, `${label}: full4612`);
}
function cpu(c) {
  const bytes = arena(c);
  return {registers: Array.from({length: 8}, (_, index) => bytes.readUInt32LE(16 + index * 4)),
    pc: bytes.readUInt32LE(48), flags: bytes.readUInt32LE(52)};
}

// this projection applies only the independently listed relocations and linked iat slots.
function expectedImage(actualBase) {
  const bytes = Buffer.alloc(oracle.image.size);
  image.copy(bytes, 0, 0, oracle.image.headers_size);
  for (const section of oracle.sections) {
    image.copy(bytes, section.rva, section.raw_offset, section.raw_offset + section.raw_size);
  }
  assert.deepEqual(bytes.subarray(0x2000, 0x2000 + 109), Buffer.from(oracle.imports.original_rdata_hex, 'hex'));
  assert.deepEqual(bytes.subarray(0x4000, 0x4000 + 20), Buffer.from(oracle.relocations.hex, 'hex'));
  oracle.relocations.highlow_rvas.forEach((rva, index) => {
    assert.equal(bytes.readUInt32LE(rva), oracle.relocations.preferred_values[index]);
    bytes.writeUInt32LE((oracle.relocations.preferred_values[index] + actualBase - oracle.image.preferred_base) >>> 0, rva);
  });
  oracle.gates.forEach((gate, index) => bytes.writeUInt32LE(gate.pc, oracle.imports.iat_rva + index * 4));
  return bytes;
}
function ramRead(c, address, length) {
  if (address >= c.actualBase && address + length <= c.actualBase + c.imageBytes.length) {
    return c.imageBytes.subarray(address - c.actualBase, address - c.actualBase + length);
  }
  assert.ok(address >= STACK && address + length <= STACK_TOP);
  return c.stack.subarray(address - STACK, address - STACK + length);
}
function ramWrite(c, address, value) {
  if (address >= c.actualBase && address + 4 <= c.actualBase + c.imageBytes.length) {
    c.imageBytes.writeUInt32LE(value, address - c.actualBase);
  } else {
    assert.ok(address >= STACK && address + 4 <= STACK_TOP);
    c.stack.writeUInt32LE(value, address - STACK);
  }
}
function checkRam(c, address, expected, label) {
  assert.equal(expected.length % 4, 0);
  for (let offset = 0; offset < expected.length; offset += 4) {
    const value = expected.readUInt32LE(offset);
    host(c, label, () => c.api.read32(address + offset), [[100, packet(value)]]);
    diagnostics.push({context: c.id, label, address: address + offset, expected_value: value});
    counts.diagnostic_words++;
  }
}
function stackProjection(values) {
  const bytes = Buffer.alloc(64);
  oracle.stack_addresses.forEach((address, index) => bytes.writeUInt32LE(values[index], address - 0x8fc0));
  return bytes;
}
function dataProjection(values) {
  const bytes = Buffer.alloc(32);
  words(values).copy(bytes);
  return bytes;
}
function bindHelper(c, unit, name) {
  return (...raw) => {
    const before = arena(c), args = raw.map(value => value >>> 0);
    if (name === 'guard_resident') {
      assert.deepEqual(args, [KEY_LOW, KEY_HIGH, unit.low, unit.high, c.base, c.base + 56, c.base + 96]);
      assert.equal(c.api[name](...raw), 0);
      assert.deepEqual(arena(c), before, 'resident guard is neutral');
      return 0;
    }
    const access = c.accesses.shift();
    assert.ok(access, 'every generated helper has an original literal access');
    let expectedPacket;
    if (name === 'cpu_read32') {
      assert.equal(access.kind, 'read');
      assert.deepEqual(args, [access.address]);
      assert.equal(ramRead(c, access.address, 4).readUInt32LE(), access.value);
      expectedPacket = cpuPacket(access.value);
      counts.read32++;
    } else {
      assert.equal(name, 'cpu_store_resident32');
      assert.equal(access.kind, 'store');
      assert.deepEqual(args, [KEY_LOW, KEY_HIGH, unit.low, unit.high, access.address, access.value]);
      ramWrite(c, access.address, access.value);
      expectedPacket = cpuPacket();
      counts.store32++;
    }
    assert.equal(c.api[name](...raw), 0, name);
    const expected = Buffer.from(before);
    expected.set(expectedPacket, 100);
    assert.deepEqual(arena(c), expected, `${name}: only its40-byte checked packet changes`);
    c.latest = arena(c);
    helpers.push({context: c.id, phase: c.phase, name, args, packet_hex: expectedPacket.toString('hex')});
    counts.helper_calls++;
    return 0;
  };
}
function captureUnit(c, label, entries, gates, slot) {
  input(c, words([...entries, ...gates.flatMap(gate => [gate.pc, gate.id])]));
  const before = arena(c);
  assert.equal(c.api.compile_resident_entries(entries.length, gates.length), 0);
  const after = arena(c);
  const unit = {label, entries, slot, low: after.readUInt32LE(T + 8), high: after.readUInt32LE(T + 12),
    pointer: after.readUInt32LE(T + 16), length: after.readUInt32LE(T + 20)};
  assert.ok(unit.low !== 0 || unit.high !== 0);
  assert.ok(!c.units.some(other => other.low === unit.low && other.high === unit.high));
  before.set(residentRecord(unit), T);
  assert.deepEqual(after, before, 'resident capture changes only its24-byte receipt');
  assert.ok(unit.pointer > 0 && unit.length > 8 && unit.length <= 65536);
  assert.ok(unit.pointer + unit.length <= c.memory.buffer.byteLength);
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, unit.pointer, unit.length));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module);
  const expectedHelpers = ['guard_resident'];
  if (label === 'caller-1' || label === 'caller-2') expectedHelpers.push('cpu_read32');
  if (label !== 'gates') expectedHelpers.push('cpu_store_resident32');
  assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'},
    ...expectedHelpers.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  const ring3 = Object.fromEntries(expectedHelpers.map(name => [name, bindHelper(c, unit, name)]));
  unit.instance = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3});
  assert.equal(unit.instance.exports.run.length, 4);
  c.table.set(slot, unit.instance.exports.run);
  host(c, 'acknowledge exact table installation', () => c.api.acknowledge_resident_installation(
    KEY_LOW, KEY_HIGH, unit.low, unit.high, slot), [[T, installation(unit)]]);
  c.units.push(unit);
  const file = save(`context-${c.id}-${label}.wasm`, bytes);
  modules.push({context: c.id, label, file, sha256: hash(bytes), bytes: bytes.length,
    entries, id_low: unit.low, id_high: unit.high, slot, imports});
  counts.modules++;
  return unit;
}
function makeDispatcher(c) {
  const before = arena(c);
  assert.equal(c.api.dispatcher_module(KEY_LOW, KEY_HIGH), 0);
  const after = arena(c), pointer = after.readUInt32LE(T + 24), length = after.readUInt32LE(T + 28);
  before.set(record('R3DP', 1, 32, [KEY_LOW, KEY_HIGH, pointer, length]), T);
  assert.deepEqual(after, before, 'dispatcher metadata is its only mutation');
  assert.ok(pointer > 0 && length > 8 && pointer + length <= c.memory.buffer.byteLength);
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length));
  const module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'},
    {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'},
    {module: 'ring3', name: 'find_installed_resident', kind: 'function'},
  ]);
  const ring3 = {
    guard_dispatch_entry: (...raw) => {
      assert.deepEqual(raw.map(value => value >>> 0), [KEY_LOW, KEY_HIGH, c.base, c.base + 56, c.base + 96]);
      host(c, 'dispatcher entry guard', () => c.api.guard_dispatch_entry(...raw));
      return 0;
    },
    find_installed_resident: (...raw) => {
      const args = raw.map(value => value >>> 0);
      assert.deepEqual(args.slice(0, 2), [KEY_LOW, KEY_HIGH]);
      const unit = c.units.find(candidate => candidate.entries.includes(args[2]));
      host(c, 'exact installed owner lookup', () => c.api.find_installed_resident(...raw),
        unit ? [[T, installation(unit)]] : [], unit ? 0 : 17);
      c.latest = arena(c);
      return unit ? 0 : 17;
    },
  };
  c.dispatcher = new WebAssembly.Instance(module, {env: {memory: c.memory, table: c.table}, ring3});
  assert.equal(c.dispatcher.exports.run.length, 4);
  save(`context-${c.id}-dispatcher.wasm`, bytes);
  counts.dispatcher_modules++;
}
function run(c, label, budget, retired, reason, registers, pc, flags, version, detail = 0, accesses = []) {
  const before = arena(c), helperStart = helpers.length;
  c.latest = before;
  c.phase = label;
  c.accesses = [...accesses];
  assert.equal(c.dispatcher.exports.run(c.base, c.base + 56, budget, c.base + 96), 0, label);
  assert.equal(c.accesses.length, 0, 'the complete original access sequence executes once');
  const expected = Buffer.from(before);
  expected.set(c.latest.subarray(100, 140), 100);
  expected.set(c.latest.subarray(T, T + 32), T);
  expected.set(state(registers, pc, flags), 0);
  expected.set(exit(version, reason, retired, detail), 56);
  assert.deepEqual(arena(c), expected, `${label}: full4612 with unchanged FP/SIMD/segments/work`);
  const file = save(`context-${c.id}-${label}-arena.bin`, arena(c));
  observations.push({context: c.id, label, budget, retired, reason, exit_version: version,
    cpu: cpu(c), helper_calls: helpers.length - helperStart, arena: file});
  counts.runs++;
  counts.retired += retired;
  if (reason === 3) counts.cold_exits++;
}
function observedCold(c, expectedPC) {
  const bytes = arena(c);
  assert.deepEqual(bytes.subarray(56, 96), exit(3, 3, 0));
  const observedPC = cpu(c).pc;
  assert.equal(observedPC, expectedPC, 'original return/entry PC only asserts the observed cold seed');
  return observedPC;
}
function completeProvider(c, stage, token, gateOwner) {
  const lookup = arena(c);
  assert.deepEqual(lookup.subarray(T, T + 32), installation(gateOwner), 'capture uses the installed gate owner');
  const captured = record('R3CF', 1, 112, [token, stage.id, 2, stage.args.length, stage.gate_pc,
    stage.stop_registers[4], stage.return_pc, 0, ...Array.from({length: 16}, (_, index) => stage.args[index] ?? 0)]);
  host(c, 'private stdcall capture of original register CALL', () => c.api.capture_resident_call(
    KEY_LOW, KEY_HIGH, gateOwner.low, gateOwner.high, 2, stage.args.length), [[T, captured]]);
  counts.captures++;
  if (token === 1) {
    host(c, 'wrong token keeps the valid pending frame for retry', () => c.api.complete_resident_windows_call(
      KEY_LOW, KEY_HIGH, gateOwner.low, gateOwner.high, token + 1), [], 14);
    counts.wrong_token_refusals++;
  }
  host(c, 'private provider completion has no host return scalar', () => c.api.complete_resident_windows_call(
    KEY_LOW, KEY_HIGH, gateOwner.low, gateOwner.high, token),
  [[0, state(stage.returned_registers, stage.return_pc, stage.flags)], [56, exit(3, 3, 0)]]);
  counts.provider_completions++;
  if (token === 1) {
    host(c, 'consumed provider token cannot replay', () => c.api.complete_resident_windows_call(
      KEY_LOW, KEY_HIGH, gateOwner.low, gateOwner.high, token), [], 14);
    counts.replay_refusals++;
  }
  providers.push({context: c.id, api: stage.api, token, gate_owner: [gateOwner.low, gateOwner.high],
    args: stage.args, entry_esp: stage.stop_registers[4], returned_esp: stage.returned_registers[4],
    capture_hex: captured.toString('hex'), returned_cpu: cpu(c),
    arena: save(`context-${c.id}-provider-${token}-arena.bin`, arena(c))});
}

for (const test of oracle.cases) {
  const instance = new WebAssembly.Instance(engine.module, {}), api = {};
  const arities = {open: 3, close: 0, arena_ptr: 0, begin_image_input: 1, append_image_input: 2,
    load_pe32_linked_v2_input_at: 2, start_loaded_image: 2, compile_resident_entries: 2,
    acknowledge_resident_installation: 5, dispatcher_module: 2, guard_dispatch_entry: 5,
    find_installed_resident: 3, guard_resident: 7, read32: 1, store_resident32: 6,
    capture_resident_call: 6, complete_resident_windows_call: 5, cpu_read32: 1, cpu_store_resident32: 6};
  for (const [name, arity] of Object.entries(arities)) {
    api[name] = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof api[name], 'function', name);
    assert.equal(api[name].length, arity, name);
  }
  const c = {id: ++counts.contexts, actualBase: test.actual_base, api, memory: instance.exports.memory,
    units: [], imageBytes: expectedImage(test.actual_base), stack: Buffer.alloc(4096)};
  assert.equal(api.open(oracle.startup.capacity, KEY_LOW, KEY_HIGH), 0);
  c.base = api.arena_ptr() >>> 0;
  assert.ok(c.base > 0 && c.base + SIZE <= c.memory.buffer.byteLength);
  assert.deepEqual(arena(c), initial, 'fresh engine owns the complete canonical arena');
  host(c, 'begin unchanged compiler-produced image', () => api.begin_image_input(image.length));
  for (let offset = 0; offset < image.length; offset += 4096) {
    const chunk = image.subarray(offset, Math.min(offset + 4096, image.length));
    input(c, chunk);
    host(c, 'append exact staged original bytes', () => api.append_image_input(offset, chunk.length));
  }
  const linked = record('R3LI', 2, 72, [c.actualBase, oracle.image.size,
    c.actualBase + oracle.image.entry_rva, oracle.image.mapped_pages + 1, oracle.gates[0].pc, 2, 0, 0,
    ...oracle.gates.flatMap(gate => [gate.pc, gate.id]), 0, 0]);
  host(c, 'engine loads and links the staged original PE', () => api.load_pe32_linked_v2_input_at(
    c.actualBase, oracle.gates[0].pc), [[T, linked]]);
  const text = Buffer.from(c.imageBytes.subarray(0x1000, 0x1034));
  const readOnly = Buffer.from(c.imageBytes.subarray(0x2000, 0x2080));
  const relocations = Buffer.from(c.imageBytes.subarray(0x4000, 0x4018));
  const gates = Buffer.alloc(32);
  gates.writeUInt32LE(0x00000b0f); gates.writeUInt32LE(0x00000b0f, 16);
  checkRam(c, c.actualBase + 0x1000, text, 'exact relocated original text');
  checkRam(c, c.actualBase + 0x2000, readOnly, 'engine-linked IAT and retained import names');
  checkRam(c, c.actualBase + 0x4000, relocations, 'retained original relocation bytes');
  checkRam(c, c.actualBase + 0x3000, Buffer.alloc(32), 'zero-filled BSS and output canaries');
  checkRam(c, oracle.gates[0].pc, gates, 'two fixed RX provider stubs and zero neighbors');
  const entry = c.actualBase + oracle.image.entry_rva;
  host(c, 'engine startup owns CPU and stack initialization', () => api.start_loaded_image(
    oracle.startup.stack_base, oracle.startup.stack_pages),
  [[0, state(oracle.startup.registers, entry, oracle.startup.flags)], [56, exit(3, 3, 0)]]);
  checkRam(c, 0x8fc0, Buffer.alloc(64), 'canonical empty startup stack');
  c.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
  makeDispatcher(c);
  let retired = 0;
  run(c, 'initial-cold-entry', 14, 0, 3, oracle.startup.registers, entry, 2, 3);
  const firstSeed = observedCold(c, test.stages[0].entry_pc);
  const gateOwner = captureUnit(c, 'gates', oracle.gates.map(gate => gate.pc), oracle.gates, 0);
  for (const [index, stage] of test.stages.entries()) {
    let seed = firstSeed;
    if (index !== 0) {
      const previous = test.stages[index - 1];
      run(c, `cold-continuation-${index}`, 14 - retired, 0, 3,
        previous.returned_registers, previous.return_pc, previous.flags, 3);
      seed = observedCold(c, stage.entry_pc);
    }
    const caller = captureUnit(c, `caller-${index + 1}`, [seed], [], index + 1);
    run(c, `original-call-${index + 1}`, 14 - retired, stage.retired, 8,
      stage.stop_registers, stage.gate_pc, stage.flags, 3, stage.id, stage.accesses);
    retired += stage.retired;
    assert.notDeepEqual([caller.low, caller.high], [gateOwner.low, gateOwner.high]);
    checkRam(c, 0x8fc0, stackProjection(stage.stack_values), 'original stdcall stack and canaries');
    checkRam(c, c.actualBase + 0x3000, dataProjection(stage.data_words), 'guest output publication order');
    completeProvider(c, stage, index + 1, gateOwner);
  }
  const previous = test.stages.at(-1), final = test.final;
  run(c, 'cold-final-continuation', 14 - retired, 0, 3,
    previous.returned_registers, previous.return_pc, previous.flags, 3);
  const finalSeed = observedCold(c, final.entry_pc);
  captureUnit(c, 'caller-5', [finalSeed], [], 5);
  run(c, 'original-guest-stores-and-stable-loop', 14 - retired, final.retired, final.reason,
    final.registers, final.pc, final.flags, 6, 0, final.accesses);
  retired += final.retired;
  assert.equal(retired, oracle.total_retired);
  assert.equal(c.units.length, 6);
  assert.deepEqual(c.stack.subarray(0xfc0), stackProjection(final.stack_values));
  checkRam(c, 0x8fc0, stackProjection(final.stack_values), 'final retained compiler prologue and call stack');
  checkRam(c, c.actualBase + 0x3000, dataProjection(oracle.expected_output_words), 'literal Get values completion and output canaries');
  checkRam(c, c.actualBase + 0x2000, readOnly, 'IAT and readonly import bytes remain unchanged');
  checkRam(c, c.actualBase + 0x1000, text, 'original executable bytes remain unchanged');
  checkRam(c, c.actualBase + 0x4000, relocations, 'original relocation bytes remain unchanged');
  checkRam(c, oracle.gates[0].pc, gates, 'provider stubs remain unchanged');
  const finalArena = save(`context-${c.id}-final-arena.bin`, arena(c));
  contexts.push({id: c.id, name: test.name, actual_base: c.actualBase, final_cpu: cpu(c),
    resident_owners: c.units.length, final_arena: finalArena});
  host(c, 'close completed private image', () => api.close());
}
assert.deepEqual(sourceHashes(), sourcesBefore);
assert.deepEqual(counts, {contexts: 2, modules: 12, dispatcher_modules: 2, runs: 20, retired: 28,
  helper_calls: 26, read32: 4, store32: 22, cold_exits: 10, captures: 8,
  provider_completions: 8, wrong_token_refusals: 2, replay_refusals: 2, diagnostic_words: 524});
const result = {status: 'PASS', engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
  image: {bytes: image.length, sha256: hash(image)}, oracle_sha256: hash(oracleBytes), source_sha256: sourcesBefore,
  initial_arena_sha256: hash(initial), counts, contexts, modules, observations, helper_trace: helpers,
  provider_trace: providers, diagnostics, artifacts,
  claim: 'original compiler-produced stdcall import PE at preferred and rebased addresses; engine-owned staged linking and startup, cached register CALLs through one shared gate owner, private captured provider completion, observed cold continuations and literal guest GetLastError stores without host CPU/RAM/extended-state reseeding; full4612 assertions and selected RAM only; no executed guest RET, CRT/TLS/general Windows or performance claim'};
save('result.json', Buffer.from(JSON.stringify(result, null, 2) + '\n'));
console.log(JSON.stringify({output, counts, result: join(output, 'result.json')}));
