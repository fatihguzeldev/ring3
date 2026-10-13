import { groupRows } from '../../../../support/fixtures/runtime/cases.mjs';

const forms = [
  { name: 'psubusb', map: 1, opcode: 0xd8, kind: 8, lane: 1, unary: false },
  { name: 'psubusw', map: 1, opcode: 0xd9, kind: 9, lane: 2, unary: false },
  { name: 'paddusb', map: 1, opcode: 0xdc, kind: 11, lane: 1, unary: false },
  { name: 'paddusw', map: 1, opcode: 0xdd, kind: 12, lane: 2, unary: false },
  { name: 'psubsb', map: 1, opcode: 0xe8, kind: 16, lane: 1, unary: false },
  { name: 'psubsw', map: 1, opcode: 0xe9, kind: 17, lane: 2, unary: false },
  { name: 'paddsb', map: 1, opcode: 0xec, kind: 19, lane: 1, unary: false },
  { name: 'paddsw', map: 1, opcode: 0xed, kind: 20, lane: 2, unary: false },
];

function oracle(form, a, b) {
  const modulus = 1n << BigInt(form.lane * 8),
    half = modulus / 2n;
  const signed = (value) => (value >= half ? value - modulus : value);
  switch (form.kind) {
    case 8:
    case 9:
      return a < b ? 0n : a - b;
    case 11:
    case 12:
      return a + b >= modulus ? modulus - 1n : a + b;
    case 16:
    case 17:
    case 19:
    case 20: {
      const value = form.kind < 19 ? signed(a) - signed(b) : signed(a) + signed(b);
      const clamped = value < -half ? -half : value >= half ? half - 1n : value;
      return (clamped + modulus) % modulus;
    }
    default:
      throw new Error('unexpected saturating kind');
  }
}

function literals(form) {
  const sign = 1n << BigInt(form.lane * 8 - 1),
    mask = sign * 2n - 1n;
  switch (form.kind) {
    case 8:
    case 9:
      return {
        first: [0n, 1n, mask, mask, sign, sign - 1n, 2n, mask],
        second: [1n, mask, 0n, mask, sign - 1n, sign, 1n, mask - 1n],
        expected: [0n, 0n, mask, 0n, 1n, 0n, 1n, 1n],
      };
    case 11:
    case 12:
      return {
        first: [mask, mask, 0n, 1n, mask - 1n, sign, sign - 1n, 2n],
        second: [1n, mask, 0n, 1n, 1n, sign, sign, mask - 1n],
        expected: [mask, mask, 0n, 2n, mask, mask, mask, mask],
      };
    case 16:
    case 17:
      return {
        first: [sign - 1n, sign, sign - 1n, sign, 0n, mask, 1n, mask],
        second: [mask, 1n, 1n, mask, 0n, 1n, mask, mask],
        expected: [sign - 1n, sign, sign - 2n, sign + 1n, 0n, mask - 1n, 2n, 0n],
      };
    case 19:
    case 20:
      return {
        first: [sign - 1n, sign, sign - 1n, sign, 0n, mask, 1n, mask],
        second: [1n, mask, mask, 1n, 0n, 1n, 1n, mask],
        expected: [sign - 1n, sign, sign - 2n, sign + 1n, 0n, 0n, 2n, mask - 1n],
      };
    default:
      throw new Error('unexpected saturating literal');
  }
}

export const saturatingRows = groupRows('saturating', forms, oracle, literals);
export const saturatingSourcePaths = [
  'engine/tests/cpu/simd/avx/integer/saturating/native.rs',
  'engine/tests/cpu/simd/avx/integer/saturating/fixtures/p2-avx2-saturating/runtime/cases.mjs',
];
