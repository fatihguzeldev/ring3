import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {PC, FP, RAM, defined, envelope, ext, packet} from './support.mjs';

const registerStore = {name: 'register-final-store',
  code: [0xd9, 0xe0, 0xd9, 0x1f, 0xff, 0xe2],
  steps: [{opcode: 0x1e0, word: 15, control: 1},
    {opcode: 0x11f, word: 4 | 1 << 16, control: 0x401}]};
const readStore = {name: 'read-final-store',
  code: [0xd9, 0x03, 0xd9, 0x1f, 0xff, 0xe2],
  steps: [{opcode: 0x103, word: 3, control: 0x401}, registerStore.steps[1]]};
const hash = bytes => createHash('sha256').update(bytes).digest('hex');

function stop(result, reason, retired, pc, flags = 0x202) {
  assert.equal(result.status, 0);
  assert.deepEqual([72, 76, 48, 52].map(at => result.arena.readUInt32LE(at)),
    [reason, retired, pc, flags], 'literal final-store frontier and retirement');
}
function guestMemory(c, writes) {
  const memory = new DataView(c.memory.buffer);
  for (const [address, original] of [[PC, c.code], [RAM, c.data]]) {
    const expected = Buffer.from(original);
    for (const [at, value] of writes) if (at >= address && at + 4 <= address + expected.length)
      expected.writeFloatLE(value, at - address);
    const actual = Buffer.alloc(4096);
    for (let offset = 0; offset < actual.length; offset += 4) {
      c.ok('read32', address + offset);
      actual.writeUInt32LE(memory.getUint32(c.base + 120, true), offset);
    }
    assert.deepEqual(actual, expected, 'entire mapped code/data, including exact permitted store bytes');
  }
}

