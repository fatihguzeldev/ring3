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
// absent random facilities use the same synthetic-fault owner; these are literal
// unavailable-instruction observations, not successful random or entropy claims.
const randomForms = [
  ['rdrand32', [0x0f, 0xc7, 0xf0]], ['rdrand16', [0x66, 0x0f, 0xc7, 0xf0]],
  ['rdseed32', [0x0f, 0xc7, 0xf8]], ['rdseed16', [0x66, 0x0f, 0xc7, 0xf8]],
];
for (const [name, bytes] of randomForms) for (const finalByte of [false, true]) {
  positive.push(fault(`absent-${name}${finalByte ? '-last-executable-byte' : ''}`, bytes, {
    ...(finalByte ? {target: 0x2000 - bytes.length} : {}),
    schedules: [
      {}, {flags: 0x102},
      {flags: 0x10002, stateVersion: 3, expectedFlags: 2},
      {flags: 0x10102, stateVersion: 3, expectedFlags: 0x102},
      {flags: 0x10002, stateVersion: 3, expectedFlags: 2, shadow: 1},
      {flags: 0x10002, stateVersion: 3, expectedFlags: 0x10002, shadow: 1, budget: 0, reason: 1, retired: 0},
      {flags: 0x10002, stateVersion: 3, expectedFlags: 0x10002, shadow: 1, cancel: 1, reason: 2, retired: 0},
      {flags: 0x244cd7, shadow: 1},
    ],
  }));
}
for (const [name, bytes] of [randomForms[0], randomForms[3]]) {
  continuations.push({...fault(`completed-nop-before-absent-${name}`, [0x90, ...bytes]), retired: 1, offset: 1,
    schedules: [{}, {budget: 1, reason: 1}, {flags: 0x102, reason: 16, version: 7},
      {flags: 0x10002, stateVersion: 3, expectedFlags: 2}],
  });
}
for (const [name, modrm] of [['rdrand', 0xf0], ['rdseed', 0xf8]]) {
  negatives.push({name: `truncated-${name}-modrm-before-availability`, bytes: Buffer.from([0x0f, 0xc7]),
    target: 0x1ffe, repair: Buffer.from([modrm]), repaired: {reason: 20, baseVersion: 1}, x87});
}
negatives.push({name: 'duplicate-operand-prefix-random-keeps-profile-refusal',
  bytes: Buffer.from([0x66, 0x66, 0x0f, 0xc7, 0xf0]), x87});
// exact nonzero-CPL UD rules do not create a monitor, operand read or wait backend.
const monitorForms = [
  ['monitor32', [0x0f, 0x01, 0xc8]], ['monitor16-address', [0x67, 0x0f, 0x01, 0xc8]],
  ['mwait', [0x0f, 0x01, 0xc9]],
];
for (const [name, bytes] of monitorForms) for (const finalByte of [false, true]) {
  positive.push(fault(`cpl3-${name}${finalByte ? '-last-executable-byte' : ''}`, bytes,
    finalByte ? {target: 0x2000 - bytes.length} : {}));
}
positive.push(fault('cpl3-monitor-never-reads-null-ds-or-unmapped-address', monitorForms[0][1],
  {registers: {0: 0xffffffff, 1: 0}, segments: nullDs}));
continuations.push({...fault('completed-nop-before-cpl3-monitor', [0x90, ...monitorForms[0][1]]),
  retired: 1, offset: 1, schedules: [{}, {budget: 1, reason: 1}, {flags: 0x102, reason: 16, version: 7},
    {flags: 0x10002, stateVersion: 3, expectedFlags: 2}],
});
for (const [name, modrm] of [['monitor', 0xc8], ['mwait', 0xc9]]) {
  negatives.push({name: `truncated-${name}-opcode-group-before-cpl3-rule`, bytes: Buffer.from([0x0f, 0x01]),
    target: 0x1ffe, repair: Buffer.from([modrm]), repaired: {reason: 20, baseVersion: 1}, x87});
}
positive.push(fault('invalid-operand-prefix-monitor-keeps-existing-ud',
  [0x66, 0x66, 0x0f, 0x01, 0xc8]));
