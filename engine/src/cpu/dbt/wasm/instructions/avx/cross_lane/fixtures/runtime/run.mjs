import assert from "node:assert/strict";
let input = "";
for await (const chunk of process.stdin) input += chunk;
const cases = JSON.parse(input);
const STATE = 256,
  LOWER = STATE + 64,
  UPPER = STATE + 256,
  MXCSR = STATE + 32;
function requirement(c) {
  if (c.invalidState) return c.memoryBytes;
  if (c.tag === "broadcast") return c.element;
  if (c.tag === "extract") return c.destination === "memory" ? 16 : 0;
  return c.second === "memory" ? (c.tag === "insert" ? 16 : 32) : 0;
}
for (const [index, c] of cases.entries()) {
  const label = `${index}:${c.tag}:${c.immediate ?? ""}:${c.address}`;
  const module = new WebAssembly.Module(Buffer.from(c.module, "hex"));
  assert.deepEqual(
    WebAssembly.Module.imports(module),
    [{ module: "env", name: "memory", kind: "memory" }],
    label,
  );
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: "run", kind: "function" }], label);
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1 }),
    bytes = new Uint8Array(memory.buffer);
  for (let i = 0; i < bytes.length; i++) bytes[i] = (i * 37 + (i >>> 6) * 53 + 19) & 255;
  const write = new DataView(memory.buffer);
  write.setUint32(MXCSR, c.invalidState ? 0x10000 : 0xffff, true);
  write.setUint32(LOWER + 16, 0x7f800001, true);
  write.setUint32(LOWER + 20, 0x80000000, true);
  const before = bytes.slice(),
    expected = before.slice(),
    length = requirement(c);
  assert.equal(c.memoryBytes, length, label + " exact operand width");
  const vector = (r) =>
    Uint8Array.from([
      ...before.slice(LOWER + r * 16, LOWER + (r + 1) * 16),
      ...before.slice(UPPER + r * 16, UPPER + (r + 1) * 16),
    ]);
  const second = () =>
    c.second === "memory" ? before.slice(c.address, c.address + length) : vector(c.second);
  const publish = (value) => {
    const result = new Uint8Array(32);
    result.set(value);
    expected.set(result.subarray(0, 16), LOWER + c.destination * 16);
    expected.set(result.subarray(16), UPPER + c.destination * 16);
  };
  const status = c.invalidState ? 4 : length && c.address > 65536 - length ? 3 : 0;
  if (!status) {
    if (c.tag === "broadcast") {
      const result = new Uint8Array(c.width),
        source = before.slice(c.address, c.address + length);
      for (let offset = 0; offset < c.width; offset += length) result.set(source, offset);
      publish(result);
    } else if (c.tag === "extract") {
      const offset = (c.immediate % 2) * 16,
        value = vector(c.first).slice(offset, offset + 16);
      if (c.destination === "memory") expected.set(value, c.address);
      else publish(value);
    } else if (c.tag === "insert") {
      const result = vector(c.first);
      result.set(second().subarray(0, 16), (c.immediate % 2) * 16);
      publish(result);
    } else if (c.tag === "permute") {
      const a = vector(c.first),
        b = second(),
        blocks = [a.subarray(0, 16), a.subarray(16), b.subarray(0, 16), b.subarray(16)],
        result = new Uint8Array(32);
      if (!(c.immediate & 8)) result.set(blocks[c.immediate % 4], 0);
      if (!(c.immediate & 128)) result.set(blocks[Math.floor(c.immediate / 16) % 4], 16);
      publish(result);
    } else throw new Error("unhandled " + label);
  }
  const instance = new WebAssembly.Instance(module, { env: { memory } });
  assert.equal(instance.exports.run(STATE, c.address), status, label + " isolated caller status");
  assert.deepEqual(bytes, expected, label + " all64KiB state/operand/host-refusal neutrality");
}
console.log(`${cases.length} actual cross-lane modules passed`);
