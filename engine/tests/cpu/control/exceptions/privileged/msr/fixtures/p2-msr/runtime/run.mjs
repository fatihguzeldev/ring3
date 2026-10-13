import { runStoppedFaults } from '../../../../../support/stopped.mjs';

const positive = [
  { name: 'unchanged-cpl3-hlt-control', bytes: Buffer.from([0xf4]), reason: 11, baseVersion: 6 },
];
for (const [name, opcode] of [
  ['rdmsr', 0x32],
  ['wrmsr', 0x30],
]) {
  for (const prefix of [[], [0x66], [0x67], [0x26], [0x2e], [0x36], [0x3e], [0x64], [0x65]]) {
    const bytes = [...prefix, 0x0f, opcode],
      suffix = prefix.map((byte) => byte.toString(16)).join('') || 'plain';
    positive.push({
      name: `${name}-${suffix}`,
      bytes: Buffer.from(bytes),
      reason: 11,
      baseVersion: 6,
    });
    positive.push({
      name: `${name}-invalid-lock-${suffix}`,
      bytes: Buffer.from([0xf0, ...bytes]),
      reason: 20,
      baseVersion: 1,
    });
  }
  positive.push({
    name: `${name}-exact-last-code-byte`,
    bytes: Buffer.from([0x0f, opcode]),
    target: 0x1ffe,
    reason: 11,
    baseVersion: 6,
  });
}
const continuations = [0x32, 0x30].map((opcode) => ({
  name: 'completed-nop-before-msr-fault-' + opcode.toString(16),
  bytes: Buffer.from([0x90, 0x0f, opcode]),
  reason: 11,
  baseVersion: 6,
  retired: 1,
  offset: 1,
}));
const negatives = [];
for (const opcode of [0x32, 0x30])
  for (const prefix of [[0xf2], [0xf3], [0x66, 0x66]])
    negatives.push({
      name: `reserved-prefix-${prefix.map((byte) => byte.toString(16)).join('')}-${opcode.toString(16)}`,
      bytes: Buffer.from([...prefix, 0x0f, opcode]),
    });
negatives.push(
  { name: 'rdpmc-environment-remains-unsupported', bytes: Buffer.from([0x0f, 0x33]) },
  { name: 'in-environment-remains-unsupported', bytes: Buffer.from([0xe4, 0x80]) },
  { name: 'out-environment-remains-unsupported', bytes: Buffer.from([0xe6, 0x80]) },
  {
    name: 'incomplete-fetch-remains-typed-preparation-error',
    bytes: Buffer.from([0x0f]),
    target: 0x1fff,
    repair: Buffer.from([0x32]),
    repaired: { reason: 11, baseVersion: 6 },
  },
);

runStoppedFaults({
  sourcePaths: [
    'engine/tests/cpu/control/exceptions/privileged/msr/native.rs',
    'engine/tests/cpu/control/exceptions/privileged/msr/wasm.rs',
    'engine/tests/cpu/control/exceptions/privileged/msr/fixtures/p2-msr/runtime/run.mjs',
  ],
  positive,
  continuations,
  negatives,
  claim:
    'present RDMSR/WRMSR implicit forms fault GP0 at fixed CPL3 without MSR values, PC progress, retirement or operand effects; invalidLOCK stays UD, strict APIs/reserved prefixes/RDPMC/IO/fetch errors remain distinct',
});
