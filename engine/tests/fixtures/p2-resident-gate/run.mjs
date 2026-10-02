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
const SIZE = 4236, TRANSFER = 140, DATA = 0xa000, STACK = 0xc100;
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'generation', 'module_ptr', 'module_len', 'guard_resident', 'compile_resident', 'compile_resident_with_gates', 'compile_with_gates', 'capture_call', 'resident_module', 'find_resident', 'read32', 'write32', 'store_resident32'];
const stats = {engine_instances: 0, compiled_units: 0, canonical_runs: 0, rejected_runs: 0, full_arena_checks: 0, metadata_checks: 0, successful_guards: 0, failed_guards: 0, find_success: 0, find_failure: 0, continuation_initial_calls: 0, continuation_resumes: 0, gate_cases: 0, call_cases: 0, smc_cases: 0, same_byte_invalidation_cases: 0, bound_store_sites: 0, algorithm_cases: 0, algorithm_retired: 0, faults: 0, repairs: 0, cancellations: 0, compile_failures: 0, capture_failures: 0, host_reads: 0, host_writes: 0, guest_read32: 0, guest_store32: 0};
const units = [], artifacts = {}, metadata = [], authored = {}, algorithmRows = [], smcRows = [];

function refresh(engine) {
  if (engine.buffer !== engine.memory.buffer) { engine.buffer = engine.memory.buffer; engine.bytes = new Uint8Array(engine.buffer); engine.view = new DataView(engine.buffer); }
  return engine;
}

function arena(engine) { return refresh(engine).bytes.slice(engine.base, engine.base + SIZE); }

function upload(engine, address, bytes) {
  refresh(engine).bytes.set(bytes, engine.base + TRANSFER);
  assert.equal(engine.api.upload(address, bytes.length), 0); refresh(engine);
}

