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


const forms = [
  {
    "id": 113,
    "code": "VEX_Vsqrtps_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "SquareRoot",
    "width": 16,
    "element": 4,
    "scalar": false,
    "length": 16
  },
  {
    "id": 114,
    "code": "VEX_Vsqrtps_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "SquareRoot",
    "width": 32,
    "element": 4,
    "scalar": false,
    "length": 32
  },
  {
    "id": 115,
    "code": "VEX_Vsqrtpd_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "SquareRoot",
    "width": 16,
    "element": 8,
    "scalar": false,
    "length": 16
  },
  {
    "id": 116,
    "code": "VEX_Vsqrtpd_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "SquareRoot",
    "width": 32,
    "element": 8,
    "scalar": false,
    "length": 32
  },
  {
    "id": 117,
    "code": "VEX_Vsqrtss_xmm_xmm_xmmm32",
    "group": "floating_elementwise",
    "kind": "SquareRoot",
    "width": 16,
    "element": 4,
    "scalar": true,
    "length": 4
  },
  {
    "id": 118,
    "code": "VEX_Vsqrtsd_xmm_xmm_xmmm64",
    "group": "floating_elementwise",
    "kind": "SquareRoot",
    "width": 16,
    "element": 8,
    "scalar": true,
    "length": 8
  },
  {
    "id": 119,
    "code": "VEX_Vrsqrtps_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "ReciprocalSquareRoot",
    "width": 16,
    "element": 4,
    "scalar": false,
    "length": 16
  },
  {
    "id": 120,
    "code": "VEX_Vrsqrtps_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "ReciprocalSquareRoot",
    "width": 32,
    "element": 4,
    "scalar": false,
    "length": 32
  },
  {
    "id": 121,
    "code": "VEX_Vrsqrtss_xmm_xmm_xmmm32",
    "group": "floating_elementwise",
    "kind": "ReciprocalSquareRoot",
    "width": 16,
    "element": 4,
    "scalar": true,
    "length": 4
  },
  {
    "id": 122,
    "code": "VEX_Vrcpps_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Reciprocal",
    "width": 16,
    "element": 4,
    "scalar": false,
    "length": 16
  },
  {
    "id": 123,
    "code": "VEX_Vrcpps_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Reciprocal",
    "width": 32,
    "element": 4,
    "scalar": false,
    "length": 32
  },
  {
    "id": 124,
    "code": "VEX_Vrcpss_xmm_xmm_xmmm32",
    "group": "floating_elementwise",
    "kind": "Reciprocal",
    "width": 16,
    "element": 4,
    "scalar": true,
    "length": 4
  },
  {
    "id": 125,
    "code": "VEX_Vaddps_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Add",
    "width": 16,
    "element": 4,
    "scalar": false,
    "length": 16
  },
  {
    "id": 126,
    "code": "VEX_Vaddps_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Add",
    "width": 32,
    "element": 4,
    "scalar": false,
    "length": 32
  },
  {
    "id": 127,
    "code": "VEX_Vaddpd_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Add",
    "width": 16,
    "element": 8,
    "scalar": false,
    "length": 16
  },
  {
    "id": 128,
    "code": "VEX_Vaddpd_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Add",
    "width": 32,
    "element": 8,
    "scalar": false,
    "length": 32
  },
  {
    "id": 129,
    "code": "VEX_Vaddss_xmm_xmm_xmmm32",
    "group": "floating_elementwise",
    "kind": "Add",
    "width": 16,
    "element": 4,
    "scalar": true,
    "length": 4
  },
  {
    "id": 130,
    "code": "VEX_Vaddsd_xmm_xmm_xmmm64",
    "group": "floating_elementwise",
    "kind": "Add",
    "width": 16,
    "element": 8,
    "scalar": true,
    "length": 8
  },
  {
    "id": 131,
    "code": "VEX_Vmulps_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Multiply",
    "width": 16,
    "element": 4,
    "scalar": false,
    "length": 16
  },
  {
    "id": 132,
    "code": "VEX_Vmulps_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Multiply",
    "width": 32,
    "element": 4,
    "scalar": false,
    "length": 32
  },
  {
    "id": 133,
    "code": "VEX_Vmulpd_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Multiply",
    "width": 16,
    "element": 8,
    "scalar": false,
    "length": 16
  },
  {
    "id": 134,
    "code": "VEX_Vmulpd_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Multiply",
    "width": 32,
    "element": 8,
    "scalar": false,
    "length": 32
  },
  {
    "id": 135,
    "code": "VEX_Vmulss_xmm_xmm_xmmm32",
    "group": "floating_elementwise",
    "kind": "Multiply",
    "width": 16,
    "element": 4,
    "scalar": true,
    "length": 4
  },
  {
    "id": 136,
    "code": "VEX_Vmulsd_xmm_xmm_xmmm64",
    "group": "floating_elementwise",
    "kind": "Multiply",
    "width": 16,
    "element": 8,
    "scalar": true,
    "length": 8
  },
  {
    "id": 137,
    "code": "VEX_Vsubps_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Subtract",
    "width": 16,
    "element": 4,
    "scalar": false,
    "length": 16
  },
  {
    "id": 138,
    "code": "VEX_Vsubps_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Subtract",
    "width": 32,
    "element": 4,
    "scalar": false,
    "length": 32
  },
  {
    "id": 139,
    "code": "VEX_Vsubpd_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Subtract",
    "width": 16,
    "element": 8,
    "scalar": false,
    "length": 16
  },
  {
    "id": 140,
    "code": "VEX_Vsubpd_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Subtract",
    "width": 32,
    "element": 8,
    "scalar": false,
    "length": 32
  },
  {
    "id": 141,
    "code": "VEX_Vsubss_xmm_xmm_xmmm32",
    "group": "floating_elementwise",
    "kind": "Subtract",
    "width": 16,
    "element": 4,
    "scalar": true,
    "length": 4
  },
  {
    "id": 142,
    "code": "VEX_Vsubsd_xmm_xmm_xmmm64",
    "group": "floating_elementwise",
    "kind": "Subtract",
    "width": 16,
    "element": 8,
    "scalar": true,
    "length": 8
  },
  {
    "id": 143,
    "code": "VEX_Vminps_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Minimum",
    "width": 16,
    "element": 4,
    "scalar": false,
    "length": 16
  },
  {
    "id": 144,
    "code": "VEX_Vminps_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Minimum",
    "width": 32,
    "element": 4,
    "scalar": false,
    "length": 32
  },
  {
    "id": 145,
    "code": "VEX_Vminpd_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Minimum",
    "width": 16,
    "element": 8,
    "scalar": false,
    "length": 16
  },
  {
    "id": 146,
    "code": "VEX_Vminpd_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Minimum",
    "width": 32,
    "element": 8,
    "scalar": false,
    "length": 32
  },
  {
    "id": 147,
    "code": "VEX_Vminss_xmm_xmm_xmmm32",
    "group": "floating_elementwise",
    "kind": "Minimum",
    "width": 16,
    "element": 4,
    "scalar": true,
    "length": 4
  },
  {
    "id": 148,
    "code": "VEX_Vminsd_xmm_xmm_xmmm64",
    "group": "floating_elementwise",
    "kind": "Minimum",
    "width": 16,
    "element": 8,
    "scalar": true,
    "length": 8
  },
  {
    "id": 149,
    "code": "VEX_Vdivps_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Divide",
    "width": 16,
    "element": 4,
    "scalar": false,
    "length": 16
  },
  {
    "id": 150,
    "code": "VEX_Vdivps_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Divide",
    "width": 32,
    "element": 4,
    "scalar": false,
    "length": 32
  },
  {
    "id": 151,
    "code": "VEX_Vdivpd_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Divide",
    "width": 16,
    "element": 8,
    "scalar": false,
    "length": 16
  },
  {
    "id": 152,
    "code": "VEX_Vdivpd_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Divide",
    "width": 32,
    "element": 8,
    "scalar": false,
    "length": 32
  },
  {
    "id": 153,
    "code": "VEX_Vdivss_xmm_xmm_xmmm32",
    "group": "floating_elementwise",
    "kind": "Divide",
    "width": 16,
    "element": 4,
    "scalar": true,
    "length": 4
  },
  {
    "id": 154,
    "code": "VEX_Vdivsd_xmm_xmm_xmmm64",
    "group": "floating_elementwise",
    "kind": "Divide",
    "width": 16,
    "element": 8,
    "scalar": true,
    "length": 8
  },
  {
    "id": 155,
    "code": "VEX_Vmaxps_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Maximum",
    "width": 16,
    "element": 4,
    "scalar": false,
    "length": 16
  },
  {
    "id": 156,
    "code": "VEX_Vmaxps_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Maximum",
    "width": 32,
    "element": 4,
    "scalar": false,
    "length": 32
  },
  {
    "id": 157,
    "code": "VEX_Vmaxpd_xmm_xmm_xmmm128",
    "group": "floating_elementwise",
    "kind": "Maximum",
    "width": 16,
    "element": 8,
    "scalar": false,
    "length": 16
  },
  {
    "id": 158,
    "code": "VEX_Vmaxpd_ymm_ymm_ymmm256",
    "group": "floating_elementwise",
    "kind": "Maximum",
    "width": 32,
    "element": 8,
    "scalar": false,
    "length": 32
  },
  {
    "id": 159,
    "code": "VEX_Vmaxss_xmm_xmm_xmmm32",
    "group": "floating_elementwise",
    "kind": "Maximum",
    "width": 16,
    "element": 4,
    "scalar": true,
    "length": 4
  },
  {
    "id": 160,
    "code": "VEX_Vmaxsd_xmm_xmm_xmmm64",
    "group": "floating_elementwise",
    "kind": "Maximum",
    "width": 16,
    "element": 8,
    "scalar": true,
    "length": 8
  },
  {
    "id": 161,
    "code": "VEX_Vandps_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "And",
    "width": 16,
    "length": 16
  },
  {
    "id": 162,
    "code": "VEX_Vandps_ymm_ymm_ymmm256",
    "group": "logical",
    "kind": "And",
    "width": 32,
    "length": 32
  },
  {
    "id": 163,
    "code": "VEX_Vandpd_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "And",
    "width": 16,
    "length": 16
  },
  {
    "id": 164,
    "code": "VEX_Vandpd_ymm_ymm_ymmm256",
    "group": "logical",
    "kind": "And",
    "width": 32,
    "length": 32
  },
  {
    "id": 165,
    "code": "VEX_Vandnps_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "AndNot",
    "width": 16,
    "length": 16
  },
  {
    "id": 166,
    "code": "VEX_Vandnps_ymm_ymm_ymmm256",
    "group": "logical",
    "kind": "AndNot",
    "width": 32,
    "length": 32
  },
  {
    "id": 167,
    "code": "VEX_Vandnpd_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "AndNot",
    "width": 16,
    "length": 16
  },
  {
    "id": 168,
    "code": "VEX_Vandnpd_ymm_ymm_ymmm256",
    "group": "logical",
    "kind": "AndNot",
    "width": 32,
    "length": 32
  },
  {
    "id": 169,
    "code": "VEX_Vorps_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "Or",
    "width": 16,
    "length": 16
  },
  {
    "id": 170,
    "code": "VEX_Vorps_ymm_ymm_ymmm256",
    "group": "logical",
    "kind": "Or",
    "width": 32,
    "length": 32
  },
  {
    "id": 171,
    "code": "VEX_Vorpd_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "Or",
    "width": 16,
    "length": 16
  },
  {
    "id": 172,
    "code": "VEX_Vorpd_ymm_ymm_ymmm256",
    "group": "logical",
    "kind": "Or",
    "width": 32,
    "length": 32
  },
  {
    "id": 173,
    "code": "VEX_Vxorps_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "Xor",
    "width": 16,
    "length": 16
  },
  {
    "id": 174,
    "code": "VEX_Vxorps_ymm_ymm_ymmm256",
    "group": "logical",
    "kind": "Xor",
    "width": 32,
    "length": 32
  },
  {
    "id": 175,
    "code": "VEX_Vxorpd_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "Xor",
    "width": 16,
    "length": 16
  },
  {
    "id": 176,
    "code": "VEX_Vxorpd_ymm_ymm_ymmm256",
    "group": "logical",
    "kind": "Xor",
    "width": 32,
    "length": 32
  },
  {
    "id": 177,
    "code": "VEX_Vpand_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "And",
    "width": 16,
    "length": 16
  },
  {
    "id": 178,
    "code": "VEX_Vpandn_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "AndNot",
    "width": 16,
    "length": 16
  },
  {
    "id": 179,
    "code": "VEX_Vpor_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "Or",
    "width": 16,
    "length": 16
  },
  {
    "id": 180,
    "code": "VEX_Vpxor_xmm_xmm_xmmm128",
    "group": "logical",
    "kind": "Xor",
    "width": 16,
    "length": 16
  }
];

