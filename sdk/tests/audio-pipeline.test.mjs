import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {AudioHost} from '../browser/audio/host.js';
import {PcmTransport} from '../audio/transport.js';
import {Media} from '../runtime/media.js';
import {Session as BrowserSession} from '../browser/session.js';
import {Session as RuntimeSession} from '../runtime/session.js';

const packet = (sequence = 1n, firstFrame = 0n) => ({
  epoch:7n,sequence,firstFrame,frames:256,channels:2,sampleRate:48000,
  samples:new Float32Array(512).fill(0.25),
});

async function hostFixture(t) {
  const previousWindow=globalThis.window, previousNode=globalThis.AudioWorkletNode;
  const sent=[],events=[],stats=[];
  let node;
  globalThis.window={AudioContext:class {
    constructor() {this.sampleRate=48000;this.state='suspended';this.audioWorklet={async addModule(){}};}
    async resume() {events.push('resume');this.state='running';}
    async suspend() {events.push('suspend');this.state='suspended';}
    async close() {this.state='closed';}
    createGain() {return {gain:{value:0},connect(){}};}
  }};
  globalThis.AudioWorkletNode=class {
    constructor() {node=this;this.port={postMessage:message=>sent.push(message),close(){}};}
    connect(gain) {return gain;}
    disconnect() {}
  };
  const host=new AudioHost({onStats:value=>stats.push(value)});
  t.after(async()=>{
    await host.close();
    globalThis.window=previousWindow;
    globalThis.AudioWorkletNode=previousNode;
  });
  await host.start();
  return {host,sent,events,stats,receive:data=>node.port.onmessage({data})};
}

test('host queue acceptance never acknowledges consumption; cumulative output progress does',async t=>{
  const {host,receive}=await hostFixture(t);
  const progress=[];
  let settled=false;
  const completed=host.accept(packet(),value=>progress.push(value)).then(()=>{settled=true;});
  void completed.catch(()=>{});
  receive({type:'accepted',epoch:7n,sequence:1n,firstFrame:0n,frames:256});
  await Promise.resolve();
  assert.equal(settled,false);
  assert.equal(progress.length,0);
  receive({type:'consumed',epoch:7n,sequence:1n,firstFrame:0n,frames:256,consumedFrames:128});
  assert.equal(progress.length,1);
  assert.equal(progress[0].consumedFrames,128);
  assert.equal(settled,false);
  receive({type:'consumed',epoch:7n,sequence:1n,firstFrame:0n,frames:256,consumedFrames:256});
  await completed;
  assert.equal(settled,true);
  assert.deepEqual(progress.map(value=>value.consumedFrames),[128,256]);
});

test('pause freezes the processor before suspending and flushes earlier progress first',async t=>{
  const {host,receive,sent,events}=await hostFixture(t);
  const completed=host.accept(packet(),()=>events.push('consumed'));
  void completed.catch(()=>{});
  receive({type:'accepted',epoch:7n,sequence:1n,firstFrame:0n,frames:256});
  const paused=host.pause();
  const control=sent.at(-1);
  assert.equal(control.type,'pause');
  assert.equal(events.includes('suspend'),false);
  receive({type:'consumed',epoch:7n,sequence:1n,firstFrame:0n,frames:256,consumedFrames:128});
  receive({type:'paused',request:control.request});
  await paused;
  assert.deepEqual(events.slice(-2),['consumed','suspend']);
  const resumed=host.resume();
  await Promise.resolve();
  assert.equal(sent.at(-1).type,'resume');
  receive({type:'resumed',request:sent.at(-1).request});
  await resumed;
  receive({type:'consumed',epoch:7n,sequence:1n,firstFrame:0n,frames:256,consumedFrames:256});
  await completed;
});

test('close cancels outstanding PCM without reporting discarded frames consumed',async t=>{
  const {host,receive}=await hostFixture(t);
  const progress=[];
  const completed=host.accept(packet(),value=>progress.push(value));
  const rejected=assert.rejects(completed,/closed/);
  receive({type:'accepted',epoch:7n,sequence:1n,firstFrame:0n,frames:256});
  await host.close();
  await rejected;
  assert.deepEqual(progress,[]);
});

