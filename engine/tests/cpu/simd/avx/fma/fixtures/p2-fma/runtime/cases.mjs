import assert from 'node:assert/strict';

export function lanes(values, double = false) {
  const bytes = Buffer.alloc(values.length * (double ? 8 : 4));
  values.forEach((value, index) => {
    if (double) bytes.writeBigUInt64LE(BigInt(value), index * 8);
    else bytes.writeUInt32LE(Number(value) >>> 0, index * 4);
  });
  return bytes;
}

// Small integers are exact; their binary encoding uses no host floating arithmetic.
function integerBits(value, double) {
  const signed = BigInt(value);
  const magnitude = signed < 0n ? -signed : signed;
  if (magnitude === 0n) return 0n;
  const precision = double ? 52n : 23n;
  const exponent = BigInt(magnitude.toString(2).length - 1);
  return (
    (signed < 0n ? 1n << (double ? 63n : 31n) : 0n) |
    ((exponent + (double ? 1023n : 127n)) << precision) |
    ((magnitude - (1n << exponent)) << (precision - exponent))
  );
}

export function memoryBytes(row) {
  return row.scalar ? (row.double ? 8 : 4) : row.width;
}

function shape(row) {
  if (row.scalar) return row.double ? 5 : 4;
  return (row.double ? 2 : 0) + Number(row.width === 32);
}

export function descriptor(row) {
  const ordinal =
    row.kind < 4
      ? row.kind * 18 + row.order * 6 + shape(row)
      : 72 + (row.kind - 4) * 12 + row.order * 4 + shape(row);
  return 0x01001301 + ordinal;
}

export function encoding(row) {
  const opcode =
    [0x90, 0xa0, 0xb0][row.order] + [8, 10, 12, 14, 6, 7][row.kind] + Number(Boolean(row.scalar));
  const control =
    ((~row.second & 15) << 3) | 1 | (row.double ? 0x80 : 0) | (row.width === 32 || row.lig ? 4 : 0);
  const modrm = (row.memory ? 0 : 0xc0 | row.third) | (row.destination << 3);
  return Buffer.from([...(row.prefix ?? []), 0xc4, 0xe2, control, opcode, modrm]);
}

const shapes = [
  { scalar: false, double: false, width: 16 },
  { scalar: false, double: false, width: 32 },
  { scalar: false, double: true, width: 16 },
  { scalar: false, double: true, width: 32 },
  { scalar: true, double: false, width: 16 },
  { scalar: true, double: true, width: 16 },
];

function repeated(value, double) {
  return lanes(Array(double ? 4 : 8).fill(value), double);
}

function markInactive(row) {
  if (!row.scalar) return;
  const bytes = row.double ? 8 : 4;
  const signaling = lanes([row.double ? 0x7ff0000000000011n : 0x7f800011n], row.double);
  for (let at = bytes; at < 16; at += bytes) {
    signaling.copy(row.vectors[row.destination], at);
  }
}

function normalRow(kind, order, form, memory = false, aliases = [0, 1, 2]) {
  const [destination, second, third] = aliases;
  const integer = (register, lane) =>
    [2, 3, 5, 7, 11, 13, 17, 19][register] + lane * (register + 1);
  const vectors = Object.fromEntries(
    Array.from({ length: 8 }, (_, register) => [
      register,
      lanes(
        Array.from({ length: form.double ? 4 : 8 }, (_, lane) =>
          integerBits(integer(register, lane), form.double),
        ),
        form.double,
      ),
    ]),
  );
  const count = form.scalar ? 1 : form.width / (form.double ? 8 : 4);
  const result = lanes(
    Array.from({ length: count }, (_, lane) => {
      const d = integer(destination, lane);
      const a = integer(second, lane);
      const b = integer(memory ? 2 : third, lane);
      const [product, addend] = order === 0 ? [d * b, a] : order === 1 ? [a * d, b] : [a * b, d];
      const negativeProduct = kind === 2 || kind === 3;
      const negativeAddend =
        kind === 1 ||
        kind === 3 ||
        (kind === 4 && lane % 2 === 0) ||
        (kind === 5 && lane % 2 !== 0);
      const value = (negativeProduct ? -product : product) + (negativeAddend ? -addend : addend);
      return integerBits(value, form.double);
    }),
    form.double,
  );
  const row = {
    name: `family-${kind}-${order}-${shape(form)}-${memory}-${aliases.join('-')}`,
    group: 'family',
    kind,
    order,
    ...form,
    memory,
    destination,
    second,
    third,
    vectors,
    result,
    payload: Buffer.from(vectors[2].subarray(0, memoryBytes(form))),
  };
  markInactive(row);
  return row;
}

