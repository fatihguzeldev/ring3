import assert from 'node:assert/strict';
import test from 'node:test';
import {finishSession} from '../runtime/session/finalization.js';

test('failed native close is a rejected finalization and a later close can retry', async () => {
  let fail = true, closes = 0;
  const session = {cancel() {}, async settled() {}, stop() {closes++; if (fail) throw new Error('close returned 12');}};
  await assert.rejects(finishSession(session,null,'cancelled'),/close returned 12/);
  assert.equal(session.finalizing,null); fail = false;
  await finishSession(session,null,'cancelled'); assert.equal(closes,2);
});
test('concurrent finalization shares one settled close', async () => {
  let release, closes = 0;
  const session = {cancel() {}, settled() {return new Promise(resolve => {release = resolve;});}, stop() {closes++;}};
  const a = finishSession(session,null,'cancelled'), b = finishSession(session,null,'cancelled');
  assert.equal(a,b); release(); await a; assert.equal(closes,1);
});
