import assert from 'node:assert/strict';

export const geometries = [
  { id: 1, data: 4, index: 4, l: 0, lanes: 4 },
  { id: 2, data: 4, index: 4, l: 1, lanes: 8 },
  { id: 3, data: 8, index: 4, l: 0, lanes: 2 },
  { id: 4, data: 8, index: 4, l: 1, lanes: 4 },
  { id: 5, data: 4, index: 8, l: 0, lanes: 2 },
  { id: 6, data: 4, index: 8, l: 1, lanes: 4 },
  { id: 7, data: 8, index: 8, l: 0, lanes: 2 },
  { id: 8, data: 8, index: 8, l: 1, lanes: 4 },
];
const wrap = (value) => Number(BigInt.asUintN(32, BigInt(value)));
export const pattern = (register) =>
  Buffer.from(Array.from({ length: 32 }, (_, byte) => (register * 29 + byte * 13 + 7) & 255));
function elements(width, values, poison = 0x91) {
  const result = Buffer.alloc(32, poison);
  values.forEach((value, lane) => {
    if (width === 4) result.writeUInt32LE(wrap(value), lane * 4);
    else result.writeBigUInt64LE(BigInt.asUintN(64, BigInt(value)), lane * 8);
  });
  return result;
}
export function indexValue(bytes, geometry, lane) {
  return geometry.index === 4
    ? BigInt(bytes.readInt32LE(lane * 4))
    : bytes.readBigInt64LE(lane * 8);
}
export function selected(mask, geometry) {
  let bits = 0;
  for (let lane = 0; lane < geometry.lanes; lane++)
    if (mask[(lane + 1) * geometry.data - 1] & 128) bits |= 1 << lane;
  return bits;
}
export function baseOffset(row, registers) {
  return wrap((row.base === null ? 0n : BigInt(registers[row.base])) + BigInt(row.displacement));
}
export function laneOffset(row, base, index, lane) {
  return wrap(BigInt(base) + indexValue(index, row.geometry, lane) * BigInt(row.scale));
}
export function encode(row) {
  const mod =
    row.base === null
      ? 0
      : row.displacement === 0 && row.base !== 5
        ? 0
        : row.displacement >= -128 && row.displacement <= 127 && !row.disp32
          ? 1
          : 2;
  const displacement = Buffer.alloc(mod === 1 ? 1 : row.base === null || mod === 2 ? 4 : 0);
  if (displacement.length === 1) displacement.writeInt8(row.displacement);
  if (displacement.length === 4) displacement.writeUInt32LE(wrap(row.displacement));
  const opcode = (row.floating ? 0x92 : 0x90) + (row.geometry.index === 8 ? 1 : 0);
  return Buffer.concat([
    Buffer.from([
      ...row.prefix,
      0xc4,
      0xe2,
      ((15 - row.mask) << 3) | (row.geometry.l ? 4 : 0) | (row.geometry.data === 8 ? 0x80 : 0) | 1,
      opcode,
      (mod << 6) | (row.destination << 3) | 4,
      (Math.log2(row.scale) << 6) | (row.indexRegister << 3) | (row.base === null ? 5 : row.base),
    ]),
    displacement,
  ]);
}