// These are mathematical x*y+z inputs; syntax order is applied independently below.
function literalRow(form, memory, order, literal) {
  const [name, x, y, z, result, mxcsr, after, reason] = literal;
  const row = normalRow(0, order, form, memory);
  const [old, second, third] = order === 0 ? [x, z, y] : order === 1 ? [y, x, z] : [z, x, y];
  row.name = `${name}-${shape(form)}-${memory}-${order}`;
  row.group = 'numeric';
  row.vectors = {
    0: repeated(old, form.double),
    1: repeated(second, form.double),
    2: repeated(third, form.double),
  };
  row.payload = repeated(third, form.double).subarray(0, memoryBytes(form));
  row.result = lanes(
    Array(form.scalar ? 1 : memoryBytes(form) / (form.double ? 8 : 4)).fill(result),
    form.double,
  );
  Object.assign(row, { mxcsr, after, reason, detail: reason ? after : 0 });
  markInactive(row);
  return row;
}

// Literal powers-of-two and IEEE bit patterns, independent of the Rust fused adapter.
const single = [
  ['one-round-cancellation', 0x3f800001n, 0x3f7ffffen, 0xbf800000n, 0xa8800000n, 0x1f80, 0x1f80],
  [
    'qNaN-suppresses-zero-times-infinity',
    0n,
    0x7f800000n,
    0x7fc00044n,
    0x7fc00044n,
    0x1f80,
    0x1f80,
  ],
  ['masked-invalid', 0x7f800001n, 0x3f800000n, 0n, 0x7fc00001n, 0x1f80, 0x1f81],
  ['unmasked-invalid', 0x7f800001n, 0x3f800000n, 0n, 0n, 0x1f00, 0x1f01, 13],
  ['already-sticky-invalid-still-traps', 0x7f800001n, 0x3f800000n, 0n, 0n, 0x1f01, 0x1f01, 13],
  ['exact-tiny-masked', 0x00800000n, 0x3f000000n, 0n, 0x00400000n, 0x1f80, 0x1f80],
  ['exact-tiny-unmasked', 0x00800000n, 0x3f000000n, 0n, 0n, 0x1780, 0x1790, 13],
  ['FTZ', 0x00800000n, 0x3f000000n, 0n, 0n, 0x9f80, 0x9fb0],
  ['DAZ', 0x80000001n, 0x3f800000n, 0n, 0n, 0x1fc0, 0x1fc0],
  ['denormal-masked', 1n, 0x3f800000n, 0n, 1n, 0x1f80, 0x1f82],
  ['denormal-unmasked', 1n, 0x3f800000n, 0n, 0n, 0x1e80, 0x1e82, 13],
  ['directed-finite-overflow', 0x7f7fffffn, 0x40000000n, 0n, 0x7f7fffffn, 0x7f80, 0x7fa8],
  ['unmasked-overflow', 0x7f7fffffn, 0x40000000n, 0n, 0n, 0x1b80, 0x1b88, 13],
  ['rounded-minimum-normal', 0x00800000n, 0x3f7fffffn, 0n, 0x00800000n, 0x1f80, 0x1fb0],
];

