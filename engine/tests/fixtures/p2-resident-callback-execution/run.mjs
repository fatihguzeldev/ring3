import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

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
  'authorize_resident_callback', 'begin_callback', 'finish_callback',
  'resume_callback_code', 'resume_callback_entries', 'abort_callback',
];
const fields = [
  'engines', 'resident_units', 'legacy_modules', 'metadata24', 'metadata32',
  'acks', 'dispatch_calls', 'dispatch_canonical', 'dispatch_rejected',
  'dispatch_children', 'lookup_success', 'lookup_failure', 'unit_calls',
  'unit_canonical', 'unit_rejected', 'legacy_unit_calls', 'resident_captures',
  'legacy_captures', 'resident_begins', 'legacy_begins', 'authorize_success',
  'authorize_rejected', 'aborts', 'completions', 'completion_rejected',
  'abandoned', 'other_rejected', 'host_reads', 'host_writes', 'arena_checks',
  'callback_retired', 'guest_stores', 'frame_words', 'closed',
];
const stats = Object.fromEntries(fields.map(name => [name, 0]));
const artifacts = {}, authored = {}, records = [], unitBindings = [];
const installations = [], sections = [], flows = [];

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
function begin(e, outer, callback, token, stopped, entry, ret, args, legacy = false) {
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
    token + 1, token, 1, 0, entry, active.registers[4], ret,
    legacy ? LEGACY_RETURN : RETURN, args.length, 0, ...tail,
  ]);
  const expected = before.slice();
  expected.set(state56(active), 0); expected.set(exit40(3), 56); expected.set(record, TRANSFER);
  checkArena(e, expected, 'atomic admitted callback CPU/Exit and unchanged phase1 record');
  saveRecord(e, legacy ? 'R3CB64' : 'R3RC72', record, {token: token + 1, outcome: 0});
  stats[legacy ? 'legacy_begins' : 'resident_begins']++; stats.frame_words += args.length + 1;
  return {active, record};
}
function authorize(e, unit, token, status = 0, options = {}) {
  pure(e, () => e.api.authorize_resident_callback(
    options.low ?? e.low, options.high ?? e.high,
    options.unitLow ?? unit.low, options.unitHigh ?? unit.high,
    options.token ?? token,
  ), status, 'explicit authorization changes only private flag', false);
  stats[status ? 'authorize_rejected' : 'authorize_success']++;
}
function abort(e, token, stopped, frozenExit) {
  const before = arena(e);
  assert.equal(e.api.abort_callback(e.low, e.high, token), 0);
  const expected = before.slice(); expected.set(state56(stopped), 0); expected.set(frozenExit, 56);
  checkArena(e, expected, 'neutral abort discards result and restores exact private outer'); stats.aborts++;
}
function complete(e, owner, token, value, s, status = 0) {
  const before = arena(e);
  assert.equal(e.api.complete_resident_call(e.low, e.high, owner.low, owner.high, token, value), status);
  const expected = before.slice();
  if (!status) { expected.set(state56(s), 0); expected.set(exit40(3), 56); }
  checkArena(e, expected, 'explicit outer scalar completion uses frozen frame');
  stats[status ? 'completion_rejected' : 'completions']++;
}
function close(e) {
  pure(e, () => e.api.close(), 0, 'close revokes private execution authority'); stats.closed++;
}
function section(name, before) {
  sections.push({name, stats: Object.fromEntries(fields.map(field => [field, stats[field] - before[field]]))});
}

