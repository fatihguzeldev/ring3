import {arithmetic} from './oracle.mjs';

const FLAGS = 0x8d5;
const parity = value => {
  let ones = 0;
  for (let byte = Number(value & 255n); byte; byte >>>= 1) ones += byte & 1;
  return (ones & 1) === 0 ? 4 : 0;
};

function shift(kind, width, input, count, base, source = 0) {
  const bits = BigInt(width), modulus = 1n << bits, mask = modulus - 1n, sign = modulus >> 1n;
  const original = BigInt(input) & mask, masked = count & 31;
  let value = original, carry = base & 1;
  if (!masked) return {value: Number(value), flags: base};
  if (kind.startsWith('r')) {
    const through = kind === 'rcl' || kind === 'rcr', left = kind === 'rol' || kind === 'rcl';
    const digits = Array.from({length: width}, (_, index) => Number(original >> BigInt(index) & 1n));
    if (through) digits.push(carry);
    const effective = masked % digits.length;
    if (through && effective === 0) return {value: Number(value), flags: base};
    for (let index = 0; index < effective; index++) {
      if (left) digits.unshift(digits.pop());
      else digits.push(digits.shift());
    }
    value = digits.slice(0, width).reduce((sum, digit, index) => sum + BigInt(digit) * (1n << BigInt(index)), 0n);
    carry = through ? digits[width] : left ? digits[0] : digits[width - 1];
    let flags = (base & ~0x801) | carry;
    if (masked === 1) {
      const top = digits[width - 1], next = left ? carry : digits[width - 2];
      flags |= (top ^ next) << 11;
    }
    return {value: Number(value), flags: flags >>> 0};
  }
  const double = kind === 'shld' || kind === 'shrd';
  let high = BigInt(source) & mask;
  for (let index = 0; index < masked; index++) {
    if (kind === 'shl' || kind === 'shld') {
      carry = Number(value >= sign);
      value = (value * 2n) & mask;
      if (double) { value |= high >= sign ? 1n : 0n; high = high * 2n & mask; }
    } else {
      carry = Number(value & 1n);
      const top = kind === 'sar' ? value & sign : double && (high & 1n) ? sign : 0n;
      value = value / 2n | top;
      if (double) high /= 2n;
    }
  }
  if (!double && masked >= width && kind !== 'sar') carry = 0;
  if (double && masked > width) { value = 0n; carry = 0; }
  let flags = (base & ~FLAGS) | carry | parity(value);
  if (value === 0n) flags |= 0x40;
  if (value >= sign) flags |= 0x80;
  if (masked === 1) {
    const oldSign = Number(original >= sign), newSign = Number(value >= sign);
    const overflow = double ? oldSign ^ newSign : kind === 'shl' ? newSign ^ carry : kind === 'shr' ? oldSign : 0;
    flags |= overflow << 11;
  }
  return {value: Number(value), flags: flags >>> 0};
}

const registers = (eax = 5, count = 3, ebx = 0x3000, esi = 0x3000, edi = 0x4000) => [eax, count, 7, ebx, 0x4800, 0, esi, edi];

