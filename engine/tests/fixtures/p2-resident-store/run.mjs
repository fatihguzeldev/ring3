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
const SIZE = 4236, TRANSFER = 140, DATA = 0x5000;
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'generation', 'module_ptr', 'module_len', 'guard_resident', 'compile_resident', 'resident_module', 'find_resident', 'read8', 'read16', 'read32', 'write32', 'store_resident32', 'compile', 'compile_with_gates', 'guard', 'capture_call', 'abandon_call'];
const stats = {engine_instances: 0, compiled_units: 0, canonical_runs: 0, rejected_runs: 0, full_arena_checks: 0, metadata_checks: 0, successful_guards: 0, failed_guards: 0, find_success: 0, find_failure: 0, continuation_initial_calls: 0, continuation_resumes: 0, form_cases: 0, alias_cases: 0, smc_cases: 0, bound_store_sites: 0, raw_store_success: 0, raw_store_rejected: 0, legacy_compiles: 0, legacy_runs: 0, pending_captures: 0, pending_abandons: 0, algorithm_cases: 0, algorithm_retired: 0, faults: 0, repairs: 0, cancellations: 0, host_reads: 0, host_writes: 0, mixed_cases: 0, guest_read8: 0, guest_read16: 0, guest_read32: 0, guest_store32: 0};
const units = [], artifacts = {}, metadata = [];

function refresh(engine) {
  if (engine.buffer !== engine.memory.buffer) { engine.buffer = engine.memory.buffer; engine.bytes = new Uint8Array(engine.buffer); engine.view = new DataView(engine.buffer); }
  return engine;
}

function arena(engine) { return refresh(engine).bytes.slice(engine.base, engine.base + SIZE); }

function upload(engine, address, bytes) {
  refresh(engine).bytes.set(bytes, engine.base + TRANSFER);
  assert.equal(engine.api.upload(address, bytes.length), 0); refresh(engine);
}

function descriptors(engine, blocks) {
  refresh(engine); blocks.forEach(([pc, length], index) => { engine.view.setUint32(engine.base + TRANSFER + index * 8, pc, true); engine.view.setUint32(engine.base + TRANSFER + index * 8 + 4, length, true); });
}

function header(bytes, view, pointer, magic, size, version = 1) {
  bytes.set(Buffer.from(magic, 'ascii'), pointer); view.setUint16(pointer + 4, version, true); view.setUint16(pointer + 6, 1, true); view.setUint32(pointer + 8, size, true); view.setUint32(pointer + 12, 0, true);
}

function uleb(bytes, cursor) {
  let out = 0, shift = 0, byte;
  do { byte = bytes[cursor.i++]; assert.ok(byte !== undefined && shift <= 28); out += (byte & 127) * 2 ** shift; shift += 7; } while (byte & 128);
  return out;
}

function sleb32(bytes, cursor) {
  let out = 0n, shift = 0n, byte;
  do { byte = bytes[cursor.i++]; assert.ok(byte !== undefined && shift <= 28n); out |= BigInt(byte & 127) << shift; shift += 7n; } while (byte & 128);
  if (byte & 64) out -= 1n << shift;
  return Number(BigInt.asUintN(32, out));
}

function bakedPrefix(bytes, engine, id) {
  assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  const cursor = {i: 8}; let found = false;
  while (cursor.i < bytes.length) {
    const section = bytes[cursor.i++], length = uleb(bytes, cursor), end = cursor.i + length;
    if (section === 10) {
      assert.equal(uleb(bytes, cursor), 1); const bodyLength = uleb(bytes, cursor), bodyEnd = cursor.i + bodyLength; assert.equal(bodyEnd, end);
      const groups = uleb(bytes, cursor); for (let i = 0; i < groups; i++) { uleb(bytes, cursor); assert.ok([0x7f, 0x7e].includes(bytes[cursor.i++])); }
      const constants = []; for (let i = 0; i < 4; i++) { assert.equal(bytes[cursor.i++], 0x41); constants.push(sleb32(bytes, cursor)); }
      assert.deepEqual(constants, [engine.low, engine.high, Number(id & 0xffffffffn), Number(id >> 32n)], 'immutable four-limb binding independently decoded from actual function start');
      assert.deepEqual([...bytes.subarray(cursor.i, cursor.i + 8)], [0x20, 0, 0x20, 1, 0x20, 3, 0x10, 0], 'direct guard call precedes CPU preflight'); found = true;
    }
    cursor.i = end;
  }
  assert.ok(found);
}

