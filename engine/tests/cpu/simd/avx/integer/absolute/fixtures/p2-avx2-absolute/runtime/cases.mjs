import { groupRows } from '../../../../support/fixtures/runtime/cases.mjs';

const forms = [
  { name: 'pabsb', map: 2, opcode: 0x1c, kind: 32, lane: 1, unary: true },
  { name: 'pabsw', map: 2, opcode: 0x1d, kind: 33, lane: 2, unary: true },
  { name: 'pabsd', map: 2, opcode: 0x1e, kind: 34, lane: 4, unary: true },
];

function oracle(form, _first, source) {
  const modulus = 1n << BigInt(form.lane * 8);
  return source < modulus / 2n ? source : modulus - source;
}

function literals(form) {
  const sign = 1n << BigInt(form.lane * 8 - 1),
    mask = sign * 2n - 1n;
  // signed minimum keeps its high bit because the result is unsigned.
  return {
    first: [mask, mask, mask, mask, mask, mask, mask, mask],
    second: [sign, sign + 1n, mask, 0n, 1n, sign - 1n, mask - 1n, 2n],
    expected: [sign, sign - 1n, 1n, 0n, 1n, sign - 1n, 2n, 2n],
  };
}

export const absoluteRows = groupRows('absolute', forms, oracle, literals);
export const absoluteSourcePaths = [
  'engine/tests/cpu/simd/avx/integer/absolute/native.rs',
  'engine/tests/cpu/simd/avx/integer/absolute/fixtures/p2-avx2-absolute/runtime/cases.mjs',
];
