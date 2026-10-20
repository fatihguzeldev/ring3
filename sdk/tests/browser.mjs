import {createSession,selectedFolder} from '../index.js';
import {AudioHost} from '../browser/audio/host.js';
const element = id => document.getElementById(id), query = new URL(location.href).searchParams;
const events = [], report = {backend:'core',events};
const execution = Object.fromEntries(['capacity','budget','instructions','blocks']
  .filter(name => query.has(name)).map(name => [name,Number(query.get(name))]));
if (query.has('chain')) execution.chain = query.get('chain') === '1';
let local = null;
let baseWorker=null,baseCompiled=null,baseRequest=0;
let graphicsBatchRunning=false;
let audioOnly=null,audioRequest=0;
function record(type,value) {
  events.push({at_ms:performance.timeOrigin+performance.now(),type,value});
  if (events.length > 100) events.shift();
  element('ring3-report').textContent = JSON.stringify(report,(_key,value) => typeof value === 'bigint' ? String(value) : value);
}
const session = createSession({container:element('display'),engineUrl:query.get('engine') ?? '/engine.wasm',
  onStatus(value) {
    report.status = value; record('status',value);
    element('status').textContent = `${value.state}: ${value.message ?? ''}\n${JSON.stringify({pc:value.pc,stats:value.stats,presentation:value.presentation})}`;
    baseControls();
  },
  onDiagnostic(message,detail) {record('diagnostic',{message,detail});},
  onAudioStats(value) {report.audio = value; record('audio',value);},
});
element('folder').addEventListener('change',() => {
  try {
    local = selectedFolder(element('folder').files);
    element('executable').replaceChildren(...(local?.executables ?? []).map(({path}) => {
      const option = document.createElement('option'); option.value = option.textContent = path; return option;
    }));
    record('local-folder',{files:local?.manifest.files.length ?? 0,executables:local?.executables.length ?? 0});
  } catch (error) {record('error',error.message); element('status').textContent = error.message;}
});
const actions = {
  launch: () => {
    if(baseWorker)throw new Error('release the base-engine control before launching a guest');
    if(graphicsBatchRunning)throw new Error('wait for graphics validation before launching a guest');
    if(audioOnly)throw new Error('wait for audio validation before launching a guest');
    return session.launch({...(local ?? {}),
    ...(local ? {executable:element('executable').value} : {}),
    commandLine:query.get('commandLine') ?? undefined,
    execution:Object.keys(execution).length ? execution : undefined,
    frameProbe:query.get('frameProbe') === '1',observability:{level:query.get('observation') ?? 'off'},
    });
  },
  pause: () => session.pause(), resume: () => session.resume(), stop: () => session.stop(),
  capture: () => session.captureMouse(), probe: () => session.armFrameProbe(), dispose: () => session.dispose(),
};
for (const [name,action] of Object.entries(actions)) element(name).addEventListener('click',async () => {
  try {const result = await action(); record('action',{name,result});}
  catch (error) {record('action-error',{name,message:error.message}); element('status').textContent = error.message;}
});

