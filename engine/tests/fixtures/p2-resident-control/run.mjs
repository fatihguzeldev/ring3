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
const SIZE = 4236, TRANSFER = 140, DATA = 0x8000, STACK = 0xa100, BPC = 0x6000;
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'generation', 'module_ptr', 'module_len', 'guard_resident', 'compile_resident', 'resident_module', 'find_resident', 'read32', 'write32', 'store_resident32'];
const stats = {engine_instances: 0, compiled_units: 0, canonical_runs: 0, rejected_runs: 0, full_arena_checks: 0, metadata_checks: 0, successful_guards: 0, failed_guards: 0, find_success: 0, find_failure: 0, continuation_initial_calls: 0, continuation_resumes: 0, direct_cases: 0, register_cases: 0, memory_cases: 0, ret_cases: 0, overlap_cases: 0, smc_cases: 0, same_byte_code_writes: 0, same_byte_permission_faults: 0, bound_store_sites: 0, return_pc_sites: 0, algorithm_cases: 0, algorithm_retired: 0, faults: 0, read_faults: 0, store_faults: 0, repairs: 0, cancellations: 0, host_reads: 0, host_writes: 0, guest_read32: 0, guest_store32: 0};
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

function noLegacy(engine) { assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_ptr(), 0); assert.equal(engine.api.module_len(), 0); }
function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const key = 0x98889abcc0000000n + BigInt(++stats.engine_instances);
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
function compile(engine, blocks, reads, label, stores = []) {
  descriptors(engine, blocks); const before = arena(engine); assert.equal(engine.api.compile_resident(blocks.length), 0, label);
  const selected = record(engine, before, label), module = new WebAssembly.Module(selected.bytes);
  const helpers = [...reads.filter(name => name === 'read32'), ...(stores.length ? ['store_resident32'] : []), ...reads.filter(name => name !== 'read32')];
  const expected = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard_resident', kind: 'function'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), expected, 'exact direct bound guard/read/store imports'); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const imports = {guard_resident: engine.api.guard_resident, ...Object.fromEntries(reads.map(name => [name, engine.api[name]])), ...(stores.length ? {store_resident32: engine.api.store_resident32} : {})};
  boundStores(selected.bytes, engine, selected.id, stores, reads.includes('read32'));
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
  assert.ok(body); assert.deepEqual(signatures[importTypes.get('guard_resident')], {args: Array(7).fill(0x7f), returns: [0x7f]});
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
    assert.equal(body[c.i++], 0x10); assert.equal(uleb(body, c), index); assert.equal(operand.local, undefined); stats.return_pc_sites++; found++;
  }
  assert.equal(found, operands.length, 'each CALL Store4 has immutable identity, ADDRESS26 and return-PC immediate'); stats.bound_store_sites += found;
}
function miss(engine, pc, status = 17) { unchanged(engine, () => engine.api.find_resident(pc), status, 'finder failure'); stats.find_failure++; }
function at(value, pc) { const next = copy(value); next.eip = pc; return next; }
function prefixed(start) { const value = copy(start), result = arithmetic('inc', value.registers[7], value.eflags); value.registers[7] = result.value; value.eflags = result.flags; value.eip = start.eip + 1; return value; }
function called(start, target) { const value = at(start, target); value.registers[4] = (start.registers[4] - 4) >>> 0; return value; }
function returned(start, target, cleanup = 0) { const value = at(start, target); value.registers[4] = Number(BigInt.asUintN(32, BigInt(start.registers[4]) + 4n + BigInt(cleanup))); return value; }
function storeHelper(detail = 0, address = 0) { return helper(4, 0, detail, address, 2); }
function close(engine, selected = []) { const before = arena(engine); assert.equal(engine.api.close(), 0); assert.deepEqual(arena(engine), before); stats.full_arena_checks++; for (const unit of selected) rejected(engine, unit, 5, 'closed resident module'); }
const authored = {};
function authoredCode(name, bytes) { const filename = `${name}.x86`; writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes); authored[name] = bytes; return bytes; }
function directBytes(pc, target) { const bytes = Buffer.alloc(5); bytes[0] = 0xe8; bytes.writeInt32LE(Number(BigInt.asIntN(32, BigInt(target) - BigInt(pc + 5))), 1); return bytes; }
function block(instruction) { return Buffer.concat([Buffer.from([0x47]), instruction]); }
function modelWord(image, address, value) { for (let i = 0; i < 4; i++) image.set(address + i, Number((BigInt(value) >> BigInt(i * 8)) & 255n)); }
function modelValue(image, address) { let value = 0n; for (let i = 0; i < 4; i++) { assert.ok(image.has(address + i)); value |= BigInt(image.get(address + i)) << BigInt(i * 8); } return Number(value); }
function prepareRAM(e, source, destination, input = 0) {
  const watched = [...new Set([source, destination].filter(address => address !== null).flatMap(address => { const aligned = Math.floor(address / 4) * 4; return [aligned - 4, aligned, aligned + 4, ...(address % 4 ? [aligned + 8] : [])]; }).filter(address => address >= 0 && address <= 0xfffffffc))].sort((a, b) => a - b), image = new Map();
  assert.ok(watched.every(address => address < 0x1000 || address >= 0x7000), 'data/stack/neighbours never intersect code1000..6fff');
  for (const address of watched) { word(e, address, 0x11223344); modelWord(image, address, 0x11223344); } if (source !== null) { word(e, source, input); modelWord(image, source, input); } return {watched, image};
}
function observeRAM(e, ram) { for (const address of ram.watched) observe(e, address, 4, modelValue(ram.image, address)); }
const direct = [BPC, 0, 0xffffffff].map((target, i) => ({target, pc: 0x1000 + i * 0x100}));
for (const form of direct) form.code = authoredCode(`direct-${form.pc.toString(16)}`, block(directBytes(form.pc + 1, form.target)));
const registers = Array.from({length: 8}, (_, register) => ({register, pc: 0x2000 + register * 0x100, instruction: Buffer.from([0xff, 0xd0 + register])}));
for (const form of registers) form.code = authoredCode(`register-${form.register}`, block(form.instruction));
const memory = [
  {name: 'other', hex: 'ff13', source: DATA}, {name: 'old-esp', hex: 'ff1424', source: STACK},
  {name: 'exact', hex: 'ff5424fc', source: STACK - 4, overlap: true}, {name: 'partial', hex: 'ff5424fe', source: STACK - 2, overlap: true},
  {name: 'sib-esp', hex: 'ff548c04', source: STACK + 12}, {name: 'eax-displacement', hex: 'ff50fc', source: DATA},
  {name: 'index-only', hex: 'ff14adf87f0000', source: DATA}, {name: 'absolute', hex: 'ff1500800000', source: DATA},
].map((form, i) => ({...form, pc: 0x3000 + i * 0x100, instruction: Buffer.from(form.hex, 'hex')}));
for (const form of memory) form.code = authoredCode(`memory-${form.name}`, block(form.instruction));
const rets = [{hex: 'c3', cleanup: 0}, ...[0, 1, 4, 65535].map(cleanup => ({hex: `c2${Buffer.from([cleanup & 255, cleanup >>> 8]).toString('hex')}`, cleanup}))].map((form, i) => ({...form, pc: 0x4000 + i * 0x100}));
for (const form of rets) form.code = authoredCode(`ret-${form.pc.toString(16)}`, block(Buffer.from(form.hex, 'hex')));
const composition = [[0x5000, authoredCode('compose-call', Buffer.from('ff13', 'hex'))], [0x5002, authoredCode('compose-continuation', Buffer.from('8d5b04490f85f4ffffff', 'hex'))], [BPC, authoredCode('compose-callee1', Buffer.from('8d4001c3', 'hex'))], [0x6100, authoredCode('compose-callee3', Buffer.from('8d4003c20000', 'hex'))]];
const engine = fresh(); assert.equal(engine.api.store_resident32.length, 6); for (const page of [0x1000, 0x2000, 0x3000, 0x4000, 0x5000, BPC]) assert.equal(engine.api.map(page, 1, 7), 0); assert.equal(engine.api.map(0x7000, 7, 3), 0);
for (const form of [...direct, ...registers, ...memory, ...rets]) upload(engine, form.pc, form.code); for (const [pc, bytes] of composition) upload(engine, pc, bytes);
const directUnit = compile(engine, direct.map(form => [form.pc, form.code.length]), [], 'direct near CALL', direct.map(form => ({immediate: form.pc + form.code.length})));
const registerUnit = compile(engine, registers.map(form => [form.pc, form.code.length]), [], 'all original GPR CALL targets', registers.map(form => ({immediate: form.pc + form.code.length})));
const memoryUnit = compile(engine, memory.map(form => [form.pc, form.code.length]), ['read32'], 'old-EA memory CALL captures target before return-word store', memory.map(form => ({immediate: form.pc + form.code.length})));
const retUnit = compile(engine, rets.map(form => [form.pc, form.code.length]), ['read32'], 'near RET unsigned cleanup read only');
const composeUnit = compile(engine, composition.map(([pc, bytes]) => [pc, bytes.length]), ['read32'], 'live pointer-array caller and two callees', [{immediate: 0x5002}]);
function unknown(unit, expected) { run(engine, unit, 8, expected, exit(3, 0), undefined, 'committed target resumes without another CALL/RET access', 'resume'); }
for (const form of direct) {
  select(engine, form.pc + 1, directUnit);
  for (const flags of [2, 0xcd7]) { const ram = prepareRAM(engine, null, STACK - 4), start = initial(form.pc + 1); start.eflags = flags; reset(engine, start); const expected = called(start, form.target);
    run(engine, directUnit, 1, expected, exit(1, 1), storeHelper(), 'rel32 CALL stores its exact following PC before target dispatch', 'initial', 1); modelWord(ram.image, STACK - 4, form.pc + form.code.length); observeRAM(engine, ram); unknown(directUnit, expected); stats.direct_cases++;
  }
}
for (const form of registers) {
  select(engine, form.pc, registerUnit); select(engine, form.pc + 1, registerUnit);
  for (const flags of [2, 0xcd7]) { const ram = prepareRAM(engine, null, STACK - 4), start = initial(form.pc + 1); start.eflags = flags; if (form.register !== 4) start.registers[form.register] = BPC; const target = start.registers[form.register]; reset(engine, start); const expected = called(start, target);
    run(engine, registerUnit, 1, expected, exit(1, 1), storeHelper(), 'captured original register target including old ESP survives Store4 validation', 'initial', 1); modelWord(ram.image, STACK - 4, form.pc + form.code.length); observeRAM(engine, ram); unknown(registerUnit, expected); stats.register_cases++;
  }
}
function memoryStart(form, prefix = false) { const start = initial(form.pc + Number(!prefix)); start.registers[0] = DATA + 4; start.registers[1] = 2; start.registers[3] = DATA; start.registers[5] = 2; return start; }
for (const form of memory) {
  select(engine, form.pc + 1, memoryUnit);
  for (const flags of [2, 0xcd7]) { const ram = prepareRAM(engine, form.source, STACK - 4, BPC), start = memoryStart(form); start.eflags = flags; reset(engine, start); const expected = called(start, BPC);
    run(engine, memoryUnit, 1, expected, exit(1, 1), storeHelper(), 'old ESP/displacement/index target is captured before exact or partial overlap', 'initial', 1, 1); modelWord(ram.image, STACK - 4, form.pc + form.code.length); observeRAM(engine, ram); unknown(memoryUnit, expected); stats.memory_cases++; if (form.overlap) stats.overlap_cases++;
  }
}
for (const form of rets) {
  select(engine, form.pc + 1, retUnit);
  for (const flags of [2, 0xcd7]) { const ram = prepareRAM(engine, STACK, STACK, BPC), start = initial(form.pc + 1); start.eflags = flags; reset(engine, start); const expected = returned(start, BPC, form.cleanup);
    run(engine, retUnit, 1, expected, exit(1, 1), helper(4, BPC), 'RET performs one Read4 and unsigned unaligned cleanup without a store', 'initial', 0, 1); observeRAM(engine, ram); unknown(retUnit, expected); stats.ret_cases++;
  }
}
for (const [pc] of composition) select(engine, pc, composeUnit); for (const unit of [directUnit, registerUnit, memoryUnit, retUnit, composeUnit]) guard(engine, unit);
word(engine, 0xdff8, 0x11223344); word(engine, 0xdffc, BPC); const skip = initial(rets[4].pc + 1); skip.registers[4] = 0xdffc; reset(engine, skip);
run(engine, retUnit, 1, returned(skip, BPC, 65535), exit(1, 1), helper(4, BPC), 'RET cleanup skips unmapped e000 rather than accessing its bytes', 'initial', 0, 1); observe(engine, 0xdff8, 4, 0x11223344); observe(engine, 0xdffc, 4, BPC);
function memoryAddressed(source, destination) { const start = memoryStart(memory[0], true); start.registers[3] = source; start.registers[4] = (destination + 4) >>> 0; return start; }
for (const page of [0xe000, 0xf000]) assert.equal(engine.api.map(page, 1, 3), 0);
for (const access of [1, 2]) {
  for (const page of [0xe000, 0xf000]) assert.equal(engine.api.protect(page, 1, 3), 0); const source = 0xe100, destination = 0xf100, ram = prepareRAM(engine, source, destination, BPC), address = access === 1 ? source : destination;
  assert.equal(engine.api.protect(access === 1 ? 0xe000 : 0xf000, 1, access === 1 ? 2 : 1), 0); const start = memoryAddressed(source, destination); reset(engine, start); const stopped = prefixed(start);
  run(engine, memoryUnit, 8, stopped, exit(5, 1, 2, address, 4, 2, access), helper(4, 0, 2, address, access), 'memory CALL first-read/second-store permission fault preserves old target and ESP', 'initial', Number(access === 2), 1); stats.faults++; stats[access === 1 ? 'read_faults' : 'store_faults']++;
  assert.equal(engine.api.protect(access === 1 ? 0xe000 : 0xf000, 1, 3), 0); observeRAM(engine, ram); word(engine, source, 0x6100); modelWord(ram.image, source, 0x6100);
  run(engine, memoryUnit, 8, called(stopped, 0x6100), exit(3, 1), storeHelper(), 'CALL repair rereads changed current pointer while storing original return-PC', 'resume', 1, 1); modelWord(ram.image, destination, memory[0].pc + memory[0].code.length); observeRAM(engine, ram); stats.repairs++;
}
for (const [unit, form, target] of [[directUnit, direct[0], BPC], [registerUnit, registers[4], 0xf104]]) {
  assert.equal(engine.api.protect(0xf000, 1, 3), 0); word(engine, 0xf100, 0x11223344); assert.equal(engine.api.protect(0xf000, 1, 1), 0); const start = initial(form.pc); start.registers[4] = 0xf104; reset(engine, start); const stopped = prefixed(start);
  run(engine, unit, 8, stopped, exit(5, 1, 2, 0xf100, 4), storeHelper(2, 0xf100), 'direct/register CALL has only Store4 and leaves captured old ESP uncommitted on fault', 'initial', 1); stats.faults++; stats.store_faults++;
  assert.equal(engine.api.protect(0xf000, 1, 3), 0); observe(engine, 0xf100, 4, 0x11223344); word(engine, 0xf100, 0xfeedface); run(engine, unit, 8, called(stopped, target), exit(3, 1), storeHelper(), 'destination repair keeps source register and exact return-PC immediate', 'resume', 1); observe(engine, 0xf100, 4, form.pc + form.code.length); stats.repairs++;
}
assert.equal(engine.api.protect(0xe000, 1, 3), 0); word(engine, 0xe100, BPC); assert.equal(engine.api.protect(0xe000, 1, 2), 0); const failedRet = initial(rets[3].pc); failedRet.registers[4] = 0xe100; reset(engine, failedRet); const stoppedRet = prefixed(failedRet);
run(engine, retUnit, 8, stoppedRet, exit(5, 1, 2, 0xe100, 4, 2, 1), helper(4, 0, 2, 0xe100), 'RET read fault preserves old ESP and cleanup after completed prefix', 'initial', 0, 1); stats.faults++; stats.read_faults++;
assert.equal(engine.api.protect(0xe000, 1, 3), 0); observe(engine, 0xe100, 4, BPC); word(engine, 0xe100, 0x6100); run(engine, retUnit, 8, returned(stoppedRet, 0x6100, 4), exit(3, 1), helper(4, 0x6100), 'RET repair reads current popped stack word and applies cleanup once', 'resume', 0, 1); observe(engine, 0xe100, 4, 0x6100); stats.repairs++;
const sameForm = memory[2], samePC = sameForm.pc + sameForm.code.length; assert.equal(engine.api.protect(0xa000, 1, 3), 0); const sameRAM = prepareRAM(engine, STACK - 4, STACK - 4, samePC); assert.equal(engine.api.protect(0xa000, 1, 1), 0); const sameStart = memoryStart(sameForm, true); reset(engine, sameStart);
run(engine, memoryUnit, 8, prefixed(sameStart), exit(5, 1, 2, STACK - 4, 4), storeHelper(2, STACK - 4), 'same-byte return-PC store still requires Write permission', 'initial', 1, 1); observeRAM(engine, sameRAM); stats.faults++; stats.store_faults++; stats.same_byte_permission_faults++;
const readonlyRet = initial(rets[0].pc + 1); readonlyRet.registers[4] = STACK - 4; reset(engine, readonlyRet); run(engine, retUnit, 1, returned(readonlyRet, samePC), exit(1, 1), helper(4, samePC), 'RET succeeds with read-only stack and cannot invalidate code', 'initial', 0, 1); observeRAM(engine, sameRAM); assert.equal(engine.api.protect(0xa000, 1, 3), 0);
assert.equal(engine.api.map(0x11000, 2, 3), 0);
for (const access of [1, 2]) {
  assert.equal(engine.api.protect(0x12000, 1, 3), 0); const source = access === 1 ? 0x11fff : 0xd100, destination = access === 1 ? 0xd100 : 0x11fff, ram = prepareRAM(engine, source, destination, BPC);
  if (access === 1) assert.equal(engine.api.unmap(0x12000, 1), 0); else assert.equal(engine.api.protect(0x12000, 1, 1), 0); const start = memoryAddressed(source, destination); reset(engine, start); const stopped = prefixed(start), detail = access === 1 ? 1 : 2;
  run(engine, memoryUnit, 8, stopped, exit(5, 1, detail, 0x12000, 4, 2, access), helper(4, 0, detail, 0x12000, access), 'cross-page target read or atomic return-word store rejects before CALL commit', 'initial', Number(access === 2), 1); for (const address of ram.watched.filter(address => access === 2 || address < 0x12000)) observe(engine, address, 4, modelValue(ram.image, address)); stats.faults++; stats[access === 1 ? 'read_faults' : 'store_faults']++;
  if (access === 1) { assert.equal(engine.api.map(0x12000, 1, 3), 0); for (const address of ram.watched.filter(address => address >= 0x12000)) word(engine, address, modelValue(ram.image, address)); } else assert.equal(engine.api.protect(0x12000, 1, 3), 0);
  word(engine, source, 0x6100); modelWord(ram.image, source, 0x6100); run(engine, memoryUnit, 8, called(stopped, 0x6100), exit(3, 1), storeHelper(), 'cross-page repair uses current target before original return-PC store', 'resume', 1, 1); modelWord(ram.image, destination, memory[0].pc + memory[0].code.length); observeRAM(engine, ram); stats.repairs++;
}
assert.equal(engine.api.protect(0x12000, 1, 3), 0); const crossRetRAM = prepareRAM(engine, 0x11fff, 0x11fff, BPC); assert.equal(engine.api.unmap(0x12000, 1), 0); const crossRet = initial(rets[2].pc); crossRet.registers[4] = 0x11fff; reset(engine, crossRet); const stoppedCrossRet = prefixed(crossRet);
run(engine, retUnit, 8, stoppedCrossRet, exit(5, 1, 1, 0x12000, 4, 2, 1), helper(4, 0, 1, 0x12000), 'RET Read4 crossing rejects before target or unsigned cleanup', 'initial', 0, 1); for (const address of crossRetRAM.watched.filter(address => address < 0x12000)) observe(engine, address, 4, modelValue(crossRetRAM.image, address)); stats.faults++; stats.read_faults++;
assert.equal(engine.api.map(0x12000, 1, 3), 0); for (const address of crossRetRAM.watched.filter(address => address >= 0x12000)) word(engine, address, modelValue(crossRetRAM.image, address)); word(engine, 0x11fff, 0x6100); modelWord(crossRetRAM.image, 0x11fff, 0x6100); run(engine, retUnit, 8, returned(stoppedCrossRet, 0x6100, 1), exit(3, 1), helper(4, 0x6100), 'RET cross repair rereads current stack word and applies byte cleanup', 'resume', 0, 1); observeRAM(engine, crossRetRAM); stats.repairs++;
for (const access of [1, 2]) {
  const source = access === 1 ? 0xfffffffd : DATA, destination = access === 1 ? 0xd100 : 0xfffffffd; word(engine, access === 1 ? 0xd100 : DATA, BPC); const start = memoryAddressed(source, destination); reset(engine, start); const stopped = prefixed(start);
  run(engine, memoryUnit, 8, stopped, exit(5, 1, 3, 0xfffffffd, 4, 2, access), helper(4, 0, 3, 0xfffffffd, access), 'wrapping effective arithmetic does not permit overflowing width4 access', 'initial', Number(access === 2), 1); observe(engine, access === 1 ? 0xd100 : DATA, 4, BPC); stats.faults++; stats[access === 1 ? 'read_faults' : 'store_faults']++;
}
const overflowRet = initial(rets[4].pc); overflowRet.registers[4] = 0xfffffffd; reset(engine, overflowRet); run(engine, retUnit, 8, prefixed(overflowRet), exit(5, 1, 3, 0xfffffffd, 4, 2, 1), helper(4, 0, 3, 0xfffffffd), 'RET cleanup cannot bypass its overflowing initial Read4', 'initial', 0, 1); stats.faults++; stats.read_faults++;
const overflowCall = initial(registers[4].pc); overflowCall.registers[4] = 1; reset(engine, overflowCall); run(engine, registerUnit, 8, prefixed(overflowCall), exit(5, 1, 3, 0xfffffffd, 4), storeHelper(3, 0xfffffffd), 'CALL ESP wrap computes address but forbids overflowing return-word write', 'initial', 1); stats.faults++; stats.store_faults++;
assert.equal(engine.api.map(0, 1, 3), 0); assert.equal(engine.api.map(0xfffff000, 1, 3), 0);
const wrapCallRAM = prepareRAM(engine, null, 0xfffffffc), wrapCall = initial(registers[4].pc + 1); wrapCall.registers[4] = 0; reset(engine, wrapCall); run(engine, registerUnit, 1, called(wrapCall, 0), exit(1, 1), storeHelper(), 'CALL ESP captures zero before decrement to last valid word', 'initial', 1); modelWord(wrapCallRAM.image, 0xfffffffc, registers[4].pc + registers[4].code.length); observeRAM(engine, wrapCallRAM);
for (const row of [{form: memory[1], esp: 0, source: 0, destination: 0xfffffffc, ecx: 2}, {form: memory[2], esp: 0, source: 0xfffffffc, destination: 0xfffffffc, ecx: 2}, {form: memory[4], esp: 0xfffffffc, source: 4, destination: 0xfffffff8, ecx: 1}]) {
  const ram = prepareRAM(engine, row.source, row.destination, BPC), start = memoryStart(row.form); start.registers[4] = row.esp; start.registers[1] = row.ecx; reset(engine, start); run(engine, memoryUnit, 1, called(start, BPC), exit(1, 1), storeHelper(), 'old-ESP/SIB target address wraps while both checked accesses remain valid', 'initial', 1, 1); modelWord(ram.image, row.destination, row.form.pc + row.form.code.length); observeRAM(engine, ram);
}
for (const form of [rets[0], rets[2], rets[3], rets[4]]) { const ram = prepareRAM(engine, 0xfffffffc, 0xfffffffc, BPC), start = initial(form.pc + 1); start.registers[4] = 0xfffffffc; reset(engine, start); run(engine, retUnit, 1, returned(start, BPC, form.cleanup), exit(1, 1), helper(4, BPC), 'valid last word RET allows unsigned cleanup and ESP wrap without extra reads', 'initial', 0, 1); observeRAM(engine, ram); }
const splitRAM = prepareRAM(engine, DATA, STACK - 4, BPC), split = memoryStart(memory[0], true); reset(engine, split); run(engine, memoryUnit, 0, split, exit(1, 0), undefined, 'zero budget before prefix skips all CALL helpers', 'initial'); const prefix = prefixed(split);
run(engine, memoryUnit, 1, prefix, exit(1, 1), undefined, 'prefix exact budget stops before target read and return write', 'resume'); engine.view.setUint32(engine.base + 96, 1, true); run(engine, memoryUnit, 0, prefix, exit(2, 0), undefined, 'cancel wins over zero budget before CALL access', 'resume'); stats.cancellations++; engine.view.setUint32(engine.base + 96, 0, true);
const splitCalled = called(prefix, BPC); run(engine, memoryUnit, 1, splitCalled, exit(1, 1), storeHelper(), 'one CALL budget commits target and return word exactly once', 'resume', 1, 1); modelWord(splitRAM.image, STACK - 4, memory[0].pc + memory[0].code.length); word(engine, DATA, 0x6100); modelWord(splitRAM.image, DATA, 0x6100); unknown(memoryUnit, splitCalled); observeRAM(engine, splitRAM);
const splitRetRAM = prepareRAM(engine, STACK, STACK, BPC), splitRet = initial(rets[3].pc); reset(engine, splitRet); run(engine, retUnit, 0, splitRet, exit(1, 0), undefined, 'zero RET budget performs no stack read', 'initial'); const prefixRet = prefixed(splitRet);
run(engine, retUnit, 1, prefixRet, exit(1, 1), undefined, 'RET prefix budget leaves old stack pointer', 'resume'); const retDone = returned(prefixRet, BPC, 4); run(engine, retUnit, 1, retDone, exit(1, 1), helper(4, BPC), 'exact RET budget commits target and cleanup once', 'resume', 0, 1); word(engine, STACK, 0x6100); modelWord(splitRetRAM.image, STACK, 0x6100); unknown(retUnit, retDone); observeRAM(engine, splitRetRAM);
const cancelled = memoryStart(memory[0], true); reset(engine, cancelled, 1); run(engine, memoryUnit, 8, cancelled, exit(2, 0), undefined, 'preset cancel suppresses target read and return-PC store', 'initial'); stats.cancellations++;
const middle = initial(memory[1].pc + 2); reset(engine, middle); miss(engine, middle.eip); run(engine, memoryUnit, 8, middle, exit(3, 0), undefined, 'middle byte is not a decoded control entry', 'initial');
for (const target of [0, 0xffffffff, memory[1].pc + 2]) { const ram = prepareRAM(engine, STACK, STACK, target), start = initial(rets[0].pc + 1); reset(engine, start); const expected = returned(start, target); run(engine, retUnit, 1, expected, exit(1, 1), helper(4, target), 'RET commits arbitrary target data without prefetch or execute access', 'initial', 0, 1); miss(engine, target); observeRAM(engine, ram); unknown(retUnit, expected); }
const algorithmRows = [];
for (const n of [1, 2, 5]) {
  const source = DATA + 0x100, targets = Array.from({length: n}, (_, i) => (i + n) % 2 ? 0x6100 : BPC), deltas = targets.map(target => target === BPC ? 1 : 3), start = initial(0x5000); start.registers[0] = 0xfffffffd; start.registers[1] = n; start.registers[3] = source; reset(engine, start);
  for (const address of [source - 4, source + n * 4, STACK - 8, STACK - 4, STACK]) word(engine, address, 0x11223344); for (let i = 0; i < n; i++) word(engine, source + i * 4, targets[i]);
  const budgets = n === 2 ? [1, 1, 1, 3, 1, 2, 3] : [6 * n + 1]; let retired = 0, calls = 0;
  for (const budget of budgets) {
    const next = Math.min(6 * n, retired + budget), cycles = Math.floor(next / 6), phase = next % 6, previousCycles = Math.floor(retired / 6), previousPhase = retired % 6, callsDone = cycles + Number(phase >= 1), returnsDone = cycles + Number(phase >= 3), stores = callsDone - previousCycles - Number(previousPhase >= 1), reads = stores + returnsDone - previousCycles - Number(previousPhase >= 3), calculated = cycles + Number(phase >= 2);
    const pc = phase === 0 ? (cycles === n ? 0x500c : 0x5000) : phase === 1 ? targets[cycles] : phase === 2 ? targets[cycles] + 3 : [0, 0, 0, 0x5002, 0x5005, 0x5006][phase], expected = at(start, pc);
    expected.registers[0] = Number(BigInt.asUintN(32, BigInt(start.registers[0]) + deltas.slice(0, calculated).reduce((sum, delta) => sum + BigInt(delta), 0n))); expected.registers[1] = n - cycles - Number(phase >= 5); expected.registers[3] = source + (cycles + Number(phase >= 4)) * 4; expected.registers[4] = STACK - Number(phase === 1 || phase === 2) * 4;
    if (phase >= 5) expected.eflags = arithmetic('dec', n - cycles, start.eflags).flags; else if (cycles) expected.eflags = arithmetic('dec', n - cycles + 1, start.eflags).flags;
    run(engine, composeUnit, budget, expected, exit(next - retired === budget ? 1 : 3, next - retired), reads ? (phase === 1 || phase === 2 ? storeHelper() : helper(4, 0x5002)) : undefined, 'independent pointer-array six-instruction caller/callee composition', retired === 0 ? 'initial' : 'resume', stores, reads);
    for (let i = 0; i < n; i++) observe(engine, source + i * 4, 4, targets[i]); for (const address of [source - 4, source + n * 4, STACK - 8, STACK]) observe(engine, address, 4, 0x11223344); observe(engine, STACK - 4, 4, callsDone ? 0x5002 : 0x11223344); retired = next; calls++;
  }
  assert.equal(retired, 6 * n); const output = Number(BigInt.asUintN(32, BigInt(start.registers[0]) + deltas.reduce((sum, delta) => sum + BigInt(delta), 0n))); assert.deepEqual([n, output], n === 1 ? [1, 0] : n === 2 ? [2, 1] : [5, 8]); algorithmRows.push({n, targets, deltas, initial_eax: start.registers[0], eax: output, retired, calls, esp: STACK, flags: 0x447}); stats.algorithm_cases++; stats.algorithm_retired += retired;
}
const pendingRAM = prepareRAM(engine, DATA, STACK - 4, BPC), pending = initial(0x5000); pending.registers[1] = 1; pending.registers[3] = DATA; reset(engine, pending); const pendingCall = called(pending, BPC);
run(engine, composeUnit, 1, pendingCall, exit(1, 1), storeHelper(), 'split composed CALL captures target and return word once', 'initial', 1, 1); modelWord(pendingRAM.image, STACK - 4, 0x5002); word(engine, DATA, 0x6100); modelWord(pendingRAM.image, DATA, 0x6100); const pendingRet = at(pendingCall, BPC + 3); pendingRet.registers[0] = (pendingRet.registers[0] + 1) >>> 0;
run(engine, composeUnit, 1, pendingRet, exit(1, 1), undefined, 'changed pointer cannot redirect the already captured callee', 'resume'); assert.equal(engine.api.protect(0xa000, 1, 2), 0);
run(engine, composeUnit, 8, pendingRet, exit(5, 0, 2, STACK - 4, 4, 2, 1), helper(4, 0, 2, STACK - 4), 'callee RET read failure preserves committed CALL and callee output', 'resume', 0, 1); stats.faults++; stats.read_faults++; engine.view.setUint32(engine.base + 96, 1, true); run(engine, composeUnit, 0, pendingRet, exit(2, 0), undefined, 'cancelled pending RET repeats neither CALL nor read', 'resume'); stats.cancellations++; engine.view.setUint32(engine.base + 96, 0, true);
assert.equal(engine.api.protect(0xa000, 1, 3), 0); observeRAM(engine, pendingRAM); word(engine, STACK - 4, 0x5005); modelWord(pendingRAM.image, STACK - 4, 0x5005); const repairedRet = returned(pendingRet, 0x5005);
run(engine, composeUnit, 1, repairedRet, exit(1, 1), helper(4, 0x5005), 'RET repair rereads changed popped return word and skips caller LEA by guest data', 'resume', 0, 1); const pendingDone = at(repairedRet, 0x500c); pendingDone.registers[1] = 0; pendingDone.eflags = 0x447; run(engine, composeUnit, 8, pendingDone, exit(3, 2), undefined, 'fresh RET target resumes remaining DEC/JNZ without stack replay', 'resume'); observeRAM(engine, pendingRAM); stats.repairs++;
close(engine, [directUnit, registerUnit, memoryUnit, retUnit, composeUnit]);
const smcCases = [
  {name: 'self-direct', kind: 'direct', target: 'self'}, {name: 'self-register', kind: 'register', target: 'self'}, {name: 'self-memory', kind: 'memory', target: 'self'}, {name: 'same-byte', kind: 'memory', target: 'self', same: true},
  {name: 'whole-unit', kind: 'memory', target: 'whole'}, {name: 'shared-page', kind: 'memory', target: 'shared'}, {name: 'separate-B', kind: 'memory', target: 'separate'},
], smcRows = [];
for (const fixture of smcCases) {
  const e = fresh(), bpc = fixture.target === 'shared' ? 0x1800 : 0x2000, instruction = fixture.kind === 'direct' ? directBytes(0x1001, bpc) : Buffer.from(fixture.kind === 'register' ? 'ffd0' : 'ff13', 'hex'), callCode = block(instruction), returnPC = 0x1000 + callCode.length, oldWord = fixture.same ? returnPC : 0x13579bdf;
  const continuation = Buffer.alloc(10); continuation[0] = 0xb9; continuation.writeUInt32LE(oldWord, 1); continuation[5] = 0xe9; continuation.writeInt32LE(0x3000 - (returnPC + 10), 6); const code = Buffer.concat([callCode, continuation]), callee = Buffer.from('b8df9b5713c3', 'hex'), other = Buffer.from('b9df9b5713eb00', 'hex');
  const destination = fixture.target === 'self' ? returnPC + 1 : fixture.target === 'whole' ? 0x4001 : bpc + 1;
  authoredCode(`${fixture.name}-A`, code); authoredCode(`${fixture.name}-B`, callee); authoredCode(`${fixture.name}-other-A`, other); assert.equal(e.api.map(0x1000, 1, 7), 0); if (bpc === 0x2000) assert.equal(e.api.map(bpc, 1, 7), 0); assert.equal(e.api.map(0x4000, 1, 7), 0); assert.equal(e.api.map(0x7000, 2, 3), 0);
  if (fixture.target === 'whole') assert.equal(e.api.map(0x3000, 1, 1), 0);
  upload(e, 0x1000, code); upload(e, bpc, callee); upload(e, 0x4000, other); for (const [address, value] of [[DATA - 4, 0x11223344], [DATA, bpc], [DATA + 4, 0x11223344]]) word(e, address, value);
  const watched = [destination - 4, destination, destination + 4, DATA - 4, DATA, DATA + 4], image = new Map(); for (const address of watched) for (let i = 0; i < 4; i++) image.set(address + i, 0); for (const [pc, bytes] of [[0x1000, code], [bpc, callee], [0x4000, other]]) for (let i = 0; i < bytes.length; i++) image.set(pc + i, bytes[i]); modelWord(image, DATA - 4, 0x11223344); modelWord(image, DATA, bpc); modelWord(image, DATA + 4, 0x11223344);
  const reads = fixture.kind === 'memory' ? ['read32'] : [], oldA = compile(e, [[0x1000, callCode.length], [returnPC, 10], [0x4000, 7]], reads, `${fixture.name} A`, [{immediate: returnPC}]), oldB = compile(e, [[bpc, callee.length]], ['read32'], `${fixture.name} callee`);
  const start = initial(0x1000); start.registers[4] = destination + 4; start.registers[3] = DATA; if (fixture.kind === 'register') start.registers[0] = bpc; reset(e, start); const stopped = called(prefixed(start), bpc), invalidatesA = fixture.target !== 'separate';
  run(e, oldA, invalidatesA ? 2 : 8, stopped, exit(invalidatesA ? 6 : 3, 2), storeHelper(), 'return word/ESP/captured target/retirement commit before executing-unit invalidation', 'initial', 1, Number(fixture.kind === 'memory')); modelWord(image, destination, returnPC); for (const address of watched) observe(e, address, 4, modelValue(image, address)); stats.smc_cases++; if (fixture.same) stats.same_byte_code_writes++;
  smcRows.push({name: fixture.name, destination, old: fixture.target === 'self' ? oldWord : 0x13579bdf, return_pc: returnPC, target: bpc, old_esp: start.registers[4], committed_esp: stopped.registers[4], flags: stopped.eflags, reason: invalidatesA ? 6 : 3, retired: 2}); guard(e, oldA, invalidatesA ? 4 : 0); const staleB = fixture.target === 'shared' || fixture.target === 'separate'; guard(e, oldB, staleB ? 4 : 0);
  let successor = oldA; if (invalidatesA) { miss(e, returnPC, 4); if (fixture.target === 'whole') miss(e, 0x4000, 4); if (fixture.same || fixture.target === 'whole') { rejected(e, oldA, 4, 'same-byte or whole-unit return write makes original A stale'); const copied = new WebAssembly.Instance(new WebAssembly.Module(oldA.bytes.slice()), {env: {memory: e.memory}, ring3: {guard_resident: e.api.guard_resident, read32: e.api.read32, store_resident32: e.api.store_resident32}}); rejected(e, {...oldA, run: copied.exports.run}, 4, 'copied CALL module retains stale immutable binding'); } successor = compile(e, [[returnPC, 10]], [], 'fresh caller return-PC successor'); assert.ok(successor.id > oldA.id); }
  else run(e, oldA, 8, stopped, exit(3, 0), undefined, 'returning separate current A cannot push again at committed callee target', 'resume');
  let currentB = oldB; if (staleB) { miss(e, bpc, 4); rejected(e, oldB, 4, 'modified callee rejects its old immutable binding'); if (fixture.target === 'shared') { const copied = new WebAssembly.Instance(new WebAssembly.Module(oldB.bytes.slice()), {env: {memory: e.memory}, ring3: {guard_resident: e.api.guard_resident, read32: e.api.read32}}); rejected(e, {...oldB, run: copied.exports.run}, 4, 'copied shared-page callee retains stale immutable binding'); } currentB = compile(e, [[bpc, callee.length]], ['read32'], 'fresh modified callee'); } select(e, bpc, currentB);
  const afterRet = returned(stopped, returnPC); afterRet.registers[0] = staleB ? returnPC : 0x13579bdf; run(e, currentB, 3, afterRet, exit(3, 2), helper(4, returnPC), 'current or fresh callee consumes committed return word and restores old ESP once', 'resume', 0, 1); select(e, returnPC, successor); const done = at(afterRet, 0x3000); done.registers[1] = fixture.target === 'self' ? returnPC : oldWord;
  run(e, successor, 3, done, exit(3, 2, 0, 0, 0, invalidatesA ? 1 : 2), undefined, 'fresh return-PC continuation cannot repeat CALL or RET', 'resume'); for (const address of watched) observe(e, address, 4, modelValue(image, address)); close(e);
}
assert.deepEqual(stats, {engine_instances: 8, compiled_units: 27, canonical_runs: 181, rejected_runs: 12, full_arena_checks: 684, metadata_checks: 77, successful_guards: 11, failed_guards: 8, find_success: 50, find_failure: 13, continuation_initial_calls: 89, continuation_resumes: 92, direct_cases: 6, register_cases: 16, memory_cases: 16, ret_cases: 10, overlap_cases: 4, smc_cases: 7, same_byte_code_writes: 1, same_byte_permission_faults: 1, bound_store_sites: 27, return_pc_sites: 27, algorithm_cases: 3, algorithm_retired: 48, faults: 14, read_faults: 7, store_faults: 7, repairs: 9, cancellations: 3, host_reads: 451, host_writes: 369, guest_read32: 87, guest_store32: 72}, 'independently precomputed authored-path counts');
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = ['engine/tests/process_resident_control_wasm.rs', 'engine/tests/fixtures/p2-resident-control/run.mjs', 'engine/tests/process_resident_control.rs', 'engine/tests/process_resident.rs', 'engine/tests/process_resident_memory.rs', 'engine/tests/process_resident_indirect.rs', 'engine/tests/process_resident_store.rs', 'engine/tests/process_resident_unary.rs', 'engine/tests/process_resident_binary.rs', 'engine/tests/process_resident_stack.rs', 'engine/tests/process_resident_wasm.rs', 'engine/tests/fixtures/p2-resident-process/run.mjs', 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', ...productionPaths];
const provenance = {
  stats, actual_resident_runs: stats.canonical_runs + stats.rejected_runs, actual_resident_guard_entries: stats.canonical_runs + stats.rejected_runs + stats.successful_guards + stats.failed_guards, bound_store_identity_checks: stats.guest_store32, guest_helper_calls_from_authored_execution: stats.guest_store32 + stats.guest_read32,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  authored: Object.fromEntries(Object.entries(authored).map(([name, bytes]) => [name, bytes.toString('hex')])), unit_bindings: units.map(unit => ({label: unit.label, id: unit.id.toString(), pointer: unit.pointer, length: unit.length, filename: unit.filename, reads: unit.reads, store_operands: unit.stores})), algorithm_rows: algorithmRows, smc_rows: smcRows, metadata_observations: metadata,
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_control_wasm', '--', '--nocapture']},
  reused: 'unchanged canonical06/07/09 fixed-edition primary sources and existing emitter/helpers/deep20,001-frame chain/synthetic protocol/atomic bound Store4/exhaustion/rawidentity/Busy/callback evidence are reused rather than rerun. Prior stack29/binary48/unary31/MOV27/read/indirect actual/module evidence is not replayed; late non0/non11 Infrastructure detail3 is source/prior-proof reuse. Affected old resident target separately proves its14modules/three other inputs unchanged versus R3-323; unsupportedFF10 deliberately becomes privilegedCLTS0F06. Five legacy-family zero-CPU captures are parent-owned.',
  claim: 'actual Node engine-Wasm flat32 near CALL E8rel32/FF2reg/FF2mem and RET C3/C2imm16 through direct checked helpers and the same engine memory, no legacy artifact. All eight original register targets including ESP, original-ESP/index/displacement/exact-partial alias capture; every six-argument Store4 is four immutable key/unitID limbs plus ADDRESS26 and immediate return-PC, never target RESULT18. Exact guard0/store1 or guard0/read1/store2 or guard0/read1 signatures; whole4236 arena/full flags/GPR/ESP/EIP/target-source/return-word/neighbour RAM/Helperv1/Exitv2. Unsigned cleanup0/1/4/65535/skipped-unmapped bytes/MAX-valid/wrapping ESP+EA distinguished from forbidden access overflow; precise read/store faults and current-pointer/popped-stack repair/prefix/exact-budget/cancel/interior/arbitrary target/no replay without intermediate host CPU patches. Self/whole/shared/separateB and same-byte CALL Store11 commit target/ESP/return word/retirement once before fresh callee and return-PC continuation; copied stale bindings. Four-block live pointer-array two-callee6*n composition n1/2/5 has independent BigInt EAX0/1/8, restored ESP and finalflags447, with actual internal returning Wasm dispatch and split CALL/RET budgets. Counts derive from authored paths, not instrumentation. No helper interception/new ISA/hardware exception/CET/LOCK/calling-convention/gates/installer/lifetime/browser/async cancellation/performance/game/fullP2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2)); console.log(JSON.stringify({...provenance, sources: Object.keys(provenance.sources).length, metadata_observations: metadata.length, artifacts: outputDir}));
