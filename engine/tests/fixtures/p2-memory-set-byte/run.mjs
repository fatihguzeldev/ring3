import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
assert.equal(WebAssembly.Module.exports(engineModule).length, 71);
assert.equal(WebAssembly.Module.exports(engineModule).filter(row => row.kind === 'function').length, 70);
function engineSignatures(bytes) {
  let at = 8;
  const unsigned = () => {let value = 0, shift = 0; for (let count = 0; count < 5; count++) {assert.ok(at < bytes.length); const byte = bytes[at++]; value |= (byte & 127) << shift; if (!(byte & 128)) return value >>> 0; shift += 7;} assert.fail('bounded engine unsigned LEB');};
  const name = () => {const length = unsigned(), end = at + length; assert.ok(end <= bytes.length); const result = Buffer.from(bytes.subarray(at, end)).toString(); at = end; return result;};
  const types = [], functions = [], exports = [];
  assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  while (at < bytes.length) {
    const section = bytes[at++], length = unsigned(), end = at + length; assert.ok(end <= bytes.length);
    if (section === 1) for (let count = unsigned(); count > 0; count--) {assert.equal(bytes[at++], 0x60); types.push({parameters: Array.from({length: unsigned()}, () => bytes[at++]), results: Array.from({length: unsigned()}, () => bytes[at++])});}
    else if (section === 2) assert.equal(unsigned(), 0, 'engine has no binary imports');
    else if (section === 3) for (let count = unsigned(); count > 0; count--) functions.push(unsigned());
    else if (section === 7) for (let count = unsigned(); count > 0; count--) exports.push({name: name(), kind: bytes[at++], index: unsigned()});
    else at = end;
    assert.equal(at, end);
  }
  const expectedArities = {'ring3_abi_v1_open': 3, 'ring3_abi_v1_close': 0, 'ring3_abi_v1_arena_ptr': 0, 'ring3_abi_v1_map': 3, 'ring3_abi_v1_protect': 3, 'ring3_abi_v1_unmap': 2, 'ring3_abi_v1_upload': 2, 'ring3_abi_v1_load_pe32': 1, 'ring3_abi_v1_load_pe32_at': 2, 'ring3_abi_v1_load_pe32_linked_at': 3, 'ring3_abi_v1_load_pe32_linked_v2_at': 3, 'ring3_abi_v1_begin_image_input': 1, 'ring3_abi_v1_append_image_input': 2, 'ring3_abi_v1_abort_image_input': 0, 'ring3_abi_v1_load_pe32_linked_v2_input_at': 2, 'ring3_abi_v1_start_loaded_image': 2, 'ring3_abi_v1_compile': 1, 'ring3_abi_v1_generation': 0, 'ring3_abi_v1_compile_with_gates': 2, 'ring3_abi_v1_compile_entries': 2, 'ring3_abi_v1_compile_resident': 1, 'ring3_abi_v1_compile_resident_with_gates': 2, 'ring3_abi_v1_compile_resident_entries': 2, 'ring3_abi_v1_find_resident': 1, 'ring3_abi_v1_compile_resident_callback_unit': 7, 'ring3_abi_v1_acknowledge_resident_callback_installation': 8, 'ring3_abi_v1_dispatcher_module': 2, 'ring3_abi_v1_resident_module': 2, 'ring3_abi_v1_retire_stale_resident': 4, 'ring3_abi_v1_discard_unacknowledged_resident': 4, 'ring3_abi_v1_acknowledge_resident_installation': 5, 'ring3_abi_v1_find_installed_resident': 3, 'ring3_abi_v1_guard_dispatch_entry': 5, 'ring3_abi_v1_guard_resident': 7, 'ring3_abi_v1_module_ptr': 0, 'ring3_abi_v1_module_len': 0, 'ring3_abi_v1_guard': 6, 'ring3_abi_v1_write8': 2, 'ring3_abi_v1_store8': 2, 'ring3_abi_v1_store_resident8': 6, 'ring3_abi_v1_read8': 1, 'ring3_abi_v1_read16': 1, 'ring3_abi_v1_read32': 1, 'ring3_abi_v1_write32': 2, 'ring3_abi_v1_write_words32': 1, 'ring3_abi_v1_store32': 2, 'ring3_abi_v1_store_resident32': 6, 'ring3_abi_v1_capture_call': 5, 'ring3_abi_v1_complete_call': 5, 'ring3_abi_v1_capture_resident_call': 6, 'ring3_abi_v1_complete_resident_call': 6, 'ring3_abi_v1_abandon_call': 3, 'ring3_abi_v1_complete_windows_call': 4, 'ring3_abi_v1_complete_resident_windows_call': 5, 'ring3_abi_v1_select_resident_callback_unit': 7, 'ring3_abi_v1_capture_active_resident_callback_call': 7, 'ring3_abi_v1_complete_active_resident_callback_call': 7, 'ring3_abi_v1_complete_active_resident_callback_windows_call': 6, 'ring3_abi_v1_capture_resident_callback_call': 7, 'ring3_abi_v1_complete_resident_callback_call': 7, 'ring3_abi_v1_begin_callback': 8, 'ring3_abi_v1_finish_callback': 4, 'ring3_abi_v1_begin_resident_callback': 11, 'ring3_abi_v1_finish_resident_callback': 5, 'ring3_abi_v1_authorize_resident_callback': 5, 'ring3_abi_v1_abort_callback': 3, 'ring3_abi_v1_resume_callback_code': 6, 'ring3_abi_v1_resume_callback_entries': 6, 'ring3_abi_v1_load_pe32_linked_v3_input_at': 2, 'ring3_abi_v1_load_pe32_linked_v4_input_at': 2};
  assert.equal(exports.length, 71); assert.deepEqual(exports.filter(row => row.kind !== 0), [{name: 'memory', kind: 2, index: 0}]);
  const signatures = {};
  for (const item of exports.filter(row => row.kind === 0)) {assert.ok(Object.hasOwn(expectedArities, item.name)); assert.ok(!Object.hasOwn(signatures, item.name)); const expected = {parameters: Array(expectedArities[item.name]).fill(0x7f), results: [0x7f]}; assert.deepEqual(types[functions[item.index]], expected, item.name); signatures[item.name] = expected;}
  assert.equal(Object.keys(signatures).length, 70); return signatures;
}
const engineFunctionSignatures = engineSignatures(engineBytes);
const SIZE = 4236, TRANSFER = 140, EXIT = 0x8020, EXIT_ID = 0x10003, MARKER = 0xc8c00015;
const irrelevantSeeds = [0, 0x10, 0x400, 0x410];
const initialRegisters = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, unmap: 2, upload: 2, read8: 1, read32: 1, write8: 2, store8: 2, store_resident8: 6, compile_entries: 2, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store32: 2, store_resident32: 6, begin_image_input: 1, append_image_input: 2, load_pe32_linked_v2_input_at: 2, start_loaded_image: 2, capture_call: 5, capture_resident_call: 6, complete_windows_call: 4, complete_resident_windows_call: 5};
const sourcePaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/control.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/cpu/dbt/wasm/memory/byte_store.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/locals.rs', 'engine/src/process/startup.rs', 'engine/src/abi/x86/exit.rs', 'engine/src/process/instance.rs', 'engine/src/process/wasm.rs', 'engine/src/process/call.rs', 'engine/src/process/windows.rs', 'engine/src/abi/wasm/exports.rs', 'engine/src/abi/memory_helper.rs', 'engine/src/loader/mod.rs', 'engine/src/loader/imports.rs', 'engine/src/loader/relocation.rs', 'engine/src/memory/space.rs', 'engine/src/loader/pe32.rs', 'engine/src/process/image_input.rs', 'engine/src/process/image.rs', 'engine/src/cpu/dbt/resident.rs', 'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/gate.rs', 'engine/src/memory/address.rs', 'engine/src/memory/code.rs', 'engine/src/abi/x86/state.rs', 'engine/src/abi/call_frame.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/wasm/mod.rs', 'engine/tests/support/pe32.rs', 'engine/tests/cpu_memory_set_byte_wasm.rs', 'engine/tests/fixtures/p2-memory-set-byte/run.mjs', 'engine/tests/fixtures/p2-memory-set-byte/integer.S'];
sourcePaths.push('engine/src/lib.rs', 'engine/src/process/mod.rs', 'engine/src/process/resident.rs', 'engine/src/windows/mod.rs', 'engine/src/windows/provider.rs', 'engine/src/windows/calling_convention.rs', 'engine/src/abi/arena.rs', 'engine/src/abi/wasm/mod.rs', 'engine/src/cpu/x86/decode/mod.rs', 'engine/src/cpu/dbt/mod.rs', 'engine/src/memory/mod.rs', 'engine/src/abi/mod.rs', 'engine/src/abi/header.rs', 'engine/src/abi/x86/mod.rs', 'engine/src/cpu/mod.rs', 'engine/src/cpu/x86/mod.rs', 'engine/src/process/installation.rs', 'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/exit.rs');
assert.equal(sourcePaths.length, 65); assert.equal(new Set(sourcePaths).size, 65);
const sourceHashes = Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
writeFileSync(join(output, 'source-hashes.json'), JSON.stringify({engine_sha256: hash(engineBytes), sources: sourceHashes}, null, 2));
writeFileSync(join(output, 'engine.wasm'), engineBytes);
const rows = [], controls = [], liveRows = [], smcRows = [], peRows = [], modules = [], generatedRuns = [];
let ordinal = 0, ramChecks = 0, pageChecks = 0, hostRead32Calls = 0, hostRead8Calls = 0, residentPeak = 0;

