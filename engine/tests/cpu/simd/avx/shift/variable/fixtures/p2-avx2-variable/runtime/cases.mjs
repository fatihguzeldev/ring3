import assert from 'node:assert/strict';

const forms = [
  { opcode: 0x45, kind: 19, lane: 4, direction: 'right' },
  { opcode: 0x45, kind: 20, lane: 8, direction: 'right' },
  { opcode: 0x46, kind: 21, lane: 4, direction: 'arithmetic' },
  { opcode: 0x47, kind: 22, lane: 4, direction: 'left' },
  { opcode: 0x47, kind: 23, lane: 8, direction: 'left' },
];
function elements(lane, values) {
  const output = Buffer.alloc(32);
  for (let at = 0; at < 32; at += lane) {
    let value = BigInt.asUintN(lane * 8, values[(at / lane) % values.length]);
    for (let byte = 0; byte < lane; byte++) {
      output[at + byte] = Number(value % 256n);
      value /= 256n;
    }
  }
  return output;
}
function pattern(form, register) {
  const sign = 1n << BigInt(form.lane * 8 - 1),
    mask = sign * 2n - 1n;
  const values = [mask, sign, sign - 1n, 1n, 0n, mask - 1n, sign + 1n, mask / 3n, 0x12345678n];
  return elements(
    form.lane,
    values.map((_, lane) => values[(lane + register * 3) % values.length]),
  );
}
function counts(form, phase) {
  const bits = BigInt(form.lane * 8);
  const values = [
    0n,
    1n,
    bits - 1n,
    bits,
    bits + 1n,
    1n << 31n,
    0xffffffffffffffffn,
    1n << 32n,
    1n << 63n,
  ];
  return elements(
    form.lane,
    values.map((_, lane) => values[(lane + phase) % values.length]),
  );
}
function oracle(form, source, counts, width) {
  const output = Buffer.alloc(32),
    bits = BigInt(form.lane * 8),
    modulus = 1n << bits;
  for (let at = 0; at < width; at += form.lane) {
    let value = 0n,
      count = 0n;
    for (let byte = form.lane - 1; byte >= 0; byte--) {
      value = value * 256n + BigInt(source[at + byte]);
      count = count * 256n + BigInt(counts[at + byte]);
    }
    if (form.direction === 'arithmetic') {
      const signed = value >= modulus / 2n ? value - modulus : value;
      value = BigInt.asUintN(Number(bits), signed >> (count >= bits ? bits - 1n : count));
    } else if (count >= bits) value = 0n;
    else
      value = form.direction === 'left' ? (value * (1n << count)) % modulus : value / (1n << count);
    for (let byte = 0; byte < form.lane; byte++) {
      output[at + byte] = Number(value % 256n);
      value /= 256n;
    }
  }
  return output;
}
function encode(
  form,
  width,
  [destination, source, count],
  { memory = false, prefix = [], address16 = null, stack = false } = {},
) {
  let modrm = [0xc0 | (destination << 3) | count];
  if (memory)
    modrm = stack
      ? [(destination << 3) | 4, 0x24]
      : address16 === null
        ? [destination << 3]
        : [(destination << 3) | 6, address16 & 255, address16 >>> 8];
  return Buffer.from([
    ...prefix,
    ...(address16 === null ? [] : [0x67]),
    0xc4,
    0xe2,
    ((15 - source) << 3) | (width === 32 ? 4 : 0) | (form.lane === 8 ? 0x80 : 0) | 1,
    form.opcode,
    ...modrm,
  ]).toString('hex');
}
function row(form, width, roles, vectors, label) {
  const [destination, source, count] = roles;
  const output = oracle(form, vectors[source], vectors[count], width);
  return {
    name: `variable-${form.kind}-${width}-${roles.join('/')}-${label}`,
    code: encode(form, width, roles),
    lower: Object.fromEntries(vectors.map((value, register) => [register, value.subarray(0, 16)])),
    upper: Object.fromEntries(vectors.map((value, register) => [register, value.subarray(16)])),
    outLower: { [destination]: output.subarray(0, 16) },
    outUpper: { [destination]: output.subarray(16) },
  };
}