function descriptors(engine, blocks, gates = []) {
  refresh(engine); [...blocks, ...gates].forEach(([pc, length], index) => { engine.view.setUint32(engine.base + TRANSFER + index * 8, pc, true); engine.view.setUint32(engine.base + TRANSFER + index * 8 + 4, length, true); });
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

function noLegacy(engine) { if (engine.legacy) return; assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_ptr(), 0); assert.equal(engine.api.module_len(), 0); }
function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const key = 0x9a889abcc0000000n + BigInt(++stats.engine_instances);
  const engine = refresh({instance, api, memory: instance.exports.memory, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n)});
  assert.equal(api.compile_resident_with_gates.length, 2); assert.equal(api.guard_resident.length, 7); assert.equal(api.store_resident32.length, 6);
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
  stats.metadata_checks++; metadata.push({label, key: engine.key.toString(), id: id.toString(), pointer, length, sha256: hash(bytes)}); return {id, low, high, pointer, length, bytes};
}
function compile(engine, blocks, gates, reads, label, stores = []) {
  descriptors(engine, blocks, gates); const before = arena(engine); assert.equal(engine.api.compile_resident_with_gates(blocks.length, gates.length), 0, label);
  const selected = record(engine, before, label), module = new WebAssembly.Module(selected.bytes);
  const helpers = [...reads.filter(name => name === 'read32'), ...(stores.length ? ['store_resident32'] : []), ...reads.filter(name => name !== 'read32')];
  const expected = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard_resident', kind: 'function'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), expected, 'exact direct bound guard/read/store imports'); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const imports = {guard_resident: engine.api.guard_resident, ...Object.fromEntries(reads.map(name => [name, engine.api[name]])), ...(stores.length ? {store_resident32: engine.api.store_resident32} : {})};
  boundStores(selected.bytes, engine, selected.id, stores, reads.includes('read32'));
  const instance = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: imports}); assert.equal(instance.exports.run.length, 4);
  const filename = `unit-${++stats.compiled_units}.wasm`; writeFileSync(join(outputDir, filename), selected.bytes); artifacts[filename] = hash(selected.bytes);
  const unit = {...selected, key: engine.key.toString(), module, instance, run: instance.exports.run, blocks, gates, reads, stores, filename, label}; units.push(unit); return unit;
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
function rejected(engine, unit, status, label, pointers = [engine.base, engine.base + 56, engine.base + 96]) {
  unchanged(engine, () => unit.run(pointers[0], pointers[1], 4, pointers[2]), status, label); stats.rejected_runs++;
}
function initial(eip) {
  return {registers: [0x7fffffff, 0x13579bdf, 0x23456789, 0x3456789a, STACK, 0x56789abc, DATA, 15], eip, eflags: 0xcd7};
}
function copy(value) { return {...value, registers: [...value.registers]}; }
function reset(engine, value, cancel = 0) {
  refresh(engine); header(engine.bytes, engine.view, engine.base, 'R3ST', 56);
  value.registers.forEach((r, i) => engine.view.setUint32(engine.base + 16 + i * 4, r, true)); engine.view.setUint32(engine.base + 48, value.eip, true); engine.view.setUint32(engine.base + 52, value.eflags, true);
  engine.bytes.fill(0xa5, engine.base + 56, engine.base + 96); engine.view.setUint32(engine.base + 96, cancel, true); engine.bytes.fill(0x5a, engine.base + 100, engine.base + 140);
}
function arithmetic(kind, input, oldFlags) {
  const a = BigInt(input), value = Number(BigInt.asUintN(32, kind === 'inc' ? a + 1n : a - 1n)); let flags = (oldFlags & 0x401) | 2;
  if (kind === 'inc' ? (a & 15n) === 15n : (a & 15n) === 0n) flags |= 0x10;
  if (kind === 'inc' ? a === 0x7fffffffn : a === 0x80000000n) flags |= 0x800;
  if (value === 0) flags |= 0x40; if (value >= 0x80000000) flags |= 0x80;
  let ones = 0; for (let byte = value & 255; byte; byte >>>= 1) ones += byte & 1; if (ones % 2 === 0) flags |= 4;
  return {value, flags};
}
for (const [kind, input, oldFlags, value, flags] of [['inc', 15, 0xcd7, 16, 0x413], ['inc', 0xffffffff, 2, 0, 0x56], ['dec', 1, 0xcd7, 0, 0x447], ['dec', 0x80000000, 2, 0x7fffffff, 0x816]]) assert.deepEqual(arithmetic(kind, input, oldFlags), {value, flags}, 'literal prefix/loop arithmetic flag anchors');
function exit(reason, retired, detail = 0, address = 0, width = 0, version = 3, access = 2) {
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
  stats.guest_read32 += reads; stats.guest_store32 += stores;
}
function word(engine, address, value) { assert.equal(engine.api.write32(address, value), 0); stats.host_writes++; refresh(engine); }
function observe(engine, address, width, expected) {
  const before = arena(engine); assert.equal(engine.api[`read${width * 8}`](address), 0); stats.host_reads++;
  const wanted = before.slice(); wanted.set(helper(width, expected), 100); assert.deepEqual(arena(engine), wanted, 'host observation only replaces helper40'); stats.full_arena_checks++;
}
function boundStores(bytes, engine, id, operands, read) {
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
  assert.ok(body); assert.deepEqual(signatures[0], {args: Array(4).fill(0x7f), returns: [0x7f]});
  assert.deepEqual(signatures[importTypes.get('guard_resident')], {args: Array(7).fill(0x7f), returns: [0x7f]});
  if (read) assert.deepEqual(signatures[importTypes.get('read32')], {args: [0x7f], returns: [0x7f]}); else assert.equal(importTypes.has('read32'), false);
  if (!operands.length) { assert.equal(importTypes.has('store_resident32'), false); return; }
  const index = read ? 2 : 1;
  assert.deepEqual(signatures[importTypes.get('store_resident32')], {args: Array(6).fill(0x7f), returns: [0x7f]});
  const limbs = [engine.low, engine.high, Number(id & 0xffffffffn), Number(id >> 32n)]; let found = 0;
  for (let offset = 0; offset < body.length; offset++) {
    if (body[offset] !== 0x41) continue;
    const c = {i: offset}, constants = []; let match = true;
    try { for (let j = 0; j < 4; j++) { if (body[c.i++] !== 0x41) { match = false; break; } constants.push(sleb32(body, c)); } } catch { match = false; }
    if (!match || !constants.every((value, j) => value === limbs[j]) || body[c.i] !== 0x20 || body[c.i + 1] !== 26) continue;
    c.i += 2; const opcode = body[c.i++], operand = opcode === 0x20 ? {local: uleb(body, c)} : (assert.equal(opcode, 0x41), {immediate: sleb32(body, c)});
    assert.deepEqual(operand, operands[found], 'CALL stores the exact return-PC immediate while RESULT18 retains its target');
    assert.equal(body[c.i++], 0x10); assert.equal(uleb(body, c), index); assert.equal(operand.local, undefined); found++;
  }
  assert.equal(found, operands.length, 'each CALL Store4 has immutable identity, ADDRESS26 and return-PC immediate'); stats.bound_store_sites += found;
}
function miss(engine, pc, status = 17) { unchanged(engine, () => engine.api.find_resident(pc), status, 'finder failure'); stats.find_failure++; }
function at(value, pc) { const next = copy(value); next.eip = pc; return next; }
function prefixed(start) { const value = copy(start), result = arithmetic('inc', value.registers[7], value.eflags); value.registers[7] = result.value; value.eflags = result.flags; value.eip = start.eip + 1; return value; }
function called(start, target) { const value = at(start, target); value.registers[4] = (start.registers[4] - 4) >>> 0; return value; }
function storeHelper(detail = 0, address = 0) { return helper(4, 0, detail, address, 2); }
function authoredCode(name, bytes) { assert.equal(authored[name], undefined); authored[name] = Buffer.from(bytes); const filename = `${name}.x86`; writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes); }
function close(engine) { unchanged(engine, () => engine.api.close(), 0, 'close preserves full arena'); }
function compileFailure(engine, blocks, gates, status, label, counts = [blocks.length, gates.length]) { descriptors(engine, blocks, gates); unchanged(engine, () => engine.api.compile_resident_with_gates(...counts), status, label); stats.compile_failures++; }
function ram(engine, destination, target) {
  const words = [[DATA - 4, 0x11223344], [DATA, target], [DATA + 4, 0x55667788], [destination - 4, 0x12345678], [destination, 0x89abcdef], [destination + 4, 0xabcdef01]];
  for (const [address, value] of words) word(engine, address, value); return words;
}
function observeWords(engine, words) { for (const [address, value] of words) observe(engine, address, 4, value); }
function copied(engine, unit) { const instance = new WebAssembly.Instance(new WebAssembly.Module(unit.bytes.slice()), {env: {memory: engine.memory}, ring3: {guard_resident: engine.api.guard_resident, ...Object.fromEntries(unit.reads.map(name => [name, engine.api[name]])), ...(unit.stores.length ? {store_resident32: engine.api.store_resident32} : {})}}); return {...unit, run: instance.exports.run}; }
function directBytes(pc, target) { const bytes = Buffer.alloc(5); bytes[0] = 0xe8; bytes.writeInt32LE(target - (pc + 5), 1); return bytes; }

