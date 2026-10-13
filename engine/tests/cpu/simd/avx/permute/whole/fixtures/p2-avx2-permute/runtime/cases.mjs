import assert from 'node:assert/strict';

const forms = [
  { name: 'ps', map: 2, opcode: 0x16, immediate: false },
  { name: 'd', map: 2, opcode: 0x36, immediate: false },
  { name: 'q', map: 3, opcode: 0, immediate: true },
  { name: 'pd', map: 3, opcode: 1, immediate: true },
];
function words(values) {
  const output = Buffer.alloc(32);
  values.forEach((value, lane) => output.writeUInt32LE(value >>> 0, lane * 4));
  return output;
}
const rawDwords = [
  0x7f800001, 0x80000000, 1, 0xdeadbeef, 0x7fc01234, 0xff800001, 0x3f800000, 0x12345678,
];
const rawQwords = Buffer.from(
  '010000000000f07f00000000000000800100000000000000110000000000f0ff',
  'hex',
);
function data(register) {
  return words(rawDwords.map((_, lane) => rawDwords[(lane + register) % 8]));
}
function indices(rotation, poison) {
  return words(Array.from({ length: 8 }, (_, lane) => ((7 + rotation - lane) % 8) | (poison & ~7)));
}
function select(source, selections, width) {
  return Buffer.concat(selections.map((lane) => source.subarray(lane * width, (lane + 1) * width)));
}
function variable(source, control) {
  return select(
    source,
    Array.from({ length: 8 }, (_, lane) => control[lane * 4] & 7),
    4,
  );
}
function immediate(source, control) {
  return select(
    source,
    Array.from({ length: 4 }, (_, lane) => (control >> (lane * 2)) & 3),
    8,
  );
}
function encode(form, [destination, control, source], value = 0, options = {}) {
  const { memory = false, prefix = [], address16 = null } = options;
  const vvvv = form.immediate ? 15 : 15 - control;
  const modrm = memory
    ? address16 === null
      ? [destination << 3]
      : [(destination << 3) | 6, address16 & 255, address16 >>> 8]
    : [0xc0 | (destination << 3) | source];
  return Buffer.from([
    ...prefix,
    ...(address16 === null ? [] : [0x67]),
    0xc4,
    0xe0 | form.map,
    (vvvv << 3) | 5 | (form.immediate ? 0x80 : 0),
    form.opcode,
    ...modrm,
    ...(form.immediate ? [value] : []),
  ]).toString('hex');
}
function output(destination, value) {
  return {
    outLower: { [destination]: value.subarray(0, 16) },
    outUpper: { [destination]: value.subarray(16) },
  };
}
export const wholeRows = [];
for (const form of forms.filter((form) => !form.immediate)) {
  for (const roles of [
    [7, 6, 5],
    [6, 6, 5],
    [5, 6, 5],
    [7, 6, 6],
    [6, 6, 6],
  ]) {
    for (const [rotation, poison] of [
      [0, 0],
      [3, 0xfffffff8],
      [7, 0x80000000],
    ]) {
      const vectors = Array.from({ length: 8 }, (_, register) => data(register));
      vectors[roles[1]] = indices(rotation, poison);
      wholeRows.push({
        name: `whole-${form.name}-${roles.join('/')}-${rotation}-${poison}`,
        code: encode(form, roles),
        lower: Object.fromEntries(
          vectors.map((value, register) => [register, value.subarray(0, 16)]),
        ),
        upper: Object.fromEntries(vectors.map((value, register) => [register, value.subarray(16)])),
        ...output(roles[0], variable(vectors[roles[2]], vectors[roles[1]])),
      });
    }
  }
  for (let register = 0; register < 8; register++) {
    const source = data(register),
      control = indices(register, 0xdeadbee8);
    const destination = (register + 2) % 8,
      index = (register + 1) % 8;
    wholeRows.push({
      name: `whole-${form.name}-source${register}`,
      code: encode(form, [destination, index, register]),
      lower: { [register]: source.subarray(0, 16), [index]: control.subarray(0, 16) },
      upper: { [register]: source.subarray(16), [index]: control.subarray(16) },
      ...output(destination, variable(source, control)),
    });
  }
}
assert.deepEqual(
  variable(words([10, 11, 12, 13, 20, 21, 22, 23]), words([7, 4, 1, 6, 0, 5, 2, 3])),
  words([23, 20, 11, 22, 10, 21, 12, 13]),
);
assert.deepEqual(
  immediate(rawQwords, 0x1b),
  Buffer.from('110000000000f0ff01000000000000000000000000000080010000000000f07f', 'hex'),
);
assert.deepEqual(
  immediate(rawQwords, 0xaa),
  Buffer.from('0100000000000000010000000000000001000000000000000100000000000000', 'hex'),
);
for (const form of forms.filter((form) => form.immediate)) {
  // the common qword lowering sees every immediate; the second opcode shares it.
  const controls =
    form.name === 'q'
      ? Array.from({ length: 256 }, (_, value) => value)
      : [0, 0x1b, 0xaa, 0xe4, 255];
  for (const control of controls)
    for (const destination of [6, 7]) {
      wholeRows.push({
        name: `whole-${form.name}-imm${control}-destination${destination}`,
        code: encode(form, [destination, 0, 6], control),
        lower: { 6: rawQwords.subarray(0, 16) },
        upper: { 6: rawQwords.subarray(16) },
        ...output(destination, immediate(rawQwords, control)),
      });
    }
}
for (const form of forms) {
  const source = data(0);
  const value = form.immediate ? immediate(source, 0) : variable(source, Buffer.alloc(32));
  const memory = (address, options = {}) => ({
    name: `whole-${form.name}-memory-${address.toString(16)}`,
    code: encode(form, [0, 1, 0], 0, { memory: true }),
    address,
    read: 32,
    memory: source,
    lower: { 0: Buffer.alloc(16, 0x91), 1: Buffer.alloc(16) },
    upper: { 0: Buffer.alloc(16, 0xe7), 1: Buffer.alloc(16) },
    ...output(0, value),
    ...options,
  });
  for (const address of [0x5011, 0x5fe0]) wholeRows.push(memory(address));
  wholeRows.push(
    memory(0x5013, {
      name: `whole-${form.name}-address16`,
      code: encode(form, [0, 1, 0], 0, { memory: true, address16: 0x5013 }),
    }),
  );
  wholeRows.push(
    memory(0x5015, {
      name: `whole-${form.name}-fs`,
      code: encode(form, [0, 1, 0], 0, { memory: true, prefix: [0x64] }),
    }),
  );
  for (const [address, label] of [
    [0x5fe1, 'last-byte'],
    [0x5ff0, 'unused-high-half'],
  ])
    wholeRows.push(
      memory(address, {
        name: `whole-${form.name}-${label}-fault`,
        memory: source.subarray(0, 0x6000 - address),
        reason: 5,
        detail: 1,
        fault: 0x6000,
        access: 1,
        length: 32,
      }),
    );
  for (const [segment, prefix, reason] of [
    ['ds', [], 11],
    ['ss', [0x36], 15],
  ]) {
    const fault = memory(0xffffffe1, {
      name: `whole-${form.name}-${segment}-span-before-page`,
      code: encode(form, [0, 1, 0], 0, { memory: true, prefix }),
      helpers: 0,
      reason,
    });
    delete fault.memory;
    wholeRows.push(fault);
  }
}
const source = words([10, 11, 12, 13, 20, 21, 22, 23]);
for (const [name, code, control, expected] of [
  [
    'variable-ps',
    'c4e2750cc2',
    words([7, 4, 1, 6, 0, 5, 2, 3]),
    words([13, 10, 11, 12, 20, 21, 22, 23]),
  ],
  ['variable-pd', 'c4e2750dc2', words([2, 0, 0, 0, 2, 0, 0, 0]), select(source, [1, 0, 3, 2], 8)],
  ['immediate-ps', 'c4e37d04c11b', Buffer.alloc(32), words([13, 12, 11, 10, 23, 22, 21, 20])],
  ['immediate-pd', 'c4e37d05c105', Buffer.alloc(32), select(source, [1, 0, 3, 2], 8)],
])
  wholeRows.push({
    name: `old-lane-local-${name}`,
    code,
    lower: { 1: source.subarray(0, 16), 2: control.subarray(0, 16) },
    upper: { 1: source.subarray(16), 2: control.subarray(16) },
    ...output(0, expected),
  });
assert.equal(wholeRows.length, 604);
assert.equal(new Set(wholeRows.map((row) => row.name)).size, wholeRows.length);
assert.equal(wholeRows.filter((row) => row.reason).length, 16);
export const wholeSourcePaths = [
  'engine/src/cpu/x86/avx/permute.rs',
  'engine/src/cpu/x86/avx/operation/factory/permute.rs',
  'engine/src/cpu/x86/decode/avx/permute.rs',
  'engine/tests/cpu/simd/avx/permute/whole/native.rs',
  'engine/tests/cpu/simd/avx/permute/whole/fixtures/p2-avx2-permute/runtime/cases.mjs',
];