// exact CPUID-defined transaction absence precedes any available-feature behavior.
const transactionForms = [
  ['xabort', [0xc6, 0xf8, 0x5a]], ['xbegin16', [0x66, 0xc7, 0xf8, 0, 0]],
  ['xbegin32', [0xc7, 0xf8, 0, 0, 0, 0]], ['xend', [0x0f, 0x01, 0xd5]], ['xtest', [0x0f, 0x01, 0xd6]],
];
for (const [name, bytes] of transactionForms) for (const finalByte of [false, true]) {
  positive.push(fault(`absent-${name}${finalByte ? '-last-executable-byte' : ''}`, bytes,
    finalByte ? {target: 0x2000 - bytes.length} : {}));
}
const boundedCs = Buffer.from(createArena().subarray(4524, 4588)); boundedCs.writeUInt16LE(2, 4);
[0xf33fffff, 0xf2b01fff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0].forEach((value, index) =>
  boundedCs.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][index]));
positive.push(fault('absent-xbegin-before-out-of-limit-fallback', [0xc7, 0xf8, 0xff, 0xff, 0xff, 0x7f], {segments: boundedCs}));
for (const [name, bytes] of [transactionForms[0], transactionForms[4]]) {
  continuations.push({...fault(`completed-nop-before-absent-${name}`, [0x90, ...bytes]), retired: 1, offset: 1,
    schedules: [{}, {budget: 1, reason: 1}, {flags: 0x102, reason: 16, version: 7},
      {flags: 0x10002, stateVersion: 3, expectedFlags: 2}],
  });
}
negatives.push({name: 'truncated-xbegin-final-displacement-before-availability',
  bytes: Buffer.from([0xc7, 0xf8, 0, 0, 0]), target: 0x1ffb, repair: Buffer.from([0]), repaired: {reason: 20, baseVersion: 1}, x87});
negatives.push({name: 'truncated-xabort-immediate-before-availability',
  bytes: Buffer.from([0xc6, 0xf8]), target: 0x1ffe, repair: Buffer.from([0x5a]), repaired: {reason: 20, baseVersion: 1}, x87});
negatives.push({name: 'duplicate-operand-prefix-transaction-keeps-profile-refusal',
  bytes: Buffer.from([0x66, 0x66, 0xc7, 0xf8, 0, 0]), x87});
// the virtual SYSCALL feature is absent; neither EFER nor an OS return frame is assumed.
const systemCallForms = [['syscall', [0x0f, 0x05]], ['sysret', [0x0f, 0x07]]];
for (const [name, bytes] of systemCallForms) for (const finalByte of [false, true]) {
  positive.push(fault(`absent-${name}${finalByte ? '-last-executable-byte' : ''}`, bytes,
    finalByte ? {target: 0x2000 - bytes.length} : {}));
}
for (const [name, bytes] of systemCallForms) {
  continuations.push({...fault(`completed-nop-before-absent-${name}`, [0x90, ...bytes]), retired: 1, offset: 1,
    schedules: [{}, {budget: 1, reason: 1}, {flags: 0x102, reason: 16, version: 7},
      {flags: 0x10002, stateVersion: 3, expectedFlags: 2}],
  });
  negatives.push({name: `truncated-${name}-opcode-before-capability`, bytes: Buffer.from([0x0f]),
    target: 0x1fff, repair: Buffer.from([bytes[1]]), repaired: {reason: 20, baseVersion: 1}, x87});
}
negatives.push({name: 'duplicate-operand-prefix-syscall-keeps-profile-refusal',
  bytes: Buffer.from([0x66, 0x66, 0x0f, 0x05]), x87});