export function runExtended(check) {
  for (const width of [8, 16, 32]) {
    for (const kind of ['shl', 'shr', 'sar', 'rol', 'ror', 'rcl', 'rcr']) {
      const lhs = width === 8 ? 0x12345681 : width === 16 ? 0x12348001 : 0x80000001;
      for (const count of [...new Set([0, 1, width - 1, width, width + 1, 31])]) {
        const flags = 0xcd7, regs = registers(lhs, count), expected = shift(kind, width, lhs, count, flags);
        const next = width === 16 ? 0x1003 : 0x1002;
        regs[0] = width === 32 ? expected.value : ((lhs & (width === 8 ? 0xffffff00 : 0xffff0000)) | expected.value) >>> 0;
        check(`shift${width}-${kind}`, `${kind}${width}-count${count}`, 8,
          {regs: registers(lhs, count), flags}, {reason: 4, pc: next, retired: 1, regs, flags: expected.flags});
      }
    }
  }
  for (const width of [16, 32]) {
    for (const kind of ['shld', 'shrd']) {
      for (const count of [0, 1, width, 31]) {
        const lhs = 0x80000001, rhs = 0x12345678, flags = 0xcd7;
        const regs = registers(lhs, count); regs[2] = rhs;
        const result = shift(kind, width, lhs, count, flags, rhs), expected = [...regs];
        expected[0] = width === 32 ? result.value : ((lhs & 0xffff0000) | result.value) >>> 0;
        check(`shift${width}-${kind}`, `${kind}${width}-count${count}`, 8, {regs, flags},
          {reason: 4, pc: width === 16 ? 0x1004 : 0x1003, retired: 1, regs: expected, flags: result.flags});
      }
    }
  }
  check('memory-shift', 'zero-count-still-checks-write-rights', 8,
    {regs: registers(5, 0), flags: 0x893, readOnly: true},
    {reason: 3, pc: 0x1000, retired: 0, reads: 1, fault: [0x3000, 4 | 2 << 8 | 2 << 16]});
  check('memory-shift', 'zero-count-code-write-commits', 8,
    {regs: registers(5, 0, 0x1000), flags: 0x893},
    {reason: 6, pc: 0x1002, retired: 1, helpers: 1, writes: [[0x1000, 4, 0x0b0f23d3]]});
  check('extend', 'signed-byte-and-word-preserve-flags', 8,
    {regs: [5, 0x80, 0x12348001, 0x3000, 0x4800, 0, 0x3000, 0x4000], flags: 0x893},
    {reason: 4, pc: 0x1006, retired: 2, regs: [0xffffff80, 0x80, 0xffff8001, 0x3000, 0x4800, 0, 0x3000, 0x4000]});
  check('byte-swap', 'byte-swap-full-register', 8, {regs: registers(0x11223344)},
    {reason: 4, pc: 0x1002, retired: 1, regs: registers(0x44332211)});
  check('flags-ah', 'lahf-sahf-preserve-overflow', 8, {regs: registers(0x12345678), flags: 0x893},
    {reason: 4, pc: 0x1002, retired: 2, regs: registers(0x12349378)});
  check('translate-byte', 'xlat-uses-al-without-clobbering-upper-eax', 8,
    {regs: registers(0x12345603), initial: [[0x3003, 1, 0xab]]},
    {reason: 4, pc: 0x1001, retired: 1, regs: registers(0x123456ab)});
  check('memory-exchange', 'memory-exchange-preserves-flags', 8,
    {regs: registers(5), flags: 0x893, initial: [[0x3000, 4, 10]]},
    {reason: 4, pc: 0x1002, retired: 1, regs: registers(10), writes: [[0x3000, 4, 5]]});
  const callRegs = registers(0x12340005);
  check('word-call', 'word-call-ret-full', 16, {regs: callRegs},
    {reason: 4, pc: 0x1008, retired: 4, regs: registers(0x1234000a), flags: 6, writes: [[0x47fe, 2, 0x1004]]});
  check('word-call', 'word-call-budget-prefix', 1, {regs: callRegs},
    {reason: 2, pc: 0x1010, retired: 1, regs: [...callRegs.slice(0, 4), 0x47fe, ...callRegs.slice(5)], writes: [[0x47fe, 2, 0x1004]]});
  check('word-call', 'word-call-fault-before-stack-or-target-commit', 8,
    {regs: callRegs, flags: 0x893, unmapStack: true},
    {reason: 3, pc: 0x1000, retired: 0, helpers: 1, fault: [0x47fe, 2 | 2 << 8 | 1 << 16]});
  check('word-pop-sp', 'pop-sp-applies-stack-increment-before-register-write', 8,
    {regs: registers(), initial: [[0x4800, 2, 0x1234]]},
    {reason: 4, pc: 0x1002, retired: 1, regs: [5, 3, 7, 0x3000, 0x1234, 0, 0x3000, 0x4000]});
  check('word-pop-flags', 'popf-preserves-privileged-bits', 8,
    {regs: registers(), flags: 0x202, initial: [[0x4800, 2, 0x41]]},
    {reason: 4, pc: 0x1002, retired: 1, regs: [5, 3, 7, 0x3000, 0x4802, 0, 0x3000, 0x4000], flags: 0x243});
  check('word-pop-flags', 'popf-fault-preserves-input-flags', 8,
    {regs: registers(), flags: 0x893, unmapStack: true},
    {reason: 3, pc: 0x1000, retired: 0, reads: 1, fault: [0x4800, 2 | 1 << 8 | 1 << 16]});
  const source = [[0x3000, 1, 0x11], [0x3001, 1, 0x22], [0x3002, 1, 0x33]];
  for (const budget of [1, 2, 3, 8]) {
    const done = budget >= 3, count = Math.min(3, budget), regs = registers();
    regs[1] -= count; regs[6] += count; regs[7] += count;
    check('rep-move', `rep-move-cut-${budget}`, budget, {regs: registers(), initial: source},
      {reason: done && budget > 3 ? 4 : 2, pc: done ? 0x1002 : 0x1000, retired: Number(done), work: count, regs,
        writes: source.slice(0, count).map(([address, width, value]) => [address + 0x1000, width, value])});
  }
  check('rep-move', 'rep-zero-count-never-accesses-invalid-addresses', 1,
    {regs: registers(5, 0, 0x3000, 0x6000, 0x7000), flags: 3, readOnly: true},
    {reason: 2, pc: 0x1002, retired: 1, work: 1});
  check('rep-move', 'rep-backward-direction-commits-each-element', 8,
    {regs: registers(5, 3, 0x3000, 0x3002, 0x4002), flags: 0x402, initial: source},
    {reason: 4, pc: 0x1002, retired: 1, work: 3, regs: registers(5, 0, 0x3000, 0x2fff, 0x3fff),
      writes: [[0x4002, 1, 0x33], [0x4001, 1, 0x22], [0x4000, 1, 0x11]]});
  check('rep-move', 'rep-source-fault-retains-one-completed-element', 8,
    {regs: registers(5, 3, 0x3000, 0x3fff, 0x5000), unmapStack: true, initial: [[0x3fff, 1, 0xaa]]},
    {reason: 3, pc: 0x1000, retired: 0, work: 1, regs: registers(5, 2, 0x3000, 0x4000, 0x5001),
      writes: [[0x5000, 1, 0xaa]], reads: 1, fault: [0x4000, 1 | 1 << 8 | 1 << 16]});
  check('rep-move', 'rep-destination-fault-retains-one-completed-element', 8,
    {regs: registers(5, 3, 0x3000, 0x3000, 0x3fff), unmapStack: true, initial: source},
    {reason: 3, pc: 0x1000, retired: 0, work: 1, regs: registers(5, 2, 0x3000, 0x3001, 0x4000),
      writes: [[0x3fff, 1, 0x11]], helpers: 1, fault: [0x4000, 1 | 2 << 8 | 1 << 16]});
  check('rep-move', 'rep-code-write-retains-unfinished-instruction', 8,
    {regs: registers(5, 3, 0x3000, 0x3000, 0x1000), initial: source},
    {reason: 6, pc: 0x1000, retired: 0, work: 1, regs: registers(5, 2, 0x3000, 0x3001, 0x1001),
      writes: [[0x1000, 1, 0x11]], helpers: 1});
  check('rep-prefix', 'rep-budget-charges-prior-instruction-only-once', 2,
    {regs: registers(), initial: source},
    {reason: 2, pc: 0x1001, retired: 1, work: 2, regs: registers(6, 2, 0x3000, 0x3001, 0x4001), flags: 6, writes: [[0x4000, 1, 0x11]]});
  check('rep-compare', 'repe-compare-stops-on-first-mismatch', 8,
    {regs: registers(), flags: 0x893, initial: [[0x3000, 2, 0x0201], [0x4000, 2, 0x0301]]},
    {reason: 4, pc: 0x1002, retired: 1, work: 2, regs: registers(5, 1, 0x3000, 0x3002, 0x4002), flags: 0x97});
  check('rep-compare', 'repe-budget-exposes-last-comparison-flags', 1,
    {regs: registers(), flags: 0x893, initial: [[0x3000, 2, 0x0201], [0x4000, 2, 0x0301]]},
    {reason: 2, pc: 0x1000, retired: 0, work: 1, regs: registers(5, 2, 0x3000, 0x3001, 0x4001), flags: 0x46});
  check('rep-scan', 'repne-scan-stops-on-equality', 8,
    {regs: registers(5, 4), initial: [[0x4000, 3, 0x050201]]},
    {reason: 4, pc: 0x1002, retired: 1, work: 3, regs: registers(5, 1, 0x3000, 0x3000, 0x4003), flags: 0x46});
  check('rep-move16-address', 'rep-address-size-preserves-upper-register-halves', 8,
    {regs: registers(5, 0x12340002, 0x3000, 0xaaaa3000, 0xbbbb4000), initial: source},
    {reason: 4, pc: 0x1003, retired: 1, work: 2, regs: registers(5, 0x12340000, 0x3000, 0xaaaa3002, 0xbbbb4002),
      writes: [[0x4000, 1, 0x11], [0x4001, 1, 0x22]]});
  check('move-word-string', 'nonrepeat-word-move-retirement', 8,
    {regs: registers(), initial: [[0x3000, 2, 0xabcd]]},
    {reason: 4, pc: 0x1002, retired: 1, regs: registers(5, 3, 0x3000, 0x3002, 0x4002), writes: [[0x4000, 2, 0xabcd]]});
  runMultiply(check);
  runAtomic(check);
}