function baseControls() {
  element('base-create').disabled=baseWorker!==null || graphicsBatchRunning || audioOnly!==null;
  element('base-reuse').disabled=baseWorker!==null || baseCompiled===null || graphicsBatchRunning || audioOnly!==null;
  element('base-release').disabled=baseWorker===null;
  const guestActive=!['idle','stopped'].includes(report.status?.state ?? 'idle');
  element('graphics-batch').disabled=graphicsBatchRunning || baseWorker!==null || audioOnly!==null || guestActive;
  element('audio-only').disabled=graphicsBatchRunning || baseWorker!==null || audioOnly!==null || guestActive;
  element('launch').disabled=audioOnly!==null;
}
function baseCreate(reuse) {
  if(baseWorker)throw new Error('release the current base engine first');
  if(graphicsBatchRunning)throw new Error('wait for graphics validation before a base-engine control');
  if(audioOnly)throw new Error('wait for audio validation before a base-engine control');
  if(!['idle','stopped'].includes(session.status().state))throw new Error('stop the guest session before a base-engine control');
  if(reuse&&!baseCompiled)throw new Error('no compiled base engine is retained');
  const worker=new Worker(new URL('./base-engine-worker.mjs',import.meta.url),{type:'module'});
  baseWorker=worker;
  const request=++baseRequest;
  report.baseEngine={request,arm:reuse?'reused-module':'fresh-request',state:'creating',
    requestedAtEpochMs:performance.timeOrigin+performance.now(),noGuest:true,complete:false};
  worker.onmessage=({data})=>{
    if(baseWorker!==worker || data.request!==request)return;
    if(data.type==='base-ready') {
      baseCompiled=data.compiledEngine;
      const {compiledEngine,...metadata}=data;
      Object.assign(report.baseEngine,metadata,{state:'ready'});
      element('status').textContent=`base engine ready: ${data.encodedBytes} bytes · ${data.createWallMs.toFixed(1)} ms · no guest`;
      record('base-engine-ready',metadata);
    } else if(data.type==='base-released') {
      Object.assign(report.baseEngine,{state:'released',release:data,complete:true});
      record('base-engine-released',data);
      worker.terminate();baseWorker=null;
      element('status').textContent='base engine released; compiled module retained for explicit reuse';
    } else if(data.type==='base-error') {
      Object.assign(report.baseEngine,{state:'error',error:data,complete:false});
      record('base-engine-error',data);
      element('status').textContent=`base engine error: ${data.message}`;
      worker.terminate();baseWorker=null;
    }
    baseControls();
  };
  worker.onerror=event=>{
    if(baseWorker!==worker)return;
    const message=event.message||'base-engine Worker failed';
    Object.assign(report.baseEngine,{state:'error',error:message,complete:false});
    record('base-engine-error',{message});element('status').textContent=message;
    worker.terminate();baseWorker=null;baseControls();
  };
  baseControls();record('base-engine-request',{...report.baseEngine});
  worker.postMessage({type:'base-create',request,engineUrl:query.get('engine')??'/engine.wasm',
    ...(reuse?{compiledEngine:baseCompiled}:{})});
}
for(const [id,reuse] of [['base-create',false],['base-reuse',true]])element(id).addEventListener('click',()=>{
  try{baseCreate(reuse);}catch(error){record('base-engine-error',{message:error.message});element('status').textContent=error.message;}
});
element('base-release').addEventListener('click',()=>{
  if(baseWorker)baseWorker.postMessage({type:'base-release',request:baseRequest});
});
element('graphics-batch').addEventListener('click',async()=>{
  let acquired=false;
  try {
    if(graphicsBatchRunning || baseWorker || audioOnly || !['idle','stopped'].includes(session.status().state))
      throw new Error('stop the guest and release the base engine before graphics validation');
    graphicsBatchRunning=true;acquired=true;baseControls();
    report.graphicsBatch={complete:false,requestedAtEpochMs:performance.timeOrigin+performance.now()};
    record('graphics-batch-request',report.graphicsBatch);
    element('status').textContent='validating graphics submission ordering…';
    const {runGraphicsBatchValidation}=await import('../graphics/validation.js');
    const result=await runGraphicsBatchValidation(document.createElement('canvas'));
    report.graphicsBatch={...report.graphicsBatch,complete:true,result};
    element('status').textContent=`graphics validation passed: ${result.queueSubmissions} queue submissions · ${result.draws} draws · ${result.presents.length} completed Presents`;
    record('graphics-batch-result',report.graphicsBatch);
  } catch(error) {
    if(graphicsBatchRunning)report.graphicsBatch={...report.graphicsBatch,complete:false,error:error.message};
    record('graphics-batch-error',{message:error.message});element('status').textContent=error.message;
  } finally {if(acquired)graphicsBatchRunning=false;baseControls();}
});
element('audio-only').addEventListener('click',async()=>{
  if(audioOnly || graphicsBatchRunning || baseWorker || !['idle','stopped'].includes(session.status().state))return;
  const result={complete:false,noGuest:true,requestedLiveMs:2000,statistics:[],
    sourceUrls:['./browser.mjs','./audio-only-worker.mjs','../browser/audio/host.js','../browser/audio/processor.js',
      '../runtime/engine.js','../runtime/media.js'].map(path=>new URL(path,import.meta.url).href),
    semantics:'frames copied into Web Audio output; not device playback',
    limits:['test-only Worker uses production Media/AudioHost/AudioWorklet; no RuntimeSession CPU loop or guest API producer',
      'live snapshots run while PCM production continues; hold/freeze underruns are reported separately',
      'a zero live-underrun count does not prove the guest is the cause of a game underrun',
      'initial base-engine creation is outside the live window; background compiler tier completion is unknown']};
  report.audioOnly=result;
  const request=++audioRequest,pending=new Map();let nextId=0,closing=false,cancelled=false,failure=null;
  let firstResolve,firstReject;
  const firstProgress=new Promise((resolve,reject)=>{firstResolve=resolve;firstReject=reject;});
  void firstProgress.catch(()=>{});
  const worker=new Worker(new URL('./audio-only-worker.mjs',import.meta.url),{type:'module'});
  const host=new AudioHost({onDiagnostic:message=>{result.lastDiagnostic=message;},onStats:value=>{
    result.statistics.push(value);if(result.statistics.length>8)result.statistics.shift();
    if(value.event==='paused')result.paused=value;
    if(value.event==='closed')result.closed=value;
    record('audio-only-stats',value);
  }});
  const active={worker,host,request,cancel:null};audioOnly=active;baseControls();
  const fail=error=>{
    failure??=error;firstReject(error);
    for(const entry of pending.values()){clearTimeout(entry.timer);entry.reject(error);}pending.clear();
  };
  const ask=type=>new Promise((resolve,reject)=>{
    if(cancelled){reject(new Error('audio-only control cancelled'));return;}
    const id=++nextId,timer=setTimeout(()=>{pending.delete(id);reject(new Error(`${type} timed out`));},type==='audio-only-create'?30000:5000);
    const expected={'audio-only-create':'audio-only-ready','audio-only-start':'audio-only-started','audio-only-hold':'audio-only-held',
      'audio-only-snapshot':'audio-only-snapshot','audio-only-close':'audio-only-closed'}[type];
    pending.set(id,{resolve,reject,timer,expected});
    try{worker.postMessage({type,request,id,engineUrl:query.get('engine')??'/engine.wasm'});}
    catch(error){clearTimeout(timer);pending.delete(id);reject(error);}
  });
  active.cancel=()=>{cancelled=true;closing=true;fail(new Error('audio-only control cancelled'));worker.terminate();void host.close().catch(()=>{});};
  worker.onmessage=({data})=>{
    if(audioOnly!==active || data.request!==request || cancelled)return;
    if(data.type==='pcm') {
      if(closing)return;
      void host.accept(data,progress=>{
        if(audioOnly!==active || closing)return;
        const {epoch,sequence,firstFrame,frames,consumedFrames}=progress;
        worker.postMessage({type:'pcm-consumed',request,epoch,sequence,firstFrame,frames,consumedFrames});firstResolve();
      }).catch(error=>{if(!closing)fail(error);});
    } else if(data.type==='audio-only-error')fail(new Error(data.message));
    else {
      const entry=pending.get(data.id);if(!entry)return;
      if(data.type!==entry.expected){fail(new Error('audio-only control acknowledgement changed'));return;}
      pending.delete(data.id);clearTimeout(entry.timer);entry.resolve(data);
    }
  };
  worker.onerror=event=>fail(new Error(event.message||'audio-only Worker failed'));
  const check=(condition,message)=>{if(!condition)throw new Error(message);};
  try {
    element('status').textContent='validating native audio pipeline without guest execution…';
    record('audio-only-request',{request,requestedLiveMs:2000});
    await host.start();host.setVolume(0,true); // the connected output graph runs with muted gain.
    result.ready=await ask('audio-only-create');
    check(result.ready.type==='audio-only-ready'&&result.ready.coreContext===0&&result.ready.cpuExecutionRequested===false,'invalid audio-only setup');
    await ask('audio-only-start');
    let progressTimer;
    try {await Promise.race([firstProgress,new Promise((_,reject)=>{progressTimer=setTimeout(()=>reject(new Error('no audio output progress')),10000);})]);}
    finally{clearTimeout(progressTimer);}
    const startBefore=performance.timeOrigin+performance.now(),start=await host.control('resume');
    const startAfter=performance.timeOrigin+performance.now();
    await new Promise(resolve=>setTimeout(resolve,2000));if(failure)throw failure;
    const endBefore=performance.timeOrigin+performance.now(),end=await host.control('resume'),endAfter=performance.timeOrigin+performance.now();
    result.live={startBefore,startAfter,start,endBefore,endAfter,end,
      renderedFrames:end.renderedFrames-start.renderedFrames,
      underruns:end.underruns-start.underruns,underrunFrames:end.underrunFrames-start.underrunFrames};
    record('audio-only-live-end',result.live);
    check(!start.paused&&!end.paused&&start.epoch===result.ready.epoch&&end.epoch===start.epoch,'live audio identity changed');
    check(start.capacityFrames===1024&&end.capacityFrames===1024&&start.primeFrames===512&&end.primeFrames===512,'audio queue configuration changed');
    check(result.live.renderedFrames>0&&result.live.underruns>=0&&result.live.underrunFrames>=0,'invalid live progress');
    result.hold=await ask('audio-only-hold');await host.pause();
    const native=await ask('audio-only-snapshot'),frozen=result.paused;result.native=native;
    check(frozen?.paused&&frozen.contextState==='suspended','output did not freeze');
    result.teardown={underruns:frozen.underruns-end.underruns,underrunFrames:frozen.underrunFrames-end.underrunFrames,
      renderedFrames:frozen.renderedFrames-end.renderedFrames};
    check(result.teardown.underruns>=0&&result.teardown.underrunFrames>=0&&result.teardown.renderedFrames>=0,'teardown counters moved backwards');
    const [submitted,consumed,head,headFrames,reserved]=native.state.values;
    const remaining=native.queue.reduce((sum,p)=>sum+p.frames-p.consumedFrames,0);
    check(native.state.epoch===frozen.epoch&&native.position.epoch===frozen.epoch,'native epoch changed');
    check(submitted===BigInt(frozen.acceptedFrames)&&consumed===BigInt(frozen.renderedFrames),'native/output progress differs after quiescence');
    check(frozen.acceptedFrames-frozen.renderedFrames===frozen.queuedFrames&&remaining===frozen.queuedFrames&&remaining<=1024,'queued credit differs');
    check(native.maximumQueue<=4&&native.queue.length<=4&&native.coreContext===0&&native.progress>0,'invalid bounded audio-only state');
    check(native.position.values[0]===consumed%4n&&reserved===0n,'native voice cursor differs from consumed frames');
    check(head===(native.queue[0]?.sequence??0n)&&headFrames===BigInt(native.queue[0]?.frames??0),'native pending head differs');
    check(host.renderedFrames===frozen.renderedFrames&&host.acceptedFrames===frozen.acceptedFrames,'host/output accounting differs');
    if(failure)throw failure;
    closing=true;await host.close();result.nativeClose=await ask('audio-only-close');
    check(result.closed?.contextState==='closed'&&host.pending.size===0&&host.controls.size===0,'output did not close');
    check(result.closed.renderedFrames===frozen.renderedFrames&&result.closed.cancelledPendingFrames===remaining,'close fabricated consumption');
    check(result.nativeClose.closedAudioStatus===5,'native close failed');
    result.liveUnderrunObserved=result.live.underruns>0;result.complete=true;
    element('status').textContent=`audio-only validation complete: ${result.live.renderedFrames} live frames · ${result.live.underruns} live underruns · ${result.teardown.underruns} hold/freeze underruns`;
    record('audio-only-complete',result);
  } catch(error) {
    result.complete=false;result.error=error.message;record('audio-only-error',{message:error.message});element('status').textContent=error.message;
  } finally {
    closing=true;
    try{await host.close();}catch(error){result.complete=false;result.cleanupError=error.message;}
    if(audioOnly===active){worker.terminate();audioOnly=null;}
    for(const entry of pending.values()){clearTimeout(entry.timer);entry.reject(new Error('audio-only control closed'));}pending.clear();
    record('audio-only-final',{complete:result.complete});baseControls();
  }
});
window.addEventListener('pagehide',()=>{baseWorker?.terminate();baseWorker=null;audioOnly?.cancel();});
