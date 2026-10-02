import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const KEY = 0xa1345678c0000001n, SIZE = 4236, TRANSFER = 140;
const names = ['open', 'close', 'arena_ptr', 'map', 'upload', 'compile_with_gates', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_dispatch_entry', 'compile_resident_with_gates', 'resident_module', 'guard_resident', 'acknowledge_resident_installation', 'read32', 'write32', 'capture_resident_call', 'capture_call', 'abandon_call', 'begin_callback', 'abort_callback'];
const stats = {engine_instances: 0, preopen_guard_calls: 0, direct_guard_calls: 0, direct_guard_success: 0, direct_guard_rejections: 0, probe_calls: 0, probe_guard_success: 0, probe_guard_rejections: 0, probe_child_calls: 0, probe_child_canonical: 0, probe_child_rejections: 0, direct_resident_runs: 0, direct_legacy_runs: 0, resident_compiles: 0, resident_getters: 0, metadata24_checks: 0, installation32_checks: 0, resident_captures: 0, legacy_captures: 0, pending_abandons: 0, callback_begins: 0, callback_aborts: 0, host_reads: 0, host_writes: 0, callback_word_writes: 0, full_arena_checks: 0, ram_snapshots: 0, header_cases: 0, pointer_cases: 0, closed_engines: 0};
const artifacts = {}, authored = {}, units = [], metadata = [], controls = [], sections = [];
for (const file of ['probe.wasm', 'foreign-probe.wasm']) artifacts[file] = hash(readFileSync(join(outputDir, file)));
function header(b, v, magic, size, version = 1) { b.set(Buffer.from(magic)); v.setUint16(4, version, true); v.setUint16(6, 1, true); v.setUint32(8, size, true); v.setUint32(12, 0, true); }
function state(pc = 0x1000, flags = 0xcd7) { return {registers: [0xfffffffe, 0x13579bdf, 0x23456789, 0x3456789a, 0x9000, 0x56789abc, 0x6789abcd, 0x789abcde], eip: pc, eflags: flags}; }
function stateBytes(s) { const b = new Uint8Array(56), v = new DataView(b.buffer); header(b, v, 'R3ST', 56); s.registers.forEach((x, i) => v.setUint32(16 + i * 4, x, true)); v.setUint32(48, s.eip, true); v.setUint32(52, s.eflags, true); return b; }
function exit(reason, retired, version = 1, detail = 0) { const b = new Uint8Array(40), v = new DataView(b.buffer); header(b, v, 'R3EX', 40, version); [reason, retired, detail, 0, 0, 0].forEach((x, i) => v.setUint32(16 + i * 4, x, true)); return b; }
function helper(value = 0) { const b = new Uint8Array(40), v = new DataView(b.buffer); header(b, v, 'R3MH', 40); v.setUint32(20, value, true); return b; }
function refresh(e) { if (e.buffer !== e.memory.buffer) { e.buffer = e.memory.buffer; e.bytes = new Uint8Array(e.buffer); e.view = new DataView(e.buffer); } return e; }
function arena(e) { return refresh(e).bytes.slice(e.base, e.base + SIZE); }
function check(e, wanted, label) { assert.deepEqual(arena(e), wanted, `${label}: exact4236 arena`); stats.full_arena_checks++; }
function reset(e, s, cancel = 0) { refresh(e).bytes.set(stateBytes(s), e.base); e.bytes.fill(0xa5, e.base + 56, e.base + 96); e.view.setUint32(e.base + 96, cancel, true); e.bytes.fill(0x5a, e.base + 100, e.base + 140); }
function cancel(e, value) { refresh(e).view.setUint32(e.base + 96, value, true); }
function guard(e, status, label, options = {}) {
  const pointers = options.pointers ?? [e.base, e.base + 56, e.base + 96], before = e.opened ? arena(e) : undefined;
  const args = [options.low ?? e.low, options.high ?? e.high, ...pointers]; assert.equal(e.api.guard_dispatch_entry(...args), status, label);
  if (before) check(e, before, label); else stats.preopen_guard_calls++;
  stats.direct_guard_calls++; stats[status ? 'direct_guard_rejections' : 'direct_guard_success']++;
  controls.push({kind: 'direct', label, status, arguments: args});
}
function probe(e, budget, guardStatus, childStatus, label, options = {}) {
  const before = arena(e), pointers = options.pointers ?? [e.base, e.base + 56, e.base + 96], wanted = before.slice();
  const run = options.foreign ? e.foreignProbe.exports.run : e.probe.exports.run;
  assert.equal(run(pointers[0], pointers[1], budget, pointers[2]), guardStatus || childStatus, label);
  if (!guardStatus) { stats.probe_guard_success++; stats.probe_child_calls++; stats[childStatus ? 'probe_child_rejections' : 'probe_child_canonical']++; }
  else stats.probe_guard_rejections++;
  if (options.state) wanted.set(stateBytes(options.state), 0);
  if (options.exit) wanted.set(options.exit, 56);
  check(e, wanted, label); stats.probe_calls++;
  controls.push({kind: 'probe', label, guard_status: guardStatus, child_status: childStatus, budget, arguments: pointers, foreign_key: !!options.foreign});
}
function word(e, address, value) { const before = arena(e), wanted = before.slice(); assert.equal(e.api.write32(address, value), 0); wanted.set(helper(), 100); check(e, wanted, 'host word initialization'); stats.host_writes++; }
const ram = new Map([[0x7ffc, 0x11223344], [0x8000, 0x89abcdef], [0x8004, 0x55667788], [0x8ffc, 0x12345678], [0x9000, 0x5000], [0x9004, 0x76543210]]);
function observeRAM(e, label) { for (const [address, value] of ram) { const before = arena(e), wanted = before.slice(); assert.equal(e.api.read32(address), 0); wanted.set(helper(value), 100); check(e, wanted, `${label}: RAM${address.toString(16)}`); stats.host_reads++; } stats.ram_snapshots++; }
function upload(e, pc, bytes, label) { refresh(e).bytes.set(bytes, e.base + TRANSFER); assert.equal(e.api.upload(pc, bytes.length), 0); const file = `${label}.x86`; writeFileSync(join(outputDir, file), bytes); artifacts[file] = hash(bytes); authored[file] = Buffer.from(bytes).toString('hex'); }
function descriptors(e, blocks, gates) { refresh(e); [...blocks, ...gates].forEach(([pc, value], i) => { e.view.setUint32(e.base + TRANSFER + i * 8, pc, true); e.view.setUint32(e.base + TRANSFER + i * 8 + 4, value, true); }); }
function uleb(b, c) { let n = 0, shift = 0, byte; do { byte = b[c.i++]; assert.ok(byte !== undefined && shift <= 28); n += (byte & 127) * 2 ** shift; shift += 7; } while (byte & 128); return n; }
function sleb(b, c) { let n = 0n, shift = 0n, byte; do { byte = b[c.i++]; assert.ok(byte !== undefined && shift <= 28n); n |= BigInt(byte & 127) << shift; shift += 7n; } while (byte & 128); if (byte & 64) n -= 1n << shift; return Number(BigInt.asUintN(32, n)); }
function instructions(b) { assert.deepEqual([...b.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]); const c = {i: 8}; while (c.i < b.length) { const tag = b[c.i++], length = uleb(b, c), end = c.i + length; if (tag === 10) { assert.equal(uleb(b, c), 1); const size = uleb(b, c); assert.equal(c.i + size, end); const groups = uleb(b, c); for (let i = 0; i < groups; i++) { uleb(b, c); assert.ok([0x7f, 0x7e].includes(b[c.i++])); } return b.subarray(c.i, end); } c.i = end; } assert.fail('missing code body'); }
function unitPrefix(b, limbs) { const code = instructions(b), c = {i: 0}; for (const limb of limbs) { assert.equal(code[c.i++], 0x41); assert.equal(sleb(code, c), limb); } assert.deepEqual([...code.subarray(c.i, c.i + 8)], [0x20, 0, 0x20, 1, 0x20, 3, 0x10, 0]); }
function probeStructure(bytes, key) {
  const code = instructions(bytes), c = {i: 0};
  for (const value of [Number(key & 0xffffffffn), Number(key >> 32n)]) { assert.equal(code[c.i++], 0x41); assert.equal(sleb(code, c), value); }
  for (const [op, immediate] of [[0x20, 0], [0x20, 1], [0x20, 3], [0x10, 0], [0x22, 4]]) { assert.equal(code[c.i++], op); assert.equal(uleb(code, c), immediate); }
  assert.equal(code[c.i++], 0x04); assert.equal(code[c.i++], 0x40); assert.equal(code[c.i++], 0x20); assert.equal(uleb(code, c), 4); assert.equal(code[c.i++], 0x0f); assert.equal(code[c.i++], 0x0b);
  for (const local of [0, 1, 2, 3]) { assert.equal(code[c.i++], 0x20); assert.equal(uleb(code, c), local); }
  assert.equal(code[c.i++], 0x41); assert.equal(sleb(code, c), 0); assert.equal(code[c.i++], 0x11); assert.equal(uleb(code, c), 0); assert.equal(uleb(code, c), 0); assert.equal(code[c.i++], 0x0b); assert.equal(c.i, code.length, 'entire guard-first body has no memory/finder/marker/instrumentation instructions');
}
function metadata24(e, before, id, label, known) {
  const after = arena(e), v = new DataView(after.buffer), pointer = v.getUint32(TRANSFER + 16, true), length = v.getUint32(TRANSFER + 20, true), low = Number(id & 0xffffffffn), high = Number(id >> 32n);
  assert.ok(pointer > 0 && length > 8 && pointer + length <= refresh(e).bytes.length); const bytes = e.bytes.slice(pointer, pointer + length); assert.ok(WebAssembly.validate(bytes)); unitPrefix(bytes, [e.low, e.high, low, high]);
  const wanted = before.slice(), view = new DataView(wanted.buffer); [1, 24, low, high, pointer, length].forEach((x, i) => view.setUint32(TRANSFER + i * 4, x, true)); check(e, wanted, label);
  if (known) { assert.equal(pointer, known.pointer); assert.equal(length, known.length); assert.deepEqual(bytes, known.bytes); }
  metadata.push({label, key: e.key.toString(), id: id.toString(), pointer, length, sha256: hash(bytes)}); stats.metadata24_checks++; return {id, low, high, pointer, length, bytes};
}
function compileResident(e, blocks, gates, label, id) {
  descriptors(e, blocks, gates); const before = arena(e); assert.equal(e.api.compile_resident_with_gates(blocks.length, gates.length), 0); const record = metadata24(e, before, id, label);
  const module = new WebAssembly.Module(record.bytes); assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard_resident', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3: {guard_resident: e.api.guard_resident}}); assert.equal(instance.exports.run.length, 4);
  const file = `unit-${++stats.resident_compiles}.wasm`; writeFileSync(join(outputDir, file), record.bytes); artifacts[file] = hash(record.bytes); const u = {...record, file, run: instance.exports.run, module, instance, blocks, gates, label}; units.push(u); return u;
}
function getter(e, u) { const before = arena(e); assert.equal(e.api.resident_module(u.low, u.high), 0); metadata24(e, before, u.id, 'unchanged immutable resident getter', u); stats.resident_getters++; }
function run(e, u, budget, s, expectedExit, legacy = false) { const before = arena(e), wanted = before.slice(); assert.equal(u.run(e.base, e.base + 56, budget, e.base + 96), 0); wanted.set(stateBytes(s), 0); wanted.set(expectedExit, 56); check(e, wanted, 'real emitted numeric Gate stop'); stats[legacy ? 'direct_legacy_runs' : 'direct_resident_runs']++; }
function callRecord(token, id, s) { const b = new Uint8Array(112), v = new DataView(b.buffer); header(b, v, 'R3CF', 112); [token, id, 1, 0, s.eip, s.registers[4], 0x5000, 0].forEach((x, i) => v.setUint32(16 + i * 4, x, true)); return b; }
function section(label, before) { sections.push({label, stats: Object.fromEntries(Object.keys(stats).map(k => [k, stats[k] - before[k]]))}); }

const instance = new WebAssembly.Instance(engineModule, {}), api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
assert.equal(api.guard_dispatch_entry.length, 5);
const e = refresh({instance, api, key: KEY, low: Number(KEY & 0xffffffffn), high: Number(KEY >> 32n), memory: instance.exports.memory, base: 0, opened: false}); stats.engine_instances++;
guard(e, 5, 'absent instance precedes bad key and pointers', {low: 0, high: 0, pointers: [0xffffffff, 0xffffffff, 0xffffffff]});
assert.equal(api.open(12, e.low, e.high), 0); e.base = api.arena_ptr() >>> 0; e.opened = true; refresh(e); assert.ok(e.base > 0 && e.base + SIZE <= e.bytes.length);
assert.equal(api.map(0x7000, 3, 3), 0); for (const [address, value] of ram) word(e, address, value);
e.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
for (const [file, field, key] of [['probe.wasm', 'probe', KEY], ['foreign-probe.wasm', 'foreignProbe', KEY ^ (1n << 32n)]]) {
  const bytes = readFileSync(join(outputDir, file)), module = new WebAssembly.Module(bytes); probeStructure(bytes, key);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'}, {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'}]); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  e[field] = new WebAssembly.Instance(module, {env: {memory: e.memory, table: e.table}, ring3: {guard_dispatch_entry: e.api.guard_dispatch_entry}}); assert.equal(e[field].exports.run.length, 4);
}
section('engine initialization', Object.fromEntries(Object.keys(stats).map(k => [k, 0])));

