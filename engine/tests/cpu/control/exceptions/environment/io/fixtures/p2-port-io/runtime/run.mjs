import { runStoppedFaults } from '../../../../../support/stopped.mjs';

const forms = [
  ['in-byte-immediate', [0xe4, 0x80]],
  ['in-word-immediate', [0x66, 0xe5, 0x80]],
  ['in-dword-immediate', [0xe5, 0x80]],
  ['in-byte-dx', [0xec]],
  ['in-word-dx', [0x66, 0xed]],
  ['in-dword-dx', [0xed]],
  ['out-byte-immediate', [0xe6, 0x80]],
  ['out-word-immediate', [0x66, 0xe7, 0x80]],
  ['out-dword-immediate', [0xe7, 0x80]],
  ['out-byte-dx', [0xee]],
  ['out-word-dx', [0x66, 0xef]],
  ['out-dword-dx', [0xef]],
];
const positive = [
  { name: 'unchanged-cpl3-hlt-control', bytes: Buffer.from([0xf4]), reason: 11, baseVersion: 6 },
];
for (const [name, bytes] of forms) {
  positive.push({ name: name + '-denied', bytes: Buffer.from(bytes), reason: 11, baseVersion: 6 });
  positive.push({
    name: name + '-invalid-lock',
    bytes: Buffer.from([0xf0, ...bytes]),
    reason: 20,
    baseVersion: 1,
  });
  positive.push({
    name: name + '-exact-last-code-byte',
    bytes: Buffer.from(bytes),
    target: 0x2000 - bytes.length,
    reason: 11,
    baseVersion: 6,
  });
  for (const port of [0, name.endsWith('immediate') ? 255 : 65535]) {
    const encoded = [...bytes];
    if (name.endsWith('immediate')) encoded[encoded.length - 1] = port;
    positive.push({
      name: name + '-denied-port-' + port.toString(16),
      bytes: Buffer.from(encoded),
      registers: { 0: port === 0 ? 0 : 0xffffffff, 2: 0xabcd0000 | port },
      reason: 11,
      baseVersion: 6,
    });
  }
}
for (const [name, bytes] of [
  ['in-dword-dx', [0xed]],
  ['out-dword-immediate', [0xe7, 0x80]],
]) {
  for (const prefix of [0x67, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65]) {
    positive.push({
      name: name + '-ignored-' + prefix.toString(16),
      bytes: Buffer.from([prefix, ...bytes]),
      reason: 11,
      baseVersion: 6,
    });
  }
}
for (const [name, bytes] of forms.filter(([name]) => name.includes('-byte-'))) {
  positive.push({
    name: name + '-ignored66',
    bytes: Buffer.from([0x66, ...bytes]),
    reason: 11,
    baseVersion: 6,
  });
}
const continuations = forms
  .filter(([name]) => name.includes('-dword-'))
  .map(([name, bytes]) => ({
    name: 'completed-nop-before-' + name,
    bytes: Buffer.from([0x90, ...bytes]),
    reason: 11,
    baseVersion: 6,
    retired: 1,
    offset: 1,
  }));
const negatives = [];
for (const [name, bytes] of forms) {
  for (const prefix of [0xf2, 0xf3])
    negatives.push({
      name: name + '-reserved-' + prefix.toString(16),
      bytes: Buffer.from([prefix, ...bytes]),
    });
  const opcode = bytes[0] === 0x66 ? bytes.slice(1) : bytes;
  negatives.push({ name: name + '-duplicate66', bytes: Buffer.from([0x66, 0x66, ...opcode]) });
}
negatives.push({ name: 'duplicate-operand-prefix-int80-keeps-profile-refusal', bytes: Buffer.from([0x66, 0x66, 0xcd, 0x80]) });
for (const opcode of [0xe4, 0xe5, 0xe6, 0xe7]) {
  for (const prefix of [[], [0x66]]) {
    const bytes = [...prefix, opcode];
    negatives.push({
      name: 'missing-immediate-' + bytes.map((byte) => byte.toString(16)).join(''),
      bytes: Buffer.from(bytes),
      target: 0x2000 - bytes.length,
      repair: Buffer.from([0x80]),
      repaired: { reason: 11, baseVersion: 6 },
    });
  }
}
runStoppedFaults({
  sourcePaths: [
    'engine/tests/cpu/control/exceptions/environment/io/native.rs',
    'engine/tests/cpu/control/exceptions/environment/io/wasm.rs',
    'engine/tests/cpu/control/exceptions/environment/io/fixtures/p2-port-io/runtime/run.mjs',
  ],
  positive,
  continuations,
  negatives,
  claim:
    'baseline scalar IN/OUT forms fault GP0 under explicit trusted DenyAll user-port permissions at CPL3/IOPL0 without a fabricated TSS or device effect; strict APIs, invalidLOCK and immediate fetch faults remain distinct; conditional denied I/O strings have a separate context-owned proof',
});
