import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {runPeProof} from './pe.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = 4612;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sourcePaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/src/abi/arena.rs',
  'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/resident.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/cpu/dbt/wasm/memory/byte_store.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar_tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs',
  'engine/tests/cpu/predicates/set/memory/native/byte.rs', 'engine/tests/cpu/predicates/native.rs', 'engine/tests/support/cpu/prefix/byte.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/cpu/predicates/set/memory/wasm/byte.rs', 'engine/tests/cpu/predicates/wasm.rs', 'engine/tests/support/loader/pe32.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/predicates/set/memory/fixtures/p2-memory-set-byte/runtime/run.mjs', 'engine/tests/cpu/predicates/set/memory/fixtures/p2-memory-set-byte/runtime/pe.mjs',
  'engine/tests/cpu/predicates/set/memory/fixtures/p2-memory-set-byte/integer.S', 'engine/tests/support/wasm/engine.mjs'];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
const sources = () => Object.fromEntries(sourcePaths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcePins = sources(), modules = [], observations = [], helperTrace = [], contexts = [], rows = [], pageObservations = [], guardControls = [];
const counts = {contexts: 0, runs: 0, retired: 0, canonical_cases: 0, ignored_cases: 0, ea_cases: 0,
  controls: 0, live_paths: 0, live_faults: 0, repair_phases: 0, smc_paths: 0, identity_smc: 0,
  helper_calls: 0, diagnostic_read8: 0, diagnostic_fault_read8: 0, diagnostic_read32: 0, public_guard_controls: 0, selected_ranges: 0, whole_pages: 0};
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, upload: 2, read8: 1, read32: 1,
  compile: 1, compile_entries: 2, compile_resident: 1, compile_resident_entries: 2,
  generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store8: 2, store_resident8: 6};
const irrelevantSeeds = [0, 0x10, 0x400, 0x410];
const initialRegisters = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const anchors = [[0x802, 2], [2, 0x802], [3, 2], [2, 3], [0x42, 2], [2, 0x42], [3, 2], [2, 3],
  [0x82, 2], [2, 0x82], [6, 2], [2, 6], [0x82, 2], [2, 0x82], [0x42, 2], [2, 0x42]];