// SEP0, independently of MSR instruction presence, makes both entry opcodes UD.
const systemEntryForms = [['sysenter', [0x0f, 0x34]], ['sysexit', [0x0f, 0x35]]];
for (const [name, bytes] of systemEntryForms) for (const finalByte of [false, true]) {
  positive.push(fault(`sep0-${name}${finalByte ? '-last-executable-byte' : ''}`, bytes, {
    ...(finalByte ? {target: 0x2000 - bytes.length} : {}),
    schedules: [
      {}, {flags: 0x202}, {flags: 0x102},
      {flags: 0x10002, stateVersion: 3, expectedFlags: 2},
      {flags: 0x244fd7, shadow: 1},
      {flags: 0x10002, stateVersion: 3, expectedFlags: 0x10002, shadow: 1, budget: 0, reason: 1, retired: 0},
      {flags: 0x10002, stateVersion: 3, expectedFlags: 0x10002, shadow: 1, cancel: 1, reason: 2, retired: 0},
    ],
  }));
}
for (const [name, bytes] of systemEntryForms) {
  continuations.push({...fault(`completed-nop-before-sep0-${name}`, [0x90, ...bytes]), retired: 1, offset: 1,
    schedules: [{}, {budget: 1, reason: 1}, {flags: 0x102, reason: 16, version: 7},
      {flags: 0x10002, stateVersion: 3, expectedFlags: 2}],
  });
  negatives.push({name: `truncated-${name}-opcode-before-sep-availability`, bytes: Buffer.from([0x0f]),
    target: 0x1fff, repair: Buffer.from([bytes[1]]), repaired: {reason: 20, baseVersion: 1}, x87});
}
negatives.push({name: 'duplicate-operand-prefix-sysenter-keeps-profile-refusal',
  bytes: Buffer.from([0x66, 0x66, 0x0f, 0x34]), x87});
runStoppedFaults({
  sourcePaths: [
    'engine/src/cpu/x86/decode/execution_faults/tests/availability/native.rs',
    'engine/src/cpu/x86/decode/execution_faults/tests/random.rs',
    'engine/src/cpu/x86/decode/execution_faults/tests/monitor.rs',
    'engine/src/cpu/x86/decode/execution_faults/tests/transactions.rs',
    'engine/src/cpu/x86/decode/execution_faults/tests/syscall.rs',
    'engine/src/cpu/x86/decode/execution_faults/tests/sep.rs',
    'engine/tests/cpu/control/wasm.rs',
    'engine/tests/cpu/control/exceptions/availability/wasm.rs',
    'engine/tests/cpu/control/exceptions/availability/fixtures/p2-availability/runtime/run.mjs',
  ],
  positive,
  continuations,
  negatives,
  claim:
    'virtual CPUID1.EDX.SEP11 absence requires SYSENTER/SYSEXIT UD independently of MSR presence, without entry-MSR values, IF-success effects or a return CPL GP; virtual CPUID80000001.EDX11 absence requires SYSCALL/SYSRET UD without EFER or OS-state assumptions; absent RTM XABORT/XBEGIN16/32/XEND and absent RTM+HLE XTEST require their exact CPUID-defined UD before transaction or fallback behavior; nonzero-CPL MONITOR address16/32 and MWAIT require UD without monitor-address or wait effects; unadvertised RDRAND16/32 and RDSEED16/32 require exact CPUID-defined UD with no entropy helper, RF consumed only on attempted instructions, plus fixed non-SMM RSM, defined EVEX vectors requiring XCR0&0xe7==0xe7, and VEX opmask instructions requiring XCR0&0xe3==0xe3 retire zero at the canonical current EIP as UD with current XCR0=7, without operand access; strict compilation remains a host refusal. Actual truncated-fetch preparation stays neutral until executable-page repair; companion native tests establish the precise Execute MemoryFault rather than claiming a guest PF packet from compile_stopped_entry.',
});
