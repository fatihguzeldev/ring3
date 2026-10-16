import {generatedImports, attachDetectionApi} from '../../../../../../../support/wasm/binding.mjs';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createArena, ARENA_SIZE } from '../../../../../../../support/wasm/arena.mjs';
import { readEngine } from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/);
const engine = readEngine(enginePath),
  PC = 0x12341000,
  FP = 4236;
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const paths = ['engine/tests/support/wasm/binding.mjs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/lower.rs', 
  'engine/src/cpu/x86/decode/x87.rs',
  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs',
  'engine/src/cpu/x86/x87/execute.rs',
  'engine/src/process/cpu/x87.rs',
  'engine/src/cpu/dbt/wasm/instructions/x87.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs',
  'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs',
  'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/abi/x86/x87.rs',
  'engine/src/abi/cpu_helper.rs',
  'engine/tests/cpu/x87/native.rs',
  'engine/tests/cpu/x87/wasm.rs',
  'engine/tests/cpu/x87/control/compatibility/native.rs',
  'engine/tests/cpu/x87/control/compatibility/wasm.rs',
  'engine/tests/cpu/x87/control/compatibility/fixtures/p2-x87-compatibility/runtime/run.mjs',
  'engine/tests/support/wasm.rs',
  'engine/tests/support/wasm/arena.mjs',
  'engine/tests/support/wasm/engine.mjs',
];
assert.equal(new Set(paths).size, paths.length);
const pins = () =>
  Object.fromEntries(paths.map((path) => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(),
  modules = [],
  observations = [];
const counts = { contexts: 0, modules: 0, runs: 0, retired: 0, x87Calls: 0 };
function words(values) {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
}
function record(magic, version, fields) {
  const bytes = Buffer.alloc(40);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(40, 8);
  bytes.set(words(fields), 16);
  return bytes;
}
const arena = (c) => Buffer.from(new Uint8Array(c.memory.buffer, c.base, ARENA_SIZE));
function input(c, offset, bytes) {
  const expected = arena(c);
  expected.set(bytes, offset);
  new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'declared input preserves remaining arena');
}
function host(c, name, args) {
  const expected = arena(c);
  assert.equal(c.api[name](...args), 0, name);
  if (name === 'compile_resident') {
    const after = arena(c);
    assert.equal(after.readUInt32LE(140), 1);
    assert.equal(after.readUInt32LE(144), 24);
    expected.set(after.subarray(140, 164), 140);
  }
  assert.deepEqual(arena(c), expected, name + ' full-arena effects');
}
const bare = [0xe0, 0xe1, 0xe4].map((opcode, index) => ({
  opcode,
  pc: PC + index * 8,
  bytes: Buffer.from([0xdb, opcode, 0xeb, 0]),
}));
const waiting = bare.map((row, index) => ({
  ...row,
  pc: PC + 0x40 + index * 8,
  bytes: Buffer.from([0x9b, 0xdb, row.opcode, 0xeb, 0]),
}));
function compile(c, rows, wait) {
  input(c, 140, words(rows.flatMap((row) => [row.pc, row.bytes.length])));
  host(c, c.resident ? 'compile_resident' : 'compile', [rows.length]);
  const receipt = arena(c);
  const pointer = c.resident ? receipt.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  const length = c.resident ? receipt.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length));
  const module = new WebAssembly.Module(bytes);
  const x87 = c.resident ? 'x87_resident_op' : 'x87_op';
  assert.deepEqual(WebAssembly.Module.imports(module), generatedImports({resident:c.resident,helpers:wait?[x87]:[]}));
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: 'run', kind: 'function' }]);
  const ring3 = {
    ...c.api,
    [x87]: (...args) => {
      c.x87Calls++;
      return c.api[x87](...args);
    },
  };
  const run = new WebAssembly.Instance(module, { env: { memory: c.memory }, ring3 }).exports.run;
  assert.equal(run.length, 4);
  modules.push({ resident: c.resident, wait, bytes: length, sha256: hash(bytes) });
  counts.modules++;
  return { run, version: wait ? 6 : 1 };
}
function seed(c, pc, pending, flags = 2, cancel = 0) {
  const frame = createArena();
  [
    0xa1b2c3d4, 0x23456789, 0x3456789a, 0x456789ab, 0xffffffff, 0x56789abc, 0x6789abcd, 0x789abcde,
  ].forEach((value, index) => frame.writeUInt32LE(value, 16 + index * 4));
  frame.writeUInt16LE(flags & ~0xcd7 ? 2 : 1, 4);
  frame.writeUInt32LE(pc, 48);
  frame.writeUInt32LE(flags, 52);
  frame.writeUInt32LE(cancel, 96);
  frame.set(record('R3MH', 1, [0, 0xdecafbad, 0, 0, 0, 0]), 100);
  for (let index = 0; index < 4096; index++) frame[140 + index] = (index * 17 + 23) & 255;
  frame.writeUInt16LE(0x037e, FP + 16);
  frame.writeUInt16LE(pending ? 1 : 0, FP + 18);
  frame.writeUInt16LE(0x55aa, FP + 20);
  frame.writeUInt16LE(0x7ff, FP + 22);
  frame.writeUInt32LE(0xf1234567, FP + 24);
  frame.writeUInt32LE(0xfedcba98, FP + 28);
  frame.writeUInt16LE(0xf135, FP + 32);
  frame.writeUInt16LE(0xe246, FP + 34);
  for (const [offset, length] of [
    [FP + 40, 80],
    [4396, 128],
    [4628, 128],
  ])
    for (let index = 0; index < length; index++)
      frame[offset + index] = (index * 29 + offset) & 255;
  input(c, 0, frame);
}
function execute(
  c,
  unit,
  label,
  { budget = 1, retired = 1, pc, reason = 1, helper = false, mf = false } = {},
) {
  const expected = arena(c),
    calls = c.x87Calls;
  expected.writeUInt32LE(pc, 48);
  expected.set(
    record('R3EX', reason === 16 ? 7 : unit.version, [reason, retired, mf ? 0x8081 : 0, 0, 0, 0]),
    56,
  );
  if (helper)
    expected.set(
      record('R3MH', 7, mf ? [4, 0, 0x8081, 0, 0, 0] : [0, expected.readUInt32LE(52), 0, 0, 0, 0]),
      100,
    );
  if (mf) expected.writeUInt16LE(0x8081, FP + 18);
  assert.equal(unit.run(c.base, c.base + 56, budget, c.base + 96), 0, label);
  assert.deepEqual(
    arena(c),
    expected,
    `${label}: full${ARENA_SIZE} CPU/FLAGS/x87/XMM/YMM/helper/transfer arena`,
  );
  assert.equal(c.x87Calls - calls, Number(helper), label + ' exact WAIT-only helper calls');
  observations.push({
    resident: c.resident,
    label,
    pc,
    budget,
    reason,
    retired,
    helper,
    sha256: hash(expected),
  });
  counts.runs++;
  counts.retired += retired;
}
for (const resident of [false, true]) {
  const instance = new WebAssembly.Instance(engine.module, {}),
    c = { resident, memory: instance.exports.memory, api: {}, x87Calls: 0 };
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
    'x87_op',
    'x87_resident_op',
  ]) {
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
    assert.equal(typeof c.api[name], 'function', name);
  }
  attachDetectionApi(c.api, instance.exports);
  assert.equal(c.api.open(1, 1, 0x786e6f70), 0);
  c.base = c.api.arena_ptr() >>> 0;
  host(c, 'map', [PC, 1, 7]);
  for (const row of [...bare, ...waiting]) {
    input(c, 140, row.bytes);
    host(c, 'upload', [row.pc, row.bytes.length]);
  }
  const integer = compile(c, bare, false);
  for (const row of bare) {
    for (const pending of [false, true]) {
      seed(c, row.pc, pending);
      execute(c, integer, 'bare clean/pending', { pc: row.pc + 2 });
    }
    seed(c, row.pc, true, 0x102);
    execute(c, integer, 'bare TF', { pc: row.pc + 2, reason: 16 });
    seed(c, row.pc, true);
    execute(c, integer, 'bare budget0', { budget: 0, retired: 0, pc: row.pc });
    seed(c, row.pc, true, 2, 1);
    execute(c, integer, 'bare cancel', { reason: 2, retired: 0, pc: row.pc });
  }
  const wait = compile(c, waiting, true);
  for (const row of waiting) {
    seed(c, row.pc, true);
    execute(c, wait, 'WAIT pending MF', {
      budget: 2,
      retired: 0,
      pc: row.pc,
      reason: 12,
      helper: true,
      mf: true,
    });
    seed(c, row.pc, false);
    execute(c, wait, 'WAIT plus integer NOP', {
      budget: 2,
      retired: 2,
      pc: row.pc + 3,
      helper: true,
    });
    seed(c, row.pc, false);
    execute(c, wait, 'WAIT budget1 boundary', { pc: row.pc + 1, helper: true });
    execute(c, wait, 'resume bare FN without WAIT replay', { pc: row.pc + 3 });
    seed(c, row.pc, false, 0x102);
    execute(c, wait, 'WAIT TF before FN', { budget: 2, pc: row.pc + 1, reason: 16, helper: true });
    seed(c, row.pc, true, 0x102);
    execute(c, wait, 'WAIT MF before TF', {
      budget: 2,
      retired: 0,
      pc: row.pc,
      reason: 12,
      helper: true,
      mf: true,
    });
    seed(c, row.pc, true);
    execute(c, wait, 'WAIT budget0', { budget: 0, retired: 0, pc: row.pc });
    seed(c, row.pc, true, 2, 1);
    execute(c, wait, 'WAIT cancel', { retired: 0, pc: row.pc, reason: 2 });
  }
  host(c, 'close', []);
  counts.x87Calls += c.x87Calls;
  counts.contexts++;
}
assert.deepEqual(counts, { contexts: 2, modules: 4, runs: 78, retired: 48, x87Calls: 30 });
assert.deepEqual(pins(), sourcePins, 'source epoch unchanged');
const result = {
  status: 'ok',
  engine: { sha256: engine.sha256 },
  counts,
  modules,
  observations,
  sourcePins,
  claim:
    'three legacy integer NOPs preserve pending and dirty FP state with no helper import; WAIT remains independent, synchronizes MF at its own EIP and retains budget/TF/cancel boundaries; exact two bound profiles and full4756 arenas, no hardware/performance or new x87 arithmetic claim',
};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), { flag: 'wx' });
console.log(JSON.stringify({ status: result.status, ...counts }));
