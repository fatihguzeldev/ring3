import assert from 'node:assert/strict';

const operations = [
  {name: 'and', opcode: 0xdb, apply: (a, b) => a & b},
  {name: 'andnot', opcode: 0xdf, apply: (a, b) => ~a & b},
  {name: 'or', opcode: 0xeb, apply: (a, b) => a | b},
  {name: 'xor', opcode: 0xef, apply: (a, b) => a ^ b},
];
const encodings = ['c5', 'c4-w0', 'c4-w1'];
const pattern = seed => Buffer.from(Array.from({length: 16}, (_, i) => (seed + i * 13) & 255));
const combine = (operation, a, b) => Buffer.from(a.map((value, i) => operation.apply(value, b[i])));

function encode(operation, encoding, destination, first, second, prefix = []) {
  const control = 0x85 | (15 - first) << 3;
  const vex = encoding === 'c5'
    ? [0xc5, control]
    : [0xc4, 0xe1, control & 0x7f | (encoding === 'c4-w1' ? 0x80 : 0)];
  const operand = second === null ? destination << 3 : 0xc0 | destination << 3 | second;
  return Buffer.from([...prefix, ...vex, operation.opcode, operand]).toString('hex');
}

export const logicalRows = [];
for (const [operationIndex, operation] of operations.entries()) {
  for (const [encodingIndex, encoding] of encodings.entries()) {
    const base = (operationIndex * 2 + encodingIndex) % 8;
    const next = (base + 1) % 8;
    const other = (base + 2) % 8;
    const aliases = [
      {name: 'distinct', destination: base, first: next, second: other},
      {name: 'destination-first', destination: next, first: next, second: other},
      {name: 'destination-second', destination: other, first: next, second: other},
      {name: 'sources-equal', destination: base, first: next, second: next},
      {name: 'all-equal', destination: next, first: next, second: next},
    ];
    for (const {name, destination, first, second} of aliases) {
      const firstLower = pattern(0x15);
      const firstUpper = pattern(0xa1);
      const secondLower = first === second ? firstLower : pattern(0x83);
      const secondUpper = first === second ? firstUpper : pattern(0x39);
      logicalRows.push({
        name: `avx2-${operation.name}-${encoding}-${name}-${destination}-${first}-${second}`,
        code: encode(operation, encoding, destination, first, second),
        lower: {[first]: firstLower, [second]: secondLower},
        upper: {[first]: firstUpper, [second]: secondUpper},
        outLower: {[destination]: combine(operation, firstLower, secondLower)},
        outUpper: {[destination]: combine(operation, firstUpper, secondUpper)},
      });
    }
  }

  const destination = operationIndex * 2;
  const first = destination + 1;
  const firstLower = pattern(0x15);
  const firstUpper = pattern(0xa1);
  const memoryLower = pattern(0x83);
  const memoryUpper = pattern(0x39);
  for (const address of [0x5011, 0x5fe0]) {
    logicalRows.push({
      name: `avx2-${operation.name}-memory-exact32-${address.toString(16)}`,
      code: encode(operation, 'c5', destination, first, null),
      address,
      read: 32,
      memory: Buffer.concat([memoryLower, memoryUpper]),
      lower: {[first]: firstLower},
      upper: {[first]: firstUpper},
      outLower: {[destination]: combine(operation, firstLower, memoryLower)},
      outUpper: {[destination]: combine(operation, firstUpper, memoryUpper)},
    });
  }
  for (const address of [0x5fe1, 0x6000]) {
    logicalRows.push({
      name: `avx2-${operation.name}-memory-page-fault-${address.toString(16)}`,
      code: encode(operation, 'c5', destination, first, null),
      address,
      read: 32,
      reason: 5,
      detail: 1,
      fault: 0x6000,
      access: 1,
      length: 32,
    });
  }
  for (const [segment, prefix, reason] of [['ds', [], 11], ['ss', [0x36], 15]]) {
    logicalRows.push({
      name: `avx2-${operation.name}-${segment}-span-overflow-before-page-access`,
      code: encode(operation, 'c5', destination, first, null, prefix),
      address: 0xfffffff0,
      read: 32,
      helpers: 0,
      reason,
    });
  }
}

assert.equal(logicalRows.length, 84);
assert.equal(new Set(logicalRows.map(row => row.name)).size, logicalRows.length);
assert.equal(logicalRows.filter(row => row.reason).length, 16);
assert.equal(logicalRows.filter(row => row.memory).length, 8);
assert.equal(logicalRows.filter(row => !row.read).length, 60);
export const logicalSourcePaths = [
  'engine/src/cpu/x86/decode/avx/logical.rs',
  'engine/src/cpu/x86/avx/operation/factory/logical.rs',
  'engine/tests/cpu/simd/avx/logical/fixtures/p2-avx2-logical/runtime/cases.mjs',
];
