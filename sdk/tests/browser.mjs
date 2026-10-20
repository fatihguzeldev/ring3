import {createSession,selectedFolder} from '../index.js';
const element = id => document.getElementById(id), query = new URL(location.href).searchParams;
const events = [], report = {backend:'core',events};
const execution = Object.fromEntries(['capacity','budget','instructions','blocks']
  .filter(name => query.has(name)).map(name => [name,Number(query.get(name))]));
if (query.has('chain')) execution.chain = query.get('chain') === '1';
let local = null;
let baseWorker=null,baseCompiled=null,baseRequest=0;
let graphicsBatchRunning=false;
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
  element('base-create').disabled=baseWorker!==null || graphicsBatchRunning;
  element('base-reuse').disabled=baseWorker!==null || baseCompiled===null || graphicsBatchRunning;
  element('base-release').disabled=baseWorker===null;
  element('graphics-batch').disabled=graphicsBatchRunning || baseWorker!==null || !['idle','stopped'].includes(report.status?.state ?? 'idle');
}
function baseCreate(reuse) {
  if(baseWorker)throw new Error('release the current base engine first');
  if(graphicsBatchRunning)throw new Error('wait for graphics validation before a base-engine control');
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
    if(graphicsBatchRunning || baseWorker || !['idle','stopped'].includes(session.status().state))
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
window.addEventListener('pagehide',()=>{baseWorker?.terminate();baseWorker=null;});
