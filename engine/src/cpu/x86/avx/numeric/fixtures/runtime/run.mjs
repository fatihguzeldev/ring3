import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const binaryPath = resolve(process.argv[2] ?? process.env.RING3_AVX_NUMERIC_WASM ?? "");
assert(binaryPath.endsWith(".wasm"), "supply the isolated real Rust numeric backend .wasm");
const binary = readFileSync(binaryPath);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
if (process.env.RING3_AVX_PROOF_SHA256) {
  assert.equal(sha256(binary), process.env.RING3_AVX_PROOF_SHA256);
}
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

const truth = [2, 1, 3, 8, 13, 14, 12, 7, 10, 9, 11, 0, 5, 6, 4, 15];
const signaling = 0x99996666;
for (let op = 22; op <= 27; op++) {
  const scalar = op >= 26;
  const width = op === 24 || op === 25 || op === 27 ? 8 : 4;
  const total = scalar ? width : op % 2 ? 32 : 16;
  const [one, two, three, qnan] =
    width === 4
      ? [0x3f800000n, 0x40000000n, 0x40400000n, 0x7fc01234n]
      : [0x3ff0000000000000n, 0x4000000000000000n, 0x4008000000000000n, 0x7ff8000000001234n];
  for (let predicate = 0; predicate < 32; predicate++) {
    for (const memory of [false, true]) {
      for (const [d, a, b] of memory
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
        publish(before, a, lanes(scalar ? [qnan] : [one, two, three, qnan], width));
        source(before, b, lanes([two], width), memory, total);
        const first = vector(before, a);
        const expected = before.slice();
        const result = scalar ? Buffer.from(first) : Buffer.alloc(32);
        for (let lane = 0; lane < total / width; lane++) {
          const relation = scalar ? 3 : lane % 4;
          const literal = truth[predicate & 15] & (1 << relation) ? -1n : 0n;
          lanes([literal], width).copy(result, lane * width, 0, width);
        }
        if (scalar) result.fill(0, 16);
        publish(expected, d, result);
        const unorderedPresent = scalar || total / width >= 4;
        expected[10] |= unorderedPresent ? (signaling >>> predicate) & 1 : 0;
        run(
          "predicate",
          `${op}/${predicate}/${memory}/${d}`,
          op,
          registers(d, a, b, memory),
          predicate,
          memory ? total : 0,
          before,
          expected,
        );
        if (d === 0)
          run(
            "ignored-immediate-bits",
            `${op}/${predicate}/${memory}`,
            op,
            registers(d, a, b, memory),
            predicate | 0xe0,
            memory ? total : 0,
            before,
            expected,
          );
      }
    }
  }
}

for (let op = 6; op <= 9; op++) {
  const width = op % 2 ? 8 : 4;
  const one = width === 4 ? 0x3f800000n : 0x3ff0000000000000n;
  const two = width === 4 ? 0x40000000n : 0x4000000000000000n;
  const qnan = width === 4 ? 0x7fc00001n : 0x7ff8000000000001n;
  const snan = width === 4 ? 0x7f800001n : 0x7ff0000000000001n;
  for (const [a, b, flags] of [
    [one, two, 1],
    [two, one, 0],
    [one, one, 0x40],
    [qnan, one, 0x45],
    [snan, one, 0x45],
  ]) {
    for (const memory of [false, true])
      for (const unmasked of [false, true]) {
        const before = state(unmasked ? 0x1f00 : 0x1f80);
        publish(before, 1, lanes([a], width));
        source(before, 2, lanes([b], width), memory, width);
        const invalid = a === snan || (a === qnan && op >= 8);
        const fault = invalid && unmasked;
        const expected = before.slice();
        expected[10] |= Number(invalid);
        if (!fault) expected[9] = ((before[9] & ~0x8d5) | flags) >>> 0;
        run(
          "flags",
          `${op}/${a.toString(16)}/${memory}/${unmasked}`,
          op,
          registers(0, 1, 2, memory),
          0,
          memory ? width : 0,
          before,
          expected,
          fault ? 3 : 0,
        );
      }
  }
}

