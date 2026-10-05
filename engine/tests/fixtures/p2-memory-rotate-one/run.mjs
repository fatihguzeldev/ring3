import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = 0x1080, DATA = 0x4000, SECOND = 0x5000, WORD = 0x4010;
const REG = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const INPUTS = [0, 1, 2, 0x100, 0x40000000, 0x80000000, 0x7fffffff, 0xffffffff, 0x80000001, 0x12345678, 0xaaaaaaaa, 0x55555555];
const FLAGS = Array.from({length: 128}, (_, seed) => 2 + [1, 4, 0x10, 0x40, 0x80, 0x400, 0x800].reduce((flags, bit, index) => flags + (seed & 2 ** index ? bit : 0), 0));
const ECX = [0xa1b2c300, 0xa1b2c301, 0xa1b2c31f, 0xa1b2c320, 0xa1b2c3ff];
const ANCHORS = [[0, 0, 0, 0, 0, 0, 0], [1, 2, 0, 0, 0x80000000, 1, 1], [2, 4, 0, 0, 1, 0, 0],
  [0x40000000, 0x80000000, 0, 1, 0x20000000, 0, 0], [0x40000002, 0x80000004, 0, 1, 0x20000001, 0, 0],
  [0x80000000, 1, 1, 1, 0x40000000, 0, 1], [0x7fffffff, 0xfffffffe, 0, 1, 0xbfffffff, 1, 1],
  [0xffffffff, 0xffffffff, 1, 0, 0xffffffff, 1, 0], [0x80000001, 3, 1, 1, 0xc0000000, 1, 0],
  [0x12345678, 0x2468acf0, 0, 0, 0x091a2b3c, 0, 0], [0xaaaaaaaa, 0x55555555, 1, 1, 0x55555555, 0, 1], [0x55555555, 0xaaaaaaaa, 0, 1, 0xaaaaaaaa, 1, 1]];
