import assert from 'node:assert/strict';

const raw = Buffer.from(
  '010000000000f07f0100807f0000008001000000000000004523c17f0000803f',
  'hex',
);
assert.equal(raw.length, 32);
const forms = [
  { name: 'dword-load128', width: 16, lane: 4, store: false, kind: 2 },
  { name: 'dword-load256', width: 32, lane: 4, store: false, kind: 3 },
  { name: 'qword-load128', width: 16, lane: 8, store: false, kind: 4 },
  { name: 'qword-load256', width: 32, lane: 8, store: false, kind: 5 },
  { name: 'dword-store128', width: 16, lane: 4, store: true, kind: 6 },
  { name: 'dword-store256', width: 32, lane: 4, store: true, kind: 7 },
  { name: 'qword-store128', width: 16, lane: 8, store: true, kind: 8 },
  { name: 'qword-store256', width: 32, lane: 8, store: true, kind: 9 },
];

function segments({ segment = 3, limit = null, readonly = false, nullData = false, fsBase = 0 } = {}) {
  const bytes = Buffer.alloc(64);
  bytes.write('R3SG');
  bytes.writeUInt16LE(2, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(64, 8);
  const selectors = [0x23, 0x1b, 0x23, nullData ? 0 : 0x23, 0x3b, 0];
  selectors.forEach((selector, index) => bytes.writeUInt16LE(selector, 16 + index * 2));
  const caches = [0xf33fffff, 0xf3bfffff, 0xf33fffff, nullData ? 0 : 0xf33fffff, 0xf33fffff, 0];
  if (limit !== null) caches[segment] = (readonly ? 0xf2100000 : 0xf2300000) + limit;
  [28, 32, 36, 40, 44, 56].forEach((offset, index) => bytes.writeUInt32LE(caches[index], offset));
  bytes.writeUInt32LE(fsBase >>> 0, 48);
  return bytes;
}

function encode(form, data, mask, { prefix = [], address16 = false, stack = false } = {}) {
  const control = (form.lane === 8 ? 0x80 : 0) | ((15 - mask) << 3) | (form.width === 32 ? 4 : 0) | 1;
  const address = address16 ? [6, 5, 0x50] : stack ? [4, 0x24] : [0];
  address[0] |= data << 3;
  return Buffer.from([
    ...prefix,
    ...(address16 ? [0x67] : []),
    0xc4, 0xe2, control, form.store ? 0x8e : 0x8c,
    ...address,
  ]).toString('hex');
}

function row(form, name, active, options = {}) {
  const {
    address = 0x5003,
    offset = address,
    data = 1,
    mask = 3,
    segment = 3,
    memoryLength = form.width,
    reason,
    context = {},
    prefix = [],
    address16 = false,
    stack = false,
    r,
  } = options;
  const vectors = Array.from({ length: 8 }, (_, index) => Buffer.from(raw.map(byte => byte ^ (index * 0x11))));
  const maskValue = Buffer.alloc(32, 0x7f);
  for (let lane = 0; lane < form.width / form.lane; lane++) {
    maskValue[(lane + 1) * form.lane - 1] = active.includes(lane) ? 0x80 : 0x7f;
  }
  vectors[mask] = maskValue;
  const value = Buffer.alloc(32);
  const stores = [];
  for (const lane of active) {
    const at = lane * form.lane;
    if (form.store) {
      stores.push({ at, value: vectors[data].subarray(at, at + form.lane) });
    } else {
      raw.copy(value, at, at, at + form.lane);
    }
  }
  const operands = mask << 9 | (form.store ? data << 6 | 1 << 13 : data | 1 << 12);
  const result = {
    name: `avx2-mask-${form.name}-${name}`,
    code: encode(form, data, mask, { prefix, address16, stack }),
    address,
    r,
    segments: segments({ segment, ...context }),
    mxcsr: 0xffff,
    lower: Object.fromEntries(vectors.map((value, index) => [index, value.subarray(0, 16)])),
    upper: Object.fromEntries(vectors.map((value, index) => [index, value.subarray(16)])),
    selected: {
      length: form.width,
      access: form.store ? 2 : 1,
      descriptor: 0x01000d00 | form.kind,
      operands,
      segment,
      offset: active.length ? offset >>> 0 : 0,
      stores,
    },
  };
  if (memoryLength) result.memory = raw.subarray(0, memoryLength);
  if (reason) {
    result.reason = reason;
    if (reason === 5) {
      result.detail = 1;
      result.fault = 0x6000;
      result.access = form.store ? 2 : 1;
      result.length = form.width;
    }
  } else if (!form.store) {
    result.outLower = { [data]: value.subarray(0, 16) };
    result.outUpper = { [data]: value.subarray(16) };
  }
  return result;
}

export const maskedRows = [];
for (const form of forms) {
  const lanes = Array.from({ length: form.width / form.lane }, (_, index) => index);
  for (const lane of lanes) maskedRows.push(row(form, `only-lane${lane}`, [lane]));
  for (const [name, active] of [
    ['zero-nonzero-low-mask-bits', []],
    ['full', lanes],
    ['even', lanes.filter(lane => lane % 2 === 0)],
    ['odd', lanes.filter(lane => lane % 2 !== 0)],
  ]) {
    maskedRows.push(row(form, name, active));
  }
  for (const [name, active] of [['zero', []], ['full', lanes], ['even', lanes.filter(lane => lane % 2 === 0)]]) {
    maskedRows.push(row(form, `data-equals-mask-${name}`, active, { data: 3 }));
  }
  maskedRows.push(row(form, 'only-first-lane-at-page-end', [0], {
    address: 0x6000 - form.lane,
    memoryLength: form.lane,
  }));
  maskedRows.push(row(form, 'only-highest-lane-at-page-end', [lanes.at(-1)], {
    address: 0x6000 - form.width,
  }));
  maskedRows.push(row(form, 'late-selected-lane-fault-is-atomic', [0, 1], {
    address: 0x6000 - form.lane,
    memoryLength: form.lane,
    reason: 5,
  }));
  for (const [segment, prefix, reason] of [[3, [], 11], [2, [0x36], 15]]) {
    const context = { limit: 0x5000 + form.lane - 1 };
    maskedRows.push(row(form, `segment${segment}-only-authorized-lane`, [0], {
      address: 0x5000, segment, prefix, context,
    }));
    maskedRows.push(row(form, `segment${segment}-selected-denied-lane-before-pages`, [0, 1], {
      address: 0x5000, segment, prefix, context, reason,
    }));
    maskedRows.push(row(form, `segment${segment}-cached-denial-precedes-unmapped-lane`, [0, 1], {
      address: 0x6000 - form.lane,
      segment, prefix, context: { limit: 0x5fff },
      memoryLength: form.lane, reason,
    }));
    maskedRows.push(row(form, `segment${segment}-zero-mask-overflow`, [], {
      address: 0xffffffff, segment, prefix, memoryLength: 0,
    }));
    maskedRows.push(row(form, `segment${segment}-selected-overflow`, [0], {
      address: 0xffffffff, segment, prefix, memoryLength: 0, reason,
    }));
  }
  for (const [name, active, reason] of [['zero', [], undefined], ['selected', [0], 11]]) {
    maskedRows.push(row(form, `null-ds-${name}`, active, {
      address: 0xffffffff, memoryLength: 0, context: { nullData: true }, reason,
    }));
  }
  maskedRows.push(row(form, 'address16-displacement', [0], { address: 0x5005, address16: true }));
  maskedRows.push(row(form, 'fs-wrapped-linear-base', [0], {
    address: 0x5003, offset: 0x6003, segment: 4, prefix: [0x64],
    r: { 0: 0x6003 }, context: { fsBase: 0xfffff000 },
  }));
  maskedRows.push(row(form, 'esp-default-selects-ss', [0], {
    segment: 2, stack: true, r: { 4: 0x5003 },
  }));
  maskedRows.push(row(form, 'readonly-cache-authority', [0], {
    context: { limit: 0x5fff, readonly: true }, reason: form.store ? 11 : undefined,
  }));
}

for (const form of forms.filter(form => form.store)) {
  const smc = row(form, 'selected-code-store-publishes-before-invalidation', [0], {
    address: 0x1000, memoryLength: 0,
  });
  smc.selected.invalidates = true;
  smc.selected.exitReason = 6;
  maskedRows.push(smc);
}

assert.equal(maskedRows.length, 248);
assert.equal(new Set(maskedRows.map(row => row.name)).size, maskedRows.length);
assert.equal(maskedRows.filter(row => row.reason).length, 68);
for (const row of maskedRows) {
  assert.equal(row.selected.stores.some(store => store.value.length !== 4 && store.value.length !== 8), false);
  if (row.memory) assert.ok(row.address >= 0x5000 && row.address + row.memory.length <= 0x6000, row.name);
}
export const maskedSourcePaths = [
  'engine/src/cpu/x86/decode/avx/masked.rs',
  'engine/src/cpu/x86/avx/operation/factory/masked.rs',
  'engine/src/cpu/x86/avx/masked.rs',
  'engine/src/abi/x86/avx.rs',
  'engine/src/abi/x86/segments.rs',
  'engine/src/cpu/x86/segments/cache.rs',
  'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/tests/cpu/simd/avx/execution/wasm.rs',
  'engine/tests/cpu/simd/avx/masked/fixtures/p2-avx2-masked/runtime/cases.mjs',
];
