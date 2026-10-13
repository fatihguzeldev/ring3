import assert from 'node:assert/strict';

const forms = [
  { opcode: 0x71, group: 2, kind: 1, lane: 2, direction: 'right' },
  { opcode: 0x71, group: 4, kind: 2, lane: 2, direction: 'arithmetic' },
  { opcode: 0x71, group: 6, kind: 3, lane: 2, direction: 'left' },
  { opcode: 0x72, group: 2, kind: 4, lane: 4, direction: 'right' },
  { opcode: 0x72, group: 4, kind: 5, lane: 4, direction: 'arithmetic' },
  { opcode: 0x72, group: 6, kind: 6, lane: 4, direction: 'left' },
  { opcode: 0x73, group: 2, kind: 7, lane: 8, direction: 'right' },
  { opcode: 0x73, group: 3, kind: 8, lane: 1, direction: 'bytes-right' },
  { opcode: 0x73, group: 6, kind: 9, lane: 8, direction: 'left' },
  { opcode: 0x73, group: 7, kind: 10, lane: 1, direction: 'bytes-left' },
  { opcode: 0xd1, group: 0, kind: 11, lane: 2, direction: 'right' },
  { opcode: 0xd2, group: 0, kind: 12, lane: 4, direction: 'right' },
  { opcode: 0xd3, group: 0, kind: 13, lane: 8, direction: 'right' },
  { opcode: 0xe1, group: 0, kind: 14, lane: 2, direction: 'arithmetic' },
  { opcode: 0xe2, group: 0, kind: 15, lane: 4, direction: 'arithmetic' },
  { opcode: 0xf1, group: 0, kind: 16, lane: 2, direction: 'left' },
  { opcode: 0xf2, group: 0, kind: 17, lane: 4, direction: 'left' },
  { opcode: 0xf3, group: 0, kind: 18, lane: 8, direction: 'left' },
];
const immediate = (form) => form.kind <= 10;
const boundary = (form) => BigInt(form.lane === 1 ? 16 : form.lane * 8);

function encode(
  form,
  encoding,
  [destination, source, count],
  value,
  width = 32,
  memory = false,
  prefix = [],
) {
  const vvvv = 15 - (immediate(form) ? destination : source);
  const control = (vvvv << 3) | (width === 32 ? 4 : 0) | 1;
  const vex =
    encoding === 'c5'
      ? [0xc5, control | 0x80]
      : [0xc4, 0xe1, control | (encoding === 'w1' ? 0x80 : 0)];
  const modrm = immediate(form)
    ? 0xc0 | (form.group << 3) | source
    : (memory ? 0 : 0xc0) | (destination << 3) | (memory ? 0 : count);
  return Buffer.from([
    ...prefix,
    ...vex,
    form.opcode,
    modrm,
    ...(immediate(form) ? [Number(value)] : []),
  ]).toString('hex');
}

function pattern(form, register) {
  if (form.lane === 1)
    return Buffer.from(Array.from({ length: 32 }, (_, byte) => (byte + 1 + register * 37) & 255));
  const sign = 1n << BigInt(form.lane * 8 - 1),
    mask = sign * 2n - 1n;
  const numbers = [
    mask,
    sign,
    sign - 1n,
    1n,
    0n,
    mask - 1n,
    sign + 1n,
    mask / 3n,
    2n,
    mask / 2n + 3n,
    mask - 3n,
  ];
  const result = Buffer.alloc(32);
  for (let at = 0; at < 32; at += form.lane) {
    let value = numbers[(at / form.lane + register * 3) % numbers.length];
    for (let byte = 0; byte < form.lane; byte++) {
      result[at + byte] = Number(value & 255n);
      value >>= 8n;
    }
  }
  return result;
}