function noLegacy(engine) {
  const legacy = engine.legacy;
  assert.equal(engine.api.generation() >>> 0, legacy?.generation ?? 0);
  assert.equal(engine.api.module_ptr() >>> 0, legacy && !legacy.stale ? legacy.pointer : 0); assert.equal(engine.api.module_len() >>> 0, legacy && !legacy.stale ? legacy.length : 0);
  if (legacy) assert.deepEqual(refresh(engine).bytes.slice(legacy.pointer, legacy.pointer + legacy.length), legacy.bytes);
}
function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const key = 0x9445678990000000n + BigInt(++stats.engine_instances);
  const engine = refresh({instance, api, memory: instance.exports.memory, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n)});
  assert.equal(api.open(24, engine.low, engine.high), 0); engine.base = api.arena_ptr() >>> 0; refresh(engine);
  assert.ok(engine.base > 0 && engine.base + SIZE <= engine.bytes.length); noLegacy(engine); return engine;
}
function record(engine, before, label) {
  const after = arena(engine), view = new DataView(after.buffer), fields = Array.from({length: 6}, (_, i) => view.getUint32(TRANSFER + i * 4, true));
  assert.equal(fields[0], 1); assert.equal(fields[1], 24);
  const [,, low, high, pointer, length] = fields, id = BigInt(low) | (BigInt(high) << 32n);
  assert.ok(id > 0n && pointer > 0 && length > 8 && pointer + length <= refresh(engine).bytes.length);
  const wanted = before.slice(); wanted.set(after.subarray(TRANSFER, TRANSFER + 24), TRANSFER); assert.deepEqual(after, wanted, `${label}: only metadata24`);
  const bytes = engine.bytes.slice(pointer, pointer + length); assert.ok(WebAssembly.validate(bytes)); bakedPrefix(bytes, engine, id); noLegacy(engine);
  stats.metadata_checks++; metadata.push({label, id: id.toString(), pointer, length, sha256: hash(bytes)}); return {id, low, high, pointer, length, bytes};
}
function compile(engine, blocks, reads, label, stores = 0) {
  descriptors(engine, blocks); const before = arena(engine); assert.equal(engine.api.compile_resident(blocks.length), 0, label);
  const selected = record(engine, before, label), module = new WebAssembly.Module(selected.bytes);
  const helpers = [...reads.filter(name => name === 'read32'), ...(stores ? ['store_resident32'] : []), ...reads.filter(name => name !== 'read32')];
  const expected = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard_resident', kind: 'function'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), expected, 'exact direct bound guard/read/store imports'); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const imports = {guard_resident: engine.api.guard_resident, ...Object.fromEntries(reads.map(name => [name, engine.api[name]])), ...(stores ? {store_resident32: engine.api.store_resident32} : {})};
  boundStores(selected.bytes, engine, selected.id, stores, 1 + Number(reads.includes('read32')));
  const instance = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: imports}); assert.equal(instance.exports.run.length, 4);
  const filename = `unit-${++stats.compiled_units}.wasm`; writeFileSync(join(outputDir, filename), selected.bytes); artifacts[filename] = hash(selected.bytes);
  const unit = {...selected, module, instance, run: instance.exports.run, blocks, reads, stores, filename, label}; units.push(unit); return unit;
}
function select(engine, pc, unit) {
  const before = arena(engine); assert.equal(engine.api.find_resident(pc), 0); const selected = record(engine, before, 'exact-PC selection');
  assert.equal(selected.id, unit.id); assert.equal(selected.pointer, unit.pointer); assert.deepEqual(selected.bytes, unit.bytes); stats.find_success++;
}
function unchanged(engine, action, status, label) {
  const before = arena(engine); assert.equal(action(), status, label); assert.deepEqual(arena(engine), before, `${label}: full arena preserved`); stats.full_arena_checks++; noLegacy(engine);
}
function guard(engine, unit, status = 0) {
  unchanged(engine, () => engine.api.guard_resident(engine.low, engine.high, unit.low, unit.high, engine.base, engine.base + 56, engine.base + 96), status, 'direct resident guard'); stats[status === 0 ? 'successful_guards' : 'failed_guards']++;
}
function rejected(engine, unit, status, label) {
  unchanged(engine, () => unit.run(engine.base, engine.base + 56, 4, engine.base + 96), status, label); stats.rejected_runs++;
}
function initial(eip) {
  return {registers: [0x7fffffff, 0x13579bdf, 0x23456789, 0x3456789a, DATA, 0x56789abc, DATA, 15], eip, eflags: 0xcd7};
}
function copy(value) { return {...value, registers: [...value.registers]}; }
function reset(engine, value, cancel = 0) {
  refresh(engine); header(engine.bytes, engine.view, engine.base, 'R3ST', 56);
  value.registers.forEach((r, i) => engine.view.setUint32(engine.base + 16 + i * 4, r, true)); engine.view.setUint32(engine.base + 48, value.eip, true); engine.view.setUint32(engine.base + 52, value.eflags, true);
  engine.bytes.fill(0xa5, engine.base + 56, engine.base + 96); engine.view.setUint32(engine.base + 96, cancel, true); engine.bytes.fill(0x5a, engine.base + 100, engine.base + 140);
}
function unary(kind, input, oldFlags) {
  const a = BigInt(input), increment = kind === 'inc', value = Number(BigInt.asUintN(32, a + (increment ? 1n : -1n)));
  let result = (oldFlags & 0x401) | 2;
  if (increment ? (a & 15n) === 15n : (a & 15n) === 0n) result |= 0x10;
  if (increment ? a === 0x7fffffffn : a === 0x80000000n) result |= 0x800;
  if (value === 0) result |= 0x40; if (value >= 0x80000000) result |= 0x80;
  let ones = 0; for (let byte = value & 255; byte; byte >>>= 1) ones += byte & 1; if (ones % 2 === 0) result |= 4;
  return {value, flags: result};
}
for (const [kind, input, oldFlags, value, flags] of [['inc', 15, 0xcd7, 16, 0x413], ['inc', 0xffffffff, 2, 0, 0x56], ['inc', 0x7fffffff, 0x403, 0x80000000, 0xc97], ['dec', 1, 0xcd7, 0, 0x447], ['dec', 0x80000000, 2, 0x7fffffff, 0x816]]) assert.deepEqual(unary(kind, input, oldFlags), {value, flags}, 'literal INC/DEC flag anchor');
function exit(reason, retired, detail = 0, address = 0, width = 0, version = 2, access = 2) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer); header(bytes, view, 0, 'R3EX', 40, version);
  [reason, retired, detail, address, reason === 5 ? access : 0, width].forEach((v, i) => view.setUint32(16 + i * 4, v, true)); return bytes;
}
function helper(width, value, detail = 0, address = 0, access = 1) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer); header(bytes, view, 0, 'R3MH', 40, width === 4 ? 1 : 2);
  [detail ? 1 : 0, detail ? 0 : value, detail, address, detail ? access : 0, width === 4 && detail === 0 ? 0 : width].forEach((v, i) => view.setUint32(16 + i * 4, v, true)); return bytes;
}
function run(engine, unit, budget, expected, expectedExit, expectedHelper, label, continuation, stores = 0, reads = 0) {
  const before = arena(engine); assert.equal(unit.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, label);
  const wanted = before.slice(), view = new DataView(wanted.buffer);
  expected.registers.forEach((r, i) => view.setUint32(16 + i * 4, r, true)); view.setUint32(48, expected.eip, true); view.setUint32(52, expected.eflags, true); wanted.set(expectedExit, 56);
  if (expectedHelper !== undefined) wanted.set(expectedHelper, 100);
  assert.deepEqual(arena(engine), wanted, `${label}: all4236 state/exit/helper/cancel/transfer bytes`); stats.canonical_runs++; stats.full_arena_checks++; noLegacy(engine);
  if (continuation === 'initial') stats.continuation_initial_calls++; else if (continuation === 'resume') stats.continuation_resumes++;
  if (typeof reads === 'number') stats.guest_read32 += reads; else for (const [width, count] of Object.entries(reads)) stats[`guest_read${width}`] += count; stats.guest_store32 += stores;
}
function word(engine, address, value) { assert.equal(engine.api.write32(address, value), 0); stats.host_writes++; refresh(engine); }
function observe(engine, address, width, expected) {
  const before = arena(engine); assert.equal(engine.api[`read${width * 8}`](address), 0); stats.host_reads++;
  const wanted = before.slice(); wanted.set(helper(width, expected), 100); assert.deepEqual(arena(engine), wanted, 'host observation only replaces helper40'); stats.full_arena_checks++;
}
function boundStores(bytes, engine, id, count, index) {
  const cursor = {i: 8}, signatures = [], importTypes = new Map(); let body;
  function string() { const length = uleb(bytes, cursor), value = Buffer.from(bytes.subarray(cursor.i, cursor.i + length)).toString(); cursor.i += length; return value; }
  while (cursor.i < bytes.length) {
    const section = bytes[cursor.i++], length = uleb(bytes, cursor), end = cursor.i + length;
    if (section === 1) {
      const n = uleb(bytes, cursor); for (let i = 0; i < n; i++) { assert.equal(bytes[cursor.i++], 0x60); const parameters = uleb(bytes, cursor), args = [...bytes.subarray(cursor.i, cursor.i + parameters)]; cursor.i += parameters; const results = uleb(bytes, cursor), returns = [...bytes.subarray(cursor.i, cursor.i + results)]; cursor.i += results; signatures.push({args, returns}); }
    } else if (section === 2) {
      const n = uleb(bytes, cursor); for (let i = 0; i < n; i++) { string(); const name = string(), kind = bytes[cursor.i++]; if (kind === 0) importTypes.set(name, uleb(bytes, cursor)); else { assert.equal(kind, 2); const flags = uleb(bytes, cursor); uleb(bytes, cursor); if (flags & 1) uleb(bytes, cursor); } }
    } else if (section === 10) { assert.equal(uleb(bytes, cursor), 1); const n = uleb(bytes, cursor); body = bytes.subarray(cursor.i, cursor.i + n); }
    cursor.i = end;
  }
  assert.ok(body); assert.deepEqual(signatures[importTypes.get('guard_resident')], {args: Array(7).fill(0x7f), returns: [0x7f]});
  if (!count) { assert.equal(importTypes.has('store_resident32'), false); return; }
  assert.deepEqual(signatures[importTypes.get('store_resident32')], {args: Array(6).fill(0x7f), returns: [0x7f]});
  const limbs = [engine.low, engine.high, Number(id & 0xffffffffn), Number(id >> 32n)]; let found = 0;
  for (let offset = 0; offset < body.length; offset++) {
    if (body[offset] !== 0x41) continue;
    const c = {i: offset}, constants = []; let match = true;
    try { for (let j = 0; j < 4; j++) { if (body[c.i++] !== 0x41) { match = false; break; } constants.push(sleb32(body, c)); } } catch { match = false; }
    if (!match || !constants.every((value, j) => value === limbs[j]) || body[c.i] !== 0x20 || body[c.i + 1] !== 26) continue;
    c.i += 2; const source = body[c.i++]; assert.ok(source === 0x20 || source === 0x41);
    if (source === 0x20) assert.ok(uleb(body, c) >= 4 && body[c.i - 1] <= 11); else sleb32(body, c);
    assert.equal(body[c.i++], 0x10); assert.equal(uleb(body, c), index); found++;
  }
  assert.equal(found, count, 'each Store4 call has the immutable entry key/ID, old EA and register/immediate value'); stats.bound_store_sites += found;
}
function miss(engine, pc, status = 17) { unchanged(engine, () => engine.api.find_resident(pc), status, 'finder failure'); stats.find_failure++; }
function at(value, pc) { const next = copy(value); next.eip = pc; return next; }
function prefixed(start, pc = start.eip + 1) { const value = copy(start), flags = unary('inc', value.registers[7], value.eflags); value.registers[7] = flags.value; value.eflags = flags.flags; value.eip = pc; return value; }
function afterA(start) { const value = prefixed(start), flags = unary('inc', value.registers[1], value.eflags); value.registers[1] = flags.value; value.eflags = flags.flags; value.eip = 0x2000; return value; }
function storeHelper(detail = 0, address = 0) { return helper(4, 0, detail, address, 2); }
function rawStore(engine, unit, address, value, status, label, limbs = {}) {
  unchanged(engine, () => engine.api.store_resident32(limbs.low ?? engine.low, limbs.high ?? engine.high, limbs.idLow ?? unit.low, limbs.idHigh ?? unit.high, address, value), status, label); stats[status === 0 || status === 11 ? 'raw_store_success' : 'raw_store_rejected']++;
}
function close(engine, selected) { const before = arena(engine); assert.equal(engine.api.close(), 0); assert.deepEqual(arena(engine), before); stats.full_arena_checks++; engine.legacy = undefined; for (const unit of selected) rejected(engine, unit, 5, 'closed resident module'); rawStore(engine, selected[0], DATA, 0, 5, 'closed bound helper'); }
function authoredCode(name, bytes) { const filename = `${name}.x86`; writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes); return bytes; }
function directBlock(pc, store) { const bytes = Buffer.alloc(1 + store.length + 1 + 5); bytes[0] = 0x47; store.copy(bytes, 1); bytes[1 + store.length] = 0x41; bytes[2 + store.length] = 0xe9; bytes.writeInt32LE(0x2000 - (pc + bytes.length), 3 + store.length); return bytes; }
const forms = [
  {name: 'mov89', pc: 0x1000, hex: '8903', address: DATA, value: 0x87654321},
  {name: 'mov89-base-source', pc: 0x1100, hex: '891b', address: DATA, value: DATA},
  {name: 'mov89-esp', pc: 0x1200, hex: '892424', address: DATA, value: DATA},
  {name: 'movA3', pc: 0x1300, hex: 'a300500000', address: DATA, value: 0x87654321},
  {name: 'movC7', pc: 0x1400, hex: 'c70378563412', address: DATA, value: 0x12345678},
  {name: 'movC7-sib', pc: 0x1500, hex: 'c7448c04efbeadde', address: DATA + 12, value: 0xdeadbeef},
  {name: 'mov89-index-source', pc: 0x1600, hex: '894c8c04', address: DATA + 12, value: 2},
];
const authored = Object.fromEntries(forms.map(form => [form.name, authoredCode(form.name, directBlock(form.pc, Buffer.from(form.hex, 'hex')))]));
authored.reader = authoredCode('reader', Buffer.from('8b16eb00', 'hex')); authored.loop = authoredCode('loop', Buffer.from('890340490f85f6ffffff', 'hex'));
const engine = fresh(); assert.equal(engine.api.store_resident32.length, 6);
for (const page of [0x1000, 0x2000, 0x3000]) assert.equal(engine.api.map(page, 1, 7), 0); assert.equal(engine.api.map(0x4000, 3, 3), 0);
for (const form of forms) upload(engine, form.pc, authored[form.name]); upload(engine, 0x2000, authored.reader); upload(engine, 0x3000, authored.loop);
const a = compile(engine, forms.map(form => [form.pc, authored[form.name].length]), [], 'three MOV forms and old-EA aliases', forms.length);
const b = compile(engine, [[0x2000, 4]], ['read32'], 'reader sees live stored RAM'); const loop = compile(engine, [[0x3000, 10]], [], 'four-instruction store loop', 1);
for (const form of forms) {
  const start = initial(form.pc); start.registers[0] = 0x87654321; start.registers[1] = 2; start.registers[3] = DATA; start.registers[6] = form.address;
  for (const [offset, value] of [[-4, 0x11223344], [0, 0xaabbccdd], [4, 0x55667788]]) word(engine, form.address + offset, value);
  reset(engine, start); select(engine, form.pc, a); select(engine, form.pc + 1, a);
  const expected = afterA(start); run(engine, a, 8, expected, exit(3, 4), storeHelper(), form.name, 'initial', 1);
  observe(engine, form.address - 4, 4, 0x11223344); observe(engine, form.address, 4, form.value); observe(engine, form.address + 4, 4, 0x55667788);
  const read = copy(expected); read.registers[2] = form.value; read.eip = 0x2004; select(engine, 0x2000, b);
  run(engine, b, 3, read, exit(3, 2), helper(4, form.value), 'live reader true continuation', 'resume', 0, 1); stats.form_cases++;
}
assert.equal(engine.api.map(0x8000, 1, 2), 0); word(engine, 0x8000, 0xaabbccdd); const writeOnly = initial(0x1000); writeOnly.registers[0] = 0x10203040; writeOnly.registers[3] = 0x8000; writeOnly.registers[6] = 0x8000; reset(engine, writeOnly);
run(engine, a, 8, afterA(writeOnly), exit(3, 4), storeHelper(), 'write-only data does not require a Read4', 'initial', 1); assert.equal(engine.api.protect(0x8000, 1, 1), 0); observe(engine, 0x8000, 4, 0x10203040);
const writeOnlyRead = afterA(writeOnly); writeOnlyRead.registers[2] = 0x10203040; writeOnlyRead.eip = 0x2004; run(engine, b, 3, writeOnlyRead, exit(3, 2), helper(4, 0x10203040), 'read permission is applied after the completed store', 'resume', 0, 1);
for (const limbs of [{low: engine.low ^ 1}, {high: engine.high ^ 1}, {idLow: 0, idHigh: 0}, {idLow: 0xffffffff, idHigh: 0xffffffff}]) rawStore(engine, a, DATA, 0, 3, 'identity failure leaves RAM/arena unchanged', limbs);
observe(engine, DATA, 4, 0x12345678); guard(engine, a); guard(engine, b); guard(engine, loop);