// independent ISA outcome model: caller supplies a literal mapped-RAM model, never engine results.
export function outcome(row, destination, mask, index, registers, read) {
  const g = row.geometry,
    active = g.lanes * g.data,
    initial = selected(mask, g);
  const value = Buffer.from(destination),
    remaining = Buffer.alloc(32);
  for (let lane = 0; lane < g.lanes; lane++)
    if (initial & (1 << lane)) remaining.fill(255, lane * g.data, (lane + 1) * g.data);
  if (g.data !== g.index) value.fill(0, active);
  const base = initial ? baseOffset(row, registers) : 0;
  let completed = 0;
  for (let lane = 0; lane < g.lanes; lane++) {
    if (!(initial & (1 << lane))) continue;
    const offset = laneOffset(row, base, index, lane);
    const last = BigInt(offset) + BigInt(g.data) - 1n;
    const segment = row.segment;
    if (segment.null || last > 0xffffffffn || last > BigInt(segment.limit))
      return {
        value,
        mask: remaining,
        completed,
        lane,
        status: segment.id === 2 ? 7 : 3,
        detail: 0,
        address: 0,
        access: 0,
        base,
      };
    const linear = wrap(BigInt(segment.base) + BigInt(offset));
    const memory = read(linear, g.data);
    if (memory.fault)
      return {
        value,
        mask: remaining,
        completed,
        lane,
        status: 1,
        detail: memory.detail,
        address: memory.address,
        access: 1,
        base,
      };
    memory.bytes.copy(value, lane * g.data);
    remaining.fill(0, lane * g.data, (lane + 1) * g.data);
    completed |= 1 << lane;
  }
  value.fill(0, active);
  return {
    value,
    mask: remaining,
    completed,
    lane: 0xffffffff,
    status: 0,
    detail: 0,
    address: 0,
    access: 0,
    base,
  };
}
function make(name, geometry, options = {}) {
  const g = geometries[geometry - 1];
  const indexes = Array.from({ length: g.lanes }, (_, lane) => lane * 16);
  const masks = Array.from({ length: g.lanes }, (_, lane) =>
    lane === 0 || lane === g.lanes - 1
      ? g.data === 4
        ? 0x80000001n
        : 0x8000000000000001n
      : g.data === 4
        ? 0x7fffffffn
        : 0x7fffffffffffffffn,
  );
  return {
    name,
    geometry: g,
    floating: false,
    destination: 0,
    indexRegister: 4,
    mask: 2,
    base: 0,
    baseValue: 0x5000,
    displacement: 0,
    scale: 1,
    prefix: [],
    segment: { id: 3, base: 0, limit: 0xffffffff, null: false },
    indexes: elements(g.index, indexes),
    masks: elements(g.data, masks),
    destinationBytes: pattern(0),
    ...options,
  };
}
export const gatherRows = [];
for (const geometry of geometries) {
  for (const floating of [false, true])
    gatherRows.push(
      make(`geometry-${geometry.id}-${floating ? 'float' : 'integer'}-sparse`, geometry.id, {
        floating,
      }),
    );
  gatherRows.push(
    make(`geometry-${geometry.id}-zero-mask-null`, geometry.id, {
      masks: Buffer.alloc(32, 0x7f),
      baseValue: 0xffffffff,
      displacement: 32,
      segment: { id: 3, base: 0, limit: 0, null: true },
    }),
  );
  const indexes = Array.from({ length: geometry.lanes }, (_, lane) =>
    lane === 0 ? 0 : 0x1000 + lane * 16,
  );
  gatherRows.push(
    make(`geometry-${geometry.id}-partial-page-repair-resume`, geometry.id, {
      indexes: elements(geometry.index, indexes),
      masks: elements(geometry.data, Array(geometry.lanes).fill(-1n)),
      repair: { map: 0x6000, changeCleared: true },
    }),
  );
}
gatherRows.push(
  make('negative-index-repairs-out-of-limit-scalar-base', 1, {
    baseValue: 0x2000,
    indexes: elements(4, [-0x1000, 0, 0, 0]),
    masks: elements(4, [-1, 0, 0, 0]),
    segment: { id: 3, base: 0, limit: 0x1fff, null: false },
  }),
);
gatherRows.push(
  make('base-plus-displacement-wrap', 1, {
    baseValue: 0xfffffff0,
    displacement: 32,
    indexes: elements(4, [0, 0, 0, 0]),
    masks: elements(4, [-1, 0, 0, 0]),
  }),
);
gatherRows.push(
  make('qword-high-index-bits-discard-before-scale', 7, {
    base: null,
    scale: 4,
    indexes: elements(8, [0x100000400n, 0]),
    masks: elements(8, [-1, 0]),
  }),
);
for (const scale of [1, 2, 4, 8])
  gatherRows.push(
    make(`vsib-index4-scale-${scale}-negative-disp8`, 1, {
      base: 3,
      baseValue: 0x5080,
      displacement: -16,
      scale,
      indexes: elements(4, [-4, 0, 4, 8]),
      masks: elements(4, [-1, -1, -1, -1]),
    }),
  );
