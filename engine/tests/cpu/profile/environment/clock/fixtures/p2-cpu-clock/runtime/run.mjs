import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, hash, record, words, registers, PC} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root), T = 140, frequency = 1_000_000_000n;
const sourcePaths = [
  'engine/src/process/clock/mod.rs', 'engine/src/process/clock/tests.rs',
  'engine/src/process/cpu/integer.rs', 'engine/src/process/wasm/clock.rs',
  'engine/src/abi/wasm/exports/clock.rs', 'engine/src/cpu/x86/capabilities.rs',
  'engine/tests/cpu/profile/environment/clock/wasm.rs',
  'engine/tests/cpu/profile/environment/clock/fixtures/p2-cpu-clock/runtime/run.mjs',
];
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), checks = [];
const wide = value => [Number(value & 0xffff_ffffn), Number(value >> 32n)];
const clock = (c, method, value) => h.host(c, method, wide(value));
function seed(pc = PC) {
  const input = Buffer.from(h.initial);
  input.set(record('R3ST', 56, [...registers, pc, 2], 3));
  for (let offset = T; offset < 4236; offset++) input[offset] = (offset * 17 + 3) & 255;
  return input;
}
function completed(input, ticks, auxiliary, pc, retired = 1, flags = 2, payloadFlags = 2) {
  const want = Buffer.from(input);
  want.writeUInt32LE(Number(ticks & 0xffff_ffffn), 16);
  want.writeUInt32LE(Number(ticks >> 32n), 24);
  if (auxiliary) want.writeUInt32LE(0, 20);
  want.writeUInt32LE(pc, 48); want.writeUInt32LE(flags, 52);
  want.set(record('R3EX', 40, [1, retired, 0, 0, 0, 0], 6), 56);
  want.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0], 7), 100);
  want.set(words([input.readUInt32LE(T), payloadFlags, Number(ticks & 0xffff_ffffn),
    Number(ticks >> 32n), input.readUInt32LE(28), auxiliary ? 0 : input.readUInt32LE(20)]), T);
  return want;
}
const helper = c => c.resident ? 'cpu_integer_resident' : 'cpu_integer';
const helperCall = (c, material, descriptor, status = 0) => ({name: helper(c),
  args: [...(c.resident ? material.binding : []), descriptor, 0], status});
function sampled(c, material, sample) {
  const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, material.pointer, material.length));
  const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(row => row.kind === 'function').map(row =>
    [row.name, (...args) => {
      if (row.name === helper(c)) clock(c, 'observe_cpu_clock', sample());
      const status = c.api[row.name](...args);
      c.calls.push({name: row.name, args: args.map(value => typeof value === 'bigint' ? value.toString() : value >>> 0),
        status: typeof status === 'bigint' ? status.toString() : status});
      return status;
    }]));
  return {...material, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run};
}
for (const resident of [false, true]) {
  const c = h.open(resident, false, 'missing host clock is infrastructure rejection', [[PC, 7]]);
  h.upload(c, PC, Buffer.from('0f31ebfe', 'hex'));
  const material = h.compile(c, 'unconfigured RDTSC', [[PC, 2], [PC + 2, 2]], [helper(c)]);
  assert.ok(material);
  const input = seed(), want = Buffer.from(input);
  want.set(record('R3EX', 40, [7, 0, 3, 0, 0, 0], 6), 56);
  want.set(words([input.readUInt32LE(T), 2, registers[0], registers[2], registers[3], registers[1]]), T);
  h.observe(c, 'missing clock does not invent guest time or an architectural exception', {input, want, material,
    helpers: [helperCall(c, material, 0x20000, 9)]});
  h.host(c, 'close');
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  const c = h.open(resident, entries, 'host-observed clock', [[PC, 7]]);
  h.host(c, 'configure_cpu_clock', [...wide(frequency), ...wide(100n)]);
  h.upload(c, PC, Buffer.from('0f310f01f9ebfe', 'hex'));
  const material = h.compile(c, 'RDTSC and RDTSCP', [[PC, 2], [PC + 2, 3], [PC + 5, 2]], [helper(c)]);
  assert.ok(material);
  clock(c, 'observe_cpu_clock', 117n);
  for (const [pc, auxiliary] of [[PC, false], [PC + 2, true], [PC, false]]) {
    const input = seed(pc), want = completed(input, 17n, auxiliary, pc + (auxiliary ? 3 : 2));
    h.observe(c, 'repeated reads retain the same observed timestamp', {input, want, material,
      helpers: [helperCall(c, material, auxiliary ? 0x20001 : 0x20000)]});
  }
  for (const [method, value] of [['observe_cpu_clock', 116n], ['advance_cpu_clock', 0xffff_ffff_ffff_ffffn]]) {
    const before = h.arena(c);
    assert.equal(c.api[method](...wide(value)), 7);
    assert.deepEqual(h.arena(c), before, 'invalid time input does not mutate the arena');
    checks.push({profile: [resident, entries], method, rejected: value.toString()});
  }
  clock(c, 'advance_cpu_clock', 5n);
  let input = seed(), want = completed(input, 22n, false, PC + 2);
  h.observe(c, 'explicit host advancement reaches the same clock', {input, want, material,
    helpers: [helperCall(c, material, 0x20000)]});
  clock(c, 'observe_cpu_clock', 100n + (1n << 32n));
  input = seed(PC + 2); want = completed(input, 1n << 32n, true, PC + 5);
  h.observe(c, 'observed time carries into EDX and retains TSC_AUX', {input, want, material,
    helpers: [helperCall(c, material, 0x20001)]});
  h.host(c, 'close');
  assert.equal(c.api.observe_cpu_clock(0, 0), 5);
}

