import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, state, exit, record, words, hash} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/exceptions/availability/optional/carryless/';
const paths = [
  own + 'native.rs', own + 'wasm.rs', own + 'fixtures/p2-carryless-availability/runtime/run.mjs',
  'engine/tests/cpu/control/native.rs', 'engine/tests/cpu/control/wasm.rs',
  'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/environment.rs',
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/x86/decode/execution_faults/optional.rs',
  'engine/src/cpu/x86/decode/execution_faults/tests/pclmul.rs',
  'engine/src/cpu/x86/decode/avx/mod.rs', 'engine/src/cpu/x86/decode/avx/encoding.rs',
  'engine/src/cpu/x86/decode/avx/crypto.rs', 'engine/src/cpu/x86/decode/xstate.rs',
  'engine/src/cpu/x86/avx/crypto.rs', 'engine/src/cpu/x86/avx/operation/factory/crypto.rs',
  'engine/src/cpu/x86/avx/dispatch.rs', 'engine/src/cpu/x86/simd/crypto.rs', 'engine/src/math/integer.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/avx/execute.rs',
  'engine/src/process/cpu/avx.rs', 'engine/src/abi/cpu_helper.rs', 'engine/src/abi/x86/avx.rs',
  support,
];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), RF = 0x10000, TF = 0x100, FLAGS = 0x244cd7;
const regs = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0x7000, 0xfffffffc, 0xfffffffb, 0xfffffffa];
const bytes = hex => Buffer.from(hex, 'hex');
let planned = 0, invoked = 0;
function seed(pc, {flags = FLAGS | RF | TF, cancel = 0, shadow = 1} = {}) {
  const input = state(h.initial, {pc, flags, cancel, regs}); input.writeUInt32LE(shadow, 12);
  for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {
    input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
  }
  const sg = record('R3SG', 64, [], 2);
  [0x23, 0x1b, 0x23, 0, 0x3b, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + i * 2));
  [0xf33fffff, 0xf3bfffff, 0xf2300fff, 0, 0xf33fffff, 0]
    .forEach((value, i) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  input.set(sg, SEG); return input;
}
function expected(input, reason, retired, pc = input.readUInt32LE(48), version = reason >= 16 ? 7 : 1) {
  const want = Buffer.from(input); want.writeUInt32LE(pc, 48);
  if (![1, 2].includes(reason) || retired) {
    want.writeUInt32LE(input.readUInt32LE(52) & ~RF, 52); want.writeUInt32LE(0, 12);
  }
  return exit(want, reason, retired, version);
}
function invoke(c, label, material, input, want, options = {}) {
  planned++; if (material) {invoked++; return h.observe(c, label, {material, input, want, ...options});}
  h.skip(c, label, 'required preparation admission'); return null;
}
function open(resident, entries, label) {
  const c = h.open(resident, entries, label, [[PC, 7], [0x5000, 3], [0x7000, 3]]);
  h.upload(c, 0x5000, Buffer.alloc(4096, 0x5a)); h.upload(c, 0x7000, Buffer.alloc(4096, 0xa5)); return c;
}
function strict(c, label, pc, length, status, helpers = []) {
  return h.compile(c, label, [[pc, length]], helpers, {expectedStatus: status});
}
function bootstrap(c, target) {
  const jump = Buffer.alloc(5); jump[0] = 0xe9; jump.writeInt32LE(target - PC - 5, 1);
  h.upload(c, PC, jump); const input = seed(PC, {flags: FLAGS | RF}); h.put(c, 0, input);
  const material = strict(c, 'real bootstrap to absent target', PC, 5, 0);
  invoke(c, 'actual branch retires once and reaches canonical NeedCode', material,
    input, expected(input, 3, 1, target), {budget: 2});
}
function stopped(c, label, status = 0) {
  h.put(c, 140, words([0xdeadbeef, 0xffffffff]));
  return h.compile(c, label, [], [], {stopped: true, expectedStatus: status});
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const hex of ['c4e37d44c100', 'c4e37d440100']) {
    const code = bytes(hex), target = 0x2000 - code.length;
    const c = open(resident, entries, `absent VEX256 VPCLMUL ${hex}`);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, 'public strict refusal remains unchanged', target, code.length, 10);
    const material = stopped(c, 'exact complete VEX256 Code captures absent-feature UD');
    const input = seed(target); invoke(c, 'absent VPCLMUL faults before null DS, unmapped memory or vector mutation', material,
      input, expected(input, 20, 0), {budget: 2}); h.host(c, 'close');
  }
  {
    const code = bytes('c4e37d44c100'), target = 0x1ffb;
    const c = open(resident, entries, 'VPCLMUL final immediate fetch repair');
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
    strict(c, 'strict truncated immediate refuses neutrally', target, code.length, 10);
    const fetchMaterial = h.fetchFault(c, 'missing final immediate prevents feature classification', {pc: target, length: code.length, address: 0x2000, errorCode: 20});
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, Buffer.concat([code.subarray(-1), bytes('ebfe')]));
    h.host(c, 'protect', [0x2000, 1, 1]); h.fetchFault(c, 'readable but non-executable immediate still prevents classification', {pc: target, length: code.length, address: 0x2000, errorCode: 21, material: fetchMaterial});
    h.host(c, 'protect', [0x2000, 1, 7]); h.reachNeedCode(c); const material = stopped(c, 'executable complete immediate repair captures exact absence UD');
    const input = seed(target); invoke(c, 'repaired complete Code reaches only the absent-facility fault', material,
      input, expected(input, 20, 0), {budget: 2}); h.host(c, 'close');
  }
  {
    const c = open(resident, entries, 'existing VEX128 carryless positive'), target = 0x1100;
    h.upload(c, target, bytes('c4e37944c100ebfe')); h.put(c, 0, seed(target));
    const helper = resident ? 'avx_execute_resident' : 'avx_execute';
    const material = strict(c, 'VEX128 PCLMUL+AVX still captures success without VPCLMUL256', target, 8, 0, [helper]);
    const input = seed(target, {flags: FLAGS | RF});
    input.writeBigUInt64LE(3n, 4396); input.writeBigUInt64LE(0x8877665544332211n, 4404);
    input.writeBigUInt64LE(5n, 4412); input.writeBigUInt64LE(0xfedcba9876543210n, 4420);
    const want = expected(input, 1, 1, target + 6, 6); want.fill(0, 4396, 4412); want[4396] = 15;
    want.fill(0, 4628, 4644); want.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0], 7), 100);
    want.fill(0, 652, 684); want.writeUInt32LE(FLAGS, 656);
    const args = [...(resident && material ? material.binding : []), 0x01001006, 0x40, 0, 0, 0, FLAGS];
    invoke(c, 'literal GF2 product3x5 equals0F, ignored high-qword decoys and clear destination upper', material,
      input, want, {budget: 1, helpers: [{name: helper, args, status: 0}]}); h.host(c, 'close');
  }
  for (const row of [
    {hex: 'cd80', label: 'INT80 precise denied application vector', invalid: false},
    {hex: 'f0c4e37d44c100', label: 'illegal LOCK VPCLMUL decoder UD', invalid: true},
  ]) {
    const code = bytes(row.hex), target = 0x1100, c = open(resident, entries, row.label);
    h.upload(c, target, code); bootstrap(c, target);
    if (row.hex === 'cd80') {
      const material = strict(c, 'original INT80 bytes bind the application interrupt instruction', target, code.length, 0);
      const input = seed(target), want = expected(input, 11, 0); want.set(record('R3EX', 40, [11, 0, 0x402, 0, 0, 0], 6), 56);
      invoke(c, 'INT80 retains precise GP(IDT vector80), original PC and all operands', material, input, want, {budget: 2});
      h.host(c, 'close'); continue;
    }

    strict(c, 'strict prefix/neighbor refusal retained', target, code.length, 10);
    const material = stopped(c, row.label, row.invalid ? 0 : 10);
    if (row.invalid) {const input = seed(target); invoke(c, 'decoder-invalid outcome remains existing UD', material,
      input, expected(input, 20, 0), {budget: 2});}
    h.host(c, 'close');
  }
}
assert.equal(planned, 44, 'bounded VPCLMUL module invocations');
assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'carryless-counts.json'), JSON.stringify({planned, invoked, exactCode: 'VEX_Vpclmulqdq_ymm_ymm_ymmm256_imm8',
  contextsPlanned: 24, originalCompileObservationPlan: 72, originalRowPlan: 112, sourcePins}, null, 2) + '\n');
h.finish([
  'exact VEX256 VPCLMUL Code with CPUID7.0.ECX10 absent reaches UD; no enabled256 semantic policy is selected',
  'both conflicting Intel enabled-form interpretations require the same UD in this absent-feature environment',
  'four profiles use real guards/helpers with literal full4796 arena/every mapped RAM page and exact retirement/vector/flags/packet/trace',
  'full executable immediate fetch precedes absence classification; poisoned memory/register operands are untouched',
  'existing VEX128 PCLMUL positive computes literal carryless3x5=0F and remains independent of absent VPCLMUL256',
  'unchanged generic UD schedule/currentness/live-CS/flag proof is reused; no broad VEX classifier, capability activation or CPU conformance claim',
]);