function signed(value, width) {
  const modulus = 1n << BigInt(width), sign = modulus >> 1n;
  const raw = BigInt.asUintN(width, BigInt(value));
  return raw >= sign ? raw - modulus : raw;
}

function low(register, value, width) {
  const mask = (1n << BigInt(width)) - 1n;
  return Number((BigInt(register >>> 0) & ~mask) | (BigInt(value) & mask)) >>> 0;
}

function multiplyResult(kind, width, input, rhs, flags, two = false) {
  const regs = [...input], mask = (1n << BigInt(width)) - 1n, sign = 1n << BigInt(width - 1);
  const lhs = BigInt(input[0]) & mask, right = BigInt(rhs) & mask;
  const exact = kind === 'mul' ? lhs * right : signed(lhs, width) * signed(right, width);
  const overflow = kind === 'mul' ? exact > mask : exact < -sign || exact >= sign;
  if (two) regs[0] = low(regs[0], exact, width);
  else if (width === 8) regs[0] = low(regs[0], exact, 16);
  else {
    const raw = BigInt.asUintN(width * 2, exact);
    regs[0] = low(regs[0], raw, width);
    regs[2] = low(regs[2], raw >> BigInt(width), width);
  }
  return {regs, flags: ((flags & ~FLAGS) | (overflow ? 0x801 : 0)) >>> 0};
}

