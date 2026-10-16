import assert from 'node:assert/strict';

export function verifyRpcLog(bytes) {
  assert.equal(bytes.length % 16, 0);
  const rows = [];
  for (let at = 0; at < bytes.length; at += 16) rows.push({event: String.fromCharCode(bytes.readUInt32LE(at)), thread: bytes.readUInt32LE(at + 4), role: bytes.readUInt32LE(at + 8), value: bytes.readUInt32LE(at + 12)});
  assert.equal(rows[0].event, 'b');
  const caller = rows[0].thread;
  const source = rows.filter(row => row.event === 'c' && row.role === 0);
  assert.equal(source.length, 1);
  const owner = source[0].thread;
  assert.notEqual(owner, caller);
  assert.equal(source[0].value, owner);
  assert.deepEqual(rows.filter(row => row.event === 'P'), [{event: 'P', role: 1, thread: caller, value: caller}]);
  assert.deepEqual(rows.filter(row => row.event === 'S'), [{event: 'S', role: 2, thread: owner, value: owner}]);
  assert.deepEqual(rows.filter(row => row.event === 'C'), [{event: 'C', role: 1, thread: caller, value: 3}]);
  for (const [event, role, thread] of [['i', 2, owner], ['v', 0, owner], ['p', 1, caller]]) {
    assert.deepEqual(rows.filter(row => row.event === event), [0, 1].map(value => ({event, role, thread, value})), 'successful and failed methods keep exact apartment and opaque guest packet behavior');
  }
  assert.deepEqual(rows.filter(row => row.event === 'o'), [{event: 'o', role: 3, thread: caller, value: owner}]);
  for (const [role, thread] of [[1, caller], [2, owner]]) {
    assert.deepEqual(rows.filter(row => row.role === role && row.event === 'd'), [{event: 'd', role, thread, value: 0}]);
    assert.deepEqual(rows.filter(row => row.role === role && row.event === 'r'), [{event: 'r', role, thread, value: 0}]);
  }
  assert.equal(rows.filter(row => row.role === 0 && row.event === 'r').at(-1).value, 0, 'source native identity is released after the stub');
  for (const role of [0, 1]) {
    assert.equal(rows.filter(row => row.role === role && row.event === 'A').length, 1);
    const unload = rows.findIndex(row => row.role === role && row.event === 'U' && row.value === 0);
    const detach = rows.findIndex(row => row.role === role && row.event === 'D');
    assert.equal(rows.filter(row => row.role === role && row.event === 'D' && row.value === 0).length, 1, 'each loaded DLL detaches exactly once');
    assert.ok(unload >= 0 && detach > unload, 'zero live native references precede actual DLL detach');
  }
  const completed = rows.findIndex(row => row.event === 'e');
  assert.deepEqual(rows.filter(row => row.event === 'e'), [{event: 'e', role: 3, thread: caller, value: 0}]);
  for (const role of [0, 1, 2]) {
    assert.ok(rows.findLastIndex(row => row.role === role && row.event === 'r' && row.value === 0) < completed, 'all native identities retire before guest completion');
  }
  // CoFreeUnusedLibraries gives Both servers an unload delay. ExitProcess may
  // therefore deliver their DLL_PROCESS_DETACH after the client's final log row.
  // https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-cofreeunusedlibrariesex
  assert.ok(rows.slice(completed + 1).every(row => row.event === 'D' && row.value === 0 && [0, 1].includes(row.role)), 'only normal DLL detach may follow guest completion');
  return rows;
}
