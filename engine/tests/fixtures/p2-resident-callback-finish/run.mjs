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
const OUTER = 0x80000011, RETURN = 0x80000012;
const LEGACY = 0x80000077, LEGACY_RETURN = 0x80000078;
const apiNames = [
  'open', 'close', 'arena_ptr', 'map', 'upload', 'compile_with_gates',
  'module_ptr', 'module_len', 'generation', 'compile_resident_with_gates',
  'dispatcher_module', 'guard_dispatch_entry', 'guard_resident', 'guard',
  'read32', 'write32', 'store_resident32', 'acknowledge_resident_installation',
  'find_installed_resident', 'capture_resident_call', 'complete_resident_call',
  'capture_call', 'complete_call', 'abandon_call', 'begin_resident_callback',
  'authorize_resident_callback', 'finish_resident_callback', 'begin_callback', 'finish_callback',
  'resume_callback_code', 'resume_callback_entries', 'abort_callback',
];
const fields = [
  'engines', 'resident_units', 'legacy_modules', 'metadata24', 'metadata32',
  'acks', 'dispatch_calls', 'dispatch_canonical', 'dispatch_rejected',
  'dispatch_children', 'lookup_success', 'lookup_failure', 'unit_calls',
  'unit_canonical', 'unit_rejected', 'legacy_unit_calls', 'resident_captures',
  'legacy_captures', 'resident_begins', 'legacy_begins', 'authorize_success',
  'finish_success', 'finish_rejected', 'result48',
  'aborts', 'completions', 'other_rejected', 'host_reads', 'host_writes', 'arena_checks',
  'callback_retired', 'guest_stores', 'frame_words', 'closed',
];
const stats = Object.fromEntries(fields.map(name => [name, 0]));
const artifacts = {}, authored = {}, records = [], unitBindings = [];
const installations = [], sections = [], flows = [];
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [
  ...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock',
  'engine/tests/abi_resident_callback_result.rs', 'engine/tests/process_resident_callback_finish.rs',
  'engine/tests/process_resident_callback_finish_wasm.rs', 'engine/tests/fixtures/p2-resident-callback-finish/run.mjs',
  'engine/tests/abi_resident_callback.rs', 'engine/tests/process_resident_callback.rs',
  'engine/tests/process_resident_callback_execution.rs', 'engine/tests/process_resident_callback_execution_wasm.rs',
  'engine/tests/fixtures/p2-resident-callback-execution/run.mjs',
  'engine/tests/abi_callback.rs', 'engine/tests/process_callbacks.rs', 'engine/tests/callbacks_wasm.rs',
  'engine/tests/fixtures/p2-callbacks/run.mjs', 'engine/tests/fixtures/p2-callbacks/integer.S',
];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
assert.equal(sourcePaths.length, productionPaths.length + 17);
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

