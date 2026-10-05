import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, DATA = 0x4000, ALIAS = 0x1100, FAULT = 0x1200;
const REG = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const MASKS = [0xffff0000, 0x0000ffff, 0xaaaaaaaa, 0x55555555, 0xf0f0f0f0, 0x0f0f0f0f, 0xfafafafa, 0x05050505,
  0xff00ff00, 0x00ff00ff, 0xcccccccc, 0x33333333, 0x00ffff00, 0xff0000ff, 0xf0fffff0, 0x0f00000f];
const FLAGS = Array.from({length: 128}, (_, combination) => 2 + [1, 4, 0x10, 0x40, 0x80, 0x400, 0x800]
  .reduce((value, bit, column) => value + ((combination >> column) & 1) * bit, 0));
function truth(cc, flags) {
  const column = [1, 4, 0x40, 0x80, 0x800].reduce((value, bit, index) => value + ((flags & bit) !== 0 ? 2 ** index : 0), 0);
  return Math.floor(MASKS[cc] / 2 ** column) % 2;
}
assert.equal(new Set(FLAGS).size, 128); assert.ok(FLAGS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
assert.deepEqual(MASKS.map((_, cc) => truth(cc, 2)), [0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1]);
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
const pattern = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1));
const code = Buffer.alloc(16 * 8 + 4 + 0x200, 0xcc);
for (let cc = 0; cc < 16; cc++) {
  code.set(Buffer.concat([Buffer.from([0x0f, 0x40 + cc, (cc % 8) << 3 | 5]), words([DATA + cc * 4])]), cc * 7);
  code.set(Buffer.concat([Buffer.from([0xbf]), words([0xaabbcc00 + cc]), Buffer.from([0x0f, 0x40 + cc, 0x03])]), 0x200 + cc * 8);
}
code.set(Buffer.from('eb000f0b', 'hex'), 112);
code.set(Buffer.from('0f42000f424c8b200f422424eb000f0b', 'hex'), 0x100);
code.set(Buffer.from('eb000f0b', 'hex'), 0x280);
writeFileSync(join(output, 'program.x86'), code, {flag: 'wx'});
const specs = [[PC, 114], [ALIAS, 14], [FAULT, 130]];
const counts = {contexts: 0, modules: 0, seeds: 0, canonical_cmov: 0, alias_cmov: 0, repaired_cmov: 0, jumps: 0,
  fault_calls: 0, prefix_retired: 0, repairs: 0, preflight: 0, owner_controls: 0, generated_calls: 0, pages: 0, arena_checks: 0};
