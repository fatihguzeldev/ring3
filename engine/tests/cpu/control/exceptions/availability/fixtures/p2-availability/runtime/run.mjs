import { runStoppedFaults } from '../../../../support/stopped.mjs';
import { createArena } from '../../../../../../../support/wasm/arena.mjs';

const x87 = Buffer.from(createArena().subarray(4236, 4364));
for (let index = 0; index < 80; index++) x87[40 + index] = (index * 29 + 17) & 255;
const nullDs = Buffer.from(createArena().subarray(4524, 4588));
nullDs.writeUInt16LE(0, 16 + 3 * 2);
const fault = (name, bytes, extra = {}) => ({
  name,
  bytes: Buffer.from(bytes),
  reason: 20,
  baseVersion: 1,
  x87,
  ...extra,
});
const rsm = [0x0f, 0xaa];
const evex = [0x62, 0xf1, 0x7c, 0x48, 0x58, 0xc1];
const kxor = [0xc5, 0xec, 0x47, 0xcb];
const kmovAbsolute = [0xc5, 0xf8, 0x90, 0x0d, 0xff, 0xff, 0xff, 0xff];
const positive = [
  fault('evex-vsib-prefetch-needs-zmm-index-state', [0x62, 0xf2, 0x7d, 0x49, 0xc6, 0x0c, 0x18]),
  fault('bare-rsm-outside-smm', rsm),
  fault('evex-vaddps-zmm-missing-xcr0-state', evex),
  fault('evex-vaddps-xmm-still-needs-zmm-state', [0x62, 0xf1, 0x7c, 0x08, 0x58, 0xc1]),
  fault('evex-no-null-ds-or-unmapped-operand-access', [0x62, 0xf1, 0x7c, 0x48, 0x58, 0x03], {
    registers: { 3: 0xffffffff },
    segments: nullDs,
  }),
  fault('complete-evex-at-final-executable-byte', evex, { target: 0x1ffa }),
  fault('complete-rsm-at-final-executable-byte', rsm, { target: 0x1ffe }),
  fault('vex-kxorw-needs-opmask-state-at-final-executable-byte', kxor, { target: 0x1ffc }),
  fault('vex-kmovw-read-fault-precedes-null-ds-and-unmapped-operand', [0xc5, 0xf8, 0x90, 0x0b], {
    registers: { 3: 0xffffffff },
    segments: nullDs,
  }),
  fault('vex-kmovw-write-fault-precedes-null-ds-and-unmapped-operand', [0xc5, 0xf8, 0x91, 0x0b], {
    registers: { 3: 0xffffffff },
    segments: nullDs,
  }),
];
const continuations = [rsm, evex].map((bytes, index) => ({
  ...fault(`completed-nop-before-${index === 0 ? 'rsm' : 'evex'}`, [0x90, ...bytes]),
  retired: 1,
  offset: 1,
  schedules: [{}, { budget: 1, reason: 1 }, { flags: 0x102, reason: 16, version: 7 }],
}));
continuations.push({
  ...fault('completed-nop-before-vex-kxorw', [0x90, ...kxor]),
  retired: 1,
  offset: 1,
  schedules: [{}, { budget: 1, reason: 1 }, { flags: 0x102, reason: 16, version: 7 }],
});
const negatives = [
  { name: 'unrelated-int80-remains-unsupported', bytes: Buffer.from([0xcd, 0x80]), x87 },
  {
    name: 'truncated-evex-fetch-before-availability-classification',
    bytes: Buffer.from(evex.slice(0, 3)),
    target: 0x1ffd,
    repair: Buffer.from(evex.slice(3)),
    repaired: { reason: 20, baseVersion: 1 },
    x87,
  },
  {
    name: 'truncated-kmovw-final-displacement-byte-before-availability-classification',
    bytes: Buffer.from(kmovAbsolute.slice(0, 7)),
    target: 0x1ff9,
    repair: Buffer.from(kmovAbsolute.slice(7)),
    repaired: { reason: 20, baseVersion: 1 },
    x87,
  },
];
runStoppedFaults({
  sourcePaths: [
    'engine/src/cpu/x86/decode/execution_faults/tests/availability/native.rs',
    'engine/tests/cpu/control/wasm.rs',
    'engine/tests/cpu/control/exceptions/availability/wasm.rs',
    'engine/tests/cpu/control/exceptions/availability/fixtures/p2-availability/runtime/run.mjs',
  ],
  positive,
  continuations,
  negatives,
  claim:
    'fixed non-SMM RSM, defined EVEX vectors requiring XCR0&0xe7==0xe7, and VEX opmask instructions requiring XCR0&0xe3==0xe3 retire zero at the canonical current EIP as UD with current XCR0=7, without operand access; strict compilation remains a host refusal. Actual truncated-fetch preparation stays neutral until executable-page repair; companion native tests establish the precise Execute MemoryFault rather than claiming a guest PF packet from compile_stopped_entry.',
});
