import assert from 'node:assert/strict';

export function beforeActivationCall(call, {api, context, pair}) {
  if (call.hook === 'clock-wrap') {
    context.now = 100_000_000n + (0x1_0000_0000n + 9n) * 1_000_000n;
    assert.equal(api.observe_cpu_clock(...pair(context.now)), 0);
  }
}

export function afterActivationCall(call, {guest, read}) {
  const address = guest.addresses;
  switch (call.hook) {
    case 'timer-caps':
      assert.equal(read(address.timerCaps), 1);
      assert.equal(read(address.timerCaps + 4), 65535);
      break;
    case 'timer-system':
      assert.equal(read(address.timerSystem), 1);
      assert.equal(read(address.timerSystem + 4), 9);
      assert.equal(read(address.timerSystem + 8), 0xfeedbeef);
      break;
    case 'activation-null-output':
      assert.equal(read(address.activationOutput), 0);
      break;
    case 'factory-identity':
      assert.equal(read(address.activationFactory), read(address.activationFactoryUnknown));
      break;
    case 'sound-uninitialized':
      assert.equal(read(address.activationSpeaker), 0xfeedbeef);
      break;
    case 'sound-initialized':
      assert.equal(read(address.activationSpeaker), 4);
      break;
  }
}
