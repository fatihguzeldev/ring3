import assert from 'node:assert/strict';

let input = '';
for await (const chunk of process.stdin) input += chunk;
const fixture = JSON.parse(input);
const bytes = Buffer.from(fixture.module, 'hex');
assert.ok(WebAssembly.validate(bytes), 'private ordinary write helper and branch structure');
const memory = new WebAssembly.Memory({initial: 1, maximum: 1});
const view = new DataView(memory.buffer);
const {exports} = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory}});
const packet = [0x484d3352, 0x10007, 40, 0, 0, 0x202, 0, 0, 0, 4];
const initial = [0, 0, 0x1000, 0xfffffff0, 0, 0x91, 0xa2, 0xb3, 0xc4, 0xd5, 0xe6, 0xf7];
let controls = 0;

function check(name, {words = packet, args = initial, mode = 1, base = 0, descriptor = 0x020fffff, expected, completed = 0, slow = true} = {}) {
  new Uint8Array(memory.buffer).fill(0x5a);
  const pointer = args[0] >>> 0;
  words.forEach((value, index) => view.setUint32(pointer + 100 + index * 4, value, true));
  view.setUint16(pointer + fixture.segments + 4, mode, true);
  view.setUint32(pointer + fixture.segments + 32, descriptor, true);
  view.setUint32(pointer + fixture.bases + 20, base, true);
  const before = new Uint8Array(memory.buffer).slice();
  const slowBefore = exports.slowCalls.value;
  const inline = exports.inline(...args).map(value => value >>> 0);
  assert.equal(exports.slowCalls.value, slowBefore, `${name}: original inline body makes no local call`);
  assert.deepEqual(new Uint8Array(memory.buffer), before, `${name}: inline keeps all RAM/FP records`);
  const outlined = exports.outlined(...args).map(value => value >>> 0);
  assert.equal(exports.slowCalls.value - slowBefore, Number(slow), `${name}: only exact ordinary success bypasses slow body`);
  assert.deepEqual(outlined, inline, `${name}: all eight output locals and continuation`);
  assert.deepEqual(new Uint8Array(memory.buffer), before, `${name}: outlined keeps all RAM/FP records`);
  if (expected) assert.deepEqual(outlined.slice(0, 5), expected, `${name}: literal terminal result`);
  assert.equal(outlined[8], completed, `${name}: continuation`);
  controls++;
  return outlined;
}

const success = check('success', {expected: [0, 0x91, 0xa2, 0xb3, 0xc4], completed: 1, slow: false});
assert.deepEqual(success.slice(5, 8), [0, 0, initial[11]], 'success clears check scratch and preserves RESULT');
for (const payload of [0, 0x202, 0x80000000, 0xffffffff]) {
  check(`success payload ${payload}`, {words: packet.with(5, payload), completed: 1, slow: false});
}
check('prior reason preserved', {args: initial.with(4, 13), expected: [13, 0x91, 0xa2, 0xb3, 0xc4], slow: false});
for (const pointer of [1, 4, 7]) {
  check(`unaligned packet base ${pointer}`, {args: initial.with(0, pointer), expected: [0, 0x91, 0xa2, 0xb3, 0xc4], completed: 1, slow: false});
}
for (let byte = 0; byte < 40; byte++) {
  if (byte >= 20 && byte < 24) continue;
  const field = byte >>> 2, bit = 1 << ((byte & 3) * 8);
  check(`success contract byte ${byte}`, {words: packet.with(field, (packet[field] ^ bit) >>> 0), expected: [7, 2, 0xa2, 0xb3, 0xc4]});
}
for (const status of [1, 2, 3, 4, 7, 0xffffffff]) {
  check(`transport ${status}`, {args: initial.with(1, status), expected: [7, 3, 0xa2, 0xb3, 0xc4]});
}
for (const [field, value] of [[0, 0], [1, 0x1000a], [2, 39], [3, 1], [4, 8], [6, 1], [7, 1], [8, 1], [9, 0], [9, 8]]) {
  check(`malformed success ${field}/${value}`, {words: packet.with(field, value), expected: [7, 2, 0xa2, 0xb3, 0xc4]});
}

for (const payload of [0, 0x202, 0x80000000, 0xffffffff]) {
  check(`SMC success payload ${payload}`, {
    words: packet.with(5, payload), args: initial.with(1, 11),
    expected: [0, 0x91, 0xa2, 0xb3, 0xc4], completed: 1,
  });
}
check('SMC success preserves prior terminal fields', {args: initial.with(1, 11).with(4, 13), expected: [13, 0x91, 0xa2, 0xb3, 0xc4]});
for (const [field, value] of [[0, 0], [1, 0x1000a], [2, 39], [3, 1], [4, 1], [4, 2], [4, 3], [4, 9], [6, 1], [7, 1], [8, 2], [9, 0]]) {
  check(`malformed SMC ${field}/${value}`, {words: packet.with(field, value), args: initial.with(1, 11), expected: [7, 2, 0xa2, 0xb3, 0xc4]});
}

