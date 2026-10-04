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
const SIZE = 4236, TRANSFER = 140, EXIT = 0x8020, EXIT_ID = 0x10003, MARKER = 0xc8c0000d;
const boundaries = [0, 1, 15, 16, 127, 128, 255], flagSeeds = [2, 3, 0xcd6, 0xcd7];
const initialRegisters = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, upload: 2, read8: 1, read32: 1, write8: 2, compile_entries: 2, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store32: 2, store_resident32: 6, store8: 2, store_resident8: 6, begin_image_input: 1, append_image_input: 2, load_pe32_linked_v2_input_at: 2, start_loaded_image: 2, capture_call: 5, capture_resident_call: 6, complete_windows_call: 4, complete_resident_windows_call: 5};
const sourcePaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/control.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/cpu/dbt/wasm/memory/byte_store.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/locals.rs', 'engine/src/process/startup.rs', 'engine/src/abi/x86/exit.rs', 'engine/src/process/instance.rs', 'engine/src/process/wasm.rs', 'engine/src/process/call.rs', 'engine/src/process/windows.rs', 'engine/src/abi/wasm/exports.rs', 'engine/src/abi/memory_helper.rs', 'engine/src/loader/mod.rs', 'engine/src/loader/imports.rs', 'engine/src/loader/relocation.rs', 'engine/src/memory/space.rs', 'engine/tests/support/pe32.rs', 'engine/tests/cpu_memory_byte_unary_wasm.rs', 'engine/tests/fixtures/p2-memory-byte-unary/run.mjs', 'engine/tests/fixtures/p2-memory-byte-unary/integer.S'];
assert.equal(sourcePaths.length, 33);
const sourceHashes = Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
writeFileSync(join(output, 'source-hashes.json'), JSON.stringify({engine_sha256: hash(engineBytes), sources: sourceHashes}, null, 2));
writeFileSync(join(output, 'engine.wasm'), engineBytes);
const rows = [], controls = [], liveRows = [], smcRows = [], peRows = [], modules = [], generatedRuns = [];
let ordinal = 0, ramChecks = 0, residentPeak = 0;

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
  const instance = new WebAssembly.Instance(engineModule, {}), key = 0xc836800000000000n + BigInt(++ordinal);
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
function compile(ctx, entries, gates, label, extraHelpers = ['read8', ctx.owner === 'replacement' ? 'store8' : 'store_resident8']) {
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
  refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length);
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

