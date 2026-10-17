import assert from 'node:assert/strict';

export function verifyLog(bytes) {
  assert.equal(bytes.length, 77 * 16, 'all eight dialogs plus nested dialog and final marker');
  const rows = [];
  for (let at = 0; at < bytes.length; at += 16) rows.push({
    event: String.fromCharCode(bytes.readUInt32LE(at)), scenario: bytes.readUInt32LE(at + 4),
    message: bytes.readUInt32LE(at + 8), value: bytes.readUInt32LE(at + 12),
  });
  assert.ok(rows.every(row => row.event !== '!'), 'the guest asserts its own ordinary API results');
  for (let scenario = 0; scenario < 8; scenario++) {
    const events = rows.filter(row => row.scenario === scenario);
    assert.equal(events[0].event, 'c');
    assert.equal(events[0].value, scenario & 1 ? 4 : 6, 'native standard/extended creation-data size word');
    assert.equal(events[1].event, 'f');
    assert.equal(events[1].value, 0xfffffff4, 'real HFONT has logical height -12');
    assert.equal(events[2].event, 'i');
    assert.equal(events[2].value, 0xaabbccdd);
    assert.deepEqual(events.filter(row => row.event === 'e').map(row => [row.message, row.value]),
      scenario & 4 ? [[0x111, 0x12345678]] : [], 'modal completion comes from the posted WM_COMMAND');
    assert.deepEqual(events.filter(row => row.event === 'd' || row.event === 'x').map(row => [row.event, row.message, row.value]),
      [['d', 2, 0], ['x', 2, 100], ['x', 0x82, 100], ['d', 0x82, 0]], 'native parent/child destruction order');
    assert.equal(events.at(-1).event, 'r');
    assert.equal(events.at(-1).value, scenario & 4 ? 0x12345678 : 1);
  }
  const nested = rows.filter(row => row.scenario === 0x107);
  assert.deepEqual(nested.map(row => row.event), ['c', 'f', 'i', 'd', 'x', 'x', 'd']);
  assert.equal(nested[2].value, 0x11223344);
  assert.deepEqual(rows.filter(row => row.event === 'n').map(row => [row.scenario, row.value]), [[7, 1]]);
  assert.deepEqual(rows.at(-1), {event: 'z', scenario: 8, message: 0, value: 0});
  return rows;
}
