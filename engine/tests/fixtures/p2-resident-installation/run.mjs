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
const SIZE = 4236, TRANSFER = 140, KEY = 0x91345678e0000000n;
const names = ['open', 'close', 'arena_ptr', 'map', 'upload', 'compile_resident_with_gates', 'resident_module', 'find_resident', 'guard_resident', 'read32', 'write32', 'store_resident32', 'capture_resident_call', 'complete_resident_call', 'acknowledge_resident_installation', 'find_installed_resident'];
const stats = {engine_instances: 0, compiled_units: 0, metadata24_checks: 0, installation32_checks: 0, ack_success: 0, ack_failure: 0, idempotent_acks: 0, installed_find_success: 0, installed_find_failure: 0, logical_find_success: 0, dispatcher_calls: 0, dispatcher_canonical: 0, dispatcher_rejections: 0, dispatcher_child_entries: 0, dispatcher_lookup_calls: 0, dispatcher_lookup_success: 0, dispatcher_lookup_failure: 0, direct_unit_calls: 0, direct_unit_canonical: 0, direct_unit_rejections: 0, direct_guard_calls: 0, full_arena_checks: 0, initial_calls: 0, resumes: 0, host_reads: 0, host_writes: 0, guest_store32: 0, resident_captures: 0, resident_completions: 0, host_module_failures: 0, host_instance_failures: 0, host_table_failures: 0, platform_traps: 0, zero_progress_stops: 0, closed_engines: 0};
const artifacts = {}, authored = {}, units = [], metadata = [], installations = [], sections = [], algorithm = [], mismatches = [];
for (const file of ['dispatcher-1.wasm', 'dispatcher-2.wasm', 'dispatcher-3.wasm', 'wrong.wasm']) artifacts[file] = hash(readFileSync(join(outputDir, file)));
function refresh(e) { if (e.buffer !== e.memory.buffer) { e.buffer = e.memory.buffer; e.bytes = new Uint8Array(e.buffer); e.view = new DataView(e.buffer); } return e; }
function arena(e) { return refresh(e).bytes.slice(e.base, e.base + SIZE); }
function header(b, v, p, magic, size, version = 1) { b.set(Buffer.from(magic), p); v.setUint16(p + 4, version, true); v.setUint16(p + 6, 1, true); v.setUint32(p + 8, size, true); v.setUint32(p + 12, 0, true); }
function exit(reason, retired, version = 3, detail = 0) { const b = new Uint8Array(40), v = new DataView(b.buffer); header(b, v, 0, 'R3EX', 40, version); [reason, retired, detail, 0, 0, 0].forEach((x, i) => v.setUint32(16 + i * 4, x, true)); return b; }
function helper(value = 0) { const b = new Uint8Array(40), v = new DataView(b.buffer); header(b, v, 0, 'R3MH', 40); v.setUint32(20, value, true); return b; }
function copy(s) { return {...s, registers: [...s.registers]}; }
function state(eax = 0xfffffffd, ecx = 5, flags = 0x403, pc = 0x1000) { return {registers: [eax, ecx, 0x23456789, 0x3456789a, 0x9000, 0x56789abc, 0x6789abcd, 0x789abcde], eip: pc, eflags: flags}; }
function stateBytes(s) { const b = new Uint8Array(56), v = new DataView(b.buffer); header(b, v, 0, 'R3ST', 56); s.registers.forEach((x, i) => v.setUint32(16 + i * 4, x, true)); v.setUint32(48, s.eip, true); v.setUint32(52, s.eflags, true); return b; }
function reset(e, s, cancel = 0) { refresh(e).bytes.set(stateBytes(s), e.base); e.bytes.fill(0xa5, e.base + 56, e.base + 96); e.view.setUint32(e.base + 96, cancel, true); e.bytes.fill(0x5a, e.base + 100, e.base + 140); }
function cancel(e, value) { refresh(e).view.setUint32(e.base + 96, value, true); }
function check(e, wanted, label) { assert.deepEqual(arena(e), wanted, `${label}: full4236 arena`); stats.full_arena_checks++; }
function unchanged(e, action, status, label) { const before = arena(e); assert.equal(action(), status, label); check(e, before, label); }
function installRecord(u, slot) { const b = new Uint8Array(32), v = new DataView(b.buffer); header(b, v, 0, 'R3IN', 32); v.setUint32(16, u.low, true); v.setUint32(20, u.high, true); v.setUint32(24, slot, true); return b; }
function tuple(e, u, slot, label, relation = true) { const actual = e.table.get(slot); assert.equal(actual === u.run, relation, label); (relation ? installations : mismatches).push({label, key: e.key.toString(), id: u.id.toString(), slot, pointer: u.pointer, length: u.length, sha256: hash(u.bytes), table_reference_equals_run: relation}); stats.installation32_checks++; }
function refreshDispatcher(e) { const bytes = readFileSync(join(outputDir, `dispatcher-${e.ordinal}.wasm`)), module = new WebAssembly.Module(bytes); assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'}, {module: 'ring3', name: 'find_installed_resident', kind: 'function'}]); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]); e.dispatcher = new WebAssembly.Instance(module, {env: {memory: e.memory, table: e.table}, ring3: {find_installed_resident: e.api.find_installed_resident}}); assert.equal(e.dispatcher.exports.run.length, 4); }
function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {}), api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]])); for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const ordinal = ++stats.engine_instances, key = KEY + BigInt(ordinal), e = refresh({instance, api, ordinal, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n), memory: instance.exports.memory, nextId: 0, installed: new Map()});
  assert.equal(api.open(16, e.low, e.high), 0); e.base = api.arena_ptr() >>> 0; refresh(e); assert.ok(e.base > 0 && e.base + SIZE <= e.bytes.length);
  for (const [name, length] of [['acknowledge_resident_installation', 5], ['find_installed_resident', 3], ['guard_resident', 7], ['store_resident32', 6], ['capture_resident_call', 6], ['complete_resident_call', 6]]) assert.equal(api[name].length, length);
  e.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8}); refreshDispatcher(e); return e;
}
function upload(e, pc, bytes, label) { refresh(e).bytes.set(bytes, e.base + TRANSFER); assert.equal(e.api.upload(pc, bytes.length), 0); const file = `engine-${e.ordinal}-${label}.x86`; writeFileSync(join(outputDir, file), bytes); artifacts[file] = hash(bytes); authored[file] = Buffer.from(bytes).toString('hex'); }
function descriptors(e, blocks, gates) { refresh(e); [...blocks, ...gates].forEach(([pc, value], i) => { e.view.setUint32(e.base + TRANSFER + i * 8, pc, true); e.view.setUint32(e.base + TRANSFER + i * 8 + 4, value, true); }); }
function uleb(b, c) { let n = 0, shift = 0, byte; do { byte = b[c.i++]; assert.ok(byte !== undefined && shift <= 28); n += (byte & 127) * 2 ** shift; shift += 7; } while (byte & 128); return n; }
function sleb(b, c) { let n = 0n, shift = 0n, byte; do { byte = b[c.i++]; assert.ok(byte !== undefined && shift <= 28n); n |= BigInt(byte & 127) << shift; shift += 7n; } while (byte & 128); if (byte & 64) n -= 1n << shift; return Number(BigInt.asUintN(32, n)); }
function prefix(b, e, id) { const c = {i: 8}; let found = false; while (c.i < b.length) { const tag = b[c.i++], n = uleb(b, c), end = c.i + n; if (tag === 10) { assert.equal(uleb(b, c), 1); const bodyLength = uleb(b, c); assert.equal(c.i + bodyLength, end); const groups = uleb(b, c); for (let i = 0; i < groups; i++) { uleb(b, c); assert.ok([0x7f, 0x7e].includes(b[c.i++])); } const limbs = []; for (let i = 0; i < 4; i++) { assert.equal(b[c.i++], 0x41); limbs.push(sleb(b, c)); } assert.deepEqual(limbs, [e.low, e.high, Number(id & 0xffffffffn), Number(id >> 32n)]); assert.deepEqual([...b.subarray(c.i, c.i + 8)], [0x20, 0, 0x20, 1, 0x20, 3, 0x10, 0]); found = true; } c.i = end; } assert.ok(found); }
function metadata24(e, before, expectedId, label, known) {
  const after = arena(e), v = new DataView(after.buffer), low = v.getUint32(TRANSFER + 8, true), high = v.getUint32(TRANSFER + 12, true), pointer = v.getUint32(TRANSFER + 16, true), length = v.getUint32(TRANSFER + 20, true), id = BigInt(low) | BigInt(high) << 32n;
  assert.equal(id, expectedId); assert.ok(pointer > 0 && length > 8 && pointer + length <= refresh(e).bytes.length); const bytes = e.bytes.slice(pointer, pointer + length); assert.ok(WebAssembly.validate(bytes)); prefix(bytes, e, id);
  const expected = before.slice(), output = new DataView(expected.buffer); [1, 24, low, high, pointer, length].forEach((x, i) => output.setUint32(TRANSFER + i * 4, x, true)); check(e, expected, label);
  if (known) { assert.equal(pointer, known.pointer); assert.equal(length, known.length); assert.deepEqual(bytes, known.bytes); }
  metadata.push({label, key: e.key.toString(), id: id.toString(), pointer, length, sha256: hash(bytes)}); stats.metadata24_checks++; return {id, low, high, pointer, length, bytes};
}
function compile(e, blocks, gates, label, stores = false) {
  descriptors(e, blocks, gates); const before = arena(e); assert.equal(e.api.compile_resident_with_gates(blocks.length, gates.length), 0); const selected = metadata24(e, before, BigInt(++e.nextId), label);
  const module = new WebAssembly.Module(selected.bytes); assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard_resident', kind: 'function'}, ...(stores ? [{module: 'ring3', name: 'store_resident32', kind: 'function'}] : [])]); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3: {guard_resident: e.api.guard_resident, store_resident32: e.api.store_resident32}}), file = `unit-${++stats.compiled_units}.wasm`; assert.equal(instance.exports.run.length, 4); writeFileSync(join(outputDir, file), selected.bytes); artifacts[file] = hash(selected.bytes); const u = {...selected, module, instance, run: instance.exports.run, file, key: e.key.toString(), blocks, gates, label}; units.push(u); return u;
}
function logical(e, pc, u) { const before = arena(e); assert.equal(e.api.find_resident(pc), 0); metadata24(e, before, u.id, 'logical finder remains available', u); stats.logical_find_success++; }
function getter(e, u) { const before = arena(e); assert.equal(e.api.resident_module(u.low, u.high), 0); metadata24(e, before, u.id, 'immutable module getter', u); }
function ack(e, u, slot, status = 0, options = {}) {
  const before = arena(e), args = [options.low ?? e.low, options.high ?? e.high, options.idLow ?? u.low, options.idHigh ?? u.high, slot]; assert.equal(e.api.acknowledge_resident_installation(...args), status);
  const wanted = before.slice(); if (!status) { wanted.set(installRecord(u, slot), TRANSFER); tuple(e, u, slot, options.relation === false ? 'isolated host mismatch acknowledgement' : 'acknowledged installation', options.relation ?? true); if (e.installed.has(u.id)) stats.idempotent_acks++; e.installed.set(u.id, slot); stats.ack_success++; } else stats.ack_failure++; check(e, wanted, 'ack publication or failure purity');
}
function installed(e, pc, u, slot, status = 0, options = {}) {
  const before = arena(e); assert.equal(e.api.find_installed_resident(options.low ?? e.low, options.high ?? e.high, pc), status); const wanted = before.slice();
  if (!status) { wanted.set(installRecord(u, slot), TRANSFER); tuple(e, u, slot, 'installed finder'); stats.installed_find_success++; } else stats.installed_find_failure++; check(e, wanted, 'installed finder changes only32 on success');
}
function guard(e, u, status = 0, options = {}) { unchanged(e, () => e.api.guard_resident(options.low ?? e.low, options.high ?? e.high, u.low, u.high, e.base, e.base + 56, e.base + 96), status, 'direct guard independent of installation'); stats.direct_guard_calls++; }
function direct(e, u, budget, expected, expectedExit, status = 0) { const before = arena(e); assert.equal(u.run(e.base, e.base + 56, budget, e.base + 96), status); const wanted = before.slice(); if (!status) { wanted.set(stateBytes(expected), 0); wanted.set(expectedExit, 56); stats.direct_unit_canonical++; } else stats.direct_unit_rejections++; check(e, wanted, 'direct unit compatibility'); stats.direct_unit_calls++; }
function copied(e, u) { return new WebAssembly.Instance(new WebAssembly.Module(u.bytes.slice()), {env: {memory: e.memory}, ring3: {guard_resident: e.api.guard_resident, store_resident32: e.api.store_resident32}}).exports.run; }
function dispatch(e, budget, expected, expectedExit, plan, continuation, status = 0, expectedHelper) {
  const before = arena(e); assert.equal(e.dispatcher.exports.run(e.base, e.base + 56, budget, e.base + 96), status); const wanted = before.slice(); wanted.set(stateBytes(expected), 0); if (expectedExit) wanted.set(expectedExit, 56); if (expectedHelper) wanted.set(expectedHelper, 100);
  if (plan.last) { wanted.set(installRecord(plan.last, plan.slot), TRANSFER); tuple(e, plan.last, plan.slot, 'dispatcher last successful lookup', plan.relation ?? true); }
  check(e, wanted, 'returning dispatcher'); stats.dispatcher_calls++; stats[status ? 'dispatcher_rejections' : 'dispatcher_canonical']++; stats.dispatcher_child_entries += plan.children; stats.dispatcher_lookup_calls += plan.lookups; stats.dispatcher_lookup_success += plan.successes; stats.dispatcher_lookup_failure += plan.lookups - plan.successes;
  if (continuation) stats[continuation === 'initial' ? 'initial_calls' : 'resumes']++;
}
function word(e, address, value) { unchangedHelper(e, () => e.api.write32(address, value), helper(), 'host RAM setup'); stats.host_writes++; }
function observe(e, address, value) { unchangedHelper(e, () => e.api.read32(address), helper(value), 'host RAM observation'); stats.host_reads++; }
function unchangedHelper(e, action, expected, label) { const before = arena(e); assert.equal(action(), 0); const wanted = before.slice(); wanted.set(expected, 100); check(e, wanted, label); }
function close(e) { unchanged(e, () => e.api.close(), 0, 'close preserves arena'); stats.closed_engines++; }
function section(name, before) { sections.push({name, stats: Object.fromEntries(Object.entries(stats).map(([key, value]) => [key, value - before[key]]))}); }

