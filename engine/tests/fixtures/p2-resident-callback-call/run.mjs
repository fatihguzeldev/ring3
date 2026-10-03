import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root);
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath);
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140;
const KEY = 0xd3456789e0000000n;
const OUTER = 0x80000011, RETURN = 0x80000012, INNER = 0x80000033;
const LEGACY = 0x80000077, LEGACY_RETURN = 0x80000078;
const apiNames = [
  'open', 'close', 'arena_ptr', 'map', 'upload', 'compile_with_gates',
  'module_ptr', 'module_len', 'generation', 'compile_resident_with_gates',
  'dispatcher_module', 'guard_dispatch_entry', 'guard_resident', 'guard',
  'read32', 'write32', 'store_resident32', 'acknowledge_resident_installation',
  'find_installed_resident', 'capture_resident_call', 'complete_resident_call',
  'capture_call', 'complete_call', 'abandon_call', 'begin_resident_callback',
  'authorize_resident_callback', 'finish_resident_callback', 'begin_callback', 'finish_callback',
  'capture_resident_callback_call', 'complete_resident_callback_call', 'abort_callback',
];
const fields = [
  'engines', 'resident_units', 'legacy_modules', 'metadata24',
  'metadata32', 'acks', 'dispatch_calls', 'dispatch_canonical',
  'dispatch_rejected', 'dispatch_children', 'lookup_success', 'lookup_failure',
  'unit_calls', 'unit_canonical', 'unit_rejected', 'legacy_unit_calls',
  'resident_captures', 'legacy_captures', 'resident_begins', 'legacy_begins',
  'authorize_success', 'inner_captures', 'inner_capture_rejected', 'inner_completions',
  'inner_completion_rejected', 'inner_abandons', 'finish_success', 'finish_rejected',
  'result48', 'aborts', 'completions', 'other_rejected',
  'host_reads', 'host_writes', 'arena_checks', 'callback_retired',
  'guest_stores', 'frame_words', 'closed', 'outer_abandons',
];
const stats = Object.fromEntries(fields.map(name => [name, 0]));
const artifacts = {}, authored = {}, records = [], unitBindings = [];
const installations = [], sections = [], flows = [];
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
];
const productionPaths = sourcePaths.filter(path => path.startsWith('engine/src/'));
assert.equal(sourcePaths.length, 92);
assert.equal(productionPaths.length, 63);
assert.equal(new Set(sourcePaths).size, 92);
const sourceBytes = Object.fromEntries(sourcePaths.map(path => [path, readFileSync(join(root, path))]));
for (const [path, bytes] of Object.entries(sourceBytes)) {
  const destination = join(outputDir, 'sources', path);
  mkdirSync(dirname(destination), {recursive: true}); writeFileSync(destination, bytes);
}
writeFileSync(join(outputDir, 'executed-engine.wasm'), engineBytes);

