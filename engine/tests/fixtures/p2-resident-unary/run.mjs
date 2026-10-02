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
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'generation', 'module_ptr', 'module_len', 'guard_resident', 'compile_resident', 'resident_module', 'find_resident', 'read32', 'write32', 'store_resident32'];
const stats = {engine_instances: 0, compiled_units: 0, canonical_runs: 0, rejected_runs: 0, full_arena_checks: 0, metadata_checks: 0, successful_guards: 0, failed_guards: 0, find_success: 0, find_failure: 0, continuation_initial_calls: 0, continuation_resumes: 0, form_cases: 0, alias_cases: 0, smc_cases: 0, same_byte_code_writes: 0, same_byte_permission_faults: 0, bound_store_sites: 0, algorithm_cases: 0, algorithm_retired: 0, faults: 0, read_faults: 0, store_faults: 0, repairs: 0, cancellations: 0, host_reads: 0, host_writes: 0, guest_read32: 0, guest_store32: 0};
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
  const key = 0x9556789aa0000000n + BigInt(++stats.engine_instances);
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
  const a = BigInt(input), value = Number(BigInt.asUintN(32, kind === 'inc' ? a + 1n : kind === 'dec' ? a - 1n : kind === 'not' ? a ^ 0xffffffffn : -a));
  if (kind === 'not') return {value, flags: oldFlags};
  let flags = (oldFlags & (kind === 'neg' ? 0x400 : 0x401)) | 2;
  if (kind === 'neg' && a !== 0n) flags |= 1;
  if ((a ^ BigInt(kind === 'neg' ? 0 : 1) ^ BigInt(value)) & 16n) flags |= 0x10;
  if (kind === 'inc' ? a === 0x7fffffffn : a === 0x80000000n) flags |= 0x800;
  if (value === 0) flags |= 0x40; if (value >= 0x80000000) flags |= 0x80;
  let ones = 0; for (let byte = value & 255; byte; byte >>>= 1) ones += byte & 1; if (ones % 2 === 0) flags |= 4;
  return {value, flags};
}
for (const [kind, input, oldFlags, value, flags] of [['inc', 15, 0xcd7, 16, 0x413], ['inc', 0xffffffff, 2, 0, 0x56], ['inc', 0x7fffffff, 0x403, 0x80000000, 0xc97], ['dec', 0, 2, 0xffffffff, 0x96], ['dec', 1, 0xcd7, 0, 0x447], ['dec', 0x80000000, 2, 0x7fffffff, 0x816], ['not', 0, 0xcd7, 0xffffffff, 0xcd7], ['not', 0x80000000, 2, 0x7fffffff, 2], ['neg', 0, 2, 0, 0x46], ['neg', 1, 2, 0xffffffff, 0x97], ['neg', 0x80000000, 2, 0x80000000, 0x887], ['neg', 0xffffffff, 2, 1, 0x13]]) assert.deepEqual(unary(kind, input, oldFlags), {value, flags}, 'literal four-kind unary flag anchors');
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
  assert.deepEqual(signatures[importTypes.get('read32')], {args: [0x7f], returns: [0x7f]}); assert.equal(index, 2);
  assert.deepEqual(signatures[importTypes.get('store_resident32')], {args: Array(6).fill(0x7f), returns: [0x7f]});
  const limbs = [engine.low, engine.high, Number(id & 0xffffffffn), Number(id >> 32n)]; let found = 0;
  for (let offset = 0; offset < body.length; offset++) {
    if (body[offset] !== 0x41) continue;
    const c = {i: offset}, constants = []; let match = true;
    try { for (let j = 0; j < 4; j++) { if (body[c.i++] !== 0x41) { match = false; break; } constants.push(sleb32(body, c)); } } catch { match = false; }
    if (!match || !constants.every((value, j) => value === limbs[j]) || body[c.i] !== 0x20 || body[c.i + 1] !== 26) continue;
    c.i += 2; assert.equal(body[c.i++], 0x20); assert.equal(uleb(body, c), 18, 'unary value comes from preserved RESULT local');
    assert.equal(body[c.i++], 0x10); assert.equal(uleb(body, c), index); found++;
  }
  assert.equal(found, count, 'each Store4 call has the immutable entry key/ID, ADDRESS26 and RESULT18'); stats.bound_store_sites += found;
}
function miss(engine, pc, status = 17) { unchanged(engine, () => engine.api.find_resident(pc), status, 'finder failure'); stats.find_failure++; }
function at(value, pc) { const next = copy(value); next.eip = pc; return next; }
function prefixed(start) { const value = copy(start), result = unary('inc', value.registers[7], value.eflags); value.registers[7] = result.value; value.eflags = result.flags; value.eip = start.eip + 1; return value; }
function applied(start, kind, operand, pc) { const value = at(start, pc), result = unary(kind, operand, start.eflags); value.eflags = result.flags; return value; }
function storeHelper(detail = 0, address = 0) { return helper(4, 0, detail, address, 2); }
function close(engine, selected = []) { const before = arena(engine); assert.equal(engine.api.close(), 0); assert.deepEqual(arena(engine), before); stats.full_arena_checks++; for (const unit of selected) rejected(engine, unit, 5, 'closed resident module'); }
const authored = {};
function authoredCode(name, bytes) { const filename = `${name}.x86`; writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes); authored[name] = bytes; return bytes; }
function directBlock(pc, instruction) { const bytes = Buffer.alloc(1 + instruction.length + 5); bytes[0] = 0x47; instruction.copy(bytes, 1); bytes[1 + instruction.length] = 0xe9; bytes.writeInt32LE(0x2000 - (pc + bytes.length), 2 + instruction.length); return bytes; }
const forms = [
  {kind: 'inc', pc: 0x1000, hex: 'ff03', address: DATA}, {kind: 'dec', pc: 0x1100, hex: 'ff0b', address: DATA},
  {kind: 'not', pc: 0x1200, hex: 'f713', address: DATA}, {kind: 'neg', pc: 0x1300, hex: 'f71b', address: DATA},
  {kind: 'inc', pc: 0x1400, hex: 'ff0424', address: DATA, alias: 'esp'}, {kind: 'dec', pc: 0x1500, hex: 'ff4d00', address: DATA, alias: 'ebp'},
  {kind: 'not', pc: 0x1600, hex: 'f7548c04', address: DATA + 12, alias: 'esp-index'}, {kind: 'neg', pc: 0x1700, hex: 'f75cb4fc', address: DATA + 4, alias: 'esp-index-negative-disp'},
];
for (const form of forms) { form.name = `${form.kind}${form.alias ? `-${form.alias}` : ''}`; form.code = authoredCode(form.name, directBlock(form.pc, Buffer.from(form.hex, 'hex'))); }
const readerBytes = authoredCode('reader', Buffer.from('8b12eb00', 'hex')), loopBytes = authoredCode('countdown', Buffer.from('ff0b0f85f8ffffff', 'hex'));
const engine = fresh(); assert.equal(engine.api.store_resident32.length, 6); for (const page of [0x1000, 0x2000, 0x3000]) assert.equal(engine.api.map(page, 1, 7), 0); assert.equal(engine.api.map(0x4000, 3, 3), 0);
for (const form of forms) upload(engine, form.pc, form.code); upload(engine, 0x2000, readerBytes); upload(engine, 0x3000, loopBytes);
const a = compile(engine, forms.map(form => [form.pc, form.code.length]), ['read32'], 'all four unary forms and representative old EAs', forms.length), b = compile(engine, [[0x2000, 4]], ['read32'], 'live read destination/base alias'), counter = compile(engine, [[0x3000, 8]], ['read32'], 'DEC memory/JNZ counter', 1);
function startFor(form, interior = true) { const value = initial(form.pc + Number(interior)); value.registers[1] = 2; value.registers[2] = form.address; value.registers[3] = DATA; value.registers[5] = DATA; value.registers[6] = 2; return value; }
function values(form, operand, flags, label) {
  for (const [offset, value] of [[-4, 0x11223344], [0, operand], [4, 0x55667788]]) word(engine, form.address + offset, value);
  const start = startFor(form); start.eflags = flags; reset(engine, start); const result = unary(form.kind, operand, flags), expected = applied(start, form.kind, operand, 0x2000);
  run(engine, a, 3, expected, exit(3, 2), storeHelper(), label, 'initial', 1, 1);
  observe(engine, form.address - 4, 4, 0x11223344); observe(engine, form.address, 4, result.value); observe(engine, form.address + 4, 4, 0x55667788);
  const read = at(expected, 0x2004); read.registers[2] = result.value; run(engine, b, 3, read, exit(3, 2), helper(4, result.value), 'live B read consumes existing CPU', 'resume', 0, 1);
}
for (const form of forms.slice(0, 4)) {
  select(engine, form.pc, a); select(engine, form.pc + 1, a);
  for (const operand of [0, 1, 15, 16, 0x7fffffff, 0x80000000, 0xffffffff]) for (const flags of [2, 0xcd7]) { values(form, operand, flags, `${form.kind} literal/BigInt flags and preserved GPRs`); stats.form_cases++; }
}
select(engine, 0x2000, b);
for (const form of forms.slice(4)) { select(engine, form.pc + 1, a); values(form, 0x80000000, 0xcd7, form.alias); stats.alias_cases++; }
guard(engine, a); guard(engine, b); guard(engine, counter);