const witnesses = Array.from({length: 16}, () => [0, 0]), modules = [], faults = [];
function record(magic, size, fields, version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired, detail = 0, address = 0) => record('R3EX', 40, [reason, retired, detail, address, reason === 5 ? 1 : 0, reason === 5 ? 4 : 0], 2);
const helper = (value = 0, detail = 0, address = 0) => record('R3MH', 40, [detail ? 1 : 0, detail ? 0 : value, detail, address, detail ? 1 : 0, detail ? 4 : 0]);
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'host request');}
function upload(ctx, address, bytes) {request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), 'data/code upload');}
function map(ctx, address) {pure(ctx, () => ctx.api.map(address, 1, 3), 'data mapping'); upload(ctx, address, pattern);}
function protect(ctx, address, permissions) {pure(ctx, () => ctx.api.protect(address, 1, permissions), 'data permission input');}
function fresh(owner) {
  const instance = new WebAssembly.Instance(engineModule, {}), ordinal = ++counts.contexts;
  const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, unmap: 2, protect: 3, upload: 2, read32: 1, write8: 2,
    compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
  const api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
    const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
  }));
  const ctx = {owner, api, memory: instance.exports.memory, low: ordinal, high: 0xc8393000, expected: Buffer.alloc(SIZE)};
  assert.equal(api.open(6, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(record('R3EX', 40, [1, 0, 0, 0, 0, 0]), 56); ctx.expected.set(helper(), 100);
  check(ctx, 'independently initialized arena');
  request(ctx, pattern); pure(ctx, () => api.map(PC, 1, 7), 'code map'); upload(ctx, PC, code);
  map(ctx, DATA); protect(ctx, DATA, 1); return ctx;
}
function compile(ctx) {
  let binding;
  if (ctx.owner === 'replacement') {
    request(ctx, words(specs.map(([entry]) => entry))); pure(ctx, () => ctx.api.compile_entries(specs.length, 0), 'entry compiler');
    binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);
  } else {
    request(ctx, words(specs.flat())); check(ctx, 'before extent compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
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
  writeFileSync(join(output, `${ctx.owner}.wasm`), bytes, {flag: 'wx'}); counts.modules++;
  modules.push({owner: ctx.owner, sha256: hash(bytes), length: bytes.length, ...binding}); return {...binding, run: child.exports.run};
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
function page(ctx, address) {
  const actual = Buffer.alloc(4096);
  for (let offset = 0; offset < 4096; offset += 4) {
    check(ctx, 'before diagnostic read'); assert.equal(ctx.api.read32(address + offset), 0); refresh(ctx);
    actual.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset);
    ctx.expected.set(helper(pattern.readUInt32LE(offset)), 100); check(ctx, 'diagnostic read changes helper only');
  }
  assert.deepEqual(actual, pattern, 'complete source and neighbors unchanged'); counts.pages++;
}
function preflight(ctx, unit, registers, pc, flags) {
  run(ctx, unit, 0, 'zero budget before unconditional read', registers, pc, flags, 1, 0); counts.preflight++;
  cancel(ctx, 1); run(ctx, unit, 1, 'cancel before unconditional read', registers, pc, flags, 2, 0); counts.preflight++;
  cancel(ctx, 0);
}
function canonical(ctx, unit) {
  for (const [index, flags] of FLAGS.entries()) {
    const registers = [...REG]; seed(ctx, registers, PC, flags);
    if (index === 0) preflight(ctx, unit, registers, PC, flags);
    for (let cc = 0; cc < 16; cc++) {
      const destination = cc % 8, value = pattern.readUInt32LE(cc * 4), selected = truth(cc, flags);
      assert.notEqual(registers[destination], value, 'distinct selected/unchanged witness'); witnesses[cc][selected]++;
      if (selected) registers[destination] = value;
      run(ctx, unit, 1, `cc-${cc}-flags-${flags}`, registers, PC + (cc + 1) * 7, flags, 1, 1, value); counts.canonical_cmov++;
    }
    run(ctx, unit, 1, 'canonical following jump', registers, PC + 114, flags); counts.jumps++;
  }
}
function aliases(ctx, unit) {
  for (const flags of [2, 3, 0xcd6, 0xcd7]) {
    const registers = [...REG]; registers[0] = DATA; registers[1] = 1; registers[3] = DATA; registers[4] = DATA + 8;
    seed(ctx, registers, ALIAS, flags);
    for (const [destination, dataOffset, pcOffset] of [[0, 0, 3], [1, 0x24, 8], [4, 8, 12]]) {
      const value = pattern.readUInt32LE(dataOffset); assert.notEqual(registers[destination], value);
      if (truth(2, flags)) registers[destination] = value;
      run(ctx, unit, 1, 'old base/index/ESP address', registers, ALIAS + pcOffset, flags, 1, 1, value); counts.alias_cmov++;
    }
    run(ctx, unit, 1, 'alias following jump', registers, ALIAS + 14, flags); counts.jumps++;
  }
}
const faultShapes = [
  {name: 'unmapped', address: 0x8000, detail: 1, fault: 0x8000, mapRepair: 0x8000},
  {name: 'write-only', address: DATA, detail: 2, fault: DATA, permissionRepair: DATA},
  {name: 'cross-page-unmapped', address: 0x4ffe, detail: 1, fault: 0x5000, mapRepair: 0x5000},
  {name: 'cross-page-write-only', address: 0x4ffe, detail: 2, fault: 0x5000, permissionRepair: 0x5000},
  {name: 'overflow', address: 0xfffffffd, detail: 3, fault: 0xfffffffd},
];
function faultMatrix(ctx, unit) {
  for (const shape of faultShapes) {
    if (shape.name === 'cross-page-write-only') map(ctx, 0x5000);
    if (shape.name === 'overflow') {map(ctx, 0xfffff000); protect(ctx, 0xfffff000, 1);}
    for (let cc = 0; cc < 16; cc++) {
      if (shape.permissionRepair !== undefined) protect(ctx, shape.permissionRepair, 2);
      const flags = FLAGS.find(value => !truth(cc, value)), registers = [...REG], entry = FAULT + cc * 8;
      registers[3] = shape.address; seed(ctx, registers, entry, flags); registers[7] = 0xaabbcc00 + cc;
      run(ctx, unit, 20, `${shape.name}/false-cc-${cc}/prefix-then-fault`, registers, entry + 5, flags, 5, 1, undefined, shape.detail, shape.fault);
      counts.fault_calls++; counts.prefix_retired++;
      run(ctx, unit, 1, 'unrepaired retry preserves fault PC and prefix', registers, entry + 5, flags, 5, 0, undefined, shape.detail, shape.fault); counts.fault_calls++;
      preflight(ctx, unit, registers, entry + 5, flags);
      if (shape.mapRepair !== undefined || shape.permissionRepair !== undefined) {
        if (shape.mapRepair !== undefined) {map(ctx, shape.mapRepair); protect(ctx, shape.mapRepair, 1);}
        else protect(ctx, shape.permissionRepair, 1);
        const value = shape.address === 0x4ffe ? Buffer.from([pattern[4094], pattern[4095], pattern[0], pattern[1]]).readUInt32LE() : pattern.readUInt32LE(0);
        run(ctx, unit, 1, 'data-only repair retries same false CMOV', registers, entry + 8, flags, 1, 1, value); counts.repaired_cmov++; counts.repairs++;
        if (shape.mapRepair !== undefined) pure(ctx, () => ctx.api.unmap(shape.mapRepair, 1), 'reset only data mapping for next initial case');
      }
      faults.push({owner: ctx.owner, shape: shape.name, condition: cc, flags, request: shape.address, fault: shape.fault, detail: shape.detail, repaired: shape.name !== 'overflow'});
    }
    if (shape.name === 'cross-page-write-only') pure(ctx, () => ctx.api.unmap(0x5000, 1), 'finish second data page');
  }
}
function guards(ctx, unit) {
  for (const wrong of ['key', 'identity']) {
    const low = wrong === 'key' ? ctx.low ^ 1 : ctx.low;
    const action = ctx.owner === 'replacement'
      ? () => ctx.api.guard(low, ctx.high, unit.generation + (wrong === 'identity' ? 1 : 0), ctx.base, ctx.base + 56, ctx.base + 96)
      : () => ctx.api.guard_resident(low, ctx.high, unit.low ^ (wrong === 'identity' ? 1 : 0), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
    pure(ctx, action, `wrong-${wrong}`, 3); counts.owner_controls++;
  }
  check(ctx, 'before same-byte invalidation'); assert.equal(ctx.api.write8(PC, code[0]), 0);
  ctx.expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 1], 3), 100); check(ctx, 'same-byte write helper only');
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'stale before pointer validation', 4); counts.owner_controls++; counts.generated_calls++;
  pure(ctx, () => ctx.api.close(), 'close retains arena');
  pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 'closed before pointer validation', 5); counts.owner_controls++; counts.generated_calls++;
}
for (const owner of ['replacement', 'resident']) {
  const ctx = fresh(owner), unit = compile(ctx); page(ctx, DATA); canonical(ctx, unit); page(ctx, DATA);
  aliases(ctx, unit); page(ctx, DATA); faultMatrix(ctx, unit); page(ctx, DATA); page(ctx, 0xfffff000); guards(ctx, unit);
}
for (let cc = 0; cc < 16; cc++) assert.ok(witnesses[cc][0] > 0 && witnesses[cc][1] > 0, 'every condition has distinct false/true witnesses');
assert.equal(counts.contexts, 2); assert.equal(counts.modules, 2); assert.equal(counts.seeds, 424);
assert.equal(counts.canonical_cmov, 4096); assert.equal(counts.alias_cmov, 24); assert.equal(counts.repaired_cmov, 128);
assert.equal(counts.jumps, 264); assert.equal(counts.fault_calls, 320); assert.equal(counts.prefix_retired, 160);
assert.equal(counts.repairs, 128); assert.equal(counts.preflight, 324); assert.equal(counts.owner_controls, 8);
assert.equal(counts.generated_calls, 5160); assert.equal(counts.pages, 10);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, modules, faults,
  oracle: {flag_seeds: FLAGS, truth_masks: MASKS, bit_order: [1, 4, 0x40, 0x80, 0x800], witnesses, registers: REG,
    program_sha256: hash(code), specs, arena_bytes: SIZE, source_page_sha256: hash(pattern)},
  test_sha256: Object.fromEntries(['engine/tests/cpu_memory_conditional_move_wasm.rs', 'engine/tests/fixtures/p2-memory-conditional-move/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'flat32 memory-source CMOVcc in actual engine-generated replacement/resident modules with direct actual read32; all16 conditions/all128 FLAGS seeds, distinct outcomes, old base/index/ESP aliases, full GPR/FLAGS/arena and complete patterned source/neighbors; every condition faults while false on unmapped/write-only/cross-page/overflow Read4, preserves committed prefix, retries with data-only repair and no CPU edit/recompile; focused preflight and stale/closed guards; no standalone memory binding, PE/provider, SDK/browser, performance, wider ISA or game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