function refresh(e) {
  if (e.buffer !== e.memory.buffer) {
    e.buffer = e.memory.buffer;
    e.bytes = new Uint8Array(e.buffer);
    e.view = new DataView(e.buffer);
  }
  return e;
}
function arena(e) { return refresh(e).bytes.slice(e.base, e.base + SIZE); }
function header(bytes, magic, version = 1) {
  const view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic));
  view.setUint16(4, version, true);
  view.setUint16(6, 1, true);
  view.setUint32(8, bytes.length, true);
}
function wordsRecord(size, magic, words, version = 1) {
  const bytes = new Uint8Array(size), view = new DataView(bytes.buffer);
  header(bytes, magic, version);
  words.forEach((word, index) => view.setUint32(16 + index * 4, word, true));
  return bytes;
}
function state(pc = 0x3000, esp = 0x9000, ebx = 0xa010) {
  return {
    registers: [0x89abcdef, 3, 0x23456789, ebx, esp, 0x56789abc, 0x6789abcd, 0x789abcde],
    eip: pc, eflags: 0xcd7,
  };
}
function copy(s) { return {...s, registers: [...s.registers]}; }
function state56(s) {
  return wordsRecord(56, 'R3ST', [...s.registers, s.eip, s.eflags]);
}
function exit40(reason, retired = 0, detail = 0) {
  return wordsRecord(40, 'R3EX', [reason, retired, detail, 0, 0, 0], 3);
}
function helper40(value = 0) {
  return wordsRecord(40, 'R3MH', [0, value, 0, 0, 0, 0]);
}
function reset(e, s) {
  refresh(e).bytes.set(state56(s), e.base);
  e.bytes.fill(0xa5, e.base + 56, e.base + 96);
  e.view.setUint32(e.base + 96, 0, true);
  e.bytes.fill(0x5a, e.base + 100, e.base + 140);
}
function cancel(e, value) { refresh(e).view.setUint32(e.base + 96, value, true); }
function checkArena(e, expected, label) {
  assert.deepEqual(arena(e), expected, `${label}: entire4236arena`);
  stats.arena_checks++;
}
function pure(e, action, status, label, countOther = true) {
  const before = arena(e);
  assert.equal(action(), status, label);
  checkArena(e, before, label);
  if (status && countOther) stats.other_rejected++;
}
function saveRecord(e, kind, bytes, extra = {}) {
  records.push({
    key: e.key.toString(), kind, hex: Buffer.from(bytes).toString('hex'),
    sha256: sha256(bytes), ...extra,
  });
}
function installation32(unit, slot) {
  return wordsRecord(32, 'R3IN', [unit.low, unit.high, slot, 0]);
}
function dispatcherGetter(e) {
  const before = arena(e);
  assert.equal(e.api.dispatcher_module(e.low, e.high), 0);
  const after = arena(e), view = new DataView(after.buffer);
  const pointer = view.getUint32(TRANSFER + 24, true);
  const length = view.getUint32(TRANSFER + 28, true);
  assert.ok(pointer > 0 && pointer + length <= refresh(e).bytes.length);
  const record = wordsRecord(32, 'R3DP', [e.low, e.high, pointer, length]);
  const expected = before.slice(); expected.set(record, TRANSFER);
  checkArena(e, expected, 'immutable product dispatcher getter transfer32 only');
  const bytes = e.bytes.slice(pointer, pointer + length);
  if (e.dispatcherBytes) {
    assert.equal(pointer, e.dispatcherPointer);
    assert.deepEqual(bytes, e.dispatcherBytes);
  } else {
    const module = new WebAssembly.Module(bytes);
    assert.deepEqual(WebAssembly.Module.imports(module), [
      {module: 'env', name: 'memory', kind: 'memory'},
      {module: 'env', name: 'table', kind: 'table'},
      {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'},
      {module: 'ring3', name: 'find_installed_resident', kind: 'function'},
    ]);
    e.dispatcher = new WebAssembly.Instance(module, {
      env: {memory: e.memory, table: e.table},
      ring3: {
        guard_dispatch_entry: e.api.guard_dispatch_entry,
        find_installed_resident: e.api.find_installed_resident,
      },
    });
    e.dispatcherBytes = bytes; e.dispatcherPointer = pointer;
    saveArtifact(`dispatcher-${e.ordinal}.wasm`, bytes);
  }
  saveRecord(e, 'R3DP32', record, {pointer, length, module_sha256: sha256(bytes)});
  stats.metadata32++;
}
function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(apiNames.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const ordinal = ++stats.engines, key = KEY + BigInt(ordinal);
  const e = refresh({
    api, instance, ordinal, key, nextId: 0, memory: instance.exports.memory,
    low: Number(key & 0xffffffffn), high: Number(key >> 32n),
  });
  assert.equal(api.open(16, e.low, e.high), 0);
  assert.equal(api.authorize_resident_callback.length, 5);
  assert.equal(api.finish_resident_callback.length, 5);
  assert.equal(api.capture_resident_callback_call.length, 7);
  assert.equal(api.complete_resident_callback_call.length, 7);
  e.base = api.arena_ptr() >>> 0;
  assert.ok(e.base > 0 && e.base + SIZE <= refresh(e).bytes.length);
  e.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
  dispatcherGetter(e);
  return e;
}
function saveArtifact(file, bytes) {
  writeFileSync(join(outputDir, file), bytes); artifacts[file] = sha256(bytes);
}
function upload(e, pc, hex, label) {
  const bytes = Buffer.from(hex, 'hex');
  refresh(e).bytes.set(bytes, e.base + TRANSFER);
  assert.equal(e.api.upload(pc, bytes.length), 0);
  const file = `engine-${e.ordinal}-${label}.x86`;
  saveArtifact(file, bytes); authored[file] = {pc, hex};
}
function descriptors(e, blocks, gates) {
  refresh(e);
  [...blocks, ...gates].forEach(([pc, value], index) => {
    e.view.setUint32(e.base + TRANSFER + index * 8, pc, true);
    e.view.setUint32(e.base + TRANSFER + index * 8 + 4, value, true);
  });
}
function resident(e, blocks, gates, helpers, label) {
  descriptors(e, blocks, gates);
  const before = arena(e);
  assert.equal(e.api.compile_resident_with_gates(blocks.length, gates.length), 0);
  const after = arena(e), view = new DataView(after.buffer);
  const low = view.getUint32(TRANSFER + 8, true), high = view.getUint32(TRANSFER + 12, true);
  const pointer = view.getUint32(TRANSFER + 16, true), length = view.getUint32(TRANSFER + 20, true);
  const id = BigInt(low) | (BigInt(high) << 32n);
  assert.equal(id, BigInt(++e.nextId));
  assert.ok(pointer > 0 && pointer + length <= refresh(e).bytes.length);
  const expected = before.slice(), ev = new DataView(expected.buffer);
  [1, 24, low, high, pointer, length].forEach((word, index) => ev.setUint32(TRANSFER + index * 4, word, true));
  checkArena(e, expected, 'resident prepare publishes metadata24 only');
  const bytes = e.bytes.slice(pointer, pointer + length), module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: 'guard_resident', kind: 'function'},
    ...helpers.map(name => ({module: 'ring3', name, kind: 'function'})),
  ]);
  const instance = new WebAssembly.Instance(module, {
    env: {memory: e.memory},
    ring3: Object.fromEntries(['guard_resident', ...helpers].map(name => [name, e.api[name]])),
  });
  const file = `resident-${++stats.resident_units}.wasm`;
  saveArtifact(file, bytes);
  const unit = {id, low, high, pointer, length, bytes, run: instance.exports.run, label, file};
  unitBindings.push({key: e.key.toString(), id: id.toString(), pointer, length, sha256: sha256(bytes), file, blocks, gates, label});
  saveRecord(e, 'resident24', after.slice(TRANSFER, TRANSFER + 24), {id: id.toString(), pointer, length});
  stats.metadata24++;
  return unit;
}
function legacyModule(e, blocks, gates, helpers = []) {
  descriptors(e, blocks, gates);
  const before = arena(e);
  assert.equal(e.api.compile_with_gates(blocks.length, gates.length), 0);
  checkArena(e, before, 'legacy compilation remains arena pure');
  const pointer = e.api.module_ptr() >>> 0, length = e.api.module_len() >>> 0;
  const generation = e.api.generation() >>> 0;
  assert.equal(generation, 1);
  assert.ok(pointer > 0 && pointer + length <= refresh(e).bytes.length);
  const bytes = e.bytes.slice(pointer, pointer + length), module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: 'guard', kind: 'function'},
    ...helpers.map(name => ({module: 'ring3', name, kind: 'function'})),
  ]);
  const instance = new WebAssembly.Instance(module, {
    env: {memory: e.memory}, ring3: Object.fromEntries(['guard', ...helpers].map(name => [name, e.api[name]])),
  });
  const file = `legacy-${++stats.legacy_modules}.wasm`; saveArtifact(file, bytes);
  unitBindings.push({key: e.key.toString(), generation, pointer, length, sha256: sha256(bytes), file, blocks, gates});
  return {generation, run: instance.exports.run};
}
function ack(e, unit, slot) {
  e.table.set(slot, unit.run);
  const before = arena(e);
  assert.equal(e.api.acknowledge_resident_installation(e.low, e.high, unit.low, unit.high, slot), 0);
  const record = installation32(unit, slot), expected = before.slice();
  expected.set(record, TRANSFER); checkArena(e, expected, 'pre-suspension exact table acknowledgement');
  assert.equal(e.table.get(slot), unit.run);
  installations.push({key: e.key.toString(), id: unit.id.toString(), slot, exact_reference: true, sha256: sha256(unit.bytes)});
  saveRecord(e, 'R3IN32 acknowledgement', record); stats.acks++;
}
function hostWord(e, address, value, read = false) {
  const before = arena(e);
  assert.equal(read ? e.api.read32(address) : e.api.write32(address, value), 0);
  const expected = before.slice(); expected.set(helper40(read ? value : 0), 100);
  checkArena(e, expected, read ? 'bounded RAM observation' : 'host RAM setup');
  stats[read ? 'host_reads' : 'host_writes']++;
}
function observe(e, words) { for (const [address, value] of words) hostWord(e, address, value, true); }
function direct(e, unit, s, ex, options = {}) {
  const {status = 0, budget = 9, helper, callbackRetired = 0, legacy = false} = options;
  const before = arena(e);
  assert.equal(unit.run(e.base, e.base + 56, budget, e.base + 96), status);
  const expected = before.slice();
  if (!status) { expected.set(state56(s), 0); expected.set(ex, 56); }
  if (helper) expected.set(helper, 100);
  checkArena(e, expected, 'direct product unit guard and actual body');
  if (legacy) stats.legacy_unit_calls++;
  else { stats.unit_calls++; stats[status ? 'unit_rejected' : 'unit_canonical']++; }
  stats.callback_retired += callbackRetired;
}
function dispatch(e, budget, s, ex, options = {}) {
  const {status = 0, child = 0, successes = 0, failures = 0, last, slot, helper, badPointers = false, callbackRetired = 0} = options;
  const before = arena(e);
  assert.equal(e.dispatcher.exports.run(
    badPointers ? 0xffffffff : e.base, badPointers ? 0xffffffff : e.base + 56,
    budget, badPointers ? 0xffffffff : e.base + 96,
  ), status);
  const expected = before.slice();
  if (s) expected.set(state56(s), 0);
  if (ex) expected.set(ex, 56);
  if (helper) expected.set(helper, 100);
  if (last) {
    const record = installation32(last, slot); expected.set(record, TRANSFER);
    assert.equal(e.table.get(slot), last.run);
    installations.push({key: e.key.toString(), id: last.id.toString(), slot, exact_reference: true, sha256: sha256(last.bytes)});
    saveRecord(e, 'R3IN32 selected child', record);
  }
  checkArena(e, expected, 'product dispatcher callback authority and literal complete exit');
  stats.dispatch_calls++; stats[status ? 'dispatch_rejected' : 'dispatch_canonical']++;
  stats.dispatch_children += child; stats.lookup_success += successes; stats.lookup_failure += failures;
  stats.callback_retired += callbackRetired;
}
function capture(e, owner, stopped, token, returnPc, legacy = false) {
  const before = arena(e);
  assert.equal(legacy
    ? e.api.capture_call(e.low, e.high, owner.generation, 1, 0)
    : e.api.capture_resident_call(e.low, e.high, owner.low, owner.high, 1, 0), 0);
  const record = wordsRecord(112, 'R3CF', [token, legacy ? LEGACY : OUTER, 1, 0, stopped.eip, stopped.registers[4], returnPc, 0]);
  const expected = before.slice(); expected.set(record, TRANSFER);
  checkArena(e, expected, 'actual Gate capture and private frozen outer');
  saveRecord(e, 'R3CF112', record, {token, family: legacy ? 'Replacement' : 'Resident'});
  stats[legacy ? 'legacy_captures' : 'resident_captures']++;
}
function begin(e, outer, callback, token, stopped, entry, ret, args, legacy = false, callbackToken = token + 1) {
  refresh(e);
  args.forEach((value, i) => e.view.setUint32(e.base + TRANSFER + i * 4, value, true));
  const before = arena(e);
  assert.equal(legacy
    ? e.api.begin_callback(e.low, e.high, outer.generation, token, entry, ret, LEGACY_RETURN, args.length)
    : e.api.begin_resident_callback(e.low, e.high, outer.low, outer.high, callback.low, callback.high, token, entry, ret, RETURN, args.length), 0);
  const active = copy(stopped); active.eip = entry;
  active.registers[4] = stopped.registers[4] - (args.length + 1) * 4;
  const tail = legacy ? [outer.generation, 0] : [outer.low, outer.high, callback.low, callback.high];
  const record = wordsRecord(legacy ? 64 : 72, legacy ? 'R3CB' : 'R3RC', [
    callbackToken, token, 1, 0, entry, active.registers[4], ret,
    legacy ? LEGACY_RETURN : RETURN, args.length, 0, ...tail,
  ]);
  const expected = before.slice();
  expected.set(state56(active), 0); expected.set(exit40(3), 56); expected.set(record, TRANSFER);
  checkArena(e, expected, 'atomic admitted callback CPU/Exit and unchanged phase1 record');
  saveRecord(e, legacy ? 'R3CB64' : 'R3RC72', record, {token: callbackToken, outcome: 0});
  stats[legacy ? 'legacy_begins' : 'resident_begins']++; stats.frame_words += args.length + 1;
  return {active, record};
}
function authorize(e, unit, token) {
  pure(e, () => e.api.authorize_resident_callback(e.low, e.high, unit.low, unit.high, token),
    0, 'explicit authorization changes only private flag', false);
  stats.authorize_success++;
}
function abort(e, token, stopped, frozenExit) {
  const before = arena(e);
  assert.equal(e.api.abort_callback(e.low, e.high, token), 0);
  const expected = before.slice(); expected.set(state56(stopped), 0); expected.set(frozenExit, 56);
  checkArena(e, expected, 'neutral abort discards result and restores exact private outer'); stats.aborts++;
}
function complete(e, owner, token, value, s) {
  const before = arena(e);
  assert.equal(e.api.complete_resident_call(e.low, e.high, owner.low, owner.high, token, value), 0);
  const expected = before.slice();
  expected.set(state56(s), 0); expected.set(exit40(3), 56);
  checkArena(e, expected, 'explicit outer scalar completion uses frozen frame');
  stats.completions++;
}
function close(e) {
  pure(e, () => e.api.close(), 0, 'close revokes private execution authority'); stats.closed++;
}
function section(name, before) {
  sections.push({name, stats: Object.fromEntries(fields.map(field => [field, stats[field] - before[field]]))});
}

