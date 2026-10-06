import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, KEEP = 0x3000, DATA = 0x4000, SECOND = 0x5000, WORD = 0x4010;
const REG = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const INPUTS = [0x80000001, 0x12345678, 0xffffffff], FLAGS = [2, 0xcd7], IMMEDIATES = [1, 33, 65, 97, 129, 161, 193, 225];
const ECX = [0xa1b2c300, 0xa1b2c301, 0xa1b2c31f, 0xa1b2c320, 0xa1b2c3ff];
const ANCHORS = [[0, 0, 0, 0, 0, 0, 0], [1, 2, 0, 0, 0x80000000, 1, 1],
  [0x80000000, 1, 1, 1, 0x40000000, 0, 1], [0xffffffff, 0xffffffff, 1, 0, 0xffffffff, 1, 0],
  [0x80000001, 3, 1, 1, 0xc0000000, 1, 0], [0x12345678, 0x2468acf0, 0, 0, 0x091a2b3c, 0, 0]];
function rotate(kind, value) {
  const bits = value.toString(2).padStart(32, '0');
  const result = kind === 'left' ? bits.slice(1) + bits[0] : bits[31] + bits.slice(0, 31), cf = Number(result[kind === 'left' ? 31 : 0]);
  return {value: parseInt(result, 2), cf, of: Number(result[0] !== (kind === 'left' ? String(cf) : result[1]))};
}
const rotateFlags = (before, result) => [2, 4, 0x10, 0x40, 0x80, 0x400].reduce((flags, bit) => flags + (before & bit ? bit : 0), 0) + result.cf + result.of * 0x800;
const PRODUCERS = [[0xffffffff, 0, 0x457, 0x80000001], [0x7fffffff, 0x80000000, 0xc96, 0x40000002]];
const D1_ROWS = [[0x80000000, 2], [0x12345678, 0xcd7], [0xffffffff, 2]];
function addOne(value, flags) {
  const wide = value + 1, result = wide % 2 ** 32, signed = value < 2 ** 31 ? value : value - 2 ** 32;
  const even = (result % 256).toString(2).replaceAll('0', '').length % 2 === 0;
  return {value: result, flags: 2 + (flags & 0x400) + Number(wide >= 2 ** 32) + Number(even) * 4
    + Number(value % 16 + 1 >= 16) * 0x10 + Number(result === 0) * 0x40 + Number(result >= 2 ** 31) * 0x80
    + Number(signed + 1 > 2 ** 31 - 1) * 0x800};
}
for (const [before, after, flags] of PRODUCERS) assert.deepEqual(addOne(before, 0xcd7), {value: after, flags});
for (const [value, left, leftCf, leftOf, right, rightCf, rightOf] of ANCHORS) {
  assert.deepEqual(rotate('left', value), {value: left, cf: leftCf, of: leftOf}); assert.deepEqual(rotate('right', value), {value: right, cf: rightCf, of: rightOf});
}
const banks = [];
for (const kind of ['left', 'right']) {
  const extension = kind === 'left' ? 0 : 8;
  for (let group = 0; group < 2; group++) {
    const bytes = Buffer.alloc(group === 0 ? 172 : 162, 0xcc), specs = [], scans = []; let offset = 0;
    for (let slot = 0; slot < 4; slot++) {
      const entry = offset, destination = group * 4 + slot; if (slot === 0) {bytes.set([0x83, 0xc3, 1], offset); offset += 3;}
      const rotate = PC + offset; for (const immediate of IMMEDIATES) {bytes.set([0xc1, 0xc0 | extension | destination, immediate], offset); offset += 3;}
      bytes.set([0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0xeb, 160 - offset - 8], offset); offset += 8;
      specs.push([PC + entry, offset - entry]); scans.push({entry: PC + entry, rotate, next: rotate + 24, destination, count: 8, canary: false});
    }
    assert.equal(offset, 131); bytes.set([0x0f, 0x0b], 160);
    const d1 = group === 0 ? {entry: PC + 162, rotate: PC + 162, next: PC + 164, destination: 0, count: 1, canary: false} : null;
    if (d1) bytes.set([0xd1, 0xc0 | extension, 0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0xeb, 0xf4], 162);
    const id = `register-${kind}-${group}`; assert.deepEqual(readFileSync(join(output, `${id}.x86`)), bytes);
    banks.push({id, kind, group, location: 'register', bytes, specs, scans, d1, cold: PC + 160, instructions: 45});
  }
  const bytes = Buffer.alloc(144, 0xcc), specs = [], scans = []; let offset = 0;
  const forms = [[0x83, 0xc3, 1, 0xc1, 5 | extension, 0x10, 0x40, 0, 0, 1], [0xc1, 0x40 | extension, 0x10, 33],
    [0xc1, 0x42 | extension, 0xfe, 65], [0xc1, 0x44 | extension, 0x24, 8, 97], [0xc1, 0x44 | extension, 0x87, 0x10, 129],
    [0xc1, 0x44 | extension, 0x57, 0xfe, 161], [0x8d, 0x5b, 1, 0xc1, 7 | extension, 193], [0xc1, 7 | extension, 225]];
  for (const [index, form] of forms.entries()) {
    const tail = [0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2]; if (index === 7) tail.push(0xbb, 0xef, 0xbe, 0xad, 0xde);
    const length = form.length + tail.length + 2; bytes.set([...form, ...tail, 0xeb, 128 - offset - length], offset);
    specs.push([PC + offset, length]); scans.push({entry: PC + offset, rotate: PC + offset + (index === 0 || index === 6 ? 3 : 0), next: PC + offset + form.length, count: 1, canary: index === 7, immediate: IMMEDIATES[index]}); offset += length;
  }
  assert.equal(offset, 111); bytes.set([0x0f, 0x0b], 128); bytes.set([0xd1, 5 | extension, 0x10, 0x40, 0, 0, 0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0xeb, 0xf0], 130);
  const id = `memory-${kind}`; assert.deepEqual(readFileSync(join(output, `${id}.x86`)), bytes);
  banks.push({id, kind, location: 'memory', bytes, specs, scans, d1: {entry: PC + 130, rotate: PC + 130, next: PC + 136, count: 1, canary: false}, cold: PC + 128, instructions: 35});
}
const counts = {contexts: 0, modules: 0, seeds: 0, register_matrix: 0, memory_matrix: 0, c1_register: 0, c1_memory: 0, d1_register: 0, d1_memory: 0,
  rol: 0, ror: 0, setb: 0, seto: 0, jumps: 0, canaries: 0, boundaries: 0, live_producers: 0, fault_calls: 0, read_faults: 0, write_faults: 0, prefixes: 0,
  repairs: 0, permission_only_repairs: 0, map_only_repairs: 0, code_stores: 0, same_value_code_stores: 0, changing_code_stores: 0, continuation_canaries: 0,
  mutations: 0, compiler_refusals: 0, rejection_survivals: 0, preflight: 0, owner_controls: 0, generated_calls: 0, maps: 0, unmaps: 0, protects: 0,
  host_uploads: 0, host_uploaded_bytes: 0, standalone_page_inputs: 0, pages: 0, arena_checks: 0};
