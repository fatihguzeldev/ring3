import assert from 'node:assert/strict';

export function checkImmediate(call, host, guest) {
  const {read, get} = host, a = guest.addresses;
  if (call.expected === 'pointer') assert.ok(get(16) !== 0);
  else if (!call.private && !['wait-child', 'wait-apc', 'wait-timer', 'child-blocked'].includes(call.hook)) {
    assert.equal(get(16), call.expected, call.name);
  }
  switch (call.hook) {
    case 'recursive-owner':
      assert.equal(read(a.critical + 8), 2);
      assert.equal(read(a.critical + 12), 1);
      break;
    case 'suspended-child':
      assert.equal(read(a.childTid), 2);
      assert.equal(read(a.childParameter), 0);
      assert.equal(read(a.childEntered), 0);
      break;
    case 'still-active': assert.equal(read(a.childExit), 259); break;
    case 'still-contended':
      assert.equal(read(a.critical + 8), 1);
      assert.equal(read(a.critical + 12), 1);
      assert.equal(read(a.childEntered), 0);
      break;
    case 'child-exited': assert.equal(read(a.childExit), 37); break;
  }
}

export function checkFinal(host, guest) {
  const {read, durations} = host, a = guest.addresses;
  for (const segment of guest.segments.filter(segment => !segment.private && segment.expected !== 'exit')) {
    const result = read(segment.status);
    if (segment.expected === 'pointer') assert.ok(result !== 0 && result !== 0xffffffff, segment.name);
    else assert.equal(result, segment.expected, `${segment.thread}:${segment.name}`);
  }
  assert.equal(read(a.mainTid), 1);
  assert.equal(read(a.childTidFromFs), 2);
  assert.notEqual(read(a.mainTeb), read(a.childTeb));
  assert.equal(read(a.childPeb), read(a.mainPeb));
  assert.equal(read(a.childParameter), 0x13572468);
  assert.equal(read(a.childEntered), 1);
  assert.equal(read(a.childTlsInitial), 0);
  assert.equal(read(a.childTlsFinal), 0x2222);
  assert.equal(read(a.mainTls), 0x1111);
  assert.equal(read(a.childExit), 37);
  assert.equal(read(a.apcCount), 1);
  assert.equal(read(a.apcArgument), 0xabcdef01);
  assert.equal(read(a.apcTid), 1);
  assert.equal(read(a.timerCount), 1);
  assert.equal(read(a.timerTid), 1);
  assert.equal(read(a.timerArguments), 0x76543210);
  const filetime = BigInt(read(a.timerArguments + 4)) | BigInt(read(a.timerArguments + 8)) << 32n;
  assert.equal(filetime, 116444736000010000n);
  const time = offset => BigInt(read(a.threadTimes + offset)) | BigInt(read(a.threadTimes + offset + 4)) << 32n;
  assert.equal(time(0), 116444736000000000n);
  assert.equal(time(8), time(0));
  assert.equal(time(16), 0n);
  assert.equal(time(24), durations.get(2) / 100n);
  assert.ok(time(24) > 0n);
}