// all code and positive installation acknowledgements precede suspension.
// literal CPU images below come from authored instructions, not engine output.
{
  const before = {...stats}, e = fresh();
  assert.equal(e.api.map(0x3000, 4, 7), 0);
  assert.equal(e.api.map(0x8000, 2, 3), 0);
  assert.equal(e.api.map(0xa000, 1, 3), 0);
  upload(e, 0x3000, 'e8fb0f00008d4003', 'caller-call-lea');
  upload(e, 0x4000, '0f0b', 'outer-gate');
  upload(e, 0x5000, '8d40038903c20800', 'callback-lea-store-ret8');
  upload(e, 0x5100, '0f0b', 'callback-return-gate');
  upload(e, 0x6000, '0f0b', 'legacy-gate');
  const outer = resident(e, [[0x4000, 2]], [[0x4000, OUTER]], [], 'real CALL outer');
  const caller = resident(e, [[0x3000, 5], [0x3005, 3]], [], ['store_resident32'], 'real caller');
  const cb = resident(e, [[0x5000, 8], [0x5100, 2]], [[0x5100, RETURN]], ['read32', 'store_resident32'], 'callback LEA/store/RET8');
  const legacy = legacyModule(e, [[0x6000, 2]], [[0x6000, LEGACY]]);
  ack(e, outer, 0); ack(e, caller, 1); ack(e, cb, 2);
  const neighbours = [
    [0x8fec, 0x11223344], [0x8ff0, 0x23456789], [0x8ff4, 0x3456789a],
    [0x8ff8, 0x456789ab], [0x8ffc, 0x56789abc], [0x9000, 0x6789abcd],
  ];
  for (const [address, value] of neighbours) hostWord(e, address, value);
  hostWord(e, 0xa010, 0x10203040);
  const start = state(), stopped = state(0x4000, 0x8ffc);
  reset(e, start);
  dispatch(e, 9, stopped, exit40(8, 1, OUTER), {
    child: 2, successes: 2, last: outer, slot: 0, helper: helper40(),
  });
  stats.guest_stores++; // real CALL pushes 0x3005 through bound Store6.
  const afterCall = neighbours.map(([address, value]) => [address, address === 0x8ffc ? 0x3005 : value]);
  observe(e, afterCall); observe(e, [[0xa010, 0x10203040]]);
  capture(e, outer, stopped, 1, 0x3005);
  const {active, record} = begin(e, outer, cb, 1, stopped, 0x5000, 0x5100, [0x12345678, 0xffffffff]);
  const frame = [
    [0x8fec, 0x11223344], [0x8ff0, 0x5100], [0x8ff4, 0x12345678],
    [0x8ff8, 0xffffffff], [0x8ffc, 0x3005], [0x9000, 0x6789abcd],
  ];
  observe(e, frame);
  direct(e, cb, undefined, undefined, {status: 12});
  direct(e, legacy, undefined, undefined, {status: 12, legacy: true});
  cancel(e, 1); refresh(e).bytes.fill(0, e.base, e.base + 4);
  dispatch(e, 0, undefined, undefined, {status: 12, badPointers: true});
  e.bytes.set(state56(active), e.base); cancel(e, 0);
  dispatcherGetter(e); // deliberately overwrites the public callback record.
  authorize(e, cb, 2, 3, {high: (e.high ^ 1) >>> 0});
  authorize(e, cb, 2, 3, {unitHigh: 1});
  authorize(e, cb, 2, 14, {token: 0});
  authorize(e, cb, 2, 14, {token: 3});
  authorize(e, outer, 2, 14);
  e.view.setUint32(e.base + 16, 0, true);
  authorize(e, cb, 2, 15);
  e.bytes.set(state56(active), e.base);
  cancel(e, 1); authorize(e, cb, 2, 16); cancel(e, 0);
  authorize(e, cb, 2);
  assert.equal(Buffer.from(record).toString('hex'), Buffer.from(wordsRecord(72, 'R3RC', [
    2, 1, 1, 0, 0x5000, 0x8ff0, 0x5100, RETURN, 2, 0, 1, 0, 3, 0,
  ])).toString('hex'));
  direct(e, outer, undefined, undefined, {status: 12});
  direct(e, caller, undefined, undefined, {status: 12});
  direct(e, legacy, undefined, undefined, {status: 12, legacy: true});
  pure(e, () => e.api.find_installed_resident(e.low, e.high, 0x3005), 12, 'current foreign finder cannot publish');
  pure(e, () => e.api.capture_resident_call(e.low, e.high, cb.low, cb.high, 0, 17), 12, 'capture excludes any callback before malformed request');
  pure(e, () => e.api.acknowledge_resident_installation(e.low, e.high, cb.low, cb.high, 8), 12, 'ack excludes callback before invalid slot');
  pure(e, () => e.api.compile_resident_with_gates(0, 0), 12, 'resident preparation excludes suspension');
  pure(e, () => e.api.compile_with_gates(0, 0), 12, 'legacy preparation excludes suspension');
  pure(e, () => e.api.begin_resident_callback(e.low, e.high, outer.low, outer.high, cb.low, cb.high, 1, 0x5000, 0x5100, RETURN, 0), 12, 'new callback cannot nest under armed authority');
  pure(e, () => e.api.finish_callback(e.low, e.high, legacy.generation, 2), 14, 'legacy finish cannot consume Resident token');
  pure(e, () => e.api.resume_callback_code(e.low, e.high, legacy.generation, 2, 0, 0), 14, 'legacy code migration retains family');
  pure(e, () => e.api.resume_callback_entries(e.low, e.high, legacy.generation, 2, 0, 0), 14, 'legacy entry migration retains family');
  pure(e, () => e.api.abandon_call(e.low, e.high, 1), 12, 'outer abandon requires callback revocation');
  complete(e, outer, 1, 55, undefined, 12);
  const afterLea = copy(active); afterLea.registers[0] = 0x89abcdf2; afterLea.eip = 0x5003;
  direct(e, cb, afterLea, exit40(1, 1), {budget: 1, callbackRetired: 1});
  cancel(e, 1);
  dispatch(e, 0, afterLea, exit40(2));
  cancel(e, 0); dispatch(e, 0, afterLea, exit40(1));
  dispatch(e, 0, undefined, undefined, {status: 1, badPointers: true});
  cancel(e, 1); e.bytes.fill(0, e.base, e.base + 4);
  dispatch(e, 0, undefined, undefined, {status: 2});
  e.bytes.set(state56(afterLea), e.base); cancel(e, 0);
  authorize(e, cb, 2, 12); // busy precedes the legitimate advanced-image mismatch.
  const afterStore = copy(afterLea); afterStore.eip = 0x5005;
  direct(e, cb, afterStore, exit40(1, 1), {budget: 1, helper: helper40(), callbackRetired: 1});
  stats.guest_stores++; observe(e, [[0xa010, 0x89abcdf2]]);
  const returned = copy(afterStore); returned.eip = 0x5100; returned.registers[4] = 0x8ffc;
  dispatch(e, 9, returned, exit40(8, 1, RETURN), {
    child: 1, successes: 1, last: cb, slot: 2, helper: helper40(0x5100), callbackRetired: 1,
  });
  pure(e, () => e.api.capture_resident_call(e.low, e.high, cb.low, cb.high, 1, 0), 12, 'return Gate does not enable inner capture');
  complete(e, outer, 1, 55, undefined, 12);
  abort(e, 2, stopped, exit40(8, 1, OUTER));
  const resumed = state(0x3005, 0x9000); resumed.registers[0] = 55;
  complete(e, outer, 1, 55, resumed);
  const final = copy(resumed); final.eip = 0x3008; final.registers[0] = 58;
  dispatch(e, 9, final, exit40(3, 1), {child: 1, successes: 1, failures: 1, last: caller, slot: 1});
  observe(e, frame); observe(e, [[0xa010, 0x89abcdf2]]);
  flows.push({name: 'installed real CALL and callback', key: e.key.toString(),
    start: state56(start), outer: state56(stopped), initial_callback: state56(active),
    callback_return: state56(returned), caller_final: state56(final),
    callback_record_hex: Buffer.from(record).toString('hex'), frame,
    callback_result: 0x89abcdf2, explicitly_completed_outer_result: 55, real_caller_result: 58});

  // auxiliary real translated Gate with a host-authored return word isolates close.
  hostWord(e, 0x9000, 0x6002);
  const closeStop = state(0x4000, 0x9000); reset(e, closeStop);
  direct(e, outer, closeStop, exit40(8, 0, OUTER));
  capture(e, outer, closeStop, 3, 0x6002);
  begin(e, outer, cb, 3, closeStop, 0x5000, 0x5100, []);
  authorize(e, cb, 4); observe(e, [[0x8ffc, 0x5100]]);
  cancel(e, 1); e.bytes.fill(0xff, e.base, e.base + 96); close(e);
  authorize(e, cb, 0, 5, {high: (e.high ^ 1) >>> 0});
  dispatch(e, 0, undefined, undefined, {status: 5, badPointers: true});
  direct(e, cb, undefined, undefined, {status: 5});
  pure(e, () => e.api.abort_callback(e.low, (e.high ^ 1) >>> 0, 0), 5, 'closed authority precedes identity and token');
  pure(e, () => e.api.find_installed_resident(e.low, e.high, 0x5000), 5, 'closed finder preserves transfer');
  section('installed execution, private outer continuation and armed close', before);
}