const modules = [], pageRows = [], faultRows = [], smcRows = [], mutationRows = [], refusalRows = [], observedCl = new Set();
function record(magic, size, fields, version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired, detail = 0, address = 0, access = 0, version = 2) => record('R3EX', 40, [reason, retired, detail, address, access, access ? 4 : 0], version);
const helper = (value = 0, detail = 0, address = 0, access = 0) => record('R3MH', 40, [access ? 1 : 0, value, detail, address, access, access ? 4 : 0]);
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
  expected.set(state(Array(8).fill(0), 0, 2)); expected.set(record('R3EX', 40, [1, 0, 0, 0, 0, 0]), 56); expected.set(helper(), 100);
  const ctx = {owner, kind: bank.kind, bank, expected, low: ordinal, high: 0xc8404000, pages: new Map(), permissions: new Map(), units: []};
  if (owner === 'standalone') {
    ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; refresh(ctx).bytes.set(expected, ctx.base);
    for (const [address, bytes] of [[PC, Buffer.alloc(4096)], [KEEP, Buffer.from(pattern)]]) {ctx.pages.set(address, bytes); ctx.permissions.set(address, 3);}
    ctx.pages.get(PC).set(bank.bytes); ctx.bytes.set(ctx.pages.get(PC), 0x6000); ctx.bytes.set(pattern, 0x8000); counts.standalone_page_inputs += 2;
  } else {
    const instance = new WebAssembly.Instance(engineModule, {}), api = {};
    const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, unmap: 2, protect: 3, upload: 2, read32: 1,
      compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store32: 2, store_resident32: 6};
    for (const [name, arity] of Object.entries(arities)) {api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);}
    ctx.api = api; ctx.memory = instance.exports.memory; assert.equal(api.open(bank.location === 'memory' ? 7 : 3, ctx.low, ctx.high), 0);
    ctx.base = api.arena_ptr() >>> 0; check(ctx, 'independently initialized arena'); pure(ctx, () => api.map(PC, 1, 7), 'code map');
    ctx.pages.set(PC, Buffer.alloc(4096)); ctx.permissions.set(PC, 7); counts.maps++; dataInput(ctx, PC, bank.bytes); map(ctx, KEEP);
    if (bank.location === 'memory') for (const address of [DATA, SECOND, 0xfffff000, 0]) map(ctx, address);
  }
  check(ctx, 'complete initialized context'); return ctx;
}
function compile(ctx, specs = ctx.bank.specs, continuation = false, role = 'c1') {
  let bytes, binding = {}, imports = []; const pureModule = ctx.bank.location === 'register' || continuation;
  if (ctx.owner === 'standalone') bytes = readFileSync(join(output, `standalone-${role === 'd1' ? `d1-${ctx.kind}` : ctx.bank.id}.wasm`));
  else {
    request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat()));
    if (ctx.owner === 'replacement') {
      pure(ctx, () => ctx.api.compile_entries(specs.length, 0), `${specs.length}-entry compiler`);
      binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);
    } else {
      check(ctx, 'before resident compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
      binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
      assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only');
    }
    refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length);
    bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length));
    imports = (pureModule ? ['guard'] : ['guard', 'read32', 'store32']).map(name => ctx.owner === 'replacement' ? name : name === 'guard' ? 'guard_resident' : name === 'store32' ? 'store_resident32' : name);
    writeFileSync(join(output, `${ctx.owner}-${ctx.bank.id}-${ctx.low}-${counts.modules + 1}.wasm`), bytes, {flag: 'wx'});
  }
  const module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...imports.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.api ? {ring3: ctx.api} : {})}); assert.equal(child.exports.run.length, 4);
  const unit = {...binding, bytes, run: child.exports.run, exitVersion: pureModule ? 1 : 2}; counts.modules++; ctx.units.push(unit);
  modules.push({owner: ctx.owner, bank: ctx.bank.id, role, sha256: hash(bytes), length: bytes.length, imports, specs, exit_version: unit.exitVersion, code_page_sha256: hash(ctx.pages.get(PC)), ...binding}); return unit;
}
function seed(ctx, registers, pc, flags) {
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 100), ctx.base); ctx.expected.set(helper(0xdecafbad), 100); ctx.bytes.set(ctx.expected.subarray(100, 140), ctx.base + 100); check(ctx, 'one initial CPU/helper seed'); counts.seeds++;
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
function guestWord(ctx, address) {const bytes = Buffer.alloc(4); for (let index = 0; index < 4; index++) {const at = address + index; bytes[index] = ctx.pages.get(Math.floor(at / 4096) * 4096)[at % 4096];} return bytes.readUInt32LE();}
function observeWord(ctx, address, value) {
  check(ctx, 'before separate diagnostic read'); assert.equal(ctx.api.read32(address), 0); ctx.expected.set(helper(value), 100); check(ctx, 'diagnostic read changes helper only');
}
function pages(ctx, label, addresses = [...ctx.pages.keys()]) {
  for (const address of addresses) {
    const expected = ctx.pages.get(address); assert.ok(expected);
    const permissions = ctx.permissions.get(address); if (!(permissions & 1)) protect(ctx, address, permissions | 1);
    const actual = Buffer.alloc(4096);
    if (ctx.owner === 'standalone') actual.set(refresh(ctx).bytes.subarray(address === PC ? 0x6000 : 0x8000, (address === PC ? 0x6000 : 0x8000) + 4096));
    else for (let offset = 0; offset < 4096; offset += 4) {observeWord(ctx, address + offset, expected.readUInt32LE(offset)); actual.writeUInt32LE(refresh(ctx).view.getUint32(ctx.base + 120, true), offset);}
    assert.deepEqual(actual, expected, 'complete current declared page'); if (!(permissions & 1)) protect(ctx, address, permissions);
    counts.pages++; pageRows.push({owner: ctx.owner, kind: ctx.kind, label, address, sha256: hash(actual)});
  }
}
function countRotate(ctx, encoding) {counts[`${encoding}_${ctx.bank.location}`]++; counts[ctx.kind === 'left' ? 'rol' : 'ror']++;}
function consumers(ctx, unit, registers, scan, flags) {
  registers[0] = registers[0] - registers[0] % 256 + flags % 2; run(ctx, unit, 1, 'SETB observes live CF', registers, scan.next + 3, flags); counts.setb++;
  registers[2] = registers[2] - registers[2] % 256 + Number(Boolean(flags & 0x800)); run(ctx, unit, 1, 'SETO observes live OF', registers, scan.next + 6, flags); counts.seto++;
  if (scan.canary) {registers[3] = 0xdeadbeef; run(ctx, unit, 1, 'current MOV canary', registers, scan.next + 11, flags); counts.canaries++;}
  run(ctx, unit, 1, 'following JMP', registers, ctx.bank.cold, flags); counts.jumps++;
}
function registerChain(ctx, unit, registers, scan, flags, start = 0, encoding = 'c1') {
  for (let index = start; index < scan.count; index++) {
    const result = rotate(ctx.kind, registers[scan.destination]); flags = rotateFlags(flags, result); registers[scan.destination] = result.value;
    run(ctx, unit, 1, 'complete rotated CPU before partial consumers', registers, scan.rotate + (index + 1) * (encoding === 'd1' ? 2 : 3), flags); countRotate(ctx, encoding);
  }
  consumers(ctx, unit, registers, scan, flags); return flags;
}
function memoryRotate(ctx, unit, registers, scan, address, flags, encoding = 'c1', reason = 1) {
  const result = rotate(ctx.kind, guestWord(ctx, address)); flags = rotateFlags(flags, result); patch(ctx, address, words([result.value]));
  run(ctx, unit, 1, 'complete checked memory rotation before partial consumers', registers, scan.next, flags, reason, 1, helper()); countRotate(ctx, encoding);
  observeWord(ctx, address, result.value); return flags;
}
function rotateAndConsume(ctx, unit, registers, scan, address, flags, label, encoding = 'c1') {
  const after = memoryRotate(ctx, unit, registers, scan, address, flags, encoding); consumers(ctx, unit, registers, scan, after); return after;
}
function numeric(ctx, unit) {
  for (const [inputIndex, value] of INPUTS.entries()) for (const [form, scan] of ctx.bank.scans.entries()) for (const [flagIndex, flags] of FLAGS.entries()) {
    const registers = [...REG]; registers[1] = ECX[(inputIndex + form + flagIndex) % ECX.length];
    if (ctx.bank.location === 'register') registers[scan.destination] = value;
    else {
      if (form === 1) registers[0] = DATA; if (form === 2) registers[2] = WORD + 2; if (form === 3) registers[4] = WORD - 8;
      if (form === 4) {registers[0] = 3; registers[7] = DATA - 12;} if (form === 5) {registers[2] = 9; registers[7] = DATA;}
      if (form >= 6) registers[7] = WORD; dataInput(ctx, WORD, words([value]));
    }
    if (ctx.bank.location === 'memory' || scan.destination !== 1) observedCl.add(registers[1] % 256);
    seed(ctx, registers, scan.rotate, flags); if (inputIndex === 0 && form === 0 && flagIndex === 0) preflight(ctx, unit, registers, scan.rotate, flags);
    if (ctx.bank.location === 'register') registerChain(ctx, unit, registers, scan, flags);
    else rotateAndConsume(ctx, unit, registers, scan, WORD, flags);
    counts[`${ctx.bank.location}_matrix`]++;
  }
  if (ctx.bank.location === 'memory') for (const address of [0x4ffe, 0xfffffffc]) for (const value of [0x80000001, 0x12345678]) for (const flags of FLAGS) {
    const registers = [...REG], scan = ctx.bank.scans[3]; registers[4] = address - 8; dataInput(ctx, address, words([value])); seed(ctx, registers, scan.rotate, flags);
    rotateAndConsume(ctx, unit, registers, scan, address, flags); counts.boundaries++;
  }
  for (const [oldEbx, afterEbx, producedFlags, value] of PRODUCERS) {
    const registers = [...REG], scan = ctx.bank.scans[0]; registers[1] = ECX[4]; registers[3] = oldEbx;
    if (ctx.bank.location === 'register') registers[scan.destination] = value; else dataInput(ctx, WORD, words([value]));
    seed(ctx, registers, scan.entry, 0xcd7); if (oldEbx === 0xffffffff) preflight(ctx, unit, registers, scan.entry, 0xcd7);
    const produced = addOne(oldEbx, 0xcd7); assert.deepEqual(produced, {value: afterEbx, flags: producedFlags}); registers[3] = afterEbx;
    run(ctx, unit, 1, 'ADD producer full CPU before rotate', registers, scan.rotate, producedFlags); counts.live_producers++;
    if (ctx.bank.location === 'register') registerChain(ctx, unit, registers, scan, producedFlags);
    else rotateAndConsume(ctx, unit, registers, scan, WORD, producedFlags);
  }
}
function d1Controls(ctx) {
  const scan = ctx.bank.d1, unit = compile(ctx, [[scan.entry, ctx.bank.location === 'register' ? 10 : 14]], false, 'd1');
  for (const [value, flags] of D1_ROWS) {
    const registers = [...REG]; if (ctx.bank.location === 'register') registers[0] = value; else dataInput(ctx, WORD, words([value])); seed(ctx, registers, scan.rotate, flags);
    if (ctx.bank.location === 'register') registerChain(ctx, unit, registers, scan, flags, 0, 'd1'); else rotateAndConsume(ctx, unit, registers, scan, WORD, flags, 'D1 control', 'd1');
  }
}
const faultShapes = [
  {name: 'unmapped', address: 0x8000, detail: 1, fault: 0x8000, access: 1, missing: 0x8000},
  {name: 'write-only', address: WORD, detail: 2, fault: WORD, access: 1, denied: DATA, permissions: 2},
  {name: 'cross-read-unmapped', address: 0x4ffe, detail: 1, fault: SECOND, access: 1, missing: SECOND},
  {name: 'cross-read-denied', address: 0x4ffe, detail: 2, fault: SECOND, access: 1, denied: SECOND, permissions: 2},
  {name: 'overflow', address: 0xfffffffd, detail: 3, fault: 0xfffffffd, access: 1},
  {name: 'cross-first-read-only', address: 0x4ffe, detail: 2, fault: 0x4ffe, access: 2, denied: DATA, permissions: 1},
  {name: 'cross-second-read-only', address: 0x4ffe, detail: 2, fault: SECOND, access: 2, denied: SECOND, permissions: 1},
];
function faults(ctx, unit) {
  const scan = ctx.bank.scans[6];
  for (const shape of faultShapes) for (const flags of [2, 0xcd7]) {
    for (const address of [DATA, SECOND]) {if (!ctx.pages.has(address)) map(ctx, address); protect(ctx, address, 3);}
    const original = shape.access === 2 ? 0x80000001 : 0x12345678;
    const setupValue = shape.detail === 3 || shape.missing === 0x8000 ? null : original;
    if (setupValue !== null) dataInput(ctx, shape.address, words([setupValue]));
    if (shape.missing !== undefined && ctx.pages.has(shape.missing)) unmap(ctx, shape.missing);
    if (shape.denied !== undefined) protect(ctx, shape.denied, shape.permissions);
    const registers = [...REG]; registers[7] = shape.address; seed(ctx, registers, scan.entry, flags); registers[3]++;
    const faultHelper = helper(0, shape.detail, shape.fault, shape.access);
    run(ctx, unit, 2, 'retired LEA then precise RMW fault', registers, scan.rotate, flags, 5, 1, faultHelper, shape.detail, shape.fault, shape.access); counts.prefixes++;
    const touched = [...new Set([Math.floor(shape.address / 4096) * 4096, Math.floor((shape.address + 3) / 4096) * 4096])].filter(address => ctx.pages.has(address));
    const faultPages = touched.length ? touched : [DATA, SECOND]; pages(ctx, `${shape.name} first fault atomicity`, faultPages);
    if (flags === 2) preflight(ctx, unit, registers, scan.rotate, flags);
    run(ctx, unit, 1, 'unrepaired retry retires zero and preserves full CPU', registers, scan.rotate, flags, 5, 0, faultHelper, shape.detail, shape.fault, shape.access);
    counts.fault_calls += 2; counts[shape.access === 1 ? 'read_faults' : 'write_faults'] += 2; pages(ctx, `${shape.name} retry atomicity`, faultPages);
    const permissionOnly = shape.access === 2 && flags === 0xcd7;
    const mapOnly = shape.missing === 0x8000 && flags === 0xcd7;
    const repairValue = shape.detail === 3 ? null : mapOnly ? 0 : permissionOnly ? original : 0x89abcdef;
    const repairMode = shape.detail === 3 ? null : mapOnly ? 'map-only' : permissionOnly ? 'permission-only' : shape.missing !== undefined ? 'map+value' : 'permission+value';
    if (repairValue !== null) {
      if (shape.missing !== undefined) map(ctx, shape.missing, mapOnly ? null : pattern); if (shape.denied !== undefined) protect(ctx, shape.denied, 3);
      if (mapOnly) counts.map_only_repairs++; else if (permissionOnly) counts.permission_only_repairs++; else dataInput(ctx, shape.address, words([repairValue]));
      assert.equal(guestWord(ctx, shape.address), repairValue);
      rotateAndConsume(ctx, unit, registers, scan, shape.address, flags, 'same CPU/module retry after declared data repair'); counts.repairs++;
      if (shape.missing === 0x8000) {pages(ctx, 'repaired complete unmapped source page', [0x8000]); unmap(ctx, 0x8000);}
    }
    faultRows.push({owner: ctx.owner, kind: ctx.kind, ...shape, flags, setup_value: setupValue, retired_prefix: 1, unrepaired_retired: 0, repair_value: repairValue, repair_mode: repairMode});
  }
  for (const address of [DATA, SECOND]) protect(ctx, address, 3); pages(ctx, 'complete post-fault pages');
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
  protect(ctx, KEEP, 7);
  for (const immediate of [0, 2, 32, 255]) {
    const extension = ctx.kind === 'left' ? 0 : 8, bytes = ctx.bank.location === 'register' ? Buffer.from([0x66, 0xc1, 0xc0 | extension, immediate, 0xeb, 0, 0x0f, 0x0b])
      : Buffer.from([0xc1, 5 | extension, 0x10, 0x40, 0, 0, immediate, 0xeb, 0, 0x0f, 0x0b]);
    dataInput(ctx, KEEP, bytes); const published = [ctx.api.generation(), ctx.api.module_ptr(), ctx.api.module_len()];
    request(ctx, words(ctx.owner === 'replacement' ? [KEEP] : [KEEP, bytes.length - 2]));
    pure(ctx, () => ctx.owner === 'replacement' ? ctx.api.compile_entries(1, 0) : ctx.api.compile_resident(1), ctx.bank.location === 'register' ? 'word-prefixed register count refused' : 'other memory effective count refused', 10); counts.compiler_refusals++;
    assert.deepEqual([ctx.api.generation(), ctx.api.module_ptr(), ctx.api.module_len()], published, 'failed compiler preserves published metadata');
    assert.deepEqual(Buffer.from(refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length)), unit.bytes, 'failed compiler preserves published module bytes');
    const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), pc = ctx.expected.readUInt32LE(48), flags = ctx.expected.readUInt32LE(52);
    run(ctx, unit, 0, 'published owner remains valid after refusal', registers, pc, flags, 1, 0); counts.rejection_survivals++;
    refusalRows.push({owner: ctx.owner, bank: ctx.bank.id, immediate, prefix: ctx.bank.location === 'register' ? 0x66 : null, status: 10, published_module_sha256: hash(unit.bytes)});
  }
  dataInput(ctx, KEEP, pattern.subarray(0, ctx.bank.location === 'register' ? 8 : 11)); protect(ctx, KEEP, 3); pages(ctx, 'restored patterned compiler-rejection page', [KEEP]);
}
function mutation(ctx, unit) {
  const scan = ctx.bank.scans[0], registers = [...REG], beforeFlags = 0xcd7; let flags;
  if (ctx.bank.location === 'register') registers[scan.destination] = 0x80000001; else dataInput(ctx, WORD, words([0x80000001])); seed(ctx, registers, scan.rotate, beforeFlags);
  if (ctx.bank.location === 'register') {
    const result = rotate(ctx.kind, registers[scan.destination]); flags = rotateFlags(beforeFlags, result); registers[scan.destination] = result.value;
    run(ctx, unit, 1, 'first immediate 1 completes before code mutation', registers, scan.rotate + 3, flags); countRotate(ctx, 'c1');
  } else flags = memoryRotate(ctx, unit, registers, scan, WORD, beforeFlags);
  const address = scan.rotate + (ctx.bank.location === 'register' ? 2 : 6); assert.equal(ctx.pages.get(PC)[address - PC], 1);
  dataInput(ctx, address, Buffer.from([33])); cancel(ctx, 1);
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'consumed immediate 1 to 33 invalidates before other controls', 4); counts.owner_controls++; counts.generated_calls++; cancel(ctx, 0);
  const current = compile(ctx, ctx.bank.specs, false, 'mutated-c1');
  if (ctx.bank.location === 'register') registerChain(ctx, current, registers, scan, flags, 1); else consumers(ctx, current, registers, scan, flags);
  mutationRows.push({owner: ctx.owner, bank: ctx.bank.id, address, old_byte: 1, new_byte: 33, upload_bytes: 1, resume_pc: scan.rotate + (ctx.bank.location === 'register' ? 3 : 7), stale_status: 4, cpu_writes_during_continuation: 0});
  counts.mutations++; pages(ctx, 'current complete pages after consumed immediate upload'); return current;
}
function close(ctx) {
  if (ctx.owner === 'standalone') return; pure(ctx, () => ctx.api.close(), 'close');
  for (const unit of ctx.units) {pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'closed owner before pointers/cancel/budget', 5); counts.owner_controls++; counts.generated_calls++;}
}
function smc(ctx, unit, value) {
  const address = 0x1f00, scan = ctx.bank.scans[7], registers = [...REG]; registers[7] = address; seed(ctx, registers, scan.rotate, 0xcd7);
  const result = rotate(ctx.kind, value), flags = rotateFlags(0xcd7, result); patch(ctx, address, words([result.value]));
  run(ctx, unit, 1, 'Store4 commits helper/flags/EIP/retirement before code exit', registers, scan.next, flags, 6, 1, helper()); countRotate(ctx, 'c1'); counts.code_stores++;
  counts[value === result.value ? 'same_value_code_stores' : 'changing_code_stores']++; observeWord(ctx, address, result.value); pages(ctx, 'complete committed code-store pages');
  smcRows.push({owner: ctx.owner, kind: ctx.kind, immediate: 225, before: value, after: result.value, address, pc: scan.next, flags, retired: 1, canary_before: registers[3]});
  cancel(ctx, 1); pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'stale after successful code store', 4); counts.owner_controls++; counts.generated_calls++; cancel(ctx, 0);
  const current = compile(ctx, [[scan.next, 13]], true, 'current-code-tail'); consumers(ctx, current, registers, scan, flags); counts.continuation_canaries++;
  observeWord(ctx, address, result.value); pages(ctx, 'fresh current consumers/canary/JMP keep committed pages'); close(ctx);
}
for (const bank of banks.filter(bank => bank.location === 'register')) for (const owner of ['standalone', 'replacement', 'resident']) {
  const ctx = fresh(owner, bank); let unit = compile(ctx); pages(ctx, 'initial pure pages'); numeric(ctx, unit); pages(ctx, 'finite C1 chains preserve full pages');
  controls(ctx, unit); ownerChecks(ctx, unit);
  if (owner !== 'standalone' && bank.group === 0) {rejectedCounts(ctx, unit); unit = mutation(ctx, unit);}
  if (bank.group === 0) d1Controls(ctx);
  if (owner !== 'standalone' && bank.group === 1) {
    dataInput(ctx, PC, Buffer.from([ctx.pages.get(PC)[0]])); cancel(ctx, 1); pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'same-byte code upload invalidates owner', 4); counts.owner_controls++; counts.generated_calls++; cancel(ctx, 0);
  }
  pages(ctx, 'final current pure pages'); close(ctx);
}
for (const bank of banks.filter(bank => bank.location === 'memory')) for (const owner of ['replacement', 'resident']) {
  const ctx = fresh(owner, bank); dataInput(ctx, 0x1f00, words([0])); d1Controls(ctx); let unit = compile(ctx); pages(ctx, 'initial checked-memory pages');
  numeric(ctx, unit); pages(ctx, 'finite C1 memory rows and boundaries'); faults(ctx, unit); controls(ctx, unit); ownerChecks(ctx, unit); rejectedCounts(ctx, unit); unit = mutation(ctx, unit); smc(ctx, unit, 0);
  for (const value of [0xffffffff, 0x80000001]) {const extra = fresh(owner, bank); dataInput(extra, 0x1f00, words([value])); const extraUnit = compile(extra); ownerChecks(extra, extraUnit); smc(extra, extraUnit, value);}
}
const expectedCounts = {contexts: 24, modules: 54, seeds: 650, register_matrix: 288, memory_matrix: 192, c1_register: 2528, c1_memory: 296,
  d1_register: 18, d1_memory: 12, rol: 1427, ror: 1427, setb: 642, seto: 642, jumps: 642, canaries: 36, boundaries: 32, live_producers: 32,
  fault_calls: 112, read_faults: 80, write_faults: 32, prefixes: 56, repairs: 48, permission_only_repairs: 8, map_only_repairs: 4,
  code_stores: 12, same_value_code_stores: 8, changing_code_stores: 4, continuation_canaries: 12, mutations: 8, compiler_refusals: 32, rejection_survivals: 32,
  preflight: 200, owner_controls: 112, generated_calls: 5264, maps: 104, unmaps: 16, protects: 264, host_uploads: 488, host_uploaded_bytes: 332480,
  standalone_page_inputs: 8, pages: 512};
