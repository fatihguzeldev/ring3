import assert from 'node:assert/strict';
import test from 'node:test';
import {Session} from '../runtime/session.js';
import {Media} from '../runtime/media.js';
import {GraphicsTransport} from '../runtime/media/graphics.js';

const ack = overrides => ({requestId:11,epoch:7n,sequence:3n,...overrides});
let workerImport = 0;

function transportFixture() {
  const calls=[],controller=new AbortController();
  const engine={signal:controller.signal,
    ok(...args) {calls.push(args);},request() {},
  };
  const transport=new GraphicsTransport(engine,()=>{});
  transport.pending={requestId:11,epoch:7n,sequence:3n};
  transport.batches.set(7n,{sent:1,acknowledged:0,bytes:64});
  const media=Object.assign(Object.create(Media.prototype),{graphicsTransport:transport});
  return {engine,transport,media,calls,controller};
}

async function workerFixture(t) {
  const timers=new Map(),messages=[],previousSelf=globalThis.self,previousPost=globalThis.postMessage;
  let nextTimer=1,owner,started;
  const ready=new Promise(resolve=>{started=resolve;});
  t.mock.method(globalThis,'setTimeout',(callback,milliseconds)=>{
    const id=nextTimer++;timers.set(id,{callback,milliseconds});return id;
  });
  t.mock.method(globalThis,'clearTimeout',id=>{timers.delete(id);});
  const fixture=transportFixture();
  t.mock.method(Session.prototype,'start',async function() {
    owner=this;this.running=true;this.media=fixture.media;fixture.engine.signal=this.abort.signal;started();
  });
  globalThis.self={};globalThis.postMessage=message=>messages.push(message);
  t.after(()=>{
    owner?.cancel();
    if(previousSelf===undefined)delete globalThis.self;else globalThis.self=previousSelf;
    if(previousPost===undefined)delete globalThis.postMessage;else globalThis.postMessage=previousPost;
  });
  await import(`../runtime/worker.js?graphics-wake=${++workerImport}`);
  const receive=data=>globalThis.self.onmessage({data});
  receive({type:'start'});await ready;
  const fire=()=>{
    const [id,timer]=timers.entries().next().value;timers.delete(id);timer.callback();
  };
  return {...fixture,owner,timers,messages,receive,fire};
}

test('a validated graphics ACK resumes a pending host timer after native publication',async t=>{
  const f=await workerFixture(t),order=[];
  f.engine.ok=(...args)=>{f.calls.push(args);order.push('native ACK');};
  let resumed=false;
  const waiting=f.owner.scheduler.wait(4).then(()=>{resumed=true;order.push('host continuation');});
  void waiting.catch(()=>{});
  f.receive({type:'graphics-complete',...ack()});
  await Promise.resolve();
  assert.equal(resumed,true,'a fully applied ACK must release the host polling delay');
  await waiting;
  assert.deepEqual(order,['native ACK','host continuation']);
  assert.deepEqual(f.calls,[['d3d8_acknowledge_batch',7,0,3,0]],'waking does not complete a native guest wait');
  assert.equal(f.transport.pending,null);assert.equal(f.timers.size,0);
  assert.equal(f.owner.scheduler.pending,null);
});

test('accepted input releases a running host timer only after enqueue without completing native waits',async t=>{
  const f=await workerFixture(t),order=[],events=[{kind:'key',code:17,down:true}];
  f.media.enqueue=(received,sentAtMs)=>{
    assert.deepEqual(received,events);assert.equal(sentAtMs,123);order.push('enqueue');
  };
  let resumed=false;
  const waiting=f.owner.scheduler.wait(4).then(()=>{resumed=true;order.push('continuation');});
  void waiting.catch(()=>{});
  f.receive({type:'input',events,sentAtMs:123});await Promise.resolve();
  assert.equal(resumed,true);
  await waiting;
  assert.deepEqual(order,['enqueue','continuation']);
  assert.equal(f.calls.length,0,'host wake leaves native guest wait resolution to the runtime loop');
  assert.equal(f.timers.size,0);
});

test('empty input does not release a host timer',async t=>{
  const f=await workerFixture(t);f.media.enqueue=()=>{};
  let resumed=false;
  const waiting=f.owner.scheduler.wait(4).then(()=>{resumed=true;});void waiting.catch(()=>{});
  f.receive({type:'input',events:[]});await Promise.resolve();
  assert.equal(resumed,false);assert.equal(f.timers.size,1);
  f.fire();await waiting;
});

