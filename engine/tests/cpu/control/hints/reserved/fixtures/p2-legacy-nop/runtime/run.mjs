import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit selected engine, output and source root');
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, state, record, exit, hash} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/hints/reserved/';
const paths = [own + 'wasm.rs', own + 'fixtures/p2-legacy-nop/runtime/run.mjs',
  'engine/tests/cpu/control/hints/native/nop.rs', 'engine/tests/cpu/control/native.rs',
  'engine/tests/cpu/control/wasm.rs', 'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/x86/decode/profile/tests/nop.rs',
  'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs',
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', support];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), RF = 0x10000, TF = 0x100, FLAGS = 0x244ed7, TARGET = 0x1100;
const regs = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0x7000, 0xfffffffc, 0xfffffffb, 0xfffffffa];
const bytes = hex => Buffer.from(hex, 'hex');
// These ten literal byte strings select all ten exact default-decoder Codes.
const selected = [
  {code: 'Reservednop_rm32_r32_0F19', hex: '0f19c0'},
  {code: 'Reservednop_rm16_r16_0F19', hex: '660f19c0'},
  {code: 'Reservednop_rm32_r32_0F1C', hex: '640f1c8c88f0ffffff'},
  {code: 'Reservednop_rm16_r16_0F1C', hex: '64660f1c8c88f0ffffff'},
  {code: 'Reservednop_rm32_r32_0F1D', hex: '0f1dc0'},
  {code: 'Reservednop_rm16_r16_0F1D', hex: '660f1dc0'},
  {code: 'Reservednop_rm32_r32_0F1E', hex: '0f1ec0'},
  {code: 'Reservednop_rm16_r16_0F1E', hex: '660f1ec0'},
  {code: 'Reservednop_rm32_r32_0F1F', hex: '0f1fc8'},
  {code: 'Reservednop_rm16_r16_0F1F', hex: '660f1fc8'},
];
assert.equal(new Set(selected.map(row => row.code)).size, 10);
const controls = [
  {code: 'D16 default word alias', hex: '0f19c0', d16: true},
  {code: 'D16 operand32/address32 nullFS alias', hex: '6466670f1d8c88f0ffffff', d16: true},
  {code: 'canonical NOP memory operand ignored', hex: '0f1f00'},
  {code: 'RDSSPD concrete F3 overlay with CET disabled', hex: 'f30f1ec8'},
  {code: 'ENDBR32 concrete F3 overlay', hex: 'f30f1efb'},
  {code: 'CLDEMOTE concrete memory overlay', hex: '0f1c00'},
];
const refused = [
  ['unconsumed REPNE', 'f20f19c0'],
  ['consumed REP must not bypass raw guard', 'f30f19c0'],
  ['duplicate operand prefix', '66660f1dc0'],
  ['invalid LOCK', 'f00f1fc8'],
  ['unselected 0F0D register form', '0f0dc0'],
  ['unqualified 0F18 reserved memory form', '0f1820'],
];
let contexts = 0, plannedCompiles = 0, plannedRefusals = 0, plannedInvocations = 0, invoked = 0;
function seed(pc, {d16 = false, flags = FLAGS | RF, shadow = 1, cancel = 0} = {}) {
  const input = state(h.initial, {pc, flags, regs, cancel}); input.writeUInt32LE(shadow, 12);
  // Null DS/FS and short SS make accidental data or segment checks fail visibly.
  const sg = record('R3SG', 64, [], d16 ? 3 : 2);
  [0x23, 0x1b, 0x23, 0, 0, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + i * 2));
  [0xf33fffff, d16 ? 0xf1bfffff : 0xf3bfffff, 0xf23000ff, 0, 0, 0]
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
  if (retired !== 0) {want.writeUInt32LE(input.readUInt32LE(52) & ~RF, 52); want.writeUInt32LE(0, 12);}
  return exit(want, reason, retired, reason >= 16 ? 7 : 1);
}
function open(resident, entries, label) {
  contexts++; const c = h.open(resident, entries, label, [[PC, 7], [0x5000, 3], [0x7000, 3]]);
  h.upload(c, 0x5000, Buffer.alloc(4096, 0x5a)); h.upload(c, 0x7000, Buffer.alloc(4096, 0xa5)); return c;
}
function compile(c, label, pc, length, status = 0) {
  plannedCompiles++; if (status !== 0) plannedRefusals++;
  return h.compile(c, label, [[pc, length]], [], {expectedStatus: status});
}
function invoke(c, label, material, input, want, options = {}) {
  plannedInvocations++;
  if (material) {invoked++; return h.observe(c, label, {material, input, want, ...options});}
  h.skip(c, label, 'required preparation admission'); return null;
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const [index, row] of [...selected, ...controls].entries()) {
    const code = bytes(row.hex), body = Buffer.concat([code, bytes('ebfe')]);
    const c = open(resident, entries, row.code); h.upload(c, TARGET, body); h.put(c, 0, seed(TARGET, row));
    const material = compile(c, 'complete literal NOP form compiles with only the real currentness guard', TARGET, body.length);
    const input = seed(TARGET, row);
    invoke(c, 'NOP retires once, ignores all encoded operands and preserves full non-lifecycle state', material, input,
      expected(input, 1, 1, TARGET + code.length), {budget: 1});
    if (index === 0) {
      for (const cancel of [0, 1]) {
        const idle = seed(TARGET, {flags: FLAGS | RF | TF, cancel});
        invoke(c, 'zero budget or cancellation begins no instruction and preserves RF/shadow', material, idle,
          expected(idle, cancel ? 2 : 1, 0), {budget: 0, kind: 'control'});
      }
      const trap = seed(TARGET, {flags: FLAGS | RF | TF, shadow: 0});
      invoke(c, 'ordinary NOP retires before TF trap and consumes RF', material, trap,
        expected(trap, 16, 1, TARGET + code.length), {budget: 2, kind: 'control'});
    }
    h.host(c, 'close');
  }
  for (const [label, hex] of refused) {
    const code = bytes(hex), c = open(resident, entries, label); h.upload(c, TARGET, Buffer.concat([code, bytes('ebfe')]));
    h.put(c, 0, seed(TARGET)); compile(c, label + ' retains neutral strict refusal', TARGET, code.length + 2, 10);
    h.host(c, 'close');
  }
  {
    const code = bytes('640f1c8c88f0ffffff'), target = 0x2000 - code.length + 1;
    const c = open(resident, entries, 'full SIB/displacement fetch and last-byte source identity');
    h.upload(c, target, code.subarray(0, -1)); h.put(c, 0, seed(target));
    compile(c, 'missing final displacement byte cannot become an opcode-only NOP', target, code.length + 2, 10);
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, Buffer.concat([code.subarray(-1), bytes('ebfe')])); h.host(c, 'protect', [0x2000, 1, 1]);
    compile(c, 'READ without EXECUTE cannot supply the final displacement', target, code.length + 2, 10);
    h.host(c, 'protect', [0x2000, 1, 7]);
    const material = compile(c, 'full executable repair admits the original complete NOP', target, code.length + 2);
    const input = seed(target);
    invoke(c, 'repaired NOP consumes its complete original length with no operand effects', material, input,
      expected(input, 1, 1, target + code.length), {budget: 1});
    h.upload(c, 0x2000, code.subarray(-1));
    invoke(c, 'identity write to final displacement invalidates held physical material before RF/state change',
      material, input, input, {status: 4, budget: 1, kind: 'physical-currentness'});
    h.host(c, 'close');
  }
}
assert.equal(contexts, 92); assert.equal(plannedCompiles, 100); assert.equal(plannedRefusals, 32);
assert.equal(plannedInvocations, 84); assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'legacy-nop-counts.json'), JSON.stringify({contexts, plannedCompiles, plannedRefusals,
  plannedInvocations, invoked, totalObservations: plannedCompiles + plannedInvocations,
  selectedCodes: selected.map(row => row.code), sourcePins}, null, 2) + '\n');
h.finish([
  'Selected AMD-compatible exact ten default-decoder ModRM NOP Codes; current Ring3Engine identity and CPUID unchanged, no universal Intel reserved-NOP claim',
  'Four real replacement/resident by extent/entry profiles; Rust-produced full4796 arena, all mapped RAM and exact real guard-only imports/calls',
  'All ten literal Codes, D16 independent operand/address defaults, nullDS/FS, shortSS, poisoned GPRs and numeric sentinels; no data/segment helper or flags effect',
  'Last SIB displacement missing/nonEXEC/EXEC repair and identity-write invalidation retain full encoded fetch and physical snapshot authority',
  'Concrete canonical NOP/RDSSPD/ENDBR32/CLDEMOTE owners stay positive; repeat/duplicate/LOCK and unselected0F0D/0F18 remain neutral strict refusals',
  'Existing generic invalid-LOCK guestUD, CS-limit and fault budget/cancel/currentness proof reused; focused ordinary NOP RF/TF and zero-budget/cancel checks included',
  'No broad Reservednop, MPX or prefetch classifier, enabled CET semantics, browser/game or exhaustive CPU claim',
]);
