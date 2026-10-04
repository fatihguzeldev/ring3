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
const SIZE = 4236, TRANSFER = 140, EXIT = 0x8020, EXIT_ID = 0x10003, MARKER = 0xc8c0000c;
const boundaries = [0, 1, 15, 16, 127, 128, 255], flagSeeds = [2, 3, 0xcd6, 0xcd7];
const initialRegisters = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, upload: 2, read8: 1, read32: 1, write8: 2, compile_entries: 2, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store32: 2, store_resident32: 6, begin_image_input: 1, append_image_input: 2, load_pe32_linked_v2_input_at: 2, start_loaded_image: 2, capture_call: 5, capture_resident_call: 6, complete_windows_call: 4, complete_resident_windows_call: 5};
const sourcePaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/control.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/cpu/dbt/wasm/memory/byte_store.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/locals.rs', 'engine/src/process/instance.rs', 'engine/src/process/wasm.rs', 'engine/src/process/call.rs', 'engine/src/process/windows.rs', 'engine/src/abi/wasm/exports.rs', 'engine/src/abi/memory_helper.rs', 'engine/src/loader/mod.rs', 'engine/src/loader/imports.rs', 'engine/src/loader/relocation.rs', 'engine/src/memory/space.rs', 'engine/tests/support/pe32.rs', 'engine/tests/cpu_memory_byte_predicate_wasm.rs', 'engine/tests/fixtures/p2-memory-byte-predicate/run.mjs', 'engine/tests/fixtures/p2-memory-byte-predicate/integer.S'];
assert.equal(sourcePaths.length, 30);
const sourceHashes = Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
writeFileSync(join(output, 'source-hashes.json'), JSON.stringify({engine_sha256: hash(engineBytes), sources: sourceHashes}, null, 2));
writeFileSync(join(output, 'engine.wasm'), engineBytes);
const rows = [], controls = [], liveRows = [], peRows = [], modules = [];
let ordinal = 0, ramChecks = 0;

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
  const instance = new WebAssembly.Instance(engineModule, {}), key = 0xc836700000000000n + BigInt(++ordinal);
  const api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
    const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
  }));
  const ctx = {owner, memory: instance.exports.memory, api, low: Number(key & 0xffffffffn), high: Number(key >> 32n), ramPages: new Map()};
  assert.equal(api.open(pages, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0; return refresh(ctx);
}
function upload(ctx, address, bytes) { request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), 0, 'explicit host upload preserves arena', false); }
function guardCurrent(ctx, unit, label = 'data-only effects retain code owner') {
  const action = ctx.owner === 'replacement' ? () => ctx.api.guard(ctx.low, ctx.high, unit.generation, ctx.base, ctx.base + 56, ctx.base + 96)
    : () => ctx.api.guard_resident(ctx.low, ctx.high, unit.low, unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
  pure(ctx, action, 0, label, false);
  refresh(ctx); assert.deepEqual(ctx.bytes.slice(unit.pointer, unit.pointer + unit.length), unit.bytes, 'current generated module bytes retain their original artifact');
}
function childShape(ctx, bytes, binding, helpers) {
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
      locals = Array.from({length: unsigned()}, () => [unsigned(), bytes[at++]]); assert.deepEqual(locals, [[16, 0x7f], [1, 0x7e], [6, 0x7f]]);
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
  assert.ok(entries.length <= 8); request(ctx, words([...entries, ...gates.flat()])); const before = arena(ctx); let binding, expected = before.slice();
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
  assert.ok(WebAssembly.validate(bytes)); const shape = childShape(ctx, bytes, binding, helpers), module = new WebAssembly.Module(bytes);
  const imports = WebAssembly.Module.imports(module); assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}); assert.equal(instance.exports.run.length, 4);
  writeFileSync(join(output, `${label}.wasm`), bytes); modules.push({label, owner: ctx.owner, entries, ...binding, key: [ctx.low, ctx.high], sha256: hash(bytes), imports, ...shape});
  return {...binding, run: instance.exports.run, label, bytes};
}
function seed(ctx, registers, pc, flags, cancel = 0) {
  refresh(ctx).bytes.set(stateRecord(registers, pc, flags), ctx.base); ctx.bytes.set(exitRecord(3, 0, 1), ctx.base + 56);
  ctx.view.setUint32(ctx.base + 96, cancel, true); ctx.bytes.fill(0xa5, ctx.base + 100, ctx.base + SIZE);
}
function byte(registers, source) {return Math.floor(registers[source % 4] / (source < 4 ? 1 : 256)) % 256;}
function predicateFlags(kind, left, right, oldFlags) {
  let result;
  if (kind === 'cmp') result = (left - right + 256) % 256;
  else {result = 0; for (let place = 1; place <= 128; place *= 2) if (Math.floor(left / place) % 2 && Math.floor(right / place) % 2) result += place;}
  let ones = 0; for (let value = result; value > 0; value = Math.floor(value / 2)) ones += value % 2;
  const signed = value => value < 128 ? value : value - 256, difference = signed(left) - signed(right);
  return Math.floor(oldFlags / 0x400) % 2 * 0x400 + 2 + (ones % 2 === 0 ? 4 : 0) + (result === 0 ? 0x40 : 0) + (result >= 128 ? 0x80 : 0)
    + (kind === 'cmp' && left < right ? 1 : 0) + (kind === 'cmp' && left % 16 < right % 16 ? 0x10 : 0)
    + (kind === 'cmp' && (difference < -128 || difference > 127) ? 0x800 : 0);
}
for (const [kind, left, right, flags] of [['cmp', 0x80, 1, 0x812], ['cmp', 0x7f, 0xff, 0x883], ['cmp', 0, 1, 0x97], ['cmp', 0x80, 0x80, 0x46], ['test', 0x80, 0x7f, 0x46], ['test', 0x80, 0xff, 0x82]]) assert.equal(predicateFlags(kind, left, right, 2), flags);
function read8(ctx, address, expectedValue, reason = 0) {
  const before = arena(ctx), expected = before.slice(); expected.set(helper(expectedValue, reason, reason ? address : 0), 100);
  assert.equal(ctx.api.read8(address), 0); assert.deepEqual(arena(ctx), expected, 'actual host Read8 changes only strict helper record'); return expectedValue;
}
function readDeclared(ctx, address, length) {
  const page = Math.floor(address / 4096) * 4096, expected = ctx.ramPages.get(page); assert.ok(expected && address + length <= page + 4096);
  const bytes = expected.subarray(address - page, address - page + length);
  for (let index = 0; index < length; index++) read8(ctx, address + index, bytes[index]); ramChecks++; return bytes.toString('hex');
}
function setLeft(ctx, address, value) {
  const page = Math.floor(address / 4096) * 4096; pure(ctx, () => ctx.api.protect(page, 1, 3), 0, 'explicit data-input writable phase', false);
  const before = arena(ctx), expected = before.slice(); expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 1], 3), 100);
  assert.equal(ctx.api.write8(address, value), 0); assert.deepEqual(arena(ctx), expected, 'explicit host input Write8 record only'); ctx.ramPages.get(page)[address - page] = value;
  pure(ctx, () => ctx.api.protect(page, 1, 1), 0, 'explicit data-input read-only phase', false);
}
function runPredicate(ctx, unit, form, registers, left, flags, name, group) {
  setLeft(ctx, form.address, left); guardCurrent(ctx, unit); const range = Math.floor(form.address / 32) * 32, ram = readDeclared(ctx, range, 32);
  seed(ctx, registers, form.pc, flags); const expected = arena(ctx), right = form.immediate ?? byte(registers, form.source), expectedFlags = predicateFlags(form.kind, left, right, flags);
  expected.set(stateRecord(registers, form.pc + form.bytes.length, expectedFlags)); expected.set(exitRecord(1, 1), 56); expected.set(helper(left), 100);
  assert.equal(unit.run(ctx.base, ctx.base + 56, 1, ctx.base + 96), 0, name); assert.deepEqual(arena(ctx), expected, `${name}: full GPR/FLAGS/EIP/exit/helper/transfer arena`);
  assert.equal(readDeclared(ctx, range, 32), ram, `${name}: declared operand and neighboring RAM unchanged`); guardCurrent(ctx, unit);
  rows.push({owner: ctx.owner, name, group, kind: form.kind, family: form.immediate === undefined ? 'register' : 'immediate', source: form.source, encoding: Buffer.from(form.bytes).toString('hex'), pc: form.pc, address: form.address, initial_registers: registers, left, right, initial_flags: flags, flags: expectedFlags, retired: 1, next_pc: form.pc + form.bytes.length, declared_ram: {address: range, bytes: 32, expected_hex: ram, expected_sha256: hash(Buffer.from(ram, 'hex'))}});
}
function noRetirement(ctx, unit, pc, cancel) {
  seed(ctx, initialRegisters, pc, 0xcd7, cancel); const expected = arena(ctx); expected.set(exitRecord(cancel ? 2 : 1, 0), 56);
  assert.equal(unit.run(ctx.base, ctx.base + 56, cancel ? 1 : 0, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expected, 'zero/cancel precedes real Read8 and preserves helper');
  controls.push({owner: ctx.owner, name: `${unit.label}:${cancel ? 'positive_cancel_before_read' : 'zero_before_read'}`, retired: 0, budget: cancel ? 1 : 0});
}

const forms = []; let pc = 0x8000;
for (const [kind, opcode, immediateOpcode, extension] of [['cmp', 0x38, 0x80, 7], ['test', 0x84, 0xf6, 0]]) {
  for (let source = 0; source < 8; source++) {const bytes = [opcode, source * 8 + 6]; forms.push({kind, source, bytes, pc, address: 0x3fff}); pc += bytes.length;}
  for (const immediate of [0, 0x7f, 0x80, 0xff]) {const bytes = [immediateOpcode, extension * 8 + 6, immediate]; forms.push({kind, immediate, bytes, pc, address: 0x3fff}); pc += bytes.length;}
}
assert.equal(forms.length, 24);
const canonicalCode = Buffer.from([...forms.flatMap(form => form.bytes), 0xeb, 0, 0x0f, 0x0b]); assert.equal(canonicalCode.length, 60); assert.deepEqual(canonicalCode, readFileSync(join(output, 'canonical.x86')));
const aliasShapes = [
  {source: 0, parent: 0, value: 0x3001, bytes: [0x00], address: 0x3001},
  {source: 4, parent: 0, value: 0x3070, bytes: [0x60, 0x20], address: 0x3090},
  {source: 1, parent: 1, value: 0x3042, bytes: [0x49, 0xf0], address: 0x3032},
  {source: 5, parent: 1, value: 0x3043, bytes: [0xa9, 0, 1, 0, 0], address: 0x3143},
  {source: 2, parent: 2, value: 0x3054, index: 7, index_value: 4, bytes: [0x54, 0x7a, 0x10], address: 0x306c},
  {source: 6, parent: 2, value: 0x3055, index: 7, index_value: 3, bytes: [0x74, 0xba, 0xf8], address: 0x3059},
  {source: 3, parent: 3, value: 0x3066, index: 6, index_value: 2, bytes: [0x9c, 0xf3, 0xe0, 0xff, 0xff, 0xff], address: 0x3056},
  {source: 7, parent: 3, value: 0x3067, index: 6, index_value: 6, bytes: [0x3c, 0x33], address: 0x306d},
];
const aliases = []; pc = 0x8200;
for (const [kind, opcode] of [['cmp', 0x38], ['test', 0x84]]) for (const shape of aliasShapes) {const bytes = [opcode, ...shape.bytes]; aliases.push({...shape, kind, bytes, pc}); pc += bytes.length;}
const aliasCode = Buffer.from([...aliases.flatMap(form => form.bytes), 0xeb, 0, 0x0f, 0x0b]); assert.equal(aliases.length, 16); assert.deepEqual(aliasCode, readFileSync(join(output, 'aliases.x86')));
const liveCode = Buffer.from('bf00400000f607000f94c038260f90c33845000f90c784444a110f94c2eb000f0b', 'hex'); assert.equal(liveCode.length, 33); assert.deepEqual(liveCode, readFileSync(join(output, 'live.x86')));
function live(ctx, unit) {
  const registers = [...initialRegisters]; registers[0] = 0x1234017f; registers[1] = 0x1800; registers[2] = 0xfffffff0; registers[3] = 0x45670000; registers[5] = 0xffffffff; registers[6] = 0x5000;
  seed(ctx, registers, 0x9000, 0xcd7); const initial = [...registers], slices = [];
  function slice(name, budget, retired, offset, flags, changes = {}, read, fault, cancel = 0) {
    refresh(ctx).view.setUint32(ctx.base + 96, cancel, true); const expected = arena(ctx);
    for (const [index, value] of Object.entries(changes)) registers[Number(index)] = value;
    expected.set(stateRecord(registers, 0x9000 + offset, flags)); expected.set(fault ? exitRecord(5, retired, 2, fault.reason, fault.address, 1, 1) : exitRecord(cancel ? 2 : 1, retired), 56);
    if (read !== undefined || fault) expected.set(helper(read ?? 0, fault?.reason ?? 0, fault?.address ?? 0), 100);
    assert.equal(unit.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0, name); assert.deepEqual(arena(ctx), expected, `${name}: genuine continuation and full arena`);
    const row = {name, budget, retired, pc: 0x9000 + offset, flags, registers: [...registers], cancel, read, fault}; slices.push(row);
    if (!retired && !fault) controls.push({owner: ctx.owner, name: `${unit.label}:${name}`, budget, retired: 0});
  }
  slice('unmapped_zero_before_prefix_and_read', 0, 0, 0, 0xcd7); slice('unmapped_cancel_before_prefix_and_read', 1, 0, 0, 0xcd7, {}, undefined, undefined, 1);
  slice('mov_prefix_retires_then_test_immediate_zero_faults', 64, 1, 5, 0xcd7, {7: 0x4000}, undefined, {reason: 1, address: 0x4000});
  pure(ctx, () => ctx.api.map(0x4000, 1, 3), 0, 'data-only zero mapping repair', false); ctx.ramPages.set(0x4000, Buffer.alloc(4096)); guardCurrent(ctx, unit);
  slice('same_unit_test_retry_no_cpu_patch', 1, 1, 8, 0x446, {}, 0);
  slice('test_zero_before_setz', 0, 0, 8, 0x446); slice('test_cancel_before_setz', 1, 0, 8, 0x446, {}, undefined, undefined, 1);
  slice('setz_consumes_test_zero', 1, 1, 11, 0x446, {0: 0x12340101});
  slice('cmp_write_only_precise_fault', 64, 0, 11, 0x446, {}, undefined, {reason: 2, address: 0x5000});
  pure(ctx, () => ctx.api.protect(0x5000, 1, 1), 0, 'data-only permission repair', false); guardCurrent(ctx, unit);
  slice('same_unit_cmp_retry_no_cpu_patch', 1, 1, 13, 0xc12, {}, 0x80);
  slice('cmp_zero_before_seto', 0, 0, 13, 0xc12); slice('cmp_cancel_before_seto', 1, 0, 13, 0xc12, {}, undefined, undefined, 1);
  slice('seto_consumes_cmp_overflow', 1, 1, 16, 0xc12, {3: 0x45670001});
  slice('max_address_exact_one_byte', 1, 1, 19, 0xc12, {}, 0x80);
  slice('seto_consumes_max_compare', 1, 1, 22, 0xc12, {3: 0x45670101});
  slice('wrapping_ea_read_original_index', 1, 1, 26, 0x446, {}, 0x80);
  slice('setz_consumes_wrapping_test', 1, 1, 29, 0x446, {2: 0xffffff01}); slice('jmp_finishes', 1, 1, 31, 0x446);
  assert.equal(slices.length, 17); assert.equal(slices.reduce((sum, row) => sum + row.retired, 0), 10);
  for (const address of [0x3000, 0x3fe0, 0x4000, 0x5000, 0xffffffe0]) readDeclared(ctx, address, 32); guardCurrent(ctx, unit);
  liveRows.push({owner: ctx.owner, initial_registers: initial, initial_pc: 0x9000, initial_flags: 0xcd7, guest_retired: 10, slices, repairs: [{kind: 'map', address: 0x4000, permissions: 3}, {kind: 'protect', address: 0x5000, permissions: 1}], claim: 'single initial CPU seed; subsequent host inputs are cancelword and data-only repair, with no CPU/FLAGS/EIP patch or prefix replay'});
}
for (const owner of ['replacement', 'resident']) {
  const ctx = fresh(owner);
  for (const address of [0x8000, 0x9000]) pure(ctx, () => ctx.api.map(address, 1, 7), 0, 'map authored code', false);
  for (const address of [0x3000, 0x5000, 0xfffff000]) pure(ctx, () => ctx.api.map(address, 1, 3), 0, 'map declared finite data', false);
  const data = Buffer.alloc(4096, 0x5a); for (const form of aliases) data[form.address - 0x3000] = 0x80; data[1] = 0x80; data[4095] = 0x80;
  const denied = Buffer.alloc(4096, 0x5a), endpoint = Buffer.alloc(4096, 0x5a); denied[0] = 0x80; endpoint[4095] = 0x80;
  for (const [address, bytes] of [[0x3000, data], [0x5000, denied], [0xfffff000, endpoint]]) {upload(ctx, address, bytes); ctx.ramPages.set(address, Buffer.from(bytes));}
  upload(ctx, 0x8000, canonicalCode); upload(ctx, 0x8200, aliasCode); upload(ctx, 0x9000, liveCode);
  for (const [address, permissions] of [[0x3000, 1], [0x5000, 2], [0xfffff000, 1]]) pure(ctx, () => ctx.api.protect(address, 1, permissions), 0, 'protect finite data before compilation', false);
  const canonical = compile(ctx, [0x8000], [], `${owner}-canonical`);
  for (const form of forms) for (const flags of flagSeeds) {const registers = [...initialRegisters]; registers[6] = 0x3fff; runPredicate(ctx, canonical, form, registers, 0x80, flags, `${owner}-canonical-${form.kind}-${form.source ?? form.immediate}-flags${flags}`, 'canonical');}
  for (const form of forms.filter(form => form.source === 0)) for (const left of boundaries) for (const right of boundaries) {
    const registers = [...initialRegisters]; registers[0] += right - byte(registers, 0); registers[6] = 0x3fff;
    runPredicate(ctx, canonical, form, registers, left, 0xcd7, `${owner}-${form.kind}-boundary${left}-${right}`, 'boundary');
  }
  noRetirement(ctx, canonical, 0x8000, 0); noRetirement(ctx, canonical, 0x8000, 1);
  const alias = compile(ctx, [0x8200], [], `${owner}-aliases`);
  for (const form of aliases) for (const flags of [2, 0xcd7]) {const registers = [...initialRegisters]; registers[form.parent] = form.value; if (form.index !== undefined) registers[form.index] = form.index_value; runPredicate(ctx, alias, form, registers, 0x80, flags, `${owner}-alias-${form.kind}-${form.source}-flags${flags}`, 'alias');}
  noRetirement(ctx, alias, 0x8200, 0); noRetirement(ctx, alias, 0x8200, 1);
  read8(ctx, 0x4000, 0, 1); controls.push({owner, name: 'page_after_readonly_endpoint_is_unmapped', address: 0x4000, reason: 1, length: 1});
  const unit = compile(ctx, [0x9000], [], `${owner}-live`);
  const guard = (low, high, identityLow, identityHigh, pointer = ctx.base) => owner === 'replacement' ? ctx.api.guard(low, high, identityLow, pointer, ctx.base + 56, ctx.base + 96) : ctx.api.guard_resident(low, high, identityLow, identityHigh, pointer, ctx.base + 56, ctx.base + 96);
  const identity = owner === 'replacement' ? unit.generation : unit.low;
  pure(ctx, () => guard(ctx.low + 1, ctx.high, identity, unit.high), 3, `${owner}: wrong key low`);
  pure(ctx, () => guard(ctx.low, ctx.high + 1, identity, unit.high), 3, `${owner}: wrong key high`);
  pure(ctx, () => guard(ctx.low, ctx.high, identity + 1, unit.high), 3, `${owner}: wrong generation or id low`);
  pure(ctx, () => guard(ctx.low, ctx.high, owner === 'replacement' ? 0 : identity, owner === 'replacement' ? undefined : unit.high + 1), 3, `${owner}: zero generation or wrong id high`);
  pure(ctx, () => guard(ctx.low, ctx.high, identity, unit.high, 0xffffffff), 1, `${owner}: current guard malformed pointer`);
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 1, 0xffffffff), 1, `${owner}: current generated malformed pointers`);
  live(ctx, unit);
  const beforeStale = arena(ctx), expectedStale = beforeStale.slice(); expectedStale.set(record('R3MH', 40, [0, 0, 0, 0, 0, 1], 3), 100);
  assert.equal(ctx.api.write8(0x9000, liveCode[0]), 0); assert.deepEqual(arena(ctx), expectedStale, 'declared same-byte code write publishes only store helper');
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 4, `${owner}: stale before malformed pointers`);
  pure(ctx, () => ctx.api.close(), 0, 'close preserves arena', false); pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 5, `${owner}: Closed before stale and malformed pointers`);
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
const assemblyPath = join(root, 'engine/tests/fixtures/p2-memory-byte-predicate/integer.S'), objectPath = join(output, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assemblyPath, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(output, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), symbols = elfFixtures(object), llvm = [];
const encodingSamples = [['cmp_register', '3826'], ['cmp_immediate', '803e80'], ['test_register', '8406'], ['test_immediate', 'f60680']];
let llvmOffset = 0;
for (const [name, hex] of encodingSamples) {
  const symbol = symbols.get(name); assert.ok(symbol, name); assert.equal(symbol.offset, llvmOffset); assert.equal(symbol.bytes.toString('hex'), hex);
  llvm.push({name, offset: llvmOffset, hex}); llvmOffset += hex.length / 2;
  const start = disassembly.indexOf(`<${name}>:`); assert.ok(start >= 0);
  const next = disassembly.indexOf('\n\n', start), body = disassembly.slice(start, next < 0 ? undefined : next);
  assert.match(body, new RegExp(`\\b${name.split('_')[0]}\\s+`), `${name}: LLVM decodes the canonical family`);
}
assert.equal(symbols.size, 4); assert.equal(llvmOffset, 10);

const program = 'b87f013412beff3f40003826713a803e80753584060f94c3752ef606800f98c7792681fb01010000751ebe0c00c0c889350030400056ff1550314000c70504304000a5a5a5a50f0bbedec0adde89350030400056ff15503140000f0b';
const callerOffsets = [0, 14, 19, 26, 34, 42, 72], relocationOffsets = [6, 49, 56, 62, 79, 86];
const branchEdges = [[12, 0x71, 72, 14], [17, 0x75, 72, 19], [24, 0x75, 72, 26], [32, 0x79, 72, 34], [40, 0x75, 72, 42]];
assert.equal(program.length / 2, 92);
for (const [offset, opcode, target, fallthrough] of branchEdges) {const bytes = Buffer.from(program, 'hex'); assert.equal(bytes[offset], opcode); assert.equal(offset + 2, fallthrough); assert.equal(offset + 2 + bytes.readInt8(offset + 1), target); assert.ok(callerOffsets.includes(target) && callerOffsets.includes(fallthrough));}
function literalImage() {
  const bytes = Buffer.alloc(0x1600); bytes.write('MZ'); bytes.writeUInt32LE(0x80, 0x3c); bytes.write('PE\0\0', 0x80);
  for (const [at, value] of [[0x84, 0x14c], [0x86, 3], [0x94, 224], [0x96, 0x102], [0x98, 0x10b], [0xdc, 3], [0xde, 0x100]]) bytes.writeUInt16LE(value, at);
  const optional = [[4, 0x200], [8, 0x1200], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, 0x400000], [32, 4096], [36, 512], [56, 0x6000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16], [104, 0x3100], [108, 40], [136, 0x5000], [140, 20], [192, 0x3150], [196, 8]];
  for (const [at, value] of optional) bytes.writeUInt32LE(value, 0x98 + at);
  for (const [index, name, virtualSize, rva, rawSize, rawPointer, flags] of [[0, '.text', 92, 0x1000, 512, 0x200, 0x60000020], [1, '.data', 4096, 0x3000, 4096, 0x400, 0xc0000040], [2, '.fixups', 20, 0x5000, 512, 0x1400, 0x40000040]]) {
    const at = 0x178 + index * 40; bytes.write(name, at); for (const [offset, value] of [[8, virtualSize], [12, rva], [16, rawSize], [20, rawPointer], [36, flags]]) bytes.writeUInt32LE(value, at + offset);
  }
  bytes.fill(0xcc, 0x200, 0x400); bytes.set(Buffer.from(program, 'hex'), 0x200); bytes[0x13ff] = 0x80;
  bytes.set(words([0x3140, 0, 0, 0x3160, 0x3150]), 0x500); bytes.writeUInt32LE(0x3170, 0x540); bytes.writeUInt32LE(0x3170, 0x550);
  bytes.write('kernel32.dll\0', 0x560); bytes.write('ExitProcess\0', 0x572); bytes.writeUInt32LE(0x1000, 0x1400); bytes.writeUInt32LE(20, 0x1404);
  relocationOffsets.forEach((offset, index) => bytes.writeUInt16LE(0x3000 + offset, 0x1408 + index * 2)); return bytes;
}
const image = readFileSync(join(output, 'predicate.exe')); assert.deepEqual(image, literalImage(), 'independent complete PE header/sections/imports/raw page/relocations');
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
  for (let index = 0; index < 32; index++) read8(ctx, base + 0x3fe0 + index, index === 31 ? 0x80 : 0);
  read8(ctx, base + 0x4000, 0, 1); controls.push({owner, name: `${label}: page after loaded operand is unmapped`, address: base + 0x4000, reason: 1, length: 1});
  const entries = callerOffsets.map(offset => entry + offset), gates = [[EXIT, EXIT_ID]]; if (owner === 'replacement') entries.push(EXIT);
  const caller = compile(ctx, entries, owner === 'replacement' ? gates : [], `${label}-caller`, ['read32', owner === 'replacement' ? 'store32' : 'store_resident32', 'read8']);
  const gate = owner === 'replacement' ? caller : compile(ctx, [EXIT], gates, `${label}-gate`, []);
  const beforeStart = arena(ctx), expectedStart = beforeStart.slice(); expectedStart.set(stateRecord([0, 0, 0, 0, 0x71000, 0, 0, 0], entry, 2)); expectedStart.set(exitRecord(3, 0, 3), 56);
  assert.equal(ctx.api.start_loaded_image(0x70000, 1), 0); assert.deepEqual(arena(ctx), expectedStart, 'Rust startup owns initial registers and stack');
  assertWords(ctx, 0x70fe0, Array(8).fill(0));
  const finalRegisters = [0x1234017f, 0, 0, 0x101, 0x70ff8, 0, MARKER, 0], expectedRun = arena(ctx);
  expectedRun.set(stateRecord(finalRegisters, EXIT, 0x46)); expectedRun.set(exitRecord(owner === 'resident' ? 3 : 8, 18, owner === 'resident' ? 2 : 3, owner === 'resident' ? 0 : EXIT_ID), 56); expectedRun.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100);
  assert.equal(caller.run(ctx.base, ctx.base + 56, 64, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedRun, `${label}:18 genuine guest instructions and full arena`);
  if (owner === 'resident') {const expectedGate = arena(ctx); expectedGate.set(exitRecord(8, 0, 3, EXIT_ID), 56); assert.equal(gate.run(ctx.base, ctx.base + 56, 1, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedGate, 'separate current resident Exit gate');}
  const frame = record('R3CF', 112, [1, EXIT_ID, 2, 1, EXIT, 0x70ff8, entry + 60, 0, MARKER, ...Array(15).fill(0)]), expectedCapture = arena(ctx); expectedCapture.set(frame, TRANSFER);
  assert.equal(owner === 'replacement' ? ctx.api.capture_call(ctx.low, ctx.high, gate.generation, 2, 1) : ctx.api.capture_resident_call(ctx.low, ctx.high, gate.low, gate.high, 2, 1), 0); assert.deepEqual(arena(ctx), expectedCapture, 'real named CALL argument/return frame only');
  const expectedComplete = arena(ctx); expectedComplete.set(exitRecord(9, 0, 4, MARKER), 56);
  assert.equal(owner === 'replacement' ? ctx.api.complete_windows_call(ctx.low, ctx.high, gate.generation, 1) : ctx.api.complete_resident_windows_call(ctx.low, ctx.high, gate.low, gate.high, 1), 0); assert.deepEqual(arena(ctx), expectedComplete, 'ExitProcess terminal marker preserves committed CPU');
  assertWords(ctx, base + 0x3000, [MARKER, 0, 0, 0, 0, 0, 0, 0]); assertWords(ctx, 0x70fe0, [0, 0, 0, 0, 0, 0, entry + 60, MARKER]);
  for (let index = 0; index < 32; index++) read8(ctx, base + 0x3fe0 + index, index === 31 ? 0x80 : 0);
  refresh(ctx); for (const unit of new Set([caller, gate])) assert.deepEqual(ctx.bytes.slice(unit.pointer, unit.pointer + unit.length), unit.bytes, 'retained generated module bytes unchanged');
  pure(ctx, () => caller.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 21, `${label}: terminal latch before pointers and budget`);
  peRows.push({owner, base, guest_retired: 18, result: MARKER, final_registers: finalRegisters, final_flags: 0x46, canary: 0, operand_address: base + 0x3fff, operand_value: 0x80, operand_neighbor_hex: '00'.repeat(31) + '80', receipt_hex: Buffer.from(receipt).toString('hex'), receipt_sha256: hash(receipt), frame_hex: Buffer.from(frame).toString('hex'), stack_words: [0, 0, 0, 0, 0, 0, entry + 60, MARKER]});
  pure(ctx, () => ctx.api.close(), 0, 'close completed PE', false);
}
const counts = {instances: ordinal, canonical_forms_per_owner: forms.length, canonical_cases: rows.filter(row => row.group === 'canonical').length, boundary_cases: rows.filter(row => row.group === 'boundary').length, alias_cases: rows.filter(row => row.group === 'alias').length, finite_guest_retired: rows.length, non_pe_modules: modules.filter(row => !row.label.startsWith('pe-')).length, modules: modules.length, controls: controls.length, live_paths: liveRows.length, live_runs: liveRows.reduce((sum, row) => sum + row.slices.length, 0), live_faults: liveRows.reduce((sum, row) => sum + row.slices.filter(slice => slice.fault).length, 0), live_guest_retired: liveRows.reduce((sum, row) => sum + row.guest_retired, 0), pe_paths: peRows.length, pe_guest_retired: peRows.reduce((sum, row) => sum + row.guest_retired, 0), llvm_encoding_symbols: llvm.length};
assert.deepEqual(counts, {instances: 6, canonical_forms_per_owner: 24, canonical_cases: 192, boundary_cases: 196, alias_cases: 64, finite_guest_retired: 452, non_pe_modules: 6, modules: 12, controls: 46, live_paths: 2, live_runs: 34, live_faults: 4, live_guest_retired: 20, pe_paths: 4, pe_guest_retired: 72, llvm_encoding_symbols: 4});
for (const owner of ['replacement', 'resident']) {
  const owned = rows.filter(row => row.owner === owner); assert.equal(owned.length, 226);
  for (const kind of ['cmp', 'test']) assert.equal(owned.filter(row => row.kind === kind).length, 113);
  for (const flags of flagSeeds) assert.equal(owned.filter(row => row.group === 'canonical' && row.initial_flags === flags).length, 24);
}
assert.deepEqual(Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), sourceHashes, 'source freeze unchanged during execution'); assert.equal(hash(readFileSync(enginePath)), hash(engineBytes), 'engine freeze unchanged');
const result = {status: 'ok', counts, ram_checks: ramChecks, engine_sha256: hash(engineBytes), engine_exports: WebAssembly.Module.exports(engineModule), pe_sha256: hash(image), source_sha256: sourceHashes, tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0], llvm: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).split('\n')[0]}, assembly: {command: ['clang', ...assemble], object_sha256: hash(object), disassembly_sha256: hash(Buffer.from(disassembly)), symbols: llvm, scope: 'encoding only; four canonical memory-left byte predicate families'}, oracle: {program, caller_offsets: callerOffsets, branches: branchEdges, relocation_offsets: relocationOffsets, relocation_rvas: [0x3fff, 0x3000, 0x3150, 0x3004, 0x3000, 0x3150], pe_file_bytes: 5632, pe_data_raw_bytes: 4096, pe_chunks: [[0, 4096], [4096, 1536]], flag_seeds: flagSeeds, boundaries, canonical_hex: canonicalCode.toString('hex'), alias_hex: aliasCode.toString('hex'), alias_shapes: aliasShapes, live_hex: liveCode.toString('hex'), live_pc: 0x9000, live_compile_bytes: 31, live_initial_registers: liveRows[0].initial_registers, live_flags: [0xcd7, 0x446, 0xc12, 0x446], flag_policy: 'CMP unsigned borrow/nibble borrow and signed8 subtraction range; TEST numeric per-bit AND with deterministic AF0; both preserve DF/fixed bit1 value2 and overwrite incoming CF; one real strict R3MHv2 Read8 before flags/retirement and no operand write', pe_final_registers: [0x1234017f, 0, 0, 0x101, 0x70ff8, 0, MARKER, 0], pe_final_flags: 0x46}, cases: rows, controls, live: liveRows, pe: peRows, modules, claim: 'finite authored memory-left CMP8/TEST8 in engine-Wasm-generated replacement/resident modules with real Read8, original EA/source-parent alias capture, read-only/page-last/MAX/wrapping access, precise unmapped and write-only faults including TEST immediate0, genuine data-only repair and zero/cancel/SETcc continuation without post-start CPU edits, plus four copied PE startup/predicate/SETcc/branch/CMP32/named ExitProcess paths;452 counts expected-outcome rows backed by live full-arena and declared32byte RAM assertions, not saved full-state snapshots or unique states; LLVM proves encoding only; no standalone memory binding, memory writes/RMW, width16/3A memory RHS/alternate encoding, SDK/browser/performance/general ISA/playable game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2)); console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