{
  const before = {...stats}, e = fresh();
  assert.equal(e.api.map(0x4000, 3, 7), 0);
  assert.equal(e.api.map(0x8000, 2, 3), 0);
  upload(e, 0x4000, '0f0b', 'outer-gate');
  upload(e, 0x5000, '8d4003e9f80f0000', 'callback-lea-cross-unit-jump');
  upload(e, 0x5100, '0f0b', 'callback-return-gate');
  upload(e, 0x6000, '90', 'foreign-nop');
  const outer = resident(e, [[0x4000, 2]], [[0x4000, OUTER]], [], 'uninstalled outer');
  const cb = resident(e, [[0x5000, 8], [0x5100, 2]], [[0x5100, RETURN]], [], 'uninstalled callback');
  const foreign = resident(e, [[0x6000, 1]], [], [], 'foreign current unit');
  ack(e, outer, 0); ack(e, foreign, 1);
  assert.equal(e.table.get(2), null);
  const initialWords = [[0x8ff8, 0x11223344], [0x8ffc, 0x23456789], [0x9000, 0x6002], [0x9004, 0x3456789a]];
  for (const [address, value] of initialWords) hostWord(e, address, value);
  const stopped = state(0x4000, 0x9000); reset(e, stopped);
  direct(e, outer, stopped, exit40(8, 0, OUTER)); capture(e, outer, stopped, 1, 0x6002);
  const {active, record} = begin(e, outer, cb, 1, stopped, 0x5000, 0x5100, []);
  const frame = [[0x8ff8, 0x11223344], [0x8ffc, 0x5100], [0x9000, 0x6002], [0x9004, 0x3456789a]];
  observe(e, frame); dispatcherGetter(e); authorize(e, cb, 2);
  dispatch(e, 9, active, exit40(3), {failures: 1});
  pure(e, () => e.api.acknowledge_resident_installation(e.low, e.high, cb.low, cb.high, 2), 12, 'uninstalled callback cannot be acknowledged while armed');
  const afterLea = copy(active); afterLea.eip = 0x5003; afterLea.registers[0] = 0x89abcdf2;
  direct(e, cb, afterLea, exit40(1, 1), {budget: 1, callbackRetired: 1});
  dispatch(e, 9, afterLea, exit40(3), {failures: 1});
  const crossed = copy(afterLea); crossed.eip = 0x6000;
  direct(e, cb, crossed, exit40(3, 1), {callbackRetired: 1});
  dispatch(e, 9, undefined, undefined, {status: 12, failures: 1});
  direct(e, foreign, undefined, undefined, {status: 12});
  hostWord(e, 0x5000, 0xe903408d); // same bytes still invalidate the retained callback snapshot.
  cancel(e, 1); refresh(e).bytes.fill(0xff, e.base, e.base + 96);
  dispatch(e, 0, undefined, undefined, {status: 4, badPointers: true});
  direct(e, cb, undefined, undefined, {status: 4}); authorize(e, cb, 2, 4);
  direct(e, foreign, undefined, undefined, {status: 12});
  abort(e, 2, stopped, exit40(8, 0, OUTER));
  complete(e, outer, 1, 55, undefined, 16); cancel(e, 0);
  const resumed = state(0x6002, 0x9004); resumed.registers[0] = 55;
  complete(e, outer, 1, 55, resumed); observe(e, frame); close(e);
  flows.push({name: 'uninstalled direct callback and foreign boundary', key: e.key.toString(),
    initial_callback: state56(active), cross_unit_needcode: state56(crossed),
    final_outer_completion: state56(resumed), callback_record_hex: Buffer.from(record).toString('hex'),
    frame, callback_remained_uninstalled: true, callback_snapshot_invalidated_after_guest_retired: 2});
  section('uninstalled direct execution, foreign finder and callback stale', before);
}

