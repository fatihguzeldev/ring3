import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit selected engine, output and source root');
const supportPath = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, words, record, state, exit, hash} = await import(pathToFileURL(join(root, supportPath)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/hints/prefetch/';
const paths = [
  'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/capabilities/native.rs',
  'engine/src/cpu/x86/environment.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/x86/decode/lower.rs', 
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/code_fetch.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/process/execution/stopped.rs',
  'engine/tests/cpu/control/native.rs', 'engine/tests/cpu/control/wasm.rs',
  own + 'native.rs', own + 'wasm.rs', own + 'fixtures/p2-data-prefetch/runtime/run.mjs',
  'engine/tests/cpu/profile/environment/cpuid/fixtures/p2-cpuid/runtime/oracle.mjs',
  'engine/tests/cpu/profile/integer/fixtures/p2-integer-completion/runtime/run.mjs', supportPath,
];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), bytes = hex => Buffer.from(hex, 'hex');
const RF = 0x10000, TF = 0x100, FLAGS = 0x244cd7, TARGET = 0x1100;
const poison = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0x2000, 0xfffffffc, 0xfffffffb, 0xfffffffa];
const families = [
  {name: 'PREFETCH', modrm: '00', absolute: '05'},
  {name: 'PREFETCHW', modrm: '08', absolute: '0d'},
];
const forms = families.flatMap(family => [
  {name: family.name, hex: `0f0d${family.modrm}`, canonical: true},
  {name: `${family.name}/operand16`, hex: `660f0d${family.modrm}`},
  {name: `${family.name}/address16`, hex: `670f0d${family.modrm}`},
  {name: `${family.name}/null-FS`, hex: `640f0d${family.modrm}`},
  {name: `${family.name}/limited-SS`, hex: `360f0d${family.modrm}`},
  {name: `${family.name}/absolute-unmapped`, hex: `0f0d${family.absolute}ffffffff`},
  {name: `${family.name}/independent-66-67`, hex: `66670f0d${family.modrm}`},
]);
const aliases = [
  {name: 'absent-WT1 /2', hex: '0f0d10', trap: true},
  {name: 'reserved /3 null FS', hex: '640f0d18'},
  {name: 'reserved /4 limited SS', hex: '360f0d20'},
  {name: 'reserved /5 absolute unmapped', hex: '0f0d2dffffffff'},
  {name: 'reserved /6 independent operand/address prefixes', hex: '66670f0d30'},
  {name: 'reserved /7 D16 nonflat CS override', hex: '2e0f0d38', d16: true, csBase: 0x4000},
];
const refusals = [
  ['register /2 remains outside memory aliases', '0f0dd0'],
  ['register /0 remains reserved', '0f0dc0'],
  ['register /1 remains reserved', '0f0dc8'],
  ['repeat-ne W remains profile refusal', 'f20f0d08'],
  ['repeat /0 remains profile refusal', 'f30f0d00'],
  ['duplicate operand prefix remains profile refusal', '66660f0d08'],
  ['register /3 remains outside memory aliases', '0f0dd8'],
  ['absent-WT1 repeat-ne remains profile refusal', 'f20f0d10'],
  ['alias /3 repeat remains profile refusal', 'f30f0d18'],
  ['alias /4 duplicate operand prefix remains profile refusal', '66660f0d20'],
];
let planned = 0, invoked = 0;
function seed(pc, {flags = FLAGS | RF, cancel = 0, shadow = 0, regs = poison, csBase = 0, d16 = false, csLimit} = {}) {
  const input = state(h.initial, {pc, flags, regs, cancel}); input.writeUInt32LE(shadow, 12);
  for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {
    input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
  }
  // null ds/fs and a short ss make an accidental operand-authority check observable.
  const current = csBase !== 0 || d16, sg = record('R3SG', 64, [], current ? 3 : 2);
  [0x23, 0x1b, 0x23, 0, 0, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + i * 2));
  const code = csLimit === undefined ? d16 ? 0xf1bfffff : 0xf3bfffff : 0xf2b00000 | csLimit;
  [0xf33fffff, code, 0xf23000ff, 0, 0, 0].forEach((value, i) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  input.set(sg, SEG); input.set(current ? record('R3SB', 40, [0, csBase, 0, 0, 0, 0]) : Buffer.alloc(40), BASES);
  return input;
}
function expected(input, reason, retired, pc = input.readUInt32LE(48), began = retired !== 0 || reason === 20) {
  const want = Buffer.from(input); want.writeUInt32LE(pc, 48);
  if (began) {want.writeUInt32LE(input.readUInt32LE(52) & ~RF, 52); want.writeUInt32LE(0, 12);}
  return exit(want, reason, retired, reason >= 16 ? 7 : reason >= 11 ? 6 : 1);
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
function strict(c, label, pc, length, expectedStatus = 0) {
  return h.compile(c, label, [[pc, length]], [], {expectedStatus});
}
function bootstrap(c, target) {
  const jump = Buffer.alloc(5); jump[0] = 0xe9; jump.writeInt32LE(target - PC - 5, 1);
  h.upload(c, PC, jump); const input = seed(PC); h.put(c, 0, input);
  const material = strict(c, 'capture real branch to missing code', PC, 5);
  invoke(c, 'actual JMP retires before NeedCode', material, input, expected(input, 3, 1, target), {budget: 2});
}
function stopped(c, label, expectedStatus = 0) {
  h.put(c, 140, words([0xdeadbeef, 0xffffffff]));
  return h.compile(c, label, [], [], {stopped: true, expectedStatus});
}
function retireCases(c, material, code, options = {}) {
  for (const flags of [FLAGS, FLAGS | RF, FLAGS | RF | TF]) {
    const input = seed(TARGET, {flags, ...options}), reason = flags & TF ? 16 : 1;
    invoke(c, 'hint retires once with exact flags and no operand or numeric helper', material, input,
      expected(input, reason, 1, TARGET + code.length), {budget: flags & TF ? 2 : 1});
  }
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of forms) {
    const code = bytes(row.hex), c = open(resident, entries, row.name);
    h.upload(c, TARGET, Buffer.concat([code, bytes('ebfe')])); h.put(c, 0, seed(TARGET));
    const material = strict(c, 'exact supported hint strictly compiles', TARGET, code.length + 2);
    retireCases(c, material, code);
    if (row.canonical) {
      for (const cancel of [0, 1]) {
        const input = seed(TARGET, {flags: FLAGS | RF | TF, shadow: 1, cancel});
        invoke(c, 'zero budget or cancellation preserves RF and pending shadow', material, input,
          expected(input, cancel ? 2 : 1, 0), {budget: 0, kind: 'control'});
      }
      const input = seed(TARGET); h.upload(c, TARGET + code.length - 1, bytes('01'));
      invoke(c, 'changed consumed ModRM rejects held material before RF or state publication', material,
        input, input, {status: 4, kind: 'physical-currentness'});
    }
    h.host(c, 'close');
  }
  for (const row of aliases) {
    const code = bytes(row.hex), c = open(resident, entries, row.name);
    if (row.csBase) {
      h.host(c, 'protect', [0x5000, 1, 7]);
      h.upload(c, TARGET, bytes('b8efbeaddeebfe'));
    }
    h.upload(c, (row.csBase || 0) + TARGET, Buffer.concat([code, bytes('ebfe')]));
    const input = seed(TARGET, {flags: FLAGS | RF | (row.trap ? TF : 0),
      csBase: row.csBase || 0, d16: row.d16 || false});
    h.put(c, 0, input);
    const material = strict(c, 'exact memory compatibility alias strictly compiles', TARGET, code.length + 2);
    invoke(c, 'alias retires once with full flags and no operand or numeric effects', material, input,
      expected(input, row.trap ? 16 : 1, 1, TARGET + code.length), {budget: row.trap ? 2 : 1});
    h.host(c, 'close');
  }
  for (const modrm of ['10', '18']) {
    const c = open(resident, entries, `memory alias ${modrm}/illegal LOCK`), invalid = bytes(`f00f0d${modrm}`);
    h.upload(c, TARGET, invalid); bootstrap(c, TARGET);
    strict(c, 'invalid alias LOCK remains a strict refusal', TARGET, invalid.length, 10);
    const material = stopped(c, 'execution captures existing invalid-LOCK UD');
    const input = seed(TARGET, {flags: FLAGS | RF | TF});
    invoke(c, 'alias LOCK UD precedes poisoned operands and TF; RF consumed, retired zero', material,
      input, expected(input, 20, 0), {budget: 2}); h.host(c, 'close');
  }
  {
    const code = bytes('0f0d15ffffffff'), target = 0x2000 - code.length + 1;
    const c = open(resident, entries, 'absent-WT1 alias/final-byte fetch repair');
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
    strict(c, 'missing final byte prevents alias strict classification', target, code.length, 10);
    const fetchMaterial = h.fetchFault(c, 'missing final byte prevents alias execution preparation', {pc: target, length: code.length, address: 0x2000, errorCode: 20});
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, Buffer.concat([code.subarray(-1), bytes('ebfe')]));
    h.host(c, 'protect', [0x2000, 1, 1]); h.fetchFault(c, 'READ without EXECUTE cannot classify alias', {pc: target, length: code.length, address: 0x2000, errorCode: 21, material: fetchMaterial});
    h.host(c, 'protect', [0x2000, 1, 7]); h.reachNeedCode(c); const material = stopped(c, 'full executable repair admits exact alias');
    const input = seed(target, {flags: FLAGS | RF | TF});
    invoke(c, 'repaired alias retires without access to absolute unmapped operand', material,
      input, expected(input, 16, 1, target + code.length), {budget: 2}); h.host(c, 'close');
  }
  for (const family of families) {
    const code = bytes(`0f0d${family.modrm}`);
    {
      const c = open(resident, entries, `${family.name}/D16`), input = seed(TARGET, {d16: true});
      h.upload(c, TARGET, Buffer.concat([code, bytes('ebfe')])); h.put(c, 0, input);
      const material = strict(c, 'current v3 D16 key admits the same exact hint', TARGET, code.length + 2);
      retireCases(c, material, code, {d16: true}); h.host(c, 'close');
    }
    {
      const c = open(resident, entries, `${family.name}/nonflat-CS-override`), based = bytes(`2e0f0d${family.modrm}`);
      h.host(c, 'protect', [0x5000, 1, 7]);
      h.upload(c, TARGET, bytes('b8efbeaddeebfe'));
      h.upload(c, 0x4000 + TARGET, Buffer.concat([based, bytes('ebfe')]));
      h.put(c, 0, seed(TARGET, {csBase: 0x4000}));
      const material = strict(c, 'physical CS source differs from logical decoy', TARGET, based.length + 2);
      retireCases(c, material, based, {csBase: 0x4000}); h.host(c, 'close');
    }
    {
      const c = open(resident, entries, `${family.name}/CS limit precedes instruction begin`);
      h.upload(c, TARGET, Buffer.concat([code, bytes('ebfe')])); h.put(c, 0, seed(TARGET));
      const material = strict(c, 'capture valid full code span before live limit change', TARGET, code.length + 2);
      for (const csLimit of [TARGET - 1, TARGET + code.length - 2]) {
        const input = seed(TARGET, {flags: FLAGS | RF | TF, shadow: 1, csLimit});
        invoke(c, 'first or final byte CS-limit GP preserves RF, registers, flags and pending shadow', material,
          input, expected(input, 11, 0), {budget: 2, kind: 'fetch-priority'});
      }
      h.host(c, 'close');
    }
    {
      const c = open(resident, entries, `${family.name}/illegal LOCK`), invalid = bytes(`f00f0d${family.modrm}`);
      h.upload(c, TARGET, invalid); bootstrap(c, TARGET);
      strict(c, 'invalid LOCK stays a strict refusal', TARGET, invalid.length, 10);
      const material = stopped(c, 'execution captures existing invalid-LOCK UD');
      const input = seed(TARGET, {flags: FLAGS | RF | TF});
      invoke(c, 'LOCK UD precedes poisoned operands and TF; RF consumed, retired zero', material,
        input, expected(input, 20, 0), {budget: 2}); h.host(c, 'close');
    }
    {
      const code = bytes(`0f0d${family.absolute}ffffffff`), target = 0x2000 - code.length + 1;
      const c = open(resident, entries, `${family.name}/final-byte fetch repair`);
      h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
      strict(c, 'missing final byte prevents strict classification', target, code.length, 10);
      const fetchMaterial = h.fetchFault(c, 'missing final byte prevents execution preparation', {pc: target, length: code.length, address: 0x2000, errorCode: 20});
      h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
      h.upload(c, 0x2000, Buffer.concat([code.subarray(-1), bytes('ebfe')]));
      h.host(c, 'protect', [0x2000, 1, 1]); h.fetchFault(c, 'READ without EXECUTE cannot classify hint', {pc: target, length: code.length, address: 0x2000, errorCode: 21, material: fetchMaterial});
      h.host(c, 'protect', [0x2000, 1, 7]); h.reachNeedCode(c); const material = stopped(c, 'full executable repair admits exact hint');
      const input = seed(target, {flags: FLAGS | RF | TF});
      invoke(c, 'repaired hint retires with no access to absolute unmapped operand', material,
        input, expected(input, 16, 1, target + code.length), {budget: 2}); h.host(c, 'close');
    }
  }
  for (const [label, hex] of refusals) {
    const c = open(resident, entries, label), code = bytes(hex);
    h.upload(c, TARGET, code); bootstrap(c, TARGET);
    strict(c, label + ': strict refusal is neutral', TARGET, code.length, 10);
    stopped(c, label + ': no broadened execution admission', 10); h.host(c, 'close');
  }
  {
    const c = open(resident, entries, 'explicit PREFETCH capability before and after ignored hint'), queryPC = 0x1300;
    h.upload(c, queryPC, bytes('0fa2ebfe')); h.upload(c, queryPC + 16, bytes('0fa2ebfe'));
    h.upload(c, TARGET, bytes('0f0d08ebfe')); h.upload(c, 0x1400, bytes('0f0d10ebfe'));
    const queries = [
      {leaf: 0, subleaf: 0, output: [13, 0x676e6952, 0x20656e69, 0x676e4533]},
      {leaf: 7, subleaf: 0, output: [0, 0x128, 0x400000, 0]},
      {leaf: 0x80000001, subleaf: 0xffffffff, output: [0, 0, 0x120, 0x08100000]},
    ];
    function query(phase) {
      // resident units retain distinct entry ownership across the two observations.
      const pc = phase === 'before' ? queryPC : queryPC + 16;
      h.put(c, 0, seed(pc)); const material = strict(c, `${phase} exact CPUID module`, pc, 4);
      for (const row of queries) {
        const regs = [...poison]; regs[0] = row.leaf; regs[1] = row.subleaf;
        const input = seed(pc, {regs}), want = expected(input, 1, 1, pc + 2);
        row.output.forEach((value, i) => want.writeUInt32LE(value >>> 0, 16 + [0, 3, 1, 2][i] * 4));
        invoke(c, `${phase} fixed vendor, unadvertised WT1 and supported data-prefetch bit`, material, input, want);
      }
    }
    query('before'); h.put(c, 0, seed(TARGET));
    const material = strict(c, 'ignored supported W between capability observations', TARGET, 5), input = seed(TARGET);
    invoke(c, 'ignored hint cannot mutate fixed capabilities', material, input, expected(input, 1, 1, TARGET + 3));
    h.put(c, 0, seed(0x1400));
    const alias = strict(c, 'absent-WT1 alias between unchanged capability observations', 0x1400, 5);
    const aliasInput = seed(0x1400);
    invoke(c, 'alias cannot enable WT1 or change virtual identity', alias, aliasInput,
      expected(aliasInput, 1, 1, 0x1403));
    query('after'); h.host(c, 'close');
  }
}
assert.equal(planned, 408, 'finite four-profile invocation plan');
assert.deepEqual(pins(), sourcePins, 'product, source, wrapper and independent literal oracles stay pinned');
writeFileSync(join(output, 'prefetch-counts.json'), JSON.stringify({planned_module_invocations: planned,
  admitted_module_invocations: invoked, selected_codes: ['Prefetch_m8', 'Prefetchw_m8', 'Prefetchwt1_m8', 'Prefetchreserved3_m8',
    'Prefetchreserved4_m8', 'Prefetchreserved5_m8', 'Prefetchreserved6_m8', 'Prefetchreserved7_m8'],
  compatibility_alias_types: [2, 3, 4, 5, 6, 7], wt1_enabled: false, source_pins: sourcePins}, null, 2) + '\n');
h.finish([
  'existing supported/ignored /0 and /1 plus AMD-defined /2..7 memory compatibility aliases; ECX8=1 and WT1 bit0 stay unchanged',
  'four real replacement/resident by extent/entry profiles; complete literal 4796-byte arena and every mapped RAM page',
  'real currentness guard is the only module helper; null DS/FS, short SS, unmapped/high operand, address/operand prefixes, D16 and nonflat CS cannot cause operand effects',
  'fullfetch and dynamic CS-limit checks precede hint retirement and RF clearing; executable-page repair is required for final-byte classification',
  'illegal LOCK keeps execution UD; repeat and duplicate-prefix refusals plus reserved register controls remain distinct',
  'Intel enabled-WT1 flags inconsistency remains unresolved; absent-WT1 alias does not enable WT1, generic 0F0D, physical cache, browser, game or throughput',
]);
