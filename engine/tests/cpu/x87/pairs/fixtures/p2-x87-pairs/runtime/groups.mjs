import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {PC, FP, RAM, defined, groupEnvelope, packet, ext} from './support.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const operations = {
  load: {bytes: [0xd9, 0x03], opcode: 0x103, word: 3, control: 0x401},
  mul: {bytes: [0xd8, 0x0f], opcode: 0x00f, word: 9 | 1 << 8, control: 0x401},
  twice: {bytes: [0xd8, 0xc0], opcode: 0x0c0, word: 8, control: 1},
  add: {bytes: [0xd8, 0x03], opcode: 0x003, word: 9, control: 0x401},
  div: {bytes: [0xd8, 0x37], opcode: 0x037, word: 9 | 4 << 8, control: 0x401},
  neg: {bytes: [0xd9, 0xe0], opcode: 0x1e0, word: 15, control: 1},
  abs: {bytes: [0xd9, 0xe1], opcode: 0x1e1, word: 16, control: 1},
  store: {bytes: [0xd9, 0x1f], opcode: 0x11f, word: 4 | 1 << 16, control: 0x401},
};
export function program(name, names, data = []) {
  return {name, code: [...names.flatMap(name => operations[name].bytes), 0xff, 0xe2],
    steps: names.map((name, index) => ({...operations[name], pc: PC + index * 2, name})),
    groups: [{first: 0, length: names.length}], data};
}
const data = [{offset: 0, value: 1.234567}, {offset: 4, value: 0.7654321}];
const four = program('group-mixed4', ['load', 'mul', 'twice', 'div'], data);
const eight = program('group-mixed8', ['load', 'mul', 'twice', 'div', 'mul', 'add', 'div', 'neg'], data);
const store = program('group-final-store4', ['load', 'mul', 'add', 'store'], data);

// positive finite rational arithmetic, rounded independently to pc24/pc53/pc64/rn.
// the bounded fixture never approaches exponent, denormal or NaN boundaries.
const bits = value => {
  const bytes = Buffer.alloc(4); bytes.writeFloatLE(value); return bytes.readUInt32LE();
};
function binary32(value) {
  const raw = bits(value), significand = BigInt((raw & 0x7fffff) | 0x800000);
  const exponent = (raw >>> 23 & 255) - 127 - 23;
  return exponent >= 0 ? {n: significand << BigInt(exponent), d: 1n}
    : {n: significand, d: 1n << BigInt(-exponent)};
}
function rounded(value, precision) {
  let exponent = value.n.toString(2).length - value.d.toString(2).length;
  const lower = exponent >= 0 ? value.d << BigInt(exponent) : value.d;
  if ((exponent >= 0 ? value.n : value.n << BigInt(-exponent)) < lower) exponent--;
  const shift = precision - 1 - exponent;
  const numerator = shift >= 0 ? value.n << BigInt(shift) : value.n;
  const denominator = shift >= 0 ? value.d : value.d << BigInt(-shift);
  let significand = numerator / denominator;
  const remainder = numerator % denominator;
  const incremented = remainder * 2n > denominator || remainder * 2n === denominator && (significand & 1n) !== 0n;
  if (incremented) significand++;
  if (significand === 1n << BigInt(precision)) { significand >>= 1n; exponent++; }
  const scale = exponent - precision + 1;
  const rational = scale >= 0 ? {n: significand << BigInt(scale), d: 1n}
    : {n: significand, d: 1n << BigInt(-scale)};
  const raw = Buffer.alloc(10);
  raw.writeBigUInt64LE(significand << BigInt(64 - precision)); raw.writeUInt16LE(0x3fff + exponent, 8);
  return {rational, raw, exponent, significand, inexact: remainder !== 0n, incremented};
}
export function numeric(program, count, control) {
  const precision = new Map([[0x003f, 24], [0x023f, 53], [0x037f, 64]]).get(control);
  assert.ok(precision, 'explicit supported precision with round-to-nearest');
  const a = binary32(program.data[0].value), b = binary32(program.data[1].value);
  let value, raw = ext(1), status = 0, negative = false;
  for (const step of program.steps.slice(0, count)) {
    if (step.name === 'load') {
      value = a; raw = rounded(value, 64).raw; status = 0x3800;
    } else if (step.name === 'neg') {
      negative = !negative; raw = Buffer.from(raw); raw[9] ^= 0x80; status &= ~0x200;
    } else if (step.name !== 'store') {
      let exact;
      if (step.name === 'mul') exact = {n: value.n * b.n, d: value.d * b.d};
      if (step.name === 'twice') exact = {n: value.n * 2n, d: value.d};
      if (step.name === 'add') exact = {n: value.n * a.d + a.n * value.d, d: value.d * a.d};
      if (step.name === 'div') exact = {n: value.n * b.d, d: value.d * b.n};
      const next = rounded(exact, precision);
      value = next.rational; raw = next.raw;
      status = status & ~0x200 | (next.inexact ? 0x20 : 0) | (next.incremented ? 0x200 : 0);
    } else {
      const next = rounded(value, 24);
      status = (status & ~0x3a00) | (next.inexact ? 0x20 : 0) | (next.incremented ? 0x200 : 0);
      const storedBits = (next.exponent + 127) * 0x800000 + Number(next.significand & 0x7fffffn);
      return {raw, status, storedBits, tag: 0xfffc};
    }
  }
  return {raw, status, tag: count ? 0x3ffc : 0xfffc};
}

