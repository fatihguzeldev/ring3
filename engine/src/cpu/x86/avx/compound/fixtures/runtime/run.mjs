import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { verifyNumericReceipt } from "../../../../../../../tests/support/numeric/receipt.mjs";

const binaryPath = resolve(process.argv[2] ?? process.env.RING3_AVX_NUMERIC_WASM ?? "");
assert(binaryPath.endsWith(".wasm"), "supply the isolated real Rust numeric backend .wasm");
const binary = readFileSync(binaryPath);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
if (process.env.RING3_AVX_PROOF_SHA256) {
  assert.equal(sha256(binary), process.env.RING3_AVX_PROOF_SHA256);
}
const backendSourceArgument = process.argv[4] ?? process.env.RING3_AVX_NUMERIC_SOURCE_ROOT;
assert.equal(typeof backendSourceArgument, "string", "supply the matching backend source root");
assert(backendSourceArgument.length > 0, "supply the matching backend source root");
const backendSourceRoot = resolve(backendSourceArgument);
const backendReceiptArgument = process.argv[5] ?? process.env.RING3_AVX_NUMERIC_RECEIPT;
assert.equal(typeof backendReceiptArgument, "string", "supply the matching backend receipt");
assert(backendReceiptArgument.length > 0, "supply the matching backend receipt");
const backendReceiptPath = resolve(backendReceiptArgument);
const backendReceiptBytes = readFileSync(backendReceiptPath);
const backendReceipt = JSON.parse(backendReceiptBytes);
const backendSources = verifyNumericReceipt(binary, backendSourceRoot, backendReceipt, "compound");
const module = new WebAssembly.Module(binary);
const declarations = WebAssembly.Module.exports(module);
const probeNames = ["ring3_avx_probe_read", "ring3_avx_probe_run", "ring3_avx_probe_write"];
assert.deepEqual(
  declarations
    .filter((e) => e.name.startsWith("ring3_avx_probe_"))
    .map((e) => [e.name, e.kind])
    .sort(),
  probeNames.map((name) => [name, "function"]),
);
assert.deepEqual(WebAssembly.Module.imports(module), []);
const backend = new WebAssembly.Instance(module).exports;
for (const [name, arity] of [
  [probeNames[0], 1],
  [probeNames[1], 4],
  [probeNames[2], 2],
]) {
  assert.equal(backend[name].length, arity);
}

