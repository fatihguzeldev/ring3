import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, KEEP = 0x3000, DATA = 0x4000, SECOND = 0x5000, BYTE = 0x4010;
const REG = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const INPUTS = [0, 0x81, 0xff], FLAGS = [2, 0xcd7], IMMEDIATES = [1, 33, 65, 97, 129, 161, 193, 225];
const ALIASES = [['AL', 0, 1], ['CL', 1, 1], ['DL', 2, 1], ['BL', 3, 1], ['AH', 0, 256], ['CH', 1, 256], ['DH', 2, 256], ['BH', 3, 256]];

const ECX = [0xa1b2c300, 0xa1b2c301, 0xa1b2c31f, 0xa1b2c320, 0xa1b2c3ff];
const ANCHORS = [[0, 0, 0, 0, 0, 0, 0], [1, 2, 0, 0, 0x80, 1, 1], [2, 4, 0, 0, 1, 0, 0],
  [0x40, 0x80, 0, 1, 0x20, 0, 0], [0x80, 1, 1, 1, 0x40, 0, 1], [0x7f, 0xfe, 0, 1, 0xbf, 1, 1],
  [0xff, 0xff, 1, 0, 0xff, 1, 0], [0x81, 3, 1, 1, 0xc0, 1, 0], [0x12, 0x24, 0, 0, 9, 0, 0],
  [0xaa, 0x55, 1, 1, 0x55, 0, 1], [0x55, 0xaa, 0, 1, 0xaa, 1, 1], [0x42, 0x84, 0, 1, 0x21, 0, 0]];
