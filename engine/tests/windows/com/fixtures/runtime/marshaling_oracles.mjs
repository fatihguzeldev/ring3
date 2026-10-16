import assert from 'node:assert/strict';

export function verifyMarshalingLog(bytes, streamCopy = false) {
  assert.equal(bytes.length % 16, 0);
  const rows = [];
  for (let at = 0; at < bytes.length; at += 16) rows.push({event: String.fromCharCode(bytes.readUInt32LE(at)), thread: bytes.readUInt32LE(at + 4), role: bytes.readUInt32LE(at + 8), value: bytes.readUInt32LE(at + 12)});
  assert.equal(rows[0].event, 'b');
  const caller = rows[0].thread;
  assert.equal(rows[0].value, caller);
  const source = rows.filter(row => row.role === 0 && row.event === 'c');
  assert.equal(source.length, 2);
  assert.ok(source.every(row => row.thread !== caller && row.thread === row.value));
  assert.equal(source[0].thread, source[1].thread, 'both calls use the actual source STA');
  for (const event of ['g', 's', 'w']) {
    const calls = rows.filter(row => row.event === event);
    assert.equal(calls.length, 2, `two real IMarshal ${event} calls`);
    assert.ok(calls.every(row => row.role === 0 && row.thread === source[0].thread));
  }
  assert.ok(rows.filter(row => row.event === 'g').every(row => row.value === 3));
  assert.ok(rows.filter(row => row.event === 's').every(row => row.value === 8));
  const unmarshal = rows.filter(row => row.event === 'u');
  assert.equal(unmarshal.length, 2);
  assert.ok(unmarshal.every(row => row.role === 1 && row.thread === caller && row.value === 0x13579bdf));
  const packets = rows.filter(row => row.event === 'd');
  assert.deepEqual(packets, [{event: 'd', thread: source[0].thread, role: 0, value: 0x13579bdf}], 'only failed normal unmarshal needs ReleaseMarshalData');
  const values = rows.filter(row => row.event === 'v');
  assert.deepEqual(values, [{event: 'v', thread: caller, role: 1, value: 0x13579bdf}], 'caller receives a real client-apartment object');
  assert.equal(rows.filter(row => row.event === 'p').length, 1);
  assert.equal(rows.filter(row => row.event === 'f').length, 1);
  for (const role of [0, 1]) {
    assert.equal(rows.filter(row => row.role === role && row.event === 'A').length, 1);
    const unload = rows.findIndex(row => row.role === role && row.event === 'U' && row.value === 0);
    const detach = rows.findIndex(row => row.role === role && row.event === 'D');
    assert.ok(unload >= 0 && detach > unload, 'actual zero-live reference check precedes actual DLL detach');
  }
  if (streamCopy) {
    const writes = rows.filter(row => row.event === 'k');
    assert.deepEqual(writes.map(row => row.value), [0x808, 0x1000803, 0x2000802, 0x808, 0x1000803, 0x2000802]);
    assert.ok(writes.every(row => row.role === 0 && row.thread === source[0].thread), 'actual application Write runs in the original source apartment');
    assert.deepEqual(rows.filter(row => row.event === 'h').map(row => row.value), [0, 0x80030070, 0x80004005, 0, 0x80030070, 0x80004005]);
  }
  assert.equal(rows.at(-1).event, 'e');
  assert.ok(rows.every(row => row.thread > 0));
  return rows;
}
