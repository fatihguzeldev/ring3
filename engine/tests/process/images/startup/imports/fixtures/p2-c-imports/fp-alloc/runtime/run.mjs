import {ARENA_SIZE} from '../../../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'expected engine, output directory and repository root');
const fixture = 'engine/tests/process/images/startup/imports/fixtures/p2-c-imports/fp-alloc';
const SIZE = ARENA_SIZE, T = 140, FP = 4236, SV = 4364, SG = 4524;
const STACK = 0x8000, STACK_TOP = 0x9000;
const KEY_LOW = 0xf5790001, KEY_HIGH = 0x4650414c;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engine = readEngine(enginePath);
const initial = readFileSync(join(output, 'initial-arena.bin'));
const image = readFileSync(join(root, fixture, 'program.exe'));
const oracleBytes = readFileSync(join(root, fixture, 'oracle.json'));
const oracle = JSON.parse(oracleBytes);
assert.equal(initial.length, SIZE);
assert.equal(oracle.schema, 'ring3.fp-alloc.v1');
assert.equal(image.length, oracle.image.bytes);
assert.equal(hash(image), oracle.image.sha256);
const originalText = Buffer.from(oracle.original_text.hex, 'hex');
assert.equal(hash(originalText), oracle.original_text.sha256);
assert.deepEqual(image.subarray(oracle.original_text.file_offset,
  oracle.original_text.file_offset + originalText.length), originalText);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
assert.equal(oracle.kernel.bytes, 116);
assert.equal(oracle.kernel.sha256, '08fad7ecb51ad9026481bf5dbe347b26ef413fc292d4b27599cc6f26a62b243d');

const artifacts = {}, contexts = [], modules = [], observations = [], helpers = [], providers = [], diagnostics = [];
const counts = {contexts: 0, modules: 0, dispatcher_modules: 0, runs: 0, retired: 0,
  helper_calls: 0, read32: 0, store32: 0, cpu_read: 0, cpu_write: 0, simd_execute: 0,
  cold_exits: 0, captures: 0, provider_completions: 0, wrong_token_refusals: 0,
  replay_refusals: 0, diagnostic_words: 0, post_free_faults: 0};