assert.equal(engine.api.map(0xa000, 1, 3), 0); word(engine, 0xa000, 0x11111111); assert.equal(engine.api.protect(0xa000, 1, 1), 0);
assert.equal(engine.api.map(0xb000, 2, 3), 0); word(engine, 0xbffc, 0x11223344); word(engine, 0xc000, 0x55667788); assert.equal(engine.api.unmap(0xc000, 1), 0);
const faults = [{address: 0x9000, detail: 1, at: 0x9000, repair() { assert.equal(engine.api.map(0x9000, 1, 3), 0); word(engine, 0x9000, 0x99999999); }}, {address: 0xa000, detail: 2, at: 0xa000, repair() { assert.equal(engine.api.protect(0xa000, 1, 3), 0); word(engine, 0xa000, 0x99999999); }}, {address: 0xbfff, detail: 1, at: 0xc000, repair() { assert.equal(engine.api.map(0xc000, 1, 3), 0); word(engine, 0xc000, 0x55667788); }}, {address: 0xbfff, detail: 2, at: 0xc000, setup() { word(engine, 0xbffc, 0x11223344); word(engine, 0xc000, 0x55667788); assert.equal(engine.api.protect(0xc000, 1, 1), 0); }, repair() { assert.equal(engine.api.protect(0xc000, 1, 3), 0); }}, {address: 0xffffffff, detail: 3, at: 0xffffffff}];
for (const fault of faults) {
  fault.setup?.(); const start = initial(0x1000); start.registers[0] = 0x87654321; start.registers[3] = fault.address; reset(engine, start); const stopped = prefixed(start);
  run(engine, a, 8, stopped, exit(5, 1, fault.detail, fault.at, 4), storeHelper(fault.detail, fault.at), 'fault commits prefix and preserves MOV/flags', 'initial', 1); stats.faults++;
  if (fault.address === 0xa000) observe(engine, 0xa000, 4, 0x11111111);
  if (fault.address === 0xbfff) { observe(engine, 0xbffc, 4, 0x11223344); if (fault.detail === 2) observe(engine, 0xc000, 4, 0x55667788); }
  if (fault.repair) {
    fault.repair(); const expected = afterA(start); run(engine, a, 8, expected, exit(3, 3), storeHelper(), 'same CPU resumes repaired MOV without repeating prefix', 'resume', 1); stats.repairs++;
    if (fault.address === 0xbfff) { observe(engine, 0xbffc, 4, 0x21223344); observe(engine, 0xc000, 4, 0x55876543); } else observe(engine, fault.address, 4, 0x87654321);
  } else { run(engine, a, 8, stopped, exit(5, 0, 3, 0xffffffff, 4), storeHelper(3, 0xffffffff), 'overflow retry remains unretired', 'resume', 1); stats.faults++; }
  guard(engine, a); guard(engine, b);
}
assert.equal(engine.api.map(0xfffff000, 1, 3), 0); word(engine, 0xfffffff8, 0x10203040); word(engine, 0xfffffffc, 0x11223344);
const top = initial(0x1000); top.registers[3] = 0xfffffffc; top.registers[0] = 0xdeadbeef; reset(engine, top); run(engine, a, 8, afterA(top), exit(3, 4), storeHelper(), 'last valid Store4', 'initial', 1); observe(engine, 0xfffffff8, 4, 0x10203040); observe(engine, 0xfffffffc, 4, 0xdeadbeef);

