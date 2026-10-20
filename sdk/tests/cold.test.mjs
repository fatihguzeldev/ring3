import assert from 'node:assert/strict';
import {test} from 'node:test';
import {Session} from '../runtime/session.js';

for(const measured of [false,true]) {
  test(`session selects the cold tier without eager entry${measured?' with phase measurements':''}`,async()=>{
    const calls=[],phases=[];
    const session=Object.assign(Object.create(Session.prototype),{
      running:true,paused:false,stopped:false,lastSchedule:Infinity,lastStatus:Infinity,
      recent:[],thread:1,blocked:new Map(),exits:new Map(),
      engine:{get:()=>3,api:{pending_windows_thread_schedule:()=>0}},
      code:{
        publish(){calls.push('publish');},import(){calls.push('import');},
        entry(){throw new Error('eager compilation bypassed cold selection');},
        select(){calls.push('select');return null;},
        execute(selected){
          assert.equal(selected,null);calls.push('execute');session.running=false;
          return {kind:'cold',id:null,pc:0x1004,coreReason:2,reason:1,detail:0,retired:1,work:1,regions:0};
        },
      },
      media:{update(){},async graphicsOutput(){},graphicsTransport:{}},
      async providers(){return false;},observeClock:()=>0n,initializer:()=>null,stack:()=>[],
      async wait(){},
      measurements:measured ? {
        run(name,operation){phases.push(name);return operation();},
        async runAsync(name,operation){phases.push(name);return operation();},
      } : null,
    });
    await session.loop();
    assert.deepEqual(calls,['publish','import','select','execute','publish']);
    assert.equal(session.recent.length,1);assert.equal(session.recent[0].kind,'cold');
    assert.equal(session.recent[0].id,null);assert.equal(session.recent[0].regions,0);
    assert.deepEqual(phases,measured ? ['providers','entry','execution'] : []);
  });
}
