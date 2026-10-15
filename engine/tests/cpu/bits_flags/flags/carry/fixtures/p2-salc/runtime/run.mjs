import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit selected engine, unique output and source root');
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, state, exit, record, words, hash} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/bits_flags/flags/carry/';
const paths = [own + 'wasm.rs', own + 'native.rs', own + 'fixtures/p2-salc/runtime/run.mjs',
  'engine/tests/cpu/bits_flags/wasm.rs', 'engine/tests/cpu/bits_flags/native.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/lower/tests/salc.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs', 'engine/src/cpu/x86/decode/execution_faults.rs',
  'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/environment.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/control.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', support];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
const regs = [0xa1b2c3d4, 0x23456789, 0x3456789a, 0x456789ab, 0x7800, 0x56789abc, 0x6789abcd, 0x789abcde];
const bytes = hex => Buffer.from(hex, 'hex');
const selected = [
  {label: 'D32 CF0 preserves high EAX and every flag', eax: 0xa1b2c3d4, flags: 0x244cd6, result: 0xa1b2c300},
  {label: 'D32 CF1 preserves high EAX and every flag', eax: 0x12345678, flags: 0x244cd7, result: 0x123456ff},
  {label: 'D16 CF0 writes AL independently of code default', d16: true, eax: 0x89abcdef, flags: 0x202, result: 0x89abcd00},
  {label: 'D16 CF1 writes AL independently of code default', d16: true, eax: 0x76543210, flags: 0x244cd7, result: 0x765432ff},
  {label: 'ordinary RF consumption and shadow clearing precede one TF retirement', eax: 0xa1e2d3c4, flags: 0x10dd7,
    result: 0xa1e2d3ff, outputFlags: 0xdd7, shadow: 1, reason: 16, budget: 2},
  {label: 'existing SETB AL remains a distinct 01 result', hex: '0f92c0', eax: 0x12345678, flags: 0x244cd7, result: 0x12345601},
];
let contexts = 0, plannedCompiles = 0, plannedRefusals = 0, plannedInvocations = 0, invoked = 0;
function seed(pc, {d16 = false, eax = 0xa1b2c3d4, flags = 0x244cd6, shadow = 0} = {}) {
  const input = state(h.initial, {pc, flags, regs: [eax, ...regs.slice(1)]}); input.writeUInt32LE(shadow, 12);
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
function expected(input, {pc = input.readUInt32LE(48), eax = input.readUInt32LE(16),
  flags = input.readUInt32LE(52), reason = 1, retired = 1} = {}) {
  const want = Buffer.from(input); want.writeUInt32LE(pc, 48); want.writeUInt32LE(eax, 16);
  want.writeUInt32LE(flags, 52); want.writeUInt32LE(0, 12);
  return exit(want, reason, retired, reason >= 16 ? 7 : 1);
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
function invoke(c, label, material, input, want, options = {}) {
  plannedInvocations++;
  if (material) {invoked++; return h.observe(c, label, {material, input, want, ...options});}
  h.skip(c, label, 'required preparation admission'); return null;
}
function bootstrap(c, target) {
  const jump = bytes('e900000000'); jump.writeInt32LE(target - PC - 5, 1); h.upload(c, PC, jump);
  const input = seed(PC); h.put(c, 0, input);
  const material = strict(c, 'real branch bootstrap', PC, jump.length);
  invoke(c, 'bootstrap retires once then returns canonical NeedCode', material, input,
    expected(input, {pc: target, reason: 3}), {budget: 2});
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of selected) {
    const code = bytes(row.hex ?? 'd6'), target = 0x1200, c = open(resident, entries, row.label);
    const body = Buffer.concat([code, bytes('ebfe')]); h.upload(c, target, body);
    const input = seed(target, row); h.put(c, 0, input);
    const material = strict(c, 'complete instruction and terminal loop preparation', target, body.length);
    invoke(c, row.label, material, input, expected(input, {pc: target + code.length, eax: row.result,
      flags: row.outputFlags ?? row.flags, reason: row.reason ?? 1}), {budget: row.budget ?? 1});
    h.host(c, 'close');
  }
  {
    const c = open(resident, entries, 'missing/nonEXEC/EXEC single opcode repair'); bootstrap(c, 0x2000);
    strict(c, 'missing SALC opcode remains a neutral strict fetch refusal', 0x2000, 3, 10);
    stopped(c, 'missing SALC opcode remains a neutral stopped fetch refusal', 10);
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, bytes('d6ebfe')); h.host(c, 'protect', [0x2000, 1, 1]);
    stopped(c, 'READ without EXECUTE cannot classify SALC', 10);
    h.host(c, 'protect', [0x2000, 1, 7]); const material = stopped(c, 'EXECUTE repair captures real SALC source');
    const input = seed(0x2000); invoke(c, 'repaired SALC uses the actual one-byte source', material, input,
      expected(input, {pc: 0x2001, eax: 0xa1b2c300})); h.host(c, 'close');
  }
  for (const row of [
    {hex: '66d6', label: 'reserved operand prefix stays a neutral unsupported input'},
    {hex: 'f0d6', label: 'explicit LOCK retains existing decoder UD', invalid: true},
  ]) {
    const code = bytes(row.hex), target = 0x1200, c = open(resident, entries, row.label);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, 'strict prefix or invalid encoding refusal', target, code.length, 10);
    const material = stopped(c, row.label, row.invalid ? 0 : 10);
    if (row.invalid) {
      const input = seed(target); invoke(c, row.label, material, input,
        expected(input, {reason: 20, retired: 0}), {budget: 2});
    }
    h.host(c, 'close');
  }
}
assert.equal(contexts, 36); assert.equal(plannedCompiles, 68); assert.equal(plannedRefusals, 24);
assert.equal(plannedInvocations, 44); assert.deepEqual(pins(), sourcePins, 'retained literal oracle and source pins');
writeFileSync(join(output, 'salc-counts.json'), JSON.stringify({contexts, plannedCompiles, plannedRefusals,
  plannedInvocations, invoked, totalRowsPlanned: plannedCompiles + plannedInvocations,
  selected: selected.map(row => row.label), sourcePins}, null, 2) + '\n');
h.finish([
  'bareD6 only: CF0/CF1 set AL00/FF under current D16/D32, highEAX/allotherGPRs/allflags/numeric/segment/arena and allmappedRAM literal unchanged',
  'ordinary existing RF consumption/shadow clearing/TFdebug after one retirement; guard-only realimports and no operandhelper',
  'four replacement/resident by extent/entry routes; full4796 arena/allmappedRAM and exact empty operandhelper trace',
  'one-byte missing/nonEXEC/EXEC repair uses real branch-to-NeedCode and source mapping; no fabricated prefix or fetched byte',
  'SETB AL remains distinct01; explicitLOCK existingUD; undefined66 prefix remains neutral rather than inventing a hardware result',
  'existing generic cancel/budget/currentness and broader control-transfer proofs reused; no newhelperwire/CPUfeature/math dependency',
  'source-only consumer proposal until selected integrated engine executes it; no wholeCPU completion claim',
]);