function oracle(form, source, count, width = 32) {
  const output = Buffer.alloc(32);
  if (form.lane === 1) {
    if (count >= 16n) return output;
    for (let group = 0; group < width; group += 16) {
      for (let index = 0; index < 16; index++) {
        const input =
          form.direction === 'bytes-left' ? index - Number(count) : index + Number(count);
        if (input >= 0 && input < 16) output[group + index] = source[group + input];
      }
    }
    return output;
  }
  const bits = BigInt(form.lane * 8),
    modulus = 1n << bits,
    mask = modulus - 1n;
  for (let at = 0; at < width; at += form.lane) {
    let value = 0n;
    for (let byte = form.lane - 1; byte >= 0; byte--)
      value = value * 256n + BigInt(source[at + byte]);
    if (form.direction === 'arithmetic') {
      const signed = value >= modulus / 2n ? value - modulus : value;
      value = (signed >> (count >= bits ? bits - 1n : count)) & mask;
    } else if (count >= bits) value = 0n;
    else value = form.direction === 'left' ? (value << count) & mask : value >> count;
    for (let byte = 0; byte < form.lane; byte++) {
      output[at + byte] = Number(value & 255n);
      value >>= 8n;
    }
  }
  return output;
}

function registerRow(form, encoding, roles, count, width = 32, high64 = 255, label = '') {
  const [destination, source, countRegister] = roles;
  const vectors = Array.from({ length: 8 }, (_, register) => pattern(form, register));
  if (!immediate(form)) {
    vectors[countRegister].writeBigUInt64LE(count);
    vectors[countRegister].fill(high64, 8, 16);
    vectors[countRegister].fill(0xb7, 16);
  }
  const expected = oracle(form, vectors[source], count, width);
  return {
    name: `avx2-uniform-${form.kind}-${encoding}-${roles.join('/')}-${count}-${width}-${high64}-${label}`,
    code: encode(form, encoding, roles, count, width),
    lower: Object.fromEntries(vectors.map((value, register) => [register, value.subarray(0, 16)])),
    upper: Object.fromEntries(vectors.map((value, register) => [register, value.subarray(16)])),
    outLower: { [destination]: expected.subarray(0, 16) },
    outUpper: { [destination]: expected.subarray(16) },
  };
}

