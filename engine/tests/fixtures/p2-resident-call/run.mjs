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
const SIZE = 4236, TRANSFER = 140, STACK = 0xa100, marker = Buffer.from('0f0b', 'hex');
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'compile_with_gates', 'compile_resident', 'compile_resident_with_gates', 'resident_module', 'find_resident', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident', 'read32', 'write32', 'store_resident32', 'capture_call', 'complete_call', 'capture_resident_call', 'complete_resident_call', 'abandon_call', 'begin_callback'];
const stats = {engine_instances: 0, compiled_units: 0, legacy_modules: 0, canonical_runs: 0, rejected_runs: 0, full_arena_checks: 0, metadata_checks: 0, find_success: 0, find_failure: 0, initial_calls: 0, resumes: 0, resident_captures: 0, legacy_captures: 0, resident_completions: 0, legacy_completions: 0, abandonments: 0, capture_failures: 0, completion_failures: 0, rejected_operations: 0, guard_success: 0, guard_failure: 0, host_reads: 0, host_writes: 0, guest_store32: 0, bound_store_sites: 0, convention_cases: 0, capture_read_faults: 0, guest_store_faults: 0, repairs: 0, cancellation_cases: 0, currency_cases: 0};
const artifacts = {}, authored = {}, units = [], metadata = [], captures = [], completions = [], sections = [], legacyControls = [];
function refresh(e) { if (e.buffer !== e.memory.buffer) { e.buffer = e.memory.buffer; e.bytes = new Uint8Array(e.buffer); e.view = new DataView(e.buffer); } return e; }
function arena(e) { return refresh(e).bytes.slice(e.base, e.base + SIZE); }
function header(bytes, view, pointer, magic, size, version = 1) { bytes.set(Buffer.from(magic), pointer); view.setUint16(pointer + 4, version, true); view.setUint16(pointer + 6, 1, true); view.setUint32(pointer + 8, size, true); view.setUint32(pointer + 12, 0, true); }
function exit(reason, retired, detail = 0, address = 0, access = 0, length = 0, version = 3) { const b = new Uint8Array(40), v = new DataView(b.buffer); header(b, v, 0, 'R3EX', 40, version); [reason, retired, detail, address, access, length].forEach((x, i) => v.setUint32(16 + i * 4, x, true)); return b; }
function helper(value = 0, detail = 0, address = 0, access = 0) { const b = new Uint8Array(40), v = new DataView(b.buffer); header(b, v, 0, 'R3MH', 40); [detail ? 1 : 0, detail ? 0 : value, detail, address, access, detail ? 4 : 0].forEach((x, i) => v.setUint32(16 + i * 4, x, true)); return b; }
function copy(s) { return {...s, registers: [...s.registers]}; }
function state(pc = 0x3000, esp = STACK) { return {registers: [0x89abcdef, 0x13579bdf, 0x23456789, 0x3456789a, esp, 0x56789abc, 0x6789abcd, 0x789abcde], eip: pc, eflags: 0xcd7}; }
function stateBytes(s) { const b = new Uint8Array(56), v = new DataView(b.buffer); header(b, v, 0, 'R3ST', 56); s.registers.forEach((x, i) => v.setUint32(16 + i * 4, x, true)); v.setUint32(48, s.eip, true); v.setUint32(52, s.eflags, true); return b; }
function reset(e, s, cancel = 0) { refresh(e).bytes.set(stateBytes(s), e.base); e.bytes.fill(0xa5, e.base + 56, e.base + 96); e.view.setUint32(e.base + 96, cancel, true); e.bytes.fill(0x5a, e.base + 100, e.base + 140); }
function cancelled(e, value) { refresh(e).view.setUint32(e.base + 96, value, true); }
function check(e, before, wanted, label) { assert.deepEqual(arena(e), wanted, `${label}: full4236 arena`); stats.full_arena_checks++; }
function unchanged(e, action, status, label) { const before = arena(e); assert.equal(action(), status, label); check(e, before, before, label); }
function failure(e, action, status, label, kind) { assert.ok(status > 0); unchanged(e, action, status, label); stats.rejected_operations++; if (kind) stats[`${kind}_failures`]++; }
function upload(e, address, bytes) { refresh(e).bytes.set(bytes, e.base + TRANSFER); assert.equal(e.api.upload(address, bytes.length), 0); refresh(e); }
function authoredCode(e, name, bytes) { const file = `engine-${e.ordinal}-${name}.x86`; assert.equal(artifacts[file], undefined); writeFileSync(join(outputDir, file), bytes); authored[file] = Buffer.from(bytes).toString('hex'); artifacts[file] = hash(bytes); }
function descriptors(e, blocks, gates) { refresh(e); [...blocks, ...gates].forEach(([pc, value], i) => { e.view.setUint32(e.base + TRANSFER + i * 8, pc, true); e.view.setUint32(e.base + TRANSFER + i * 8 + 4, value, true); }); }
function word(e, address, value) { assert.equal(e.api.write32(address, value), 0); stats.host_writes++; assert.deepEqual(arena(e).slice(100, 140), helper()); }
function observe(e, address, value) { const before = arena(e); assert.equal(e.api.read32(address), 0); stats.host_reads++; const wanted = before.slice(); wanted.set(helper(value), 100); check(e, before, wanted, 'host RAM observation changes only helper40'); }
function uleb(bytes, c) { let value = 0, shift = 0, byte; do { byte = bytes[c.i++]; assert.ok(byte !== undefined && shift <= 28); value += (byte & 127) * 2 ** shift; shift += 7; } while (byte & 128); return value; }
function sleb(bytes, c) { let value = 0n, shift = 0n, byte; do { byte = bytes[c.i++]; assert.ok(byte !== undefined && shift <= 28n); value |= BigInt(byte & 127) << shift; shift += 7n; } while (byte & 128); if (byte & 64) value -= 1n << shift; return Number(BigInt.asUintN(32, value)); }
function binding(bytes, e, id, returns) {
  const c = {i: 8}, types = [], imports = new Map(); let body;
  function string() { const n = uleb(bytes, c), s = Buffer.from(bytes.subarray(c.i, c.i + n)).toString(); c.i += n; return s; }
  while (c.i < bytes.length) {
    const section = bytes[c.i++], n = uleb(bytes, c), end = c.i + n;
    if (section === 1) for (let i = 0, count = uleb(bytes, c); i < count; i++) { assert.equal(bytes[c.i++], 0x60); const argc = uleb(bytes, c), args = [...bytes.subarray(c.i, c.i + argc)]; c.i += argc; const retc = uleb(bytes, c), result = [...bytes.subarray(c.i, c.i + retc)]; c.i += retc; types.push({args, result}); }
    else if (section === 2) for (let i = 0, count = uleb(bytes, c); i < count; i++) { string(); const name = string(), kind = bytes[c.i++]; if (kind === 0) imports.set(name, uleb(bytes, c)); else { assert.equal(kind, 2); const flags = uleb(bytes, c); uleb(bytes, c); if (flags & 1) uleb(bytes, c); } }
    else if (section === 10) { assert.equal(uleb(bytes, c), 1); const length = uleb(bytes, c); body = bytes.subarray(c.i, c.i + length); }
    c.i = end;
  }
  assert.ok(body); assert.deepEqual(types[0], {args: Array(4).fill(0x7f), result: [0x7f]}); assert.deepEqual(types[imports.get('guard_resident')], {args: Array(7).fill(0x7f), result: [0x7f]});
  const cursor = {i: 0}, groups = uleb(body, cursor); for (let i = 0; i < groups; i++) { uleb(body, cursor); cursor.i++; }
  const limbs = [e.low, e.high, Number(id & 0xffffffffn), Number(id >> 32n)], actual = [];
  for (let i = 0; i < 4; i++) { assert.equal(body[cursor.i++], 0x41); actual.push(sleb(body, cursor)); } assert.deepEqual(actual, limbs); assert.deepEqual([...body.subarray(cursor.i, cursor.i + 8)], [0x20, 0, 0x20, 1, 0x20, 3, 0x10, 0]);
  assert.equal(imports.has('read32'), false);
  if (!returns.length) { assert.equal(imports.has('store_resident32'), false); return; }
  assert.deepEqual(types[imports.get('store_resident32')], {args: Array(6).fill(0x7f), result: [0x7f]}); let count = 0;
  for (let offset = 0; offset < body.length; offset++) {
    if (body[offset] !== 0x41) continue; const p = {i: offset}, values = []; let match = true;
    try { for (let i = 0; i < 4; i++) { if (body[p.i++] !== 0x41) { match = false; break; } values.push(sleb(body, p)); } } catch { match = false; }
    if (!match || !values.every((x, i) => x === limbs[i]) || body[p.i] !== 0x20 || body[p.i + 1] !== 26) continue;
    p.i += 2; assert.equal(body[p.i++], 0x41); assert.equal(sleb(body, p), returns[count++]); assert.equal(body[p.i++], 0x10); assert.equal(uleb(body, p), 1);
  }
  assert.equal(count, returns.length); stats.bound_store_sites += count;
}
function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {}), api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]])); for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const ordinal = ++stats.engine_instances, key = 0x92345678c0000000n + BigInt(ordinal), e = refresh({instance, api, ordinal, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n), memory: instance.exports.memory, token: 0});
  assert.equal(api.open(24, e.low, e.high), 0); e.base = api.arena_ptr() >>> 0; refresh(e); assert.ok(e.base > 0 && e.base + SIZE <= e.bytes.length);
  for (const [name, length] of [['capture_resident_call', 6], ['complete_resident_call', 6], ['capture_call', 5], ['complete_call', 5], ['abandon_call', 3], ['guard_resident', 7], ['store_resident32', 6]]) assert.equal(api[name].length, length);
  assert.equal(api.generation(), 0); assert.equal(api.module_ptr(), 0); assert.equal(api.module_len(), 0); return e;
}
function record(e, before, label) {
  const after = arena(e), v = new DataView(after.buffer), f = Array.from({length: 6}, (_, i) => v.getUint32(TRANSFER + i * 4, true)); assert.equal(f[0], 1); assert.equal(f[1], 24);
  const [,, low, high, pointer, length] = f, id = BigInt(low) | BigInt(high) << 32n; assert.ok(id > 0n && pointer > 0 && pointer + length <= refresh(e).bytes.length);
  const bytes = e.bytes.slice(pointer, pointer + length), wanted = before.slice(); wanted.set(after.slice(TRANSFER, TRANSFER + 24), TRANSFER); check(e, before, wanted, label); assert.ok(WebAssembly.validate(bytes));
  stats.metadata_checks++; metadata.push({label, key: e.key.toString(), id: id.toString(), pointer, length, sha256: hash(bytes)}); return {id, low, high, pointer, length, bytes};
}
function compile(e, blocks, gates, label, returns = []) {
  descriptors(e, blocks, gates); const before = arena(e); assert.equal(e.api.compile_resident_with_gates(blocks.length, gates.length), 0, label); const selected = record(e, before, label); binding(selected.bytes, e, selected.id, returns);
  const module = new WebAssembly.Module(selected.bytes), helpers = returns.length ? ['store_resident32'] : [];
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard_resident', kind: 'function'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))]); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3: {guard_resident: e.api.guard_resident, store_resident32: e.api.store_resident32}}); assert.equal(instance.exports.run.length, 4);
  const filename = `unit-${++stats.compiled_units}.wasm`; writeFileSync(join(outputDir, filename), selected.bytes); artifacts[filename] = hash(selected.bytes);
  const unit = {...selected, module, instance, run: instance.exports.run, key: e.key.toString(), blocks, gates, returns, label, filename}; units.push(unit); return unit;
}
function select(e, pc, unit) { const before = arena(e); assert.equal(e.api.find_resident(pc), 0); const selected = record(e, before, 'finder'); assert.equal(selected.id, unit.id); assert.equal(selected.pointer, unit.pointer); assert.equal(selected.length, unit.length); assert.deepEqual(selected.bytes, unit.bytes); stats.find_success++; }
function getter(e, unit) { const before = arena(e); assert.equal(e.api.resident_module(unit.low, unit.high), 0); const selected = record(e, before, 'module getter while pending'); assert.equal(selected.id, unit.id); assert.equal(selected.pointer, unit.pointer); assert.deepEqual(selected.bytes, unit.bytes); }
function miss(e, pc, status = 17) { failure(e, () => e.api.find_resident(pc), status, 'controlled finder miss'); stats.find_failure++; }
function guard(e, unit, status = 0) { unchanged(e, () => e.api.guard_resident(e.low, e.high, unit.low, unit.high, e.base, e.base + 56, e.base + 96), status, 'direct resident guard'); stats[status ? 'guard_failure' : 'guard_success']++; }
function reject(e, unit, status, label, pointers = [e.base, e.base + 56, e.base + 96]) { unchanged(e, () => unit.run(pointers[0], pointers[1], 3, pointers[2]), status, label); stats.rejected_runs++; }
function copied(e, unit) { const instance = new WebAssembly.Instance(new WebAssembly.Module(unit.bytes.slice()), {env: {memory: e.memory}, ring3: {guard_resident: e.api.guard_resident, store_resident32: e.api.store_resident32}}); return {...unit, run: instance.exports.run}; }
function run(e, unit, budget, expected, expectedExit, label, continuation, expectedHelper, stores = 0) {
  const before = arena(e); assert.equal(unit.run(e.base, e.base + 56, budget, e.base + 96), 0, label); const wanted = before.slice(); wanted.set(stateBytes(expected), 0); wanted.set(expectedExit, 56); if (expectedHelper !== undefined) wanted.set(expectedHelper, 100); check(e, before, wanted, label);
  stats.canonical_runs++; stats[continuation === 'initial' ? 'initial_calls' : 'resumes']++; stats.guest_store32 += stores;
}
function callBytes(pc, target) { const b = Buffer.alloc(11); b[0] = 0xe8; b.writeInt32LE(target - pc - 5, 1); b.set(Buffer.from('8d40078d4007', 'hex'), 5); return b; }
function setup(targets = [[0x4000, 17]], extra = false) {
  const e = fresh(); for (const [address, pages, permissions] of [[0x3000, 3, 7], [0x9000, 3, 3]]) assert.equal(e.api.map(address, pages, permissions), 0);
  for (const [pc] of targets) { upload(e, pc, marker); authoredCode(e, `gate-${pc}`, marker); }
  if (extra) { upload(e, 0x4500, Buffer.from('8d4007', 'hex')); authoredCode(e, 'whole-unit-extra', Buffer.from('8d4007', 'hex')); }
  const gateBlocks = [...targets.map(([pc]) => [pc, 2]), ...(extra ? [[0x4500, 3]] : [])], b = compile(e, gateBlocks, targets, 'gate B compiled first'); assert.equal(b.id, 1n);
  const forms = targets.map(([target, id], i) => ({pc: 0x3000 + i * 0x100, target, id}));
  for (const form of forms) { const bytes = callBytes(form.pc, form.target); upload(e, form.pc, bytes); authoredCode(e, `caller-${form.pc}`, bytes); }
  const a = compile(e, forms.flatMap(f => [[f.pc, 5], [f.pc + 5, 3]]), [], 'caller A and return LEA', forms.map(f => f.pc + 5)); return {e, a, b, forms, gateBlocks, targets};
}
function frameWords(e, esp, args) {
  const words = [[(esp - 8) >>> 0, 0x11223344], [(esp - 4) >>> 0, 0x89abcdef], ...args.map((value, i) => [(esp + i * 4) >>> 0, value]), [(esp + args.length * 4) >>> 0, 0x55667788]];
  for (const [address, value] of words) word(e, address, value); return words;
}
function entered(e, a, b, form, start, budget = 8, budgetGate = false) {
  select(e, form.pc, a); const stopped = copy(start); stopped.registers[4] = (start.registers[4] - 4) >>> 0; stopped.eip = form.target;
  run(e, a, budget, stopped, exit(budget === 1 ? 1 : 3, 1, 0, 0, 0, 0, 2), 'actual CALL stores return PC and enters B', 'initial', helper(), 1);
  if (budget === 1) failure(e, () => e.api.capture_resident_call(e.low, e.high, b.low, b.high, 1, 0), 13, 'CALL Budget is not a Gate', 'capture');
  select(e, form.target, b);
  if (budgetGate) { run(e, b, 0, stopped, exit(1, 0), 'zero Gate budget', 'resume'); failure(e, () => e.api.capture_resident_call(e.low, e.high, b.low, b.high, 1, 0), 13, 'Gate Budget cannot capture', 'capture'); }
  run(e, budgetGate ? copied(e, b) : b, 1, stopped, exit(8, 0, form.id), 'actual generated numeric Gate', 'resume'); return stopped;
}
function callRecord(token, id, tag, stopped, args, returnPC) {
  const b = new Uint8Array(112), v = new DataView(b.buffer); header(b, v, 0, 'R3CF', 112); [token, id, tag, args.length, stopped.eip, stopped.registers[4], returnPC, tag === 3 ? stopped.registers[1] : 0].forEach((x, i) => v.setUint32(16 + i * 4, x, true)); for (let i = 0; i < 16; i++) v.setUint32(48 + i * 4, args[i] ?? 0, true); return b;
}
function scalar(id, tag, args, thisPointer) { let sum = BigInt(id) + (tag === 3 ? BigInt(thisPointer) : 0n); args.forEach((value, i) => { sum += BigInt(value) * BigInt(i + 1); }); return Number(BigInt.asUintN(32, sum)); }
assert.equal(scalar(17, 1, [1, 2], 0), 22); assert.equal(scalar(0xffffffff, 2, [1, 2], 0), 4); assert.equal(scalar(0x80000001, 3, [1, 2], 0xfffffffd), 0x80000003);
function capture(e, b, tag, stopped, args, returnPC, legacy = false) {
  const before = arena(e), wanted = before.slice(), expected = callRecord(e.token + 1, b.gates.find(([pc]) => pc === stopped.eip)[1], tag, stopped, args, returnPC);
  assert.equal(legacy ? e.api.capture_call(e.low, e.high, 1, tag, args.length) : e.api.capture_resident_call(e.low, e.high, b.low, b.high, tag, args.length), 0); wanted.set(expected, TRANSFER); check(e, before, wanted, 'capture publishes only112'); e.token++;
  stats[legacy ? 'legacy_captures' : 'resident_captures']++; const actual = arena(e).slice(TRANSFER, TRANSFER + 112), view = new DataView(actual.buffer), actualArgs = Array.from({length: view.getUint32(28, true)}, (_, i) => view.getUint32(48 + i * 4, true));
  const result = scalar(view.getUint32(20, true), view.getUint32(24, true), actualArgs, view.getUint32(44, true)); assert.equal(result, scalar(b.gates.find(([pc]) => pc === stopped.eip)[1], tag, args, tag === 3 ? stopped.registers[1] : 0));
  captures.push({key: e.key.toString(), unit_id: b.id.toString(), owner: legacy ? 'replacement1' : 'resident', token: e.token, record_hex: Buffer.from(actual).toString('hex'), result}); return {token: e.token, result, tag, args, stopped: copy(stopped), returnPC};
}
function completed(frame, result = frame.result) { const s = copy(frame.stopped); s.registers[0] = result; s.registers[4] = (s.registers[4] + 4 + (frame.tag === 1 ? 0 : frame.args.length * 4)) >>> 0; s.eip = frame.returnPC; return s; }
function complete(e, b, frame, legacy = false) { const before = arena(e), s = completed(frame), wanted = before.slice(); assert.equal(legacy ? e.api.complete_call(e.low, e.high, 1, frame.token, frame.result) : e.api.complete_resident_call(e.low, e.high, b.low, b.high, frame.token, frame.result), 0); wanted.set(stateBytes(s), 0); wanted.set(exit(3, 0), 56); check(e, before, wanted, 'completion publishes only56+40'); stats[legacy ? 'legacy_completions' : 'resident_completions']++; completions.push({key: e.key.toString(), unit_id: b.id.toString(), token: frame.token, result: frame.result, state: s, owner: legacy ? 'replacement1' : 'resident'}); return s; }
function returned(e, a, s) { select(e, s.eip, a); const next = copy(s); next.registers[0] = Number(BigInt.asUintN(32, BigInt(s.registers[0]) + 7n)); next.eip += 3; run(e, a, 1, next, exit(1, 1, 0, 0, 0, 0, a.returns.length ? 2 : 1), 'actual caller LEA continuation', 'resume'); return next; }
function abandon(e, token) { unchanged(e, () => e.api.abandon_call(e.low, e.high, token), 0, 'owner-neutral abandon changes no arena or RAM'); stats.abandonments++; }
function close(e) { unchanged(e, () => e.api.close(), 0, 'close changes no arena'); }
function section(name, before) { sections.push({name, stats: Object.fromEntries(Object.entries(stats).map(([key, value]) => [key, value - (before[key] ?? 0)]))}); }

