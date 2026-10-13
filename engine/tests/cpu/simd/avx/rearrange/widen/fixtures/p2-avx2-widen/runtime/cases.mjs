import assert from 'node:assert/strict';

const forms = [
  { opcode: 0x20, from: 1, to: 2, signed: true },
  { opcode: 0x21, from: 1, to: 4, signed: true },
  { opcode: 0x22, from: 1, to: 8, signed: true },
  { opcode: 0x23, from: 2, to: 4, signed: true },
  { opcode: 0x24, from: 2, to: 8, signed: true },
  { opcode: 0x25, from: 4, to: 8, signed: true },
  { opcode: 0x30, from: 1, to: 2, signed: false },
  { opcode: 0x31, from: 1, to: 4, signed: false },
  { opcode: 0x32, from: 1, to: 8, signed: false },
  { opcode: 0x33, from: 2, to: 4, signed: false },
  { opcode: 0x34, from: 2, to: 8, signed: false },
  { opcode: 0x35, from: 4, to: 8, signed: false },
];

function source(form, register) {
  const modulus = 1n << BigInt(form.from * 8);
  const sign = modulus / 2n;
  const values = [
    sign, sign - 1n, modulus - 1n, 1n, 0n, modulus - 2n, sign + 1n, sign - 2n,
    2n, modulus - 3n, sign + 2n, sign - 3n, 0x12n, modulus - 0x13n, 0x23n, modulus - 0x24n,
  ];
  return Buffer.from(Array.from({ length: 16 }, (_, byte) => {
    const value = values[(Math.floor(byte / form.from) + register * 3) % values.length];
    return Number(value >> BigInt(byte % form.from * 8) & 255n);
  }));
}

function extend(form, input, size = 32) {
  const output = Buffer.alloc(32);
  for (let lane = 0; lane < size / form.to; lane++) {
    let value = 0n;
    for (let byte = form.from - 1; byte >= 0; byte--) {
      value = value * 256n + BigInt(input[lane * form.from + byte]);
    }
    if (form.signed) value = BigInt.asIntN(form.from * 8, value);
    value = BigInt.asUintN(form.to * 8, value);
    for (let byte = 0; byte < form.to; byte++) {
      output[lane * form.to + byte] = Number(value >> BigInt(byte * 8) & 255n);
    }
  }
  return output;
}

assert.deepEqual(
  Array.from({ length: 4 }, (_, lane) => extend(forms[2], Buffer.from([0x80, 0x7f, 0xff, 1])).readBigInt64LE(lane * 8)),
  [-128n, 127n, -1n, 1n],
);
assert.deepEqual(
  Array.from({ length: 4 }, (_, lane) => extend(forms[8], Buffer.from([0x80, 0x7f, 0xff, 1])).readBigUInt64LE(lane * 8)),
  [128n, 127n, 255n, 1n],
);

function encode(form, destination, operand, options = {}) {
  const { wide = true, w = false, prefix = [], address16 = null, memory = false, stack = false } = options;
  let modrm = [0xc0 + destination * 8 + operand];
  if (memory) {
    modrm = stack
      ? [destination * 8 + 4, 0x24]
      : address16 === null
        ? [destination * 8 + operand]
        : [destination * 8 + 6, address16 & 255, address16 >>> 8];
  }
  return Buffer.from([
    ...prefix,
    ...(address16 === null ? [] : [0x67]),
    0xc4, 0xe2, 0x79 + (wide ? 4 : 0) + (w ? 0x80 : 0), form.opcode,
    ...modrm,
  ]).toString('hex');
}