const marker = Buffer.from('0f0b', 'hex'), loopCode = Buffer.from('8d40034975fa', 'hex');
const engine = fresh();
for (const [address, pages, permissions] of [[0x1000, 5, 7], [0x9000, 2, 3], [0xc000, 1, 3]]) assert.equal(engine.api.map(address, pages, permissions), 0);
const gateEntries = [[0x1000, 17], [0x1100, 0x80000001], [0x1200, 0xffffffff]];
for (const [pc, id] of gateEntries) { upload(engine, pc, marker); authoredCode(`gate-${id}`, marker); }
upload(engine, 0x2000, Buffer.from('470f0b', 'hex')); authoredCode('prefix-gate', Buffer.from('470f0b', 'hex'));
const callForms = [{pc: 0x3000, target: 0x3100, bytes: directBytes(0x3000, 0x3100), kind: 'direct'}, {pc: 0x3400, target: 0x4000, bytes: directBytes(0x3400, 0x4000), kind: 'direct'}, {pc: 0x3200, bytes: Buffer.from('ffd0', 'hex'), kind: 'register'}, {pc: 0x3300, bytes: Buffer.from('ff13', 'hex'), kind: 'memory'}];
for (const form of callForms) { upload(engine, form.pc, form.bytes); authoredCode(`call-${form.pc}`, form.bytes); }
for (const pc of [0x3100, 0x4000]) { upload(engine, pc, marker); authoredCode(`call-gate-${pc}`, marker); }
upload(engine, 0x5000, Buffer.concat([loopCode, marker])); authoredCode('three-phase-loop-gate', Buffer.concat([loopCode, marker]));
assert.equal(engine.api.protect(0x1000, 1, 4), 0);
const gates = compile(engine, gateEntries.map(([pc]) => [pc, 2]), gateEntries, [], 'three numeric gate IDs');
const prefix = compile(engine, [[0x2000, 1], [0x2001, 2]], [[0x2001, 23]], [], 'ordinary prefix and gate');
const calls = compile(engine, [...callForms.map(form => [form.pc, form.bytes.length]), [0x3100, 2]], [[0x3100, 31]], ['read32'], 'all CALL forms and same-unit gate', callForms.map(form => ({immediate: form.pc + form.bytes.length})));
const separate = compile(engine, [[0x4000, 2]], [[0x4000, 41]], [], 'separate gate');
const loop = compile(engine, [[0x5000, 6], [0x5006, 2]], [[0x5006, 51]], [], 'returning three-phase loop and gate');
for (const [pc, id] of gateEntries) {
  const start = initial(pc); start.registers[4] = 0xdead0000; reset(engine, start); select(engine, pc, gates); run(engine, gates, 8, start, exit(8, 0, id), undefined, 'gate detail is the exact unsigned descriptor ID', 'initial'); stats.gate_cases++;
  run(engine, gates, 8, start, exit(8, 0, id), undefined, 'repeat gate consumes no guest instruction or budget', 'resume');
  run(engine, gates, 0, start, exit(1, 0), undefined, 'zero budget precedes gate', 'resume'); engine.view.setUint32(engine.base + 96, 1, true);
  run(engine, gates, 0, start, exit(2, 0), undefined, 'cancel precedes zero budget and gate', 'resume'); stats.cancellations++; engine.view.setUint32(engine.base + 96, 0, true);
  for (const unknown of [pc + 1, pc + 2]) { const value = initial(unknown); reset(engine, value); miss(engine, unknown); run(engine, gates, 8, value, exit(3, 0), undefined, 'gate interior and end are not executable starts', 'initial'); }
}
const prefixStart = initial(0x2000), prefixDone = prefixed(prefixStart); reset(engine, prefixStart); select(engine, 0x2000, prefix); select(engine, 0x2001, prefix);
run(engine, prefix, 8, prefixDone, exit(8, 1, 23), undefined, 'ordinary prefix retires while gate remains unretired', 'initial'); run(engine, prefix, 8, prefixDone, exit(8, 0, 23), undefined, 'prefix cannot repeat after a numeric stop', 'resume');
reset(engine, prefixStart); run(engine, prefix, 1, prefixDone, exit(1, 1), undefined, 'exact prefix budget suppresses adjacent gate', 'initial'); run(engine, prefix, 8, prefixDone, exit(8, 0, 23), undefined, 'positive resume observes gate without prefix replay', 'resume');
reset(engine, initial(0x1000)); guard(engine, gates); unchanged(engine, () => engine.api.guard_resident(engine.low ^ 1, engine.high, gates.low, gates.high, 0xffffffff, 0xffffffff, 0xffffffff), 3, 'wrong key precedes invalid pointers'); stats.failed_guards++;
unchanged(engine, () => engine.api.guard_resident(engine.low, engine.high, 0, 0, 0xffffffff, 0xffffffff, 0xffffffff), 3, 'invalid unit ID precedes bad pointer validation'); stats.failed_guards++;
rejected(engine, gates, 1, 'current gate still validates exact canonical arena pointers', [0xffffffff, 0xffffffff, 0xffffffff]);
compileFailure(engine, [[0x1000, 2]], [[0x1000, 17]], 7, 'duplicate current gate start rejects publication');
compileFailure(engine, [[0x1000, 2]], [[0x1000, 0]], 10, 'zero gate ID rejects before guest decoding');
compileFailure(engine, [[0x1000, 2]], [], 10, 'unregistered UD2 keeps ordinary decode rejection');
compileFailure(engine, [[0x1000, 2]], [[0x1000, 17]], 7, 'invalid ingress count rejects before transfer reads', [0, 0]);
for (const form of callForms) for (const target of form.target === undefined ? [0x3100, 0x4000] : [form.target]) {
  const gateUnit = target === 0x3100 ? calls : separate, id = target === 0x3100 ? 31 : 41, start = initial(form.pc); start.registers[0] = target; start.registers[3] = DATA;
  let words = ram(engine, STACK - 4, target); reset(engine, start); const stopped = called(start, target), reads = Number(form.kind === 'memory');
  run(engine, calls, 8, stopped, exit(target === 0x3100 ? 8 : 3, 1, target === 0x3100 ? id : 0), storeHelper(), 'CALL with remaining fuel commits before same-unit gate or separate NeedCode', 'initial', 1, reads); words[4][1] = form.pc + form.bytes.length; observeWords(engine, words); select(engine, target, gateUnit);
  run(engine, gateUnit, 8, stopped, exit(8, 0, id), undefined, 'selected gate cannot repeat CALL or overwrite return word', 'resume'); observeWords(engine, words);
  words = ram(engine, STACK - 4, target); reset(engine, start); run(engine, calls, 1, stopped, exit(1, 1), storeHelper(), 'exact CALL budget precedes gate and commits once', 'initial', 1, reads); words[4][1] = form.pc + form.bytes.length; observeWords(engine, words);
  if (target === 0x4000) run(engine, calls, 8, stopped, exit(3, 0), undefined, 'separate caller at committed gate target cannot repeat CALL', 'resume');
  select(engine, target, gateUnit); run(engine, gateUnit, 8, stopped, exit(8, 0, id), undefined, 'positive gate resume after exact CALL budget', 'resume'); observeWords(engine, words); stats.call_cases++;
}
const faultStart = initial(0x3300); faultStart.registers[3] = DATA; const faultWords = ram(engine, STACK - 4, 0x3100); assert.equal(engine.api.protect(0xa000, 1, 2), 0); reset(engine, faultStart);
run(engine, calls, 8, faultStart, exit(5, 0, 2, DATA, 4, 3, 1), helper(4, 0, 2, DATA), 'memory CALL read failure commits no target/stack/flags', 'initial', 0, 1); stats.faults++;
assert.equal(engine.api.protect(0xa000, 1, 3), 0); word(engine, DATA, 0x4000); faultWords[1][1] = 0x4000; const repaired = called(faultStart, 0x4000);
run(engine, calls, 1, repaired, exit(1, 1), storeHelper(), 'repair reads current changed pointer before storing return PC', 'resume', 1, 1); faultWords[4][1] = 0x3302; observeWords(engine, faultWords); run(engine, separate, 8, repaired, exit(8, 0, 41), undefined, 'repaired pointer reaches separate gate without reread', 'resume'); stats.repairs++;
const writeFault = initial(0x3200); writeFault.registers[0] = 0x3100; const writeWords = ram(engine, STACK - 4, 0x3100); assert.equal(engine.api.protect(0xc000, 1, 1), 0); reset(engine, writeFault);
run(engine, calls, 8, writeFault, exit(5, 0, 2, STACK - 4, 4), storeHelper(2, STACK - 4), 'return-word write failure preserves captured target and old ESP/full flags', 'initial', 1); observeWords(engine, writeWords); stats.faults++;
assert.equal(engine.api.protect(0xc000, 1, 3), 0); const writeRepaired = called(writeFault, 0x3100); run(engine, calls, 8, writeRepaired, exit(8, 1, 31), storeHelper(), 'store permission repair commits CALL once then same-unit Gate', 'resume', 1); writeWords[4][1] = 0x3202; observeWords(engine, writeWords); stats.repairs++;
for (const n of [1, 2, 5]) {
  const start = initial(0x5000); start.registers[0] = 0xfffffffd; start.registers[1] = n; start.eflags = 0x403; reset(engine, start); const budgets = n === 2 ? [1, 1, 2, 1, 1] : [3 * n]; let retired = 0, invocation = 0;
  for (const budget of budgets) {
    const next = Math.min(3 * n, retired + budget), cycles = Math.floor(next / 3), phase = next % 3, expected = copy(start);
    expected.registers[0] = Number(BigInt.asUintN(32, BigInt(start.registers[0]) + 3n * BigInt(cycles + Number(phase > 0)))); expected.registers[1] = n - cycles - Number(phase === 2);
    expected.eip = next === 3 * n ? 0x5006 : [0x5000, 0x5003, 0x5004][phase]; const decrements = cycles + Number(phase === 2); if (decrements) expected.eflags = arithmetic('dec', n - decrements + 1, start.eflags).flags;
    run(engine, loop, budget, expected, exit(1, next - retired), undefined, 'three-phase closed-form loop resumes at exact instruction boundaries', invocation++ ? 'resume' : 'initial'); retired = next;
  }
  const done = at(start, 0x5006); done.registers[0] = Number(BigInt.asUintN(32, BigInt(start.registers[0]) + 3n * BigInt(n))); done.registers[1] = 0; done.eflags = 0x447;
  assert.deepEqual([n, done.registers[0], retired], n === 1 ? [1, 0, 3] : n === 2 ? [2, 3, 6] : [5, 12, 15]); select(engine, 0x5006, loop); run(engine, loop, 8, done, exit(8, 0, 51), undefined, 'exact loop budget is followed by unretired gate stop', 'resume');
  algorithmRows.push({n, initial_eax: start.registers[0], eax: done.registers[0], ecx: 0, flags: 0x447, retired, calls: invocation + 1}); stats.algorithm_cases++; stats.algorithm_retired += retired;
}
close(engine); rejected(engine, gates, 5, 'closed instance rejects gate before state or exit writes', [0xffffffff, 0xffffffff, 0xffffffff]); miss(engine, 0x1000, 5);