export function verifyFinalStores(make, observations) {
  function compare(label, program, row, verify, {writes = [], protect} = {}) {
    const scalar = make(program, {scalar: true}), paired = make(program);
    try {
      assert.deepEqual(paired.bytes, scalar.bytes);
      assert.ok(paired.imports.some(item => item.name === 'x87_resident_pair'));
      scalar.seed(row); paired.seed(row);
      if (protect) for (const c of [scalar, paired]) c.ok('protect', RAM, 1, protect);
      const a = scalar.execute(row.budget ?? 2), b = paired.execute(row.budget ?? 2);
      assert.deepEqual(defined(b.arena), defined(a.arena), `${label}: exact full nontransfer arena`);
      assert.deepEqual(b.receipt, a.receipt, `${label}: real receipt/owner/prefix`);
      assert.equal(b.cr2, a.cr2);
      verify(b);
      guestMemory(scalar, writes); guestMemory(paired, writes);
      observations.push({label, retired: b.arena.readUInt32LE(76), reason: b.arena.readUInt32LE(72),
        arena_sha256: hash(defined(b.arena)), pairCalls: b.calls.filter(call => call.name === 'x87_resident_pair')});
    } finally {scalar.close(); paired.close();}
  }
  for (const pc of [PC, PC + 2]) for (const budget of [0, 1, 2, 3]) {
    const available = pc === PC ? 3 : 2, retired = Math.min(available, budget);
    const stored = pc === PC ? retired >= 2 : retired >= 1;
    const value = pc === PC ? -1 : 1;
    compare(`final-store entry${pc - PC}/budget${budget}`, registerStore, {pc, budget}, result => {
      stop(result, budget > available ? 3 : 1, retired,
        retired === available ? 0x1100 : pc + retired * 2);
      const calls = result.calls.filter(call => call.name === 'x87_resident_pair');
      assert.equal(calls.length, pc === PC && budget > 0 ? 1 : 0);
      if (calls.length) assert.equal(BigInt(calls[0].result),
        envelope(0, Math.min(2, budget), budget >= 2 ? 1 : 0));
      assert.equal(result.arena.readUInt16LE(FP + 20), stored ? 0xffff : 0xfffc);
      assert.deepEqual(result.arena.subarray(FP + 40, FP + 50), ext(retired && pc === PC ? -1 : 1));
    }, {writes: stored ? [[RAM + 4, value]] : []});
  }
  compare('read then final store uses write4 rather than same-width read validator', readStore,
    {}, result => {
      stop(result, 1, 2, PC + 4);
      assert.equal(result.arena.readUInt16LE(FP + 20), 0xfffc);
      assert.equal(result.arena.readUInt32LE(FP + 28), RAM + 4);
      assert.equal(result.arena.readUInt32LE(136), 4);
      assert.equal(BigInt(result.calls.find(call => call.name === 'x87_resident_pair').result), envelope(0, 2, 1));
    }, {writes: [[RAM + 4, 3]]});
  compare('final store alias observes the actual first load', readStore,
    {secondAddress: RAM}, result => stop(result, 1, 2, PC + 4), {writes: [[RAM, 3]]});
  for (const row of [{flags: 0x302, reason: 16, retired: 1},
    {flags: 0x10202, reason: 1, retired: 2}, {shadow: 1, reason: 1, retired: 2}]) {
    compare('final-store original debug boundary', registerStore, row, result => {
      stop(result, row.reason, row.retired, PC + row.retired * 2, row.flags === 0x302 ? 0x302 : 0x202);
      assert.equal(result.calls.find(call => call.name === 'x87_resident_pair').args[8], 1);
    }, {writes: row.retired === 2 ? [[RAM + 4, -1]] : []});
  }
  compare('final store write PF preserves first FP result without pop', registerStore,
    {secondAddress: 0x6000}, result => {
      stop(result, 23, 1, PC + 2);
      assert.deepEqual([80, 84, 88, 92].map(at => result.arena.readUInt32LE(at)), [6, 0x6000, 2, 4]);
      assert.equal(result.cr2, 0x6000);
      assert.equal(result.arena.readUInt16LE(FP + 20), 0xfffc);
      assert.deepEqual(result.arena.subarray(FP + 40, FP + 50), ext(-1));
      assert.equal(BigInt(result.calls.find(call => call.name === 'x87_resident_pair').result), envelope(23, 1, 1));
    });
  compare('final store read-only PF retains first FP prefix', registerStore, {}, result => {
    stop(result, 23, 1, PC + 2);
    assert.deepEqual([80, 84, 88, 92].map(at => result.arena.readUInt32LE(at)), [7, RAM + 4, 2, 4]);
    assert.deepEqual(result.arena.subarray(FP + 40, FP + 50), ext(-1));
  }, {protect: 1});
  for (const budget of [2, 3]) compare('final self-modifying store retires before invalidation exit',
    registerStore, {secondAddress: PC + 2, budget}, result => {
      stop(result, 6, 2, PC + 4);
      assert.equal(result.arena.readUInt16LE(FP + 20), 0xffff);
      assert.equal(BigInt(result.calls.find(call => call.name === 'x87_resident_pair').result), envelope(11, 2, 1));
    }, {writes: [[PC + 2, -1]]});

  // synthetic controls prove only the generated adapter's envelope/packet rules.
  for (const [name, progress, modify, row = {}, retired = 0] of [
    ['SMC missing completion', envelope(11, 1, 1)],
    ['SMC reports first', envelope(11, 2, 0)],
    ['SMC exceeds original debug max', envelope(11, 2, 1), undefined, {flags: 0x302}],
    ['SMC failed packet', envelope(11, 2, 1), bytes => bytes.writeUInt32LE(1, 16)],
    ['SMC malformed success header', envelope(11, 2, 1), bytes => bytes.writeUInt32LE(39, 8), {}, 1],
    ['SMC wrong length', envelope(11, 2, 1), bytes => bytes.writeUInt32LE(0, 36), {}, 1],
    ['SMC forged flags', envelope(11, 2, 1), bytes => bytes.writeUInt32LE(0x203, 20), {}, 1],
  ]) {
    const c = make(registerStore);
    try {
      c.seed(row); const before = c.arena();
      c.synthetic = () => { const bytes = packet(4, row.flags ?? 0x202); modify?.(bytes); return {envelope: progress, packet: bytes}; };
      const result = c.execute(2, {consume: false});
      stop(result, 7, retired, retired ? PC + 2 : PC, row.flags ?? 0x202);
      assert.equal(result.arena.readUInt32LE(80), 2);
      assert.deepEqual(result.arena.subarray(FP, FP + 128), before.subarray(FP, FP + 128));
      guestMemory(c, []);
      observations.push({label: `malformed final-store ${name}`, reason: 7, retired});
    } finally {c.close();}
  }
  const noStore = {...registerStore, name: 'nonstore-SMC-negative',
    code: [0xd9, 0xe0, 0xd9, 0xe1, 0xff, 0xe2]};
  const c = make(noStore);
  try {
    c.seed(); c.synthetic = () => ({envelope: envelope(11, 2, 1), packet: packet(0)});
    const result = c.execute(2, {consume: false});
    stop(result, 7, 0, PC);
    assert.equal(result.arena.readUInt32LE(80), 2);
    guestMemory(c, []);
    observations.push({label: 'nonstore pair cannot claim committed SMC retirement', reason: 7, retired: 0});
  } finally {c.close();}
}