const split = initial(0x1000); split.registers[0] = 0x12345678; split.registers[3] = DATA; reset(engine, split);
run(engine, a, 0, split, exit(1, 0), undefined, 'zero budget has no store', 'initial'); const prefix = prefixed(split);
run(engine, a, 1, prefix, exit(1, 1), undefined, 'budget at exact MOV interior', 'resume'); select(engine, 0x1001, a);
engine.view.setUint32(engine.base + 96, 1, true); run(engine, a, 0, prefix, exit(2, 0), undefined, 'cancel wins over zero budget before store', 'resume'); stats.cancellations++;
engine.view.setUint32(engine.base + 96, 0, true); const stored = at(prefix, 0x1003); run(engine, a, 1, stored, exit(1, 1), storeHelper(), 'exact store budget retires once', 'resume', 1);
word(engine, DATA, 0xfeedface); const incremented = copy(stored), inc = unary('inc', incremented.registers[1], incremented.eflags); incremented.registers[1] = inc.value; incremented.eflags = inc.flags; incremented.eip = 0x1004;
run(engine, a, 1, incremented, exit(1, 1), undefined, 'successor consumes current CPU', 'resume'); run(engine, a, 1, at(incremented, 0x2000), exit(1, 1), undefined, 'direct target wins exact branch budget', 'resume'); run(engine, a, 8, at(incremented, 0x2000), exit(3, 0), undefined, 'external PC never repeats retired MOV', 'resume');
const afterRead = at(incremented, 0x2004); afterRead.registers[2] = 0xfeedface; run(engine, b, 3, afterRead, exit(3, 2), helper(4, 0xfeedface), 'reader proves store was not repeated', 'resume', 0, 1);
const preset = initial(0x1000); reset(engine, preset, 1); run(engine, a, 8, preset, exit(2, 0), undefined, 'preset cancel skips prefix/store', 'initial'); stats.cancellations++;
const middle = initial(0x1002); reset(engine, middle); miss(engine, 0x1002); run(engine, a, 8, middle, exit(3, 0), undefined, 'middle instruction byte is not an entry', 'initial');