// all positive installation references are prepared and fixed before suspension.
{
  const before = {...stats}, e = fresh();
  assert.equal(e.api.map(0x3000, 3, 7), 0);
  assert.equal(e.api.map(0x8000, 2, 3), 0);
  upload(e, 0x3000, 'e8fb0f00008d4003', 'caller-call-lea');
  upload(e, 0x4000, '0f0b', 'outer-gate');
  upload(e, 0x5000, '8d40038d4905c3', 'callback-lea-eax-ecx-ret');
  upload(e, 0x5100, '0f0b', 'callback-return-gate');
  const outer = resident(e, [[0x4000, 2]], [[0x4000, OUTER]], [], 'zero-argument outer');
  const caller = resident(e, [[0x3000, 5], [0x3005, 3]], [], ['store_resident32'], 'real caller');
  const cb = resident(e, [[0x5000, 7], [0x5100, 2]], [[0x5100, RETURN]], ['read32'], 'zero-argument callback');
  ack(e, outer, 0); ack(e, caller, 1); ack(e, cb, 2);
  const initialWords = [[0x8ff4, 0x11223344], [0x8ff8, 0x23456789], [0x8ffc, 0x3456789a], [0x9000, 0x456789ab]];
  for (const [address, value] of initialWords) hostWord(e, address, value);
  const start = state(), stopped = state(0x4000, 0x8ffc);
  reset(e, start);
  dispatch(e, 9, stopped, exit40(8, 1, OUTER), {child: 2, successes: 2, last: outer, slot: 0, helper: helper40()});
  stats.guest_stores++;
  capture(e, outer, stopped, 1, 0x3005);
  const first = begin(e, outer, cb, 1, stopped, 0x5000, 0x5100, []);
  const frame = [[0x8ff4, 0x11223344], [0x8ff8, 0x5100], [0x8ffc, 0x3005], [0x9000, 0x456789ab]];
  observe(e, frame);
  finish(e, outer, cb, 2, undefined, undefined, undefined, 12);
  direct(e, cb, undefined, undefined, {status: 12});
  dispatcherGetter(e); authorize(e, cb, 2);
  cancel(e, 1); finish(e, outer, cb, 2, undefined, undefined, undefined, 13); cancel(e, 0);
  const returned = copy(first.active);
  returned.eip = 0x5100; returned.registers[4] = 0x8ffc;
  returned.registers[0] = 0x89abcdf2; returned.registers[1] = 8;
  dispatch(e, 9, returned, exit40(8, 3, RETURN), {
    child: 1, successes: 1, last: cb, slot: 2, helper: helper40(0x5100), callbackRetired: 3,
  });
  finish(e, outer, cb, 2, undefined, undefined, undefined, 3, {high: (e.high ^ 1) >>> 0});
  finish(e, outer, cb, 2, undefined, undefined, undefined, 3, {unitHigh: 1});
  finish(e, outer, cb, 2, undefined, undefined, undefined, 14, {token: 0});
  refresh(e).view.setUint32(e.base + 32, 0x8ff8, true); cancel(e, 1);
  finish(e, outer, cb, 2, undefined, undefined, undefined, 13);
  e.bytes.set(state56(returned), e.base);
  finish(e, outer, cb, 2, undefined, undefined, undefined, 16); cancel(e, 0);
  dispatcherGetter(e);
  e.bytes.fill(0x7c, e.base + TRANSFER, e.base + SIZE);
  const resultRecord = finish(e, outer, cb, 2, 0x89abcdf2, stopped, exit40(8, 1, OUTER));
  const published = new DataView(resultRecord.buffer).getUint32(24, true);
  assert.equal(published, 0x89abcdf2);
  observe(e, frame);
  finish(e, outer, cb, 2, undefined, undefined, undefined, 14);
  pure(e, () => e.api.abort_callback(e.low, e.high, 2), 14, 'consumed callback token cannot be aborted');
  dispatch(e, 0, undefined, undefined, {status: 12, badPointers: true});
  direct(e, cb, undefined, undefined, {status: 12});
  pure(e, () => e.api.acknowledge_resident_installation(e.low, e.high, cb.low, cb.high, 8), 12, 'restored pending excludes acknowledgement before slot validation');

  // fresh callback keeps outer token1, receives token3 and starts unarmed.
  const second = begin(e, outer, cb, 1, stopped, 0x5000, 0x5100, [], false, 3);
  finish(e, outer, cb, 3, undefined, undefined, undefined, 12);
  authorize(e, cb, 3);
  dispatch(e, 9, returned, exit40(8, 3, RETURN), {
    child: 1, successes: 1, last: cb, slot: 2, helper: helper40(0x5100), callbackRetired: 3,
  });
  hostWord(e, 0x5000, 0x8d03408d); // same bytes invalidate the retained callback page.
  cancel(e, 1); refresh(e).bytes.fill(0xff, e.base, e.base + 96);
  finish(e, outer, cb, 3, undefined, undefined, undefined, 4);
  abort(e, 3, stopped, exit40(8, 1, OUTER)); cancel(e, 0);
  const resumed = state(0x3005, 0x9000); resumed.registers[0] = 0x89abcdf2;
  complete(e, outer, 1, published, resumed);
  const final = copy(resumed); final.eip = 0x3008; final.registers[0] = 0x89abcdf5;
  dispatch(e, 9, final, exit40(3, 1), {child: 1, successes: 1, failures: 1, last: caller, slot: 1});
  observe(e, frame); close(e);
  finish(e, outer, cb, 0, undefined, undefined, undefined, 5, {high: (e.high ^ 1) >>> 0});
  flows.push({name: 'RET0 scalar finish and fresh callback stale recovery', key: e.key.toString(),
    actual_CALL_initial: state56(start), private_outer: state56(stopped),
    first_initial_callback: state56(first.active), callback_return: state56(returned),
    initial_admission_hex: Buffer.from(first.record).toString('hex'), result48_hex: Buffer.from(resultRecord).toString('hex'),
    fresh_initial_callback: state56(second.active), fresh_admission_hex: Buffer.from(second.record).toString('hex'),
    explicit_completed: state56(resumed), actual_caller_final: state56(final), frame,
    restored_outer_ecx: 3, discarded_callback_ecx: 8, first_published_result: 0x89abcdf2,
    fresh_stale_abort_uses_original_outer_token: 1});
  section('zero-argument RET result, exact pending restoration and stale fresh callback', before);
}

