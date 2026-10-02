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
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'generation', 'module_ptr', 'module_len', 'guard_resident', 'compile_resident', 'resident_module', 'find_resident', 'read8', 'read16', 'read32', 'write32'];
const stats = {engine_instances: 0, compiled_units: 0, canonical_runs: 0, rejected_runs: 0, full_arena_checks: 0, metadata_checks: 0, successful_guards: 0, failed_guards: 0, find_success: 0, find_failure: 0, continuation_initial_calls: 0, continuation_resumes: 0, form_cases: 0, faults: 0, repairs: 0, cancellations: 0, host_reads: 0, host_writes: 0, guest_read8: 0, guest_read16: 0, guest_read32: 0};
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
  assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_ptr(), 0); assert.equal(engine.api.module_len(), 0);
}
function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const key = 0x9123456780000000n + BigInt(++stats.engine_instances);
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
function compile(engine, blocks, reads, label) {
  descriptors(engine, blocks); const before = arena(engine); assert.equal(engine.api.compile_resident(blocks.length), 0, label);
  const selected = record(engine, before, label), module = new WebAssembly.Module(selected.bytes);
  const expected = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard_resident', kind: 'function'}, ...reads.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), expected, 'exact direct guard/read imports, no store'); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const imports = {guard_resident: engine.api.guard_resident, ...Object.fromEntries(reads.map(name => [name, engine.api[name]]))};
  const instance = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: imports}); assert.equal(instance.exports.run.length, 4);
  const filename = `unit-${++stats.compiled_units}.wasm`; writeFileSync(join(outputDir, filename), selected.bytes); artifacts[filename] = hash(selected.bytes);
  const unit = {...selected, module, instance, run: instance.exports.run, blocks, reads, filename, label}; units.push(unit); return unit;
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
function flags(kind, left, right, oldFlags, preserveCarry = false) {
  const a = BigInt(left), b = BigInt(right);
  const arithmetic = ['add', 'sub', 'cmp'].includes(kind), add = kind === 'add';
  const raw = arithmetic ? add ? a + b : a - b : kind === 'or' ? a | b : kind === 'xor' ? a ^ b : a & b;
  const value = Number(BigInt.asUintN(32, raw)); let result = (oldFlags & 0x400) | 2;
  if (preserveCarry ? oldFlags & 1 : arithmetic && (add ? raw > 0xffffffffn : a < b)) result |= 1;
  if (arithmetic && (add ? (a & 15n) + (b & 15n) > 15n : (a & 15n) < (b & 15n))) result |= 0x10;
  if (arithmetic) { const signed = add ? BigInt.asIntN(32, a) + BigInt.asIntN(32, b) : BigInt.asIntN(32, a) - BigInt.asIntN(32, b); if (signed < -2147483648n || signed > 2147483647n) result |= 0x800; }
  if (value === 0) result |= 0x40; if (value >= 0x80000000) result |= 0x80;
  let ones = 0; for (let byte = value & 255; byte; byte >>>= 1) ones += byte & 1; if (ones % 2 === 0) result |= 4;
  return {value, flags: result};
}
for (const [kind, a, b, old, expected] of [['add', 0xffffffff, 1, 2, 0x57], ['add', 0x7fffffff, 1, 2, 0x896], ['sub', 0, 1, 2, 0x97], ['sub', 0x80000000, 1, 2, 0x816], ['and', 0, 0xffffffff, 0xcd7, 0x446], ['or', 0x80000000, 1, 2, 0x82]]) assert.equal(flags(kind, a, b, old).flags, expected, 'literal flag anchor');
function exit(reason, retired, detail = 0, address = 0, width = 0) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer); header(bytes, view, 0, 'R3EX', 40, 2);
  [reason, retired, detail, address, reason === 5 ? 1 : 0, width].forEach((v, i) => view.setUint32(16 + i * 4, v, true)); return bytes;
}
function helper(width, value, detail = 0, address = 0) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer); header(bytes, view, 0, 'R3MH', 40, width === 4 ? 1 : 2);
  [detail ? 1 : 0, detail ? 0 : value, detail, address, detail ? 1 : 0, width === 4 && detail === 0 ? 0 : width].forEach((v, i) => view.setUint32(16 + i * 4, v, true)); return bytes;
}
function run(engine, unit, budget, expected, expectedExit, expectedHelper, label, continuation, reads = {}) {
  const before = arena(engine); assert.equal(unit.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, label);
  const wanted = before.slice(), view = new DataView(wanted.buffer);
  expected.registers.forEach((r, i) => view.setUint32(16 + i * 4, r, true)); view.setUint32(48, expected.eip, true); view.setUint32(52, expected.eflags, true); wanted.set(expectedExit, 56);
  if (expectedHelper !== undefined) wanted.set(expectedHelper, 100);
  assert.deepEqual(arena(engine), wanted, `${label}: all4236 state/exit/helper/cancel/transfer bytes`); stats.canonical_runs++; stats.full_arena_checks++; noLegacy(engine);
  if (continuation === 'initial') stats.continuation_initial_calls++; else if (continuation === 'resume') stats.continuation_resumes++;
  for (const [width, count] of Object.entries(reads)) stats[`guest_read${Number(width) * 8}`] += count;
}
function word(engine, address, value) { assert.equal(engine.api.write32(address, value), 0); stats.host_writes++; refresh(engine); }
function observe(engine, address, width, expected) {
  const before = arena(engine); assert.equal(engine.api[`read${width * 8}`](address), 0); stats.host_reads++;
  const wanted = before.slice(); wanted.set(helper(width, expected), 100); assert.deepEqual(arena(engine), wanted, 'host observation only replaces helper40'); stats.full_arena_checks++;
}
function jump(pc, target) { const bytes = Buffer.alloc(5); bytes[0] = 0xe9; bytes.writeInt32LE(target - pc - 5, 1); return bytes; }
const forms = [
  {name: 'mov8b', hex: '8b00', width: 4, kind: 'mov', destination: 0, alias: 0},
  {name: 'movA1', hex: 'a100500000', width: 4, kind: 'mov', destination: 0},
  {name: 'movzx8', hex: '0fb62424', width: 1, kind: 'zero', destination: 4, alias: 4},
  {name: 'movsx8', hex: '0fbe06', width: 1, kind: 'sign', destination: 0},
  {name: 'movzx16', hex: '0fb706', width: 2, kind: 'zero', destination: 0},
  {name: 'movsx16', hex: '0fbf06', width: 2, kind: 'sign', destination: 0},
  {name: 'add', hex: '0300', width: 4, kind: 'add', destination: 0, alias: 0},
  {name: 'sub', hex: '2b06', width: 4, kind: 'sub', destination: 0},
  {name: 'cmp3b', hex: '3b06', width: 4, kind: 'cmp', order: 'register'},
  {name: 'and', hex: '2306', width: 4, kind: 'and', destination: 0},
  {name: 'or', hex: '0b06', width: 4, kind: 'or', destination: 0},
  {name: 'xor', hex: '3306', width: 4, kind: 'xor', destination: 0},
  {name: 'cmp39', hex: '3906', width: 4, kind: 'cmp', order: 'memory'},
  {name: 'cmp81', hex: '813effffff7f', width: 4, kind: 'cmp', order: 'memory', immediate: 0x7fffffff},
  {name: 'cmp83', hex: '833eff', width: 4, kind: 'cmp', order: 'memory', immediate: 0xffffffff},
  {name: 'test85', hex: '8506', width: 4, kind: 'test', order: 'memory'},
  {name: 'testF7', hex: 'f70601000080', width: 4, kind: 'test', order: 'memory', immediate: 0x80000001},
];
function startFor(form, address = DATA) {
  const state = initial(form.pc); state.registers[4] = address; state.registers[6] = address;
  if (form.alias !== undefined) state.registers[form.alias] = address; return state;
}
function prefix(form, start) {
  const value = copy(start), result = flags('add', start.registers[7], 1, start.eflags, true); value.registers[7] = result.value; value.eflags = result.flags; value.eip = form.pc + 1; return value;
}
function loaded(form, start, wordValue) {
  const value = prefix(form, start), raw = Number(BigInt.asUintN(form.width * 8, BigInt(wordValue)));
  if (['mov', 'zero', 'sign'].includes(form.kind)) value.registers[form.destination] = form.kind === 'sign' ? Number(BigInt.asUintN(32, BigInt.asIntN(form.width * 8, BigInt(raw)))) : raw;
  else {
    const left = form.order === 'memory' ? raw : start.registers[0], right = form.order === 'memory' ? form.immediate ?? start.registers[0] : raw;
    const result = flags(form.kind, left, right, value.eflags); value.eflags = result.flags; if (form.destination !== undefined) value.registers[form.destination] = result.value;
  }
  value.eip = form.pc + 1 + form.bytes.length; return value;
}
function completed(form, start, wordValue) { const value = loaded(form, start, wordValue); value.eip = 0x2000; return value; }
const bBytes = Buffer.from('8b1e851e0f85f60f0000', 'hex');
function bResult(start, wordValue) { const value = copy(start); value.registers[3] = wordValue; value.eflags = flags('test', wordValue, wordValue, start.eflags).flags; value.eip = wordValue === 0 ? 0x200a : 0x3000; return value; }
const contexts = [];
for (const offset of [0, 6, 12]) {
  const engine = fresh(), group = forms.slice(offset, offset + 6);
  assert.equal(engine.api.map(0x1000, 2, 7), 0); assert.equal(engine.api.map(DATA, 2, 3), 0);
  group.forEach((form, i) => {
    form.pc = 0x1000 + i * 32; form.bytes = Buffer.from(form.hex, 'hex'); form.program = Buffer.concat([Buffer.from([0x47]), form.bytes, jump(form.pc + 1 + form.bytes.length, 0x2000)]);
    upload(engine, form.pc, form.program); const filename = `${form.name}.x86`; writeFileSync(join(outputDir, filename), form.program); artifacts[filename] = hash(form.program);
  });
  upload(engine, 0x2000, bBytes);
  const reads = [4, 1, 2].filter(width => group.some(form => form.width === width)).map(width => `read${width * 8}`);
  const a = compile(engine, group.map(form => [form.pc, form.program.length]), reads, `A forms${offset}`), b = compile(engine, [[0x2000, bBytes.length]], ['read32'], `B forms${offset}`);
  contexts.push({engine, group, a, b}); assert.notEqual(a.id, b.id);
  for (const form of group) for (const wordValue of [0, 0x800080ff, 0xffffffff, 0x7fffffff]) {
    word(engine, DATA, wordValue); const start = startFor(form); reset(engine, start); select(engine, form.pc + 1, a);
    const afterA = completed(form, start, wordValue); run(engine, a, 4, afterA, exit(3, 3), helper(form.width, Number(BigInt.asUintN(form.width * 8, BigInt(wordValue)))), `${form.name} live RAM/flags`, 'initial', {[form.width]: 1});
    run(engine, b, 4, bResult(afterA, wordValue), exit(3, 3), helper(4, wordValue), 'B reads live RAM and branches on TEST', 'resume', {4: 2});
    observe(engine, DATA, 4, wordValue); stats.form_cases++;
  }
  guard(engine, a); guard(engine, b);
}
writeFileSync(join(outputDir, 'b.x86'), bBytes); artifacts['b.x86'] = hash(bBytes);