function storeHelper(reason = 0, address = 0) {return record('R3MH', 40, [reason ? 1 : 0, 0, reason, reason ? address : 0, reason ? 2 : 0, 1], 3);}
function unary(kind, value, oldFlags) {
  const result = kind === 'inc' ? (value + 1) % 256 : kind === 'dec' ? (value + 255) % 256 : kind === 'neg' ? (256 - value) % 256 : 255 - value;
  if (kind === 'not') return {result, flags: oldFlags};
  let ones = 0; for (let number = result; number > 0; number = Math.floor(number / 2)) ones += number % 2;
  const carry = kind === 'neg' ? Number(value !== 0) : oldFlags % 2;
  const auxiliary = kind === 'inc' ? value % 16 === 15 : kind === 'dec' ? value % 16 === 0 : value % 16 !== 0;
  const signed = value < 128 ? value : value - 256;
  const exact = kind === 'inc' ? signed + 1 : kind === 'dec' ? signed - 1 : -signed;
  const flags = Math.floor(oldFlags / 0x400) % 2 * 0x400 + 2 + carry + (ones % 2 === 0 ? 4 : 0)
    + (auxiliary ? 0x10 : 0) + (result === 0 ? 0x40 : 0) + (result >= 128 ? 0x80 : 0) + (exact < -128 || exact > 127 ? 0x800 : 0);
  return {result, flags};
}
for (const [kind, value, result, flags] of [['inc', 255, 0, 0x56], ['inc', 127, 128, 0x892], ['dec', 0, 255, 0x96], ['dec', 128, 127, 0x812], ['neg', 0, 0, 0x46], ['neg', 128, 128, 0x883], ['neg', 1, 255, 0x97]]) assert.deepEqual(unary(kind, value, 2), {result, flags});
assert.deepEqual(unary('not', 128, 0xcd7), {result: 127, flags: 0xcd7});
function read8(ctx, address, expectedValue, reason = 0) {
  const expected = arena(ctx); expected.set(helper(expectedValue, reason, reason ? address : 0), 100);
  assert.equal(ctx.api.read8(address), 0); assert.deepEqual(arena(ctx), expected, 'actual host Read8 changes only strict v2 helper record'); return expectedValue;
}
function readDeclared(ctx, address, length = 32) {
  const page = Math.floor(address / 4096) * 4096, expected = ctx.ramPages.get(page); assert.ok(expected && address + length <= page + 4096);
  const bytes = expected.subarray(address - page, address - page + length);
  for (let index = 0; index < length; index++) read8(ctx, address + index, bytes[index]); ramChecks++; return bytes.toString('hex');
}
function hostByte(ctx, address, value) {
  const expected = arena(ctx); expected.set(storeHelper(), 100); assert.equal(ctx.api.write8(address, value), 0);
  assert.deepEqual(arena(ctx), expected, 'explicit host operand update changes strict v3 helper only');
  ctx.ramPages.get(Math.floor(address / 4096) * 4096)[address % 4096] = value;
}
function runUnary(ctx, unit, form, registers, input, flags, name, group) {
  hostByte(ctx, form.address, input); guardCurrent(ctx, unit);
  const range = Math.floor(form.address / 32) * 32, beforeHex = readDeclared(ctx, range), outcome = unary(form.kind, input, flags);
  seed(ctx, registers, form.pc, flags); const expected = arena(ctx);
  expected.set(stateRecord(registers, form.pc + form.bytes.length, outcome.flags)); expected.set(exitRecord(1, 1), 56); expected.set(storeHelper(), 100);
  assert.equal(guestRun(ctx, unit, ctx.base, ctx.base + 56, 1, ctx.base + 96), 0, name); assert.deepEqual(arena(ctx), expected, `${name}: full GPR/FLAGS/EIP/exit/helper/transfer arena`);
  ctx.ramPages.get(Math.floor(form.address / 4096) * 4096)[form.address % 4096] = outcome.result;
  const afterHex = readDeclared(ctx, range); guardCurrent(ctx, unit);
  const expectedBytes = Buffer.from(beforeHex, 'hex'); expectedBytes[form.address - range] = outcome.result; assert.equal(afterHex, expectedBytes.toString('hex'), 'only addressed byte may change, including idempotent writes');
  rows.push({owner: ctx.owner, name, group, kind: form.kind, encoding: Buffer.from(form.bytes).toString('hex'), pc: form.pc, address: form.address, initial_registers: registers, input, initial_flags: flags, result: outcome.result, flags: outcome.flags, retired: 1, next_pc: form.pc + form.bytes.length, declared_ram: {address: range, bytes: 32, before_hex: beforeHex, expected_hex: afterHex, expected_sha256: hash(Buffer.from(afterHex, 'hex'))}});
}
function noRetirement(ctx, unit, pc, cancel) {
  seed(ctx, initialRegisters, pc, 0xcd7, cancel); const expected = arena(ctx); expected.set(exitRecord(cancel ? 2 : 1, 0), 56);
  assert.equal(guestRun(ctx, unit, ctx.base, ctx.base + 56, cancel ? 1 : 0, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expected, 'zero/cancel precedes checked read and store');
  controls.push({owner: ctx.owner, name: `${unit.label}:${cancel ? 'cancel_before_read' : 'zero_before_read'}`, budget: cancel ? 1 : 0, retired: 0});
}
const kinds = [['inc', 0xfe, 0], ['dec', 0xfe, 1], ['neg', 0xf6, 3], ['not', 0xf6, 2]];
const forms = kinds.map(([kind, opcode, extension], index) => ({kind, bytes: [opcode, extension * 8 + 6], pc: 0x8000 + index * 2, address: 0x3fff}));
const canonicalCode = Buffer.from([...forms.flatMap(form => form.bytes), 0xeb, 0, 0x0f, 0x0b]); assert.equal(canonicalCode.length, 12); assert.deepEqual(canonicalCode, readFileSync(join(output, 'canonical.x86')));
const aliasShapes = [
  {parent: 0, value: 0x3001, bytes: [0x00], address: 0x3001},
  {parent: 1, value: 0x3042, bytes: [0x41, 0xf0], address: 0x3032},
  {parent: 2, value: 0xfffffff0, index: 1, index_value: 0x1800, bytes: [0x44, 0x4a, 0x11], address: 0x3001},
  {parent: 3, value: 0x3066, index: 6, index_value: 2, bytes: [0x84, 0xf3, 0xe0, 0xff, 0xff, 0xff], address: 0x3056},
  {parent: 4, value: 0x3070, bytes: [0x04, 0x24], address: 0x3070},
  {parent: 5, value: 0xffffffff, bytes: [0x45, 0], address: 0xffffffff},
  {parent: 6, value: 0x3fff, bytes: [0x06], address: 0x3fff},
  {parent: 7, value: 0x3080, bytes: [0x87, 0, 1, 0, 0], address: 0x3180},
];
const aliases = []; let aliasPc = 0x8200;
for (const [kind, opcode, extension] of kinds) for (const shape of aliasShapes) {
  const bytes = [opcode, shape.bytes[0] + extension * 8, ...shape.bytes.slice(1)]; aliases.push({...shape, kind, bytes, pc: aliasPc}); aliasPc += bytes.length;
}
const aliasCode = Buffer.from([...aliases.flatMap(form => form.bytes), 0xeb, 0, 0x0f, 0x0b]); assert.equal(aliases.length, 32); assert.equal(aliasCode.length, 124); assert.deepEqual(aliasCode, readFileSync(join(output, 'aliases.x86')));
for (const owner of ['replacement', 'resident']) {
  const ctx = fresh(owner);
  for (const address of [0x8000, 0x3000, 0x5000, 0xfffff000]) pure(ctx, () => ctx.api.map(address, 1, 3 | (address === 0x8000 ? 4 : 0)), 0, 'map authored code/data', false);
  for (const address of [0x3000, 0x5000, 0xfffff000]) {const data = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1)); upload(ctx, address, data); ctx.ramPages.set(address, data);}
  upload(ctx, 0x8000, canonicalCode); upload(ctx, 0x8200, aliasCode); pure(ctx, () => ctx.api.protect(0x5000, 1, 2), 0, 'write-only target', false);
  const canonical = compile(ctx, [0x8000], [], `${owner}-canonical`);
  for (const form of forms) for (const input of boundaries) for (const flags of flagSeeds) {const registers = [...initialRegisters]; registers[6] = form.address; runUnary(ctx, canonical, form, registers, input, flags, `${owner}-${form.kind}-${input}-flags${flags}`, 'canonical');}
  noRetirement(ctx, canonical, 0x8000, 0); noRetirement(ctx, canonical, 0x8000, 1);
  const alias = compile(ctx, [0x8200], [], `${owner}-aliases`);
  for (const form of aliases) for (const flags of [2, 0xcd7]) {const registers = [...initialRegisters]; registers[form.parent] = form.value; if (form.index !== undefined) registers[form.index] = form.index_value; runUnary(ctx, alias, form, registers, 0x80, flags, `${owner}-alias-${form.kind}-parent${form.parent}-flags${flags}`, 'alias');}
  noRetirement(ctx, alias, 0x8200, 0); noRetirement(ctx, alias, 0x8200, 1);
  read8(ctx, 0x4000, 0, 1); controls.push({owner, name: 'page after writable endpoint is unmapped', reason: 1, address: 0x4000, length: 1});
  const denied = [...initialRegisters]; denied[6] = 0x500f;
  const current = owner === 'replacement' ? alias : canonical, faultPc = owner === 'replacement' ? aliases[6].pc : 0x8000;
  seed(ctx, denied, faultPc, 0xcd7); const faultExpected = arena(ctx); faultExpected.set(exitRecord(5, 0, 2, 2, 0x500f, 1, 1), 56); faultExpected.set(helper(0, 2, 0x500f), 100);
  assert.equal(guestRun(ctx, current, ctx.base, ctx.base + 56, 1, ctx.base + 96), 0); assert.deepEqual(arena(ctx), faultExpected, 'write-only unary fails on real Read8 before Store8');
  pure(ctx, () => ctx.api.protect(0x5000, 1, 3), 0, 'diagnostic permission only', false); readDeclared(ctx, 0x5000); pure(ctx, () => ctx.api.protect(0x5000, 1, 2), 0, 'restore declared write-only page', false);
  controls.push({owner, name: 'write-only unary reads before store', address: 0x500f, reason: 2, access: 1, length: 1, retired: 0});
  const guard = (low, high, identityLow, identityHigh, pointer = ctx.base) => owner === 'replacement' ? ctx.api.guard(low, high, identityLow, pointer, ctx.base + 56, ctx.base + 96) : ctx.api.guard_resident(low, high, identityLow, identityHigh, pointer, ctx.base + 56, ctx.base + 96);
  const identity = owner === 'replacement' ? alias.generation : alias.low;
  pure(ctx, () => guard(ctx.low + 1, ctx.high, identity, alias.high), 3, `${owner}: wrong key low`);
  pure(ctx, () => guard(ctx.low, ctx.high + 1, identity, alias.high), 3, `${owner}: wrong key high`);
  pure(ctx, () => guard(ctx.low, ctx.high, identity + 1, alias.high), 3, `${owner}: wrong generation or id low`);
  pure(ctx, () => guard(ctx.low, ctx.high, owner === 'replacement' ? 0 : identity, owner === 'replacement' ? undefined : alias.high + 1), 3, `${owner}: zero generation or wrong id high`);
  pure(ctx, () => guard(ctx.low, ctx.high, identity, alias.high, 0xffffffff), 1, `${owner}: current guard malformed pointer`);
  pure(ctx, () => guestRun(ctx, alias, 0xffffffff, 0xffffffff, 1, 0xffffffff), 1, `${owner}: current generated malformed pointers`);
  pure(ctx, () => ctx.api.close(), 0, 'close preserves arena', false); pure(ctx, () => guestRun(ctx, alias, 0xffffffff, 0xffffffff, 0, 0xffffffff), 5, `${owner}: Closed before malformed pointers`);
}
function livePath(owner, kind, opcode, extension, firstInput, changedInput) {
  const ctx = fresh(owner), label = `${owner}-live-${kind}`, address = 0x400f;
  const code = Buffer.from([0xbf, 0x0f, 0x40, 0, 0, opcode, extension * 8 + 7, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0, 0x0f, 0x0b]); assert.equal(code.length, 17); assert.deepEqual(code, readFileSync(join(output, `live-${kind}.x86`)));
  pure(ctx, () => ctx.api.map(0x9000, 1, 7), 0, 'map genuine live code', false); upload(ctx, 0x9000, code); const unit = compile(ctx, [0x9000], [], label);
  const registers = [...initialRegisters], initial = [...registers], slices = [], repairs = [], flags = 0xcd7, outcome = unary(kind, changedInput, flags);
  seed(ctx, registers, 0x9000, flags);
  function slice(name, budget, retired, offset, nextFlags, changes = {}, fault, stored = false, cancel = 0) {
    refresh(ctx).view.setUint32(ctx.base + 96, cancel, true); const expected = arena(ctx);
    for (const [index, value] of Object.entries(changes)) registers[Number(index)] = value;
    expected.set(stateRecord(registers, 0x9000 + offset, nextFlags)); expected.set(fault ? exitRecord(5, retired, 2, fault.reason, address, fault.access, 1) : exitRecord(cancel ? 2 : 1, retired), 56);
    if (fault) expected.set(fault.access === 1 ? helper(0, fault.reason, address) : storeHelper(fault.reason, address), 100); else if (stored) expected.set(storeHelper(), 100);
    assert.equal(guestRun(ctx, unit, ctx.base, ctx.base + 56, budget, ctx.base + 96), 0, name); assert.deepEqual(arena(ctx), expected, `${label}:${name}: whole arena and genuine continuation`);
    slices.push({name, budget, retired, pc: 0x9000 + offset, flags: nextFlags, registers: [...registers], cancel, fault});
    if (!retired && !fault) controls.push({owner, name: `${label}:${name}`, budget, retired: 0});
  }
  slice('zero_before_prefix_and_read', 0, 0, 0, flags); slice('cancel_before_prefix_and_read', 1, 0, 0, flags, {}, undefined, false, 1);
  slice('mov_prefix_retires_then_unmapped_read_fault', 64, 1, 5, flags, {7: address}, {reason: 1, access: 1});
  const page = Buffer.alloc(4096), neighbors = Buffer.from(Array.from({length: 32}, (_, index) => 0xa1 + index)); neighbors[15] = firstInput;
  pure(ctx, () => ctx.api.map(0x4000, 1, 3), 0, 'first data-only repair map RW', false); upload(ctx, 0x4000, neighbors); page.set(neighbors); ctx.ramPages.set(0x4000, page);
  pure(ctx, () => ctx.api.protect(0x4000, 1, 1), 0, 'first repair ends read-only', false); guardCurrent(ctx, unit); const firstHex = readDeclared(ctx, 0x4000);
  repairs.push({phase: 1, actions: [{api: 'map', address: 0x4000, pages: 1, permissions: 3}, {api: 'upload', address: 0x4000, bytes: 32, hex: firstHex}, {api: 'protect', address: 0x4000, pages: 1, permissions: 1}]});
  slice('same_pc_reads_initialized_byte_then_write_fault', 1, 0, 5, flags, {}, {reason: 2, access: 2}); assert.equal(readDeclared(ctx, 0x4000), firstHex, 'readable read-only operand and every nonzero neighbor unchanged after Store8 fault');
  pure(ctx, () => ctx.api.protect(0x4000, 1, 3), 0, 'second data-only repair RW', false); hostByte(ctx, address, changedInput); guardCurrent(ctx, unit); const changedHex = readDeclared(ctx, 0x4000);
  repairs.push({phase: 2, actions: [{api: 'protect', address: 0x4000, pages: 1, permissions: 3}, {api: 'write8', address, value: changedInput}]});
  assert.notEqual(unary(kind, firstInput, flags).result, outcome.result, 'changed input distinguishes recomputation from stale computed result');
  slice('same_pc_re_reads_changed_current_byte_and_commits', 1, 1, 7, outcome.flags, {}, undefined, true); ctx.ramPages.get(0x4000)[15] = outcome.result; const afterHex = readDeclared(ctx, 0x4000);
  const exactAfter = Buffer.from(changedHex, 'hex'); exactAfter[15] = outcome.result; assert.equal(afterHex, exactAfter.toString('hex'));
  slice('zero_after_unary_before_consumer', 0, 0, 7, outcome.flags); slice('cancel_after_unary_before_consumer', 1, 0, 7, outcome.flags, {}, undefined, false, 1);
  slice('setc_consumes_live_unary_carry', 1, 1, 10, outcome.flags, {3: initialRegisters[3] - initialRegisters[3] % 256 + outcome.flags % 2});
  slice('seto_consumes_live_unary_overflow', 1, 1, 13, outcome.flags, {3: registers[3] - Math.floor(registers[3] / 256) % 256 * 256 + Math.floor(outcome.flags / 0x800) % 2 * 256});
  slice('jmp_finishes', 1, 1, 15, outcome.flags); guardCurrent(ctx, unit); assert.equal(slices.length, 10); assert.equal(slices.reduce((sum, row) => sum + row.retired, 0), 5);
  liveRows.push({owner, kind, initial_registers: initial, initial_pc: 0x9000, initial_flags: flags, address, first_input: firstInput, changed_input: changedInput, result: outcome.result, flags: outcome.flags, first_ram_hex: firstHex, changed_ram_hex: changedHex, final_ram_hex: afterHex, guest_retired: 5, slices, repairs, claim: 'one initial CPU seed; subsequent inputs cancelword and data-only map/upload/protect/write8; no CPU/FLAGS/EIP patch or prefix replay'});
  pure(ctx, () => ctx.api.close(), 0, 'close live context', false);
}
for (const owner of ['replacement', 'resident']) for (const [kind, opcode, extension] of kinds) livePath(owner, kind, opcode, extension, {inc: 0x7f, dec: 0x80, neg: 0x80, not: 0x80}[kind], {inc: 0xff, dec: 0, neg: 1, not: 0xff}[kind]);
for (const owner of ['replacement', 'resident']) for (const [name, kind, input] of [['inc', 'inc', 127], ['dec', 'dec', 128], ['not', 'not', 128], ['neg-zero', 'neg', 0], ['neg-min', 'neg', 128]]) {
  const ctx = fresh(owner), label = `${owner}-smc-${name}`, definition = kinds.find(row => row[0] === kind), code = Buffer.from([definition[1], definition[2] * 8 + 6, 0xbf, 0xee, 0xff, 0xc0, 0x51, 0xeb, 0, 0x0f, 0x0b]); assert.deepEqual(code, readFileSync(join(output, `smc-${kind}.x86`)));
  const page = Buffer.alloc(4096, 0xcc), target = Buffer.from(Array.from({length: 32}, (_, index) => 0x61 + index)); target[15] = input; page.set(code); page.set(target, 0x100);
  pure(ctx, () => ctx.api.map(0x9000, 1, 7), 0, 'map consumed SMC page', false); upload(ctx, 0x9000, page); ctx.ramPages.set(0x9000, page);
  const unit = compile(ctx, [0x9000], [], `${label}-writer`), registers = [...initialRegisters]; registers[6] = 0x910f; const initial = [...registers], outcome = unary(kind, input, 0xcd7), beforeHex = readDeclared(ctx, 0x9100);
  seed(ctx, registers, 0x9000, 0xcd7); const expected = arena(ctx); expected.set(stateRecord(registers, 0x9002, outcome.flags)); expected.set(exitRecord(6, 1), 56); expected.set(storeHelper(), 100);
  assert.equal(guestRun(ctx, unit, ctx.base, ctx.base + 56, 64, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expected, 'changed/idempotent Store8 commits flags/next EIP/one retirement then CodeInvalidated');
  ctx.ramPages.get(0x9000)[0x10f] = outcome.result; const afterHex = readDeclared(ctx, 0x9100), exact = Buffer.from(beforeHex, 'hex'); exact[15] = outcome.result; assert.equal(afterHex, exact.toString('hex'));
  refresh(ctx).view.setUint32(ctx.base + 96, 1, true); pure(ctx, () => guestRun(ctx, unit, 0xffffffff, 0xffffffff, 0, 0xffffffff), 4, `${label}: stale dominates cancel zero and malformed pointers`);
  const successor = compile(ctx, [0x9002], [], `${label}-successor`, []); refresh(ctx).view.setUint32(ctx.base + 96, 0, true);
  const expectedSuccessor = arena(ctx); registers[7] = 0x51c0ffee; expectedSuccessor.set(stateRecord(registers, 0x9007, outcome.flags)); expectedSuccessor.set(exitRecord(1, 1, 1), 56);
  assert.equal(guestRun(ctx, successor, ctx.base, ctx.base + 56, 1, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedSuccessor, 'fresh successor begins at committed product PC and executes canary once without replaying unary');
  assert.equal(readDeclared(ctx, 0x9100), afterHex, 'successor never repeats byte transform'); guardCurrent(ctx, successor);
  pure(ctx, () => guestRun(ctx, unit, 0xffffffff, 0xffffffff, 0, 0xffffffff), owner === 'replacement' ? 3 : 4, `${label}: retained old generated function after new publication`);
  smcRows.push({owner, name, kind, input, initial_flags: 0xcd7, result: outcome.result, flags: outcome.flags, target: 0x910f, initial_registers: initial, writer_pc: 0x9000, committed_pc: 0x9002, writer_retired: 1, reason: 6, guest_run_status: 0, store_completion_contract: 'strict generated CodeInvalidated requires real Store8 completion status11; no synthetic helper', successor_pc: 0x9007, successor_retired: 1, canary_before: initial[7], canary_after: 0x51c0ffee, before_hex: beforeHex, final_hex: afterHex, idempotent: input === outcome.result, writer_module: unit.label, successor_module: successor.label});
  pure(ctx, () => ctx.api.close(), 0, 'close SMC context', false); pure(ctx, () => guestRun(ctx, successor, 0xffffffff, 0xffffffff, 0, 0xffffffff), 5, `${label}: Closed before malformed pointers`);
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
const assemblyPath = join(root, 'engine/tests/fixtures/p2-memory-byte-unary/integer.S'), objectPath = join(output, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assemblyPath, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(output, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), symbols = elfFixtures(object), llvm = [];
const encodingSamples = [['inc_memory', 'fe06'], ['dec_memory', 'fe0e'], ['neg_memory', 'f61e'], ['not_memory', 'f616']];
let llvmOffset = 0;
for (const [name, hex] of encodingSamples) {
  const symbol = symbols.get(name); assert.ok(symbol, name); assert.equal(symbol.offset, llvmOffset); assert.equal(symbol.bytes.toString('hex'), hex);
  llvm.push({name, offset: llvmOffset, hex}); llvmOffset += hex.length / 2;
  const start = disassembly.indexOf(`<${name}>:`); assert.ok(start >= 0);
  const next = disassembly.indexOf('\n\n', start), body = disassembly.slice(start, next < 0 ? undefined : next);
  assert.match(body, new RegExp(`\\b${name.split('_')[0]}\\s+`), `${name}: LLVM decodes the canonical family`);
}
assert.equal(symbols.size, 4); assert.equal(llvmOffset, 8);

const program = 'b8ff013412beff3f40000401fe060f92c37141fe0e0f92c7f6167138f606807933f61e0f90c2732c803e800f94c681fa01010000751ebe0d00c0c889350030400056ff1550314000c70504304000a5a5a5a50f0bbedec0adde89350030400056ff15503140000f0b';
const callerOffsets = [0, 19, 28, 33, 40, 54, 84], relocationOffsets = [6, 61, 68, 74, 91, 98];
const branchEdges = [[17, 0x71, 84, 19], [26, 0x71, 84, 28], [31, 0x79, 84, 33], [38, 0x73, 84, 40], [52, 0x75, 84, 54]];
assert.equal(program.length / 2, 104);
for (const [offset, opcode, target, fallthrough] of branchEdges) {const bytes = Buffer.from(program, 'hex'); assert.equal(bytes[offset], opcode); assert.equal(offset + 2, fallthrough); assert.equal(offset + 2 + bytes.readInt8(offset + 1), target); assert.ok(callerOffsets.includes(target) && callerOffsets.includes(fallthrough));}
function literalImage() {
  const bytes = Buffer.alloc(0x1600); bytes.write('MZ'); bytes.writeUInt32LE(0x80, 0x3c); bytes.write('PE\0\0', 0x80);
  for (const [at, value] of [[0x84, 0x14c], [0x86, 3], [0x94, 224], [0x96, 0x102], [0x98, 0x10b], [0xdc, 3], [0xde, 0x100]]) bytes.writeUInt16LE(value, at);
  const optional = [[4, 0x200], [8, 0x1200], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, 0x400000], [32, 4096], [36, 512], [56, 0x6000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16], [104, 0x3100], [108, 40], [136, 0x5000], [140, 20], [192, 0x3150], [196, 8]];
  for (const [at, value] of optional) bytes.writeUInt32LE(value, 0x98 + at);
  for (const [index, name, virtualSize, rva, rawSize, rawPointer, flags] of [[0, '.text', 104, 0x1000, 512, 0x200, 0x60000020], [1, '.data', 4096, 0x3000, 4096, 0x400, 0xc0000040], [2, '.fixups', 20, 0x5000, 512, 0x1400, 0x40000040]]) {
    const at = 0x178 + index * 40; bytes.write(name, at); for (const [offset, value] of [[8, virtualSize], [12, rva], [16, rawSize], [20, rawPointer], [36, flags]]) bytes.writeUInt32LE(value, at + offset);
  }
  bytes.fill(0xcc, 0x200, 0x400); bytes.set(Buffer.from(program, 'hex'), 0x200); for (let index = 0; index < 31; index++) bytes[0x13e0 + index] = index + 1; bytes[0x13ff] = 0x7f;
  bytes.set(words([0x3140, 0, 0, 0x3160, 0x3150]), 0x500); bytes.writeUInt32LE(0x3170, 0x540); bytes.writeUInt32LE(0x3170, 0x550);
  bytes.write('kernel32.dll\0', 0x560); bytes.write('ExitProcess\0', 0x572); bytes.writeUInt32LE(0x1000, 0x1400); bytes.writeUInt32LE(20, 0x1404);
  relocationOffsets.forEach((offset, index) => bytes.writeUInt16LE(0x3000 + offset, 0x1408 + index * 2)); return bytes;
}
const image = readFileSync(join(output, 'unary.exe')); assert.deepEqual(image, literalImage(), 'independent complete PE header/sections/imports/raw page/relocations');
writeFileSync(join(output, 'program.x86'), Buffer.from(program, 'hex'));
function read32(ctx, address, value) {
  const before = arena(ctx), expected = before.slice(); expected.set(record('R3MH', 40, [0, value, 0, 0, 0, 0]), 100);
  assert.equal(ctx.api.read32(address), 0); assert.deepEqual(arena(ctx), expected, 'actual Read32 host observation changes only existing helper'); return value;
}
function assertWords(ctx, address, values) {values.forEach((value, index) => read32(ctx, address + index * 4, value)); ramChecks++;}
for (const base of [0x400000, 0x500000]) for (const owner of ['replacement', 'resident']) {
  const label = `pe-${owner}-${base.toString(16)}`, ctx = fresh(owner), entry = base + 0x1000;
  pure(ctx, () => ctx.api.begin_image_input(image.length), 0, 'begin full copied PE', false);
  for (const [offset, length] of [[0, 4096], [4096, 1536]]) {
    request(ctx, image.subarray(offset, offset + length)); pure(ctx, () => ctx.api.append_image_input(offset, length), 0, 'bounded staging copy', false);
    refresh(ctx).bytes.fill(0xa5, ctx.base + TRANSFER, ctx.base + SIZE);
  }
  const beforeLoad = arena(ctx), receipt = record('R3LI', 72, [base, 0x6000, entry, 5, 0x8000, 1, 0, 0, EXIT, EXIT_ID, 0, 0, 0, 0], 2), expectedLoad = beforeLoad.slice(); expectedLoad.set(receipt, TRANSFER);
  assert.equal(ctx.api.load_pe32_linked_v2_input_at(base, 0x8000), 0); assert.deepEqual(arena(ctx), expectedLoad, 'exact independently reconstructed72byte linked-v2 receipt');
  writeFileSync(join(output, `${label}-receipt.bin`), receipt);
  read32(ctx, base + 0xb4, 0x400000); read32(ctx, base + 0x3150, EXIT); read32(ctx, base + 0x3154, 0);
  const relocationRvas = [0x3fff, 0x3000, 0x3150, 0x3004, 0x3000, 0x3150]; relocationOffsets.forEach((offset, index) => read32(ctx, entry + offset, base + relocationRvas[index]));
  assertWords(ctx, base + 0x3000, Array(8).fill(0));
  for (let index = 0; index < 32; index++) read8(ctx, base + 0x3fe0 + index, index === 31 ? 0x7f : index + 1);
  read8(ctx, base + 0x4000, 0, 1); controls.push({owner, name: `${label}: page after loaded operand is unmapped`, address: base + 0x4000, reason: 1, length: 1});
  const entries = callerOffsets.map(offset => entry + offset), gates = [[EXIT, EXIT_ID]]; if (owner === 'replacement') entries.push(EXIT);
  const caller = compile(ctx, entries, owner === 'replacement' ? gates : [], `${label}-caller`, ['read32', owner === 'replacement' ? 'store32' : 'store_resident32', 'read8', owner === 'replacement' ? 'store8' : 'store_resident8']);
  const gate = owner === 'replacement' ? caller : compile(ctx, [EXIT], gates, `${label}-gate`, []);
  const beforeStart = arena(ctx), expectedStart = beforeStart.slice(); expectedStart.set(stateRecord([0, 0, 0, 0, 0x71000, 0, 0, 0], entry, 2)); expectedStart.set(exitRecord(3, 0, 3), 56);
  assert.equal(ctx.api.start_loaded_image(0x70000, 1), 0); assert.deepEqual(arena(ctx), expectedStart, 'Rust startup owns initial registers and stack');
  assertWords(ctx, 0x70fe0, Array(8).fill(0));
  const finalRegisters = [0x12340100, 0, 0x101, 0x101, 0x70ff8, 0, MARKER, 0], expectedRun = arena(ctx);
  expectedRun.set(stateRecord(finalRegisters, EXIT, 0x46)); expectedRun.set(exitRecord(owner === 'resident' ? 3 : 8, 23, owner === 'resident' ? 2 : 3, owner === 'resident' ? 0 : EXIT_ID), 56); expectedRun.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100);
  assert.equal(guestRun(ctx, caller, ctx.base, ctx.base + 56, 64, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedRun, `${label}:23 genuine guest instructions and full arena`);
  if (owner === 'resident') {const expectedGate = arena(ctx); expectedGate.set(exitRecord(8, 0, 3, EXIT_ID), 56); assert.equal(guestRun(ctx, gate, ctx.base, ctx.base + 56, 1, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedGate, 'separate current resident Exit gate');}
  const frame = record('R3CF', 112, [1, EXIT_ID, 2, 1, EXIT, 0x70ff8, entry + 72, 0, MARKER, ...Array(15).fill(0)]), expectedCapture = arena(ctx); expectedCapture.set(frame, TRANSFER);
  assert.equal(owner === 'replacement' ? ctx.api.capture_call(ctx.low, ctx.high, gate.generation, 2, 1) : ctx.api.capture_resident_call(ctx.low, ctx.high, gate.low, gate.high, 2, 1), 0); assert.deepEqual(arena(ctx), expectedCapture, 'real named CALL argument/return frame only');
  const expectedComplete = arena(ctx); expectedComplete.set(exitRecord(9, 0, 4, MARKER), 56);
  assert.equal(owner === 'replacement' ? ctx.api.complete_windows_call(ctx.low, ctx.high, gate.generation, 1) : ctx.api.complete_resident_windows_call(ctx.low, ctx.high, gate.low, gate.high, 1), 0); assert.deepEqual(arena(ctx), expectedComplete, 'ExitProcess terminal marker preserves committed CPU');
  assertWords(ctx, base + 0x3000, [MARKER, 0, 0, 0, 0, 0, 0, 0]); assertWords(ctx, 0x70fe0, [0, 0, 0, 0, 0, 0, entry + 72, MARKER]);
  for (let index = 0; index < 32; index++) read8(ctx, base + 0x3fe0 + index, index === 31 ? 0x80 : index + 1);
  refresh(ctx); for (const unit of new Set([caller, gate])) assert.deepEqual(ctx.bytes.slice(unit.pointer, unit.pointer + unit.length), unit.bytes, 'retained generated module bytes unchanged');
  pure(ctx, () => guestRun(ctx, caller, 0xffffffff, 0xffffffff, 0, 0xffffffff), 21, `${label}: terminal latch before pointers and budget`);
  peRows.push({owner, base, guest_retired: 23, result: MARKER, final_registers: finalRegisters, final_flags: 0x46, canary: 0, operand_address: base + 0x3fff, operand_value: 0x80, operand_neighbor_hex: Buffer.from(Array.from({length: 31}, (_, index) => index + 1)).toString('hex') + '80', receipt_hex: Buffer.from(receipt).toString('hex'), receipt_sha256: hash(receipt), frame_hex: Buffer.from(frame).toString('hex'), stack_words: [0, 0, 0, 0, 0, 0, entry + 72, MARKER]});
  pure(ctx, () => ctx.api.close(), 0, 'close completed PE', false);
}

const counts = {instances: ordinal, canonical_forms_per_owner: forms.length, alias_forms_per_owner: aliases.length, canonical_cases: rows.filter(row => row.group === 'canonical').length, alias_cases: rows.filter(row => row.group === 'alias').length, finite_guest_retired: rows.length, modules: modules.length, controls: controls.length, live_paths: liveRows.length, live_runs: liveRows.reduce((sum, row) => sum + row.slices.length, 0), live_faults: liveRows.reduce((sum, row) => sum + row.slices.filter(slice => slice.fault).length, 0), live_guest_retired: liveRows.reduce((sum, row) => sum + row.guest_retired, 0), semantic_repair_phases: liveRows.reduce((sum, row) => sum + row.repairs.length, 0), repair_api_calls: liveRows.reduce((sum, row) => sum + row.repairs.reduce((subtotal, phase) => subtotal + phase.actions.length, 0), 0), smc_paths: smcRows.length, smc_guest_retired: smcRows.reduce((sum, row) => sum + row.writer_retired + row.successor_retired, 0), smc_idempotent_paths: smcRows.filter(row => row.idempotent).length, pe_paths: peRows.length, pe_guest_retired: peRows.reduce((sum, row) => sum + row.guest_retired, 0), total_guest_retired: rows.length + 40 + 20 + 92, llvm_encoding_symbols: llvm.length, runtime_sources: sourcePaths.length, generated_function_invocations: generatedRuns.length, maximum_resident_units: residentPeak};
assert.deepEqual(counts, {instances: 24, canonical_forms_per_owner: 4, alias_forms_per_owner: 32, canonical_cases: 224, alias_cases: 128, finite_guest_retired: 352, modules: 38, controls: 96, live_paths: 8, live_runs: 80, live_faults: 16, live_guest_retired: 40, semantic_repair_phases: 16, repair_api_calls: 40, smc_paths: 10, smc_guest_retired: 20, smc_idempotent_paths: 4, pe_paths: 4, pe_guest_retired: 92, total_guest_retired: 504, llvm_encoding_symbols: 4, runtime_sources: 33, generated_function_invocations: 506, maximum_resident_units: 2});
for (const owner of ['replacement', 'resident']) {
  const owned = rows.filter(row => row.owner === owner); assert.equal(owned.length, 176);
  for (const [kind] of kinds) assert.equal(owned.filter(row => row.kind === kind).length, 44);
  for (const flags of flagSeeds) assert.equal(owned.filter(row => row.group === 'canonical' && row.initial_flags === flags).length, 28);
}
const repairCensus = Object.fromEntries(['map', 'upload', 'protect', 'write8'].map(api => [api, liveRows.flatMap(row => row.repairs.flatMap(phase => phase.actions)).filter(action => action.api === api).length]));
assert.deepEqual(repairCensus, {map: 8, upload: 8, protect: 16, write8: 8});
assert.equal(modules.filter(row => row.label.includes('-smc-')).length, 20); assert.equal(modules.filter(row => row.label.startsWith('pe-')).length, 6);
assert.deepEqual(Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), sourceHashes, 'source freeze unchanged during execution'); assert.equal(hash(readFileSync(enginePath)), hash(engineBytes), 'engine freeze unchanged');
const result = {status: 'ok', counts, repair_api_census: repairCensus, ram_checks: ramChecks, engine_sha256: hash(engineBytes), engine_exports: WebAssembly.Module.exports(engineModule), pe_sha256: hash(image), source_sha256: sourceHashes, tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0], llvm: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).split('\n')[0]}, assembly: {command: ['clang', ...assemble], object_sha256: hash(object), disassembly_sha256: hash(Buffer.from(disassembly)), symbols: llvm, scope: 'encoding only; four canonical memory byte unary families, eight bytes'}, oracle: {program, caller_offsets: callerOffsets, branches: branchEdges, relocation_offsets: relocationOffsets, relocation_rvas: [0x3fff, 0x3000, 0x3150, 0x3004, 0x3000, 0x3150], pe_file_bytes: 5632, pe_data_raw_bytes: 4096, pe_chunks: [[0, 4096], [4096, 1536]], pe_return_offset: 72, pe_initial_operand: 127, pe_final_operand: 128, flag_seeds: flagSeeds, boundaries, canonical_hex: canonicalCode.toString('hex'), canonical_instruction_count: 5, alias_hex: aliasCode.toString('hex'), alias_instruction_count: 33, alias_shapes: aliasShapes, live_compile_bytes: 15, live_instruction_count: 5, smc_compile_bytes: 9, smc_successor_pc: 0x9002, maximum_resident_units_per_instance: 2, flag_policy: 'INC/DEC incoming CF preserved; numeric signed8 range, nibble carry/borrow and even parity after successful strict Store8; NEG CF iff input nonzero, OF for input80; NOT full FLAGS unchanged; DF/fixedbit1 retained', pe_final_registers: [0x12340100, 0, 0x101, 0x101, 0x70ff8, 0, MARKER, 0], pe_final_flags: 0x46}, generated_runs: generatedRuns, cases: rows, controls, live: liveRows, smc: smcRows, pe: peRows, modules, claim: 'finite authored memory-byte INC/DEC/NEG/NOT generated by actual engine-Wasm bound replacement/resident compilers with real Read8v2 and strict Store8v3; exact byte and nonzero neighboring RAM; eight genuine data-only read/write fault continuations recompute changed current operands without post-start CPU/FLAGS/EIP patches or prefix replay; ten changed/idempotent consumed-page SMC and fresh successors, plus four copied PE/startup/predicate/SETcc/Jcc/named ExitProcess paths;352 saved expected-outcome rows are backed by live full4236arena and declared32byte RAM assertions, not raw per-row snapshots or unique states; generated Reason6 is the strict status11 completion contract, not a captured direct helper return; LLVM proves encoding only; no standalone binding, general RMW/ISA/EA domain, browser/SDK/performance/P2complete/playable-game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2)); console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