const packed = [
  [10, 4, 8, 2, 0xbfc00000n, 0xbff8000000000000n, 0],
  [11, 4, 8, 4, 0xbfc00000n, 0xbff8000000000000n, 0],
  [12, 8, 4, 2, 0xbff8000000000000n, 0xbfc00000n, 0],
  [13, 8, 4, 4, 0xbff8000000000000n, 0xbfc00000n, 0],
  [16, 4, 4, 4, -2n, 0xc0000000n, 0],
  [17, 4, 4, 8, -2n, 0xc0000000n, 0],
  [18, 4, 4, 4, 0xbfc00000n, -2n, 0x20],
  [19, 4, 4, 8, 0xbfc00000n, -2n, 0x20],
  [20, 4, 4, 4, 0xbfc00000n, -1n, 0x20],
  [21, 4, 4, 8, 0xbfc00000n, -1n, 0x20],
  [28, 8, 4, 2, 0xbff8000000000000n, -1n, 0x20],
  [29, 8, 4, 4, 0xbff8000000000000n, -1n, 0x20],
  [30, 4, 8, 2, -2n, 0xc000000000000000n, 0],
  [31, 4, 8, 4, -2n, 0xc000000000000000n, 0],
  [32, 8, 4, 2, 0xbff8000000000000n, -2n, 0x20],
  [33, 8, 4, 4, 0xbff8000000000000n, -2n, 0x20],
];
for (const [op, inputWidth, outputWidth, count, raw, literal, flags] of packed) {
  for (const memory of [false, true])
    for (const destination of [0, 2]) {
      const before = state();
      source(before, 2, lanes([raw], inputWidth), memory, count * inputWidth);
      const expected = before.slice();
      const result = Buffer.alloc(32);
      lanes([literal], outputWidth).copy(result, 0, 0, count * outputWidth);
      publish(expected, destination, result);
      expected[10] |= flags;
      run(
        "packed-conversion",
        `${op}/${memory}/${destination}`,
        op,
        registers(destination, 0, 2, memory),
        0,
        memory ? count * inputWidth : 0,
        before,
        expected,
      );
    }
}

for (const [op, inputWidth, outputWidth, raw, literal] of [
  [0, 4, 4, -2n, 0xc0000000n],
  [1, 4, 8, -2n, 0xc000000000000000n],
  [14, 4, 8, 0xbfc00000n, 0xbff8000000000000n],
  [15, 8, 4, 0xbff8000000000000n, 0xbfc00000n],
]) {
  for (const memory of [false, true])
    for (const [d, a, b] of [
      [0, 1, 2],
      [1, 1, 2],
      [2, 1, 2],
      [1, 1, 1],
    ]) {
      const before = state();
      if (op < 2 && !memory) before[b] = Number(BigInt.asUintN(32, raw));
      else source(before, b, lanes([raw], inputWidth), memory, inputWidth);
      const expected = before.slice();
      const result = vector(before, a);
      lanes([literal], outputWidth).copy(result, 0, 0, outputWidth);
      result.fill(0, 16);
      publish(expected, d, result);
      run(
        "scalar-merge",
        `${op}/${memory}/${d}/${a}/${b}`,
        op,
        registers(d, a, b, memory),
        0,
        memory ? inputWidth : 0,
        before,
        expected,
      );
    }
}