let before = {...stats};
const main = setup([[0x4000, 17], [0x4100, 0x80000001], [0x4200, 0xffffffff]]);
for (const tag of [1, 2, 3]) for (const count of [0, 16]) {
  const {e, a, b} = main, form = main.forms[tag - 1], start = state(form.pc), args = Array.from({length: count}, (_, i) => (0xfffff000 + i * 31) >>> 0), words = frameWords(e, STACK, args); reset(e, start);
  const first = tag === 1 && count === 0, stopped = entered(e, a, b, form, start, first ? 1 : 8, first), frame = capture(e, b, tag, stopped, args, form.pc + 5);
  if (count === 16) assert.equal(frame.result, [0xfff824c1, 0x7ff824b1, 0x134fc08e][tag - 1], 'literal full16 handler anchors');
  const result = complete(e, b, frame); returned(e, a, result); words[1][1] = form.pc + 5; for (const [address, value] of words) observe(e, address, value); stats.convention_cases++;
  assert.equal(e.api.generation(), 0); assert.equal(e.api.module_ptr(), 0); assert.equal(e.api.module_len(), 0);
}
section('six conventions and real caller returns', before);

before = {...stats};
{
  const {e, a, b} = main, form = main.forms[0], start = state(), args = [1, 2], words = frameWords(e, STACK, args); reset(e, start); const stopped = entered(e, a, b, form, start);
  for (const [low, high, idLow, idHigh, tag, count, status, label] of [[e.low ^ 1, e.high, b.low, b.high, 1, 0, 3, 'wrong process key'], [e.low, e.high, b.low, 1, 1, 0, 3, 'nonzero high limb must not truncate'], [e.low, e.high, 0, 0, 1, 0, 3, 'zero unit ID'], [e.low, e.high, a.low, a.high, 1, 0, 13, 'caller is not the gate owner'], [e.low, e.high, b.low, b.high, 0, 0, 7, 'invalid convention'], [e.low, e.high, b.low, b.high, 0, 17, 7, 'count before convention']]) failure(e, () => e.api.capture_resident_call(low, high, idLow, idHigh, tag, count), status, label, 'capture');
  const originalMagic = refresh(e).bytes[e.base]; e.bytes[e.base] ^= 1; failure(e, () => e.api.capture_resident_call(e.low, e.high, b.low, b.high, 1, 0), 13, 'malformed state header', 'capture'); e.bytes[e.base] = originalMagic;
  const gateID = e.view.getUint32(e.base + 80, true); e.view.setUint32(e.base + 80, gateID + 1, true); failure(e, () => e.api.capture_resident_call(e.low, e.high, b.low, b.high, 1, 0), 13, 'mismatched gate ID', 'capture'); e.view.setUint32(e.base + 80, gateID, true);
  cancelled(e, 1); failure(e, () => e.api.capture_resident_call(e.low, e.high, b.low, b.high, 1, 0), 16, 'capture cancellation', 'capture'); cancelled(e, 0);
  const frame = capture(e, b, 3, stopped, args, 0x3005); assert.equal(frame.token, 7); assert.equal(frame.result, 0x13579bf5);
  guard(e, a, 12); guard(e, b, 12); reject(e, a, 12, 'pending before malformed run pointers', [0xffffffff, 0xffffffff, 0xffffffff]); reject(e, copied(e, b), 12, 'copied current unit also remains parked');
  getter(e, b); select(e, 0x4000, b); select(e, 0x3005, a);
  failure(e, () => e.api.compile(0xffffffff), 12, 'legacy compile parks before invalid count'); failure(e, () => e.api.compile_resident(0xffffffff), 12, 'resident compile parks before invalid count'); failure(e, () => e.api.capture_resident_call(e.low, e.high, b.low, b.high, 0, 17), 12, 'pending before capture count/tag', 'capture');
  for (const [low, high, idLow, idHigh, token, status, label] of [[e.low ^ 1, e.high, b.low, b.high, frame.token, 3, 'completion wrong key'], [e.low, e.high, b.low, 1, frame.token, 3, 'completion bad high limb'], [e.low, e.high, 0, 0, frame.token, 3, 'completion zero ID'], [e.low, e.high, a.low, a.high, frame.token, 14, 'completion wrong current owner'], [e.low, e.high, b.low, b.high, 0, 14, 'zero token'], [e.low, e.high, b.low, b.high, frame.token + 1, 14, 'wrong token']]) failure(e, () => e.api.complete_resident_call(low, high, idLow, idHigh, token, 7), status, label, 'completion');
  failure(e, () => e.api.complete_call(e.low, e.high, 1, frame.token, 7), 3, 'absent legacy identity precedes owner mismatch', 'completion');
  const reserved = e.bytes[e.base + 68]; e.bytes[e.base + 68] = 1; cancelled(e, 1); failure(e, () => e.api.complete_resident_call(e.low, e.high, b.low, b.high, frame.token, 7), 15, 'saved exit byte equality precedes cancel', 'completion'); e.bytes[e.base + 68] = reserved;
  failure(e, () => e.api.complete_resident_call(e.low, e.high, b.low, b.high, frame.token, 7), 16, 'completion cancellation retry', 'completion'); cancelled(e, 0);
  word(e, STACK - 4, 0xdeadbeef); word(e, STACK, 91); word(e, STACK + 4, 92); e.bytes.fill(0xc3, e.base + TRANSFER, e.base + TRANSFER + 112);
  const result = complete(e, b, frame); assert.equal(result.eip, 0x3005); returned(e, a, result); failure(e, () => e.api.complete_resident_call(e.low, e.high, b.low, b.high, frame.token, 7), 14, 'single-use completion', 'completion');
  words[1][1] = 0xdeadbeef; words[2][1] = 91; words[3][1] = 92; for (const [address, value] of words) observe(e, address, value);
}
section('identity priorities and private frozen frame', before);

