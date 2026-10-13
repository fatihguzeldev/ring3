import assert from 'node:assert/strict';

const forms = [
  { name: 'byte128', opcode: 0x78, element: 1, width: 16, memory: true },
  { name: 'byte256', opcode: 0x78, element: 1, width: 32, memory: true },
  { name: 'word128', opcode: 0x79, element: 2, width: 16, memory: true },
  { name: 'word256', opcode: 0x79, element: 2, width: 32, memory: true },
  { name: 'dword128', opcode: 0x58, element: 4, width: 16, memory: true },
  { name: 'dword256', opcode: 0x58, element: 4, width: 32, memory: true },
  { name: 'qword128', opcode: 0x59, element: 8, width: 16, memory: true },
  { name: 'qword256', opcode: 0x59, element: 8, width: 32, memory: true },
  { name: 'single128', opcode: 0x18, element: 4, width: 16, memory: false },
  { name: 'single256', opcode: 0x18, element: 4, width: 32, memory: false },
  { name: 'double256', opcode: 0x19, element: 8, width: 32, memory: false },
];
const literals = new Map([
  [1, Buffer.from('80', 'hex')],
  [2, Buffer.from('0180', 'hex')],
  [4, Buffer.from('0100807f', 'hex')],
  [8, Buffer.from('010000000000f07f', 'hex')],
]);
const repeated = new Map([
  [1, Buffer.from('80808080808080808080808080808080', 'hex')],
  [2, Buffer.from('01800180018001800180018001800180', 'hex')],
  [4, Buffer.from('0100807f0100807f0100807f0100807f', 'hex')],
  [8, Buffer.from('010000000000f07f010000000000f07f', 'hex')],
]);

