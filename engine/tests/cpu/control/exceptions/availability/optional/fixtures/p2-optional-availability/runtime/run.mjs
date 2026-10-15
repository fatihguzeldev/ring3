import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
import {unavailable, memoryForms, nops, repairs, refusals, invalidLocks, cpuidQueries} from './vectors.mjs';
import {tsxUnavailable, mpxNops, boundedRepairs, boundedRefusals, boundedInvalidLocks, boundedCompatibilityNops} from './tsxldtrk-mpx.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit selected engine, output and source root');
const supportPath = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, words, record, state, exit, hash} = await import(pathToFileURL(join(root, supportPath)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/exceptions/availability/optional/';
const paths = [
  'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/environment.rs',
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/x86/decode/execution_faults/optional.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/mod.rs', 'engine/src/cpu/x86/decode/mpx.rs',
  'engine/src/cpu/x86/decode/mpx/tests.rs', 'engine/src/cpu/x86/decode/execution_faults/tests/tsxldtrk.rs',
  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs', 'engine/src/cpu/x86/decode/simd.rs',
  'engine/src/cpu/x86/decode/avx/mod.rs', 'engine/src/cpu/x86/decode/avx/encoding.rs',
  'engine/src/cpu/dbt/wasm/instructions/control_instructions.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer_system.rs', 'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs',
  'engine/src/cpu/dbt/wasm/code_fetch.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs',
  'engine/src/process/execution/stopped.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/tests/cpu/control/wasm.rs', own + 'wasm.rs',
  own + 'fixtures/p2-optional-availability/runtime/run.mjs',
  own + 'fixtures/p2-optional-availability/runtime/vectors.mjs',
  own + 'fixtures/p2-optional-availability/runtime/tsxldtrk-mpx.mjs', supportPath,
];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), codeSet = new Set(unavailable.map(row => row.code));
assert.equal(codeSet.size, 31); assert.equal(unavailable.length, 31);
assert.equal(new Set(nops.map(row => row.code)).size, 2);
for (const row of memoryForms) assert.ok(codeSet.has(row.code));
const bytes = hex => Buffer.from(hex, 'hex'), RF = 0x10000, TF = 0x100, FLAGS = 0x244cd7;
const poison = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0x2000, 0xfffffffc, 0xfffffffb, 0xfffffffa];
let planned = 0, invoked = 0;
function seed(pc, {flags = FLAGS | RF | TF, shadow = 0, cancel = 0, regs = poison, operandPoison = true} = {}) {
  const input = state(h.initial, {pc, flags, regs, cancel}); input.writeUInt32LE(shadow, 12);
  for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {
    input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
  }
  if (operandPoison) {
    const sg = record('R3SG', 64, [], 2);
    [0x23, 0x1b, 0x23, 0, 0x3b, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + i * 2));
    [0xf33fffff, 0xf3bfffff, 0xf2300fff, 0, 0xf33fffff, 0]
      .forEach((value, i) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
    input.set(sg, SEG);
  }
  return input;
}
function expected(input, reason, retired, pc = input.readUInt32LE(48)) {
  const want = Buffer.from(input); want.writeUInt32LE(pc, 48);
  if (![1, 2].includes(reason) || retired > 0) {
    want.writeUInt32LE(input.readUInt32LE(52) & ~RF, 52); want.writeUInt32LE(0, 12);
  }
  return exit(want, reason, retired, reason >= 16 ? 7 : 1);
}
function invoke(c, label, material, input, want, options = {}) {
  planned++;
  if (material) {invoked++; return h.observe(c, label, {input, want, material, ...options});}
  h.skip(c, label, 'required compile admission'); return null;
}
function open(resident, entries, label) {
  const c = h.open(resident, entries, label, [[PC, 7], [0x5000, 3], [0x7000, 3]]);
  h.upload(c, 0x5000, Buffer.alloc(4096, 0x5a)); h.upload(c, 0x7000, Buffer.alloc(4096, 0xa5));
  return c;
}
function moduleBytes(c, material) {
  return Buffer.from(new Uint8Array(c.memory.buffer, material.pointer, material.length));
}
function strict(c, label, pc, length, expectedStatus) {
  const previous = c.material, body = previous && moduleBytes(c, previous);
  const result = h.compile(c, label, [[pc, length]], [], {expectedStatus});
  if (expectedStatus !== 0 && previous) {
    assert.equal(c.material, previous, 'strict refusal retains the same material binding');
    assert.deepEqual(moduleBytes(c, previous), body, 'strict refusal preserves exact prior module bytes');
  }
  return result;
}
function bootstrap(c, target) {
  const jump = Buffer.alloc(5); jump[0] = 0xe9; jump.writeInt32LE(target - PC - 5, 1);
  h.upload(c, PC, jump); const input = seed(PC, {flags: FLAGS | RF}); h.put(c, 0, input);
  const material = h.compile(c, 'capture real missing-code bootstrap', [[PC, 5]]);
  invoke(c, 'actual branch retires before canonical NeedCode', material, input, expected(input, 3, 1, target), {budget: 2});
  return material;
}
function stopped(c, label, expectedStatus = 0) {
  // the endpoint must use the actual stopped EIP, not these poisoned strict descriptors.
  h.put(c, 140, words([0xdeadbeef, 0xffffffff]));
  return h.compile(c, label, [], [], {stopped: true, expectedStatus});
}
function scheduleStops(c, material, pc, title) {
  for (const cancel of [0, 1]) {
    const input = seed(pc, {cancel, shadow: 1});
    invoke(c, `${title}: ${cancel ? 'cancel' : 'zero budget'} never begins instruction`, material,
      input, expected(input, cancel ? 2 : 1, 0), {budget: 0, kind: 'control'});
  }
}