for (const order of ['ordinary-first', 'gate-first']) {
  const e = fresh(), mov = Buffer.from('b80f0b9090', 'hex'); assert.equal(e.api.map(0x1000, 1, 7), 0); upload(e, 0x1000, mov); authoredCode(`${order}-overlap`, mov);
  let ordinary, gate;
  if (order === 'ordinary-first') { ordinary = compile(e, [[0x1000, 5]], [], [], 'ordinary MOV exact start'); gate = compile(e, [[0x1001, 2]], [[0x1001, 71]], [], 'gate inside immediate has a distinct executable start'); }
  else { gate = compile(e, [[0x1001, 2]], [[0x1001, 71]], [], 'gate exact start first'); ordinary = compile(e, [[0x1000, 5]], [], [], 'ordinary MOV around current gate'); }
  select(e, 0x1000, ordinary); select(e, 0x1001, gate); miss(e, 0x1002); compileFailure(e, [[0x1001, 2]], [[0x1001, 72]], 7, 'current gate/gate exact-start collision fails atomically');
  const start = initial(0x1000), done = at(start, 0x1005); done.registers[0] = 0x90900b0f; reset(e, start); run(e, ordinary, 8, done, exit(3, 1, 0, 0, 0, 1), undefined, 'ordinary decode keeps full overlapping immediate and Exitv1', 'initial');
  const stopped = initial(0x1001); reset(e, stopped); run(e, gate, 8, stopped, exit(8, 0, 71), undefined, 'registered overlapping marker is its own exact stop', 'initial'); close(e);
}

