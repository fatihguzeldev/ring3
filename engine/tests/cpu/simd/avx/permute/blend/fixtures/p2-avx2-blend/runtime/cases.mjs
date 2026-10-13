import assert from 'node:assert/strict';

function words(values) {
  const output = Buffer.alloc(32);
  values.forEach((value, lane) => output.writeUInt32LE(value >>> 0, lane * 4));
  return output;
}
function pattern(register) {
  const values = [
    0x7f800001, 0x80000000, 1, 0xdeadbeef, 0x7fc01234, 0xff800001, 0x3f800000, 0x12345678,
  ];
  return words(values.map((_, lane) => values[(lane + register) % 8]));
}
function select(first, second, immediate, width) {
  const output = Buffer.alloc(32);
  for (let lane = 0; lane < width / 4; lane++) {
    const source = immediate & (1 << lane) ? second : first;
    source.copy(output, lane * 4, lane * 4, lane * 4 + 4);
  }
  return output;
}
function encode(
  width,
  [destination, first, second],
  immediate,
  { memory = false, prefix = [], address16 = null } = {},
) {
  const modrm = memory
    ? address16 === null
      ? [destination << 3]
      : [(destination << 3) | 6, address16 & 255, address16 >>> 8]
    : [0xc0 | (destination << 3) | second];
  return Buffer.from([
    ...prefix,
    ...(address16 === null ? [] : [0x67]),
    0xc4,
    0xe3,
    ((15 - first) << 3) | (width === 32 ? 4 : 0) | 1,
    2,
    ...modrm,
    immediate,
  ]).toString('hex');
}
function output(destination, value) {
  return {
    outLower: { [destination]: value.subarray(0, 16) },
    outUpper: { [destination]: value.subarray(16) },
  };
}
const a = words([10, 11, 12, 13, 20, 21, 22, 23]);
const b = words([30, 31, 32, 33, 40, 41, 42, 43]);
assert.deepEqual(select(a, b, 0xf0, 16), words([10, 11, 12, 13]));
assert.deepEqual(select(a, b, 0xf0, 32), words([10, 11, 12, 13, 40, 41, 42, 43]));
assert.deepEqual(select(a, b, 0x55, 32), words([30, 11, 32, 13, 40, 21, 42, 23]));
export const blendRows = [];
for (const width of [16, 32]) {
  for (const roles of [
    [7, 6, 5],
    [6, 6, 5],
    [5, 6, 5],
    [7, 6, 6],
    [6, 6, 6],
  ]) {
    for (const control of [0, 1, 15, 16, 128, 0x55, 0xaa, 255]) {
      const vectors = Array.from({ length: 8 }, (_, register) => pattern(register));
      blendRows.push({
        name: `blend-${width}-${roles.join('/')}-${control}`,
        code: encode(width, roles, control),
        lower: Object.fromEntries(
          vectors.map((value, register) => [register, value.subarray(0, 16)]),
        ),
        upper: Object.fromEntries(vectors.map((value, register) => [register, value.subarray(16)])),
        ...output(roles[0], select(vectors[roles[1]], vectors[roles[2]], control, width)),
      });
    }
  }
  const first = pattern(0),
    second = pattern(3),
    address = 0x6000 - width;
  const memoryRow = (at, control, options = {}) => ({
    name: `blend-${width}-memory-${at.toString(16)}-${control}`,
    code: encode(width, [0, 0, 0], control, { memory: true }),
    address: at,
    read: width,
    memory: second.subarray(0, width),
    lower: { 0: first.subarray(0, 16) },
    upper: { 0: first.subarray(16) },
    ...output(0, select(first, second, control, width)),
    ...options,
  });
  for (const control of [0, 255]) blendRows.push(memoryRow(address, control));
  blendRows.push(memoryRow(0x5011, 0x55));
  blendRows.push(
    memoryRow(0x5013, 0xaa, {
      code: encode(width, [0, 0, 0], 0xaa, { memory: true, address16: 0x5013 }),
    }),
  );
  blendRows.push(
    memoryRow(address + 1, 0, {
      memory: second.subarray(0, width - 1),
      reason: 5,
      detail: 1,
      fault: 0x6000,
      access: 1,
      length: width,
    }),
  );
  for (const [segment, prefix, reason] of [
    ['ds', [], 11],
    ['ss', [0x36], 15],
  ]) {
    const fault = memoryRow(0x100000001 - width, 0, {
      name: `blend-${width}-${segment}-span-before-page`,
      code: encode(width, [0, 0, 0], 0, { memory: true, prefix }),
      reason,
      helpers: 0,
    });
    delete fault.memory;
    blendRows.push(fault);
  }
  const old = encode(width, [0, 1, 2], 0x55);
  blendRows.push({
    name: `old-vblendps-${width}`,
    code: old.slice(0, 6) + '0c' + old.slice(8),
    lower: { 1: a.subarray(0, 16), 2: b.subarray(0, 16) },
    upper: { 1: a.subarray(16), 2: b.subarray(16) },
    ...output(0, select(a, b, 0x55, width)),
  });
}
assert.equal(blendRows.length, 96);
assert.equal(new Set(blendRows.map((row) => row.name)).size, blendRows.length);
assert.equal(blendRows.filter((row) => row.reason).length, 6);
export const blendSourcePaths = [
  'engine/src/cpu/x86/avx/permute.rs',
  'engine/src/cpu/x86/avx/operation/factory/permute.rs',
  'engine/src/cpu/x86/decode/avx/permute.rs',
  'engine/tests/cpu/simd/avx/permute/blend/native.rs',
  'engine/tests/cpu/simd/avx/permute/blend/fixtures/p2-avx2-blend/runtime/cases.mjs',
];
