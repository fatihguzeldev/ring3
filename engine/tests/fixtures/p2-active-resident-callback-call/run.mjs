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
const OUTER = 0x80000011, RETURN = 0x80000012, INNER = 0x80000013;
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
  'engine/tests/process_active_resident_callback_call.rs',
  'engine/tests/process_active_resident_callback_call_wasm.rs',
  'engine/tests/fixtures/p2-active-resident-callback-call/run.mjs',
];
const productionPaths = sourcePaths.filter(path => path.startsWith('engine/src/'));
assert.equal(sourcePaths.length, 98);
assert.equal(new Set(sourcePaths).size, 98);
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
  'capture_active_resident_callback_call', 'complete_active_resident_callback_call',
];
const fields = [
  'engines', 'resident_units', 'metadata24', 'metadata32', 'acknowledgements',
  'selectors', 'selectors_success', 'selectors_rejected', 'dispatch_calls',
  'dispatch_canonical', 'dispatch_rejected', 'dispatch_children', 'lookup_success',
  'lookup_failure', 'guard_checks', 'api_rejections', 'resident_captures',
  'resident_begins', 'authorizations', 'active_captures', 'active_capture_rejected',
  'active_completions', 'active_completion_rejected', 'finishes', 'result48', 'completions',
  'host_reads', 'host_writes', 'arena_checks', 'callback_retired', 'guest_stores',
  'frame_words', 'host_frame_corruptions', 'closed',
];
const stats = Object.fromEntries(fields.map(field => [field, 0]));
const artifacts = {}, authored = {}, records = [], bindings = [], references = [], executions = [];
const selections = [], corruptions = [], ramObservations = [], activeCalls = [];
const guards = [], rejections = [], frameCorruptions = [];
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
  pure(e, action, status, label); stats.api_rejections++; rejections.push({label, status});
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
  stats.guard_checks++; guards.push({id: unit.id.toString(), label: unit.label, status});
}
function hostWord(e, address, value, read = false) {
  const before = arena(e); assert.equal(read ? e.api.read32(address) : e.api.write32(address, value), 0);
  const expected = before.slice(); expected.set(helper40(read ? value : 0), 100);
  check(e, expected, read ? 'bounded RAM read' : 'RAM setup'); stats[read ? 'host_reads' : 'host_writes']++;
}
function observe(e, innerReturn, callbackReturn, callerReturn, label) {
  const values = [[0x8ff4, innerReturn], [0x8ff8, callbackReturn], [0x8ffc, callerReturn], [0x9000, 0x99aabbcc]];
  for (const [address, value] of values) hostWord(e, address, value, true);
  ramObservations.push({label, words: values});
}
function observeCode(e) {
  const values = [
    [0x3000, 0x000ffbe8], [0x3004, 0x03408d00], [0x4000, 0x00000b0f],
    [0x5000, 0xe903408d], [0x5004, 0x00000ff8], [0x5100, 0x00000b0f],
    [0x6000, 0xe805408d], [0x6004, 0x000001f8], [0x6008, 0xc303408d], [0x6200, 0x00000b0f],
  ];
  for (const [address, value] of values) hostWord(e, address, value, true);
  ramObservations.push({label: 'complete authored guest code plus zero padding', words: values});
}
function activeCall(kind, e, unit, token, first, second, status, label, keyHigh = e.high, unitHigh = unit.high) {
  assert.notEqual(status, 0);
  const name = kind === 'capture' ? 'capture_active_resident_callback_call' : 'complete_active_resident_callback_call';
  pure(e, () => e.api[name](e.low, keyHigh, unit.low, unitHigh, token, first, second), status, label);
  activeCalls.push({kind, key_high: keyHigh >>> 0, unit_low: unit.low, unit_high: unitHigh >>> 0, callback_token: token >>> 0, first: first >>> 0, second: second >>> 0, status, label});
  stats[kind === 'capture' ? 'active_capture_rejected' : 'active_completion_rejected']++;
}
function corruptFrameWord(e, address, value, label) {
  hostWord(e, address, value);
  frameCorruptions.push({address, value, label, restored: false}); stats.host_frame_corruptions++;
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
for (const name of ['capture_active_resident_callback_call', 'complete_active_resident_callback_call', 'capture_resident_callback_call', 'complete_resident_callback_call', 'select_resident_callback_unit']) assert.equal(api[name].length, 7);
assert.equal(api.finish_resident_callback.length, 5);
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
  ['continuation', 0x6000, '8d4005e8f80100008d4003c3'], ['inner-gate', 0x6200, '0f0b'],
]) {
  const bytes = Buffer.from(hex, 'hex'); refresh(e).bytes.set(bytes, e.base + TRANSFER);
  assert.equal(api.upload(pc, bytes.length), 0); artifact(`${label}.x86`, bytes); authored[`${label}.x86`] = {pc, hex};
}
const outer = resident(e, [[0x4000, 2]], [[0x4000, OUTER]], [], 'outer', 0);
const home = resident(e, [[0x5000, 8], [0x5100, 2]], [[0x5100, RETURN]], [], 'home', 1);
const continuation = resident(e, [[0x6000, 8], [0x6008, 4], [0x6200, 2]], [[0x6200, INNER]], ['read32', 'store_resident32'], 'continuation', 2);
const caller = resident(e, [[0x3000, 5], [0x3005, 3]], [], ['store_resident32'], 'caller', 3);
e.units = [outer, home, continuation, caller];
e.refs = {memory: e.memory, instance, table: e.table, dispatcher: e.dispatcher, dispatcherModule: e.dispatcherModule, units: e.units.map(({module, instance, run}) => ({module, instance, run}))};
stable(e);
activeCall('capture', e, continuation, 0, 0, 17, 14, 'absent callback before invalid descriptor');
activeCall('complete', e, continuation, 0, 0, 44, 14, 'absent callback completion');
for (const [address, value] of [[0x8ff4, 0x11223344], [0x8ff8, 0x55667788], [0x8ffc, 0x22334455], [0x9000, 0x99aabbcc]]) hostWord(e, address, value);
refresh(e).bytes.set(state56(cpu(0x3000, 0x9000)), e.base);
e.bytes.fill(0xa5, e.base + 56, e.base + 96); e.view.setUint32(e.base + 96, 0, true);
e.bytes.fill(0x5a, e.base + 100, e.base + 140);
dispatch(e, 9, cpu(0x4000, 0x8ffc), exit40(8, 1, OUTER), {children: 2, success: 2, last: outer, helper: helper40(), label: 'real caller CALL then outer Gate'}); stats.guest_stores++;
observe(e, 0x11223344, 0x55667788, 0x3005, 'real caller return word');
const outerBefore = arena(e); assert.equal(api.capture_resident_call(e.low, e.high, outer.low, outer.high, 1, 0), 0);
const outerRecord = words(112, 'R3CF', [1, OUTER, 1, 0, 0x4000, 0x8ffc, 0x3005, 0]);
const outerExpected = outerBefore.slice(); outerExpected.set(outerRecord, TRANSFER); check(e, outerExpected, 'literal outer CF112'); record('R3CF112 outer', outerRecord); stats.resident_captures++;
const beginBefore = arena(e); assert.equal(api.begin_resident_callback(e.low, e.high, outer.low, outer.high, home.low, home.high, 1, 0x5000, 0x5100, RETURN, 0), 0);
const callbackRecord = words(72, 'R3RC', [2, 1, 1, 0, 0x5000, 0x8ff8, 0x5100, RETURN, 0, 0, outer.low, outer.high, home.low, home.high]);
const beginExpected = beginBefore.slice(); beginExpected.set(state56(cpu(0x5000, 0x8ff8)), 0); beginExpected.set(exit40(3), 56); beginExpected.set(callbackRecord, TRANSFER);
check(e, beginExpected, 'zero-argument RC72 admission'); record('R3RC72', callbackRecord); stats.resident_begins++; stats.frame_words++;
observe(e, 0x11223344, 0x5100, 0x3005, 'callback return push');
activeCall('capture', e, continuation, 2, 1, 0, 12, 'unarmed active capture');
activeCall('complete', e, continuation, 2, 0, 44, 12, 'unarmed active completion');
select(e, home, 2, continuation, 12, 'unarmed selection');
pure(e, () => api.authorize_resident_callback(e.low, e.high, home.low, home.high, 2), 0, 'pure initial authorization'); stats.authorizations++;
activeCall('capture', e, home, 2, 1, 0, 13, 'home active canonical NeedCode is not an API Gate');
dispatch(e, 9, cpu(0x6000, 0x8ff8, 13), exit40(3, 2), {status: 12, children: 1, success: 1, failure: 1, last: home, callbackRetired: 2, label: 'real home LEA/JMP retained-prefix foreign Busy'});
activeCall('capture', e, continuation, 2, 1, 0, 12, 'B cannot capture before explicit active selection');
select(e, home, 2, continuation, 0, 'pure preinstalled B activation');
activeCall('capture', e, continuation, 2, 0, 0, 7, 'unknown convention before non-Gate CPU');
activeCall('capture', e, continuation, 2, 1, 17, 7, 'oversized arguments before non-Gate CPU');
activeCall('capture', e, continuation, 2, 1, 0, 13, 'selected B NeedCode is not an API Gate');
activeCall('capture', e, continuation, 2, 1, 0, 3, 'full key high-limb mismatch', e.high ^ 1);
activeCall('capture', e, continuation, 2, 1, 0, 3, 'full executor high-limb mismatch', e.high, continuation.high ^ 1);
activeCall('capture', e, continuation, 0x80000002, 1, 0, 14, 'high-bit callback token is not token2');
activeCall('capture', e, continuation, 0, 1, 0, 14, 'zero callback token');
activeCall('capture', e, continuation, 1, 1, 0, 14, 'outer token is not callback token');
activeCall('capture', e, caller, 2, 1, 0, 12, 'current foreign caller lacks active authority');
activeCall('complete', e, continuation, 2, 3, 44, 14, 'active completion without inner pending');
activeCall('complete', e, continuation, 0x80000002, 3, 44, 14, 'completion high-bit callback token mismatch');
activeCall('complete', e, continuation, 2, 3, 44, 3, 'completion full executor high-limb mismatch', e.high, continuation.high ^ 1);
activeCall('complete', e, continuation, 2, 3, 44, 3, 'completion full key high-limb mismatch', e.high ^ 1);
guard(e, continuation, 0); guard(e, home, 12);
reject(e, () => api.capture_resident_callback_call(e.low, e.high, home.low, home.high, 2, 1, 0), 12, 'old home capture remains Busy while B active');
reject(e, () => api.capture_resident_callback_call(e.low, e.high, continuation.low, continuation.high, 2, 1, 0), 14, 'B cannot impersonate home for old capture');
reject(e, () => api.complete_resident_callback_call(e.low, e.high, home.low, home.high, 2, 3, 44), 12, 'old home completion remains Busy while B active');
reject(e, () => api.complete_resident_callback_call(e.low, e.high, continuation.low, continuation.high, 2, 3, 44), 14, 'B cannot impersonate home for old completion');
reject(e, () => api.finish_resident_callback(e.low, e.high, home.low, home.high, 2), 12, 'home finish remains Busy while B active');
reject(e, () => api.finish_resident_callback(e.low, e.high, continuation.low, continuation.high, 2), 14, 'B cannot own callback finish');
reject(e, () => api.compile_resident_with_gates(1, 0), 12, 'suspended compilation remains Busy');
reject(e, () => api.acknowledge_resident_installation(e.low, e.high, continuation.low, continuation.high, 2), 12, 'suspended acknowledgement remains Busy');
dispatch(e, 9, cpu(0x6200, 0x8ff4, 18), exit40(8, 2, INNER), {children: 1, success: 1, last: continuation, helper: helper40(), callbackRetired: 2, label: 'real B LEA/CALL then ordinary API Gate'}); stats.guest_stores++;
observe(e, 0x6008, 0x5100, 0x3005, 'real B inner CALL return word');
tamper(e, 0, Uint8Array.of(0), () => activeCall('capture', e, continuation, 2, 1, 0, 13, 'malformed Gate State header'), 'malformed Gate State header');
refresh(e).view.setUint32(e.base + 96, 1, true);
activeCall('capture', e, continuation, 2, 1, 0, 16, 'cancel after valid current Gate');
activeCall('capture', e, continuation, 3, 1, 0, 14, 'wrong callback token before cancel');
e.view.setUint32(e.base + 96, 0, true);
const innerBefore = arena(e); assert.equal(api.capture_active_resident_callback_call(e.low, e.high, continuation.low, continuation.high, 2, 1, 0), 0);
const innerRecord = words(112, 'R3CF', [3, INNER, 1, 0, 0x6200, 0x8ff4, 0x6008, 0]);
const innerExpected = innerBefore.slice(); innerExpected.set(innerRecord, TRANSFER); check(e, innerExpected, 'literal active B inner CF112 token3'); record('R3CF112 active inner', innerRecord); stats.active_captures++;
activeCalls.push({kind: 'capture', key_high: e.high, unit_low: continuation.low, unit_high: continuation.high, callback_token: 2, first: 1, second: 0, status: 0, label: 'literal active B inner CF112 token3'});
activeCall('capture', e, continuation, 2, 0, 17, 12, 'pending before invalid descriptor');
activeCall('complete', e, continuation, 2, 0, 44, 14, 'zero inner token');
activeCall('complete', e, continuation, 2, 2, 44, 14, 'callback token is not inner token');
activeCall('complete', e, continuation, 2, 1, 44, 12, 'retained outer token protected while inner pending');
activeCall('complete', e, continuation, 3, 3, 44, 14, 'inner token is not callback token');
activeCall('complete', e, caller, 2, 3, 44, 12, 'foreign current executor cannot consume B pending');
reject(e, () => api.complete_resident_call(e.low, e.high, continuation.low, continuation.high, 3, 44), 14, 'ordinary Resident owner cannot consume callback inner');
reject(e, () => api.complete_resident_callback_call(e.low, e.high, home.low, home.high, 2, 3, 44), 12, 'old home completion cannot consume active B inner');
reject(e, () => api.complete_resident_call(e.low, e.high, outer.low, outer.high, 1, 44), 12, 'ordinary outer completion remains protected');
reject(e, () => api.finish_resident_callback(e.low, e.high, home.low, home.high, 2), 12, 'pending keeps home finish Busy');
reject(e, () => api.finish_resident_callback(e.low, e.high, continuation.low, continuation.high, 2), 14, 'pending B still cannot own finish');
guard(e, continuation, 12); guard(e, home, 12);
select(e, home, 2, home, 12, 'inner pending prevents active selection');
dispatch(e, 9, cpu(0x6200, 0x8ff4, 18), exit40(8, 2, INNER), {status: 12, label: 'pending dispatcher guard-first rejection before finder or child'});
tamper(e, 52, new Uint8Array(4), () => activeCall('complete', e, continuation, 2, 3, 44, 15, 'frozen CPU flags mismatch before decoding'), 'frozen CPU flags mismatch');
tamper(e, 56, Uint8Array.of(0), () => activeCall('complete', e, continuation, 2, 3, 44, 15, 'frozen Exit header mismatch before decoding'), 'frozen Exit header mismatch');
refresh(e).view.setUint32(e.base + 96, 1, true);
activeCall('complete', e, continuation, 2, 3, 44, 16, 'cancel after exact inner identity and frozen CPU');
activeCall('complete', e, continuation, 2, 0, 44, 14, 'wrong inner token before cancel');
activeCall('complete', e, continuation, 2, 1, 44, 12, 'protected outer token before cancel');
e.view.setUint32(e.base + 96, 0, true);
corruptFrameWord(e, 0x8ff4, 0xdeadbeef, 'overwrite captured inner return word');
corruptFrameWord(e, 0x8ffc, 0xcafebabe, 'overwrite captured retained outer return word');
observe(e, 0xdeadbeef, 0x5100, 0xcafebabe, 'committed frozen-frame stack corruption');
const completeInnerBefore = arena(e); assert.equal(api.complete_active_resident_callback_call(e.low, e.high, continuation.low, continuation.high, 2, 3, 44), 0);
const completeInnerExpected = completeInnerBefore.slice(); completeInnerExpected.set(state56(cpu(0x6008, 0x8ff8, 44)), 0); completeInnerExpected.set(exit40(3), 56);
check(e, completeInnerExpected, 'active completion44 from frozen return6008 despite stack corruption'); stats.active_completions++;
activeCalls.push({kind: 'complete', key_high: e.high, unit_low: continuation.low, unit_high: continuation.high, callback_token: 2, first: 3, second: 44, status: 0, label: 'active completion44 from frozen return6008 despite stack corruption'});
observe(e, 0xdeadbeef, 0x5100, 0xcafebabe, 'inner completion does not repair or reread committed stack');
activeCall('complete', e, continuation, 2, 3, 44, 14, 'consumed inner cannot replay');
activeCall('complete', e, continuation, 2, 1, 44, 12, 'retained outer protected even without inner pending');
guard(e, continuation, 0); guard(e, home, 12);
stable(e);
dispatch(e, 9, cpu(0x5100, 0x8ffc, 47), exit40(3, 2), {status: 12, children: 1, success: 1, failure: 1, last: continuation, helper: helper40(0x5100), callbackRetired: 2, label: 'real B LEA/RET0 retained-prefix home Busy'});
select(e, home, 2, home, 0, 'pure home return-Gate activation');
guard(e, home, 0); guard(e, continuation, 12);
dispatch(e, 1, cpu(0x5100, 0x8ffc, 47), exit40(8, 0, RETURN), {children: 1, success: 1, last: home, label: 'real home registered return Gate'});
const finishBefore = arena(e); assert.equal(api.finish_resident_callback(e.low, e.high, home.low, home.high, 2), 0);
const result = words(48, 'R3RR', [2, 1, 47, 0, outer.low, outer.high, home.low, home.high]);
const finishExpected = finishBefore.slice(); finishExpected.set(state56(cpu(0x4000, 0x8ffc)), 0); finishExpected.set(exit40(8, 1, OUTER), 56); finishExpected.set(result, TRANSFER);
check(e, finishExpected, 'finish fixed scalar47 and exact immutable private outer'); record('R3RR48', result); stats.finishes++; stats.result48++;
activeCall('capture', e, continuation, 2, 1, 0, 14, 'consumed callback cannot capture');
activeCall('complete', e, continuation, 2, 3, 44, 14, 'consumed callback cannot complete');
select(e, home, 2, continuation, 14, 'consumed callback cannot select');
const completeOuterBefore = arena(e); assert.equal(api.complete_resident_call(e.low, e.high, outer.low, outer.high, 1, 47), 0);
const completeOuterExpected = completeOuterBefore.slice(); completeOuterExpected.set(state56(cpu(0x3005, 0x9000, 47)), 0); completeOuterExpected.set(exit40(3), 56);
check(e, completeOuterExpected, 'explicit outer47 from frozen caller3005 despite stack corruption'); stats.completions++;
dispatch(e, 9, cpu(0x3008, 0x9000, 50), exit40(3, 1), {children: 1, success: 1, failure: 1, last: caller, label: 'real caller LEA resumes once with result50'});
observe(e, 0xdeadbeef, 0x5100, 0xcafebabe, 'final caller preserves committed corrupted return words');
observeCode(e); stable(e);
pure(e, () => api.close(), 0, 'close public arena remains exact'); stats.closed++;
activeCall('capture', e, continuation, 0, 0, 17, 5, 'closed before executor identity or token', e.high ^ 1, continuation.high ^ 1);
activeCall('complete', e, continuation, 0, 0, 44, 5, 'closed completion before executor identity or token', e.high ^ 1, continuation.high ^ 1);
select(e, home, 0, continuation, 5, 'closed selector before identity and token', e.high ^ 1, home.high ^ 1, continuation.high ^ 1);