gatherRows.push(
  make('vsib-no-base-disp32', 5, {
    base: null,
    displacement: 0x5000,
    indexes: elements(8, [0, 16]),
    masks: elements(4, [-1, -1]),
  }),
);
for (const geometry of [1, 7]) {
  const g = geometries[geometry - 1],
    start = 0x6000 - g.data + 1;
  gatherRows.push(
    make(`split-element-${g.data}-is-atomic`, geometry, {
      indexes: elements(g.index, [0, start - 0x5000, 0, 0].slice(0, g.lanes)),
      masks: elements(g.data, [-1, -1, 0, 0].slice(0, g.lanes)),
      repair: { map: 0x6000, changeCleared: true },
    }),
  );
}
gatherRows.push(
  make('lower-page-fault-wins-over-higher-segment-limit', 1, {
    indexes: elements(4, [0x1000, 0x3000, 0, 0]),
    masks: elements(4, [-1, -1, 0, 0]),
    segment: { id: 3, base: 0, limit: 0x6fff, null: false },
  }),
);
gatherRows.push(
  make('partial-ss-limit-repair-resume', 1, {
    base: 4,
    indexes: elements(4, [0, 16, 32, 48]),
    masks: elements(4, [-1, -1, 0, 0]),
    segment: { id: 2, base: 0, limit: 0x500f, null: false },
    repair: { limit: 0xffffffff, changeCleared: true },
  }),
);
gatherRows.push(
  make('zero-mask-ss-zero-limit', 1, {
    base: 4,
    baseValue: 0xffffffff,
    masks: Buffer.alloc(32, 0x7f),
    segment: { id: 2, base: 0, limit: 0, null: false },
  }),
);
gatherRows.push(
  make('fs-base-wrap-after-logical-lane', 1, {
    prefix: [0x64],
    indexes: elements(4, [0, 0, 0, 0]),
    masks: elements(4, [-1, 0, 0, 0]),
    segment: { id: 4, base: 0xffffb000, limit: 0xffffffff, null: false },
  }),
);
gatherRows.push(
  make('selected-full-logical-span-overflow', 7, {
    baseValue: 0xfffffffc,
    indexes: elements(8, [0, 0]),
    masks: elements(8, [-1, 0]),
  }),
);
gatherRows.push(
  make('fs-linear-span-overflow-after-valid-logical-span', 7, {
    prefix: [0x64],
    baseValue: 0xffffeffc,
    indexes: elements(8, [0, 0]),
    masks: elements(8, [-1, 0]),
    segment: { id: 4, base: 0x1000, limit: 0xffffffff, null: false },
  }),
);
gatherRows.push(
  make('partial-read-permission-fault', 1, {
    indexes: elements(4, [0, 0x3000, 0, 0]),
    masks: elements(4, [-1, -1, 0, 0]),
    deniedPage: 0x8000,
    repair: { permission: 0x8000, changeCleared: true },
  }),
);
gatherRows.push(
  make('readable-code-cache-is-valid-data-source', 1, {
    segment: { id: 3, base: 0, limit: 0xffffffff, null: false, code: true },
  }),
);
for (const [name, geometry, options] of [
  ['tf-success-with-consumed-shadow', 1, { flags: 0x244dd7, shadow: true }],
  ['tf-zero-mask-still-retires-once', 5, { flags: 0x244dd7, masks: Buffer.alloc(32) }],
  [
    'tf-partial-fault-before-retirement',
    1,
    {
      flags: 0x244dd7,
      shadow: true,
      indexes: elements(4, [0, 0x1000, 0, 0]),
      masks: elements(4, [-1, -1, 0, 0]),
    },
  ],
  ['mixed-rep-success-charges-one-work-unit', 1, { mixed: true }],
  [
    'mixed-rep-partial-fault-charges-zero',
    1,
    { mixed: true, indexes: elements(4, [0, 0x1000, 0, 0]), masks: elements(4, [-1, -1, 0, 0]) },
  ],
])
  gatherRows.push(make(name, geometry, options));

