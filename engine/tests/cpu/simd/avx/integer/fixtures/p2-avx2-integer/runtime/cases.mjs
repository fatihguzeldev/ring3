import assert from 'node:assert/strict';
import { mechanicsSourcePaths } from '../../../support/fixtures/runtime/cases.mjs';
import {
  saturatingRows,
  saturatingSourcePaths,
} from '../../../saturating/fixtures/p2-avx2-saturating/runtime/cases.mjs';
import {
  minmaxRows,
  minmaxSourcePaths,
} from '../../../minmax/fixtures/p2-avx2-minmax/runtime/cases.mjs';
import {
  averageRows,
  averageSourcePaths,
} from '../../../average/fixtures/p2-avx2-average/runtime/cases.mjs';
import {
  absoluteRows,
  absoluteSourcePaths,
} from '../../../absolute/fixtures/p2-avx2-absolute/runtime/cases.mjs';
import { signRows, signSourcePaths } from '../../../sign/fixtures/p2-avx2-sign/runtime/cases.mjs';

export const integerRows = [
  ...saturatingRows,
  ...minmaxRows,
  ...averageRows,
  ...absoluteRows,
  ...signRows,
];

export const integerSourcePaths = [
  ...mechanicsSourcePaths,
  ...saturatingSourcePaths,
  ...minmaxSourcePaths,
  ...averageSourcePaths,
  ...absoluteSourcePaths,
  ...signSourcePaths,
  'engine/tests/cpu/simd/avx/integer/fixtures/p2-avx2-integer/runtime/cases.mjs',
];

assert.equal(integerRows.length, 556);
assert.equal(integerRows.filter((row) => row.reason).length, 84);
assert.equal(new Set(integerRows.map((row) => row.name)).size, integerRows.length);
assert.equal(new Set(integerSourcePaths).size, integerSourcePaths.length);
