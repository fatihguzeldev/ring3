import assert from 'node:assert/strict';

const pair = value => [Number(value & 0xffffffffn), Number(value >> 32n)];
const identity = entry => [pair(entry.epoch), pair(entry.sequence), pair(entry.revision)].flat();

export function configureRegistry({api, request, take}) {
  assert.equal(api.configure_registry(256, 1024, 64, 1024 * 1024, 256, 0), 0);
  const seedPath = Buffer.from('Software\\Ring3\\Seed', 'utf16le');
  const seedName = Buffer.from('Boot', 'utf16le');
  const seed = Buffer.alloc(64 + seedPath.length + seedName.length + 4);
  seed.write('R3RS');
  seed.writeUInt16LE(1, 4);
  seed.writeUInt16LE(1, 6);
  seed.writeUInt32LE(seed.length, 8);
  seed.writeUInt32LE(0x80000001, 16);
  seed.writeUInt32LE(32, 20);
  seed.writeUInt32LE(seedPath.length / 2, 24);
  seed.writeUInt32LE(seedName.length / 2, 28);
  seed.writeUInt32LE(4, 36);
  seed.writeUInt32LE(4, 40);
  seed.writeBigUInt64LE(1000n, 44);
  seedPath.copy(seed, 64);
  seedName.copy(seed, 64 + seedPath.length);
  seed.writeUInt32LE(0x12345678, seed.length - 4);
  assert.equal(api.begin_registry_seed(seed.length), 0);
  const seedReceipt = take(40);
  assert.equal(seedReceipt.subarray(0, 4).toString(), 'R3RI');
  let epoch = seedReceipt.readBigUInt64LE(16);
  const seedToken = seedReceipt.readBigUInt64LE(24);
  assert.ok(epoch > 0n && seedToken > 0n);
  assert.equal(seedReceipt.readUInt32LE(32), 0);
  assert.equal(seedReceipt.readUInt32LE(36), seed.length);
  request(seed);
  assert.notEqual(api.append_registry_seed(...pair(epoch + 1n), ...pair(seedToken), 0, seed.length), 0);
  request(seed);
  assert.equal(api.append_registry_seed(...pair(epoch), ...pair(seedToken), 0, seed.length), 0);
  assert.equal(api.finish_registry_seed(...pair(epoch), ...pair(seedToken)), 0);
  assert.notEqual(api.finish_registry_seed(...pair(epoch), ...pair(seedToken)), 0, 'seed receipt cannot replay');
  function descriptor() {
    assert.equal(api.registry_change(0), 0);
    const record = take(128);
    assert.equal(record.subarray(0, 4).toString(), 'R3RG');
    assert.equal(record.readUInt32LE(8), 128);
    assert.ok(record.subarray(84).every(byte => byte === 0));
    const entry = {
      epoch: record.readBigUInt64LE(16),
      sequence: record.readBigUInt64LE(24),
      revision: record.readBigUInt64LE(32),
      kind: record.readUInt32LE(40),
      root: record.readUInt32LE(44),
      view: record.readUInt32LE(48),
      pathUnits: record.readUInt32LE(52),
      nameUnits: record.readUInt32LE(56),
      classUnits: record.readUInt32LE(60),
      modified: record.readBigUInt64LE(64),
      valueKind: record.readUInt32LE(72),
      dataBytes: record.readUInt32LE(76),
      hasValue: record.readUInt32LE(80),
    };
    if (entry.epoch === 0n) return undefined;
    assert.ok(entry.sequence > 0n && entry.revision > 0n);
    if (epoch === undefined) epoch = entry.epoch;
    assert.equal(entry.epoch, epoch);
    assert.ok([1, 2, 3, 4].includes(entry.kind));
    assert.ok([0x80000002, 0x80000003].includes(entry.root));
    assert.equal(entry.view, 32);
    assert.ok([0, 1].includes(entry.hasValue));
    return entry;
  }
  function text(entry, part, units) {
    const pieces = [];
    let offset = 0;
    while (offset < units) {
      assert.equal(api.registry_read_change_text(...identity(entry), part, offset), 0);
      const header = take(16);
      assert.equal(header.subarray(0, 4).toString(), 'R3RT');
      const count = header.readUInt32LE(12);
      assert.ok(count > 0 && count <= units - offset);
      assert.equal(header.readUInt32LE(8), 16 + count * 2);
      pieces.push(take(16 + count * 2).subarray(16));
      offset += count;
    }
    return Buffer.concat(pieces).toString('utf16le');
  }
  function data(entry) {
    const pieces = [];
    let offset = 0;
    while (offset < entry.dataBytes) {
      assert.equal(api.registry_read_change_data(...identity(entry), offset), 0);
      const header = take(16);
      assert.equal(header.subarray(0, 4).toString(), 'R3RT');
      const count = header.readUInt32LE(12);
      assert.ok(count > 0 && count <= entry.dataBytes - offset);
      assert.equal(header.readUInt32LE(8), 16 + count);
      pieces.push(take(16 + count).subarray(16));
      offset += count;
    }
    return Buffer.concat(pieces).toString('hex');
  }
  function drain(stage) {
    const changes = [];
    for (let index = 0; index <= 256; index++) {
      const entry = descriptor();
      if (!entry) return changes;
      const path = text(entry, 0, entry.pathUnits);
      const name = text(entry, 1, entry.nameUnits);
      const keyClass = text(entry, 2, entry.classUnits);
      const bytes = data(entry);
      assert.notEqual(api.acknowledge_registry_change(...identity({...entry, revision: entry.revision + 1n})), 0, 'stale revision cannot consume dirty change');
      const current = descriptor();
      assert.deepEqual(identity(current), identity(entry), 'failed ACK leaves exact current descriptor');
      assert.equal(api.acknowledge_registry_change(...identity(entry)), 0);
      assert.notEqual(api.acknowledge_registry_change(...identity(entry)), 0, 'ACK cannot replay');
      changes.push({stage, epoch: String(entry.epoch), sequence: String(entry.sequence), revision: String(entry.revision), kind: entry.kind, root: entry.root, view: entry.view, path, name, keyClass, valueKind: entry.valueKind, bytes});
    }
    assert.fail('bounded registry change export');
  }
  assert.equal(descriptor(), undefined, 'host seed is the initial snapshot, not a guest dirty change');
  return {drain};
}