for (let op = 2; op <= 5; op++) {
  const width = op % 2 ? 8 : 4;
  for (const [raw, nearest, down, up, zero] of width === 4
    ? [
        [0x3fc00000n, 2, 1, 2, 1],
        [0xbfc00000n, -2, -2, -1, -1],
      ]
    : [
        [0x3ff8000000000000n, 2, 1, 2, 1],
        [0xbff8000000000000n, -2, -2, -1, -1],
      ]) {
    for (let rounding = 0; rounding < 4; rounding++)
      for (const memory of [false, true]) {
        const before = state(0x1f80 | (rounding << 13));
        source(before, 2, lanes([raw], width), memory, width);
        const expected = before.slice();
        expected[0] = [nearest, down, up, zero][op < 4 ? 3 : rounding] >>> 0;
        expected[10] |= 0x20;
        run(
          "integer-rounding",
          `${op}/${raw.toString(16)}/${rounding}/${memory}`,
          op,
          registers(0, 0, 2, memory),
          0,
          memory ? width : 0,
          before,
          expected,
        );
      }
  }
}

for (const [integer, nearest, directed] of [
  [0x01000001, 0x4b800000n, 0x4b800001n],
  [0xfeffffff, 0xcb800000n, 0xcb800001n],
]) {
  for (let rounding = 0; rounding < 4; rounding++)
    for (const memory of [false, true]) {
      const before = state(0x1f80 | (rounding << 13));
      if (memory) source(before, 2, lanes([BigInt(integer)], 4), true, 4);
      else before[2] = integer;
      const expected = before.slice();
      const result = vector(before, 1);
      const literal = rounding === (integer === 0x01000001 ? 2 : 1) ? directed : nearest;
      lanes([literal], 4).copy(result, 0, 0, 4);
      result.fill(0, 16);
      publish(expected, 0, result);
      expected[10] |= 0x20;
      run(
        "integer-to-single-rounding",
        `${integer}/${rounding}/${memory}`,
        0,
        registers(0, 1, 2, memory),
        0,
        memory ? 4 : 0,
        before,
        expected,
      );
    }
}
for (const [op, inputWidth, outputWidth] of [
  [26, 4, 4],
  [27, 8, 8],
  [11, 4, 8],
]) {
  for (const daz of [false, true]) {
    const before = state(daz ? 0x1fc0 : 0x1f80);
    publish(before, 1, lanes([1n], inputWidth));
    publish(before, 2, Buffer.alloc(32));
    const expected = before.slice();
    const result = op === 11 ? Buffer.alloc(32) : vector(before, 1);
    if (op === 11) {
      publish(before, 2, lanes([1n], inputWidth));
      source(expected, 2, lanes([1n], inputWidth), false, 0);
      lanes([daz ? 0n : 0x36a0000000000000n], 8).copy(result);
    } else {
      lanes([daz ? -1n : 0n], outputWidth).copy(result, 0, 0, outputWidth);
      result.fill(0, 16);
    }
    publish(expected, 0, result);
    expected[10] |= daz ? 0 : 2;
    run(
      "daz",
      `${op}/${daz}`,
      op,
      registers(0, op === 11 ? 0 : 1, 2, false),
      0,
      0,
      before,
      expected,
    );
  }
}

const narrowing = [
  [0x36a0000000000000n, 0x1f80, 1n, 0, false],
  [0x36a0000000000000n, 0x9f80, 0n, 0x30, false],
  [0x3690000000000000n, 0x1f80, 0n, 0x30, false],
  [0x3690000000000000n, 0x5f80, 1n, 0x30, false],
  [0x36a0000000000000n, 0x1780, 1n, 0x10, true],
  [0x47f0000000000000n, 0x1f80, 0x7f800000n, 0x28, false],
  [0x47f0000000000000n, 0x7f80, 0x7f7fffffn, 0x28, false],
  [0x47f0000000000000n, 0x1b80, 0x7f800000n, 8, true],
  [0x7ff0000020000000n, 0x1f80, 0x7fc00001n, 1, false],
];
for (const [raw, mxcsr, literal, flags, fault] of narrowing)
  for (const memory of [false, true]) {
    const before = state(mxcsr);
    source(before, 2, lanes([raw], 8), memory, 32);
    const expected = before.slice();
    expected[10] |= flags;
    if (!fault) {
      const result = Buffer.alloc(32);
      lanes([literal], 4).copy(result, 0, 0, 16);
      publish(expected, 0, result);
    }
    run(
      "narrowing",
      `${raw.toString(16)}/${mxcsr}/${memory}`,
      13,
      registers(0, 0, 2, memory),
      0,
      memory ? 32 : 0,
      before,
      expected,
      fault ? 3 : 0,
    );
  }

