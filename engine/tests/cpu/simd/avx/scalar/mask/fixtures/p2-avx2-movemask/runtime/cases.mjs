import assert from 'node:assert/strict';

function encode(destination, source, mode, wide = true) {
  const control = 0x79 + (wide ? 4 : 0);
  return Buffer.from([
    ...(mode === 'c5' ? [0xc5, control + 0x80] : [0xc4, 0xe1, control + (mode === 'w1' ? 0x80 : 0)]),
    0xd7, 0xc0 + destination * 8 + source,
  ]).toString('hex');
}

function row(destination, source, pattern, mode, wide = true) {
  const bytes = Buffer.from(Array.from({ length: 32 }, (_, index) =>
    (index * 7 + source * 11) % 128 + (pattern & 2 ** index ? 128 : 0)));
  let mask = 0n;
  for (let index = 0; index < (wide ? 32 : 16); index++) {
    if (bytes[index] >= 128) mask += 1n << BigInt(index);
  }
  const expected = Number(mask);
  assert.equal(expected, wide ? pattern >>> 0 : pattern & 0xffff);
  return {
    name: `movemask-d${destination}-b${source}-bits${pattern.toString(16)}-${mode}-${wide ? 256 : 128}`,
    code: encode(destination, source, mode, wide),
    lower: { [source]: bytes.subarray(0, 16) },
    upper: { [source]: bytes.subarray(16) },
    outR: { [destination]: expected },
  };
}

export const movemaskRows = [];
const patterns = [0, 0xffffffff, 1, 0x8000, 0x10000, 0x80000000, 0xaaaa5555, 0x12345678];
for (let destination = 0; destination < 8; destination++) {
  for (const source of [destination, (destination + 1) % 8]) {
    for (const pattern of patterns) {
      for (const mode of ['c5', 'w0', 'w1']) {
        movemaskRows.push(row(destination, source, pattern, mode));
      }
    }
  }
  for (const mode of ['w0', 'w1']) {
    movemaskRows.push(row(destination, destination, 0x80018001, mode, false));
  }
}
for (let bit = 0; bit < 32; bit++) movemaskRows.push(row(7, 6, 2 ** bit, 'c5'));

assert.equal(movemaskRows.length, 432);
assert.equal(new Set(movemaskRows.map(row => row.name)).size, movemaskRows.length);
assert.equal(movemaskRows.filter(row => row.reason || row.read || row.write).length, 0);
export const movemaskSourcePaths = [
  'engine/src/cpu/x86/avx/scalar.rs',
  'engine/src/cpu/x86/avx/operation/factory/scalar.rs',
  'engine/src/cpu/x86/decode/avx/scalar.rs',
  'engine/src/cpu/x86/simd/scalar.rs',
  'engine/tests/cpu/simd/avx/scalar/mask/native.rs',
  'engine/tests/cpu/simd/avx/execution/wasm.rs',
  'engine/tests/cpu/simd/avx/scalar/mask/fixtures/p2-avx2-movemask/runtime/cases.mjs',
];