const {engine, a, b} = contexts[0], first = forms[0];
assert.equal(engine.api.map(0x7000, 1, 3), 0);
const faultForms = [forms[2], forms[4], forms[0]];
for (const [index, form] of faultForms.entries()) {
  const unmapped = 0x9000 + index * 0x2000, denied = 0x8000 + index * 0x2000;
  assert.equal(engine.api.map(denied, 1, 3), 0); word(engine, denied, 0x11111111); assert.equal(engine.api.protect(denied, 1, 4), 0);
  const cases = [{address: unmapped, detail: 1, at: unmapped, repair: 'map'}, {address: denied, detail: 2, at: denied, repair: 'permission'}, {address: form.width === 1 ? 0x7000 : 0x6fff, detail: 1, at: 0x7000, repair: 'cross'}];
  if (form.width > 1) cases.push({address: 0xffffffff, detail: 3, at: 0xffffffff});
  for (const fault of cases) {
    if (fault.repair === 'cross') { assert.equal(engine.api.unmap(0x7000, 1), 0); word(engine, 0x6ffc, 0x11223344); }
    const start = startFor(form, fault.address), stopped = prefix(form, start); reset(engine, start);
    run(engine, a, 4, stopped, exit(5, 1, fault.detail, fault.at, form.width), helper(form.width, 0, fault.detail, fault.at), 'fault preserves load destination and INC prefix flags', 'initial', {[form.width]: 1}); stats.faults++;
    if (fault.repair) {
      if (fault.repair === 'permission') assert.equal(engine.api.protect(denied, 1, 3), 0);
      else assert.equal(engine.api.map(fault.repair === 'cross' ? 0x7000 : unmapped, 1, 3), 0);
      const repaired = 0x876580ff; word(engine, fault.address, repaired);
      if (fault.repair === 'permission') assert.equal(engine.api.protect(denied, 1, 1), 0);
      const expected = completed(form, start, repaired);
      run(engine, a, 3, expected, exit(3, 2), helper(form.width, Number(BigInt.asUintN(form.width * 8, BigInt(repaired)))), 'same unit retries current repaired RAM without CPU patch', 'resume', {[form.width]: 1}); stats.repairs++;
      observe(engine, fault.address, 4, repaired); guard(engine, a); guard(engine, b);
    } else {
      run(engine, a, 4, stopped, exit(5, 0, fault.detail, fault.at, form.width), helper(form.width, 0, fault.detail, fault.at), 'overflow repeats zero retirement without CPU patch', 'resume', {[form.width]: 1}); stats.faults++;
    }
  }
}
assert.equal(engine.api.map(0xfffff000, 1, 3), 0); word(engine, 0xfffffffc, 0x80000000);
const topForm = forms[2], topStart = startFor(topForm, 0xffffffff); reset(engine, topStart);
run(engine, a, 4, completed(topForm, topStart, 0x80), exit(3, 3), helper(1, 0x80), 'read8 at unsigned maximum succeeds without address overflow', 'initial', {1: 1}); observe(engine, 0xffffffff, 1, 0x80);
assert.equal(engine.api.unmap(0x7000, 1), 0); word(engine, 0x6ffc, 0x80000000); const edgeStart = startFor(topForm, 0x6fff); reset(engine, edgeStart);
run(engine, a, 4, completed(topForm, edgeStart, 0x80), exit(3, 3), helper(1, 0x80), 'read8 last byte of mapped page does not require next page', 'initial', {1: 1}); observe(engine, 0x6fff, 1, 0x80);

