import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, state, exit, record, words, hash} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/exceptions/availability/optional/modern/';
const paths = [own + 'wasm.rs', own + 'fixtures/p2-modern-availability/runtime/run.mjs',
  'engine/tests/cpu/control/wasm.rs', 'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/environment.rs',
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/x86/decode/execution_faults/optional.rs',
  'engine/src/cpu/x86/decode/execution_faults/optional/modern.rs',
  'engine/src/cpu/x86/decode/execution_faults/optional/modern/tests.rs',
  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/avx/mod.rs', 'engine/src/cpu/x86/decode/avx/crypto.rs',
  'engine/src/cpu/dbt/wasm/instructions/control_instructions.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/execute.rs', 'engine/src/process/cpu/avx.rs', 'engine/src/abi/cpu_helper.rs',
  'engine/src/abi/x86/avx.rs', 'engine/src/process/execution/stopped.rs', support];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), RF = 0x10000, TF = 0x100, FLAGS = 0x244cd7;
const regs = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0x7000, 0xfffffffc, 0xfffffffb, 0xfffffffa];
const bytes = hex => Buffer.from(hex, 'hex');
const selected = [
  {tag: 'AVX_VNNI', code: 'VEX_Vpdpbusd_xmm_xmm_xmmm128', hex: 'c4e27950c1'},
  {tag: 'AVX_VNNI_INT8', code: 'VEX_Vpdpbssd_ymm_ymm_ymmm256', hex: 'c4e27f50c1'},
  {tag: 'AVX_VNNI_INT16', code: 'VEX_Vpdpwuud_xmm_xmm_xmmm128', hex: '6467c4e278d200', d16: true},
  {tag: 'AVX_NE_CONVERT', code: 'VEX_Vcvtneps2bf16_xmm_xmmm128', hex: 'c4e27a72c1'},
  {tag: 'AVX_IFMA', code: 'VEX_Vpmadd52luq_ymm_ymm_ymmm256', hex: 'c4e2fdb4c1'},
  {tag: 'SHA512', code: 'VEX_Vsha512rnds2_ymm_ymm_xmm', hex: 'c4e27fcbc1'},
  {tag: 'SM3', code: 'VEX_Vsm3rnds2_xmm_xmm_xmmm128_imm8', hex: 'c4e379dec100'},
  {tag: 'SM4', code: 'VEX_Vsm4rnds4_ymm_ymm_ymmm256', hex: 'c4e27fdac1'},
  {tag: 'RAO_INT', code: 'Aor_m32_r32', hex: 'f20f38fc08'},
];
const operands = [
  {tag: 'AVX_NE_CONVERT', code: 'VEX_Vbcstnesh2ps_xmm_m16', hex: '6467c4e279b100'},
  {tag: 'RAO_INT', code: 'Aadd_m32_r32', hex: '360f38fc08'},
];
let contexts = 0, planned = 0, invoked = 0;
function seed(pc, {d16 = false, flags = FLAGS | RF | TF, shadow = 1} = {}) {
  const input = state(h.initial, {pc, flags, regs}); input.writeUInt32LE(shadow, 12);
  for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {
    input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
  }
  const sg = record('R3SG', 64, [], d16 ? 3 : 2);
  [0x23, 0x1b, 0x23, 0, 0, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + i * 2));
  [0xf33fffff, d16 ? 0xf1bfffff : 0xf3bfffff, 0xf2300fff, 0, 0, 0]
    .forEach((value, i) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  input.set(sg, SEG); input.set(d16 ? record('R3SB', 40, [0, 0, 0, 0, 0, 0]) : Buffer.alloc(40), BASES);
  return input;
}
function expected(input, reason, retired, pc = input.readUInt32LE(48), version = reason >= 16 ? 7 : 1) {
  const want = Buffer.from(input); want.writeUInt32LE(pc, 48);
  want.writeUInt32LE(input.readUInt32LE(52) & ~RF, 52); want.writeUInt32LE(0, 12);
  return exit(want, reason, retired, version);
}
function invoke(c, label, material, input, want, options = {}) {
  planned++; if (material) {invoked++; return h.observe(c, label, {material, input, want, ...options});}
  h.skip(c, label, 'required preparation admission'); return null;
}
function open(resident, entries, label) {
  contexts++; const c = h.open(resident, entries, label, [[PC, 7], [0x5000, 3], [0x7000, 3]]);
  h.upload(c, 0x5000, Buffer.alloc(4096, 0x5a)); h.upload(c, 0x7000, Buffer.alloc(4096, 0xa5)); return c;
}
function strict(c, label, pc, length, status = 0, helpers = []) {
  return h.compile(c, label, [[pc, length]], helpers, {expectedStatus: status});
}
function bootstrap(c, target, d16 = false) {
  const jump = Buffer.alloc(d16 ? 3 : 5); jump[0] = 0xe9;
  if (d16) jump.writeInt16LE(target - PC - jump.length, 1); else jump.writeInt32LE(target - PC - jump.length, 1);
  h.upload(c, PC, jump); const input = seed(PC, {d16, flags: FLAGS | RF}); h.put(c, 0, input);
  const material = strict(c, 'real same-mode bootstrap to unavailable target', PC, jump.length);
  invoke(c, 'actual branch retires once and reaches canonical NeedCode', material,
    input, expected(input, 3, 1, target), {budget: 2});
}
function stopped(c, label, status = 0) {
  h.put(c, 140, words([0xdeadbeef, 0xffffffff]));
  return h.compile(c, label, [], [], {stopped: true, expectedStatus: status});
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of [...selected, ...operands]) {
    const code = bytes(row.hex), target = 0x2000 - code.length, c = open(resident, entries, row.code);
    h.upload(c, target, code); bootstrap(c, target, row.d16);
    strict(c, 'diagnostic decoder retains valid unsupported refusal', target, code.length, 10);
    const material = stopped(c, 'exact qualified feature metadata captures UD'), input = seed(target, row);
    invoke(c, `${row.tag}: absence UD precedes GPR/vector/flags mutation, nullDS/FS, shortSS or memory`, material,
      input, expected(input, 20, 0), {budget: 2}); h.host(c, 'close');
  }
  {
    const code = bytes('c4e379dec100'), target = 0x1ffb, c = open(resident, entries, 'SM3 final immediate repair');
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
    strict(c, 'strict final-immediate fetch remains a neutral refusal', target, code.length, 10);
    const fetchMaterial = h.fetchFault(c, 'missing immediate cannot classify absence', {pc: target, length: code.length, address: 0x2000, errorCode: 20});
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, code.subarray(-1)); h.host(c, 'protect', [0x2000, 1, 1]);
    h.fetchFault(c, 'READ without EXECUTE still cannot classify SM3', {pc: target, length: code.length, address: 0x2000, errorCode: 21, material: fetchMaterial});
    h.host(c, 'protect', [0x2000, 1, 7]); h.reachNeedCode(c); const material = stopped(c, 'full executable immediate selects absence UD');
    const input = seed(target); invoke(c, 'complete SM3 faults before vector, flags or operand effects', material,
      input, expected(input, 20, 0), {budget: 2}); h.host(c, 'close');
  }
  for (const row of [
    {hex: 'f2f20f38fc08', label: 'duplicate RAO mandatory prefix remains refused'},
    {hex: 'cd80', label: 'INT80 precise denied application vector'},
    {hex: 'f0c4e27950c1', label: 'invalid LOCK VNNI remains decoder UD', invalid: true},
  ]) {
    const code = bytes(row.hex), target = 0x1100, c = open(resident, entries, row.label);
    h.upload(c, target, code); bootstrap(c, target);
    if (row.hex === 'cd80') {
      const material = strict(c, 'original INT80 bytes bind the application interrupt instruction', target, code.length, 0);
      const input = seed(target), want = expected(input, 11, 0); want.set(record('R3EX', 40, [11, 0, 0x402, 0, 0, 0], 6), 56);
      invoke(c, 'INT80 retains precise GP(IDT vector80), original PC and all operands', material, input, want, {budget: 2});
      h.host(c, 'close'); continue;
    }

    strict(c, 'strict prefix or neighbor refusal retained', target, code.length, 10);
    const material = stopped(c, row.label, row.invalid ? 0 : 10);
    if (row.invalid) {const input = seed(target); invoke(c, 'invalid LOCK keeps existing UD outcome', material,
      input, expected(input, 20, 0), {budget: 2});}
    h.host(c, 'close');
  }
  {
    const c = open(resident, entries, 'existing VEX128 AES positive'), target = 0x1100;
    h.upload(c, target, bytes('c4e279dcc1ebfe')); h.put(c, 0, seed(target));
    const helper = resident ? 'avx_execute_resident' : 'avx_execute';
    const material = strict(c, 'narrow AES remains available without modern feature metadata', target, 7, 0, [helper]);
    const input = seed(target, {flags: FLAGS | RF}); input.fill(0, 4396, 4428);
    const want = expected(input, 1, 1, target + 5, 6); want.fill(0x63, 4396, 4412); want.fill(0, 4628, 4644);
    want.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0], 7), 100); want.fill(0, 652, 684); want.writeUInt32LE(FLAGS, 656);
    const args = [...(resident && material ? material.binding : []), 0x01001002, 0x40, 0, 0, 0, FLAGS];
    invoke(c, 'literal AES zero-state round yields sixteen63 bytes, typed packet and clear destination upper', material,
      input, want, {budget: 1, helpers: [{name: helper, args, status: 0}]}); h.host(c, 'close');
  }
}
assert.equal(contexts, 64); assert.equal(planned, 120); assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'modern-counts.json'), JSON.stringify({contexts, planned, invoked, selectedTags: selected.map(row => row.tag),
  originalCompileObservationPlan: 192, originalRowPlan: 308, sourcePins}, null, 2) + '\n');
h.finish([
  'exact nine primary-qualified absent feature tags only, with all64 mode32 Codes selected by source-local tests and metadata census',
  'four real replacement/resident by extent/entry profiles; literal full4796 arena, every mapped RAM page, real guard/helper packets and retirement',
  'each feature has an actual representative; nullFS D16 memory, m16 broadcast and shortSS RAO operands discriminate accidental accesses',
  'complete executable immediate fetch before SM3 absence, retained duplicate RAO prefix and neutral neighbor refusals, invalid LOCK UD',
  'real VEX128 AES zero-state positive retained; unchanged AVX2/FMA3/PCLMUL positives have direct source-local controls and their existing actual suites',
  'generic UD budget/cancel/currentness/liveCS/flags evidence is reused because no IR/emitter/ABI/lifecycle owner changes',
  'CPUID, environment and helper wire unchanged; no arithmetic/conversion/crypto/remote-atomic success facility or generalVEX/CPU-completeness claim',
]);
