import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';

const [baselinePath, currentPath, output] = process.argv.slice(2);
assert.ok(output, 'node wasm.mjs baseline.wasm current.wasm result.json');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const read = path => {
  const bytes = readFileSync(path);
  return {path, sha256: hash(bytes), module: new WebAssembly.Module(bytes)};
};
const artifacts = {baseline: read(baselinePath), current: read(currentPath)};
const ITERATIONS = 1_000_000, EXPECTED = 499_999_500_000;
function prepare(artifact) {
  const instance = new WebAssembly.Instance(artifact.module, {}), api = instance.exports;
  const start = process.hrtime.bigint();
  assert.equal(api.ring3_abi_v1_open(32, 97, 0), 0);
  assert.equal(api.ring3_abi_v1_map(0x10000, 8, 3), 0);
  const startup = Number(process.hrtime.bigint() - start);
  return {api, memory: api.memory, base: api.ring3_abi_v1_arena_ptr() >>> 0, startup};
}
function hot(e, count) {
  const view = new DataView(e.memory.buffer), read = e.api.ring3_abi_v1_read32, write = e.api.ring3_abi_v1_write32;
  let checksum = 0;
  const start = process.hrtime.bigint();
  for (let index = 0; index < count; index++) {
    const address = 0x10000 + (index & 2047) * 4;
    if (write(address, index) !== 0 || read(address) !== 0) throw new Error('scalar memory operation rejected');
    checksum += view.getUint32(e.base + 120, true);
  }
  return {elapsed_ns: Number(process.hrtime.bigint() - start), checksum};
}
function measure(artifact) {
  const e = prepare(artifact);
  hot(e, 20_000);
  const result = hot(e, ITERATIONS);
  assert.equal(result.checksum, EXPECTED);
  assert.equal(e.api.ring3_abi_v1_close(), 0);
  return {startup_ns: e.startup, ...result};
}
const rounds = [];
for (let round = 0; round < 28; round++) {
  const order = round % 2 ? ['current', 'baseline'] : ['baseline', 'current'], row = {};
  for (const label of order) row[label] = measure(artifacts[label]);
  rounds.push(row);
}
function median(values) { values.sort((a, b) => a - b); return (values[13] + values[14]) / 2; }
const summary = Object.fromEntries(Object.keys(artifacts).map(label => [label, {
  path: artifacts[label].path,
  sha256: artifacts[label].sha256,
  rounds: rounds.length,
  startup_median_ns: median(rounds.map(row => row[label].startup_ns)),
  hot_median_ns: median(rounds.map(row => row[label].elapsed_ns)),
  checksum: EXPECTED,
}]));
const result = {summary, rounds, iterations: ITERATIONS, scope: 'actual Node WebAssembly cdylib scalar host APIs on ordinary nonaliased guest RAM; includes JS/Wasm and ABI/helper overhead; distinct from direct native AddressSpace benchmark and browser/game throughput'};
writeFileSync(output, JSON.stringify(result, null, 2));
console.log(JSON.stringify(summary));