function rotate(kind, value) {
  const bits = value.toString(2).padStart(8, '0'); assert.equal(bits.length, 8);
  const result = kind === 'left' ? bits.slice(1) + bits[0] : bits[7] + bits.slice(0, 7), cf = Number(result[kind === 'left' ? 7 : 0]);
  return {value: parseInt(result, 2), cf, of: Number(result[0] !== (kind === 'left' ? String(cf) : result[1]))};
}
const rotateFlags = (before, result) => [2, 4, 0x10, 0x40, 0x80, 0x400].reduce((flags, bit) => flags + (before & bit ? bit : 0), 0) + result.cf + result.of * 0x800;
const PRODUCERS = [[0xffffffff, 0, 0x457], [0x7fffffff, 0x80000000, 0xc96]];
function addOne(value, flags) {
  const wide = value + 1, result = wide % 2 ** 32, signed = value < 2 ** 31 ? value : value - 2 ** 32;
  const even = (result % 256).toString(2).replaceAll('0', '').length % 2 === 0;
  return {value: result, flags: 2 + (flags & 0x400) + Number(wide >= 2 ** 32) + Number(even) * 4
    + Number(value % 16 + 1 >= 16) * 0x10 + Number(result === 0) * 0x40 + Number(result >= 2 ** 31) * 0x80
    + Number(signed + 1 > 2 ** 31 - 1) * 0x800};
}
for (const [before, value, flags] of PRODUCERS) assert.deepEqual(addOne(before, 0xcd7), {value, flags});
for (const [before, left, leftCf, leftOf, right, rightCf, rightOf] of ANCHORS) {
  assert.deepEqual(rotate('left', before), {value: left, cf: leftCf, of: leftOf}); assert.deepEqual(rotate('right', before), {value: right, cf: rightCf, of: rightOf});
}
assert.equal(rotateFlags(0xcd7, rotate('left', 0)), 0x4d6); assert.equal(rotateFlags(0xcd7, rotate('right', 0xff)), 0x4d7);
assert.equal(rotateFlags(2, rotate('left', 0x80)), 0x803); assert.equal(rotateFlags(2, rotate('right', 0x80)), 0x802);
function getByte(registers, alias) {const [, parent, factor] = ALIASES[alias]; return Math.floor(registers[parent] / factor) % 256;}
function putByte(registers, alias, value) {const [, parent, factor] = ALIASES[alias]; registers[parent] += (value - getByte(registers, alias)) * factor;}
const insertionCheck = [...REG]; putByte(insertionCheck, 4, 0x81); assert.equal(insertionCheck[0], 0x1234817f); putByte(insertionCheck, 0, 0x42); assert.equal(insertionCheck[0], 0x12348142);
const banks = [];
for (const [kind, extension] of [['left', 0], ['right', 8]]) {
  for (let group = 0; group < 2; group++) {
    const bytes = Buffer.alloc(162, 0xcc), specs = [], scans = []; let offset = 0;
    for (let slot = 0; slot < 4; slot++) {
      const entry = offset, alias = group * 4 + slot; if (slot === 0) {bytes.set([0x83, 0xc3, 1], offset); offset += 3;}
      const first = offset, operand = 0xc0 | extension | alias;
      for (const immediate of IMMEDIATES) {bytes.set([0xc0, operand, immediate], offset); offset += 3;}
      const next = offset; bytes.set([0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0xc0, operand, 225], offset); offset += 9;
      if (slot === 3) {bytes.set([0xbf, 0xef, 0xbe, 0xad, 0xde], offset); offset += 5;}
      bytes.set([0xeb, 160 - offset - 2], offset); offset += 2;
      specs.push([PC + entry, offset - entry]); scans.push({entry: PC + entry, rotate: PC + first, next: PC + next, alias, secondaryNext: PC + next + 9, canary: slot === 3});
    }
    assert.equal(offset, 148); assert.deepEqual(specs.map(([entry]) => entry - PC), [0, 38, 73, 108]); bytes.set([0x0f, 0x0b], 160);
    const id = `register-${kind}-${group}`; assert.deepEqual(readFileSync(join(output, `${id}.x86`)), bytes);
    banks.push({id, kind, extension, group, location: 'register', bytes, specs, scans, cold: PC + 160, instructions: 50});
  }
  const bytes = Buffer.alloc(130, 0xcc), specs = [], scans = []; let offset = 0;
  const forms = [[0x83, 0xc3, 1, 0xc0, extension, 1], [0xc0, 0x41 | extension, 0xf0, 33], [0xc0, 0x44 | extension, 0x4a, 0x11, 65],
    [0xc0, 0x84 | extension, 0xf3, 0xe0, 0xff, 0xff, 0xff, 97], [0xc0, 0x04 | extension, 0x24, 129], [0xc0, 0x45 | extension, 0, 161],
    [0x8d, 0x5b, 1, 0xc0, 0x06 | extension, 193], [0xc0, 0x87 | extension, 0, 1, 0, 0, 225]];
  for (const [index, form] of forms.entries()) {
    const tail = [0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2];
    if (index === 0) tail.push(0xc0, 5 | extension, 0x10, 0x40, 0, 0, 225); if (index === 7) tail.push(0xbb, 0xef, 0xbe, 0xad, 0xde);
    const next = PC + offset + form.length, length = form.length + tail.length + 2; bytes.set([...form, ...tail, 0xeb, 128 - offset - length], offset);
    specs.push([PC + offset, length]); scans.push({entry: PC + offset, rotate: PC + offset + (index === 0 || index === 6 ? 3 : 0), next,
      secondaryNext: index === 0 ? next + 13 : null, canary: index === 7, immediate: IMMEDIATES[index]}); offset += length;
  }
  assert.equal(offset, 120); assert.deepEqual(specs.map(([entry]) => entry - PC), [0, 21, 33, 46, 62, 74, 86, 100]); assert.equal(scans[7].next - PC, 107);
  assert.equal(specs[7][1] - (scans[7].next - scans[7].entry), 13); bytes.set([0x0f, 0x0b], 128);
  const id = `memory-${kind}`; assert.deepEqual(readFileSync(join(output, `${id}.x86`)), bytes);
  banks.push({id, kind, extension, location: 'memory', bytes, specs, scans, cold: PC + 128, instructions: 36});
}
const counts = {contexts: 0, modules: 0, seeds: 0, register_matrix: 0, memory_matrix: 0, committed_rotations: 0, primary_register: 0, secondary_register: 0, primary_memory: 0, secondary_memory: 0,
  rol: 0, ror: 0, setb: 0, seto: 0, jumps: 0, canaries: 0, boundaries: 0, live_producers: 0, fault_calls: 0, read_faults: 0, write_faults: 0, prefixes: 0,
  repairs: 0, permission_only_repairs: 0, map_only_repairs: 0, changed_value_repairs: 0, code_stores: 0, same_value_code_stores: 0, changing_code_stores: 0, continuation_canaries: 0,
  mutations: 0, compiler_refusals: 0, rejection_survivals: 0, preflight: 0, owner_controls: 0, generated_calls: 0, maps: 0, unmaps: 0, protects: 0,
  host_uploads: 0, host_uploaded_bytes: 0, standalone_page_inputs: 0, pages: 0, arena_checks: 0};