const double = [
  [
    'one-round-cancellation',
    0x3ff0000000000001n,
    0x3feffffffffffffen,
    0xbff0000000000000n,
    0xb970000000000000n,
    0x1f80,
    0x1f80,
  ],
  [
    'qNaN-suppresses-zero-times-infinity',
    0n,
    0x7ff0000000000000n,
    0x7ff8000000000044n,
    0x7ff8000000000044n,
    0x1f80,
    0x1f80,
  ],
  [
    'masked-invalid',
    0x7ff0000000000001n,
    0x3ff0000000000000n,
    0n,
    0x7ff8000000000001n,
    0x1f80,
    0x1f81,
  ],
  ['unmasked-invalid', 0x7ff0000000000001n, 0x3ff0000000000000n, 0n, 0n, 0x1f00, 0x1f01, 13],
  [
    'already-sticky-invalid-still-traps',
    0x7ff0000000000001n,
    0x3ff0000000000000n,
    0n,
    0n,
    0x1f01,
    0x1f01,
    13,
  ],
  [
    'exact-tiny-masked',
    0x0010000000000000n,
    0x3fe0000000000000n,
    0n,
    0x0008000000000000n,
    0x1f80,
    0x1f80,
  ],
  ['exact-tiny-unmasked', 0x0010000000000000n, 0x3fe0000000000000n, 0n, 0n, 0x1780, 0x1790, 13],
  ['FTZ', 0x0010000000000000n, 0x3fe0000000000000n, 0n, 0n, 0x9f80, 0x9fb0],
  ['DAZ', 0x8000000000000001n, 0x3ff0000000000000n, 0n, 0n, 0x1fc0, 0x1fc0],
  ['denormal-masked', 1n, 0x3ff0000000000000n, 0n, 1n, 0x1f80, 0x1f82],
  ['denormal-unmasked', 1n, 0x3ff0000000000000n, 0n, 0n, 0x1e80, 0x1e82, 13],
  [
    'directed-finite-overflow',
    0x7fefffffffffffffn,
    0x4000000000000000n,
    0n,
    0x7fefffffffffffffn,
    0x7f80,
    0x7fa8,
  ],
  ['unmasked-overflow', 0x7fefffffffffffffn, 0x4000000000000000n, 0n, 0n, 0x1b80, 0x1b88, 13],
  [
    'rounded-minimum-normal',
    0x0010000000000000n,
    0x3fefffffffffffffn,
    0n,
    0x0010000000000000n,
    0x1f80,
    0x1fb0,
  ],
];

