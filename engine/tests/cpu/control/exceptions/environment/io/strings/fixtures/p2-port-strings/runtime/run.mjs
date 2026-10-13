import { runStoppedFaults } from '../../../../../../support/stopped.mjs';

const forms = [
  ['ins-byte', [0x6c]],
  ['ins-word', [0x66, 0x6d]],
  ['ins-dword', [0x6d]],
  ['outs-byte', [0x6e]],
  ['outs-word', [0x66, 0x6f]],
  ['outs-dword', [0x6f]],
];
const counts = [
  [0, true, true],
  [1, false, false],
  [0xffffffff, false, false],
  [0x10000, false, true],
];
const zeroSchedules = [
  {},
  { flags: 0x102, reason: 16, version: 7 },
  { flags: 0x244cd7, shadow: 1 },
  { flags: 0x102, shadow: 1, reason: 16, version: 7 },
  { budget: 1, reason: 1 },
  { flags: 0x102, shadow: 1, budget: 0, reason: 1, retired: 0, offset: 0, work: 0 },
  { flags: 0x102, shadow: 1, cancel: 1, reason: 2, retired: 0, offset: 0, work: 0 },
];
function portRow(name, bytes, repeated, zero, count, extra = {}) {
  return {
    name,
    bytes: Buffer.from(bytes),
    registers: { 0: 0xaabbccdd, 1: count, 2: 0xffff, 6: 0xffffffff, 7: 0xffffffff },
    reason: zero ? 3 : 11,
    baseVersion: 6,
    retired: zero ? 1 : 0,
    offset: zero ? bytes.length : 0,
    ...(repeated ? { work: zero ? 1 : 0 } : {}),
    ...(zero ? { schedules: zeroSchedules } : {}),
    ...extra,
  };
}
const positive = [
  { name: 'unchanged-cpl3-hlt-control', bytes: Buffer.from([0xf4]), reason: 11, baseVersion: 6 },
];
for (const [name, form] of forms) {
  for (const address16 of [false, true]) {
    for (const repeated of [false, true]) {
      const bytes = [...(address16 ? [0x67] : []), ...(repeated ? [0xf3] : []), ...form];
      const mode = `${name}/${address16 ? 'cx' : 'ecx'}/${repeated ? 'rep' : 'single'}`;
      for (const [count, zero32, zero16] of counts) {
        const zero = repeated && (address16 ? zero16 : zero32);
        positive.push(portRow(`${mode}/count-${count.toString(16)}`, bytes, repeated, zero, count));
      }
      positive.push(
        portRow(`${mode}/complete-at-page-end`, bytes, repeated, repeated, 0, {
          target: 0x2000 - bytes.length,
        }),
      );
    }
  }
  for (const repeated of [false, true]) {
    positive.push({
      name: `${name}/invalid-lock/${repeated ? 'rep' : 'single'}`,
      bytes: Buffer.from([0xf0, ...(repeated ? [0xf3] : []), ...form]),
      registers: { 1: 0 },
      reason: 20,
      baseVersion: 1,
    });
  }
}
for (const opcode of [0x6c, 0x6e]) {
  positive.push(
    portRow(`byte-${opcode.toString(16)}/ignored66`, [0x66, 0xf3, opcode], true, true, 0),
  );
  for (const prefix of [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65]) {
    positive.push(
      portRow(
        `byte-${opcode.toString(16)}/segment-${prefix.toString(16)}`,
        [prefix, 0xf3, opcode],
        true,
        true,
        0,
      ),
    );
  }
}
function segmentRecord({ nullIndex, cacheIndex, cacheBits, baseIndex } = {}) {
  const record = Buffer.alloc(64);
  record.write('R3SG');
  record.writeUInt16LE(cacheIndex === undefined ? 1 : 2, 4);
  record.writeUInt16LE(1, 6);
  record.writeUInt32LE(64, 8);
  [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((selector, index) =>
    record.writeUInt16LE(selector, 16 + index * 2),
  );
  if (nullIndex !== undefined) record.writeUInt16LE(0, 16 + nullIndex * 2);
  if (cacheIndex !== undefined) {
    const offsets = [28, 32, 36, 40, 44, 56];
    [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0].forEach((bits, index) =>
      record.writeUInt32LE(bits, offsets[index]),
    );
    record.writeUInt32LE(cacheBits, offsets[cacheIndex]);
  }
  if (baseIndex !== undefined) {
    record.writeUInt16LE(baseIndex === 4 ? 0x3b : 0x43, 16 + baseIndex * 2);
    record.writeUInt32LE(0xfffffff0, 32 + baseIndex * 4);
  }
  return record;
}
const noAccess = [
  ['ins-null-es', [0x6c], segmentRecord({ nullIndex: 0 })],
  ['ins-readonly-zero-limit-es', [0x6d], segmentRecord({ cacheIndex: 0, cacheBits: 0xf2100000 })],
  ['outs-null-ds', [0x6e], segmentRecord({ nullIndex: 3 })],
  ['outs-null-fs', [0x64, 0x6e], segmentRecord({ nullIndex: 4 })],
  ['outs-null-gs', [0x65, 0x6e], segmentRecord({ nullIndex: 5 })],
  ['outs-zero-limit-ds', [0x6f], segmentRecord({ cacheIndex: 3, cacheBits: 0xf2100000 })],
  ['outs-zero-limit-ss', [0x36, 0x6f], segmentRecord({ cacheIndex: 2, cacheBits: 0xf2300000 })],
  ['outs-overflow-fs-base', [0x64, 0x6e], segmentRecord({ baseIndex: 4 })],
  ['outs-overflow-gs-base', [0x65, 0x6e], segmentRecord({ baseIndex: 5 })],
];
for (const [name, form, segments] of noAccess) {
  positive.push(portRow(`${name}/zero-repeat`, [0xf3, ...form], true, true, 0, { segments }));
  positive.push(portRow(`${name}/denied-repeat`, [0xf3, ...form], true, false, 1, { segments }));
  positive.push(portRow(`${name}/denied-single-count-zero`, form, false, false, 0, { segments }));
}
const continuations = [
  {
    ...portRow('nop-before-zero-ins', [0x90, 0xf3, 0x6c], true, true, 0),
    retired: 2,
    work: 2,
    schedules: [
      {},
      { budget: 1, reason: 1, retired: 1, offset: 1, work: 1 },
      { flags: 0x102, reason: 16, version: 7, retired: 1, offset: 1, work: 1 },
      { flags: 0x102, shadow: 1, reason: 16, version: 7, retired: 1, offset: 1, work: 1 },
    ],
  },
  {
    ...portRow('nop-before-denied-outs', [0x90, 0xf3, 0x6e], true, false, 1),
    retired: 1,
    offset: 1,
    work: 1,
    schedules: [{}, { budget: 1, reason: 1 }, { flags: 0x102, reason: 16, version: 7 }],
  },
];
const negatives = [];
for (const [name, form] of forms) {
  for (const prefix of [[0xf2], [0xf3, 0xf3], [0xf2, 0xf3], [0xf3, 0xf2]]) {
    negatives.push({
      name: `${name}/reserved-${Buffer.from(prefix).toString('hex')}`,
      bytes: Buffer.from([...prefix, ...form]),
    });
  }
  const opcode = form[0] === 0x66 ? form.slice(1) : form;
  negatives.push({ name: `${name}/duplicate66`, bytes: Buffer.from([0x66, 0x66, ...opcode]) });
  negatives.push({ name: `${name}/duplicate67`, bytes: Buffer.from([0x67, 0x67, ...form]) });
}
negatives.push({ name: 'valid-unsupported-int80', bytes: Buffer.from([0xcd, 0x80]) });
for (const [prefix, opcode, count] of [
  [[0xf3], 0x6c, 0],
  [[0x66], 0x6d, 0],
  [[0x67], 0x6e, 0],
  [[0xf3, 0x66], 0x6d, 0],
  [[0x67, 0xf3], 0x6e, 0x10000],
]) {
  const repeated = prefix.includes(0xf3);
  negatives.push({
    name: `missing-opcode-${Buffer.from(prefix).toString('hex')}/zero-count`,
    bytes: Buffer.from(prefix),
    registers: { 1: count },
    target: 0x2000 - prefix.length,
    repair: Buffer.from([opcode]),
    repaired: portRow('repaired-fetch', [...prefix, opcode], repeated, repeated, count),
  });
}
runStoppedFaults({
  sourcePaths: [
    'engine/src/abi/x86/segments.rs',
    'engine/src/cpu/x86/segments/cache.rs',
    'engine/tests/cpu/control/exceptions/environment/io/strings/native.rs',
    'engine/tests/cpu/control/exceptions/environment/io/strings/wasm.rs',
    'engine/tests/cpu/control/exceptions/environment/io/strings/fixtures/p2-port-strings/runtime/run.mjs',
  ],
  positive,
  continuations,
  negatives,
  claim:
    'Under explicit DenyAll user-port policy, complete non-REP INS/OUTS always fault GP0; REP skips the attempt only for selected CX/ECX zero, retires once and consumes one work unit without port, segment, pointer or RAM effects. Full fetch, strict APIs, prefix refusals, LOCK UD, debug shadow, TF and initial stops remain distinct. Competing operand faults follow this bounded Ring3 policy rather than a universal hardware-priority claim.',
});