{
  const before = {...stats}, e = fresh();
  assert.equal(e.api.map(0x3000, 4, 7), 0);
  assert.equal(e.api.map(0x8000, 2, 3), 0);
  upload(e, 0x3000, 'e8fb0f00008d4003', 'caller-call-lea');
  upload(e, 0x4000, '0f0b', 'outer-gate');
  upload(e, 0x5000, '8b4424048d4003c20800', 'callback-read-arg-lea-ret8');
  upload(e, 0x5100, '0f0b', 'callback-return-gate');
  upload(e, 0x6000, '0f0b', 'legacy-outer-gate');
  upload(e, 0x6100, '8d4003c3', 'legacy-callback-not-executed');
  upload(e, 0x6200, '0f0b', 'legacy-return-gate');
  const outer = resident(e, [[0x4000, 2]], [[0x4000, OUTER]], [], 'two-argument outer');
  const caller = resident(e, [[0x3000, 5], [0x3005, 3]], [], ['store_resident32'], 'real caller');
  const cb = resident(e, [[0x5000, 10], [0x5100, 2]], [[0x5100, RETURN]], ['read32'], 'callback reads first stack arg and wraps to zero');
  const legacy = legacyModule(e, [[0x6000, 2], [0x6100, 4], [0x6200, 2]], [[0x6000, LEGACY], [0x6200, LEGACY_RETURN]], ['read32']);
  ack(e, outer, 0); ack(e, caller, 1); ack(e, cb, 2);
  const initialWords = [
    [0x8fec, 0x11223344], [0x8ff0, 0x23456789], [0x8ff4, 0x3456789a],
    [0x8ff8, 0x456789ab], [0x8ffc, 0x56789abc], [0x9000, 0x6789abcd],
  ];
  for (const [address, value] of initialWords) hostWord(e, address, value);
  const start = state(), stopped = state(0x4000, 0x8ffc); reset(e, start);
  dispatch(e, 9, stopped, exit40(8, 1, OUTER), {child: 2, successes: 2, last: outer, slot: 0, helper: helper40()});
  stats.guest_stores++; capture(e, outer, stopped, 1, 0x3005);
  const first = begin(e, outer, cb, 1, stopped, 0x5000, 0x5100, [0xfffffffd, 0x12345678]);
  const frame = [
    [0x8fec, 0x11223344], [0x8ff0, 0x5100], [0x8ff4, 0xfffffffd],
    [0x8ff8, 0x12345678], [0x8ffc, 0x3005], [0x9000, 0x6789abcd],
  ];
  observe(e, frame);
  finish(e, outer, cb, 2, undefined, undefined, undefined, 12);
  dispatcherGetter(e); authorize(e, cb, 2);
  const afterRead = copy(first.active); afterRead.eip = 0x5004; afterRead.registers[0] = 0xfffffffd;
  direct(e, cb, afterRead, exit40(1, 1), {budget: 1, helper: helper40(0xfffffffd), callbackRetired: 1});
  const returned = copy(afterRead); returned.eip = 0x5100; returned.registers[4] = 0x8ffc; returned.registers[0] = 0;
  dispatch(e, 9, returned, exit40(8, 2, RETURN), {
    child: 1, successes: 1, last: cb, slot: 2, helper: helper40(0x5100), callbackRetired: 2,
  });
  pure(e, () => e.api.finish_callback(e.low, e.high, legacy.generation, 2), 14, 'legacy finish cannot consume Resident callback token');
  const resultRecord = finish(e, outer, cb, 2, 0, stopped, exit40(8, 1, OUTER));
  const published = new DataView(resultRecord.buffer).getUint32(24, true); assert.equal(published, 0);
  dispatch(e, 0, undefined, undefined, {status: 12}); observe(e, frame);
  const resumed = state(0x3005, 0x9000); resumed.registers[0] = 0;
  complete(e, outer, 1, published, resumed);
  const final = copy(resumed); final.eip = 0x3008; final.registers[0] = 3;
  dispatch(e, 9, final, exit40(3, 1), {child: 1, successes: 1, failures: 1, last: caller, slot: 1});
  observe(e, frame);

  // auxiliary legacy Gate uses a host-authored return word; no callback body runs.
  hostWord(e, 0x9000, 0x3005);
  const legacyStop = state(0x6000, 0x9000); reset(e, legacyStop);
  direct(e, legacy, legacyStop, exit40(8, 0, LEGACY), {legacy: true});
  capture(e, legacy, legacyStop, 3, 0x3005, true);
  const replacement = begin(e, legacy, legacy, 3, legacyStop, 0x6100, 0x6200, [], true);
  finish(e, outer, outer, 4, undefined, undefined, undefined, 14);
  close(e);
  flows.push({name: 'RET8 stack argument result wraps to zero and family alias control', key: e.key.toString(),
    actual_CALL_initial: state56(start), private_outer: state56(stopped),
    initial_callback: state56(first.active), after_argument_read: state56(afterRead), callback_return: state56(returned),
    admission_hex: Buffer.from(first.record).toString('hex'), result48_hex: Buffer.from(resultRecord).toString('hex'),
    explicit_completed: state56(resumed), actual_caller_final: state56(final), frame,
    first_argument: 0xfffffffd, published_callback_result: 0, actual_caller_result: 3,
    replacement_initial: state56(replacement.active), replacement64_hex: Buffer.from(replacement.record).toString('hex'),
    replacement_generation_aliases_requested_Resident_ID: 1, legacy_callback_guest_retired: 0});
  section('two-argument RET8 reads stack, publishes zero and preserves family boundary', before);
}