export function stop(result, reason, retired, pc, flags = 0x202) {
  assert.equal(result.status, 0);
  assert.deepEqual([72, 76, 48, 52].map(at => result.arena.readUInt32LE(at)), [reason, retired, pc, flags]);
}
export function memory(c, writes = []) {
  const view = new DataView(c.memory.buffer);
  for (const [address, original] of [[PC, c.code], [RAM, c.data]]) {
    const expected = Buffer.from(original), actual = Buffer.alloc(4096);
    for (const [at, value] of writes) if (at >= address && at + 4 <= address + expected.length)
      expected.writeUInt32LE(value >>> 0, at - address);
    for (let offset = 0; offset < actual.length; offset += 4) {
      c.ok('read32', address + offset); actual.writeUInt32LE(view.getUint32(c.base + 120, true), offset);
    }
    assert.deepEqual(actual, expected, 'entire mapped code/data follows literal write set');
  }
}

export function verifyGroups(make, observations) {
  function compare(label, source, row, verify, {writes = [], forceOne = false, cancelAfterFirst = false} = {}) {
    const scalar = make(source, {scalar: true}), grouped = make(source);
    try {
      assert.deepEqual(grouped.bytes, scalar.bytes);
      assert.ok(grouped.imports.some(item => item.name === 'x87_resident_group'));
      scalar.seed(row); grouped.seed(row);
      scalar.forceOne = grouped.forceOne = forceOne;
      scalar.cancelAfterFirst = grouped.cancelAfterFirst = cancelAfterFirst;
      const a = scalar.execute(row.budget ?? source.steps.length), b = grouped.execute(row.budget ?? source.steps.length);
      assert.deepEqual(defined(b.arena), defined(a.arena), `${label}: complete architectural/helper/exit arena`);
      assert.deepEqual(b.receipt, a.receipt, `${label}: literal resident receipt/owner`);
      assert.equal(b.cr2, a.cr2); verify(b);
      memory(scalar, writes); memory(grouped, writes);
      observations.push({label, retired: b.arena.readUInt32LE(76), reason: b.arena.readUInt32LE(72),
        arena_sha256: hash(defined(b.arena)), groupCalls: b.calls.filter(call => call.name === 'x87_resident_group')});
    } finally {scalar.close(); grouped.close();}
  }
  for (const source of [four, eight]) for (const control of [0x003f, 0x023f, 0x037f]) {
    for (let budget = 0; budget <= source.steps.length + 1; budget++) {
      compare(`${source.name}/CW${control}/budget${budget}`, source, {control, budget}, result => {
        const retired = Math.min(budget, source.steps.length + 1), count = Math.min(budget, source.steps.length);
        stop(result, 1, retired, budget > source.steps.length ? 0x1100 : PC + count * 2);
        const wanted = numeric(source, count, control);
        assert.deepEqual(result.arena.subarray(FP + (count ? 110 : 40), FP + (count ? 120 : 50)), wanted.raw,
          'independent precision-rounded full80 value');
        assert.equal(result.arena.readUInt16LE(FP + 18), wanted.status, 'literal TOP/PE/C1');
        assert.equal(result.arena.readUInt16LE(FP + 20), wanted.tag);
        const calls = result.calls.filter(call => call.name === 'x87_resident_group');
        assert.equal(calls.length, budget ? 1 : 0);
        if (calls.length) {
          assert.equal(calls[0].args[4], 0); assert.equal(calls[0].args[14], Math.min(budget, source.steps.length));
          assert.equal(BigInt(calls[0].result), groupEnvelope(0, count, count - 1));
        }
      });
    }
    for (let index = 1; index < source.steps.length; index++) compare(`${source.name}/side${index}/CW${control}`,
      source, {control, pc: PC + index * 2, budget: 1}, result => {
        stop(result, 1, 1, PC + (index + 1) * 2);
        assert.equal(result.calls.filter(call => call.name === 'x87_resident_group').length, 0);
      });
  }
  for (const row of [{flags: 0x302}, {flags: 0x10202}, {shadow: 1}])
    compare('group original debug metadata limits first native call', four, {...row, budget: 4}, result => {
      const trapped = row.flags === 0x302;
      stop(result, trapped ? 16 : 1, trapped ? 1 : 4, PC + (trapped ? 2 : 8), trapped ? 0x302 : 0x202);
      assert.equal(result.calls.find(call => call.name === 'x87_resident_group').args[14], 1);
    });
  compare('group initial Cancel precedes pending MF', four, {cancel: 1, control: 0x37e, status: 1}, result => {
    stop(result, 2, 0, PC); assert.equal(result.calls.length, 0);
  });
  compare('group first waiting MF precedes RF clearing and operand access', four,
    {status: 1, control: 0x37e, flags: 0x10202, nullDs: true}, result => {
      stop(result, 12, 0, PC, 0x10202);
      assert.equal(result.arena.readUInt16LE(FP + 18), 0x8081);
      assert.equal(result.cr2, null);
      assert.deepEqual(result.calls.map(call => call.name), ['x87_resident_op'],
        'the original scalar waiting precheck publishes MF before the group import');
      assert.deepEqual(result.calls[0].args.slice(4), [3, RAM, 0x10202, PC, 0x103, 0x401]);
    });
  compare('group cancellation after one completed native step uses original boundary', four, {}, result => {
    stop(result, 2, 1, PC + 2);
    assert.deepEqual(result.arena.subarray(FP + 110, FP + 120), numeric(four, 1, 0x37f).raw);
    assert.equal(BigInt(result.calls.find(call => call.name === 'x87_resident_group').result), groupEnvelope(0, 1, 0));
  }, {forceOne: true, cancelAfterFirst: true});
  compare('group later read PF retains real arithmetic prefix', four, {secondAddress: 0x6000}, result => {
    stop(result, 23, 1, PC + 2);
    assert.deepEqual([80, 84, 88, 92].map(at => result.arena.readUInt32LE(at)), [4, 0x6000, 1, 4]);
    assert.equal(result.cr2, 0x6000);
    assert.equal(BigInt(result.calls.find(call => call.name === 'x87_resident_group').result), groupEnvelope(23, 1, 1));
  });
  compare('group CS limit returns completed prefix for original next boundary', four, {csLimit: PC + 3}, result => {
    stop(result, 11, 2, PC + 4);
    assert.equal(BigInt(result.calls.find(call => call.name === 'x87_resident_group').result), groupEnvelope(0, 2, 1));
  });
  const deferred = {name: 'group-deferred-MF', code: [0xd8, 0xf1, 0xd9, 0x07, 0xd9, 0xe0, 0xd9, 0xe1, 0xff, 0xe2],
    steps: [{pc: PC, opcode: 0xf1, word: 8 | 4 << 8 | 0x10 << 16, control: 1},
      {pc: PC + 2, opcode: 0x107, word: 3, control: 0x401},
      {...operations.neg, pc: PC + 4}, {...operations.abs, pc: PC + 6}], groups: [{first: 0, length: 4}]};
  compare('group new pending MF defers before unmapped next operand', deferred,
    {control: 0x37b, zeroSecondStack: true, secondAddress: 0x6000}, result => {
      stop(result, 12, 1, PC + 2); assert.equal(result.cr2, null);
      assert.equal(result.arena.readUInt16LE(FP + 18), 0x8084);
      assert.equal(BigInt(result.calls.find(call => call.name === 'x87_resident_group').result), groupEnvelope(0, 1, 0));
    });
  for (const control of [0x003f, 0x023f, 0x037f]) {
    const wanted = numeric(store, 4, control);
    compare(`group terminal store/CW${control}`, store, {control, budget: 4}, result => {
      stop(result, 1, 4, PC + 8);
      assert.equal(result.arena.readUInt16LE(FP + 18), wanted.status);
      assert.equal(result.arena.readUInt16LE(FP + 20), wanted.tag);
      assert.equal(BigInt(result.calls.find(call => call.name === 'x87_resident_group').result), groupEnvelope(0, 4, 3));
    }, {writes: [[RAM + 4, wanted.storedBits]]});
  }
  const storeFault = program('group-final-store-writePF', ['load', 'twice', 'twice', 'store'], data);
  compare('group terminal write PF preserves three committed operations and no pop', storeFault,
    {secondAddress: 0x6000}, result => {
      stop(result, 23, 3, PC + 6);
      assert.deepEqual([80, 84, 88, 92].map(at => result.arena.readUInt32LE(at)), [6, 0x6000, 2, 4]);
      assert.equal(result.cr2, 0x6000);
      assert.equal(result.arena.readUInt16LE(FP + 20), 0x3ffc);
      assert.deepEqual(result.arena.subarray(FP + 110, FP + 120), numeric(storeFault, 3, 0x37f).raw);
      assert.equal(BigInt(result.calls.find(call => call.name === 'x87_resident_group').result), groupEnvelope(23, 3, 3));
    });
  const terminal = {...four, name: 'group-terminal4', code: four.code.slice(0, 8)};
  for (const budget of [4, 5]) compare(`terminal group/budget${budget}`, terminal, {budget}, result => {
    stop(result, budget === 4 ? 1 : 3, 4, PC + 8);
    assert.equal(BigInt(result.calls.find(call => call.name === 'x87_resident_group').result), groupEnvelope(0, 4, 3));
  });
  for (const [label, progress, alter, row = {}, retired = 0] of [
    ['reserved', groupEnvelope(0, 4, 3) | 1n << 39n],
    ['count exceeds max', groupEnvelope(0, 5, 3)],
    ['reported outside group', groupEnvelope(0, 4, 4)],
    ['success without completed report', groupEnvelope(0, 3, 3)],
    ['success skips reported instruction', groupEnvelope(0, 2, 0)],
    ['failed packet claims reported completion', groupEnvelope(0, 2, 1), bytes => bytes.writeUInt32LE(1, 16)],
    ['original debug max exceeded', groupEnvelope(0, 4, 3), undefined, {flags: 0x302}],
    ['nonstore SMC', groupEnvelope(11, 4, 3)],
    ['malformed last packet', groupEnvelope(0, 4, 3), bytes => bytes.writeUInt32LE(39, 8), {}, 3],
    ['malformed middle packet keeps reported PC', groupEnvelope(0, 2, 1), bytes => bytes.writeUInt32LE(39, 8), {}, 1],
    ['nonpacket stale success', groupEnvelope(7, 0, 0)],
    ['nonpacket stale success keeps reported PC', groupEnvelope(7, 2, 2), undefined, {}, 2],
  ]) {
    const c = make(four);
    try {
      c.seed(row); const before = c.arena();
      c.groupSynthetic = () => { const bytes = packet(4, row.flags ?? 0x202); alter?.(bytes); return {envelope: progress, packet: bytes}; };
      const result = c.execute(4, {consume: false});
      stop(result, 7, retired, PC + retired * 2, row.flags ?? 0x202);
      assert.deepEqual(result.arena.subarray(FP, FP + 128), before.subarray(FP, FP + 128));
      assert.equal(result.receipt, undefined); memory(c);
      observations.push({label: `group malformed ${label}`, reason: 7, retired});
    } finally {c.close();}
  }
  const c = make(four);
  try {
    c.seed(); const before = c.arena(); c.groupOrdinal = 0xffffffff;
    const result = c.execute(4, {consume: false}); stop(result, 7, 0, PC);
    assert.deepEqual(result.arena.subarray(FP, FP + 128), before.subarray(FP, FP + 128));
    assert.equal(result.calls.length, 1); memory(c);
    observations.push({label: 'group invalid final-IR ordinal is neutral', reason: 7, retired: 0});
  } finally {c.close();}
}
