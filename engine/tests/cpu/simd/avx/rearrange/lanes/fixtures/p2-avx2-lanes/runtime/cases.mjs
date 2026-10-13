import assert from 'node:assert/strict';

const forms = [
  { opcode: 0x60, map: 1, pp: 1, kind: 1 },
  { opcode: 0x61, map: 1, pp: 1, kind: 2 },
  { opcode: 0x62, map: 1, pp: 1, kind: 3 },
  { opcode: 0x63, map: 1, pp: 1, kind: 4 },
  { opcode: 0x67, map: 1, pp: 1, kind: 5 },
  { opcode: 0x68, map: 1, pp: 1, kind: 6 },
  { opcode: 0x69, map: 1, pp: 1, kind: 7 },
  { opcode: 0x6a, map: 1, pp: 1, kind: 8 },
  { opcode: 0x6b, map: 1, pp: 1, kind: 9 },
  { opcode: 0x6c, map: 1, pp: 1, kind: 10 },
  { opcode: 0x6d, map: 1, pp: 1, kind: 11 },
  { opcode: 0x70, map: 1, pp: 1, kind: 12 },
  { opcode: 0x70, map: 1, pp: 2, kind: 13 },
  { opcode: 0x70, map: 1, pp: 3, kind: 14 },
  { opcode: 0x00, map: 2, pp: 1, kind: 15 },
  { opcode: 0x2b, map: 2, pp: 1, kind: 22 },
  { opcode: 0x0e, map: 3, pp: 1, kind: 29 },
  { opcode: 0x0f, map: 3, pp: 1, kind: 30 },
  { opcode: 0x4c, map: 3, pp: 1, kind: 31 },
];

const unary = form => form.kind >= 12 && form.kind <= 14;
const immediate = form => unary(form) || form.kind === 29 || form.kind === 30;
const controlFor = form => form.kind === 29 ? 0x96 : form.kind === 30 ? 19 : immediate(form) ? 0x1b : 0;
const raw = Buffer.from('0080ff7f0100ffff80ff7f000001ff00807fff0100fe817e02fd827d12ed23dc', 'hex');

function vector(register) {
  return Buffer.from(raw.map(byte => byte ^ (register * 37)));
}

function encode(form, roles, control = controlFor(form), options = {}) {
  const [destination, first, second, mask] = roles;
  const { w = false, prefix = [], memory = false, address16 = null, stack = false } = options;
  const vvvv = unary(form) ? 15 : 15 - first;
  let operand = [0xc0 + destination * 8 + second];
  if (memory) {
    operand = stack
      ? [destination * 8 + 4, 0x24]
      : address16 === null
        ? [destination * 8]
        : [destination * 8 + 6, address16 & 255, address16 >>> 8];
  }
  return Buffer.from([
    ...prefix,
    ...(address16 === null ? [] : [0x67]),
    0xc4, 0xe0 + form.map, vvvv * 8 + 4 + form.pp + (w ? 0x80 : 0), form.opcode,
    ...operand,
    ...(form.kind === 31 ? [mask * 16 | control] : immediate(form) ? [control] : []),
  ]).toString('hex');
}

function evaluate(form, a, b, mask, control) {
  const output = Buffer.alloc(32);
  for (const half of [0, 16]) {
    const first = a.subarray(half, half + 16);
    const second = b.subarray(half, half + 16);
    const result = output.subarray(half, half + 16);
    if ([1, 2, 3, 6, 7, 8, 10, 11].includes(form.kind)) {
      const width = { 1: 1, 2: 2, 3: 4, 6: 1, 7: 2, 8: 4, 10: 8, 11: 8 }[form.kind];
      const start = [6, 7, 8, 11].includes(form.kind) ? 8 : 0;
      const pieces = [];
      for (let lane = 0; lane < 8 / width; lane++) {
        pieces.push(first.subarray(start + lane * width, start + (lane + 1) * width));
        pieces.push(second.subarray(start + lane * width, start + (lane + 1) * width));
      }
      Buffer.concat(pieces).copy(result);
    } else if ([4, 5, 9, 22].includes(form.kind)) {
      const width = form.kind === 4 || form.kind === 5 ? 1 : 2;
      const unsigned = form.kind === 5 || form.kind === 22;
      const bits = BigInt(width * 8);
      const min = unsigned ? 0n : -(1n << (bits - 1n));
      const max = unsigned ? (1n << bits) - 1n : (1n << (bits - 1n)) - 1n;
      let target = 0;
      for (const source of [first, second]) {
        for (let byte = 0; byte < 16; byte += width * 2) {
          const signed = BigInt(width === 1 ? source.readInt16LE(byte) : source.readInt32LE(byte));
          let value = BigInt.asUintN(Number(bits), signed < min ? min : signed > max ? max : signed);
          for (let i = 0; i < width; i++) {
            result[target++] = Number(value & 255n);
            value >>= 8n;
          }
        }
      }
    } else if (form.kind === 12 || form.kind === 13 || form.kind === 14) {
      second.copy(result);
      const width = form.kind === 12 ? 4 : 2;
      const start = form.kind === 13 ? 8 : 0;
      for (let lane = 0; lane < 4; lane++) {
        const index = start + (control >> (lane * 2) & 3) * width;
        second.copy(result, start + lane * width, index, index + width);
      }
    } else if (form.kind === 15) {
      for (let lane = 0; lane < 16; lane++) {
        result[lane] = second[lane] & 0x80 ? 0 : first[second[lane] & 15];
      }
    } else if (form.kind === 29) {
      for (let lane = 0; lane < 8; lane++) {
        const source = control & (1 << lane) ? second : first;
        source.copy(result, lane * 2, lane * 2, lane * 2 + 2);
      }
    } else if (form.kind === 30) {
      if (control < 32) Buffer.concat([second, first, Buffer.alloc(16)]).copy(result, 0, control, control + 16);
    } else if (form.kind === 31) {
      for (let lane = 0; lane < 16; lane++) {
        result[lane] = mask[half + lane] & 0x80 ? second[lane] : first[lane];
      }
    } else {
      assert.fail(`unknown primary lane-local kind ${form.kind}`);
    }
  }
  return output;
}