test('paused input queues for resume without releasing the paused-loop timer',async t=>{
  const f=await workerFixture(t);f.owner.paused=true;let queued=false,resumed=false;
  f.media.enqueue=()=>{queued=true;};
  const waiting=f.owner.scheduler.wait(20).then(()=>{resumed=true;});void waiting.catch(()=>{});
  f.receive({type:'input',events:[{kind:'focus',focused:false}]});await Promise.resolve();
  assert.equal(queued,true);assert.equal(resumed,false);assert.equal(f.timers.size,1);
  f.fire();await waiting;
});

test('stale and duplicate request ACKs do not release unrelated host timers',async t=>{
  const f=await workerFixture(t);
  let resumed=false;
  const waiting=f.owner.scheduler.wait(1).then(()=>{resumed=true;});void waiting.catch(()=>{});
  f.receive({type:'graphics-complete',...ack({requestId:10})});
  await Promise.resolve();
  assert.equal(resumed,false);assert.equal(f.calls.length,0);assert.equal(f.transport.pending.requestId,11);
  f.fire();await waiting;
  f.receive({type:'graphics-complete',...ack()});
  resumed=false;
  const next=f.owner.scheduler.wait(4).then(()=>{resumed=true;});void next.catch(()=>{});
  f.receive({type:'graphics-complete',...ack()});
  await Promise.resolve();
  assert.equal(resumed,false);assert.equal(f.calls.length,1);
  f.fire();await next;
});

test('an accepted ACK leaves a MessageChannel yield queued',async t=>{
  const f=await workerFixture(t);let resumed=false;
  const waiting=f.owner.scheduler.wait(0).then(()=>{resumed=true;});void waiting.catch(()=>{});
  f.receive({type:'graphics-complete',...ack()});await Promise.resolve();
  assert.equal(resumed,false);assert.equal(f.owner.scheduler.pending.kind,'task');
  assert.equal(f.timers.size,0);assert.equal(f.calls.length,1);
  await waiting;
});

test('a paused owner applies its ACK without waking the paused-loop timer',async t=>{
  const f=await workerFixture(t);f.owner.paused=true;let resumed=false;
  const waiting=f.owner.scheduler.wait(20).then(()=>{resumed=true;});void waiting.catch(()=>{});
  f.receive({type:'graphics-complete',...ack()});await Promise.resolve();
  assert.equal(resumed,false);assert.equal(f.transport.pending,null);assert.equal(f.calls.length,1);
  assert.equal(f.timers.values().next().value.milliseconds,20);
  f.fire();await waiting;
});

test('a non-running owner applies its ACK without waking a host timer',async t=>{
  const f=await workerFixture(t);f.owner.running=false;let resumed=false;
  const waiting=f.owner.scheduler.wait(4).then(()=>{resumed=true;});void waiting.catch(()=>{});
  f.receive({type:'graphics-complete',...ack()});await Promise.resolve();
  assert.equal(resumed,false);assert.equal(f.transport.pending,null);assert.equal(f.calls.length,1);
  f.fire();await waiting;
});

test('abort rejects the pending timer and a later ACK cannot revive the scheduler',async t=>{
  const f=await workerFixture(t),waiting=f.owner.scheduler.wait(4);
  const rejected=assert.rejects(waiting,{name:'AbortError'});
  f.owner.cancel();await rejected;
  f.receive({type:'graphics-complete',...ack()});await Promise.resolve();
  assert.equal(f.calls.length,0);assert.equal(f.timers.size,0);
  assert.equal(f.owner.scheduler.pending,null);assert.equal(f.owner.scheduler.closed,true);
});

test('only fully applied transport ACKs report acceptance through Media',()=>{
  const f=transportFixture();
  assert.equal(f.media.graphicsComplete(ack({requestId:10})),false);
  assert.equal(f.media.graphicsComplete(ack()),true);
  assert.equal(f.transport.batches.get(7n).acknowledged,1);assert.equal(f.transport.pending,null);
  assert.equal(f.media.graphicsComplete(ack()),false);
  assert.equal(f.calls.length,1);
});

test('identity, readback and native ACK failures never report accepted transport completion',()=>{
  for(const failure of ['epoch','sequence','readback','native']) {
    const f=transportFixture(),pending=f.transport.pending;
    let message=ack();
    if(failure==='epoch')message=ack({epoch:8n});
    if(failure==='sequence')message=ack({sequence:4n});
    if(failure==='readback') {
      message=ack({readbacks:[{receipt:new Uint8Array(24),bytes:new Uint8Array(1)}]});
      f.engine.request=()=>{throw new Error('readback rejected');};
    }
    if(failure==='native')f.engine.ok=()=>{throw new Error('native ACK rejected');};
    assert.throws(()=>f.media.graphicsComplete(message),/identity|rejected/);
    assert.equal(f.transport.pending,pending);assert.equal(f.transport.batches.get(7n).acknowledged,0);
  }
});