for (const page of [0x8000, 0x9000]) assert.equal(engine.api.map(page, 1, 3), 0);
for (const form of forms.slice(0, 4)) for (const access of [1, 2]) {
  const address = access === 1 ? 0x8000 : 0x9000; assert.equal(engine.api.protect(address, 1, 3), 0); word(engine, address, 0x10); assert.equal(engine.api.protect(address, 1, access === 1 ? 2 : 1), 0);
  const start = startFor(form, false); start.registers[3] = address; reset(engine, start); const stopped = prefixed(start);
  run(engine, a, 8, stopped, exit(5, 1, 2, address, 4, 2, access), helper(4, 0, 2, address, access), 'first read or second store permission failure preserves unary flags', 'initial', access === 2 ? 1 : 0, 1); stats.faults++; stats[access === 1 ? 'read_faults' : 'store_faults']++;
  assert.equal(engine.api.protect(address, 1, 3), 0); observe(engine, address, 4, 0x10); word(engine, address, 0x80000000);
  const expected = applied(stopped, form.kind, 0x80000000, 0x2000); run(engine, a, 8, expected, exit(3, 2), storeHelper(), 'same CPU retries changed current RAM without repeating prefix', 'resume', 1, 1); observe(engine, address, 4, unary(form.kind, 0x80000000, stopped.eflags).value); stats.repairs++;
}
for (const operand of [0, 0x80000000]) {
  assert.equal(engine.api.protect(0x9000, 1, 3), 0); word(engine, 0x9000, operand); assert.equal(engine.api.protect(0x9000, 1, 1), 0); const form = forms[3], start = startFor(form, false); start.registers[3] = 0x9000; reset(engine, start); const stopped = prefixed(start);
  run(engine, a, 8, stopped, exit(5, 1, 2, 0x9000, 4), storeHelper(2, 0x9000), 'same-byte NEG still requires Write permission and leaves flags uncommitted', 'initial', 1, 1); observe(engine, 0x9000, 4, operand); stats.faults++; stats.store_faults++; stats.same_byte_permission_faults++;
}
assert.equal(engine.api.map(0xb000, 2, 3), 0);
for (const access of [1, 2]) {
  assert.equal(engine.api.protect(0xc000, 1, 3), 0); word(engine, 0xbffc, 0x11223344); word(engine, 0xc000, 0x55667788);
  if (access === 1) assert.equal(engine.api.unmap(0xc000, 1), 0); else assert.equal(engine.api.protect(0xc000, 1, 1), 0);
  const start = startFor(forms[1], false); start.registers[3] = 0xbfff; reset(engine, start); const stopped = prefixed(start), detail = access === 1 ? 1 : 2;
  run(engine, a, 8, stopped, exit(5, 1, detail, 0xc000, 4, 2, access), helper(4, 0, detail, 0xc000, access), 'cross-page access rejects before partial write or flags commit', 'initial', access === 2 ? 1 : 0, 1); observe(engine, 0xbffc, 4, 0x11223344); if (access === 2) observe(engine, 0xc000, 4, 0x55667788); stats.faults++; stats[access === 1 ? 'read_faults' : 'store_faults']++;
  if (access === 1) { assert.equal(engine.api.map(0xc000, 1, 3), 0); word(engine, 0xc000, 0x55667788); } else assert.equal(engine.api.protect(0xc000, 1, 3), 0); word(engine, 0xbfff, 0x80000000);
  run(engine, a, 8, applied(stopped, 'dec', 0x80000000, 0x2000), exit(3, 2), storeHelper(), 'cross-page retry reads repaired current operand', 'resume', 1, 1); observe(engine, 0xbffc, 4, 0xff223344); observe(engine, 0xc000, 4, 0x557fffff); stats.repairs++;
}
const overflow = startFor(forms[1], false); overflow.registers[3] = 0xffffffff; reset(engine, overflow); const overflowStopped = prefixed(overflow);
run(engine, a, 8, overflowStopped, exit(5, 1, 3, 0xffffffff, 4, 2, 1), helper(4, 0, 3, 0xffffffff), 'Read4 overflow suppresses Store4', 'initial', 0, 1); run(engine, a, 8, overflowStopped, exit(5, 0, 3, 0xffffffff, 4, 2, 1), helper(4, 0, 3, 0xffffffff), 'overflow retry never retires unary', 'resume', 0, 1); stats.faults += 2; stats.read_faults += 2;
assert.equal(engine.api.map(0xfffff000, 1, 3), 0); word(engine, 0xfffffff8, 0x11223344); word(engine, 0xfffffffc, 0x80000000); const top = startFor(forms[1], false); top.registers[3] = 0xfffffffc; reset(engine, top);
run(engine, a, 8, applied(prefixed(top), 'dec', 0x80000000, 0x2000), exit(3, 3), storeHelper(), 'last valid Read4/Store4 address', 'initial', 1, 1); observe(engine, 0xfffffff8, 4, 0x11223344); observe(engine, 0xfffffffc, 4, 0x7fffffff);