test('production handoff transfers the PCM endpoint while lifecycle controls use the owning Worker',async t=>{
  const {host,receive,events}=await hostFixture(t),sent=[];
  const worker={postMessage:message=>sent.push(message)};
  const port=host.connectWorker(worker);
  assert.ok(port);assert.equal(port.onmessage,null);
  const paused=host.pause();
  assert.deepEqual(sent.at(-1),{type:'audio-control',operation:'pause',request:1});
  assert.equal(events.includes('suspend'),false);
  host.receive({type:'paused',request:1,epoch:7n,acceptedFrames:1024,renderedFrames:128,queuedFrames:896});
  await paused;assert.equal(events.at(-1),'suspend');
  await assert.rejects(host.accept(packet()),/Worker|direct/i);
  assert.throws(()=>host.connectWorker(worker),/already|connected/i);
});

test('direct transport close uses the final Worker report and rejects stale control snapshots',async t=>{
  const {host,stats}=await hostFixture(t);
  host.connectWorker({postMessage(){}});
  host.receive({type:'stats',epoch:7n,acceptedFrames:1024,renderedFrames:512,queuedFrames:512,paused:false});
  host.receive({type:'paused',request:123,epoch:1n,acceptedFrames:0,renderedFrames:0,queuedFrames:0});
  assert.equal(host.epoch,7n);assert.equal(host.renderedFrames,512);
  host.reportClosed({epoch:7n,acceptedFrames:1536,renderedFrames:1280,queuedFrames:0,cancelledPendingFrames:256});
  assert.equal(host.acceptedFrames,1536);assert.equal(host.renderedFrames,1280);
  await host.close();
  assert.equal(stats.at(-1).renderedFrames,1280);assert.equal(stats.at(-1).cancelledPendingFrames,256);
  assert.equal(stats.at(-1).cancellationSnapshotComplete,true);
});

test('forced close labels an active direct snapshot as incomplete instead of claiming a final cancellation count',async t=>{
  const {host,stats}=await hostFixture(t);
  host.connectWorker({postMessage(){}});
  host.receive({type:'stats',epoch:7n,acceptedFrames:1024,renderedFrames:512,queuedFrames:512,paused:false});
  await host.close();assert.equal(stats.at(-1).cancellationSnapshotComplete,false);assert.equal(stats.at(-1).cancelledPendingFrames,null);
});

test('a transferred port closed before its first report has no final cancellation snapshot',async t=>{
  const {host,stats}=await hostFixture(t);host.connectWorker({postMessage(){}});
  await host.close();assert.equal(stats.at(-1).cancellationSnapshotComplete,false);assert.equal(stats.at(-1).cancelledPendingFrames,null);
});

test('a processor rejection after acceptance still rejects the consumption promise',async t=>{
  const {host,receive}=await hostFixture(t);
  const progress=[];
  const completed=host.accept(packet(),value=>progress.push(value));
  const rejected=assert.rejects(completed,/output failed/);
  receive({type:'accepted',epoch:7n,sequence:1n,firstFrame:0n,frames:256});
  receive({type:'error',epoch:7n,sequence:1n,message:'output failed'});
  await rejected;
  assert.deepEqual(progress,[]);
});

test('cold lifecycle snapshots label rendered progress and cancelled pending frames separately',async t=>{
  const {host,receive,sent,stats}=await hostFixture(t);
  const completed=host.accept(packet());
  const rejected=assert.rejects(completed,/closed/);
  receive({type:'accepted',epoch:7n,sequence:1n,firstFrame:0n,frames:256});
  receive({type:'consumed',epoch:7n,sequence:1n,firstFrame:0n,frames:256,consumedFrames:128});
  const paused=host.pause();
  receive({type:'paused',request:sent.at(-1).request,epoch:7n,acceptedFrames:256,renderedFrames:128,queuedFrames:128,underruns:0,underrunFrames:0});
  await paused;
  assert.equal(stats.at(-1).event,'paused');
  assert.equal(stats.at(-1).contextState,'suspended');
  assert.equal(stats.at(-1).renderedFrames,128);
  assert.match(stats.at(-1).semantics,/output|render/i);
  await host.close();await rejected;
  const stopped=stats.at(-1);
  assert.equal(stopped.event,'closed');
  assert.equal(stopped.contextState,'closed');
  assert.equal(stopped.queuedFrames,0);
  assert.equal(stopped.renderedFrames,128);
  assert.equal(stopped.cancelledPendingFrames,128);
  assert.match(stopped.semantics,/reported/i);
});

