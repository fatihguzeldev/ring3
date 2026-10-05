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
assert.equal(WebAssembly.Module.exports(engineModule).length, 70);
assert.equal(WebAssembly.Module.exports(engineModule).filter(row => row.kind === 'function').length, 69);
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
  const expectedArities = {'ring3_abi_v1_open': 3, 'ring3_abi_v1_close': 0, 'ring3_abi_v1_arena_ptr': 0, 'ring3_abi_v1_map': 3, 'ring3_abi_v1_protect': 3, 'ring3_abi_v1_unmap': 2, 'ring3_abi_v1_upload': 2, 'ring3_abi_v1_load_pe32': 1, 'ring3_abi_v1_load_pe32_at': 2, 'ring3_abi_v1_load_pe32_linked_at': 3, 'ring3_abi_v1_load_pe32_linked_v2_at': 3, 'ring3_abi_v1_begin_image_input': 1, 'ring3_abi_v1_append_image_input': 2, 'ring3_abi_v1_abort_image_input': 0, 'ring3_abi_v1_load_pe32_linked_v2_input_at': 2, 'ring3_abi_v1_start_loaded_image': 2, 'ring3_abi_v1_compile': 1, 'ring3_abi_v1_generation': 0, 'ring3_abi_v1_compile_with_gates': 2, 'ring3_abi_v1_compile_entries': 2, 'ring3_abi_v1_compile_resident': 1, 'ring3_abi_v1_compile_resident_with_gates': 2, 'ring3_abi_v1_compile_resident_entries': 2, 'ring3_abi_v1_find_resident': 1, 'ring3_abi_v1_compile_resident_callback_unit': 7, 'ring3_abi_v1_acknowledge_resident_callback_installation': 8, 'ring3_abi_v1_dispatcher_module': 2, 'ring3_abi_v1_resident_module': 2, 'ring3_abi_v1_retire_stale_resident': 4, 'ring3_abi_v1_acknowledge_resident_installation': 5, 'ring3_abi_v1_find_installed_resident': 3, 'ring3_abi_v1_guard_dispatch_entry': 5, 'ring3_abi_v1_guard_resident': 7, 'ring3_abi_v1_module_ptr': 0, 'ring3_abi_v1_module_len': 0, 'ring3_abi_v1_guard': 6, 'ring3_abi_v1_write8': 2, 'ring3_abi_v1_store8': 2, 'ring3_abi_v1_store_resident8': 6, 'ring3_abi_v1_read8': 1, 'ring3_abi_v1_read16': 1, 'ring3_abi_v1_read32': 1, 'ring3_abi_v1_write32': 2, 'ring3_abi_v1_write_words32': 1, 'ring3_abi_v1_store32': 2, 'ring3_abi_v1_store_resident32': 6, 'ring3_abi_v1_capture_call': 5, 'ring3_abi_v1_complete_call': 5, 'ring3_abi_v1_capture_resident_call': 6, 'ring3_abi_v1_complete_resident_call': 6, 'ring3_abi_v1_abandon_call': 3, 'ring3_abi_v1_complete_windows_call': 4, 'ring3_abi_v1_complete_resident_windows_call': 5, 'ring3_abi_v1_select_resident_callback_unit': 7, 'ring3_abi_v1_capture_active_resident_callback_call': 7, 'ring3_abi_v1_complete_active_resident_callback_call': 7, 'ring3_abi_v1_complete_active_resident_callback_windows_call': 6, 'ring3_abi_v1_capture_resident_callback_call': 7, 'ring3_abi_v1_complete_resident_callback_call': 7, 'ring3_abi_v1_begin_callback': 8, 'ring3_abi_v1_finish_callback': 4, 'ring3_abi_v1_begin_resident_callback': 11, 'ring3_abi_v1_finish_resident_callback': 5, 'ring3_abi_v1_authorize_resident_callback': 5, 'ring3_abi_v1_abort_callback': 3, 'ring3_abi_v1_resume_callback_code': 6, 'ring3_abi_v1_resume_callback_entries': 6, 'ring3_abi_v1_load_pe32_linked_v3_input_at': 2, 'ring3_abi_v1_load_pe32_linked_v4_input_at': 2};
  assert.equal(exports.length, 70); assert.deepEqual(exports.filter(row => row.kind !== 0), [{name: 'memory', kind: 2, index: 0}]);
  const signatures = {};
  for (const item of exports.filter(row => row.kind === 0)) {assert.ok(Object.hasOwn(expectedArities, item.name)); assert.ok(!Object.hasOwn(signatures, item.name)); const expected = {parameters: Array(expectedArities[item.name]).fill(0x7f), results: [0x7f]}; assert.deepEqual(types[functions[item.index]], expected, item.name); signatures[item.name] = expected;}
  assert.equal(Object.keys(signatures).length, 69); return signatures;
}
const engineFunctionSignatures = engineSignatures(engineBytes);
const SIZE = 4236, TRANSFER = 140, EXIT = 0x8020, EXIT_ID = 0x10003, MARKER = 0xc8c00014;
const boundaries = [0, 1, 15, 16, 127, 128, 255], flagSeeds = [0x8d6, 0x8d7, 0xcd6, 0xcd7];
const initialRegisters = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, unmap: 2, upload: 2, read8: 1, read32: 1, write8: 2, compile_entries: 2, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store32: 2, store_resident32: 6, begin_image_input: 1, append_image_input: 2, load_pe32_linked_v2_input_at: 2, start_loaded_image: 2, capture_call: 5, capture_resident_call: 6, complete_windows_call: 4, complete_resident_windows_call: 5};
const sourcePaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/control.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/cpu/dbt/wasm/memory/byte_store.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/locals.rs', 'engine/src/process/startup.rs', 'engine/src/abi/x86/exit.rs', 'engine/src/process/instance.rs', 'engine/src/process/wasm.rs', 'engine/src/process/call.rs', 'engine/src/process/windows.rs', 'engine/src/abi/wasm/exports.rs', 'engine/src/abi/memory_helper.rs', 'engine/src/loader/mod.rs', 'engine/src/loader/imports.rs', 'engine/src/loader/relocation.rs', 'engine/src/memory/space.rs', 'engine/src/loader/pe32.rs', 'engine/src/process/image_input.rs', 'engine/src/process/image.rs', 'engine/src/cpu/dbt/resident.rs', 'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/gate.rs', 'engine/src/memory/address.rs', 'engine/src/memory/code.rs', 'engine/src/abi/x86/state.rs', 'engine/src/abi/call_frame.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/wasm/mod.rs', 'engine/tests/support/pe32.rs', 'engine/tests/cpu_register_memory_byte_compare_wasm.rs', 'engine/tests/fixtures/p2-register-memory-byte-compare/run.mjs', 'engine/tests/fixtures/p2-register-memory-byte-compare/integer.S'];
sourcePaths.push('engine/src/lib.rs', 'engine/src/process/mod.rs', 'engine/src/process/resident.rs', 'engine/src/windows/mod.rs', 'engine/src/windows/provider.rs', 'engine/src/windows/calling_convention.rs', 'engine/src/abi/arena.rs', 'engine/src/abi/wasm/mod.rs', 'engine/src/cpu/x86/decode/mod.rs', 'engine/src/cpu/dbt/mod.rs', 'engine/src/memory/mod.rs', 'engine/src/abi/mod.rs', 'engine/src/abi/header.rs', 'engine/src/abi/x86/mod.rs', 'engine/src/cpu/mod.rs', 'engine/src/cpu/x86/mod.rs', 'engine/src/process/installation.rs', 'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/exit.rs');
assert.equal(sourcePaths.length, 65); assert.equal(new Set(sourcePaths).size, 65);
const sourceHashes = Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
writeFileSync(join(output, 'source-hashes.json'), JSON.stringify({engine_sha256: hash(engineBytes), sources: sourceHashes}, null, 2));
writeFileSync(join(output, 'engine.wasm'), engineBytes);
const rows = [], controls = [], liveRows = [], peRows = [], modules = [], generatedRuns = [];
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
  const instance = new WebAssembly.Instance(engineModule, {}), key = 0xc83f000000000000n + BigInt(++ordinal);
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
function compile(ctx, entries, gates, label, extraHelpers = ['read8']) {
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
function compare(left, right, oldFlags) {
  assert.ok(left >= 0 && left <= 255 && right >= 0 && right <= 255);
  const difference = left - right, result = (difference + 256) % 256;
  const signedLeft = left >= 128 ? left - 256 : left, signedRight = right >= 128 ? right - 256 : right;
  const signedDifference = signedLeft - signedRight;
  let ones = 0; for (let value = result; value > 0; value = Math.floor(value / 2)) ones += value % 2;
  const flags = Math.floor(oldFlags / 0x400) % 2 * 0x400 + 2 + (left < right ? 1 : 0) + (ones % 2 === 0 ? 4 : 0)
    + (left % 16 < right % 16 ? 16 : 0) + (result === 0 ? 64 : 0) + (result >= 128 ? 128 : 0)
    + (signedDifference < -128 || signedDifference > 127 ? 2048 : 0);
  return {result, flags};
}
for (const [left, right, result, flags] of [[127, 128, 255, 0x887], [128, 127, 1, 0x812], [0, 0, 0, 0x46], [0, 1, 255, 0x97], [128, 128, 0, 0x46], [255, 0, 255, 0x86], [127, 255, 128, 0x883]]) {
  assert.deepEqual(compare(left, right, 2), {result, flags}); assert.deepEqual(compare(left, right, 3), {result, flags});
}
function byte(registers, left) {return Math.floor(registers[left % 4] / (left < 4 ? 1 : 256)) % 256;}
function inserted(registers, target, value) {
  const values = [...registers], parent = target % 4, scale = target < 4 ? 1 : 256;
  values[parent] = registers[parent] - byte(registers, target) * scale + value * scale; return values;
}
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
function runCompare(ctx, unit, form, registers, input, flags, name, group) {
  const range = Math.floor(form.address / 32) * 32, neighbors = Buffer.from(Array.from({length: 32}, (_, index) => index + 1)); neighbors[form.address - range] = input;
  upload(ctx, range, neighbors); ctx.ramPages.get(Math.floor(range / 4096) * 4096).set(neighbors, range % 4096); guardCurrent(ctx, unit);
  const beforePages = readPages(ctx), beforeHex = readDeclared(ctx, range), lhs = byte(registers, form.destination), outcome = compare(lhs, input, flags), expectedRegisters = [...registers];
  seed(ctx, registers, form.pc, flags); const expected = arena(ctx); expected.set(stateRecord(expectedRegisters, form.pc + form.bytes.length, outcome.flags)); expected.set(exitRecord(1, 1), 56); expected.set(helper(input), 100);
  assert.equal(guestRun(ctx, unit, ctx.base, ctx.base + 56, 1, ctx.base + 96), 0, name); assert.deepEqual(arena(ctx), expected, `${name}: whole arena BEFORE normal observation helper effects`);
  const afterPages = readPages(ctx), afterHex = readDeclared(ctx, range); guardCurrent(ctx, unit);
  assert.equal(afterHex, beforeHex, 'compare never writes its source or31 nonzero neighbors'); assert.deepEqual(afterPages, beforePages, 'all declared source/code pages remain byte-exact');
  rows.push({owner: ctx.owner, name, group, kind: form.kind, destination: form.destination, encoding: Buffer.from(form.bytes).toString('hex'), pc: form.pc, address: form.address, initial_registers: registers, initial_destination: lhs, input, initial_flags: flags, result: outcome.result, flags: outcome.flags, expected_registers: expectedRegisters, retired: 1, next_pc: form.pc + form.bytes.length, declared_ram: {address: range, bytes: 32, before_hex: beforeHex, expected_hex: afterHex, expected_sha256: hash(Buffer.from(afterHex, 'hex'))}, complete_pages_before: beforePages, complete_pages_after: afterPages});
}
function noRetirement(ctx, unit, pc, cancel) {
  seed(ctx, initialRegisters, pc, 0xcd7, cancel); const expected = arena(ctx); expected.set(exitRecord(cancel ? 2 : 1, 0), 56);
  assert.equal(guestRun(ctx, unit, ctx.base, ctx.base + 56, cancel ? 1 : 0, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expected, 'zero/cancel precedes real Read8 and flags publication'); readPages(ctx);
  controls.push({owner: ctx.owner, name: `${unit.label}:${cancel ? 'cancel_before_read' : 'zero_before_read'}`, budget: cancel ? 1 : 0, retired: 0});
}
const canonical = []; let canonicalPc = 0x8000;
for (let destination = 0; destination < 8; destination++) {
  const bytes = [0x3a, destination * 8 + 6]; canonical.push({kind: 'cmp', destination, bytes, pc: canonicalPc, address: 0x3fff}); canonicalPc += bytes.length;
}
const canonicalCode = Buffer.from([...canonical.flatMap(form => form.bytes), 0xeb, 0, 0x0f, 0x0b]); assert.equal(canonical.length, 8); assert.equal(canonicalCode.length, 20); assert.deepEqual(canonicalCode, readFileSync(join(output, 'canonical.x86')));
const aliasShapes = [
  {destination: 0, values: {0: 0x3001}, terms: [[0, 1]], displacement: 0, bytes: [0x00], address: 0x3001},
  {destination: 4, values: {0: 0x7fffffff}, terms: [[0, 2]], displacement: 1, bytes: [0x04, 0x45, 1, 0, 0, 0], address: 0xffffffff},
  {destination: 1, values: {1: 0x3042}, terms: [[1, 1]], displacement: -16, bytes: [0x41, 0xf0], address: 0x3032},
  {destination: 5, values: {1: 0x80001800}, terms: [[1, 2]], displacement: 1, bytes: [0x04, 0x4d, 1, 0, 0, 0], address: 0x3001},
  {destination: 2, values: {2: 0xfffffff0, 4: 0x3010}, terms: [[4, 1], [2, 2]], displacement: 17, bytes: [0x44, 0x54, 0x11], address: 0x3001},
  {destination: 6, values: {2: 0xffffffff, 5: 0x3004}, terms: [[5, 1], [2, 4]], displacement: 0, bytes: [0x44, 0x95, 0], address: 0x3000},
  {destination: 3, values: {3: 0x3066, 6: 2}, terms: [[3, 1], [6, 8]], displacement: -32, bytes: [0x84, 0xf3, 0xe0, 0xff, 0xff, 0xff], address: 0x3056},
  {destination: 7, values: {3: 0x3eff}, terms: [[3, 1]], displacement: 256, bytes: [0x83, 0, 1, 0, 0], address: 0x3fff},
];
assert.deepEqual([...aliasShapes.map(row => row.destination)].sort(), [0, 1, 2, 3, 4, 5, 6, 7]);
const aliases = []; let aliasPc = 0x8200;
for (const shape of aliasShapes) {
  assert.equal(shape.bytes[0] & 0x38, 0); assert.ok(shape.terms.some(([index]) => index === shape.destination % 4));
  const registers = [...initialRegisters]; for (const [index, value] of Object.entries(shape.values)) registers[Number(index)] = value;
  const sum = shape.terms.reduce((value, [index, scale]) => value + registers[index] * scale, shape.displacement); assert.equal((sum % 0x100000000 + 0x100000000) % 0x100000000, shape.address);
  const bytes = [0x3a, shape.bytes[0] + shape.destination * 8, ...shape.bytes.slice(1)]; aliases.push({...shape, kind: 'cmp', bytes, pc: aliasPc}); aliasPc += bytes.length;
}
const aliasCode = Buffer.from([...aliases.flatMap(form => form.bytes), 0xeb, 0, 0x0f, 0x0b]); assert.equal(aliases.length, 8); assert.equal(aliasCode.length, 44); assert.deepEqual(aliasCode, readFileSync(join(output, 'aliases.x86')));
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
const assemblyPath = join(root, 'engine/tests/fixtures/p2-register-memory-byte-compare/integer.S'), objectPath = join(output, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assemblyPath, '-o', objectPath];
const disassemble = ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', disassemble, {encoding: 'utf8'});
writeFileSync(join(output, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), symbols = elfFixtures(object), llvm = [];
const encodingSamples = [['cmp_al_memory', '3a06']];
let llvmOffset = 0;
for (const [name, hex] of encodingSamples) {
  const symbol = symbols.get(name); assert.ok(symbol, name); assert.equal(symbol.offset, llvmOffset); assert.equal(symbol.bytes.toString('hex'), hex);
  llvm.push({name, offset: llvmOffset, hex}); llvmOffset += hex.length / 2;
  const start = disassembly.indexOf(`<${name}>:`); assert.ok(start >= 0);
  const next = disassembly.indexOf('\n\n', start), body = disassembly.slice(start, next < 0 ? undefined : next);
  assert.match(body, new RegExp(`\\b${name.split('_')[0]}\\s+`), `${name}: LLVM decodes the canonical family`);
}
assert.equal(symbols.size, 1); assert.equal(llvmOffset, 2);
for (const owner of ['replacement', 'resident']) {
  const ctx = fresh(owner), codePage = Buffer.alloc(4096, 0xcc); codePage.set(canonicalCode); codePage.set(aliasCode, 0x200);
  for (const address of [0x8000, 0x3000, 0xfffff000]) {
    pure(ctx, () => ctx.api.map(address, 1, address === 0x8000 ? 7 : 3), 0, 'map declared authored page', false);
    const data = address === 0x8000 ? codePage : Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1)); upload(ctx, address, data); ctx.ramPages.set(address, data);
  }
  const unit = compile(ctx, [0x8000], [], `${owner}-canonical`);
  for (const form of canonical) for (const flags of flagSeeds) {
    const registers = inserted(initialRegisters, form.destination, 127); registers[6] = form.address;
    runCompare(ctx, unit, form, registers, 128, flags, `${owner}-canonical-${form.kind}-${form.destination}-${flags}`, 'canonical');
  }
  const boundaryForm = canonical.find(row => row.destination === 0);
  for (const left of boundaries) for (const right of boundaries) {
    const registers = inserted(initialRegisters, 0, left); registers[6] = boundaryForm.address;
    runCompare(ctx, unit, boundaryForm, registers, right, 0xcd7, `${owner}-boundary-${left}-${right}`, 'boundary');
  }
  noRetirement(ctx, unit, 0x8000, 0); noRetirement(ctx, unit, 0x8000, 1);
  const alias = compile(ctx, [0x8200], [], `${owner}-aliases`);
  for (const form of aliases) for (const input of [127, 128]) for (const flags of [0x8d6, 0xcd7]) {
    const registers = [...initialRegisters]; for (const [index, value] of Object.entries(form.values)) registers[Number(index)] = value;
    runCompare(ctx, alias, form, registers, input, flags, `${owner}-alias-${form.destination}-${input}-${flags}`, 'alias');
  }
  noRetirement(ctx, alias, 0x8200, 0); noRetirement(ctx, alias, 0x8200, 1);
  pure(ctx, () => guestRun(ctx, unit, 0xffffffff, 0xffffffff, 0, 0xffffffff), owner === 'replacement' ? 3 : 1, `${owner}: earlier canonical identity before malformed pointer`); readPages(ctx);
  read8(ctx, 0x4000, 0, 1); controls.push({owner, name: 'page following last-byte source stays unmapped', address: 0x4000, reason: 1, length: 1});
  const guard = (low, high, identityLow, identityHigh, pointer = ctx.base) => owner === 'replacement' ? ctx.api.guard(low, high, identityLow, pointer, ctx.base + 56, ctx.base + 96) : ctx.api.guard_resident(low, high, identityLow, identityHigh, pointer, ctx.base + 56, ctx.base + 96), identity = owner === 'replacement' ? alias.generation : alias.low;
  pure(ctx, () => guard(ctx.low + 1, ctx.high, identity, alias.high), 3, `${owner}: wrong key low`); pure(ctx, () => guard(ctx.low, ctx.high + 1, identity, alias.high), 3, `${owner}: wrong key high`);
  pure(ctx, () => guard(ctx.low, ctx.high, identity + 1, alias.high), 3, `${owner}: wrong generation or id low`); pure(ctx, () => guard(ctx.low, ctx.high, owner === 'replacement' ? 0 : identity, owner === 'replacement' ? undefined : alias.high + 1), 3, `${owner}: zero generation or wrong id high`);
  pure(ctx, () => guard(ctx.low, ctx.high, identity, alias.high, 0xffffffff), 1, `${owner}: current malformed guard pointer`); pure(ctx, () => guestRun(ctx, alias, 0xffffffff, 0xffffffff, 1, 0xffffffff), 1, `${owner}: current generated malformed pointer`); readPages(ctx);
  hostByte(ctx, 0x8200, aliasCode[0]); refresh(ctx).view.setUint32(ctx.base + 96, 1, true);
  pure(ctx, () => guard(ctx.low, ctx.high, identity, alias.high), 4, `${owner}: external same-byte code input makes alias stale`);
  pure(ctx, () => guestRun(ctx, alias, 0xffffffff, 0xffffffff, 0, 0xffffffff), 4, `${owner}: stale before cancel/zero/malformed pointers`); readPages(ctx);
  pure(ctx, () => ctx.api.close(), 0, 'close micro context', false); pure(ctx, () => guestRun(ctx, alias, 0xffffffff, 0xffffffff, 0, 0xffffffff), 5, `${owner}: Closed before stale/malformed pointers`);
}
function livePath(owner, leftIndex, changedInput) {
  const ctx = fresh(owner), label = `${owner}-live-${leftIndex === 0 ? 'low' : 'high'}`, address = 0x400f;
  const code = Buffer.from([0x80, 0xc1, 0x80, 0xbe, 0x0f, 0x40, 0, 0, 0x3a, leftIndex * 8 + 6, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0, 0x0f, 0x0b]);
  assert.deepEqual(code, readFileSync(join(output, `live-${leftIndex === 0 ? 'low' : 'high'}.x86`))); const pageCode = Buffer.alloc(4096, 0xcc); pageCode.set(code);
  pure(ctx, () => ctx.api.map(0x9000, 1, 7), 0, 'map live code before publication', false); upload(ctx, 0x9000, pageCode); ctx.ramPages.set(0x9000, pageCode);
  const unit = compile(ctx, [0x9000], [], label), registers = [...initialRegisters]; registers[3] = 0x4567ffff;
  const initial = [...registers], initialFlags = owner === 'replacement' ? 0x8d7 : 0xcd7, producerFlags = owner === 'replacement' ? 0x847 : 0xc47;
  const firstInput = 0, lhs = byte(registers, leftIndex), outcome = compare(lhs, changedInput, producerFlags), hypothetical = compare(lhs, firstInput, producerFlags), slices = [], repairs = [], observationInputs = [];
  assert.equal(initial[1] % 256, 128, 'real ADD CL80 starts from CL80');
  assert.notEqual(hypothetical.flags, producerFlags, 'premature CMP FLAGS publication differs from the real producer');
  assert.notEqual(outcome.flags, producerFlags, 'successful retry publishes fresh compare flags'); assert.notDeepEqual(outcome, hypothetical, 'data-only retry observes changed memory');
  assert.deepEqual(outcome, {result: leftIndex === 0 ? 255 : 1, flags: (leftIndex === 0 ? 0x887 : 0x812) + (owner === 'resident' ? 0x400 : 0)});
  seed(ctx, registers, 0x9000, initialFlags); let unreadableData = false;
  function slice(name, budget, retired, offset, flags, changes = {}, fault, loaded = false, cancel = 0) {
    refresh(ctx).view.setUint32(ctx.base + 96, cancel, true); const expected = arena(ctx);
    for (const [index, value] of Object.entries(changes)) registers[Number(index)] = value;
    expected.set(stateRecord(registers, 0x9000 + offset, flags)); expected.set(fault ? exitRecord(5, retired, 2, fault, address, 1, 1) : exitRecord(cancel ? 2 : 1, retired), 56);
    if (fault) expected.set(helper(0, fault, address), 100); else if (loaded) expected.set(helper(changedInput), 100);
    assert.equal(guestRun(ctx, unit, ctx.base, ctx.base + 56, budget, ctx.base + 96), 0, name); assert.deepEqual(arena(ctx), expected, `${label}:${name}: full arena BEFORE observations`);
    if (unreadableData) {pure(ctx, () => ctx.api.protect(0x4000, 1, 3), 0, 'explicit observation-only RW', false); observationInputs.push({slice: name, api: 'protect', address: 0x4000, permissions: 3});}
    const pages = readPages(ctx);
    if (unreadableData) {pure(ctx, () => ctx.api.protect(0x4000, 1, 2), 0, 'restore WRITE-only before next guest input', false); observationInputs.push({slice: name, api: 'protect', address: 0x4000, permissions: 2});}
    slices.push({name, budget, retired, pc: 0x9000 + offset, flags, registers: [...registers], cancel, fault: fault ? {reason: fault, address, access: 1, length: 1} : undefined, complete_pages: pages});
    if (!retired && !fault) controls.push({owner, name: `${label}:${name}`, budget, retired: 0});
  }
  slice('zero_before_real_ADD', 0, 0, 0, initialFlags); slice('cancel_before_real_ADD', 1, 0, 0, initialFlags, {}, undefined, false, 1);
  slice('ADD_CL80_produces_CF_OF_once', 1, 1, 3, producerFlags, {1: 0x23457f00});
  slice('zero_after_ADD_before_MOV', 0, 0, 3, producerFlags); slice('cancel_after_ADD_before_MOV', 1, 0, 3, producerFlags, {}, undefined, false, 1);
  slice('MOV_address_prefix_once', 1, 1, 8, producerFlags, {6: address});
  slice('zero_after_MOV_before_read', 0, 0, 8, producerFlags); slice('cancel_after_MOV_before_read', 1, 0, 8, producerFlags, {}, undefined, false, 1);
  slice('real_unmapped_Read8_preserves_producer_FLAGS_and_all_GPR', 1, 0, 8, producerFlags, {}, 1);
  slice('zero_at_unmapped_fault_PC', 0, 0, 8, producerFlags); slice('cancel_at_unmapped_fault_PC', 1, 0, 8, producerFlags, {}, undefined, false, 1);
  const data = Buffer.alloc(4096), neighbors = Buffer.from(Array.from({length: 32}, (_, index) => 0xa1 + index)); neighbors[15] = firstInput;
  pure(ctx, () => ctx.api.map(0x4000, 1, 3), 0, 'first data-only repair map RW', false); upload(ctx, 0x4000, neighbors); data.set(neighbors); ctx.ramPages.set(0x4000, data); const firstHex = readDeclared(ctx, 0x4000);
  pure(ctx, () => ctx.api.protect(0x4000, 1, 2), 0, 'initialized data becomes WRITE-only', false); unreadableData = true; guardCurrent(ctx, unit);
  repairs.push({phase: 1, actions: [{api: 'map', address: 0x4000, pages: 1, permissions: 3}, {api: 'upload', address: 0x4000, bytes: 32, hex: firstHex}, {api: 'protect', address: 0x4000, pages: 1, permissions: 2}]});
  slice('same_PC_real_Read_permission_fault_on_WRITE_only_page', 1, 0, 8, producerFlags, {}, 2);
  slice('zero_at_permission_fault_PC', 0, 0, 8, producerFlags); slice('cancel_at_permission_fault_PC', 1, 0, 8, producerFlags, {}, undefined, false, 1);
  hostByte(ctx, address, changedInput); pure(ctx, () => ctx.api.protect(0x4000, 1, 1), 0, 'second repair READ-only permits byte source', false); unreadableData = false; guardCurrent(ctx, unit); const changedHex = readDeclared(ctx, 0x4000);
  repairs.push({phase: 2, actions: [{api: 'write8', address, value: changedInput, permissions: 2}, {api: 'protect', address: 0x4000, pages: 1, permissions: 1}]});
  const beforeCompare = [...registers]; slice('same_PC_reads_changed_RAM_then_only_publishes_FLAGS', 1, 1, 10, outcome.flags, {}, undefined, true);
  assert.deepEqual(registers, beforeCompare, 'successful CMP changes no GPR'); const finalHex = readDeclared(ctx, 0x4000); assert.equal(finalHex, changedHex, 'CMP leaves changed operand and31 neighbors untouched');
  slice('zero_after_compare_before_consumers', 0, 0, 10, outcome.flags); slice('cancel_after_compare_before_consumers', 1, 0, 10, outcome.flags, {}, undefined, false, 1);
  const carry = leftIndex === 0 ? 1 : 0;
  slice('SETC_consumes_fresh_CF', 1, 1, 13, outcome.flags, {3: inserted(registers, 3, carry)[3]});
  slice('SETO_consumes_fresh_OF', 1, 1, 16, outcome.flags, {3: inserted(registers, 7, 1)[3]});
  slice('JMP_finishes_without_prefix_or_compare_replay', 1, 1, 18, outcome.flags);
  guardCurrent(ctx, unit); assert.equal(slices.length, 20); assert.equal(slices.reduce((sum, row) => sum + row.retired, 0), 6); assert.equal(observationInputs.length, 6);
  liveRows.push({owner, left: leftIndex, module_label: unit.label, initial_registers: initial, initial_flags: initialFlags, producer_flags: producerFlags, initial_pc: 0x9000, compare_pc: 0x9008, address, first_input: firstInput, changed_input: changedInput, original_left: lhs, hypothetical_first: hypothetical, result: outcome.result, flags: outcome.flags, first_ram_hex: firstHex, changed_ram_hex: changedHex, final_ram_hex: finalHex, guest_retired: 6, slices, repairs, observation_inputs: observationInputs,
    claim: 'one initial seed, real ADD producer and MOV once; unmapped/WRITE-only read faults preserve producer flags and all GPR; changed-byte READ-only retry at samePC only publishes CMP flags; SETC/SETO consume them; no CPU/FLAGS/EIP patch/recompile/prefix replay'});
  pure(ctx, () => ctx.api.close(), 0, 'close live context', false);
}
for (const owner of ['replacement', 'resident']) for (const [left, input] of [[0, 128], [4, 127]]) livePath(owner, left, input);
const program = 'b8ff3f4000b9ffff4523bbffff6745baffff5634b18080c1803a00723870363a200f92c30f90c7792c7a2ab1803a080f94c20f9ac6751ebe1400c0c889350030400056ff1550314000c70504304000a5a5a5a50f0bbedec0adde89350030400056ff15503140000f0b';
const callerOffsets = [0, 29, 31, 41, 43, 55, 85], relocationOffsets = [1, 62, 69, 75, 92, 99], relocationRvas = [0x3fff, 0x3000, 0x3150, 0x3004, 0x3000, 0x3150];
const branchEdges = [[27, 0x72, 85, 29], [29, 0x70, 85, 31], [39, 0x79, 85, 41], [41, 0x7a, 85, 43], [53, 0x75, 85, 55]];
assert.equal(program.length / 2, 105);
for (const [offset, opcode, target, fallthrough] of branchEdges) {const bytes = Buffer.from(program, 'hex'); assert.equal(bytes[offset], opcode); assert.equal(offset + 2, fallthrough); assert.equal(offset + 2 + bytes.readInt8(offset + 1), target); assert.ok(callerOffsets.includes(target) && callerOffsets.includes(fallthrough));}
for (const [offset, hex] of [[25, '3a00'], [31, '3a20'], [45, '3a08']]) assert.equal(Buffer.from(program, 'hex').subarray(offset, offset + 2).toString('hex'), hex, 'original parent supplies the low/high memory address and both CMP operands survive');
for (const [offset, hex] of [[33, '0f92c3'], [36, '0f90c7'], [47, '0f94c2'], [50, '0f9ac6']]) assert.equal(Buffer.from(program, 'hex').subarray(offset, offset + 3).toString('hex'), hex, 'direct fresh CF/OF/ZF/PF consumers before later flag producers');
assert.equal(Buffer.from(program, 'hex').subarray(20, 25).toString('hex'), 'b18080c180', 'MOVCL80 then ADDCL80 genuinely sets CF/OF before low compare');
function literalImage() {
  const bytes = Buffer.alloc(0x1600); bytes.write('MZ'); bytes.writeUInt32LE(0x80, 0x3c); bytes.write('PE\0\0', 0x80);
  for (const [at, value] of [[0x84, 0x14c], [0x86, 3], [0x94, 224], [0x96, 0x102], [0x98, 0x10b], [0xdc, 3], [0xde, 0x100]]) bytes.writeUInt16LE(value, at);
  for (const [at, value] of [[4, 0x200], [8, 0x1200], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, 0x400000], [32, 4096], [36, 512], [56, 0x6000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16], [104, 0x3100], [108, 40], [136, 0x5000], [140, 20], [192, 0x3150], [196, 8]]) bytes.writeUInt32LE(value, 0x98 + at);
  for (const [index, name, virtualSize, rva, rawSize, rawPointer, flags] of [[0, '.text', 105, 0x1000, 512, 0x200, 0x60000020], [1, '.data', 4096, 0x3000, 4096, 0x400, 0xc0000040], [2, '.fixups', 20, 0x5000, 512, 0x1400, 0x40000040]]) {
    const at = 0x178 + index * 40; bytes.write(name, at); for (const [offset, value] of [[8, virtualSize], [12, rva], [16, rawSize], [20, rawPointer], [36, flags]]) bytes.writeUInt32LE(value, at + offset);
  }
  bytes.fill(0xcc, 0x200, 0x400); bytes.set(Buffer.from(program, 'hex'), 0x200); for (let index = 0; index < 31; index++) bytes[0x13e0 + index] = index + 1; bytes[0x13ff] = 128;
  bytes.set(words([0x3140, 0, 0, 0x3160, 0x3150]), 0x500); bytes.writeUInt32LE(0x3170, 0x540); bytes.writeUInt32LE(0x3170, 0x550); bytes.write('kernel32.dll\0', 0x560); bytes.write('ExitProcess\0', 0x572);
  bytes.writeUInt32LE(0x1000, 0x1400); bytes.writeUInt32LE(20, 0x1404); relocationOffsets.forEach((offset, index) => bytes.writeUInt16LE(0x3000 + offset, 0x1408 + index * 2)); return bytes;
}
const image = readFileSync(join(output, 'compare.exe')); assert.deepEqual(image, literalImage(), 'independently reconstructed entire5632byte PE headers/sections/imports/data/fixups'); writeFileSync(join(output, 'program.x86'), Buffer.from(program, 'hex'));
function read32(ctx, address, value) {
  const expected = arena(ctx); expected.set(record('R3MH', 40, [0, value, 0, 0, 0, 0]), 100); assert.equal(ctx.api.read32(address), 0); hostRead32Calls++; assert.deepEqual(arena(ctx), expected, 'normal Read32 strict helper only'); return value;
}
function assertWords(ctx, address, values) {values.forEach((value, index) => read32(ctx, address + index * 4, value)); ramChecks++;}
for (const base of [0x400000, 0x500000]) for (const owner of ['replacement', 'resident']) {
  const label = `pe-${owner}-${base.toString(16)}`, ctx = fresh(owner), entry = base + 0x1000;
  pure(ctx, () => ctx.api.begin_image_input(image.length), 0, 'begin full copied input', false);
  for (const [offset, length] of [[0, 4096], [4096, 1536]]) {request(ctx, image.subarray(offset, offset + length)); pure(ctx, () => ctx.api.append_image_input(offset, length), 0, 'bounded full-file staging copy', false); refresh(ctx).bytes.fill(0xa5, ctx.base + TRANSFER, ctx.base + SIZE);}
  const beforeLoad = arena(ctx), receipt = record('R3LI', 72, [base, 0x6000, entry, 5, 0x8000, 1, 0, 0, EXIT, EXIT_ID, 0, 0, 0, 0], 2), expectedLoad = beforeLoad.slice(); expectedLoad.set(receipt, TRANSFER);
  assert.equal(ctx.api.load_pe32_linked_v2_input_at(base, 0x8000), 0); assert.deepEqual(arena(ctx), expectedLoad, 'strict gate-inclusive linked-v2 receipt'); writeFileSync(join(output, `${label}-receipt.bin`), receipt);
  const header = Buffer.alloc(4096), text = Buffer.alloc(4096), data = Buffer.alloc(4096), fixups = Buffer.alloc(4096), gatePage = Buffer.alloc(4096);
  header.set(image.subarray(0, 512)); text.set(image.subarray(0x200, 0x400)); data.set(image.subarray(0x400, 0x1400)); fixups.set(image.subarray(0x1400, 0x1600)); gatePage.set([0x0f, 0x0b], 0x20);
  relocationOffsets.forEach((offset, index) => text.writeUInt32LE(base + relocationRvas[index], offset)); data.writeUInt32LE(EXIT, 0x150);
  for (const [address, bytes] of [[base, header], [entry, text], [base + 0x3000, data], [base + 0x5000, fixups], [0x8000, gatePage]]) ctx.ramPages.set(address, bytes);
  const initialPages = readPages(ctx); read32(ctx, base + 0xb4, 0x400000); read32(ctx, base + 0x3150, EXIT); read32(ctx, base + 0x3154, 0); relocationOffsets.forEach((offset, index) => read32(ctx, entry + offset, base + relocationRvas[index]));
  assertWords(ctx, base + 0x3000, Array(8).fill(0)); for (let index = 0; index < 32; index++) read8(ctx, base + 0x3fe0 + index, index < 31 ? index + 1 : 128);
  read8(ctx, base + 0x4000, 0, 1); controls.push({owner, name: `${label}: page following last-byte operand is unmapped`, address: base + 0x4000, reason: 1, length: 1});
  const entries = callerOffsets.map(offset => entry + offset), gates = [[EXIT, EXIT_ID]]; if (owner === 'replacement') entries.push(EXIT);
  const caller = compile(ctx, entries, owner === 'replacement' ? gates : [], `${label}-caller`, ['read32', owner === 'replacement' ? 'store32' : 'store_resident32', 'read8']);
  const gate = owner === 'replacement' ? caller : compile(ctx, [EXIT], gates, `${label}-gate`, []);
  const expectedStart = arena(ctx); expectedStart.set(stateRecord([0, 0, 0, 0, 0x71000, 0, 0, 0], entry, 2)); expectedStart.set(exitRecord(3, 0, 3), 56);
  assert.equal(ctx.api.start_loaded_image(0x70000, 1), 0); assert.deepEqual(arena(ctx), expectedStart, 'Rust startup R3EXv3 owns CPU/stack'); ctx.ramPages.set(0x70000, Buffer.alloc(4096)); const startupPages = readPages(ctx);
  const finalRegisters = [base + 0x3fff, 0x2345ff80, 0x34560101, 0x45670101, 0x70ff8, 0, MARKER, 0], expectedRun = arena(ctx); expectedRun.set(stateRecord(finalRegisters, EXIT, 0x46)); expectedRun.set(exitRecord(owner === 'resident' ? 3 : 8, 23, owner === 'resident' ? 2 : 3, owner === 'resident' ? 0 : EXIT_ID), 56); expectedRun.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100);
  assert.equal(guestRun(ctx, caller, ctx.base, ctx.base + 56, 64, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedRun, `${label}:23 genuine retirements, unchanged original-address parent and direct CF/OF/SF/ZF/PF conditions`);
  data.writeUInt32LE(MARKER, 0); const stack = ctx.ramPages.get(0x70000); stack.writeUInt32LE(entry + 73, 0xff8); stack.writeUInt32LE(MARKER, 0xffc); const executedPages = readPages(ctx);
  if (owner === 'resident') {const expectedGate = arena(ctx); expectedGate.set(exitRecord(8, 0, 3, EXIT_ID), 56); assert.equal(guestRun(ctx, gate, ctx.base, ctx.base + 56, 1, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedGate, 'separate current resident named gate');}
  const frame = record('R3CF', 112, [1, EXIT_ID, 2, 1, EXIT, 0x70ff8, entry + 73, 0, MARKER, ...Array(15).fill(0)]), expectedCapture = arena(ctx); expectedCapture.set(frame, TRANSFER);
  assert.equal(owner === 'replacement' ? ctx.api.capture_call(ctx.low, ctx.high, gate.generation, 2, 1) : ctx.api.capture_resident_call(ctx.low, ctx.high, gate.low, gate.high, 2, 1), 0); assert.deepEqual(arena(ctx), expectedCapture, 'real CALL frame/return/argument only');
  const expectedComplete = arena(ctx); expectedComplete.set(exitRecord(9, 0, 4, MARKER), 56); assert.equal(owner === 'replacement' ? ctx.api.complete_windows_call(ctx.low, ctx.high, gate.generation, 1) : ctx.api.complete_resident_windows_call(ctx.low, ctx.high, gate.low, gate.high, 1), 0); assert.deepEqual(arena(ctx), expectedComplete, 'named ExitProcess terminal marker preserves committed CPU');
  const terminalPages = readPages(ctx); assertWords(ctx, base + 0x3000, [MARKER, 0, 0, 0, 0, 0, 0, 0]); assertWords(ctx, 0x70fe0, [0, 0, 0, 0, 0, 0, entry + 73, MARKER]);
  for (let index = 0; index < 32; index++) read8(ctx, base + 0x3fe0 + index, index < 31 ? index + 1 : 128);
  refresh(ctx); for (const unit of new Set([caller, gate])) assert.deepEqual(ctx.bytes.slice(unit.pointer, unit.pointer + unit.length), unit.bytes, 'retained current PE module bytes exact');
  pure(ctx, () => guestRun(ctx, caller, 0xffffffff, 0xffffffff, 0, 0xffffffff), 21, `${label}: terminal latch before malformed pointers/budget`);
  peRows.push({owner, base, guest_retired: 23, result: MARKER, final_registers: finalRegisters, final_flags: 0x46, canary: 0, operand_address: base + 0x3fff, operand_value: 128, operand_neighbor_hex: Buffer.from(Array.from({length: 31}, (_, index) => index + 1)).toString('hex') + '80', receipt_hex: Buffer.from(receipt).toString('hex'), receipt_sha256: hash(receipt), frame_hex: Buffer.from(frame).toString('hex'), stack_words: [0, 0, 0, 0, 0, 0, entry + 73, MARKER], initial_pages: initialPages, startup_pages: startupPages, executed_pages: executedPages, terminal_pages: terminalPages});
  pure(ctx, () => ctx.api.close(), 0, 'close PE context', false);
}
const counts = {
  instances: ordinal, canonical_forms_per_owner: canonical.length, alias_forms_per_owner: aliases.length,
  canonical_cases: rows.filter(row => row.group === 'canonical').length, boundary_cases: rows.filter(row => row.group === 'boundary').length, alias_cases: rows.filter(row => row.group === 'alias').length,
  finite_guest_retired: rows.length, modules: modules.length, controls: controls.length,
  live_paths: liveRows.length, live_runs: liveRows.reduce((sum, row) => sum + row.slices.length, 0), live_faults: liveRows.reduce((sum, row) => sum + row.slices.filter(slice => slice.fault).length, 0), live_guest_retired: liveRows.reduce((sum, row) => sum + row.guest_retired, 0),
  semantic_repair_phases: liveRows.reduce((sum, row) => sum + row.repairs.length, 0), repair_api_calls: liveRows.reduce((sum, row) => sum + row.repairs.reduce((subtotal, phase) => subtotal + phase.actions.length, 0), 0), observation_permission_inputs: liveRows.reduce((sum, row) => sum + row.observation_inputs.length, 0),
  pe_paths: peRows.length, pe_guest_retired: peRows.reduce((sum, row) => sum + row.guest_retired, 0), total_guest_retired: rows.length + 24 + 92,
  llvm_encoding_symbols: llvm.length, runtime_sources: sourcePaths.length, generated_function_invocations: generatedRuns.length, maximum_resident_units: residentPeak,
  complete_page_observations: pageChecks, host_read32_observations: hostRead32Calls, host_read8_observations: hostRead8Calls, neighbor_or_stack_range_checks: ramChecks,
};
assert.deepEqual(counts, {instances: 10, canonical_forms_per_owner: 8, alias_forms_per_owner: 8, canonical_cases: 64, boundary_cases: 98, alias_cases: 64, finite_guest_retired: 226, modules: 14, controls: 86, live_paths: 4, live_runs: 80, live_faults: 8, live_guest_retired: 24, semantic_repair_phases: 8, repair_api_calls: 20, observation_permission_inputs: 24, pe_paths: 4, pe_guest_retired: 92, total_guest_retired: 342, llvm_encoding_symbols: 1, runtime_sources: 65, generated_function_invocations: 332, maximum_resident_units: 2, complete_page_observations: 1606, host_read32_observations: 1644676, host_read8_observations: 15110, neighbor_or_stack_range_checks: 476});
for (const owner of ['replacement', 'resident']) {
  const owned = rows.filter(row => row.owner === owner); assert.equal(owned.length, 113);
  for (const flags of flagSeeds) assert.equal(owned.filter(row => row.group === 'canonical' && row.initial_flags === flags).length, 8);
  assert.equal(owned.filter(row => row.group === 'boundary' && row.initial_flags === 0xcd7).length, 49);
  for (const flags of [0x8d6, 0xcd7]) assert.equal(owned.filter(row => row.group === 'alias' && row.initial_flags === flags).length, 16);
}
const repairCensus = Object.fromEntries(['map', 'upload', 'protect', 'write8'].map(api => [api, liveRows.flatMap(row => row.repairs.flatMap(phase => phase.actions)).filter(action => action.api === api).length])); assert.deepEqual(repairCensus, {map: 4, upload: 4, protect: 8, write8: 4});
assert.equal(modules.filter(row => row.label.startsWith('pe-')).length, 6); assert.equal(modules.filter(row => row.label.includes('-live-')).length, 4); assert.equal(modules.filter(row => row.imports.some(item => ['store8', 'store_resident8'].includes(item.name))).length, 0);
const statuses = Object.fromEntries([0, 1, 3, 4, 5, 21].map(status => [status, generatedRuns.filter(row => row.status === status).length])), budgets = Object.fromEntries([0, 1, 64].map(budget => [budget, generatedRuns.filter(row => row.budget === budget).length]));
assert.deepEqual(statuses, {0: 320, 1: 3, 3: 1, 4: 2, 5: 2, 21: 4}); assert.deepEqual(budgets, {0: 38, 1: 290, 64: 4});
assert.deepEqual(Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), sourceHashes, 'first sources unchanged through run'); assert.equal(hash(readFileSync(enginePath)), hash(engineBytes), 'first engine unchanged through run');
const result = {
  status: 'ok', counts, repair_api_census: repairCensus, generated_status_census: statuses, generated_budget_census: budgets, engine_sha256: hash(engineBytes), engine_exports: WebAssembly.Module.exports(engineModule), engine_function_signatures: engineFunctionSignatures, pe_sha256: hash(image), source_sha256: sourceHashes,
  tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0], llvm: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).split('\n')[0]},
  assembly: {command: ['clang', ...assemble], disassembly_command: ['xcrun', ...disassemble], object_sha256: hash(object), disassembly_sha256: hash(Buffer.from(disassembly)), symbols: llvm, scope: 'encoding only; one canonical register-left memory-RHS CMP8 symbol,2 bytes; collected once before first generated guest calls'},
  oracle: {program, caller_offsets: callerOffsets, branches: branchEdges, relocation_offsets: relocationOffsets, relocation_rvas: relocationRvas, pe_file_bytes: 5632, pe_data_raw_bytes: 4096, pe_chunks: [[0, 4096], [4096, 1536]], pe_return_offset: 73, pe_initial_operands: [128], pe_final_operands: [128], pe_guest_instruction_count: 23,
    flag_seeds: flagSeeds, boundaries, canonical_hex: canonicalCode.toString('hex'), canonical_instruction_count: 9, alias_hex: aliasCode.toString('hex'), alias_instruction_count: 9, alias_shapes: aliasShapes,
    alias_inputs: [127, 128], alias_flag_seeds: [0x8d6, 0xcd7], live_instruction_count: 6, live_compile_bytes: 18, maximum_resident_units_per_instance: 2,
    flag_policy: 'strict validated Read8 RHS then original byte-register LHS; independent numeric unsigned borrow/nibble borrow/signed8 range/even parity; CF/AF/OF/SF/ZF/PF freshly computed, DF retained and fixed bit1=2; incoming CF is not an operand; no register insertion/Store8/RAM write',
    pe_final_registers: {eax: 'base+3fff', ecx: 0x2345ff80, edx: 0x34560101, ebx: 0x45670101, esp: 0x70ff8, ebp: 0, esi: MARKER, edi: 0}, pe_final_flags: 0x46,
    host_observation_scope: 'guest whole4236arena asserted before normal public Read32/Read8; complete declared4096byte pages and last strict helper preserve all other bytes; WRITE-only pages temporarily become RW for counted observations then return to WRITE-only before next guest input'},
  generated_runs: generatedRuns, cases: rows, controls, live: liveRows, pe: peRows, modules,
  claim: 'finite authored canonical CMP3A register-left memory-RHS with checked Read8 through actual engine-Wasm bound replacement/resident children;226 saved expected-outcome rows backed by live4236arena/complete4096pages/31nonzero neighbors/fullGPR assertions, not raw per-row snapshots;four ADDproducer/MOVonce paths with8 realReadfaults and operand-only repair/READ-only retry, no CPU/FLAGS/EIP patch/recompile/prefix replay;external code invalidation separate,no Store8/status11 guest claim;four fresh copiedPE paths with dirty realCF/OF producer then original-parent low/high/equalCMP/directconditions/SETcc/realCALL/namedExitProcess;LLVM encoding only;no standalonebinding/generalISA/EA/flagdomain/SDK/browser/performance/fullP2/playablegame claim',
};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2)); console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
