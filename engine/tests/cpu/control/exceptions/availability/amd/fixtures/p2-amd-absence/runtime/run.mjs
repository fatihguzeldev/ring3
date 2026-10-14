import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const supportPath = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, words, record, state, exit, hash} = await import(pathToFileURL(join(root, supportPath)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/exceptions/availability/amd/';
const paths = ['engine/src/cpu/x86/environment.rs', 'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/x86/decode/execution_faults/optional.rs',
  'engine/src/cpu/x86/decode/execution_faults/optional/amd.rs',
  'engine/src/cpu/x86/decode/execution_faults/optional/amd/tests.rs', 'engine/src/cpu/x86/decode/decoder.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/wasm/control_instructions.rs',
  'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/process/execution/stopped.rs',
  'engine/tests/cpu/control/native.rs', 'engine/tests/cpu/control/wasm.rs', own + 'native.rs', own + 'wasm.rs',
  own + 'fixtures/p2-amd-absence/runtime/run.mjs', supportPath];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), RF = 0x10000, TF = 0x100, FLAGS = 0x244cd7, bytes = hex => Buffer.from(hex, 'hex');
const poison = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0x7000, 0xfffffffc, 0xfffffffb, 0xfffffffa];
const rows = [
  {code: 'Movntss_m32_xmm', hex: 'f30f2b00', tag: 'SSE4A'},
  {code: 'Femms', hex: '0f0e', tag: 'D3NOW'},
  {code: 'D3NOW_Pfadd_mm_mmm64', hex: '360f0f04249e', tag: 'D3NOW'},
  {code: 'D3NOW_Pswapd_mm_mmm64', hex: '0f0fc1bb', tag: 'D3NOWEXT'},
  {code: 'XOP_Vprotb_xmm_xmmm128_xmm', hex: '648fe9789000', tag: 'XOP'},
  {code: 'VEX_Vpermil2ps_xmm_xmm_xmmm128_xmm_imm4', hex: 'c4e37948c120', tag: 'XOP', d16: true},
  {code: 'VEX_Vfmaddps_xmm_xmm_xmmm128_xmm', hex: 'c4e37968c120', tag: 'FMA4'},
  {code: 'VEX_Vfmaddps_ymm_ymm_ymm_ymmm256', hex: 'c4e3fd680020', tag: 'FMA4'},
];
assert.deepEqual([...new Set(rows.map(row => row.tag))], ['SSE4A', 'D3NOW', 'D3NOWEXT', 'XOP', 'FMA4']);
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
    const material = stopped(c, 'exact absent AMD prerequisite captures execution UD'), input = seed(target, row);
    invoke(c, `${row.code}: UD before operand segments, memory, MMX/FMA state or TF`, material,
      input, expected(input, 20, 0), {budget: 2}); h.host(c, 'close');
  }
  for (const hex of ['0f0fc19e', 'c4e37968c120']) {
    const code = bytes(hex), target = 0x2000 - code.length + 1;
    const c = open(resident, entries, 'trailing 3DNow opcode / FMA4 is4 fetch repair');
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
    strict(c, 'missing final byte remains strict fetch refusal', target, code.length, 10);
    stopped(c, 'absent feature cannot classify incomplete executable source', 10);
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, code.subarray(-1)); h.host(c, 'protect', [0x2000, 1, 1]);
    stopped(c, 'READ without EXECUTE cannot classify missing family/immediate byte', 10);
    h.host(c, 'protect', [0x2000, 1, 7]); const material = stopped(c, 'complete executable repair selects exact absence UD');
    const input = seed(target);
    invoke(c, 'repaired instruction never reaches numeric/operand execution or TF', material,
      input, expected(input, 20, 0), {budget: 2}); h.host(c, 'close');
  }
  for (const hex of ['66660f78c00408', '8fea7810c100000000', 'f08fe97890c1']) {
    const invalid = hex.startsWith('f0'), tbm = hex.startsWith('8f'), code = bytes(hex), target = 0x1200, c = open(resident, entries, hex);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, 'prefix boundary remains a strict refusal', target, code.length, 10);
    const material = stopped(c, invalid ? 'existing INVALID LOCK UD' : tbm ? 'separately selected absent TBM UD' : 'duplicate66 neutral refusal', invalid || tbm ? 0 : 10);
    if (invalid || tbm) {
      const input = seed(target); invoke(c, invalid ? 'invalid LOCK remains UD rather than feature or operand execution' : 'same TBM bytes now select their separate qualified absence prerequisite', material,
        input, expected(input, 20, 0), {budget: 2});
    }
    h.host(c, 'close');
  }
  {
    const c = open(resident, entries, 'PREFETCH independent of absent 3DNow'), target = 0x1200;
    h.upload(c, target, bytes('0f0d00ebfe')); const input = seed(target); h.put(c, 0, input);
    const material = strict(c, 'supported PREFETCH is not a 3DNow unavailable Code', target, 5);
    invoke(c, 'ignored supported hint retires while preserving all operand/numeric state', material, input,
      expected(input, 16, 1, target + 3), {budget: 2}); h.host(c, 'close');
  }
}
assert.equal(contexts, 56); assert.equal(planned, 104); assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'amd-absence-counts.json'), JSON.stringify({contexts, planned_module_invocations: planned,
  admitted_module_invocations: invoked, selected_codes: rows.map(row => row.code), source_pins: sourcePins}, null, 2) + '\n');
h.finish(['five original AMD absent prerequisite tags plus separately qualified TBM control; no CPU profile or success state change',
  'real guard-only execution of all five tags, plus FEMMS/PFADD state-vs-memory, XOP-tagged VEX and FMA4 width/W distinctions',
  'literal full4796 arena/allRAM, UD0retirement before nullDS/FS, shortSS, unmapped operands, MMX/FMA/numeric state or TF',
  'D16 explicit current segment transport; trailing3DNow opcode andFMA4is4 executable repair before classification',
  'invalidLOCK and duplicate66 remain separate; XOP-encodedTBM stays outside AMD5 tags but selects separate TBM absence UD; supported PREFETCH remains NOP',
  'source/native MMX EMMS andFMA3 positives plus generic disabled-state lifecycle/CPUID/currentness/budget proof are retained and reused']);