let before = {...stats}; observeRAM(e, 'initial independent RAM model');
for (const value of [0, 1, 0xffffffff]) { reset(e, state(0xffffffff, value ? 0xcd7 : 2), value); guard(e, 0, 'empty registry and arbitrary output/cancel accepted'); }
const malformed = [['magic', 0, 0], ['version', 4, 2], ['profile', 6, 2], ['length', 8, 55], ['reserved', 12, 1], ['mandatory flag bit1', 52, 0xcd5], ['unsupported flag bit3', 52, 0xcdf]];
for (const [label, offset, value] of malformed) {
  reset(e, state(), 0xffffffff); if (offset === 0) e.bytes[e.base] = value; else if (offset === 4 || offset === 6) e.view.setUint16(e.base + offset, value, true); else e.view.setUint32(e.base + offset, value, true);
  guard(e, 2, `invalid own ${label}`); probe(e, 0, 2, 0, `header rejection before zero-budget child and null table: ${label}`); stats.header_cases++;
}
const canonical = [e.base, e.base + 56, e.base + 96];
for (let index = 0; index < 3; index++) for (const [label, value] of [['null', 0], ['shifted', canonical[index] + 1], ['MAX', 0xffffffff], ['overlap', canonical[(index + 1) % 3]], ['foreign numeric offset', e.base + 1024]]) {
  reset(e, state(), 1); e.bytes[e.base] = 0; const pointers = [...canonical]; pointers[index] = value;
  guard(e, 1, `${label} pointer${index} before malformed header`, {pointers}); probe(e, 0xffffffff, 1, 0, `${label} probe pointer${index} before null child`, {pointers}); stats.pointer_cases++;
}
for (const options of [{low: 0, high: 0}, {low: e.low ^ 1}, {high: e.high ^ 1}]) guard(e, 3, 'complete key precedes pointer/header errors', {...options, pointers: [0, 0, 0]});
probe(e, 0, 3, 0, 'foreign baked high key precedes pointer/header/budget', {foreign: true, pointers: [0, 0, 0]});
observeRAM(e, 'rejected empty entry controls preserve RAM'); section('empty authority header flags and numeric pointer controls', before);

