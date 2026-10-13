import assert from 'node:assert/strict';

const zero = Buffer.alloc(16);
const raw = Buffer.from(
  '0100807f4523c17f0000008001000000efbeaddeffffffff0000803f000000ff',
  'hex',
);
const memory = Buffer.from(
  '0020807f00aa557f78563412fedcba98112233445566778899aabbccddeeff00',
  'hex',
);
assert.equal(raw.length, 32);
assert.equal(memory.length, 32);

function vector(register) {
  return Buffer.from(raw.map(byte => byte ^ (register * 0x11)));
}

function sources(...registers) {
  return {
    lower: Object.fromEntries(registers.map(register => [register, vector(register).subarray(0, 16)])),
    upper: Object.fromEntries(registers.map(register => [register, vector(register).subarray(16)])),
  };
}

function encode(kind, destination, first, second, immediate = 0, options = {}) {
  const { prefix = [], memory = false, address16 = null } = options;
  const opcode = { broadcast: 0x5a, insert: 0x38, extract: 0x39, permute: 0x46 }[kind];
  const reserved = kind === 'broadcast' || kind === 'extract';
  const control = 5 + (reserved ? 15 : 15 - first) * 8;
  const field = kind === 'extract' ? first : destination;
  const operand = memory
    ? address16 === null
      ? [field * 8]
      : [field * 8 + 6, address16 & 255, address16 >>> 8]
    : [0xc0 + field * 8 + (kind === 'extract' ? destination : second)];
  const bytes = [
    ...prefix,
    ...(address16 === null ? [] : [0x67]),
    0xc4,
    kind === 'broadcast' ? 0xe2 : 0xe3,
    control,
    opcode,
    ...operand,
    ...(kind === 'broadcast' ? [] : [immediate]),
  ];
  return Buffer.from(bytes).toString('hex');
}

export const blocksRows = [];
const aliases = [
  { name: 'distinct', destination: 0, first: 1, second: 2 },
  { name: 'destination-first', destination: 1, first: 1, second: 2 },
  { name: 'destination-second', destination: 2, first: 1, second: 2 },
  { name: 'sources-equal', destination: 0, first: 1, second: 1 },
  { name: 'all-equal', destination: 1, first: 1, second: 1 },
];

for (const { name, destination, first, second } of aliases) {
  for (const selectedHalf of [0, 1]) {
    for (const ignored of [0, 2, 4, 8, 16, 32, 64, 128]) {
      const immediate = selectedHalf + ignored;
      const expected = Buffer.from(vector(first));
      vector(second).copy(expected, selectedHalf * 16, 0, 16);
      blocksRows.push({
        name: `avx2-insert-${name}-half${selectedHalf}-ignored${ignored}`,
        code: encode('insert', destination, first, second, immediate),
        ...sources(first, second),
        outLower: { [destination]: expected.subarray(0, 16) },
        outUpper: { [destination]: expected.subarray(16) },
      });
    }
  }
}

for (let source = 0; source < 8; source++) {
  for (const destination of [source, (source + 1) % 8]) {
    for (const selectedHalf of [0, 1]) {
      blocksRows.push({
        name: `avx2-extract-source${source}-destination${destination}-half${selectedHalf}`,
        code: encode('extract', destination, source, null, selectedHalf),
        ...sources(source),
        outLower: { [destination]: vector(source).subarray(selectedHalf * 16, selectedHalf * 16 + 16) },
        outUpper: { [destination]: zero },
      });
    }
  }
}
for (const selectedHalf of [0, 1]) {
  for (const ignored of [2, 4, 8, 16, 32, 64, 128]) {
    blocksRows.push({
      name: `avx2-extract-ignored${ignored}-half${selectedHalf}`,
      code: encode('extract', 7, 6, null, selectedHalf + ignored),
      ...sources(6),
      outLower: { 7: vector(6).subarray(selectedHalf * 16, selectedHalf * 16 + 16) },
      outUpper: { 7: zero },
    });
  }
}

function permuteRow(name, destination, first, second, low, high, immediate) {
  const a = vector(first);
  const b = vector(second);
  const choices = [a.subarray(0, 16), a.subarray(16), b.subarray(0, 16), b.subarray(16), zero];
  return {
    name,
    code: encode('permute', destination, first, second, immediate),
    ...sources(first, second),
    outLower: { [destination]: choices[low] },
    outUpper: { [destination]: choices[high] },
  };
}