const splitForm = forms[4], splitValue = 0x876580ff, splitStart = startFor(splitForm); word(engine, DATA, splitValue); reset(engine, splitStart);
run(engine, a, 0, splitStart, exit(1, 0), undefined, 'memory unit zero budget uses Exitv2', 'initial');
const splitPrefix = prefix(splitForm, splitStart); run(engine, a, 1, splitPrefix, exit(1, 1), undefined, 'budget stops before read at exact interior PC', 'resume');
run(engine, a, 0, splitPrefix, exit(1, 0), undefined, 'zero budget resumes no prefix twice', 'resume');
engine.view.setUint32(engine.base + 96, 1, true); run(engine, a, 0, splitPrefix, exit(2, 0), undefined, 'cancel between resumes precedes zero budget and read', 'resume'); stats.cancellations++; engine.view.setUint32(engine.base + 96, 0, true);
const splitLoaded = loaded(splitForm, splitStart, splitValue); run(engine, a, 1, splitLoaded, exit(1, 1), helper(2, 0x80ff), 'one interior read resumes with current RAM', 'resume', {2: 1});
const splitDone = completed(splitForm, splitStart, splitValue); run(engine, a, 1, splitDone, exit(1, 1), undefined, 'JMP commits before exact budget exhaustion', 'resume');
const splitB = bResult(splitDone, splitValue); run(engine, b, 3, splitB, exit(1, 3), helper(4, splitValue), 'B completes exact retirement budget', 'resume', {4: 2}); run(engine, b, 1, splitB, exit(3, 0), undefined, 'completed B repeats no memory read', 'resume');
select(engine, splitForm.pc + 1, a); guard(engine, a); guard(engine, b); observe(engine, DATA, 4, splitValue);
const preset = startFor(forms[3]); reset(engine, preset, 1); run(engine, a, 10, preset, exit(2, 0), undefined, 'preset cancel precedes read8 and prefix', 'initial'); stats.cancellations++;

