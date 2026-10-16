import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, hash, record, words, registers, PC, SEG, BASES} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root), TARGET = 0x2000;
const sourcePaths = ['engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs',
  'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs', 'engine/src/cpu/dbt/wasm/instructions/stack/pointer.rs',
  'engine/tests/cpu/control/near/repeat/native.rs', 'engine/tests/cpu/control/near/repeat/wasm.rs',
  'engine/tests/cpu/control/near/repeat/fixtures/p2-repeat-return/runtime/run.mjs'];
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();

function context(code32, stack32) {
  const sg = record('R3SG', 64, [], 3);
  [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + i * 2));
  [0xf33fffff, code32 ? 0xf3bfffff : 0xf1bfffff, stack32 ? 0xf33fffff : 0xf13fffff,
    0xf33fffff, 0xf33fffff, 0].forEach((value, i) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  return {sg, sb: record('R3SB', 40, [0, 0, 0, 0, 0, 0])};
}
function seed(code32, {stack32 = true, esp = 0x7800, flags = 0x10002, shadow = 1, cancel = 0} = {}) {
  const bytes = Buffer.from(h.initial), regs = [...registers]; regs[4] = esp;
  bytes.set(record('R3ST', 56, [...regs, PC, flags], 3)); bytes.writeUInt32LE(shadow, 12);
  bytes.writeUInt32LE(cancel, 96);
  const {sg, sb} = context(code32, stack32); bytes.set(sg, SEG); bytes.set(sb, BASES);
  for (let at = 140; at < 4236; at++) bytes[at] = (at * 17 + 3) & 255;
  return bytes;
}
function exit(bytes, reason, retired, detail = 0, address = 0, access = 0, length = 0, version = 6) {
  bytes.set(record('R3EX', 40, [reason, retired, detail, address, access, length], version), 56);
}
function finish(epoch, reason, detail = 0, address = 0, access = 0, length = 0, debug = 0) {
  return {name: 'finish_exception_detection', args: [String(epoch), reason, detail, address, access, length,
    debug, 0, 0, 0, 0, 0], status: 0};
}

for (const resident of [false, true]) for (const entries of [false, true]) for (const code32 of [false, true]) {
  for (const prefix of [[0xf3], [0x66, 0xf3], [0xf3, 0x66]]) for (const adjust of [0, 0x28]) {
    const word = code32 === prefix.includes(0x66), width = word ? 2 : 4;
    const bytes = Buffer.from([...prefix, ...(adjust ? [0xc2, adjust, 0] : [0xc3])]);
    const c = h.open(resident, entries, `repeat return D${code32 ? 32 : 16}/${bytes.toString('hex')}`, [[PC, 7], [0x7000, 3], [0xf000, 3]]);
    const {sg, sb} = context(code32, true); h.put(c, SEG, sg); h.put(c, BASES, sb);
    h.upload(c, PC, bytes); h.upload(c, 0x7800, words([word ? 0x5a5a2000 : TARGET]));
    h.upload(c, 0xfffc, words([word ? 0xa5a52000 : TARGET]));
    const helper = word ? 'cpu_read16' : 'cpu_read32';
    const material = h.compile(c, 'single REP near-return is a precise terminator', [[PC, bytes.length]], [helper]);
    if (!material) {h.skip(c, 'return execution', 'repeat near-return compilation'); h.host(c, 'close'); continue;}
    const cases = [
      ['complete ordinary pop and unsigned adjustment', {}],
      ['zero budget preserves RF, shadow and unmapped stack', {budget: 0, esp: 0x9000, flags: 0x10102}],
      ['cancel preserves RF, shadow and unmapped stack', {cancel: 1, esp: 0x9000, flags: 0x10102}],
      ['real stack page fault consumes RF without retirement', {esp: 0x9000, flags: 0x10102, fault: true}],
      ['old TF traps after the completed return', {flags: 0x10102, shadow: 0, step: true}],
      ['SS.B16 wraps SP while preserving high ESP', {stack32: false, esp: 0xabcdfffc}],
    ];
    for (const [index, [label, options]] of cases.entries()) {
      const input = seed(code32, options), want = Buffer.from(input), calls = [];
      const stopped = options.cancel || options.budget === 0;
      if (stopped) exit(want, options.cancel ? 2 : 1, 0);
      else {
        want.writeUInt32LE(input.readUInt32LE(52) & ~0x10000, 52); want.writeUInt32LE(0, 12);
        const address = options.stack32 === false ? 0xfffc : options.esp ?? 0x7800;
        calls.push({name: helper, args: [address], status: options.fault ? 23 : 0});
        if (options.fault) {
          want.set(record('R3MH', 40, [9, 0, 4, address, 1, width], 15), 100);
          exit(want, 23, 0, 4, address, 1, width, 12);
          calls.push(finish(index + 1, 23, 4, address, 1, width));
        } else {
          want.set(record('R3MH', 40, [0, TARGET, 0, 0, 0, width], 10), 100);
          want.writeUInt32LE(TARGET, 48);
          const oldEsp = input.readUInt32LE(32), delta = width + adjust;
          want.writeUInt32LE(options.stack32 === false ? ((oldEsp & 0xffff0000) | ((oldEsp + delta) & 0xffff)) >>> 0 : oldEsp + delta, 32);
          exit(want, options.step ? 16 : 1, 1, 0, 0, 0, 0, options.step ? 7 : 6);
          if (options.step) calls.push(finish(index + 1, 16, 0, 0, 0, 0, 2));
        }
      }
      h.observe(c, label, {input, want, material, budget: options.budget ?? 1, helpers: calls});
    }
    h.host(c, 'close');
  }
}
assert.deepEqual(pins(), sourcePins, 'instruction owners and literal return oracles remain fixed');
writeFileSync(join(output, 'repeat-source-pins.json'), JSON.stringify(sourcePins, null, 2) + '\n');
h.finish(['single F3 C3/C2 near-return alias only, both code defaults and optional operand override orders',
  'complete arena and mapped RAM, exact stack pop/unsigned adjustment, B16 pointer wrapping, RF/TF and scheduling/fault priority',
  'no far-return, IRET, generalized prefix admission, browser, game or throughput claim']);