function divisionInput(width, numerator, divisor) {
  const regs = registers(0xa5a55a5a, 0x5a5aa5a5), raw = BigInt.asUintN(width * 2, numerator);
  regs[2] = 0x6b6bb6b6;
  regs[0] = low(regs[0], raw, width === 8 ? 16 : width);
  if (width !== 8) regs[2] = low(regs[2], raw >> BigInt(width), width);
  regs[1] = low(regs[1], divisor, width);
  return regs;
}

function divisionResult(width, numerator, divisor, input, signedOperation) {
  const bits = BigInt(width), modulus = 1n << bits, sign = modulus >> 1n;
  const a = signedOperation ? signed(numerator, width * 2) : BigInt.asUintN(width * 2, numerator);
  const b = signedOperation ? signed(divisor, width) : BigInt.asUintN(width, divisor);
  if (b === 0n) return null;
  const quotient = a / b, remainder = a % b;
  if (signedOperation ? quotient < -sign || quotient >= sign : quotient < 0n || quotient >= modulus) return null;
  const regs = [...input];
  if (width === 8) regs[0] = low(regs[0], BigInt.asUintN(8, quotient) | BigInt.asUintN(8, remainder) << 8n, 16);
  else { regs[0] = low(regs[0], quotient, width); regs[2] = low(regs[2], remainder, width); }
  return regs;
}

