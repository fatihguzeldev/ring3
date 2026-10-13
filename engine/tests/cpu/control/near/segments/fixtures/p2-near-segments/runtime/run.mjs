import assert from 'node:assert/strict';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {join, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'selected engine, output and repository root are required');
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/, 'select an explicit engine SHA-256');
const initial = join(output, 'initial-arena.bin');
assert.equal(readFileSync(initial).length, 4796, 'Rust initializer supplies the complete current arena');
const runtime = dirname(fileURLToPath(import.meta.url)), groups = {};
for (const name of ['direct', 'indirect']) {
  const destination = join(output, name); mkdirSync(destination, {recursive: true});
  const child = spawnSync(process.execPath, [join(runtime, `${name}.mjs`), enginePath, initial, root, destination],
    {encoding: 'utf8', env: process.env, maxBuffer: 20 * 1024 * 1024});
  writeFileSync(join(destination, 'node.stdout.log'), child.stdout ?? '');
  writeFileSync(join(destination, 'node.stderr.log'), child.stderr ?? '');
  assert.ifError(child.error);
  assert.ok(child.status === 0 || child.status === 1, `${name}: fixture exited abnormally: ${child.stderr}`);
  const result = JSON.parse(readFileSync(join(destination, 'result.json')));
  assert.equal(result.engine_sha256, process.env.RING3_ENGINE_SHA256, `${name}: selected artifact`);
  assert.equal(result.status === 'PASS', child.status === 0, `${name}: receipt and exit agree`);
  groups[name] = result;
}
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const result = {
  status: Object.values(groups).every(group => group.status === 'PASS') ? 'PASS' : 'FAIL',
  engine_sha256: process.env.RING3_ENGINE_SHA256,
  initial_arena_sha256: hash(readFileSync(initial)),
  fixture_sha256: Object.fromEntries(['run', 'direct', 'indirect'].map(name => [name, hash(readFileSync(join(runtime, `${name}.mjs`)))])),
  groups: Object.fromEntries(Object.entries(groups).map(([name, group]) => [name, {status: group.status, counts: group.counts, source_sha256: group.source_sha256, receipt: `${name}/result.json`}]))
};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify(result)); process.exitCode = result.status === 'PASS' ? 0 : 1;