function segments({ nullData = false, segment = 3, limit, fsBase = 0 } = {}) {
  const bytes = Buffer.alloc(64);
  bytes.write('R3SG');
  bytes.writeUInt16LE(2, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(64, 8);
  const selectors = [
    0x23,
    0x1b,
    0x23,
    nullData ? 0 : 0x23,
    nullData ? 0 : 0x3b,
    0,
  ];
  selectors.forEach((selector, index) =>
    bytes.writeUInt16LE(selector, 16 + index * 2),
  );
  const caches = [
    0xf33fffff,
    0xf3bfffff,
    0xf33fffff,
    nullData ? 0 : 0xf33fffff,
    nullData ? 0 : 0xf33fffff,
    0,
  ];
  if (limit !== undefined) caches[segment] = 0xf2300000 + limit;
  [28, 32, 36, 40, 44, 56].forEach((at, index) =>
    bytes.writeUInt32LE(caches[index], at),
  );
  bytes.writeUInt32LE(fsBase, 48);
  return bytes;
}

function encode(form, destination, source, prefix = [], address16 = false) {
  const modrm =
    (destination << 3) |
    (source === null ? (address16 ? 7 : 0) : 0xc0 | source);
  return Buffer.from([
    ...prefix,
    ...(address16 ? [0x67] : []),
    0xc4,
    0xe2,
    form.width === 32 ? 0x7d : 0x79,
    form.opcode,
    modrm,
  ]).toString('hex');
}

function output(form, destination) {
  return {
    outLower: { [destination]: repeated.get(form.element) },
    outUpper: {
      [destination]:
        form.width === 32 ? repeated.get(form.element) : Buffer.alloc(16),
    },
  };
}

export const broadcastRows = [];
for (const form of forms) {
  for (let source = 0; source < 8; source++) {
    for (const alias of [false, true]) {
      const destination = alias ? source : (source + 3) % 8;
      for (const poison of [0x31, 0xe2]) {
        const value = Buffer.alloc(32, poison);
        literals.get(form.element).copy(value);
        broadcastRows.push({
          name: `broadcast-${form.name}-s${source}-d${destination}-poison${poison}`,
          code: encode(
            form,
            destination,
            source,
            poison === 0x31 ? [0x64] : [],
          ),
          lower: { [source]: value.subarray(0, 16) },
          upper: { [source]: value.subarray(16) },
          segmentRecord: segments({ nullData: true }),
          mxcsr: 0,
          ...output(form, destination),
        });
      }
    }
  }
  if (!form.memory) continue;
  const memory = literals.get(form.element);
  const end = 0x6000 - form.element;
  const normal = {
    code: encode(form, 7, null),
    address: end,
    read: form.element,
    memory,
    mxcsr: 0,
    ...output(form, 7),
  };
  broadcastRows.push({
    ...normal,
    name: `broadcast-${form.name}-exact-page-end`,
  });
  broadcastRows.push({
    ...normal,
    name: `broadcast-${form.name}-address16-truncates-initial-offset`,
    code: encode(form, 7, null, [], true),
    r: { 3: 0xaabb0000 | end },
  });
  broadcastRows.push({
    ...normal,
    name: `broadcast-${form.name}-fs-linear-offset`,
    code: encode(form, 7, null, [0x64]),
    r: { 0: end - 0x5000 },
    segmentRecord: segments({ fsBase: 0x5000 }),
  });
  broadcastRows.push({
    ...normal,
    name: `broadcast-${form.name}-ss-exact-selected-limit`,
    code: encode(form, 7, null, [0x36]),
    segmentRecord: segments({ segment: 2, limit: 0x5fff }),
  });
  broadcastRows.push({
    name: `broadcast-${form.name}-no-operand-segment`,
    code: encode(form, 7, null),
    address: end,
    read: form.element,
    segmentRecord: segments({ nullData: true }),
    helpers: 0,
    reason: 11,
  });
  broadcastRows.push({
    name: `broadcast-${form.name}-ss-limit-before-page`,
    code: encode(form, 7, null, [0x36]),
    address: 0x6000,
    read: form.element,
    segmentRecord: segments({ segment: 2, limit: 0x5fff }),
    helpers: 0,
    reason: 15,
  });
  broadcastRows.push({
    name: `broadcast-${form.name}-full-logical-span-ffffffff`,
    code: encode(form, 7, null),
    address: 0xffffffff,
    read: form.element,
    ...(form.element === 1
      ? { reason: 5, detail: 1, fault: 0xffffffff, access: 1, length: 1 }
      : { reason: 11, helpers: 0 }),
  });
  if (form.element > 1)
    broadcastRows.push({
      name: `broadcast-${form.name}-late-byte-unmapped`,
      code: encode(form, 7, null),
      address: end + 1,
      read: form.element,
      reason: 5,
      detail: 1,
      fault: 0x6000,
      access: 1,
      length: form.element,
    });
}

for (const [name, code, element, width] of [
  ['old-avx1-single128-memory', 'c4e2791800', 4, 16],
  ['old-avx1-single256-memory', 'c4e27d1800', 4, 32],
  ['old-avx1-double256-memory', 'c4e27d1900', 8, 32],
])
  broadcastRows.push({
    name,
    code,
    address: 0x6000 - element,
    read: element,
    memory: literals.get(element),
    mxcsr: 0,
    ...output({ element, width }, 0),
  });

const blockA = Buffer.from('000102030405060708090a0b0c0d0e0f', 'hex');
const blockB = Buffer.from('808182838485868788898a8b8c8d8e8f', 'hex');
broadcastRows.push(
  {
    name: 'old-avx2-block-broadcast',
    code: 'c4e27d5a08',
    address: 0x5ff0,
    read: 16,
    memory: blockA,
    outLower: { 1: blockA },
    outUpper: { 1: blockA },
  },
  {
    name: 'old-avx2-block-insert',
    code: 'c4e36d38cb01',
    lower: { 2: blockA, 3: blockB },
    outLower: { 1: blockA },
    outUpper: { 1: blockB },
  },
  {
    name: 'old-avx2-block-extract',
    code: 'c4e37d39cb01',
    upper: { 1: blockB },
    outLower: { 3: blockB },
    outUpper: { 3: Buffer.alloc(16) },
  },
  {
    name: 'old-avx2-block-permute',
    code: 'c4e36d46cb21',
    lower: { 3: blockA },
    upper: { 2: blockB },
    outLower: { 1: blockB },
    outUpper: { 1: blockA },
  },
);

assert.equal(forms.length, 11);
assert.equal(forms.filter((form) => form.memory).length, 8);
assert.equal(broadcastRows.length, 421);
assert.equal(broadcastRows.filter((row) => row.reason).length, 30);
export const broadcastSourcePaths = [
  'engine/src/cpu/x86/avx/cross_lane/mod.rs',
  'engine/src/cpu/x86/avx/operation/factory/cross_lane.rs',
  'engine/src/cpu/x86/decode/avx/cross_lane.rs',
  'engine/src/abi/x86/segments.rs',
  'engine/src/cpu/dbt/wasm/segments/access.rs',
  'engine/tests/cpu/simd/avx/execution/wasm.rs',
  'engine/tests/cpu/simd/avx/broadcast/fixtures/p2-avx2-broadcast/runtime/cases.mjs',
];
