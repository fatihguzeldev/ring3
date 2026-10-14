import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, state, exit, record, words, hash} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/exceptions/availability/system/';
const paths = [own + 'wasm.rs', own + 'fixtures/p2-system-availability/runtime/run.mjs',
  'engine/tests/cpu/control/wasm.rs', 'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/environment.rs',
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/x86/decode/execution_faults/tests/system.rs',
  'engine/src/cpu/x86/decode/execution_faults/optional.rs', 'engine/src/cpu/x86/decode/execution_faults/optional/amd.rs',
  'engine/src/cpu/x86/decode/execution_faults/optional/amd/tests.rs', 'engine/src/cpu/x86/decode/decoder.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/wasm/control_instructions.rs',
  'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/abi/x86/exit.rs',
  'engine/src/process/execution/stopped.rs', support];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), RF = 0x10000, TF = 0x100, FLAGS = 0x244cd7;
const regs = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0x7000, 0xfffffffc, 0xfffffffb, 0xfffffffa];
const bytes = hex => Buffer.from(hex, 'hex');
const selected = [
  {code: 'XOP_Llwpcb_r32', hex: '8fe97812c0', reason: 20, why: 'poison EAX/nullDS cannot read or publish an implicit control block'},
  {code: 'XOP_Slwpcb_r32', hex: '8fe97812c8', reason: 20, why: 'nonzero EAX must not become a guessed inactive-profiling zero'},
  {code: 'XOP_Lwpins_r32_rm32_imm32', hex: '8fea7812c100000000', reason: 20, why: 'initial CF1 must not become a guessed inactive-profiling CF0'},
  {code: 'XOP_Lwpval_r32_rm32_imm32', hex: '64678fea78120800000000', reason: 20, d16: true,
    why: 'D16/address32 nullFS memory never reaches an operand read'},
  {code: 'Pconfig', hex: '0f01c5', reason: 20, why: 'invalid leaf and unaligned/unmapped EBX cannot precede absent-feature/CPL UD'},
  {code: 'Invlpgbd', hex: '0f01fe', reason: 20, why: 'absent feature precedes CPL3/reserved-input GP'},
  {code: 'Invlpgbw', hex: '670f01fe', reason: 20, why: 'address16 broadcast form has the same absent-feature UD'},
  {code: 'Tlbsync', hex: '0f01ff', reason: 20, why: 'absent feature needs no guessed EFER/VMCB or synchronization effect'},
  {code: 'Wbnoinvd', hex: 'f30f09', reason: 11, why: 'feature absence falls back to privileged WBINVD and must produce GP0'},
];
assert.equal(new Set(selected.map(row => row.code)).size, 9);
let contexts = 0, plannedInvocations = 0, invoked = 0, plannedCompiles = 0, plannedRefusals = 0;
function seed(pc, {d16 = false, flags = FLAGS | RF | TF, shadow = 1} = {}) {
  const input = state(h.initial, {pc, flags, regs}); input.writeUInt32LE(shadow, 12);
  const sg = record('R3SG', 64, [], d16 ? 3 : 2);
  [0x23, 0x1b, 0x23, 0, 0, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + i * 2));
  [0xf33fffff, d16 ? 0xf1bfffff : 0xf3bfffff, 0xf2300fff, 0, 0, 0]
    .forEach((value, i) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  input.set(sg, SEG); input.set(d16 ? record('R3SB', 40, [0, 0, 0, 0, 0, 0]) : Buffer.alloc(40), BASES);
  for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {
    input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
  }
  return input;
}
function expected(input, reason, retired, pc = input.readUInt32LE(48)) {
  const want = Buffer.from(input); want.writeUInt32LE(pc, 48);
  want.writeUInt32LE(input.readUInt32LE(52) & ~RF, 52); want.writeUInt32LE(0, 12);
  return exit(want, reason, retired, reason === 20 ? 7 : reason === 11 ? 6 : 1);
}
function invoke(c, label, material, input, want, options = {}) {
  plannedInvocations++;
  if (material) {invoked++; return h.observe(c, label, {material, input, want, ...options});}
  h.skip(c, label, 'required preparation admission'); return null;
}
function open(resident, entries, label) {
  contexts++; const c = h.open(resident, entries, label, [[PC, 7], [0x5000, 3], [0x7000, 3]]);
  h.upload(c, 0x5000, Buffer.alloc(4096, 0x5a)); h.upload(c, 0x7000, Buffer.alloc(4096, 0xa5)); return c;
}
function compile(c, label, spans, {status = 0, stopped = false} = {}) {
  plannedCompiles++; if (status !== 0) plannedRefusals++;
  return h.compile(c, label, spans, [], {expectedStatus: status, stopped});
}
function strict(c, label, pc, length, status = 0) {return compile(c, label, [[pc, length]], {status});}
function stopped(c, label, status = 0) {
  h.put(c, 140, words([0xdeadbeef, 0xffffffff])); return compile(c, label, [], {status, stopped: true});
}
function bootstrap(c, target, d16 = false) {
  const jump = Buffer.alloc(d16 ? 3 : 5); jump[0] = 0xe9;
  if (d16) jump.writeInt16LE(target - PC - jump.length, 1); else jump.writeInt32LE(target - PC - jump.length, 1);
  h.upload(c, PC, jump); const input = seed(PC, {d16, flags: FLAGS | RF}); h.put(c, 0, input);
  const material = strict(c, 'real same-mode bootstrap', PC, jump.length);
  invoke(c, 'actual branch retires once before canonical NeedCode', material, input,
    expected(input, 3, 1, target), {budget: 2});
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of selected) {
    const code = bytes(row.hex), target = 0x2000 - code.length, c = open(resident, entries, row.code);
    h.upload(c, target, code); bootstrap(c, target, row.d16);
    strict(c, 'diagnostic decoder retains a neutral strict refusal', target, code.length, 10);
    const material = stopped(c, 'qualified complete instruction captures its precise architectural fault');
    const input = seed(target, row);
    invoke(c, `${row.code}: ${row.why}`, material, input, expected(input, row.reason, 0), {budget: 2});
    h.host(c, 'close');
  }
  for (const row of [
    {hex: '8fea7812c100000000', reason: 20, label: 'LWP final immediate'},
    {hex: 'f30f09', reason: 11, label: 'WBNOINVD final opcode'},
  ]) {
    const code = bytes(row.hex), target = 0x2000 - code.length + 1, c = open(resident, entries, row.label);
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
    strict(c, 'missing final byte remains a neutral strict fetch refusal', target, code.length, 10);
    stopped(c, 'incomplete executable source cannot classify the selected fault', 10);
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, code.subarray(-1)); h.host(c, 'protect', [0x2000, 1, 1]);
    stopped(c, 'READ without EXECUTE still cannot classify the final byte', 10);
    h.host(c, 'protect', [0x2000, 1, 7]); const material = stopped(c, 'full executable repair captures the selected fault');
    const input = seed(target);
    invoke(c, `${row.label}: repair preserves UD versus GP0 and all operand state`, material, input,
      expected(input, row.reason, 0), {budget: 2}); h.host(c, 'close');
  }
  for (const row of [
    {hex: 'f3f30f09', label: 'duplicate mandatory F3 stays a neutral profile refusal'},
    {hex: 'f20f09', label: 'unconsumed REPNE WBINVD stays a neutral profile refusal'},
    {hex: 'f0f30f09', label: 'invalid LOCK WBNOINVD keeps existing decoder UD', invalid: true},
    {hex: '660f01c5', label: 'invalid operand-size PCONFIG keeps existing decoder UD', invalid: true},
  ]) {
    const code = bytes(row.hex), target = 0x1200, c = open(resident, entries, row.label);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, 'strict invalid or profile prefix refusal retained', target, code.length, 10);
    const material = stopped(c, row.label, row.invalid ? 0 : 10);
    if (row.invalid) {
      const input = seed(target); invoke(c, row.label, material, input, expected(input, 20, 0), {budget: 2});
    }
    h.host(c, 'close');
  }
}
assert.equal(contexts, 60); assert.equal(plannedCompiles, 196); assert.equal(plannedRefusals, 84);
assert.equal(plannedInvocations, 112); assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'system-availability-counts.json'), JSON.stringify({contexts, plannedCompiles, plannedRefusals,
  plannedInvocations, invoked, totalRowsPlanned: plannedCompiles + plannedInvocations,
  selectedCodes: selected.map(row => row.code), sourcePins}, null, 2) + '\n');
h.finish([
  'nine qualified protected16/32 Codes only: LWP4 and PCONFIG/INVLPGB/TLBSYNC UD versus absent-WBNOINVD fallback GP0',
  'four real replacement/resident by extent/entry routes; literal full4796 arena/allmappedRAM, allGPR/flags/numeric/segment state, guard-only helper traces and retirement',
  'nullDS implicit pointer, preserved EAX and CF, D16/address32 nullFS operand, poison platform/TLB inputs and mandatoryF3 fallback distinguish accidental success or operand checks',
  'LWP last-immediate and WBNOINVD last-opcode missing/nonEXEC/EXEC repair retain fullfetch software policy; AMD permits model-dependent earlierUD at later-byte boundaries',
  'duplicateF3 and unconsumedREPNE remain neutral; LOCK WBNOINVD and66 PCONFIG retain existing invalid-encoding UD',
  'generic fault budget/cancel/currentness/RF/TF/shadow and old WBINVD/INVD/AMD5/MMX/FMA3/PREFETCH controls are reused unchanged',
  'no CPUID change, enabled profiling/platform/TLB/cache backend, guessed MSR/OS value or wholeCPU claim',
]);