function uleb(number) {
  const bytes = [];
  do {
    const byte = number & 127;
    number >>>= 7;
    bytes.push(byte | (number ? 128 : 0));
  } while (number);
  return bytes;
}
function sleb(number) {
  const bytes = [];
  number |= 0;
  for (;;) {
    const byte = number & 127;
    number >>= 7;
    const done = (number === 0 && !(byte & 64)) || (number === -1 && byte & 64);
    bytes.push(byte | (done ? 0 : 128));
    if (done) return bytes;
  }
}
const text = (value) => [...uleb(Buffer.byteLength(value)), ...Buffer.from(value)];
const section = (id, bytes) => [id, ...uleb(bytes.length), ...bytes];
const calls = new Map();
function caller(operation, registers, immediate, length) {
  const key = `${operation}/${registers}/${immediate}/${length}`;
  if (!calls.has(key)) {
    const body = [
      0,
      ...[operation, registers, immediate, length].flatMap((n) => [0x41, ...sleb(n)]),
      0x10,
      0,
      0x0b,
    ];
    const bytes = Uint8Array.from([
      0,
      97,
      115,
      109,
      1,
      0,
      0,
      0,
      ...section(1, [2, 0x60, 4, 0x7f, 0x7f, 0x7f, 0x7f, 1, 0x7f, 0x60, 0, 1, 0x7f]),
      ...section(2, [1, ...text("numeric"), ...text("run"), 0, 0]),
      ...section(3, [1, 1]),
      ...section(7, [1, ...text("run"), 0, 1]),
      ...section(10, [1, ...uleb(body.length), ...body]),
    ]);
    const generated = new WebAssembly.Module(bytes);
    assert.deepEqual(WebAssembly.Module.imports(generated), [
      { module: "numeric", name: "run", kind: "function" },
    ]);
    assert.deepEqual(WebAssembly.Module.exports(generated), [{ name: "run", kind: "function" }]);
    calls.set(
      key,
      new WebAssembly.Instance(generated, { numeric: { run: backend.ring3_avx_probe_run } }).exports
        .run,
    );
  }
  return calls.get(key);
}
function state(mxcsr = 0x1f80) {
  const words = Uint32Array.from({ length: 96 }, (_, i) => (0x9e3779b9 * (i + 1)) >>> 0);
  words[8] = 0x12345678;
  words[9] = 0xa5f7ffff;
  words[10] = mxcsr;
  return words;
}
function vector(words, register) {
  const bytes = Buffer.alloc(32);
  for (let i = 0; i < 4; i++) {
    bytes.writeUInt32LE(words[12 + register * 4 + i], i * 4);
    bytes.writeUInt32LE(words[44 + register * 4 + i], 16 + i * 4);
  }
  return bytes;
}
function publish(words, register, bytes) {
  assert.equal(bytes.length, 32);
  for (let i = 0; i < 4; i++) {
    words[12 + register * 4 + i] = bytes.readUInt32LE(i * 4);
    words[44 + register * 4 + i] = bytes.readUInt32LE(16 + i * 4);
  }
}
function lanes(values, width) {
  const bytes = Buffer.alloc(32);
  for (let offset = 0; offset < 32; offset += width) {
    const value = BigInt(values[(offset / width) % values.length]);
    if (width === 4) bytes.writeUInt32LE(Number(BigInt.asUintN(32, value)), offset);
    else bytes.writeBigUInt64LE(BigInt.asUintN(64, value), offset);
  }
  return bytes;
}
function source(words, register, bytes, memory, length) {
  if (!memory) publish(words, register, bytes);
  else for (let i = 0; i < length / 4; i++) words[76 + i] = bytes.readUInt32LE(i * 4);
}
const registers = (d, a, b, memory) => (d | (a << 8) | (b << 16) | (memory ? 1 << 24 : 0)) >>> 0;
let runs = 0;
const groups = {};
function run(group, name, operation, regs, immediate, length, before, expected, status = 0) {
  for (let i = 0; i < 96; i++) assert.equal(backend.ring3_avx_probe_write(i, before[i]), 0);
  assert.equal(
    caller(operation, regs, immediate, length)() >>> 0,
    status,
    `${group}/${name}: status`,
  );
  const observed = Uint32Array.from(
    { length: 96 },
    (_, i) => backend.ring3_avx_probe_read(i) >>> 0,
  );
  assert.deepEqual(observed, expected, `${group}/${name}: whole 96-word state`);
  runs++;
  groups[group] = (groups[group] ?? 0) + 1;
}

