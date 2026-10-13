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
];
const continuations = [rsm, evex].map((bytes, index) => ({
  ...fault(`completed-nop-before-${index === 0 ? 'rsm' : 'evex'}`, [0x90, ...bytes]),
  retired: 1,
  offset: 1,
  schedules: [{}, { budget: 1, reason: 1 }, { flags: 0x102, reason: 16, version: 7 }],
}));
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
    'fixed non-SMM RSM and defined EVEX vectors with XCR0=7 retire zero at the canonical current EIP as UD, without operand access; strict compilation remains a host refusal. Actual truncated-fetch preparation stays neutral until executable-page repair; companion native tests establish the precise Execute MemoryFault rather than claiming a guest PF packet from compile_stopped_entry.',
});
