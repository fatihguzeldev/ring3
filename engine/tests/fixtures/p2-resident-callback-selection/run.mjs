import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root);
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath);
const SIZE = 4236, TRANSFER = 140, KEY = 0xd3456789e0000000n;
const OUTER = 0x80000011, RETURN = 0x80000012;
const sourcePaths = [
  'Cargo.lock',
  'Cargo.toml',
  'engine/Cargo.toml',
  'engine/src/abi/arena.rs',
  'engine/src/abi/call_frame.rs',
  'engine/src/abi/callback.rs',
  'engine/src/abi/header.rs',
  'engine/src/abi/memory_helper.rs',
  'engine/src/abi/mod.rs',
  'engine/src/abi/resident_callback.rs',
  'engine/src/abi/wasm/exports.rs',
  'engine/src/abi/wasm/mod.rs',
  'engine/src/abi/wasm/tests/run.mjs',
  'engine/src/abi/x86/exit.rs',
  'engine/src/abi/x86/mod.rs',
  'engine/src/abi/x86/state.rs',
  'engine/src/cpu/dbt/artifact.rs',
  'engine/src/cpu/dbt/cold.rs',
  'engine/src/cpu/dbt/cold_tests.rs',
  'engine/src/cpu/dbt/gate.rs',
  'engine/src/cpu/dbt/mod.rs',
  'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/resident.rs',
  'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/dbt/wasm/control.rs',
  'engine/src/cpu/dbt/wasm/dispatcher.rs',
  'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/cpu/dbt/wasm/fixtures/baseline.S',
  'engine/src/cpu/dbt/wasm/fixtures/baseline_oracle.c',
  'engine/src/cpu/dbt/wasm/integer.rs',
  'engine/src/cpu/dbt/wasm/locals.rs',
  'engine/src/cpu/dbt/wasm/memory.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs',
  'engine/src/cpu/dbt/wasm/mod.rs',
  'engine/src/cpu/dbt/wasm/tests/run.mjs',
  'engine/src/cpu/exit.rs',
  'engine/src/cpu/mod.rs',
  'engine/src/cpu/x86/decode/decoder.rs',
  'engine/src/cpu/x86/decode/fixtures/integer.S',
  'engine/src/cpu/x86/decode/flow.rs',
  'engine/src/cpu/x86/decode/integer.rs',
  'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/decode/mod.rs',
  'engine/src/cpu/x86/decode/operands.rs',
  'engine/src/cpu/x86/decode/profile.rs',
  'engine/src/cpu/x86/ir.rs',
  'engine/src/cpu/x86/mod.rs',
  'engine/src/cpu/x86/state.rs',
  'engine/src/lib.rs',
  'engine/src/memory/address.rs',
  'engine/src/memory/code.rs',
  'engine/src/memory/mod.rs',
  'engine/src/memory/space.rs',
  'engine/src/process/call.rs',
  'engine/src/process/callback.rs',
  'engine/src/process/callback_code.rs',
  'engine/src/process/callback_code_tests.rs',
  'engine/src/process/installation.rs',
  'engine/src/process/instance.rs',
  'engine/src/process/mod.rs',
  'engine/src/process/resident.rs',
  'engine/src/process/resident_callback.rs',
  'engine/src/process/wasm.rs',
  'engine/src/windows/callback_frame.rs',
  'engine/src/windows/calling_convention.rs',
  'engine/src/windows/mod.rs',
  'engine/tests/abi_callback.rs',
  'engine/tests/abi_resident_callback.rs',
  'engine/tests/abi_resident_callback_result.rs',
  'engine/tests/callback_code_install.rs',
  'engine/tests/callback_code_wasm.rs',
  'engine/tests/callbacks_wasm.rs',
  'engine/tests/fixtures/p2-callback-code/integer.S',
  'engine/tests/fixtures/p2-callback-code/run.mjs',
  'engine/tests/fixtures/p2-callbacks/integer.S',
  'engine/tests/fixtures/p2-callbacks/run.mjs',
  'engine/tests/fixtures/p2-resident-callback-call/run.mjs',
  'engine/tests/fixtures/p2-resident-callback-execution/run.mjs',
  'engine/tests/fixtures/p2-resident-callback-finish/run.mjs',
  'engine/tests/fixtures/p2-resident-callback/run.mjs',
  'engine/tests/fixtures/p2-returning-dispatcher/run.mjs',
  'engine/tests/process_callbacks.rs',
  'engine/tests/process_resident_callback.rs',
  'engine/tests/process_resident_callback_call.rs',
  'engine/tests/process_resident_callback_call_wasm.rs',
  'engine/tests/process_resident_callback_execution.rs',
  'engine/tests/process_resident_callback_execution_wasm.rs',
  'engine/tests/process_resident_callback_finish.rs',
  'engine/tests/process_resident_callback_finish_wasm.rs',
  'engine/tests/process_resident_callback_wasm.rs',
  'engine/tests/process_returning_dispatcher.rs',
  'engine/tests/process_returning_dispatcher_wasm.rs',
  'engine/tests/process_resident_callback_selection.rs',
  'engine/tests/process_resident_callback_selection_wasm.rs',
  'engine/tests/fixtures/p2-resident-callback-selection/run.mjs',
];
const productionPaths = sourcePaths.filter(path => path.startsWith('engine/src/'));
assert.equal(sourcePaths.length, 95);
assert.equal(new Set(sourcePaths).size, 95);
assert.equal(productionPaths.length, 63);
const sourceBytes = Object.fromEntries(sourcePaths.map(path => [path, readFileSync(join(root, path))]));
for (const [path, bytes] of Object.entries(sourceBytes)) {
  const destination = join(outputDir, 'sources', path);
  mkdirSync(dirname(destination), {recursive: true});
  writeFileSync(destination, bytes);
}
writeFileSync(join(outputDir, 'executed-engine.wasm'), engineBytes);
// the complete source union is preserved before any module is created.
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const names = [
  'open', 'close', 'arena_ptr', 'map', 'upload', 'compile_resident_with_gates',
  'dispatcher_module', 'guard_dispatch_entry', 'guard_resident', 'read32', 'write32',
  'store_resident32', 'acknowledge_resident_installation', 'find_installed_resident', 'generation',
  'capture_resident_call', 'complete_resident_call', 'begin_resident_callback',
  'authorize_resident_callback', 'finish_resident_callback', 'select_resident_callback_unit',
  'capture_resident_callback_call', 'complete_resident_callback_call',
];
const fields = [
  'engines', 'resident_units', 'metadata24', 'metadata32', 'acknowledgements',
  'selectors', 'selectors_success', 'selectors_rejected', 'dispatch_calls',
  'dispatch_canonical', 'dispatch_rejected', 'dispatch_children', 'lookup_success',
  'lookup_failure', 'guard_checks', 'api_rejections', 'resident_captures',
  'resident_begins', 'authorizations', 'finishes', 'result48', 'completions',
  'host_reads', 'host_writes', 'arena_checks', 'callback_retired', 'guest_stores',
  'frame_words', 'closed',
];
const stats = Object.fromEntries(fields.map(field => [field, 0]));
const artifacts = {}, authored = {}, records = [], bindings = [], references = [], executions = [];
const selections = [], corruptions = [], ramObservations = [];
function words(size, magic, values, version = 1) {
  const bytes = new Uint8Array(size), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true);
  view.setUint16(6, 1, true); view.setUint32(8, size, true);
  values.forEach((value, index) => view.setUint32(16 + index * 4, value, true));
  return bytes;
}
function cpu(pc, esp, eax = 10) {
  return {registers: [eax, 0x11111111, 0x22222222, 0x33333333, esp, 0x55555555, 0x66666666, 0x77777777], eip: pc, eflags: 0xcd7};
}
const state56 = value => words(56, 'R3ST', [...value.registers, value.eip, value.eflags]);
const exit40 = (reason, retired = 0, detail = 0, version = 3) => words(40, 'R3EX', [reason, retired, detail, 0, 0, 0], version);
const helper40 = (value = 0) => words(40, 'R3MH', [0, value, 0, 0, 0, 0]);
function refresh(e) {
  if (e.buffer !== e.memory.buffer) {
    e.buffer = e.memory.buffer; e.bytes = new Uint8Array(e.buffer); e.view = new DataView(e.buffer);
  }
  return e;
}
const arena = e => refresh(e).bytes.slice(e.base, e.base + SIZE);
function check(e, expected, label) {
  assert.deepEqual(arena(e), expected, `${label}: complete4236arena`); stats.arena_checks++;
}
function record(kind, bytes, extra = {}) {
  records.push({key: KEY.toString(), kind, byte_length: bytes.length, hex: Buffer.from(bytes).toString('hex'), sha256: sha256(bytes), ...extra});
}
function artifact(name, bytes) {
  writeFileSync(join(outputDir, name), bytes); artifacts[name] = sha256(bytes);
}
function pure(e, action, expectedStatus, label) {
  const before = arena(e); assert.equal(action(), expectedStatus, label); check(e, before, label);
}
function reject(e, action, status, label) {
  pure(e, action, status, label); stats.api_rejections++;
}
function select(e, home, token, target, status, label, keyHigh = e.high, homeHigh = home.high, targetHigh = target.high) {
  pure(e, () => e.api.select_resident_callback_unit(e.low, keyHigh, home.low, homeHigh, token, target.low, targetHigh), status, label);
  selections.push({home: home.id.toString(), token, target: target.id.toString(), status, label, key_high: keyHigh >>> 0, home_high: homeHigh >>> 0, target_high: targetHigh >>> 0});
  stats.selectors++; stats[status ? 'selectors_rejected' : 'selectors_success']++;
}
function tamper(e, offset, changed, action, label) {
  const original = arena(e);
  refresh(e).bytes.set(changed, e.base + offset);
  action();
  e.bytes.set(original, e.base); check(e, original, `${label}: restore exact negative input`);
  corruptions.push({offset, hex: Buffer.from(changed).toString('hex'), label, restored: true});
}
function guard(e, unit, status) {
  pure(e, () => e.api.guard_resident(e.low, e.high, unit.low, unit.high, e.base, e.base + 56, e.base + 96), status, `named guard ${unit.label}`);
  stats.guard_checks++;
}
function hostWord(e, address, value, read = false) {
  const before = arena(e); assert.equal(read ? e.api.read32(address) : e.api.write32(address, value), 0);
  const expected = before.slice(); expected.set(helper40(read ? value : 0), 100);
  check(e, expected, read ? 'bounded RAM read' : 'RAM setup'); stats[read ? 'host_reads' : 'host_writes']++;
}
function observe(e, callbackReturn, callerReturn, label) {
  const values = [[0x8ff4, 0x11223344], [0x8ff8, callbackReturn], [0x8ffc, callerReturn], [0x9000, 0x99aabbcc]];
  for (const [address, value] of values) hostWord(e, address, value, true);
  ramObservations.push({label, words: values});
}
const installation32 = (unit, slot) => words(32, 'R3IN', [unit.low, unit.high, slot, 0]);
function resident(e, blocks, gates, helpers, label, slot) {
  refresh(e);
  [...blocks, ...gates].forEach(([pc, value], index) => {
    e.view.setUint32(e.base + TRANSFER + index * 8, pc, true);
    e.view.setUint32(e.base + TRANSFER + index * 8 + 4, value, true);
  });
  const before = arena(e); assert.equal(e.api.compile_resident_with_gates(blocks.length, gates.length), 0);
  const after = arena(e), view = new DataView(after.buffer);
  const low = view.getUint32(TRANSFER + 8, true), high = view.getUint32(TRANSFER + 12, true);
  const pointer = view.getUint32(TRANSFER + 16, true), length = view.getUint32(TRANSFER + 20, true);
  const id = BigInt(low) | (BigInt(high) << 32n);
  assert.equal(id, BigInt(++stats.resident_units)); assert.equal(high, 0);
  assert.ok(pointer > 0 && pointer + length <= refresh(e).bytes.length);
  const metadata = new Uint8Array(24), mv = new DataView(metadata.buffer);
  [1, 24, low, high, pointer, length].forEach((value, index) => mv.setUint32(index * 4, value, true));
  const expected = before.slice(); expected.set(metadata, TRANSFER); check(e, expected, 'resident metadata24 only');
  const bytes = e.bytes.slice(pointer, pointer + length), module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: 'guard_resident', kind: 'function'},
    ...helpers.map(name => ({module: 'ring3', name, kind: 'function'})),
  ]);
  const instance = new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3: Object.fromEntries(['guard_resident', ...helpers].map(name => [name, e.api[name]]))});
  const unit = {id, low, high, pointer, length, bytes, module, instance, run: instance.exports.run, label, slot};
  const file = `resident-${low}.wasm`; artifact(file, bytes);
  bindings.push({key: KEY.toString(), id: id.toString(), pointer, length, sha256: sha256(bytes), file, label, blocks, gates, slot});
  record('resident24', metadata, {id: id.toString(), pointer, length}); stats.metadata24++;
  e.table.set(slot, unit.run);
  const ackBefore = arena(e); assert.equal(e.api.acknowledge_resident_installation(e.low, e.high, low, high, slot), 0);
  const acknowledgement = installation32(unit, slot), ackExpected = ackBefore.slice(); ackExpected.set(acknowledgement, TRANSFER);
  check(e, ackExpected, 'pre-suspension exact acknowledgement'); record('R3IN32 acknowledgement', acknowledgement);
  references.push({key: KEY.toString(), id: id.toString(), slot, label: 'acknowledgement', exact_reference: e.table.get(slot) === unit.run});
  assert.equal(e.table.get(slot), unit.run); stats.acknowledgements++;
  return unit;
}
function dispatch(e, budget, expectedCpu, expectedExit, options = {}) {
  const {status = 0, children = 0, success = 0, failure = 0, last, helper, callbackRetired = 0, label} = options;
  const before = arena(e); assert.equal(e.dispatcher.exports.run(e.base, e.base + 56, budget, e.base + 96), status, label);
  const expected = before.slice(); expected.set(state56(expectedCpu), 0); expected.set(expectedExit, 56);
  if (helper) expected.set(helper, 100);
  if (last) {
    const selected = installation32(last, last.slot); expected.set(selected, TRANSFER);
    assert.equal(e.table.get(last.slot), last.run); record('R3IN32 selected child', selected);
    references.push({key: KEY.toString(), id: last.id.toString(), slot: last.slot, label, exact_reference: true});
  }
  check(e, expected, label); stats.dispatch_calls++; stats[status ? 'dispatch_rejected' : 'dispatch_canonical']++;
  stats.dispatch_children += children; stats.lookup_success += success; stats.lookup_failure += failure;
  stats.callback_retired += callbackRetired;
  executions.push({label, budget, status, children, lookup_success: success, lookup_failure: failure, state56_hex: Buffer.from(state56(expectedCpu)).toString('hex'), exit40_hex: Buffer.from(expectedExit).toString('hex'), helper40_hex: helper ? Buffer.from(helper).toString('hex') : null});
}
function stable(e) {
  assert.equal(e.memory, e.refs.memory); assert.equal(e.instance, e.refs.instance); assert.equal(e.api.generation(), 0);
  assert.equal(e.table, e.refs.table); assert.equal(e.dispatcher, e.refs.dispatcher);
  assert.equal(e.dispatcherModule, e.refs.dispatcherModule); assert.equal(e.table.length, 8);
  assert.deepEqual(refresh(e).bytes.slice(e.dispatcherPointer, e.dispatcherPointer + e.dispatcherBytes.length), e.dispatcherBytes);
  e.units.forEach((unit, index) => {
    assert.equal(unit.module, e.refs.units[index].module); assert.equal(unit.instance, e.refs.units[index].instance);
    assert.equal(unit.run, e.refs.units[index].run); assert.equal(e.table.get(unit.slot), unit.run);
    assert.deepEqual(e.bytes.slice(unit.pointer, unit.pointer + unit.length), unit.bytes);
  });
  for (let slot = 4; slot < 8; slot++) assert.equal(e.table.get(slot), null);
}

