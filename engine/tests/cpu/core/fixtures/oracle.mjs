const MASK = 0x8d5;

export function arithmetic(kind, lhs, rhs, carry, width, base) {
  const bits = BigInt(width), modulus = 1n << bits, sign = modulus >> 1n;
  const a = BigInt(lhs) & (modulus - 1n), b = BigInt(rhs) & (modulus - 1n);
  const signed = value => value >= sign ? value - modulus : value;
  const subtract = ['sub', 'sbb', 'dec', 'neg'].includes(kind);
  const logical = kind === 'xor';
  const left = kind === 'neg' ? 0n : a;
  const right = kind === 'neg' ? a : b;
  const inputCarry = ['adc', 'sbb'].includes(kind) ? BigInt(carry & 1) : 0n;
  const exact = logical ? a ^ b : subtract ? left - right - inputCarry : left + right + inputCarry;
  const result = (exact % modulus + modulus) % modulus;
  const signedExact = subtract ? signed(left) - signed(right) - inputCarry : signed(left) + signed(right) + inputCarry;
  let flags = base & ~MASK;
  if (['inc', 'dec'].includes(kind)) flags |= base & 1;
  else if (!logical && (exact < 0n || exact >= modulus)) flags |= 1;
  let ones = 0;
  for (let byte = Number(result & 255n); byte; byte >>>= 1) ones += byte & 1;
  if ((ones & 1) === 0) flags |= 4;
  if (!logical && (subtract
    ? (left & 15n) < (right & 15n) + inputCarry
    : (left & 15n) + (right & 15n) + inputCarry > 15n)) flags |= 0x10;
  if (result === 0n) flags |= 0x40;
  if (result >= sign) flags |= 0x80;
  if (!logical && (signedExact < -sign || signedExact >= sign)) flags |= 0x800;
  return {value: Number(result), flags: (flags | 2) >>> 0};
}

export function loopOutcome(regs, flags, iterations, budget) {
  const result = [...regs], writes = [];
  let retired = 0, pc = 0x1000, counter = 0;
  while (retired < budget && result[1] !== 0) {
    let next = arithmetic('add', result[0], 3, 0, 32, flags);
    result[0] = next.value; flags = next.flags; pc = 0x1003;
    if (++retired === budget) break;
    next = arithmetic('xor', result[2], result[0], 0, 32, flags);
    result[2] = next.value; flags = next.flags; pc = 0x1005;
    if (++retired === budget) break;
    next = arithmetic('add', counter, 1, 0, 32, flags);
    counter = next.value; flags = next.flags; writes.push([0x300c, 4, counter]); pc = 0x1009;
    if (++retired === budget) break;
    next = arithmetic('dec', result[1], 1, 0, 32, flags);
    result[1] = next.value; flags = next.flags; pc = 0x100a;
    if (++retired === budget) break;
    pc = result[1] === 0 ? 0x100c : 0x1000;
    retired++;
  }
  if (iterations < 1 || regs[1] !== iterations) throw new Error('positive authored loop count required');
  return {regs: result, flags, pc, retired, reason: retired === budget ? 2 : 4, writes};
}
