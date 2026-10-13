import assert from 'node:assert/strict';

const data = Buffer.from(Array.from({ length: 32 }, (_, byte) => (byte * 13 + 7) & 255));

function encode(destination, options = {}) {
  const { w = false, wide = true, prefix = [], address16 = null, stack = false } = options;
  const operand = stack
    ? [destination * 8 + 4, 0x24]
    : address16 === null
      ? [destination * 8]
      : [destination * 8 + 6, address16 & 255, address16 >>> 8];
  return Buffer.from([
    ...prefix, ...(address16 === null ? [] : [0x67]),
    0xc4, 0xe2, 0x79 + (wide ? 4 : 0) + (w ? 0x80 : 0), 0x2a, ...operand,
  ]).toString('hex');
}

function row(destination, address, options = {}) {
  return {
    name: `stream-d${destination}-${address.toString(16)}-w${Number(Boolean(options.w))}`,
    code: encode(destination, options),
    address,
    read: 32,
    alignment: 32,
    memory: data,
    outLower: { [destination]: data.subarray(0, 16) },
    outUpper: { [destination]: data.subarray(16) },
  };
}

export const streamRows = [];
for (let destination = 0; destination < 8; destination++) {
  for (const w of [false, true]) {
    for (const address of [0x5020, 0x5fe0]) streamRows.push(row(destination, address, { w }));
  }
  streamRows.push({
    ...row(destination, 0x5040, { address16: 0x5040 }),
    name: `stream-d${destination}-address16`,
  });
  streamRows.push({
    ...row(destination, 0x5060, { stack: true }),
    name: `stream-d${destination}-default-ss`,
    r: { 4: 0x5060 },
  });
  streamRows.push({
    ...row(destination, 0x5080, { prefix: [0x64] }),
    name: `stream-d${destination}-fs`,
  });
  for (const address of [0x5010, 0x5ff0, 0x6001, 0x6000, 0x6020]) {
    const fault = row(destination, address, { w: true });
    delete fault.memory;
    if (address % 32) Object.assign(fault, { reason: 11, helpers: 0 });
    else Object.assign(fault, { reason: 5, detail: 1, fault: address, access: 1, length: 32 });
    streamRows.push(fault);
  }
  const old = row(destination, 0x5ff0, { wide: false, w: true });
  old.name = `legacy-stream-d${destination}-aligned16`;
  old.read = 16;
  old.alignment = 16;
  old.memory = data.subarray(0, 16);
  old.outUpper = { [destination]: Buffer.alloc(16) };
  streamRows.push(old);
}

function segments({ selected = null, limit = null, fsBase = 0 } = {}) {
  const bytes = Buffer.alloc(64);
  bytes.write('R3SG');
  bytes.writeUInt16LE(limit === null ? 1 : 2, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(64, 8);
  [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((selector, register) => bytes.writeUInt16LE(selector, 16 + register * 2));
  if (limit !== null) {
    [28, 32, 36, 40, 44, 56].forEach((offset, register) =>
      bytes.writeUInt32LE(register === 5 ? 0 : register === selected ? 0xf2300000 + limit : register === 1 ? 0xf3bfffff : 0xf33fffff, offset));
  }
  bytes.writeUInt32LE(fsBase, 48);
  return bytes;
}

for (const [selected, prefix, reason] of [[3, [], 11], [2, [0x36], 15]]) {
  for (const allowed of [true, false]) {
    const probe = row(3, 0x5000, { prefix });
    probe.name = `stream-cache-${selected}-${allowed ? 'lastbyte-fits' : 'lastbyte-denied'}`;
    probe.segments = segments({ selected, limit: allowed ? 0x501f : 0x501e });
    if (!allowed) {
      delete probe.memory;
      Object.assign(probe, { reason, helpers: 0 });
    }
    streamRows.push(probe);
  }
}
for (const [address, logical, allowed] of [[0x5020, 0x5010, true], [0x5030, 0x5020, false]]) {
  const probe = row(7, address, { prefix: [0x64], w: true });
  probe.name = `stream-linear-fs-base-${allowed ? 'aligned' : 'misaligned'}`;
  probe.r = { 0: logical };
  probe.segments = segments({ fsBase: 0x10 });
  if (!allowed) {
    delete probe.memory;
    Object.assign(probe, { reason: 11, helpers: 0 });
  }
  streamRows.push(probe);
}

assert.equal(streamRows.length, 110);
assert.equal(new Set(streamRows.map(row => row.name)).size, streamRows.length);
assert.equal(streamRows.filter(row => row.reason).length, 43);
assert.equal(streamRows.filter(row => row.segments).length, 6);
assert.equal(streamRows.filter(row => row.reason === 5 && row.address % 32).length, 0, 'no fabricated aligned cross-page fault');
export const streamSourcePaths = [
  'engine/src/cpu/x86/decode/avx/moves.rs',
  'engine/src/cpu/x86/avx/moves.rs',
  'engine/src/cpu/x86/avx/operation/factory/moves.rs',
  'engine/src/cpu/x86/segments/cache.rs',
  'engine/src/abi/x86/segments.rs',
  'engine/tests/cpu/simd/avx/moves/stream/native.rs',
  'engine/tests/cpu/simd/avx/execution/wasm.rs',
  'engine/tests/cpu/simd/avx/moves/stream/fixtures/p2-avx2-stream/runtime/cases.mjs',
];
