import assert from "node:assert/strict";

export function verify(call, { get, read, guest, callbackHistory }) {
  const a = guest.addresses;
  const message = () => Array.from({ length: 7 }, (_, index) => read(a.message + index * 4));
  const eax = get(16);
  if (call.expected === "pointer") assert.ok(eax !== 0 && eax !== 0xffffffff, `${call.name} must succeed`);
  else if (call.expected === "atom") assert.ok(eax >= 0xc000 && eax <= 0xffff);
  else if (call.expected?.ref) assert.equal(eax, read(a[call.expected.ref]), call.name);
  else if (typeof call.expected === "number") assert.equal(eax, call.expected, call.hook ?? call.name);
  if (!call.hook) return;
  const rows = callbackHistory.slice(call.start).filter((row) => row.procedure !== guest.wndproc);
  const hook = call.hook;
  const expected = {
    "peek-down": [[3, 0x41, 0x001e0001]],
    "peek-again": [[3, 0x41, 0x001e0001]],
    "remove-down": [[0, 0x41, 0x001e0001]],
    "remove-up": [[0, 0x41, 0xc01e0001]],
    "suppress-noremove": [[3, 0x42, 0x00300001]],
    "skip-suppressed": [[0, 0x43, 0x002e0001], [0, 0x44, 0x00200001]],
    "waiting-get": [[0, 0x45, 0x00120001]],
  }[hook] ?? [];
  assert.equal(rows.length, expected.length * 2, `${hook}: real keyboard callback count`);
  for (const [index, args] of expected.entries()) {
    assert.deepEqual(rows.slice(index * 2, index * 2 + 2).map((row) => row.procedure),
      [guest.callbacks[1].entry, guest.callbacks[0].entry], "newest hook forwards to older guest procedure");
    for (const row of rows.slice(index * 2, index * 2 + 2)) {
      assert.deepEqual(row.arguments, args, `${hook}: hook arguments`);
      assert.equal(row.result, args[1] === 0x42 || args[1] === 0x43 ? 0x51 : 0,
        "real hook return propagates through CallNextHookEx");
    }
  }
  if (call.name === "PeekMessageA" || call.name === "GetMessageA") {
    const actual = message();
    if (call.expected === 0) assert.deepEqual(actual, Array(7).fill(0xa5a5a5a5), "empty/suppressed retrieval preserves MSG");
    else {
      const values = {
        "peek-down": [0x100, 0x41, 0x001e0001, 2],
        "peek-again": [0x100, 0x41, 0x001e0001, 2],
        "remove-down": [0x100, 0x41, 0x001e0001, 2],
        "remove-up": [0x101, 0x41, 0xc01e0001, 3],
        "skip-suppressed": [0x100, 0x44, 0x00200001, 5],
        "waiting-get": [0x100, 0x45, 0x00120001, 6],
        "posted-negative": [0x100, 0x51, 0x12345678, 6],
        "unhooked": [0x100, 0x46, 0x00210001, 7],
      }[hook];
      assert.deepEqual(actual, [read(a.window), ...values, 0, 0], `${hook}: original MSG output and filter survive callbacks`);
    }
  }
  const total = callbackHistory.filter((row) => row.procedure === guest.callbacks[0].entry).length;
  assert.equal(read(a.olderCount), total);
  assert.equal(read(a.newerCount), total);
  if (expected.length) {
    assert.deepEqual(Array.from({ length: 3 }, (_, index) => read(a.olderArguments + index * 4)), expected.at(-1));
    assert.deepEqual(Array.from({ length: 3 }, (_, index) => read(a.newerArguments + index * 4)), expected.at(-1));
    assert.equal(read(a.chainResult), hook === "suppress-noremove" ? 0x51 : 0);
  }
  return { hook, callbacks: rows.length, result: eax, message: call.name.includes("Message") ? message() : undefined };
}
