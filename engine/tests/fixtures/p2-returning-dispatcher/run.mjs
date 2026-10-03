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
const SIZE = 4236, TRANSFER = 140, KEY = 0xb2345678d0000000n;
const names = ['open', 'close', 'arena_ptr', 'map', 'upload', 'compile_resident_with_gates', 'guard_dispatch_entry', 'dispatcher_module', 'guard_resident', 'read32', 'write32', 'store_resident32', 'capture_resident_call', 'complete_resident_call', 'acknowledge_resident_installation', 'find_installed_resident'];
const stats = {engine_instances: 0, compiled_units: 0, dispatcher_metadata32: 0, resident_metadata24: 0, getter_rejections: 0, preopen_getter_calls: 0, ack_success: 0, dispatcher_calls: 0, dispatcher_canonical: 0, dispatcher_rejections: 0, dispatcher_child_entries: 0, dispatcher_lookup_calls: 0, dispatcher_lookup_success: 0, dispatcher_lookup_failure: 0, platform_traps: 0, zero_progress_stops: 0, full_arena_checks: 0, host_reads: 0, host_writes: 0, guest_store32: 0, resident_captures: 0, resident_completions: 0, closed_engines: 0};
const artifacts = {'wrong.wasm': hash(readFileSync(join(outputDir, 'wrong.wasm')))}, authored = {}, units = [], metadata = [], dispatcherMetadata = [], bindings = [], sections = [], algorithm = [], mismatch = [];
function refresh(e) { if (e.buffer !== e.memory.buffer) { e.buffer = e.memory.buffer; e.bytes = new Uint8Array(e.buffer); e.view = new DataView(e.buffer); } return e; }
function arena(e) { return refresh(e).bytes.slice(e.base, e.base + SIZE); }
function header(b, v, p, magic, size, version = 1) { b.set(Buffer.from(magic), p); v.setUint16(p + 4, version, true); v.setUint16(p + 6, 1, true); v.setUint32(p + 8, size, true); v.setUint32(p + 12, 0, true); }
function exit(reason, retired, version = 3, detail = 0, address = 0, access = 0, length = 0) { const b = new Uint8Array(40), v = new DataView(b.buffer); header(b, v, 0, 'R3EX', 40, version); [reason, retired, detail, address, access, length].forEach((x, i) => v.setUint32(16 + i * 4, x, true)); return b; }
function helper(value = 0, fault) { const b = new Uint8Array(40), v = new DataView(b.buffer); header(b, v, 0, 'R3MH', 40); (fault ? [1, 0, 1, fault, 1, 4] : [0, value, 0, 0, 0, 0]).forEach((x, i) => v.setUint32(16 + i * 4, x, true)); return b; }
function copy(s) { return {...s, registers: [...s.registers]}; }
function state(eax = 0xfffffffd, ecx = 5, flags = 0x403, pc = 0x1000) { return {registers: [eax, ecx, 0x23456789, 0x3456789a, 0x9000, 0x56789abc, 0x6789abcd, 0x789abcde], eip: pc, eflags: flags}; }
function stateBytes(s) { const b = new Uint8Array(56), v = new DataView(b.buffer); header(b, v, 0, 'R3ST', 56); s.registers.forEach((x, i) => v.setUint32(16 + i * 4, x, true)); v.setUint32(48, s.eip, true); v.setUint32(52, s.eflags, true); return b; }
function reset(e, s, cancelValue = 0) { refresh(e).bytes.set(stateBytes(s), e.base); e.bytes.fill(0xa5, e.base + 56, e.base + 96); e.view.setUint32(e.base + 96, cancelValue, true); e.bytes.fill(0x5a, e.base + 100, e.base + 140); }
function cancel(e, value) { refresh(e).view.setUint32(e.base + 96, value, true); }
function check(e, wanted, label) { assert.deepEqual(arena(e), wanted, `${label}: full4236 arena`); stats.full_arena_checks++; }
function unchanged(e, action, status, label) { const before = arena(e); assert.equal(action(), status, label); check(e, before, label); }
function installation(u, slot) { const b = new Uint8Array(32), v = new DataView(b.buffer); header(b, v, 0, 'R3IN', 32); v.setUint32(16, u.low, true); v.setUint32(20, u.high, true); v.setUint32(24, slot, true); return b; }
function binding(e, u, slot, label, relation = true) { assert.equal(e.table.get(slot) === u.run, relation, label); (relation ? bindings : mismatch).push({label, key: e.key.toString(), id: u.id.toString(), slot, pointer: u.pointer, length: u.length, sha256: hash(u.bytes), table_reference_equals_run: relation}); }
function instantiateDispatcher(e, module) { const instance = new WebAssembly.Instance(module, {env: {memory: e.memory, table: e.table}, ring3: {guard_dispatch_entry: e.api.guard_dispatch_entry, find_installed_resident: e.api.find_installed_resident}}); assert.equal(instance.exports.run.length, 4); return instance; }
function dispatcherGetter(e, label) {
  const before = arena(e); assert.equal(e.api.dispatcher_module(e.low, e.high), 0); const observed = arena(e), v = new DataView(observed.buffer), pointer = v.getUint32(TRANSFER + 24, true), length = v.getUint32(TRANSFER + 28, true);
  assert.ok(pointer > 0 && length > 8 && pointer + length <= refresh(e).bytes.length); const bytes = e.bytes.slice(pointer, pointer + length), expected = before.slice(), record = new Uint8Array(32), rv = new DataView(record.buffer);
  header(record, rv, 0, 'R3DP', 32); [e.low, e.high, pointer, length].forEach((value, i) => rv.setUint32(16 + i * 4, value, true)); expected.set(record, TRANSFER); check(e, expected, label);
  if (e.dispatcherBytes) { assert.equal(pointer, e.dispatcherPointer); assert.deepEqual(bytes, e.dispatcherBytes); assert.equal(length, e.dispatcherBytes.length); }
  else { const module = new WebAssembly.Module(bytes); assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'}, {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'}, {module: 'ring3', name: 'find_installed_resident', kind: 'function'}]); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]); e.dispatcherModule = module; e.dispatcher = instantiateDispatcher(e, module); e.dispatcherBytes = bytes; e.dispatcherPointer = pointer; const file = `dispatcher-${e.ordinal}.wasm`; writeFileSync(join(outputDir, file), bytes); artifacts[file] = hash(bytes); }
  dispatcherMetadata.push({label, key: e.key.toString(), pointer, length, sha256: hash(bytes), record_hex: Buffer.from(record).toString('hex')}); stats.dispatcher_metadata32++;
}
function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {}), api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]])); for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const ordinal = ++stats.engine_instances, key = KEY + BigInt(ordinal), e = refresh({instance, api, ordinal, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n), memory: instance.exports.memory, nextId: 0, installed: new Map()});
  assert.equal(api.dispatcher_module(0, 0), 5); stats.getter_rejections++; stats.preopen_getter_calls++; assert.equal(api.open(16, e.low, e.high), 0); e.base = api.arena_ptr() >>> 0; assert.ok(e.base > 0 && e.base + SIZE <= refresh(e).bytes.length); e.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
  for (const [name, length] of [['dispatcher_module', 2], ['guard_dispatch_entry', 5], ['find_installed_resident', 3], ['guard_resident', 7], ['store_resident32', 6], ['capture_resident_call', 6], ['complete_resident_call', 6]]) assert.equal(api[name].length, length);
  dispatcherGetter(e, 'empty open process exports product module'); return e;
}
function upload(e, pc, bytes, label) { refresh(e).bytes.set(bytes, e.base + TRANSFER); assert.equal(e.api.upload(pc, bytes.length), 0); const file = `engine-${e.ordinal}-${label}.x86`; writeFileSync(join(outputDir, file), bytes); artifacts[file] = hash(bytes); authored[file] = Buffer.from(bytes).toString('hex'); }
function compile(e, blocks, gates, label, helpers = []) {
  refresh(e); [...blocks, ...gates].forEach(([pc, value], i) => { e.view.setUint32(e.base + TRANSFER + i * 8, pc, true); e.view.setUint32(e.base + TRANSFER + i * 8 + 4, value, true); });
  const before = arena(e); assert.equal(e.api.compile_resident_with_gates(blocks.length, gates.length), 0); const observed = arena(e), v = new DataView(observed.buffer), low = v.getUint32(TRANSFER + 8, true), high = v.getUint32(TRANSFER + 12, true), pointer = v.getUint32(TRANSFER + 16, true), length = v.getUint32(TRANSFER + 20, true), id = BigInt(low) | BigInt(high) << 32n;
  assert.equal(id, BigInt(++e.nextId)); assert.ok(pointer > 0 && length > 8 && pointer + length <= refresh(e).bytes.length); const bytes = e.bytes.slice(pointer, pointer + length), expected = before.slice(), ev = new DataView(expected.buffer); [1, 24, low, high, pointer, length].forEach((x, i) => ev.setUint32(TRANSFER + i * 4, x, true)); check(e, expected, label);
  const module = new WebAssembly.Module(bytes); assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard_resident', kind: 'function'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))]); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3: Object.fromEntries(['guard_resident', ...helpers].map(name => [name, e.api[name]]))}), file = `unit-${++stats.compiled_units}.wasm`; assert.equal(instance.exports.run.length, 4); writeFileSync(join(outputDir, file), bytes); artifacts[file] = hash(bytes);
  const u = {id, low, high, pointer, length, bytes, module, instance, run: instance.exports.run, file, key: e.key.toString(), blocks, gates, label}; units.push(u); metadata.push({key: u.key, id: id.toString(), pointer, length, sha256: hash(bytes)}); stats.resident_metadata24++; return u;
}
function ack(e, u, slot, relation = true) { const before = arena(e); assert.equal(e.api.acknowledge_resident_installation(e.low, e.high, u.low, u.high, slot), 0); const wanted = before.slice(); wanted.set(installation(u, slot), TRANSFER); check(e, wanted, 'cold acknowledgement changes only transfer32'); binding(e, u, slot, 'acknowledged reference relation', relation); e.installed.set(u.id, slot); stats.ack_success++; }
function copied(e, u) { return new WebAssembly.Instance(new WebAssembly.Module(u.bytes.slice()), {env: {memory: e.memory}, ring3: {guard_resident: e.api.guard_resident, store_resident32: e.api.store_resident32, read32: e.api.read32}}).exports.run; }
function dispatch(e, budget, expected, expectedExit, plan, status = 0, expectedHelper, options = {}) {
  const before = arena(e), active = options.dispatcher ?? e.dispatcher; assert.equal(active.exports.run(options.state ?? e.base, options.exit ?? e.base + 56, budget, options.cancel ?? e.base + 96), status); const wanted = before.slice(); if (expected) wanted.set(stateBytes(expected), 0); if (expectedExit) wanted.set(expectedExit, 56); if (expectedHelper) wanted.set(expectedHelper, 100);
  if (plan.last) { wanted.set(installation(plan.last, plan.slot), TRANSFER); binding(e, plan.last, plan.slot, 'last successful product lookup', plan.relation ?? true); } check(e, wanted, 'product returning dispatcher'); stats.dispatcher_calls++; stats[status ? 'dispatcher_rejections' : 'dispatcher_canonical']++; stats.dispatcher_child_entries += plan.children; stats.dispatcher_lookup_calls += plan.lookups; stats.dispatcher_lookup_success += plan.successes; stats.dispatcher_lookup_failure += plan.lookups - plan.successes;
}
const emptyPlan = {children: 0, lookups: 0, successes: 0};
function hostWord(e, address, value, read = false) { const before = arena(e); assert.equal(read ? e.api.read32(address) : e.api.write32(address, value), 0); const wanted = before.slice(); wanted.set(helper(read ? value : 0), 100); check(e, wanted, read ? 'bounded guest RAM observation' : 'host guest RAM setup'); stats[read ? 'host_reads' : 'host_writes']++; }
function observe(e, words) { for (const [address, value] of words) hostWord(e, address, value, true); }
function close(e) { unchanged(e, () => e.api.close(), 0, 'close keeps arena'); stats.closed_engines++; unchanged(e, () => e.api.dispatcher_module(e.low ^ 1, e.high ^ 1), 5, 'closed wins before wrong getter key'); stats.getter_rejections++; }
function section(name, before) { sections.push({name, stats: Object.fromEntries(Object.entries(stats).map(([key, value]) => [key, value - before[key]]))}); }
function flags(op, operand, oldFlags) {
  const a = BigInt(operand), delta = op === 'inc' ? 1n : -1n, result = Number(BigInt.asUintN(32, a + delta)); let f = (oldFlags & 0x401) | 2, parity = 0;
  for (let byte = result & 255; byte; byte >>>= 1) parity += byte & 1; if (parity % 2 === 0) f |= 4; if (!result) f |= 0x40; if (result >= 0x80000000) f |= 0x80;
  if (op === 'inc' ? (operand & 15) === 15 : (operand & 15) === 0) f |= 0x10; const signed = BigInt.asIntN(32, a) + delta; if (signed < -2147483648n || signed > 2147483647n) f |= 0x800; return f;
}
for (const row of [['inc', 0xffffffff, 3, 0x57], ['inc', 0x7fffffff, 2, 0x896], ['dec', 1, 0x403, 0x447], ['dec', 0, 2, 0x96], ['dec', 0x80000000, 2, 0x816]]) assert.equal(flags(...row.slice(0, 3)), row[3]);
function trace(start, steps) { const n = start.registers[1], cycles = Math.floor(steps / 4), phase = steps % 4, s = copy(start); assert.ok(steps >= 0 && steps <= 4 * n); s.registers[0] = Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(cycles + (phase ? 1 : 0)))); s.registers[1] = n - cycles - (phase === 3 ? 1 : 0); s.eip = steps === 4 * n ? 0x2007 : [0x1000, 0x1001, 0x2000, 0x2001][phase]; if (steps) { const inc = phase === 1 || phase === 2, operand = inc ? Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(cycles))) : n - cycles + (phase === 0 ? 1 : 0); s.eflags = flags(inc ? 'inc' : 'dec', operand, start.eflags); } return s; }
function slice(e, start, from, budget, a, b) {
  const remaining = start.registers[1] * 4 - from, to = Math.min(start.registers[1] * 4, from + budget), retired = to - from, children = retired ? Math.ceil(to / 2) - Math.floor(from / 2) : 0, last = children ? (Math.floor((to - 1) / 2) % 2 ? b : a) : undefined, miss = budget > remaining;
  dispatch(e, budget, trace(start, to), exit(budget <= remaining ? 1 : 3, retired, budget === 0 || miss ? 3 : 1), {children, lookups: children + Number(miss), successes: children, last, slot: last ? e.installed.get(last.id) : undefined}); return to;
}

