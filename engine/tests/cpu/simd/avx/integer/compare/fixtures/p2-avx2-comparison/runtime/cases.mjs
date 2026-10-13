import assert from 'node:assert/strict';

const forms = [
  { name: 'gt-byte', map: 1, opcode: 0x64, lane: 1, greater: true },
  { name: 'gt-word', map: 1, opcode: 0x65, lane: 2, greater: true },
  { name: 'gt-dword', map: 1, opcode: 0x66, lane: 4, greater: true },
  { name: 'gt-qword', map: 2, opcode: 0x37, lane: 8, greater: true },
  { name: 'eq-byte', map: 1, opcode: 0x74, lane: 1, greater: false },
  { name: 'eq-word', map: 1, opcode: 0x75, lane: 2, greater: false },
  { name: 'eq-dword', map: 1, opcode: 0x76, lane: 4, greater: false },
  { name: 'eq-qword', map: 2, opcode: 0x29, lane: 8, greater: false },
];

function encode(form, encoding, destination, first, second, width = 32, prefix = []) {
  const control = 0x81 | ((15 - first) << 3) | (width === 32 ? 4 : 0);
  const vex =
    encoding === 'c5'
      ? [0xc5, control]
      : [0xc4, 0xe0 | form.map, (control & 0x7f) | (encoding === 'w1' ? 0x80 : 0)];
  assert.ok(form.map === 1 || encoding !== 'c5');
  const operand = second === null ? destination << 3 : 0xc0 | (destination << 3) | second;
  return Buffer.from([...prefix, ...vex, form.opcode, operand]).toString('hex');
}

function lanes(width, numbers, start = 0) {
  const bytes = Buffer.alloc(32);
  for (let at = 0; at < 32; at += width) {
    let value = numbers[(at / width + start) % numbers.length];
    for (let byte = 0; byte < width; byte++) {
      bytes[at + byte] = Number(value & 255n);
      value >>= 8n;
    }
  }
  return bytes;
}

function pattern(width, seed) {
  const modulus = 1n << BigInt(width * 8),
    sign = modulus / 2n;
  return lanes(
    width,
    [0n, modulus - 1n, sign, sign - 1n, 1n, modulus - 2n, sign + 1n, modulus / 3n],
    seed,
  );
}

function comparison(form, first, second, width = 32) {
  const result = Buffer.alloc(32),
    modulus = 1n << BigInt(form.lane * 8),
    sign = modulus / 2n;
  function read(bytes, offset) {
    let value = 0n;
    for (let byte = form.lane - 1; byte >= 0; byte--)
      value = value * 256n + BigInt(bytes[offset + byte]);
    return value;
  }
  const signed = (value) => (value >= sign ? value - modulus : value);
  for (let offset = 0; offset < width; offset += form.lane) {
    const a = read(first, offset),
      b = read(second, offset);
    const yes = form.greater ? signed(a) > signed(b) : a === b;
    result.fill(yes ? 255 : 0, offset, offset + form.lane);
  }
  return result;
}

function registers(form, encoding, destination, first, second, width = 32) {
  const values = Array.from({ length: 8 }, (_, register) => pattern(form.lane, register * 3));
  const result = comparison(form, values[first], values[second], width);
  return {
    name: `avx2-compare-${form.name}-${encoding}-${width}-${destination}/${first}/${second}`,
    code: encode(form, encoding, destination, first, second, width),
    lower: Object.fromEntries(values.map((value, register) => [register, value.subarray(0, 16)])),
    upper: Object.fromEntries(values.map((value, register) => [register, value.subarray(16)])),
    outLower: { [destination]: result.subarray(0, 16) },
    outUpper: { [destination]: result.subarray(16) },
  };
}