for (const [op, width, raw, literal, flags] of [
  [11, 4, 1n, 0x36a0000000000000n, 2],
  [19, 4, 0x7fc00001n, 0x80000000n, 1],
  [29, 8, 0x7ff0000000000000n, 0x80000000n, 1],
]) {
  for (const memory of [false, true]) {
    const count = op === 29 ? 4 : op === 11 ? 4 : 8;
    const before = state();
    source(before, 2, lanes([raw], width), memory, count * width);
    const expected = before.slice();
    const result = Buffer.alloc(32);
    const outputWidth = op === 11 ? 8 : 4;
    lanes([literal], outputWidth).copy(result, 0, 0, count * outputWidth);
    publish(expected, 0, result);
    expected[10] |= flags;
    run(
      "special-input",
      `${op}/${memory}`,
      op,
      registers(0, 0, 2, memory),
      0,
      memory ? count * width : 0,
      before,
      expected,
    );
  }
}

for (const op of [23, 19]) {
  const before = state(0x1f00);
  publish(before, 1, Buffer.alloc(32));
  publish(
    before,
    2,
    lanes([0x3fc00000n, 0, 0, 0, 0, 0, 0, op === 23 ? 0x7f800001n : 0x7fc00001n], 4),
  );
  const expected = before.slice();
  expected[10] = 0x1f01;
  run(
    "all-lane-fault",
    `${op}`,
    op,
    registers(0, op === 23 ? 1 : 0, 2, false),
    0,
    0,
    before,
    expected,
    3,
  );
}
for (const [op, length] of [
  [0, 4],
  [1, 4],
  [2, 4],
  [3, 8],
  [14, 4],
  [15, 8],
  [10, 8],
  [11, 16],
  [12, 16],
  [13, 32],
  [22, 16],
  [23, 32],
  [26, 4],
  [27, 8],
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
  const invalid = state(0x11f80);
  run("invalid-state", `${op}`, op, registers(0, 1, 0, true), 0, length, invalid, invalid, 2);
}
for (const [op, regs, imm, len] of [
  [38, 0, 0, 0],
  [23, 8, 0, 0],
  [23, 8 << 8, 0, 0],
  [23, 8 << 16, 0, 0],
  [23, 1 << 25, 0, 0],
  [23, 0, 256, 0],
  [10, 0, 1, 0],
  [23, 0, 0, 33],
]) {
  const before = state();
  run("invalid-request", `${op}/${regs}/${imm}/${len}`, op, regs, imm, len, before, before, 1);
}

const sourceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../../../../../..");
const sourcePaths = [
  "src/abi/wasm/avx_proof.rs",
  "src/cpu/x86/avx/compare/mod.rs",
  "src/cpu/x86/avx/convert/mod.rs",
  "src/cpu/x86/simd/float.rs",
  "src/math/ieee.rs",
].map((path) => [path, sha256(readFileSync(resolve(sourceRoot, path)))]);
const receipt = {
  backend: binaryPath,
  backendSha256: sha256(binary),
  runs,
  generatedModules: calls.size,
  groups,
  sourcePaths,
  numericExecution:
    "real Rust-compiled Wasm CPU kernels through a direct generated Wasm import; JavaScript contains literal oracles only",
  publicAVX: false,
  node: process.version,
};
if (process.argv[3]) {
  const output = resolve(process.argv[3]);
  mkdirSync(output, { recursive: true });
  writeFileSync(resolve(output, "proof.json"), JSON.stringify(receipt, null, 2) + "\n");
}
console.log(JSON.stringify(receipt));