{
  const before = {...stats}, e = fresh();
  assert.equal(e.api.map(0x4000, 3, 7), 0);
  assert.equal(e.api.map(0x8000, 2, 3), 0);
  upload(e, 0x4000, '0f0b', 'outer-gate');
  upload(e, 0x5000, '8903c3', 'callback-store-outer-ret');
  upload(e, 0x5100, '0f0b', 'callback-return-gate');
  upload(e, 0x6000, '0f0b', 'legacy-gate');
  const words = [[0x8ff8, 0x11223344], [0x8ffc, 0x23456789], [0x9000, 0x6002], [0x9004, 0x3456789a]];
  for (const [address, value] of words) hostWord(e, address, value);
  hostWord(e, 0x4200, 0x10203040); // before compiling the outer page snapshot.
  const outer = resident(e, [[0x4000, 2]], [[0x4000, OUTER]], [], 'outer-only write victim');
  const cb = resident(e, [[0x5000, 3], [0x5100, 2]], [[0x5100, RETURN]], ['read32', 'store_resident32'], 'callback writes distinct outer page');
  const legacy = legacyModule(e, [[0x6000, 2]], [[0x6000, LEGACY]]);
  ack(e, outer, 0); ack(e, cb, 1);
  const stopped = state(0x4000, 0x9000, 0x4200); reset(e, stopped);
  direct(e, outer, stopped, exit40(8, 0, OUTER)); capture(e, outer, stopped, 1, 0x6002);
  const {active, record} = begin(e, outer, cb, 1, stopped, 0x5000, 0x5100, []);
  dispatcherGetter(e); authorize(e, cb, 2);
  const returned = copy(active); returned.eip = 0x5100; returned.registers[4] = 0x9000;
  dispatch(e, 9, returned, exit40(8, 2, RETURN), {
    child: 1, successes: 1, last: cb, slot: 1, helper: helper40(0x5100), callbackRetired: 2,
  });
  stats.guest_stores++;
  const frameAndOuter = [[0x8ff8, 0x11223344], [0x8ffc, 0x5100], [0x9000, 0x6002], [0x9004, 0x3456789a], [0x4200, 0x89abcdef]];
  observe(e, frameAndOuter); dispatcherGetter(e);
  assert.equal(new DataView(record.buffer).getUint32(28, true), 0); // historical begin outcome.
  cancel(e, 1); refresh(e).bytes.fill(0xff, e.base, e.base + 96);
  dispatch(e, 0, undefined, undefined, {status: 4, badPointers: true});
  direct(e, cb, undefined, undefined, {status: 4});
  direct(e, outer, undefined, undefined, {status: 4});
  direct(e, legacy, undefined, undefined, {status: 12, legacy: true});
  authorize(e, cb, 2, 4);
  abort(e, 2, stopped, exit40(8, 0, OUTER));
  complete(e, outer, 1, 55, undefined, 4);
  pure(e, () => e.api.abandon_call(e.low, e.high, 1), 0, 'stale outer can be abandoned after neutral abort');
  stats.abandoned++; observe(e, frameAndOuter); close(e);
  flows.push({name: 'committed outer-only Store6', key: e.key.toString(),
    initial_callback: state56(active), returned: state56(returned), private_outer: state56(stopped),
    callback_record_hex: Buffer.from(record).toString('hex'), historical_begin_outcome: 0,
    frame_and_outer_word: frameAndOuter, retired_before_next_entry_stale: 2});
  section('outer-only store commits through RET then next entry is stale', before);
}

