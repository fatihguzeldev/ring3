import assert from 'node:assert/strict';

function pattern(register) {
  return Buffer.from(
    Array.from(
      { length: 32 },
      (_, byte) => ((byte * 13 + register * 17) ^ (Math.floor(byte / 16) * 0xa5)) & 255,
    ),
  );
}
function words(values) {
  const output = Buffer.alloc(32);
  values.forEach((value, lane) => output.writeUInt16LE(value, lane * 2));
  return output;
}

// independent bounded integer sums; the upper half uses a different immediate field.
function oracle(first, second, immediate, width = 32) {
  const output = Buffer.alloc(32);
  for (let half = 0; half < width / 16; half++) {
    const control = (immediate >>> (half * 3)) & 7;
    const a = half * 16 + ((control >>> 2) & 1) * 4;
    const b = half * 16 + (control & 3) * 4;
    for (let lane = 0; lane < 8; lane++) {
      let sum = 0;
      for (let byte = 0; byte < 4; byte++)
        sum += Math.abs(first[a + lane + byte] - second[b + byte]);
      output.writeUInt16LE(sum, half * 16 + lane * 2);
    }
  }
  return output;
}
function encode(
  width,
  [destination, first, second],
  immediate,
  { memory = false, w = false, prefix = [], address16 = null } = {},
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
    ((15 - first) << 3) | (width === 32 ? 4 : 0) | (w ? 0x80 : 0) | 1,
    0x42,
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
function row(name, width, roles, immediate, options = {}) {
  const vectors = Array.from({ length: 8 }, (_, register) => pattern(register));
  return {
    name,
    code: encode(width, roles, immediate, options),
    lower: Object.fromEntries(vectors.map((value, register) => [register, value.subarray(0, 16)])),
    upper: Object.fromEntries(vectors.map((value, register) => [register, value.subarray(16)])),
    ...output(roles[0], oracle(vectors[roles[1]], vectors[roles[2]], immediate, width)),
  };
}
const first = Buffer.from(Array.from({ length: 32 }, (_, byte) => byte));
const second = Buffer.from(Array.from({ length: 32 }, (_, byte) => byte + 32));
const literal = words([
  128, 124, 120, 116, 112, 108, 104, 100, 144, 140, 136, 132, 128, 124, 120, 116,
]);
assert.deepEqual(oracle(first, second, 8), literal);
assert.deepEqual(oracle(first, second, 200), literal);
assert.notDeepEqual(literal.subarray(0, 16), literal.subarray(16));
assert.deepEqual(oracle(Buffer.alloc(32), Buffer.alloc(32, 255), 63), words(Array(16).fill(1020)));
export const sadRows = [];
const aliases = [
  [7, 6, 5],
  [6, 6, 5],
  [5, 6, 5],
  [7, 6, 6],
  [6, 6, 6],
];
for (let control = 0; control < 64; control++) {
  for (const [alias, roles] of aliases.entries())
    sadRows.push(
      row(`sad-controls-${control}-alias-${alias}`, 32, roles, control, {
        w: Boolean((control + alias) & 1),
      }),
    );
}
for (const control of [0, 8, 32, 63]) {
  for (const ignored of [0, 64, 128, 192])
    sadRows.push(row(`sad-ignored-${control}-${ignored}`, 32, [7, 6, 5], control | ignored));
}
for (const immediate of [8, 200]) {
  sadRows.push({
    name: `sad-literal-independent-halves-${immediate}`,
    code: encode(32, [0, 1, 2], immediate),
    lower: { 1: first.subarray(0, 16), 2: second.subarray(0, 16) },
    upper: { 1: first.subarray(16), 2: second.subarray(16) },
    ...output(0, literal),
  });
}
sadRows.push({
  name: 'sad-literal-unsigned-maximum-1020',
  code: encode(32, [0, 1, 2], 63),
  lower: { 1: Buffer.alloc(16), 2: Buffer.alloc(16, 255) },
  upper: { 1: Buffer.alloc(16), 2: Buffer.alloc(16, 255) },
  ...output(0, words(Array(16).fill(1020))),
});
for (const immediate of [0, 8, 200, 255])
  sadRows.push(
    row(`old-xmm-sad-upperzero-${immediate}`, 16, [6, 6, 5], immediate, {
      w: Boolean(immediate & 1),
    }),
  );
const source = pattern(3);
function memoryRow(name, address, immediate, options = {}) {
  const input = pattern(0);
  return {
    name,
    code: encode(32, [0, 0, 0], immediate, { memory: true }),
    address,
    read: 32,
    memory: source,
    lower: { 0: input.subarray(0, 16) },
    upper: { 0: input.subarray(16) },
    ...output(0, oracle(input, source, immediate)),
    ...options,
  };
}
for (const immediate of [0, 63])
  sadRows.push(memoryRow(`sad-memory-exact-final32-${immediate}`, 0x5fe0, immediate));
sadRows.push(
  memoryRow('sad-memory-unaligned', 0x5011, 8, {
    code: encode(32, [0, 0, 0], 8, { memory: true, w: true }),
  }),
);
sadRows.push(
  memoryRow('sad-memory-address16', 0x5013, 63, {
    code: encode(32, [0, 0, 0], 63, { memory: true, address16: 0x5013 }),
  }),
);
for (const [name, address, immediate] of [
  ['last-byte-unmapped', 0x5fe1, 0],
  ['upper-half-unmapped', 0x5ff0, 63],
]) {
  sadRows.push(
    memoryRow(`sad-memory-${name}`, address, immediate, {
      memory: source.subarray(0, 0x6000 - address),
      reason: 5,
      detail: 1,
      fault: 0x6000,
      access: 1,
      length: 32,
    }),
  );
}
for (const [segment, prefix, reason, immediate] of [
  ['ds', [], 11, 0],
  ['ss', [0x36], 15, 63],
]) {
  const fault = memoryRow(`sad-${segment}-fullspan-before-page`, 0xffffffe1, immediate, {
    code: encode(32, [0, 0, 0], immediate, { memory: true, prefix }),
    reason,
    helpers: 0,
  });
  delete fault.memory;
  sadRows.push(fault);
}
assert.equal(sadRows.length, 351);
assert.equal(new Set(sadRows.map((row) => row.name)).size, sadRows.length);
assert.equal(sadRows.filter((row) => row.reason).length, 4);
export const sadSourcePaths = [
  'engine/src/cpu/x86/avx/multiply.rs',
  'engine/src/cpu/x86/avx/operation/factory/multiply.rs',
  'engine/src/cpu/x86/decode/avx/multiply.rs',
  'engine/src/cpu/x86/simd/multiply.rs',
  'engine/src/cpu/x86/simd/packed.rs',
  'engine/tests/cpu/simd/avx/multiply/sad/native.rs',
  'engine/tests/cpu/simd/avx/multiply/sad/fixtures/p2-avx2-sad/runtime/cases.mjs',
];