for (const [label, address] of [['arena-number', engine.base + TRANSFER], ['module-number', a.pointer]]) {
  const start = initial(0x1000); start.registers[0] = 0x87654321; start.registers[3] = address; reset(engine, start); const stopped = prefixed(start), linearBefore = refresh(engine).bytes.slice(address, address + 4);
  run(engine, a, 8, stopped, exit(5, 1, 1, address, 4), storeHelper(1, address), `${label} is a guest address, not an engine pointer`, 'initial', 1); stats.faults++;
  assert.equal(engine.api.map(address & 0xfffff000, 1, 3), 0); run(engine, a, 8, afterA(start), exit(3, 3), storeHelper(), `${label} separate guest page repair`, 'resume', 1); stats.repairs++;
  assert.deepEqual(refresh(engine).bytes.slice(address, address + 4), linearBefore); observe(engine, address, 4, 0x87654321); assert.deepEqual(refresh(engine).bytes.slice(a.pointer, a.pointer + a.length), a.bytes); stats.alias_cases++; assert.equal(engine.api.unmap(address & 0xfffff000, 1), 0);
}
function loopTrace(start, retired) {
  const n = start.registers[1], limit = 4 * n; assert.ok(n > 0 && retired <= limit); const value = copy(start), q = Math.floor(retired / 4), phase = retired % 4;
  value.registers[0] = Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(q + (phase >= 2 ? 1 : 0)))); value.registers[1] = n - q - (phase >= 3 ? 1 : 0);
  value.eip = retired === limit ? 0x300a : [0x3000, 0x3002, 0x3003, 0x3004][phase];
  if (phase === 2) value.eflags = unary('inc', Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(q))), start.eflags).flags;
  else if (phase === 3 || q > 0) value.eflags = unary('dec', n - q + (phase < 3 ? 1 : 0), start.eflags).flags;
  return value;
}
const algorithmRows = [];
for (const n of [1, 3, 5]) {
  const start = initial(0x3000); start.registers[0] = n === 3 ? 0xffffffff : 0x7ffffffe; start.registers[1] = n; start.registers[3] = DATA; word(engine, DATA, 0xaabbccdd); reset(engine, start);
  const schedules = n === 3 ? [1, 2, 9] : [4 * n + 1]; let retired = 0, calls = 0;
  for (const budget of schedules) {
    const next = Math.min(4 * n, retired + budget), stores = Math.ceil(next / 4) - Math.ceil(retired / 4), result = loopTrace(start, next), reason = next - retired === budget ? 1 : 3;
    run(engine, loop, budget, result, exit(reason, next - retired), stores ? storeHelper() : undefined, 'closed-form four-phase loop', retired === 0 ? 'initial' : 'resume', stores); calls++; retired = next;
    if (retired) observe(engine, DATA, 4, Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(Math.ceil(retired / 4) - 1))));
  }
  assert.equal(retired, 4 * n); algorithmRows.push({n, retired, calls, eax: loopTrace(start, retired).registers[0], ecx: 0, ram: Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(n - 1)))}); stats.algorithm_cases++; stats.algorithm_retired += retired;
}
assert.equal(engine.api.map(0x7000, 1, 7), 0);
const mixedUnits = [];
for (const [label, pc, hex, reads, expectedEdx, counters] of [['narrow-mixed', 0x7000, '89030fb6130fb70beb00', ['read8', 'read16'], 255, {8: 1, 16: 1}], ['wide-mixed', 0x7100, '89038b130fb70beb00', ['read32', 'read16'], 0x876580ff, {32: 1, 16: 1}]]) {
  const code = authoredCode(label, Buffer.from(hex, 'hex')); authored[label] = code; upload(engine, pc, code); const unit = compile(engine, [[pc, code.length]], reads, label, 1); mixedUnits.push(unit);
  const start = initial(pc); start.registers[0] = 0x876580ff; start.registers[3] = DATA; reset(engine, start); const expected = at(start, pc + code.length); expected.registers[2] = expectedEdx; expected.registers[1] = 0x80ff;
  run(engine, unit, 5, expected, exit(3, 4), helper(2, 0x80ff), 'store/read type and function index composition', 'initial', 1, counters); observe(engine, DATA, 4, 0x876580ff); stats.mixed_cases++;
}
close(engine, [a, b, loop, ...mixedUnits]);