function mediaFixture() {
  const emitted=[],acknowledgements=[];
  let sequence=0n,firstFrame=0n,latest;
  const engine={
    media(operation,frames) {
      assert.equal(operation,'audio_enqueue');
      latest={epoch:7n,values:[++sequence,firstFrame,BigInt(frames)]};
      firstFrame+=BigInt(frames);
      return latest;
    },
    take(length) {
      assert.equal(length,64+Number(latest.values[2])*8);
      const bytes=new Uint8Array(length);
      new Float32Array(bytes.buffer,64).fill(Number(latest.values[0])/16);
      return bytes;
    },
    ok(...args) {acknowledgements.push(args);},
  };
  const media=Object.assign(Object.create(Media.prototype),{
    engine,epoch:7n,audio:[],audioPaused:false,audioSequence:0n,audioFrame:0n,
    emit:message=>emitted.push(message),
  });
  return {media,emitted,acknowledgements};
}

test('producer fills only four credits and refills only a completed consumed packet',()=>{
  const {media,emitted,acknowledgements}=mediaFixture();
  media.fillAudio();
  media.fillAudio();
  assert.equal(emitted.length,4);
  assert.deepEqual(emitted.map(value=>[value.epoch,value.sequence,value.firstFrame,value.frames]),
    [[7n,1n,0n,256],[7n,2n,256n,256],[7n,3n,512n,256],[7n,4n,768n,256]]);
  assert.equal(acknowledgements.length,0);
  const progress={...emitted[0],consumedFrames:128};
  media.consumedAudio(progress);
  assert.equal(acknowledgements.length,1);
  assert.equal(acknowledgements[0].at(-1),128);
  assert.equal(emitted.length,4);
  media.consumedAudio({...progress,consumedFrames:256});
  assert.equal(acknowledgements.length,2);
  assert.equal(emitted.length,5);
  assert.equal(emitted[4].firstFrame,1024n);
  assert.equal(media.audio.length,4);
});

test('stale epoch progress is ignored and an out-of-order head cannot consume or refill',()=>{
  const {media,emitted,acknowledgements}=mediaFixture();
  media.fillAudio();
  media.consumedAudio({...emitted[0],epoch:6n,consumedFrames:256});
  assert.throws(()=>media.consumedAudio({...emitted[1],consumedFrames:256}),/order|head|identity/i);
  assert.equal(acknowledgements.length,0);
  assert.equal(emitted.length,4);
  media.consumedAudio({...emitted[0],consumedFrames:128});
  media.consumedAudio({...emitted[0],consumedFrames:128});
  assert.equal(acknowledgements.length,1);
});

test('paused producer can settle real prior progress but cannot refill until resume',()=>{
  const {media,emitted,acknowledgements}=mediaFixture();
  media.fillAudio();
  media.pauseAudio();
  media.consumedAudio({...emitted[0],consumedFrames:256});
  media.fillAudio();
  assert.equal(acknowledgements.length,1);
  assert.equal(emitted.length,4);
  assert.equal(media.audio.length,3);
  media.resumeAudio();
  assert.equal(emitted.length,5);
  assert.equal(media.audio.length,4);
});

test('a rejected native acknowledgement cannot release credit or advance the retained packet',()=>{
  const {media,emitted}=mediaFixture();
  media.fillAudio();
  media.engine.ok=()=>{throw new Error('native audio rejected');};
  assert.throws(()=>media.consumedAudio({...emitted[0],consumedFrames:256}),/native audio rejected/);
  assert.equal(media.audio[0].consumedFrames,0);
  assert.equal(media.audio.length,4);
  assert.equal(emitted.length,4);
});