// helper operands are guest addresses even when numerically equal to engine-linear pointers.
for (const address of [engine.base + TRANSFER, a.pointer]) {
  const page = Math.floor(address / 4096) * 4096; assert.ok(page > 0x100000 && address % 4096 <= 4092);
  const start = startFor(first, address), stopped = prefix(first, start); reset(engine, start);
  run(engine, a, 4, stopped, exit(5, 1, 1, address, 4), helper(4, 0, 1, address), 'numeric engine pointer has no implicit guest mapping', 'initial', {4: 1}); stats.faults++;
  const linear = refresh(engine).view.getUint32(address, true), guest = linear === 0x11223344 ? 0x55667788 : 0x11223344;
  assert.equal(engine.api.map(page, 1, 3), 0); word(engine, address, guest);
  run(engine, a, 3, completed(first, start, guest), exit(3, 2), helper(4, guest), 'mapped guest pointer alias reads independent RAM', 'resume', {4: 1}); stats.repairs++;
  assert.equal(refresh(engine).view.getUint32(address, true), linear, 'guest mapping does not expose or overwrite engine bytes'); observe(engine, address, 4, guest); assert.equal(engine.api.unmap(page, 1), 0); guard(engine, a); guard(engine, b);
}

word(engine, DATA, 0x12345678); guard(engine, a); guard(engine, b); select(engine, 0x2000, b);
assert.equal(engine.api.write32(first.pc, first.program.readUInt32LE(0)), 0); stats.host_writes++;
guard(engine, a, 4); guard(engine, b); rejected(engine, a, 4, 'stale A rejects before all read helpers');
unchanged(engine, () => engine.api.find_resident(first.pc), 4, 'stale A lookup'); stats.find_failure++;
const copied = new WebAssembly.Instance(new WebAssembly.Module(a.bytes.slice()), {env: {memory: engine.memory}, ring3: Object.fromEntries(['guard_resident', ...a.reads].map(name => [name, engine.api[name]]))});
rejected(engine, {...a, run: copied.exports.run}, 4, 'copied stale memory unit retains immutable guard binding');
const independentB = initial(0x2000); reset(engine, independentB); run(engine, b, 4, bResult(independentB, 0x12345678), exit(3, 3), helper(4, 0x12345678), 'current B reads live RAM without any legacy artifact', 'initial', {4: 2});
const freshA = compile(engine, a.blocks, a.reads, 'fresh same-PC read A'); assert.ok(freshA.id > a.id && freshA.id > b.id); select(engine, first.pc, freshA); select(engine, 0x2000, b); guard(engine, a, 4); guard(engine, freshA);
const replacementStart = startFor(first); reset(engine, replacementStart); const replacementDone = completed(first, replacementStart, 0x12345678);
run(engine, freshA, 4, replacementDone, exit(3, 3), helper(4, 0x12345678), 'fresh A uses current memory under new ID', 'initial', {4: 1}); run(engine, b, 4, bResult(replacementDone, 0x12345678), exit(3, 3), helper(4, 0x12345678), 'B continuation survives A replacement', 'resume', {4: 2});
observe(engine, DATA, 4, 0x12345678);
const beforeClose = arena(engine); assert.equal(engine.api.close(), 0); assert.deepEqual(arena(engine), beforeClose); stats.full_arena_checks++;
rejected(engine, freshA, 5, 'closed memory unit rejects before read'); rejected(engine, b, 5, 'closed retained read unit rejects');