const pairwise = [
  [
    38,
    8,
    [0x3ff0000000000000n, 0x4000000000000000n, 0x4030000000000000n, 0x4040000000000000n],
    [0x4010000000000000n, 0x4020000000000000n, 0x3ff0000000000000n, 0x4000000000000000n],
    [0x4008000000000000n, 0x4028000000000000n, 0x4048000000000000n, 0x4008000000000000n],
  ],
  [
    40,
    4,
    [
      0x3f800000n,
      0x40000000n,
      0x40800000n,
      0x41000000n,
      0x41800000n,
      0x42000000n,
      0x42800000n,
      0x43000000n,
    ],
    [
      0x41800000n,
      0x42000000n,
      0x42800000n,
      0x43000000n,
      0x3f800000n,
      0x40000000n,
      0x40800000n,
      0x41000000n,
    ],
    [
      0x40400000n,
      0x41400000n,
      0x42400000n,
      0x43400000n,
      0x42400000n,
      0x43400000n,
      0x40400000n,
      0x41400000n,
    ],
  ],
  [
    42,
    8,
    [0x3ff0000000000000n, 0x4000000000000000n, 0x4030000000000000n, 0x4040000000000000n],
    [0x4010000000000000n, 0x4020000000000000n, 0x3ff0000000000000n, 0x4000000000000000n],
    [0xbff0000000000000n, 0xc010000000000000n, 0xc030000000000000n, 0xbff0000000000000n],
  ],
  [
    44,
    4,
    [
      0x3f800000n,
      0x40000000n,
      0x40800000n,
      0x41000000n,
      0x41800000n,
      0x42000000n,
      0x42800000n,
      0x43000000n,
    ],
    [
      0x41800000n,
      0x42000000n,
      0x42800000n,
      0x43000000n,
      0x3f800000n,
      0x40000000n,
      0x40800000n,
      0x41000000n,
    ],
    [
      0xbf800000n,
      0xc0800000n,
      0xc1800000n,
      0xc2800000n,
      0xc1800000n,
      0xc2800000n,
      0xbf800000n,
      0xc0800000n,
    ],
  ],
  [
    46,
    8,
    [0x3ff0000000000000n, 0x4000000000000000n, 0x4030000000000000n, 0x4040000000000000n],
    [0x4010000000000000n, 0x4020000000000000n, 0x3ff0000000000000n, 0x4000000000000000n],
    [0xc008000000000000n, 0x4024000000000000n, 0x402e000000000000n, 0x4041000000000000n],
  ],
  [
    48,
    4,
    [
      0x3f800000n,
      0x40000000n,
      0x40800000n,
      0x41000000n,
      0x41800000n,
      0x42000000n,
      0x42800000n,
      0x43000000n,
    ],
    [
      0x41800000n,
      0x42000000n,
      0x42800000n,
      0x43000000n,
      0x3f800000n,
      0x40000000n,
      0x40800000n,
      0x41000000n,
    ],
    [
      0xc1700000n,
      0x42080000n,
      0xc2700000n,
      0x43080000n,
      0x41700000n,
      0x42080000n,
      0x42700000n,
      0x43080000n,
    ],
  ],
];
for (const [base, width, a, b, literal] of pairwise)
  for (let size = 0; size < 2; size++) {
    const op = base + size;
    const length = size ? 32 : 16;
    for (const memory of [false, true])
      for (const [d, first, second] of memory
        ? [
            [0, 1, 0],
            [1, 1, 0],
          ]
        : [
            [0, 1, 2],
            [1, 1, 2],
            [2, 1, 2],
          ]) {
        const before = state();
        publish(before, first, lanes(a, width));
        source(before, second, lanes(b, width), memory, length);
        const expected = before.slice();
        const result = lanes(literal, width);
        result.fill(0, length);
        publish(expected, d, result);
        run(
          "pairwise",
          `${op}/${memory}/${d}`,
          op,
          registers(d, first, second, memory),
          0,
          memory ? length : 0,
          before,
          expected,
        );
      }
  }