before = {...stats};
{
  const {e, a, b, forms} = setup(), form = forms[0]; assert.equal(b.id, 1n); descriptors(e, [[0x4000, 2]], [[0x4000, 17]]); unchanged(e, () => e.api.compile_with_gates(1, 1), 0, 'legacy generation1 installed before pending'); assert.equal(e.api.generation(), 1);
  const pointer = e.api.module_ptr() >>> 0, length = e.api.module_len() >>> 0, bytes = refresh(e).bytes.slice(pointer, pointer + length); assert.ok(WebAssembly.validate(bytes)); const filename = 'legacy-owner-alias.wasm'; writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes); stats.legacy_modules++;
  legacyControls.push({filename, key: e.key.toString(), generation: 1, pointer, length, sha256: hash(bytes), gate_pc: 0x4000, gate_id: 17, resident_unit_id: b.id.toString(), translated_legacy_guest_runs: 0});
  reject(e, copied(e, main.b), 3, 'same numeric unit1 from another process retains baked key', [0xffffffff, 0xffffffff, 0xffffffff]);
  for (const owner of ['resident-abandon', 'replacement-complete', 'resident-complete']) {
    const start = state(), words = frameWords(e, STACK, []); reset(e, start); const stopped = entered(e, a, b, form, start), legacy = owner === 'replacement-complete', frame = capture(e, b, 1, stopped, [], 0x3005, legacy); assert.equal(frame.token, ['resident-abandon', 'replacement-complete', 'resident-complete'].indexOf(owner) + 1);
    if (legacy) { failure(e, () => e.api.complete_resident_call(e.low, e.high, b.low, b.high, frame.token, 7), 14, 'Replacement1 cannot complete as Resident1', 'completion'); returned(e, a, complete(e, b, frame, true)); }
    else {
      failure(e, () => e.api.complete_call(e.low, e.high, 1, frame.token, 7), 14, 'Resident1 cannot complete as Replacement1', 'completion');
      if (owner === 'resident-abandon') { failure(e, () => e.api.begin_callback(e.low, e.high, 1, frame.token, 0x3005, 0x4000, 17, 17), 7, 'legacy callback count priority'); failure(e, () => e.api.begin_callback(e.low, e.high, 1, frame.token, 0x3005, 0x4000, 17, 0), 14, 'resident outer cannot create callback'); abandon(e, frame.token); }
      else returned(e, a, complete(e, b, frame));
    }
    words[1][1] = 0x3005; for (const [address, value] of words) observe(e, address, value);
  }
  close(e);
}
section('explicit numeric alias and shared tokens', before);