function rotate(kind, value) {
  const bits = value.toString(2).padStart(32, '0'), result = kind === 'left' ? bits.slice(1) + bits[0] : bits[31] + bits.slice(0, 31);
  const cf = Number(result[kind === 'left' ? 31 : 0]);
  return {value: parseInt(result, 2), cf, of: Number(result[0] !== (kind === 'left' ? String(cf) : result[1]))};
}
const rotateFlags = (before, result) => [2, 4, 0x10, 0x40, 0x80, 0x400].reduce((flags, bit) => flags + (before & bit ? bit : 0), 0) + result.cf + result.of * 0x800;
const PRODUCERS = [[0xffffffff, 0, 0x457, 0x80000001], [0x7fffffff, 0x80000000, 0xc96, 0x40000002]];
function addOne(value, flags) {
  const wide = value + 1, result = wide % 2 ** 32, signed = value < 2 ** 31 ? value : value - 2 ** 32;
  return {value: result, flags: 2 + (flags & 0x400) + Number(wide >= 2 ** 32)
    + Number((result % 256).toString(2).replaceAll('0', '').length % 2 === 0) * 4 + Number(value % 16 + 1 >= 16) * 0x10
    + Number(result === 0) * 0x40 + Number(result >= 2 ** 31) * 0x80 + Number(signed + 1 > 2 ** 31 - 1) * 0x800};
}
for (const [value, left, leftCf, leftOf, right, rightCf, rightOf] of ANCHORS) {
  assert.deepEqual(rotate('left', value), {value: left, cf: leftCf, of: leftOf}); assert.deepEqual(rotate('right', value), {value: right, cf: rightCf, of: rightOf});
}
for (const [ebx, after, flags] of PRODUCERS) assert.deepEqual(addOne(ebx, 0xcd7), {value: after, flags});
assert.equal(new Set(INPUTS).size, 12); assert.equal(new Set(FLAGS).size, 128); assert.ok(FLAGS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
const banks = Object.fromEntries(['left', 'right'].map(kind => {
  const extension = kind === 'left' ? 0 : 8, bytes = Buffer.alloc(130, 0xcc), specs = [], scans = []; let offset = 0;
  const forms = [[0x83, 0xc3, 1, 0xd1, 5 | extension, 0x10, 0x40, 0, 0], [0xd1, 0x40 | extension, 0x10],
    [0xd1, 0x42 | extension, 0xfe], [0xd1, 0x44 | extension, 0x24, 8], [0xd1, 0x44 | extension, 0x87, 0x10],
    [0xd1, 0x44 | extension, 0x57, 0xfe], [0x8d, 0x5b, 1, 0xd1, 7 | extension], [0xd1, 7 | extension]];
  for (const [index, form] of forms.entries()) {
    const tail = index === 7 ? [0xbb, 0xef, 0xbe, 0xad, 0xde] : [0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2], length = form.length + tail.length + 2;
    bytes.set([...form, ...tail, 0xeb, 128 - offset - length], offset); specs.push([PC + offset, length]);
    scans.push({entry: PC + offset, rotate: PC + offset + ([0, 6].includes(index) ? 3 : 0), next: PC + offset + form.length}); offset += length;
  }
  assert.equal(offset, 97); bytes.set([0x0f, 0x0b], 128); assert.deepEqual(readFileSync(join(output, `${kind}.x86`)), bytes);
  return [kind, {bytes, specs, scans}];
}));
const counts = {contexts: 0, modules: 0, seeds: 0, matrix: 0, bases: 0, indexes: 0, boundaries: 0, repaired: 0, live_producers: 0,
  rol: 0, ror: 0, setb: 0, seto: 0, jumps: 0, fault_calls: 0, read_faults: 0, write_faults: 0, prefixes: 0, repairs: 0, permission_only_repairs: 0,
  code_stores: 0, same_value_code_stores: 0, changing_code_stores: 0, preflight: 0, owner_controls: 0, generated_calls: 0,
  maps: 0, unmaps: 0, protects: 0, host_uploads: 0, host_uploaded_bytes: 0, pages: 0, arena_checks: 0};
const modules = [], pageRows = [], faultRows = [], smcRows = [], observedCl = new Set();
function record(magic, size, fields, version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired, detail = 0, address = 0, access = 0) => record('R3EX', 40, [reason, retired, detail, address, access, access ? 4 : 0], 2);
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
  dataInput(ctx, address, bytes);
}
function fresh(owner, kind) {
  const instance = new WebAssembly.Instance(engineModule, {}), api = {}, expected = Buffer.alloc(SIZE), ordinal = ++counts.contexts;
  const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, unmap: 2, protect: 3, upload: 2, read32: 1,
    compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store32: 2, store_resident32: 6};
  for (const [name, arity] of Object.entries(arities)) {api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);}
  const ctx = {owner, kind, api, memory: instance.exports.memory, low: ordinal, high: 0xc8401000, expected, pages: new Map(), permissions: new Map()};
  assert.equal(api.open(7, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  expected.set(state(Array(8).fill(0), 0, 2)); expected.set(record('R3EX', 40, [1, 0, 0, 0, 0, 0]), 56); expected.set(helper(), 100); check(ctx, 'independently initialized arena');
  pure(ctx, () => api.map(PC, 1, 7), 'code map'); const codePage = Buffer.alloc(4096); ctx.pages.set(PC, codePage); ctx.permissions.set(PC, 7); counts.maps++;
  dataInput(ctx, PC, banks[kind].bytes); for (const address of [0x3000, DATA, SECOND, 0xfffff000, 0]) map(ctx, address); return ctx;
}
function compile(ctx) {
  const specs = banks[ctx.kind].specs; request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat())); let binding;
  if (ctx.owner === 'replacement') {
    pure(ctx, () => ctx.api.compile_entries(specs.length, 0), 'eight-entry compiler');
    binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);
  } else {
    check(ctx, 'before eight-extent compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
    binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
    assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only');
  }
  refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length);
  const bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)), module = new WebAssembly.Module(bytes);
  const imports = ['guard', 'read32', 'store32'].map(name => ctx.owner === 'replacement' ? name : name === 'guard' ? 'guard_resident' : name === 'store32' ? 'store_resident32' : name);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...imports.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]); const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}); assert.equal(child.exports.run.length, 4);
  writeFileSync(join(output, `${ctx.owner}-${ctx.kind}-${counts.contexts}.wasm`), bytes, {flag: 'wx'}); counts.modules++;
  modules.push({owner: ctx.owner, kind: ctx.kind, sha256: hash(bytes), length: bytes.length, imports, ...binding}); return {...binding, run: child.exports.run};
}
function seed(ctx, registers, pc, flags) {
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 100), ctx.base); ctx.expected.set(helper(0xdecafbad), 100); ctx.bytes.set(ctx.expected.subarray(100, 140), ctx.base + 100); check(ctx, 'one initial CPU/helper seed'); counts.seeds++;
}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, label, registers, pc, flags, reason = 1, retired = 1, helperBytes, detail = 0, fault = 0, access = 0) {
  check(ctx, `before ${label}`); ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired, detail, fault, access), 56);
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
    for (let offset = 0; offset < 4096; offset += 4) {observeWord(ctx, address + offset, expected.readUInt32LE(offset)); actual.writeUInt32LE(refresh(ctx).view.getUint32(ctx.base + 120, true), offset);}
    assert.deepEqual(actual, expected, 'complete current declared page'); if (!(permissions & 1)) protect(ctx, address, permissions);
    counts.pages++; pageRows.push({owner: ctx.owner, kind: ctx.kind, label, address, sha256: hash(actual)});
  }
}
function rotateAndConsume(ctx, unit, registers, scan, address, flags, label) {
  const result = rotate(ctx.kind, guestWord(ctx, address)), afterFlags = rotateFlags(flags, result); patch(ctx, address, words([result.value]));
  run(ctx, unit, 1, label, registers, scan.next, afterFlags, 1, 1, helper()); counts[ctx.kind === 'left' ? 'rol' : 'ror']++;
  observeWord(ctx, address, result.value);
  registers[0] = registers[0] - registers[0] % 256 + result.cf; run(ctx, unit, 1, 'SETB observes live CF', registers, scan.next + 3, afterFlags); counts.setb++;
  registers[2] = registers[2] - registers[2] % 256 + result.of; run(ctx, unit, 1, 'SETO observes live OF', registers, scan.next + 6, afterFlags); counts.seto++;
  run(ctx, unit, 1, 'following jump', registers, COLD, afterFlags); counts.jumps++;
}
function numeric(ctx, unit) {
  const scans = banks[ctx.kind].scans;
  for (const [index, value] of INPUTS.entries()) for (const [flagIndex, flags] of FLAGS.entries()) {
    const registers = [...REG]; registers[1] = ECX[(index + flagIndex) % ECX.length]; observedCl.add(registers[1] % 256); dataInput(ctx, WORD, words([value])); seed(ctx, registers, scans[0].rotate, flags);
    if (index === 0 && flagIndex === 0) preflight(ctx, unit, registers, scans[0].rotate, flags);
    rotateAndConsume(ctx, unit, registers, scans[0], WORD, flags, 'absolute rotate full state before partial consumers'); counts.matrix++;
  }
  for (let form = 1; form <= 5; form++) for (const value of [0, 0x80000001, 0x12345678]) for (const flags of [2, 0xcd7]) {
    const registers = [...REG]; if (form === 1) registers[0] = DATA; if (form === 2) registers[2] = WORD + 2; if (form === 3) registers[4] = WORD - 8;
    if (form === 4) {registers[0] = 3; registers[7] = DATA - 12;} if (form === 5) {registers[2] = 9; registers[7] = DATA;}
    dataInput(ctx, WORD, words([value])); seed(ctx, registers, scans[form].rotate, flags); rotateAndConsume(ctx, unit, registers, scans[form], WORD, flags, 'old GPR effective address preserved'); counts[form < 4 ? 'bases' : 'indexes']++;
  }
  for (const address of [0x4ffe, 0x4ffc, 0xfffffffc]) for (const value of [0, 0x80000001]) for (const flags of [2, 0xcd7]) {
    const registers = [...REG]; registers[0] = address - 0x10; dataInput(ctx, address, words([value])); seed(ctx, registers, scans[1].rotate, flags);
    rotateAndConsume(ctx, unit, registers, scans[1], address, flags, 'cross-page or final valid Read4/Store4'); counts.boundaries++;
  }
  for (const [ebx, after, flags, value] of PRODUCERS) {
    const registers = [...REG]; registers[3] = ebx; registers[1] = ECX[4]; dataInput(ctx, WORD, words([value])); seed(ctx, registers, scans[0].entry, 0xcd7);
    if (ebx === 0xffffffff) preflight(ctx, unit, registers, scans[0].entry, 0xcd7);
    assert.deepEqual(addOne(ebx, 0xcd7), {value: after, flags}); registers[3] = after;
    run(ctx, unit, 1, 'ADD produces complete live flags', registers, scans[0].rotate, flags); counts.live_producers++;
    rotateAndConsume(ctx, unit, registers, scans[0], WORD, flags, 'rotate retains producer status flags');
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
  const scan = banks[ctx.kind].scans[6];
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
    const repairValue = shape.detail === 3 ? null : permissionOnly ? original : 0x89abcdef;
    const repairMode = shape.detail === 3 ? null : permissionOnly ? 'permission-only' : shape.missing !== undefined ? 'map+value' : 'permission+value';
    if (repairValue !== null) {
      if (shape.missing !== undefined) map(ctx, shape.missing); if (shape.denied !== undefined) protect(ctx, shape.denied, 3);
      if (permissionOnly) counts.permission_only_repairs++; else dataInput(ctx, shape.address, words([repairValue]));
      assert.equal(guestWord(ctx, shape.address), repairValue);
      rotateAndConsume(ctx, unit, registers, scan, shape.address, flags, 'same CPU/module retry after declared data repair'); counts.repaired++; counts.repairs++;
      if (shape.missing === 0x8000) unmap(ctx, 0x8000);
    }
    faultRows.push({owner: ctx.owner, kind: ctx.kind, ...shape, flags, setup_value: setupValue, retired_prefix: 1, unrepaired_retired: 0, repair_value: repairValue, repair_mode: repairMode});
  }
  for (const address of [DATA, SECOND]) protect(ctx, address, 3); pages(ctx, 'complete post-fault pages');
}
function controls(ctx, unit) {
  seed(ctx, [...REG], COLD, 2);
  for (const [name, cancelled, budget, reason] of [['zero budget', 0, 0, 1], ['cancel with budget', 1, 1, 2], ['cancel with zero budget', 1, 0, 2]]) {
    cancel(ctx, cancelled); run(ctx, unit, budget, name, REG, COLD, 2, reason, 0); counts.preflight++;
  }
  cancel(ctx, 0); run(ctx, unit, 1, 'cold entry', REG, COLD, 2, 3, 0); counts.preflight++;
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'malformed pointers', 1); counts.preflight++; counts.generated_calls++;
}
function ownerChecks(ctx, unit) {
  for (const wrong of ['key', 'identity']) {
    const low = ctx.low ^ Number(wrong === 'key');
    const action = ctx.owner === 'replacement' ? () => ctx.api.guard(low, ctx.high, unit.generation + Number(wrong === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96)
      : () => ctx.api.guard_resident(low, ctx.high, unit.low ^ Number(wrong === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
    pure(ctx, action, `wrong ${wrong}`, 3); counts.owner_controls++;
  }
}
function smc(ctx, unit, value) {
  const address = 0x1f00, scan = banks[ctx.kind].scans[7], registers = [...REG]; registers[7] = address;
  const result = rotate(ctx.kind, value), flags = rotateFlags(0xcd7, result); seed(ctx, registers, scan.rotate, 0xcd7); patch(ctx, address, words([result.value]));
  run(ctx, unit, 1, 'code-page Store4 commits before budget/canary', registers, scan.next, flags, 6, 1, helper()); counts[ctx.kind === 'left' ? 'rol' : 'ror']++; counts.code_stores++;
  counts[value === result.value ? 'same_value_code_stores' : 'changing_code_stores']++; observeWord(ctx, address, result.value); pages(ctx, 'committed code-alias store');
  smcRows.push({owner: ctx.owner, kind: ctx.kind, before: value, after: result.value, address, pc: scan.next, flags, retired: 1, canary_ebx: registers[3]});
  cancel(ctx, 1); pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'stale before cancel/budget/pointers', 4); counts.owner_controls++; counts.generated_calls++;
  pure(ctx, () => ctx.api.close(), 'close'); pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'closed before stale/cancel/pointers', 5); counts.owner_controls++; counts.generated_calls++;
}
for (const owner of ['replacement', 'resident']) for (const kind of ['left', 'right']) {
  const ctx = fresh(owner, kind); dataInput(ctx, 0x1f00, words([0])); const unit = compile(ctx); pages(ctx, 'initial pages');
  numeric(ctx, unit); pages(ctx, 'numeric pages'); faults(ctx, unit); controls(ctx, unit); ownerChecks(ctx, unit); smc(ctx, unit, 0);
  for (const value of [0xffffffff, 0x40000002]) {const extra = fresh(owner, kind); dataInput(extra, 0x1f00, words([value])); const extraUnit = compile(extra); ownerChecks(extra, extraUnit); smc(extra, extraUnit, value);}
}
assert.equal(counts.contexts, 12); assert.equal(counts.modules, 12); assert.equal(counts.seeds, 6392);
assert.equal(counts.matrix, 6144); assert.equal(counts.bases, 72); assert.equal(counts.indexes, 48); assert.equal(counts.boundaries, 48); assert.equal(counts.repaired, 48);
assert.equal(counts.rol, 3190); assert.equal(counts.ror, 3190); assert.equal(counts.setb, 6368); assert.equal(counts.seto, 6368); assert.equal(counts.jumps, 6368);
assert.equal(counts.fault_calls, 112); assert.equal(counts.read_faults, 80); assert.equal(counts.write_faults, 32); assert.equal(counts.prefixes, 56); assert.equal(counts.repairs, 48);
assert.equal(counts.permission_only_repairs, 8);
assert.equal(counts.live_producers, 8); assert.equal(counts.code_stores, 12); assert.equal(counts.same_value_code_stores, 8); assert.equal(counts.changing_code_stores, 4);
assert.equal(counts.preflight, 92); assert.equal(counts.owner_controls, 48); assert.equal(counts.generated_calls, 25720); assert.equal(counts.pages, 320);
assert.deepEqual([...observedCl].sort((a, b) => a - b), [0, 1, 31, 32, 255]);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, modules, faults: faultRows, code_stores: smcRows, pages: pageRows,
  source: {url: 'https://cdrdv2-public.intel.com/929354/253667-093-sdm-vol-2b.pdf', order: '253667-093US', edition: 'September 2026',
    pdf_sha256: 'a261998ace8e07f624bf3e2486bb6cfe950fb8be8d0dc9631bcecb4c5ec953e4', extract_sha256: '7a0e03302580204f99e8283af389f84ec082701807f1a9615a028af4d6f849d0', pages: [541, 542, 543, 544, 545]},
  oracle: {sources: INPUTS, flag_seeds: FLAGS, literal_anchors: ANCHORS, live_add_anchors: PRODUCERS, registers: REG, ecx_seeds: ECX,
    observed_cl: [...observedCl].sort((a, b) => a - b), fault_shapes: faultShapes, repair_values: [0x80000001, 0x89abcdef],
    repair_modes: ['permission-only', 'map+value', 'permission+value'], arena_bytes: SIZE, untouched_page: 0x3000, pattern_sha256: hash(pattern),
    banks: Object.fromEntries(Object.entries(banks).map(([kind, bank]) => [kind, {specs: bank.specs, scans: bank.scans, hex: bank.bytes.toString('hex'), sha256: hash(bank.bytes), instructions: 33}])),
    flag_policy: 'CF and OF from count-one rotation after validated Store4; SF, ZF, AF, PF, DF and fixed bit 1 preserved'},
  test_sha256: Object.fromEntries(['engine/tests/cpu_memory_rotate_one_wasm.rs', 'engine/tests/fixtures/p2-memory-rotate-one/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'flat32 count-one memory ROL/ROR through actual engine-Wasm-generated replacement/resident modules and exact existing Read4/Store4 imports; declared 12 source values at one address with all 128 valid FLAGS, focused old EAX/EDX/ESP base and scaled EAX/EDX index forms, cross-page and final valid word; independent binary-string/literal rotation and widened/literal ADD oracle, first complete rotate state before live SETB AL/SETO DL/JMP without CPU repair; precise Read4/Write4 faults, retired LEA, unrepaired retry zero, eight permission-only repairs and declared map/permission/value repairs at same CPU/module with changed data re-read, overflow separately unrepairable, failed cross-page stores leave full pages unchanged; same-value and changing code-page stores publish successful Store4, new FLAGS/EIP and one retirement with reason 6 before canary, then stale 4 and closed 5; complete 4236-byte arena and current declared pages with explicit host input counters/page hashes; no standalone memory binding, exhaustive operands/EA forms, other counts, carry-through rotate, wider ISA, ABI, PE/provider, SDK/browser, performance or game claim'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, result_sha256: hash(resultBytes), counts, output}));