gatherRows.push(
  make('live-base-written-before-gather', 1, { baseValue: 0x8000, priorBase: 0x5000 }),
);

gatherRows.push(
  make('gather-then-ordinary-avx-memory-reuses-transfer-pointer', 1, { continuation: true }),
);

// hand-derived geometry literal: only lane0 and last load; poisoned inactive lanes merge.
for (const g of geometries) {
  const row = make(`literal-${g.id}`, g.id),
    destination = Buffer.alloc(32, 0x91);
  const value = outcome(row, destination, row.masks, row.indexes, [0x5000], (_address, width) => ({
    bytes: Buffer.alloc(width, 0x3c),
  }));
  const expected = Buffer.alloc(32);
  expected.fill(0x91, 0, g.lanes * g.data);
  expected.fill(0x3c, 0, g.data);
  expected.fill(0x3c, (g.lanes - 1) * g.data, g.lanes * g.data);
  assert.deepEqual(value.value, expected);
  assert.deepEqual(value.mask, Buffer.alloc(32));
  assert.equal(value.completed, 1 | (1 << (g.lanes - 1)));
}
for (const g of geometries) {
  const row = gatherRows.find((row) => row.name === `geometry-${g.id}-partial-page-repair-resume`);
  const old = Buffer.alloc(32, 0x91),
    widths = [];
  const fault = outcome(row, old, row.masks, row.indexes, [0x5000], (address, width) => {
    widths.push(width);
    return address === 0x5000
      ? { bytes: Buffer.alloc(width, 0x3c) }
      : { fault: true, detail: 1, address };
  });
  assert.deepEqual(widths, [g.data, g.data]);
  assert.equal(fault.completed, 1);
  assert.equal(fault.lane, 1);
  assert.deepEqual(fault.value.subarray(0, g.data), Buffer.alloc(g.data, 0x3c));
  assert.deepEqual(fault.mask.subarray(0, g.data), Buffer.alloc(g.data));
  assert.deepEqual(
    fault.mask.subarray(g.data, g.lanes * g.data),
    Buffer.alloc((g.lanes - 1) * g.data, 255),
  );
  assert.deepEqual(fault.mask.subarray(g.lanes * g.data), Buffer.alloc(32 - g.lanes * g.data));
  assert.deepEqual(
    fault.value.subarray(g.lanes * g.data),
    Buffer.alloc(32 - g.lanes * g.data, g.data === g.index ? 0x91 : 0),
  );
}
assert.equal(new Set(gatherRows.map((row) => row.name)).size, gatherRows.length);
assert.equal(
  new Set(
    gatherRows
      .slice(0, 32)
      .filter((row) => row.name.includes('sparse'))
      .map((row) => encode(row).toString('hex').slice(4, 8)),
  ).size,
  16,
);
assert.equal(laneOffset(make('wrap', 7, { scale: 4 }), 0, elements(8, [0x100000400n]), 0), 0x1000);
assert.equal(baseOffset(make('base', 1, { displacement: 32 }), [0xfffffff0]), 16);
export const gatherSourcePaths = [
  'engine/tests/cpu/simd/avx/gather/wasm.rs',
  'engine/tests/cpu/simd/avx/gather/fixtures/p2-avx2-gather/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/gather/fixtures/p2-avx2-gather/runtime/run.mjs',
  'engine/src/cpu/x86/avx/gather/mod.rs',
  'engine/src/cpu/x86/avx/operation/factory/gather.rs',
  'engine/src/cpu/x86/decode/avx/gather.rs',
  'engine/src/abi/x86/avx/gather.rs',
  'engine/src/process/cpu/avx/gather.rs',
  'engine/src/cpu/dbt/wasm/avx/gather.rs',
];
