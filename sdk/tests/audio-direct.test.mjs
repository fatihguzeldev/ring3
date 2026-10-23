import assert from 'node:assert/strict';
import test from 'node:test';
import {MessageChannel,Worker} from 'node:worker_threads';
import {PcmTransport} from '../audio/transport.js';

const event=(worker,type)=>new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>{cleanup();reject(new Error(`audio fixture timed out: ${type}`));},5000);
  const receive=message=>{if(message.type==='failure'){cleanup();reject(new Error(message.message));}else if(message.type===type){cleanup();resolve(message);}};
  const failed=error=>{cleanup();reject(error);};
  const cleanup=()=>{clearTimeout(timer);worker.off('message',receive);worker.off('error',failed);};
  worker.on('message',receive);worker.on('error',failed);
});
const command=(worker,type)=>{const reply=event(worker,type==='pause'?'paused':type==='resume'?'resumed':type==='start'?'started':'closed');worker.postMessage({type});return reply;};

test('real audio ports keep refilling while the main relay is blocked and preserve the pause barrier',async t=>{
  const {port1,port2}=new MessageChannel(),counters=new SharedArrayBuffer(6*4),values=new Int32Array(counters);
  const runtime=new Worker(new URL('./audio-direct-worker.mjs',import.meta.url),{workerData:{role:'runtime',port:port1,counters},transferList:[port1]});
  const processor=new Worker(new URL('./audio-direct-worker.mjs',import.meta.url),{workerData:{role:'processor',port:port2,counters},transferList:[port2]});
  t.after(async()=>{await Promise.all([runtime.terminate(),processor.terminate()]);});
  await Promise.all([event(runtime,'ready'),event(processor,'ready')]);
  processor.postMessage({type:'start'});await command(runtime,'start');
  // no PCM or ACK is relayed by this thread, and it cannot dispatch any messages here.
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,150);
  assert.ok(Atomics.load(values,0)>4,'consumption must continue beyond the initial four credits');
  assert.ok(Atomics.load(values,5)>4,'Worker must enqueue replacement PCM without main-thread dispatch');
  assert.equal(Atomics.load(values,2),4);assert.equal(Atomics.load(values,4),0);
  const paused=await command(runtime,'pause');
  assert.equal(paused.reportedFrames,paused.snapshot.renderedFrames);
  assert.equal(paused.snapshot.acceptedFrames,paused.snapshot.renderedFrames+paused.snapshot.queuedFrames);
  const frozen=Atomics.load(values,3);
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,30);
  assert.equal(Atomics.load(values,3),frozen);
  await command(runtime,'resume');
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,60);
  assert.ok(Atomics.load(values,3)>frozen);
  const finalPause=await command(runtime,'pause'),closed=await command(runtime,'close');
  assert.equal(closed.snapshot.renderedFrames,finalPause.snapshot.renderedFrames);
  assert.equal(closed.snapshot.cancelledPendingFrames,finalPause.snapshot.queuedFrames);
  assert.equal(closed.snapshot.queuedFrames,0);assert.equal(Atomics.load(values,4),0);
  await command(processor,'close');
});

test('a consumed callback failure rejects the completed packet even after releasing its refill credit',async()=>{
  const {port1,port2}=new MessageChannel(),transport=new PcmTransport({port:port1});
  try {
    const completed=transport.accept({epoch:1n,sequence:1n,firstFrame:0n,frames:256,channels:2,sampleRate:48000,samples:new Float32Array(512)},()=>{throw new Error('native acknowledgement failed');});
    const rejected=assert.rejects(completed,/native acknowledgement failed/);
    transport.receive({type:'accepted',epoch:1n,sequence:1n,firstFrame:0n,frames:256});
    transport.receive({type:'consumed',epoch:1n,sequence:1n,firstFrame:0n,frames:256,consumedFrames:256});
    await rejected;assert.equal(transport.pending.size,0);
  } finally {transport.close();port2.close();}
});
