import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = 0x1080, DATA = 0x4000, SECOND = 0x5000, TOP = 0xfffff000;
const REG = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const INPUTS = [0, ...Array.from({length: 32}, (_, bit) => 2 ** bit), 0x101, 0x80000001, 0x01010101, 0xffffffff, 0xdeadbeef];
const FLAGS = [0, 0x400].flatMap(df => [0, 0x40].flatMap(zf => [0, 4].flatMap(pf => [0, 0x891].map(other => 2 + df + zf + pf + other))));
const ANCHORS = [[0, null, null, true], [1, 0, 0, false], [0x100, 8, 8, false], [0x101, 0, 8, true],
  [0x80000000, 31, 31, false], [0x80000001, 0, 31, true], [0x01010101, 0, 24, true], [0xffffffff, 0, 31, true], [0xdeadbeef, 0, 31, true]];
function scan(kind, value) {
  const bits = value.toString(2).padStart(32, '0'), ones = bits.replaceAll('0', '').length;
  return {index: value === 0 ? null : 31 - (kind === 'forward' ? bits.lastIndexOf('1') : bits.indexOf('1')), even: ones % 2 === 0, zero: value === 0};
}
for (const [value, low, high, even] of ANCHORS) {
  assert.deepEqual(scan('forward', value), {index: low, even, zero: value === 0}); assert.deepEqual(scan('reverse', value), {index: high, even, zero: value === 0});
}
assert.equal(new Set(INPUTS).size, 38); assert.equal(new Set(FLAGS).size, 16); assert.ok(FLAGS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
const pattern = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1)), data = Buffer.from(pattern);
INPUTS.forEach((value, index) => data.writeUInt32LE(value, index * 4));
for (const [index, value] of [0, 0x101, 0xdeadbeef].entries()) {
  data.writeUInt32LE(value, 0x106 + index * 4); data.writeUInt32LE(value, 0x128 + index * 8);
}
const banks = Object.fromEntries(['forward', 'reverse'].flatMap(kind => ['matrix', 'base', 'flow'].map(bank => {
  const bytes = Buffer.alloc(130, 0xcc), opcode = kind === 'forward' ? 0xbc : 0xbd, specs = [], lengths = [];
  function block(offset, body) {
    const instructions = [...body, 0x0f, 0x94, 0xc0, 0x0f, 0x9a, 0xc2, 0xeb, 0x80 - offset - body.length - 8];
    bytes.set(instructions, offset); specs.push([PC + offset, instructions.length]); lengths.push(body.length);
  }
  if (bank === 'matrix') for (let destination = 0; destination < 8; destination++) block(destination * 11, [0x0f, opcode, destination << 3 | 3]);
  else if (bank === 'base') for (let destination = 0; destination < 8; destination++) {
    const body = destination === 4 ? [0x0f, opcode, 0x24, 0x24] : destination === 5 ? [0x0f, opcode, 0x6d, 0] : [0x0f, opcode, destination << 3 | destination];
    block(destination * 12, body);
  } else {
    block(0, [0x0f, opcode, 0x44, 0x43, 4]); block(0x20, [0x0f, opcode, 0x54, 0x93, 4]);
    block(0x40, [0x0f, opcode, 0x3b]); block(0x60, [0x8d, 0x49, 1, 0x0f, opcode, 0x3b]);
  }
  bytes.set([0x0f, 0x0b], 128); const name = `${kind}-${bank}`; writeFileSync(join(output, `${name}.x86`), bytes, {flag: 'wx'});
  return [name, {bytes, specs, lengths}];
})));
const counts = {contexts: 0, modules: 0, seeds: 0, matrix_scan: 0, base_scan: 0, index_scan: 0, boundary_scan: 0,
  repaired_scan: 0, bsf: 0, bsr: 0, zero_destination_preserved: 0, setz: 0, setp: 0, jumps: 0, fault_calls: 0,
  prefixes: 0, repairs: 0, preflight: 0, owner_controls: 0, generated_calls: 0, pages: 0, arena_checks: 0};
