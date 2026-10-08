import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/);
const engine = readEngine(enginePath);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const SIZE = 4364, FP = 4236, TRANSFER = 140, PC = 0x1000;
const program = Buffer.from('dfe0dbe2dfe0eb00', 'hex');
assert.deepEqual(readFileSync(join(output, 'chain.x86')), program);
const REG = [0xa53c1234, 0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde];
const STATUS = [[0, 0], [0xffff, 0x7f00], [1, 0], [2, 0], [4, 0], [8, 0], [0x10, 0], [0x20, 0], [0x40, 0], [0x80, 0], [0x100, 0x100], [0x200, 0x200], [0x400, 0x400], [0x800, 0x800], [0x1000, 0x1000], [0x2000, 0x2000], [0x4000, 0x4000], [0x8000, 0]];
const rawRows = [], rawSnapshots = [];
const pattern = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const counts = {modules: 0, chains: 0, clears: 0, status_reads: 0, jumps: 0, controls: 0, preflight: 0, capability_modules: 0, capability_clears: 0, raw_pairs: 0};
const modules = [];
function record(magic, size, fields = []) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(1, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + 4 * index)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0]);
function fpu(status, seed = 0) {
  const bytes = record('R3FP', 128);
  bytes.writeUInt16LE(seed & 1 ? 0xffff : 0, 16); bytes.writeUInt16LE(status, 18);
  bytes.writeUInt16LE(0x55aa, 20); bytes.writeUInt16LE(0x7ff, 22);
  bytes.writeUInt32LE(0xf1234567, 24); bytes.writeUInt32LE(0xfedcba98, 28);
  bytes.writeUInt16LE(0xf135, 32); bytes.writeUInt16LE(0xe246, 34);
  bytes.set(Buffer.from(Array.from({length: 80}, (_, index) => (index * 73 + seed * 29 + 17) & 255)), 40); return bytes;
}
function initial() {
  const bytes = Buffer.alloc(SIZE); bytes.set(state(Array(8).fill(0), 0, 2)); bytes.set(exit(1, 0), 56);
  bytes.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100);
  const fp = record('R3FP', 128); fp.writeUInt16LE(0x037f, 16); fp.writeUInt16LE(0xffff, 20); bytes.set(fp, FP); return bytes;
}
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer);}
function observed(ctx) {refresh(ctx); return Buffer.from(ctx.bytes.subarray(ctx.base, ctx.base + SIZE));}
function check(ctx, label) {assert.deepEqual(observed(ctx), ctx.expected, `${ctx.owner}/${ctx.entries}/${label}`);}
function input(ctx, offset, bytes) {refresh(ctx); ctx.bytes.set(bytes, ctx.base + offset); ctx.expected.set(bytes, offset); check(ctx, 'explicit input');}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function fresh(owner, entries) {
  const ctx = {owner, entries, expected: initial(), low: 1, high: 0x9abcde00};
  if (owner === 'standalone') {
    ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; refresh(ctx); ctx.bytes.set(ctx.expected, ctx.base);
  } else {
    const instance = new WebAssembly.Instance(engine.module, {}); ctx.memory = instance.exports.memory;
    const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, upload: 2, write8: 2, compile: 1, compile_entries: 2,
      compile_resident: 1, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
    ctx.api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
      const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
    }));
    assert.equal(ctx.api.open(2, ctx.low, ctx.high), 0); ctx.base = ctx.api.arena_ptr() >>> 0; check(ctx, 'default including exact FPU');
    pure(ctx, () => ctx.api.map(PC, 1, 7), 'code map'); input(ctx, TRANSFER, program);
    pure(ctx, () => ctx.api.upload(PC, program.length), 'code upload');
    input(ctx, TRANSFER, words(entries ? [PC] : [PC, program.length]));
  }
  return ctx;
}
function compile(ctx) {
  let bytes, binding = {};
  if (ctx.owner === 'standalone') bytes = readFileSync(join(output, `standalone-x87-${ctx.entries}.wasm`));
  else if (ctx.owner === 'replacement') {
    pure(ctx, () => ctx.entries ? ctx.api.compile_entries(1, 0) : ctx.api.compile(1), 'replacement compile');
    binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
  } else {
    check(ctx, 'before resident compile'); assert.equal(ctx.entries ? ctx.api.compile_resident_entries(1, 0) : ctx.api.compile_resident(1), 0); refresh(ctx);
    assert.equal(ctx.view.getUint32(ctx.base + TRANSFER, true), 1); assert.equal(ctx.view.getUint32(ctx.base + TRANSFER + 4, true), 24);
    binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
    ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata');
  }
  if (ctx.owner !== 'standalone') {
    refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length);
    bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length));
    writeFileSync(join(output, `${ctx.owner}-${ctx.entries}.wasm`), bytes, {flag: 'wx'});
  }
  const module = new WebAssembly.Module(bytes), guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'},
    ...(ctx.owner === 'standalone' ? [] : [{module: 'ring3', name: guard, kind: 'function'}])]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.owner === 'standalone' ? {} : {ring3: ctx.api})});
  assert.equal(instance.exports.run.length, 4); counts.modules++;
  modules.push({owner: ctx.owner, entries: ctx.entries, bytes: bytes.length, sha256: hash(bytes)});
  input(ctx, TRANSFER, pattern); return {...binding, run: instance.exports.run};
}
function run(ctx, unit, budget, registers, pc, flags, reason = 1, retired = 1, nextFpu) {
  check(ctx, 'before run'); ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);
  if (nextFpu) ctx.expected.set(nextFpu, FP);
  assert.equal(unit.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0); check(ctx, 'after run');
}
function chain(ctx, unit, status, cleared, seed, flags) {
  const fp = fpu(status, seed);
  input(ctx, 0, state(REG, PC, flags)); input(ctx, 56, exit(3, 0)); input(ctx, 96, words([0])); input(ctx, FP, fp);
  const registers = [...REG]; registers[0] = ((REG[0] & 0xffff0000) | status) >>> 0;
  run(ctx, unit, 1, registers, PC + 2, flags); counts.status_reads++;
  const before = observed(ctx), next = Buffer.from(fp); next.writeUInt16LE(cleared, 18);
  run(ctx, unit, 1, registers, PC + 4, flags, 1, 1, next); counts.clears++;
  const after = observed(ctx);
  rawRows.push({owner: ctx.owner, entries: ctx.entries, seed, status, flags}); rawSnapshots.push(before, after);
  registers[0] = ((REG[0] & 0xffff0000) | cleared) >>> 0;
  run(ctx, unit, 1, registers, PC + 6, flags); counts.status_reads++;
  run(ctx, unit, 1, registers, PC + 8, flags); counts.jumps++; counts.chains++;
}
function controls(ctx, unit) {
  input(ctx, 0, state(REG, PC + 2, 0xcd7)); input(ctx, FP, fpu(0xffff));
  for (const [cancel, budget, reason] of [[0, 0, 1], [1, 1, 2], [1, 0, 2]]) {
    input(ctx, 96, words([cancel])); run(ctx, unit, budget, REG, PC + 2, 0xcd7, reason, 0); counts.controls++;
  }
  input(ctx, 96, words([0])); input(ctx, 0, state(REG, PC + 8, 0xcd7));
  run(ctx, unit, 1, REG, PC + 8, 0xcd7, 3, 0); counts.controls++;
  if (ctx.owner === 'standalone') return;
  const direct = (wrongKey, wrongIdentity) => ctx.owner === 'replacement'
    ? ctx.api.guard(ctx.low ^ wrongKey, ctx.high, unit.generation + wrongIdentity, ctx.base, ctx.base + 56, ctx.base + 96)
    : ctx.api.guard_resident(ctx.low ^ wrongKey, ctx.high, unit.low ^ wrongIdentity, unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
  pure(ctx, () => direct(1, 0), 'wrong key', 3); pure(ctx, () => direct(0, 1), 'wrong identity', 3); counts.controls += 2;
  check(ctx, 'before same-byte invalidation'); assert.equal(ctx.api.write8(PC, program[0]), 0);
  const helper = record('R3MH', 40, [0, 0, 0, 0, 0, 1]); helper.writeUInt16LE(3, 4); ctx.expected.set(helper, 100); check(ctx, 'helper-only invalidation');
  pure(ctx, () => unit.run(-1, -1, 0, -1), 'stale before pointer validation', 4); counts.controls++;
  pure(ctx, () => ctx.api.close(), 'close'); pure(ctx, () => unit.run(-1, -1, 0, -1), 'closed before pointer validation', 5); counts.controls++;
}
function preflight(entries) {
  const memory = new WebAssembly.Memory({initial: 1}), bytes = new Uint8Array(memory.buffer);
  const unit = new WebAssembly.Instance(new WebAssembly.Module(readFileSync(join(output, `standalone-clear-only-${entries}.wasm`))), {env: {memory}}).exports.run;
  counts.capability_modules++;
  const base = 128;
  const neutral = (status, pointers = [base, base + 56, base + 96]) => {
    const before = Buffer.from(bytes); assert.equal(unit(pointers[0], pointers[1], 1, pointers[2]), status);
    assert.deepEqual(Buffer.from(bytes), before, 'preflight failure must preserve whole memory'); counts.preflight++;
  };
  for (const offset of [0, 4, 6, 8, 12, 36, 120, 127]) {
    bytes.fill(0); bytes.set(initial(), base); bytes.set(state(REG, PC, 2), base); bytes[base + FP + offset] ^= 1; neutral(2);
  }
  bytes.fill(0); bytes.set(initial(), base); bytes.set(state(REG, PC, 2), base); new DataView(memory.buffer).setUint16(base + FP + 22, 0x800, true); neutral(2);
  bytes.fill(0); bytes.set(initial(), base); bytes.set(state(REG, PC, 2), base);
  for (const pointers of [[base, base + FP, base + 96], [base, base + 56, base + FP + 40], [-1, base + 56, base + 96]]) neutral(1, pointers);
  const nearEnd = 65536 - 56; bytes.set(state(REG, PC, 2), nearEnd); neutral(1, [nearEnd, 64, 104]);
  bytes.fill(0); bytes.set(initial(), base); bytes.set(state(REG, PC, 2), base); bytes.set(fpu(0xffff), base + FP);
  const expected = Buffer.from(bytes); expected.writeUInt16LE(0x7f00, base + FP + 18);
  expected.writeUInt32LE(PC + 2, base + 48); expected.set(exit(1, 1), base + 56);
  assert.equal(unit(base, base + 56, 1, base + 96), 0);
  assert.deepEqual(Buffer.from(bytes), expected, 'clear-only module discovers the exact x87 preflight capability');
  counts.capability_clears++;
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const entries of [false, true]) {
  const ctx = fresh(owner, entries), unit = compile(ctx);
  for (const [seed, [status, cleared]] of STATUS.entries()) for (const flags of [2, 0xcd7]) chain(ctx, unit, status, cleared, seed, flags);
  controls(ctx, unit);
}
for (const entries of [false, true]) preflight(entries);
assert.deepEqual(counts, {modules: 6, chains: 216, clears: 216, status_reads: 432, jumps: 216, controls: 40, preflight: 26, capability_modules: 2, capability_clears: 2, raw_pairs: 0});
assert.equal(rawRows.length, 216);
assert.equal(new Set(rawRows.map(row => `${row.owner}/${row.entries}/${row.seed}/${row.flags}`)).size, 216);
const rawPath = join(output, 'fnclex-raw.bin'); writeFileSync(rawPath, Buffer.concat(rawSnapshots), {flag: 'wx'});
const physical = readFileSync(rawPath); assert.equal(physical.length, 216 * 2 * SIZE);
for (let index = 0; index < rawRows.length; index++) {
  const before = physical.subarray(index * 2 * SIZE, (index * 2 + 1) * SIZE), after = physical.subarray((index * 2 + 1) * SIZE, (index * 2 + 2) * SIZE);
  assert.equal(before.readUInt32LE(48), PC + 2); assert.deepEqual(program.subarray(2, 4), Buffer.from([0xdb, 0xe2]));
  const oldStatus = before.readUInt16LE(FP + 18);
  assert.equal(oldStatus, rawRows[index].status); assert.equal(before.readUInt32LE(52), rawRows[index].flags);
  let retained = 0; for (const bit of [0x100, 0x200, 0x400, 0x800, 0x1000, 0x2000, 0x4000]) if ((oldStatus & bit) !== 0) retained += bit;
  const independent = Buffer.from(before); independent.writeUInt16LE(retained, FP + 18); independent.writeUInt32LE(PC + 4, 48); independent.set(exit(1, 1), 56);
  assert.deepEqual(after, independent, `independent physical bit-basis oracle row${index}`); counts.raw_pairs++;
}
const result = {status: 'ok', engine_sha256: engine.sha256, counts, modules, raw: {file: 'fnclex-raw.bin', bytes: physical.length, sha256: hash(physical), rows: rawRows},
  source_sha256: Object.fromEntries(['engine/tests/cpu_x87_clear_wasm.rs', 'engine/tests/fixtures/p2-x87-clear/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'exact no-wait FNCLEX status cleanup and retained FNSTSW AX projection in all six compiler profiles; literal16-bit basis with explicit undefinedC preservation, full4364arena/raw128FPU and saved physical raw independent bit-basis oracle; no waited forms, stack, arithmetic, helper/ABI expansion or performance claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: engine.sha256, counts, output}));