const roundInput = {
  4: [0x3fc00000n, 0x40200000n, 0xbfc00000n, 0xc0200000n],
  8: [0x3ff8000000000000n, 0x4004000000000000n, 0xbff8000000000000n, 0xc004000000000000n],
};
const roundResult = {
  4: [
    [0x40000000n, 0x40000000n, 0xc0000000n, 0xc0000000n],
    [0x3f800000n, 0x40000000n, 0xc0000000n, 0xc0400000n],
    [0x40000000n, 0x40400000n, 0xbf800000n, 0xc0000000n],
    [0x3f800000n, 0x40000000n, 0xbf800000n, 0xc0000000n],
  ],
  8: [
    [0x4000000000000000n, 0x4000000000000000n, 0xc000000000000000n, 0xc000000000000000n],
    [0x3ff0000000000000n, 0x4000000000000000n, 0xc000000000000000n, 0xc008000000000000n],
    [0x4000000000000000n, 0x4008000000000000n, 0xbff0000000000000n, 0xc000000000000000n],
    [0x3ff0000000000000n, 0x4000000000000000n, 0xbff0000000000000n, 0xc000000000000000n],
  ],
};
for (let op = 50; op <= 55; op++) {
  const scalar = op >= 54;
  const width = [52, 53, 55].includes(op) ? 8 : 4;
  const length = scalar ? width : op % 2 ? 32 : 16;
  for (let immediate = 0; immediate < 16; immediate++)
    for (let mode = 0; mode < 4; mode++)
      for (const memory of [false, true]) {
        const before = state(0x1f80 | (mode << 13));
        source(before, 2, lanes(roundInput[width], width), memory, length);
        const expected = before.slice();
        const selected = immediate & 4 ? mode : immediate & 3;
        const result = scalar ? vector(before, 1) : Buffer.alloc(32);
        lanes(roundResult[width][selected], width).copy(result, 0, 0, length);
        if (scalar) result.fill(0, 16);
        publish(expected, 0, result);
        expected[10] |= immediate & 8 ? 0 : 0x20;
        run(
          "round",
          `${op}/${immediate}/${mode}/${memory}`,
          op,
          registers(0, 1, 2, memory),
          immediate,
          memory ? length : 0,
          before,
          expected,
        );
        if (mode === 0)
          run(
            "round-reserved-high-bits",
            `${op}/${immediate}/${memory}`,
            op,
            registers(0, 1, 2, memory),
            immediate | 0xf0,
            memory ? length : 0,
            before,
            expected,
          );
      }
}
const sums = [
  0n,
  0x3f800000n,
  0x40000000n,
  0x40400000n,
  0x40800000n,
  0x40a00000n,
  0x40c00000n,
  0x40e00000n,
  0x41000000n,
  0x41100000n,
  0x41200000n,
  0x41300000n,
  0x41400000n,
  0x41500000n,
  0x41600000n,
  0x41700000n,
];
for (let op = 56; op <= 58; op++) {
  const width = op === 58 ? 8 : 4;
  const length = op === 57 ? 32 : 16;
  const a =
    op === 58
      ? [0x3ff0000000000000n, 0x4000000000000000n]
      : [
          0x3f800000n,
          0x40000000n,
          0x40800000n,
          0x41000000n,
          0x41800000n,
          0x42000000n,
          0x42800000n,
          0x43000000n,
        ];
  const b = [op === 58 ? 0x3ff0000000000000n : 0x3f800000n];
  for (let immediate = 0; immediate < 256; immediate++)
    for (const memory of [false, true])
      for (const [d, first, second] of memory
        ? [
            [0, 1, 0],
            [1, 1, 0],
          ]
        : [
            [0, 1, 2],
            [1, 1, 2],
            [2, 1, 2],
          ]) {
        const before = state();
        publish(before, first, lanes(a, width));
        source(before, second, lanes(b, width), memory, length);
        const expected = before.slice();
        const result = Buffer.alloc(32);
        for (let lane = 0; lane < length / width; lane++)
          if (immediate & (1 << (lane % (16 / width)))) {
            const mask = immediate >> 4;
            const value =
              op === 58
                ? [0n, 0x3ff0000000000000n, 0x4000000000000000n, 0x4008000000000000n][mask & 3]
                : sums[mask] + (lane >= 4 && mask !== 0 ? 0x02000000n : 0n);
            lanes([value], width).copy(result, lane * width, 0, width);
          }
        publish(expected, d, result);
        run(
          "dot-mask",
          `${op}/${immediate}/${memory}/${d}`,
          op,
          registers(d, first, second, memory),
          immediate,
          memory ? length : 0,
          before,
          expected,
        );
      }
}
function pairwiseFault(name, op, a, b, mxcsr, expectedMxcsr, expectedValue, status = 3) {
  for (const memory of [false, true]) {
    const length = op % 2 ? 32 : 16;
    const width = [38, 39, 42, 43, 46, 47].includes(op) ? 8 : 4;
    const before = state(mxcsr);
    publish(before, 1, lanes(a, width));
    source(before, 2, lanes(b, width), memory, length);
    const expected = before.slice();
    expected[10] = expectedMxcsr;
    if (expectedValue) {
      const result = lanes(expectedValue, width);
      result.fill(0, length);
      publish(expected, 0, result);
    }
    run(
      "pairwise-special",
      `${name}/${memory}`,
      op,
      registers(0, 1, 2, memory),
      0,
      memory ? length : 0,
      before,
      expected,
      status,
    );
  }
}
pairwiseFault(
  "nan-order",
  40,
  [0x7fc01234n, 0x7f805678n, 0, 0],
  [0],
  0x1f80,
  0x1f81,
  [0x7fc01234n, 0, 0, 0],
  0,
);
pairwiseFault(
  "upper-exception-aggregation",
  41,
  [0x4b800000n, 0x3f800000n, 0, 0, 0x7f800000n, 0xff800000n, 1, 0],
  [0],
  0x1f00,
  0x1f03,
  null,
);
pairwiseFault(
  "directed-overflow-metadata",
  48,
  [0x7f7fffffn, 0],
  [0xff7fffffn, 0],
  0x7f80,
  0x7fa8,
  [0x7f7fffffn, 0, 0x7f7fffffn, 0],
  0,
);
for (const [mxcsr, expectedMxcsr] of [
  [0x0f00, 0x0f01],
  [0x1f00, 0x1f01],
])
  for (const memory of [false, true]) {
    const before = state(mxcsr);
    publish(before, 1, lanes([0x4b800000n, 0x3f800000n, 0, 0, 0x7f800001n, 0, 0, 0], 4));
    source(before, 2, lanes([0x3f800000n], 4), memory, 32);
    const expected = before.slice();
    expected[10] = expectedMxcsr;
    run(
      "dot-stage-order",
      `${mxcsr}/${memory}`,
      57,
      registers(0, 1, 2, memory),
      0xff,
      memory ? 32 : 0,
      before,
      expected,
      3,
    );
  }