const modules = [], pageRows = [], faultRows = [], smcRows = [], mutationRows = [], refusalRows = [], observedCl = new Set();
function record(magic, size, fields, version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired, detail = 0, address = 0, access = 0, version = 2) => record('R3EX', 40, [reason, retired, detail, address, access, access ? 1 : 0], version);
const wordHelper = value => record('R3MH', 40, [0, value, 0, 0, 0, 0]);
const readHelper = (value, detail = 0, address = 0) => record('R3MH', 40, [detail ? 1 : 0, detail ? 0 : value, detail, detail ? address : 0, detail ? 1 : 0, 1], 2);
const storeHelper = (detail = 0, address = 0) => record('R3MH', 40, [detail ? 1 : 0, 0, detail, detail ? address : 0, detail ? 2 : 0, 1], 3);
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
const pattern = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1));
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function check(ctx, label) {refresh(ctx); assert.deepEqual(Buffer.from(ctx.bytes.subarray(ctx.base, ctx.base + SIZE)), ctx.expected, `${ctx.owner}/${ctx.kind}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'explicit host request');}
function protect(ctx, address, permissions) {pure(ctx, () => ctx.api.protect(address, 1, permissions), 'data permission change'); ctx.permissions.set(address, permissions); counts.protects++;}
function unmap(ctx, address) {pure(ctx, () => ctx.api.unmap(address, 1), 'data unmap'); ctx.pages.delete(address); ctx.permissions.delete(address); counts.unmaps++;}
function patch(ctx, address, bytes) {
  for (const [index, value] of bytes.entries()) {const at = address + index, page = Math.floor(at / 4096) * 4096; assert.ok(ctx.pages.has(page)); ctx.pages.get(page)[at % 4096] = value;}
}
function dataInput(ctx, address, bytes) {
  request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), 'explicit host data upload'); patch(ctx, address, bytes);
  counts.host_uploads++; counts.host_uploaded_bytes += bytes.length;
}
function map(ctx, address, bytes = pattern) {
  pure(ctx, () => ctx.api.map(address, 1, 3), 'data map'); ctx.pages.set(address, Buffer.alloc(4096)); ctx.permissions.set(address, 3); counts.maps++;
  if (bytes !== null) dataInput(ctx, address, bytes);
}
function fresh(owner, bank) {
  const expected = Buffer.alloc(SIZE), ordinal = ++counts.contexts;
  expected.set(state(Array(8).fill(0), 0, 2)); expected.set(record('R3EX', 40, [1, 0, 0, 0, 0, 0]), 56); expected.set(wordHelper(0), 100);
  const ctx = {owner, kind: bank.kind, bank, expected, low: ordinal, high: 0xc8409000, pages: new Map(), permissions: new Map(), units: []};
  if (owner === 'standalone') {
    ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; ctx.physical = new Map([[PC, 0x6000], [KEEP, 0x8000]]);
    ctx.pages.set(PC, Buffer.alloc(4096)); ctx.pages.get(PC).set(bank.bytes); ctx.pages.set(KEEP, Buffer.from(pattern)); ctx.permissions.set(PC, 7); ctx.permissions.set(KEEP, 3);
    refresh(ctx).bytes.set(expected, ctx.base); for (const [address, bytes] of ctx.pages) ctx.bytes.set(bytes, ctx.physical.get(address)); counts.standalone_page_inputs += 2;
  } else {
    const instance = new WebAssembly.Instance(engineModule, {}), api = {};
    const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, unmap: 2, protect: 3, upload: 2, read8: 1, read32: 1,
      compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store8: 2, store_resident8: 6};
    for (const [name, arity] of Object.entries(arities)) {api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);}
    ctx.api = api; ctx.memory = instance.exports.memory; assert.equal(api.open(bank.location === 'memory' ? 7 : 3, ctx.low, ctx.high), 0);
    ctx.base = api.arena_ptr() >>> 0; check(ctx, 'independently initialized arena'); pure(ctx, () => api.map(PC, 1, 7), 'code map');
    ctx.pages.set(PC, Buffer.alloc(4096)); ctx.permissions.set(PC, 7); counts.maps++; dataInput(ctx, PC, bank.bytes); map(ctx, KEEP);
    if (bank.location === 'memory') for (const address of [DATA, SECOND, 0xfffff000]) map(ctx, address);
  }
  ctx.expected.set(pattern, TRANSFER); refresh(ctx).bytes.set(pattern, ctx.base + TRANSFER); check(ctx, 'explicit complete initialized context'); return ctx;
}
function compile(ctx, specs = ctx.bank.specs, continuation = false, role = 'c0') {
  let binding = {}, bytes; const pureModule = ctx.bank.location === 'register' || continuation;
  if (ctx.owner === 'standalone') bytes = readFileSync(join(output, `standalone-${ctx.bank.id}.wasm`));
  else {
    request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat()));
    if (ctx.owner === 'replacement') {
      pure(ctx, () => ctx.api.compile_entries(specs.length, 0), `${specs.length}-entry compiler`);
      binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);
    } else {
      check(ctx, 'before resident compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
      assert.equal(ctx.view.getUint32(ctx.base + TRANSFER, true), 1); assert.equal(ctx.view.getUint32(ctx.base + TRANSFER + 4, true), 24);
      binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
      assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only');
    }
    refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
    bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)); writeFileSync(join(output, `${ctx.owner}-${ctx.bank.id}-${ctx.low}-${counts.modules + 1}.wasm`), bytes, {flag: 'wx'});
  }
  assert.ok(bytes.length <= 65536); const module = new WebAssembly.Module(bytes);
  const imports = ctx.owner === 'standalone' ? [] : (pureModule ? ['guard'] : ['guard', 'read8', 'store8']).map(name => ctx.owner === 'replacement' ? name : name === 'guard' ? 'guard_resident' : name === 'store8' ? 'store_resident8' : name);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...imports.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.api ? {ring3: ctx.api} : {})}); assert.equal(child.exports.run.length, 4);
  const unit = {...binding, bytes, run: child.exports.run, exitVersion: pureModule ? 1 : 2}; counts.modules++; ctx.units.push(unit);
  modules.push({owner: ctx.owner, bank: ctx.bank.id, role, sha256: hash(bytes), length: bytes.length, imports, specs, exit_version: unit.exitVersion, code_page_sha256: hash(ctx.pages.get(PC)), ...binding}); return unit;
}
function seed(ctx, registers, pc, flags) {
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(3, 0, 0, 0, 0, ctx.bank.location === 'register' ? 1 : 2), 56); ctx.expected.writeUInt32LE(0, 96);
  ctx.expected.set(ctx.bank.location === 'register' ? wordHelper(0xdecafbad) : readHelper(0xdecafbad), 100);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 140), ctx.base); check(ctx, 'one initial CPU/helper seed'); counts.seeds++;
}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, label, registers, pc, flags, reason = 1, retired = 1, helperBytes, detail = 0, fault = 0, access = 0) {
  check(ctx, `before ${label}`); ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired, detail, fault, access, unit.exitVersion), 56);
  if (helperBytes !== undefined) ctx.expected.set(helperBytes, 100);
  assert.equal(unit.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0, label); check(ctx, label); counts.generated_calls++;
}
function preflight(ctx, unit, registers, pc, flags) {
  run(ctx, unit, 0, 'zero budget before guest access', registers, pc, flags, 1, 0); counts.preflight++;
  cancel(ctx, 1); run(ctx, unit, 1, 'cancel before guest access', registers, pc, flags, 2, 0); counts.preflight++; cancel(ctx, 0);
}
function guestByte(ctx, address) {return ctx.pages.get(Math.floor(address / 4096) * 4096)[address % 4096];}
function observeByte(ctx, address, value) {
  check(ctx, 'before separate diagnostic Read8'); assert.equal(ctx.api.read8(address), 0); ctx.expected.set(readHelper(value), 100); check(ctx, 'diagnostic Read8 changes helper v2 only');
}
function pages(ctx, label, addresses = [...ctx.pages.keys()]) {
  for (const address of addresses) {
    const expected = ctx.pages.get(address); assert.ok(expected); const permissions = ctx.permissions.get(address); if (!(permissions & 1)) protect(ctx, address, permissions | 1);
    const actual = Buffer.alloc(4096);
    if (ctx.owner === 'standalone') {const at = ctx.physical.get(address); actual.set(refresh(ctx).bytes.subarray(at, at + 4096));}
    else for (let offset = 0; offset < 4096; offset += 4) {
      check(ctx, 'before separate page diagnostic Read32'); assert.equal(ctx.api.read32(address + offset), 0); ctx.expected.set(wordHelper(expected.readUInt32LE(offset)), 100); check(ctx, 'diagnostic Read32 changes helper v1 only');
      actual.writeUInt32LE(refresh(ctx).view.getUint32(ctx.base + 120, true), offset);
    }
    assert.deepEqual(actual, expected, 'complete current declared page and non-target neighbors'); if (!(permissions & 1)) protect(ctx, address, permissions);
    counts.pages++; pageRows.push({owner: ctx.owner, kind: ctx.kind, label, address, sha256: hash(actual)});
  }
}
function countRotate(ctx, phase) {counts.committed_rotations++; counts[ctx.kind === 'left' ? 'rol' : 'ror']++; counts[`${phase}_${ctx.bank.location}`]++;}
function memoryRotate(ctx, unit, registers, next, address, flags, phase = 'primary') {
  const result = rotate(ctx.kind, guestByte(ctx, address)), afterFlags = rotateFlags(flags, result); patch(ctx, address, Buffer.from([result.value]));
  run(ctx, unit, 1, 'complete checked memory byte rotation before consumers', registers, next, afterFlags, 1, 1, storeHelper()); countRotate(ctx, phase);
  observeByte(ctx, address, result.value); return afterFlags;
}
function consumers(ctx, unit, registers, scan, flags) {
  putByte(registers, 0, flags % 2); run(ctx, unit, 1, 'SETB observes live CF', registers, scan.next + 3, flags); counts.setb++;
  putByte(registers, 2, Number(Boolean(flags & 0x800))); run(ctx, unit, 1, 'SETO observes live OF', registers, scan.next + 6, flags); counts.seto++;
  let cursor = scan.next + 6;
  if (scan.secondaryNext !== null) {
    if (ctx.bank.location === 'register') {
      const result = rotate(ctx.kind, getByte(registers, scan.alias)); putByte(registers, scan.alias, result.value); flags = rotateFlags(flags, result);
      run(ctx, unit, 1, 'second rotation reads current byte after partial consumers', registers, scan.secondaryNext, flags); countRotate(ctx, 'secondary');
    } else flags = memoryRotate(ctx, unit, registers, scan.secondaryNext, BYTE, flags, 'secondary');
    cursor = scan.secondaryNext;
  }
  if (scan.canary) {registers[ctx.bank.location === 'register' ? 7 : 3] = 0xdeadbeef; cursor += 5; run(ctx, unit, 1, 'following MOV canary', registers, cursor, flags); counts.canaries++;}
  run(ctx, unit, 1, 'following JMP', registers, ctx.bank.cold, flags); counts.jumps++;
}
function registerChain(ctx, unit, registers, scan, flags, start = 0) {
  for (let index = start; index < 8; index++) {
    const result = rotate(ctx.kind, getByte(registers, scan.alias)); putByte(registers, scan.alias, result.value); flags = rotateFlags(flags, result);
    run(ctx, unit, 1, 'complete byte and parent state before partial consumers', registers, scan.rotate + (index + 1) * 3, flags); countRotate(ctx, 'primary');
  }
  consumers(ctx, unit, registers, scan, flags);
}
function rotateAndConsume(ctx, unit, registers, scan, address, flags) {
  const after = memoryRotate(ctx, unit, registers, scan.next, address, flags); consumers(ctx, unit, registers, scan, after);
}
function addressRegisters(form, address, ecx) {
  const registers = [...REG]; registers[1] = ecx;
  if (form === 0) registers[0] = address;
  if (form === 1) registers[1] = address + 16;
  if (form === 2) {registers[1] = 3; registers[2] = address - 6 - 17;}
  if (form === 3) {registers[6] = 2; registers[3] = address + 32 - 16;}
  if (form === 4) registers[4] = address;
  if (form === 5) registers[5] = address;
  if (form === 6) registers[6] = address;
  if (form === 7) registers[7] = address - 0x100;
  return registers;
}
function numeric(ctx, unit) {
  for (const [inputIndex, value] of INPUTS.entries()) for (const [form, scan] of ctx.bank.scans.entries()) for (const [flagIndex, flags] of FLAGS.entries()) {
    const ecx = ECX[(inputIndex + form + flagIndex) % ECX.length]; let registers;
    if (ctx.bank.location === 'register') {registers = [...REG]; registers[1] = ecx; putByte(registers, scan.alias, value); if (scan.alias !== 1) observedCl.add(registers[1] % 256);}
    else {registers = addressRegisters(form, BYTE, ecx); observedCl.add(registers[1] % 256); dataInput(ctx, BYTE, Buffer.from([value]));}
    seed(ctx, registers, scan.rotate, flags); if (inputIndex === 0 && form === 0 && flagIndex === 0) preflight(ctx, unit, registers, scan.rotate, flags);
    if (ctx.bank.location === 'register') registerChain(ctx, unit, registers, scan, flags); else rotateAndConsume(ctx, unit, registers, scan, BYTE, flags);
    counts[`${ctx.bank.location}_matrix`]++;
  }
  if (ctx.bank.location === 'memory') for (const address of [0x4fff, 0x5000, 0xffffffff]) for (const flags of FLAGS) {
    if (address === 0x4fff && ctx.pages.has(SECOND)) unmap(ctx, SECOND); if (address === SECOND && !ctx.pages.has(SECOND)) map(ctx, SECOND);
    const registers = addressRegisters(6, address, ECX[4]), scan = ctx.bank.scans[6]; dataInput(ctx, address, Buffer.from([0x81])); seed(ctx, registers, scan.rotate, flags);
    rotateAndConsume(ctx, unit, registers, scan, address, flags); counts.boundaries++;
  }
  for (const [oldEbx, afterEbx, producedFlags] of PRODUCERS) {
    const scan = ctx.bank.scans[0], registers = ctx.bank.location === 'register' ? [...REG] : addressRegisters(0, BYTE, ECX[4]); registers[1] = ECX[4]; registers[3] = oldEbx;
    if (ctx.bank.location === 'register') putByte(registers, scan.alias, 0x81); else dataInput(ctx, BYTE, Buffer.from([0x81]));
    seed(ctx, registers, scan.entry, 0xcd7); if (oldEbx === 0xffffffff) preflight(ctx, unit, registers, scan.entry, 0xcd7);
    assert.deepEqual(addOne(oldEbx, 0xcd7), {value: afterEbx, flags: producedFlags}); registers[3] = afterEbx;
    run(ctx, unit, 1, 'ADD producer full CPU before byte rotate', registers, scan.rotate, producedFlags); counts.live_producers++;
    if (ctx.bank.location === 'register') registerChain(ctx, unit, registers, scan, producedFlags); else rotateAndConsume(ctx, unit, registers, scan, BYTE, producedFlags);
  }
}
const faultShapes = [
  {name: 'unmapped', address: 0x8000, detail: 1, fault: 0x8000, access: 1, missing: 0x8000},
  {name: 'write-only last byte', address: 0x4fff, detail: 2, fault: 0x4fff, access: 1, denied: DATA, permissions: 2},
  {name: 'read-only last byte', address: 0x4fff, detail: 2, fault: 0x4fff, access: 2, denied: DATA, permissions: 1},
];
function faults(ctx, unit) {
  const scan = ctx.bank.scans[6];
  for (const shape of faultShapes) for (const flags of FLAGS) {
    protect(ctx, DATA, 3); const setupValue = shape.missing === 0x8000 ? null : 0x81;
    if (setupValue !== null) dataInput(ctx, shape.address, Buffer.from([setupValue]));
    if (shape.denied !== undefined) protect(ctx, shape.denied, shape.permissions);
    const registers = addressRegisters(6, shape.address, ECX[4]); seed(ctx, registers, scan.entry, flags); registers[3]++;
    const faultHelper = shape.access === 1 ? readHelper(0, shape.detail, shape.fault) : storeHelper(shape.detail, shape.fault);
    run(ctx, unit, 2, 'retired LEA then precise width-one RMW fault', registers, scan.rotate, flags, 5, 1, faultHelper, shape.detail, shape.fault, shape.access); counts.prefixes++;
    pages(ctx, `${shape.name} first fault keeps operand and neighbors`, [DATA]);
    if (flags === 2) preflight(ctx, unit, registers, scan.rotate, flags);
    run(ctx, unit, 1, 'unrepaired retry retires zero and preserves full CPU', registers, scan.rotate, flags, 5, 0, faultHelper, shape.detail, shape.fault, shape.access);
    counts.fault_calls += 2; counts[shape.access === 1 ? 'read_faults' : 'write_faults'] += 2; pages(ctx, `${shape.name} unrepaired retry`, [DATA]);
    const permissionOnly = shape.access === 2 && flags === 0xcd7, mapOnly = shape.missing === 0x8000 && flags === 0xcd7;
    const repairValue = mapOnly ? 0 : permissionOnly ? setupValue : 1;
    const repairMode = mapOnly ? 'map-only' : permissionOnly ? 'permission-only' : shape.missing !== undefined ? 'map+value' : 'permission+value';
    if (shape.missing !== undefined) map(ctx, shape.missing, mapOnly ? null : pattern);
    if (shape.denied !== undefined) protect(ctx, shape.denied, 3);
    if (mapOnly) counts.map_only_repairs++; else if (permissionOnly) counts.permission_only_repairs++; else {dataInput(ctx, shape.address, Buffer.from([repairValue])); counts.changed_value_repairs++;}
    assert.equal(guestByte(ctx, shape.address), repairValue); rotateAndConsume(ctx, unit, registers, scan, shape.address, flags); counts.repairs++;
    if (shape.missing === 0x8000) {pages(ctx, 'complete repaired mapped page', [0x8000]); unmap(ctx, 0x8000);}
    faultRows.push({owner: ctx.owner, kind: ctx.kind, ...shape, flags, setup_value: setupValue, helper_version: shape.access === 1 ? 2 : 3, width: 1, retired_prefix: 1, unrepaired_retired: 0, repair_value: repairValue, repair_mode: repairMode});
  }
  protect(ctx, DATA, 3); pages(ctx, 'complete post-fault pages');
}
function controls(ctx, unit) {
  const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), flags = ctx.expected.readUInt32LE(52);
  for (const [name, cancelled, budget, reason] of [['zero budget', 0, 0, 1], ['cancel with budget', 1, 1, 2], ['cancel with zero budget', 1, 0, 2]]) {
    cancel(ctx, cancelled); run(ctx, unit, budget, name, registers, ctx.bank.cold, flags, reason, 0); counts.preflight++;
  }
  cancel(ctx, 0); run(ctx, unit, 1, 'cold entry', registers, ctx.bank.cold, flags, 3, 0); counts.preflight++;
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'malformed pointers', 1); counts.preflight++; counts.generated_calls++;
}
function ownerChecks(ctx, unit) {
  if (ctx.owner === 'standalone') return;
  for (const wrong of ['key', 'identity']) {
    const low = ctx.low ^ Number(wrong === 'key');
    const action = ctx.owner === 'replacement' ? () => ctx.api.guard(low, ctx.high, unit.generation + Number(wrong === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96)
      : () => ctx.api.guard_resident(low, ctx.high, unit.low ^ Number(wrong === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
    pure(ctx, action, `wrong ${wrong}`, 3); counts.owner_controls++;
  }
}
function rejectedCounts(ctx, unit) {
  protect(ctx, KEEP, 7); const memory = ctx.bank.location === 'memory';
  for (const immediate of [0, 2, 32, 255]) {
    const bytes = memory ? Buffer.from([0x66, 0xc0, 5 | ctx.bank.extension, 0x10, 0x40, 0, 0, immediate, 0xeb, 0, 0x0f, 0x0b])
      : Buffer.from([0xc0, 0xc0 | ctx.bank.extension | ctx.bank.scans[0].alias, immediate, 0xeb, 0, 0x0f, 0x0b]);
    dataInput(ctx, KEEP, bytes); const published = [ctx.api.generation(), ctx.api.module_ptr(), ctx.api.module_len()];
    request(ctx, words(ctx.owner === 'replacement' ? [KEEP] : [KEEP, bytes.length - 2]));
    pure(ctx, () => ctx.owner === 'replacement' ? ctx.api.compile_entries(1, 0) : ctx.api.compile_resident(1), 'strict prefixed memory or unsupported register count refused', 10); counts.compiler_refusals++;
    assert.deepEqual([ctx.api.generation(), ctx.api.module_ptr(), ctx.api.module_len()], published, 'failed compiler preserves published metadata');
    assert.deepEqual(Buffer.from(refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length)), unit.bytes, 'failed compiler preserves published module bytes');
    const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), pc = ctx.expected.readUInt32LE(48), flags = ctx.expected.readUInt32LE(52);
    run(ctx, unit, 0, 'published owner remains valid after refusal', registers, pc, flags, 1, 0); counts.rejection_survivals++;
    refusalRows.push({owner: ctx.owner, bank: ctx.bank.id, immediate, status: 10, published_module_sha256: hash(unit.bytes)});
  }
  dataInput(ctx, KEEP, pattern.subarray(0, memory ? 12 : 7)); protect(ctx, KEEP, 3); pages(ctx, 'restored patterned compiler-rejection page', [KEEP]);
}
function mutation(ctx, unit) {
  const scan = ctx.bank.scans[0], registers = ctx.bank.location === 'register' ? [...REG] : addressRegisters(0, BYTE, ECX[4]); registers[1] = ECX[4];
  if (ctx.bank.location === 'register') putByte(registers, scan.alias, 0x81); else dataInput(ctx, BYTE, Buffer.from([0x81])); seed(ctx, registers, scan.rotate, 0xcd7);
  let flags;
  if (ctx.bank.location === 'register') {
    const result = rotate(ctx.kind, getByte(registers, scan.alias)); putByte(registers, scan.alias, result.value); flags = rotateFlags(0xcd7, result);
    run(ctx, unit, 1, 'first immediate completes before code mutation', registers, scan.rotate + 3, flags); countRotate(ctx, 'primary');
  } else flags = memoryRotate(ctx, unit, registers, scan.next, BYTE, 0xcd7);
  const address = scan.rotate + 2; assert.equal(ctx.pages.get(PC)[address - PC], 1); dataInput(ctx, address, Buffer.from([33])); cancel(ctx, 1);
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'consumed immediate 1 to 33 invalidates before other controls', 4); counts.owner_controls++; counts.generated_calls++; cancel(ctx, 0);
  const current = compile(ctx, ctx.bank.specs, false, 'mutated-c0');
  if (ctx.bank.location === 'register') registerChain(ctx, current, registers, scan, flags, 1); else consumers(ctx, current, registers, scan, flags);
  mutationRows.push({owner: ctx.owner, bank: ctx.bank.id, address, old_byte: 1, new_byte: 33, upload_bytes: 1, resume_pc: scan.rotate + 3, stale_status: 4, cpu_writes_during_continuation: 0});
  counts.mutations++; pages(ctx, 'current complete pages after consumed immediate upload'); return current;
}
function close(ctx) {
  if (ctx.owner === 'standalone') return; cancel(ctx, 1); pure(ctx, () => ctx.api.close(), 'close');
  for (const unit of ctx.units) {pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'closed owner before cancelled malformed pointers/budget', 5); counts.owner_controls++; counts.generated_calls++;}
}
function smc(ctx, unit, value) {
  const address = 0x1f00, scan = ctx.bank.scans[7], registers = addressRegisters(7, address, ECX[4]); seed(ctx, registers, scan.rotate, 0xcd7);
  const result = rotate(ctx.kind, value), flags = rotateFlags(0xcd7, result); assert.equal(guestByte(ctx, address), value); patch(ctx, address, Buffer.from([result.value]));
  run(ctx, unit, 1, 'Store8 v3 commits helper/flags/EIP/retirement before code exit', registers, scan.next, flags, 6, 1, storeHelper()); countRotate(ctx, 'primary'); counts.code_stores++;
  counts[value === result.value ? 'same_value_code_stores' : 'changing_code_stores']++; observeByte(ctx, address, result.value); pages(ctx, 'complete committed width-one code-store pages');
  smcRows.push({owner: ctx.owner, kind: ctx.kind, before: value, after: result.value, address, width: 1, helper_version: 3, pc: scan.next, flags, retired: 1, canary_before: registers[3]});
  cancel(ctx, 1); pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'stale after successful code store', 4); counts.owner_controls++; counts.generated_calls++; cancel(ctx, 0);
  const current = compile(ctx, [[scan.next, 13]], true, 'current-code-tail'); consumers(ctx, current, registers, scan, flags); counts.continuation_canaries++;
  observeByte(ctx, address, result.value); pages(ctx, 'fresh pure consumers/canary/JMP keep committed pages'); close(ctx);
}
for (const bank of banks.filter(bank => bank.location === 'register')) for (const owner of ['standalone', 'replacement', 'resident']) {
  const ctx = fresh(owner, bank); let unit = compile(ctx); pages(ctx, 'initial complete pure pages'); numeric(ctx, unit); pages(ctx, 'complete pages after finite C0 chains'); controls(ctx, unit); ownerChecks(ctx, unit);
  if (owner !== 'standalone') {rejectedCounts(ctx, unit); unit = mutation(ctx, unit);} close(ctx);
}
for (const bank of banks.filter(bank => bank.location === 'memory')) for (const owner of ['replacement', 'resident']) {
  const ctx = fresh(owner, bank); dataInput(ctx, 0x1f00, Buffer.from([0])); let unit = compile(ctx); pages(ctx, 'initial checked-memory pages');
  numeric(ctx, unit); pages(ctx, 'finite C0 rows and one-byte page endpoints'); faults(ctx, unit); controls(ctx, unit); ownerChecks(ctx, unit); rejectedCounts(ctx, unit); unit = mutation(ctx, unit); smc(ctx, unit, 0);
  for (const value of [0xff, 0x81]) {const extra = fresh(owner, bank); dataInput(extra, 0x1f00, Buffer.from([value])); const extraUnit = compile(extra); ownerChecks(extra, extraUnit); smc(extra, extraUnit, value);}
}
const expectedCounts = {contexts: 24, modules: 48, seeds: 584, register_matrix: 288, memory_matrix: 192, committed_rotations: 3180,
  primary_register: 2560, secondary_register: 320, primary_memory: 264, secondary_memory: 36, rol: 1590, ror: 1590, setb: 584, seto: 584, jumps: 584,
  canaries: 108, boundaries: 24, live_producers: 32, fault_calls: 48, read_faults: 32, write_faults: 16, prefixes: 24, repairs: 24,
  permission_only_repairs: 4, map_only_repairs: 4, changed_value_repairs: 16, code_stores: 12, same_value_code_stores: 8, changing_code_stores: 4, continuation_canaries: 12,
  mutations: 12, compiler_refusals: 48, rejection_survivals: 48, preflight: 168, owner_controls: 108, generated_calls: 5404, maps: 88, unmaps: 12, protects: 116,
  host_uploads: 428, host_uploaded_bytes: 265804, standalone_page_inputs: 8, pages: 332, arena_checks: 661340};
for (const [name, expected] of Object.entries(expectedCounts)) assert.equal(counts[name], expected, `exact observed ${name} census`);
assert.deepEqual([...observedCl].sort((a, b) => a - b), [0, 1, 3, 31, 32, 255], 'literal effective count one despite CL and indexed ECX values');
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, modules, faults: faultRows,
  code_stores: smcRows, mutations: mutationRows, compiler_refusals: refusalRows, pages: pageRows,
  source: {url: 'https://cdrdv2-public.intel.com/929354/253667-093-sdm-vol-2b.pdf', order: '253667-093US', edition: 'September 2026',
    pdf_sha256: 'a261998ace8e07f624bf3e2486bb6cfe950fb8be8d0dc9631bcecb4c5ec953e4', extract_sha256: '7a0e03302580204f99e8283af389f84ec082701807f1a9615a028af4d6f849d0', pages: [541, 542, 543, 544, 545]},
  oracle: {sources: INPUTS, flag_seeds: FLAGS, raw_counts: IMMEDIATES, secondary_raw_count: 225, aliases: ALIASES, literal_anchors: ANCHORS,
    live_add_anchors: PRODUCERS, live_add_byte: 0x81, registers: REG, ecx_seeds: ECX, observed_cl: [...observedCl].sort((a, b) => a - b),
    fault_shapes: faultShapes, repair_modes: ['map-only', 'permission-only', 'map+value', 'permission+value'], changed_repair_byte: 1, arena_bytes: SIZE, patterned_page: KEEP, pattern_sha256: hash(pattern),
    endpoints: [0x4fff, 0x5000, 0xffffffff], helper_read_version: 2, helper_store_version: 3, helper_width: 1, memory_exit_version: 2, pure_exit_version: 1,
    limits: {instructions: 64, blocks: 8, wasm_bytes: 65536},
    banks: banks.map(bank => ({id: bank.id, specs: bank.specs, scans: bank.scans, instructions: bank.instructions, sha256: hash(bank.bytes), hex: bank.bytes.toString('hex')})),
    flag_policy: 'defined effective count-one CF/OF, retain SF/ZF/AF/PF/DF/fixed bit 1; memory flags publish only after successful Store8'},
  artifact_census: {x86_banks: 6, modules: 48, result_files: 1, total: 55},
  test_sha256: Object.fromEntries(['engine/tests/cpu_byte_rotate_immediate_one_wasm.rs', 'engine/tests/fixtures/p2-byte-rotate-immediate-one/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'finite flat32 C0 byte ROL/ROR for raw counts 1/33/65/97/129/161/193/225 with masked count one: every raw count for AL/CL/DL/BL/AH/CH/DH/BH through native-generated standalone and actual engine-Wasm-generated replacement/resident modules, eight declared memory count/EA pairings through both bound owners, bytes 0/81/ff and FLAGS 2/cd7 plus declared evolved intermediates and repair byte 1; independent eight-character bitstring/literal rotation, numerical byte-parent insertion and widened/literal ADD oracles, full CPU after each rotation and producer before partial consumers, current second register byte and row-zero absolute memory byte without CPU repair; unchanged broad D0 arithmetic evidence remains separate; exact pure and Read8 v2/Store8 v3 width-one imports, default caps, complete 4236-byte arenas and current whole pages including neighbors; precise Read1/Store1 faults, retired LEA and zero-retirement retry, permission-only/map-only/changed-value data repair in the same CPU/module, valid byte endpoints; zero/ff unchanged and 81 changing code stores reason 6 then stale 4, fresh pure guard-only consumers/canary/JMP without CPU writes or RMW replay, closed 5; consumed immediate 1 to 33 one-byte uploads invalidate old snapshots despite unchanged normalized semantics, fresh modules resume the same CPU/EIP without replay; strict66-prefix memory refusals and register raw0/2/32/255 refusals preserve published modules; no full byte/FLAGS or count/EA Cartesian suite, width-one overflow/cross-page faults, other count/CL/carry arithmetic evidence, new helper/ABI/capacity, PE/browser/performance/game claim'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, result_sha256: hash(resultBytes), counts, output}));