function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true)); return bytes;
}
function words(fields) {
  const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer);
  fields.forEach((value, index) => view.setUint32(index * 4, value, true)); return bytes;
}
function refresh(ctx) { ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx; }
function arena(ctx) { return refresh(ctx).bytes.slice(ctx.base, ctx.base + SIZE); }
function stateRecord(registers, pc, flags) { return record('R3ST', 56, [...registers, pc, flags]); }
function exitRecord(reason, count, version = 2, detail = 0, address = 0, access = 0, length = 0) { return record('R3EX', 40, [reason, count, detail, address, access, length], version); }
function helper(value, reason = 0, address = 0) { return record('R3MH', 40, [reason ? 1 : 0, reason ? 0 : value, reason, address, reason ? 1 : 0, 1], 2); }
function request(ctx, bytes) { assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); }
function pure(ctx, action, status, name, control = true) {
  const before = arena(ctx); assert.equal(action(), status, name); assert.deepEqual(arena(ctx), before, `${name}: full arena unchanged`);
  if (control) controls.push({owner: ctx.owner, name, status});
}
function fresh(owner, pages = 8) {
  const instance = new WebAssembly.Instance(engineModule, {}), key = 0xc840000000000000n + BigInt(++ordinal);
  const api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
    const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
  }));
  const ctx = {owner, memory: instance.exports.memory, api, low: Number(key & 0xffffffffn), high: Number(key >> 32n), ramPages: new Map(), units: 0};
  assert.equal(api.open(pages, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0; return refresh(ctx);
}
function upload(ctx, address, bytes) { request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), 0, 'explicit host upload preserves arena', false); }
function guardCurrent(ctx, unit, label = 'data-only effects retain code owner') {
  const action = ctx.owner === 'replacement' ? () => ctx.api.guard(ctx.low, ctx.high, unit.generation, ctx.base, ctx.base + 56, ctx.base + 96)
    : () => ctx.api.guard_resident(ctx.low, ctx.high, unit.low, unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
  pure(ctx, action, 0, label, false);
  refresh(ctx); assert.deepEqual(ctx.bytes.slice(unit.pointer, unit.pointer + unit.length), unit.bytes, 'current generated module bytes retain their original artifact');
}
function childShape(ctx, bytes, binding, helpers, hasGates) {
  let at = 8;
  const unsigned = () => {let value = 0, shift = 0; for (let count = 0; count < 5; count++) {assert.ok(at < bytes.length); const next = bytes[at++]; value |= (next & 127) << shift; if (!(next & 128)) return value >>> 0; shift += 7;} assert.fail('bounded unsigned LEB');};
  const signed = () => {let value = 0n, shift = 0n, next; do {assert.ok(at < bytes.length && shift < 35n); next = bytes[at++]; value |= BigInt(next & 127) << shift; shift += 7n;} while (next & 128); if (next & 64) value -= 1n << shift; return Number(BigInt.asUintN(32, value));};
  const name = () => {const length = unsigned(), end = at + length; assert.ok(end <= bytes.length); const value = Buffer.from(bytes.subarray(at, end)).toString(); at = end; return value;};
  const types = [], imports = [], functions = [], exports = []; let locals, guard;
  assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  while (at < bytes.length) {
    const section = bytes[at++], length = unsigned(), end = at + length; assert.ok(end <= bytes.length);
    if (section === 1) for (let count = unsigned(); count > 0; count--) {assert.equal(bytes[at++], 0x60); types.push({parameters: Array.from({length: unsigned()}, () => bytes[at++]), results: Array.from({length: unsigned()}, () => bytes[at++])});}
    else if (section === 2) for (let count = unsigned(); count > 0; count--) {const module = name(), field = name(), kind = bytes[at++]; if (kind === 0) imports.push({module, name: field, kind: 'function', type: unsigned()}); else {assert.equal(kind, 2); assert.equal(unsigned(), 0); assert.equal(unsigned(), 1); imports.push({module, name: field, kind: 'memory'});}}
    else if (section === 3) for (let count = unsigned(); count > 0; count--) functions.push(unsigned());
    else if (section === 7) for (let count = unsigned(); count > 0; count--) exports.push({name: name(), kind: bytes[at++], index: unsigned()});
    else if (section === 10) {
      assert.equal(unsigned(), 1); const bodyLength = unsigned(); assert.equal(at + bodyLength, end);
      locals = Array.from({length: unsigned()}, () => [unsigned(), bytes[at++]]); assert.deepEqual(locals, helpers.length > 1 || hasGates ? [[16, 0x7f], [1, 0x7e], [6, 0x7f]] : [[16, 0x7f], [1, 0x7e]]);
      const constants = [ctx.low, ctx.high, ...(ctx.owner === 'replacement' ? [binding.generation] : [binding.low, binding.high])];
      for (const value of constants) {assert.equal(bytes[at++], 0x41); assert.equal(signed(), value);}
      for (const index of [0, 1, 3]) {assert.equal(bytes[at++], 0x20); assert.equal(unsigned(), index);}
      assert.equal(bytes[at++], 0x10); assert.equal(unsigned(), 0); guard = {name: helpers[0], constants, parameters: [0, 1, 3]}; at = end;
    } else assert.fail(`unexpected child section ${section}`);
    assert.equal(at, end);
  }
  assert.deepEqual(functions, [0]); assert.deepEqual(types[0], {parameters: Array(4).fill(0x7f), results: [0x7f]});
  assert.deepEqual(imports[0], {module: 'env', name: 'memory', kind: 'memory'});
  assert.deepEqual(imports.slice(1).map(({module, name, kind}) => ({module, name, kind})), helpers.map(name => ({module: 'ring3', name, kind: 'function'})));
  assert.deepEqual(exports, [{name: 'run', kind: 0, index: helpers.length}]); assert.ok(locals && guard);
  const signatures = Object.fromEntries(imports.slice(1).map(row => {
    const expected = {parameters: Array(arities[row.name]).fill(0x7f), results: [0x7f]}; assert.deepEqual(types[row.type], expected); return [row.name, expected];
  }));
  return {locals, guard, import_signatures: signatures, run_signature: types[0]};
}
function compile(ctx, entries, gates, label, extraHelpers = [storeName(ctx)]) {
  assert.ok(entries.length <= 8); if (ctx.owner === 'resident') {assert.ok(++ctx.units <= 8, 'finite resident occupancy'); residentPeak = Math.max(residentPeak, ctx.units);} request(ctx, words([...entries, ...gates.flat()])); const before = arena(ctx); let binding, expected = before.slice();
  if (ctx.owner === 'replacement') {
    assert.equal(ctx.api.compile_entries(entries.length, gates.length), 0);
    binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
  } else {
    assert.equal(ctx.api.compile_resident_entries(entries.length, gates.length), 0); refresh(ctx); const at = ctx.base + TRANSFER;
    assert.equal(ctx.view.getUint32(at, true), 1); assert.equal(ctx.view.getUint32(at + 4, true), 24);
    binding = {low: ctx.view.getUint32(at + 8, true), high: ctx.view.getUint32(at + 12, true), pointer: ctx.view.getUint32(at + 16, true), length: ctx.view.getUint32(at + 20, true)};
    assert.ok(binding.low !== 0 || binding.high !== 0); expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER);
  }
  assert.deepEqual(arena(ctx), expected, 'successful publication preserves CPU/exit/helper and exact owner receipt');
  refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
  const bytes = ctx.bytes.slice(binding.pointer, binding.pointer + binding.length), helpers = [ctx.owner === 'replacement' ? 'guard' : 'guard_resident', ...extraHelpers];
  assert.ok(WebAssembly.validate(bytes)); const shape = childShape(ctx, bytes, binding, helpers, gates.length > 0), module = new WebAssembly.Module(bytes);
  const imports = WebAssembly.Module.imports(module); assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}); assert.equal(instance.exports.run.length, 4);
  writeFileSync(join(output, `${label}.wasm`), bytes); modules.push({label, owner: ctx.owner, entries, ...binding, key: [ctx.low, ctx.high], sha256: hash(bytes), imports, ...shape});
  return {...binding, run: instance.exports.run, label, bytes};
}
function guestRun(ctx, unit, state, exit, budget, cancel) {
  const status = unit.run(state, exit, budget, cancel); generatedRuns.push({owner: ctx.owner, module: unit.label, status, budget}); return status;
}
function seed(ctx, registers, pc, flags, cancel = 0) {
  refresh(ctx).bytes.set(stateRecord(registers, pc, flags), ctx.base); ctx.bytes.set(exitRecord(3, 0, 1), ctx.base + 56);
  ctx.view.setUint32(ctx.base + 96, cancel, true); ctx.bytes.fill(0xa5, ctx.base + 100, ctx.base + SIZE);
}

