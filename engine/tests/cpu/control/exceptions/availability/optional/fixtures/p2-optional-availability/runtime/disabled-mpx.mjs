import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const supportPath = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, words, record, state, exit, hash} = await import(pathToFileURL(join(root, supportPath)));
const h = await harness(enginePath, output, root);
const own = 'engine/tests/cpu/control/exceptions/availability/optional/';
const paths = ['engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/decode/mpx.rs',
  'engine/src/cpu/x86/decode/mpx/tests.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/lower.rs',  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs',
  'engine/src/cpu/dbt/wasm/code_fetch/hoist.rs', 'engine/src/cpu/dbt/wasm/abi.rs',
  own + 'wasm.rs', own + 'fixtures/p2-optional-availability/runtime/disabled-mpx.mjs', supportPath];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), bytes = hex => Buffer.from(hex, 'hex');
const RF = 0x10000, TF = 0x100, FLAGS = 0x244cd7;
const poison = [0xffffffff, 0xfffffffe, 0xfffffff9, 0xfffffffd, 0x2000, 0xfffffffc, 0xfffffffb, 0xfffffffa];
// all eight opcode/prefix families are exercised with memory syntax in both code defaults.
const families = [
  ['BNDLDX', '0f1a00'], ['BNDSTX', '0f1b00'], ['BNDCL', 'f30f1a00'],
  ['BNDCU', 'f20f1a00'], ['BNDCN', 'f20f1b00'], ['BNDMK', 'f30f1b00'],
  ['BNDMOV load', '660f1a00'], ['BNDMOV store', '660f1b00'],
];
const forms = [
  ...[false, true].flatMap(default32 => families.map(([label, hex]) => ({label, hex, default32}))),
  {label: 'D32 address16 disp16 final byte', hex: '67660f1a06ffff', default32: true},
  {label: 'D16 address32 SIB disp32', hex: '67660f1a848bffffff7f', default32: false},
  {label: 'D32 FS memory', hex: '64660f1a00', default32: true},
  {label: 'D16 SS memory outside cached SS', hex: '36660f1b00', default32: false},
  {label: 'D32 distinct 66/F2 prefixes', hex: '66f20f1ac1', default32: true},
  {label: 'D16 distinct F2/66 prefixes', hex: 'f2660f1ac1', default32: false},
  {label: 'D32 beyond BND3 disabled destination', hex: '660f1ae1', default32: true},
  {label: 'D16 beyond BND3 disabled source', hex: '660f1bc4', default32: false},
];
assert.equal(forms.length, 24);
let invoked = 0, planned = 0;
function seed(pc, {default32 = true, flags = FLAGS | RF, shadow = 1, cancel = 0} = {}) {
  const input = state(h.initial, {pc, flags, regs: poison, cancel}); input.writeUInt32LE(shadow, 12);
  for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {
    input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
  }
  const sg = record('R3SG', 64, [], default32 ? 2 : 3);
  [0x23, 0x1b, 0x23, 0, 0x3b, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + i * 2));
  [0xf33fffff, default32 ? 0xf3bfffff : 0xf1bfffff, 0xf2300fff, 0, 0xf33fffff, 0]
    .forEach((value, i) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  input.set(sg, SEG); input.set(default32 ? Buffer.alloc(40) : record('R3SB', 40, [0, 0, 0, 0, 0, 0]), BASES);
  return input;
}
function expected(input, reason, retired, pc = input.readUInt32LE(48)) {
  const want = Buffer.from(input); want.writeUInt32LE(pc, 48);
  if (retired > 0 || reason >= 16) {want.writeUInt32LE(input.readUInt32LE(52) & ~RF, 52); want.writeUInt32LE(0, 12);}
  return exit(want, reason, retired, reason >= 16 ? 7 : reason === 11 ? 6 : 1);
}
function observe(c, label, material, input, want, options = {}) {
  planned++;
  if (material) {invoked++; return h.observe(c, label, {input, want, material, ...options});}
  h.skip(c, label, 'required compile admission'); return null;
}
function open(resident, entries, label) {
  const c = h.open(resident, entries, label, [[PC, 7], [0x5000, 3], [0x7000, 3]]);
  h.upload(c, 0x5000, Buffer.alloc(4096, 0x5a)); h.upload(c, 0x7000, Buffer.alloc(4096, 0xa5)); return c;
}
function strict(c, label, target, length, expectedStatus = 0) {
  return h.compile(c, label, [[target, length]], [], {expectedStatus});
}
function stopped(c, label, expectedStatus = 0) {
  h.put(c, 140, words([0xdeadbeef, 0xffffffff]));
  return h.compile(c, label, [], [], {stopped: true, expectedStatus});
}
function bootstrap(c, target, default32) {
  const code = Buffer.alloc(default32 ? 5 : 3); code[0] = 0xe9;
  if (default32) code.writeInt32LE(target - PC - code.length, 1);
  else code.writeInt16LE(target - PC - code.length, 1);
  h.upload(c, PC, code); const input = seed(PC, {default32, shadow: 0}); h.put(c, 0, input);
  const material = strict(c, 'real same-mode JMP reaches stopped target', PC, code.length);
  observe(c, 'same-mode bootstrap retires before canonical NeedCode', material, input,
    expected(input, 3, 1, target), {budget: 2});
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of forms) {
    const code = bytes(row.hex), target = 0x1100, c = open(resident, entries, `${row.default32 ? 'D32' : 'D16'} ${row.label}`);
    h.upload(c, target, Buffer.concat([code, bytes('ebfe')]));
    const input = seed(target, row); h.put(c, 0, input);
    const material = strict(c, 'complete disabled-MPX source is strict ordinary NOP', target, code.length + 2);
    observe(c, 'no operand segment/EA/RAM/helper or numeric effect; exact full arena and all RAM', material,
      input, expected(input, 1, 1, target + code.length));
    if (row.label === 'BNDMOV load' && !row.default32) {
      const traced = seed(target, {...row, flags: FLAGS | RF | TF});
      observe(c, 'D16 current-v3 NOP retains literal numeric payload; TF after RF/shadow consumption', material,
        traced, expected(traced, 16, 1, target + code.length), {budget: 2});
    }
    h.host(c, 'close');
  }
  for (const default32 of [false, true]) {
    const code = bytes(default32 ? '660f1a05ffffffff' : '660f1a06ffff');
    const target = 0x2000 - code.length + 1, c = open(resident, entries, `D${default32 ? 32 : 16} consumed displacement repair`);
    h.upload(c, target, code.subarray(0, -1)); bootstrap(c, target, default32);
    strict(c, 'missing final displacement prevents strict NOP capture', target, code.length, 10);
    const fetchMaterial = h.fetchFault(c, 'stopped capture cannot classify incomplete displacement', {pc: target, length: code.length, address: 0x2000, errorCode: 20});
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, Buffer.concat([code.subarray(-1), bytes('ebfe')])); h.host(c, 'protect', [0x2000, 1, 1]);
    h.fetchFault(c, 'READ without EXECUTE on final displacement remains neutral', {pc: target, length: code.length, address: 0x2000, errorCode: 21, material: fetchMaterial});
    h.host(c, 'protect', [0x2000, 1, 7]); h.reachNeedCode(c); const material = stopped(c, 'only full executable displacement repair captures NOP');
    const input = seed(target, {default32, flags: FLAGS | RF | TF});
    observe(c, 'repaired source retires one no-operand NOP at original logical PC', material,
      input, expected(input, 16, 1, target + code.length), {budget: 2}); h.host(c, 'close');
  }
  for (const row of [
    {label: 'duplicate66 host refusal', hex: '66660f1a00', default32: false},
    {label: 'original neighbor0F19 is a defined NOP', hex: '660f19c1', default32: true, compatible: true},
    {label: 'original neighbor0F1C is a defined NOP', hex: '660f1cc1', default32: false, compatible: true},
    {label: 'LOCK memory MPX is existing UD', hex: 'f0660f1b00', default32: false, invalid: true},
    {label: 'LOCK memory MPX is existing UD', hex: 'f0660f1b00', default32: true, invalid: true},
  ]) {
    const code = bytes(row.hex), target = 0x1100, c = open(resident, entries, row.label);
    h.upload(c, target, row.compatible ? Buffer.concat([code, bytes('ebfe')]) : code); bootstrap(c, target, row.default32);
    if (row.compatible) {
      const material = strict(c, row.label, target, code.length + 2, 0), input = seed(target, {...row, flags: FLAGS | RF | TF});
      observe(c, 'original neighbor bytes retire without operand access before TF', material, input, expected(input, 16, 1, target + code.length), {budget: 2});
      h.host(c, 'close'); continue;
    }

    strict(c, row.label + ': strict refusal retains existing authority', target, code.length, 10);
    const material = stopped(c, row.label + ': stopped policy remains distinct', row.invalid ? 0 : 10);
    if (row.invalid) {
      const input = seed(target, {...row, flags: FLAGS | RF | TF});
      observe(c, 'invalid LOCK faults UD before operands/TF and never becomes NOP', material,
        input, expected(input, 20, 0), {budget: 2});
    }
    h.host(c, 'close');
  }
}
assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'disabled-mpx-counts.json'), JSON.stringify({forms: forms.length,
  contexts: 124, planned_module_invocations: planned, admitted_module_invocations: invoked,
  profiles: ['replacement-extents', 'replacement-entries', 'resident-extents', 'resident-entries'],
  source_pins: sourcePins}, null, 2) + '\n');
h.finish(['exact four reserved Codes for eight disabled-MPX opcode/prefix families; no enabled MPX state or general reserved-NOP admission',
  'literal complete arena, all mapped RAM, exact helper-free imports/trace, code lengths and retirement across all four profiles',
  'D16 current-v3 with explicit zero R3SB; DS null, SS limited and all effective operands unmapped are intentional',
  'strict/stopped full-fetch refusals are host admission observations; no fake operand/page fault or fetched bytes',
  'canonical D32 budget/cancel/currentness/flags and generic CS authority proofs are retained and reused, without repeating them per encoding',
  'CPUID immutability and all canonical32 register pairs remain in the existing optional availability fixture']);