for (const [op, input] of [
  [56, [0x4b800000n, 0x3f800000n, 0x7f800000n, 0xff800000n]],
  [57, [0x4b800000n, 0x3f800000n, 0, 0, 0, 0, 0x7f800000n, 0xff800000n]],
])
  for (const memory of [false, true])
    for (const destination of [0, 1, 2])
      for (const [mxcsr, expectedMxcsr] of [
        [0x0f00, 0x0f01],
        [0x1f00, 0x1f01],
      ]) {
        const before = state(mxcsr);
        publish(before, 1, lanes(input, 4));
        source(before, 2, lanes([0x3f800000n], 4), memory, op === 56 ? 16 : 32);
        const expected = before.slice();
        expected[10] = expectedMxcsr;
        run(
          "dot-pair-stage-priority",
          `${op}/${memory}/${destination}/${mxcsr}`,
          op,
          registers(destination, 1, 2, memory),
          0xff,
          memory ? (op === 56 ? 16 : 32) : 0,
          before,
          expected,
          3,
        );
      }
for (const op of [56, 57]) {
  const before = state(0x1f80);
  publish(before, 1, lanes([0x4b800000n, 0x3f800000n, 0x7f800000n, 0xff800000n], 4));
  publish(before, 2, lanes([0x3f800000n], 4));
  const expected = before.slice();
  const result = lanes([0xffc00000n], 4);
  if (op === 56) result.fill(0, 16);
  publish(expected, 0, result);
  expected[10] = 0x1fa1;
  run(
    "dot-pair-stage-masked",
    String(op),
    op,
    registers(0, 1, 2, false),
    0xff,
    0,
    before,
    expected,
    0,
  );
}
for (const [name, a, b, imm, mxcsr, result, status] of [
  [
    "non-fused",
    [0x3f800001n, 0xbf800000n, 0, 0],
    [0x3f7ffffen, 0x3f800000n, 0x3f800000n, 0x3f800000n],
    0xff,
    0x1fa0,
    [0],
    0,
  ],
  ["masked-inputs", [0x7f800001n, 1, 0x7f800000n, 0xff800000n], [0], 0x0f, 0, [0], 0],
  ["masked-output-still-computes", [0x7f800001n], [0x3f800000n], 0xf0, 0x1f01, null, 3],
]) {
  const before = state(
    name === "masked-inputs" ? 0 : name === "masked-output-still-computes" ? 0x1f00 : 0x1f80,
  );
  publish(before, 1, lanes(a, 4));
  publish(before, 2, lanes(b, 4));
  const expected = before.slice();
  expected[10] = mxcsr;
  if (result) publish(expected, 0, lanes(result, 4));
  run("dot-special", name, 56, registers(0, 1, 2, false), imm, 0, before, expected, status);
}
for (const op of [54, 55])
  for (const [mxcsr, raw, expectedMxcsr, status] of [
    [0x1f80, 1n, 0x1fa0, 0],
    [0x1fc0, 1n, 0x1fc0, 0],
    [0x0f80, 1n, 0x0fa0, 3],
    [0x1f00, op === 54 ? 0x7f800001n : 0x7ff0000000000001n, 0x1f01, 3],
  ]) {
    const width = op === 54 ? 4 : 8;
    const before = state(mxcsr);
    publish(before, 2, lanes([raw], width));
    const expected = before.slice();
    expected[10] = expectedMxcsr;
    if (status === 0) {
      const result = vector(before, 1);
      result.fill(0, 0, width);
      result.fill(0, 16);
      publish(expected, 0, result);
    }
    run(
      "round-special",
      `${op}/${mxcsr}`,
      op,
      registers(0, 1, 2, false),
      0,
      0,
      before,
      expected,
      status,
    );
  }
