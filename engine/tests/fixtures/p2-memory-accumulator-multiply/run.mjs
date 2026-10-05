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
const PAIRS = [[0, 0xffffffff], [1, 0xffffffff], [0xffffffff, 0xffffffff], [0x80000000, 0xffffffff], [0x7fffffff, 2],
  [0x80000000, 1], [0x80000000, 2], [0x10000, 0x10000], [0xfffffffe, 0x80000001], [0x12345678, 0x89abcdef]];
const ANCHORS = {
  unsigned: ['0000000000000000', '00000000ffffffff', 'fffffffe00000001', '7fffffff80000000', '00000000fffffffe',
    '0000000080000000', '0000000100000000', '0000000100000000', '7ffffffffffffffe', '09ca39e0e242d208'],
  signed: ['0000000000000000', 'ffffffffffffffff', '0000000000000001', '0000000080000000', '00000000fffffffe',
    'ffffffff80000000', 'ffffffff00000000', '0000000100000000', '00000000fffffffe', 'f795e368e242d208'],
};
const FLAGS = [0, 0x400].flatMap(df => [0, 1].flatMap(cf => [0, 0x800].flatMap(of => [0, 0xd4].map(other => 2 + df + cf + of + other))));
assert.equal(new Set(FLAGS).size, 16); assert.ok(FLAGS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
function product(kind, a, b) {
  const value = input => kind === 'signed' ? BigInt.asIntN(32, BigInt(input)) : BigInt(input);
  const wide = value(a) * value(b), bits = BigInt.asUintN(64, wide);
  return {low: Number(bits % 4294967296n), high: Number(bits / 4294967296n), hex: bits.toString(16).padStart(16, '0'),
    overflow: kind === 'signed' ? wide < -2147483648n || wide > 2147483647n : wide > 4294967295n};
}
for (const kind of ['unsigned', 'signed']) for (const [index, [a, b]] of PAIRS.entries()) assert.equal(product(kind, a, b).hex, ANCHORS[kind][index]);
const pattern = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1)), data = Buffer.from(pattern);
PAIRS.forEach(([, value], index) => data.writeUInt32LE(value, index * 4));
for (const [offset, value] of [[0x40, 0xfffffffe], [0x46, 0x89abcdef], [0x5c, 0xffffffff], [0x60, 0x80000001]]) data.writeUInt32LE(value, offset);
const specs = [[PC, 6], [PC + 0x20, 4], [PC + 0x30, 4], [PC + 0x40, 6], [PC + 0x50, 6], [PC + 0x60, 4], [PC + 0x70, 7]];
const programs = Object.fromEntries(['unsigned', 'signed'].map(kind => {
  const bytes = Buffer.alloc(130, 0xcc), field = kind === 'unsigned' ? 0x20 : 0x28, branch = kind === 'unsigned' ? 0x72 : 0x70;
  const bodies = [[0, [0xf7, field | 3, 0xf7, field | 3]], [0x20, [0xf7, field]], [0x30, [0xf7, field | 2]],
    [0x40, [0xf7, field | 0x44, 0x43, 0x40]], [0x50, [0xf7, field | 0x44, 0x93, 0x50]],
    [0x60, [0xf7, field | 3]], [0x70, [0x8d, 0x49, 1, 0xf7, field | 3]]];
  for (const [offset, body] of bodies) bytes.set([...body, branch, 0x80 - offset - body.length - 2], offset);
  bytes.set([0x0f, 0x0b], 128); writeFileSync(join(output, `${kind}.x86`), bytes, {flag: 'wx'}); return [kind, bytes];
}));
const counts = {contexts: 0, modules: 0, seeds: 0, canonical_multiply: 0, alias_multiply: 0, boundary_multiply: 0,
  repaired_multiply: 0, branches: 0, fault_calls: 0, zero_eax_fault_calls: 0, prefixes: 0, repairs: 0, preflight: 0,
  owner_controls: 0, generated_calls: 0, pages: 0, arena_checks: 0};