word(engine, DATA, 1); const split = startFor(forms[1], false); reset(engine, split); run(engine, a, 0, split, exit(1, 0), undefined, 'zero budget skips prefix and helpers', 'initial'); const prefix = prefixed(split);
run(engine, a, 1, prefix, exit(1, 1), undefined, 'exact prefix budget resumes at DEC', 'resume'); select(engine, 0x1101, a); engine.view.setUint32(engine.base + 96, 1, true); run(engine, a, 0, prefix, exit(2, 0), undefined, 'cancel precedes zero budget and both helpers', 'resume'); stats.cancellations++;
engine.view.setUint32(engine.base + 96, 0, true); const stored = applied(prefix, 'dec', 1, 0x1103); run(engine, a, 1, stored, exit(1, 1), storeHelper(), 'exact unary budget commits RAM and flags once', 'resume', 1, 1); word(engine, DATA, 0xfeedface);
run(engine, a, 1, at(stored, 0x2000), exit(1, 1), undefined, 'branch continues from committed unary flags', 'resume'); run(engine, a, 8, at(stored, 0x2000), exit(3, 0), undefined, 'external PC does not repeat unary', 'resume'); const read = at(stored, 0x2004); read.registers[2] = 0xfeedface; run(engine, b, 3, read, exit(3, 2), helper(4, 0xfeedface), 'reader proves retired DEC was not repeated', 'resume', 0, 1);
const preset = startFor(forms[0], false); reset(engine, preset, 1); run(engine, a, 8, preset, exit(2, 0), undefined, 'preset cancel suppresses Read4/Store4', 'initial'); stats.cancellations++;
const middle = initial(0x1102); reset(engine, middle); miss(engine, 0x1102); run(engine, a, 8, middle, exit(3, 0), undefined, 'middle instruction byte is not a resumable entry', 'initial');