before = {...stats};
assert.equal(api.map(0x1000, 2, 7), 0); upload(e, 0x1000, Buffer.from('8d4003', 'hex'), 'ordinary'); upload(e, 0x2000, Buffer.from('0f0b', 'hex'), 'resident-gate');
const gateId = 0x80000011, gate = compileResident(e, [[0x2000, 2]], [[0x2000, gateId]], 'actual resident Gate', 1n), ordinary = compileResident(e, [[0x1000, 3]], [], 'actual LEA unit', 2n);
e.table.set(0, ordinary.run); assert.equal(e.table.get(0), ordinary.run);
cancel(e, 0);
const beforeAck = arena(e), wantedAck = beforeAck.slice(), ackView = new DataView(wantedAck.buffer); [0x4e493352, 0x10001, 32, 0, 2, 0, 0, 0].forEach((x, i) => ackView.setUint32(TRANSFER + i * 4, x, true));
assert.equal(api.acknowledge_resident_installation(e.low, e.high, ordinary.low, ordinary.high, 0), 0); check(e, wantedAck, 'existing installation32 binding unchanged'); stats.installation32_checks++;
const start = state(), advanced = {...start, registers: [...start.registers], eip: 0x1003}; advanced.registers[0] = Number(BigInt.asUintN(32, BigInt(start.registers[0]) + 3n)); assert.equal(advanced.registers[0], 1);
for (const [budget, value, expectedState, expectedExit] of [[1, 0, advanced, exit(1, 1)], [0xffffffff, 0, advanced, exit(3, 1)], [0, 0, start, exit(1, 0)], [0, 1, start, exit(2, 0)], [0xffffffff, 0xffffffff, start, exit(2, 0)]]) {
  reset(e, start, value); guard(e, 0, 'entry guard ignores output/cancel and is budget-independent'); probe(e, budget, 0, 0, 'unchanged child LEA budget/cancel policy', {state: expectedState, exit: expectedExit}); assert.equal(e.table.get(0), ordinary.run);
}
reset(e, state(0xffffffff)); guard(e, 0, 'unknown PC is not entry authority'); probe(e, 1, 0, 0, 'unit decides unknown PC', {state: state(0xffffffff), exit: exit(3, 0)});
getter(e, ordinary); getter(e, gate); observeRAM(e, 'accepted child controls preserve RAM');
refresh(e).bytes.set(Buffer.from('8d4003', 'hex'), e.base + TRANSFER); assert.equal(api.upload(0x1000, 3), 0);
reset(e, start); guard(e, 0, 'stale code is not entry authority'); probe(e, 0, 0, 4, 'unit currency rejects stale table function even with zero budget');
section('real emitted child and stale versus entry authority', before);

