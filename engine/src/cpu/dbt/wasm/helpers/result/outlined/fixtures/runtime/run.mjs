import assert from 'node:assert/strict';

let input = '';
for await (const chunk of process.stdin) input += chunk;
const fixture = JSON.parse(input);
const bytes = Buffer.from(fixture.module, 'hex');
assert.ok(WebAssembly.validate(bytes), 'private multivalue helper and branch structure');
const memory = new WebAssembly.Memory({initial: 1, maximum: 1});
const view = new DataView(memory.buffer);
const {exports} = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory}});
const packet = [0x484d3352, 0x10007, 40, 0, 0, 0x202, 0, 0, 0, 0];
const initial = [0, 0, 0x1000, 0xfffffff0, 0, 0x91, 0xa2, 0xb3, 0xc4, 0xd5, 0xe6, 0xf7];
let controls = 0;

function check(name, {words = packet, args = initial, mode = 1, base = 0, descriptor = 0x020fffff, expected, completed, slow = true} = {}) {
  new Uint8Array(memory.buffer).fill(0x5a);
  words.forEach((value, index) => view.setUint32(100 + index * 4, value, true));
  view.setUint16(fixture.segments + 4, mode, true);
  view.setUint32(fixture.segments + 32, descriptor, true);
  view.setUint32(fixture.bases + 20, base, true);
  const before = new Uint8Array(memory.buffer).slice();
  const slowBefore = exports.slowCalls.value;
  const inline = exports.inline(...args).map(value => value >>> 0);
  assert.equal(exports.slowCalls.value, slowBefore, `${name}: retained inline validator makes no local call`);
  assert.deepEqual(new Uint8Array(memory.buffer), before, `${name}: inline is read-only, including FP/CPU records`);
  const outlined = exports.outlined(...args).map(value => value >>> 0);
  assert.equal(exports.slowCalls.value - slowBefore, Number(slow), `${name}: exact success avoids the slow function`);
  assert.deepEqual(outlined, inline, `${name}: all eight modified locals and exit branch`);
  assert.deepEqual(new Uint8Array(memory.buffer), before, `${name}: outlined is read-only, including FP/CPU records`);
  if (expected) assert.deepEqual(outlined.slice(0, 5), expected, `${name}: literal terminal outcome`);
  if (completed !== undefined) assert.equal(outlined[8], completed, `${name}: caller continuation`);
  controls++;
  return outlined;
}

const success = check('success preserves prior fields', {expected: [0, 0x91, 0xa2, 0xb3, 0xc4], completed: 1, slow: false});
assert.equal(success[7], initial[11], 'success preserves RESULT scratch');
check('success with an existing reason still exits', {args: initial.with(4, 13), expected: [13, 0x91, 0xa2, 0xb3, 0xc4], completed: 0, slow: false});
check('success payload is left for caller flags validation', {words: packet.with(5, 0xffffffff), expected: [0, 0x91, 0xa2, 0xb3, 0xc4], completed: 1, slow: false});
check('same protected context reads the current descriptor', {mode: 3, expected: [0, 0x91, 0xa2, 0xb3, 0xc4], completed: 1, slow: false});

for (const status of [1, 2, 3, 4, 5, 6, 7, 0xffffffff]) {
  check(`transport ${status}`, {args: initial.with(1, status), expected: [7, 3, 0xa2, 0xb3, 0xc4], completed: 0});
}
check('SMC is not permitted', {args: initial.with(1, 11), expected: [7, 3, 0xa2, 0xb3, 0xc4], completed: 0});
check('zero span cannot report a memory fault', {words: packet.with(4, 1), expected: [7, 2, 0xa2, 0xb3, 0xc4], completed: 0});
for (const detail of [0, 1, 2, 3]) {
  check(`rejected ${detail}`, {words: packet.with(4, 2).with(5, 0).with(6, detail), expected: [7, detail === 1 ? 1 : detail === 2 ? 3 : 2, 0xa2, 0xb3, 0xc4], completed: 0});
}
check('rejection with a success payload is malformed before detail handling', {words: packet.with(4, 2).with(6, 1), expected: [7, 2, 0xa2, 0xb3, 0xc4], completed: 0});
for (const status of [3, 4, 5, 6, 7]) {
  for (const detail of [0, 0x1234, 0x10000]) {
    const malformed = (status === 4 || status === 5) && detail > 0xffff;
    check(`exception ${status}/${detail}`, {
      words: packet.with(4, status).with(5, 0).with(6, detail),
      expected: [malformed ? 7 : status + 8, malformed ? 2 : detail, 0xa2, 0xb3, 0xc4], completed: 0,
    });
  }
}
for (const [field, value] of [[0, 0], [1, 0x10008], [2, 39], [3, 1], [6, 1], [7, 1], [8, 1], [9, 1], [4, 8], [4, 0xffffffff]]) {
  check(`malformed legacy field ${field}`, {words: packet.with(field, value), expected: [7, 2, 0xa2, 0xb3, 0xc4], completed: 0});
}
const typed = [0x484d3352, 0x1000f, 40, 0, 9, 0, 4, 0, 1, 0];
for (const status of [0, 11, 23, 7]) {
  check(`typed zero span/status ${status}`, {words: typed, args: initial.with(1, status), expected: [7, status === 0 || status === 23 ? 2 : 3, 0xa2, 0xb3, 0xc4], completed: 0});
}
for (let field = 0; field < typed.length; field++) {
  check(`typed malformed field ${field}`, {words: typed.with(field, typed[field] ^ 0x55), args: initial.with(1, 23), expected: [7, 2, 0xa2, 0xb3, 0xc4], completed: 0});
}
check('transport23 rejects a legacy packet before legacy handling', {args: initial.with(1, 23), expected: [7, 2, 0xa2, 0xb3, 0xc4], completed: 0});

// use the same instantiated helper repeatedly while live segment data changes between calls.
for (const [base, descriptor, reason] of [[0, 0x020fffff, 7], [0x8000, 0x020fffff, 3], [0x8000, 0x020000ff, 11], [0, 0x020fffff, 7]]) {
  check(`live context ${base}/${descriptor}`, {
    mode: 3, base, descriptor, args: initial.with(1, 7),
    expected: [reason, reason === 7 ? 3 : 0, reason === 7 ? 0xa2 : 0, reason === 7 ? 0xb3 : 0, reason === 7 ? 0xc4 : 0], completed: 0,
  });
}
check('D-bit mismatch also invalidates rejected execution', {mode: 3, descriptor: 0x000fffff, args: initial.with(1, 7), expected: [3, 0, 0, 0, 0], completed: 0});
check('successful status does not hoist a rejection-only context check', {mode: 3, base: 0x8000, expected: [0, 0x91, 0xa2, 0xb3, 0xc4], completed: 1, slow: false});
check('SMC status keeps its original context-check exemption', {mode: 3, base: 0x8000, args: initial.with(1, 11), expected: [7, 3, 0xa2, 0xb3, 0xc4], completed: 0});
assert.ok(controls >= 60, 'failure families and currentness controls were exercised');
console.log('zero-span inline/outlined differential controls passed');
