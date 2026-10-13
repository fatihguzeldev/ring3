import { groupRows } from '../../../../support/fixtures/runtime/cases.mjs';

const forms = [
  { name: 'pminub', map: 1, opcode: 0xda, kind: 10, lane: 1, unary: false },
  { name: 'pmaxub', map: 1, opcode: 0xde, kind: 13, lane: 1, unary: false },
  { name: 'pminsw', map: 1, opcode: 0xea, kind: 18, lane: 2, unary: false },
  { name: 'pmaxsw', map: 1, opcode: 0xee, kind: 21, lane: 2, unary: false },
  { name: 'pminsb', map: 2, opcode: 0x38, kind: 37, lane: 1, unary: false },
  { name: 'pminsd', map: 2, opcode: 0x39, kind: 38, lane: 4, unary: false },
  { name: 'pminuw', map: 2, opcode: 0x3a, kind: 39, lane: 2, unary: false },
  { name: 'pminud', map: 2, opcode: 0x3b, kind: 40, lane: 4, unary: false },
  { name: 'pmaxsb', map: 2, opcode: 0x3c, kind: 41, lane: 1, unary: false },
  { name: 'pmaxsd', map: 2, opcode: 0x3d, kind: 42, lane: 4, unary: false },
  { name: 'pmaxuw', map: 2, opcode: 0x3e, kind: 43, lane: 2, unary: false },
  { name: 'pmaxud', map: 2, opcode: 0x3f, kind: 44, lane: 4, unary: false },
];

function oracle(form, a, b) {
  const modulus = 1n << BigInt(form.lane * 8),
    sign = modulus / 2n;
  const signed = (value) => (value >= sign ? value - modulus : value);
  switch (form.kind) {
    case 10:
    case 39:
    case 40:
      return a < b ? a : b;
    case 13:
    case 43:
    case 44:
      return a > b ? a : b;
    case 18:
    case 37:
    case 38:
      return signed(a) < signed(b) ? a : b;
    case 21:
    case 41:
    case 42:
      return signed(a) > signed(b) ? a : b;
    default:
      throw new Error('unexpected minmax kind');
  }
}

function literals(form) {
  const sign = 1n << BigInt(form.lane * 8 - 1),
    mask = sign * 2n - 1n;
  const first = [mask, sign, sign - 1n, 0n, 0n, sign, mask, 1n];
  const second = [0n, sign - 1n, sign, mask, 0n, sign, mask, mask];
  switch (form.kind) {
    case 18:
    case 37:
    case 38:
    case 13:
    case 43:
    case 44:
      return {
        first,
        second,
        expected: [mask, sign, sign, mask, 0n, sign, mask, mask],
      };
    case 10:
    case 39:
    case 40:
    case 21:
    case 41:
    case 42:
      return {
        first,
        second,
        expected: [0n, sign - 1n, sign - 1n, 0n, 0n, sign, mask, 1n],
      };
    default:
      throw new Error('unexpected minmax literal');
  }
}

export const minmaxRows = groupRows('minmax', forms, oracle, literals);
export const minmaxSourcePaths = [
  'engine/tests/cpu/simd/avx/integer/minmax/native.rs',
  'engine/tests/cpu/simd/avx/integer/minmax/fixtures/p2-avx2-minmax/runtime/cases.mjs',
];
