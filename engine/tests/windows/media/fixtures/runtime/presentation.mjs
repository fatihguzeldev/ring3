import assert from 'node:assert/strict';

export function windowInfo(host, window) {
  const before = Buffer.from(host.bytes().slice(host.base, host.base + 140));
  assert.equal(host.api.user32_window_info(window), 0);
  const record = host.take(80);
  assert.deepEqual(Buffer.from(host.bytes().slice(host.base, host.base + 140)), before,
    'the read query preserves CPU, exit, and completion state');
  assert.equal(record.subarray(0, 4).toString(), 'R3UW');
  assert.equal(record.readUInt16LE(4), 1);
  assert.equal(record.readUInt16LE(6), 1);
  assert.equal(record.readUInt32LE(8), 80);
  const present = record.readUInt32LE(12);
  assert.ok(present === 0 || present === 1);
  if (!present) {
    assert.ok(record.subarray(16).every(byte => byte === 0), 'absent records cannot retain a stale HWND');
    return undefined;
  }
  return {
    window: record.readUInt32LE(16), parent: record.readUInt32LE(20), owner: record.readUInt32LE(24),
    thread: record.readUInt32LE(28), style: record.readUInt32LE(32), exStyle: record.readUInt32LE(36),
    outer: [40, 44, 48, 52].map(offset => record.readInt32LE(offset)),
    clientOrigin: [56, 60].map(offset => record.readInt32LE(offset)),
    clientExtent: [64, 68].map(offset => record.readUInt32LE(offset)),
    visible: record.readUInt32LE(72), enabled: record.readUInt32LE(76),
  };
}

export function unconfiguredWindowQuery(host) {
  assert.equal(host.api.user32_window_info.length, 1);
  const before = Buffer.from(host.bytes().slice(host.base, host.base + 220));
  assert.equal(host.api.user32_window_info(0), 7, 'the USER32 owner has not been configured');
  assert.deepEqual(Buffer.from(host.bytes().slice(host.base, host.base + 220)), before,
    'a failed query changes neither CPU nor the existing transfer payload');
}

export function createdWindow(host, window) {
  assert.deepEqual(windowInfo(host, window), {
    window, parent: 0, owner: 0, thread: 1, style: 0x10000000, exStyle: 0,
    outer: [0, 0, 64, 64], clientOrigin: [0, 0], clientExtent: [64, 64], visible: 1, enabled: 1,
  });
}