export const uniformRows = [];
for (const [formIndex, form] of forms.entries()) {
  const edge = boundary(form);
  if (immediate(form)) {
    for (const count of [0n, 1n, edge - 1n, edge, 255n]) {
      for (const roles of [
        [7, 6, 0],
        [7, 7, 0],
      ])
        uniformRows.push(registerRow(form, 'c5', roles, count));
    }
    for (const encoding of ['w0', 'w1'])
      uniformRows.push(registerRow(form, encoding, [6, 7, 0], 1n));
  } else {
    const counts = [1n, 0n, edge - 1n, edge, 1n << 32n, 0xffffffffffffffffn];
    const partitions = [
      [7, 6, 5],
      [6, 6, 5],
      [5, 6, 5],
      [7, 6, 6],
      [6, 6, 6],
    ];
    for (const [encodingIndex, encoding] of ['c5', 'w0', 'w1'].entries()) {
      for (const [alias, roles] of partitions.entries()) {
        uniformRows.push(
          registerRow(form, encoding, roles, counts[(encodingIndex * 5 + alias) % counts.length]),
        );
      }
    }
    uniformRows.push(registerRow(form, 'c5', [7, 6, 5], 1n, 32, 0, 'high64-pair'));
  }
  uniformRows.push(registerRow(form, 'w1', [7, 6, 5], 1n, 16, 255, 'xmm-control'));

  for (const count of [1n, edge]) {
    const source =
      form.lane === 1
        ? Buffer.from(Array.from({ length: 32 }, (_, i) => i + 1))
        : Buffer.alloc(32, 255);
    let literal;
    if (form.lane === 1) {
      if (count === 16n) literal = Buffer.alloc(32);
      else
        literal = Buffer.from(
          form.direction === 'bytes-left'
            ? [
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0, 17, 18, 19, 20, 21, 22, 23,
                24, 25, 26, 27, 28, 29, 30, 31,
              ]
            : [
                2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 0, 18, 19, 20, 21, 22, 23, 24,
                25, 26, 27, 28, 29, 30, 31, 32, 0,
              ],
        );
    } else {
      const mask = (1n << BigInt(form.lane * 8)) - 1n;
      let value =
        form.direction === 'arithmetic'
          ? mask
          : count === edge
            ? 0n
            : form.direction === 'left'
              ? mask - 1n
              : mask / 2n;
      const lane = Buffer.alloc(form.lane);
      for (let i = 0; i < lane.length; i++) {
        lane[i] = Number(value & 255n);
        value >>= 8n;
      }
      literal = Buffer.concat(Array.from({ length: 32 / form.lane }, () => lane));
    }
    assert.deepEqual(
      oracle(form, source, count),
      literal,
      `literal kind${form.kind} count${count}`,
    );
    const countVector = Buffer.alloc(32, 0xa5);
    countVector.writeBigUInt64LE(count);
    uniformRows.push({
      name: `avx2-uniform-${form.kind}-literal-${count}`,
      code: encode(form, 'w0', [0, 1, 2], count),
      lower: { 1: source.subarray(0, 16), 2: countVector.subarray(0, 16) },
      upper: { 1: source.subarray(16), 2: countVector.subarray(16) },
      outLower: { 0: literal.subarray(0, 16) },
      outUpper: { 0: literal.subarray(16) },
    });
  }
  if (immediate(form)) continue;
  const first = pattern(form, formIndex);
  for (const [address, count] of [
    [0x5011, 1n],
    [0x5ff0, 0n],
    [0x5ff0, 0xffffffffffffffffn],
  ]) {
    const memory = Buffer.alloc(16, 255);
    memory.writeBigUInt64LE(count);
    const expected = oracle(form, first, count);
    uniformRows.push({
      name: `avx2-uniform-${form.kind}-m128-success-${address.toString(16)}-${count}`,
      code: encode(form, 'w1', [0, 0, 0], 0n, 32, true),
      address,
      read: 16,
      memory,
      lower: { 0: first.subarray(0, 16) },
      upper: { 0: first.subarray(16) },
      outLower: { 0: expected.subarray(0, 16) },
      outUpper: { 0: expected.subarray(16) },
    });
  }
  for (const [address, length, label] of [
    [0x5ff1, 15, 'sixteenth-byte'],
    [0x5ff8, 8, 'ignored-high64-still-read'],
  ]) {
    const memory = Buffer.alloc(length, 255);
    memory.writeBigUInt64LE(0n);
    uniformRows.push({
      name: `avx2-uniform-${form.kind}-${label}-fault`,
      code: encode(form, 'w0', [0, 0, 0], 0n, 32, true),
      address,
      read: 16,
      memory,
      lower: { 0: first.subarray(0, 16) },
      upper: { 0: first.subarray(16) },
      reason: 5,
      detail: 1,
      fault: 0x6000,
      access: 1,
      length: 16,
    });
  }
  for (const [segment, prefix, reason] of [
    ['ds', [], 11],
    ['ss', [0x36], 15],
  ])
    uniformRows.push({
      name: `avx2-uniform-${form.kind}-${segment}-span-before-page`,
      code: encode(form, 'w0', [0, 0, 0], 0n, 32, true, prefix),
      address: 0xfffffff8,
      read: 16,
      helpers: 0,
      reason,
    });
}
assert.equal(uniformRows.length, 358);
assert.equal(new Set(uniformRows.map((row) => row.name)).size, uniformRows.length);
assert.equal(uniformRows.filter((row) => row.reason).length, 32);
assert.equal(uniformRows.filter((row) => row.memory && !row.reason).length, 24);
export const uniformSourcePaths = [
  'engine/src/cpu/x86/avx/shift.rs',
  'engine/src/cpu/x86/avx/operation/factory/shift.rs',
  'engine/src/cpu/x86/decode/avx/shift.rs',
  'engine/src/cpu/x86/simd/shift.rs',
  'engine/src/cpu/x86/simd/packed.rs',
  'engine/tests/cpu/simd/avx/shift/uniform/native.rs',
  'engine/tests/cpu/simd/avx/shift/uniform/fixtures/p2-avx2-shift/runtime/cases.mjs',
];