for (const [name, expected] of Object.entries(expectedCounts)) assert.equal(counts[name], expected, `exact observed ${name} census`);
assert.deepEqual([...observedCl].sort((a, b) => a - b), [0, 1, 31, 32, 255]);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, modules, faults: faultRows, code_stores: smcRows, mutations: mutationRows, compiler_refusals: refusalRows, pages: pageRows,
  source: {url: 'https://cdrdv2-public.intel.com/929354/253667-093-sdm-vol-2b.pdf', order: '253667-093US', edition: 'September 2026', pdf_sha256: 'a261998ace8e07f624bf3e2486bb6cfe950fb8be8d0dc9631bcecb4c5ec953e4', extract_sha256: '7a0e03302580204f99e8283af389f84ec082701807f1a9615a028af4d6f849d0', pages: [541, 542, 543, 544, 545]},
  oracle: {sources: INPUTS, flag_seeds: FLAGS, immediate_bytes: IMMEDIATES, literal_anchors: ANCHORS, live_add_anchors: PRODUCERS, d1_rows: D1_ROWS, registers: REG, ecx_seeds: ECX,
    fault_shapes: faultShapes, repair_modes: ['map-only', 'permission-only', 'map+value', 'permission+value'], arena_bytes: SIZE, patterned_page: KEEP, pattern_sha256: hash(pattern),
    banks: banks.map(bank => ({id: bank.id, specs: bank.specs, scans: bank.scans, d1: bank.d1, instructions: bank.instructions, sha256: hash(bank.bytes), hex: bank.bytes.toString('hex')})),
    flag_policy: 'CF/OF from effective count-one rotation; preserve SF/ZF/AF/PF/DF/fixed bit 1; memory flags publish only after successful Store4'},
  test_sha256: Object.fromEntries(['engine/tests/cpu_rotate_immediate_one_wasm.rs', 'engine/tests/fixtures/p2-rotate-immediate-one/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'finite flat32 C1 ROL/ROR encodings for masked-one immediate bytes 1/33/65/97/129/161/193/225: all eight register destinations through actual standalone/replacement/resident modules, eight declared memory EA/immediate forms through actual replacement/resident modules; three initial sources and two FLAGS seeds plus declared evolved intermediates, independent binary-string/literal rotation and widened/literal ADD oracle, complete producer and rotate CPU before live partial consumers without CPU repair; compact D1 controls; exact existing imports and 4236-byte arena/current declared pages; precise Read4/Write4 faults and atomic failure, retired LEA, zero-retirement retry, permission-only/map-only/changed-value data repairs without CPU/module reset, overflow separately unrepairable; unchanged and changing code stores reason 6 then stale 4, fresh current consumers/canary/JMP without CPU writes, closed 5; one-byte consumed immediate 1 to 33 uploads invalidate despite equal semantics and fresh compiled banks resume same CPU/EIP; actual compiler refusals use word-prefixed register forms and prefix-free non-one memory counts 0/2/32/255, preserving published modules; register KEEP restore covers all eight uploaded bytes; this fixture purpose remains finite masked-one arithmetic, with prior unchanged R400/R401 full FLAGS math evidence retained rather than rerun'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, result_sha256: hash(resultBytes), counts, output}));