export const variableRows = [];
for (const form of forms)
  for (const width of [16, 32]) {
    for (const roles of [
      [7, 6, 5],
      [6, 6, 5],
      [5, 6, 5],
      [7, 6, 6],
      [6, 6, 6],
    ]) {
      for (const phase of [0, 3, 6]) {
        const vectors = Array.from({ length: 8 }, (_, register) => pattern(form, register));
        vectors[roles[2]] = counts(form, phase);
        variableRows.push(row(form, width, roles, vectors, `mixed${phase}`));
      }
    }
    const dwordData = elements(4, [
      0xffffffffn,
      0x80000000n,
      0x7fffffffn,
      1n,
      0x7fffffffn,
      0x80000000n,
      0xffffffffn,
      0x12345678n,
    ]);
    const dwordCounts = elements(4, [0n, 1n, 31n, 32n, 0x80000000n, 0xffffffffn, 33n, 4n]);
    const qwordData = elements(8, [
      0xffffffffffffffffn,
      0x8000000000000000n,
      0x7fffffffffffffffn,
      0x123456789abcdef0n,
    ]);
    const qwordCounts = elements(8, [1n, 63n, 1n << 32n, 0xffffffffffffffffn]);
    const literals = {
      19: elements(4, [0xffffffffn, 0x40000000n, 0n, 0n, 0n, 0n, 0n, 0x01234567n]),
      20: elements(8, [0x7fffffffffffffffn, 1n, 0n, 0n]),
      21: elements(4, [
        0xffffffffn,
        0xc0000000n,
        0n,
        0n,
        0n,
        0xffffffffn,
        0xffffffffn,
        0x01234567n,
      ]),
      22: elements(4, [0xffffffffn, 0n, 0x80000000n, 0n, 0n, 0n, 0n, 0x23456780n]),
      23: elements(8, [0xfffffffffffffffen, 0n, 0n, 0n]),
    };
    const vectors = Array.from({ length: 8 }, () => Buffer.alloc(32, 0xa5));
    vectors[1] = form.lane === 4 ? dwordData : qwordData;
    vectors[2] = form.lane === 4 ? dwordCounts : qwordCounts;
    const literal = literals[form.kind];
    if (width === 16) literal.fill(0, 16);
    assert.deepEqual(
      oracle(form, vectors[1], vectors[2], width),
      literal,
      `literal kind${form.kind} width${width}`,
    );
    variableRows.push(row(form, width, [0, 1, 2], vectors, 'literal'));

    const data = pattern(form, 3),
      count = counts(form, 0),
      output = oracle(form, data, count, width);
    const memoryRow = (address, options = {}) => ({
      name: `variable-${form.kind}-${width}-memory-${address.toString(16)}`,
      code: encode(form, width, [0, 0, 0], { memory: true }),
      address,
      read: width,
      memory: count.subarray(0, width),
      lower: { 0: data.subarray(0, 16) },
      upper: { 0: data.subarray(16) },
      outLower: { 0: output.subarray(0, 16) },
      outUpper: { 0: output.subarray(16) },
      ...options,
    });
    for (const address of [0x5011, 0x6000 - width]) variableRows.push(memoryRow(address));
    for (const [label, options] of [
      ['address16', { address16: 0x5013 }],
      ['fs', { prefix: [0x64] }],
    ])
      variableRows.push(
        memoryRow(0x5013, {
          name: `variable-${form.kind}-${width}-${label}`,
          code: encode(form, width, [0, 0, 0], { memory: true, ...options }),
        }),
      );
    for (const [address, label] of [
      [0x6001 - width, 'last-byte'],
      [0x5ff8, 'late-payload'],
    ]) {
      variableRows.push(
        memoryRow(address, {
          name: `variable-${form.kind}-${width}-${label}-fault`,
          memory: count.subarray(0, 0x6000 - address),
          reason: 5,
          detail: 1,
          fault: 0x6000,
          access: 1,
          length: width,
        }),
      );
    }
    for (const [segment, prefix, reason] of [
      ['ds', [], 11],
      ['ss', [0x36], 15],
    ]) {
      const fault = memoryRow(0x100000001 - width, {
        name: `variable-${form.kind}-${width}-${segment}-span-before-page`,
        code: encode(form, width, [0, 0, 0], { memory: true, prefix }),
        helpers: 0,
        reason,
      });
      delete fault.memory;
      variableRows.push(fault);
    }
  }
assert.equal(variableRows.length, 240);
assert.equal(new Set(variableRows.map((row) => row.name)).size, variableRows.length);
assert.equal(variableRows.filter((row) => row.reason).length, 40);
assert.equal(variableRows.filter((row) => row.memory && !row.reason).length, 40);
// the uniform baseline already pins all shared shift/math/decode owners exactly once.
export const variableSourcePaths = [
  'engine/tests/cpu/simd/avx/shift/variable/native.rs',
  'engine/tests/cpu/simd/avx/shift/variable/fixtures/p2-avx2-variable/runtime/cases.mjs',
];
