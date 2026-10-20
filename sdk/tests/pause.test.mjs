import assert from 'node:assert/strict';
import {test} from 'node:test';
import {Session} from '../runtime/session.js';
import {PhaseMeasurements} from '../runtime/session/measurement.js';

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
      scheduler:{async wait(){session.running=false;}},
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

function measuredSession(t) {
  let now=0;
  t.mock.method(performance,'now',()=>now);
  const statuses=[],native=[],audio=[];
  const session=new Session(value=>statuses.push(value));
  Object.assign(session,{
    started:0,pausedDuration:0,running:true,
    engine:{get:()=>0,ok:(...args)=>native.push(args)},
    code:{publish(){},sampleUsage(){},stats:()=>({})},
    media:{pauseAudio:()=>audio.push('hold'),resumeAudio:()=>audio.push('refill')},
    measurements:new PhaseMeasurements({observer:{event(){}},capture:{observer:()=>({enabled:false})}}),
  });
  return {session,statuses,native,audio,set:time=>{now=time;},advance:time=>{now+=time;}};
}

test('manual pause does not pollute host scheduler waits or resumed guest elapsed time',async t=>{
  const f=measuredSession(t),{session}=f;
  session.scheduler={async wait(milliseconds){
    if(milliseconds===20){f.advance(200);session.running=false;}
    else f.advance(milliseconds===0?5:milliseconds);
  }};
  await session.wait(10);
  session.pause();
  await session.loop();
  session.resume();
  await session.wait(0);
  session.status('running','accounting');
  assert.equal(session.measurements.snapshot().wait.elapsedNs,15_000_000);
  assert.equal(session.measurements.snapshot().wait.count,2);
  assert.equal(f.statuses.at(-1).manualPauseMs,200);
  assert.match(f.statuses.at(-1).waitSemantics,/host scheduler.*event-loop/i);
  assert.match(f.statuses.at(-1).waitSemantics,/not guest wait/i);
  assert.equal(session.observeClock(),15_000_000n);
  assert.deepEqual(f.native.at(-1),['observe_cpu_clock',15_000_000,0]);
  assert.deepEqual(f.audio,['hold','refill']);
});

test('open pause snapshots do not consume or double count the interval on resume',t=>{
  const f=measuredSession(t),{session}=f;
  f.set(40);session.pause();
  f.set(90);session.status('paused','first snapshot');
  assert.equal(f.statuses.at(-1).manualPauseMs,50);
  f.set(100);session.status('paused','second snapshot');
  assert.equal(f.statuses.at(-1).manualPauseMs,60);
  assert.equal(session.pausedDuration,0);
  f.set(240);session.resume();
  assert.equal(f.statuses.at(-1).manualPauseMs,200);
  f.set(300);session.resume();session.status('running','later snapshot');
  assert.equal(f.statuses.at(-1).manualPauseMs,200);
  assert.equal(session.observeClock(),100_000_000n);
  assert.deepEqual(f.audio,['hold','refill']);
});

test('stop closes an open manual pause once without resuming audio',t=>{
  const f=measuredSession(t),{session}=f;
  session.pause();f.set(200);session.stop();
  assert.equal(f.statuses.at(-1).manualPauseMs,200);
  assert.equal(session.pausedDuration,200);
  f.set(350);session.stop();session.status('stopped','later snapshot');
  assert.equal(f.statuses.at(-1).manualPauseMs,200);
  assert.equal(f.native.filter(([name])=>name==='close').length,1);
  assert.deepEqual(f.audio,['hold','hold']);
});