const instance = new WebAssembly.Instance(engineModule, {});
const api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
assert.equal(api.select_resident_callback_unit.length, 7); assert.equal(api.finish_resident_callback.length, 5);
assert.equal(api.capture_resident_callback_call.length, 7); assert.equal(api.complete_resident_callback_call.length, 7);
const e = refresh({api, instance, memory: instance.exports.memory, key: KEY, low: Number(KEY & 0xffffffffn), high: Number(KEY >> 32n)});
assert.equal(api.open(16, e.low, e.high), 0); stats.engines++;
e.base = api.arena_ptr() >>> 0; assert.ok(e.base > 0 && e.base + SIZE <= refresh(e).bytes.length);
e.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
const beforeGetter = arena(e); assert.equal(api.dispatcher_module(e.low, e.high), 0);
const getterAfter = arena(e), getterView = new DataView(getterAfter.buffer);
e.dispatcherPointer = getterView.getUint32(TRANSFER + 24, true);
const dispatcherLength = getterView.getUint32(TRANSFER + 28, true);
assert.ok(e.dispatcherPointer > 0 && e.dispatcherPointer + dispatcherLength <= refresh(e).bytes.length);
const getterRecord = words(32, 'R3DP', [e.low, e.high, e.dispatcherPointer, dispatcherLength]);
const getterExpected = beforeGetter.slice(); getterExpected.set(getterRecord, TRANSFER); check(e, getterExpected, 'dispatcher getter32 only');
e.dispatcherBytes = e.bytes.slice(e.dispatcherPointer, e.dispatcherPointer + dispatcherLength);
e.dispatcherModule = new WebAssembly.Module(e.dispatcherBytes);
assert.deepEqual(WebAssembly.Module.imports(e.dispatcherModule), [
  {module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'},
  {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'},
  {module: 'ring3', name: 'find_installed_resident', kind: 'function'},
]);
e.dispatcher = new WebAssembly.Instance(e.dispatcherModule, {env: {memory: e.memory, table: e.table}, ring3: {guard_dispatch_entry: api.guard_dispatch_entry, find_installed_resident: api.find_installed_resident}});
artifact('dispatcher.wasm', e.dispatcherBytes); record('R3DP32', getterRecord, {pointer: e.dispatcherPointer, length: dispatcherLength}); stats.metadata32++;
assert.equal(api.map(0x3000, 4, 7), 0); assert.equal(api.map(0x8000, 2, 3), 0);
for (const [label, pc, hex] of [
  ['caller', 0x3000, 'e8fb0f00008d4003'], ['outer-gate', 0x4000, '0f0b'],
  ['home', 0x5000, '8d4003e9f80f0000'], ['return-gate', 0x5100, '0f0b'],
  ['continuation', 0x6000, '8d4005c3'],
]) {
  const bytes = Buffer.from(hex, 'hex'); refresh(e).bytes.set(bytes, e.base + TRANSFER);
  assert.equal(api.upload(pc, bytes.length), 0); artifact(`${label}.x86`, bytes); authored[`${label}.x86`] = {pc, hex};
}
const outer = resident(e, [[0x4000, 2]], [[0x4000, OUTER]], [], 'outer', 0);
const home = resident(e, [[0x5000, 8], [0x5100, 2]], [[0x5100, RETURN]], [], 'home', 1);
const continuation = resident(e, [[0x6000, 4]], [], ['read32'], 'continuation', 2);
const caller = resident(e, [[0x3000, 5], [0x3005, 3]], [], ['store_resident32'], 'caller', 3);
e.units = [outer, home, continuation, caller];
e.refs = {memory: e.memory, instance, table: e.table, dispatcher: e.dispatcher, dispatcherModule: e.dispatcherModule, units: e.units.map(({module, instance, run}) => ({module, instance, run}))};
stable(e);
select(e, home, 0, continuation, 14, 'absent callback after current-home admission');
for (const [address, value] of [[0x8ff4, 0x11223344], [0x8ff8, 0x55667788], [0x8ffc, 0x22334455], [0x9000, 0x99aabbcc]]) hostWord(e, address, value);
refresh(e).bytes.set(state56(cpu(0x3000, 0x9000)), e.base);
e.bytes.fill(0xa5, e.base + 56, e.base + 96); e.view.setUint32(e.base + 96, 0, true);
e.bytes.fill(0x5a, e.base + 100, e.base + 140);
dispatch(e, 9, cpu(0x4000, 0x8ffc), exit40(8, 1, OUTER), {children: 2, success: 2, last: outer, helper: helper40(), label: 'real caller CALL then outer Gate'}); stats.guest_stores++;
observe(e, 0x55667788, 0x3005, 'real caller return word');
const outerBefore = arena(e); assert.equal(api.capture_resident_call(e.low, e.high, outer.low, outer.high, 1, 0), 0);
const callRecord = words(112, 'R3CF', [1, OUTER, 1, 0, 0x4000, 0x8ffc, 0x3005, 0]);
const outerExpected = outerBefore.slice(); outerExpected.set(callRecord, TRANSFER); check(e, outerExpected, 'literal outer CF112'); record('R3CF112', callRecord); stats.resident_captures++;
const beginBefore = arena(e); assert.equal(api.begin_resident_callback(e.low, e.high, outer.low, outer.high, home.low, home.high, 1, 0x5000, 0x5100, RETURN, 0), 0);
const callbackRecord = words(72, 'R3RC', [2, 1, 1, 0, 0x5000, 0x8ff8, 0x5100, RETURN, 0, 0, outer.low, outer.high, home.low, home.high]);
const beginExpected = beginBefore.slice(); beginExpected.set(state56(cpu(0x5000, 0x8ff8)), 0); beginExpected.set(exit40(3), 56); beginExpected.set(callbackRecord, TRANSFER);
check(e, beginExpected, 'zero-argument RC72 admission'); record('R3RC72', callbackRecord); stats.resident_begins++; stats.frame_words++;
observe(e, 0x5100, 0x3005, 'callback return push');
select(e, home, 2, continuation, 12, 'unarmed callback cannot select');
pure(e, () => api.authorize_resident_callback(e.low, e.high, home.low, home.high, 2), 0, 'pure initial authorization'); stats.authorizations++;
select(e, home, 2, home, 0, 'same-active canonical retired-zero selection');
dispatch(e, 0, cpu(0x5000, 0x8ff8), exit40(1), {label: 'actual zero-budget stop before callback guest code'});
select(e, home, 2, home, 13, 'same-active Budget stop rejected without CPU change');
dispatch(e, 9, cpu(0x6000, 0x8ff8, 13), exit40(3, 2), {status: 12, children: 1, success: 1, failure: 1, last: home, callbackRetired: 2, label: 'real home LEA/JMP retained-prefix foreign Busy'});
observe(e, 0x5100, 0x3005, 'home prefix stack unchanged');
select(e, home, 2, continuation, 3, 'full-key high-limb mismatch', e.high ^ 1);
select(e, home, 2, continuation, 3, 'full-home high-limb mismatch', e.high, home.high ^ 1);
select(e, home, 0, continuation, 14, 'zero callback token');
select(e, home, 3, continuation, 14, 'wrong callback token');
select(e, continuation, 2, continuation, 14, 'current B is not immutable callback home');
select(e, home, 2, continuation, 3, 'full-target high-limb mismatch', e.high, home.high, continuation.high ^ 1);
select(e, home, 2, outer, 7, 'retained outer cannot become active');
select(e, home, 2, home, 13, 'same-active target must contain current stopped PC');
select(e, home, 2, caller, 13, 'current acknowledged foreign target lacks stopped PC');
tamper(e, 0, Uint8Array.of(0), () => select(e, home, 2, continuation, 13, 'malformed State header'), 'malformed State header');
tamper(e, 52, new Uint8Array(4), () => select(e, home, 2, continuation, 13, 'invalid CPU flags'), 'invalid CPU flags');
tamper(e, 60, Uint8Array.of(6, 0), () => select(e, home, 2, continuation, 13, 'unsupported Exit version'), 'unsupported Exit version');
tamper(e, 72, Uint8Array.of(1, 0, 0, 0), () => select(e, home, 2, continuation, 13, 'canonical non-NeedCode Exit'), 'canonical non-NeedCode Exit');
tamper(e, 80, Uint8Array.of(1, 0, 0, 0), () => select(e, home, 2, continuation, 13, 'noncanonical NeedCode detail'), 'noncanonical NeedCode detail');
refresh(e).view.setUint32(e.base + 96, 1, true);
select(e, home, 2, continuation, 16, 'cancel after valid stop and installed target');
e.view.setUint32(e.base + 96, 0, true);
select(e, home, 2, continuation, 0, 'pure preinstalled B activation');
select(e, home, 2, continuation, 0, 'same-active B selection validates current PC');
guard(e, continuation, 0); guard(e, home, 12); guard(e, outer, 12); guard(e, caller, 12);
reject(e, () => api.finish_resident_callback(e.low, e.high, home.low, home.high, 2), 12, 'home finish Busy while selected away');
reject(e, () => api.capture_resident_callback_call(e.low, e.high, home.low, home.high, 2, 1, 0), 12, 'home inner capture Busy while selected away');
reject(e, () => api.complete_resident_callback_call(e.low, e.high, home.low, home.high, 2, 0, 18), 12, 'home inner completion Busy while selected away');
reject(e, () => api.finish_resident_callback(e.low, e.high, continuation.low, continuation.high, 2), 14, 'B cannot impersonate callback home for finish');
reject(e, () => api.compile_resident_with_gates(1, 0), 12, 'suspended Resident compile remains Busy');
reject(e, () => api.acknowledge_resident_installation(e.low, e.high, continuation.low, continuation.high, 2), 12, 'suspended acknowledgement remains Busy');
reject(e, () => api.store_resident32(e.low, e.high, home.low, home.high, 0x8ff4, 0), 12, 'home Store6 rejected before RAM/helper changes');
stable(e); observe(e, 0x5100, 0x3005, 'pure selection preserves stack');
dispatch(e, 9, cpu(0x5100, 0x8ffc, 18), exit40(3, 2, 0, 2), {status: 12, children: 1, success: 1, failure: 1, last: continuation, helper: helper40(0x5100), callbackRetired: 2, label: 'real B LEA/RET0 NeedCodev2 retained-prefix home Busy'});
select(e, home, 2, home, 0, 'pure home return-Gate activation');
select(e, home, 2, home, 0, 'same-active exact home return-Gate idempotence');
guard(e, home, 0); guard(e, continuation, 12);
dispatch(e, 1, cpu(0x5100, 0x8ffc, 18), exit40(8, 0, RETURN), {children: 1, success: 1, last: home, label: 'real home registered return Gate'});
const finishBefore = arena(e); assert.equal(api.finish_resident_callback(e.low, e.high, home.low, home.high, 2), 0);
const result = words(48, 'R3RR', [2, 1, 18, 0, outer.low, outer.high, home.low, home.high]);
const finishExpected = finishBefore.slice(); finishExpected.set(state56(cpu(0x4000, 0x8ffc)), 0); finishExpected.set(exit40(8, 1, OUTER), 56); finishExpected.set(result, TRANSFER);
check(e, finishExpected, 'finish fixed scalar18 and exact private outer'); record('R3RR48', result); stats.finishes++; stats.result48++;
select(e, home, 2, continuation, 14, 'consumed callback cannot select while outer pending');
observe(e, 0x5100, 0x3005, 'finish preserves committed return words');
const completeBefore = arena(e); assert.equal(api.complete_resident_call(e.low, e.high, outer.low, outer.high, 1, 18), 0);
const completeExpected = completeBefore.slice(); completeExpected.set(state56(cpu(0x3005, 0x9000, 18)), 0); completeExpected.set(exit40(3), 56);
check(e, completeExpected, 'explicit outer scalar completion'); stats.completions++;
dispatch(e, 9, cpu(0x3008, 0x9000, 21), exit40(3, 1), {children: 1, success: 1, failure: 1, last: caller, label: 'real caller LEA resumes once with result21'});
observe(e, 0x5100, 0x3005, 'caller continuation final stack'); stable(e);
pure(e, () => api.close(), 0, 'close public arena remains exact'); stats.closed++;
select(e, home, 0, outer, 5, 'closed before identity and token checks', e.high ^ 1, home.high ^ 1, outer.high ^ 1);

const expectedCounters = [1, 4, 4, 1, 4, 25, 5, 20, 6, 4, 2, 6, 6, 3, 6, 7, 1, 1, 1, 1, 1, 1, 24, 4, 92, 4, 1, 1, 1];
assert.equal(fields.length, 29);
assert.deepEqual(fields.map(field => stats[field]), expectedCounters, 'independently counted complete selector trace');
assert.equal(Object.keys(artifacts).length, 10); assert.equal(bindings.length, 4);
assert.equal(records.length, 17); assert.equal(references.length, 9);
assert.equal(executions.length, 6); assert.equal(selections.length, 25);
assert.equal(corruptions.length, 5); assert.equal(ramObservations.length, 6);
for (const path of sourcePaths) assert.deepEqual(readFileSync(join(root, path)), sourceBytes[path], `source unchanged after proof: ${path}`);
const sources = Object.fromEntries(sourcePaths.map(path => [path, sha256(sourceBytes[path])]));
const provenance = {
  fixture: 'preinstalled-resident-callback-selection',
  engine_sha256: sha256(engineBytes), executed_engine_copy: 'executed-engine.wasm',
  module_set_sha256: sha256(JSON.stringify(Object.entries(artifacts).filter(([name]) => name.endsWith('.wasm')).sort())),
  production_source_set_sha256: sha256(JSON.stringify(productionPaths.map(path => [path, sources[path]]).sort())),
  sources, source_snapshot_directory: 'sources', source_snapshot_before_first_module: true,
  source_snapshot_before_first_engine_instance: true, source_bytes_unchanged_after_proof: true,
  artifact_sha256: artifacts, authored, unit_bindings: bindings, installed_references: references,
  raw_records: records, executions, selections, deliberate_restored_corruptions: corruptions,
  bounded_ram_observations: ramObservations, stats,
  sections: [{name: 'preinstalled-selection', ...stats}],
  selected_fields: fields, expected_counters: expectedCounters,
  flow: {key: KEY.toString(), outer_id: '1', home_id: '2', continuation_id: '3', caller_id: '4', outer_token: 1, callback_token: 2, immutable_home: true, selector_publication_bytes: 0, callback_retired: 4, total_guest_retired: 6, finish_result: 18, caller_result: 21},
  environment: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch},
  git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(),
  git_status: execFileSync('git', ['status', '--porcelain'], {cwd: root, encoding: 'utf8'}).trim(),
  claim_ceiling: 'one synchronous trusted-host preinstalled selector trace; immutable home and outer; no cold installation, B inner capture, nested callbacks, table/memory authentication, SDK/browser/async/race/performance/game/fullP2-V0 claim',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({status: 'ok', fixture: provenance.fixture, stats, source_count: sourcePaths.length, artifact_count: Object.keys(artifacts).length, engine_sha256: provenance.engine_sha256, module_set_sha256: provenance.module_set_sha256}));
