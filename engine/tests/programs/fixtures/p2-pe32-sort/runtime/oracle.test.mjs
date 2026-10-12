import assert from 'node:assert/strict';
import test from 'node:test';

import { batch, checksum, generate, sorted } from './oracle.mjs';

const seed = 0x6d2b79f5;
const expectedChecksum = 2003890592;

test('the default batch matches independent input, unsigned sort and checksum anchors', () => {
  const expectedInputPrefix = [
    1085196063, 2447379481, 2618286376, 1701901981,
    265159372, 1030440423, 4012273292, 2080899351,
  ];
  const expectedSortedPrefix = [
    25829934, 72700159, 74127373, 112741569,
    127205589, 163423342, 164538570, 200387207,
  ];
  const expectedSortedSuffix = [
    4136117020, 4163133871, 4163998849, 4184948546,
    4198727462, 4231518697, 4235914991, 4292761194,
  ];
  const actual = batch();

  assert.equal(actual.input.length, 256);
  assert.equal(actual.sorted.length, 256);
  assert.deepEqual(actual.input.slice(0, 8), expectedInputPrefix);
  assert.deepEqual(actual.sorted.slice(0, 8), expectedSortedPrefix);
  assert.deepEqual(actual.sorted.slice(-8), expectedSortedSuffix);
  assert.equal(actual.checksum, expectedChecksum);
  assert.equal(checksum(actual.sorted), expectedChecksum);
  assert.deepEqual(generate(seed), actual.input);
  assert.deepEqual(sorted(seed), actual.sorted);
  assert.notStrictEqual(actual.input, actual.sorted);

  for (const value of actual.input) {
    assert.ok(Number.isInteger(value) && value >= 0 && value <= 0xffffffff);
  }
  for (let index = 1; index < actual.sorted.length; index += 1) {
    assert.ok(actual.sorted[index - 1] <= actual.sorted[index]);
  }
  assert.deepEqual(
    [...actual.input].sort((left, right) => left - right),
    actual.sorted,
  );
});

test('the zero seed retains every duplicate and has a zero checksum', () => {
  const zeros = Array(256).fill(0);
  const actual = batch(0);

  assert.deepEqual(generate(0), zeros);
  assert.deepEqual(sorted(0), zeros);
  assert.deepEqual(actual.input, zeros);
  assert.deepEqual(actual.sorted, zeros);
  assert.equal(actual.checksum, 0);
  assert.equal(checksum(zeros), 0);
  assert.deepEqual(generate(0, 3), [0, 0, 0]);
  assert.deepEqual(sorted(0, 3), [0, 0, 0]);
});

test('unsigned boundaries and corrupt or signed-sorted data cannot match the expected checksum', () => {
  const boundaries = [0, 1, 0x7fffffff, 0x80000000, 0xffffffff];
  assert.equal(checksum(boundaries), 0x8210);
  assert.equal(checksum([0x80000000]), 0x80000000);
  assert.equal(checksum([0xffffffff]), 0xffffffff);

  const actual = batch(seed, 256);
  const signedOrder = [...actual.input].sort((left, right) => (left | 0) - (right | 0));
  assert.notDeepEqual(signedOrder, actual.sorted);
  assert.notEqual(checksum(signedOrder), expectedChecksum);

  const corrupt = [...actual.sorted];
  corrupt[corrupt.length - 1] = (corrupt.at(-1) ^ 1) >>> 0;
  assert.notEqual(checksum(corrupt), expectedChecksum);
  assert.notEqual(actual.checksum, (expectedChecksum ^ 1) >>> 0);
});