function flags(op, operand, oldFlags) {
  const a = BigInt(operand), delta = op === 'inc' ? 1n : -1n, result = Number(BigInt.asUintN(32, a + delta)); let f = (oldFlags & 0x401) | 2, parity = 0;
  for (let byte = result & 255; byte; byte >>>= 1) parity += byte & 1; if (parity % 2 === 0) f |= 4; if (!result) f |= 0x40; if (result >= 0x80000000) f |= 0x80;
  if (op === 'inc' ? (operand & 15) === 15 : (operand & 15) === 0) f |= 0x10; const signed = BigInt.asIntN(32, a) + delta; if (signed < -2147483648n || signed > 2147483647n) f |= 0x800; return f;
}
for (const row of [['inc', 0xffffffff, 3, 0x57], ['inc', 0x7fffffff, 2, 0x896], ['dec', 1, 0x403, 0x447], ['dec', 0, 2, 0x96], ['dec', 0x80000000, 2, 0x816]]) assert.equal(flags(...row.slice(0, 3)), row[3]);
function trace(start, steps) { const n = start.registers[1], cycles = Math.floor(steps / 4), phase = steps % 4, s = copy(start); assert.ok(steps >= 0 && steps <= 4 * n); s.registers[0] = Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(cycles + (phase ? 1 : 0)))); s.registers[1] = n - cycles - (phase === 3 ? 1 : 0); s.eip = steps === 4 * n ? 0x2007 : [0x1000, 0x1001, 0x2000, 0x2001][phase]; if (steps) { const inc = phase === 1 || phase === 2, operand = inc ? Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(cycles))) : n - cycles + (phase === 0 ? 1 : 0); s.eflags = flags(inc ? 'inc' : 'dec', operand, start.eflags); } return s; }