function condition(index, flags) {
  const bit = weight => Math.floor(flags / weight) % 2 !== 0;
  const c = bit(1), p = bit(4), z = bit(64), s = bit(128), o = bit(2048);
  return Number([o, !o, c, !c, z, !z, c || z, !c && !z, s, !s, p, !p,
    s !== o, s === o, z || s !== o, !z && s === o][index]);
}
for (let index = 0; index < 16; index++) for (const irrelevant of irrelevantSeeds) {
  assert.equal(condition(index, anchors[index][0] + irrelevant), 1);
  assert.equal(condition(index, anchors[index][1] + irrelevant), 0);
}
assert.equal(condition(12, 0x882), 0); assert.equal(condition(15, 0x882), 1);
function canonicalFlags(value, irrelevant) {
  return 2 + irrelevant + [1, 4, 64, 128, 2048].reduce((sum, bit, i) => sum + Math.floor(value / 2 ** i) % 2 * bit, 0);
}
const allFlags = Array.from({length: 32}, (_, i) => irrelevantSeeds.map(other => canonicalFlags(i, other))).flat();
assert.equal(new Set(allFlags).size, 128); assert.ok(allFlags.every(f => (f & 2) && !(f & ~0xcd7)));
const shapeDefinitions = [
  {values: {0: 0x3001}, terms: [[0, 1]], displacement: 0, tail: [0x00], address: 0x3001},
  {values: {0: 0x7fffffff}, terms: [[0, 2]], displacement: 1, tail: [0x04, 0x45, 1, 0, 0, 0], address: 0xffffffff},
  {values: {1: 0x3042}, terms: [[1, 1]], displacement: -16, tail: [0x41, 0xf0], address: 0x3032},
  {values: {1: 0x80001800}, terms: [[1, 2]], displacement: 1, tail: [0x04, 0x4d, 1, 0, 0, 0], address: 0x3001},
  {values: {2: 0xfffffff0, 4: 0x3010}, terms: [[4, 1], [2, 2]], displacement: 17, tail: [0x44, 0x54, 0x11], address: 0x3001},
  {values: {2: 0xffffffff, 5: 0x3004}, terms: [[5, 1], [2, 4]], displacement: 0, tail: [0x44, 0x95, 0], address: 0x3000},
  {values: {3: 0x3066, 6: 2}, terms: [[3, 1], [6, 8]], displacement: -32, tail: [0x84, 0xf3, 0xe0, 0xff, 0xff, 0xff], address: 0x3056},
  {values: {7: 0x3eff}, terms: [[7, 1]], displacement: 256, tail: [0x87, 0, 1, 0, 0], address: 0x3fff},
];
for (const shape of shapeDefinitions) {
  assert.equal(shape.tail[0] & 0x38, 0);
  const registers = [...initialRegisters]; for (const [index, value] of Object.entries(shape.values)) registers[Number(index)] = value;
  const sum = shape.terms.reduce((total, [index, scale]) => total + registers[index] * scale, shape.displacement);
  assert.equal((sum % 0x100000000 + 0x100000000) % 0x100000000, shape.address);
}
const batches = [], canonicalForms = [], ignoredForms = [], eaForms = [];
function batch(forms, pc, group) {
  let next = pc; for (const form of forms) {form.pc = next; next += form.bytes.length;}
  const code = Buffer.from([...forms.flatMap(form => form.bytes), 0xeb, 0, 0x0f, 0x0b]);
  assert.ok(forms.length + 1 <= 64 && code.length < 0x400); const index = batches.length;
  assert.deepEqual(code, readFileSync(join(output, `code-${index}.x86`))); batches.push({index, pc, group, forms, code});
}
for (let index = 0; index < 16; index++) {
  canonicalForms.push({condition: index, ignored: 0, address: 0x3fff, bytes: [0x0f, 0x90 + index, 6]});
  for (let field = 0; field < 8; field++) ignoredForms.push({condition: index, ignored: field, address: 0x3fff, bytes: [0x0f, 0x90 + index, field * 8 + 6]});
  for (let shapeIndex = 0; shapeIndex < 8; shapeIndex++) {
    const shape = shapeDefinitions[shapeIndex], field = (index + shapeIndex) % 8;
    eaForms.push({...shape, shape: shapeIndex, condition: index, ignored: field, bytes: [0x0f, 0x90 + index, shape.tail[0] + field * 8, ...shape.tail.slice(1)]});
  }
}
batch(canonicalForms, 0x8000, 'canonical');
for (let start = 0; start < 128; start += 43) batch(ignoredForms.slice(start, start + 43), 0x8100 + Math.floor(start / 43) * 0x100, 'ignored');
for (let start = 0; start < 128; start += 43) batch(eaForms.slice(start, start + 43), 0x8400 + Math.floor(start / 43) * 0x400, 'ea');
assert.equal(batches.length, 7);

const liveCode = readFileSync(join(output, 'live.x86'));
assert.deepEqual(liveCode, Buffer.from('38c8be0f4000000f90060f90c38a06eb000f0b', 'hex'));
const smcCodes = new Map();
for (const truth of [0, 1]) for (const same of [0, 1]) {
  const original = same ? truth : truth ^ 1, bytes = readFileSync(join(output, `smc-${truth}-${same}.x86`));
  assert.deepEqual(bytes, Buffer.from([0x38, 0xc8, 0xbe, 0x0b, 0xa0, 0, 0, 0x0f, 0x94, 6,
    0xb8, original, 0, 0x34, 0x12, 0xeb, 0, 0x0f, 0x0b])); smcCodes.set(`${truth}-${same}`, bytes);
}
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
const state = s => record('R3ST', 56, [...s.registers, s.pc, s.flags]);
const exit = (reason, retired, version = 2, fault = null) => record('R3EX', 40,
  [reason, retired, fault?.reason ?? 0, fault?.address ?? 0, fault ? 2 : 0, fault ? 1 : 0], version);
