import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, state, exit, record, words, hash} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/exceptions/availability/extended/';
const paths = [
  own + 'native.rs', own + 'wasm.rs', own + 'fixtures/p2-extended-availability/runtime/run.mjs',
  'engine/tests/cpu/control/native.rs', 'engine/tests/cpu/control/wasm.rs',
  'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/environment.rs',
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/x86/decode/execution_faults/optional.rs',
  'engine/src/cpu/x86/decode/execution_faults/tests/extended.rs',
  'engine/src/cpu/x86/decode/avx/mod.rs', 'engine/src/cpu/x86/decode/avx/encoding.rs',
  'engine/src/cpu/x86/decode/avx/crypto.rs', 'engine/src/cpu/x86/decode/xstate.rs',
  'engine/src/cpu/x86/avx/crypto.rs', 'engine/src/cpu/x86/avx/operation/factory/crypto.rs',
  'engine/src/cpu/x86/avx/dispatch.rs', 'engine/src/cpu/x86/simd/crypto.rs', 'engine/src/math/aes.rs',
  'engine/src/cpu/dbt/wasm/avx/mod.rs', 'engine/src/cpu/dbt/wasm/avx/execute.rs',
  'engine/src/process/cpu/avx.rs', 'engine/src/abi/cpu_helper.rs', 'engine/src/abi/x86/avx.rs',
  support,
];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), RF = 0x10000, TF = 0x100, FLAGS = 0x244cd7;
const regs = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0x7000, 0xfffffffc, 0xfffffffb, 0xfffffffa];
const bytes = hex => Buffer.from(hex, 'hex');
const exact = [
  {code: 'Xrstors_mem', hex: '0fc718'}, {code: 'Xsaves_mem', hex: '0fc728'},
  {code: 'VEX_Vaesenc_ymm_ymm_ymmm256', hex: 'c4e27ddcc1'},
  {code: 'VEX_Vaesenclast_ymm_ymm_ymmm256', hex: 'c4e27dddc1'},
  {code: 'VEX_Vaesdec_ymm_ymm_ymmm256', hex: 'c4e27ddec1'},
  {code: 'VEX_Vaesdeclast_ymm_ymm_ymmm256', hex: 'c4e27ddfc1'},
];
const memory = [
  {...exact[2], hex: 'c4e27ddc01', variant: 'unmapped/null-DS memory'},
  {...exact[1], hex: '360fc72c24', variant: 'limited SS memory'},
];
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
  for (const row of [...exact, ...memory]) {
    const code = bytes(row.hex), target = 0x2000 - code.length;
    const c = open(resident, entries, `${row.code}/${row.variant ?? 'canonical'}`);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, 'public strict refusal remains separate from execution availability', target, code.length, 10);
    const material = stopped(c, 'complete exact absent facility captures UD');
    const input = seed(target);
    invoke(c, 'UD before user privilege, null DS, limited SS, unmapped operand or numerical effects', material,
      input, expected(input, 20, 0), {budget: 2});
    h.host(c, 'close');
  }
  for (const row of exact.filter(row => ['Xsaves_mem', 'VEX_Vaesenc_ymm_ymm_ymmm256'].includes(row.code))) {
    const code = bytes(row.hex), target = 0x2000 - code.length + 1;
    const c = open(resident, entries, `${row.code} final-byte repair`);
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
    strict(c, 'incomplete exact Code strict preparation refuses neutrally', target, code.length, 10);
    stopped(c, 'missing final byte prevents absence classification', 10);
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, Buffer.concat([code.subarray(-1), bytes('ebfe')]));
    h.host(c, 'protect', [0x2000, 1, 1]); stopped(c, 'READ without EXECUTE still prevents absence classification', 10);
    h.host(c, 'protect', [0x2000, 1, 7]); const material = stopped(c, 'full executable repair captures exact UD');
    const input = seed(target); invoke(c, 'repaired Code faults without touching any operand', material,
      input, expected(input, 20, 0), {budget: 2}); h.host(c, 'close');
  }
  for (const row of [
    {hex: 'c4e37d44c100', label: 'selected absent VPCLMUL256 UD', invalid: false, absent: true},
    {hex: 'f00fc728', label: 'illegal LOCK XSAVES decoder UD', invalid: true},
  ]) {
    const code = bytes(row.hex), target = 0x1100, c = open(resident, entries, row.label);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, 'strict prefix/neighbor refusal retained', target, code.length, 10);
    const fault = row.invalid || row.absent;
    const material = stopped(c, row.label, fault ? 0 : 10);
    if (fault) {const input = seed(target); invoke(c, row.invalid ? 'existing decoder-invalid outcome stays UD' : 'selected absent VPCLMUL256 reaches UD', material,
      input, expected(input, 20, 0), {budget: 2});}
    h.host(c, 'close');
  }
  for (const [index, opcode] of ['dc', 'dd', 'de', 'df'].entries()) {
    const c = open(resident, entries, `VEX128 AES ${opcode} positive control`), target = 0x1100;
    h.upload(c, target, bytes(`c4e279${opcode}c1ebfe`)); const initial = seed(target); h.put(c, 0, initial);
    const helper = resident ? 'avx_execute_resident' : 'avx_execute';
    const material = strict(c, 'existing narrow AES+AVX capture remains successful without VAES', target, 7, 0, [helper]);
    for (const tf of [0, TF]) {
      const input = seed(target, {flags: FLAGS | RF | tf}); input.fill(0, 4396, 4428);
      const want = expected(input, tf ? 16 : 1, 1, target + 5, tf ? 7 : 6);
      want.fill(index < 2 ? 0x63 : 0x52, 4396, 4412); want.fill(0, 4628, 4644);
      want.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0], 7), 100);
      want.fill(0, 652, 684); want.writeUInt32LE(FLAGS | tf, 656);
      const args = [...(resident && material ? material.binding : []), 0x01001002 + index, 0x40, 0, 0, 0, FLAGS | tf];
      invoke(c, 'zero-state AES round has literal 63/52 bytes and clears only destination YMM upper', material,
        input, want, {budget: tf ? 2 : 1, helpers: [{name: helper, args, status: 0}]});
    }
    h.host(c, 'close');
  }

}
assert.equal(planned, 128, 'finite planned module invocations across four profiles');
assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'extended-counts.json'), JSON.stringify({planned, invoked, exactCodes: exact.map(row => row.code),
  contextsPlanned: 64, compileObservationsPlanned: 176, totalRowsPlanned: 304, sourcePins}, null, 2) + '\n');
h.finish([
  'exact six-Code absence cut only: two supervisor XSAVE forms and four VEX256 VAES forms; no facility activated',
  'four replacement/resident by extent/entry profiles use real guard/helper exports and independent full4796 arena/every mapped RAM page oracles',
  'real bootstrap reaches NeedCode before execution capture; strict public refusal and unselected neighbors remain separate',
  'full executable fetch precedes classification; no helper, operand, SSP or XSS authority reached by UD',
  'unchanged synthetic UD IR/emitter/ABI reuses existing optional-suite zero-budget/cancel/currentness/live-CS/flags ordering proof; no duplicated schedule matrix',
  'four VEX128 AES controls remain real successful operations with literal zero-state round results, typed packets, complete vector/GPR/flags state and exact helper trace',
  'missing final byte is neutral preparation error until executable repair; no invented guest PF or successful supervisor-state semantics',
  'no general optional-family, kernel, hardware-equivalence or CPU-completeness claim',
]);
