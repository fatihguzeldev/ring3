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

function nativeQueueFixture(specifications) {
  const controller=new AbortController(),calls=[],messages=[];
  const packet=(sequence,sealed=false)=>({sequence:BigInt(sequence),sealed,bytes:new Uint8Array(64).fill(sequence)});
  const devices=specifications.map(({epoch,retired=false,packets=[]})=>({epoch:BigInt(epoch),retired,
    packets:packets.map(([sequence,sealed])=>packet(sequence,sealed))}));
  const number=(low,high)=>BigInt(low)|(BigInt(high)<<32n);
  let copied=new Uint8Array(),failure;
  const engine={signal:controller.signal,
    media(operation,...args) {
      calls.push([operation,...args]);if(failure)throw failure;
      if(operation==='d3d8_device') {
        const device=devices[args[0]];
        return device?{epoch:device.epoch,values:[device.epoch*4096n,1n,BigInt(device.retired),0n]}:{epoch:0n,values:[]};
      }
      const epoch=number(args[0],args[1]),device=devices.find(row=>row.epoch===epoch);
      assert.ok(device,'native inventory epoch is still live');
      if(operation==='d3d8_seal_batch') {
        if(device.packets.length)device.packets[0].sealed=true;
        else if(device.retired)devices.splice(devices.indexOf(device),1);
      } else assert.equal(operation,'d3d8_batch_info');
      const current=device.packets[0];
      return {epoch,values:current?.sealed?[current.sequence,BigInt(current.bytes.length)]:[0n,0n]};
    },
    ok(operation,...args) {
      calls.push([operation,...args]);const device=devices.find(row=>row.epoch===number(args[0],args[1]));
      assert.ok(device);const current=device.packets[0];assert.ok(current?.sealed);
      assert.equal(number(args[2],args[3]),current.sequence);
      if(operation==='d3d8_read_batch')copied=current.bytes.slice(args[4],args[4]+args[5]);
      else {assert.equal(operation,'d3d8_acknowledge_batch');device.packets.shift();}
    },
    take(length) {assert.equal(copied.length,length);return copied;},
  };
  const transport=new GraphicsTransport(engine,message=>messages.push(message));
  return {transport,controller,calls,messages,devices,packet,
    acknowledge(){assert.ok(transport.pending);return transport.complete({...transport.pending});},
    fail(error){failure=error;}};
}

test('deferred sealing holds normal partial packets but always forwards native sealed fragments',async()=>{
  const f=nativeQueueFixture([{epoch:7,packets:[[1,false]]}]),t=f.transport;
  const initial=t.deferredSealing;t.deferredSealing=true;
  await t.output();assert.equal(f.messages.length,0);
  assert.equal(f.calls.some(([operation])=>operation==='d3d8_seal_batch'),false);
  assert.equal(initial,false);
  f.devices[0].packets[0].sealed=true;await t.output();
  assert.equal(f.messages.length,1);assert.equal(t.pending.epoch,7n);
  assert.deepEqual([...new Uint8Array(f.messages[0].packet)],Array(64).fill(1));
  assert.equal(f.acknowledge(),true);assert.equal(t.pending,null);
  assert.throws(()=>t.configureDeferredSealing(false,{paused:false,open:false}),/paused|owner/);
  assert.equal(t.configureDeferredSealing(false,{paused:true,open:false}),false);
  f.devices[0].packets.push(f.packet(2));await t.output();assert.equal(f.messages.length,2);
  assert.equal(f.acknowledge(),true);
  t.configureDeferredSealing(true,{paused:true,open:false});t.probe=()=>({batch(){return null;}});
  f.devices[0].packets.push(f.packet(3));await t.output();assert.equal(f.messages.length,3);
  assert.equal(f.acknowledge(),true,'active probes retain eager sealing');
});

test('sticky flush visits multiple devices fairly and retires queues without losing later demand',async()=>{
  const f=nativeQueueFixture([{epoch:7,packets:[[1,false]]},{epoch:8,packets:[[1,false]]},
    {epoch:9,retired:true,packets:[[1,false]]}]),t=f.transport;
  t.deferredSealing=true;await t.output();assert.equal(t.pending.epoch,9n);
  assert.deepEqual(f.calls.filter(([operation])=>operation==='d3d8_seal_batch').map(([,low])=>low),[9]);
  t.requestFlush();f.acknowledge();
  for(const epoch of [7n,8n]) {
    await t.output();assert.equal(t.pending.epoch,epoch);assert.equal(t.forceDemand,true);
    const count=f.messages.length;await t.output();assert.equal(f.messages.length,count);
    f.acknowledge();assert.equal(t.forceDemand,true,'an ACK alone cannot discard the remaining inventory demand');
  }
  await t.output();assert.equal(t.pending,null);assert.equal(t.forceDemand,false);
  assert.deepEqual(f.devices.map(row=>row.epoch),[7n,8n]);
  assert.deepEqual(f.messages.map(message=>message.requestId),[1,2,3]);
  assert.equal(f.calls.filter(([operation])=>operation==='d3d8_acknowledge_batch').length,3);
});