function save(name, bytes) {
  writeFileSync(join(output, name), bytes, {flag: 'wx'});
  artifacts[name] = {bytes: bytes.length, sha256: hash(bytes)};
  return name;
}
const sourcePaths = [
  `${fixture}/runtime/run.mjs`, `${fixture}/oracle.json`, `${fixture}/entry.c`, `${fixture}/build.py`,
  `${fixture}/program.exe`, `${fixture}/entry.obj`, `${fixture}/receipt.json`,
  'engine/tests/programs/numeric/fixtures/p2-fp-compiled/sse2/kernel.obj',
  'engine/tests/programs/numeric/fixtures/p2-fp-compiled/sse2/kernel.text.bin',
  'engine/tests/process/images/startup/imports/floating/allocation/native.rs', 'engine/tests/process/images/native.rs', 'engine/tests/process/images/startup/imports/floating/allocation/wasm.rs', 'engine/tests/process/images/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/support/wasm/engine.mjs', 'engine/src/process/loading/input.rs', 'engine/src/process/loading/mod.rs', 'engine/src/process/mod.rs',
  'engine/src/process/loading/startup.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/execution/installation.rs',
  'engine/src/process/calls/call.rs', 'engine/src/process/cpu/context.rs', 'engine/src/process/calls/mod.rs', 'engine/src/process/windows/calls.rs', 'engine/src/process/windows/mod.rs', 'engine/src/process/windows/virtual_memory.rs', 'engine/src/windows/provider.rs', 'engine/src/windows/api/mod.rs', 'engine/src/windows/api/catalog.rs',
  'engine/src/cpu/dbt/wasm/dispatcher.rs', 'engine/src/process/cpu/simd.rs', 'engine/src/process/cpu/mod.rs',
  'engine/src/process/memory/span.rs', 'engine/src/process/memory/mod.rs',
  'engine/src/cpu/dbt/wasm/simd.rs', 'engine/src/cpu/dbt/wasm/simd/direct.rs',
  'engine/src/cpu/x86/simd/float.rs', 'engine/src/math/mod.rs', 'engine/src/math/ieee.rs', 'engine/src/math/integer.rs', 'engine/src/lib.rs', 'engine/src/abi/x86/simd.rs',
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
  const kernel = Buffer.from(oracle.kernel.hex, 'hex');
  assert.equal(hash(kernel), oracle.kernel.sha256);
  assert.deepEqual(bytes.subarray(oracle.kernel.rva, oracle.kernel.rva + kernel.length), kernel);
  oracle.relocations.highlow_rvas.forEach((rva, index) => {
    assert.equal(bytes.readUInt32LE(rva), oracle.relocations.preferred_values[index]);
    bytes.writeUInt32LE((oracle.relocations.preferred_values[index] + actualBase - oracle.image.preferred_base) >>> 0, rva);
  });
  oracle.imports.slots.forEach(slot => bytes.writeUInt32LE(slot.gate_pc, slot.rva));
  return bytes;
}
function ramRead(c, address, length) {
  if (address >= c.actualBase && address + length <= c.actualBase + c.imageBytes.length) {
    return c.imageBytes.subarray(address - c.actualBase, address - c.actualBase + length);
  }
  if (c.heap && address >= oracle.allocation.base &&
      address + length <= oracle.allocation.base + c.heap.length) {
    return c.heap.subarray(address - oracle.allocation.base, address - oracle.allocation.base + length);
  }
  assert.ok(address >= STACK && address + length <= STACK_TOP);
  return c.stack.subarray(address - STACK, address - STACK + length);
}
function ramWrite(c, address, bytes) {
  if (address >= c.actualBase && address + bytes.length <= c.actualBase + c.imageBytes.length) {
    bytes.copy(c.imageBytes, address - c.actualBase);
  } else if (c.heap && address >= oracle.allocation.base &&
      address + bytes.length <= oracle.allocation.base + c.heap.length) {
    bytes.copy(c.heap, address - oracle.allocation.base);
  } else {
    assert.ok(address >= STACK && address + bytes.length <= STACK_TOP);
    bytes.copy(c.stack, address - STACK);
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
function checkSelectedRam(c, label, includeStack = true,
  heapRanges = oracle.allocation.selected_ranges) {
  for (const range of oracle.selected_ram) {
    checkRam(c, c.actualBase + range.rva,
      c.imageBytes.subarray(range.rva, range.rva + range.length), `${label}: ${range.label}`);
  }
  if (includeStack) {
    const range = oracle.selected_stack;
    checkRam(c, range.address, c.stack.subarray(range.address - STACK,
      range.address - STACK + range.length), `${label}: natural original stack slots`);
  }
  if (c.heap) {
    for (const range of heapRanges) {
      checkRam(c, oracle.allocation.base + range.offset,
        c.heap.subarray(range.offset, range.offset + range.length), `${label}: ${range.label}`);
    }
  }
}

function checkReleasedHeap(c) {
  assert.equal(c.heap, null);
  for (const offset of oracle.allocation.unmapped_probe_offsets) {
    const address = oracle.allocation.base + offset;
    const fault = record('R3MH', 1, 40, [1, 0, 1, address, 1, 4]);
    host(c, 'released allocation host diagnostic preserves architectural state',
      () => c.api.read32(address), [[100, fault]]);
    diagnostics.push({context: c.id, label: 'released allocation', address,
      expected_fault: {detail: 1, access: 1, length: 4}});
    counts.diagnostic_words++;
    counts.post_free_faults++;
  }
}
function bindHelper(c, unit, name) {
  return (...raw) => {
    const before = arena(c), expected = Buffer.from(before), args = raw.map(value => value >>> 0);
    if (name === 'guard_resident') {
      assert.deepEqual(args, [KEY_LOW, KEY_HIGH, unit.low, unit.high, c.base, c.base + 56, c.base + 96]);
      assert.equal(c.api[name](...raw), 0);
      assert.deepEqual(arena(c), before, 'resident guard is neutral');
      return 0;
    }
    const resident = ['cpu_store_resident32', 'cpu_write_resident', 'simd_execute_resident'].includes(name);
    if (resident) assert.deepEqual(args.slice(0, 4), [KEY_LOW, KEY_HIGH, unit.low, unit.high]);
    const payload = args.slice(resident ? 4 : 0);
    let family, resultPacket;
    if (name === 'cpu_read32' || name === 'cpu_store_resident32') {
      const access = c.accesses.shift();
      assert.ok(access, 'each generated DWORD helper has an original literal access');
      if (name === 'cpu_read32') {
        family = 'read32';
        assert.equal(access.kind, 'read');
        assert.deepEqual(payload, [access.address]);
        assert.equal(ramRead(c, access.address, 4).readUInt32LE(), access.value);
        resultPacket = cpuPacket(access.value);
      } else {
        family = 'store32';
        assert.equal(access.kind, 'store');
        assert.deepEqual(payload, [access.address, access.value]);
        ramWrite(c, access.address, word(access.value));
        resultPacket = cpuPacket();
      }
    } else if (name === 'cpu_read') {
      family = 'cpu_read';
      const next = c.spanReads.shift();
      assert.ok(next, 'each original FP read has a literal span');
      const bytes = Buffer.from(next.hex, 'hex');
      assert.deepEqual(payload, [next.address, bytes.length, next.alignment]);
      assert.deepEqual(ramRead(c, next.address, bytes.length), bytes);
      expected.set(bytes, T);
      resultPacket = record('R3MH', 7, 40, [0, 0, 0, 0, 0, bytes.length]);
    } else if (name === 'cpu_write_resident') {
      family = 'cpu_write';
      const next = c.spanWrites.shift();
      assert.ok(next, 'each original FP store has a literal output span');
      const bytes = Buffer.from(next.hex, 'hex');
      assert.deepEqual(payload, [next.address, bytes.length, next.alignment]);
      assert.deepEqual(before.subarray(T, T + bytes.length), bytes, 'literal bits before checked FP store');
      ramWrite(c, next.address, bytes);
      resultPacket = record('R3MH', 7, 40, [0, 0, 0, 0, 0, bytes.length]);
    } else {
      assert.equal(name, 'simd_execute_resident');
      family = 'simd_execute';
      assert.equal(payload[5], c.simdFlags.shift(), 'ordered original FLAGS passed into semantic helper');
      resultPacket = record('R3MH', 7, 40, [0, 0, 0, 0, 0, 0]);
    }
    assert.equal(c.api[name](...raw), 0, name);
    const after = arena(c);
    if (family === 'simd_execute') {
      expected.set(after.subarray(SV, SV + 160), SV);
      expected.set(words([0, payload[5]]), T + 512);
    }
    expected.set(resultPacket, 100);
    assert.deepEqual(after, expected, `${name}: exact declared helper mutations`);
    c.latest = after;
    helpers.push({context: c.id, phase: c.phase, name, args, packet_hex: resultPacket.toString('hex')});
    counts.helper_calls++;
    counts[family]++;
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
  const expectedHelpers = ['guard_resident', ...c.captureImports.map(name => ({read32: 'cpu_read32', store_resident32: 'cpu_store_resident32'}[name] ?? name))];
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
function run(c, phase) {
  const before = arena(c), helperStart = helpers.length;
  c.latest = before;
  c.phase = phase.label;
  c.accesses = [...(phase.accesses ?? [])];
  c.spanReads = [...(phase.span_reads ?? [])];
  c.spanWrites = [...(phase.span_writes ?? [])];
  c.simdFlags = [...(phase.simd_flags ?? [])];
  assert.equal(c.dispatcher.exports.run(c.base, c.base + 56, phase.budget, c.base + 96), 0, phase.label);
  assert.equal(c.accesses.length, 0, 'complete literal DWORD access chronology');
  assert.equal(c.spanReads.length, 0, 'complete literal FP read chronology');
  assert.equal(c.spanWrites.length, 0, 'complete literal FP store chronology');
  assert.equal(c.simdFlags.length, 0, 'complete original semantic helper chronology');
  const expected = Buffer.from(before);
  expected.set(c.latest.subarray(100, 140), 100);
  expected.set(c.latest.subarray(T, T + 32), T);
  expected.set(c.latest.subarray(T + 512, T + 520), T + 512);
  expected.set(state(phase.registers, phase.pc, phase.flags), 0);
  expected.set(exit(phase.exit_version === 2 ? 6 : phase.exit_version, phase.reason, phase.retired, phase.detail ?? 0), 56);
  const expectedSimd = Buffer.from(phase.simd_hex, 'hex');
  assert.equal(expectedSimd.length, 160, 'oracle provides the complete physical SIMD bank');
  expected.set(expectedSimd, SV);
  assert.deepEqual(arena(c), expected, `${phase.label}: full4612 literal CPU/SIMD and unchanged x87/segments/work`);
  assert.deepEqual(arena(c).subarray(FP, FP + 128), initial.subarray(FP, FP + 128));
  assert.deepEqual(arena(c).subarray(SG, SG + 64), initial.subarray(SG, SG + 64));
  const file = save(`context-${c.id}-${phase.label}-arena.bin`, arena(c));
  observations.push({context: c.id, label: phase.label, budget: phase.budget,
    retired: phase.retired, reason: phase.reason, exit_version: phase.exit_version,
    cpu: cpu(c), simd_hex: phase.simd_hex, helper_calls: helpers.length - helperStart, arena: file});
  counts.runs++;
  counts.retired += phase.retired;
  if (phase.reason === 3) counts.cold_exits++;
}
function cold(c, label, budget, pc) {
  const before = arena(c), current = cpu(c);
  assert.equal(current.pc, pc, 'only the observed natural guest PC seeds cold capture');
  run(c, {label, budget, retired: 0, reason: 3, exit_version: 3,
    registers: current.registers, pc, flags: current.flags,
    simd_hex: before.subarray(SV, SV + 160).toString('hex')});
}
function completeProvider(c, stage, token, gateOwner) {
  assert.deepEqual(arena(c).subarray(T, T + 32), installation(gateOwner), 'capture uses installed Gate owner');
  const captured = record('R3CF', 1, 112, [token, stage.id, 2, stage.args.length, stage.gate_pc,
    stage.stop_registers[4], stage.return_pc, 0, ...Array.from({length: 16}, (_, index) => stage.args[index] ?? 0)]);
  host(c, 'capture natural allocation stdcall context', () => c.api.capture_resident_call(
    KEY_LOW, KEY_HIGH, gateOwner.low, gateOwner.high, 2, stage.args.length), [[T, captured]]);
  counts.captures++;
  if (token === 1) {
    host(c, 'wrong allocation token preserves pending context for valid retry', () => c.api.complete_resident_windows_call(
      KEY_LOW, KEY_HIGH, gateOwner.low, gateOwner.high, token + 1), [], 14);
    counts.wrong_token_refusals++;
  }
  host(c, 'token-only provider completion preserves every extended byte', () => c.api.complete_resident_windows_call(
    KEY_LOW, KEY_HIGH, gateOwner.low, gateOwner.high, token),
    [[0, state(stage.returned_registers, stage.return_pc, stage.flags)], [56, exit(3, 3, 0)]]);
  counts.provider_completions++;
  if (stage.api === 'VirtualAlloc') {
    assert.equal(c.heap, null);
    assert.equal(stage.returned_registers[0], oracle.allocation.base);
    c.heap = Buffer.alloc(oracle.allocation.length);
  } else {
    assert.equal(stage.api, 'VirtualFree');
    assert.ok(c.heap);
    assert.equal(stage.returned_registers[0], 1);
    c.heap = null;
  }
  if (token === 1) {
    host(c, 'consumed allocation token replay preserves completed context', () => c.api.complete_resident_windows_call(
      KEY_LOW, KEY_HIGH, gateOwner.low, gateOwner.high, token), [], 14);
    counts.replay_refusals++;
  }
  for (const unit of c.units) {
    host(c, 'installed code owner remains current after allocation mapping change',
      () => c.api.guard_resident(KEY_LOW, KEY_HIGH, unit.low, unit.high,
        c.base, c.base + 56, c.base + 96));
  }
  providers.push({context: c.id, api: stage.api, token, gate_owner: [gateOwner.low, gateOwner.high],
    args: stage.args, capture_hex: captured.toString('hex'), returned_cpu: cpu(c),
    current_owners: c.units.map(unit => ({label: unit.label, id_low: unit.low, id_high: unit.high})),
    simd_hex: arena(c).subarray(SV, SV + 160).toString('hex'),
    arena: save(`context-${c.id}-provider-${token}-arena.bin`, arena(c))});
}

for (const test of oracle.cases) {
  const instance = new WebAssembly.Instance(engine.module, {}), api = {};
  const arities = {open: 3, close: 0, arena_ptr: 0, begin_image_input: 1, append_image_input: 2,
    load_pe32_linked_v5_input_at: 2, start_loaded_image: 2, compile_resident_entries: 2,
    acknowledge_resident_installation: 5, dispatcher_module: 2, guard_dispatch_entry: 5,
    find_installed_resident: 3, guard_resident: 7, read32: 1, store_resident32: 6,
    cpu_read: 3, cpu_write_resident: 7, simd_execute_resident: 10,
    capture_resident_call: 6, complete_resident_windows_call: 5, cpu_read32: 1, cpu_store_resident32: 6};
  for (const [name, arity] of Object.entries(arities)) {
    api[name] = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof api[name], 'function', name);
    assert.equal(api[name].length, arity, name);
  }
  const c = {id: ++counts.contexts, actualBase: test.actual_base, api, memory: instance.exports.memory,
    units: [], heap: null, imageBytes: expectedImage(test.actual_base), stack: Buffer.alloc(STACK_TOP - STACK)};
  assert.equal(api.open(oracle.startup.capacity, KEY_LOW, KEY_HIGH), 0);
  c.base = api.arena_ptr() >>> 0;
  assert.ok(c.base > 0 && c.base + SIZE <= c.memory.buffer.byteLength);
  assert.deepEqual(arena(c), initial, 'fresh engine owns complete canonical state before any host seed');
  host(c, 'begin unchanged compiler-produced image', () => api.begin_image_input(image.length));
  for (let offset = 0; offset < image.length; offset += 4096) {
    const chunk = image.subarray(offset, Math.min(offset + 4096, image.length));
    input(c, chunk);
    host(c, 'append exact original PE', () => api.append_image_input(offset, chunk.length));
  }
  const linked = record('R3LI', 5, 96, [c.actualBase, oracle.image.size,
    c.actualBase + oracle.image.entry_rva, oracle.image.mapped_pages + 1, oracle.gate_base,
    oracle.gates.length, 0, 0, ...oracle.linked_gates.flatMap(gate => [gate.pc, gate.id])]);
  host(c, 'engine staged allocation-linked load', () => api.load_pe32_linked_v5_input_at(
    c.actualBase, oracle.gate_base), [[T, linked]]);
  checkSelectedRam(c, 'loaded original image before startup', false);
  host(c, 'engine-owned canonical image startup', () => api.start_loaded_image(
    oracle.startup.stack_base, oracle.startup.stack_pages),
    [[0, state(oracle.startup.registers, c.actualBase + oracle.image.entry_rva, oracle.startup.flags)],
      [56, exit(3, 3, 0)]]);
  c.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
  makeDispatcher(c);
  cold(c, 'initial-cold-entry', test.phases[0].budget, test.phases[0].entry_pc);
  c.captureImports = [];
  const gateOwner = captureUnit(c, 'gates', oracle.gates.map(gate => gate.pc), oracle.gates, 0);
  let token = 0, pendingCold = false;
  for (const phase of test.phases) {
    if (pendingCold) {
      cold(c, `cold-before-${phase.label}`, phase.budget, phase.entry_pc);
      pendingCold = false;
    }
    const existing = c.units.find(unit => unit.label === phase.unit);
    if (!existing) {
      assert.equal(cpu(c).pc, phase.entry_pc, 'capture entry is the observed natural guest PC');
      assert.ok(phase.entries.includes(phase.entry_pc));
      c.captureImports = phase.imports;
      captureUnit(c, phase.unit, phase.entries, [], phase.slot);
    } else {
      assert.equal(existing.label, 'kernel', 'only original kernel owner is reused');
    }
    run(c, phase);
    if (phase.unit === 'kernel') {
      const result = Buffer.from(oracle.allocation.result_hex, 'hex');
      assert.deepEqual(c.heap.subarray(0, result.length), result, 'whole original kernel writes literal allocated results');
    }
    checkSelectedRam(c, phase.label);
    if (phase.provider) {
      completeProvider(c, phase.provider, ++token, gateOwner);
      if (phase.provider.api === 'VirtualAlloc') {
        checkSelectedRam(c, 'provider-created zero allocation', true,
          [{offset: 0, length: oracle.allocation.length, label: 'complete newly zeroed allocation'}]);
      }
      else checkReleasedHeap(c);
      pendingCold = true;
    }
  }
  assert.equal(pendingCold, false);
  assert.equal(token, 2);
  assert.equal(c.units.length, oracle.resident_owners_per_base);
  assert.equal(c.heap, null);
  const copied = oracle.copied_output;
  assert.deepEqual(c.imageBytes.subarray(copied.rva, copied.rva + copied.hex.length / 2),
    Buffer.from(copied.hex, 'hex'), 'guest copies literal allocated results into image BSS before release');
  const final = save(`context-${c.id}-final-arena.bin`, arena(c));
  contexts.push({id: c.id, name: test.name, actual_base: c.actualBase, final_cpu: cpu(c),
    resident_owners: c.units.length, final_arena: final});
  host(c, 'close completed original allocated FP image', () => api.close());
}
assert.deepEqual(sourceHashes(), sourcesBefore);
assert.deepEqual(counts, oracle.total_counts);
const result = {status: 'PASS', engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
  image: {bytes: image.length, sha256: hash(image)}, oracle_sha256: hash(oracleBytes), source_sha256: sourcesBefore,
  initial_arena_sha256: hash(initial), counts, contexts, modules, observations, helper_trace: helpers,
  provider_trace: providers, diagnostics, artifacts,
  claim: 'one original linked C PE consumes the genuine VirtualAlloc return in a whole SSE2 callee, copies its literal results to image BSS, then frees that same allocation; preferred/rebased engine startup and cold resident continuation without host CPU/FP/RAM reseeding; literal full4612 architectural assertions and selected assertion-backed RAM only; released-address failures are host diagnostics, with no guest fault recovery, whole-RAM or live-XMM-across-C-ABI promise'};
save('result.json', Buffer.from(JSON.stringify(result, null, 2) + '\n'));
console.log(JSON.stringify({output, counts, result: join(output, 'result.json')}));
