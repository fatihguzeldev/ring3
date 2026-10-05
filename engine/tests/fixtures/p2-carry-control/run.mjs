import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000;
const REG = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const programs = {
  controls: Buffer.from('f8f9f9f5f5eb000f0b', 'hex'),
  live: Buffer.from('f91500000000f51d00000000f81500000000f91d00000000f5f51500000000eb000f0b', 'hex'),
};
const FLAGS = Array.from({length: 128}, (_, combination) => 2 + [1, 4, 0x10, 0x40, 0x80, 0x400, 0x800]
  .reduce((value, bit, column) => value + ((combination >> column) & 1) * bit, 0));
assert.equal(new Set(FLAGS).size, 128);
assert.ok(FLAGS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
for (const [name, bytes] of Object.entries(programs)) assert.deepEqual(readFileSync(join(output, `${name}.x86`)), bytes);
const counts = {contexts: 0, modules: 0, seed_groups: 0, carry_controls: 0, arithmetic: 0, jumps: 0, generated_runs: 0, guard_controls: 0, pages: 0, arena_checks: 0};
const modules = [];
function record(magic, size, fields) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(1, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0]);
const helper = value => record('R3MH', 40, [0, value, 0, 0, 0, 0]);
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
const pattern = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1));
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.kind}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'host request');}
function fresh(owner, kind) {
  const expected = Buffer.alloc(SIZE), ordinal = ++counts.contexts;
  expected.set(state(Array(8).fill(0), 0, 2)); expected.set(exit(1, 0), 56); expected.set(helper(0), 100);
  const ctx = {owner, kind, expected, low: ordinal, high: 0xc8392000};
  if (owner === 'standalone') {
    ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; ctx.dataAddress = 0x8000;
    refresh(ctx).bytes.set(expected, ctx.base); ctx.bytes.set(pattern, ctx.dataAddress);
  } else {
    const instance = new WebAssembly.Instance(engineModule, {});
    const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, upload: 2, read32: 1, write8: 2, compile_entries: 2,
      compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
    ctx.api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
      const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
    }));
    ctx.memory = instance.exports.memory; assert.equal(ctx.api.open(3, ctx.low, ctx.high), 0);
    ctx.base = ctx.api.arena_ptr() >>> 0; ctx.dataAddress = 0x3000; check(ctx, 'independently initialized arena');
    pure(ctx, () => ctx.api.map(ctx.dataAddress, 1, 3), 'data map'); request(ctx, pattern);
    pure(ctx, () => ctx.api.upload(ctx.dataAddress, pattern.length), 'pattern upload');
  }
  ctx.expected.set(pattern, TRANSFER); refresh(ctx).bytes.set(pattern, ctx.base + TRANSFER); check(ctx, 'patterned transfer');
  return ctx;
}
function compile(ctx) {
  let bytes, binding = {};
  if (ctx.owner === 'standalone') bytes = readFileSync(join(output, `standalone-${ctx.kind}.wasm`));
  else {
    pure(ctx, () => ctx.api.map(PC, 1, 7), 'code map'); request(ctx, programs[ctx.kind]);
    pure(ctx, () => ctx.api.upload(PC, programs[ctx.kind].length), 'code upload');
    request(ctx, words(ctx.owner === 'replacement' ? [PC] : [PC, programs[ctx.kind].length - 2]));
    if (ctx.owner === 'replacement') {
      pure(ctx, () => ctx.api.compile_entries(1, 0), 'entry compiler');
      binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
      assert.ok(binding.generation > 0);
    } else {
      check(ctx, 'before extent compiler'); assert.equal(ctx.api.compile_resident(1), 0); refresh(ctx);
      assert.equal(ctx.view.getUint32(ctx.base + TRANSFER, true), 1); assert.equal(ctx.view.getUint32(ctx.base + TRANSFER + 4, true), 24);
      binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
      assert.ok(binding.low !== 0 || binding.high !== 0);
      ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only');
    }
    refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length);
    bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length));
    writeFileSync(join(output, `${ctx.owner}-${ctx.kind}.wasm`), bytes, {flag: 'wx'});
  }
  const module = new WebAssembly.Module(bytes), guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'},
    ...(ctx.owner === 'standalone' ? [] : [{module: 'ring3', name: guard, kind: 'function'}])]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.owner === 'standalone' ? {} : {ring3: ctx.api})});
  assert.equal(child.exports.run.length, 4); counts.modules++;
  modules.push({owner: ctx.owner, kind: ctx.kind, sha256: hash(bytes), length: bytes.length, ...binding});
  return {...binding, run: child.exports.run};
}
function seed(ctx, registers, flags) {
  ctx.expected.set(state(registers, PC, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 100), ctx.base); check(ctx, 'initial cpu'); counts.seed_groups++;
}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, label, registers, pc, flags, reason = 1, retired = 1, status = 0, malformed = false) {
  check(ctx, `before ${label}`);
  if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);}
  const pointers = malformed ? [0xffffffff, 0xffffffff, 0xffffffff] : [ctx.base, ctx.base + 56, ctx.base + 96];
  assert.equal(unit.run(pointers[0], pointers[1], budget, pointers[2]), status, label); check(ctx, label); counts.generated_runs++;
}
function page(ctx) {
  const actual = Buffer.alloc(4096);
  if (ctx.owner === 'standalone') actual.set(refresh(ctx).bytes.subarray(ctx.dataAddress, ctx.dataAddress + 4096));
  else for (let offset = 0; offset < 4096; offset += 4) {
    check(ctx, 'before diagnostic read'); assert.equal(ctx.api.read32(ctx.dataAddress + offset), 0); refresh(ctx);
    actual.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset);
    ctx.expected.set(helper(pattern.readUInt32LE(offset)), 100); check(ctx, 'diagnostic read changes helper only');
  }
  assert.deepEqual(actual, pattern, 'complete patterned page unchanged'); counts.pages++;
}
function controls(ctx, unit) {
  for (const flags of FLAGS) {
    seed(ctx, REG, flags);
    for (const [index, cf] of [0, 1, 1, 0, 1].entries()) {
      run(ctx, unit, 1, `flags-${flags}-control-${index}`, REG, PC + index + 1, (flags & ~1) | cf); counts.carry_controls++;
    }
    run(ctx, unit, 1, `flags-${flags}-following-jump`, REG, PC + 7, (flags & ~1) | 1); counts.jumps++;
  }
}
function live(ctx, unit) {
  const registers = [...REG]; registers[0] = 0; seed(ctx, registers, 0xcd6);
  const steps = [
    [1, 0, 0xcd7, 'stc'], [6, 1, 0x402, 'adc'], [7, 1, 0x403, 'cmc'], [12, 0, 0x446, 'sbb'],
    [13, 0, 0x446, 'clc'], [18, 0, 0x446, 'adc'], [19, 0, 0x447, 'stc'], [24, 0xffffffff, 0x497, 'sbb'],
    [25, 0xffffffff, 0x496, 'cmc'], [26, 0xffffffff, 0x497, 'cmc'], [31, 0, 0x457, 'adc'], [33, 0, 0x457, 'jump'],
  ];
  for (const [offset, eax, flags, name] of steps) {
    registers[0] = eax; run(ctx, unit, 1, `live-${offset}-${name}`, registers, PC + offset, flags);
    if (name === 'jump') counts.jumps++; else if (name === 'adc' || name === 'sbb') counts.arithmetic++; else counts.carry_controls++;
  }
}
function finish(ctx, unit) {
  const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4));
  const pc = ctx.expected.readUInt32LE(48), flags = ctx.expected.readUInt32LE(52);
  for (const [name, cancelled, budget, reason] of [['zero-budget', 0, 0, 1], ['cancel-positive-budget', 1, 1, 2], ['cancel-zero-budget', 1, 0, 2]]) {
    cancel(ctx, cancelled); run(ctx, unit, budget, name, registers, pc, flags, reason, 0); counts.guard_controls++;
  }
  cancel(ctx, 0); run(ctx, unit, 1, 'cold-entry', registers, pc, flags, 3, 0); counts.guard_controls++;
  run(ctx, unit, 0, 'malformed-pointers', registers, pc, flags, 0, 0, 1, true); counts.guard_controls++;
  page(ctx);
  if (ctx.owner === 'standalone') return;
  for (const wrong of ['key', 'identity']) {
    const low = wrong === 'key' ? ctx.low ^ 1 : ctx.low;
    const action = ctx.owner === 'replacement'
      ? () => ctx.api.guard(low, ctx.high, unit.generation + (wrong === 'identity' ? 1 : 0), ctx.base, ctx.base + 56, ctx.base + 96)
      : () => ctx.api.guard_resident(low, ctx.high, unit.low ^ (wrong === 'identity' ? 1 : 0), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
    pure(ctx, action, `wrong-${wrong}`, 3); counts.guard_controls++;
  }
  check(ctx, 'before same-byte invalidation'); assert.equal(ctx.api.write8(PC, programs[ctx.kind][0]), 0);
  const updatedHelper = record('R3MH', 40, [0, 0, 0, 0, 0, 1]); updatedHelper.writeUInt16LE(3, 4);
  ctx.expected.set(updatedHelper, 100); check(ctx, 'same-byte invalidation changes helper only');
  run(ctx, unit, 0, 'stale-before-pointer-validation', registers, pc, flags, 0, 0, 4, true); counts.guard_controls++;
  pure(ctx, () => ctx.api.close(), 'close');
  run(ctx, unit, 0, 'closed-before-pointer-validation', registers, pc, flags, 0, 0, 5, true); counts.guard_controls++;
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const kind of ['controls', 'live']) {
  const ctx = fresh(owner, kind), unit = compile(ctx); page(ctx);
  if (kind === 'controls') controls(ctx, unit); else live(ctx, unit);
  finish(ctx, unit);
}
assert.equal(counts.contexts, 6); assert.equal(counts.modules, 6); assert.equal(counts.seed_groups, 387);
assert.equal(counts.carry_controls, 1938); assert.equal(counts.arithmetic, 15); assert.equal(counts.jumps, 387);
assert.equal(counts.generated_runs, 2378); assert.equal(counts.guard_controls, 46); assert.equal(counts.pages, 12);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, modules,
  oracle: {flag_seeds: FLAGS, registers: REG, programs: Object.fromEntries(Object.entries(programs).map(([name, bytes]) => [name, bytes.toString('hex')])), arena_bytes: SIZE, page_sha256: hash(pattern)},
  test_sha256: Object.fromEntries(['engine/tests/cpu_carry_control_wasm.rs', 'engine/tests/fixtures/p2-carry-control/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'flat32 CLC/STC/CMC in native-generated standalone and actual-engine-Wasm-generated replacement/resident modules; all128 valid FLAGS seeds, eight preserved GPRs, full non-CF FLAGS and complete arena; sequential budget1 carry controls feed literal ADC/SBB without CPU repair, complete untouched patterned4KB page, focused preflight and stale/closed owner guards; no wider ISA, PE/provider, SDK/browser, performance or game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