for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of [...unavailable, ...memoryForms]) {
    const code = bytes(row.hex), target = 0x2000 - code.length;
    const c = open(resident, entries, `${row.code}/${row.variant ?? 'canonical'}`);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, 'exact optional Code retains public strict refusal', target, code.length, 10);
    const material = stopped(c, 'complete exact optional Code captures execution UD');
    const input = seed(target, {shadow: 1});
    invoke(c, 'UD precedes poisoned operand, TF and optional facility access; RF consumed', material,
      input, expected(input, 20, 0), {budget: 2});
    if (row.code === 'Adcx_r32_rm32' && !row.variant) {
      scheduleStops(c, material, target, 'UD capture');
      h.upload(c, target, bytes('90'));
      invoke(c, 'stale UD bytes reject held material before RF or fault publication', material,
        input, input, {status: 4, kind: 'physical-currentness'});
    }
    h.host(c, 'close');
  }
  for (const row of nops) {
    const code = bytes(row.hex), target = 0x1100, c = open(resident, entries, `${row.code}/${row.variant ?? 'canonical'} NOP`);
    h.upload(c, target, Buffer.concat([code, bytes('ebfe')]));
    h.put(c, 0, seed(target)); const material = strict(c, 'compatibility NOP strictly captures ordinary execution', target, code.length + 2, 0);
    for (const flags of [FLAGS, FLAGS | RF, FLAGS | RF | TF]) {
      const input = seed(target, {flags}), reason = flags & TF ? 16 : 1;
      invoke(c, 'NOP preserves destination, flags and poisoned operand without a helper', material,
        input, expected(input, reason, 1, target + code.length), {budget: flags & TF ? 2 : 1});
    }
    if (row.code === 'Cldemote_m8' && !row.variant) {
      scheduleStops(c, material, target, 'NOP capture');
      const input = seed(target); h.upload(c, target + code.length - 1, bytes('00'));
      invoke(c, 'stale consumed NOP displacement rejects before RF or destination publication', material,
        input, input, {status: 4, kind: 'physical-currentness'});
    }
    h.host(c, 'close');
  }
  for (const row of repairs) {
    const code = bytes(row.hex), target = 0x2000 - code.length + 1;
    const c = open(resident, entries, `${row.code} final-byte repair`);
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
    strict(c, 'truncated exact Code strict fetch remains a host preparation error', target, code.length, 10);
    stopped(c, 'missing final byte prevents classification and publication', 10);
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, Buffer.concat([code.subarray(-1), bytes('ebfe')]));
    h.host(c, 'protect', [0x2000, 1, 1]);
    stopped(c, 'READ without EXECUTE still prevents classification and publication', 10);
    h.host(c, 'protect', [0x2000, 1, 7]);
    const material = stopped(c, 'only executable fullfetch repair admits exact outcome');
    const input = seed(target), retired = row.reason === 20 ? 0 : 1;
    invoke(c, 'repaired full instruction has exact UD or NOP without operand access', material,
      input, expected(input, row.reason === 1 ? 16 : 20, retired, target + retired * code.length), {budget: 2});
    h.host(c, 'close');
  }
  for (const row of [...refusals, ...invalidLocks]) {
    const code = bytes(row.hex), target = 0x1100, c = open(resident, entries, row.label);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, row.label + ': strict refuses', target, code.length, 10);
    const invalid = invalidLocks.includes(row), material = stopped(c,
      row.label + ': execution distinguishes invalid encoding from profile refusal', invalid ? 0 : 10);
    if (invalid) {
      const input = seed(target);
      invoke(c, 'invalid LOCK encoding captures existing UD, never NOP', material, input, expected(input, 20, 0), {budget: 2});
    }
    h.host(c, 'close');
  }
  {
    const c = open(resident, entries, 'immutable CPUID absence before and after real UD/NOP'), queryPC = 0x1300;
    h.upload(c, queryPC, bytes('0fa2ebfe')); h.upload(c, 0x1200, bytes('0f1c05ffffffffebfe'));
    h.upload(c, 0x1100, bytes('660f38f6c1'));
    function queries(phase) {
      h.put(c, 0, seed(queryPC)); const material = strict(c, `${phase} exact CPUID capture`, queryPC, 4, 0);
      for (const row of cpuidQueries) {
        const regs = [...poison]; regs[0] = row.leaf; regs[1] = row.subleaf;
        const input = seed(queryPC, {regs, flags: FLAGS | RF}), want = expected(input, 1, 1, queryPC + 2);
        row.output.forEach((value, i) => want.writeUInt32LE(value >>> 0, 16 + [0, 3, 1, 2][i] * 4));
        invoke(c, `${phase} CPUID ${row.leaf}.${row.subleaf} exact immutable absent bits`, material, input, want);
      }
    }
    queries('before');
    const nopInput = seed(0x1200, {flags: FLAGS | RF}); h.put(c, 0, nopInput);
    const nopMaterial = strict(c, 'actual NOP between CPUID observations', 0x1200, 9, 0);
    invoke(c, 'actual NOP cannot enable a virtual facility', nopMaterial, nopInput, expected(nopInput, 1, 1, 0x1207));
    bootstrap(c, 0x1100); const udMaterial = stopped(c, 'actual UD between CPUID observations');
    const udInput = seed(0x1100);
    invoke(c, 'actual unavailable instruction cannot enable a virtual facility', udMaterial, udInput, expected(udInput, 20, 0));
    queries('after'); h.host(c, 'close');
  }
}
// the original 1016-row group above is unchanged; this additive group owns only
// tsxldtrk absence and the finite canonical D32 mpx register compatibility cut.
assert.equal(tsxUnavailable.length, 2);
assert.equal(new Set(tsxUnavailable.map(row => row.code)).size, 2);
assert.equal(mpxNops.length, 32);
assert.equal(new Set(mpxNops.map(row => row.hex)).size, 32);
assert.equal(new Set(mpxNops.map(row => row.code)).size, 2);
const originalPlanned = planned;
function boundedFetch(c, material, target, length) {
  for (const [label, limit] of [['first byte', target - 1], ['last consumed byte', target + length - 2]]) {
    const input = seed(target, {shadow: 1});
    input.writeUInt32LE((0xf2b00000 | limit) >>> 0, SEG + 32);
    // cs authority fails before begin_instruction: RF and pending ss shadow survive.
    invoke(c, `${label} outside live CS limit precedes classification and RF consumption`, material,
      input, exit(Buffer.from(input), 11, 0, 6), {budget: 2, kind: 'logical-fetch-authority'});
  }
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of tsxUnavailable) {
    const code = bytes(row.hex), target = 0x2000 - code.length;
    const c = open(resident, entries, `${row.code} absent TSXLDTRK`);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, 'exact absent TSXLDTRK retains strict public refusal', target, code.length, 10);
    const material = stopped(c, 'complete TSXLDTRK captures absence UD');
    const input = seed(target, {shadow: 1});
    invoke(c, 'absent TSXLDTRK is UD, with no outside-transaction NOP or state effect', material,
      input, expected(input, 20, 0), {budget: 2});
    scheduleStops(c, material, target, row.code);
    boundedFetch(c, material, target, code.length);
    // both encodings have the same absent-ud result; currentness must still notice.
    h.upload(c, target + 3, bytes(row.code === 'Xsusldtrk' ? 'e9' : 'e8'));
    invoke(c, 'changed TSXLDTRK opcode rejects held material before RF/fault publication', material,
      input, input, {status: 4, kind: 'physical-currentness'});
    h.host(c, 'close');
  }
  for (const row of mpxNops) {
    const code = bytes(row.hex), target = 0x1100;
    const c = open(resident, entries, `${row.variant}/${row.code} disabled MPX`);
    h.upload(c, target, Buffer.concat([code, bytes('ebfe')])); h.put(c, 0, seed(target));
    const material = strict(c, 'canonical BND0..3 register form strictly captures ordinary NOP', target, 6, 0);
    for (const flags of [FLAGS, FLAGS | RF, FLAGS | RF | TF]) {
      const input = seed(target, {flags, shadow: 1}), reason = flags & TF ? 16 : 1;
      invoke(c, 'disabled canonical MPX retires one NOP with exact flags/registers/numeric state', material,
        input, expected(input, reason, 1, target + 4), {budget: flags & TF ? 2 : 1});
    }
    if (row.schedules) {
      scheduleStops(c, material, target, row.code);
      boundedFetch(c, material, target, 4);
      const input = seed(target, {shadow: 1}); h.upload(c, target + 3, bytes('c2'));
      invoke(c, 'changed canonical MPX ModRM rejects held material before NOP/flags publication', material,
        input, input, {status: 4, kind: 'physical-currentness'});
    }
    h.host(c, 'close');
  }
  for (const row of boundedRepairs) {
    const code = bytes(row.hex), target = 0x2000 - code.length + 1;
    const c = open(resident, entries, `${row.code} complete-fetch repair`);
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
    strict(c, 'missing final opcode or ModRM byte prevents strict preparation', target, code.length, 10);
    stopped(c, 'missing final byte prevents UD/NOP classification and publication', 10);
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, Buffer.concat([code.subarray(-1), bytes('ebfe')]));
    h.host(c, 'protect', [0x2000, 1, 1]);
    stopped(c, 'final byte readable without execute permission still prevents classification', 10);
    h.host(c, 'protect', [0x2000, 1, 7]);
    const material = stopped(c, 'executable final-byte repair admits exact absent-feature result');
    const input = seed(target, {shadow: 1}), retired = row.reason === 20 ? 0 : 1;
    invoke(c, 'fullfetch repair selects exact TSXLDTRK UD or canonical MPX NOP', material,
      input, expected(input, row.reason, retired, target + retired * code.length), {budget: 2});
    h.host(c, 'close');
  }
  for (const row of boundedCompatibilityNops) {
    const code = bytes(row.hex), target = 0x1100, c = open(resident, entries, row.label);
    h.upload(c, target, Buffer.concat([code, bytes('ebfe')])); h.put(c, 0, seed(target));
    const material = strict(c, row.label + ': complete source strictly captures NOP', target, code.length + 2, 0);
    for (const flags of [FLAGS, FLAGS | RF, FLAGS | RF | TF]) {
      const input = seed(target, {flags, shadow: 1}), reason = flags & TF ? 16 : 1;
      invoke(c, row.label + ': no operand helper, state or RAM effect', material, input,
        expected(input, reason, 1, target + code.length), {budget: flags & TF ? 2 : 1});
    }
    h.host(c, 'close');
  }
  for (const row of [...boundedRefusals, ...boundedInvalidLocks]) {
    const code = bytes(row.hex), target = 0x1100, c = open(resident, entries, row.label);
    h.upload(c, target, code); bootstrap(c, target);
    strict(c, row.label + ': strict preparation refuses neutrally', target, code.length, 10);
    const invalid = boundedInvalidLocks.includes(row), material = stopped(c,
      row.label + ': invalid encoding differs from bounded profile refusal', invalid ? 0 : 10);
    if (invalid) {
      const input = seed(target, {shadow: 1});
      invoke(c, 'illegal LOCK is the existing invalid-encoding UD, never absent-MPX NOP', material,
        input, expected(input, 20, 0), {budget: 2});
    }
    h.host(c, 'close');
  }
  {
    const c = open(resident, entries, 'MPX and TSXLDTRK absence stays immutable'), queryPC = 0x1300;
    h.upload(c, queryPC, bytes('0fa2ebfe'));
    h.upload(c, 0x1200, bytes('660f1bc1ebfe')); h.upload(c, 0x1100, bytes('f20f01e8'));
    function query(phase) {
      const regs = [...poison]; regs[0] = 7; regs[1] = 0;
      const input = seed(queryPC, {regs, flags: FLAGS | RF}); h.put(c, 0, input);
      const material = strict(c, `${phase} actual CPUID7.0`, queryPC, 4, 0), want = expected(input, 1, 1, queryPC + 2);
      [0, 0x128, 0x400000, 0].forEach((value, i) => want.writeUInt32LE(value, 16 + [0, 3, 1, 2][i] * 4));
      invoke(c, `${phase} CPUID7.0 has MPX EBX14 and TSXLDTRK EDX16 clear`, material, input, want);
    }
    query('before');
    const nopInput = seed(0x1200, {flags: FLAGS | RF, shadow: 1}); h.put(c, 0, nopInput);
    const nopMaterial = strict(c, 'real canonical disabled-MPX NOP between feature queries', 0x1200, 6, 0);
    invoke(c, 'canonical MPX NOP cannot create bounds state or enable MPX', nopMaterial,
      nopInput, expected(nopInput, 1, 1, 0x1204));
    bootstrap(c, 0x1100); const udMaterial = stopped(c, 'real absent TSXLDTRK between feature queries');
    const udInput = seed(0x1100, {shadow: 1});
    invoke(c, 'TSXLDTRK absence UD cannot create transaction or tracking state', udMaterial,
      udInput, expected(udInput, 20, 0));
    query('after'); h.host(c, 'close');
  }
}
assert.equal(originalPlanned, 432, 'all original optional module invocations remain');
assert.equal(planned - originalPlanned, 628, 'finite four-profile TSXLDTRK/MPX extension with four selected compatibility migrations');
assert.deepEqual(pins(), sourcePins, 'optional primary-backed oracles and all current execution sources stay pinned');
writeFileSync(join(output, 'optional-counts.json'), JSON.stringify({planned_module_invocations: planned,
  admitted_module_invocations: invoked, exact_unavailable_codes: [...codeSet], compatible_nop_codes: [...new Set(nops.map(row => row.code))],
  bounded_tsxldtrk_codes: tsxUnavailable.map(row => row.code), canonical_mpx_register_forms: mpxNops.length, migrated_mpx_compatibility_forms: boundedCompatibilityNops.length,
  canonical_mpx_decoder_codes: [...new Set(mpxNops.map(row => row.code))], source_pins: sourcePins}, null, 2) + '\n');
h.finish([
  'finite exact IA32 group: 31 absence-UD Codes and two absent-feature NOP Codes; no optional facility enabled',
  'all four replacement/resident by extent/entry profiles use real helpers, literal complete arena and every mapped RAM page',
  'actual JMP reaches canonical NeedCode before execution-only capture; strict preparation remains separate',
  'fullfetch repair rows establish host preparation neutrality until the final byte is executable; native companion owns exact Execute MemoryFault details, not an invented guest PF exit',
  'UD/NOP modules import only the real currentness guard; poisoned nullDS/limitedSS/unmapped operands and all numeric/register payloads remain untouched',
  'exact CPUID leaf7.0, Dh.1 and14h.0 values agree before and after real NOP/UD in the same instance; no hidden activation',
  'additive exact TSXLDTRK2 absence-UD and canonical D32 BNDMOV32 register-NOP group; MPX stays absent and no bounds or transaction state is synthesized',
  'PKRU/CET stateful faults, enabled MPX, prefetch/vendor/newer vector families remain outside the frozen group; no general optional-ISA completion claim',
]);