for (const mutation of ['same-marker', 'protect', 'unmap-remap', 'cross-first', 'cross-second']) {
  const e = fresh(), cross = mutation.startsWith('cross'), gatePC = cross ? 0x1fff : 0x1200, blocks = [[0x1000, 1], [gatePC, 2]], gateID = cross ? 0xfffffffe : 81;
  assert.equal(e.api.map(0x1000, cross ? 2 : 1, 7), 0); assert.equal(e.api.map(0x4000, 1, 7), 0); assert.equal(e.api.map(0x9000, 2, 3), 0);
  upload(e, 0x1000, Buffer.from('90', 'hex')); upload(e, gatePC, marker); upload(e, 0x4000, marker); authoredCode(`${mutation}-ordinary`, Buffer.from('90', 'hex')); authoredCode(`${mutation}-A-marker`, marker); authoredCode(`${mutation}-B-marker`, marker);
  const a = compile(e, blocks, [[gatePC, gateID]], [], `${mutation} ordinary and marker unit`), b = compile(e, [[0x4000, 2]], [[0x4000, 82]], [], `${mutation} isolated B`);
  const bStart = initial(0x4000); reset(e, bStart); run(e, b, 8, bStart, exit(8, 0, 82), undefined, 'separate B actual gate begins as current', 'initial');
  const words = [[DATA - 4, 0x11223344], [DATA, 0x89abcdef], [DATA + 4, 0x55667788]]; for (const [address, value] of words) word(e, address, value);
  const start = initial(gatePC); reset(e, start); select(e, gatePC, a); run(e, a, 8, start, exit(8, 0, gateID), undefined, 'whole ordinary-plus-gate unit initially stops at exact marker', 'initial');
  word(e, DATA, 0x76543210); words[1][1] = 0x76543210; guard(e, a); run(e, a, 8, start, exit(8, 0, gateID), undefined, 'data-only changes retain marker/ordinary snapshots', 'resume'); observeWords(e, words);
  if (mutation === 'same-marker') { word(e, gatePC, 0x00000b0f); stats.same_byte_invalidation_cases++; }
  else if (mutation === 'protect') assert.equal(e.api.protect(0x1000, 1, 5), 0);
  else if (mutation === 'unmap-remap') { assert.equal(e.api.unmap(0x1000, 1), 0); assert.equal(e.api.map(0x1000, 1, 7), 0); }
  else { word(e, mutation === 'cross-first' ? 0x1000 : 0x2000, mutation === 'cross-first' ? 0x90 : 0x0b); stats.same_byte_invalidation_cases++; }
  guard(e, a, 4); rejected(e, a, 4, 'stale whole-unit marker snapshot precedes invalid pointers', [0xffffffff, 0xffffffff, 0xffffffff]); rejected(e, copied(e, a), 4, 'copied bytes keep the original stale baked identity', [0xffffffff, 0xffffffff, 0xffffffff]); miss(e, gatePC, 4); guard(e, b); select(e, 0x4000, b);
  if (mutation === 'protect') assert.equal(e.api.protect(0x1000, 1, 7), 0); upload(e, 0x1000, Buffer.from('90', 'hex')); upload(e, gatePC, marker);
  const current = compile(e, blocks, [[gatePC, gateID]], [], `${mutation} fresh marker replacement`); assert.ok(current.id > a.id); select(e, gatePC, current); run(e, current, 8, start, exit(8, 0, gateID), undefined, 'fresh ID stops at unchanged committed PC without host CPU repair', 'resume'); observeWords(e, words);
  close(e); guard(e, current, 5); rejected(e, current, 5, 'closed copied or current gate cannot write arena', [0xffffffff, 0xffffffff, 0xffffffff]);
}