before = {...stats};
for (const kind of ['return-read', 'argument-read', 'cross-return', 'wrapped-frame', 'guest-overflow']) {
  const {e, a, b, forms} = setup(), form = forms[0], esp = kind === 'argument-read' ? 0xb000 : kind === 'cross-return' ? 0xb002 : kind === 'wrapped-frame' ? 0 : kind === 'guest-overflow' ? 2 : STACK;
  if (kind === 'wrapped-frame') { assert.equal(e.api.map(0xfffff000, 1, 3), 0); assert.equal(e.api.map(0, 1, 3), 0); }
  const args = [kind === 'wrapped-frame' ? 0xfffffffe : 1], words = kind === 'guest-overflow' ? [] : frameWords(e, esp, args), start = state(0x3000, esp); reset(e, start);
  if (kind === 'guest-overflow') {
    run(e, a, 8, start, exit(5, 0, 3, 0xfffffffe, 2, 4, 2), 'CALL wrapping EA with overflowing Store4 fails before Gate', 'initial', helper(0, 3, 0xfffffffe, 2), 1); stats.guest_store_faults++;
    failure(e, () => e.api.capture_resident_call(e.low, e.high, b.low, b.high, 1, 0), 13, 'memory fault exit is not Gate', 'capture'); close(e); continue;
  }
  const stopped = entered(e, a, b, form, start); let currentArgs = args, returnPC = 0x3005;
  if (kind !== 'wrapped-frame') {
    if (kind === 'argument-read') assert.equal(e.api.unmap(0xb000, 1), 0); else assert.equal(e.api.protect(kind === 'cross-return' ? 0xb000 : 0xa000, 1, 2), 0);
    failure(e, () => e.api.capture_resident_call(e.low, e.high, b.low, b.high, 2, 1), 8, 'host direct Read4 fault preserves helper and whole arena', 'capture'); stats.capture_read_faults++;
    if (kind === 'argument-read') { assert.equal(e.api.map(0xb000, 1, 3), 0); word(e, esp, 0xabcdef01); word(e, esp + 4, 0x55667788); currentArgs = [0xabcdef01]; }
    else { assert.equal(e.api.protect(kind === 'cross-return' ? 0xb000 : 0xa000, 1, 3), 0); currentArgs = [kind === 'cross-return' ? 99 : 91]; word(e, esp, currentArgs[0]); }
    if (kind === 'return-read') { returnPC = 0x3008; word(e, esp - 4, returnPC); }
    stats.repairs++;
  }
  const frame = capture(e, b, kind === 'wrapped-frame' ? 3 : 2, stopped, currentArgs, returnPC), result = complete(e, b, frame);
  if (returnPC === 0x3008) { miss(e, returnPC); const cold = compile(e, [[returnPC, 3]], [], 'cold return block only after pending is cleared'); returned(e, cold, result); }
  else returned(e, a, result);
  words[1][1] = returnPC; words[2][1] = currentArgs[0]; for (const [address, value] of words) observe(e, address, value); close(e);
}
section('direct capture reads repair and wrapped addresses', before);

