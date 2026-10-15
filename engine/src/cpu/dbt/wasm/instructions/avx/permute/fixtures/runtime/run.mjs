import assert from 'node:assert/strict';
let input = '';
for await (const chunk of process.stdin) input += chunk;
const cases = JSON.parse(input);
const STATE = 256,
  LOWER = STATE + 64,
  UPPER = STATE + 256,
  GENERAL = STATE,
  MXCSR = STATE + 32;
function requirement(c) {
  if (c.tag === 'extract') return c.destination === 'memory' ? 4 : 0;
  if (c.second !== 'memory') return 0;
  return c.tag === 'insert' ? 4 : c.width;
}
for (const [index, c] of cases.entries()) {
  const label = `${index}:${c.tag}:${c.element ?? ''}:${c.width ?? ''}:${c.immediate ?? ''}:${c.address}`;
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1 }),
    bytes = new Uint8Array(memory.buffer);
  for (let i = 0; i < bytes.length; i++) bytes[i] = (i * 37 + (i >>> 6) * 53 + 19) & 255;
  const write = new DataView(memory.buffer);
  write.setUint32(MXCSR, c.invalidState ? 0x10000 : 0xe5af, true);
  write.setUint32(LOWER + 16, 0x7f800001, true);
  write.setUint32(LOWER + 20, 0x80000000, true);
  const length = requirement(c);
  assert.equal(c.memoryBytes, length, `${label}: full operand width`);
  if (c.tag === 'variable') {
    const control = new Uint8Array(c.width).fill(0xff);
    for (let lane = 0; lane < c.width / c.element; lane++)
      control[lane * c.element] = (c.pattern + lane) % 4;
    if (c.second === 'memory') {
      if (c.address + length <= 65536) bytes.set(control, c.address);
    } else {
      bytes.set(control.subarray(0, 16), LOWER + c.second * 16);
      if (c.width === 32) bytes.set(control.subarray(16), UPPER + c.second * 16);
    }
  }
  if (c.tag === 'blend_variable') {
    const mask = new Uint8Array(32).fill(0xff);
    for (let lane = 0; lane < 32 / c.element; lane++) {
      const sign =
        c.pattern === 0
          ? false
          : c.pattern === 1
            ? true
            : c.pattern === 2
              ? lane % 2 === 0
              : lane % 2 !== 0;
      mask[(lane + 1) * c.element - 1] = sign ? 0x80 : 0x7f;
    }
    bytes.set(mask.subarray(0, 16), LOWER + c.mask * 16);
    bytes.set(mask.subarray(16), UPPER + c.mask * 16);
  }
  const before = bytes.slice(),
    expected = before.slice();
  const vector = (r) =>
    Uint8Array.from([
      ...before.slice(LOWER + r * 16, LOWER + (r + 1) * 16),
      ...before.slice(UPPER + r * 16, UPPER + (r + 1) * 16),
    ]);
  const second = () =>
    c.second === 'memory' ? before.slice(c.address, c.address + length) : vector(c.second);
  const publish = (value) => {
    const result = new Uint8Array(32);
    result.set(value);
    expected.set(result.subarray(0, 16), LOWER + c.destination * 16);
    expected.set(result.subarray(16), UPPER + c.destination * 16);
  };
  const status = c.invalidState ? 4 : length && c.address > 65536 - length ? 3 : 0;
  if (!status) {
    if (c.tag === 'extract') {
      const value = vector(c.first).slice((c.immediate & 3) * 4, (c.immediate & 3) * 4 + 4);
      expected.set(value, c.destination === 'memory' ? c.address : GENERAL + c.destination * 4);
    } else if (c.tag === 'insert') {
      const value = vector(c.first).slice(0, 16),
        source = second();
      const at = c.second === 'memory' ? 0 : (c.immediate >>> 6) * 4;
      value.set(source.subarray(at, at + 4), ((c.immediate >>> 4) & 3) * 4);
      for (let lane = 0; lane < 4; lane++)
        if (c.immediate & (1 << lane)) value.fill(0, lane * 4, lane * 4 + 4);
      publish(value);
    } else {
      const a = vector(c.first),
        b = second(),
        value = new Uint8Array(c.width),
        element = c.element,
        lanes = 16 / element;
      for (let lane = 0; lane < c.width / element; lane++) {
        const region = Math.floor(lane / lanes) * 16,
          local = lane % lanes;
        let source, selected;
        switch (c.tag) {
          case 'unpack_low':
          case 'unpack_high':
            source = local % 2 ? b : a;
            selected = Math.floor(local / 2) + (c.tag === 'unpack_high' ? lanes / 2 : 0);
            break;
          case 'shuffle':
            source = local < lanes / 2 ? a : b;
            selected =
              element === 4 ? (c.immediate >>> (local * 2)) & 3 : (c.immediate >>> lane) & 1;
            break;
          case 'immediate':
            source = b;
            selected =
              element === 4 ? (c.immediate >>> (local * 2)) & 3 : (c.immediate >>> lane) & 1;
            break;
          case 'variable':
            source = a;
            selected = element === 4 ? b[lane * element] & 3 : (b[lane * element] >>> 1) & 1;
            break;
          case 'blend':
            source = c.immediate & (1 << lane) ? b : a;
            selected = local;
            break;
          case 'blend_variable':
            source = vector(c.mask)[(lane + 1) * element - 1] & 128 ? b : a;
            selected = local;
            break;
          default:
            throw new Error(`unhandled ${label}`);
        }
        value.set(
          source.subarray(region + selected * element, region + (selected + 1) * element),
          lane * element,
        );
      }
      publish(value);
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
  assert.deepEqual(bytes, expected, `${label}: all64KiB state/result/fault neutrality`);
}
console.log(`${cases.length} actual permutation modules passed`);