{
  const before = {...stats}, e = fresh();
  assert.equal(e.api.map(0x4000, 3, 7), 0);
  assert.equal(e.api.map(0x8000, 2, 3), 0);
  upload(e, 0x4000, '0f0b', 'outer-gate');
  upload(e, 0x4100, '8d4003c3', 'same-unit-callback');
  upload(e, 0x4110, '0f0b', 'same-unit-return-gate');
  upload(e, 0x6000, '0f0b', 'legacy-outer-gate');
  upload(e, 0x6200, '8d4003c3', 'legacy-callback');
  upload(e, 0x6300, '0f0b', 'legacy-return-gate');
  const same = resident(e, [[0x4000, 2], [0x4100, 4], [0x4110, 2]], [[0x4000, OUTER], [0x4110, RETURN]], ['read32'], 'same-unit admission remains supported');
  const legacy = legacyModule(e, [[0x6000, 2], [0x6200, 4], [0x6300, 2]], [[0x6000, LEGACY], [0x6300, LEGACY_RETURN]], ['read32']);
  const words = [[0x8ff8, 0x11223344], [0x8ffc, 0x23456789], [0x9000, 0x6002], [0x9004, 0x3456789a]];
  for (const [address, value] of words) hostWord(e, address, value);
  const stopped = state(0x4000, 0x9000); reset(e, stopped);
  direct(e, same, stopped, exit40(8, 0, OUTER)); capture(e, same, stopped, 1, 0x6002);
  const sameBegin = begin(e, same, same, 1, stopped, 0x4100, 0x4110, []);
  dispatcherGetter(e); authorize(e, same, 2, 7);
  direct(e, same, undefined, undefined, {status: 12});
  dispatch(e, 0, undefined, undefined, {status: 12, badPointers: true});
  abort(e, 2, stopped, exit40(8, 0, OUTER));
  const resumed = state(0x6002, 0x9004); resumed.registers[0] = 55;
  complete(e, same, 1, 55, resumed);
  hostWord(e, 0x9004, 0x6002);
  const legacyStopped = state(0x6000, 0x9004); reset(e, legacyStopped);
  direct(e, legacy, legacyStopped, exit40(8, 0, LEGACY), {legacy: true});
  capture(e, legacy, legacyStopped, 3, 0x6002, true);
  const legacyBegin = begin(e, legacy, legacy, 3, legacyStopped, 0x6200, 0x6300, [], true);
  authorize(e, same, 4, 14); direct(e, same, undefined, undefined, {status: 12});
  const finalWords = [[0x8ff8, 0x11223344], [0x8ffc, 0x4110], [0x9000, 0x6300], [0x9004, 0x6002]];
  observe(e, finalWords); close(e);
  flows.push({name: 'same-ID ceiling and Replacement family', key: e.key.toString(),
    same_unit_initial: state56(sameBegin.active), same_unit_record_hex: Buffer.from(sameBegin.record).toString('hex'),
    replacement_initial: state56(legacyBegin.active), replacement_record_hex: Buffer.from(legacyBegin.record).toString('hex'),
    final_words: finalWords, callback_guest_instructions_retired: 0});
  section('same-ID execution refusal and Replacement family separation', before);
}