export const widenRows = [];
for (const form of forms) {
  for (let register = 0; register < 8; register++) {
    const input = source(form, register);
    const expected = extend(form, input);
    assert.notDeepEqual(expected.subarray(0, 16), expected.subarray(16), 'contiguous source windows discriminate repeated lower output');
    for (const destination of [register, (register + 1) % 8]) {
      for (const w of [false, true]) {
        widenRows.push({
          name: `widen-${form.opcode.toString(16)}-source${register}-destination${destination}-w${Number(w)}`,
          code: encode(form, destination, register, { w }),
          lower: { [register]: input },
          upper: { [register]: Buffer.alloc(16, w ? 0xca : 0x35) },
          outLower: { [destination]: expected.subarray(0, 16) },
          outUpper: { [destination]: expected.subarray(16) },
        });
      }
    }
  }
  const input = source(form, 3);
  const expected = extend(form, input, 16);
  widenRows.push({
    name: `legacy-widen-${form.opcode.toString(16)}-alias-upperzero`,
    code: encode(form, 3, 3, { wide: false, w: true }),
    lower: { 3: input },
    upper: { 3: Buffer.alloc(16, 0xd7) },
    outLower: { 3: expected.subarray(0, 16) },
    outUpper: { 3: expected.subarray(16) },
  });
}

function memoryRow(form, address, options = {}) {
  const width = 32 / form.to * form.from;
  const input = source(form, 0).subarray(0, width);
  const output = extend(form, input);
  return {
    name: `widen-${form.opcode.toString(16)}-memory-${address.toString(16)}`,
    code: encode(form, 3, 0, { memory: true }),
    address,
    read: width,
    memory: input,
    lower: { 3: Buffer.alloc(16, 0xa9) },
    upper: { 3: Buffer.alloc(16, 0x5e) },
    outLower: { 3: output.subarray(0, 16) },
    outUpper: { 3: output.subarray(16) },
    ...options,
  };
}

for (const form of forms) {
  const width = 32 / form.to * form.from;
  for (const address of [0x5011, 0x6000 - width]) widenRows.push(memoryRow(form, address));
  widenRows.push(memoryRow(form, 0x5013, {
    name: `widen-${form.opcode.toString(16)}-address16`,
    code: encode(form, 3, 0, { memory: true, address16: 0x5013 }),
  }));
  widenRows.push(memoryRow(form, 0x5015, {
    name: `widen-${form.opcode.toString(16)}-fs`,
    code: encode(form, 3, 0, { memory: true, prefix: [0x64] }),
  }));
  widenRows.push(memoryRow(form, 0x5017, {
    name: `widen-${form.opcode.toString(16)}-default-ss`,
    code: encode(form, 3, 0, { memory: true, stack: true }),
    r: { 4: 0x5017 },
  }));
  for (const address of [0x6001 - width, 0x6000]) {
    const row = memoryRow(form, address, {
      reason: 5,
      detail: 1,
      fault: 0x6000,
      access: 1,
      length: width,
    });
    delete row.memory;
    widenRows.push(row);
  }
  for (const [segment, prefix, reason] of [['ds', [], 11], ['ss', [0x36], 15]]) {
    const row = memoryRow(form, 0x100000001 - width, {
      name: `widen-${form.opcode.toString(16)}-${segment}-span-overflow`,
      code: encode(form, 3, 0, { memory: true, prefix }),
      helpers: 0,
      reason,
    });
    delete row.memory;
    widenRows.push(row);
  }
}

assert.equal(widenRows.length, 504);
assert.equal(new Set(widenRows.map(row => row.name)).size, widenRows.length);
assert.equal(widenRows.filter(row => row.reason).length, 48);
assert.equal(widenRows.filter(row => row.memory).length, 60);
export const widenSourcePaths = [
  'engine/src/cpu/x86/avx/rearrange.rs',
  'engine/src/cpu/x86/avx/operation/factory/rearrange.rs',
  'engine/src/cpu/x86/decode/avx/rearrange.rs',
  'engine/src/cpu/x86/simd/rearrange.rs',
  'engine/src/cpu/x86/simd/packed.rs',
  'engine/tests/cpu/simd/avx/execution/wasm.rs',
  'engine/tests/cpu/simd/avx/rearrange/widen/fixtures/p2-avx2-widen/runtime/cases.mjs',
];