let before = {...stats};
const main = fresh();
for (const options of [{state: main.base + 1}, {exit: main.base}, {cancel: 0xffffffff}]) { reset(main, state(), 1); dispatch(main, 0, undefined, undefined, emptyPlan, 1, undefined, options); }
for (const [offset, value] of [[12, 1], [52, 0]]) { reset(main, state(), 1); main.view.setUint32(main.base + offset, value, true); dispatch(main, 0, undefined, undefined, emptyPlan, 2); }
reset(main, state()); unchanged(main, () => main.api.dispatcher_module(main.low, main.high ^ 1), 3, 'full high key required before metadata publication'); stats.getter_rejections++;
assert.equal(main.api.map(0x1000, 2, 7), 0); assert.equal(main.api.map(0x7000, 3, 3), 0);
upload(main, 0x1000, Buffer.from('40e9fa0f0000', 'hex'), 'a'); upload(main, 0x2000, Buffer.from('490f85f9efffff', 'hex'), 'b'); upload(main, 0x2100, Buffer.from('8b03', 'hex'), 'read');
const a = compile(main, [[0x1000, 6]], [], 'ordinary A'); main.table.set(0, a.run); ack(main, a, 0);
const watched = [[0x7ffc, 0x11223344], [0x8000, 0x89abcdef], [0x8004, 0x76543210], [0x8008, 0x55667788]]; for (const [address, value] of watched) hostWord(main, address, value);
const coldStart = state(); reset(main, coldStart);
dispatch(main, 0xffffffff, trace(coldStart, 2), exit(3, 2), {children: 1, lookups: 2, successes: 1, last: a, slot: 0});
const b = compile(main, [[0x2000, 7]], [], 'B prepared only after product cold miss'); dispatcherGetter(main, 'resident preparation does not replace dispatcher bytes'); main.table.set(1, b.run); ack(main, b, 1); slice(main, coldStart, 2, 0xffffffff, a, b); observe(main, watched);
const stable = [main.memory, main.table, main.dispatcher, main.dispatcherModule, a.module, a.instance, b.module, b.instance, a.run, b.run];
for (const [n, eax] of [[1, 0xffffffff], [2, 0x7fffffff], [5, 0xfffffffd]]) {
  const start = state(eax, n);
  for (const budget of [0, 1, 3, n * 4, 0xffffffff]) { reset(main, start); slice(main, start, 0, budget, a, b); [main.memory, main.table, main.dispatcher, main.dispatcherModule, a.module, a.instance, b.module, b.instance, a.run, b.run].forEach((reference, i) => assert.equal(reference, stable[i])); assert.equal(main.table.get(0), a.run); assert.equal(main.table.get(1), b.run); }
  const final = trace(start, 4 * n); assert.equal(final.registers[0], Number(BigInt.asUintN(32, BigInt(eax) + BigInt(n)))); assert.equal(final.registers[1], 0); assert.equal(final.eflags, 0x447); algorithm.push({n, input_eax: eax, final_eax: final.registers[0], final_ecx: 0, flags: 0x447, retired: 4 * n, child_entries: 2 * n});
}
const splitStart = state(0x7ffffffe, 2); reset(main, splitStart); let position = 0;
for (const [index, budget] of [1, 1, 1, 0, 2, 3, 1].entries()) { position = slice(main, splitStart, position, budget, a, b); if (index === 2) { cancel(main, 0x80000000); dispatch(main, 0, trace(splitStart, position), exit(2, 0), emptyPlan); cancel(main, 0); } }
assert.equal(position, 8);
const unknown = state(7, 1, 0x403, 0xffffffff); reset(main, unknown); dispatch(main, 9, unknown, exit(3, 0), {children: 0, lookups: 1, successes: 0});
const reader = compile(main, [[0x2100, 2]], [], 'read32 Exitv2 unit', ['read32']); main.table.set(3, reader.run); ack(main, reader, 3);
const faultStart = state(0x12345678, 1, 0xcd7, 0x2100); faultStart.registers[3] = 0x6000; reset(main, faultStart);
dispatch(main, 9, faultStart, exit(5, 0, 2, 1, 0x6000, 1, 4), {children: 1, lookups: 1, successes: 1, last: reader, slot: 3}, 0, helper(0, 0x6000));
const readStart = copy(faultStart); readStart.registers[3] = 0x8000; reset(main, readStart); const readDone = copy(readStart); readDone.registers[0] = 0x89abcdef; readDone.eip = 0x2102;
dispatch(main, 1, readDone, exit(1, 1, 2), {children: 1, lookups: 1, successes: 1, last: reader, slot: 3}, 0, helper(0x89abcdef)); observe(main, watched);
hostWord(main, 0x2000, 0xf9850f49); dispatcherGetter(main, 'SMC does not replace dispatcher module');
const staleStart = state(9, 1); reset(main, staleStart); dispatch(main, 9, trace(staleStart, 2), exit(3, 2, 1), {children: 1, lookups: 2, successes: 1, last: a, slot: 0}, 4);
const freshB = compile(main, [[0x2000, 7]], [], 'fresh B at same PC'); assert.notEqual(freshB.id, b.id); dispatcherGetter(main, 'fresh unit retains dispatcher pointer and bytes'); main.table.set(2, freshB.run); ack(main, freshB, 2); assert.equal(main.table.get(1), b.run); slice(main, staleStart, 2, 9, a, freshB); observe(main, watched);
close(main); dispatch(main, 0, undefined, undefined, emptyPlan, 5, undefined, {state: 0xffffffff, exit: 0xffffffff, cancel: 0xffffffff});
section('guard-first, true cold preparation, warm arithmetic, Exitv2 and SMC', before);