export const comparisonRows = [];
for (const [formIndex, form] of forms.entries()) {
  const encodings = form.map === 1 ? ['c5', 'w0', 'w1'] : ['w0', 'w1'];
  for (const [encodingIndex, encoding] of encodings.entries()) {
    const destination = (formIndex * 2 + encodingIndex) % 8;
    const first = (destination + 1) % 8,
      second = (destination + 2) % 8;
    for (const [d, a, b] of [
      [destination, first, second],
      [first, first, second],
      [second, first, second],
      [destination, first, first],
      [first, first, first],
    ]) {
      comparisonRows.push(registers(form, encoding, d, a, b));
    }
  }
  comparisonRows.push(registers(form, 'w1', 7, 6, 7, 16));

  // these literal sign-boundary masks are independent of the generated oracle.
  const modulus = 1n << BigInt(form.lane * 8),
    sign = modulus / 2n,
    mask = modulus - 1n;
  const firstNumbers = [mask, sign, sign - 1n, 0n, 0n, sign, mask, mask];
  const secondNumbers = [0n, sign - 1n, sign, mask, 0n, sign, mask, 1n];
  const literal = form.greater ? [0, 0, 255, 255, 0, 0, 0, 0] : [0, 0, 0, 0, 255, 255, 255, 0];
  for (const start of [0, 4]) {
    const first = lanes(form.lane, firstNumbers, start),
      second = lanes(form.lane, secondNumbers, start);
    const expected = Buffer.from(
      Array.from({ length: 32 }, (_, byte) => literal[(Math.floor(byte / form.lane) + start) % 8]),
    );
    assert.deepEqual(comparison(form, first, second), expected);
    comparisonRows.push({
      name: `avx2-compare-${form.name}-literal-signed-boundary-${start}`,
      code: encode(form, 'w0', 7, 6, 5),
      lower: { 6: first.subarray(0, 16), 5: second.subarray(0, 16) },
      upper: { 6: first.subarray(16), 5: second.subarray(16) },
      outLower: { 7: expected.subarray(0, 16) },
      outUpper: { 7: expected.subarray(16) },
    });
  }

  const destination = formIndex,
    first = pattern(form.lane, 0),
    second = pattern(form.lane, 7);
  const result = comparison(form, first, second);
  for (const address of [0x5011, 0x5fe0]) {
    comparisonRows.push({
      name: `avx2-compare-${form.name}-memory-exact32-${address.toString(16)}`,
      code: encode(form, 'w1', destination, destination, null),
      address,
      read: 32,
      memory: second,
      lower: { [destination]: first.subarray(0, 16) },
      upper: { [destination]: first.subarray(16) },
      outLower: { [destination]: result.subarray(0, 16) },
      outUpper: { [destination]: result.subarray(16) },
    });
  }
  comparisonRows.push({
    name: `avx2-compare-${form.name}-late32nd-byte-page-fault`,
    code: encode(form, 'w0', destination, destination, null),
    address: 0x5fe1,
    read: 32,
    memory: second.subarray(0, 31),
    lower: { [destination]: first.subarray(0, 16) },
    upper: { [destination]: first.subarray(16) },
    reason: 5,
    detail: 1,
    fault: 0x6000,
    access: 1,
    length: 32,
  });
  for (const [segment, prefix, reason] of [
    ['ds', [], 11],
    ['ss', [0x36], 15],
  ]) {
    comparisonRows.push({
      name: `avx2-compare-${form.name}-${segment}-cached-span-before-page-access`,
      code: encode(form, 'w0', destination, destination, null, 32, prefix),
      address: 0xfffffff0,
      read: 32,
      helpers: 0,
      reason,
    });
  }
}

assert.equal(new Set(comparisonRows.map((row) => row.name)).size, comparisonRows.length);
assert.equal(comparisonRows.length, 174);
assert.equal(comparisonRows.filter((row) => row.reason).length, 24);
assert.equal(comparisonRows.filter((row) => row.memory && !row.reason).length, 16);
export const comparisonSourcePaths = [
  'engine/src/cpu/x86/decode/avx/integer.rs',
  'engine/src/cpu/x86/avx/operation/factory/integer.rs',
  'engine/src/cpu/x86/avx/integer.rs',
  'engine/tests/cpu/simd/avx/integer/compare/fixtures/p2-avx2-comparison/runtime/cases.mjs',
];
