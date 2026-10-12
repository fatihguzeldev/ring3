import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {describe, it} from 'node:test';

import {readEngine} from './engine.mjs';

const empty = Buffer.from('0061736d01000000', 'hex');
const current = Buffer.from('0061736d01000000000401782a2b', 'hex');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const pinError = error => !(error instanceof WebAssembly.CompileError) && /sha.?256/i.test(error.message);
const malformedPinError = error => error instanceof TypeError || pinError(error);

function withFixture(bytes, action) {
  const directory = mkdtempSync(join(tmpdir(), 'ring3-engine-test-'));
  const path = join(directory, 'engine.wasm');
  const previousPin = process.env.RING3_ENGINE_SHA256;
  try {
    delete process.env.RING3_ENGINE_SHA256;
    writeFileSync(path, bytes);
    return action(path);
  } finally {
    if (previousPin === undefined) delete process.env.RING3_ENGINE_SHA256;
    else process.env.RING3_ENGINE_SHA256 = previousPin;
    rmSync(directory, {recursive: true, force: true});
  }
}

describe('readEngine', {concurrency: false}, () => {
  it('returns the current bytes, module and identity without a historical pin', () => {
    withFixture(empty, path => {
      const first = readEngine(path);
      assert.deepEqual(Buffer.from(first.bytes), empty);
      assert.equal(first.sha256, sha256(empty));
      assert.ok(first.module instanceof WebAssembly.Module);

      writeFileSync(path, current);
      const next = readEngine(path);
      assert.deepEqual(Buffer.from(next.bytes), current);
      assert.equal(next.sha256, sha256(current));
      assert.notEqual(next.sha256, first.sha256);
      assert.deepEqual(WebAssembly.Module.customSections(next.module, 'x').map(bytes => Buffer.from(bytes)), [Buffer.from([0x2a, 0x2b])]);
    });
  });

  it('accepts a matching explicit pin', () => {
    withFixture(current, path => {
      const expected = sha256(current);
      const result = readEngine(path, expected);
      assert.equal(result.sha256, expected);
      assert.deepEqual(Buffer.from(result.bytes), current);
      assert.ok(result.module instanceof WebAssembly.Module);
    });
  });

  it('rejects a mismatched pin before parsing malformed wasm', () => {
    withFixture(Buffer.from([0xff]), path => {
      assert.throws(() => readEngine(path), WebAssembly.CompileError);
      assert.throws(() => readEngine(path, '0'.repeat(64)), pinError);
    });
  });

  it('rejects malformed explicit pins before parsing wasm', () => {
    withFixture(Buffer.from([0xff]), path => {
      for (const pin of ['', null, 0, '0'.repeat(63), '0'.repeat(65), 'g'.repeat(64), sha256(current).toUpperCase(), [sha256(current)]]) {
        assert.throws(() => readEngine(path, pin), malformedPinError);
      }
    });
  });

  it('uses the environment pin by default and lets an explicit pin override it', () => {
    withFixture(current, path => {
      const expected = sha256(current);
      process.env.RING3_ENGINE_SHA256 = expected;
      assert.equal(readEngine(path).sha256, expected);

      process.env.RING3_ENGINE_SHA256 = '0'.repeat(64);
      assert.throws(() => readEngine(path), pinError);
      assert.equal(readEngine(path, expected).sha256, expected);

      process.env.RING3_ENGINE_SHA256 = '';
      assert.throws(() => readEngine(path), pinError);
    });
  });
});
