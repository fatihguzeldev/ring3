import assert from 'node:assert/strict';

const forms = [
  { kind: 1, map: 1, opcode: 0xd5, operation: 'low', lane: 2 },
  { kind: 2, map: 1, opcode: 0xe4, operation: 'high-unsigned', lane: 2 },
  { kind: 3, map: 1, opcode: 0xe5, operation: 'high-signed', lane: 2 },
  { kind: 4, map: 1, opcode: 0xf4, operation: 'dword-unsigned', lane: 4 },
  { kind: 5, map: 1, opcode: 0xf5, operation: 'word-pairs', lane: 2 },
  { kind: 6, map: 1, opcode: 0xf6, operation: 'sad', lane: 1 },
  { kind: 7, map: 2, opcode: 1, operation: 'horizontal-add', lane: 2 },
  { kind: 8, map: 2, opcode: 2, operation: 'horizontal-add', lane: 4 },
  { kind: 9, map: 2, opcode: 3, operation: 'horizontal-add-saturate', lane: 2 },
  { kind: 10, map: 2, opcode: 4, operation: 'byte-pairs', lane: 1 },
  { kind: 11, map: 2, opcode: 5, operation: 'horizontal-subtract', lane: 2 },
  { kind: 12, map: 2, opcode: 6, operation: 'horizontal-subtract', lane: 4 },
  {
    kind: 13,
    map: 2,
    opcode: 7,
    operation: 'horizontal-subtract-saturate',
    lane: 2,
  },
  { kind: 14, map: 2, opcode: 0x0b, operation: 'round-signed', lane: 2 },
  { kind: 15, map: 2, opcode: 0x28, operation: 'dword-signed', lane: 4 },
  { kind: 16, map: 2, opcode: 0x40, operation: 'low', lane: 4 },
];
function read(bytes, offset, width, signed = false) {
  let value = 0n;
  for (let byte = width - 1; byte >= 0; byte--)
    value = value * 256n + BigInt(bytes[offset + byte]);
  return signed ? BigInt.asIntN(width * 8, value) : value;
}
function write(bytes, offset, width, value) {
  for (let byte = 0; byte < width; byte++)
    bytes[offset + byte] = Number(BigInt.asUintN(8, value >> BigInt(byte * 8)));
}
const clamp = (value) =>
  value < -32768n ? -32768n : value > 32767n ? 32767n : value;