before = {...stats};
const residentStop = state(0x2000); reset(e, residentStop); run(e, gate, 1, residentStop, exit(8, 0, 3, gateId));
const beforeResidentCapture = arena(e), expectedResidentCapture = beforeResidentCapture.slice(); expectedResidentCapture.set(callRecord(1, gateId, residentStop), TRANSFER);
assert.equal(api.capture_resident_call(e.low, e.high, gate.low, gate.high, 1, 0), 0); check(e, expectedResidentCapture, 'real resident Gate capture112'); stats.resident_captures++;
guard(e, 12, 'pending resident frame blocks dispatch entry'); probe(e, 0, 12, 0, 'pending guard precedes stale child and zero budget');
e.bytes[e.base] = 0; cancel(e, 1); guard(e, 12, 'pending Busy precedes pointer/header/cancel', {pointers: [0, 0, 0]}); probe(e, 1, 12, 0, 'pending Busy precedes bad pointers and stale child', {pointers: [0, 0, 0]});
guard(e, 3, 'wrong high key precedes pending Busy', {high: e.high ^ 1, pointers: [0, 0, 0]});
const beforeAbandon = arena(e); assert.equal(api.abandon_call(e.low, e.high, 1), 0); check(e, beforeAbandon, 'abandon changes private pending authority only'); stats.pending_abandons++;
guard(e, 2, 'after abandon malformed State is visible'); observeRAM(e, 'resident pending entry controls preserve RAM'); section('actual resident pending priority and private release', before);

