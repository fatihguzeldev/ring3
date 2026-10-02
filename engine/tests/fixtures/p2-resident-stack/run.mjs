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
const SIZE = 4236, TRANSFER = 140, DATA = 0x8000, STACK = 0xa100, DEST = 0xc000, BPC = 0x6000;
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'generation', 'module_ptr', 'module_len', 'guard_resident', 'compile_resident', 'resident_module', 'find_resident', 'read32', 'write32', 'store_resident32'];
const stats = {engine_instances: 0, compiled_units: 0, canonical_runs: 0, rejected_runs: 0, full_arena_checks: 0, metadata_checks: 0, successful_guards: 0, failed_guards: 0, find_success: 0, find_failure: 0, continuation_initial_calls: 0, continuation_resumes: 0, register_cases: 0, push_only_cases: 0, pop_cases: 0, memory_cases: 0, overlap_cases: 0, smc_cases: 0, same_byte_code_writes: 0, same_byte_permission_faults: 0, bound_store_sites: 0, original_register_sites: 0, immediate_sites: 0, result_sites: 0, algorithm_cases: 0, algorithm_retired: 0, faults: 0, read_faults: 0, store_faults: 0, repairs: 0, cancellations: 0, host_reads: 0, host_writes: 0, guest_read32: 0, guest_store32: 0};
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
  const key = 0x97789abcc0000000n + BigInt(++stats.engine_instances);
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
    assert.deepEqual(operand, operands[found], 'original GPR/immediate or captured RESULT18 as appropriate');
    assert.equal(body[c.i++], 0x10); assert.equal(uleb(body, c), index); stats[operand.local === 18 ? 'result_sites' : operand.local !== undefined ? 'original_register_sites' : 'immediate_sites']++; found++;
  }
  assert.equal(found, operands.length, 'each Store4 call has immutable identity, ADDRESS26 and exact value operand'); stats.bound_store_sites += found;
}
function miss(engine, pc, status = 17) { unchanged(engine, () => engine.api.find_resident(pc), status, 'finder failure'); stats.find_failure++; }
function at(value, pc) { const next = copy(value); next.eip = pc; return next; }
function prefixed(start) { const value = copy(start), result = arithmetic('inc', value.registers[7], value.eflags); value.registers[7] = result.value; value.eflags = result.flags; value.eip = start.eip + 1; return value; }
function completed(start, kind, pc) { const value = at(start, pc); value.registers[4] = kind === 'push' ? (start.registers[4] - 4) >>> 0 : (start.registers[4] + 4) >>> 0; return value; }
function storeHelper(detail = 0, address = 0) { return helper(4, 0, detail, address, 2); }
function close(engine, selected = []) { const before = arena(engine); assert.equal(engine.api.close(), 0); assert.deepEqual(arena(engine), before); stats.full_arena_checks++; for (const unit of selected) rejected(engine, unit, 5, 'closed resident module'); }
const authored = {};
function authoredCode(name, bytes) { const filename = `${name}.x86`; writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes); authored[name] = bytes; return bytes; }
function block(pc, instruction) { const bytes = Buffer.alloc(1 + instruction.length + 5); bytes[0] = 0x47; instruction.copy(bytes, 1); bytes[1 + instruction.length] = 0xe9; bytes.writeInt32LE(BPC - (pc + bytes.length), 2 + instruction.length); return bytes; }
function modelWord(image, address, value) { for (let i = 0; i < 4; i++) image.set(address + i, Number((BigInt(value) >> BigInt(i * 8)) & 255n)); }
function modelValue(image, address) { let value = 0n; for (let i = 0; i < 4; i++) { assert.ok(image.has(address + i)); value |= BigInt(image.get(address + i)) << BigInt(i * 8); } return Number(value); }
function prepareRAM(engine, source, destination, input) {
  const watched = [...new Set([source, destination].flatMap(address => { const aligned = Math.floor(address / 4) * 4; return [aligned - 4, aligned, aligned + 4, ...(address % 4 ? [aligned + 8] : [])]; }).filter(address => address >= 0 && address <= 0xfffffffc))].sort((a, b) => a - b), image = new Map();
  assert.ok(watched.every(address => address < 0x1000 || address >= 0x7000), 'all data/stack/adjacent-neighbour words are outside code pages1000..6fff');
  for (const address of watched) { word(engine, address, 0x11223344); modelWord(image, address, 0x11223344); }
  word(engine, source, input); modelWord(image, source, input); return {watched, image};
}
function observeRAM(engine, ram) { for (const address of ram.watched) observe(engine, address, 4, modelValue(ram.image, address)); }
const registers = Array.from({length: 8}, (_, register) => ({register, pc: 0x1000 + register * 0x100, instruction: Buffer.from([0x50 + register, 0x58 + register, 0xff, 0xf0 + register, 0x8f, 0xc0 + register])}));
for (const form of registers) form.code = authoredCode(`register-${form.register}`, block(form.pc, form.instruction));
const immediates = [
  {hex: '68efcdab89', value: 0x89abcdef}, {hex: '6a80', value: 0xffffff80}, {hex: '6aff', value: 0xffffffff}, {hex: '6a00', value: 0}, {hex: '6a7f', value: 127}, {hex: '54', register: 4},
].map((form, i) => ({...form, pc: 0x2000 + i * 0x100, instruction: Buffer.from(form.hex, 'hex')}));
for (const form of immediates) form.code = authoredCode(`push-only-${form.pc.toString(16)}`, block(form.pc, form.instruction));
const pops = [{hex: '58', register: 0}, {hex: '8fc0', register: 0}, {hex: '5c', register: 4}, {hex: '8fc4', register: 4}].map((form, i) => ({...form, pc: 0x4000 + i * 0x100, instruction: Buffer.from(form.hex, 'hex')}));
for (const form of pops) form.code = authoredCode(`pop-only-${form.pc.toString(16)}`, block(form.pc, form.instruction));
const memoryForms = [
  {name: 'push-other', kind: 'push', hex: 'ff33', source: DATA, destination: STACK - 4},
  {name: 'push-old-esp', kind: 'push', hex: 'ff3424', source: STACK, destination: STACK - 4},
  {name: 'push-exact', kind: 'push', hex: 'ff7424fc', source: STACK - 4, destination: STACK - 4, overlap: true},
  {name: 'push-partial', kind: 'push', hex: 'ff7424fe', source: STACK - 2, destination: STACK - 4, overlap: true},
  {name: 'pop-other', kind: 'pop', hex: '8f03', source: STACK, destination: DEST},
  {name: 'pop-pending-esp', kind: 'pop', hex: '8f0424', source: STACK, destination: STACK + 4},
  {name: 'pop-exact', kind: 'pop', hex: '8f4424fc', source: STACK, destination: STACK, overlap: true},
  {name: 'pop-pending-sib', kind: 'pop', hex: '8f448c04', source: STACK, destination: STACK + 16},
  {name: 'push-sib', kind: 'push', hex: 'ff748c04', source: STACK + 12, destination: STACK - 4},
  {name: 'pop-partial', kind: 'pop', hex: '8f4424fa', source: STACK, destination: STACK - 2, overlap: true},
].map((form, i) => ({...form, pc: 0x3000 + i * 0x100, instruction: Buffer.from(form.hex, 'hex')}));
for (const form of memoryForms) form.code = authoredCode(form.name, block(form.pc, form.instruction));
const loopBytes = authoredCode('copy', Buffer.from('ff368f078d76048d7f04490f85efffffff', 'hex')), control = authoredCode('control', Buffer.from('90eb00', 'hex'));
const engine = fresh(); assert.equal(engine.api.store_resident32.length, 6); for (const page of [0x1000, 0x2000, 0x3000, 0x4000, 0x5000, BPC]) assert.equal(engine.api.map(page, 1, 7), 0); assert.equal(engine.api.map(0x7000, 7, 3), 0);
for (const form of [...registers, ...immediates, ...pops, ...memoryForms]) upload(engine, form.pc, form.code); upload(engine, 0x5000, loopBytes); upload(engine, BPC, control);
const registerUnit = compile(engine, registers.map(form => [form.pc, form.code.length]), ['read32'], 'all short and ModRM register PUSH/POP aliases', registers.flatMap(form => [{local: 4 + form.register}, {local: 4 + form.register}]));
const pushUnit = compile(engine, immediates.map(form => [form.pc, form.code.length]), [], 'PUSH-only original ESP and signed/full immediates', immediates.map(form => form.register === undefined ? {immediate: form.value} : {local: 8}));
const popUnit = compile(engine, pops.map(form => [form.pc, form.code.length]), ['read32'], 'POP-only ordinary/ModRM and loaded-value ESP');
for (const selected of [memoryForms.slice(0, 8), memoryForms.slice(8)]) { const unit = compile(engine, selected.map(form => [form.pc, form.code.length]), ['read32'], 'old/pending EA and captured memory transfers', selected.map(() => ({local: 18}))); for (const form of selected) form.unit = unit; }
const copyUnit = compile(engine, [[0x5000, loopBytes.length]], ['read32'], 'PUSH memory/POP memory copy with LEA/DEC/JNZ', [{local: 18}, {local: 18}]), b = compile(engine, [[BPC, control.length]], [], 'register-only continuation');
function continueB(expected) { run(engine, b, 3, at(expected, BPC + 3), exit(3, 2, 0, 0, 0, 1), undefined, 'B consumes existing CPU without another stack access', 'resume'); }
for (const form of registers) {
  select(engine, form.pc, registerUnit); select(engine, form.pc + 1, registerUnit);
  for (const flags of [2, 0xcd7]) {
    const start = initial(form.pc + 1); start.eflags = flags; reset(engine, start); for (const [address, value] of [[STACK - 8, 0x11223344], [STACK - 4, 0x55667788], [STACK, 0x99aabbcc]]) word(engine, address, value);
    const value = start.registers[form.register], expected = at(start, BPC); run(engine, registerUnit, 6, expected, exit(3, 5), helper(4, value), 'all GPRs short/ModRM PUSH POP preserve original values and full flags', 'initial', 2, 2);
    observe(engine, STACK - 8, 4, 0x11223344); observe(engine, STACK - 4, 4, value); observe(engine, STACK, 4, 0x99aabbcc); continueB(expected); stats.register_cases++;
  }
}
for (const form of immediates) {
  select(engine, form.pc + 1, pushUnit);
  for (const flags of [2, 0xcd7]) {
    const start = initial(form.pc + 1); start.eflags = flags; reset(engine, start); for (const [address, value] of [[STACK - 8, 0x11223344], [STACK - 4, 0x55667788], [STACK, 0x99aabbcc]]) word(engine, address, value);
    const value = form.register === undefined ? form.value : STACK, expected = completed(start, 'push', BPC); run(engine, pushUnit, 3, expected, exit(3, 2), storeHelper(), 'PUSH-only signed/full immediate or original ESP commits once', 'initial', 1);
    observe(engine, STACK - 8, 4, 0x11223344); observe(engine, STACK - 4, 4, value); observe(engine, STACK, 4, 0x99aabbcc); continueB(expected); stats.push_only_cases++;
  }
}
for (const form of pops) {
  select(engine, form.pc + 1, popUnit);
  for (const flags of [2, 0xcd7]) {
    const start = initial(form.pc + 1); start.eflags = flags; reset(engine, start); for (const [address, value] of [[STACK - 4, 0x11223344], [STACK, 0x2468ace0], [STACK + 4, 0x55667788]]) word(engine, address, value);
    const expected = completed(start, 'pop', BPC); expected.registers[form.register] = 0x2468ace0; run(engine, popUnit, 3, expected, exit(3, 2), helper(4, 0x2468ace0), 'POP-only ESP loaded value wins over pending increment', 'initial', 0, 1);
    observe(engine, STACK - 4, 4, 0x11223344); observe(engine, STACK, 4, 0x2468ace0); observe(engine, STACK + 4, 4, 0x55667788); continueB(expected); stats.pop_cases++;
  }
}
function memoryStart(form, interior = true) { const start = initial(form.pc + Number(interior)); start.registers[1] = 2; start.registers[3] = form.kind === 'push' ? DATA : DEST; return start; }
for (const form of memoryForms) {
  select(engine, form.pc + 1, form.unit);
  for (const flags of [2, 0xcd7]) {
    const ram = prepareRAM(engine, form.source, form.destination, 0x89abcdef), start = memoryStart(form); start.eflags = flags; reset(engine, start); const expected = completed(start, form.kind, BPC);
    run(engine, form.unit, 3, expected, exit(3, 2), storeHelper(), 'memory transfer captures before overlap and defers ESP', 'initial', 1, 1); modelWord(ram.image, form.destination, 0x89abcdef); observeRAM(engine, ram); continueB(expected); stats.memory_cases++; if (form.overlap) stats.overlap_cases++;
  }
}
for (const unit of [registerUnit, pushUnit, popUnit, memoryForms[0].unit, memoryForms[8].unit, copyUnit, b]) guard(engine, unit);
select(engine, BPC, b);
function addressed(form, source, destination) { const start = memoryStart(form, false); start.registers[4] = form.kind === 'push' ? (destination + 4) >>> 0 : source; start.registers[3] = form.kind === 'push' ? source : destination; return start; }
for (const page of [0xe000, 0xf000]) assert.equal(engine.api.map(page, 1, 3), 0);
for (const form of [memoryForms[0], memoryForms[4]]) for (const access of [1, 2]) {
  for (const page of [0xe000, 0xf000]) assert.equal(engine.api.protect(page, 1, 3), 0); const source = 0xe100, destination = 0xf100, ram = prepareRAM(engine, source, destination, 0x10203040), address = access === 1 ? source : destination;
  assert.equal(engine.api.protect(access === 1 ? 0xe000 : 0xf000, 1, access === 1 ? 2 : 1), 0); const start = addressed(form, source, destination); reset(engine, start); const stopped = prefixed(start);
  run(engine, form.unit, 8, stopped, exit(5, 1, 2, address, 4, 2, access), helper(4, 0, 2, address, access), 'memory first-read/second-store permission failure retains old ESP and prefix flags', 'initial', access === 2 ? 1 : 0, 1); stats.faults++; stats[access === 1 ? 'read_faults' : 'store_faults']++;
  assert.equal(engine.api.protect(access === 1 ? 0xe000 : 0xf000, 1, 3), 0); observeRAM(engine, ram); word(engine, source, 0x80000000); modelWord(ram.image, source, 0x80000000);
  run(engine, form.unit, 8, completed(stopped, form.kind, BPC), exit(3, 2), storeHelper(), 'repair rereads changed sourceEA or popped old-stack word without prefix replay', 'resume', 1, 1); modelWord(ram.image, destination, 0x80000000); observeRAM(engine, ram); stats.repairs++;
}
assert.equal(engine.api.protect(0xf000, 1, 3), 0); word(engine, 0xf100, 0x11223344); assert.equal(engine.api.protect(0xf000, 1, 1), 0);
const pushESP = immediates[5], failedPush = initial(pushESP.pc); failedPush.registers[4] = 0xf104; reset(engine, failedPush); const stoppedPush = prefixed(failedPush);
run(engine, pushUnit, 8, stoppedPush, exit(5, 1, 2, 0xf100, 4), storeHelper(2, 0xf100), 'PUSH original ESP single Store4 failure commits no decrement', 'initial', 1); stats.faults++; stats.store_faults++;
assert.equal(engine.api.protect(0xf000, 1, 3), 0); observe(engine, 0xf100, 4, 0x11223344); word(engine, 0xf100, 0xbadc0ffe); run(engine, pushUnit, 8, completed(stoppedPush, 'push', BPC), exit(3, 2), storeHelper(), 'PUSH retry retains original ESP source after destination repair', 'resume', 1); observe(engine, 0xf100, 4, 0xf104); stats.repairs++;
assert.equal(engine.api.protect(0xe000, 1, 3), 0); word(engine, 0xe100, 0x10203040); assert.equal(engine.api.protect(0xe000, 1, 2), 0);
const popESP = pops[2], failedPop = initial(popESP.pc); failedPop.registers[4] = 0xe100; reset(engine, failedPop); const stoppedPop = prefixed(failedPop);
run(engine, popUnit, 8, stoppedPop, exit(5, 1, 2, 0xe100, 4, 2, 1), helper(4, 0, 2, 0xe100), 'POP ESP single read failure preserves old ESP and destination', 'initial', 0, 1); stats.faults++; stats.read_faults++;
assert.equal(engine.api.protect(0xe000, 1, 3), 0); observe(engine, 0xe100, 4, 0x10203040); word(engine, 0xe100, 0x13579bdf); const repairedPop = at(stoppedPop, BPC); repairedPop.registers[4] = 0x13579bdf; run(engine, popUnit, 8, repairedPop, exit(3, 2), helper(4, 0x13579bdf), 'POP retry reads changed old-stack word and loaded ESP wins', 'resume', 0, 1); observe(engine, 0xe100, 4, 0x13579bdf); stats.repairs++;
for (const form of [memoryForms[2], memoryForms[6]]) {
  assert.equal(engine.api.protect(0xa000, 1, 3), 0); const ram = prepareRAM(engine, form.source, form.destination, 0x89abcdef); assert.equal(engine.api.protect(0xa000, 1, 1), 0); const start = memoryStart(form, false); reset(engine, start); const stopped = prefixed(start);
  run(engine, form.unit, 8, stopped, exit(5, 1, 2, form.destination, 4), storeHelper(2, form.destination), 'same-byte overlapping transfer still requires Write permission before ESP commit', 'initial', 1, 1); observeRAM(engine, ram); stats.faults++; stats.store_faults++; stats.same_byte_permission_faults++;
}
assert.equal(engine.api.protect(0xa000, 1, 3), 0); assert.equal(engine.api.map(0x11000, 2, 3), 0);
for (const form of [memoryForms[0], memoryForms[4]]) for (const access of [1, 2]) {
  assert.equal(engine.api.protect(0x12000, 1, 3), 0); const source = access === 1 ? 0x11fff : 0xd100, destination = access === 1 ? 0xd100 : 0x11fff, ram = prepareRAM(engine, source, destination, 0x10203040);
  if (access === 1) assert.equal(engine.api.unmap(0x12000, 1), 0); else assert.equal(engine.api.protect(0x12000, 1, 1), 0); const start = addressed(form, source, destination); reset(engine, start); const stopped = prefixed(start), detail = access === 1 ? 1 : 2;
  run(engine, form.unit, 8, stopped, exit(5, 1, detail, 0x12000, 4, 2, access), helper(4, 0, detail, 0x12000, access), 'cross-page first read or atomic second store rejects before ESP mutation', 'initial', access === 2 ? 1 : 0, 1); for (const address of ram.watched.filter(address => access === 2 || address < 0x12000)) observe(engine, address, 4, modelValue(ram.image, address)); stats.faults++; stats[access === 1 ? 'read_faults' : 'store_faults']++;
  if (access === 1) { assert.equal(engine.api.map(0x12000, 1, 3), 0); for (const address of ram.watched.filter(address => address >= 0x12000)) word(engine, address, modelValue(ram.image, address)); } else assert.equal(engine.api.protect(0x12000, 1, 3), 0);
  word(engine, source, 0x80000000); modelWord(ram.image, source, 0x80000000); run(engine, form.unit, 8, completed(stopped, form.kind, BPC), exit(3, 2), storeHelper(), 'cross-page repair retries current source with original architectural ESP', 'resume', 1, 1); modelWord(ram.image, destination, 0x80000000); observeRAM(engine, ram); stats.repairs++;
}
for (const form of [memoryForms[0], memoryForms[4]]) for (const access of [1, 2]) {
  const source = access === 1 ? 0xfffffffd : DATA, destination = access === 1 ? DEST : 0xfffffffd; word(engine, access === 1 ? DEST : DATA, 0x10203040); const start = addressed(form, source, destination); reset(engine, start); const stopped = prefixed(start);
  run(engine, form.unit, 8, stopped, exit(5, 1, 3, 0xfffffffd, 4, 2, access), helper(4, 0, 3, 0xfffffffd, access), 'EA or stack arithmetic is distinct from forbidden width4 access wrap', 'initial', access === 2 ? 1 : 0, 1); observe(engine, access === 1 ? DEST : DATA, 4, 0x10203040); stats.faults++; stats[access === 1 ? 'read_faults' : 'store_faults']++;
}
assert.equal(engine.api.map(0, 1, 3), 0); assert.equal(engine.api.map(0xfffff000, 1, 3), 0);
word(engine, 0xfffffff8, 0x11223344); word(engine, 0xfffffffc, 0x55667788); const zeroPush = initial(pushESP.pc + 1); zeroPush.registers[4] = 0; reset(engine, zeroPush);
run(engine, pushUnit, 3, completed(zeroPush, 'push', BPC), exit(3, 2), storeHelper(), 'PUSH original zero ESP wraps to last valid word and stores old zero', 'initial', 1); observe(engine, 0xfffffff8, 4, 0x11223344); observe(engine, 0xfffffffc, 4, 0);
for (const form of [pops[0], pops[2]]) {
  const input = form.register === 4 ? 0x12345678 : 0xcafebabe; word(engine, 0xfffffffc, input); const start = initial(form.pc + 1); start.registers[4] = 0xfffffffc; reset(engine, start); const expected = completed(start, 'pop', BPC); expected.registers[form.register] = input;
  run(engine, popUnit, 3, expected, exit(3, 2), helper(4, input), 'last valid POP read permits increment wrap but loaded ESP wins', 'initial', 0, 1); observe(engine, 0xfffffffc, 4, input);
}
for (const row of [
  {form: memoryForms[1], esp: 0, source: 0, destination: 0xfffffffc, ecx: 2},
  {form: memoryForms[5], esp: 0xfffffffc, source: 0xfffffffc, destination: 0, ecx: 2},
  {form: memoryForms[6], esp: 0xfffffffc, source: 0xfffffffc, destination: 0xfffffffc, ecx: 2},
  {form: memoryForms[8], esp: 0xfffffffc, source: 4, destination: 0xfffffff8, ecx: 1},
  {form: memoryForms[7], esp: 0xfffffffc, source: 0xfffffffc, destination: 8, ecx: 1},
]) {
  const ram = prepareRAM(engine, row.source, row.destination, 0x89abcdef), start = memoryStart(row.form); start.registers[4] = row.esp; start.registers[1] = row.ecx; reset(engine, start);
  run(engine, row.form.unit, 3, completed(start, row.form.kind, BPC), exit(3, 2), storeHelper(), 'old/pending ESP and SIB EA may wrap while each Read4/Store4 remains valid', 'initial', 1, 1); modelWord(ram.image, row.destination, 0x89abcdef); observeRAM(engine, ram);
}
const splitForm = memoryForms[0], splitRAM = prepareRAM(engine, DATA, STACK - 4, 0x89abcdef), split = memoryStart(splitForm, false); reset(engine, split);
run(engine, splitForm.unit, 0, split, exit(1, 0), undefined, 'zero budget skips prefix and stack helpers', 'initial'); const prefix = prefixed(split);
run(engine, splitForm.unit, 1, prefix, exit(1, 1), undefined, 'exact prefix budget stops before memory PUSH', 'resume'); select(engine, splitForm.pc + 1, splitForm.unit); engine.view.setUint32(engine.base + 96, 1, true); run(engine, splitForm.unit, 0, prefix, exit(2, 0), undefined, 'cancel precedes zero budget and stack helpers', 'resume'); stats.cancellations++;
engine.view.setUint32(engine.base + 96, 0, true); const pushed = completed(prefix, 'push', splitForm.pc + 3); run(engine, splitForm.unit, 1, pushed, exit(1, 1), storeHelper(), 'exact stack budget commits both accesses and one decrement', 'resume', 1, 1); modelWord(splitRAM.image, STACK - 4, 0x89abcdef); word(engine, DATA, 0xfeedface); modelWord(splitRAM.image, DATA, 0xfeedface);
run(engine, splitForm.unit, 1, at(pushed, BPC), exit(1, 1), undefined, 'branch resume cannot reread changed original source or push again', 'resume'); run(engine, splitForm.unit, 8, at(pushed, BPC), exit(3, 0), undefined, 'returning external PC repeats no stack effect', 'resume'); observeRAM(engine, splitRAM); continueB(at(pushed, BPC));
const cancelled = memoryStart(splitForm, false); reset(engine, cancelled, 1); run(engine, splitForm.unit, 8, cancelled, exit(2, 0), undefined, 'preset cancel suppresses both memory accesses', 'initial'); stats.cancellations++;
const middle = initial(memoryForms[1].pc + 2); reset(engine, middle); miss(engine, middle.eip); run(engine, memoryForms[1].unit, 8, middle, exit(3, 0), undefined, 'interior instruction byte is not a decoded start', 'initial');