const smcCases = [{name: 'self', kind: 'direct', target: 'self'}, {name: 'whole', kind: 'memory', target: 'whole'}, {name: 'shared', kind: 'register', target: 'shared'}, {name: 'separate', kind: 'memory', target: 'separate'}, {name: 'same-byte', kind: 'memory', target: 'self', same: true}, {name: 'marker-write', kind: 'memory', target: 'self', marker: true}];
for (const fixture of smcCases) {
  const e = fresh(), target = fixture.target === 'separate' ? 0x2000 : 0x1800, instruction = fixture.kind === 'direct' ? directBytes(0x1000, target) : Buffer.from(fixture.kind === 'register' ? 'ffd0' : 'ff13', 'hex'), returnPC = 0x1000 + instruction.length;
  const ownTarget = fixture.target === 'self' || fixture.target === 'whole', aGates = ownTarget ? [[target, 91]] : [[0x4000, 93]], aBlocks = [[0x1000, instruction.length], ...aGates.map(([pc]) => [pc, 2])]; if (fixture.target === 'whole') { aGates.push([0x4000, 93]); aBlocks.push([0x4000, 2]); }
  const destination = fixture.marker ? target : fixture.same ? target + 8 : fixture.target === 'self' ? 0x1008 : fixture.target === 'whole' ? 0x4008 : target + 8, oldWord = fixture.marker ? 0x00000b0f : fixture.same ? returnPC : 0x13579bdf;
  for (const [address, pages, permissions] of [[0x1000, 2, 7], [0x4000, 1, 7], [0x9000, 2, 3]]) assert.equal(e.api.map(address, pages, permissions), 0);
  upload(e, 0x1000, instruction); upload(e, target, marker); upload(e, 0x4000, marker); authoredCode(`${fixture.name}-CALL`, instruction); authoredCode(`${fixture.name}-target-marker`, marker); authoredCode(`${fixture.name}-other-marker`, marker);
  word(e, destination, oldWord); for (const [address, value] of [[DATA - 4, 0x11223344], [DATA, target], [DATA + 4, 0x55667788]]) word(e, address, value);
  const watched = [destination - 4, destination, destination + 4, DATA - 4, DATA, DATA + 4], image = new Map(); for (const address of watched) for (let i = 0; i < 4; i++) image.set(address + i, 0);
  for (const [pc, bytes] of [[0x1000, instruction], [target, marker], [0x4000, marker]]) for (let i = 0; i < bytes.length; i++) image.set(pc + i, bytes[i]);
  function modelWord(address, value) { for (let i = 0; i < 4; i++) image.set(address + i, (value >>> (8 * i)) & 255); }
  function modelValue(address) { let value = 0n; for (let i = 0; i < 4; i++) value |= BigInt(image.get(address + i) ?? 0) << BigInt(8 * i); return Number(value); }
  for (const [address, value] of [[destination, oldWord], [DATA - 4, 0x11223344], [DATA, target], [DATA + 4, 0x55667788]]) modelWord(address, value);
  const reads = fixture.kind === 'memory' ? ['read32'] : [], a = compile(e, aBlocks, aGates, reads, `${fixture.name} gate-bearing caller`, [{immediate: returnPC}]), b = ownTarget ? undefined : compile(e, [[target, 2]], [[target, 91]], [], `${fixture.name} separate gate B`);
  const start = initial(0x1000); start.registers[4] = destination + 4; start.registers[0] = target; start.registers[3] = DATA; reset(e, start); const stopped = called(start, target), staleA = fixture.target !== 'separate';
  run(e, a, 1, stopped, exit(staleA ? 6 : 1, 1), storeHelper(), 'return word/target/ESP commit before Store11, budget or Gate', 'initial', 1, Number(fixture.kind === 'memory')); modelWord(destination, returnPC); for (const address of watched) observe(e, address, 4, modelValue(address)); stats.smc_cases++; if (fixture.same) stats.same_byte_invalidation_cases++;
  guard(e, a, staleA ? 4 : 0); if (b) guard(e, b, 4); const staleTarget = ownTarget || fixture.target === 'shared' || fixture.target === 'separate'; assert.ok(staleTarget); miss(e, target, 4);
  if (staleA) { rejected(e, a, 4, 'stale gate-bearing caller rejects before replaying committed CALL', [0xffffffff, 0xffffffff, 0xffffffff]); rejected(e, copied(e, a), 4, 'copied stale caller keeps its unit-bound gate snapshot'); }
  else run(e, a, 8, stopped, exit(3, 0), undefined, 'separate current caller at gate target cannot repeat CALL or return store', 'resume');
  if (b) rejected(e, b, 4, 'modified gate B rejects its immutable original identity');
  if (fixture.marker) { compileFailure(e, [[target, 2]], [[target, 91]], 10, 'overwritten marker is InvalidGate before fresh publication'); upload(e, target, marker); image.set(target, 0x0f); image.set(target + 1, 0x0b); }
  const current = compile(e, [[target, 2]], [[target, 91]], [], `${fixture.name} fresh gate after committed CALL`); assert.ok(current.id > (b ?? a).id); select(e, target, current);
  run(e, current, 8, stopped, exit(8, 0, 91), undefined, 'fresh gate stops at already committed target without CALL replay', 'resume'); run(e, current, 8, stopped, exit(8, 0, 91), undefined, 'fresh gate can repeat without stack or helper effects', 'resume');
  for (const address of watched) observe(e, address, 4, modelValue(address)); smcRows.push({name: fixture.name, target, gate_id: 91, destination, old_word: oldWord, return_pc: returnPC, esp: stopped.registers[4], reason: staleA ? 6 : 1, retired: 1, flags: stopped.eflags}); close(e);
}

