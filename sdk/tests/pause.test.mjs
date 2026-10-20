import assert from 'node:assert/strict';
import {test} from 'node:test';
import {Session} from '../runtime/session.js';

for (const boundary of ['graphics','providers']) {
  test(`pause acknowledged during ${boundary} prevents subsequent CPU execution`,async () => {
    let release, entered;
    const blocked = new Promise(resolve=>{release=resolve;});
    const reached = new Promise(resolve=>{entered=resolve;});
    const pauseAtBoundary = async () => {entered(); await blocked; return false;};
    let entries=0;
    const statuses=[];
    const session=Object.assign(Object.create(Session.prototype),{
      running:true,paused:false,stopped:false,lastSchedule:Infinity,
      engine:{get:()=>3,api:{pending_windows_thread_schedule:()=>0}},
      code:{publish(){},import(){},entry(){entries++;throw new Error('CPU executed after pause acknowledgement');}},
      media:{update(){},graphicsOutput:boundary==='graphics'?pauseAtBoundary:async()=>{},graphicsTransport:{}},
      providers:boundary==='providers'?pauseAtBoundary:async()=>false,
      observeClock:()=>0,initializer:()=>null,exits:new Map(),blocked:new Map(),
      async wait(){this.running=false;},status:state=>statuses.push(state),
    });
    const executing=session.loop();
    await reached;
    session.pause();
    assert.deepEqual(statuses,['paused']);
    release();
    await assert.doesNotReject(executing);
    assert.equal(entries,0);
  });
}