function storeHelper() {return record('R3MH', 40, [0, 0, 0, 0, 0, 1], 3);}
function read8(ctx, address, expectedValue, reason = 0) {
  const expected = arena(ctx); expected.set(helper(expectedValue, reason, reason ? address : 0), 100);
  assert.equal(ctx.api.read8(address), 0); hostRead8Calls++; assert.deepEqual(arena(ctx), expected, 'normal host Read8 changes only strict v2 helper'); return expectedValue;
}
function readPage(ctx, address) {
  const bytes = ctx.ramPages.get(address); assert.ok(bytes && bytes.length === 4096); const before = arena(ctx), observed = Buffer.alloc(4096), expected = before.slice();
  for (let offset = 0; offset < 4096; offset += 4) {
    const value = bytes.readUInt32LE(offset), strict = record('R3MH', 40, [0, value, 0, 0, 0, 0]);
    assert.equal(ctx.api.read32(address + offset), 0); hostRead32Calls++; refresh(ctx);
    assert.deepEqual(ctx.bytes.slice(ctx.base + 100, ctx.base + 140), strict, 'complete-page Read32 exact helper');
    observed.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset); expected.set(strict, 100);
  }
  assert.deepEqual(observed, bytes, 'every byte of declared complete page'); assert.deepEqual(arena(ctx), expected, 'page observation preserves CPU/exit/cancel/transfer'); pageChecks++; return hash(observed);
}
function readPages(ctx) {return [...ctx.ramPages.keys()].map(address => ({address, bytes: 4096, sha256: readPage(ctx, address)}));}
function readDeclared(ctx, address, length = 32) {
  const page = Math.floor(address / 4096) * 4096, expected = ctx.ramPages.get(page); assert.ok(expected && address + length <= page + 4096);
  const bytes = expected.subarray(address - page, address - page + length); for (let index = 0; index < length; index++) read8(ctx, address + index, bytes[index]); ramChecks++; return bytes.toString('hex');
}
function hostByte(ctx, address, value) {
  const expected = arena(ctx); expected.set(storeHelper(), 100); assert.equal(ctx.api.write8(address, value), 0); assert.deepEqual(arena(ctx), expected, 'declared host byte input changes strict helper only');
  ctx.ramPages.get(Math.floor(address / 4096) * 4096)[address % 4096] = value;
}
function storeName(ctx) {return ctx.owner === 'replacement' ? 'store8' : 'store_resident8';}
function storeFault(reason, address) {return record('R3MH', 40, [1, 0, reason, address, 2, 1], 3);}
function condition(index, flags) {
  const bit = weight => Math.floor(flags / weight) % 2 !== 0;
  const carry = bit(1), parity = bit(4), zero = bit(64), sign = bit(128), overflow = bit(2048);
  const predicates = [overflow, !overflow, carry, !carry, zero, !zero, carry || zero, !carry && !zero,
    sign, !sign, parity, !parity, sign !== overflow, sign === overflow, zero || sign !== overflow, !zero && sign === overflow];
  return predicates[index] ? 1 : 0;
}
const anchors = [[0x802, 2], [2, 0x802], [3, 2], [2, 3], [0x42, 2], [2, 0x42], [3, 2], [2, 3],
  [0x82, 2], [2, 0x82], [6, 2], [2, 6], [0x82, 2], [2, 0x82], [0x42, 2], [2, 0x42]];
