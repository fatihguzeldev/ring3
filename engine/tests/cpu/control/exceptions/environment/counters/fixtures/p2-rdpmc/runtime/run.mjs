import { runStoppedFaults } from '../../../../../support/stopped.mjs';

const positive = [
  { name: 'unchanged-cpl3-hlt-control', bytes: Buffer.from([0xf4]), reason: 11, baseVersion: 6 },
];
for (const prefix of [[], [0x66], [0x67], [0x26], [0x2e], [0x36], [0x3e], [0x64], [0x65]]) {
  const bytes = [...prefix, 0x0f, 0x33];
  const suffix = prefix.map((byte) => byte.toString(16)).join('') || 'plain';
  positive.push({
    name: 'rdpmc-pce0-' + suffix,
    bytes: Buffer.from(bytes),
    reason: 11,
    baseVersion: 6,
  });
  positive.push({
    name: 'rdpmc-invalid-lock-' + suffix,
    bytes: Buffer.from([0xf0, ...bytes]),
    reason: 20,
    baseVersion: 1,
  });
}
for (const ecx of [0, 1, 0x40000000, 0x80000000, 0xffffffff]) {
  positive.push({
    name: 'rdpmc-pce0-ecx-' + ecx.toString(16),
    bytes: Buffer.from([0x0f, 0x33]),
    registers: { 0: 0xffffffff, 1: ecx, 2: 0x80000000 },
    reason: 11,
    baseVersion: 6,
  });
}
positive.push({
  name: 'rdpmc-exact-last-code-byte',
  bytes: Buffer.from([0x0f, 0x33]),
  target: 0x1ffe,
  reason: 11,
  baseVersion: 6,
});
const continuations = [
  {
    name: 'completed-nop-before-rdpmc-fault',
    bytes: Buffer.from([0x90, 0x0f, 0x33]),
    reason: 11,
    baseVersion: 6,
    retired: 1,
    offset: 1,
  },
];
const negatives = [[0xf2], [0xf3], [0x66, 0x66]].map((prefix) => ({
  name: 'rdpmc-reserved-prefix-' + prefix.map((byte) => byte.toString(16)).join(''),
  bytes: Buffer.from([...prefix, 0x0f, 0x33]),
}));
negatives.push(
  { name: 'valid-unsupported-int80', bytes: Buffer.from([0xcd, 0x80]) },
  {
    name: 'incomplete-rdpmc-fetch-remains-typed-preparation-error',
    bytes: Buffer.from([0x0f]),
    target: 0x1fff,
    repair: Buffer.from([0x33]),
    repaired: { reason: 11, baseVersion: 6 },
  },
);
runStoppedFaults({
  sourcePaths: [
    'engine/tests/cpu/control/exceptions/environment/counters/native.rs',
    'engine/tests/cpu/control/exceptions/environment/counters/wasm.rs',
    'engine/tests/cpu/control/exceptions/environment/counters/fixtures/p2-rdpmc/runtime/run.mjs',
  ],
  positive,
  continuations,
  negatives,
  claim:
    'present RDPMC with immutable PCE0 faults GP0 at CPL3 for every ECX without counter values or GPR publication; strict APIs, invalidLOCK and fetch faults remain distinct; conditional denied I/O strings have a separate context-owned proof',
});
