
let packetControls = 0;
for (const value of [0, 0x80000000, 0xffffffff]) for (const offset of [0, 1, 4092]) {
  const row = {name: `poisoned packet ${value}/${offset}`, address: ADDRESS + offset, value, poison: true};
  const native = execute(row, false), direct = execute(row, true);
  assert.deepEqual(direct.output, native.output, `${row.name}: all terminal/scratch/value/progress locals`);
  assert.deepEqual(direct.bytes, native.bytes, `${row.name}: full Wasm RAM and arena parity`);
  assert.deepEqual(direct.output, [0, 0, 0x91, 0xa2, 0xb3, 0xc4, 0, 0, value, 1]);
  assert.equal(direct.calls, 0); assert.equal(native.calls, 1);
  const expected = Buffer.alloc(40);
  expected.write('R3MH'); expected.writeUInt32LE(0x1000a, 4); expected.writeUInt32LE(40, 8);
  expected.writeUInt32LE(value >>> 0, 20); expected.writeUInt32LE(4, 36);
  assert.deepEqual(direct.bytes.subarray(100, 140), expected, 'all40 helper bytes replace the poison exactly');
  assert.equal(direct.bytes[99], 0x6b); assert.equal(direct.bytes[140], 0xd8);
  packetControls++;
}
console.log(`direct Read32 packet poison/canary controls passed (${packetControls} cases)`);
