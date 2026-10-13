import { groupRows } from '../../../../support/fixtures/runtime/cases.mjs';

const forms = [
  { name: 'pavgb', map: 1, opcode: 0xe0, kind: 14, lane: 1, unary: false },
  { name: 'pavgw', map: 1, opcode: 0xe3, kind: 15, lane: 2, unary: false },
];

function oracle(_form, a, b) {
  // upward rounding follows a full-width sum, including its carry bit.
  return (a + b + 1n) >> 1n;
}

function literals(form) {
  const sign = 1n << BigInt(form.lane * 8 - 1),
    mask = sign * 2n - 1n;
  return {
    first: [mask, 0n, 1n, mask, sign, sign - 1n, mask - 1n, 2n],
    second: [mask, 1n, 0n, 0n, sign - 1n, sign, 1n, 3n],
    expected: [mask, 1n, 1n, sign, sign, sign, sign, 3n],
  };
}

export const averageRows = groupRows('average', forms, oracle, literals);
export const averageSourcePaths = [
  'engine/tests/cpu/simd/avx/integer/average/native.rs',
  'engine/tests/cpu/simd/avx/integer/average/fixtures/p2-avx2-average/runtime/cases.mjs',
];
