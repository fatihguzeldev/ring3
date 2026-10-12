import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync} from 'node:fs';

export function readEngine(path, expectedSha256 = process.env.RING3_ENGINE_SHA256) {
  const bytes = readFileSync(path);
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  if (expectedSha256 !== undefined) {
    assert.match(expectedSha256, /^[a-f0-9]{64}$/, 'expected a lowercase engine SHA-256');
    assert.equal(sha256, expectedSha256, 'engine binary differs from the requested SHA-256');
  }
  return {bytes, sha256, module: new WebAssembly.Module(bytes)};
}
