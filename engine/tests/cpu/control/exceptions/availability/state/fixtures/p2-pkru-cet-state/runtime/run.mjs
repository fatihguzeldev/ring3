import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit selected engine, output and repository root');
const supportPath = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, words, record, state, exit, hash} = await import(pathToFileURL(join(root, supportPath)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/exceptions/availability/state/';
const paths = ['engine/src/cpu/x86/environment.rs', 'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/x86/decode/execution_faults/state.rs',
  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/dbt/wasm/instructions/control_instructions.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs',
  'engine/src/process/execution/stopped.rs', 'engine/src/abi/x86/state.rs',
  'engine/tests/cpu/control/wasm.rs', own + 'wasm.rs',
  own + 'fixtures/p2-pkru-cet-state/runtime/run.mjs', supportPath];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), RF = 0x10000, TF = 0x100, FLAGS = 0x244cd7;
const poison = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0, 0xfffffffc, 0xfffffffb, 0xfffffffa];
const bytes = hex => Buffer.from(hex, 'hex');
const core = [
  {code: 'Rdpkru', hex: '0f01ee', states: [{1: 0}, {1: 1}, {1: 0xffffffff}]},
  {code: 'Wrpkru', hex: '0f01ef', states: [{1: 0, 2: 0}, {1: 1, 2: 0}, {1: 0, 2: 1}, {1: 0xffffffff, 2: 0xffffffff}]},
  {code: 'Incsspd_r32', hex: 'f30faee8', states: [{0: 0}, {0: 255}]},
  {code: 'Incsspd_r32', hex: 'f30faeec', states: [{4: 1}, {4: 0xffffffff}]},
];
const canonical = core.slice(0, 3);
assert.deepEqual([...new Set(core.map(row => row.code))], ['Rdpkru', 'Wrpkru', 'Incsspd_r32']);
let planned = 0, invoked = 0, contexts = 0;
function seed(pc, {set = {}, flags = FLAGS | RF | TF, cancel = 0, shadow = 1} = {}) {
  const regs = [...poison];
  for (const [index, value] of Object.entries(set)) regs[Number(index)] = value;
  const input = state(h.initial, {pc, regs, flags, cancel}); input.writeUInt32LE(shadow, 12);
  const sg = record('R3SG', 64, [], 2);
  [0x23, 0x1b, 0x23, 0, 0x3b, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + i * 2));
  [0xf33fffff, 0xf3bfffff, 0xf2300fff, 0, 0xf33fffff, 0]
    .forEach((value, i) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  input.set(sg, SEG);
  for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {
    input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
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
  h.skip(c, label, 'required capture admission'); return null;
}
function open(resident, entries, label) {
  contexts++;
  const c = h.open(resident, entries, label, [[PC, 7], [0x5000, 3], [0x7000, 3]]);
  h.upload(c, 0x5000, Buffer.alloc(4096, 0x5a)); h.upload(c, 0x7000, Buffer.alloc(4096, 0xa5));
  return c;
}
function bootstrap(c, target) {
  const jump = Buffer.alloc(5); jump[0] = 0xe9; jump.writeInt32LE(target - PC - 5, 1);
  h.upload(c, PC, jump); const input = seed(PC, {flags: FLAGS | RF}); h.put(c, 0, input);
  const material = h.compile(c, 'real bootstrap capture', [[PC, 5]]);
  invoke(c, 'real branch retires before canonical code miss', material, input, expected(input, 3, 1, target), {budget: 2});
}
function stopped(c, label, expectedStatus = 0) {
  h.put(c, 140, words([0xdeadbeef, 0xffffffff]));
  return h.compile(c, label, [], [], {stopped: true, expectedStatus});
}
function cpuid(c, phase) {
  const pc = 0x1100, input = seed(pc, {flags: FLAGS | RF, set: {0: 7, 1: 0}});
  h.upload(c, pc, bytes('0fa2ebfe')); h.put(c, 0, input);
  const material = h.compile(c, `${phase} fixed CPUID capture`, [[pc, 4]]), want = expected(input, 1, 1, pc + 2);
  [0, 0x128, 0x400000, 0].forEach((value, i) => want.writeUInt32LE(value, 16 + [0, 3, 1, 2][i] * 4));
  invoke(c, `${phase} PKU OSPKE CET_SS CET_IBT stay absent in literal full leaf7`, material, input, want);
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of core) {
    const code = bytes(row.hex), target = 0x2000 - code.length, c = open(resident, entries, row.code + '/' + row.hex);
    if (row.code === 'Wrpkru') cpuid(c, 'before');
    h.upload(c, target, code); bootstrap(c, target);
    h.compile(c, 'exact code remains a strict public refusal', [[target, code.length]], [], {expectedStatus: 10});
    const material = stopped(c, 'disabled immutable state captures exact execution UD');
    for (const set of row.states) {
      const input = seed(target, {set});
      invoke(c, `${row.code} ${JSON.stringify(set)}: UD before reserved checks, range, SSP or TF`, material,
        input, expected(input, 20, 0), {budget: 2});
    }
    for (const cancel of [0, 1]) {
      const input = seed(target, {cancel});
      invoke(c, `${row.code}: ${cancel ? 'cancel' : 'zero budget'} preserves RF and inherited shadow`, material,
        input, expected(input, cancel ? 2 : 1, 0), {budget: 0, kind: 'control'});
    }
    h.upload(c, target, bytes('90')); const stale = seed(target);
    invoke(c, `${row.code}: stale physical source rejects before any publication`, material, stale, stale,
      {status: 4, kind: 'physical-currentness'});
    if (row.code === 'Wrpkru') cpuid(c, 'after');
    h.host(c, 'close');
  }
  for (const row of canonical) {
    const code = bytes(row.hex), target = 0x2000 - code.length + 1, c = open(resident, entries, row.code + ' fullfetch');
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target);
    h.compile(c, 'strict fullfetch lacks final byte', [[target, code.length]], [], {expectedStatus: 10});
    stopped(c, 'missing final byte prevents state fault classification', 10);
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, code.subarray(-1)); h.host(c, 'protect', [0x2000, 1, 1]);
    stopped(c, 'READ without EXECUTE cannot classify state fault', 10);
    h.host(c, 'protect', [0x2000, 1, 7]);
    const material = stopped(c, 'executable final-byte repair captures real UD'), input = seed(target);
    invoke(c, `${row.code}: complete executable instruction selects UD`, material, input, expected(input, 20, 0), {budget: 2});
    h.host(c, 'close');
  }
  for (const row of canonical) {
    const code = Buffer.concat([bytes('f0'), bytes(row.hex)]), target = 0x1200;
    const c = open(resident, entries, row.code + ' invalid LOCK'); h.upload(c, target, code); bootstrap(c, target);
    h.compile(c, 'LOCK form remains strict InvalidEncoding refusal', [[target, code.length]], [], {expectedStatus: 10});
    const material = stopped(c, 'LOCK encoding uses existing invalid-instruction UD'), input = seed(target);
    invoke(c, `${row.code}: invalid LOCK stays UD with no state/helper effects`, material, input, expected(input, 20, 0), {budget: 2});
    h.host(c, 'close');
  }
  {
    const c = open(resident, entries, 'RDSSPD separate compatibility NOP'), target = 0x1200;
    h.upload(c, target, bytes('f30f1ec8ebfe')); h.put(c, 0, seed(target));
    const material = h.compile(c, 'RDSSPD remains strictly admitted NOP', [[target, 6]]);
    for (const flags of [FLAGS | RF, FLAGS | RF | TF]) {
      const input = seed(target, {flags, shadow: 0}), reason = flags & TF ? 16 : 1;
      invoke(c, 'RDSSPD NOP preserves EAX; only completed TF produces trap', material,
        input, expected(input, reason, 1, target + 4), {budget: flags & TF ? 2 : 1});
    }
    h.host(c, 'close');
  }
}
assert.equal(contexts, 44, 'finite contexts follow the literal family/profile rows');
assert.equal(planned, 172, 'finite invocations follow semantic, fetch, prefix, safepoint, stale and CPUID paths');
assert.deepEqual(pins(), sourcePins, 'contract oracles and source owners remain pinned');
writeFileSync(join(output, 'pkru-cet-counts.json'), JSON.stringify({contexts, planned_module_invocations: planned,
  admitted_module_invocations: invoked, codes: [...new Set(core.map(row => row.code))], source_pins: sourcePins}, null, 2) + '\n');
h.finish([
  'three exact protected32 state-disabled Codes; immutable CR4.PKE0/CR4.CET0 and unchanged literal CPUID absence',
  'four replacement/resident by extent/entry profiles use real guards; no synthetic helper or guest success state',
  'WRPKRU ECX/EDX reserved combinations, RDPKRU ECX, INCSSPD range0 and ESP source all execute independently',
  'literal full arena and every mapped RAM page, exact empty operand-helper schedules, RF/TF/shadow/fetch/currentness controls',
  'RDSSPD compatibility NOP remains separate; no PKRU/SSP/MSR authority fabricated, no broader CET/PKU or CPU completion claim',
]);