before = {...stats};
{
  const {e, a, b, forms} = setup(), form = forms[0], start = state(), words = frameWords(e, STACK, []); reset(e, start, 1);
  run(e, a, 8, start, exit(2, 0, 0, 0, 0, 0, 2), 'preset cancellation stops CALL', 'initial'); failure(e, () => e.api.capture_resident_call(e.low, e.high, b.low, b.high, 1, 0), 13, 'cancelled guest exit precedes capture cancellation', 'capture'); cancelled(e, 0);
  const stopped = copy(start); stopped.registers[4] -= 4; stopped.eip = 0x4000; run(e, a, 8, stopped, exit(3, 1, 0, 0, 0, 0, 2), 'CALL resumes after clearing cancellation', 'resume', helper(), 1); run(e, b, 1, stopped, exit(8, 0, 17), 'Gate after cancelled CALL retry', 'resume');
  cancelled(e, 1); failure(e, () => e.api.capture_resident_call(e.low, e.high, b.low, b.high, 1, 0), 16, 'capture can retry cancelled stop', 'capture'); cancelled(e, 0); const first = capture(e, b, 1, stopped, [], 0x3005);
  cancelled(e, 1); failure(e, () => e.api.complete_resident_call(e.low, e.high, b.low, b.high, first.token, first.result), 16, 'cancelled pending completion', 'completion'); abandon(e, first.token);
  run(e, b, 1, stopped, exit(2, 0), 'owner-neutral abandon leaves CPU at cancelled Gate', 'resume'); cancelled(e, 0); run(e, b, 1, stopped, exit(8, 0, 17), 'Gate resumes without repeating CALL', 'resume');
  const second = capture(e, b, 1, stopped, [], 0x3005); assert.equal(second.token, 2); returned(e, a, complete(e, b, second)); words[1][1] = 0x3005; for (const [address, value] of words) observe(e, address, value); stats.cancellation_cases++; close(e);
}
section('cancelled pending abandon and Gate retry', before);