before = {...stats};
const gate = fresh(), gateId = 0x80000011;
assert.equal(gate.api.map(0x3000, 2, 7), 0); assert.equal(gate.api.map(0x7000, 3, 3), 0);
upload(gate, 0x4000, Buffer.from('0f0b', 'hex'), 'gate'); upload(gate, 0x3000, Buffer.from('e8fb0f00008d4003', 'hex'), 'caller'); upload(gate, 0x3100, Buffer.from('8d4003e9f8feffff', 'hex'), 'prefix');
const gateB = compile(gate, [[0x4000, 2]], [[0x4000, gateId]], 'registered Gate B'), callerA = compile(gate, [[0x3000, 5], [0x3005, 3]], [], 'CALL and caller return', ['store_resident32']), prefixA = compile(gate, [[0x3100, 8]], [], 'LEA and JMP prefix');
gate.table.set(1, gateB.run); gate.table.set(0, callerA.run); gate.table.set(2, prefixA.run); ack(gate, gateB, 1); ack(gate, callerA, 0); ack(gate, prefixA, 2);
const stackWords = [[0x8ff8, 0x11223344], [0x8ffc, 0x89abcdef], [0x9000, 0x55667788]]; for (const [address, value] of stackWords) hostWord(gate, address, value);
const callStart = state(0x89abcdef, 3, 0xcd7, 0x3000), stopped = copy(callStart); stopped.registers[4] = 0x8ffc; stopped.eip = 0x4000;
const foreignChild = copied(gate, a), prefixStart = copy(callStart), prefixStopped = copy(stopped); prefixStart.eip = 0x3100; prefixStopped.registers[0] = 0x89abcdf2; gate.table.set(1, foreignChild); assert.equal(gate.table.get(1), foreignChild); reset(gate, prefixStart);
dispatch(gate, 9, prefixStopped, exit(3, 3, 2), {children: 3, lookups: 3, successes: 3, last: gateB, slot: 1, relation: false}, 3, helper()); stats.guest_store32++; stackWords[1][1] = 0x3005; observe(gate, stackWords);
const prefixFailure = {key: gate.key.toString(), foreign_child_key: main.key.toString(), foreign_child_id: a.id.toString(), status: 3, committed_retired: 3, last_successful_child_retired: 1, state: prefixStopped, exit_hex: Buffer.from(exit(3, 3, 2)).toString('hex'), helper_hex: Buffer.from(helper()).toString('hex'), committed_return_word: 0x3005, last_lookup_unit_id: gateB.id.toString(), table_reference_equals_foreign_run: gate.table.get(1) === foreignChild};
gate.table.set(1, gateB.run); reset(gate, callStart);
dispatch(gate, 9, stopped, exit(8, 1, 3, gateId), {children: 2, lookups: 2, successes: 2, last: gateB, slot: 1}, 0, helper()); stats.guest_store32++; observe(gate, stackWords);
const expectedCall = new Uint8Array(112), cv = new DataView(expectedCall.buffer); header(expectedCall, cv, 0, 'R3CF', 112); [1, gateId, 1, 0, 0x4000, 0x8ffc, 0x3005, 0].forEach((x, i) => cv.setUint32(16 + i * 4, x, true));
const beforeCapture = arena(gate), wantedCapture = beforeCapture.slice(); wantedCapture.set(expectedCall, TRANSFER); assert.equal(gate.api.capture_resident_call(gate.low, gate.high, gateB.low, gateB.high, 1, 0), 0); check(gate, wantedCapture, 'aggregate Gate capture publishes112'); stats.resident_captures++;
const savedCall = Buffer.from(arena(gate).slice(TRANSFER, TRANSFER + 112)), scalar = Number(BigInt.asUintN(32, BigInt(gateId) * 3n + 7n)); assert.equal(scalar, 0x8000003a); assert.deepEqual(savedCall, Buffer.from(expectedCall));
dispatcherGetter(gate, 'pending cold getter retains frozen private call frame'); cancel(gate, 1); dispatch(gate, 0, undefined, undefined, emptyPlan, 12, undefined, {state: 0xffffffff, exit: 0xffffffff, cancel: 0xffffffff}); cancel(gate, 0); assert.deepEqual(savedCall, Buffer.from(expectedCall));
const completed = copy(stopped); completed.registers[0] = scalar; completed.registers[4] = 0x9000; completed.eip = 0x3005; const beforeCompletion = arena(gate), wantedCompletion = beforeCompletion.slice(); wantedCompletion.set(stateBytes(completed), 0); wantedCompletion.set(exit(3, 0), 56);
assert.equal(gate.api.complete_resident_call(gate.low, gate.high, gateB.low, gateB.high, 1, scalar), 0); check(gate, wantedCompletion, 'completion uses frozen frame after module metadata replacement'); stats.resident_completions++;
const returned = copy(completed); returned.registers[0] = 0x8000003d; returned.eip = 0x3008; dispatch(gate, 9, returned, exit(3, 1), {children: 1, lookups: 2, successes: 1, last: callerA, slot: 0}); observe(gate, stackWords);
const callFlow = {key: gate.key.toString(), gate_unit_id: gateB.id.toString(), caller_unit_id: callerA.id.toString(), aggregate_gate_exit_hex: Buffer.from(exit(8, 1, 3, gateId)).toString('hex'), call_record_hex: savedCall.toString('hex'), scalar_result: scalar, completion_state: completed, return_state: returned, guest_store32_calls: 2, no_intermediate_host_cpu_writes_between_capture_and_return: true};
dispatcherGetter(gate, 'completion and return preserve dispatcher lifetime'); close(gate);
section('CALL committed prefix error, Gate union and pending frozen completion', before);