const expectedCounters = [1, 4, 4, 1, 4, 6, 2, 4, 7, 4, 3, 7, 7, 3, 8, 13, 1, 1, 1, 1, 19, 1, 20, 1, 1, 1, 34, 6, 133, 6, 2, 1, 2, 1];
assert.equal(fields.length, 34);
assert.deepEqual(fields.map(field => stats[field]), expectedCounters, 'independently counted complete active-executor trace');
assert.equal(Object.keys(artifacts).length, 11); assert.equal(bindings.length, 4);
assert.equal(records.length, 19); assert.equal(references.length, 10);
assert.equal(executions.length, 7); assert.equal(selections.length, 6);
assert.equal(activeCalls.length, 41); assert.equal(guards.length, 8); assert.equal(rejections.length, 13);
assert.equal(corruptions.length, 3); assert.equal(frameCorruptions.length, 2); assert.equal(ramObservations.length, 7);
for (const path of sourcePaths) assert.deepEqual(readFileSync(join(root, path)), sourceBytes[path], `source unchanged after proof: ${path}`);
const sources = Object.fromEntries(sourcePaths.map(path => [path, sha256(sourceBytes[path])]));
const provenance = {
  fixture: 'active-resident-callback-api-call',
  engine_sha256: sha256(engineBytes), executed_engine_copy: 'executed-engine.wasm',
  module_set_sha256: sha256(JSON.stringify(Object.entries(artifacts).filter(([name]) => name.endsWith('.wasm')).sort())),
  production_source_set_sha256: sha256(JSON.stringify(productionPaths.map(path => [path, sources[path]]).sort())),
  sources, source_snapshot_directory: 'sources', source_snapshot_before_first_module: true,
  source_snapshot_before_first_engine_instance: true, source_bytes_unchanged_after_proof: true,
  artifact_sha256: artifacts, authored, unit_bindings: bindings, installed_references: references,
  raw_records: records, executions, selections, active_calls: activeCalls, direct_guards: guards,
  other_api_rejections: rejections, deliberate_restored_corruptions: corruptions,
  committed_frame_corruptions: frameCorruptions, bounded_ram_observations: ramObservations, stats,
  sections: [{name: 'active-executor-inner-call', ...stats}],
  selected_fields: fields, expected_counters: expectedCounters,
  flow: {key: KEY.toString(), outer_id: '1', home_id: '2', continuation_id: '3', caller_id: '4', outer_token: 1, callback_token: 2, inner_token: 3, immutable_home: true, active_retained_after_inner_completion: true, inner_result: 44, callback_retired: 6, total_guest_retired: 8, finish_result: 47, caller_result: 50, corrupted_inner_return_word: 0xdeadbeef, corrupted_outer_return_word: 0xcafebabe, frozen_inner_return_pc: 0x6008, frozen_outer_return_pc: 0x3005},
  environment: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch},
  git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(),
  git_status: execFileSync('git', ['status', '--porcelain'], {cwd: root, encoding: 'utf8'}).trim(),
  claim_ceiling: 'one synchronous trusted-host preinstalled active B callback inner API trace; immutable home/outer and frozen scalar completion; no cold installation, callback nesting, new return/table/memory authentication, provider/SDK/browser/async/race/eviction/performance/game/fullP2-V0 claim',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({status: 'ok', fixture: provenance.fixture, stats, source_count: sourcePaths.length, artifact_count: Object.keys(artifacts).length, engine_sha256: provenance.engine_sha256, module_set_sha256: provenance.module_set_sha256}));