before = {...stats};
for (const kind of ['same-byte', 'protect', 'remap', 'whole-unit', 'marker-lower', 'marker-upper']) {
  const twoPages = kind.startsWith('marker-'), target = twoPages ? 0x4fff : 0x4000, {e, a, b, forms, gateBlocks, targets} = setup([[target, 17]], kind === 'whole-unit'), form = forms[0], start = state(), words = frameWords(e, STACK, []); reset(e, start);
  const stopped = entered(e, a, b, form, start), first = capture(e, b, 1, stopped, [], 0x3005); guard(e, a, 12); guard(e, b, 12);
  if (kind === 'protect') assert.equal(e.api.protect(0x4000, 1, 5), 0);
  else if (kind === 'remap') { assert.equal(e.api.unmap(0x4000, 1), 0); assert.equal(e.api.map(0x4000, 1, 7), 0); upload(e, target, marker); }
  else if (kind === 'whole-unit') upload(e, 0x4500, Buffer.from('8d4007', 'hex'));
  else if (twoPages) upload(e, target + (kind === 'marker-upper' ? 1 : 0), Buffer.from([kind === 'marker-upper' ? 0x0b : 0x0f]));
  else upload(e, target, marker);
  cancelled(e, 1); failure(e, () => e.api.complete_resident_call(e.low, e.high, b.low, b.high, 0, first.result), 4, 'stale owner precedes token and cancel', 'completion'); guard(e, a, 12); guard(e, b, 4); miss(e, target, 4);
  reject(e, b, 4, 'stale B cannot run'); reject(e, copied(e, b), 4, 'copied stale B cannot rebind', [0xffffffff, 0xffffffff, 0xffffffff]); abandon(e, first.token); cancelled(e, 0);
  run(e, a, 8, stopped, exit(3, 0, 0, 0, 0, 0, 2), 'current A at B does not replay CALL', 'resume');
  const freshB = compile(e, gateBlocks, targets, 'fresh gate owner after explicit abandon'); assert.ok(freshB.id > b.id); select(e, target, freshB); guard(e, freshB);
  failure(e, () => e.api.capture_resident_call(e.low, e.high, b.low, b.high, 1, 0), 4, 'old owner cannot migrate to fresh B', 'capture'); run(e, freshB, 1, stopped, exit(8, 0, 17), 'fresh B stops at committed target', 'resume');
  const second = capture(e, freshB, 1, stopped, [], 0x3005); assert.equal(second.token, 2); failure(e, () => e.api.complete_resident_call(e.low, e.high, b.low, b.high, second.token, second.result), 4, 'old unit cannot complete fresh pending', 'completion'); returned(e, a, complete(e, freshB, second));
  words[1][1] = 0x3005; for (const [address, value] of words) observe(e, address, value); for (const [address, value] of [[target - 4, 0], [target, 0x0b0f], [target + 4, 0]]) observe(e, address, value); stats.currency_cases++; close(e);
}
{
  const {e, a, b, forms} = setup(), start = state(), words = frameWords(e, STACK, []); reset(e, start); const stopped = entered(e, a, b, forms[0], start), frame = capture(e, b, 1, stopped, [], 0x3005);
  upload(e, 0x3000, callBytes(0x3000, 0x4000)); guard(e, a, 4); guard(e, b, 12); const result = complete(e, b, frame); guard(e, b); miss(e, 0x3005, 4); reject(e, copied(e, a), 4, 'stale caller rejects after valid B completion');
  const freshReturn = compile(e, [[0x3005, 3]], [], 'fresh return unit is separate from pending B'); returned(e, freshReturn, result); words[1][1] = 0x3005; for (const [address, value] of words) observe(e, address, value); stats.currency_cases++; close(e);
}
section('current caller stale gate and current gate stale caller', before);

