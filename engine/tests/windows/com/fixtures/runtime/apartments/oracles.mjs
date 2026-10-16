import assert from 'node:assert/strict';

export function verifyApartmentLog(bytes, orphan, rundown) {
  assert.equal(bytes.length % 16, 0, 'literal four-DWORD guest log records');
  const rows = [];
  for (let at = 0; at < bytes.length; at += 16) rows.push({
    event: String.fromCharCode(bytes.readUInt32LE(at)), thread: bytes.readUInt32LE(at + 4),
    model: bytes.readUInt32LE(at + 8), value: bytes.readUInt32LE(at + 12),
  });
  const only = (event, model) => {
    const found = rows.filter(row => row.event === event && row.model === model);
    assert.equal(found.length, 1, `${event}/${model} occurs once`);
    return found[0];
  };
  const main = only('s', 0).thread;
  assert.equal(only('e', 0).thread, main);
  assert.equal(rows.some(row => row.event === '!'), false, 'a terminated sender never resumes its original CoCreateInstance');
  const freeFactories = rows.filter(row => row.event === 'G' && row.model === 2);
  assert.equal(freeFactories.length, rundown ? 2 : 1);
  for (const row of freeFactories) assert.notEqual(row.thread, main, 'Free server executes inside an actual MTA worker');
  assert.equal(only('G', 3).thread, main, 'nested MainSTA server reenters the blocked caller STA');
  assert.equal(only('G', 4).thread, main, 'Neutral scope preserves the physical caller thread');
  const nestedStart = rows.indexOf(only('N', 2)), nestedEnd = rows.indexOf(only('n', 2));
  assert.ok(nestedStart < rows.indexOf(only('G', 3)) && rows.indexOf(only('F', 3)) < nestedEnd);
  assert.equal(only('L', 3).thread, main);
  assert.equal(only('l', 3).thread, main);
  for (const model of [1, 2, 4]) {
    const published = only('p', model);
    const rawFactory = rows.find(row => row.event === 'G' && row.model === model);
    assert.notEqual(published.value, rawFactory.value, `model ${model} publishes an engine proxy, never the raw server pointer`);
    const object = only('o', model);
    const rawObject = rows.find(row => row.event === 'O' && row.model === model);
    assert.notEqual(object.value, rawObject.value, `model ${model} object is marshaled through standard IUnknown`);
    if (model === 4) for (const row of rows.filter(row => row.model === model && 'GCQORFLl'.includes(row.event))) assert.equal(row.thread, main);
  }
  const worker = rows.find(row => row.event === 'w' && row.model === 1);
  assert.notEqual(worker.thread, main);
  for (const row of rows.filter(row => row.event === 'G' && row.model === 1)) {
    assert.notEqual(row.thread, main);
    assert.notEqual(row.thread, worker.thread, 'MTA→Apartment receives a distinct hosted STA');
  }
  if (orphan) {
    const blocked = only('B', 1), killed = only('K', 1), released = only('k', 1);
    assert.equal(killed.thread, main);
    assert.notEqual(blocked.thread, killed.value);
    const nativeObject = rows.find((row, index) => index > rows.indexOf(killed) && row.event === 'O' && row.model === 1);
    assert.ok(nativeObject, 'receiver returns a real native object after sender termination');
    const nativeRelease = rows.find((row, index) => index > rows.indexOf(nativeObject) && row.event === 'R' && row.model === 1 && row.value === nativeObject.value);
    assert.ok(nativeRelease, 'abandoned returned reference is released in its owner apartment');
    assert.equal(nativeRelease.thread, blocked.thread);
    assert.ok(rows.indexOf(blocked) < rows.indexOf(killed));
    assert.ok(rows.indexOf(nativeRelease) < rows.indexOf(released));
  } else assert.equal(rows.some(row => 'BKk'.includes(row.event)), false);
  if (rundown) {
    const unknown=only('X',2), factory=only('Y',2);
    assert.notEqual(unknown.value,factory.value, 'proxy exposes distinct standard interface views');
    const tail=rows.slice(rows.indexOf(factory)+1), releases=tail.filter(row=>row.event === 'F' && row.model === 2);
    assert.equal(releases.length,2,'CoUninitialize releases both acquired native references exactly once');
    for (const row of releases) {
      assert.equal(row.value,freeFactories.at(-1).value,'both references denote the same native factory');
      assert.notEqual(row.thread,main,'actual Release executes in the MTA');
    }
    const detach=only('D',2);
    assert.ok(rows.indexOf(releases[1]) < rows.indexOf(detach));
    assert.ok(rows.indexOf(detach) < rows.indexOf(only('e',0)));
  }
  return rows;
}