// complete rows and their aggregate were independently derived before execution.
const expectedRows = [
  [1, 3, 0, 3, 3, 3, 5, 4, 1, 5, 5, 1, 2, 0, 2, 0, 1, 0, 2, 0, 2, 1, 11, 1, 1, 1, 2, 12, 5, 55, 6, 1, 2, 1],
  [1, 3, 1, 3, 2, 3, 4, 3, 1, 4, 4, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 2, 1, 0, 1, 1, 18, 7, 51, 3, 1, 4, 1],
];
const expectedTotal = [2, 6, 1, 6, 5, 6, 9, 7, 2, 9, 9, 2, 3, 1, 2, 1, 2, 1, 3, 1, 3, 2, 13, 2, 1, 2, 3, 30, 12, 106, 9, 2, 6, 2];
const rowObject = row => Object.fromEntries(fields.map((field, index) => [field, row[index]]));
assert.deepEqual(stats, rowObject(expectedTotal), 'all34 aggregate counters were fixed before runtime');
assert.deepEqual(sections.map(value => value.stats), expectedRows.map(rowObject), 'all2 complete independent rows');
assert.deepEqual(expectedRows.reduce((sum, row) => row.map((value, index) => sum[index] + value), fields.map(() => 0)), expectedTotal);
assert.equal(Object.keys(authored).length, 11);
assert.equal(Object.keys(artifacts).length, 20);
assert.equal(unitBindings.length, 7);
assert.equal(installations.length, 13);
assert.equal(records.length, 33);
assert.equal(records.filter(record => record.kind === 'R3RR48').length, 2);
assert.equal(records.filter(record => record.kind === 'R3RC72').length, 3);
assert.equal(records.filter(record => record.kind === 'R3CB64').length, 1);
assert.equal(records.filter(record => record.kind === 'R3CF112').length, 3);
assert.equal(records.filter(record => record.kind === 'R3DP32').length, 5);
for (const [path, bytes] of Object.entries(sourceBytes)) {
  assert.deepEqual(readFileSync(join(root, path)), bytes, `${path}: input bytes unchanged during proof`);
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
    emitted_resident_guard_entries_from_authored_paths: 12,
    emitted_legacy_guard_entries_from_authored_paths: 1,
    product_dispatch_entry_guard_entries: 9, dispatcher_finder_calls: 11,
    callback_guest_body_entries: 4, callback_guest_instructions_retired: 9,
    bound_store_identity_checks: 2, legacy_callback_guest_instructions_retired: 0,
  },
  tools: {
    node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch,
    rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(),
    cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim(),
  },
  commands: {
    run: [process.execPath, process.argv[1], enginePath, outputDir, root],
    generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_callback_finish_wasm', '--', '--nocapture'],
  },
  counts: 'all34 counters and both complete rows were independently derived before runtime; none are Wasm instrumentation or intercepted imports. Each of106 full-arena checks compares all4236 bytes against pre-operation input plus only independent complete State56/Exit40/helper40/known record bytes. Dispatcher success/failure counts cover its11 finder calls only. Callback bodies retire9 instructions across four actual body entries: first RET0 and fresh RET0 each3, stack-argument MOV1 then LEA/RET8 two more. Initial and resumed CPU/flags/records/results are independent literals. Explicit outercompletion reads the validated published result48 but its expected CPU and caller result remain fixed independently. All20 generated/input artifacts,33 raw records,7 immutable module bindings and13 strict table observations are preserved; engine bytes are a separate executed-engine file. Every source/config/authored input is copied before the first engine instance and verified unchanged after the proof.',
  parity: 'root separately runs the unchanged331 execution target and old callbacks target once, compares their complete selected artifacts/counters/rows/records with pinned references, and reuses unaffected source-stable foundation. This portable target reads no ignored historical output.',
  claim: 'one-level distinct-unit Resident scalar finish and exact private pending restoration only. Two real CALLs reach outer Gate and capture1/begin2/authorize. First callback LEA EAX+3/LEA ECX+5/RET0 returns EAX89abcdf2/ECX8 at registered Gate; newR3RR48 carries exact result and owner tokens/fullIDs while frozen outerECX3/State/Exit return byte-for-byte. Pending execution remainsBusy. A fresh callback receives token3/defaultunarmed, executes, then same-byte callback invalidation makes finish4 preserve prefix; neutralabort restores original outer token1. The first published result is explicitly completed and actual callerLEA produces89abcdf5. Second callback reads first stack argfffffffd through existing Read32, LEA+3 wraps to0, RET8 restores original outerESP, finish publishes0, and explicit completion/actual callerLEA produces3. Public transfer overwrite/getter reuse, wrongkey/highID/token, unarmed/nonGate/wrongESP/cancel/stale/repeat/closed and both family alias directions are controlled; finish is not automatic outercompletion or stack reread. Auxiliary legacy Gate has host-authored return word; its callback body never executes. RAM proof is30 bounded four-byte frame/neighbour reads, not all guest RAM. Admission72/legacy64 stay unchanged. No innercalls/sameunitexecution/migration/provider/SDK/browser/async/race/table authentication/eviction/newISA/performance/fullP2-V0/gameplay claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({
  stats, section_derivation: sections, engine_sha256: provenance.engine_sha256,
  module_set_sha256: provenance.module_set_sha256, sources: sourcePaths.length,
  artifacts: Object.keys(artifacts).length, output: outputDir,
}));
