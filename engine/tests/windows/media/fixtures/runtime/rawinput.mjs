import assert from 'node:assert/strict';

export function beforeRawInputCall(call, input) {
  if (call.hook === 'raw-input') input([[4, 1], [1, 0x20, 1], [3, 7, -3, 120]]);
  if (call.hook === 'raw-release') input([[1, 0x20, 0]]);
}

export function afterRawInputCall(call, {guest, read}) {
  const a = guest.addresses;
  switch (call.hook) {
    case 'raw-registered':
      assert.equal(read(a.rawRegistered), 0x60001);
      assert.equal(read(a.rawRegistered + 4), 0x30);
      assert.equal(read(a.rawRegistered + 8), read(a.window));
      assert.equal(read(a.rawRegistered + 12), 0x20001);
      break;
    case 'raw-device-count':
      assert.equal(read(a.rawCount), 2);
      break;
    case 'raw-device-list':
      assert.equal(read(a.rawDevices), 0x59000001);
      assert.equal(read(a.rawDevices + 4), 0);
      assert.equal(read(a.rawDevices + 8), 0x59000002);
      assert.equal(read(a.rawDevices + 12), 1);
      break;
    case 'raw-device-info':
      assert.equal(read(a.rawInfo), 32);
      assert.equal(read(a.rawInfo + 4), 1);
      assert.equal(read(a.rawInfo + 28), 101);
      break;
    case 'raw-input':
      assert.equal(read(a.message + 4), 0xff);
      assert.equal(read(a.message + 8), 0);
      assert.ok(read(a.message + 12) >= 0x58000000);
      break;
    case 'raw-header':
      assert.equal(read(a.rawData), 1);
      assert.equal(read(a.rawData + 4), 32);
      assert.equal(read(a.rawData + 8), 0x59000002);
      break;
    case 'raw-required':
      assert.equal(read(a.rawSize), 32);
      break;
    case 'raw-keyboard':
      assert.equal(read(a.rawData + 16), 0x20);
      assert.equal(read(a.rawData + 20), 0x440000);
      assert.equal(read(a.rawData + 24), 0x100);
      break;
    case 'raw-mouse':
      assert.equal(read(a.rawData), 0);
      assert.equal(read(a.rawData + 4), 40);
      assert.equal(read(a.rawData + 20), 0x780400);
      assert.equal(read(a.rawData + 28), 7);
      assert.equal(read(a.rawData + 32), -3 >>> 0);
      break;
    case 'raw-release':
      assert.equal(read(a.rawData), 1);
      assert.equal(read(a.rawData + 16), 0x10020);
      assert.equal(read(a.rawData + 24), 0x101);
      break;
  }
}
