import assert from 'node:assert/strict';
import {test} from 'node:test';
import {Session} from '../runtime/session.js';

test('failure publishes the last 48 execution records in order with exact identities', async () => {
  const emitted = [], expected = [];
  let count = 0;
  const session = Object.assign(Object.create(Session.prototype), {
    running:true, paused:false, stopped:false, lastSchedule:Infinity, lastStatus:Infinity,
    recent:[], thread:1, blocked:new Map(), exits:new Map(),
    engine:{get:()=>3, api:{pending_windows_thread_schedule:()=>0}},
    code:{
      publish(){}, import(){}, sampleUsage(){}, stats:()=>({}), select:()=>null,
      execute(){
        const index = count++, id = index % 3 ? 9007199254740993n + BigInt(index) : null;
        const run = {kind:id === null ? 'cold' : 'compiled', id, pc:0x1000 + index,
          coreReason:2, reason:1, detail:0, retired:1, work:1, regions:id === null ? 0 : 1};
        expected.push({...run, id:id === null ? null : String(id)});
        if (count === 61) session.running = false;
        return Object.freeze(run);
      },
    },
    media:{update(){}, async graphicsOutput(){}, graphicsTransport:{}},
    async providers(){return false;}, observeClock:()=>0n, initializer:()=>null, stack:()=>[],
    async wait(){}, scheduler:{close(){}}, emit:value=>emitted.push(value),
  });
  await session.loop();
  assert.equal(count, 61);
  assert.equal(session.recent.length, 48);
  session.fail(new Error('authored failure'));
  const diagnostic = emitted.find(event => event.type === 'diagnostic');
  assert.equal(diagnostic.message, 'authored failure');
  assert.deepEqual(diagnostic.detail.recent, expected.slice(-48));
  assert.deepEqual(JSON.parse(JSON.stringify(diagnostic)).detail.recent, expected.slice(-48));
  assert.equal(emitted.at(-1).state, 'error');
});