const captureEngine = fresh(); assert.equal(captureEngine.api.map(0x1000, 1, 7), 0); assert.equal(captureEngine.api.map(0x9000, 1, 7), 0); upload(captureEngine, 0x1000, marker); upload(captureEngine, 0x9000, marker); authoredCode('capture-resident-marker', marker); authoredCode('capture-mismatched-legacy-marker', marker);
const captureUnit = compile(captureEngine, [[0x1000, 2]], [[0x1000, 101]], [], 'resident stop without capture ownership'), captureStart = initial(0x1000); reset(captureEngine, captureStart); run(captureEngine, captureUnit, 8, captureStart, exit(8, 0, 101), undefined, 'resident gate alone does not create a pending call', 'initial');
unchanged(captureEngine, () => captureEngine.api.capture_call(captureEngine.low, captureEngine.high, 1, 1, 0), 3, 'no legacy artifact means capture InvalidArtifact'); stats.capture_failures++;
descriptors(captureEngine, [[0x9000, 2]], [[0x9000, 102]]); const legacyBefore = arena(captureEngine); assert.equal(captureEngine.api.compile_with_gates(1, 1), 0); assert.deepEqual(arena(captureEngine), legacyBefore, 'legacy compilation leaves whole arena unchanged'); stats.full_arena_checks++; captureEngine.legacy = true;
assert.equal(captureEngine.api.generation(), 1); refresh(captureEngine); const legacyPointer = captureEngine.api.module_ptr() >>> 0, legacyLength = captureEngine.api.module_len() >>> 0, legacyBytes = captureEngine.bytes.slice(legacyPointer, legacyPointer + legacyLength); assert.ok(WebAssembly.validate(legacyBytes)); writeFileSync(join(outputDir, 'legacy-mismatched.wasm'), legacyBytes); artifacts['legacy-mismatched.wasm'] = hash(legacyBytes);
unchanged(captureEngine, () => captureEngine.api.capture_call(captureEngine.low, captureEngine.high, 1, 1, 0), 13, 'valid mismatched legacy gate gives InvalidStop without manufacturing capture'); stats.capture_failures++; guard(captureEngine, captureUnit); run(captureEngine, captureUnit, 8, captureStart, exit(8, 0, 101), undefined, 'resident repeat remains independent of mismatched legacy artifact', 'resume'); close(captureEngine);