function result48(outer, callback, token, outerToken, value) {
  return wordsRecord(48, 'R3RR', [token, outerToken, value, 0, outer.low, outer.high, callback.low, callback.high]);
}
function finish(e, outer, callback, token, value, stopped, frozenExit, status = 0, options = {}) {
  const before = arena(e);
  assert.equal(e.api.finish_resident_callback(
    options.low ?? e.low, options.high ?? e.high,
    options.unitLow ?? callback.low, options.unitHigh ?? callback.high,
    options.token ?? token,
  ), status);
  const expected = before.slice();
  let record;
  if (!status) {
    record = result48(outer, callback, token, options.outerToken ?? 1, value);
    expected.set(state56(stopped), 0); expected.set(frozenExit, 56);
    expected.set(record, TRANSFER);
    saveRecord(e, 'R3RR48', record, {token, outer_token: options.outerToken ?? 1, result: value});
    stats.result48++;
  }
  checkArena(e, expected, 'finish consumes armed callback and restores exact private pending outer');
  stats[status ? 'finish_rejected' : 'finish_success']++;
  return status ? undefined : arena(e).slice(TRANSFER, TRANSFER + 48);
}

function captureInner(e, cb, callbackToken, stopped, token, status = 0, options = {}) {
  const before = arena(e);
  assert.equal(e.api.capture_resident_callback_call(
    options.low ?? e.low, options.high ?? e.high,
    options.unitLow ?? cb.low, options.unitHigh ?? cb.high,
    options.callbackToken ?? callbackToken, options.convention ?? 2, options.count ?? 1,
  ), status);
  const expected = before.slice();
  let record;
  if (!status) {
    record = wordsRecord(112, 'R3CF', [
      token, INNER, 2, 1, stopped.eip, stopped.registers[4], 0x500a, 0, 0x11223344,
    ]);
    expected.set(record, TRANSFER);
    saveRecord(e, 'R3CF112 inner callback', record, {
      token, callback_token: callbackToken, unit_id: cb.id.toString(),
    });
  }
  checkArena(e, expected, 'named callback inner capture publishes only frozenCF112');
  stats[status ? 'inner_capture_rejected' : 'inner_captures']++;
  return record;
}
function completeInner(e, cb, callbackToken, innerToken, value, completed, status = 0, options = {}) {
  const before = arena(e);
  assert.equal(e.api.complete_resident_callback_call(
    options.low ?? e.low, options.high ?? e.high,
    options.unitLow ?? cb.low, options.unitHigh ?? cb.high,
    options.callbackToken ?? callbackToken, innerToken, value,
  ), status);
  const expected = before.slice();
  if (!status) {
    expected.set(state56(completed), 0); expected.set(exit40(3), 56);
  }
  checkArena(e, expected, 'explicit inner completion consumes only callback-owned pending');
  stats[status ? 'inner_completion_rejected' : 'inner_completions']++;
}
let standaloneFinderCalls = 0, standaloneFinderSuccess = 0, standaloneFinderFailure = 0;
function pendingFinder(e, pc, unit, slot, status = 0) {
  const before = arena(e);
  assert.equal(e.api.find_installed_resident(e.low, e.high, pc), status);
  const expected = before.slice();
  if (!status) {
    const record = installation32(unit, slot); expected.set(record, TRANSFER);
    assert.equal(e.table.get(slot), unit.run);
    installations.push({key: e.key.toString(), id: unit.id.toString(), slot,
      exact_reference: true, sha256: sha256(unit.bytes)});
    saveRecord(e, 'R3IN32 named pending inspection', record);
    standaloneFinderSuccess++;
  } else { standaloneFinderFailure++; stats.other_rejected++; }
  standaloneFinderCalls++;
  checkArena(e, expected, 'finder inspection preserves private pending and callback authority');
}