function sources(vectors) {
  return {
    lower: Object.fromEntries(vectors.map((value, register) => [register, value.subarray(0, 16)])),
    upper: Object.fromEntries(vectors.map((value, register) => [register, value.subarray(16)])),
  };
}

function registerRow(form, roles, control = controlFor(form), options = {}) {
  const vectors = Array.from({ length: 8 }, (_, register) => vector(register));
  const [destination, first, second, mask] = roles;
  const expected = evaluate(form, vectors[first], vectors[second], vectors[mask], control);
  return {
    name: `lane-${form.kind}-roles${roles.join('/')}-control${control}-w${Number(Boolean(options.w))}`,
    code: encode(form, roles, control, options),
    ...sources(vectors),
    outLower: { [destination]: expected.subarray(0, 16) },
    outUpper: { [destination]: expected.subarray(16) },
  };
}

export const laneRows = [];
const aliases = [
  [0, 1, 2, 3], [1, 1, 2, 3], [2, 1, 2, 3], [3, 1, 2, 3],
  [0, 1, 1, 3], [0, 1, 2, 1], [0, 1, 2, 2], [1, 1, 1, 1],
];
for (const form of forms) {
  for (const roles of aliases) {
    if (form.kind === 31) {
      for (const ignored of [0, 1, 15, 128, 143]) laneRows.push(registerRow(form, roles, ignored));
    } else {
      for (const w of [false, true]) laneRows.push(registerRow(form, roles, controlFor(form), { w }));
    }
  }
  if (immediate(form)) {
    for (const control of [0, 1, 15, 16, 31, 32, 128, 255]) {
      const row = registerRow(form, [7, 6, 7, 5], control);
      row.name += '-immediate-boundary';
      laneRows.push(row);
    }
  }
}

function memoryRow(form, address, options = {}) {
  const roles = [0, 0, 0, 1];
  const input = vector(2);
  const vectors = Array.from({ length: 8 }, (_, register) => vector(register));
  const expected = evaluate(form, vectors[0], input, vectors[1], controlFor(form));
  return {
    name: `lane-${form.kind}-memory-${address.toString(16)}`,
    code: encode(form, roles, controlFor(form), { memory: true }),
    address,
    read: 32,
    memory: input,
    ...sources(vectors),
    outLower: { 0: expected.subarray(0, 16) },
    outUpper: { 0: expected.subarray(16) },
    ...options,
  };
}

for (const form of forms) {
  for (const address of [0x5011, 0x5fe0]) laneRows.push(memoryRow(form, address));
  laneRows.push(memoryRow(form, 0x5013, {
    name: `lane-${form.kind}-address16`,
    code: encode(form, [0, 0, 0, 1], controlFor(form), { memory: true, address16: 0x5013 }),
  }));
  laneRows.push(memoryRow(form, 0x5015, {
    name: `lane-${form.kind}-fs`,
    code: encode(form, [0, 0, 0, 1], controlFor(form), { memory: true, prefix: [0x64] }),
  }));
  laneRows.push(memoryRow(form, 0x5017, {
    name: `lane-${form.kind}-default-ss`,
    code: encode(form, [0, 0, 0, 1], controlFor(form), { memory: true, stack: true }),
    r: { 4: 0x5017 },
  }));
  for (const address of [0x5fe1, 0x5ff0]) {
    const row = memoryRow(form, address, { reason: 5, detail: 1, fault: 0x6000, access: 1, length: 32 });
    delete row.memory;
    laneRows.push(row);
  }
  for (const [segment, prefix, reason] of [['ds', [], 11], ['ss', [0x36], 15]]) {
    const row = memoryRow(form, 0xfffffff0, {
      name: `lane-${form.kind}-${segment}-span-overflow`,
      code: encode(form, [0, 0, 0, 1], controlFor(form), { memory: true, prefix }),
      helpers: 0,
      reason,
    });
    delete row.memory;
    laneRows.push(row);
  }
}

