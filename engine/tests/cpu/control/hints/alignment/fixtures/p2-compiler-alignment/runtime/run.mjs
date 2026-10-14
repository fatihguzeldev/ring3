import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, hash, record, registers, PC, SEG, BASES} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const sources = ['engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/tests/cpu/control/hints/alignment/native.rs', 'engine/tests/cpu/control/hints/alignment/wasm.rs',
  'engine/tests/cpu/control/hints/alignment/fixtures/p2-compiler-alignment/runtime/run.mjs'];
const pins = () => Object.fromEntries(sources.map(path => [path, hash(readFileSync(join(root, path)))]));
const beforePins = pins();

function context(code32) {
  const sg = record('R3SG', 64, [], 3);
  [0, 0x1b, 0x23, 0, 0, 0].forEach((v, i) => sg.writeUInt16LE(v, 16 + 2 * i));
  [0, code32 ? 0xf3bfffff : 0xf1bfffff, 0xf33fffff, 0, 0, 0]
    .forEach((v, i) => sg.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
  return {sg, sb: record('R3SB', 40, [0, 0, 0, 0, 0, 0])};
}
function seed(code32, {flags = 0x10002, shadow = 1, cancel = 0} = {}) {
  const bytes = Buffer.from(h.initial);
  bytes.set(record('R3ST', 56, [...registers, PC, flags], 3)); bytes.writeUInt32LE(shadow, 12);
  bytes.writeUInt32LE(cancel, 96);
  const {sg, sb} = context(code32); bytes.set(sg, SEG); bytes.set(sb, BASES);
  for (let i = 140; i < 4236; i++) bytes[i] = (i * 13 + 7) & 255;
  return bytes;
}
function exit(bytes, reason, retired, version = 1) {
  bytes.set(record('R3EX', 40, [reason, retired, 0, 0, 0, 0], version), 56);
}
const patterns = [];
for (let length = 11; length <= 15; length++) for (const payload of [false, true]) {
  const bytes = Buffer.from([...Array(length - 9).fill(0x66), 0x2e, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0]);
  if (payload) bytes.set([0x66, 0xf3, 0xf0, 0xff], bytes.length - 4);
  patterns.push({name: `${length}-byte compiler nop${payload ? ' with literal displacement bytes' : ''}`, bytes, code32: true});
}
for (const code32 of [false, true]) patterns.push({name: 'multi66 nop with unusable default data segment', code32,
  bytes: Buffer.from([0x66, 0x66, 0x0f, 0x1f, 0])});

for (const resident of [false, true]) for (const entries of [false, true]) for (const pattern of patterns) {
  const c = h.open(resident, entries, pattern.name, [[PC, 7]]);
  const {sg, sb} = context(pattern.code32); h.put(c, SEG, sg); h.put(c, BASES, sb);
  h.upload(c, PC, Buffer.concat([pattern.bytes, Buffer.from([0xeb, 0xfe])]));
  const material = h.compile(c, 'compiler alignment and terminal loop', [[PC, pattern.bytes.length + 2]]);
  if (!material) {h.skip(c, 'alignment execution', 'compiler alignment compilation'); h.host(c, 'close'); continue;}
  const cases = [
    ['one nop retires without data or stack access', {}],
    ['zero budget preserves RF and shadow', {budget: 0, flags: 0x10102}],
    ['cancel preserves RF and shadow', {cancel: 1, flags: 0x10102}],
    ['SS shadow expires at the completed nop boundary', {flags: 0x10102, step: true}],
    ['old TF traps after the complete nop', {flags: 0x10102, shadow: 0, step: true}],
  ];
  for (const [index, [label, options]] of cases.entries()) {
    const input = seed(pattern.code32, options), want = Buffer.from(input), helpers = [];
    if (options.cancel || options.budget === 0) exit(want, options.cancel ? 2 : 1, 0);
    else {
      want.writeUInt32LE(PC + pattern.bytes.length, 48);
      want.writeUInt32LE(input.readUInt32LE(52) & ~0x10000, 52); want.writeUInt32LE(0, 12);
      exit(want, options.step ? 16 : 1, 1, options.step ? 7 : 1);
      if (options.step) helpers.push({name: 'finish_exception_detection',
        args: [String(index + 1), 16, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0], status: 0});
    }
    h.observe(c, label, {input, want, material, budget: options.budget ?? 1, helpers});
  }
  h.host(c, 'close');
}
assert.deepEqual(pins(), beforePins, 'instruction owners and literal alignment oracles stay fixed');
writeFileSync(join(output, 'alignment-source-pins.json'), JSON.stringify(beforePins, null, 2) + '\n');
h.finish(['compiler-emitted 11 through15-byte multi66 0F1F /0 nops, with literal displacement bytes',
  'four bound compile routes, complete arena and mapped RAM, no operand/stack helper even with unusable data segments',
  'ordinary RF/shadow/TF and budget/cancel semantics; no generalized duplicate-prefix or browser/game claim']);