for (const [op, length] of [
  [38, 16],
  [39, 32],
  [40, 16],
  [41, 32],
  [42, 16],
  [43, 32],
  [44, 16],
  [45, 32],
  [46, 16],
  [47, 32],
  [48, 16],
  [49, 32],
  [50, 16],
  [51, 32],
  [52, 16],
  [53, 32],
  [54, 4],
  [55, 8],
  [56, 16],
  [57, 32],
  [58, 16],
]) {
  for (const wrong of [0, length - 1, length + 1]) {
    const before = state();
    run(
      "invalid-length",
      `${op}/${wrong}`,
      op,
      registers(0, 1, 0, true),
      0,
      wrong,
      before,
      before,
      1,
    );
  }
  const before = state(0x11f80);
  run("invalid-state", `${op}`, op, registers(0, 1, 0, true), 0, length, before, before, 2);
}
for (const [op, regs, imm, len] of [
  [0xffffffff, 0, 0, 0],
  [38, 8, 0, 0],
  [38, 1 << 25, 0, 0],
  [38, 0, 1, 0],
  [50, 0, 256, 0],
  [57, 0, 0, 33],
]) {
  const before = state();
  run("invalid-request", `${op}/${regs}/${imm}/${len}`, op, regs, imm, len, before, before, 1);
}
const sourceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../../../../../..");
const runnerSources = [
  "tests/support/numeric/receipt.mjs",
  "tests/support/numeric/contract.json",
  "src/cpu/x86/avx/compound/fixtures/runtime/run.mjs",
  "src/cpu/x86/avx/compound/wasm.rs",
].map((path) => [path, sha256(readFileSync(resolve(sourceRoot, path)))]);
const receipt = {
  backend: binaryPath,
  backendSha256: sha256(binary),
  runs,
  generatedModules: calls.size,
  groups,
  backendSourceRoot,
  backendReceipt: backendReceiptPath,
  backendReceiptSha256: sha256(backendReceiptBytes),
  backendSources,
  runnerSources,
  sourceScope:
    "the pinned external backend snapshot is separate from current runner sources; this is not a current production build claim",
  numericExecution:
    "real Rust-compiled Wasm shared CPU/math semantics through direct generated Wasm import; JavaScript literal oracles",
  backendPublicAVX: backendReceipt.publicAVX,
  node: process.version,
};
if (process.argv[3]) {
  const output = resolve(process.argv[3]);
  mkdirSync(output, { recursive: true });
  writeFileSync(resolve(output, "proof.json"), JSON.stringify(receipt, null, 2) + "\n");
}
console.log(JSON.stringify(receipt));
