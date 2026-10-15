import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const cases = JSON.parse(readFileSync(0, 'utf8'));
for (const row of cases) {
  const module = new WebAssembly.Module(Buffer.from(row.module, 'hex'));
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const memory = new WebAssembly.Memory({initial: 1, maximum: 1});
  const bytes = new Uint8Array(memory.buffer); bytes.fill(0xa5);
  const state = 16, bases = [64, 256];
  for (let register = 0; register < 8; register++) {
    for (let half = 0; half < 2; half++) {
      for (let lane = 0; lane < 16; lane++) bytes[state + bases[half] + register * 16 + lane] = (register * 43 + half * 67 + lane * 13) & 255;
    }
  }
  const before = bytes.slice(), expected = bytes.slice();
  for (let half = 0; half < 2; half++) {
    for (let lane = 0; lane < 16; lane++) {
      const destination = state + bases[half] + row.destination * 16 + lane;
      if (half === 1 && row.width === 16) { expected[destination] = 0; continue; }
      const first = before[state + bases[half] + row.first * 16 + lane];
      const second = before[state + bases[half] + row.second * 16 + lane];
      expected[destination] = row.kind === 'and' ? first & second : row.kind === 'and_not' ? ~first & second : row.kind === 'or' ? first | second : first ^ second;
    }
  }
  const instance = new WebAssembly.Instance(module, {env: {memory}});
  assert.equal(instance.exports.run.length, 1);
  instance.exports.run(state);
  assert.deepEqual(bytes, expected, `${row.kind}/${row.width}/${row.destination},${row.first},${row.second}: full memory`);
}
assert.equal(cases.length, 40);
console.log('40 actual logical modules passed');