// all modules and acknowledgements precede the first pending frame.
{
  const before = {...stats}, e = fresh();
  assert.equal(e.api.map(0x3000, 4, 7), 0);
  assert.equal(e.api.map(0x8000, 2, 3), 0);
  upload(e, 0x3000, 'e8fb0f00008d4003', 'caller-call-lea');
  upload(e, 0x4000, '0f0b', 'outer-gate');
  upload(e, 0x5000, '8b44240450e8f60100008d4003c20800', 'callback-inner-call-ret8');
  upload(e, 0x5200, '0f0b', 'ordinary-inner-gate');
  upload(e, 0x5100, '0f0b', 'callback-return-gate');
  upload(e, 0x6000, '0f0b', 'legacy-owner-not-executed');
  const outer = resident(e, [[0x4000, 2]], [[0x4000, OUTER]], [], 'frozen outer');
  const caller = resident(e, [[0x3000, 5], [0x3005, 3]], [], ['store_resident32'], 'real caller');
  const cb = resident(e, [[0x5000, 10], [0x500a, 6], [0x5200, 2], [0x5100, 2]],
    [[0x5200, INNER], [0x5100, RETURN]], ['read32', 'store_resident32'], 'named callback with ordinary inner Gate');
  const legacy = legacyModule(e, [[0x6000, 2]], [[0x6000, LEGACY]]);
  ack(e, outer, 0); ack(e, caller, 1); ack(e, cb, 2);
  const initialWords = [
    [0x8fe8, 0x01234567], [0x8fec, 0x22334455], [0x8ff0, 0x23456789],
    [0x8ff4, 0x3456789a], [0x8ff8, 0x456789ab], [0x8ffc, 0x56789abc], [0x9000, 0x6789abcd],
  ];
  for (const [address, value] of initialWords) hostWord(e, address, value);
  const start = state(), stopped = state(0x4000, 0x8ffc); reset(e, start);
  dispatch(e, 9, stopped, exit40(8, 1, OUTER), {
    child: 2, successes: 2, last: outer, slot: 0, helper: helper40(),
  });
  stats.guest_stores++; capture(e, outer, stopped, 1, 0x3005);
  const admission = begin(e, outer, cb, 1, stopped, 0x5000, 0x5100, [0x11223344, 0x12345678]);
  const admittedFrame = [
    [0x8fe8, 0x01234567], [0x8fec, 0x22334455], [0x8ff0, 0x5100],
    [0x8ff4, 0x11223344], [0x8ff8, 0x12345678], [0x8ffc, 0x3005], [0x9000, 0x6789abcd],
  ];
  observe(e, admittedFrame);
  captureInner(e, cb, 2, undefined, undefined, 12);
  completeInner(e, cb, 2, 3, 0, undefined, 12);
  direct(e, cb, undefined, undefined, {status: 12});
  dispatch(e, 0, undefined, undefined, {status: 12, badPointers: true});
  dispatcherGetter(e); authorize(e, cb, 2);
  const afterPush = copy(admission.active);
  afterPush.eip = 0x5005; afterPush.registers[0] = 0x11223344; afterPush.registers[4] = 0x8fec;
  direct(e, cb, afterPush, exit40(1, 2), {
    budget: 2, helper: helper40(), callbackRetired: 2,
  });
  stats.guest_stores++;
  captureInner(e, cb, 2, undefined, undefined, 13);
  const inner = copy(afterPush); inner.eip = 0x5200; inner.registers[4] = 0x8fe8;
  dispatch(e, 9, inner, exit40(8, 1, INNER), {
    child: 1, successes: 1, last: cb, slot: 2, helper: helper40(), callbackRetired: 1,
  });
  stats.guest_stores++;
  const committedFrame = [
    [0x8fe8, 0x500a], [0x8fec, 0x11223344], [0x8ff0, 0x5100],
    [0x8ff4, 0x11223344], [0x8ff8, 0x12345678], [0x8ffc, 0x3005], [0x9000, 0x6789abcd],
  ];
  observe(e, committedFrame);
  captureInner(e, cb, 2, undefined, undefined, 3, {high: (e.high ^ 1) >>> 0});
  captureInner(e, cb, 2, undefined, undefined, 3, {unitHigh: 1});
  captureInner(e, cb, 2, undefined, undefined, 14, {callbackToken: 0});
  const innerRecord = captureInner(e, cb, 2, inner, 3);
  pendingFinder(e, 0x5200, cb, 2);
  pendingFinder(e, 0x4000, undefined, undefined, 12);
  direct(e, cb, undefined, undefined, {status: 12});
  dispatch(e, 0, undefined, undefined, {status: 12, badPointers: true});
  pure(e, () => e.api.store_resident32(e.low, e.high, cb.low, cb.high, 0x8fe8, 0xdecafbad),
    12, 'inner pending rejects bound store before guest RAM or helper publication');
  finish(e, outer, cb, 2, undefined, undefined, undefined, 12);
  pure(e, () => e.api.capture_resident_call(e.low, e.high, cb.low, cb.high, 99, 17),
    12, 'old Resident capture retains any-callback Busy');
  pure(e, () => e.api.capture_call(e.low, e.high, legacy.generation, 99, 17),
    12, 'old Replacement capture remains excluded by Resident callback');
  pure(e, () => e.api.complete_resident_call(e.low, e.high, cb.low, cb.high, 3, 9),
    14, 'old Resident owner cannot consume callback-owned inner token3');
  pure(e, () => e.api.complete_call(e.low, e.high, legacy.generation, 3, 9),
    14, 'old Replacement owner cannot consume callback-owned inner token3');
  pure(e, () => e.api.abandon_call(e.low, e.high, 1), 12, 'private outer token1 is protected');
  completeInner(e, cb, 2, 1, 0, undefined, 12);
  completeInner(e, cb, 2, 2, 0, undefined, 14);
  completeInner(e, cb, 2, 0, 0, undefined, 14);

  // deliberately malformed negative input is restored exactly, without guest advancement.
  cancel(e, 1); refresh(e).bytes.fill(0xff, e.base, e.base + 96);
  completeInner(e, cb, 2, 3, 0, undefined, 15);
  captureInner(e, cb, 2, undefined, undefined, 12, {convention: 99, count: 17});
  e.bytes.set(state56(inner), e.base); e.bytes.set(exit40(8, 1, INNER), e.base + 56);
  completeInner(e, cb, 2, 3, 0, undefined, 16); cancel(e, 0);
  e.bytes.fill(0x7c, e.base + TRANSFER, e.base + SIZE); dispatcherGetter(e);
  const completedInner = copy(inner);
  completedInner.eip = 0x500a; completedInner.registers[0] = 0xfffffffd;
  completedInner.registers[4] = 0x8ff0;
  completeInner(e, cb, 2, 3, 0xfffffffd, completedInner);
  completeInner(e, cb, 2, 1, 0, undefined, 12);
  completeInner(e, cb, 2, 3, 0, undefined, 14);
  const returned = copy(completedInner);
  returned.eip = 0x5100; returned.registers[0] = 0; returned.registers[4] = 0x8ffc;
  dispatch(e, 9, returned, exit40(8, 2, RETURN), {
    child: 1, successes: 1, last: cb, slot: 2, helper: helper40(0x5100), callbackRetired: 2,
  });
  captureInner(e, cb, 2, undefined, undefined, 13);
  const resultRecord = finish(e, outer, cb, 2, 0, stopped, exit40(8, 1, OUTER));
  const published = new DataView(resultRecord.buffer).getUint32(24, true);
  assert.equal(published, 0);
  dispatch(e, 0, undefined, undefined, {status: 12, badPointers: true});
  const resumed = state(0x3005, 0x9000); resumed.registers[0] = 0;
  complete(e, outer, 1, published, resumed);
  const final = copy(resumed); final.eip = 0x3008; final.registers[0] = 3;
  dispatch(e, 9, final, exit40(3, 1), {
    child: 1, successes: 1, failures: 1, last: caller, slot: 1,
  });
  observe(e, committedFrame); close(e);
  completeInner(e, cb, 0, 0, 0, undefined, 5, {high: (e.high ^ 1) >>> 0});
  flows.push({name: 'callback-owned inner Stdcall1 and real RET8 finish', key: e.key.toString(),
    actual_CALL_initial: state56(start), private_outer: state56(stopped),
    initial_callback: state56(admission.active), after_MOV_PUSH: state56(afterPush),
    ordinary_inner_Gate: state56(inner), inner_Gate_Exit: exit40(8, 1, INNER),
    completed_inner: state56(completedInner), completed_inner_Exit: exit40(3),
    callback_return: state56(returned), callback_return_Exit: exit40(8, 2, RETURN),
    initial_admission_hex: Buffer.from(admission.record).toString('hex'),
    inner_capture_hex: Buffer.from(innerRecord).toString('hex'), result48_hex: Buffer.from(resultRecord).toString('hex'),
    explicit_outer_completed: state56(resumed), actual_caller_final: state56(final),
    admittedFrame, committedFrame, retained_callback_token: 2, global_inner_token: 3,
    inner_stdcall_cleanup_bytes: 8, callback_RET_cleanup_bytes: 12,
    inner_host_scalar: 0xfffffffd, callback_result: 0, actual_caller_result: 3,
    named_pending_finder_transfer_bytes: 32, legacy_guest_instructions_retired: 0});
  section('named callback inner owner, frozen completion and scalar finish', before);
}