// independent BigInt integer equations; no CPU/backend execution supplies expected bytes.
function oracle(form, first, second) {
  const result = Buffer.alloc(32);
  for (const half of [0, 16]) {
    if (form.operation.startsWith('horizontal-')) {
      const width = form.lane,
        halfLanes = 8 / width;
      for (let lane = 0; lane < 16 / width; lane++) {
        const input = lane < halfLanes ? first : second;
        const at = half + (lane % halfLanes) * width * 2;
        const x = read(input, at, width, true),
          y = read(input, at + width, width, true);
        let value = form.operation.includes('subtract') ? x - y : x + y;
        if (form.operation.endsWith('saturate')) value = clamp(value);
        write(result, half + lane * width, width, value);
      }
      continue;
    }
    const stride =
      form.operation.startsWith('dword-') || form.operation === 'sad'
        ? 8
        : form.operation === 'word-pairs'
          ? 4
          : form.operation === 'byte-pairs'
            ? 2
            : form.lane;
    for (let at = half; at < half + 16; at += stride) {
      let value;
      switch (form.operation) {
        case 'dword-unsigned':
        case 'dword-signed':
          value =
            read(first, at, 4, form.operation === 'dword-signed') *
            read(second, at, 4, form.operation === 'dword-signed');
          break;
        case 'word-pairs':
          value =
            read(first, at, 2, true) * read(second, at, 2, true) +
            read(first, at + 2, 2, true) * read(second, at + 2, 2, true);
          break;
        case 'byte-pairs':
          value = clamp(
            read(first, at, 1) * read(second, at, 1, true) +
              read(first, at + 1, 1) * read(second, at + 1, 1, true),
          );
          break;
        case 'sad':
          value = 0n;
          for (let byte = 0; byte < 8; byte++)
            value += BigInt(Math.abs(first[at + byte] - second[at + byte]));
          break;
        case 'low':
          value = read(first, at, form.lane) * read(second, at, form.lane);
          break;
        case 'high-unsigned':
          value = (read(first, at, 2) * read(second, at, 2)) >> 16n;
          break;
        case 'high-signed':
          value = (read(first, at, 2, true) * read(second, at, 2, true)) >> 16n;
          break;
        case 'round-signed':
          value =
            (read(first, at, 2, true) * read(second, at, 2, true) + 16384n) >>
            15n;
          break;
        default:
          throw Error(form.operation);
      }
      write(result, at, stride, value);
    }
  }
  return result;
}
function encode(
  form,
  destination,
  first,
  second,
  { wide = true, w = false, prefix = [], address16 = false } = {},
) {
  return Buffer.from([
    ...prefix,
    ...(address16 ? [0x67] : []),
    0xc4,
    0xe0 | form.map,
    ((15 - first) << 3) | (wide ? 4 : 0) | (w ? 0x80 : 0) | 1,
    form.opcode,
    (destination << 3) |
      (second === null ? (address16 ? 7 : 0) : 0xc0 | second),
  ]).toString('hex');
}
function pattern(register, seed = 0) {
  const values = [0, 255, 128, 127, 1, 254, 129, 85];
  return Buffer.from(
    Array.from(
      { length: 32 },
      (_, byte) =>
        values[
          (byte + register * 3 + seed + Math.floor(byte / 16) * 5) %
            values.length
        ],
    ),
  );
}
function segments({ segment = 3, limit, fsBase = 0 } = {}) {
  const value = Buffer.alloc(64);
  value.write('R3SG');
  value.writeUInt16LE(2, 4);
  value.writeUInt16LE(1, 6);
  value.writeUInt32LE(64, 8);
  [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((selector, index) =>
    value.writeUInt16LE(selector, 16 + index * 2),
  );
  const caches = [
    0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0,
  ];
  if (limit !== undefined) caches[segment] = 0xf2300000 + limit;
  [28, 32, 36, 40, 44, 56].forEach((at, index) =>
    value.writeUInt32LE(caches[index], at),
  );
  value.writeUInt32LE(fsBase, 48);
  return value;
}
function row(form, name, destination, first, second, options = {}) {
  const vectors = Array.from({ length: 8 }, (_, register) =>
    pattern(register, options.w ? 3 : 0),
  );
  const operand = pattern(3, 1);
  const value = oracle(
    form,
    vectors[first],
    second === null ? operand : vectors[second],
  );
  return {
    name,
    code: encode(form, destination, first, second, options),
    mxcsr: 0,
    lower: Object.fromEntries(
      vectors.map((value, index) => [index, value.subarray(0, 16)]),
    ),
    upper: Object.fromEntries(
      vectors.map((value, index) => [index, value.subarray(16)]),
    ),
    ...(second === null ? { address: 0x5fe0, read: 32, memory: operand } : {}),
    outLower: { [destination]: value.subarray(0, 16) },
    outUpper: {
      [destination]:
        options.wide === false ? Buffer.alloc(16) : value.subarray(16),
    },
  };
}
export const multiplyRows = [];
for (const form of forms) {
  for (const [destination, first, second] of [
    [0, 1, 2],
    [1, 1, 2],
    [2, 1, 2],
    [0, 1, 1],
    [0, 0, 0],
    [7, 5, 6],
  ]) {
    for (const w of [false, true])
      multiplyRows.push(
        row(
          form,
          `multiply-${form.kind}-alias${destination}/${first}/${second}-w${+w}`,
          destination,
          first,
          second,
          { w },
        ),
      );
  }
  const normal = row(form, `multiply-${form.kind}-exact-last32`, 1, 1, null);
  multiplyRows.push(normal);
  multiplyRows.push({
    ...normal,
    name: `multiply-${form.kind}-unaligned32`,
    address: 0x5003,
  });
  multiplyRows.push({
    ...normal,
    name: `multiply-${form.kind}-address16-initial-truncation`,
    code: encode(form, 1, 1, null, { address16: true }),
    r: { 3: 0xaabb5fe0 },
  });
  multiplyRows.push({
    ...normal,
    name: `multiply-${form.kind}-fs-linear-offset`,
    code: encode(form, 1, 1, null, { prefix: [0x64] }),
    r: { 0: 0xfe0 },
    segmentRecord: segments({ fsBase: 0x5000 }),
  });
  for (const [name, address, reason, extra] of [
    [
      'late-byte-unmapped',
      0x5fe1,
      5,
      { detail: 1, fault: 0x6000, access: 1, length: 32 },
    ],
    [
      'ds-limit',
      0x5fe0,
      11,
      { helpers: 0, segmentRecord: segments({ limit: 0x5fef }) },
    ],
    [
      'ss-limit-before-page',
      0x6000,
      15,
      {
        helpers: 0,
        code: encode(form, 1, 1, null, { prefix: [0x36] }),
        segmentRecord: segments({ segment: 2, limit: 0x5fff }),
      },
    ],
    ['full-logical-span-overflow', 0xffffffff, 11, { helpers: 0 }],
  ])
    multiplyRows.push({
      name: `multiply-${form.kind}-${name}`,
      code: normal.code,
      lower: normal.lower,
      upper: normal.upper,
      mxcsr: 0,
      address,
      read: 32,
      reason,
      ...extra,
    });
  multiplyRows.push(
    row(form, `old-xmm-multiply-${form.kind}`, 0, 1, 2, { wide: false }),
  );
}
function literal(kind, name, first, second, expected) {
  const form = forms.find((form) => form.kind === kind);
  assert.deepEqual(
    oracle(form, first, second),
    expected,
    `${name}: independent hand-derived literal`,
  );
  multiplyRows.push({
    name,
    code: encode(form, 0, 1, 2),
    mxcsr: 0,
    lower: { 1: first.subarray(0, 16), 2: second.subarray(0, 16) },
    upper: { 1: first.subarray(16), 2: second.subarray(16) },
    outLower: { 0: expected.subarray(0, 16) },
    outUpper: { 0: expected.subarray(16) },
  });
}
const repeat = (hex, count) => Buffer.from(hex.repeat(count), 'hex');
literal(
  5,
  'literal-pmaddwd-signed-minimum-wrap',
  repeat('0080', 16),
  repeat('0080', 16),
  repeat('00000080', 8),
);
literal(
  14,
  'literal-pmulhrsw-minimum-wrap',
  repeat('0080', 16),
  repeat('0080', 16),
  repeat('0080', 16),
);
literal(
  14,
  'literal-pmulhrsw-ties',
  repeat('0100ffff0300fdff', 4),
  repeat('0040', 16),
  repeat('010000000200ffff', 4),
);
literal(
  10,
  'literal-maddubsw-signed-saturation',
  Buffer.alloc(32, 255),
  repeat('7f7f8080', 8),
  repeat('ff7f0080', 8),
);
literal(
  6,
  'literal-sad-2040',
  Buffer.alloc(32),
  Buffer.alloc(32, 255),
  repeat('f807000000000000', 4),
);
literal(
  9,
  'literal-horizontal-add-saturation',
  repeat('ff7f01000080ffff00000100ffffffff', 2),
  repeat('ff7f01000080ffff00000100ffffffff', 2),
  repeat('ff7f00800100feffff7f00800100feff', 2),
);
assert.equal(forms.length, 16);
assert.equal(multiplyRows.length, 342);
assert.equal(multiplyRows.filter((row) => row.reason).length, 64);
export const multiplySourcePaths = [
  'engine/src/cpu/x86/avx/multiply.rs',
  'engine/src/cpu/x86/avx/operation/factory/multiply.rs',
  'engine/src/cpu/x86/decode/avx/multiply.rs',
  'engine/src/cpu/x86/simd/multiply.rs',
  'engine/src/cpu/x86/simd/packed.rs',
  'engine/src/abi/x86/segments.rs',
  'engine/src/cpu/dbt/wasm/segments/access.rs',
  'engine/tests/cpu/simd/avx/execution/wasm.rs',
  'engine/tests/cpu/simd/avx/multiply/fixtures/p2-avx2-multiply/runtime/cases.mjs',
];