before = {...stats};
assert.equal(api.map(0x4000, 1, 7), 0); const legacyX86 = Buffer.alloc(0x202, 0x90); legacyX86.set([0x0f, 0x0b], 0); legacyX86.set([0x0f, 0x0b], 0x200); upload(e, 0x4000, legacyX86, 'legacy-callback');
descriptors(e, [[0x4000, 2], [0x4100, 1], [0x4200, 2]], [[0x4000, 17], [0x4200, 18]]); assert.equal(api.compile_with_gates(3, 2), 0); assert.equal(api.generation(), 1);
const legacyPointer = api.module_ptr() >>> 0, legacyLength = api.module_len() >>> 0, legacyBytes = refresh(e).bytes.slice(legacyPointer, legacyPointer + legacyLength); unitPrefix(legacyBytes, [e.low, e.high, 1]);
const legacyModule = new WebAssembly.Module(legacyBytes); assert.deepEqual(WebAssembly.Module.imports(legacyModule), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard', kind: 'function'}]);
const legacyInstance = new WebAssembly.Instance(legacyModule, {env: {memory: e.memory}, ring3: {guard: e.api.guard}}), legacy = {run: legacyInstance.exports.run}; writeFileSync(join(outputDir, 'legacy.wasm'), legacyBytes); artifacts['legacy.wasm'] = hash(legacyBytes);
const legacyStop = state(0x4000); reset(e, legacyStop); run(e, legacy, 1, legacyStop, exit(8, 0, 3, 17), true);
const beforeLegacyCapture = arena(e), expectedLegacyCapture = beforeLegacyCapture.slice(); expectedLegacyCapture.set(callRecord(2, 17, legacyStop), TRANSFER);
assert.equal(api.capture_call(e.low, e.high, 1, 1, 0), 0); check(e, expectedLegacyCapture, 'real legacy Gate capture112'); stats.legacy_captures++;
const callbackState = {...legacyStop, registers: [...legacyStop.registers], eip: 0x4100}; callbackState.registers[4] = 0x8ffc;
const callbackRecord = new Uint8Array(64), callbackView = new DataView(callbackRecord.buffer); header(callbackRecord, callbackView, 'R3CB', 64); [3, 2, 1, 0, 0x4100, 0x8ffc, 0x4200, 18, 0, 0, 1, 0].forEach((x, i) => callbackView.setUint32(16 + i * 4, x, true));
const beforeBegin = arena(e), wantedBegin = beforeBegin.slice(); wantedBegin.set(stateBytes(callbackState), 0); wantedBegin.set(exit(3, 0, 3), 56); wantedBegin.set(callbackRecord, TRANSFER);
assert.equal(api.begin_callback(e.low, e.high, 1, 2, 0x4100, 0x4200, 18, 0), 0); check(e, wantedBegin, 'callback begin publishes real frame and private outer owner'); stats.callback_begins++; stats.callback_word_writes++; ram.set(0x8ffc, 0x4200); observeRAM(e, 'callback return word and neighbours');
guard(e, 12, 'suspended callback blocks dispatch entry'); probe(e, 0xffffffff, 12, 0, 'callback Busy rejects stale child');
e.bytes[e.base] = 0; cancel(e, 1); guard(e, 12, 'callback Busy precedes pointer/header/cancel', {pointers: [0, 0, 0]}); probe(e, 0, 12, 0, 'callback Busy precedes malformed input and budget', {pointers: [0, 0, 0]}); guard(e, 3, 'wrong full key precedes callback Busy', {high: e.high ^ 1});
const beforeAbort = arena(e), wantedAbort = beforeAbort.slice(); wantedAbort.set(stateBytes(legacyStop), 0); wantedAbort.set(exit(8, 0, 3, 17), 56);
assert.equal(api.abort_callback(e.low, e.high, 3), 0); check(e, wantedAbort, 'abort restores frozen outer State and Exit only'); stats.callback_aborts++;
guard(e, 12, 'restored outer pending frame remains Busy'); const beforeOuterAbandon = arena(e); assert.equal(api.abandon_call(e.low, e.high, 2), 0); check(e, beforeOuterAbandon, 'release restored private outer pending'); stats.pending_abandons++;
guard(e, 0, 'released callback accepts nonzero cancellation without executing'); observeRAM(e, 'abort retains callback RAM write and neighbours');
const beforeClose = arena(e); assert.equal(api.close(), 0); check(e, beforeClose, 'close preserves pinned arena'); stats.closed_engines++;
guard(e, 5, 'closed precedes key/pointer/header', {low: 0, high: 0, pointers: [0xffffffff, 0xffffffff, 0xffffffff]}); probe(e, 0, 5, 0, 'closed entry precedes budget and stale child', {pointers: [0xffffffff, 0xffffffff, 0xffffffff]}); section('real legacy callback Busy abort and closed authority', before);

const expectedStats = {engine_instances: 1, preopen_guard_calls: 1, direct_guard_calls: 46, direct_guard_success: 11, direct_guard_rejections: 35, probe_calls: 35, probe_guard_success: 7, probe_guard_rejections: 28, probe_child_calls: 7, probe_child_canonical: 6, probe_child_rejections: 1, direct_resident_runs: 1, direct_legacy_runs: 1, resident_compiles: 2, resident_getters: 2, metadata24_checks: 4, installation32_checks: 1, resident_captures: 1, legacy_captures: 1, pending_abandons: 2, callback_begins: 1, callback_aborts: 1, host_reads: 36, host_writes: 6, callback_word_writes: 1, full_arena_checks: 136, ram_snapshots: 6, header_cases: 7, pointer_cases: 15, closed_engines: 1};
assert.deepEqual(stats, expectedStats, 'all30 totals derived manually from authored operation paths before runtime');
assert.deepEqual(sections.map(s => [s.stats.direct_guard_calls, s.stats.direct_guard_success, s.stats.probe_calls, s.stats.probe_child_calls, s.stats.full_arena_checks, s.stats.host_reads, s.stats.metadata24_checks]), [[1, 0, 0, 0, 6, 0, 0], [28, 3, 23, 0, 63, 12, 0], [7, 7, 7, 7, 25, 6, 4], [4, 0, 2, 0, 15, 6, 0], [6, 1, 3, 0, 27, 12, 0]], 'independent initialization and four control-section sums');
assert.deepEqual(Object.fromEntries(Object.keys(stats).map(k => [k, sections.reduce((sum, s) => sum + s.stats[k], 0)])), expectedStats);
assert.equal(Object.keys(artifacts).length, 8); assert.equal(Object.keys(authored).length, 3); assert.equal(units.length, 2); assert.equal(metadata.length, 4); assert.equal(e.table.get(0), ordinary.run);
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', 'engine/tests/process_dispatch_entry.rs', 'engine/tests/process_dispatch_entry_wasm.rs', 'engine/tests/fixtures/p2-dispatch-entry/run.mjs', 'engine/tests/process_resident_installation.rs', 'engine/tests/process_resident_installation_wasm.rs', 'engine/tests/fixtures/p2-resident-installation/run.mjs'];
assert.equal(new Set(sourcePaths).size, sourcePaths.length); assert.equal(sourcePaths.length, productionPaths.length + 9);
const provenance = {
  stats, section_derivation: sections, actual_dispatch_entry_guard_calls: stats.direct_guard_calls + stats.probe_calls, actual_resident_guard_entries: stats.probe_child_calls + stats.direct_resident_runs, actual_legacy_guard_entries: stats.direct_legacy_runs,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(file => file.endsWith('.wasm')).sort().map(file => `${file}:${artifacts[file]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(file => [file, hash(readFileSync(join(root, file)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(file => `${file}:${hash(readFileSync(join(root, file)))}`).join('\n'))},
  authored, controls, unit_bindings: units.map(u => ({key: KEY.toString(), id: u.id.toString(), pointer: u.pointer, length: u.length, sha256: hash(u.bytes), file: u.file, blocks: u.blocks, gates: u.gates})), metadata24_observations: metadata,
  installation: {key: KEY.toString(), id: ordinary.id.toString(), slot: 0, pointer: ordinary.pointer, length: ordinary.length, sha256: hash(ordinary.bytes), table_reference_equals_known_run: true}, legacy: {key: KEY.toString(), generation: 1, pointer: legacyPointer, length: legacyLength, sha256: hash(legacyBytes)},
  resident_call_record_hex: Buffer.from(callRecord(1, gateId, residentStop)).toString('hex'), legacy_call_record_hex: Buffer.from(callRecord(2, 17, legacyStop)).toString('hex'), callback_record_hex: Buffer.from(callbackRecord).toString('hex'), expected_final_ram: [...ram],
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_dispatch_entry_wasm', '--test', 'process_resident_installation_wasm', '--', '--nocapture']},
  counts: 'all entry/child/control/arena/RAM totals are authored-path derivations frozen before runtime, not product or JS instrumentation. Entire probe instruction stream is decoded at boundaries: guard5 is first, rejected status returns before the sole run4 call_indirect and there are no memory/finder/marker instructions. Rejected early probes use an empty table; later probes use a stale unit, so Busy/header/pointer priority cannot be attributed to that child.',
  reused: 'existing emitter, resident guard/current snapshot, installation32, integer LEA, Gate, legacy guard, call112, callback64 frame publication/abort, helpers and ABI codecs are source/prior-proof reuse. Existing installation target alone is repeated; all its11Wasm+6fullx86/all35stats compare to327 without depending on ignored reference files. No old call/callback/corpus/browser or legacy zero-CPU replay.',
  claim: 'authority-only native process guard and safe Wasm binary entry validation, with synchronous Node direct export purity and tiny guard5-first table probe. Empty registry, unknown/stale PC, arbitrary output Exit and nonzero cancel are not rejected by entry authority; child budget/cancel/current-unit rules remain unchanged. All seven malformed header/flags classes and15 shifted/null/MAX/overlap/foreign numeric pointer cases are independent literal expectations with whole4236arena preservation. Accepted real LEA preserves flags and other GPRs; actual resident Gate capture and real legacy Gate capture→callback begin/abort provide pending/callback Busy without callback guest execution. Deliberate header/cancel corruption is explicitly a negative host-input control, not guest CPU advancement; abort restores the private outer State/Exit. All watched RAM/neighbour pages mapped independently of code. Engine memory/export pairing and table slot/function references are trusted-host obligations; same numeric offsets across memories are not authenticated. No borrow9 runtime exercise/reentry, product dispatcher/finder policy, resident callback migration, SDK/browser/async/race/slot reuse/performance/game/fullP2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({stats, section_derivation: sections, sources: sourcePaths.length, artifacts: Object.keys(artifacts).length, engine_sha256: provenance.engine_sha256, module_set_sha256: provenance.module_set_sha256, output: outputDir}));