test('browser pause holds Worker production before freezing output and only then asks for paused',async()=>{
  const sent=[],events=[];
  const worker={postMessage:message=>sent.push(message)};
  const session=Object.assign(Object.create(BrowserSession.prototype),{
    state:'running',worker,generation:1,audioRequest:0,
    input:{disable(){events.push('input');}},
    audio:{async pause(){events.push('frozen');}},
  });
  const pausing=session.pause();
  void pausing.catch(()=>{});
  try {
    assert.equal(sent[0]?.type,'audio-hold');
    assert.deepEqual(events,['input']);
    await session.message({type:'audio-held',request:sent[0].request},worker);
    await pausing;
    assert.deepEqual(events,['input','frozen']);
    assert.deepEqual(sent.map(message=>message.type),['audio-hold','pause']);
  } finally {session.cancelAudioHold?.(new Error('test ended')); await pausing.catch(()=>{});}
});

test('a failed audio freeze cannot leave a running session with production silently held',async()=>{
  const worker={};
  const session=Object.assign(Object.create(BrowserSession.prototype),{
    state:'running',worker,generation:1,failed:false,
    input:{disable(){}},async holdAudio(){},
    audio:{async pause(){throw new Error('audio freeze failed');}},
    fail(error){this.failed=true;this.error=error;},
  });
  await assert.rejects(session.pause(),/audio freeze failed/);
  assert.equal(session.failed,true);
  assert.match(session.error.message,/audio freeze failed/);
});

test('PCM progress is forwarded only to the Worker generation that submitted it',async()=>{
  const sent=[];
  let progress,complete;
  const worker={},replacement={};
  const session=Object.assign(Object.create(BrowserSession.prototype),{
    worker,generation:3,
    audio:{accept(_packet,onConsumed){progress=onConsumed;return new Promise(resolve=>{complete=resolve;});}},
    post:message=>sent.push(message),
  });
  const consuming=session.message({type:'pcm',...packet()},worker);
  try {
    assert.equal(typeof progress,'function');
    progress({...packet(),type:'consumed',consumedFrames:128});
    assert.equal(sent.length,1);
    assert.equal(sent[0].type,'pcm-consumed');
    assert.equal(sent[0].consumedFrames,128);
    session.worker=replacement;session.generation++;
    progress({...packet(),type:'consumed',consumedFrames:256});
    assert.equal(sent.length,1);
  } finally {complete();await consuming;}
});

test('runtime pause, resume and cancellation keep audio refilling under the same lifecycle',async()=>{
  const events=[];
  const session=Object.assign(Object.create(RuntimeSession.prototype),{
    paused:false,pausedDuration:0,running:true,abort:new AbortController(),
    media:{pauseAudio(){events.push('hold');},resumeAudio(){events.push('refill');}},
    status:state=>events.push(state),
  });
  await session.pause();
  assert.deepEqual(events,['hold','paused']);
  await session.resume();
  assert.deepEqual(events,['hold','paused','refill','running']);
  session.cancel();
  assert.equal(events.at(-1),'hold');
  assert.equal(session.abort.signal.aborted,true);
});