const smcRows = [];
for (const kind of ['self', 'same-byte', 'whole-unit', 'shared-page', 'separate-B', 'self-RX']) {
  const e = fresh(), bpc = kind === 'shared-page' ? 0x1800 : 0x2000, destination = kind === 'whole-unit' ? 0x4000 : kind === 'separate-B' || kind === 'shared-page' ? bpc : 0x1008;
  const newWord = kind === 'self' ? 0x12345678 : destination === 0x1008 ? 0xaaaaaaaa : 0x0000eb90;
  const code = Buffer.from('47c70300000000b9aaaaaaaae900000000', 'hex'); code.writeUInt32LE(newWord, 3); code.writeInt32LE(bpc - 0x1011, 13);
  const control = Buffer.from('90eb00', 'hex'); authored[`smc-${kind}`] = authoredCode(`smc-${kind}`, code); authored[`smc-${kind}-control`] = authoredCode(`smc-${kind}-control`, control);
  assert.equal(e.api.map(0x1000, 1, 7), 0); if (bpc === 0x2000) assert.equal(e.api.map(bpc, 1, 7), 0); assert.equal(e.api.map(0x4000, 1, 7), 0); assert.equal(e.api.map(DATA, 1, 3), 0); word(e, DATA, 0x11223344);
  upload(e, 0x1000, code); upload(e, bpc, control); upload(e, 0x4000, control);
  if (kind === 'self-RX') assert.equal(e.api.protect(0x1000, 1, 5), 0);
  const oldA = compile(e, [[0x1000, code.length], [0x4000, 3]], [], `SMC ${kind} whole A`, 1), currentB = compile(e, [[bpc, 3]], [], `SMC ${kind} B`);
  const start = initial(0x1000); start.registers[3] = destination; reset(e, start); const stopped = prefixed(start, 0x1007);
  if (kind === 'self-RX') {
    stopped.eip = 0x1001; run(e, oldA, 8, stopped, exit(5, 1, 2, destination, 4), storeHelper(2, destination), 'RX self-store faults without invalidating A', 'initial', 1); stats.faults++; observe(e, destination, 4, 0xaaaaaaaa); guard(e, oldA); select(e, 0x1001, oldA); guard(e, currentB);
    smcRows.push({kind, destination, status: 0, reason: 5, retired: 1}); close(e, [oldA, currentB]); continue;
  }
  const invalidatesA = kind !== 'separate-B';
  if (invalidatesA) run(e, oldA, 2, stopped, exit(6, 2), storeHelper(), 'committed store invalidation wins exact budget before successor', 'initial', 1);
  else { const complete = at(stopped, bpc); complete.registers[1] = 0xaaaaaaaa; run(e, oldA, 8, complete, exit(3, 4), storeHelper(), 'separate B invalidation leaves executing A current', 'initial', 1); }
  observe(e, destination, 4, newWord); stats.smc_cases++; smcRows.push({kind, destination, status: invalidatesA ? 11 : 0, reason: invalidatesA ? 6 : 3, retired: invalidatesA ? 2 : 4});
  guard(e, oldA, invalidatesA ? 4 : 0); guard(e, currentB, kind === 'shared-page' || kind === 'separate-B' ? 4 : 0);
  if (invalidatesA) {
    miss(e, 0x1007, 4); miss(e, 0x4000, 4); rejected(e, oldA, 4, 'stale original A'); rawStore(e, oldA, DATA, 0, 4, 'stale bound helper preserves RAM/arena'); observe(e, DATA, 4, 0x11223344);
    const copied = new WebAssembly.Instance(new WebAssembly.Module(oldA.bytes.slice()), {env: {memory: e.memory}, ring3: {guard_resident: e.api.guard_resident, store_resident32: e.api.store_resident32}}); rejected(e, {...oldA, run: copied.exports.run}, 4, 'copied stale module retains old baked ID');
    const successor = compile(e, [[0x1007, 10]], [], `fresh ${kind} successor`); assert.ok(successor.id > oldA.id); select(e, 0x1007, successor);
    const continued = at(stopped, bpc); continued.registers[1] = kind === 'self' ? newWord : 0xaaaaaaaa; run(e, successor, 8, continued, exit(3, 2, 0, 0, 0, 1), undefined, 'fresh successor resumes without repeating retired store', 'resume');
    if (kind === 'shared-page') { miss(e, bpc, 4); const freshB = compile(e, [[bpc, 3]], [], 'fresh shared-page B'); select(e, bpc, freshB); run(e, freshB, 3, at(continued, bpc + 3), exit(3, 2, 0, 0, 0, 1), undefined, 'fresh B true continuation', 'resume'); }
    else { select(e, bpc, currentB); run(e, currentB, 3, at(continued, bpc + 3), exit(3, 2, 0, 0, 0, 1), undefined, 'separate current B true continuation', 'resume'); }
    guard(e, oldA, 4); guard(e, successor); close(e, [successor, currentB]);
  } else {
    const complete = at(stopped, bpc); complete.registers[1] = 0xaaaaaaaa; run(e, oldA, 8, complete, exit(3, 0), undefined, 'returning A does not repeat retired store', 'resume'); rejected(e, currentB, 4, 'only separately written B is stale');
    const freshB = compile(e, [[bpc, 3]], [], 'fresh separate B'); select(e, bpc, freshB); run(e, freshB, 3, at(complete, bpc + 3), exit(3, 2, 0, 0, 0, 1), undefined, 'fresh separate B continues from existing CPU', 'resume'); guard(e, oldA); close(e, [oldA, freshB]);
  }
}
function legacyCompile(e, blocks, gates, label) {
  descriptors(e, blocks); if (gates) { e.view.setUint32(e.base + TRANSFER + blocks.length * 8, blocks[0][0], true); e.view.setUint32(e.base + TRANSFER + blocks.length * 8 + 4, 77, true); }
  assert.equal(e.api[gates ? 'compile_with_gates' : 'compile'](blocks.length, ...(gates ? [1] : [])), 0); refresh(e);
  const pointer = e.api.module_ptr() >>> 0, length = e.api.module_len() >>> 0, generation = e.api.generation() >>> 0, bytes = e.bytes.slice(pointer, pointer + length), module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3: {guard: e.api.guard}}), filename = `legacy-${++stats.legacy_compiles}.wasm`; writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes);
  e.legacy = {pointer, length, generation, bytes, run: instance.exports.run, label}; noLegacy(e); return e.legacy;
}
const independent = fresh(); for (const page of [0x1000, 0x2000, 0x3000]) assert.equal(independent.api.map(page, 1, 7), 0); assert.equal(independent.api.map(DATA, 1, 3), 0);
authored.independent = authoredCode('independent', Buffer.from('478903eb00', 'hex')); authored.gate = authoredCode('gate', Buffer.from('0f0b', 'hex')); authored.replacement = authoredCode('replacement', Buffer.from('90eb00', 'hex'));
upload(independent, 0x1000, authored.independent); upload(independent, 0x2000, authored.replacement); upload(independent, 0x3000, authored.gate); upload(independent, 0x3100, authored.replacement);
const independentA = compile(independent, [[0x1000, 5]], [], 'A independent of legacy artifact', 1), independentB = compile(independent, [[0x2000, 3]], [], 'B independent of legacy artifact');
word(independent, DATA, 0x11223344); rawStore(independent, independentA, DATA, 0, 3, 'other engine key rejects bound helper', {low: engine.low, high: engine.high}); observe(independent, DATA, 4, 0x11223344);
const legacyGate = legacyCompile(independent, [[0x3000, 2]], true, 'legacy gate'); const gateState = initial(0x3000); reset(independent, gateState);
const gateBefore = arena(independent); assert.equal(legacyGate.run(independent.base, independent.base + 56, 1, independent.base + 96), 0); const gateWanted = gateBefore.slice(); gateWanted.set(exit(8, 0, 77, 0, 0, 3), 56); assert.deepEqual(arena(independent), gateWanted); stats.legacy_runs++; stats.full_arena_checks++;
const captureBefore = arena(independent); assert.equal(independent.api.capture_call(independent.low, independent.high, legacyGate.generation, 1, 0), 0); const captured = arena(independent), captureWanted = captureBefore.slice(); captureWanted.set(captured.subarray(TRANSFER, TRANSFER + 112), TRANSFER); assert.deepEqual(captured, captureWanted); stats.full_arena_checks++; stats.pending_captures++;
const token = new DataView(captured.buffer).getUint32(156, true); assert.ok(token > 0); rawStore(independent, independentA, DATA, 0, 12, 'pending call rejects store before helper/RAM mutation'); rejected(independent, independentA, 12, 'pending call rejects generated resident entry'); observe(independent, DATA, 4, 0x11223344);
select(independent, 0x1000, independentA); unchanged(independent, () => independent.api.abandon_call(independent.low, independent.high, token), 0, 'abandon pending without arena mutation'); stats.pending_abandons++;
legacyCompile(independent, [[0x3100, 3]], false, 'legacy replacement'); assert.equal(independent.legacy.generation, 2); guard(independent, independentA); guard(independent, independentB); select(independent, 0x1000, independentA);
const legacyWrite = initial(0x1000); legacyWrite.registers[3] = 0x3000; legacyWrite.registers[0] = 0x12345678; reset(independent, legacyWrite); independent.legacy.stale = true;
run(independent, independentA, 8, prefixed(legacyWrite, 0x1005), exit(3, 3), storeHelper(), 'legacy code invalidation does not invalidate A', 'initial', 1); observe(independent, 0x3000, 4, 0x12345678); guard(independent, independentA); guard(independent, independentB);
run(independent, independentA, 8, prefixed(legacyWrite, 0x1005), exit(3, 0), undefined, 'resident execution remains independent of stale legacy artifact', 'resume'); close(independent, [independentA, independentB]);