for (const kind of [15, 29, 30]) {
  const form = forms.find(form => form.kind === kind);
  const control = kind === 30 ? 255 : 0;
  const input = kind === 15 ? Buffer.alloc(32, 0x80) : vector(2);
  const expected = evaluate(form, vector(0), input, vector(1), control);
  for (const address of [0x5fe0, 0x5ff0]) {
    const row = memoryRow(form, address, {
      name: `lane-${kind}-zero-selection-still-reads32-${address.toString(16)}`,
      code: encode(form, [0, 0, 0, 1], control, { memory: true }),
      memory: input,
      outLower: { 0: expected.subarray(0, 16) },
      outUpper: { 0: expected.subarray(16) },
    });
    if (address === 0x5ff0) {
      delete row.memory;
      Object.assign(row, { reason: 5, detail: 1, fault: 0x6000, access: 1, length: 32 });
    }
    laneRows.push(row);
  }
}

function literal(kind, a, b, mask, control, expected) {
  const form = forms.find(form => form.kind === kind);
  assert.deepEqual(evaluate(form, a, b, mask, control), expected, `independent literal kind${kind}`);
  laneRows.push({
    name: `lane-${kind}-independent-literal`,
    code: encode(form, [0, 1, 2, 3], control),
    lower: { 1: a.subarray(0, 16), 2: b.subarray(0, 16), 3: mask.subarray(0, 16) },
    upper: { 1: a.subarray(16), 2: b.subarray(16), 3: mask.subarray(16) },
    outLower: { 0: expected.subarray(0, 16) },
    outUpper: { 0: expected.subarray(16) },
  });
}

const ascending = Buffer.from(Array.from({ length: 32 }, (_, i) => i));
const next = Buffer.from(Array.from({ length: 32 }, (_, i) => i + 64));
const zero = Buffer.alloc(32);
literal(1, ascending, next, zero, 0,
  Buffer.from('0040014102420343044405450646074710501151125213531454155516561757', 'hex'));
literal(30, ascending, next, zero, 17,
  Buffer.from('0102030405060708090a0b0c0d0e0f001112131415161718191a1b1c1d1e1f00', 'hex'));
literal(15, ascending, Buffer.from('10118f7f'.repeat(8), 'hex'), zero, 0,
  Buffer.from('0001000f'.repeat(4) + '1011001f'.repeat(4), 'hex'));
literal(29, ascending, next, zero, 1,
  Buffer.from('404102030405060708090a0b0c0d0e0f505112131415161718191a1b1c1d1e1f', 'hex'));
literal(31, ascending, next, Buffer.from('80007fff'.repeat(8), 'hex'), 0,
  Buffer.from('400102434405064748090a4b4c0d0e4f501112535415165758191a5b5c1d1e5f', 'hex'));
const wordBytes = values => {
  const output = Buffer.alloc(values.length * 2);
  values.forEach((value, index) => output.writeInt16LE(value, index * 2));
  return output;
};
literal(4,
  wordBytes([-129, -128, -1, 0, 1, 127, 128, 255, -32768, 32767, -1, 0, 1, 2, 3, 4]),
  wordBytes([32767, -32768, 42, -42, 128, -129, 126, -127, 127, 128, -128, -129, 5, 6, 7, 8]),
  zero, 0, Buffer.from('8080ff00017f7f7f7f802ad67f807e81807fff00010203047f7f808005060708', 'hex'));

assert.equal(laneRows.length, 551);
assert.equal(new Set(laneRows.map(row => row.name)).size, laneRows.length);
assert.equal(laneRows.filter(row => row.reason).length, 79);
export const laneSourcePaths = [
  'engine/src/cpu/x86/avx/rearrange.rs',
  'engine/src/cpu/x86/avx/operation/factory/rearrange.rs',
  'engine/src/cpu/x86/decode/avx/rearrange.rs',
  'engine/src/cpu/x86/simd/rearrange.rs',
  'engine/src/cpu/x86/simd/packed.rs',
  'engine/tests/cpu/simd/avx/rearrange/lanes/native.rs',
  'engine/tests/cpu/simd/avx/execution/wasm.rs',
  'engine/tests/cpu/simd/avx/rearrange/lanes/fixtures/p2-avx2-lanes/runtime/cases.mjs',
];
