import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const supportPath = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, record, state, exit, cloneRAM, hash} = await import(pathToFileURL(join(root, supportPath)));
const h = await harness(enginePath, output, root), CODE = 0x1100, TARGET = 0x1200, STACK = 0x7800;
const RF = 0x10000, ZF = 0x40, FLAGS = 0x244cd7;
const own = 'engine/tests/cpu/control/near/bnd/';
const paths = ['engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/decode/mpx.rs',
  'engine/src/cpu/x86/decode/mpx/tests.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/x86/decode/lower.rs', 
  'engine/src/cpu/dbt/wasm/instructions/memory/narrow.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/word_store.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/tests/cpu/control/native.rs',
  'engine/tests/cpu/control/wasm.rs', own + 'native.rs', own + 'wasm.rs',
  own + 'fixtures/p2-bnd-prefix/runtime/run.mjs', supportPath];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
// literal encodings and expected transfer widths; targets are independent of the decoder.
const cases = [
  {name: 'D32 direct JMP', hex: 'f2e9fa000000', d32: true},
  {name: 'D16 register JMP masks high source', hex: 'f2ffe0', d32: false, eax: 0xabcd1200},
  {name: 'D32 memory JMP', hex: 'f2ff20', d32: true, eax: 0x5000, read: 4},
  {name: 'D16 direct CALL includes F2 in return PC', hex: 'f2e8fc00', d32: false, write: 2, callFault: true},
  {name: 'D32 register CALL', hex: 'f2ffd0', d32: true, eax: TARGET, write: 4},
  {name: 'D16 address32 memory CALL', hex: 'f267ff10', d32: false, eax: 0x5000, read: 2, write: 2},
  {name: 'D32 RET captures stack', hex: 'f2c3', d32: true, read: 4, ret: true, retFault: true},
  {name: 'D16 RET immediate preserves word pop and byte adjustment', hex: 'f2c20800', d32: false, read: 2, ret: true, adjust: 8},
  {name: 'D16 short Jcc taken and fallthrough', hex: 'f2741d', d32: false, target: 0x1120, conditional: true},
  {name: 'D32 near Jcc taken and fallthrough', hex: 'f20f84f9000000', d32: true, conditional: true},
  {name: 'D32 operand16 direct CALL', hex: 'f266e8fb00', d32: true, write: 2},
  {name: 'D16 operand32 direct CALL', hex: 'f266e8f9000000', d32: false, write: 4},
  {name: 'D32 address16 absolute memory JMP', hex: 'f267ff260050', d32: true, read: 4},
];
let planned = 0, invoked = 0;
function seed(row, flags = FLAGS | RF) {
  const regs = [row.eax ?? 0x12345678, 0x23456789, 0x3456789a, 0x456789ab, STACK, 0x56789abc, 0x6789abcd, 0x789abcde];
  const input = state(h.initial, {pc: CODE, flags, regs}); input.writeUInt32LE(1, 12);
  for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {
    input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
  }
  const sg = record('R3SG', 64, [], row.d32 ? 2 : 3);
  [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + 2 * i));
  [0xf33fffff, row.d32 ? 0xf3bfffff : 0xf1bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0]
    .forEach((value, i) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  input.set(sg, SEG); input.set(row.d32 ? Buffer.alloc(40) : record('R3SB', 40, [0, 0, 0, 0, 0, 0]), BASES);
  return input;
}
function packet(width, value = 0) {return record('R3MH', 40, [0, value, 0, 0, 0, width], 10);}
function expected(input, {pc, retired = 1, reason = 1, cpu = false} = {}) {
  const want = Buffer.from(input); want.writeUInt32LE(pc ?? CODE, 48);
  want.writeUInt32LE(input.readUInt32LE(52) & ~RF, 52); want.writeUInt32LE(0, 12);
  return exit(want, reason, retired, cpu || reason === 11 ? 6 : 1);
}
function observe(c, label, material, input, want, options = {}) {
  planned++;
  if (material) {invoked++; h.observe(c, label, {material, input, want, ...options});}
  else h.skip(c, label, 'required prefix preparation');
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of cases) {
    const code = Buffer.from(row.hex, 'hex');
    const c = h.open(resident, entries, row.name, [[PC, 7], [0x5000, 3], [0x7000, 3]]);
    h.upload(c, 0x5000, Buffer.alloc(4096, 0x5a)); h.upload(c, 0x7000, Buffer.alloc(4096, 0xa5));
    const pointer = Buffer.alloc(4); pointer.writeUInt32LE(TARGET); h.upload(c, 0x5000, pointer); h.upload(c, STACK, pointer);
    h.upload(c, CODE, code); const input = seed(row); h.put(c, 0, input);
    const readName = row.read ? `cpu_read${row.read * 8}` : null;
    const storeName = row.write ? `cpu_store${resident ? '_resident' : ''}${row.write * 8}` : null;
    const material = h.compile(c, 'exact terminal extent retains ordinary prefixed near transfer', [[CODE, code.length]], [readName, storeName].filter(Boolean));
    for (const taken of row.conditional ? [true, false] : [true]) {
      const current = seed(row, row.conditional && !taken ? (FLAGS | RF) & ~ZF : FLAGS | RF);
      const want = expected(current, {pc: taken ? row.target ?? TARGET : CODE + code.length, cpu: !!(row.read || row.write)});
      const helpers = [], wantRAM = cloneRAM(h.ram(c));
      if (row.read) {
        want.set(packet(row.read, TARGET), 100);
        helpers.push({name: readName, args: [row.ret ? STACK : 0x5000], status: 0});
      }
      if (row.write) {
        want.writeUInt32LE(STACK - row.write, 32); want.set(packet(row.write), 100);
        helpers.push({name: storeName, args: [...(resident && material ? material.binding : []), STACK - row.write, CODE + code.length], status: 0});
        const returned = Buffer.alloc(4); returned.writeUInt32LE(CODE + code.length);
        wantRAM.get(0x7000).set(returned.subarray(0, row.write), STACK - row.write - 0x7000);
      }
      if (row.ret) want.writeUInt32LE(STACK + row.read + (row.adjust ?? 0), 32);
      observe(c, 'literal GPR/flags/ESP/packet/RAM and return-address chronology', material, current, want, {helpers, wantRAM});
    }
    if (row.callFault) {
      const limited = seed(row); limited.writeUInt32LE(0xf0b011ff, SEG + 32); limited.writeUInt32LE(0xfffffff0, 32);
      observe(c, 'BND CALL target GP precedes unmapped stack write and retirement', material, limited,
        expected(limited, {reason: 11, retired: 0, cpu: true}), {kind: 'target-authority'});
    }
    if (row.retFault) {
      const limited = seed(row); limited.writeUInt32LE(0xf2b011ff, SEG + 32);
      const want = expected(limited, {reason: 11, retired: 0, cpu: true}); want.set(packet(4, TARGET), 100);
      observe(c, 'BND RET captures once then target GP rolls ESP and retirement back', material, limited, want,
        {helpers: [{name: readName, args: [STACK], status: 0}], kind: 'target-authority'});
    }
    h.host(c, 'close');
  }
  for (const [label, hex, d32] of [
    ['duplicate F2 CALL stays neutral refusal', 'f2f2e8fb00', false],
    ['F3 near JMP stays separate repeat refusal', 'f3e9fa000000', true],
    ['short EB stays outside selected BND table', 'f2eb00', true],
  ]) {
    const code = Buffer.from(hex, 'hex'), c = h.open(resident, entries, label, [[PC, 7]]);
    h.upload(c, CODE, code); h.put(c, 0, seed({d32}));
    h.compile(c, label, [[CODE, code.length]], [], {expectedStatus: 10}); h.host(c, 'close');
  }
}
assert.equal(planned, 68, '13 selected transfers, two Jcc outcomes and two target-order controls per profile');
assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'bnd-prefix-counts.json'), JSON.stringify({forms: cases.length, contexts: 64,
  planned_module_invocations: planned, admitted_module_invocations: invoked, source_pins: sourcePins}, null, 2) + '\n');
h.finish(['exact disabled-MPX F2 near CALL/RET/JMP/Jcc prefix admission; branch lowerers and helpers unchanged',
  'all four profiles, current-v3 D16 and operand/address overrides, literal full arena/RAM/helpers and F2-inclusive CALL return address',
  'two Jcc outcomes retain target versus full next-EIP; target GP ordering retains no-stack-write CALL and staged RET ESP rollback',
  'generic RF/TF/budget/cancel/physical/key/fetch proofs reused from existing near-control/MPX owners',
  'no enabled MPX, generic REPNE/REP or shortEB/LOOP/JCXZ/far/IRET compatibility claim']);
