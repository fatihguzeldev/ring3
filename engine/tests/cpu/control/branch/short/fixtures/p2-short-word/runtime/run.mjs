import {detectionGuardName, generatedImports, attachDetectionApi} from '../../../../../../../support/wasm/binding.mjs';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createArena, ARENA_SIZE } from '../../../../../../../support/wasm/arena.mjs';
import { readEngine } from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/);
const engine = readEngine(enginePath);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/abi/arena.rs',
  'engine/src/abi/x86/state.rs',
  'engine/src/cpu/x86/decode/flow.rs',
  'engine/src/cpu/x86/decode/decoder.rs',
  'engine/src/cpu/x86/decode/profile.rs',
  'engine/src/cpu/x86/ir.rs',
  'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/control.rs',
  'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/process/wasm/execution.rs',
  'engine/tests/cpu/control/wasm.rs',
  'engine/tests/cpu/control/branch/short/wasm/word.rs',
  'engine/tests/cpu/control/branch/short/fixtures/p2-short-word/runtime/run.mjs',
  'engine/tests/support/wasm.rs',
  'engine/tests/support/wasm/arena.mjs',
  'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs',
];
assert.equal(new Set(paths).size, paths.length);
const sourcePins = () =>
  Object.fromEntries(paths.map((path) => [path, hash(readFileSync(join(root, path)))]));
const sources = sourcePins();
const observations = [],
  modules = [];
const counts = { contexts: 0, codes: 25, runs: 0, retired: 0, boundaries: 0, controls: 0 };
const mask32 = 0xffffffffn,
  mask16 = 0xffffn;
const signed8 = (raw) => BigInt(raw < 128 ? raw : raw - 256);
const target16 = (row) =>
  Number((BigInt(row.pc) + BigInt(row.bytes.length) + signed8(row.raw)) & mask16);