function runMultiply(check) {
  const flags = 0xcd7;
  for (const width of [8, 16, 32]) {
    const modulus = 1n << BigInt(width), sign = modulus >> 1n, mask = modulus - 1n;
    for (const kind of ['mul', 'imul']) {
      for (const [index, [lhs, rhs]] of [[0n, mask], [mask, mask], [sign - 1n, 2n], [sign, 1n], [sign, mask]].entries()) {
        const input = registers(low(0xa5a55a5a, lhs, width), low(0x5a5aa5a5, rhs, width));
        input[2] = 0x6b6bb6b6;
        const wanted = multiplyResult(kind, width, input, rhs, flags);
        check(`multiply${width}-${kind}`, `${kind}${width}-edge-${index}`, 8, {regs: input, flags},
          {reason: 4, pc: width === 16 ? 0x1003 : 0x1002, retired: 1, ...wanted});
      }
    }
    const divisions = [
      ['div', mask * 3n + 2n, 3n], ['div', modulus, 1n], ['div', 1n, 0n],
      ['idiv', -sign, 1n], ['idiv', -17n, 3n], ['idiv', 17n, -3n], ['idiv', -17n, -3n],
      ['idiv', sign, 1n], ['idiv', -sign - 1n, 1n], ['idiv', -sign, -1n],
      ['idiv', -(1n << BigInt(width * 2 - 1)), -1n], ['idiv', 1n, 0n],
    ];
    for (const [index, [kind, numerator, divisor]] of divisions.entries()) {
      const input = divisionInput(width, numerator, divisor);
      const wanted = divisionResult(width, numerator, divisor, input, kind === 'idiv');
      check(`multiply${width}-${kind}`, `${kind}${width}-edge-${index}`, 8, {regs: input, flags},
        wanted ? {reason: 4, pc: width === 16 ? 0x1003 : 0x1002, retired: 1, regs: wanted}
          : {reason: 9, pc: 0x1000, retired: 0});
    }
    if (width !== 8) {
      for (const [index, [lhs, rhs]] of [[sign - 1n, 2n], [sign, mask], [mask, 3n]].entries()) {
        const input = registers(low(0xa5a55a5a, lhs, width), low(0x5a5aa5a5, rhs, width));
        const two = multiplyResult('imul', width, input, rhs, flags, true);
        check(`multiply${width}-two`, `imul${width}-two-${index}`, 8, {regs: input, flags},
          {reason: 4, pc: width === 16 ? 0x1004 : 0x1003, retired: 1, ...two});
        const immediateInput = [...input]; immediateInput[0] = input[1];
        const immediate = multiplyResult('imul', width, immediateInput, -3n, flags, true);
        immediate.regs[0] = low(input[0], immediate.regs[0], width);
        check(`multiply${width}-immediate`, `imul${width}-immediate-${index}`, 8, {regs: input, flags},
          {reason: 4, pc: width === 16 ? 0x1004 : 0x1003, retired: 1, ...immediate});
      }
    }
  }
  for (const name of ['multiply32-memory', 'divide32-memory']) {
    check(name, `${name}-operand-fault-before-evaluation`, 8,
      {regs: registers(0x80000000, 3, 0x6000), flags},
      {reason: 3, pc: 0x1000, retired: 0, reads: 1, fault: [0x6000, 4 | 1 << 8 | 1 << 16]});
  }
  check('divide32-memory', 'memory-divide-zero-is-cpu-fault', 8,
    {regs: registers(0x12345678), flags}, {reason: 9, pc: 0x1000, retired: 0});
}

function runAtomic(check) {
  for (const width of [8, 16, 32]) {
    const mask = 2 ** width - 1, sign = 2 ** (width - 1), bytes = width / 8;
    for (const kind of ['add', 'adc', 'sub', 'sbb', 'and', 'or', 'xor', 'inc', 'dec', 'neg', 'not']) {
      for (const [index, [lhs, rhs, carry]] of [[mask, 1, 0], [sign - 1, 1, 1], [0, mask, 1], [sign, mask, 0]].entries()) {
        const flags = 0x402 | carry, input = registers(low(0xa5a55a5a, rhs, width));
        let result;
        if (kind === 'not') result = {value: Number(BigInt(lhs) ^ BigInt(mask)), flags};
        else if (kind === 'and' || kind === 'or') {
          const value = kind === 'and' ? BigInt(lhs) & BigInt(rhs) : BigInt(lhs) | BigInt(rhs);
          result = {value: Number(value), flags: (flags & ~FLAGS) | parity(value) | (value === 0n ? 0x40 : 0) | (value >= BigInt(sign) ? 0x80 : 0)};
        } else result = arithmetic(kind, lhs, ['inc', 'dec'].includes(kind) ? 1 : rhs, carry, width, flags);
        const name = `atomic${width}-${kind}`;
        check(name, `${name}-edge-${index}`, 8, {regs: input, flags, initial: [[0x3000, bytes, lhs]]},
          {reason: 4, pc: width === 16 ? 0x1004 : 0x1003, retired: 1, flags: result.flags,
            writes: [[0x3000, bytes, result.value]]});
      }
    }
    check(`atomic${width}-adc`, `locked${width}-write-denial-preserves-registers-and-flags`, 8,
      {regs: registers(1), flags: 0x893, readOnly: true, initial: [[0x3000, bytes, mask]]},
      {reason: 3, pc: 0x1000, retired: 0, reads: 1, fault: [0x3000, bytes | 2 << 8 | 2 << 16]});
  }
  check('atomic32-add', 'locked-code-write-stops-after-committed-result-and-flags', 8,
    {regs: registers(1, 3, 0x1000)},
    {reason: 6, pc: 0x1003, retired: 1, flags: 2, helpers: 1, writes: [[0x1000, 4, 0x0f0301f1]]});
}