before = {...stats};
const control = fresh(); assert.equal(control.api.map(0x5000, 1, 7), 0);
upload(control, 0x5000, Buffer.from('90', 'hex'), 'selected'); upload(control, 0x5100, Buffer.from('90', 'hex'), 'other');
const selected = compile(control, [[0x5000, 1]], [], 'selected control'), other = compile(control, [[0x5100, 1]], [], 'same-signature other membership');
control.table.set(0, other.run); assert.equal(control.table.get(0), other.run); ack(control, selected, 0, false); control.table.set(1, other.run); ack(control, other, 1);
const mismatchStart = state(5, 1, 0x403, 0x5000); reset(control, mismatchStart);
dispatch(control, 9, mismatchStart, exit(3, 0, 1), {children: 1, lookups: 1, successes: 1, last: selected, slot: 0, relation: false}); stats.zero_progress_stops++;
control.table.set(0, null); reset(control, mismatchStart); dispatch(control, 9, mismatchStart, exit(3, 0), {children: 0, lookups: 1, successes: 1, last: selected, slot: 0, relation: false});
const wrongModule = new WebAssembly.Module(readFileSync(join(outputDir, 'wrong.wasm'))); assert.deepEqual(WebAssembly.Module.imports(wrongModule), []); assert.deepEqual(WebAssembly.Module.exports(wrongModule), [{name: 'wrong', kind: 'function'}]); const wrong = new WebAssembly.Instance(wrongModule, {}).exports.wrong; assert.equal(wrong.length, 0); control.table.set(0, wrong); reset(control, mismatchStart);
const beforeTrap = arena(control), wantedTrap = beforeTrap.slice(); wantedTrap.set(installation(selected, 0), TRANSFER); assert.throws(() => control.dispatcher.exports.run(control.base, control.base + 56, 9, control.base + 96), WebAssembly.RuntimeError); check(control, wantedTrap, 'signature platform trap keeps prior lookup32'); binding(control, selected, 0, 'isolated wrong-signature reference', false); stats.platform_traps++; stats.dispatcher_calls++; stats.dispatcher_lookup_calls++; stats.dispatcher_lookup_success++;
const foreignDispatcher = instantiateDispatcher(control, new WebAssembly.Module(main.dispatcherBytes.slice())); reset(control, mismatchStart, 1); control.view.setUint32(control.base + 12, 1, true); dispatch(control, 0, undefined, undefined, emptyPlan, 3, undefined, {dispatcher: foreignDispatcher, state: 0xffffffff, exit: 0xffffffff, cancel: 0xffffffff});
unchanged(control, () => control.api.dispatcher_module(control.low, control.high ^ 1), 3, 'getter full-key error ignores malformed CPU and cancel'); stats.getter_rejections++;
const sameSignatureControl = {key: control.key.toString(), selected_id: selected.id.toString(), selected_sha256: hash(selected.bytes), selected_slot: 0, target_id: other.id.toString(), target_sha256: hash(other.bytes), target_reference_equals_other_run: true, child_reason: 3, child_version: 1, child_retired: 0, selected_pc: 0x5000, target_pc: 0x5100, table_authentication_claim: false};
close(control); dispatch(control, 0, undefined, undefined, emptyPlan, 5, undefined, {dispatcher: foreignDispatcher, state: 0xffffffff, exit: 0xffffffff, cancel: 0xffffffff});
section('isolated table zero-progress null signature and full-key closed priority', before);