const literal = {
  Add: [0x40c00000n, 0x4018000000000000n],
  Subtract: [0xc0000000n, 0xc000000000000000n],
  Multiply: [0x41000000n, 0x4020000000000000n],
  Divide: [0x3f000000n, 0x3fe0000000000000n],
  Minimum: [0x40000000n, 0x4000000000000000n],
  Maximum: [0x40800000n, 0x4010000000000000n],
  SquareRoot: [0x40000000n, 0x4000000000000000n],
  Reciprocal: [0x3e800000n, 0n],
  ReciprocalSquareRoot: [0x3f000000n, 0n],
};
function floatInput(element, four) {
  return lanes([element === 4 ? (four ? 0x40800000n : 0x40000000n) : (four ? 0x4010000000000000n : 0x4000000000000000n)], element);
}
for (const form of forms) {
  for (const memory of [false, true]) {
    for (const [d, a, b] of [[0, 1, 2], [1, 1, 2], [2, 1, 2]]) {
      const before = state();
      let result;
      if (form.group === 'floating_elementwise') {
        publish(before, a, floatInput(form.element, false));
        source(before, b, floatInput(form.element, true), memory, form.length);
        result = form.scalar ? vector(before, a) : Buffer.alloc(32);
        const raw = literal[form.kind][form.element === 4 ? 0 : 1];
        const count = form.scalar ? 1 : form.width / form.element;
        for (let i = 0; i < count; i++) {
          if (form.element === 4) result.writeUInt32LE(Number(raw), i * 4);
          else result.writeBigUInt64LE(raw, i * 8);
        }
        if (form.scalar) result.fill(0, 16);
      } else {
        const left = Buffer.from(Array.from({length: 32}, (_, i) => (i * 37 + 0x71) & 255));
        const right = Buffer.from(Array.from({length: 32}, (_, i) => (i * 19 + 0xb3) & 255));
        publish(before, a, left);
        source(before, b, right, memory, form.length);
        result = Buffer.alloc(32);
        for (let i = 0; i < form.width; i++) result[i] = form.kind === 'And' ? left[i] & right[i] : form.kind === 'AndNot' ? ~left[i] & right[i] : form.kind === 'Or' ? left[i] | right[i] : left[i] ^ right[i];
      }
      const expected = before.slice();
      publish(expected, d, result);
      run('literal-and-alias', `${form.code}/${memory}/${d}`, form.id, registers(d, a, b, memory), 0, memory ? form.length : 0, before, expected);
    }
  }
  for (const wrong of [0, form.length - 1, form.length + 1]) {
    const before = state();
    run('invalid-length', `${form.code}/${wrong}`, form.id, registers(0, 1, 2, true), 0, wrong, before, before, 1);
  }
  for (const memory of [false, true]) {
    const before = state(0x11f80);
    run('invalid-state', `${form.code}/${memory}`, form.id, registers(0, 1, 2, memory), 0, memory ? form.length : 0, before, before, 2);
  }
  const before = state();
  run('register-extra-operand', form.code, form.id, registers(0, 1, 2, false), 0, 1, before, before, 1);
}
// scalar unary numeric input is SRC2; SRC1 contributes only the merge bytes.
for (const form of forms.filter((f) => f.scalar && ['SquareRoot', 'Reciprocal', 'ReciprocalSquareRoot'].includes(f.kind))) {
  for (const memory of [false, true]) {
    const before = state(0x1f00);
    const snan = form.element === 4 ? 0x7f800001n : 0x7ff0000000000001n;
    publish(before, 1, lanes([snan], form.element));
    const input = lanes([snan], form.element);
    const four = floatInput(form.element, true);
    four.copy(input, 0, 0, form.element);
    // unconsumed adjacent staged words deliberately remain signaling NaNs.
    for (let i = 0; i < 8; i++) before[76 + i] = input.readUInt32LE(i * 4);
    source(before, 2, input, memory, form.length);
    const expected = before.slice(), result = vector(before, 1);
    const raw = literal[form.kind][form.element === 4 ? 0 : 1];
    if (form.element === 4) result.writeUInt32LE(Number(raw), 0);
    else result.writeBigUInt64LE(raw, 0);
    result.fill(0, 16);
    publish(expected, 1, result);
    run('scalar-unrelated-lanes', `${form.code}/${memory}`, form.id, registers(1, 1, 2, memory), 0, memory ? form.length : 0, before, expected);
  }
}
const add256 = forms.find((f) => f.kind === 'Add' && f.element === 4 && f.width === 32);
for (const memory of [false, true]) {
  for (const [mxcsr, last, after] of [[0x1f00, 0x7f800001n, 0x1f01], [0x0f80, 0x4b800000n, 0x0fa0]]) {
    const before = state(mxcsr);
    publish(before, 1, lanes([0x3f800000n], 4));
    const input = lanes([0x3f800000n], 4);
    input.writeUInt32LE(Number(last), 28);
    source(before, 2, input, memory, 32);
    const expected = before.slice(); expected[10] = after;
    run('upper-lane-unmasked', `${memory}/${mxcsr}`, add256.id, registers(1, 1, 2, memory), 0, memory ? 32 : 0, before, expected, 3);
  }
}
for (const [op, regs, imm, len] of [[181, 0, 0, 0], [113, 8, 0, 0], [113, 1 << 25, 0, 0], [113, 0, 1, 0], [113, 0, 0, 33]]) {
  const before = state();
  run('invalid-request', `${op}/${regs}/${imm}/${len}`, op, regs, imm, len, before, before, 1);
}
const sourceRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../../../../../../..');
const sourcePaths = [
  'src/abi/wasm/avx_proof.rs',
  'src/abi/wasm/avx_proof/foundation.rs',
  'src/cpu/x86/avx/mod.rs',
  'src/cpu/x86/avx/arithmetic.rs',
  'src/cpu/x86/avx/arithmetic/memory/fixtures/runtime/run.mjs',
  'src/cpu/x86/simd.rs',
  'src/cpu/x86/simd/float.rs',
  'src/math/ieee.rs',
].map((path) => [path, sha256(readFileSync(resolve(sourceRoot, path)))]);
const receipt = {
  backend: binaryPath, backendSha256: sha256(binary), runs, generatedModules: calls.size, groups, sourcePaths,
  numericExecution: 'real Rust-compiled Wasm shared CPU/math semantics through direct generated Wasm import; JavaScript literal oracles',
  boundary: 'exact checked captured operand; guest segmentation/paging and production helper integration remain root dependencies',
  publicAVX: false, node: process.version,
};
if (process.argv[3]) {
  const output = resolve(process.argv[3]); mkdirSync(output, {recursive: true});
  writeFileSync(resolve(output, 'proof.json'), JSON.stringify(receipt, null, 2) + '\n');
}
console.log(JSON.stringify(receipt));
