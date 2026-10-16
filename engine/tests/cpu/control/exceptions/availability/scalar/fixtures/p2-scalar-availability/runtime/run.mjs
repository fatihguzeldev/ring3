import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const supportPath = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, words, record, state, exit, hash} = await import(pathToFileURL(join(root, supportPath)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/exceptions/availability/scalar/';
const paths = ['engine/src/cpu/x86/environment.rs', 'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/x86/decode/execution_faults/optional.rs',
  'engine/src/cpu/x86/decode/execution_faults/optional/amd.rs',
  'engine/src/cpu/x86/decode/execution_faults/optional/amd/tests/scalar.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/x86/decode/lower.rs', 
  'engine/src/cpu/x86/decode/profile/tests/prefetch/instruction.rs',
  'engine/src/cpu/dbt/wasm/instructions/control_instructions.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs',
  'engine/src/process/execution/stopped.rs', 'engine/tests/cpu/control/native.rs', 'engine/tests/cpu/control/wasm.rs',
  own + 'native.rs', own + 'wasm.rs', own + 'fixtures/p2-scalar-availability/runtime/run.mjs', supportPath];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), RF = 0x10000, TF = 0x100, FLAGS = 0x244cd7, bytes = hex => Buffer.from(hex, 'hex');
const poison = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0x7000, 0xfffffffc, 0xfffffffb, 0xfffffffa];
const rows = [
  {code: 'XOP_Blcfill_r32_rm32', hex: '8fe97801c8', family: 'TBM'},
  {code: 'XOP_Bextr_r32_rm32_imm32', hex: '368fea7810042400000000', family: 'TBM'},
  {code: 'Monitorxd', hex: '640f01fa', family: 'MONITORX'},
  {code: 'Monitorxw', hex: '670f01fa', family: 'MONITORX'},
  {code: 'Mwaitx', hex: '0f01fb', family: 'MONITORX'},
  {code: 'Clzerod', hex: '0f01fc', family: 'CLZERO'},
  {code: 'Clzerow', hex: '0f01fc', family: 'CLZERO', d16: true},
  {code: 'Rdpru', hex: '0f01fd', family: 'RDPRU'},
  {code: 'Clac', hex: '0f01ca', family: 'CPL3'},
  {code: 'Stac', hex: '0f01cb', family: 'CPL3', flags: (FLAGS & ~0x40000) | RF | TF},
  {code: 'Prefetchit0_m8', hex: '360f183c24', family: 'PREFETCHI', nop: true},
  {code: 'Prefetchit1_m8', hex: '64670f1830', family: 'PREFETCHI', nop: true, d16: true},
];
assert.deepEqual([...new Set(rows.map(row => row.family))], ['TBM', 'MONITORX', 'CLZERO', 'RDPRU', 'CPL3', 'PREFETCHI']);
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
    const code = bytes(row.hex), target = row.nop ? 0x1200 : 0x2000 - code.length;
    const c = open(resident, entries, row.code);
    h.upload(c, target, row.nop ? Buffer.concat([code, bytes('ebfe')]) : code);
    bootstrap(c, target, row.d16);
    const material = row.nop
      ? strict(c, 'exact absent PREFETCHIT memory Code prepares ignored hint', target, code.length + 2)
      : (strict(c, 'strict diagnostic decoder remains a refusal', target, code.length, 10),
        stopped(c, 'exact absent prerequisite or CPL3 control prepares execution UD'));
    const input = seed(target, row);
    invoke(c, row.nop ? 'NOP retires once and preserves AC, operands, numeric state and RAM before TF'
      : 'UD0 before bad SS/nullFS/invalid extension or counter inputs, AC mutation, RAM and TF',
      material, input, expected(input, row.nop ? 16 : 20, row.nop ? 1 : 0, target + (row.nop ? code.length : 0)), {budget: 2});
    h.host(c, 'close');
  }
  for (const row of [
    {hex: '8fea7810c100000000', label: 'TBM final immediate', nop: false},
    {hex: '0f01fa', label: 'MONITORX final opcode', nop: false},
    {hex: '0f1830', label: 'PREFETCHIT final memory ModRM', nop: true},
  ]) {
    const code = bytes(row.hex), target = 0x2000 - code.length + 1;
    const c = open(resident, entries, row.label);
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
    strict(c, 'missing final instruction byte remains strict fetch refusal', target, code.length, 10);
    const fetchMaterial = h.fetchFault(c, 'no unavailable or NOP classification before complete executable fetch', {pc: target, length: code.length, address: 0x2000, errorCode: 20});
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, Buffer.concat([code.subarray(-1), row.nop ? bytes('ebfe') : Buffer.alloc(0)]));
    h.host(c, 'protect', [0x2000, 1, 1]);
    h.fetchFault(c, 'READ-only final byte cannot be consumed as executable source', {pc: target, length: code.length, address: 0x2000, errorCode: 21, material: fetchMaterial});
    h.host(c, 'protect', [0x2000, 1, 7]); h.reachNeedCode(c);
    const material = stopped(c, 'exact executable repair selects only the declared UD/NOP rule'), input = seed(target);
    invoke(c, 'complete source repair preserves all operands, AC and RAM', material, input,
      expected(input, row.nop ? 16 : 20, row.nop ? 1 : 0, target + (row.nop ? code.length : 0)), {budget: 2});
    h.host(c, 'close');
  }
  for (const row of [
    {hex: '66660f01fc', label: 'valid CLZERO Code with duplicate66 remains neutral refusal'},
    {hex: 'f00f1830', label: 'invalid LOCK PREFETCHIT remains existing UD', invalid: true},
    {hex: '0f18f0', label: 'register Reservednop0F18 is outside selected memory Codes'},
  ]) {
    const code = bytes(row.hex), target = 0x1200, c = open(resident, entries, row.label);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, 'strict invalid or unselected/prefix boundary remains refusal', target, code.length, 10);
    const material = stopped(c, row.label, row.invalid ? 0 : 10);
    if (row.invalid) {
      const input = seed(target); invoke(c, row.label, material, input, expected(input, 20, 0), {budget: 2});
    }
    h.host(c, 'close');
  }
  {
    const target = 0x1200, c = open(resident, entries, 'existing supported data PREFETCH positive');
    h.upload(c, target, bytes('0f0d00ebfe')); const input = seed(target); h.put(c, 0, input);
    const material = strict(c, 'existing exact supported PREFETCH remains NOP', target, 5);
    invoke(c, 'new instruction hints do not reclassify supported data prefetch', material, input,
      expected(input, 16, 1, target + 3), {budget: 2}); h.host(c, 'close');
  }
}
assert.equal(contexts, 76); assert.equal(planned, 140); assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'scalar-availability-counts.json'), JSON.stringify({contexts,
  planned_compile_observations: 236, planned_module_invocations: planned, admitted_module_invocations: invoked,
  selected_codes: rows.map(row => row.code), source_pins: sourcePins}, null, 2) + '\n');
h.finish(['twenty exact qualified Codes in source/native; twelve guest representatives across four unchanged profiles',
  'real guard-only modules and literal complete4796 arena/allmappedRAM; no operand/numeric/cache/wait/counter helper is allowed',
  'TBM register and SS-memory/immediate; MONITORX address16/32 and invalid extension; MWAITX invalid extension; CLZERO D16; RDPRU invalid index; CLAC/STAC AC unchanged',
  'both absent PREFETCHIT memory types ignore shortSS/nullFS; D16+67 default/address distinction; one retirement before TF',
  'TBM immediate, MONITORX opcode and PREFETCHIT ModRM complete-fetch/READ-only/executable-repair distinctions',
  'valid duplicate66 remains neutral; invalid LOCK remains UD; register Reservednop0F18 stays outside this selected family with no new ISA closure claim',
  'unchanged generic UD/Nop currentness, cancellation, zero budget and CPUID proof reused; no per-Code generic-state Cartesian matrix',
  'ENQCMD/S, INVPCID and HRESET remain explicitly outside this20-Code contract until their separate primary priority qualification']);