assert.equal(stats.form_cases, 17 * 4); assert.equal(stats.compiled_units, 7);
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = ['engine/tests/process_resident_memory_wasm.rs', 'engine/tests/fixtures/p2-resident-memory/run.mjs', 'engine/tests/process_resident_memory.rs', 'engine/tests/process_resident.rs', 'engine/tests/fixtures/p2-resident-process/run.mjs', 'engine/tests/process_resident_wasm.rs', 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', ...productionPaths];
const provenance = {
  stats, actual_resident_runs: stats.canonical_runs + stats.rejected_runs, actual_resident_guard_entries: stats.canonical_runs + stats.rejected_runs + stats.successful_guards + stats.failed_guards,
  guest_helper_calls_from_authored_execution: stats.guest_read8 + stats.guest_read16 + stats.guest_read32, host_observation_reads: stats.host_reads,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  forms: forms.map(form => ({name: form.name, hex: form.hex, width: form.width, pc: form.pc})), unit_bindings: units.map(unit => ({label: unit.label, id: unit.id.toString(), pointer: unit.pointer, length: unit.length, filename: unit.filename, reads: unit.reads})), metadata_observations: metadata,
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_memory_wasm', '--', '--nocapture']},
  claim: 'actual engine-Wasm resident data reads for all17 existing encoding forms; seven compiled units across three independent two-unit contexts plus one fresh replacement, direct Wasm guard/read imports and exact same engine memory, no store or legacy artifact, explicit whole-arena metadata/CPU/flags/Exit-v2/helper-v1-v2 oracles, live RAM and independent source/destination EA aliases, width-specific fault/prefix/repair/interior budget/cancel resumes without CPU patches, maximum-byte read success and word/dword overflow, code-stale A/current B/fresh A and copied stale binding, engine-linear arena/module pointers resolved solely through guest mapping. Independent literal/BigInt mathematics; no decoder/ISA interpreter/helper simulation. No product dispatcher/installer, indirect jump, resident Store4/active SMC, callbacks, browser/async cancellation/performance/game claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2)); console.log(JSON.stringify({...provenance, sources: Object.keys(provenance.sources).length, metadata_observations: metadata.length, artifacts: outputDir}));
