import assert from 'node:assert/strict';

export const pair = value => [Number(value & 0xffffffffn), Number(value >> 32n)];

export function configureFiles({api, request, take}) {
  const config = Buffer.alloc(80);
  config.write('R3FC');
  config.writeUInt16LE(1, 4);
  config.writeUInt16LE(1, 6);
  config.writeUInt32LE(80, 8);
  [
    4096, 256, 2, 128, 32, 16 * 1024 * 1024, 256, 32,
    32, 64, 32, 1024 * 1024, 0x10000000, 0x60000,
  ].forEach((value, index) => config.writeUInt32LE(value, 16 + index * 4));
  request(config);
  assert.equal(api.configure_files(config.length), 0);
  const info = take(64);
  assert.equal(info.subarray(0, 4).toString(), 'R3FS');
  const epoch = info.readBigUInt64LE(16);
  const assets = new Map([
    [1n, Buffer.from([1, 2, 3, 4, 5, 6, 7, 8])],
    [2n, Buffer.from([0x80, 0xc0, 0x80, 0x40])],
  ]);

  function mount(path, id, length) {
    const name = Buffer.from(path, 'utf16le');
    const payload = Buffer.alloc(64 + name.length);
    payload.write('R3FM');
    payload.writeUInt16LE(1, 4);
    payload.writeUInt16LE(1, 6);
    payload.writeUInt32LE(payload.length, 8);
    payload.writeBigUInt64LE(id, 16);
    payload.writeBigUInt64LE(length, 24);
    payload.writeUInt32LE(0x80, 32);
    payload.writeUInt32LE(name.length / 2, 36);
    name.copy(payload, 64);
    request(payload);
    assert.equal(api.mount_file(payload.length), 0);
  }
  mount('C:\\assets\\data.bin', 1n, 8n);
  mount('C:\\assets\\tone.pcm', 2n, 4n);
  mount('C:\\assets\\large.bin', 99n, 0x100000000n);

  function supply() {
    assert.equal(api.pending_file_request(), 0);
    const receipt = take(80);
    assert.equal(receipt.subarray(0, 4).toString(), 'R3FQ');
    const requestID = receipt.readBigUInt64LE(24);
    if (requestID === 0n) return false;
    const asset = receipt.readBigUInt64LE(48);
    const offset = receipt.readBigUInt64LE(56);
    const length = receipt.readUInt32LE(64);
    assert.equal(receipt.readBigUInt64LE(16), epoch);
    assert.ok(length > 0 && length <= 65536);
    const data = asset === 99n
      ? Buffer.from(Array.from({length}, (_, index) => Number((offset + BigInt(index)) & 255n)))
      : assets.get(asset)?.subarray(Number(offset), Number(offset) + length);
    assert.ok(data && data.length === length, 'host only serves authored asset identities');
    for (let at = 0; at < length; at += 4016) {
      const chunk = data.subarray(at, at + 4016);
      const payload = Buffer.concat([receipt, chunk]);
      const stale = Buffer.from(payload);
      stale.writeBigUInt64LE(epoch + 1n, 16);
      request(stale);
      assert.equal(api.supply_file_bytes(at, chunk.length), 40, 'stale epoch is neutral');
      request(payload);
      assert.equal(api.supply_file_bytes(at, chunk.length), 0);
    }
    return true;
  }
  return {epoch, assets, supply};
}