const algorithmRows = [], phases = [0, 2, 4, 7, 10, 11];
for (const n of [1, 2, 5]) {
  const source = DATA + 0x100, destination = DEST + 0x100, inputs = Array.from({length: n}, (_, i) => (0x89abcdef + i * 0x01020304) >>> 0), start = initial(0x5000); start.registers[1] = n; start.registers[6] = source; start.registers[7] = destination; reset(engine, start);
  for (const address of [source - 4, source + n * 4, destination - 4, destination + n * 4, STACK - 8, STACK - 4, STACK]) word(engine, address, 0x11223344);
  for (let i = 0; i < n; i++) { word(engine, source + i * 4, inputs[i]); word(engine, destination + i * 4, 0x55667788); }
  const budgets = n === 2 ? [1, 2, 3, 1, 5] : [6 * n + 1]; let retired = 0, calls = 0;
  for (const budget of budgets) {
    const next = Math.min(6 * n, retired + budget), cycles = Math.floor(next / 6), phase = next % 6, pushes = cycles + Number(phase >= 1), popsDone = cycles + Number(phase >= 2), previousCycles = Math.floor(retired / 6), previousPhase = retired % 6;
    const accesses = pushes + popsDone - previousCycles * 2 - Number(previousPhase >= 1) - Number(previousPhase >= 2), expected = at(start, next === 6 * n ? 0x5011 : 0x5000 + phases[phase]);
    expected.registers[4] = STACK - Number(phase === 1) * 4; expected.registers[6] = source + (cycles + Number(phase >= 3)) * 4; expected.registers[7] = destination + (cycles + Number(phase >= 4)) * 4; expected.registers[1] = n - cycles - Number(phase >= 5);
    if (phase >= 5) expected.eflags = arithmetic('dec', n - cycles, start.eflags).flags; else if (cycles) expected.eflags = arithmetic('dec', n - cycles + 1, start.eflags).flags;
    run(engine, copyUnit, budget, expected, exit(next - retired === budget ? 1 : 3, next - retired), accesses ? storeHelper() : undefined, 'closed-form six-instruction memory copy and JNZ', retired === 0 ? 'initial' : 'resume', accesses, accesses);
    for (let i = 0; i < n; i++) { observe(engine, source + i * 4, 4, inputs[i]); observe(engine, destination + i * 4, 4, i < popsDone ? inputs[i] : 0x55667788); }
    for (const address of [source - 4, source + n * 4, destination - 4, destination + n * 4, STACK - 8, STACK]) observe(engine, address, 4, 0x11223344); observe(engine, STACK - 4, 4, pushes ? inputs[pushes - 1] : 0x11223344); retired = next; calls++;
  }
  assert.equal(retired, 6 * n); algorithmRows.push({n, retired, calls, input: inputs, output: [...inputs], esp: STACK, flags: 0x447}); stats.algorithm_cases++; stats.algorithm_retired += retired;
}
word(engine, DATA, 0x12345678); word(engine, DEST, 0x11223344); word(engine, STACK - 4, 0x55667788); const copyStart = initial(0x5000); copyStart.registers[1] = 1; copyStart.registers[6] = DATA; copyStart.registers[7] = DEST; reset(engine, copyStart); const copiedPush = completed(copyStart, 'push', 0x5002);
run(engine, copyUnit, 1, copiedPush, exit(1, 1), storeHelper(), 'split copy commits PUSH once before pending POP', 'initial', 1, 1); assert.equal(engine.api.protect(DEST, 1, 1), 0);
run(engine, copyUnit, 8, copiedPush, exit(5, 0, 2, DEST, 4), storeHelper(2, DEST), 'pending POP store fault retains decremented ESP without repeating PUSH', 'resume', 1, 1); stats.faults++; stats.store_faults++;
engine.view.setUint32(engine.base + 96, 1, true); run(engine, copyUnit, 0, copiedPush, exit(2, 0), undefined, 'cancelled pending POP does not repeat either access', 'resume'); stats.cancellations++; engine.view.setUint32(engine.base + 96, 0, true);
assert.equal(engine.api.protect(DEST, 1, 3), 0); observe(engine, DEST, 4, 0x11223344); word(engine, STACK - 4, 0xfeedface); const copyDone = at(copyStart, 0x5011); copyDone.registers[1] = 0; copyDone.registers[6] += 4; copyDone.registers[7] += 4; copyDone.eflags = 0x447;
run(engine, copyUnit, 8, copyDone, exit(3, 5), storeHelper(), 'repair rereads popped STACKsource rather than original array and preserves committed PUSH', 'resume', 1, 1); observe(engine, DATA, 4, 0x12345678); observe(engine, DEST, 4, 0xfeedface); observe(engine, STACK - 4, 4, 0xfeedface); stats.repairs++;
close(engine, [registerUnit, pushUnit, popUnit, memoryForms[0].unit, memoryForms[8].unit, copyUnit, b]);
const smcCases = [
  {name: 'self-push', kind: 'push', target: 'self'}, {name: 'self-pop', kind: 'pop', target: 'self'},
  {name: 'identity-push', kind: 'push', target: 'self', same: true}, {name: 'identity-pop', kind: 'pop', target: 'self', same: true},
  {name: 'whole-unit', kind: 'push', target: 'whole'}, {name: 'shared-page', kind: 'pop', target: 'shared'}, {name: 'separate-B', kind: 'push', target: 'separate'},
], smcRows = [];
for (const fixture of smcCases) {
  const e = fresh(), bpc = fixture.target === 'shared' ? 0x1800 : 0x2000, input = fixture.same ? 0x13579bdf : 0x2468ace0, aWord = 0x13579bdf, bWord = 0x13579bdf;
  const destination = fixture.target === 'self' ? 0x1004 : fixture.target === 'whole' ? 0x4001 : bpc + 1;
  const instruction = Buffer.from(fixture.kind === 'push' ? 'ff33' : '8f03', 'hex'), code = Buffer.alloc(13); code.set([0x47, ...instruction, 0xb9]); code.writeUInt32LE(aWord, 4); code[8] = 0xe9; code.writeInt32LE(bpc - 0x100d, 9);
  const control = Buffer.from('b900000000eb00', 'hex'); control.writeUInt32LE(bWord, 1); const other = Buffer.from(control); other.writeUInt32LE(aWord, 1);
  authoredCode(`${fixture.name}-A`, code); authoredCode(`${fixture.name}-B`, control); authoredCode(`${fixture.name}-other-A`, other);
  assert.equal(e.api.map(0x1000, 1, 7), 0); if (bpc === 0x2000) assert.equal(e.api.map(bpc, 1, 7), 0); assert.equal(e.api.map(0x4000, 1, 7), 0); assert.equal(e.api.map(DATA, 1, 3), 0);
  upload(e, 0x1000, code); upload(e, bpc, control); upload(e, 0x4000, other); word(e, DATA, input);
  const oldA = compile(e, [[0x1000, 13], [0x4000, 7]], ['read32'], `${fixture.name} A`, [{local: 18}]), oldB = compile(e, [[bpc, 7]], [], `${fixture.name} B`);
  const start = initial(0x1000); start.registers[4] = fixture.kind === 'push' ? destination + 4 : DATA; start.registers[3] = fixture.kind === 'push' ? DATA : destination; reset(e, start); const prefix = prefixed(start), stopped = completed(prefix, fixture.kind, 0x1003), invalidatesA = fixture.target !== 'separate';
  if (invalidatesA) run(e, oldA, 2, stopped, exit(6, 2), storeHelper(), 'stack RAM/ESP/EIP commit before code invalidation and exact budget', 'initial', 1, 1);
  else { const expected = at(stopped, bpc); expected.registers[1] = aWord; run(e, oldA, 8, expected, exit(3, 4), storeHelper(), 'separate B write leaves executing A current with committed ESP', 'initial', 1, 1); }
  observe(e, destination, 4, input); stats.smc_cases++; if (input === aWord) stats.same_byte_code_writes++;
  smcRows.push({name: fixture.name, kind: fixture.kind, destination, old: aWord, value: input, old_esp: start.registers[4], committed_esp: stopped.registers[4], flags: stopped.eflags, reason: invalidatesA ? 6 : 3, retired: invalidatesA ? 2 : 4}); guard(e, oldA, invalidatesA ? 4 : 0); guard(e, oldB, ['shared', 'separate'].includes(fixture.target) ? 4 : 0);
  if (invalidatesA) {
    miss(e, 0x1003, 4); if (fixture.target === 'whole') miss(e, 0x4000, 4);
    if (fixture.name === 'identity-push') { rejected(e, oldA, 4, 'same-byte stack write makes original A stale'); const copied = new WebAssembly.Instance(new WebAssembly.Module(oldA.bytes.slice()), {env: {memory: e.memory}, ring3: {guard_resident: e.api.guard_resident, read32: e.api.read32, store_resident32: e.api.store_resident32}}); rejected(e, {...oldA, run: copied.exports.run}, 4, 'copied stale stack A retains baked identity'); }
    const successor = compile(e, [[0x1003, 10]], [], `${fixture.name} fresh successor`); assert.ok(successor.id > oldA.id); select(e, 0x1003, successor); const continued = at(stopped, bpc); continued.registers[1] = fixture.target === 'self' ? input : aWord;
    run(e, successor, 8, continued, exit(3, 2, 0, 0, 0, 1), undefined, 'fresh next-PC unit repeats neither stack access nor ESP adjustment', 'resume');
    if (fixture.target === 'shared') { miss(e, bpc, 4); const freshB = compile(e, [[bpc, 7]], [], 'fresh shared B'); select(e, bpc, freshB); const done = at(continued, bpc + 7); done.registers[1] = input; run(e, freshB, 3, done, exit(3, 2, 0, 0, 0, 1), undefined, 'fresh shared B consumes existing CPU', 'resume'); }
    else { select(e, bpc, oldB); const done = at(continued, bpc + 7); done.registers[1] = bWord; run(e, oldB, 3, done, exit(3, 2, 0, 0, 0, 1), undefined, 'independent current B true continuation', 'resume'); }
  } else {
    const continued = at(stopped, bpc); continued.registers[1] = aWord; run(e, oldA, 8, continued, exit(3, 0), undefined, 'returning A repeats no committed stack transfer', 'resume'); rejected(e, oldB, 4, 'only separate B is stale'); const freshB = compile(e, [[bpc, 7]], [], 'fresh separate B'); select(e, bpc, freshB); const done = at(continued, bpc + 7); done.registers[1] = input; run(e, freshB, 3, done, exit(3, 2, 0, 0, 0, 1), undefined, 'fresh separate B consumes existing CPU', 'resume');
  }
  observe(e, destination, 4, input); close(e);
}
assert.deepEqual(stats, {engine_instances: 8, compiled_units: 29, canonical_runs: 187, rejected_runs: 10, full_arena_checks: 679, metadata_checks: 80, successful_guards: 13, failed_guards: 8, find_success: 51, find_failure: 9, continuation_initial_calls: 94, continuation_resumes: 93, register_cases: 16, push_only_cases: 12, pop_cases: 8, memory_cases: 20, overlap_cases: 8, smc_cases: 7, same_byte_code_writes: 2, same_byte_permission_faults: 2, bound_store_sites: 41, original_register_sites: 17, immediate_sites: 5, result_sites: 19, algorithm_cases: 3, algorithm_retired: 48, faults: 17, read_faults: 7, store_faults: 10, repairs: 11, cancellations: 3, host_reads: 444, host_writes: 394, guest_read32: 118, guest_store32: 115}, 'independently precomputed authored-path counts');
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = ['engine/tests/process_resident_stack_wasm.rs', 'engine/tests/fixtures/p2-resident-stack/run.mjs', 'engine/tests/process_resident_stack.rs', 'engine/tests/process_resident.rs', 'engine/tests/process_resident_memory.rs', 'engine/tests/process_resident_indirect.rs', 'engine/tests/process_resident_store.rs', 'engine/tests/process_resident_unary.rs', 'engine/tests/process_resident_binary.rs', 'engine/tests/process_resident_wasm.rs', 'engine/tests/fixtures/p2-resident-process/run.mjs', 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', ...productionPaths];
const provenance = {
  stats, actual_resident_runs: stats.canonical_runs + stats.rejected_runs, actual_resident_guard_entries: stats.canonical_runs + stats.rejected_runs + stats.successful_guards + stats.failed_guards, bound_store_identity_checks: stats.guest_store32, guest_helper_calls_from_authored_execution: stats.guest_store32 + stats.guest_read32,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  authored: Object.fromEntries(Object.entries(authored).map(([name, bytes]) => [name, bytes.toString('hex')])), unit_bindings: units.map(unit => ({label: unit.label, id: unit.id.toString(), pointer: unit.pointer, length: unit.length, filename: unit.filename, reads: unit.reads, store_operands: unit.stores})), algorithm_rows: algorithmRows, smc_rows: smcRows, metadata_observations: metadata,
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_stack_wasm', '--', '--nocapture']},
  reused: 'prior binary48/unary31/MOV27/read/indirect actual/module evidence and native atomic Store4/version exhaustion/raw identity/Busy/callback evidence are unchanged and not rerun; late non0/non11 Infrastructure detail3 behavior is source/prior-proof reuse. The affected old resident target runs separately, its14modules/three other inputs compared against preserved R3-322 outputs; unsupportedFF30 deliberately becomes FF10. Eight legacy encoding-family baseline captures are parent-owned with zero guest CPU runs.',
  claim: 'actual Node engine-Wasm resident ordinary PUSH/POP32 all eight encoding families through direct checked helpers and immutable six-argument bound Store4 with the same engine memory, no legacy artifact. All short/ModRM GPR aliases, imm8 signed edges/full imm32, old-ESP PUSH/original register values, loaded-value POP ESP, pending-ESP memory destinations and exact/partial byte overlap. Exact guard0/store1 or guard0/read1 or guard0/read1/store2 with original register/immediate versus preserved RESULT18 operands. Whole4236 arena, full flags/GPRs/neighbour RAM/Helperv1/Exitv2, first-read/second-store faults and changed-current-source repair, cross-page atomic rejection, MAX-valid/EA+ESP wrap versus access-width overflow, prefix/budget/cancel/interior true continuations without host CPU writes. Same-byte Write permission and self/whole/shared/separateB SMC commit stack word/ESP/EIP/retirement once before fresh continuation; original/copied stale binding; independently defined bounded6*n memory-copy output/flags447 and split pending-POP stack-source repair. Counts are authored-path derived, not instrumentation. No injected helpers, hardware/concurrent atomicRMW/LOCK, CALL/RET/gates/installer/table/lifetime, browser/asynchronous cancellation/performance/game/fullP2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2)); console.log(JSON.stringify({...provenance, sources: Object.keys(provenance.sources).length, metadata_observations: metadata.length, artifacts: outputDir}));
