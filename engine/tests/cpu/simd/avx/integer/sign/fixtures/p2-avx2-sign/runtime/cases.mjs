import { groupRows } from '../../../../support/fixtures/runtime/cases.mjs';

const forms = [
  { name: 'psignb', map: 2, opcode: 0x08, kind: 29, lane: 1, unary: false },
  { name: 'psignw', map: 2, opcode: 0x09, kind: 30, lane: 2, unary: false },
  { name: 'psignd', map: 2, opcode: 0x0a, kind: 31, lane: 4, unary: false },
];

function oracle(form, data, control) {
  const modulus = 1n << BigInt(form.lane * 8);
  if (control === 0n) return 0n;
  return control < modulus / 2n ? data : (modulus - data) % modulus;
}

function literals(form) {
  const sign = 1n << BigInt(form.lane * 8 - 1),
    mask = sign * 2n - 1n;
  // a negative control selects modular negation, including signed minimum.
  return {
    first: [sign, sign, mask, mask, 1n, sign - 1n, sign + 1n, 2n],
    second: [mask, 0n, 1n, mask, sign, 1n, mask, 0n],
    expected: [sign, 0n, mask, 1n, mask, sign - 1n, sign - 1n, 0n],
  };
}

export const signRows = groupRows('sign', forms, oracle, literals);
export const signSourcePaths = [
  'engine/tests/cpu/simd/avx/integer/sign/native.rs',
  'engine/tests/cpu/simd/avx/integer/sign/fixtures/p2-avx2-sign/runtime/cases.mjs',
];