// each complete row was independently derived before any execution.
const expectedRows = [
  [1, 3, 1, 3, 2, 3, 9, 5, 4, 4, 4, 1, 7, 3, 4, 2, 2, 0, 2, 0, 2, 9, 1, 1, 2, 0, 13, 22, 8, 90, 3, 2, 4, 1],
  [1, 3, 0, 3, 2, 2, 4, 2, 2, 0, 0, 3, 6, 3, 3, 0, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 8, 5, 39, 2, 0, 1, 1],
  [1, 2, 1, 2, 3, 2, 2, 1, 1, 1, 1, 0, 3, 1, 2, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 10, 5, 37, 2, 1, 1, 1],
  [1, 1, 1, 1, 2, 0, 1, 0, 1, 0, 0, 0, 3, 1, 2, 1, 1, 1, 1, 1, 0, 2, 1, 1, 0, 0, 0, 4, 5, 27, 0, 0, 2, 1],
];
const expectedTotal = [4, 9, 3, 9, 9, 7, 16, 8, 8, 5, 5, 4, 19, 8, 11, 4, 5, 1, 5, 1, 4, 13, 4, 3, 4, 1, 14, 44, 23, 193, 7, 3, 8, 4];
const rowObject = row => Object.fromEntries(fields.map((field, index) => [field, row[index]]));
assert.deepEqual(stats, rowObject(expectedTotal), 'all34 aggregate counters were fixed before runtime');
assert.deepEqual(sections.map(value => value.stats), expectedRows.map(rowObject), 'all4 complete independently derived rows');
assert.deepEqual(expectedRows.reduce((sum, row) => row.map((value, index) => sum[index] + value), fields.map(() => 0)), expectedTotal);
assert.equal(Object.keys(authored).length, 19);
assert.equal(Object.keys(artifacts).length, 35);
assert.equal(unitBindings.length, 12);
assert.equal(installations.length, 11);
assert.equal(records.length, 41);
assert.equal(records.filter(record => record.kind === 'R3RC72').length, 5);
assert.equal(records.filter(record => record.kind === 'R3CB64').length, 1);
assert.equal(records.filter(record => record.kind === 'R3CF112').length, 6);
assert.equal(records.filter(record => record.kind === 'R3DP32').length, 9);
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [
  ...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock',
  'engine/tests/process_resident_callback_execution.rs',
  'engine/tests/process_resident_callback_execution_wasm.rs',
  'engine/tests/fixtures/p2-resident-callback-execution/run.mjs',
  'engine/tests/abi_resident_callback.rs', 'engine/tests/process_resident_callback.rs',
  'engine/tests/process_resident_callback_wasm.rs', 'engine/tests/fixtures/p2-resident-callback/run.mjs',
  'engine/tests/abi_callback.rs', 'engine/tests/process_callbacks.rs', 'engine/tests/callback_code_install.rs',
  'engine/tests/callbacks_wasm.rs', 'engine/tests/fixtures/p2-callbacks/run.mjs', 'engine/tests/fixtures/p2-callbacks/integer.S',
  'engine/tests/callback_code_wasm.rs', 'engine/tests/fixtures/p2-callback-code/run.mjs', 'engine/tests/fixtures/p2-callback-code/integer.S',
  'engine/tests/process_returning_dispatcher.rs', 'engine/tests/process_returning_dispatcher_wasm.rs',
  'engine/tests/fixtures/p2-returning-dispatcher/run.mjs',
];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
assert.equal(sourcePaths.length, productionPaths.length + 22);
function printableFlow(value) {
  if (value instanceof Uint8Array) return {hex: Buffer.from(value).toString('hex'), sha256: sha256(value)};
  if (Array.isArray(value)) return value.map(printableFlow);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, printableFlow(item)]));
  return value;
}
const provenance = {
  stats, statistic_fields: fields, section_derivation: sections,
  engine_sha256: sha256(engineBytes), engine_bytes: engineBytes.length,
  artifact_sha256: artifacts,
  module_set_sha256: sha256(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, sha256(readFileSync(join(root, path)))])),
  production: {
    git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(),
    source_files: productionPaths.length,
    source_set_sha256: sha256(productionPaths.map(path => `${path}:${sha256(readFileSync(join(root, path)))}`).join('\n')),
  },
  authored, flows: flows.map(printableFlow), exact_raw_records: records,
  module_bindings: unitBindings, installed_reference_observations: installations,
  executions: {
    product_engine_instances: 4, product_dispatcher_instances: 4,
    emitted_resident_modules: 9, emitted_legacy_modules: 3,
    emitted_resident_guard_entries_from_authored_paths: 24,
    emitted_legacy_guard_entries_from_authored_paths: 4,
    product_dispatch_entry_guard_entries: 16, dispatcher_finder_calls: 9,
    callback_guest_body_entries: 6, callback_guest_instructions_retired: 7,
    bound_store_identity_checks: 3,
  },
  tools: {
    node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch,
    rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(),
    cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim(),
  },
  commands: {
    run: [process.execPath, process.argv[1], enginePath, outputDir, root],
    generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_callback_execution_wasm', '--', '--nocapture'],
  },
  counts: 'all34 integer counters, all4 complete section rows, and all7 retired callback instructions were independently derived from the authored paths before runtime. Counters are not Wasm instrumentation or intercepted imports. Dispatcher lookup_success/failure cover only its nine actual finder calls; explicit foreign/closed finder controls are other_rejected. Each of193 full-arena checks compares all4236 bytes against pre-operation input plus only independently encoded complete State56/Exitv3-40/helper40/known metadata. Metadata pointer/length are allocator coordinates checked for bounds, full identity and immutable emitted bytes. Positive table references are exact exports installed before suspension and retained unchanged. All41 exact records include raw bytes and hashes; all35 emitted/input artifacts are preserved.',
  parity: 'root separately ran the four affected old actual targets once and checked their complete integer fields, rows and artifact/input bytes; this portable authored target depends on no ignored historical output',
  claim: 'explicit distinct-unit Resident callback execution authorization only. Main real guest CALL pushes3005, actual Gate capture1 and atomic begin2 with two arguments precede a pure private authorization after public getter reuse. Direct LEA and data Store6 each retire one under split budgets; cancel/budget stop before lookup; product dispatcher runs actual RET8 and registered return Gate. The stop keeps callback authority. Neutral abort discards callback EAX89abcdf2, restores exact private outer State/Exit, then explicit outer scalar55 and actual caller LEA produce58 without host CPU advancement. Prepared uninstalled callback runs directly while dispatcher misses17 and ack remainsBusy; cross-unit NeedCode meets current foreign finderBusy12 before any child publication. Callback stale4 and committed outer-only MOV/RET2 with next-entry stale4 preserve prefix RAM/helper/transfer. Same-ID begin/abort remains valid but execution authorizes7; Replacement family authorizes14. Close revokes armed authority. Auxiliary direct Gate controls use host-authored return words and do not claim guest CALL execution. RAM proof comprises44 bounded four-byte frame/neighbour/data reads, not all guest RAM. Historical R3RC72 outcome remains begin mask0 and phase1/result0. No resident finish/result publication, innercalls, same-unit execution, migration, provider/SDK/browser/async/race/table authentication/eviction/performance/fullP2-V0/gameplay claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({
  stats, section_derivation: sections, engine_sha256: provenance.engine_sha256,
  module_set_sha256: provenance.module_set_sha256, sources: sourcePaths.length,
  artifacts: Object.keys(artifacts).length, output: outputDir,
}));