for (const resident of [false, true]) {
  const c = h.open(resident, false, 'polling loop advances only on host samples', [[PC, 7]]);
  h.host(c, 'configure_cpu_clock', [...wide(frequency), 0, 0]);
  h.upload(c, PC, Buffer.from('0f3185c074faebfe', 'hex'));
  const material = h.compile(c, 'RDTSC TEST JZ loop', [[PC, 6], [PC + 6, 2]], [helper(c)]);
  assert.ok(material);
  const samples = [0n, 0n, 5n], observed = [];
  const bound = sampled(c, material, () => {const value = samples.shift(); assert.notEqual(value, undefined); observed.push(value.toString()); return value;});
  const input = seed(), want = completed(input, 5n, false, PC + 6, 10, 6, 0x46);
  h.observe(c, 'guest polling loop leaves after a real host-supplied advance', {input, want, material: bound, budget: 10,
    helpers: Array.from({length: 3}, () => helperCall(c, material, 0x20000))});
  assert.deepEqual(observed, ['0', '0', '5']); checks.push({polling: resident, observed});
  h.host(c, 'close');
}

const c = h.open(false, false, 'real monotonic host sample', [[PC, 7]]), epoch = BigInt(Math.floor(performance.now() * 1_000_000));
h.host(c, 'configure_cpu_clock', [...wide(frequency), ...wide(epoch)]);
h.upload(c, PC, Buffer.from('0f31ebfe', 'hex'));
const material = h.compile(c, 'real host RDTSC sample', [[PC, 2], [PC + 2, 2]], [helper(c)]);
assert.ok(material);
const samples = [], bound = sampled(c, material, () => {const value = BigInt(Math.floor(performance.now() * 1_000_000)); samples.push(value); return value;});
let previous = 0n;
for (let index = 0; index < 3; index++) {
  const input = seed(); h.put(c, 0, input);
  assert.equal(bound.run(c.base, c.base + 56, 1, c.base + 96), 0);
  const actual = h.arena(c), ticks = samples.at(-1) - epoch;
  assert.ok(ticks >= previous); previous = ticks;
  assert.deepEqual(actual, completed(input, ticks, false, PC + 2), 'real host sample has the exact complete CPU/helper result');
}
checks.push({hostSamples: samples.map(value => value.toString()), epoch: epoch.toString()});
h.host(c, 'close');
assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'clock-checks.json'), JSON.stringify({sourcePins, checks}, null, 2) + '\n');
h.finish(['host-observed virtual time and generated RDTSC/RDTSCP semantics for one virtual CPU; no QPC, Windows scheduler, clock accuracy or throughput claim']);