const modules = [], faultRows = [], pageRows = [];
function record(magic, size, fields, version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired, detail = 0, address = 0) => record('R3EX', 40, [reason, retired, detail, address, reason === 5 ? 1 : 0, reason === 5 ? 4 : 0], 2);
const helper = (value = 0, detail = 0, address = 0) => record('R3MH', 40, [detail ? 1 : 0, detail ? 0 : value, detail, address, detail ? 1 : 0, detail ? 4 : 0]);
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.name}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'host request');}
function upload(ctx, address, bytes) {request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), 'data/code upload');}
function map(ctx, address, bytes = pattern) {pure(ctx, () => ctx.api.map(address, 1, 3), 'data map'); upload(ctx, address, bytes); ctx.pages.set(address, Buffer.from(bytes));}
function unmap(ctx, address) {pure(ctx, () => ctx.api.unmap(address, 1), 'data unmap'); ctx.pages.delete(address);}
function protect(ctx, address, permissions) {pure(ctx, () => ctx.api.protect(address, 1, permissions), 'data permission input');}
function dataInput(ctx, address, bytes) {
  upload(ctx, address, bytes); const page = Math.floor(address / 4096) * 4096; assert.ok(address % 4096 + bytes.length <= 4096);
  ctx.pages.get(page).set(bytes, address % 4096);
}
function guestWord(ctx, address) {
  const bytes = Buffer.from(Array.from({length: 4}, (_, index) => {
    const at = address + index, page = Math.floor(at / 4096) * 4096; assert.ok(ctx.pages.has(page)); return ctx.pages.get(page)[at % 4096];
  })); return bytes.readUInt32LE();
}
function fresh(owner, kind, bank) {
  const instance = new WebAssembly.Instance(engineModule, {}), ordinal = ++counts.contexts;
  const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, unmap: 2, protect: 3, upload: 2, read32: 1, write8: 2,
    compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
  const api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
    const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
  }));
  const ctx = {owner, kind, bank, name: `${kind}-${bank}`, api, memory: instance.exports.memory, low: ordinal, high: 0xc8399000, expected: Buffer.alloc(SIZE), pages: new Map()};
  assert.equal(api.open(6, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(record('R3EX', 40, [1, 0, 0, 0, 0, 0]), 56); ctx.expected.set(helper(), 100); check(ctx, 'independently initialized arena');
  request(ctx, pattern); pure(ctx, () => api.map(PC, 1, 7), 'code map'); upload(ctx, PC, banks[ctx.name].bytes);
  map(ctx, DATA, data); protect(ctx, DATA, 1);
  if (bank === 'flow') {map(ctx, SECOND); protect(ctx, SECOND, 1); map(ctx, TOP); protect(ctx, TOP, 1);} return ctx;
}
function compile(ctx) {
  const {specs} = banks[ctx.name]; let binding;
  if (ctx.owner === 'replacement') {
    request(ctx, words(specs.map(([entry]) => entry))); pure(ctx, () => ctx.api.compile_entries(specs.length, 0), 'entry compiler');
    binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);
  } else {
    request(ctx, words(specs.flat())); check(ctx, 'before extent compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
    assert.equal(ctx.view.getUint32(ctx.base + TRANSFER, true), 1); assert.equal(ctx.view.getUint32(ctx.base + TRANSFER + 4, true), 24);
    binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
    assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only');
  }
  refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length);
  const bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)), module = new WebAssembly.Module(bytes);
  const guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: guard, kind: 'function'}, {module: 'ring3', name: 'read32', kind: 'function'}]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}); assert.equal(child.exports.run.length, 4);
  writeFileSync(join(output, `${ctx.owner}-${ctx.name}.wasm`), bytes, {flag: 'wx'}); counts.modules++;
  modules.push({owner: ctx.owner, kind: ctx.kind, bank: ctx.bank, sha256: hash(bytes), length: bytes.length, ...binding}); return {...binding, run: child.exports.run};
}
function seed(ctx, registers, pc, flags) {
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96); ctx.expected.set(helper(0xdecafbad), 100);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 140), ctx.base); check(ctx, 'one initial cpu seed'); counts.seeds++;
}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, label, registers, pc, flags, reason = 1, retired = 1, read, detail = 0, faultAddress = 0) {
  check(ctx, `before ${label}`); ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired, detail, faultAddress), 56);
  if (read !== undefined || detail !== 0) ctx.expected.set(helper(read, detail, faultAddress), 100);
  assert.equal(unit.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0, label); check(ctx, label); counts.generated_calls++;
}
function preflight(ctx, unit, registers, pc, flags) {
  run(ctx, unit, 0, 'zero budget before unconditional read', registers, pc, flags, 1, 0); counts.preflight++;
  cancel(ctx, 1); run(ctx, unit, 1, 'cancel before unconditional read', registers, pc, flags, 2, 0); counts.preflight++; cancel(ctx, 0);
}
function scanAndConsume(ctx, unit, registers, destination, value, afterScan, oldFlags, label) {
  const result = scan(ctx.kind, value), flags = 2 + (oldFlags & 0x400) + (result.zero ? 0x40 : 0) + (result.even ? 4 : 0);
  if (!result.zero) registers[destination] = result.index;
  else {assert.notEqual(registers[destination], 0, 'zero source has dirty destination'); counts.zero_destination_preserved++;}
  run(ctx, unit, 1, label, registers, afterScan, flags, 1, 1, value); counts[ctx.kind === 'forward' ? 'bsf' : 'bsr']++;
  registers[0] = registers[0] - registers[0] % 256 + Number(result.zero);
  run(ctx, unit, 1, 'SETZ observes guest-produced ZF', registers, afterScan + 3, flags); counts.setz++;
  registers[2] = registers[2] - registers[2] % 256 + Number(result.even);
  run(ctx, unit, 1, 'SETP observes whole-source parity', registers, afterScan + 6, flags); counts.setp++;
  run(ctx, unit, 1, 'following jump', registers, COLD, flags); counts.jumps++;
}
function page(ctx, address) {
  const expected = ctx.pages.get(address), actual = Buffer.alloc(4096); assert.ok(expected);
  for (let offset = 0; offset < 4096; offset += 4) {
    check(ctx, 'before diagnostic read'); assert.equal(ctx.api.read32(address + offset), 0); refresh(ctx);
    actual.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset); ctx.expected.set(helper(expected.readUInt32LE(offset)), 100); check(ctx, 'diagnostic read changes helper only');
  }
  assert.deepEqual(actual, expected, 'complete current declared source/neighbor page'); counts.pages++;
  pageRows.push({owner: ctx.owner, kind: ctx.kind, bank: ctx.bank, address, sha256: hash(actual)});
}
function matrix(ctx, unit) {
  for (const [index, value] of INPUTS.entries()) for (let destination = 0; destination < 8; destination++) for (const flags of FLAGS) {
    const registers = [...REG], entry = PC + destination * 11; registers[3] = DATA + index * 4; seed(ctx, registers, entry, flags);
    if (index === 0 && destination === 0 && flags === FLAGS[0]) preflight(ctx, unit, registers, entry, flags);
    scanAndConsume(ctx, unit, registers, destination, value, entry + 3, flags, `source-${value}-dst-${destination}-flags-${flags}`); counts.matrix_scan++;
  }
}
function bases(ctx, unit) {
  for (let destination = 0; destination < 8; destination++) for (const index of [0, 33, 37]) for (const flags of [2, 0xcd7]) {
    const registers = [...REG], entry = PC + destination * 12; registers[destination] = DATA + index * 4; seed(ctx, registers, entry, flags);
    if (destination === 0 && index === 0 && flags === 2) preflight(ctx, unit, registers, entry, flags);
    scanAndConsume(ctx, unit, registers, destination, INPUTS[index], entry + banks[ctx.name].lengths[destination], flags, 'old destination base address'); counts.base_scan++;
  }
}
const faultShapes = [
  {name: 'unmapped', address: 0x8000, detail: 1, fault: 0x8000, mapRepair: 0x8000},
  {name: 'write-only', address: DATA, detail: 2, fault: DATA, permissionRepair: DATA},
  {name: 'cross-page-unmapped', address: 0x4ffe, detail: 1, fault: SECOND, mapRepair: SECOND},
  {name: 'cross-page-write-only', address: 0x4ffe, detail: 2, fault: SECOND, permissionRepair: SECOND},
  {name: 'overflow', address: 0xfffffffd, detail: 3, fault: 0xfffffffd},
];
function repair(ctx, shape, value) {
  const bytes = words([value]);
  if (shape.address === 0x4ffe) {
    protect(ctx, DATA, 3); dataInput(ctx, 0x4ffe, bytes.subarray(0, 2)); protect(ctx, DATA, 1);
    if (shape.mapRepair !== undefined) {const next = Buffer.from(pattern); next.set(bytes.subarray(2), 0); map(ctx, SECOND, next);}
    else dataInput(ctx, SECOND, bytes.subarray(2));
    protect(ctx, SECOND, 1);
  } else if (shape.mapRepair !== undefined) {const next = Buffer.from(pattern); next.set(bytes, 0); map(ctx, shape.mapRepair, next); protect(ctx, shape.mapRepair, 1);}
  else {dataInput(ctx, shape.address, bytes); protect(ctx, shape.permissionRepair, 1);}
}
function flow(ctx, unit) {
  for (const destination of [0, 2]) for (const [index, value] of [0, 0x101, 0xdeadbeef].entries()) for (const flags of [2, 0xcd7]) {
    const registers = [...REG], entry = PC + (destination === 0 ? 0 : 0x20), factor = destination === 0 ? 2 : 4;
    registers[destination] = index * 2 + 1; registers[3] = DATA + (destination === 0 ? 0x100 : 0x120); seed(ctx, registers, entry, flags);
    if (destination === 0 && index === 0 && flags === 2) preflight(ctx, unit, registers, entry, flags);
    assert.equal(guestWord(ctx, registers[3] + registers[destination] * factor + 4), value);
    scanAndConsume(ctx, unit, registers, destination, value, entry + 5, flags, 'old EAX/EDX destination index'); counts.index_scan++;
  }
  for (const address of [0x4ffe, 0xfffffffc]) for (const flags of [2, 0xcd7]) {
    const registers = [...REG]; registers[3] = address; seed(ctx, registers, PC + 0x40, flags);
    scanAndConsume(ctx, unit, registers, 7, guestWord(ctx, address), PC + 0x43, flags, 'cross-page/final valid Read4'); counts.boundary_scan++;
  }
  page(ctx, SECOND);
  for (const shape of faultShapes) {
    if (shape.name === 'cross-page-unmapped') unmap(ctx, SECOND);
    if (shape.name === 'cross-page-write-only') {map(ctx, SECOND); protect(ctx, SECOND, 1);}
    for (const value of shape.name === 'overflow' ? [null] : [0, 0xdeadbeef]) for (const flags of [2, 0xcd7]) {
      if (shape.permissionRepair !== undefined) protect(ctx, shape.permissionRepair, 2);
      const registers = [...REG]; registers[3] = shape.address; seed(ctx, registers, PC + 0x60, flags); registers[1]++;
      run(ctx, unit, 20, `${shape.name}/prefix-then-fault`, registers, PC + 0x63, flags, 5, 1, undefined, shape.detail, shape.fault); counts.prefixes++; counts.fault_calls++;
      run(ctx, unit, 1, 'unrepaired retry preserves full CPU/prefix', registers, PC + 0x63, flags, 5, 0, undefined, shape.detail, shape.fault); counts.fault_calls++;
      preflight(ctx, unit, registers, PC + 0x63, flags);
      if (value !== null) {
        repair(ctx, shape, value); assert.equal(guestWord(ctx, shape.address), value);
        scanAndConsume(ctx, unit, registers, 7, value, PC + 0x66, flags, 'data-only repair resumes same scan'); counts.repaired_scan++; counts.repairs++;
        if (shape.mapRepair !== undefined) unmap(ctx, shape.mapRepair);
      }
      faultRows.push({owner: ctx.owner, kind: ctx.kind, shape: shape.name, flags, repair_value: value, request: shape.address, fault: shape.fault, detail: shape.detail});
    }
  }
  page(ctx, SECOND); page(ctx, TOP);
}
function guards(ctx, unit) {
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'malformed pointers', 1); counts.owner_controls++; counts.generated_calls++;
  for (const wrong of ['key', 'identity']) {
    const low = wrong === 'key' ? ctx.low ^ 1 : ctx.low;
    const action = ctx.owner === 'replacement'
      ? () => ctx.api.guard(low, ctx.high, unit.generation + (wrong === 'identity' ? 1 : 0), ctx.base, ctx.base + 56, ctx.base + 96)
      : () => ctx.api.guard_resident(low, ctx.high, unit.low ^ (wrong === 'identity' ? 1 : 0), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
    pure(ctx, action, `wrong-${wrong}`, 3); counts.owner_controls++;
  }
  check(ctx, 'before same-byte invalidation'); assert.equal(ctx.api.write8(PC, banks[ctx.name].bytes[0]), 0);
  ctx.expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 1], 3), 100); check(ctx, 'same-byte write helper only');
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'stale before pointer validation', 4); counts.owner_controls++; counts.generated_calls++;
  pure(ctx, () => ctx.api.close(), 'close retains arena');
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'closed before pointer validation', 5); counts.owner_controls++; counts.generated_calls++;
}
for (const owner of ['replacement', 'resident']) for (const kind of ['forward', 'reverse']) for (const [bank, action] of [['matrix', matrix], ['base', bases], ['flow', flow]]) {
  const ctx = fresh(owner, kind, bank), unit = compile(ctx); page(ctx, DATA); action(ctx, unit); page(ctx, DATA); guards(ctx, unit);
}
assert.equal(counts.contexts, 12); assert.equal(counts.modules, 12); assert.equal(counts.seeds, 19784);
assert.equal(counts.matrix_scan, 19456); assert.equal(counts.base_scan, 192); assert.equal(counts.index_scan, 48); assert.equal(counts.boundary_scan, 16); assert.equal(counts.repaired_scan, 64);
assert.equal(counts.bsf, 9888); assert.equal(counts.bsr, 9888); assert.equal(counts.zero_destination_preserved, 624);
assert.equal(counts.setz, 19776); assert.equal(counts.setp, 19776); assert.equal(counts.jumps, 19776); assert.equal(counts.fault_calls, 144); assert.equal(counts.prefixes, 72);
assert.equal(counts.repairs, 64); assert.equal(counts.preflight, 168); assert.equal(counts.owner_controls, 60); assert.equal(counts.generated_calls, 79452); assert.equal(counts.pages, 36);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, modules, faults: faultRows, pages: pageRows,
  oracle: {sources: INPUTS, flag_seeds: FLAGS, literal_anchors: ANCHORS, registers: REG, bank_specs: Object.fromEntries(Object.entries(banks).map(([name, bank]) => [name, bank.specs])),
    flag_policy: 'ZF iffzero; PF whole32 source even ones count; CF/OF/SF/AF0; DF/fixed2 preserved; zero source retains dirty destination',
    program_sha256: Object.fromEntries(Object.entries(banks).map(([name, bank]) => [name, hash(bank.bytes)])), arena_bytes: SIZE, initial_data_page_sha256: hash(data), patterned_page_sha256: hash(pattern)},
  test_sha256: Object.fromEntries(['engine/tests/cpu_memory_bit_scan_wasm.rs', 'engine/tests/fixtures/p2-memory-bit-scan/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'flat32 memory-source BSF/BSR in actual engine-generated replacement/resident modules with direct read32;38 source rows/16 FLAGS/all8 destinations plus old destination base aliases for all8 and EAX/EDX index aliases, cross-page/final valid Read4; independent bit-string index/whole-source parity and literal anchors, zero still publishes real helper read and preserves dirty destination, first scan observed before live SETZAL/SETPDL; exact Read4 faults/committed LEA/unrepaired retry0, declared data-only repair at same PC/state/module then actual consumers/JMP without CPU repair, overflow separately unrepairable; full GPR/FLAGS/arena and complete current declared source/neighbor pages, explicit host data writes recorded by repair values/page hashes; no standalone memory binding, exhaustive operands/128FLAGS/POPCNT/TZCNT/LZCNT/wider ISA, PE/provider, SDK/browser, performance or game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
