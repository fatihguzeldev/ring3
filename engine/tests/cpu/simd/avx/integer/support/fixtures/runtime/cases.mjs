import assert from 'node:assert/strict';

function encode(form, encoding, destination, first, second, width = 32, prefix = []) {
  const vvvv = form.unary ? 15 : 15 - first;
  const control = (vvvv << 3) | (width === 32 ? 4 : 0) | 1;
  const vex =
    encoding === 'c5'
      ? [0xc5, control | 0x80]
      : [0xc4, 0xe0 | form.map, control | (encoding === 'w1' ? 0x80 : 0)];
  assert.ok(form.map === 1 || encoding !== 'c5');
  const modrm = second === null ? destination << 3 : 0xc0 | (destination << 3) | second;
  return Buffer.from([...prefix, ...vex, form.opcode, modrm]).toString('hex');
}

function pack(lane, numbers, start = 0) {
  const bytes = Buffer.alloc(32);
  for (let at = 0; at < 32; at += lane) {
    let value = numbers[(at / lane + start) % numbers.length];
    for (let byte = 0; byte < lane; byte++) {
      bytes[at + byte] = Number(value & 255n);
      value >>= 8n;
    }
  }
  return bytes;
}

function pattern(lane, seed) {
  const sign = 1n << BigInt(lane * 8 - 1);
  const mask = sign * 2n - 1n;
  return pack(
    lane,
    [0n, mask, sign, sign - 1n, 1n, mask - 1n, sign + 1n, mask / 3n, 2n, mask / 2n + 3n, mask - 3n],
    seed,
  );
}

function result(form, first, second, width, oracle) {
  const value = Buffer.alloc(32);
  for (let at = 0; at < width; at += form.lane) {
    const read = (bytes) => {
      let number = 0n;
      for (let byte = form.lane - 1; byte >= 0; byte--)
        number = number * 256n + BigInt(bytes[at + byte]);
      return number;
    };
    let number = oracle(form, read(first), read(second));
    for (let byte = 0; byte < form.lane; byte++) {
      value[at + byte] = Number(number & 255n);
      number >>= 8n;
    }
  }
  return value;
}

function outputs(destination, value) {
  return {
    outLower: { [destination]: value.subarray(0, 16) },
    outUpper: { [destination]: value.subarray(16) },
  };
}

export function groupRows(group, forms, oracle, literals) {
  const rows = [];
  for (const [formIndex, form] of forms.entries()) {
    const encodings = form.map === 1 ? ['c5', 'w0', 'w1'] : ['w0', 'w1'];
    for (const [encodingIndex, encoding] of encodings.entries()) {
      const d = (formIndex * 2 + encodingIndex) % 8;
      const a = (d + 1) % 8;
      const b = (d + 2) % 8;
      const partitions = form.unary
        ? [
            [d, 0, b],
            [b, 0, b],
          ]
        : [
            [d, a, b],
            [a, a, b],
            [b, a, b],
            [d, a, a],
            [a, a, a],
          ];
      for (const [destination, first, second] of partitions) {
        const values = Array.from({ length: 8 }, (_, register) => pattern(form.lane, register * 3));
        rows.push({
          name: `avx2-${group}-${form.name}-${encoding}-${destination}/${first}/${second}`,
          code: encode(form, encoding, destination, first, second),
          lower: Object.fromEntries(
            values.map((value, register) => [register, value.subarray(0, 16)]),
          ),
          upper: Object.fromEntries(
            values.map((value, register) => [register, value.subarray(16)]),
          ),
          ...outputs(destination, result(form, values[first], values[second], 32, oracle)),
        });
      }
    }
    const controls = Array.from({ length: 8 }, (_, register) => pattern(form.lane, register * 3));
    rows.push({
      name: `avx2-${group}-${form.name}-xmm-control`,
      code: encode(form, 'w1', 7, 6, 7, 16),
      lower: Object.fromEntries(
        controls.map((value, register) => [register, value.subarray(0, 16)]),
      ),
      upper: Object.fromEntries(controls.map((value, register) => [register, value.subarray(16)])),
      ...outputs(7, result(form, controls[6], controls[7], 16, oracle)),
    });

    const literal = literals(form);
    for (const start of [0, 4]) {
      const first = pack(form.lane, literal.first, start);
      const second = pack(form.lane, literal.second, start);
      const expected = pack(form.lane, literal.expected, start);
      assert.deepEqual(
        result(form, first, second, 32, oracle),
        expected,
        `literal ${group}/${form.name}/${start}`,
      );
      rows.push({
        name: `avx2-${group}-${form.name}-literal-boundary-${start}`,
        code: encode(form, 'w0', 7, 6, 5),
        lower: { 6: first.subarray(0, 16), 5: second.subarray(0, 16) },
        upper: { 6: first.subarray(16), 5: second.subarray(16) },
        ...outputs(7, expected),
      });
    }

    const destination = formIndex % 8;
    const first = pattern(form.lane, 0);
    const second = pattern(form.lane, 7);
    const expected = result(form, first, second, 32, oracle);
    for (const address of [0x5011, 0x5fe0]) {
      rows.push({
        name: `avx2-${group}-${form.name}-memory-exact32-${address.toString(16)}`,
        code: encode(form, 'w1', destination, destination, null),
        address,
        read: 32,
        memory: second,
        lower: { [destination]: first.subarray(0, 16) },
        upper: { [destination]: first.subarray(16) },
        ...outputs(destination, expected),
      });
    }
    rows.push({
      name: `avx2-${group}-${form.name}-late32nd-byte-page-fault`,
      code: encode(form, 'w0', destination, destination, null),
      address: 0x5fe1,
      read: 32,
      memory: second.subarray(0, 31),
      lower: { [destination]: first.subarray(0, 16) },
      upper: { [destination]: first.subarray(16) },
      reason: 5,
      detail: 1,
      fault: 0x6000,
      access: 1,
      length: 32,
    });
    for (const [segment, prefix, reason] of [
      ['ds', [], 11],
      ['ss', [0x36], 15],
    ]) {
      rows.push({
        name: `avx2-${group}-${form.name}-${segment}-cached-span-before-page-access`,
        code: encode(form, 'w0', destination, destination, null, 32, prefix),
        address: 0xfffffff0,
        read: 32,
        helpers: 0,
        reason,
      });
    }
  }
  assert.equal(new Set(rows.map((row) => row.name)).size, rows.length);
  assert.equal(rows.filter((row) => row.reason).length, forms.length * 3);
  assert.equal(rows.filter((row) => row.memory && !row.reason).length, forms.length * 2);
  return rows;
}

export const mechanicsSourcePaths = [
  'engine/tests/cpu/simd/avx/integer/support/native.rs',
  'engine/tests/cpu/simd/avx/integer/support/fixtures/runtime/cases.mjs',
];