function slice(e, start, from, budget, a, b, continuation) {
  const remaining = start.registers[1] * 4 - from, to = Math.min(start.registers[1] * 4, from + budget), retired = to - from;
  const children = retired ? Math.ceil(to / 2) - Math.floor(from / 2) : 0, last = children ? (Math.floor((to - 1) / 2) % 2 ? b : a) : undefined;
  const miss = budget > remaining, lookups = children + Number(miss), expectedExit = exit(budget <= remaining ? 1 : 3, retired, budget === 0 || miss ? 3 : 1);
  dispatch(e, budget, trace(start, to), expectedExit, {children, lookups, successes: children, last, slot: last ? e.installed.get(last.id) : undefined}, continuation); return to;
}

let before = {...stats};
const main = fresh();
assert.equal(main.api.map(0x1000, 2, 7), 0); assert.equal(main.api.map(0x7000, 3, 3), 0);
const aCode = Buffer.from('40e9fa0f0000', 'hex'), bCode = Buffer.from('490f85f9efffff', 'hex');
upload(main, 0x1000, aCode, 'a'); upload(main, 0x2000, bCode, 'b');
const a = compile(main, [[0x1000, 6]], [], 'ordinary A'), b = compile(main, [[0x2000, 7]], [], 'ordinary B prepared');
const watched = [[0x7ffc, 0x11223344], [0x8000, 0x89abcdef], [0x8004, 0x76543210], [0x8008, 0x55667788]];
for (const [address, value] of watched) word(main, address, value);
for (const pc of [0, 0x1000, 0x1001, 0x2000, 0x2001, 0xffffffff]) installed(main, pc, undefined, undefined, 17);
logical(main, 0x1000, a); getter(main, b); guard(main, b);
const directStart = state(0xffffffff, 3); reset(main, trace(directStart, 2)); direct(main, b, 1, trace(directStart, 3), exit(1, 1, 1));
main.table.set(0, a.run); ack(main, a, 0); ack(main, a, 0);
ack(main, b, 0, 7); ack(main, a, 1, 7); ack(main, b, 8, 7); ack(main, b, 0xffffffff, 3, {idHigh: 1});
cancel(main, 1); ack(main, a, 0, 16); installed(main, 0x1001, a, 0); cancel(main, 0);
const hostBefore = arena(main);
assert.throws(() => new WebAssembly.Module(Uint8Array.of(0)), WebAssembly.CompileError); stats.host_module_failures++; check(main, hostBefore, 'malformed module before ack');
assert.throws(() => new WebAssembly.Instance(b.module, {env: {memory: main.memory}, ring3: {guard_resident: main.api.read32}}), WebAssembly.LinkError); stats.host_instance_failures++; check(main, hostBefore, 'wrong import signature before ack');
assert.throws(() => main.table.set(8, b.run), RangeError); stats.host_table_failures++; check(main, hostBefore, 'out-of-bounds table installation before ack');
main.table.set(1, b.run); ack(main, b, 1, 3, {high: main.high ^ 1}); main.table.set(1, null); installed(main, 0x2000, undefined, undefined, 17); assert.equal(main.table.get(0), a.run);
const coldStart = state(); reset(main, coldStart);
dispatch(main, 0xffffffff, trace(coldStart, 2), exit(3, 2), {children: 1, lookups: 2, successes: 1, last: a, slot: 0}, 'initial');
logical(main, 0x2000, b); getter(main, b); main.table.set(1, b.run); ack(main, b, 1);
slice(main, coldStart, 2, 0xffffffff, a, b, 'resume'); for (const [address, value] of watched) observe(main, address, value);
const stableWarm = [main.memory, main.table, main.dispatcher, a.module, a.instance, b.module, b.instance, a.run, b.run];
for (const [n, eax] of [[1, 0xffffffff], [2, 0x7fffffff], [5, 0xfffffffd]]) {
  const start = state(eax, n);
  for (const budget of [0, 1, 3, n * 4, 0xffffffff]) { reset(main, start); slice(main, start, 0, budget, a, b); assert.deepEqual([main.memory, main.table, main.dispatcher, a.module, a.instance, b.module, b.instance, a.run, b.run], stableWarm); assert.equal(main.table.get(0), a.run); assert.equal(main.table.get(1), b.run); }
  const final = trace(start, 4 * n); assert.equal(final.registers[0], Number(BigInt.asUintN(32, BigInt(eax) + BigInt(n)))); assert.equal(final.registers[1], 0); assert.equal(final.eflags, 0x447);
  algorithm.push({n, input_eax: eax, final_eax: final.registers[0], final_ecx: 0, flags: 0x447, retired: 4 * n, child_entries: 2 * n, full_budget: 0xffffffff});
}
const splitStart = state(0x7ffffffe, 2); reset(main, splitStart); let position = 0;
for (const [index, budget] of [1, 1, 1, 0, 2, 3, 1].entries()) {
  position = slice(main, splitStart, position, budget, a, b, index ? 'resume' : 'initial');
  if (index === 2) { cancel(main, 1); installed(main, 0x2001, b, 1); ack(main, a, 0, 16); dispatch(main, 0, trace(splitStart, position), exit(2, 0), {children: 0, lookups: 0, successes: 0}, 'resume'); cancel(main, 0); }
}
assert.equal(position, 8);
for (const pc of [0, 0x1002, 0x2002, 0xffffffff]) installed(main, pc, undefined, undefined, 17);
const unknown = state(7, 1, 0x403, 0xffffffff); reset(main, unknown); dispatch(main, 9, unknown, exit(3, 0), {children: 0, lookups: 1, successes: 0});
for (const [address, value] of watched) observe(main, address, value);
word(main, 0x2000, 0xf9850f49); installed(main, 0x2000, undefined, undefined, 4); guard(main, b, 4);
cancel(main, 1); ack(main, b, 8, 4); cancel(main, 0);
const staleStart = state(9, 1); reset(main, staleStart);
dispatch(main, 9, trace(staleStart, 2), exit(3, 2, 1), {children: 1, lookups: 2, successes: 1, last: a, slot: 0}, 'initial', 4);
direct(main, b, 1, undefined, undefined, 4); direct(main, {...b, run: copied(main, b)}, 1, undefined, undefined, 4);
const freshB = compile(main, [[0x2000, 7]], [], 'fresh B at same PC'); assert.notEqual(freshB.id, b.id);
installed(main, 0x2000, undefined, undefined, 17); logical(main, 0x2000, freshB); ack(main, freshB, 1, 7);
main.table.set(2, freshB.run); ack(main, freshB, 2); assert.equal(main.table.get(1), b.run); installed(main, 0x2001, freshB, 2); slice(main, staleStart, 2, 9, a, freshB, 'resume');
getter(main, a); getter(main, freshB); for (const [address, value] of watched) observe(main, address, value);
close(main); installed(main, 0x1000, undefined, undefined, 5); ack(main, a, 0, 5); direct(main, a, 1, undefined, undefined, 5);
section('cold installation, warm arithmetic and retained stale slot', before);

