
// tuple transitions are adversarial mock-backend changes, not a claim that native boxes move within an epoch.
const geometryRows = [], marker = 0x11223344;
const literalSuccess = (value, count) => [0, 0, 0x91, 0xa2, 0xb3, 0xc4, 0, 0, value, count];
for (const kind of ['mappings', 'backing', 'pages']) geometryRows.push({
  name: `live ${kind} tuple mismatch then restore`, count: 3, trace: true, fallback: 1,
  values: kind === 'pages' ? [0x80000000, 0x80000000, 0x80000000] : [0x80000000, marker, 0x80000000],
  expected: literalSuccess(0x80000000, 3),
  barrier(api) {
    if (api.phase === 1) {
      if (kind === 'mappings') {
        api.backend.mappings = MAP + 4096; api.set(DESC + 8, api.backend.mappings);
        api.set(api.backend.mappings + (ADDRESS >>> 12) * 4, 2);
        api.set(BACK + 4096 + (ADDRESS & 4095), marker);
      } else if (kind === 'backing') {
        api.backend.backing = BACK + 4096; api.set(DESC + 16, api.backend.backing);
        api.set(api.backend.backing + (ADDRESS & 4095), marker);
      } else { api.backend.pages = 1; api.set(DESC + 20, 1); }
    } else if (api.phase === 2) {
      api.backend.mappings = MAP; api.backend.backing = BACK; api.backend.pages = 2;
      api.set(DESC + 8, MAP); api.set(DESC + 16, BACK); api.set(DESC + 20, 2);
    }
  },
});
geometryRows.push({name: 'live permissions tuple changes the canonical read denial', count: 2, trace: true, fallback: 1,
  values: [0x80000000], fault: true,
  barrier(api) {
    if (api.phase !== 1) return;
    api.backend.permissions = PERMS + 4096; api.set(DESC + 12, api.backend.permissions);
    api.bytes[api.backend.permissions + (ADDRESS >>> 12)] = 2;
  },
});
for (const [label, offset, value] of [['null map', 8, 0], ['wrapped map', 8, 0xfffffffc], ['excess capacity', 20, 32769]])
  geometryRows.push({name: `live malformed tuple ${label}`, count: 2, trace: true, fallback: 1,
    values: [0x80000000, 0x80000000], expected: literalSuccess(0x80000000, 2),
    barrier(api) { if (api.phase === 1) api.set(DESC + offset, value); },
  });
geometryRows.push({name: 'initial capacity repair stays native until next invocation', field: [20, 0], count: 2,
  trace: true, fallback: 2, reenter: true, values: [0x80000000, 0x80000000], expected: literalSuccess(0x80000000, 2),
  barrier(api) { if (api.phase === 1) api.set(DESC + 20, 2); },
});
geometryRows.push({name: 'initial full-array extent becomes valid after grow', field: [8, BYTES - 4], grow: true,
  count: 2, trace: true, fallback: 2, reenter: true, values: [0x80000000, 0x80000000], expected: literalSuccess(0x80000000, 2),
  barrier(api) {
    if (api.phase !== 1) return;
    api.grow(64); api.backend.mappings = BYTES - 4;
    api.set(api.backend.mappings + (ADDRESS >>> 12) * 4, 1);
  },
});
geometryRows.push({name: 'initial descriptor tail becomes valid after grow', pointer: BYTES - 16, grow: true,
  count: 2, trace: true, fallback: 2, reenter: true, values: [0x80000000, 0x80000000], expected: literalSuccess(0x80000000, 2),
  barrier(api) {
    if (api.phase !== 1) return;
    api.grow(1); api.data.setBigUint64(BYTES - 16, 1n, true);
    [MAP, PERMS, BACK, 2].forEach((value, index) => api.set(BYTES - 8 + index * 4, value));
  },
});
geometryRows.push({name: 'valid cached ranges remain usable after grow', grow: true, count: 2, trace: true,
  fallback: 0, values: [0x80000000, 0x80000000], expected: literalSuccess(0x80000000, 2),
  barrier(api) { if (api.phase === 1) api.grow(1); },
});
for (const pointer of [BYTES - 16, 0xfffffff8]) geometryRows.push({name: `zero count hostile pointer ${pointer}`,
  pointer, count: 0, poison: true, fallback: 0, expected: [0, 0, 0x91, 0xa2, 0xb3, 0xc4, 0xd5, 0xe6, 0xf7, 0],
});
for (const row of geometryRows) {
  const native = execute(row, false), cached = execute(row, true);
  assert.deepEqual(cached.output, native.output, `${row.name}: exact full returned state`);
  assert.deepEqual(cached.bytes, native.bytes, `${row.name}: complete Wasm RAM and opaque arena`);
  assert.equal(cached.calls, row.fallback, `${row.name}: final geometry policy's exact native fallback count`);
  if (row.expected) assert.deepEqual(cached.output, row.expected, `${row.name}: literal frontier/scratch/value`);
  if (row.values) assert.deepEqual(cached.values, row.values, `${row.name}: every intermediate value follows live backend cells`);
  if (row.fault) {
    assert.deepEqual(cached.output.slice(0, 6), [23, 23, 5, ADDRESS, 1, 4]);
    assert.deepEqual(cached.output.slice(8), [0x80000000, 1]);
    const expected = Buffer.alloc(40);
    expected.write('R3MH'); expected.writeUInt32LE(0x1000f, 4); expected.writeUInt32LE(40, 8);
    [9, 0, 5, ADDRESS, 1, 4].forEach((value, index) => expected.writeUInt32LE(value, 16 + index * 4));
    assert.deepEqual(cached.bytes.subarray(100, 140), expected);
    assert.equal(cached.bytes.readBigUInt64LE(DESC), 0n, 'denied read revokes the projected epoch');
  }
  if (row.reenter) {
    assert.deepEqual(cached.reentry.output, native.reentry.output);
    assert.deepEqual(cached.reentry.output, literalSuccess(0x80000000, 1));
    assert.equal(cached.reentry.calls, 0, 'a new invocation retries the now-valid complete geometry');
    assert.equal(native.reentry.calls, 1);
  }
  if (row.count === 0) {
    assert.deepEqual(cached.bytes.subarray(100, 140), Buffer.alloc(40, 0xa5));
    assert.equal(cached.bytes[99], 0x6b); assert.equal(cached.bytes[140], 0xd8);
  }
}
console.log(`direct Read32 final cached-geometry controls passed (${geometryRows.length} cases)`);