{
  const before = {...stats}, e = fresh();
  assert.equal(e.api.map(0x3000, 3, 7), 0);
  assert.equal(e.api.map(0x8000, 2, 3), 0);
  upload(e, 0x3000, 'e8fb0f00008d4003', 'caller-call-lea');
  upload(e, 0x4000, '0f0b', 'outer-gate');
  upload(e, 0x5000, '8b44240450e8f60100008d4003c20800', 'callback-inner-call-ret8');
  upload(e, 0x5200, '0f0b', 'ordinary-inner-gate');
  upload(e, 0x5100, '0f0b', 'callback-return-gate');
  const outer = resident(e, [[0x4000, 2]], [[0x4000, OUTER]], [], 'stale recovery outer');
  const caller = resident(e, [[0x3000, 5], [0x3005, 3]], [], ['store_resident32'], 'recovery real caller');
  const cb = resident(e, [[0x5000, 10], [0x500a, 6], [0x5200, 2], [0x5100, 2]],
    [[0x5200, INNER], [0x5100, RETURN]], ['read32', 'store_resident32'], 'current recovery callback');
  ack(e, outer, 0); ack(e, caller, 1); ack(e, cb, 2);
  const initialWords = [
    [0x8fe8, 0x01234567], [0x8fec, 0x22334455], [0x8ff0, 0x23456789],
    [0x8ff4, 0x3456789a], [0x8ff8, 0x456789ab], [0x8ffc, 0x56789abc], [0x9000, 0x6789abcd],
  ];
  for (const [address, value] of initialWords) hostWord(e, address, value);
  const start = state(), stopped = state(0x4000, 0x8ffc); reset(e, start);
  dispatch(e, 9, stopped, exit40(8, 1, OUTER), {
    child: 2, successes: 2, last: outer, slot: 0, helper: helper40(),
  });
  stats.guest_stores++; capture(e, outer, stopped, 1, 0x3005);
  const admission = begin(e, outer, cb, 1, stopped, 0x5000, 0x5100, [0x11223344, 0x12345678]);
  authorize(e, cb, 2);
  const inner = copy(admission.active);
  inner.eip = 0x5200; inner.registers[0] = 0x11223344; inner.registers[4] = 0x8fe8;
  dispatch(e, 9, inner, exit40(8, 3, INNER), {
    child: 1, successes: 1, last: cb, slot: 2, helper: helper40(), callbackRetired: 3,
  });
  stats.guest_stores += 2;
  const innerRecord = captureInner(e, cb, 2, inner, 3);
  const committedFrame = [
    [0x8fe8, 0x500a], [0x8fec, 0x11223344], [0x8ff0, 0x5100],
    [0x8ff4, 0x11223344], [0x8ff8, 0x12345678], [0x8ffc, 0x3005], [0x9000, 0x6789abcd],
  ];
  observe(e, committedFrame);
  hostWord(e, 0x4000, 0x00000b0f); // same bytes invalidate only the retained outer page.
  cancel(e, 1); refresh(e).bytes.fill(0xff, e.base, e.base + 96);
  completeInner(e, cb, 2, 3, 0, undefined, 4);
  completeInner(e, cb, 2, 0, 0, undefined, 4);
  captureInner(e, cb, 2, undefined, undefined, 4, {convention: 99, count: 17});
  captureInner(e, cb, 2, undefined, undefined, 14, {callbackToken: 0});
  finish(e, outer, cb, 2, undefined, undefined, undefined, 4);
  dispatch(e, 0, undefined, undefined, {status: 12, badPointers: true});
  abort(e, 2, stopped, exit40(8, 1, OUTER));
  completeInner(e, cb, 2, 3, 0, undefined, 14);
  pure(e, () => e.api.abort_callback(e.low, e.high, 2), 14, 'neutral abort consumed callback2 and inner3');
  pure(e, () => e.api.complete_resident_call(e.low, e.high, outer.low, outer.high, 1, 0),
    4, 'restored stale outer remains protected by its current-unit check');
  pure(e, () => e.api.abandon_call(e.low, e.high, 1), 0, 'explicit recovery abandons restored outer1');
  stats.outer_abandons++; observe(e, committedFrame); close(e);
  captureInner(e, cb, 0, undefined, undefined, 5, {high: (e.high ^ 1) >>> 0});
  flows.push({name: 'outer-only stale inner prefix and neutral revocation', key: e.key.toString(),
    actual_CALL_initial: state56(start), private_outer: state56(stopped),
    initial_callback: state56(admission.active), ordinary_inner_Gate: state56(inner),
    inner_Gate_Exit: exit40(8, 3, INNER), initial_admission_hex: Buffer.from(admission.record).toString('hex'),
    inner_capture_hex: Buffer.from(innerRecord).toString('hex'), committedFrame,
    abort_restored_outer: state56(stopped), abort_restored_Exit: exit40(8, 1, OUTER),
    retained_outer_token: 1, consumed_callback_token: 2, discarded_inner_token: 3,
    cancel_after_abort: 1, callback_unit_still_current: true, outer_snapshot_stale: true,
    committed_callback_guest_retired: 3, body_after_inner_Gate_executed: false});
  section('retained parent currency and forced inner revocation', before);
}