for (let index = 0; index < 16; index++) {
  assert.equal(condition(index, anchors[index][0]), 1); assert.equal(condition(index, anchors[index][1]), 0);
  for (const irrelevant of irrelevantSeeds) {assert.equal(condition(index, anchors[index][0] + irrelevant), 1); assert.equal(condition(index, anchors[index][1] + irrelevant), 0);}
}
assert.equal(condition(12, 0x882), 0); assert.equal(condition(15, 0x882), 1);
function canonicalFlags(value, irrelevant) {
  const weights = [1, 4, 64, 128, 2048]; let flags = 2 + irrelevant;
  for (let index = 0; index < 5; index++) flags += Math.floor(value / 2 ** index) % 2 * weights[index];
  return flags;
}
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
function runCondition(ctx, unit, form, registers, flags, initial, group, name) {
  const range = Math.floor(form.address / 32) * 32, page = Math.floor(form.address / 4096) * 4096;
  const neighbors = Buffer.from(Array.from({length: 32}, (_, index) => index + 1)); neighbors[form.address - range] = initial;
  upload(ctx, range, neighbors); ctx.ramPages.get(page).set(neighbors, range % 4096); guardCurrent(ctx, unit);
  const beforePages = readPages(ctx), beforeHex = readDeclared(ctx, range), value = condition(form.condition, flags);
  pure(ctx, () => ctx.api.protect(page, 1, 2), 0, 'WRITE-only destination before generated Store8', false);
  seed(ctx, registers, form.pc, flags); const expected = arena(ctx);
  expected.set(stateRecord(registers, form.pc + form.bytes.length, flags)); expected.set(exitRecord(1, 1), 56); expected.set(storeHelper(), 100);
  assert.equal(guestRun(ctx, unit, ctx.base, ctx.base + 56, 1, ctx.base + 96), 0, name);
  assert.deepEqual(arena(ctx), expected, `${name}: allGPR/entireFLAGS/strictv3/wholearena BEFORE observation`);
  ctx.ramPages.get(page)[form.address % 4096] = value;
  pure(ctx, () => ctx.api.protect(page, 1, 3), 0, 'observation-only RW after WRITE-only successful store', false);
  const afterPages = readPages(ctx), afterHex = readDeclared(ctx, range); guardCurrent(ctx, unit);
  const expectedNeighbors = Buffer.from(beforeHex, 'hex'); expectedNeighbors[form.address - range] = value; assert.equal(afterHex, expectedNeighbors.toString('hex'));
  for (let index = 0; index < beforePages.length; index++) if (beforePages[index].address !== page) assert.deepEqual(afterPages[index], beforePages[index]);
  rows.push({owner: ctx.owner, name, group, condition: form.condition, ignored: form.ignored, shape: form.shape, encoding: Buffer.from(form.bytes).toString('hex'), pc: form.pc, address: form.address,
    initial_registers: registers, initial_flags: flags, initial_byte: initial, value, flags, expected_registers: registers, retired: 1, next_pc: form.pc + form.bytes.length,
    declared_ram: {address: range, bytes: 32, before_hex: beforeHex, expected_hex: afterHex, expected_sha256: hash(Buffer.from(afterHex, 'hex'))}, complete_pages_before: beforePages, complete_pages_after: afterPages,
    permissions: {guest: 2, observation: 3}, claim: 'expected outcomes backed by livefullarena/wholepages; not raw perrow snapshots'});
}
function noRetirement(ctx, unit, pc, cancel) {
  seed(ctx, initialRegisters, pc, 0xcd7, cancel); const expected = arena(ctx); expected.set(exitRecord(cancel ? 2 : 1, 0), 56);
  assert.equal(guestRun(ctx, unit, ctx.base, ctx.base + 56, cancel ? 1 : 0, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expected, 'zero/cancel precedes Store8'); readPages(ctx);
  controls.push({owner: ctx.owner, name: `${unit.label}:${cancel ? 'cancel_before_store' : 'zero_before_store'}`, budget: cancel ? 1 : 0, retired: 0});
}
function elfFixtures(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]); assert.equal(view.getUint16(18, true), 3);
  const start = view.getUint32(32, true), stride = view.getUint16(46, true), count = view.getUint16(48, true);
  const sections = Array.from({length: count}, (_, index) => {
    const at = start + index * stride;
    return {type: view.getUint32(at + 4, true), offset: view.getUint32(at + 16, true), size: view.getUint32(at + 20, true), link: view.getUint32(at + 24, true), info: view.getUint32(at + 28, true), stride: view.getUint32(at + 36, true)};
  });
  const table = sections.find(section => section.type === 2); assert.ok(table); const strings = sections[table.link], symbols = new Map();
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    const nameStart = strings.offset + view.getUint32(at, true), name = object.subarray(nameStart, object.indexOf(0, nameStart)).toString();
    const section = view.getUint16(at + 14, true), offset = view.getUint32(at + 4, true), size = view.getUint32(at + 8, true);
    if (section > 0 && section < sections.length && size > 0) {
      const source = sections[section]; assert.ok(offset + size <= source.size);
      assert.ok(!sections.some(item => [4, 9].includes(item.type) && item.info === section && item.size > 0));
      symbols.set(name, {offset, bytes: object.subarray(source.offset + offset, source.offset + offset + size)});
    }
  }
  return symbols;
}
const assemblyPath = join(root, 'engine/tests/fixtures/p2-memory-set-byte/integer.S'), objectPath = join(output, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assemblyPath, '-o', objectPath];
const disassemble = ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', disassemble, {encoding: 'utf8'});
writeFileSync(join(output, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), symbols = elfFixtures(object), llvm = [];
const encodingSamples = [['sete_memory', '0f9406'], ['setge_memory_ignored', '0f9d3c24']];
let llvmOffset = 0;
for (const [name, hex] of encodingSamples) {
  const symbol = symbols.get(name); assert.ok(symbol, name); assert.equal(symbol.offset, llvmOffset); assert.equal(symbol.bytes.toString('hex'), hex);
  llvm.push({name, offset: llvmOffset, hex}); llvmOffset += hex.length / 2;
  const start = disassembly.indexOf(`<${name}>:`); assert.ok(start >= 0);
  const next = disassembly.indexOf('\n\n', start), body = disassembly.slice(start, next < 0 ? undefined : next);
  assert.match(body, new RegExp(`\\b${name.split('_')[0]}\\s+`), `${name}: LLVM decodes the canonical family`);
}
assert.equal(symbols.size, 2); assert.equal(llvmOffset, 7);
for (const owner of ['replacement', 'resident']) {
  const ctx = fresh(owner), codePage = Buffer.alloc(4096, 0xcc);
  for (const item of batches) codePage.set(item.code, item.pc - 0x8000);
  for (const address of [0x8000, 0x3000, 0xfffff000]) {
    pure(ctx, () => ctx.api.map(address, 1, address === 0x8000 ? 7 : 3), 0, 'map declared authored page', false);
    const data = address === 0x8000 ? codePage : Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1));
    upload(ctx, address, data); ctx.ramPages.set(address, data);
  }
  let canonicalUnit, lastUnit;
  for (const item of batches) {
    const unit = compile(ctx, [item.pc], [], `${owner}-${item.group}-${item.index}`); if (!item.index) canonicalUnit = unit; lastUnit = unit;
    for (const form of item.forms) {
      const registers = [...initialRegisters]; if (form.values) for (const [index, value] of Object.entries(form.values)) registers[Number(index)] = value;
      else registers[6] = form.address;
      if (item.group === 'canonical') {
        for (let relevant = 0; relevant < 32; relevant++) for (const irrelevant of irrelevantSeeds) {
          const flags = canonicalFlags(relevant, irrelevant), initial = [0, 1, 0xa5][(relevant + irrelevantSeeds.indexOf(irrelevant) + form.condition) % 3];
          runCondition(ctx, unit, form, registers, flags, initial, 'canonical', `${owner}-canonical-${form.condition}-${relevant}-${irrelevant}`);
        }
      } else for (const truth of [1, 0]) {
        const flags = anchors[form.condition][truth ? 0 : 1] + (form.ignored % 2 ? 0x410 : 0x10);
        runCondition(ctx, unit, form, registers, flags, truth ? 0 : 1, item.group, `${owner}-${item.group}-${form.condition}-${form.ignored}-${form.shape ?? 0}-${truth}`);
      }
    }
    noRetirement(ctx, unit, item.pc, 0); noRetirement(ctx, unit, item.pc, 1);
  }
  pure(ctx, () => guestRun(ctx, canonicalUnit, 0xffffffff, 0xffffffff, 0, 0xffffffff), owner === 'replacement' ? 3 : 1, `${owner}: earlier canonical identity before malformed pointer`); readPages(ctx);
  read8(ctx, 0x4000, 0, 1); controls.push({owner, name: 'page following last-byte destination stays unmapped', address: 0x4000, reason: 1, length: 1});
  const guard = (low, high, id, highId, pointer = ctx.base) => owner === 'replacement' ? ctx.api.guard(low, high, id, pointer, ctx.base + 56, ctx.base + 96) : ctx.api.guard_resident(low, high, id, highId, pointer, ctx.base + 56, ctx.base + 96);
  const id = owner === 'replacement' ? lastUnit.generation : lastUnit.low;
  pure(ctx, () => guard(ctx.low + 1, ctx.high, id, lastUnit.high), 3, `${owner}: wrong key low`);
  pure(ctx, () => guard(ctx.low, ctx.high + 1, id, lastUnit.high), 3, `${owner}: wrong key high`);
  pure(ctx, () => guard(ctx.low, ctx.high, id + 1, lastUnit.high), 3, `${owner}: wrong generation or id low`);
  pure(ctx, () => guard(ctx.low, ctx.high, owner === 'replacement' ? 0 : id, owner === 'replacement' ? undefined : lastUnit.high + 1), 3, `${owner}: zero generation or wrong id high`);
  pure(ctx, () => guard(ctx.low, ctx.high, id, lastUnit.high, 0xffffffff), 1, `${owner}: current malformed guard pointer`);
  pure(ctx, () => guestRun(ctx, lastUnit, 0xffffffff, 0xffffffff, 1, 0xffffffff), 1, `${owner}: current generated malformed pointer`); readPages(ctx);
  hostByte(ctx, batches[6].pc, batches[6].code[0]); refresh(ctx).view.setUint32(ctx.base + 96, 1, true);
  pure(ctx, () => guard(ctx.low, ctx.high, id, lastUnit.high), 4, `${owner}: external same-byte code input stale`);
  pure(ctx, () => guestRun(ctx, lastUnit, 0xffffffff, 0xffffffff, 0, 0xffffffff), 4, `${owner}: stale before zero/cancel/malformed pointers`); readPages(ctx);
  pure(ctx, () => ctx.api.close(), 0, 'close micro context', false);
  pure(ctx, () => guestRun(ctx, lastUnit, 0xffffffff, 0xffffffff, 0, 0xffffffff), 5, `${owner}: Closed before stale/malformed pointers`);
}
function livePath(owner, truth) {
  const ctx = fresh(owner), label = `${owner}-live-${truth}`, address = 0x400f;
  const code = Buffer.from('38c8be0f4000000f90060f90c38a06eb000f0b', 'hex'); assert.deepEqual(code, readFileSync(join(output, 'live.x86')));
  const codePage = Buffer.alloc(4096, 0xcc); codePage.set(code);
  pure(ctx, () => ctx.api.map(0x9000, 1, 7), 0, 'map all live code before compilation', false); upload(ctx, 0x9000, codePage); ctx.ramPages.set(0x9000, codePage);
  const unit = compile(ctx, [0x9000], [], label, ['read8', storeName(ctx)]), registers = [...initialRegisters];
  registers[0] = truth ? 0x1234807f : 0x12348080; registers[1] = truth ? 0x23457fff : 0x23457f80;
  const initialRegistersLive = [...registers], initialFlags = owner === 'replacement' ? 0x8d7 : 0xcd7;
  const producerFlags = (truth ? 0x883 : 0x46) + (owner === 'resident' ? 0x400 : 0), changed = truth ? 0xa5 : 0xff, slices = [], repairs = [], observationInputs = [];
  assert.equal(condition(0, producerFlags), truth); seed(ctx, registers, 0x9000, initialFlags); let writeOnly = false;
  function slice(name, budget, retired, offset, flags, changes = {}, fault = 0, stored = false, loaded = false, cancel = 0) {
    refresh(ctx).view.setUint32(ctx.base + 96, cancel, true); const expected = arena(ctx);
    for (const [index, value] of Object.entries(changes)) registers[Number(index)] = value;
    expected.set(stateRecord(registers, 0x9000 + offset, flags)); expected.set(fault ? exitRecord(5, retired, 2, fault, address, 2, 1) : exitRecord(cancel ? 2 : 1, retired), 56);
    if (fault) expected.set(storeFault(fault, address), 100); else if (stored) expected.set(storeHelper(), 100); else if (loaded) expected.set(helper(truth), 100);
    assert.equal(guestRun(ctx, unit, ctx.base, ctx.base + 56, budget, ctx.base + 96), 0, name); assert.deepEqual(arena(ctx), expected, `${label}:${name}: wholearena BEFORE observations`);
    if (stored) ctx.ramPages.get(0x4000)[15] = truth;
    if (writeOnly) {pure(ctx, () => ctx.api.protect(0x4000, 1, 3), 0, 'explicit observation-only RW', false); observationInputs.push({slice: name, api: 'protect', permissions: 3});}
    const pages = readPages(ctx);
    if (writeOnly) {pure(ctx, () => ctx.api.protect(0x4000, 1, 2), 0, 'restore WRITE-only before next guest', false); observationInputs.push({slice: name, api: 'protect', permissions: 2});}
    slices.push({name, budget, retired, pc: 0x9000 + offset, flags, registers: [...registers], cancel, fault: fault ? {reason: fault, address, access: 2, length: 1} : undefined, complete_pages: pages});
    if (!retired && !fault) controls.push({owner, name: `${label}:${name}`, budget, retired: 0});
  }
  slice('zero_before_CMP', 0, 0, 0, initialFlags); slice('cancel_before_CMP', 1, 0, 0, initialFlags, {}, 0, false, false, 1);
  slice('CMP_produces_true_or_false_OF_once', 1, 1, 2, producerFlags);
  slice('zero_after_CMP', 0, 0, 2, producerFlags); slice('cancel_after_CMP', 1, 0, 2, producerFlags, {}, 0, false, false, 1);
  slice('MOV_pointer_once', 1, 1, 7, producerFlags, {6: address});
  slice('zero_before_unmapped_store', 0, 0, 7, producerFlags); slice('cancel_before_unmapped_store', 1, 0, 7, producerFlags, {}, 0, false, false, 1);
  slice('unmapped_Write1_fault_preserves_condition', 1, 0, 7, producerFlags, {}, 1);
  const data = Buffer.alloc(4096), neighbors = Buffer.from(Array.from({length: 32}, (_, index) => 0xa1 + index)); neighbors[15] = 0;
  pure(ctx, () => ctx.api.map(0x4000, 1, 3), 0, 'repair1 map RW', false); upload(ctx, 0x4000, neighbors); data.set(neighbors); ctx.ramPages.set(0x4000, data);
  const firstHex = readDeclared(ctx, 0x4000); pure(ctx, () => ctx.api.protect(0x4000, 1, 1), 0, 'repair1 initialized page becomes READ-only', false); guardCurrent(ctx, unit);
  repairs.push({phase: 1, actions: [{api: 'map', address: 0x4000, pages: 1, permissions: 3}, {api: 'upload', address: 0x4000, bytes: 32, hex: firstHex}, {api: 'protect', address: 0x4000, pages: 1, permissions: 1}]});
  slice('zero_at_READ_only_fault_PC', 0, 0, 7, producerFlags); slice('cancel_at_READ_only_fault_PC', 1, 0, 7, producerFlags, {}, 0, false, false, 1);
  slice('READ_only_Write1_fault_even_false_zero', 1, 0, 7, producerFlags, {}, 2);
  pure(ctx, () => ctx.api.protect(0x4000, 1, 3), 0, 'repair2 RW', false); hostByte(ctx, address, changed);
  const changedHex = readDeclared(ctx, 0x4000); pure(ctx, () => ctx.api.protect(0x4000, 1, 2), 0, 'repair2 WRITE-only permits store and forbids Read8', false); writeOnly = true; guardCurrent(ctx, unit);
  repairs.push({phase: 2, actions: [{api: 'protect', address: 0x4000, pages: 1, permissions: 3}, {api: 'write8', address, value: changed}, {api: 'protect', address: 0x4000, pages: 1, permissions: 2}]});
  slice('zero_before_retry', 0, 0, 7, producerFlags); slice('cancel_before_retry', 1, 0, 7, producerFlags, {}, 0, false, false, 1);
  const beforeStore = [...registers]; slice('same_SETcc_overwrites_changed_RAM_and_preserves_FLAGS', 1, 1, 10, producerFlags, {}, 0, true); assert.deepEqual(registers, beforeStore);
  slice('zero_after_store', 0, 0, 10, producerFlags); slice('cancel_after_store', 1, 0, 10, producerFlags, {}, 0, false, false, 1);
  pure(ctx, () => ctx.api.protect(0x4000, 1, 1), 0, 'declared READ-only consumer input after successful WRITE-only store', false); writeOnly = false;
  const finalHex = readDeclared(ctx, 0x4000), expectedNeighbors = Buffer.from(firstHex, 'hex'); expectedNeighbors[15] = truth; assert.equal(finalHex, expectedNeighbors.toString('hex'));
  slice('register_SETO_consumes_preserved_condition', 1, 1, 13, producerFlags, {3: registers[3] - registers[3] % 256 + truth});
  slice('MOV8_load_consumes_stored_value', 1, 1, 15, producerFlags, {0: registers[0] - registers[0] % 256 + truth}, 0, false, true);
  slice('JMP_finishes_without_prefix_replay', 1, 1, 17, producerFlags); guardCurrent(ctx, unit);
  assert.equal(slices.length, 20); assert.equal(slices.reduce((sum, row) => sum + row.retired, 0), 6); assert.equal(observationInputs.length, 10);
  liveRows.push({owner, truth, module_label: unit.label, initial_registers: initialRegistersLive, initial_flags: initialFlags, producer_flags: producerFlags, initial_pc: 0x9000, store_pc: 0x9007,
    address, changed_byte: changed, first_ram_hex: firstHex, changed_ram_hex: changedHex, final_ram_hex: finalHex, guest_retired: 6, slices, repairs, observation_inputs: observationInputs,
    consumer_permission_input: {api: 'protect', address: 0x4000, pages: 1, permissions: 1}, transfer_after_direct_write: firstHex,
    claim: 'one initial seed; genuine CMP, MOVpointer once, two Writefaults, data-only repair and same SETcc retry; Transfer retains uploadedinitial00; no CPU/FLAGS/EIP patch, recompile or prefix replay'});
  pure(ctx, () => ctx.api.close(), 0, 'close live context', false);
}
for (const owner of ['replacement', 'resident']) for (const truth of [1, 0]) livePath(owner, truth);
function smcPath(owner, truth, same) {
  const ctx = fresh(owner), label = `${owner}-smc-${truth}-${same}`, value = truth, original = same ? value : value ^ 1;
  const code = Buffer.from([0x38, 0xc8, 0xbe, 0x0b, 0xa0, 0, 0, 0x0f, 0x94, 6, 0xb8, original, 0, 0x34, 0x12, 0xeb, 0, 0x0f, 0x0b]);
  assert.deepEqual(code, readFileSync(join(output, `smc-${truth}-${same}.x86`)));
  const codePage = Buffer.alloc(4096, 0xcc); codePage.set(code); const keeperPage = Buffer.alloc(4096, 0xcc); keeperPage.set([0x90, 0xeb, 0, 0x0f, 0x0b]);
  for (const [address, bytes] of [[0xa000, codePage], [0xb000, keeperPage]]) {pure(ctx, () => ctx.api.map(address, 1, 7), 0, 'preload disjoint SMC and keeper pages', false); upload(ctx, address, bytes); ctx.ramPages.set(address, bytes);}
  const keeperCtx = {...ctx, owner: 'resident', units: 0}; const keeper = compile(keeperCtx, [0xb000], [], `${label}-keeper`, []); ctx.units = keeperCtx.units;
  const writer = compile(ctx, [0xa000], [], `${label}-writer`), registers = [...initialRegisters]; registers[0] = 0x1234807f; registers[1] = truth ? 0x23457f7f : 0x23457f80;
  const initial = [...registers], initialFlags = owner === 'replacement' ? 0x8d7 : 0xcd7, flags = (truth ? 0x46 : 0x887) + (owner === 'resident' ? 0x400 : 0), slices = [];
  assert.equal(condition(4, flags), truth); const initialPages = readPages(ctx); seed(ctx, registers, 0xa000, initialFlags);
  function slice(name, budget, retired, pc, stateFlags, cancel = 0, invalidated = false) {
    refresh(ctx).view.setUint32(ctx.base + 96, cancel, true); const expected = arena(ctx); expected.set(stateRecord(registers, pc, stateFlags));
    expected.set(exitRecord(invalidated ? 6 : cancel ? 2 : 1, retired), 56); if (invalidated) expected.set(storeHelper(), 100);
    assert.equal(guestRun(ctx, writer, ctx.base, ctx.base + 56, budget, ctx.base + 96), 0, name); assert.deepEqual(arena(ctx), expected, 'SMC whole arena BEFORE observation');
    if (invalidated) codePage[11] = value; const pages = readPages(ctx); guardCurrent(keeperCtx, keeper, 'unrelated current keeper survives every SMC stage');
    slices.push({name, budget, retired, pc, flags: stateFlags, cancel, registers: [...registers], complete_pages: pages, invalidated});
    if (!retired) controls.push({owner, name: `${label}:${name}`, budget, retired: 0});
  }
  slice('zero_before_CMP', 0, 0, 0xa000, initialFlags); slice('cancel_before_CMP', 1, 0, 0xa000, initialFlags, 1);
  slice('CMP_produces_zero_or_nonzero_condition', 1, 1, 0xa002, flags); registers[6] = 0xa00b;
  slice('MOV_target_once', 1, 1, 0xa007, flags);
  slice('zero_before_code_store', 0, 0, 0xa007, flags); slice('cancel_before_code_store', 1, 0, 0xa007, flags, 1);
  slice('SETcc_commits_one_byte_then_Invalidated', 1, 1, 0xa00a, flags, 0, true);
  const staleGuard = () => owner === 'replacement' ? ctx.api.guard(ctx.low, ctx.high, writer.generation, ctx.base, ctx.base + 56, ctx.base + 96)
    : ctx.api.guard_resident(ctx.low, ctx.high, writer.low, writer.high, ctx.base, ctx.base + 56, ctx.base + 96);
  pure(ctx, staleGuard, 4, `${label}: consumed code stale after committed store`);
  refresh(ctx).view.setUint32(ctx.base + 96, 1, true); pure(ctx, () => guestRun(ctx, writer, 0xffffffff, 0xffffffff, 0, 0xffffffff), 4, `${label}: stale before cancel/zero/malformed pointers`);
  const successor = compile(ctx, [0xa00a], [], `${label}-successor`, []); guardCurrent(keeperCtx, keeper, 'keeper bytes/pointer survive cold successor publication');
  refresh(ctx).view.setUint32(ctx.base + 96, 0, true); const expected = arena(ctx); registers[0] = 0x12340000 + value;
  expected.set(stateRecord(registers, 0xa00f, flags)); expected.set(exitRecord(1, 1, 1), 56);
  assert.equal(guestRun(ctx, successor, ctx.base, ctx.base + 56, 1, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expected, 'purev1 successor consumes modified immediate at genuine currentEIP');
  const successorPages = readPages(ctx); guardCurrent(ctx, successor); guardCurrent(keeperCtx, keeper);
  pure(ctx, () => guestRun(ctx, writer, 0xffffffff, 0xffffffff, 0, 0xffffffff), owner === 'replacement' ? 3 : 4, `${label}: old identity after fresh publication before pointers`);
  refresh(ctx); assert.deepEqual(ctx.bytes.slice(keeper.pointer, keeper.pointer + keeper.length), keeper.bytes); const finalPages = readPages(ctx);
  smcRows.push({owner, truth, same: Boolean(same), original_byte: original, stored_byte: value, target_address: 0xa00b, initial_registers: initial, initial_flags: initialFlags, producer_flags: flags,
    writer: writer.label, keeper: keeper.label, successor: successor.label, initial_pages: initialPages, slices, successor_pages: successorPages, final_pages: finalPages,
    next_pc: 0xa00a, successor_pc: 0xa00f, final_registers: registers, guest_retired: 4,
    helper_status11: 'inferred from strict unchanged Store8 validator and committed Invalidated path; not separately captured', claim: 'same and changed0/1 genuine stores invalidate; one retirement then cold successor without CPU/FLAGS/EIP seed; independent current keeper stays byte/pointer exact'});
  pure(ctx, () => ctx.api.close(), 0, 'close SMC context', false);
  pure(ctx, () => guestRun(ctx, successor, 0xffffffff, 0xffffffff, 0, 0xffffffff), 5, `${label}: Closed before malformed successor pointers`);
}
for (const owner of ['replacement', 'resident']) for (const truth of [1, 0]) for (const same of [1, 0]) smcPath(owner, truth, same);
const program = 'b8ff7f3412b980ff4523bba5a56745baa5a55634bee03f4000'
  + '38c80f923e0f9066010f9946020f9b460372567054'
  + '38cc0f9246040f9046050f9846060f9a460779407b3e'
  + '38c90f94443e080f9505e93f40000f9c460a0f9f460b'
  + '8a460880f8017520b0ffbf1500c0c8893d0030400057ff1550314000'
  + 'c70504304000a5a5a5a50f0bbfdec0adde893d0030400057ff15503140000f0b';
const callerOffsets = [0, 44, 46, 66, 68, 98, 130], relocationOffsets = [21, 78, 107, 114, 120, 137, 144];
const relocationRvas = [0x3fe0, 0x3fe9, 0x3000, 0x3150, 0x3004, 0x3000, 0x3150];
const branchEdges = [[42, 0x72, 130, 44], [44, 0x70, 130, 46], [64, 0x79, 130, 66], [66, 0x7b, 130, 68], [96, 0x75, 130, 98]];
const peFinalBytes = [0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0]; assert.equal(program.length / 2, 150);
for (const [offset, opcode, target, fallthrough] of branchEdges) {const bytes = Buffer.from(program, 'hex'); assert.equal(bytes[offset], opcode); assert.equal(offset + 2, fallthrough); assert.equal(offset + 2 + bytes.readInt8(offset + 1), target); assert.ok(callerOffsets.includes(target) && callerOffsets.includes(fallthrough));}
for (const [offset, hex] of [[27, '0f923e'], [30, '0f906601'], [34, '0f994602'], [38, '0f9b4603'], [48, '0f924604'], [52, '0f904605'], [56, '0f984606'], [60, '0f9a4607'], [70, '0f94443e08'], [75, '0f9505e93f4000'], [82, '0f9c460a'], [86, '0f9f460b'], [90, '8a4608'], [93, '80f801'], [98, 'b0ff']]) {
  assert.equal(Buffer.from(program, 'hex').subarray(offset, offset + hex.length / 2).toString('hex'), hex, 'fresh condition writes and real stored-byte consumer before marker');
}
function literalImage() {
  const bytes = Buffer.alloc(0x1600); bytes.write('MZ'); bytes.writeUInt32LE(0x80, 0x3c); bytes.write('PE\0\0', 0x80);
  for (const [at, value] of [[0x84, 0x14c], [0x86, 3], [0x94, 224], [0x96, 0x102], [0x98, 0x10b], [0xdc, 3], [0xde, 0x100]]) bytes.writeUInt16LE(value, at);
  for (const [at, value] of [[4, 0x200], [8, 0x1200], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, 0x400000], [32, 4096], [36, 512], [56, 0x6000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16], [104, 0x3100], [108, 40], [136, 0x5000], [140, 24], [192, 0x3150], [196, 8]]) bytes.writeUInt32LE(value, 0x98 + at);
  for (const [index, name, size, rva, rawSize, rawPointer, flags] of [[0, '.text', 150, 0x1000, 512, 0x200, 0x60000020], [1, '.data', 4096, 0x3000, 4096, 0x400, 0xc0000040], [2, '.fixups', 24, 0x5000, 512, 0x1400, 0x40000040]]) {
    const at = 0x178 + index * 40; bytes.write(name, at); for (const [offset, value] of [[8, size], [12, rva], [16, rawSize], [20, rawPointer], [36, flags]]) bytes.writeUInt32LE(value, at + offset);
  }
  bytes.fill(0xcc, 0x200, 0x400); bytes.set(Buffer.from(program, 'hex'), 0x200); for (let index = 0; index < 32; index++) bytes[0x13e0 + index] = index + 1;
  bytes.set(words([0x3140, 0, 0, 0x3160, 0x3150]), 0x500); bytes.writeUInt32LE(0x3170, 0x540); bytes.writeUInt32LE(0x3170, 0x550); bytes.write('kernel32.dll\0', 0x560); bytes.write('ExitProcess\0', 0x572);
  bytes.writeUInt32LE(0x1000, 0x1400); bytes.writeUInt32LE(24, 0x1404); relocationOffsets.forEach((offset, index) => bytes.writeUInt16LE(0x3000 + offset, 0x1408 + index * 2)); return bytes;
}
const image = readFileSync(join(output, 'conditions.exe')); assert.deepEqual(image, literalImage(), 'entire5632byte independently reconstructed headers/imports/4096data/relocations'); writeFileSync(join(output, 'program.x86'), Buffer.from(program, 'hex'));
function read32(ctx, address, value) {
  const expected = arena(ctx); expected.set(record('R3MH', 40, [0, value, 0, 0, 0, 0]), 100); assert.equal(ctx.api.read32(address), 0); hostRead32Calls++; assert.deepEqual(arena(ctx), expected, 'normal Read32 strict helper only'); return value;
}
function assertWords(ctx, address, values) {values.forEach((value, index) => read32(ctx, address + index * 4, value)); ramChecks++;}
for (const base of [0x400000, 0x500000]) for (const owner of ['replacement', 'resident']) {
  const label = `pe-${owner}-${base.toString(16)}`, ctx = fresh(owner), entry = base + 0x1000;
  pure(ctx, () => ctx.api.begin_image_input(image.length), 0, 'begin fresh copied fullfile', false);
  for (const [offset, length] of [[0, 4096], [4096, 1536]]) {request(ctx, image.subarray(offset, offset + length)); pure(ctx, () => ctx.api.append_image_input(offset, length), 0, 'bounded fullfile copy', false); refresh(ctx).bytes.fill(0xa5, ctx.base + TRANSFER, ctx.base + SIZE);}
  const beforeLoad = arena(ctx), receipt = record('R3LI', 72, [base, 0x6000, entry, 5, 0x8000, 1, 0, 0, EXIT, EXIT_ID, 0, 0, 0, 0], 2), expectedLoad = beforeLoad.slice(); expectedLoad.set(receipt, TRANSFER);
  assert.equal(ctx.api.load_pe32_linked_v2_input_at(base, 0x8000), 0); assert.deepEqual(arena(ctx), expectedLoad); writeFileSync(join(output, `${label}-receipt.bin`), receipt);
  const header = Buffer.alloc(4096), text = Buffer.alloc(4096), data = Buffer.alloc(4096), fixups = Buffer.alloc(4096), gatePage = Buffer.alloc(4096);
  header.set(image.subarray(0, 512)); text.set(image.subarray(0x200, 0x400)); data.set(image.subarray(0x400, 0x1400)); fixups.set(image.subarray(0x1400, 0x1600)); gatePage.set([0x0f, 0x0b], 0x20);
  relocationOffsets.forEach((offset, index) => text.writeUInt32LE(base + relocationRvas[index], offset)); data.writeUInt32LE(EXIT, 0x150);
  for (const [address, bytes] of [[base, header], [entry, text], [base + 0x3000, data], [base + 0x5000, fixups], [0x8000, gatePage]]) ctx.ramPages.set(address, bytes);
  const initialPages = readPages(ctx); read32(ctx, base + 0xb4, 0x400000); read32(ctx, base + 0x3150, EXIT); read32(ctx, base + 0x3154, 0); relocationOffsets.forEach((offset, index) => read32(ctx, entry + offset, base + relocationRvas[index]));
  assertWords(ctx, base + 0x3000, Array(8).fill(0)); for (let index = 0; index < 32; index++) read8(ctx, base + 0x3fe0 + index, index + 1);
  read8(ctx, base + 0x4000, 0, 1); controls.push({owner, name: `${label}: page following destination tail unmapped`, address: base + 0x4000, reason: 1, length: 1});
  const entries = callerOffsets.map(offset => entry + offset), gates = [[EXIT, EXIT_ID]]; if (owner === 'replacement') entries.push(EXIT);
  const caller = compile(ctx, entries, owner === 'replacement' ? gates : [], `${label}-caller`, ['read32', owner === 'replacement' ? 'store32' : 'store_resident32', 'read8', storeName(ctx)]);
  const gate = owner === 'replacement' ? caller : compile(ctx, [EXIT], gates, `${label}-gate`, []);
  const expectedStart = arena(ctx); expectedStart.set(stateRecord([0, 0, 0, 0, 0x71000, 0, 0, 0], entry, 2)); expectedStart.set(exitRecord(3, 0, 3), 56);
  assert.equal(ctx.api.start_loaded_image(0x70000, 1), 0); assert.deepEqual(arena(ctx), expectedStart); ctx.ramPages.set(0x70000, Buffer.alloc(4096)); const startupPages = readPages(ctx);
  const finalRegisters = [0x12347fff, 0x2345ff80, 0x3456a5a5, 0x4567a5a5, 0x70ff8, 0, base + 0x3fe0, MARKER];
  const expectedRun = arena(ctx); expectedRun.set(stateRecord(finalRegisters, EXIT, 0x46)); expectedRun.set(exitRecord(owner === 'resident' ? 3 : 8, 32, owner === 'resident' ? 2 : 3, owner === 'resident' ? 0 : EXIT_ID), 56); expectedRun.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100);
  assert.equal(guestRun(ctx, caller, ctx.base, ctx.base + 56, 64, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedRun, '32 genuine instructions with memory0/1 flags consumers and real byte-load before named CALL');
  data.set(peFinalBytes, 0xfe0); data.writeUInt32LE(MARKER, 0); const stack = ctx.ramPages.get(0x70000); stack.writeUInt32LE(entry + 118, 0xff8); stack.writeUInt32LE(MARKER, 0xffc); const executedPages = readPages(ctx);
  if (owner === 'resident') {const expectedGate = arena(ctx); expectedGate.set(exitRecord(8, 0, 3, EXIT_ID), 56); assert.equal(guestRun(ctx, gate, ctx.base, ctx.base + 56, 1, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedGate);}
  const frame = record('R3CF', 112, [1, EXIT_ID, 2, 1, EXIT, 0x70ff8, entry + 118, 0, MARKER, ...Array(15).fill(0)]), expectedCapture = arena(ctx); expectedCapture.set(frame, TRANSFER);
  assert.equal(owner === 'replacement' ? ctx.api.capture_call(ctx.low, ctx.high, gate.generation, 2, 1) : ctx.api.capture_resident_call(ctx.low, ctx.high, gate.low, gate.high, 2, 1), 0); assert.deepEqual(arena(ctx), expectedCapture);
  const expectedComplete = arena(ctx); expectedComplete.set(exitRecord(9, 0, 4, MARKER), 56);
  assert.equal(owner === 'replacement' ? ctx.api.complete_windows_call(ctx.low, ctx.high, gate.generation, 1) : ctx.api.complete_resident_windows_call(ctx.low, ctx.high, gate.low, gate.high, 1), 0); assert.deepEqual(arena(ctx), expectedComplete);
  const terminalPages = readPages(ctx); assertWords(ctx, base + 0x3000, [MARKER, 0, 0, 0, 0, 0, 0, 0]); assertWords(ctx, 0x70fe0, [0, 0, 0, 0, 0, 0, entry + 118, MARKER]);
  for (let index = 0; index < 32; index++) read8(ctx, base + 0x3fe0 + index, index < 12 ? peFinalBytes[index] : index + 1);
  refresh(ctx); for (const unit of new Set([caller, gate])) assert.deepEqual(ctx.bytes.slice(unit.pointer, unit.pointer + unit.length), unit.bytes);
  pure(ctx, () => guestRun(ctx, caller, 0xffffffff, 0xffffffff, 0, 0xffffffff), 21, `${label}: terminal latch before malformed pointers/zero`);
  peRows.push({owner, base, guest_retired: 32, result: MARKER, final_registers: finalRegisters, final_flags: 0x46, canary: 0, operand_address: base + 0x3fe0,
    operand_initial_hex: Buffer.from(Array.from({length: 32}, (_, index) => index + 1)).toString('hex'), operand_final_hex: Buffer.from([...peFinalBytes, ...Array.from({length: 20}, (_, index) => index + 13)]).toString('hex'),
    receipt_hex: Buffer.from(receipt).toString('hex'), receipt_sha256: hash(receipt), frame_hex: Buffer.from(frame).toString('hex'), stack_words: [0, 0, 0, 0, 0, 0, entry + 118, MARKER], initial_pages: initialPages, startup_pages: startupPages, executed_pages: executedPages, terminal_pages: terminalPages});
  pure(ctx, () => ctx.api.close(), 0, 'close PE context', false);
}
const counts = {
  instances: ordinal, canonical_cases: rows.filter(row => row.group === 'canonical').length, ignored_cases: rows.filter(row => row.group === 'ignored').length, ea_cases: rows.filter(row => row.group === 'ea').length,
  finite_guest_retired: rows.length, modules: modules.length, controls: controls.length,
  live_paths: liveRows.length, live_runs: liveRows.reduce((sum, row) => sum + row.slices.length, 0), live_faults: liveRows.reduce((sum, row) => sum + row.slices.filter(slice => slice.fault).length, 0), live_guest_retired: liveRows.reduce((sum, row) => sum + row.guest_retired, 0),
  semantic_repair_phases: liveRows.reduce((sum, row) => sum + row.repairs.length, 0), repair_api_calls: liveRows.reduce((sum, row) => sum + row.repairs.reduce((subtotal, phase) => subtotal + phase.actions.length, 0), 0), observation_permission_inputs: liveRows.reduce((sum, row) => sum + row.observation_inputs.length, 0),
  consumer_permission_inputs: liveRows.length, finite_permission_inputs: rows.length * 2, smc_paths: smcRows.length, smc_runs: smcRows.length * 11, smc_guest_retired: smcRows.reduce((sum, row) => sum + row.guest_retired, 0), smc_idempotent_paths: smcRows.filter(row => row.same).length,
  pe_paths: peRows.length, pe_guest_retired: peRows.reduce((sum, row) => sum + row.guest_retired, 0), total_guest_retired: rows.length + liveRows.reduce((sum, row) => sum + row.guest_retired, 0) + smcRows.reduce((sum, row) => sum + row.guest_retired, 0) + peRows.reduce((sum, row) => sum + row.guest_retired, 0),
  llvm_encoding_symbols: llvm.length, runtime_sources: sourcePaths.length, generated_function_invocations: generatedRuns.length, maximum_resident_units: residentPeak,
  complete_page_observations: pageChecks, host_read32_observations: hostRead32Calls, host_read8_observations: hostRead8Calls, neighbor_or_stack_range_checks: ramChecks,
};
const expectedCounts = {instances: 18, canonical_cases: 4096, ignored_cases: 512, ea_cases: 512, finite_guest_retired: 5120, modules: 48, controls: 170,
  live_paths: 4, live_runs: 80, live_faults: 8, live_guest_retired: 24, semantic_repair_phases: 8, repair_api_calls: 24, observation_permission_inputs: 40, consumer_permission_inputs: 4, finite_permission_inputs: 10240,
  smc_paths: 8, smc_runs: 88, smc_guest_retired: 32, smc_idempotent_paths: 4, pe_paths: 4, pe_guest_retired: 128, total_guest_retired: 5304, llvm_encoding_symbols: 2, runtime_sources: 65,
  generated_function_invocations: 5334, maximum_resident_units: 7, complete_page_observations: 31198, host_read32_observations: 31946888, host_read8_observations: 328326, neighbor_or_stack_range_checks: 10264};
assert.deepEqual(counts, expectedCounts);
for (const owner of ['replacement', 'resident']) {
  const owned = rows.filter(row => row.owner === owner); assert.equal(owned.length, 2560);
  for (let index = 0; index < 16; index++) {
    assert.equal(owned.filter(row => row.group === 'canonical' && row.condition === index).length, 128);
    assert.equal(owned.filter(row => row.group === 'ignored' && row.condition === index).length, 16);
    assert.equal(owned.filter(row => row.group === 'ea' && row.condition === index).length, 16);
    for (let relevant = 0; relevant < 32; relevant++) {
      const subset = owned.filter(row => row.group === 'canonical' && row.condition === index && irrelevantSeeds.some(irrelevant => row.initial_flags === canonicalFlags(relevant, irrelevant)));
      assert.equal(subset.length, 4); assert.equal(new Set(subset.map(row => row.value)).size, 1);
      for (const irrelevant of irrelevantSeeds) assert.equal(subset.filter(row => row.initial_flags === canonicalFlags(relevant, irrelevant)).length, 1);
    }
  }
}
const repairCensus = Object.fromEntries(['map', 'upload', 'protect', 'write8'].map(api => [api, liveRows.flatMap(row => row.repairs.flatMap(phase => phase.actions)).filter(action => action.api === api).length]));
assert.deepEqual(repairCensus, {map: 4, upload: 4, protect: 12, write8: 4});
assert.equal(modules.filter(row => row.label.startsWith('pe-')).length, 6); assert.equal(modules.filter(row => row.label.includes('-live-')).length, 4);
assert.equal(modules.filter(row => row.label.includes('-smc-')).length, 24); assert.equal(modules.filter(row => row.imports.some(item => ['store8', 'store_resident8'].includes(item.name))).length, 30);
const statuses = Object.fromEntries([0, 1, 3, 4, 5, 21].map(status => [status, generatedRuns.filter(row => row.status === status).length]));
const budgets = Object.fromEntries([0, 1, 64].map(budget => [budget, generatedRuns.filter(row => row.budget === budget).length]));
assert.deepEqual(statuses, {0: 5298, 1: 3, 3: 5, 4: 14, 5: 10, 21: 4}); assert.deepEqual(budgets, {0: 88, 1: 5242, 64: 4});
assert.deepEqual(Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), sourceHashes, 'FIRST current sources unchanged through actual'); assert.equal(hash(readFileSync(enginePath)), hash(engineBytes), 'FIRST current engine unchanged');
const result = {
  status: 'ok', counts, repair_api_census: repairCensus, generated_status_census: statuses, generated_budget_census: budgets,
  engine_sha256: hash(engineBytes), engine_exports: WebAssembly.Module.exports(engineModule), engine_function_signatures: engineFunctionSignatures, pe_sha256: hash(image), source_sha256: sourceHashes,
  tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0], llvm: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).split('\n')[0]},
  assembly: {command: ['clang', ...assemble], disassembly_command: ['xcrun', ...disassemble], object_sha256: hash(object), disassembly_sha256: hash(Buffer.from(disassembly)), symbols: llvm, scope: 'encoding only; two samples7bytes collected once within FIRST actual BEFORE first generated guest calls'},
  oracle: {condition_anchors: anchors, irrelevant_seeds: irrelevantSeeds, relevant_flag_weights: [1, 4, 64, 128, 2048], canonical_flag_values: 32, canonical_forms: 16, ignored_forms: 128, ea_forms: 128,
    batches: batches.map(row => ({index: row.index, pc: row.pc, group: row.group, forms: row.forms.length, instructions: row.forms.length + 1, hex: row.code.toString('hex')})), ea_shapes: shapeDefinitions,
    program, caller_offsets: callerOffsets, branches: branchEdges, relocation_offsets: relocationOffsets, relocation_rvas: relocationRvas, pe_file_bytes: 5632, pe_data_raw_bytes: 4096, pe_chunks: [[0, 4096], [4096, 1536]], pe_return_offset: 118,
    pe_initial_bytes: Array.from({length: 32}, (_, index) => index + 1), pe_final_bytes: [...peFinalBytes, ...Array.from({length: 20}, (_, index) => index + 13)], pe_guest_instruction_count: 32, pe_final_flags: 0x46,
    pe_final_registers: {eax: 0x12347fff, ecx: 0x2345ff80, edx: 0x3456a5a5, ebx: 0x4567a5a5, esp: 0x70ff8, ebp: 0, esi: 'base+3fe0', edi: MARKER},
    live_instruction_count: 6, live_compile_bytes: 17, smc_writer_compile_bytes: 17, smc_successor_instruction_count: 2,
    condition_policy: 'independent explicit16 boolean predicates over numeric CF/PF/ZF/SF/OF; canonical4 independent AF/DF combinations; normalized0/1 always strictStore8; fullFLAGS/GPR preserved without normalization; false equal0 still writes',
    host_observation_scope: 'whole4236arena asserted before normal public observations; complete declared4096pages liveasserted and expectedhashes saved; Write-only data temporarily becomes RW solely for counted observations; no saved raw perrow CPU/RAM claim'},
  generated_runs: generatedRuns, cases: rows, controls, live: liveRows, smc: smcRows, pe: peRows, modules,
  claim: 'finite authored memorySETcc16conditions/4096canonical+512ignored+512EA executions through actual boundreplacement/resident children; completeFLAGS/GPR/EIP/helper/arena and declaredwholepages assertions; genuineCMP/MOVonce4continuations8Writefaults with falseequal0 and operand-only repair/noCPUFLAGS EIPpatch/recompile/prefixreplay;8realchanged+idempotent0/1SMC paths onecommit thenInvalidated/coldsuccessor/currentkeeper;4freshcopiedPE paths memory0/1/realMOV8load/directconditions/namedCALLExitProcess;live-backed expected outcomes not raw perrow snapshots; no generalFLAGS/ISA/EA/OS/concurrency/SDK/browser/performance/fullP2/playablegame claim',
};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2)); console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
