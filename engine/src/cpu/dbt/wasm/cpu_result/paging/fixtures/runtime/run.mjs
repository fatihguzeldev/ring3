import assert from 'node:assert/strict';
let input = '';
for await (const chunk of process.stdin) input += chunk;
const rows = JSON.parse(input);
assert.equal(rows.length, 21);
for (const [index, row] of rows.entries()) {
  const bytes = Buffer.from(row.module, 'hex');
  assert.ok(WebAssembly.validate(bytes), `control ${index}: generated structure`);
  const memory = new WebAssembly.Memory({initial: 1, maximum: 1});
  const view = new DataView(memory.buffer);
  new Uint8Array(memory.buffer).set(Buffer.from('R3MH'), 100);
  view.setUint32(104, 0x10000 | row.version, true);
  view.setUint32(108, 40, true);
  row.fields.forEach((value, field) => view.setUint32(116 + field * 4, value, true));
  const before = new Uint8Array(memory.buffer).slice();
  const instance = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory}});
  instance.exports.run();
  const actual = Array.from({length: 9}, (_, field) => view.getUint32(32768 + field * 4, true));
  assert.deepEqual(actual, [...row.expected, 0x1000, 0], `control ${index}: literal outcome and zero progress`);
  new Uint8Array(memory.buffer).fill(0, 32768, 32768 + 36);
  assert.deepEqual(new Uint8Array(memory.buffer), before, `control ${index}: helper and all RAM unchanged`);
}
console.log('21 generated application memory packet controls passed');