// all40 fields and both whole rows were independently fixed before execution.
const expectedRows = [
  [1, 3, 1, 3, 3, 3, 7, 4, 3, 5, 5, 1, 3, 1, 2, 0, 1, 0, 1, 0, 1, 1, 7, 1, 9, 0, 1, 1, 1, 0, 1, 7, 21, 7, 81, 5, 3, 3, 1, 0],
  [1, 3, 0, 3, 1, 3, 3, 2, 1, 3, 3, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 3, 0, 3, 0, 0, 1, 0, 1, 0, 2, 14, 8, 48, 3, 3, 3, 1, 1],
];
const expectedTotal = [2, 6, 1, 6, 4, 6, 10, 6, 4, 8, 8, 1, 3, 1, 2, 0, 2, 0, 2, 0, 2, 2, 10, 1, 12, 0, 1, 2, 1, 1, 1, 9, 35, 15, 129, 8, 6, 6, 2, 1];
const rowObject = row => Object.fromEntries(fields.map((field, index) => [field, row[index]]));
assert.deepEqual(stats, rowObject(expectedTotal), 'all40 aggregate counters fixed before runtime');
assert.deepEqual(sections.map(value => value.stats), expectedRows.map(rowObject), 'both complete independent rows');
assert.deepEqual(expectedRows.reduce((sum, row) => row.map((value, index) => sum[index] + value), fields.map(() => 0)), expectedTotal);
assert.equal(Object.keys(authored).length, 11);
assert.equal(Object.keys(artifacts).length, 20);
assert.equal(unitBindings.length, 7);
assert.equal(installations.length, 13);
assert.equal(records.length, 30);
assert.equal(records.filter(record => record.kind === 'R3RR48').length, 1);
assert.equal(records.filter(record => record.kind === 'R3RC72').length, 2);
assert.equal(records.filter(record => record.kind === 'R3CF112').length, 2);
assert.equal(records.filter(record => record.kind === 'R3CF112 inner callback').length, 2);
assert.equal(records.filter(record => record.kind === 'R3DP32').length, 4);
assert.equal(standaloneFinderCalls, 2);
assert.equal(standaloneFinderSuccess, 1);
assert.equal(standaloneFinderFailure, 1);
for (const [path, bytes] of Object.entries(sourceBytes)) {
  assert.deepEqual(readFileSync(join(root, path)), bytes, `${path}: full source bytes unchanged during proof`);
}
function printableFlow(value) {
  if (value instanceof Uint8Array) return {hex: Buffer.from(value).toString('hex'), sha256: sha256(value)};
  if (Array.isArray(value)) return value.map(printableFlow);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, printableFlow(item)]));
  return value;
}
const provenance = {
  stats, statistic_fields: fields, section_derivation: sections,
  engine_sha256: sha256(engineBytes), engine_bytes: engineBytes.length, executed_engine_file: 'executed-engine.wasm',
  artifact_sha256: artifacts,
  module_set_sha256: sha256(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, sha256(sourceBytes[path])])),
  source_snapshot_directory: 'sources', source_snapshot_before_first_engine_instance: true,
  production: {
    git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(),
    source_files: productionPaths.length,
    source_set_sha256: sha256(productionPaths.map(path => `${path}:${sha256(sourceBytes[path])}`).join('\n')),
  },
  authored, flows: flows.map(printableFlow), exact_raw_records: records,
  module_bindings: unitBindings, installed_reference_observations: installations,
  executions: {
    product_engine_instances: 2, product_dispatcher_instances: 2,
    emitted_resident_modules: 6, emitted_legacy_modules: 1,
    emitted_resident_guard_entries_from_authored_paths: 11,
    emitted_legacy_guard_entries_from_authored_paths: 0,
    product_dispatch_entry_guard_entries: 10, dispatcher_finder_calls: 9,
    standalone_finder_calls: 2, standalone_finder_success: 1, standalone_finder_failure: 1,
    callback_guest_body_entries: 4, callback_guest_instructions_retired: 8,
    bound_store_identity_checks_from_guest_imports: 6, direct_bound_store_rejections: 1,
    legacy_guest_instructions_retired: 0,
  },
  tools: {
    node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch,
    rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(),
    cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim(),
  },
  commands: {
    run: [process.execPath, process.argv[1], enginePath, outputDir, root],
    generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_callback_call_wasm', '--', '--nocapture'],
  },
  counts: 'all40 counters and both complete rows were independently derived before runtime. Each of129 full-arena checks compares all4236 bytes against pre-operation input plus only independent full State56/Exit40/helper40/known record bytes. Source-derived counts are not intercepted imports or Wasm instrumentation. metadata32 counts four immutable dispatcher getter publications only; acknowledgements and standalone finder records have their own exact records. Dispatcher lookup counters cover nine calls (eight success/one final caller miss); two direct finder inspections are separately recorded, one named success/one foreign Busy. Thirty ordered raw records, seven module bindings and thirteen strict function-reference observations are preserved. All20 complete input/generated artifacts include nine emitted Wasm modules and eleven x86 inputs with21 instruction boundaries; executed engine bytes are separate. Every one of92 explicit portable source/config/proof paths is copied before the first engine instance and verified byte-for-byte unchanged after proof.',
  parity: 'root separately runs the unchanged332 finish target,331 execution target and old Replacement callbacks target once and compares complete selected artifacts/counters/rows/records with pinned references. This portable producer reads no ignored historical output. Unaffected foundation is reused by current full source equality.',
  claim: 'one explicitly authorized distinct-unit Resident callback owns one ordinary inner pending at a time through additive token-named capture/complete APIs. Main real callerCALL reaches outerGate/capture1/begin2; direct MOV/PUSH then product dispatcherCALL reaches ordinaryINNER5200, captures3 Stdcall1 with exact return500a/arg11223344/ESP8fe8. A named finder32 publication and immutable dispatcher getter overwrite publicCF112 without changing private authority. Protected outer1, old Resident/Replacement owners, parked guards, frozen CPU and cancel controls preserve complete outputs. Explicit inner scalarfffffffd restores ESP8ff0/PC500a; callback LEA3 wraps0 and RET8 restores ESP8ffc at registered return5100. Finish2 publishes result0/restores exact outer1; explicit outercompletion and actual callerLEA produce3. Recovery commits MOV/PUSH/CALL then same-byte outer-only invalidation, cancel and malformed CPU reject newcompletion4 without losing inner prefix; neutralabort2 discards inner3/callback2, restores exact stale outer1 and preserves committed RAM/helper/cancel/transfer. Dead inner3 rejects14 while callback unit stayscurrent, restored outercompletion4 then explicitabandon/close recovers. Callback retirement is5 main/3 recovery and no callback body executes after recoveryINNER. MAX/global-token retention/abandon-retry and wider priorities are native/source evidence; actual RAM scope is35 bounded four-byte frame/neighbour observations, not whole guest RAM. Prepared legacy Gate never executes. No nested callback, new ISA, return-target or hostTable/Memory authentication, migration/provider/SDK/browser/async/race/performance/fullP2-V0/gameplay claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({
  stats, section_derivation: sections, engine_sha256: provenance.engine_sha256,
  module_set_sha256: provenance.module_set_sha256, sources: sourcePaths.length,
  artifacts: Object.keys(artifacts).length, output: outputDir,
}));
