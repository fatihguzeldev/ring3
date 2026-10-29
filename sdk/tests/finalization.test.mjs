import assert from 'node:assert/strict';
import test from 'node:test';
import {finishSession} from '../runtime/session/finalization.js';
import {Session} from '../runtime/session.js';

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

test('healthy finalization keeps ACK authority alive until the native graphics tail is drained',async()=>{
  const events=[],controller=new AbortController();let release;
  const draining=new Promise(resolve=>{release=resolve;});
  const session=Object.assign(Object.create(Session.prototype),{running:true,abort:controller,executing:Promise.resolve(),
    code:{publish(){events.push('publish');}},scheduler:{wake(){events.push('wake');}},
    media:{pauseAudio(){events.push('hold');},graphicsTransport:{drain(){events.push('drain');return draining;}}},
    async settled(){assert.equal(controller.signal.aborted,true);events.push('settled');},
    stop(){events.push('close');}});
  const finishing=finishSession(session,{async finish(){events.push('capture');}},'cancelled');
  const repeated=finishSession(session,null,'cancelled');
  try {
    assert.equal(finishing,repeated);assert.equal(session.running,false);assert.equal(controller.signal.aborted,false);
    assert.deepEqual(events,['hold','wake','publish','drain']);release();await finishing;
    assert.deepEqual(events.slice(-3),['settled','capture','close']);assert.equal(controller.signal.aborted,true);
  } finally {release();await finishing;}
});

test('failed finalization aborts an existing healthy drain instead of waiting for an impossible ACK',async()=>{
  const controller=new AbortController(),failure=new Error('GPU output failed');let cancelled=0,closes=0;
  const session={abort:controller,cancel(){cancelled++;controller.abort(failure);},
    prepareFinalization(){return new Promise((resolve,reject)=>controller.signal.addEventListener('abort',()=>reject(controller.signal.reason),{once:true}));},
    async settled(){},stop(){closes++;}};
  const finishing=finishSession(session,null,'cancelled'),rejected=assert.rejects(finishing,error=>error===failure);
  const failed=finishSession(session,null,'failed',failure);assert.equal(failed,finishing);
  await rejected;assert(cancelled>0);assert.equal(closes,1);assert.equal(session.finalizing,null);
});