assert.equal(stats.mixed_cases, 2); assert.equal(stats.form_cases, 7); assert.equal(stats.alias_cases, 2); assert.equal(stats.algorithm_cases, 3); assert.equal(stats.algorithm_retired, 36); assert.equal(stats.smc_cases, 5);
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = ['engine/tests/process_resident_store_wasm.rs', 'engine/tests/fixtures/p2-resident-store/run.mjs', 'engine/tests/process_resident_store.rs', 'engine/tests/process_resident_indirect_wasm.rs', 'engine/tests/fixtures/p2-resident-indirect/run.mjs', 'engine/tests/process_resident_indirect.rs', 'engine/tests/process_resident_memory_wasm.rs', 'engine/tests/fixtures/p2-resident-memory/run.mjs', 'engine/tests/process_resident_memory.rs', 'engine/tests/process_resident_wasm.rs', 'engine/tests/fixtures/p2-resident-process/run.mjs', 'engine/tests/process_resident.rs', 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', ...productionPaths];
const provenance = {
  stats, actual_resident_runs: stats.canonical_runs + stats.rejected_runs, actual_resident_guard_entries: stats.canonical_runs + stats.rejected_runs + stats.successful_guards + stats.failed_guards, bound_store_identity_checks: stats.guest_store32 + stats.raw_store_rejected,
  guest_helper_calls_from_authored_execution: stats.guest_store32 + stats.guest_read8 + stats.guest_read16 + stats.guest_read32, raw_bound_store_probes: stats.raw_store_success + stats.raw_store_rejected, host_observation_reads: stats.host_reads,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  authored: Object.fromEntries(Object.entries(authored).map(([name, bytes]) => [name, bytes.toString('hex')])), unit_bindings: units.map(unit => ({label: unit.label, id: unit.id.toString(), pointer: unit.pointer, length: unit.length, filename: unit.filename, reads: unit.reads, stores: unit.stores})), algorithm_rows: algorithmRows, smc_rows: smcRows, metadata_observations: metadata,
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_store_wasm', '--', '--nocapture']},
  claim: 'actual Node engine-Wasm resident MOV32 89/A3/C7 stores through direct Wasm guard/store/read exports bound to the same engine memory. Independently decoded immutable six-argument store binding, exact import signatures, no legacy dependency, old EA/register/immediate/ESP/SIB aliases, full4236 arena and Helperv1/Exitv2, target-only RAM/neighbours, atomic guest faults and permission/map repair, current-CPU budget/cancel/interior continuations, no repeated retired store, literal/BigInt flags and bounded closed-form 4*n arithmetic. Self/same-byte/other-block/shared-page executing-unit invalidation commits once before successor; separate B and legacy invalidation keep A current; stale/copied/fresh/closed and pending Busy probes. Counts authored-path derived, not instrumentation. No ISA interpreter, product installer/table, callback/asynchronous cancellation/concurrency, narrow/read-modify-write/stack/CALL/RET stores, browser/performance/game/full P2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2)); console.log(JSON.stringify({...provenance, sources: Object.keys(provenance.sources).length, metadata_observations: metadata.length, artifacts: outputDir}));