const modules = [], faultRows = [];
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
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.kind}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'host request');}
function upload(ctx, address, bytes) {request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), 'data/code upload');}
function map(ctx, address, bytes = pattern) {pure(ctx, () => ctx.api.map(address, 1, 3), 'data map'); upload(ctx, address, bytes); ctx.pages.set(address, Buffer.from(bytes));}
function unmap(ctx, address) {pure(ctx, () => ctx.api.unmap(address, 1), 'data unmap'); ctx.pages.delete(address);}
function protect(ctx, address, permissions) {pure(ctx, () => ctx.api.protect(address, 1, permissions), 'data permission input');}
function guestWord(ctx, address) {
  const bytes = Buffer.from(Array.from({length: 4}, (_, index) => {
    const at = address + index, page = Math.floor(at / 4096) * 4096; assert.ok(ctx.pages.has(page)); return ctx.pages.get(page)[at % 4096];
  })); return bytes.readUInt32LE();
}
function fresh(owner, kind) {
  const instance = new WebAssembly.Instance(engineModule, {}), ordinal = ++counts.contexts;
  const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, unmap: 2, protect: 3, upload: 2, read32: 1, write8: 2,
    compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
  const api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
    const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
  }));
  const ctx = {owner, kind, api, memory: instance.exports.memory, low: ordinal, high: 0xc8397000, expected: Buffer.alloc(SIZE), pages: new Map()};
  assert.equal(api.open(6, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(record('R3EX', 40, [1, 0, 0, 0, 0, 0]), 56); ctx.expected.set(helper(), 100); check(ctx, 'independently initialized arena');
  request(ctx, pattern); pure(ctx, () => api.map(PC, 1, 7), 'code map'); upload(ctx, PC, programs[kind]);
  map(ctx, DATA, data); protect(ctx, DATA, 1); map(ctx, SECOND); protect(ctx, SECOND, 1); map(ctx, TOP); protect(ctx, TOP, 1); return ctx;
}
function compile(ctx) {
  let binding;
  if (ctx.owner === 'replacement') {
    request(ctx, words(specs.map(([entry]) => entry))); pure(ctx, () => ctx.api.compile_entries(specs.length, 0), 'seven-entry compiler');
    binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);
  } else {
    request(ctx, words(specs.flat())); check(ctx, 'before seven-extent compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
    assert.equal(ctx.view.getUint32(ctx.base + TRANSFER, true), 1); assert.equal(ctx.view.getUint32(ctx.base + TRANSFER + 4, true), 24);
    binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
    assert.ok(binding.low !== 0 || binding.high !== 0);
    ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only');
  }
  refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length);
  const bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)), module = new WebAssembly.Module(bytes);
  const guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: guard, kind: 'function'}, {module: 'ring3', name: 'read32', kind: 'function'}]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}); assert.equal(child.exports.run.length, 4);
  writeFileSync(join(output, `${ctx.owner}-${ctx.kind}.wasm`), bytes, {flag: 'wx'}); counts.modules++;
  modules.push({owner: ctx.owner, kind: ctx.kind, sha256: hash(bytes), length: bytes.length, ...binding}); return {...binding, run: child.exports.run};
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
function multiply(ctx, unit, registers, pc, initialFlags, value, label) {
  const result = product(ctx.kind, registers[0], value); registers[0] = result.low; registers[2] = result.high;
  const flags = 2 + (initialFlags & 0x400) + (result.overflow ? 0x801 : 0);
  run(ctx, unit, 1, label, registers, pc, flags, 1, 1, value); return {flags, overflow: result.overflow};
}
function branch(ctx, unit, registers, fallthrough, result) {
  run(ctx, unit, 1, 'branch consumes current full-product overflow', registers, result.overflow ? COLD : fallthrough, result.flags); counts.branches++;
}
function page(ctx, address) {
  const expected = ctx.pages.get(address), actual = Buffer.alloc(4096); assert.ok(expected);
  for (let offset = 0; offset < 4096; offset += 4) {
    check(ctx, 'before diagnostic read'); assert.equal(ctx.api.read32(address + offset), 0); refresh(ctx);
    actual.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset); ctx.expected.set(helper(expected.readUInt32LE(offset)), 100); check(ctx, 'diagnostic read changes helper only');
  }
  assert.deepEqual(actual, expected, 'complete source and neighbors unchanged'); counts.pages++;
}
function canonical(ctx, unit) {
  for (const [pairIndex, [eax, value]] of PAIRS.entries()) for (const flags of FLAGS) {
    const registers = [...REG]; registers[0] = eax; registers[3] = DATA + pairIndex * 4; seed(ctx, registers, PC, flags);
    if (pairIndex === 0 && flags === FLAGS[0]) preflight(ctx, unit, registers, PC, flags);
    let result;
    for (const step of [1, 2]) {result = multiply(ctx, unit, registers, PC + step * 2, flags, value, `pair-${pairIndex}-flags-${flags}-product-${step}`); counts.canonical_multiply++;}
    branch(ctx, unit, registers, PC + 6, result);
  }
}
function aliases(ctx, unit) {
  const forms = [{entry: PC + 0x20, length: 2, address: DATA + 0x40, changes: {0: DATA + 0x40}},
    {entry: PC + 0x30, length: 2, address: DATA + 0x60, changes: {0: 0x89abcdef, 2: DATA + 0x60}},
    {entry: PC + 0x40, length: 4, address: DATA + 0x46, changes: {0: 3, 3: DATA}},
    {entry: PC + 0x50, length: 4, address: DATA + 0x5c, changes: {0: 0x12345678, 2: 3, 3: DATA}}];
  for (const form of forms) for (const flags of FLAGS) {
    const registers = [...REG]; for (const [index, value] of Object.entries(form.changes)) registers[index] = value;
    seed(ctx, registers, form.entry, flags);
    const result = multiply(ctx, unit, registers, form.entry + form.length, flags, guestWord(ctx, form.address), 'old EAX/EDX base/index address'); counts.alias_multiply++;
    branch(ctx, unit, registers, form.entry + form.length + 2, result);
  }
}
function boundaries(ctx, unit) {
  for (const address of [0x4ffe, 0xfffffffc]) for (const eax of [0, 0xffffffff, 0x80000000, 0x12345678]) for (const flags of [2, 0xcd7]) {
    const registers = [...REG]; registers[0] = eax; registers[3] = address; seed(ctx, registers, PC + 0x60, flags);
    const result = multiply(ctx, unit, registers, PC + 0x62, flags, guestWord(ctx, address), 'cross-page/final valid Read4'); counts.boundary_multiply++;
    branch(ctx, unit, registers, PC + 0x64, result);
  }
}
const faultShapes = [
  {name: 'unmapped', address: 0x8000, detail: 1, fault: 0x8000, mapRepair: 0x8000},
  {name: 'write-only', address: DATA, detail: 2, fault: DATA, permissionRepair: DATA},
  {name: 'cross-page-unmapped', address: 0x4ffe, detail: 1, fault: SECOND, mapRepair: SECOND},
  {name: 'cross-page-write-only', address: 0x4ffe, detail: 2, fault: SECOND, permissionRepair: SECOND},
  {name: 'overflow', address: 0xfffffffd, detail: 3, fault: 0xfffffffd},
];
function faultMatrix(ctx, unit) {
  for (const shape of faultShapes) {
    if (shape.name === 'cross-page-unmapped') unmap(ctx, SECOND);
    if (shape.name === 'cross-page-write-only') {map(ctx, SECOND); protect(ctx, SECOND, 1);}
    for (const eax of [0, 0x89abcdef]) for (const flags of [2, 0xcd7]) {
      if (shape.permissionRepair !== undefined) protect(ctx, shape.permissionRepair, 2);
      const registers = [...REG]; registers[0] = eax; registers[3] = shape.address; seed(ctx, registers, PC + 0x70, flags); registers[1]++;
      run(ctx, unit, 20, `${shape.name}/eax-${eax}/prefix-then-fault`, registers, PC + 0x73, flags, 5, 1, undefined, shape.detail, shape.fault); counts.prefixes++; counts.fault_calls++;
      run(ctx, unit, 1, 'unrepaired retry preserves EAX/EDX/FLAGS and prefix', registers, PC + 0x73, flags, 5, 0, undefined, shape.detail, shape.fault); counts.fault_calls++;
      if (eax === 0) counts.zero_eax_fault_calls += 2;
      preflight(ctx, unit, registers, PC + 0x73, flags);
      if (shape.mapRepair !== undefined || shape.permissionRepair !== undefined) {
        if (shape.mapRepair !== undefined) {map(ctx, shape.mapRepair); protect(ctx, shape.mapRepair, 1);} else protect(ctx, shape.permissionRepair, 1);
        const result = multiply(ctx, unit, registers, PC + 0x75, flags, guestWord(ctx, shape.address), 'data-only repair resumes same multiply'); counts.repaired_multiply++; counts.repairs++;
        branch(ctx, unit, registers, PC + 0x77, result);
        if (shape.mapRepair !== undefined) unmap(ctx, shape.mapRepair);
      }
      faultRows.push({owner: ctx.owner, kind: ctx.kind, shape: shape.name, eax, flags, request: shape.address, fault: shape.fault, detail: shape.detail, repaired: shape.name !== 'overflow'});
    }
  }
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
  check(ctx, 'before same-byte invalidation'); assert.equal(ctx.api.write8(PC, programs[ctx.kind][0]), 0);
  ctx.expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 1], 3), 100); check(ctx, 'same-byte write helper only');
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'stale before pointer validation', 4); counts.owner_controls++; counts.generated_calls++;
  pure(ctx, () => ctx.api.close(), 'close retains arena');
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'closed before pointer validation', 5); counts.owner_controls++; counts.generated_calls++;
}
for (const owner of ['replacement', 'resident']) for (const kind of ['unsigned', 'signed']) {
  const ctx = fresh(owner, kind), unit = compile(ctx); page(ctx, DATA); canonical(ctx, unit); aliases(ctx, unit); boundaries(ctx, unit);
  page(ctx, SECOND); faultMatrix(ctx, unit); page(ctx, DATA); page(ctx, SECOND); page(ctx, TOP); guards(ctx, unit);
}
assert.equal(counts.contexts, 4); assert.equal(counts.modules, 4); assert.equal(counts.seeds, 1040);
assert.equal(counts.canonical_multiply, 1280); assert.equal(counts.alias_multiply, 256); assert.equal(counts.boundary_multiply, 64); assert.equal(counts.repaired_multiply, 64);
assert.equal(counts.branches, 1024); assert.equal(counts.fault_calls, 160); assert.equal(counts.zero_eax_fault_calls, 80); assert.equal(counts.prefixes, 80);
assert.equal(counts.repairs, 64); assert.equal(counts.preflight, 168); assert.equal(counts.owner_controls, 20); assert.equal(counts.generated_calls, 3028); assert.equal(counts.pages, 20);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, modules, faults: faultRows,
  oracle: {operand_pairs: PAIRS, pair_products: ANCHORS, flag_seeds: FLAGS, registers: REG, specs,
    alias_words: [[0x4040, 0xfffffffe], [0x4046, 0x89abcdef], [0x405c, 0xffffffff], [0x4060, 0x80000001]],
    flag_policy: 'CF/OF numeric-range overflow; SF/ZF/AF/PF0; fixed2/DF preserved', program_sha256: Object.fromEntries(Object.entries(programs).map(([kind, bytes]) => [kind, hash(bytes)])),
    arena_bytes: SIZE, patterned_page_sha256: hash(pattern), declared_data_page_sha256: hash(data)},
  test_sha256: Object.fromEntries(['engine/tests/cpu_memory_accumulator_multiply_wasm.rs', 'engine/tests/fixtures/p2-memory-accumulator-multiply/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'flat32 memory-source MUL32/one-operand IMUL32 in actual engine-generated replacement/resident modules with direct read32;10 operand rows/16 FLAGS basis, sequential products and real JC/JO, old EAX/EDX base/index addresses, cross-page/final valid Read4; EAX0 still reads and faults, precise fault/prefix preservation, data-only retry at same PC/state/module and real branch without CPU repair; full GPR/FLAGS/arena and complete source/neighbor pages; BigInt numeric-range/literal full-product oracle and explicit undefined-status0 policy; no standalone memory binding, exhaustive operands/128FLAGS, DIV/wider ISA, PE/provider, SDK/browser, performance or game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
