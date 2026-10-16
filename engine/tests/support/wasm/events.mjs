import assert from 'node:assert/strict';

const vectors = new Map([[5, 0xffffffff], [10, 0], [11, 13], [12, 16], [13, 19],
  [14, 11], [15, 12], [16, 1], [17, 3], [18, 4], [19, 5], [20, 6],
  [21, 10], [22, 7], [23, 14]]);

// consume only after the execution oracle, while the exact stopped context is live.
export function consumeCpuEvent(c, size = 4796) {
  const before = Buffer.from(new Uint8Array(c.memory.buffer, c.base, size));
  const reason = before.readUInt32LE(72);
  if (!vectors.has(reason) && reason !== 24) return;
  assert.equal(typeof c.api.take_cpu_event, 'function');
  assert.equal(c.api.take_cpu_event(), 0, 'consume the current stopped CPU event');
  const after = Buffer.from(new Uint8Array(c.memory.buffer, c.base, size));
  const event = after.subarray(140, 188);
  assert.deepEqual(event.subarray(0, 16), Buffer.from([
    0x52, 0x33, 0x43, 0x45, 1, 0, 1, 0, 48, 0, 0, 0, 0, 0, 0, 0,
  ]), 'literal R3CE48 envelope');
  const vector = reason === 24
    ? before.readUInt32LE(80) === 0x100 ? 0xffffffff : before.readUInt32LE(80)
    : vectors.get(reason);
  assert.equal(event.readUInt32LE(16), vector, 'event vector agrees with the semantic exit');
  assert.equal(event.readUInt32LE(24), before.readUInt32LE(48), 'exact stopped EIP');
  assert.equal(event.readUInt32LE(44), before.readUInt32LE(76), 'exact stopped retirement');
  assert.deepEqual(after.subarray(0, 140), before.subarray(0, 140), 'event take preserves CPU, exit, cancel and helper');
  assert.deepEqual(after.subarray(188), before.subarray(188), 'event take preserves the remaining arena');
  assert.equal(c.api.take_cpu_event(), 7, 'a consumed event cannot be replayed');
  assert.deepEqual(Buffer.from(new Uint8Array(c.memory.buffer, c.base, size)), after, 'rejected replay is neutral');
  // the caller owns transfer input; preserve it for the next declared execution.
  new Uint8Array(c.memory.buffer, c.base + 140, 48).set(before.subarray(140, 188));
}