before = {...stats};
const gate = fresh(), gateId = 0x80000011;
assert.equal(gate.api.map(0x3000, 2, 7), 0); assert.equal(gate.api.map(0x7000, 3, 3), 0);
upload(gate, 0x4000, Buffer.from('0f0b', 'hex'), 'gate'); upload(gate, 0x3000, Buffer.from('e8fb0f00008d4003', 'hex'), 'caller');
const gateB = compile(gate, [[0x4000, 2]], [[0x4000, gateId]], 'registered Gate B'), callerA = compile(gate, [[0x3000, 5], [0x3005, 3]], [], 'CALL and caller return', true);
gate.table.set(1, gateB.run); gate.table.set(0, callerA.run); ack(gate, gateB, 1); ack(gate, callerA, 0);
const stackWords = [[0x8ff8, 0x11223344], [0x8ffc, 0x89abcdef], [0x9000, 0x55667788]]; for (const [address, value] of stackWords) word(gate, address, value);
const callStart = state(0x89abcdef, 3, 0xcd7, 0x3000), stopped = copy(callStart); stopped.registers[4] = 0x8ffc; stopped.eip = 0x4000; reset(gate, callStart);
dispatch(gate, 9, stopped, exit(8, 1, 3, gateId), {children: 2, lookups: 2, successes: 2, last: gateB, slot: 1}, 'initial', 0, helper()); stats.guest_store32++;
stackWords[1][1] = 0x3005; for (const [address, value] of stackWords) observe(gate, address, value);
const expectedCall = new Uint8Array(112), expectedCallView = new DataView(expectedCall.buffer); header(expectedCall, expectedCallView, 0, 'R3CF', 112);
[1, gateId, 1, 0, 0x4000, 0x8ffc, 0x3005, 0].forEach((x, i) => expectedCallView.setUint32(16 + i * 4, x, true));
const beforeCapture = arena(gate), wantedCapture = beforeCapture.slice(); wantedCapture.set(expectedCall, TRANSFER);
assert.equal(gate.api.capture_resident_call(gate.low, gate.high, gateB.low, gateB.high, 1, 0), 0); check(gate, wantedCapture, 'capture aggregate Gate prefix publishes only112'); stats.resident_captures++;
const savedCall = Buffer.from(arena(gate).slice(TRANSFER, TRANSFER + 112)), scalar = Number(BigInt.asUintN(32, BigInt(savedCall.readUInt32LE(20)) * 3n + 7n)); assert.equal(scalar, 0x8000003a);
installed(gate, 0x4000, gateB, 1); getter(gate, gateB); cancel(gate, 1); ack(gate, gateB, 8, 12); installed(gate, 0x4000, gateB, 1); cancel(gate, 0);
guard(gate, gateB, 12);
dispatch(gate, 1, stopped, undefined, {children: 1, lookups: 1, successes: 1, last: gateB, slot: 1}, undefined, 12);
assert.deepEqual(savedCall, Buffer.from(expectedCall), 'saved host record copy survives transfer replacement');
const completed = copy(stopped); completed.registers[0] = scalar; completed.registers[4] = 0x9000; completed.eip = 0x3005;
const beforeCompletion = arena(gate), wantedCompletion = beforeCompletion.slice(); wantedCompletion.set(stateBytes(completed), 0); wantedCompletion.set(exit(3, 0), 56);
assert.equal(gate.api.complete_resident_call(gate.low, gate.high, gateB.low, gateB.high, 1, scalar), 0); check(gate, wantedCompletion, 'completion retains private frozen frame after metadata lookup'); stats.resident_completions++;
const returned = copy(completed); returned.registers[0] = 0x8000003d; returned.eip = 0x3008;
dispatch(gate, 9, returned, exit(3, 1), {children: 1, lookups: 2, successes: 1, last: callerA, slot: 0}, 'resume');
for (const [address, value] of stackWords) observe(gate, address, value); installed(gate, 0x3008, undefined, undefined, 17);
const callFlow = {key: gate.key.toString(), gate_unit_id: gateB.id.toString(), caller_unit_id: callerA.id.toString(), aggregate_gate_exit_hex: Buffer.from(exit(8, 1, 3, gateId)).toString('hex'), call_record_hex: savedCall.toString('hex'), scalar_result: scalar, completion_state: completed, return_state: returned, guest_store32_calls: 1, no_intermediate_host_cpu_writes: true};
close(gate); installed(gate, 0x4000, undefined, undefined, 5); ack(gate, gateB, 1, 5);
section('installed CALL Gate frozen capture and caller return', before);

