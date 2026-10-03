import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root);
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const SIZE = 4236, TRANSFER = 140, KEY = 0xd3456789e0000000n;
const LOW = Number(KEY & 0xffffffffn), HIGH = Number(KEY >> 32n);
const OUTER = 0x80000011, RETURN = 0x80000012, INNER = 0x80000013;
const engineBytes = readFileSync(enginePath);
const ownSources = [
  'engine/tests/process_resident_callback_installation_wasm.rs',
  'engine/tests/fixtures/p2-resident-callback-installation/run.mjs',
];
const sourcePaths = [...new Set([
  ...execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard',
    'Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n'),
  ...ownSources,
])].sort();
const sourceBytes = Object.fromEntries(sourcePaths.map(path => [path, readFileSync(join(root, path))]));
for (const [path, bytes] of Object.entries(sourceBytes)) {
  const destination = join(outputDir, 'sources', path);
  mkdirSync(dirname(destination), {recursive: true});
  writeFileSync(destination, bytes);
}
writeFileSync(join(outputDir, 'executed-engine.wasm'), engineBytes);

function words(size, magic, values, version = 1) {
  const bytes = new Uint8Array(size), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true);
  view.setUint16(6, 1, true); view.setUint32(8, size, true);
  values.forEach((value, index) => view.setUint32(16 + index * 4, value, true));
  return bytes;
}
const cpu = (eip, esp, eax = 10) => ({
  registers: [eax, 0x11111111, 0x22222222, 0x33333333, esp, 0x55555555, 0x66666666, 0x77777777],
  eip, eflags: 0xcd7,
});
const state56 = value => words(56, 'R3ST', [...value.registers, value.eip, value.eflags]);
const exit40 = (reason, retired = 0, detail = 0) => words(40, 'R3EX', [reason, retired, detail, 0, 0, 0], 3);
const helper40 = (value = 0) => words(40, 'R3MH', [0, value, 0, 0, 0, 0]);
const installed32 = unit => words(32, 'R3IN', [unit.low, unit.high, unit.slot, 0]);
// these authored semantics are fixed before an engine or generated module executes.
const trace = [
  ['caller CALL -> outer Gate', cpu(0x4000, 0x8ffc), exit40(8, 1, OUTER), 0, 'outer', helper40()],
  ['home LEA/JMP -> uncompiled B', cpu(0x6000, 0x8ff8, 13), exit40(3, 2), 0, 'home', null],
  ['B LEA/CALL -> inner Gate', cpu(0x6200, 0x8ff4, 18), exit40(8, 2, INNER), 0, 'B', helper40()],
  ['B LEA/RET -> home selection boundary', cpu(0x5100, 0x8ffc, 47), exit40(3, 2), 12, 'B', helper40(0x5100)],
  ['home return Gate', cpu(0x5100, 0x8ffc, 47), exit40(8, 0, RETURN), 0, 'home', null],
  ['frozen outer caller LEA', cpu(0x3008, 0x9000, 50), exit40(3, 1), 0, 'caller', null],
];
const inputs = [
  ['caller', 0x3000, 'e8fb0f00008d4003'], ['outer-gate', 0x4000, '0f0b'],
  ['home', 0x5000, '8d4003e9f80f0000'], ['return-gate', 0x5100, '0f0b'],
  ['B', 0x6000, '8d4005e8f80100008d4003c3'], ['inner-gate', 0x6200, '0f0b'],
];
const oracle = {
  trace: trace.map(([label, state, exit, status]) => ({label, state, exit_hex: Buffer.from(exit).toString('hex'), status})),
  inputs, inner_result: 44, callback_result: 47, caller_result: 50,
  frozen_inner_return: 0x6008, frozen_outer_return: 0x3005,
  corrupted_inner_word: 0xdeadbeef, corrupted_outer_word: 0xcafebabe,
  unit_identity: 'returned nonzero full-u64 IDs; no fixed allocation sequence',
};
writeFileSync(join(outputDir, 'authored-oracle.json'), JSON.stringify(oracle, null, 2));
const module = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(module), []);
const instance = new WebAssembly.Instance(module, {});
const names = [
  'open', 'close', 'arena_ptr', 'map', 'upload', 'generation', 'compile_resident_with_gates',
  'compile_resident_callback_unit', 'acknowledge_resident_callback_installation',
  'dispatcher_module', 'guard_dispatch_entry', 'guard_resident', 'find_resident',
  'find_installed_resident', 'read32', 'write32', 'store_resident32',
  'acknowledge_resident_installation', 'capture_resident_call', 'complete_resident_call',
  'begin_resident_callback', 'authorize_resident_callback', 'select_resident_callback_unit',
  'capture_active_resident_callback_call', 'complete_active_resident_callback_call', 'finish_resident_callback',
];
const api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
assert.equal(api.compile_resident_callback_unit.length, 7);
assert.equal(api.acknowledge_resident_callback_installation.length, 8);
const memory = instance.exports.memory;
let bytes, view;
function refresh() { bytes = new Uint8Array(memory.buffer); view = new DataView(memory.buffer); }
assert.equal(api.open(16, LOW, HIGH), 0);
const base = api.arena_ptr() >>> 0;
refresh(); assert.ok(base > 0 && base + SIZE <= bytes.length);
const arena = () => { refresh(); return bytes.slice(base, base + SIZE); };
const observations = [], artifacts = {}, units = {};
function check(expected, label) { assert.deepEqual(arena(), expected, `${label}: complete arena`); }
function operation(label, action, status = 0, patches = []) {
  const expected = arena();
  for (const [offset, value] of patches) expected.set(value, offset);
  assert.equal(action(), status, label); check(expected, label);
  observations.push({label, status});
}
function artifact(name, value) { writeFileSync(join(outputDir, name), value); artifacts[name] = sha256(value); }
function descriptors(blocks, gates) {
  refresh(); [...blocks, ...gates].forEach(([pc, value], index) => {
    view.setUint32(base + TRANSFER + index * 8, pc, true);
    view.setUint32(base + TRANSFER + index * 8 + 4, value, true);
  });
}
function compile(label, blocks, gates, helpers, slot, action) {
  descriptors(blocks, gates);
  const before = arena(); assert.equal(action(), 0, `${label}: compile`);
  refresh();
  const [version, size, low, high, pointer, length] = Array.from({length: 6}, (_, i) => view.getUint32(base + TRANSFER + i * 4, true));
  assert.equal(version, 1); assert.equal(size, 24);
  const id = BigInt(low) | (BigInt(high) << 32n);
  assert.notEqual(id, 0n); assert.ok(Object.values(units).every(unit => unit.id !== id));
  assert.ok(pointer > 0 && length > 8 && pointer + length <= bytes.length);
  const metadata = new Uint8Array(24), mv = new DataView(metadata.buffer);
  [1, 24, low, high, pointer, length].forEach((value, i) => mv.setUint32(i * 4, value, true));
  const expected = before.slice(); expected.set(metadata, TRANSFER); check(expected, `${label}: metadata24 only`);
  const code = bytes.slice(pointer, pointer + length), compiled = new WebAssembly.Module(code);
  assert.deepEqual(WebAssembly.Module.imports(compiled), [
    {module: 'env', name: 'memory', kind: 'memory'},
    ...['guard_resident', ...helpers].map(name => ({module: 'ring3', name, kind: 'function'})),
  ]);
  artifact(`${label}.wasm`, code);
  const unit = {label, id, low, high, pointer, length, code, module: compiled, helpers, slot};
  units[label] = unit; return unit;
}
const table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
function instantiate(unit) {
  unit.instance = new WebAssembly.Instance(unit.module, {env: {memory}, ring3: Object.fromEntries(['guard_resident', ...unit.helpers].map(name => [name, api[name]]))});
  unit.run = unit.instance.exports.run; table.set(unit.slot, unit.run);
  assert.equal(table.get(unit.slot), unit.run);
}
const dispatcherBefore = arena(); assert.equal(api.dispatcher_module(LOW, HIGH), 0); refresh();
const dp = view.getUint32(base + TRANSFER + 24, true), dl = view.getUint32(base + TRANSFER + 28, true);
assert.ok(dp > 0 && dl > 8 && dp + dl <= bytes.length);
const dpExpected = dispatcherBefore.slice(); dpExpected.set(words(32, 'R3DP', [LOW, HIGH, dp, dl]), TRANSFER); check(dpExpected, 'dispatcher32 publication');
const dispatcherBytes = bytes.slice(dp, dp + dl), dispatcherModule = new WebAssembly.Module(dispatcherBytes);
assert.deepEqual(WebAssembly.Module.imports(dispatcherModule), [
  {module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'},
  {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'},
  {module: 'ring3', name: 'find_installed_resident', kind: 'function'},
]);
const dispatcher = new WebAssembly.Instance(dispatcherModule, {env: {memory, table}, ring3: {guard_dispatch_entry: api.guard_dispatch_entry, find_installed_resident: api.find_installed_resident}});
artifact('dispatcher.wasm', dispatcherBytes);
assert.equal(api.map(0x3000, 4, 7), 0); assert.equal(api.map(0x8000, 2, 3), 0);
for (const [label, pc, hex] of inputs) {
  const input = Buffer.from(hex, 'hex'); refresh(); bytes.set(input, base + TRANSFER);
  assert.equal(api.upload(pc, input.length), 0); artifact(`${label}.x86`, input);
}
for (const [label, blocks, gates, helpers, slot] of [
  ['outer', [[0x4000, 2]], [[0x4000, OUTER]], [], 0],
  ['home', [[0x5000, 8], [0x5100, 2]], [[0x5100, RETURN]], [], 1],
  ['caller', [[0x3000, 5], [0x3005, 3]], [], ['store_resident32'], 2],
]) {
  const unit = compile(label, blocks, gates, helpers, slot, () => api.compile_resident_with_gates(blocks.length, gates.length));
  instantiate(unit);
  operation(`${label}: ordinary initial ack`, () => api.acknowledge_resident_installation(LOW, HIGH, unit.low, unit.high, slot), 0, [[TRANSFER, installed32(unit)]]);
}
const {outer, home, caller} = units;
const coldCompile = (token = 2, count = 3, high = HIGH) => api.compile_resident_callback_unit(LOW, high, home.low, home.high, token, count, 1);
const coldAck = (unit, token = 2, slot = unit.slot) => api.acknowledge_resident_callback_installation(LOW, HIGH, home.low, home.high, token, unit.low, unit.high, slot);
const select = unit => api.select_resident_callback_unit(LOW, HIGH, home.low, home.high, 2, unit.low, unit.high);
const guard = unit => api.guard_resident(LOW, HIGH, unit.low, unit.high, base, base + 56, base + 96);
function hostWord(address, value, read = false) {
  operation(`${read ? 'read' : 'write'} bounded RAM ${address.toString(16)}`, () => read ? api.read32(address) : api.write32(address, value), 0, [[100, helper40(read ? value : 0)]]);
}
function dispatch(index, budget = 9) {
  const [label, state, exit, status, last, helper] = trace[index];
  const patches = [[0, state56(state)], [56, exit], [TRANSFER, installed32(units[last])]];
  if (helper) patches.push([100, helper]);
  operation(label, () => dispatcher.exports.run(base, base + 56, budget, base + 96), status, patches);
}
for (const [address, value] of [[0x8ff4, 0x11223344], [0x8ff8, 0x55667788], [0x8ffc, 0x22334455]]) hostWord(address, value);
refresh(); bytes.set(state56(cpu(0x3000, 0x9000)), base);
bytes.fill(0xa5, base + 56, base + 96); view.setUint32(base + 96, 0, true);
bytes.fill(0x5a, base + 100, base + 140);
dispatch(0);
hostWord(0x8ffc, 0x3005, true);
operation('outer CF112', () => api.capture_resident_call(LOW, HIGH, outer.low, outer.high, 1, 0), 0,
  [[TRANSFER, words(112, 'R3CF', [1, OUTER, 1, 0, 0x4000, 0x8ffc, 0x3005, 0])]]);
operation('begin preserves immutable home/outer identities', () => api.begin_resident_callback(LOW, HIGH, outer.low, outer.high, home.low, home.high, 1, 0x5000, 0x5100, RETURN, 0), 0,
  [[0, state56(cpu(0x5000, 0x8ff8))], [56, exit40(3)], [TRANSFER, words(72, 'R3RC', [2, 1, 1, 0, 0x5000, 0x8ff8, 0x5100, RETURN, 0, 0, outer.low, outer.high, home.low, home.high])]]);
operation('unarmed cold compile', () => coldCompile(), 12);
operation('explicit callback authorization', () => api.authorize_resident_callback(LOW, HIGH, home.low, home.high, 2));
operation('B genuinely absent before home execution', () => api.find_resident(0x6000), 17);
dispatch(1);
operation('B still absent at the real cold stop', () => api.find_resident(0x6000), 17);
operation('old compile remains Busy', () => api.compile_resident_with_gates(0, 0), 12);
operation('wrong full key precedes descriptor rejection', () => coldCompile(2, 0, HIGH ^ 1), 3);
operation('wrong token precedes descriptor rejection', () => coldCompile(0, 0), 14);
operation('invalid count at admitted cold stop', () => coldCompile(2, 0), 7);
const B = compile('B', [[0x6000, 8], [0x6008, 4], [0x6200, 2]], [[0x6200, INNER]], ['read32', 'store_resident32'], 3, () => coldCompile());
operation('compile preserves active home', () => guard(home));
operation('unselected B guard remains Busy', () => guard(B), 12);
operation('compiled B cannot select without acknowledgement', () => select(B), 17);
const failedInstallBefore = arena();
assert.throws(() => new WebAssembly.Instance(B.module, {env: {memory}, ring3: {}}), WebAssembly.LinkError);
check(failedInstallBefore, 'failed host instantiate preserves arena'); assert.equal(table.get(3), null);
operation('failed host install grants no acknowledgement', () => select(B), 17);
instantiate(B);
operation('staged host slot rejects wrong callback token', () => coldAck(B, 0), 14);
table.set(3, null); assert.equal(table.get(3), null);
operation('cleaned orphan still unacknowledged', () => select(B), 17);
instantiate(B);
operation('cold ack32 only', () => coldAck(B), 0, [[TRANSFER, installed32(B)]]);
operation('same cold ack idempotent', () => coldAck(B), 0, [[TRANSFER, installed32(B)]]);
operation('cold ack conflicting slot rejected', () => coldAck(B, 2, 4), 7);
operation('old ack remains Busy', () => api.acknowledge_resident_installation(LOW, HIGH, B.low, B.high, 3), 12);
operation('ack does not select B', () => guard(B), 12);
operation('explicit B selection', () => select(B));
dispatch(2);
hostWord(0x8ff4, 0x6008, true);
operation('inner CF112', () => api.capture_active_resident_callback_call(LOW, HIGH, B.low, B.high, 2, 1, 0), 0,
  [[TRANSFER, words(112, 'R3CF', [3, INNER, 1, 0, 0x6200, 0x8ff4, 0x6008, 0])]]);
operation('pending cold compile remains Busy', () => coldCompile(), 12);
operation('pending cold ack remains Busy', () => coldAck(B), 12);
hostWord(0x8ff4, 0xdeadbeef); hostWord(0x8ffc, 0xcafebabe);
operation('inner frozen return6008 gives44', () => api.complete_active_resident_callback_call(LOW, HIGH, B.low, B.high, 2, 3, 44), 0,
  [[0, state56(cpu(0x6008, 0x8ff8, 44))], [56, exit40(3)]]);
dispatch(3);
operation('explicit home selection', () => select(home));
dispatch(4, 1);
operation('finish47 restores exact retained outer', () => api.finish_resident_callback(LOW, HIGH, home.low, home.high, 2), 0,
  [[0, state56(cpu(0x4000, 0x8ffc))], [56, exit40(8, 1, OUTER)], [TRANSFER, words(48, 'R3RR', [2, 1, 47, 0, outer.low, outer.high, home.low, home.high])]]);
operation('cold compile callback replay', () => coldCompile(), 14);
operation('cold ack callback replay', () => coldAck(B), 14);
operation('outer frozen return3005 gives47', () => api.complete_resident_call(LOW, HIGH, outer.low, outer.high, 1, 47), 0,
  [[0, state56(cpu(0x3005, 0x9000, 47))], [56, exit40(3)]]);
dispatch(5);
for (const [address, value] of [[0x8ff4, 0xdeadbeef], [0x8ff8, 0x5100], [0x8ffc, 0xcafebabe]]) hostWord(address, value, true);
assert.equal(api.generation(), 0);
for (const unit of Object.values(units)) {
  assert.equal(table.get(unit.slot), unit.run); refresh();
  assert.deepEqual(bytes.slice(unit.pointer, unit.pointer + unit.length), unit.code);
}
for (let slot = 4; slot < 8; slot++) assert.equal(table.get(slot), null);
operation('close preserves arena', () => api.close());
operation('closed cold compile wins identity/token', () => coldCompile(0, 0, HIGH ^ 1), 5);
operation('closed cold ack wins token', () => coldAck(B, 0), 5);
for (const path of sourcePaths) assert.deepEqual(readFileSync(join(root, path)), sourceBytes[path], `source unchanged: ${path}`);
const provenance = {
  fixture: 'cold-resident-callback-installation', engine_sha256: sha256(engineBytes),
  engine_copy: 'executed-engine.wasm', oracle_sha256: sha256(readFileSync(join(outputDir, 'authored-oracle.json'))),
  sources: Object.fromEntries(sourcePaths.map(path => [path, sha256(sourceBytes[path])])),
  source_snapshot: 'sources', snapshot_before_first_module: true, source_bytes_unchanged_after_proof: true,
  artifact_sha256: artifacts, observations,
  units: Object.values(units).map(({label, id, pointer, length, slot, code}) => ({label, id: id.toString(), pointer, length, slot, sha256: sha256(code)})),
  flow: {home: home.id.toString(), outer: outer.id.toString(), B: B.id.toString(), caller: caller.id.toString(), outer_token: 1, callback_token: 2, inner_token: 3, B_compiled_after_actual_missing_stop: true, explicit_selection: true, inner_result: 44, callback_result: 47, caller_result: 50},
  environment: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch},
  git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(),
  git_status: execFileSync('git', ['status', '--porcelain'], {cwd: root, encoding: 'utf8'}).trim(),
  claim_ceiling: 'one synchronous trusted-host cold callback continuation and frozen scalar inner/outer return; bounded RAM observations; no browser/async/race/table authentication/eviction/performance/game/fullP2-V0 claim',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({status: 'ok', fixture: provenance.fixture, caller_result: 50, engine_sha256: provenance.engine_sha256, artifact_count: Object.keys(artifacts).length}));
