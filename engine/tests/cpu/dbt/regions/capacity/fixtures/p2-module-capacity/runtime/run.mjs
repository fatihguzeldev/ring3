import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, state, exit, record, hash} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const owner = 'engine/tests/cpu/dbt/regions/capacity/fixtures/p2-module-capacity/runtime/run.mjs';
const pins = Object.fromEntries([owner, 'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/resident.rs'].map(path => [path, hash(readFileSync(join(root, path)))]));
const RF = 0x10000, TF = 0x100;
function seed(pc, eax = 0x12340000, flags = 2 | RF, cancel = 0, shadow = 0) {
  const regs = [eax, 0x22334455, 0x33445566, 0x5000, 0x7080, 0x55667788, 0x66778899, 0x778899aa];
  const input = state(h.initial, {pc, regs, flags, cancel}); input.writeUInt32LE(shadow, 12); return input;
}
function expected(input, reason, retired, pc, eax = input.readUInt32LE(16), flags = input.readUInt32LE(52) & ~RF, width = 0) {
  const want = Buffer.from(input); want.writeUInt32LE(pc, 48); want.writeUInt32LE(eax >>> 0, 16);
  want.writeUInt32LE(flags >>> 0, 52); if (retired) want.writeUInt32LE(0, 12);
  if (width) want.set(record('R3MH', 40, [0, 7, 0, 0, 0, 2], 10), 100);
  return exit(want, reason, retired, reason >= 16 ? 7 : width ? 6 : 1);
}
function invoke(c, label, material, input, want, options = {}) {
  if (!material) { h.skip(c, label, 'required automatic prefix admission'); return; }
  h.observe(c, label, {material, input, want, ...options});
}
function jump(from, target) { const bytes = Buffer.alloc(5); bytes[0] = 0xe9; bytes.writeInt32LE(target - from - 5, 1); return bytes; }
function capture(c, label, pc, helpers, stopped) {
  return h.compile(c, label, [[pc, 1]], helpers, {stopped});
}
function begin(c, pc, stopped) {
  if (!stopped) { h.put(c, 0, seed(pc)); return; }
  h.upload(c, 0x1000, jump(0x1000, pc)); h.put(c, 0, seed(0x1000));
  const material = h.compile(c, 'real bootstrap to uncaptured legal long block', [[0x1000, 5]]);
  const input = seed(0x1000);
  invoke(c, 'bootstrap actually retires and reaches NeedCode', material, input, expected(input, 3, 1, pc), {budget: 2});
}
function controls(c, material, pc, word) {
  for (const cancel of [0, 1]) {
    const input = seed(pc, 0x12340000, 2 | RF | TF, cancel, 1);
    invoke(c, 'zero budget/cancel precedes instruction begin on a shortened module', material, input,
      exit(Buffer.from(input), cancel ? 2 : 1, 0, word ? 6 : 1), {budget: 0, kind: 'control'});
  }
  const input = seed(pc, 0x12340000, 2 | RF | TF);
  invoke(c, 'TF traps after the first retained instruction, before the prefix boundary', material, input,
    expected(input, 16, 1, pc + (word ? 3 : 1), word ? 0x12340007 : 0x12340000, 2 | TF, word ? 2 : 0),
    {budget: 2, helpers: word ? [{name: 'cpu_read16', args: [0x5000], status: 0}] : []});
}
function strictWhole(c, pc, length) {
  const entries = c.entries; c.entries = false;
  h.compile(c, 'explicit whole unit still refuses instead of silently truncating', [[pc, length]], [], {expectedStatus: 10});
  c.entries = entries;
}
for (const resident of [false, true]) for (const stopped of [false, true]) {
  {
    const c = h.open(resident, !stopped, 'instruction quota prefix + missing successor repair', [[0x1000, 7], [0x4000, 7], [0x5000, 3]]);
    h.upload(c, 0x1fc0, Buffer.alloc(64, 0x90)); h.upload(c, 0x4000, Buffer.from('ebfe', 'hex'));
    begin(c, 0x1fc0, stopped); strictWhole(c, 0x1fc0, 65);
    const first = capture(c, 'automatic prefix stops before fetching absent instruction65', 0x1fc0, [], stopped);
    if (!first) { h.skip(c, 'continuation repair and both following prefixes', 'first automatic capture'); h.host(c, 'close'); continue; }
    controls(c, first, 0x1fc0, false);
    const input = seed(0x1fc0), want = expected(input, 3, 64, 0x2000);
    invoke(c, '64 NOPs retire then exact fallthrough NeedCode', first, input, want, {budget: 129});
    h.compile(c, 'missing continuation still refuses neutrally', [], [], {stopped: true, expectedStatus: 10});
    h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
    h.upload(c, 0x2000, Buffer.concat([Buffer.alloc(64, 0x90), jump(0x2040, 0x4000)]));
    invoke(c, 'mapping/uploading only an unfetched page preserves the held prefix', first, input, want, {budget: 129, kind: 'currentness-control'});
    const second = capture(c, 'real NeedCode captures the next64 without dropping continuation', 0x2000, [], true);
    const next = seed(0x2000);
    invoke(c, 'next64 retire exactly once', second, next, expected(next, 3, 64, 0x2040), {budget: 129});
    if (!second) { h.host(c, 'close'); continue; }
    const tail = capture(c, 'final branch remains a separate real continuation', 0x2040, [], true), last = seed(0x2040);
    invoke(c, 'terminal branch reaches the original target', tail, last, expected(last, 3, 1, 0x4000), {budget: 2});
    h.host(c, 'close');
  }
  {
    const c = h.open(resident, !stopped, 'byte quota word-read prefix + real remainder', [[0x1000, 7], [0x3000, 7], [0x4000, 7], [0x5000, 3]]);
    const body = Buffer.from('660303', 'hex');
    h.upload(c, 0x3000, Buffer.concat([...Array(63).fill(body), jump(0x30bd, 0x4000)]));
    h.upload(c, 0x4000, Buffer.from('ebfe', 'hex')); h.upload(c, 0x5000, Buffer.from([7, 0]));
    begin(c, 0x3000, stopped); strictWhole(c, 0x3000, 194);
    const first = capture(c, 'byte-budget retry retains a literal45-word-read prefix', 0x3000, ['cpu_read16'], stopped);
    if (!first) { h.skip(c, 'word-read remainder', 'first automatic byte capture'); h.host(c, 'close'); continue; }
    assert.ok(first.length <= 65536);
    controls(c, first, 0x3000, true);
    const input = seed(0x3000);
    invoke(c, '45 reads update AX and flags then stop at the exact original fallthrough', first, input,
      expected(input, 3, 45, 0x3087, 0x1234013b, 2, 2), {budget: 100, helpers: Array.from({length: 45}, () => ({name: 'cpu_read16', args: [0x5000], status: 0}))});
    const tail = capture(c, 'real NeedCode captures remaining18 operations and original branch', 0x3087, ['cpu_read16'], true);
    const next = seed(0x3087, 0x1234013b, 2 | RF);
    invoke(c, 'remaining18 reads plus branch finish all63 original operations once', tail, next,
      expected(next, 3, 19, 0x4000, 0x123401b9, 2, 2), {budget: 100, helpers: Array.from({length: 18}, () => ({name: 'cpu_read16', args: [0x5000], status: 0}))});
    h.host(c, 'close');
  }
}
for (const [path, pin] of Object.entries(pins)) assert.equal(hash(readFileSync(join(root, path))), pin);
h.finish([
  'four automatic entry/stopped by replacement/resident profiles; explicit whole extents retain their original typed resource refusal',
  'instruction65 is initially unmapped; complete real continuation repair proves capture ends before it and preserves held-prefix physical identity',
  'literal45+18 memory-read retirement, AX/FLAGS, R3MHv10 and R3EXv6; no production module metadata supplies the oracle',
  'real guards/helpers, full arena and every mapped RAM page; RF/TF and zero-budget/cancel before instruction begin',
  'bounded datawrap selected-codegen prefix oracle; no general performance, full family or interpreter claim',
]);