before = {...stats};
const control = fresh(); assert.equal(control.api.map(0x5000, 1, 7), 0);
upload(control, 0x5000, Buffer.from('90', 'hex'), 'selected'); upload(control, 0x5100, Buffer.from('90', 'hex'), 'other');
const selected = compile(control, [[0x5000, 1]], [], 'selected control unit'), other = compile(control, [[0x5100, 1]], [], 'same-signature different membership unit');
assert.equal(selected.id, a.id); assert.notEqual(control.key, main.key);
control.table.set(0, other.run); assert.equal(control.table.get(0), other.run); ack(control, selected, 0, 0, {relation: false}); control.table.set(1, other.run); ack(control, other, 1); installed(control, 0x5100, other, 1);
const mismatchStart = state(5, 1, 0x403, 0x5000); reset(control, mismatchStart);
dispatch(control, 9, mismatchStart, exit(3, 0, 1), {children: 1, lookups: 1, successes: 1, last: selected, slot: 0, relation: false}); stats.zero_progress_stops++;
control.table.set(0, null); reset(control, mismatchStart); dispatch(control, 9, mismatchStart, exit(3, 0), {children: 0, lookups: 1, successes: 1, last: selected, slot: 0, relation: false});
const wrongModule = new WebAssembly.Module(readFileSync(join(outputDir, 'wrong.wasm'))); assert.deepEqual(WebAssembly.Module.imports(wrongModule), []); assert.deepEqual(WebAssembly.Module.exports(wrongModule), [{name: 'wrong', kind: 'function'}]);
const wrong = new WebAssembly.Instance(wrongModule, {}).exports.wrong; assert.equal(wrong.length, 0); control.table.set(0, wrong); reset(control, mismatchStart);
const beforeTrap = arena(control), wantedTrap = beforeTrap.slice(); wantedTrap.set(installRecord(selected, 0), TRANSFER);
assert.throws(() => control.dispatcher.exports.run(control.base, control.base + 56, 9, control.base + 96), WebAssembly.RuntimeError); tuple(control, selected, 0, 'wrong-signature trap after finder success', false); check(control, wantedTrap, 'platform trap preserves CPU exit helper with prior lookup32'); stats.platform_traps++; stats.dispatcher_calls++; stats.dispatcher_lookup_calls++; stats.dispatcher_lookup_success++;
direct(control, {...a, run: copied(control, a)}, 1, undefined, undefined, 3); guard(control, a, 3, {low: main.low, high: main.high}); installed(control, 0x5100, undefined, undefined, 3, {high: control.high ^ 1});
const originalDispatcher = control.dispatcher, foreignDispatcher = new WebAssembly.Module(readFileSync(join(outputDir, 'dispatcher-1.wasm')));
control.dispatcher = new WebAssembly.Instance(foreignDispatcher, {env: {memory: control.memory, table: control.table}, ring3: {find_installed_resident: control.api.find_installed_resident}});
dispatch(control, 9, mismatchStart, undefined, {children: 0, lookups: 1, successes: 0}, undefined, 3); control.dispatcher = originalDispatcher;
close(control); installed(control, 0x5000, undefined, undefined, 5); ack(control, selected, 0, 5); direct(control, selected, 1, undefined, undefined, 5);
dispatch(control, 9, mismatchStart, undefined, {children: 0, lookups: 1, successes: 0}, undefined, 5);
dispatch(control, 0, mismatchStart, exit(1, 0), {children: 0, lookups: 0, successes: 0});
section('isolated trusted-host table mismatch foreign and closed controls', before);