assert.deepEqual(stats, {engine_instances: 15, compiled_units: 39, canonical_runs: 109, rejected_runs: 29, full_arena_checks: 479, metadata_checks: 84, successful_guards: 13, failed_guards: 19, find_success: 45, find_failure: 20, continuation_initial_calls: 49, continuation_resumes: 60, gate_cases: 3, call_cases: 6, smc_cases: 6, same_byte_invalidation_cases: 4, bound_store_sites: 10, algorithm_cases: 3, algorithm_retired: 24, faults: 2, repairs: 2, cancellations: 3, compile_failures: 7, capture_failures: 2, host_reads: 264, host_writes: 132, guest_read32: 10, guest_store32: 21}, 'independently precomputed authored-path counts');
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = ['engine/tests/process_resident_gate_wasm.rs', 'engine/tests/fixtures/p2-resident-gate/run.mjs', 'engine/tests/process_resident_gate.rs', 'engine/tests/process_resident.rs', 'engine/tests/dbt_gate.rs', 'engine/tests/process_resident_wasm.rs', 'engine/tests/fixtures/p2-resident-process/run.mjs', 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', ...productionPaths];
const provenance = {
  stats, actual_resident_runs: stats.canonical_runs + stats.rejected_runs, actual_resident_guard_entries: stats.canonical_runs + stats.rejected_runs + stats.successful_guards + stats.failed_guards, bound_store_identity_checks: stats.guest_store32, guest_helper_calls_from_authored_execution: stats.guest_store32 + stats.guest_read32,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  authored: Object.fromEntries(Object.entries(authored).map(([name, bytes]) => [name, bytes.toString('hex')])), unit_bindings: units.map(unit => ({label: unit.label, key: unit.key, id: unit.id.toString(), pointer: unit.pointer, length: unit.length, filename: unit.filename, reads: unit.reads, gates: unit.gates, store_operands: unit.stores})), algorithm_rows: algorithmRows, smc_rows: smcRows, metadata_observations: metadata,
  legacy_control: {filename: 'legacy-mismatched.wasm', key: captureEngine.key.toString(), generation: 1, pointer: legacyPointer, length: legacyLength, sha256: hash(legacyBytes), pc: 0x9000, gate_id: 102, guest_runs: 0},
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_gate_wasm', '--', '--nocapture']},
  reused: 'unchanged canonical10/11/26/36 fixed-edition gate marker/Exitv3/CALL/helper/callback/exhaustion and late helper Infrastructure detail3 evidence is source/prior-proof reuse, not replay. Prior near-control27/stack29/binary48/unary31/MOV27/read/indirect/atomic/deep-chain/synthetic actual proof sets are not rerun. Twelve selected native groups cover descriptor/preparation limits, Busy, retained capacity, ordinary callback membership and decoder-excluded mixed-kind same-PC ceiling. Parent-owned three legacy zero-CPU captures and separately executed old resident14Wasm/ALL4authored inputs are compared in full bytes to R3-324.',
  claim: 'actual Node engine-Wasm process-bound numeric GateSpec stops with exact0F0B/no new ISA and no legacy artifact except one explicit mismatched legacy control compiled with zero guestCPU. Exact baked key/u64unitID/guard7/run4/imports/direct checked Read4 and six-argument bound Store4; Exitv3 for every reason of gate-bearing units and Exitv1 retained for zero-gate overlap units. All4236 arena bytes/full GPR/ESP/EIP/flags/Helperv1/RAM/return-word/neighbours; unsigned17/high-bit/MAX descriptor IDs, execute-only gate code with unmapped ESP, repeat/cancel/budget/prefix/interior stops. Same/separate direct-register-memory CALL commits target/ESP/returnword/retirement before gate or exact Budget, current-pointer and permission repair without intermediate host CPU edits. Independent BigInt/literal3*n loop n1/2/5 returns EAX0/3/12 ECX0 flags447 with true phase-budget continuations. Ordinary/gate distinct-start overlaps both admission orders and current gate/gate collision, data-only isolation, same-byte/page/two-page/whole-unit invalidation, original/copied stale bindings, fresh IDs and closed guard priority. Active CALL Store11 commits before CodeInvalidatedv3 detail0; separate current caller no-replay; actual overwritten marker fails fresh admission then marker-only RAM repair resumes fresh gate at committed PC/ESP. Capture without legacy returns InvalidArtifact and mismatched valid legacy returns InvalidStop; no universal resident-stop provenance rejection, successful resident capture, pending ownership/pins/completion, installer/table/browser/async cancellation/performance/game/fullP2-V0 claim. Counters are derived from authored paths, not instrumentation.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2)); console.log(JSON.stringify({...provenance, sources: Object.keys(provenance.sources).length, metadata_observations: metadata.length, artifacts: outputDir}));