before = {...stats};
{
  const {e, a, b, forms} = setup(), start = state(), words = frameWords(e, STACK, []); reset(e, start); const stopped = entered(e, a, b, forms[0], start), frame = capture(e, b, 1, stopped, [], 0x3005);
  refresh(e).bytes[e.base] ^= 1; cancelled(e, 1); failure(e, () => e.api.complete_resident_call(e.low, e.high, b.low, b.high, frame.token, frame.result), 15, 'corrupt saved state precedes cancel', 'completion'); abandon(e, frame.token);
  failure(e, () => e.api.abandon_call(e.low, e.high, frame.token), 14, 'abandoned token cannot release twice'); words[1][1] = 0x3005; for (const [address, value] of words) observe(e, address, value); close(e);
}
{
  const {e, a, b, forms} = setup(), start = state(), words = frameWords(e, STACK, []); reset(e, start); const stopped = entered(e, a, b, forms[0], start), frame = capture(e, b, 1, stopped, [], 0x3005); close(e);
  failure(e, () => e.api.capture_resident_call(e.low ^ 1, e.high, 0, 0, 0, 17), 5, 'closed before capture identity/count', 'capture'); failure(e, () => e.api.complete_resident_call(e.low ^ 1, e.high, 0, 0, frame.token, 7), 5, 'closed before completion identity/token', 'completion'); failure(e, () => e.api.abandon_call(e.low, e.high, frame.token), 5, 'close clears pending before abandon'); miss(e, 0x4000, 5); reject(e, copied(e, b), 5, 'closed copied module', [0xffffffff, 0xffffffff, 0xffffffff]);
  assert.deepEqual(arena(e).slice(0, 56), stateBytes(stopped)); assert.deepEqual(arena(e).slice(TRANSFER, TRANSFER + 112), callRecord(frame.token, 17, 1, stopped, [], 0x3005));
  assert.equal(words[1][0], STACK - 4);
}
close(main.e);
section('corrupt cancellation-independent abandon and closed owner', before);