const algorithmRows = [];
for (const n of [1, 3, 5]) {
  word(engine, DATA, n); const start = initial(0x3000); start.registers[3] = DATA; reset(engine, start); const budgets = n === 3 ? [1, 2, 3] : [2 * n + 1]; let retired = 0, calls = 0;
  for (const budget of budgets) {
    const next = Math.min(2 * n, retired + budget), completed = Math.ceil(next / 2), previous = Math.ceil(retired / 2), expected = at(start, next === 2 * n ? 0x3008 : next % 2 ? 0x3002 : 0x3000);
    if (completed) expected.eflags = unary('dec', n - completed + 1, start.eflags).flags;
    run(engine, counter, budget, expected, exit(next - retired === budget ? 1 : 3, next - retired), completed > previous ? storeHelper() : undefined, 'closed-form DEC memory/JNZ countdown', retired === 0 ? 'initial' : 'resume', completed - previous, completed - previous); observe(engine, DATA, 4, n - completed); retired = next; calls++;
  }
  assert.equal(retired, 2 * n); algorithmRows.push({n, retired, calls, ram: 0, flags: unary('dec', 1, start.eflags).flags}); stats.algorithm_cases++; stats.algorithm_retired += retired;
}
close(engine, [a, b, counter]);
const smcCases = [
  {name: 'self-inc', kind: 'inc', target: 'self', operand: 0x7fffffff}, {name: 'self-dec', kind: 'dec', target: 'self', operand: 0x80000000},
  {name: 'self-not', kind: 'not', target: 'self', operand: 0xaaaaaaaa}, {name: 'self-neg', kind: 'neg', target: 'self', operand: 1},
  {name: 'same-byte-zero', kind: 'neg', target: 'self', operand: 0}, {name: 'same-byte-min', kind: 'neg', target: 'self', operand: 0x80000000},
  {name: 'whole-unit', kind: 'not', target: 'whole', operand: 0xaaaaaaaa}, {name: 'shared-page', kind: 'dec', target: 'shared', operand: 0x80000000}, {name: 'separate-B', kind: 'inc', target: 'separate', operand: 0x7fffffff},
];
const smcRows = [];
for (const fixture of smcCases) {
  const e = fresh(), bpc = fixture.target === 'shared' ? 0x1800 : 0x2000, aWord = fixture.target === 'self' ? fixture.operand : 0x11111111, bWord = ['shared', 'separate'].includes(fixture.target) ? fixture.operand : 0x13579bdf;
  const destination = fixture.target === 'self' ? 0x1004 : fixture.target === 'whole' ? 0x4001 : bpc + 1;
  const instruction = Buffer.from(forms.find(form => form.kind === fixture.kind).hex, 'hex'), code = Buffer.alloc(13); code.set([0x47, ...instruction, 0xb9]); code.writeUInt32LE(aWord, 4); code[8] = 0xe9; code.writeInt32LE(bpc - 0x100d, 9);
  const control = Buffer.from('b900000000eb00', 'hex'); control.writeUInt32LE(bWord, 1); const other = Buffer.from(control); other.writeUInt32LE(fixture.target === 'whole' ? fixture.operand : 0xabcdef01, 1);
  authoredCode(`${fixture.name}-A`, code); authoredCode(`${fixture.name}-B`, control); authoredCode(`${fixture.name}-other-A`, other);
  assert.equal(e.api.map(0x1000, 1, 7), 0); if (bpc === 0x2000) assert.equal(e.api.map(bpc, 1, 7), 0); assert.equal(e.api.map(0x4000, 1, 7), 0);
  upload(e, 0x1000, code); upload(e, bpc, control); upload(e, 0x4000, other);
  const oldA = compile(e, [[0x1000, 13], [0x4000, 7]], ['read32'], `${fixture.name} A`, 1), oldB = compile(e, [[bpc, 7]], [], `${fixture.name} B`);
  const start = initial(0x1000); start.registers[3] = destination; start.eflags = fixture.name === 'same-byte-zero' ? 2 : 0xcd7; reset(e, start); const prefix = prefixed(start), result = unary(fixture.kind, fixture.operand, prefix.eflags), stopped = applied(prefix, fixture.kind, fixture.operand, 0x1003), invalidatesA = fixture.target !== 'separate';
  if (invalidatesA) run(e, oldA, 2, stopped, exit(6, 2), storeHelper(), 'post-store flags and RAM commit before invalidation/exact budget/successor', 'initial', 1, 1);
  else { const completed = at(stopped, bpc); completed.registers[1] = aWord; run(e, oldA, 8, completed, exit(3, 4), storeHelper(), 'separate B-code write leaves executing A current with new flags', 'initial', 1, 1); }
  observe(e, destination, 4, result.value); stats.smc_cases++; if (result.value === fixture.operand) stats.same_byte_code_writes++;
  smcRows.push({name: fixture.name, kind: fixture.kind, destination, old: fixture.operand, value: result.value, flags: result.flags, reason: invalidatesA ? 6 : 3, retired: invalidatesA ? 2 : 4}); guard(e, oldA, invalidatesA ? 4 : 0); guard(e, oldB, ['shared', 'separate'].includes(fixture.target) ? 4 : 0);
  if (invalidatesA) {
    miss(e, 0x1003, 4); if (fixture.target === 'whole') miss(e, 0x4000, 4);
    if (fixture.name === 'same-byte-min') {
      rejected(e, oldA, 4, 'same-byte code write makes original A stale'); const copied = new WebAssembly.Instance(new WebAssembly.Module(oldA.bytes.slice()), {env: {memory: e.memory}, ring3: {guard_resident: e.api.guard_resident, read32: e.api.read32, store_resident32: e.api.store_resident32}}); rejected(e, {...oldA, run: copied.exports.run}, 4, 'copied stale unary A retains baked identity');
    }
    const successor = compile(e, [[0x1003, 10]], [], `${fixture.name} fresh successor`); assert.ok(successor.id > oldA.id); select(e, 0x1003, successor);
    const continued = at(stopped, bpc); continued.registers[1] = fixture.target === 'self' ? result.value : aWord; run(e, successor, 8, continued, exit(3, 2, 0, 0, 0, 1), undefined, 'fresh next-PC unit never repeats retired unary or flags', 'resume');
    if (fixture.target === 'shared') { miss(e, bpc, 4); const freshB = compile(e, [[bpc, 7]], [], 'fresh shared B'); select(e, bpc, freshB); const done = at(continued, bpc + 7); done.registers[1] = result.value; run(e, freshB, 3, done, exit(3, 2, 0, 0, 0, 1), undefined, 'fresh shared B true continuation', 'resume'); }
    else { select(e, bpc, oldB); const done = at(continued, bpc + 7); done.registers[1] = bWord; run(e, oldB, 3, done, exit(3, 2, 0, 0, 0, 1), undefined, 'independent current B true continuation', 'resume'); }
  } else {
    const continued = at(stopped, bpc); continued.registers[1] = aWord; run(e, oldA, 8, continued, exit(3, 0), undefined, 'returning A preserves current RAM without repeating unary', 'resume'); rejected(e, oldB, 4, 'only separate B is stale'); const freshB = compile(e, [[bpc, 7]], [], 'fresh separate B'); select(e, bpc, freshB); const done = at(continued, bpc + 7); done.registers[1] = result.value; run(e, freshB, 3, done, exit(3, 2, 0, 0, 0, 1), undefined, 'fresh separate B consumes existing CPU', 'resume');
  }
  observe(e, destination, 4, result.value); close(e);
}
assert.equal(stats.form_cases, 56); assert.equal(stats.alias_cases, 4); assert.equal(stats.same_byte_permission_faults, 2); assert.equal(stats.smc_cases, 9); assert.equal(stats.same_byte_code_writes, 2); assert.equal(stats.algorithm_cases, 3); assert.equal(stats.algorithm_retired, 18);
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = ['engine/tests/process_resident_unary_wasm.rs', 'engine/tests/fixtures/p2-resident-unary/run.mjs', 'engine/tests/process_resident_unary.rs', 'engine/tests/process_resident_store_wasm.rs', 'engine/tests/fixtures/p2-resident-store/run.mjs', 'engine/tests/process_resident_store.rs', 'engine/tests/process_resident_memory.rs', 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', ...productionPaths];
const provenance = {
  stats, actual_resident_runs: stats.canonical_runs + stats.rejected_runs, actual_resident_guard_entries: stats.canonical_runs + stats.rejected_runs + stats.successful_guards + stats.failed_guards, bound_store_identity_checks: stats.guest_store32,
  guest_helper_calls_from_authored_execution: stats.guest_store32 + stats.guest_read32, host_observation_reads: stats.host_reads,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  authored: Object.fromEntries(Object.entries(authored).map(([name, bytes]) => [name, bytes.toString('hex')])), unit_bindings: units.map(unit => ({label: unit.label, id: unit.id.toString(), pointer: unit.pointer, length: unit.length, filename: unit.filename, reads: unit.reads, stores: unit.stores})), algorithm_rows: algorithmRows, smc_rows: smcRows, metadata_observations: metadata,
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_unary_wasm', '--', '--nocapture']},
  reused: 'previous25 read/control modules and native R3-320 atomic Store4/version exhaustion/identity/Busy evidence are not rerun; unchanged helper validation and late non0/non11 Infrastructure detail3 behavior are source/prior-proof reuse. The affected old MOV actual target is run separately and all27modules/26inputs compared by the parent. Four legacy unary baseline captures are parent-owned with zero guest CPU runs.',
  claim: 'actual Node engine-Wasm resident INC/DEC/NOT/NEG m32 through direct Read4 and immutable six-argument bound Store4 with the same engine memory, no legacy artifact. Exact guard0/read1/store2 and ADDRESS26/RESULT18 operands, independent literal/BigInt output/flags including CF/DF/NEG fixed points, preserved GPRs/neighbour RAM/full4236 arena/helperv1/Exitv2, first-read/second-store/atomic-cross-page/overflow/MAX-valid faults, prefix/current-RAM repair and budget/cancel/interior true continuations without host CPU patches. Post-store SMC flags/RAM/retirement commit once before next-PC recompile, whole/shared/separate B isolation and stale copied binding, bounded real DEC-memory/JNZ2*n mathematics. Helper counts authored-path derived, not instrumentation. No helper interception, hardware atomic bus LOCK/interleaving/concurrency, binary/narrow/stack/gate writes, product installer/dispatcher/table, browser/asynchronous cancellation/performance/game/fullP2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2)); console.log(JSON.stringify({...provenance, sources: Object.keys(provenance.sources).length, metadata_observations: metadata.length, artifacts: outputDir}));