export function buildCases() {
  const rows = [];
  const add = (row, group, controls = false) => rows.push({ ...row, group, controls });

  for (let kind = 0; kind < 6; kind++) {
    for (let order = 0; order < 3; order++) {
      for (const form of shapes) {
        if (kind >= 4 && form.scalar) continue;
        for (const aliases of [
          [0, 1, 2],
          [0, 0, 2],
          [0, 1, 0],
          [0, 1, 1],
          [0, 0, 0],
        ]) {
          add(
            normalRow(kind, order, form, false, aliases),
            'family',
            kind === 0 && order === 0 && aliases.every((value) => value === 0),
          );
        }
        add(normalRow(kind, order, form, true), 'family');
        if (form.scalar) {
          for (const memory of [false, true]) {
            const row = normalRow(kind, order, form, memory);
            add({ ...row, name: `${row.name}-LIG`, lig: true, mxcsr: 0x1f00 }, 'scalar-LIG');
          }
        }
      }
    }
  }

  for (let order = 0; order < 3; order++) {
    for (const form of shapes) {
      for (const aliases of [
        [7, 0, 1],
        [0, 7, 1],
        [0, 1, 7],
        [7, 7, 7],
      ]) {
        add(normalRow(0, order, form, false, aliases), 'high-registers');
      }
    }
  }

  for (let kind = 0; kind < 4; kind++) {
    for (let order = 0; order < 3; order++) {
      for (const form of shapes) {
        for (const memory of [false, true]) {
          const row = normalRow(kind, order, form, memory);
          const values = form.double
            ? [0xfff8000000000011n, 0x7ff8000000000022n, 0x7ff0000000000033n]
            : [0xffc00011n, 0x7fc00022n, 0x7f800033n];
          row.name = `NaN-before-sign-${kind}-${order}-${shape(form)}-${memory}`;
          row.vectors = Object.fromEntries(
            values.map((value, index) => [index, repeated(value, form.double)]),
          );
          row.payload = repeated(values[2], form.double).subarray(0, memoryBytes(form));
          row.result = lanes(
            Array(form.scalar ? 1 : memoryBytes(form) / (form.double ? 8 : 4)).fill(
              values[order === 0 ? 0 : 1],
            ),
            form.double,
          );
          row.after = 0x1f81;
          markInactive(row);
          add(row, 'NaN-order');
        }
      }
    }
  }

  for (const form of shapes) {
    for (const memory of [false, true]) {
      const literals = form.double ? double : single;
      for (const literal of literals) {
        add(literalRow(form, memory, 0, literal), 'numeric', literal[0] === 'unmasked-invalid');
      }
      for (const order of [1, 2]) {
        add(literalRow(form, memory, order, literals[0]), 'fused-order');
      }
      for (let order = 0; order < 3; order++) {
        for (let rounding = 0; rounding < 4; rounding++) {
          for (const negative of [false, true]) {
            const one = form.double ? 0x3ff0000000000000n : 0x3f800000n;
            const halfUlp = form.double ? 0x3ca0000000000000n : 0x33800000n;
            const sign = negative ? (form.double ? 0x8000000000000000n : 0x80000000n) : 0n;
            const increment = (negative && rounding === 1) || (!negative && rounding === 2);
            const mxcsr = 0x1f88 | (rounding << 13);
            add(
              literalRow(form, memory, order, [
                `half-ulp-RC-${rounding}-${negative}`,
                one | sign,
                one,
                halfUlp | sign,
                (one + BigInt(increment)) | sign,
                mxcsr,
                mxcsr | 0x20,
              ]),
              'rounding',
            );
          }
        }
      }
    }
  }

  for (const form of shapes) {
    const base = normalRow(0, 0, form, true);
    const bytes = memoryBytes(form);
    add({ ...base, name: `exact-page-end-${shape(form)}`, address: 0x6000 - bytes }, 'memory');
    add({ ...base, name: `unaligned-${shape(form)}`, address: 0x5021 }, 'memory');
    const faults = [
      {
        name: 'late-page',
        address: 0x6000 - bytes + 1,
        reason: 5,
        detail: 1,
        fault: 0x6000,
        access: 1,
        initialize: false,
      },
      { name: 'cached-DS-limit', cache: { 3: 0xf2305021 }, reason: 11, detail: 0 },
      {
        name: 'cached-SS-limit',
        cache: { 2: 0xf2305021 },
        prefix: [0x36],
        segment: 2,
        reason: 15,
        detail: 0,
      },
      { name: 'null-DS', nullDs: true, reason: 11, detail: 0 },
      { name: 'logical-overflow', address: 0xffffffff, reason: 11, detail: 0, initialize: false },
    ];
    for (const fault of faults) {
      add({ ...base, ...fault, name: `${fault.name}-${shape(form)}` }, 'memory');
    }
    const priority = literalRow(form, true, 0, (form.double ? double : single)[3]);
    const page = {
      ...priority,
      name: `PF-before-would-trap-XM-${shape(form)}`,
      address: 0x6000 - bytes + 1,
      reason: 5,
      detail: 1,
      fault: 0x6000,
      access: 1,
      initialize: false,
    };
    add(page, 'authority-priority', true);
    add(
      {
        ...page,
        name: `read-permission-before-XM-${shape(form)}`,
        address: 0x5020,
        fault: 0x5020,
        detail: 2,
        permissions: 0,
      },
      'authority-priority',
    );
    add(
      { ...priority, name: `repaired-authority-delivers-XM-${shape(form)}` },
      'authority-priority',
    );
  }

  for (const isDouble of [false, true]) {
    const form = { scalar: false, double: isDouble, width: 32 };
    const row = normalRow(0, 1, form);
    const one = isDouble ? 0x3ff0000000000000n : 0x3f800000n;
    const halfUlp = isDouble ? 0x3ca0000000000000n : 0x33800000n;
    const signaling = isDouble ? 0x7ff0000000000001n : 0x7f800001n;
    const values = Array(isDouble ? 4 : 8).fill(0n);
    values[0] = halfUlp;
    values[values.length - 1] = signaling;
    Object.assign(row, {
      name: `upper-invalid-suppresses-lower-precision-${isDouble}`,
      vectors: {
        0: repeated(one, isDouble),
        1: repeated(one, isDouble),
        2: lanes(values, isDouble),
      },
      mxcsr: 0x1f02,
      reason: 13,
      detail: 0x1f03,
    });
    add(row, 'exception-priority');
  }

  const identities = new Set(rows.filter((row) => row.group === 'family').map(descriptor));
  assert.equal(identities.size, 96);
  assert.equal(new Set(rows.map((row) => row.name)).size, rows.length);
  return rows;
}