for (let low = 0; low < 4; low++) {
  for (let high = 0; high < 4; high++) {
    blocksRows.push(permuteRow(`avx2-permute-select-${low}-${high}`, 0, 1, 2, low, high, low + high * 16));
  }
  blocksRows.push(permuteRow(`avx2-permute-zero-low-select-${low}`, 0, 1, 2, 4, low, 8 + low * 16));
  blocksRows.push(permuteRow(`avx2-permute-zero-high-select-${low}`, 0, 1, 2, low, 4, 128 + low));
}
blocksRows.push(permuteRow('avx2-permute-zero-both', 0, 1, 2, 4, 4, 0xbb));
for (const ignored of [0, 4, 64, 68]) {
  blocksRows.push(permuteRow(`avx2-permute-ignored${ignored}`, 7, 5, 6, 3, 1, 0x13 + ignored));
}
for (const { name, destination, first, second } of aliases) {
  for (const [immediate, low, high] of [[0x13, 3, 1], [0x88, 4, 4], [0xfe, 4, 4]]) {
    blocksRows.push(permuteRow(`avx2-permute-${name}-control${immediate}`, destination, first, second, low, high, immediate));
  }
}

function memoryRow(kind, address, extra = {}) {
  const destination = 3;
  const first = 3;
  const write = kind === 'extract';
  const width = kind === 'permute' ? 32 : 16;
  const immediate = kind === 'permute' ? 0x13 : 0xff;
  const selected = vector(first).subarray(16);
  const value = memory.subarray(0, width);
  let lower = value;
  let upper = value;
  if (kind === 'insert') {
    lower = vector(first).subarray(0, 16);
  } else if (kind === 'permute') {
    lower = memory.subarray(16);
    upper = vector(first).subarray(16);
  }
  return {
    name: `avx2-${kind}-memory-${address.toString(16)}`,
    code: encode(kind, destination, first, null, immediate, { memory: true }),
    address,
    ...sources(first),
    ...(write
      ? { write: width, staged: selected, checkRam: selected }
      : {
          read: width,
          memory: value,
          outLower: { [destination]: lower },
          outUpper: { [destination]: upper },
        }),
    ...extra,
  };
}

for (const kind of ['broadcast', 'insert', 'extract', 'permute']) {
  const width = kind === 'permute' ? 32 : 16;
  for (const address of [0x5011, 0x6000 - width]) {
    blocksRows.push(memoryRow(kind, address));
  }
  blocksRows.push(memoryRow(kind, 0x5013, {
    name: `avx2-${kind}-address16-unaligned`,
    code: encode(kind, 3, 3, null, kind === 'permute' ? 0x13 : 0xff, { memory: true, address16: 0x5013 }),
  }));
  for (const address of [0x6001 - width, 0x6000]) {
    const fault = memoryRow(kind, address, {
      name: `avx2-${kind}-page-fault-${address.toString(16)}`,
      reason: 5,
      detail: 1,
      fault: 0x6000,
      access: kind === 'extract' ? 2 : 1,
      length: width,
    });
    delete fault.memory;
    if (kind === 'extract' && address < 0x6000) {
      fault.memory = Buffer.alloc(0x6000 - address, 0x66);
    }
    blocksRows.push(fault);
  }
  for (const [segment, prefix, reason] of [['ds', [], 11], ['ss', [0x36], 15]]) {
    const fault = memoryRow(kind, 0x100000001 - width, {
      name: `avx2-${kind}-${segment}-span-overflow`,
      code: encode(kind, 3, 3, null, kind === 'permute' ? 0x13 : 0xff, { memory: true, prefix }),
      helpers: 0,
      reason,
    });
    delete fault.memory;
    blocksRows.push(fault);
  }
}

blocksRows.push(memoryRow('permute', 0x5020, {
  name: 'avx2-permute-zero-both-still-reads-full32',
  code: encode('permute', 3, 3, null, 0x88, { memory: true }),
  outLower: { 3: zero },
  outUpper: { 3: zero },
}));
const zeroFault = memoryRow('permute', 0x5ff0, {
  name: 'avx2-permute-zero-both-keeps-late-memory-fault',
  code: encode('permute', 3, 3, null, 0x88, { memory: true }),
  reason: 5,
  detail: 1,
  fault: 0x6000,
  access: 1,
  length: 32,
});
delete zeroFault.memory;
blocksRows.push(zeroFault);

assert.equal(blocksRows.length, 200);
assert.equal(new Set(blocksRows.map(row => row.name)).size, blocksRows.length);
assert.equal(blocksRows.filter(row => row.reason).length, 17);
assert.equal(blocksRows.filter(row => row.memory).length, 11);
export const blocksSourcePaths = [
  'engine/src/cpu/x86/decode/avx/cross_lane.rs',
  'engine/src/cpu/x86/avx/operation/factory/cross_lane.rs',
  'engine/src/cpu/x86/avx/cross_lane/mod.rs',
  'engine/tests/cpu/simd/avx/execution/wasm.rs',
  'engine/tests/cpu/simd/avx/blocks/fixtures/p2-avx2-blocks/runtime/cases.mjs',
];