test('resuming an emptied held queue refills before the Worklet can report an avoidable underrun',async t=>{
  const {host}=await hostFixture(t),{media,emitted,acknowledgements}=mediaFixture();
  const replies=[],diagnostics=[];let Processor,processor,heldRequest,pendingResume;
  const realm=vm.createContext({Float32Array,sampleRate:48000,currentFrame:0,
    AudioWorkletProcessor:class {constructor(){this.port={postMessage:message=>replies.push(message)};}},
    registerProcessor(_name,type){Processor=type;},
  });
  vm.runInContext(await readFile(new URL('../browser/audio/processor.js',import.meta.url),'utf8'),realm);
  processor=new Processor({processorOptions:{capacityFrames:1024,primeFrames:512}});
  const port={postMessage:message=>processor.receive(message),close(){}};
  const output=new PcmTransport({port,onDiagnostic:message=>diagnostics.push(message),beforeResume:()=>{
    assert.equal(host.context.state,'running','AudioContext activation begins before asynchronous Worker priming');
    media.resumeAudio();
  }});
  t.after(()=>output.close());
  const deliverReplies=()=>{while(replies.length)port.onmessage({data:replies.shift()});};
  const originalEmit=media.emit;
  media.emit=message=>{originalEmit(message);void output.accept(message,progress=>media.consumedAudio(progress)).catch(()=>{});};
  const render=()=>{
    const channels=[new Float32Array(128),new Float32Array(128)];
    assert.equal(processor.process([],[channels]),true);realm.currentFrame+=128;
    deliverReplies();return channels;
  };
  let browser;
  const runtime=Object.assign(Object.create(RuntimeSession.prototype),{
    paused:false,pausedDuration:0,running:true,abort:new AbortController(),media,status:state=>{browser.state=state;},
  });
  const worker={postMessage(message){
    if(message.type==='audio-hold'){media.pauseAudio();heldRequest=message.request;}
    else if(message.type==='audio-control'){
      const reply=output.control(message.operation);deliverReplies();
      void reply.then(value=>host.receive({...value,request:message.request}));
    } else if(message.type==='pause')runtime.pause();
    else if(message.type==='resume')pendingResume=message;
    else assert.fail(`unexpected Worker message ${message.type}`);
  }};
  host.connectWorker(worker);
  browser=Object.assign(Object.create(BrowserSession.prototype),{
    state:'running',worker,generation:1,audioRequest:0,input:{disable(){}},audio:host,
  });
  media.fillAudio();deliverReplies();
  assert.equal(processor.queuedFrames,1024);assert.equal(output.pending.size,4);
  const pausing=browser.pause();
  assert.equal(media.audioPaused,true);
  // already queued audio may finish while the main thread awaits the hold reply.
  for(let index=0;index<8;index++)assert.notEqual(render()[0][0],0);
  assert.equal(processor.started,true);assert.equal(processor.priming,true);
  assert.equal(processor.queuedFrames,0);assert.equal(processor.underruns,0);
  assert.equal(media.audio.length,0);assert.equal(output.pending.size,0);
  await browser.message({type:'audio-held',request:heldRequest},worker);
  await pausing;
  await runtime.pausing;
  assert.equal(browser.state,'paused');assert.equal(host.context.state,'suspended');
  assert.equal(processor.paused,true);assert.equal(processor.renderedFrames,1024);
  assert.deepEqual([...render()[0]],new Array(128).fill(0));
  assert.equal(processor.underruns,0,'paused rendering is not an underrun');
  await browser.resume();
  assert.equal(pendingResume?.type,'resume');assert.equal(processor.paused,false);
  assert.equal(runtime.paused,true,'PCM priming does not resume guest execution before the browser handshake');
  assert.equal(media.audioPaused,false);assert.equal(processor.queuedFrames,1024);
  assert.equal(processor.started,true,'resume retains the existing playback epoch and underrun history');
  // the audio thread can render before the posted worker resume task is delivered.
  const before=processor.snapshot(),channels=render(),after=processor.snapshot();
  runtime.resume();deliverReplies();
  assert.equal(media.audioPaused,false);assert.equal(processor.queuedFrames,896);
  assert.equal(emitted.length,8);assert.equal(output.pending.size,4);
  assert.deepEqual([...render()[0]],new Array(128).fill(5/16));
  assert.equal(processor.renderedFrames,1280);assert.equal(acknowledgements.length,10);
  assert.deepEqual(diagnostics,[]);assert.equal(processor.epoch,7n);
  assert.deepEqual([...channels[0]],new Array(128).fill(5/16));
  assert.equal(after.renderedFrames-before.renderedFrames,128,'resume renders actual queued PCM before guest execution resumes');
  assert.equal(after.underrunFrames-before.underrunFrames,0,'resume must not expose output before its held producer can refill');
});

test('a failed producer prime cannot send the Worklet resume control',async()=>{
  const sent=[],failure=new Error('native refill failed');
  const output=new PcmTransport({port:{postMessage:message=>sent.push(message),close(){}},beforeResume(){throw failure;}});
  try {
    await assert.rejects(output.control('resume'),error=>error===failure);
    assert.deepEqual(sent,[]);assert.equal(output.controls.size,0);
    assert.equal(output.acceptedFrames,0);assert.equal(output.renderedFrames,0);
  } finally {output.close();}
});