const expectedStats = {engine_instances: 3, compiled_units: 9, dispatcher_metadata32: 8, resident_metadata24: 9, getter_rejections: 8, preopen_getter_calls: 3, ack_success: 9, dispatcher_calls: 45, dispatcher_canonical: 33, dispatcher_rejections: 11, dispatcher_child_entries: 69, dispatcher_lookup_calls: 81, dispatcher_lookup_success: 71, dispatcher_lookup_failure: 10, platform_traps: 1, zero_progress_stops: 1, full_arena_checks: 110, host_reads: 21, host_writes: 8, guest_store32: 2, resident_captures: 1, resident_completions: 1, closed_engines: 3};
const expectedSections = [
  {engine_instances: 1, compiled_units: 4, dispatcher_metadata32: 4, resident_metadata24: 4, getter_rejections: 3, preopen_getter_calls: 1, ack_success: 4, dispatcher_calls: 36, dispatcher_canonical: 29, dispatcher_rejections: 7, dispatcher_child_entries: 62, dispatcher_lookup_calls: 71, dispatcher_lookup_success: 62, dispatcher_lookup_failure: 9, platform_traps: 0, zero_progress_stops: 0, full_arena_checks: 68, host_reads: 12, host_writes: 5, guest_store32: 0, resident_captures: 0, resident_completions: 0, closed_engines: 1},
  {engine_instances: 1, compiled_units: 3, dispatcher_metadata32: 3, resident_metadata24: 3, getter_rejections: 2, preopen_getter_calls: 1, ack_success: 3, dispatcher_calls: 4, dispatcher_canonical: 2, dispatcher_rejections: 2, dispatcher_child_entries: 6, dispatcher_lookup_calls: 7, dispatcher_lookup_success: 6, dispatcher_lookup_failure: 1, platform_traps: 0, zero_progress_stops: 0, full_arena_checks: 29, host_reads: 9, host_writes: 3, guest_store32: 2, resident_captures: 1, resident_completions: 1, closed_engines: 1},
  {engine_instances: 1, compiled_units: 2, dispatcher_metadata32: 1, resident_metadata24: 2, getter_rejections: 3, preopen_getter_calls: 1, ack_success: 2, dispatcher_calls: 5, dispatcher_canonical: 2, dispatcher_rejections: 2, dispatcher_child_entries: 1, dispatcher_lookup_calls: 3, dispatcher_lookup_success: 3, dispatcher_lookup_failure: 0, platform_traps: 1, zero_progress_stops: 1, full_arena_checks: 13, host_reads: 0, host_writes: 0, guest_store32: 0, resident_captures: 0, resident_completions: 0, closed_engines: 1},
];
assert.deepEqual(stats, expectedStats, 'all authored-path totals derived before runtime'); assert.deepEqual(sections.map(row => row.stats), expectedSections, 'complete independent per-context count rows');
assert.equal(dispatcherMetadata.length, 8); assert.equal(metadata.length, 9); assert.equal(bindings.length, 33); assert.equal(mismatch.length, 5); assert.equal(Object.keys(artifacts).length, 21); assert.equal(Object.keys(authored).length, 8);
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', 'engine/tests/process_returning_dispatcher.rs', 'engine/tests/process_returning_dispatcher_wasm.rs', 'engine/tests/fixtures/p2-returning-dispatcher/run.mjs', 'engine/tests/process_dispatch_entry.rs', 'engine/tests/process_dispatch_entry_wasm.rs', 'engine/tests/fixtures/p2-dispatch-entry/run.mjs', 'engine/tests/process_resident_installation.rs', 'engine/tests/process_resident_installation_wasm.rs', 'engine/tests/fixtures/p2-resident-installation/run.mjs'];
assert.equal(new Set(sourcePaths).size, sourcePaths.length); assert.equal(sourcePaths.length, productionPaths.length + 12);
const provenance = {
  stats, section_derivation: sections, actual_entry_guard_calls_from_authored_paths: stats.dispatcher_calls, actual_resident_guard_entries_from_authored_paths: stats.dispatcher_child_entries, bound_store_identity_checks: stats.guest_store32,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  authored, algorithm, prefix_failure: prefixFailure, call_flow: callFlow, same_signature_control: sameSignatureControl, dispatcher_metadata32_observations: dispatcherMetadata, resident_metadata24_observations: metadata, positive_installation32_observations: bindings, isolated_host_mismatch_observations: mismatch,
  unit_bindings: units.map(u => ({key: u.key, id: u.id.toString(), pointer: u.pointer, length: u.length, sha256: hash(u.bytes), file: u.file, blocks: u.blocks, gates: u.gates})),
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_returning_dispatcher_wasm', '--', '--nocapture']},
  parity: {entry: {wasm_modules: 5, full_x86: 3, statistic_fields: 30, complete_rows: 5}, installation: {wasm_modules: 11, full_x86: 6, statistic_fields: 35, complete_rows: 3}, note: 'root separately runs unchanged consumers once and compares their full artifacts and rows to328; this portable target requires no ignored historical output'},
  counts: 'all fields/full rows are static authored-path derivations, not product instrumentation. One signature trap enters entry guard/finder but no child guard. Positive table observations assert exact known instance.exports.run references; isolated false relations do not authenticate table provenance. Getter pointer/length are runtime allocator coordinates; record header/full key and immutable bytes are independently checked.',
  reused: 'unchanged unit emitter/registry/guard7/read32/store6/near-control/Gate/capture112/completion/State/helper/Exit contracts are prior-proof reuse after full bytes. New actual plus unchanged entry/installation consumers only; no callback corpus, million experiment, browser, benchmark or full CI replay.',
  claim: 'synchronous product returning Wasm dispatcher with guard5 before any memory/finder/table work, full-key process-owned immutable R3DP32 module getter, direct engine finder3 and returning run4 table children. True cold B is not prepared until A has retired2 and stopped at B; host preparation/ack then resumes without CPU patch. Warm4n/full flags/split/MAX/cancel/v1/v2/v3 stop unions, prefix3 versus last-child1 host error with RAM/helper/metadata preservation, frozen pending capture/completion after module metadata reuse, SMC fresh slot and closed retained instance are observed. Every full4236 arena expectation permits only explicit CPU/Exit/helper/known metadata records; watched RAM comprises12main and9stack read observations, not all guest memory. Table/memory object provenance is trusted. Null/signature/foreign-key/same-signature wrong-membership controls are isolated violations; fixed8 slot-bounds branch is source/invariant-only, no actual bounds claim. No async/race/browser/SDK/provider/resident callback migration/slotreuse/eviction/pins/performance/fullP2-V0/gameplay claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({stats, section_derivation: sections, engine_sha256: provenance.engine_sha256, module_set_sha256: provenance.module_set_sha256, sources: sourcePaths.length, artifacts: Object.keys(artifacts).length, output: outputDir}));
