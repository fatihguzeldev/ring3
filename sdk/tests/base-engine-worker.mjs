import {Engine} from '../runtime/engine.js';

let engine=null,compiledModule=null,used=false,control=Promise.resolve();
const stamp=()=>{const offsetMs=performance.now();return {originMs:performance.timeOrigin,offsetMs,epochMs:performance.timeOrigin+offsetMs};};
const send=value=>postMessage({...value,noGuest:true});

async function create(message) {
  if(used)throw new Error('a base-engine request requires a fresh Worker');
  used=true;
  let encodedBytes=0,fetches=0,bodyReads=0;
  const originalFetch=self.fetch;
  self.fetch=async(...args)=>{
    fetches++;
    const response=await originalFetch.apply(self,args),arrayBuffer=response.arrayBuffer.bind(response);
    response.arrayBuffer=async()=>{const bytes=await arrayBuffer();bodyReads++;encodedBytes=bytes.byteLength;return bytes;};
    return response;
  };
  const before=stamp();
  performance.mark('ring3.base-engine.before');
  try {
    engine=await Engine.create(message.engineUrl,undefined,undefined,message.compiledEngine);
    const ready=stamp();
    performance.mark('ring3.base-engine.ready');
    if(fetches!==1 || bodyReads!==1 || !Number.isSafeInteger(encodedBytes) || encodedBytes<8)
      throw new Error('base-engine input was not exactly one complete encoded module');
    if(typeof engine.api.core_context_ptr!=='function' || engine.api.core_context_ptr()!==0)
      throw new Error('base-engine control unexpectedly has a core context');
    compiledModule=engine.module;
    send({type:'base-ready',request:message.request,before,ready,createWallMs:ready.offsetMs-before.offsetMs,
      engineHash:engine.hash,encodedBytes,fetches,bodyReads,
      suppliedCompiledModule:message.compiledEngine!==undefined,
      reusedModule:message.compiledEngine?.module===compiledModule,
      configurationApplied:false,coreContext:0,
      cacheCeiling:'fresh request is not guaranteed browser compiler-cache cold; reuse does not prove background compilation finished',
      compiledEngine:{module:compiledModule,hash:engine.hash}});
  } finally {self.fetch=originalFetch;}
}

function release(message) {
  if(!engine)throw new Error('no base engine to release');
  const before=stamp(),status=engine.status('close');
  if(status!==0)throw new Error(`base-engine close returned ${status}`);
  engine=null;compiledModule=null;
  const released=stamp();
  performance.mark('ring3.base-engine.released');
  send({type:'base-released',request:message.request,before,released,status});
}

self.onmessage=({data})=>{
  if(!['base-create','base-release'].includes(data?.type))return;
  control=control.then(()=>data.type==='base-create'?create(data):release(data))
    .catch(error=>send({type:'base-error',request:data.request,message:error.message,at:stamp()}));
};