test('forced native drain advances an ACK chain while guest execution stays paused', {timeout:1000},async()=>{
  const f=nativeQueueFixture([{epoch:7,packets:[[1,false],[2,true]]},{epoch:8,packets:[[1,false]]}]),t=f.transport;
  t.deferredSealing=true;let drained=false;
  const boundary=t.drain(),draining=boundary.then(()=>{drained=true;});
  assert.equal(t.drain(),boundary,'pause and healthy stop share one native tail boundary');
  assert.throws(()=>t.configureDeferredSealing(false,{paused:true,open:false}),/drained|owner/);
  for(const [index,epoch] of [7n,8n,7n].entries()) {
    await new Promise(resolve=>setImmediate(resolve));
    assert.equal(drained,false);assert.equal(t.pending.epoch,epoch);assert.equal(f.messages.length,index+1);
    assert.equal(f.acknowledge(),true);
  }
  await draining;assert.equal(drained,true);assert.equal(t.pending,null);assert.equal(t.forceDemand,false);
  assert.equal(f.devices.every(row=>row.packets.length===0),true);
  assert.deepEqual(f.calls.filter(([operation])=>operation==='d3d8_acknowledge_batch').map(([,low,,sequence])=>[low,sequence]),[[7,1],[8,1],[7,2]]);
});

test('native drain rejects abort and output failure without publishing a false clean boundary', {timeout:1000},async testContext=>{
  const f=nativeQueueFixture([{epoch:7,packets:[[1,false]]}]),t=f.transport;
  t.deferredSealing=true;const stopped=new Error('owner stopped');
  const draining=t.drain(),rejected=assert.rejects(draining,error=>error===stopped);
  await new Promise(resolve=>setImmediate(resolve));assert.ok(t.pending);
  const pending={...t.pending};f.controller.abort(stopped);await rejected;
  assert.deepEqual(t.pending,pending);assert.equal(f.calls.some(([operation])=>operation==='d3d8_acknowledge_batch'),false);
  const broken=nativeQueueFixture([{epoch:7,packets:[[1,false]]}]),failure=new Error('native inventory failed');
  broken.transport.deferredSealing=true;broken.fail(failure);
  await assert.rejects(broken.transport.drain(),error=>error===failure);
  assert.equal(broken.messages.length,0);assert.equal(broken.devices[0].packets[0].sealed,false);
  const wrong=nativeQueueFixture([{epoch:7,packets:[[1,false]]}]),bad=wrong.transport.drain();
  const invalid=assert.rejects(bad,/identity mismatch/);
  assert.throws(()=>wrong.transport.complete({...wrong.transport.pending,epoch:8n}),/identity mismatch/);
  await invalid;assert.ok(wrong.transport.pending);
  let deadline;
  testContext.mock.method(globalThis,'setTimeout',(callback,milliseconds)=>{assert.equal(milliseconds,5000);deadline=callback;return 0;});
  const silent=nativeQueueFixture([{epoch:7,packets:[[1,false]]}]),lost=silent.transport.drain();
  const timeout=assert.rejects(lost,/drain timed out/);deadline();await timeout;
  assert.ok(silent.transport.pending);assert.equal(silent.transport.sealing.drainsCompleted,0);
});

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
    owner?.audioOutput?.close();
    if(previousSelf===undefined)delete globalThis.self;else globalThis.self=previousSelf;
    if(previousPost===undefined)delete globalThis.postMessage;else globalThis.postMessage=previousPost;
  });
  await import(`../runtime/worker.js?graphics-wake=${++workerImport}`);
  const receive=data=>globalThis.self.onmessage({data});
  receive({type:'start',audioPort:{postMessage(){},close(){}}});await ready;
  const fire=()=>{
    const [id,timer]=timers.entries().next().value;timers.delete(id);timer.callback();
  };
  return {...fixture,owner,timers,messages,receive,fire};
}

test('cancelled audio owners cannot publish queued worklet statistics',async t=>{
  const f=await workerFixture(t);
  f.owner.audioOutput.receive({type:'stats',epoch:7n,renderedFrames:128});
  assert.equal(f.messages.filter(message=>message.type==='audio-stats').length,1);
  f.owner.cancel();
  f.owner.audioOutput.receive({type:'stats',epoch:7n,renderedFrames:256});
  assert.equal(f.messages.filter(message=>message.type==='audio-stats').length,1);
});

test('Worker audio resume primes the producer before posting the Worklet resume',async t=>{
  const f=await workerFixture(t),order=[];
  f.owner.paused=true;
  f.media.resumeAudio=()=>order.push('prime');
  f.owner.audioOutput.port.postMessage=message=>{
    order.push(message.type);
    f.owner.audioOutput.receive({type:'resumed',request:message.request,epoch:7n,acceptedFrames:1024,renderedFrames:0,queuedFrames:1024});
  };
  f.receive({type:'audio-control',operation:'resume',request:9});
  await Promise.resolve();
  assert.deepEqual(order,['prime','resume']);assert.equal(f.owner.paused,true);
  const reply=f.messages.find(message=>message.type==='audio-control-reply');
  assert.equal(reply.reply.request,9);assert.equal(reply.reply.queuedFrames,1024);
  assert.equal(f.timers.size,0);
});

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