const expectedStats = {engine_instances: 17, compiled_units: 42, legacy_modules: 1, canonical_runs: 86, rejected_runs: 17, full_arena_checks: 544, metadata_checks: 118, find_success: 75, find_failure: 9, initial_calls: 25, resumes: 61, resident_captures: 30, legacy_captures: 1, resident_completions: 20, legacy_completions: 1, abandonments: 9, capture_failures: 25, completion_failures: 28, rejected_operations: 68, guard_success: 7, guard_failure: 28, host_reads: 141, host_writes: 134, guest_store32: 25, bound_store_sites: 19, convention_cases: 6, capture_read_faults: 3, guest_store_faults: 1, repairs: 3, cancellation_cases: 1, currency_cases: 7};
assert.deepEqual(stats, expectedStats, 'independently precomputed authored-path counts');
assert.deepEqual(sections.map(s => [s.stats.canonical_runs, s.stats.full_arena_checks, s.stats.metadata_checks, s.stats.resident_captures, s.stats.resident_completions, s.stats.host_reads, s.stats.host_writes]), [[19, 119, 20, 6, 6, 66, 66], [3, 42, 6, 1, 1, 5, 8], [8, 41, 10, 2, 1, 9, 9], [13, 70, 23, 4, 4, 16, 21], [6, 20, 3, 2, 1, 3, 3], [33, 224, 48, 13, 7, 39, 21], [4, 28, 8, 2, 0, 3, 6]], 'manual seven-section derivation');
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const oldFamilies = ['p2-process-call', 'p2-callbacks', 'p2-callback-code'];
const sourcePaths = ['engine/tests/process_resident_call_wasm.rs', 'engine/tests/fixtures/p2-resident-call/run.mjs', 'engine/tests/process_resident_call.rs', 'engine/tests/process_call32.rs', 'engine/tests/process_callbacks.rs', 'engine/tests/callback_code_install.rs', 'engine/tests/process_call_wasm.rs', 'engine/tests/callbacks_wasm.rs', 'engine/tests/callback_code_wasm.rs', ...oldFamilies.flatMap(name => [`engine/tests/fixtures/${name}/run.mjs`, `engine/tests/fixtures/${name}/integer.S`]), 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', ...productionPaths];
assert.equal(new Set(sourcePaths).size, sourcePaths.length); assert.equal(sourcePaths.length, 77);
const provenance = {
  stats, section_derivation: sections, actual_resident_runs: stats.canonical_runs + stats.rejected_runs, actual_resident_guard_entries: stats.canonical_runs + stats.rejected_runs + stats.guard_success + stats.guard_failure, bound_store_identity_checks: stats.guest_store32, actual_guest_helper_calls_from_authored_execution: stats.guest_store32,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  authored, unit_bindings: units.map(u => ({label: u.label, key: u.key, id: u.id.toString(), pointer: u.pointer, length: u.length, filename: u.filename, blocks: u.blocks, gates: u.gates, store_return_pcs: u.returns})), metadata_observations: metadata, capture_records: captures, completion_rows: completions,
  legacy_controls: legacyControls, legacy_stop_qualification: 'one legacy capture deliberately accepts a real resident Gate at the identical installed legacy GatePC/ID; this proves Replacement1 vs Resident1 API owner selection, not hostile-host stop provenance authentication',
  prior_read_only_inventory: {path: 'target/r3-326-old-reference-inventory.json', sha256: '4eca6783a7b8e912c0404e51f2815d215c99756496d8fb5708121114fc0cf7b9', modules: 31, objects: 3, assemblies: 3, llvm_symbols: 33, wrapper_sources: 3, historical_actual_runs: 75, note: 'reference only; this portable integration does not read or require the ignored historical inventory. Parent delivery compares full modules/object/symbol bodies and exact old top-level counters after three separate legacy consumers run once.'},
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_call_wasm', '--test', 'process_call_wasm', '--test', 'callbacks_wasm', '--test', 'callback_code_wasm', '--', '--nocapture']},
  reused: 'unchanged scalar CallFrame32/CallRecord32/Exitv3/helper codec/CALL lowering/bound Store4 and native token-exhaustion/current-snapshot algorithms are source/prior-proof reuse. Unchanged gate/control/stack/binary/unary/MOV/read/indirect/table and injected/deep/late-helper actual suites are not replayed. Parent three legacy zero-CPU baseline captures and three affected process-call/callback/code-installation actual consumers are separately audited without fixture edits.',
  claim: 'actual synchronous Node engine-Wasm resident CALL A3000→Gate B4000→capture112→independent weighted BigInt scalar handler→complete56+Exitv3NeedCode40→finder→actual caller LEA continuation for cdecl/stdcall/thiscall zero/full16 arguments and high/MAX GateIDs; no CPU writes between successful continued phases. Separate initializations only, except explicitly labelled negative header/Gate-ID/exit-byte corruption and exact restoration for retry. Every generated module retains baked process key/full u64 unit ID/guard7/run4 and direct Store6 when needed; only19 return-PC-immediate binding sites, ADDRESS26/storeindex1, no new hot callback/capture import or JS guest helper. Full4236 arena compares include every metadata/capture/completion/failure/run/guard/RAM observer; data/stack/returnword/neighbours explicit and distinct from code. Host capture uses direct width4 reads, preserves helper40 on success/fault and rereads current repaired RAM; wrapped argument addresses and completion ESP use uint32 while overflowing actual guest Store4 faults before Gate. Both directions numeric Resident1/Replacement1 owner separation and shared tokens, Busy compile/execution, metadata inspection, frozen stack/args/public transfer, cancellation/state/currency ordering, owner-neutral abandon, close/original/copied/foreign bindings. Whole/two-page/same-byte/protect/remap B invalidation preserves pending; abandon then fresh B resumes committed CPU/ESP without CALL replay. Valid B can complete to stale or absent A return code, followed by controlled finder and explicit cold/fresh return compilation. Unit IDs in this bounded run have high0; bad high limbs exercise full-u64 parsing without valid high-ID runtime claim. No resident callback/provider/pointer marshalling/RET/provenance authentication/async/scheduler/installer/pin/eviction/table/browser/performance/game/fullP2-V0 claim. Counters derive from authored paths, not instrumentation.',
};
assert.equal(Object.keys(artifacts).length, 82); assert.equal(Object.keys(authored).length, 39);
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2)); console.log(JSON.stringify({...provenance, sources: sourcePaths.length, metadata_observations: metadata.length, artifacts: outputDir}));