const fault = packet.with(4, 1).with(5, 0).with(6, 1).with(7, initial[3]).with(8, 2);
for (const detail of [1, 2]) {
  for (const offset of [0, 3]) {
    const address = (initial[3] + offset) >>> 0;
    check(`write fault ${detail}/${offset}`, {words: fault.with(6, detail).with(7, address), expected: [5, detail, address, 2, 4]});
  }
}
check('write fault span wraps at u32 boundary', {words: fault.with(7, 0), args: initial.with(3, 0xfffffffe), expected: [5, 1, 0, 2, 4]});
for (const [field, value] of [[5, 1], [6, 0], [6, 3], [7, 0xffffffef], [7, 0xfffffff4], [8, 0], [8, 1], [8, 3], [8, 0x102], [9, 0]]) {
  check(`malformed write fault ${field}/${value}`, {words: fault.with(field, value), expected: [7, 2, 0xa2, 0xb3, 0xc4]});
}
check('SMC cannot also report a valid legacy fault', {words: fault, args: initial.with(1, 11), expected: [7, 2, 0xa2, 0xb3, 0xc4]});
for (const detail of [0, 1, 2, 3]) {
  check(`rejected ${detail}`, {words: packet.with(4, 2).with(5, 0).with(6, detail), expected: [7, detail === 1 ? 1 : detail === 2 ? 3 : 2, 0xa2, 0xb3, 0xc4]});
}
for (const status of [3, 4, 5, 6, 7]) {
  for (const detail of [0, 0x1234, 0x10000]) {
    const malformed = (status === 4 || status === 5) && detail > 0xffff;
    check(`exception ${status}/${detail}`, {
      words: packet.with(4, status).with(5, 0).with(6, detail),
      expected: [malformed ? 7 : status + 8, malformed ? 2 : detail, 0xa2, 0xb3, 0xc4],
    });
  }
}

const typed = [0x484d3352, 0x1000f, 40, 0, 9, 0, 6, initial[3], 2, 4];
for (const detail of [6, 7]) {
  for (const offset of [0, 3]) {
    const address = (initial[3] + offset) >>> 0;
    check(`typed write PF ${detail}/${offset}`, {words: typed.with(6, detail).with(7, address), args: initial.with(1, 23), expected: [23, detail, address, 2, 4]});
  }
}
check('typed write PF span wraps at u32 boundary', {words: typed.with(7, 0), args: initial.with(1, 23).with(3, 0xfffffffe), expected: [23, 6, 0, 2, 4]});
for (const [field, value] of [[0, 0], [1, 0x10007], [2, 39], [3, 1], [4, 0], [4, 1], [5, 1], [6, 0], [6, 2], [6, 4], [6, 5], [6, 8], [7, 0xffffffef], [7, 0xfffffff4], [8, 0], [8, 1], [8, 3], [8, 0x102], [9, 0], [9, 8]]) {
  check(`typed malformed ${field}/${value}`, {words: typed.with(field, value), args: initial.with(1, 23), expected: [7, 2, 0xa2, 0xb3, 0xc4]});
}
for (const status of [0, 11, 7]) {
  check(`typed transport ${status}`, {words: typed, args: initial.with(1, status), expected: [7, status === 7 ? 3 : 2, 0xa2, 0xb3, 0xc4]});
}
check('transport23 requires typed packet', {args: initial.with(1, 23), expected: [7, 2, 0xa2, 0xb3, 0xc4]});

// the same function must read live code context, while successful SMC retains its exemption.
for (const [base, descriptor, reason] of [[0, 0x020fffff, 7], [0x8000, 0x020fffff, 3], [0x8000, 0x020000ff, 11], [0, 0x020fffff, 7]]) {
  check(`current context ${base}/${descriptor}`, {
    mode: 3, base, descriptor, args: initial.with(1, 7),
    expected: [reason, reason === 7 ? 3 : 0, reason === 7 ? 0xa2 : 0, reason === 7 ? 0xb3 : 0, reason === 7 ? 0xc4 : 0],
  });
}
check('current D-bit mismatch', {mode: 3, descriptor: 0x000fffff, args: initial.with(1, 7), expected: [3, 0, 0, 0, 0]});
check('typed PF retains live context rejection priority', {mode: 3, base: 0x8000, words: typed, args: initial.with(1, 23), expected: [3, 0, 0, 0, 0]});
check('successful write keeps rejection-only check ordering', {mode: 3, base: 0x8000, completed: 1, slow: false});
check('SMC success keeps context-check exemption', {mode: 3, base: 0x8000, args: initial.with(1, 11), expected: [0, 0x91, 0xa2, 0xb3, 0xc4], completed: 1});
check('malformed SMC keeps context-check exemption', {mode: 3, base: 0x8000, words: packet.with(0, 0), args: initial.with(1, 11), expected: [7, 2, 0xa2, 0xb3, 0xc4]});
console.log(`x87 write32 inline/outlined differential controls passed (${controls} cases)`);