const expectedStats = {engine_instances: 3, compiled_units: 7, metadata24_checks: 15, installation32_checks: 41, ack_success: 8, ack_failure: 13, idempotent_acks: 1, installed_find_success: 6, installed_find_failure: 18, logical_find_success: 3, dispatcher_calls: 37, dispatcher_canonical: 32, dispatcher_rejections: 4, dispatcher_child_entries: 65, dispatcher_lookup_calls: 79, dispatcher_lookup_success: 67, dispatcher_lookup_failure: 12, direct_unit_calls: 6, direct_unit_canonical: 1, direct_unit_rejections: 5, direct_guard_calls: 4, full_arena_checks: 141, initial_calls: 4, resumes: 10, host_reads: 18, host_writes: 8, guest_store32: 1, resident_captures: 1, resident_completions: 1, host_module_failures: 1, host_instance_failures: 1, host_table_failures: 1, platform_traps: 1, zero_progress_stops: 1, closed_engines: 3};
assert.deepEqual(stats, expectedStats, 'manually derived authored-path counts before actual execution');
assert.deepEqual(sections.map(s => [s.stats.dispatcher_calls, s.stats.dispatcher_child_entries, s.stats.dispatcher_lookup_calls, s.stats.full_arena_checks, s.stats.metadata24_checks, s.stats.installation32_checks]), [[28, 60, 69, 96, 10, 28], [3, 4, 5, 27, 3, 7], [6, 1, 5, 18, 2, 6]], 'independent three-context derivation');
assert.equal(metadata.length, 15); assert.equal(installations.length, 37); assert.equal(mismatches.length, 4);
assert.equal(Object.keys(artifacts).length, 17); assert.equal(Object.keys(authored).length, 6);
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', 'engine/tests/process_resident_installation_wasm.rs', 'engine/tests/fixtures/p2-resident-installation/run.mjs', 'engine/tests/process_resident_installation.rs', 'engine/tests/process_resident_wasm.rs', 'engine/tests/fixtures/p2-resident-process/run.mjs', 'engine/tests/process_resident.rs'];
assert.equal(new Set(sourcePaths).size, sourcePaths.length); assert.equal(sourcePaths.length, productionPaths.length + 9);
const provenance = {
  stats, section_derivation: sections, actual_child_guard_entries: stats.dispatcher_child_entries + stats.direct_unit_calls, actual_exported_guard_calls_from_authored_paths: stats.dispatcher_child_entries + stats.direct_unit_calls + stats.direct_guard_calls, bound_store_identity_checks: stats.guest_store32,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  authored, algorithm, call_flow: callFlow, unit_bindings: units.map(u => ({key: u.key, id: u.id.toString(), pointer: u.pointer, length: u.length, sha256: hash(u.bytes), file: u.file, blocks: u.blocks, gates: u.gates})), metadata24_observations: metadata, positive_installation32_observations: installations, isolated_host_mismatch_observations: mismatches,
  same_signature_host_mismatch: {key: control.key.toString(), selected_id: selected.id.toString(), selected_sha256: hash(selected.bytes), selected_slot: 0, actual_target_id: other.id.toString(), actual_target_sha256: hash(other.bytes), actual_target_reference_equals_other_run: true, table_staged_before_ack: true, target_has_no_selected_pc: 0x5000, child_exit_version: 1, child_exit_reason: 3, child_retired: 0, native_table_authentication_claim: false},
  old_parity_reference: {path: 'target/p2-resident-process-fixtures/wasm-1244-1790971873860424000/provenance.json', sha256: '46ae7cf6aee4d1cd32ed8bdb619c5be527b6a15636fe1de8ddfdbd7b8fbe5e14', module_set_sha256: 'c062117d8053eb46089efe8a911cd94de3a038a60c0cae2419c7d8951c43e929', wasm_modules: 14, authored_inputs: 4, old_actual_runs: 91, old_exported_guards: 111, note: 'reference only; this portable integration does not read or require ignored historical output. Parent delivery compares all18 full artifacts and all old stats after the separate old target runs once.'},
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_installation_wasm', '--test', 'process_resident_wasm', '--', '--nocapture']},
  counts: 'all child/lookup/guard/store totals derive from fixed authored paths and independently checked output oracles; no JS or Wasm instrumentation counter, intercepted import, readiness bitmap or observed-output expectation. Platform wrong-signature trap attempts enter no child guard. Positive reference observations assert table.get(slot)===known instance.exports.run; false platform relations are isolated and never count as authenticated installation.',
  reused: 'unchanged registry/currency/guard7/emitter/Store6/CALL/Gate/capture112/scalar completion/Exit/header/helper codecs and existing memory/stack/control/native Busy/exhaustion/late-helper proofs are source/prior-proof reuse. Root captures three legacy modules and full x86 inputs with zero guest CPU. Old resident process actual target alone is repeated; no call/callback/deep/ISA corpus or two-million platform experiment.',
  claim: 'process-owned fixed8 fresh/no-reuse installed bindings plus actual synchronous Node cold table installation→ack→direct Wasm finder3→returning call_indirect run4 handoff; A/B full4n state/flags/budgets/cancel/interior and staleold/freshsamePC17/newslot continuation, one CALL/Gate aggregate Exitv3→resident capture112→BigInt scalar→complete56+Exitv3NeedCode40→actual caller LEA with no intermediate host CPU writes. Success native ABI changes transfer32 only; all failed native operations preserve4236 arena and installed view. Every warm last metadata32 expected independently from known fullID/slot; all general registers/flags/helper/remaining transfer preserved and watched RAM neighbours mapped separately from code. Synthetic dispatcher stops use Exitv3; child-derived version/union/GateID preserved with only aggregate retired updated, host error prefix retained. Priority cancel/budget-before-lookup deliberately differs from native identity/current/Busy order. Independently instantiated engine keys are unique and full key+u64ID domain is required; executed unit IDs have high0, invalid high limbs do not claim valid high-ID runtime. Host Module/Instance/Table objects remain host-owned; acknowledgement cannot authenticate actual function provenance. Isolated malformed-module/import/bounds/null/wrong-signature/same-signature wrong-membership controls are host/platform evidence, not guest faults or successful installed binding. No product dispatcher/SDK/browser/provider/resident callback/async/race/generation/pin/slotreuse/eviction/performance/playable-game/fullP2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({stats, section_derivation: sections, engine_sha256: provenance.engine_sha256, module_set_sha256: provenance.module_set_sha256, sources: sourcePaths.length, artifacts: Object.keys(artifacts).length, output: outputDir}));