const next32 = (row) => Number((BigInt(row.pc) + BigInt(row.bytes.length)) & mask32);
function condition(index, flags) {
  const flag = (bit) => (BigInt(flags) & BigInt(bit)) !== 0n;
  const c = flag(1),
    p = flag(4),
    z = flag(64),
    s = flag(128),
    o = flag(2048);
  return [
    o,
    !o,
    c,
    !c,
    z,
    !z,
    c || z,
    !c && !z,
    s,
    !s,
    p,
    !p,
    s !== o,
    s === o,
    z || s !== o,
    !z && s === o,
  ][index];
}
const names = [
  'Jo',
  'Jno',
  'Jb',
  'Jae',
  'Je',
  'Jne',
  'Jbe',
  'Ja',
  'Js',
  'Jns',
  'Jp',
  'Jnp',
  'Jl',
  'Jge',
  'Jle',
  'Jg',
];
const trueFlags = [0x802, 2, 3, 2, 0x42, 2, 3, 2, 0x82, 2, 6, 2, 0x82, 2, 0x42, 2];
const falseFlags = [2, 0x802, 2, 3, 2, 0x42, 2, 3, 2, 0x82, 2, 6, 2, 0x82, 2, 0x42];
const rows = names.map((name, cc) => ({
  code: `${name}_rel8_16`,
  kind: 'conditional',
  cc,
  pc: 0x12341000 + cc * 8,
  raw: cc % 2 ? 0x80 : 0x7f,
  bytes: [0x66, 0x70 + cc, cc % 2 ? 0x80 : 0x7f],
}));
rows.push({
  code: 'Jmp_rel8_16',
  kind: 'jump',
  pc: 0x12341080,
  raw: 0xff,
  bytes: [0x66, 0xeb, 0xff],
});
for (const word of [false, true]) {
  for (const [kind, opcode, name] of [
    ['notEqual', 0xe0, 'Loopne'],
    ['equal', 0xe1, 'Loope'],
    ['loop', 0xe2, 'Loop'],
  ]) {
    rows.push({
      code: `${name}_rel8_16_${word ? 'CX' : 'ECX'}`,
      kind,
      word,
      pc: 0x12341088 + (rows.length - 17) * 8,
      raw: 0x80,
      bytes: [0x66, ...(word ? [0x67] : []), opcode, 0x80],
    });
  }
  rows.push({
    code: `${word ? 'Jcxz' : 'Jecxz'}_rel8_16`,
    kind: 'zero',
    word,
    pc: 0x12341088 + (rows.length - 17) * 8,
    raw: 0x7f,
    bytes: [0x66, ...(word ? [0x67] : []), 0xe3, 0x7f],
  });
}
assert.equal(rows.length, 25);
assert.equal(new Set(rows.map((row) => row.code)).size, 25);
const boundaries = [
  { ...rows[4], pc: 0x1234fffd, raw: 2, bytes: [0x66, 0x74, 2] },
  { ...rows[16], pc: 0xfffffffd, raw: 0x80, bytes: [0x66, 0xeb, 0x80] },
  {
    ...rows.find((row) => row.code === 'Loop_rel8_16_CX'),
    pc: 0xfffffffc,
    raw: 0x7f,
    bytes: [0x66, 0x67, 0xe2, 0x7f],
  },
];
assert.equal(target16(boundaries[0]), 2);
assert.equal(target16(boundaries[1]), 0xff80);
assert.equal(target16(boundaries[2]), 0x7f);
assert.equal(next32(boundaries[0]), 0x12350000);
assert.equal(next32(boundaries[1]), 0);
function words(values) {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
}
function record(magic, size, fields) {
  const bytes = Buffer.alloc(size);
  bytes.write(magic);
  bytes.writeUInt16LE(1, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8);
  bytes.set(words(fields), 16);
  return bytes;
}
const state = (cpu) => record('R3ST', 56, [...cpu.registers, cpu.pc, cpu.flags]);
const exit = (reason, retired) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0]);
const arena = (ctx) => Buffer.from(new Uint8Array(ctx.memory.buffer, ctx.base, ARENA_SIZE));
function input(ctx, offset, bytes) {
  const expected = arena(ctx);
  expected.set(bytes, offset);
  new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + offset);
  assert.deepEqual(arena(ctx), expected, 'declared full-arena input');
}
function host(ctx, name, args) {
  const expected = arena(ctx);
  assert.equal(ctx.api[name](...args), 0, `${ctx.owner} ${name}`);
  if (name === 'compile_resident') {
    const after = arena(ctx);
    assert.equal(after.readUInt32LE(140), 1);
    assert.equal(after.readUInt32LE(144), 24);
    expected.set(after.subarray(140, 164), 140);
  }
  assert.deepEqual(arena(ctx), expected, `${ctx.owner} ${name} full-arena effects`);
}
function model(row, before) {
  const after = { registers: [...before.registers], pc: next32(row), flags: before.flags };
  let take = row.kind === 'jump';
  if (row.kind === 'conditional') take = condition(row.cc, before.flags);
  if (['notEqual', 'equal', 'loop', 'zero'].includes(row.kind)) {
    const mask = row.word ? mask16 : mask32;
    const count = BigInt(before.registers[1]) & mask;
    const remaining = row.kind === 'zero' ? count : (count - 1n) & mask;
    if (row.kind !== 'zero')
      after.registers[1] = Number((BigInt(before.registers[1]) & (mask32 ^ mask)) | remaining);
    take =
      row.kind === 'zero'
        ? count === 0n
        : remaining !== 0n &&
          (row.kind === 'loop' ||
            (row.kind === 'equal' ? (before.flags & 64) !== 0 : (before.flags & 64) === 0));
  }
  if (take) after.pc = target16(row);
  return after;
}
function run(ctx, unit, row, ecx, flags, budget = 1, cancel = false) {
  const before = {
    registers: [
      0x12345678,
      ecx,
      0x34567890,
      0x456789ab,
      0x56789abc,
      0x6789abcd,
      0x789abcde,
      0x89abcdef,
    ],
    pc: row.pc,
    flags,
  };
  input(ctx, 0, state(before));
  input(ctx, 56, exit(3, 0));
  input(ctx, 96, words([cancel ? 1 : 0]));
  const expected = arena(ctx),
    retired = budget && !cancel ? 1 : 0;
  expected.set(state(retired ? model(row, before) : before));
  expected.set(exit(cancel ? 2 : 1, retired), 56);
  assert.equal(unit.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0, row.code);
  assert.deepEqual(arena(ctx), expected, `${ctx.owner} ${row.code} full ${ARENA_SIZE}-byte arena`);
  observations.push({
    owner: ctx.owner,
    code: row.code,
    pc: row.pc,
    bytes: row.bytes,
    ecx,
    flags,
    budget,
    cancel,
    next: expected.readUInt32LE(48),
    retired,
    sha256: hash(expected),
  });
  counts.runs++;
  counts.retired += retired;
}
function compile(ctx, batch) {
  assert.ok(batch.length <= 8);
  input(ctx, 140, words(batch.flatMap((row) => [row.pc, row.bytes.length])));
  host(ctx, ctx.owner === 'resident' ? 'compile_resident' : 'compile', [batch.length]);
  const receipt = arena(ctx);
  const pointer = ctx.owner === 'resident' ? receipt.readUInt32LE(156) : ctx.api.module_ptr() >>> 0;
  const length = ctx.owner === 'resident' ? receipt.readUInt32LE(160) : ctx.api.module_len() >>> 0;
  assert.ok(length > 0 && length <= 65536);
  const bytes = Buffer.from(new Uint8Array(ctx.memory.buffer, pointer, length));
  const module = new WebAssembly.Module(bytes),
    guard = detectionGuardName(ctx.owner === 'resident');
  assert.deepEqual(WebAssembly.Module.imports(module), generatedImports({bound: true, resident: ctx.owner === 'resident', helpers: []}));
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: 'run', kind: 'function' }]);
  const run = new WebAssembly.Instance(module, { env: { memory: ctx.memory }, ring3: ctx.api })
    .exports.run;
  assert.equal(run.length, 4);
  modules.push({
    owner: ctx.owner,
    codes: batch.map((row) => row.code),
    bytes: bytes.length,
    sha256: hash(bytes),
  });
  return { run };
}
for (const owner of ['replacement', 'resident']) {
  const instance = new WebAssembly.Instance(engine.module, {}),
    ctx = { owner, memory: instance.exports.memory, api: {} };
  for (const name of [
    'open',
    'close',
    'arena_ptr',
    'map',
    'upload',
    'compile',
    'compile_resident',
    'module_ptr',
    'module_len',
    'guard',
    'guard_resident',
  ]) {
    ctx.api[name] = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof ctx.api[name], 'function', name);
  }
 attachDetectionApi(ctx.api, instance.exports);
  assert.equal(ctx.api.open(3, 1, 0x73686f72), 0);
  ctx.base = ctx.api.arena_ptr() >>> 0;
  const initial = createArena();
  for (const [start, length] of [
    [4236 + 40, 80],
    [4364 + 32, 128],
    [4612 + 16, 128],
  ]) {
    for (let index = 0; index < length; index++)
      initial[start + index] = (index * 29 + start) & 255;
  }
  input(ctx, 0, initial);
  for (const page of [0x12341000, 0x1234f000, 0xfffff000]) host(ctx, 'map', [page, 1, 7]);
  for (const row of rows) {
    input(ctx, 140, Buffer.from(row.bytes));
    host(ctx, 'upload', [row.pc, row.bytes.length]);
  }
  for (let start = 0; start < rows.length; start += 8) {
    const batch = rows.slice(start, start + 8),
      unit = compile(ctx, batch);
    for (const row of batch) {
      if (row.kind === 'conditional') {
        assert.equal(condition(row.cc, trueFlags[row.cc]), true);
        assert.equal(condition(row.cc, falseFlags[row.cc]), false);
        for (const flags of [trueFlags[row.cc], falseFlags[row.cc]])
          run(ctx, unit, row, 0xaabb0002, flags);
      } else if (row.kind === 'jump') run(ctx, unit, row, 0xaabb0002, 0xcd7);
      else
        for (const ecx of [0, 1, 2, 0x10000, 0x10001, 0xffffffff]) {
          for (const flags of [2, 0x42]) run(ctx, unit, row, ecx, flags);
        }
    }
    if (start === 0) {
      run(ctx, unit, batch[0], 0xaabb0002, trueFlags[0], 0);
      counts.controls++;
      run(ctx, unit, batch[0], 0xaabb0002, trueFlags[0], 1, true);
      counts.controls++;
    }
  }
  for (const row of boundaries) {
    input(ctx, 140, Buffer.from(row.bytes));
    host(ctx, 'upload', [row.pc, row.bytes.length]);
    const unit = compile(ctx, [row]);
    run(ctx, unit, row, 0xaabb0002, 0x42);
    counts.boundaries++;
    if (row.kind === 'conditional') {
      run(ctx, unit, row, 0xaabb0002, 2);
      counts.boundaries++;
    }
  }
  host(ctx, 'close', []);
  counts.contexts++;
}
assert.deepEqual(sourcePins(), sources, 'source epoch unchanged');
assert.equal(counts.contexts, 2);
assert.equal(counts.controls, 4);
assert.equal(counts.boundaries, 8);
assert.equal(modules.length, 14);
const result = {
  status: 'ok',
  engine: { sha256: engine.sha256, bytes: engine.bytes.length },
  counts,
  source_pins: sources,
  modules,
  observations,
  limits: [
    '25 distinct Iced Codes, not an exhaustive prefix/flags universe',
    'two actual bound modes; standalone, entries capture and native prefix/fetch tests remain separate',
    'full4756 arenas; no guest memory operands, hardware or performance claim',
  ],
};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), { flag: 'wx' });
console.log(JSON.stringify({ status: result.status, ...counts, output }));
