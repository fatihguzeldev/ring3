import assert from 'node:assert/strict';
let input = '';
for await (const chunk of process.stdin) input += chunk;
const cases = JSON.parse(input);
const STATE = 256,
  LOWER = STATE + 64,
  UPPER = STATE + 256,
  MXCSR = STATE + 32;
function write(bytes, at, width, value) {
  for (let b = 0; b < width; b++) bytes[at + b] = Number((value >> BigInt(b * 8)) & 255n);
}
function read(bytes, at, width) {
  let result = 0n;
  for (let b = width - 1; b >= 0; b--) result = (result << 8n) | BigInt(bytes[at + b]);
  return result;
}
function pattern(width, seed, bytes = 16) {
  const modulus = 1n << BigInt(width * 8),
    sign = modulus / 2n,
    mask = modulus - 1n;
  const values = [0n, mask, sign, sign - 1n, 1n, mask - 1n, sign + 1n, mask / 3n],
    result = new Uint8Array(bytes);
  for (let lane = 0; lane < bytes / width; lane++)
    write(result, lane * width, width, values[(lane + seed) % 8]);
  return result;
}
function expectedLane(kind, width, a, b) {
  const modulus = 1n << BigInt(width * 8),
    sign = modulus / 2n;
  const signed = (n) => (n >= sign ? n - modulus : n);
  const sa = signed(a),
    sb = signed(b);
  const min = (a, b) => (a < b ? a : b),
    max = (a, b) => (a > b ? a : b);
  const clamp = (v) => min(max(v, -sign), sign - 1n);
  let result;
  switch (kind) {
    case 'Add':
      result = a + b;
      break;
    case 'Subtract':
      result = a - b;
      break;
    case 'AddSignedSaturate':
      result = clamp(sa + sb);
      break;
    case 'SubtractSignedSaturate':
      result = clamp(sa - sb);
      break;
    case 'AddUnsignedSaturate':
      result = min(a + b, modulus - 1n);
      break;
    case 'SubtractUnsignedSaturate':
      result = max(a - b, 0n);
      break;
    case 'MinimumSigned':
      result = min(sa, sb);
      break;
    case 'MaximumSigned':
      result = max(sa, sb);
      break;
    case 'MinimumUnsigned':
      result = min(a, b);
      break;
    case 'MaximumUnsigned':
      result = max(a, b);
      break;
    case 'AverageUnsigned':
      result = (a + b + 1n) / 2n;
      break;
    case 'Equal':
      result = a === b ? modulus - 1n : 0n;
      break;
    case 'GreaterSigned':
      result = sa > sb ? modulus - 1n : 0n;
      break;
    case 'Absolute':
      result = sb < 0n ? -sb : sb;
      break;
    case 'Sign':
      result = sb < 0n ? -sa : sb === 0n ? 0n : sa;
      break;
    default:
      throw new Error(`unknown integer operation ${kind}`);
  }
  return BigInt.asUintN(width * 8, result);
}
for (const [index, c] of cases.entries()) {
  const label = `${index}:${c.kind}/${c.width}:aliases${c.destination}/${c.first}/${c.second}:seed${c.seed}:address${c.address}`;
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1 }),
    bytes = new Uint8Array(memory.buffer);
  for (let i = 0; i < bytes.length; i++) bytes[i] = (i * 37 + (i >>> 6) * 53 + 19) & 255;
  new DataView(memory.buffer).setUint32(MXCSR, c.invalidState ? 0x10000 : 0xe5af, true);
  for (let r = 0; r < 8; r++) bytes.set(pattern(c.width, c.seed + r * 3), LOWER + r * 16);
  const vectorBytes = c.vectorBytes;
  assert.ok(vectorBytes === 16 || vectorBytes === 32, `${label}: closed vector width`);
  if (vectorBytes === 32) {
    for (let r = 0; r < 8; r++) {
      const value = pattern(c.width, c.seed + r * 3, 32);
      bytes.set(value.subarray(16), UPPER + r * 16);
    }
  }
  const length = c.second === 'memory' ? vectorBytes : 0;
  assert.equal(c.memoryBytes, length, `${label}: exact full operand`);
  if (length && c.address <= 65536 - length)
    bytes.set(pattern(c.width, c.seed + 7, length), c.address);
  const before = bytes.slice(),
    expected = before.slice();
  const status = c.invalidState ? 4 : length && c.address > 65536 - length ? 3 : 0;
  if (!status) {
    for (let offset = 0; offset < vectorBytes; offset += c.width) {
      const half = offset < 16 ? LOWER : UPPER;
      const within = offset % 16;
      const first = half + c.first * 16 + within;
      const second = c.second === 'memory' ? c.address + offset : half + c.second * 16 + within;
      write(
        expected,
        half + c.destination * 16 + within,
        c.width,
        expectedLane(c.kind, c.width, read(before, first, c.width), read(before, second, c.width)),
      );
    }
    if (vectorBytes === 16) {
      expected.fill(0, UPPER + c.destination * 16, UPPER + (c.destination + 1) * 16);
    }
  }
  const module = new WebAssembly.Module(Buffer.from(c.module, 'hex'));
  assert.deepEqual(
    WebAssembly.Module.imports(module),
    [{ module: 'env', name: 'memory', kind: 'memory' }],
    label,
  );
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: 'run', kind: 'function' }], label);
  const instance = new WebAssembly.Instance(module, { env: { memory } });
  assert.equal(instance.exports.run(STATE, c.address), status, `${label}: status`);
  assert.deepEqual(bytes, expected, `${label}: all64KiB state/result/refusal neutrality`);
}
console.log(`${cases.length} actual integer modules passed`);