const storePacket = fault => record('R3MH', 40, fault ? [1, 0, fault.reason, fault.address, 2, 1] : [0, 0, 0, 0, 0, 1], 3);
const readPacket = value => record('R3MH', 40, [0, value, 0, 0, 0, 1], 2);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
function input(c, offset, bytes) {
  const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'explicit input preserves remaining full4612');
}
function host(c, name, args) {
  const expected = arena(c); assert.equal(c.api[name](...args), 0, name);
  if (name.startsWith('compile_resident')) expected.set(arena(c).subarray(140, 164), 140);
  assert.deepEqual(arena(c), expected, 'host full arena effect: ' + name);
}
function upload(c, address, bytes) {
  input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);
  const page = c.pages.get(Math.floor(address / 4096) * 4096);
  if (page) {assert.ok(address % 4096 + bytes.length <= 4096); page.set(bytes, address % 4096);}
}
function map(c, address, bytes, permissions = 3) {
  host(c, 'map', [address, 1, permissions]); c.pages.set(address, Buffer.alloc(4096));
  if (bytes) upload(c, address, bytes);
}
function fresh(owner, entries, purpose) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: `micro-${++counts.contexts}`, owner, entries,
    purpose, memory: instance.exports.memory, api: {}, pages: new Map(), captures: 0, resident: 0, runOrdinal: 0};
  for (const [name, arity] of Object.entries(arities)) {
    const fn = instance.exports['ring3_abi_v1_' + name]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); c.api[name] = fn;
  }
  c.low = counts.contexts; c.high = 0xc8400000;
  assert.equal(c.api.open(8, c.low, c.high), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial); return c;
}
function close(c) {
  contexts.push({id: c.id, owner: c.owner, entries: c.entries, purpose: c.purpose,
    captures: c.captures, resident_units: c.resident, mapped_pages: c.pages.size}); host(c, 'close', []);
}
function seed(c, pc, flags = 0xcd7, registers = initialRegisters) {
  const b = Buffer.alloc(140); b.set(state({registers, pc, flags})); b.set(exit(3, 0, 1), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
const storeName = owner => owner === 'replacement' ? 'store8' : 'store_resident8';
function compile(c, label, specs, instructions, helpers = [storeName(c.owner)], owner = c.owner) {
  assert.ok(specs.length > 0 && specs.length <= 8 && instructions <= 64);
  c.captures++; if (owner === 'resident') assert.ok(++c.resident <= 8);
  input(c, 140, words(specs.flatMap(b => c.entries ? [b.pc] : [b.pc, b.length])));
  const method = owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [specs.length, 0] : [specs.length]); let binding;
  if (owner === 'resident') {
    const b = arena(c); assert.equal(b.readUInt32LE(140), 1); assert.equal(b.readUInt32LE(144), 24);
    binding = {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)};
  } else binding = {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  assert.ok(binding.length > 8 && binding.length <= 65536);
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const names = [owner === 'resident' ? 'guard_resident' : 'guard', ...helpers];
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const ring3 = {...c.api};
  for (const name of helpers) ring3[name] = (...args) => {
    const status = c.api[name](...args), read = name === 'read8';
    helperTrace.push({context: c.id, run: c.runOrdinal, name, address: args[read ? 0 : args.length - 2] >>> 0,
      value: read ? null : args.at(-1) >>> 0, status, helper_hex: arena(c).subarray(100, 140).toString('hex')}); counts.helper_calls++; return status;
  };
  const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run; assert.equal(run.length, 4);
  const file = `micro-module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, label, owner, entries: c.entries, blocks: specs.length, instructions,
    file, sha256: hash(bytes), bytes: bytes.length, imports, ...binding});
  return {...binding, run, owner, version: helpers.length ? 2 : 1, file, bytes};
}
function run(c, unit, budget, next, label, {reason = 1, retired = 1, status = 0, packet = null, fault = null, pointers = null} = {}) {
  const expected = arena(c), traceStart = helperTrace.length;
  if (!status) {expected.set(state(next)); expected.set(exit(reason, retired, unit.version, fault), 56); if (packet) expected.set(packet, 100);}
  c.runOrdinal++; const observed = unit.run(...(pointers ?? [c.base, c.base + 56, budget, c.base + 96]));
  assert.equal(observed, status, label); assert.deepEqual(arena(c), expected, 'full4612 CPU/FLAGS/EIP/exit/helper/dirtyFP before diagnostics: ' + label);
  const b = arena(c); observations.push({context: c.id, run: c.runOrdinal, module: unit.file, label, budget,
    expected_status: status, observed_status: observed, expected_pc: status ? null : next.pc, observed_pc: status ? null : b.readUInt32LE(48),
    expected_flags: status ? null : next.flags, observed_flags: status ? null : b.readUInt32LE(52),
    expected_reason: status ? null : reason, observed_reason: status ? null : b.readUInt32LE(72),
    expected_retired: status ? null : retired, observed_retired: status ? null : b.readUInt32LE(76),
    helper_events: helperTrace.slice(traceStart)});
  counts.runs++; if (!status) counts.retired += b.readUInt32LE(76);
}
function diagnostic8(c, address, value) {
  const expected = arena(c); expected.set(readPacket(value), 100); assert.equal(c.api.read8(address), 0);
  assert.deepEqual(arena(c), expected, 'public read8 diagnostic changes helper only'); counts.diagnostic_read8++;
}
function neighbors(c, address) {
  const start = Math.floor(address / 32) * 32, page = c.pages.get(Math.floor(address / 4096) * 4096);
  assert.ok(page && start + 32 <= 4294967296);
  const bytes = page.subarray(start % 4096, start % 4096 + 32);
  for (let i = 0; i < 32; i++) diagnostic8(c, start + i, bytes[i]); counts.selected_ranges++;
  return {address: start, bytes: 32, expected_hex: bytes.toString('hex')};
}
function pages(c, group, phase) {
  for (const [address, bytes] of c.pages) {
    const expected = arena(c), observed = Buffer.alloc(4096);
    for (let offset = 0; offset < 4096; offset += 4) {
      const value = bytes.readUInt32LE(offset), packet = record('R3MH', 40, [0, value, 0, 0, 0, 0]);
      assert.equal(c.api.read32(address + offset), 0); const b = arena(c); assert.deepEqual(b.subarray(100, 140), packet);
      observed.writeUInt32LE(b.readUInt32LE(120), offset); expected.set(packet, 100); counts.diagnostic_read32++;
    }
    assert.deepEqual(observed, bytes); assert.deepEqual(arena(c), expected, 'whole declared page diagnostic preserves rest of full arena');
    pageObservations.push({context: c.id, group, phase, address, bytes: 4096, sha256: hash(observed)}); counts.whole_pages++;
  }
}
function guard(c, unit) {
  const expected = arena(c), args = unit.owner === 'resident' ? [c.low, c.high, unit.low, unit.high, c.base, c.base + 56, c.base + 96]
    : [c.low, c.high, unit.generation, c.base, c.base + 56, c.base + 96];
  assert.equal(c.api[unit.owner === 'resident' ? 'guard_resident' : 'guard'](...args), 0); assert.deepEqual(arena(c), expected);
}
function guardProbe(c, unit, label, status, changes = {}) {
  const low = changes.low ?? c.low, high = changes.high ?? c.high, id = changes.id ?? (unit.owner === 'resident' ? unit.low : unit.generation);
  const highId = changes.highId ?? unit.high, pointer = changes.pointer ?? c.base;
  const name = unit.owner === 'resident' ? 'guard_resident' : 'guard';
  const args = unit.owner === 'resident' ? [low, high, id, highId, pointer, c.base + 56, c.base + 96]
    : [low, high, id, pointer, c.base + 56, c.base + 96];
  const expected = arena(c), observed = c.api[name](...args); assert.equal(observed, status, label); assert.deepEqual(arena(c), expected);
  guardControls.push({context: c.id, module: unit.file, label, name, args, expected_status: status, observed_status: observed}); counts.public_guard_controls++;
}
function missingPageDiagnostic(c) {
  const expected = arena(c); expected.set(record('R3MH', 40, [1, 0, 1, 0x4000, 1, 1], 2), 100);
  assert.equal(c.api.read8(0x4000), 0); assert.deepEqual(arena(c), expected, 'page after last-byte destination remains unmapped; diagnostic helper only');
  guardControls.push({context: c.id, label: 'following page unmapped', name: 'read8', address: 0x4000,
    expected_status: 0, observed_status: 0, helper_hex: arena(c).subarray(100, 140).toString('hex')}); counts.diagnostic_fault_read8++;
}
function microCase(c, unit, form, registers, flags, initialByte, group) {
  const address = form.address, start = Math.floor(address / 32) * 32, page = Math.floor(address / 4096) * 4096;
  const bytes = Buffer.from(Array.from({length: 32}, (_, i) => i + 1)); bytes[address - start] = initialByte; upload(c, start, bytes);
  host(c, 'protect', [page, 1, 2]); seed(c, form.pc, flags, registers); const value = condition(form.condition, flags);
  run(c, unit, 1, {...cpu(c), pc: form.pc + form.bytes.length}, 'finite ' + group, {packet: storePacket()});
  c.pages.get(page)[address % 4096] = value; host(c, 'protect', [page, 1, 3]); const range = neighbors(c, address);
  rows.push({context: c.id, group, condition: form.condition, ignored: form.ignored, shape: form.shape ?? null,
    pc: form.pc, encoding: Buffer.from(form.bytes).toString('hex'), address, registers, flags, initial_byte: initialByte, value, selected_ram: range});
  counts[group + '_cases']++;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = fresh(owner, entries, 'finite conditions/ignored bits/EAs'), code = Buffer.alloc(4096, 0xcc);
  for (const item of batches) code.set(item.code, item.pc - 0x8000);
  map(c, 0x8000, code, 7);
  for (const address of [0x3000, 0xfffff000]) map(c, address, Buffer.from(Array.from({length: 4096}, (_, i) => i % 251 + 1)));
  let first, last;
  for (const item of batches) {
    const unit = compile(c, item.group, [{pc: item.pc, length: item.code.length - 2}], item.forms.length + 1);
    first ??= unit; last = unit; pages(c, item.index, 'before group');
    for (const form of item.forms) {
      const registers = [...initialRegisters];
      if (form.values) for (const [index, value] of Object.entries(form.values)) registers[Number(index)] = value;
      else registers[6] = form.address;
      if (item.group === 'canonical') {
        for (let relevant = 0; relevant < 32; relevant++) for (const irrelevant of irrelevantSeeds) {
          const f = canonicalFlags(relevant, irrelevant), original = [0, 1, 0xa5][(relevant + irrelevantSeeds.indexOf(irrelevant) + form.condition) % 3];
          microCase(c, unit, form, registers, f, original, 'canonical');
        }
      } else for (const truth of [1, 0]) {
        const f = anchors[form.condition][truth ? 0 : 1] + (form.ignored % 2 ? 0x410 : 0x10);
        microCase(c, unit, form, registers, f, truth ? 0 : 1, item.group);
      }
    }
    seed(c, item.pc); run(c, unit, 0, cpu(c), 'zero before store', {retired: 0});
    input(c, 96, words([1])); run(c, unit, 1, cpu(c), 'cancel before store', {reason: 2, retired: 0});
    run(c, unit, 0, cpu(c), 'cancel precedes zero', {reason: 2, retired: 0}); input(c, 96, words([0])); counts.controls += 3;
    guard(c, unit); pages(c, item.index, 'after group');
  }
  seed(c, batches[0].pc);
  run(c, first, 0, cpu(c), 'retained first owner before any source mutation', owner === 'replacement' ? {status: 3, retired: 0} : {retired: 0});
  run(c, first, 0, cpu(c), 'earlier canonical owner before malformed pointers', {status: owner === 'replacement' ? 3 : 1,
    pointers: [0xffffffff, 0xffffffff, 0, 0xffffffff]});
  missingPageDiagnostic(c);
  guardProbe(c, last, 'wrong owner key low', 3, {low: c.low + 1});
  guardProbe(c, last, 'wrong owner key high', 3, {high: c.high + 1});
  guardProbe(c, last, 'wrong generation or resident id low', 3, {id: (owner === 'resident' ? last.low : last.generation) + 1});
  guardProbe(c, last, 'zero generation or wrong resident id high', 3, owner === 'resident' ? {highId: last.high + 1} : {id: 0});
  guardProbe(c, last, 'current malformed guard pointer', 1, {pointer: 0xffffffff});
  run(c, last, 1, cpu(c), 'current owner rejects malformed pointers before store', {status: 1, pointers: [0xffffffff, 0xffffffff, 1, 0xffffffff]});
  upload(c, batches[6].pc, batches[6].code.subarray(0, 1)); input(c, 96, words([1]));
  guardProbe(c, last, 'same-byte source mutation before guard pointer validation', 4, {pointer: 0xffffffff});
  run(c, last, 0, cpu(c), 'same-byte source input stale before cancel/budget/pointers', {status: 4, pointers: [0xffffffff, 0xffffffff, 0, 0xffffffff]});
  close(c); run(c, last, 0, cpu(c), 'closed owner before malformed pointers', {status: 5, pointers: [0xffffffff, 0xffffffff, 0, 0xffffffff]}); counts.controls += 5;
}
function livePath(owner, entries, truth) {
  const c = fresh(owner, entries, 'live fault/repair/consumer'), address = 0x400f;
  const code = Buffer.alloc(4096, 0xcc); code.set(liveCode); map(c, 0x9000, code, 7);
  const unit = compile(c, 'live', [{pc: 0x9000, length: 17}], 6, ['read8', storeName(owner)]);
  const r = [...initialRegisters]; r[0] = truth ? 0x1234807f : 0x12348080; r[1] = truth ? 0x23457fff : 0x23457f80;
  const initialFlags = owner === 'resident' ? 0xcd7 : 0x8d7, f = (truth ? 0x883 : 0x46) + (owner === 'resident' ? 0x400 : 0);
  assert.equal(condition(0, f), truth); seed(c, 0x9000, initialFlags, r); pages(c, 'live', 'before live');
  run(c, unit, 0, cpu(c), 'zero before live producer', {retired: 0});
  input(c, 96, words([1])); run(c, unit, 1, cpu(c), 'cancel before live producer', {reason: 2, retired: 0});
  run(c, unit, 0, cpu(c), 'cancel before live producer and zero', {reason: 2, retired: 0}); input(c, 96, words([0])); counts.controls += 3;
  let next = cpu(c); next.pc = 0x9007; next.flags = f; next.registers[6] = address;
  const missing = {reason: 1, address}, denied = {reason: 2, address};
  run(c, unit, 3, next, 'CMP/MOV retire once before initial unmapped Write1 fault', {reason: 5, retired: 2, fault: missing, packet: storePacket(missing)});
  run(c, unit, 1, cpu(c), 'unchanged missing-page retry retains current flags/PC', {reason: 5, retired: 0, fault: missing, packet: storePacket(missing)});
  map(c, 0x4000, null, 1); counts.repair_phases++;
  run(c, unit, 1, cpu(c), 'map-only zero page remains read-only; false still needs write', {reason: 5, retired: 0, fault: denied, packet: storePacket(denied)}); neighbors(c, address);
  run(c, unit, 1, cpu(c), 'unchanged denied-write retry publishes no byte', {reason: 5, retired: 0, fault: denied, packet: storePacket(denied)}); neighbors(c, address);
  run(c, unit, 0, cpu(c), 'zero at stopped instruction', {retired: 0});
  input(c, 96, words([1])); run(c, unit, 1, cpu(c), 'cancel at stopped instruction', {reason: 2, retired: 0});
  run(c, unit, 0, cpu(c), 'cancel before stopped zero', {reason: 2, retired: 0}); input(c, 96, words([0])); counts.controls += 3;
  host(c, 'protect', [0x4000, 1, 2]); counts.repair_phases++; guard(c, unit);
  run(c, unit, 1, {...cpu(c), pc: 0x900a}, 'protect-only WRITE repair resumes same SETcc without CPU reseed', {packet: storePacket()});
  c.pages.get(0x4000)[15] = truth; host(c, 'protect', [0x4000, 1, 1]); neighbors(c, address);
  next = cpu(c); next.pc = 0x900d; next.registers[3] = r[3] - r[3] % 256 + truth;
  run(c, unit, 1, next, 'register SETO consumes preserved live condition');
  next = cpu(c); next.pc = 0x900f; next.registers[0] = r[0] - r[0] % 256 + truth;
  run(c, unit, 1, next, 'real read8 consumes the stored zero or one', {packet: readPacket(truth)});
  run(c, unit, 2, {...cpu(c), pc: 0x9011}, 'live tail reaches cold entry without replay', {reason: 3});
  run(c, unit, 1, cpu(c), 'cold entry preserves returned live state', {reason: 3, retired: 0});
  pages(c, 'live', 'after live'); counts.live_paths++; counts.live_faults += 4; close(c);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const truth of [1, 0]) livePath(owner, entries, truth);
function smcPath(owner, entries, truth, same) {
  const c = fresh(owner, entries, 'SMC committed store/current successor'), bytes = smcCodes.get(`${truth}-${same}`);
  const code = Buffer.alloc(4096, 0xcc); code.set(bytes); map(c, 0xa000, code, 7);
  const keeperCode = Buffer.alloc(4096, 0xcc); keeperCode.set([0x90, 0xeb, 0]); map(c, 0xb000, keeperCode, 7);
  const keeper = compile(c, 'independent resident keeper', [{pc: 0xb000, length: 3}], 2, [], 'resident');
  const writer = compile(c, 'SMC writer', [{pc: 0xa000, length: 17}], 5);
  const r = [...initialRegisters]; r[0] = 0x1234807f; r[1] = truth ? 0x23457f7f : 0x23457f80;
  const f = (truth ? 0x46 : 0x887) + (owner === 'resident' ? 0x400 : 0), initialFlags = owner === 'resident' ? 0xcd7 : 0x8d7;
  assert.equal(condition(4, f), truth); seed(c, 0xa000, initialFlags, r); pages(c, 'SMC', 'before SMC');
  run(c, writer, 0, cpu(c), 'zero before SMC producer', {retired: 0});
  input(c, 96, words([1])); run(c, writer, 1, cpu(c), 'cancel before SMC producer', {reason: 2, retired: 0});
  run(c, writer, 0, cpu(c), 'cancel before SMC producer and zero', {reason: 2, retired: 0}); input(c, 96, words([0]));
  let next = cpu(c); next.pc = 0xa007; next.flags = f; next.registers[6] = 0xa00b;
  run(c, writer, 2, next, 'CMP/MOV produce condition and original target once', {retired: 2});
  run(c, writer, 0, cpu(c), 'zero before executable byte store', {retired: 0});
  input(c, 96, words([1])); run(c, writer, 1, cpu(c), 'cancel before executable byte store', {reason: 2, retired: 0}); input(c, 96, words([0]));
  const beforeTrace = helperTrace.length;
  run(c, writer, 1, {...cpu(c), pc: 0xa00a}, 'identity or changed SETcc commits once before SMC6', {reason: 6, packet: storePacket()});
  assert.equal(helperTrace.length, beforeTrace + 1); assert.equal(helperTrace.at(-1).status, 11, 'observed canonical store status11 after committed code mutation');
  c.pages.get(0xa000)[11] = truth; neighbors(c, 0xa00b); guard(c, keeper);
  input(c, 96, words([1])); run(c, writer, 0, cpu(c), 'stale consumed source4 before zero/cancel', {status: 4});
  const successor = compile(c, 'current successor', [{pc: 0xa00a, length: 7}], 2, []); guard(c, keeper);
  input(c, 96, words([0])); next = cpu(c); next.pc = 0xa011; next.registers[0] = 0x12340000 + truth;
  run(c, successor, 3, next, 'current pure successor consumes changed immediate then cold; no head replay', {reason: 3, retired: 2});
  run(c, writer, 0, cpu(c), 'old writer after fresh publication distinguishes generation3/source4', {status: owner === 'replacement' ? 3 : 4});
  assert.deepEqual(Buffer.from(new Uint8Array(c.memory.buffer, keeper.pointer, keeper.length)), keeper.bytes); guard(c, keeper);
  pages(c, 'SMC', 'after successor'); close(c);
  run(c, successor, 0, cpu(c), 'closed successor before zero budget', {status: 5});
  counts.controls += 8; counts.smc_paths++; if (same) counts.identity_smc++;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const truth of [1, 0]) for (const same of [1, 0]) smcPath(owner, entries, truth, same);
assert.equal(rows.length, 10240); assert.equal(counts.canonical_cases, 8192); assert.equal(counts.ignored_cases, 1024); assert.equal(counts.ea_cases, 1024);
assert.equal(counts.contexts, 28); assert.equal(modules.length, 84);
assert.equal(counts.live_paths, 8); assert.equal(counts.live_faults, 32); assert.equal(counts.repair_phases, 16);
assert.equal(counts.smc_paths, 16); assert.equal(counts.identity_smc, 8);
assert.equal(counts.runs, 10640); assert.equal(counts.retired, 10368); assert.equal(counts.helper_calls, 10304);
assert.equal(counts.diagnostic_fault_read8, 4); assert.equal(counts.public_guard_controls, 24); assert.equal(counts.controls, 280);
assert.equal(counts.selected_ranges, 10280); assert.equal(counts.whole_pages, 256);
assert.equal(counts.diagnostic_read8, counts.selected_ranges * 32); assert.equal(counts.diagnostic_read32, counts.whole_pages * 1024);
const pe = runPeProof({engine, output, initial});
assert.deepEqual(sources(), sourcePins); assert.equal(hash(readFileSync(enginePath)), engine.sha256);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins,
  tools: {node: process.version, v8: process.versions.v8},
  micro: {counts, contexts, modules, observations, helper_trace: helperTrace, cases: rows, page_observations: pageObservations, guard_controls: guardControls,
    oracle: {condition_anchors: anchors, irrelevant_seeds: irrelevantSeeds, canonical_flags: allFlags, ea_shapes: shapeDefinitions,
      batches: batches.map(b => ({index: b.index, pc: b.pc, group: b.group, forms: b.forms.length, instructions: b.forms.length + 1, hex: b.code.toString('hex')}))}}, pe,
  limits: ['existing engine memory SETcc; four bound replacement/resident explicit/entry profiles; exact13 Rust-authored micro payloads and copied PE input retained',
    'full4612 arena asserted after each generated run before diagnostics, including dirtyFP128; saved rows are selected observations rather than raw full-arena dumps',
    '32-byte selected RAM range after each semantic row; whole declared pages only at group/live/SMC boundaries, not per-row whole guest memory',
    'actual forwarded guest memory helper chronology/status/packets is separate from public read8/read32 diagnostics; guards are canonical and not instrumented',
    'seven micro bank captures use resident7 of unchanged8slots; max8blocks/64instructions/65536Wasm unchanged; replacement banks compiled and exercised sequentially',
    'live repair scope is map-zero/protect-only; changed-RAM false overwrites are covered in finite rows, not a separate live host RAM edit',
    'false stores need Write permission; live map/protect-only repairs retain stopped CPU/FLAGS and do not replay CMP/MOV; true/false and same/changed SMC commit before6, reject4, resume current successor',
    'routine fixture requires frozen authored bytes and engine capabilities, no clang/LLVM execution; historical assembly encoding proof remains separate',
    'finite authored inputs and PE program only; no product change, performance/generalISA/OS/hardware/compiler/SDK/browser/game or hostedCI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', engine_sha256: engine.sha256, micro: counts, pe: pe.counts, modules: modules.length + pe.modules.length, output}));
