import assert from 'node:assert/strict';

let input = '';
for await (const chunk of process.stdin) input += chunk;
const fixture = JSON.parse(input);
const bytes = Buffer.from(fixture.module, 'hex');
assert.ok(WebAssembly.validate(bytes), 'private ordinary read helper and branch structure');
const memory = new WebAssembly.Memory({initial: 1, maximum: 1});
const view = new DataView(memory.buffer);
const {exports} = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory}});
const packet = [0x484d3352, 0x10007, 40, 0, 0, 0x202, 0, 0, 0, 4];
const initial = [0, 0, 0x1000, 0xfffffff0, 0, 0x91, 0xa2, 0xb3, 0xc4, 0xd5, 0xe6, 0xf7];
let controls = 0;

function check(name, {words = packet, args = initial, mode = 1, base = 0, descriptor = 0x020fffff, expected, completed = 0, slow = true} = {}) {
  new Uint8Array(memory.buffer).fill(0x5a);
  words.forEach((value, index) => view.setUint32(100 + index * 4, value, true));
  view.setUint16(fixture.segments + 4, mode, true);
  view.setUint32(fixture.segments + 32, descriptor, true);
  view.setUint32(fixture.bases + 20, base, true);
  const before = new Uint8Array(memory.buffer).slice();
  const slowBefore = exports.slowCalls.value;
  const inline = exports.inline(...args).map(value => value >>> 0);
  assert.equal(exports.slowCalls.value, slowBefore, `${name}: original inline body makes no local call`);
  assert.deepEqual(new Uint8Array(memory.buffer), before, `${name}: inline keeps all RAM/FP records`);
  const outlined = exports.outlined(...args).map(value => value >>> 0);
  assert.equal(exports.slowCalls.value - slowBefore, Number(slow), `${name}: only exact success bypasses slow body`);
  assert.deepEqual(outlined, inline, `${name}: all eight output locals and continuation`);
  assert.deepEqual(new Uint8Array(memory.buffer), before, `${name}: outlined keeps all RAM/FP records`);
  if (expected) assert.deepEqual(outlined.slice(0, 5), expected, `${name}: literal terminal result`);
  assert.equal(outlined[8], completed, `${name}: continuation`);
  controls++;
  return outlined;
}

const success = check('success', {expected: [0, 0x91, 0xa2, 0xb3, 0xc4], completed: 1, slow: false});
assert.equal(success[7], initial[11], 'RESULT and flags payload remain caller-owned');
for (const payload of [0, 0x202, 0x80000000, 0xffffffff]) {
  check(`success payload ${payload}`, {words: packet.with(5, payload), completed: 1, slow: false});
}
check('prior reason preserved', {args: initial.with(4, 13), expected: [13, 0x91, 0xa2, 0xb3, 0xc4], slow: false});
for (const status of [1, 2, 7, 11, 0xffffffff]) {
  check(`transport ${status}`, {args: initial.with(1, status), expected: [7, 3, 0xa2, 0xb3, 0xc4]});
}
for (const [field, value] of [[0, 0], [1, 0x1000a], [2, 39], [3, 1], [4, 8], [6, 1], [7, 1], [8, 1], [9, 0], [9, 8]]) {
  check(`malformed success ${field}/${value}`, {words: packet.with(field, value), expected: [7, 2, 0xa2, 0xb3, 0xc4]});
}
const fault = packet.with(4, 1).with(5, 0).with(6, 1).with(7, 0xfffffff0).with(8, 1);
for (const detail of [1, 2]) {
  for (const offset of [0, 3]) {
    const address = (initial[3] + offset) >>> 0;
    check(`read fault ${detail}/${offset}`, {words: fault.with(6, detail).with(7, address), expected: [5, detail, address, 1, 4]});
  }
}
for (const [field, value] of [[5, 1], [6, 0], [6, 3], [7, 0xffffffef], [7, 0xfffffff4], [8, 0], [8, 2], [8, 3], [8, 0x101], [9, 0]]) {
  check(`malformed read fault ${field}/${value}`, {words: fault.with(field, value), expected: [7, 2, 0xa2, 0xb3, 0xc4]});
}
for (const detail of [0, 1, 2, 3]) {
  check(`rejected ${detail}`, {words: packet.with(4, 2).with(5, 0).with(6, detail), expected: [7, detail === 1 ? 1 : detail === 2 ? 3 : 2, 0xa2, 0xb3, 0xc4]});
}
for (const status of [3, 4, 5, 6, 7]) {
  check(`exception ${status}`, {words: packet.with(4, status).with(5, 0).with(6, 0x1234), expected: [status + 8, 0x1234, 0xa2, 0xb3, 0xc4]});
}
const typed = [0x484d3352, 0x1000f, 40, 0, 9, 0, 4, 0xfffffff0, 1, 4];
for (const detail of [4, 5]) {
  for (const offset of [0, 3]) {
    const address = (initial[3] + offset) >>> 0;
    check(`typed read PF ${detail}/${offset}`, {words: typed.with(6, detail).with(7, address), args: initial.with(1, 23), expected: [23, detail, address, 1, 4]});
  }
}
for (const [field, value] of [[0, 0], [1, 0x10007], [2, 39], [3, 1], [4, 0], [5, 1], [6, 0], [6, 6], [6, 8], [7, 0xfffffff4], [8, 2], [8, 0x101], [9, 0]]) {
  check(`typed malformed ${field}/${value}`, {words: typed.with(field, value), args: initial.with(1, 23), expected: [7, 2, 0xa2, 0xb3, 0xc4]});
}
for (const status of [0, 11, 7]) {
  check(`typed transport ${status}`, {words: typed, args: initial.with(1, status), expected: [7, status === 0 ? 2 : 3, 0xa2, 0xb3, 0xc4]});
}
check('transport23 requires typed packet', {args: initial.with(1, 23), expected: [7, 2, 0xa2, 0xb3, 0xc4]});

// the same instantiated function observes live code context on every rejected helper result.
for (const [base, descriptor, reason] of [[0, 0x020fffff, 7], [0x8000, 0x020fffff, 3], [0x8000, 0x020000ff, 11], [0, 0x020fffff, 7]]) {
  check(`current context ${base}/${descriptor}`, {
    mode: 3, base, descriptor, args: initial.with(1, 7),
    expected: [reason, reason === 7 ? 3 : 0, reason === 7 ? 0xa2 : 0, reason === 7 ? 0xb3 : 0, reason === 7 ? 0xc4 : 0],
  });
}
check('current D-bit mismatch', {mode: 3, descriptor: 0x000fffff, args: initial.with(1, 7), expected: [3, 0, 0, 0, 0]});
check('successful read keeps rejection-only check ordering', {mode: 3, base: 0x8000, completed: 1, slow: false});
check('SMC exemption is retained', {mode: 3, base: 0x8000, args: initial.with(1, 11), expected: [7, 3, 0xa2, 0xb3, 0xc4]});
console.log(`x87 read32 inline/outlined differential controls passed (${controls} cases)`);
