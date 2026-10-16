import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const supportPath = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, words, record, state, exit, hash} = await import(pathToFileURL(join(root, supportPath)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/exceptions/availability/cet/';
const paths = ['engine/src/cpu/x86/environment.rs', 'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/x86/decode/execution_faults/state.rs',
  'engine/src/cpu/x86/decode/execution_faults/tests/state.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/dbt/wasm/instructions/control_instructions.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/process/execution/stopped.rs',
  'engine/tests/cpu/control/native.rs', 'engine/tests/cpu/control/wasm.rs', own + 'native.rs', own + 'wasm.rs',
  own + 'fixtures/p2-cet-disabled/runtime/run.mjs', supportPath];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), RF = 0x10000, TF = 0x100, FLAGS = 0x244cd7, bytes = hex => Buffer.from(hex, 'hex');
const poison = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0, 0xfffffffc, 0xfffffffb, 0xfffffffa];
const rows = [
  {code: 'Rstorssp_m64', hex: '6467f30f0128'},
  {code: 'Saveprevssp', hex: 'f30f01ea'},
  {code: 'Setssbsy', hex: 'f30f01e8'},
  {code: 'Clrssbsy_m64', hex: '36f30fae30'},
  {code: 'Wrssd_m32_r32', hex: '0f38f605ffffffff'},
  {code: 'Wrussd_m32_r32', hex: '67660f38f500', d16: true},
];
let contexts = 0, planned = 0, invoked = 0;
function seed(pc, {d16 = false, flags = FLAGS | RF | TF, shadow = 1} = {}) {
  const input = state(h.initial, {pc, flags, regs: poison}); input.writeUInt32LE(shadow, 12);
  const sg = record('R3SG', 64, [], d16 ? 3 : 2);
  [0x23, 0x1b, 0x23, 0, 0, 0].forEach((v, i) => sg.writeUInt16LE(v, 16 + i * 2));
  [0xf33fffff, d16 ? 0xf1bfffff : 0xf3bfffff, 0xf2300fff, 0, 0, 0]
    .forEach((v, i) => sg.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
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
  return exit(want, reason, retired, reason >= 16 ? 7 : 1);
}
function invoke(c, label, material, input, want, options = {}) {
  planned++;
  if (material) {invoked++; return h.observe(c, label, {input, want, material, ...options});}
  h.skip(c, label, 'required capture admission'); return null;
}
function open(resident, entries, label) {
  contexts++; const c = h.open(resident, entries, label, [[PC, 7], [0x5000, 3], [0x7000, 3]]);
  h.upload(c, 0x5000, Buffer.alloc(4096, 0x5a)); h.upload(c, 0x7000, Buffer.alloc(4096, 0xa5)); return c;
}
function strict(c, label, target, length, expectedStatus = 0) {
  return h.compile(c, label, [[target, length]], [], {expectedStatus});
}
function stopped(c, label, expectedStatus = 0) {
  h.put(c, 140, words([0xdeadbeef, 0xffffffff]));
  return h.compile(c, label, [], [], {stopped: true, expectedStatus});
}
function bootstrap(c, target, d16 = false) {
  const jump = Buffer.alloc(d16 ? 3 : 5); jump[0] = 0xe9;
  if (d16) jump.writeInt16LE(target - PC - jump.length, 1);
  else jump.writeInt32LE(target - PC - jump.length, 1);
  h.upload(c, PC, jump); const input = seed(PC, {d16, flags: FLAGS | RF}); h.put(c, 0, input);
  const material = strict(c, 'real same-mode branch capture', PC, jump.length);
  invoke(c, 'branch retires before canonical NeedCode', material, input, expected(input, 3, 1, target), {budget: 2});
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of rows) {
    const code = bytes(row.hex), target = 0x2000 - code.length, c = open(resident, entries, row.code);
    h.upload(c, target, code); bootstrap(c, target, row.d16);
    strict(c, 'diagnostic decoder remains strict', target, code.length, 10);
    const material = stopped(c, 'exact CET0 execution fault is captured'), input = seed(target, row);
    invoke(c, `${row.code}: UD before privilege, SSP, alignment, operand segments or memory and TF`, material,
      input, expected(input, 20, 0), {budget: 2}); h.host(c, 'close');
  }
  {
    const code = bytes('f30f012dffffffff'), target = 0x2000 - code.length + 1;
    const c = open(resident, entries, 'RSTORSSP final displacement repair');
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
    strict(c, 'missing final displacement remains strict fetch refusal', target, code.length, 10);
    stopped(c, 'CET0 cannot classify incomplete executable source', 10);
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, code.subarray(-1)); h.host(c, 'protect', [0x2000, 1, 1]);
    stopped(c, 'READ without EXECUTE still cannot classify CET0', 10);
    h.host(c, 'protect', [0x2000, 1, 7]); const material = stopped(c, 'complete executable repair selects CET0 UD');
    const input = seed(target);
    invoke(c, 'repaired RSTORSSP UD precedes null DS/unmapped unaligned token and TF', material,
      input, expected(input, 20, 0), {budget: 2}); h.host(c, 'close');
  }
  for (const hex of ['6666f30f0128', 'f2f30f0128', 'f00f38f600']) {
    const invalid = hex.startsWith('f0'), code = bytes(hex), target = 0x1200, c = open(resident, entries, hex);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, 'prefix boundary remains a strict refusal', target, code.length, 10);
    const material = stopped(c, invalid ? 'existing INVALID LOCK UD' : 'duplicate/repeat neutral refusal', invalid ? 0 : 10);
    if (invalid) {
      const input = seed(target); invoke(c, 'invalid LOCK remains UD rather than feature or operand execution', material,
        input, expected(input, 20, 0), {budget: 2});
    }
    h.host(c, 'close');
  }
  {
    const c = open(resident, entries, 'RDSSPD remains compatibility NOP'), target = 0x1200;
    h.upload(c, target, bytes('f30f1ec8ebfe')); const input = seed(target); h.put(c, 0, input);
    const material = strict(c, 'RDSSPD is not in the six-code fault cut', target, 6);
    invoke(c, 'RDSSPD preserves EAX and completes one instruction before TF', material, input,
      expected(input, 16, 1, target + 4), {budget: 2}); h.host(c, 'close');
  }
}
assert.equal(contexts, 44); assert.equal(planned, 76); assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'cet-disabled-counts.json'), JSON.stringify({contexts, planned_module_invocations: planned,
  admitted_module_invocations: invoked, selected_codes: rows.map(row => row.code), source_pins: sourcePins}, null, 2) + '\n');
h.finish(['six exact disabled-CET forms: UD before SSP, memory, alignment and supervisor CPL, full literal arena/RAM and real guard-only execution',
  'source-local exact mode/prefix/fullfetch tests and generic PKRU/INCSSPD lifecycle/CPUID/currentness/budget controls are reused unchanged',
  'D16 explicit current segment record and address67, nullFS/DS, shortSS and unmapped unaligned operands distinguish accidental access',
  'one final-displacement fetch repair, existing LOCK UD, repeat/duplicate profile refusals and RDSSPD NOP remain distinct',
  'immutable CR4.CET0 and existing CPUID/environment/PKRU behavior unchanged; no SSP/token/MSR state or enabled CET execution introduced']);
