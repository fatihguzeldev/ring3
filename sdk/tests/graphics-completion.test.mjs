import assert from 'node:assert/strict';
import test from 'node:test';
import {createGraphicsHost} from '../graphics/index.js';
import {Session} from '../browser/session.js';

const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};};
const turn=()=>new Promise(resolve=>setImmediate(resolve));
const surface=[1,0,0,1,2,0,0,0];
const definition=[1,0,0,1,1,21,1,0,1,1,1,1,0,1,0,0,1,1,1,1,4,4,4,0,0];
const clear=[...surface,0,1,0xff123456,0x3f800000,0,0];
const present=[1,0,1,0,1,0,...surface,0,0,0];
const readback=[1,0,1,0,...surface,1,1,21,1,1,1,4,4,4];
function packet(commands) {
  const bytes=new Uint8Array(64+commands.reduce((sum,[,words])=>sum+32+words.length*4,0)),view=new DataView(bytes.buffer);
  bytes.set(new TextEncoder().encode('R3D8'));view.setUint16(4,2,true);view.setUint16(6,8,true);
  view.setUint32(8,bytes.length,true);view.setUint32(12,commands.length,true);
  for(const offset of [16,24,32])view.setBigUint64(offset,1n,true);
  view.setBigUint64(40,BigInt(commands.length),true);
  let at=64;
  for(const [index,[opcode,words]] of commands.entries()) {
    view.setUint16(at,opcode,true);view.setUint16(at+2,3,true);view.setUint32(at+4,32+words.length*4,true);
    view.setBigUint64(at+8,BigInt(index+1),true);view.setUint32(at+20,words.length*4,true);
    words.forEach((value,i)=>view.setUint32(at+32+i*4,value,true));at+=32+words.length*4;
  }
  return bytes.buffer;
}

async function graphicsFixture(t) {
  const fence=deferred(),mapping=deferred(),events=[],presents=[],buffers=[],errors=[];
  const globals={navigator:{gpu:{async requestAdapter(){return adapter;},getPreferredCanvasFormat(){return 'rgba8unorm';}}},
    GPUTextureUsage:{COPY_SRC:1,COPY_DST:2,TEXTURE_BINDING:4,RENDER_ATTACHMENT:8},
    GPUBufferUsage:{STORAGE:1,COPY_DST:2,COPY_SRC:4,MAP_READ:8,INDEX:16},GPUMapMode:{READ:1},GPUShaderStage:{VERTEX:1,FRAGMENT:2}};
  const saved=new Map(Object.keys(globals).map(name=>[name,Object.getOwnPropertyDescriptor(globalThis,name)]));
  for(const [name,value] of Object.entries(globals))Object.defineProperty(globalThis,name,{configurable:true,writable:true,value});
  t.after(()=>{for(const [name,descriptor] of saved)if(descriptor)Object.defineProperty(globalThis,name,descriptor);else delete globalThis[name];});
  const queue={
    submit(){events.push('submit');},writeTexture(){events.push('write-texture');},writeBuffer(){events.push('write-buffer');},
    onSubmittedWorkDone(){events.push('fence');return fence.promise;},
  };
  const device={queue,features:[],limits:{maxTextureDimension2D:4096,maxStorageBufferBindingSize:1024*1024,minStorageBufferOffsetAlignment:256},lost:new Promise(()=>{}),
    addEventListener(){},pushErrorScope(){},async popErrorScope(){return null;},destroy(){events.push('device-destroy');},
    createTexture(){return {createView(){return {};},destroy(){events.push('texture-destroy');}};},
    createBuffer({size}){const buffer={destroyed:false,async mapAsync(){events.push('map');await mapping.promise;},getMappedRange(){return new ArrayBuffer(size);},unmap(){events.push('unmap');},destroy(){this.destroyed=true;events.push('buffer-destroy');}};buffers.push(buffer);return buffer;},
    createCommandEncoder(){return {beginRenderPass(){return {setPipeline(){},setBindGroup(){},setViewport(){},draw(){},setIndexBuffer(buffer,format){events.push(['index-buffer',buffer,format]);},drawIndexed(...args){events.push(['indexed',...args]);},end(){events.push('pass-end');}};},beginComputePass(){return {setPipeline(){},setBindGroup(){},dispatchWorkgroups(){},end(){events.push('compute-end');}};},copyTextureToBuffer(){events.push('copy-to-buffer');},finish(){events.push('finish');return {};}};},
    createShaderModule({code}){return {code,async getCompilationInfo(){return {messages:[]};}};},
    async createRenderPipelineAsync(descriptor){return {descriptor,getBindGroupLayout(){return {};}};},createPipelineLayout(value){return value;},createBindGroupLayout(value){return value;},createBindGroup(){return {};},createSampler(){return {};},
  };
  const adapter={features:[],limits:device.limits,async requestDevice(){return device;}};
  const context={configure(){},unconfigure(){},getCurrentTexture(){return {createView(){return {};}};}};
  const host=await createGraphicsHost({width:1,height:1,getContext(){return context;}},{onDiagnostic:value=>{if(value.type==='present')presents.push(value);if(value.type==='error')errors.push(value);}});
  t.after(()=>{fence.resolve();mapping.resolve();host.close();});
  return {host,fence,mapping,events,presents,buffers,errors};
}

test('canvas configures once at its initial extent and again only after a real resize',async t=>{
  const {host,fence,presents,events}=await graphicsFixture(t),configurations=[];
  host.context.configure=descriptor=>configurations.push(descriptor);
  host.resize(1,1);
  assert.equal(configurations.length,1,'an already matching initial canvas still needs configuration');
  for(let i=0;i<10;i++)host.resize(1,1);
  assert.equal(configurations.length,1,'unchanged dimensions must not replace the canvas configuration');
  host.resize(2,1);host.resize(2,1);
  assert.equal(configurations.length,2);assert.equal(host.canvas.width,2);assert.equal(host.canvas.height,1);
  host.resize(1,1);
  assert.equal(configurations.length,3);
  assert(configurations.every(row=>row.device===host.device&&row.format===host.format&&row.alphaMode==='opaque'));
  fence.resolve();await host.consume(packet([[1,definition],[11,present],[11,present]]));
  assert.equal(configurations.length,3,'same-size Presents retain the existing configuration');
  assert.deepEqual(presents.map(row=>row.presents),[1,2]);
  assert.equal(events.filter(row=>row==='fence').length,2,'each Present keeps its completion fence');
});

function pipelineState() {
  const render=Array(174).fill(0);
  for(const [index,value] of [[8,3],[19,2],[20,1],[22,1],[23,4],[25,8],[168,15],[171,1],[172,3]])render[index]=value;
  const stages=Array.from({length:8},()=>{const state=Array(29).fill(0);state[1]=state[4]=state[13]=state[14]=1;return state;});
  return {render,stages,lights:[],viewport:{x:0,y:0,width:1,height:1,min:0,max:1},
    vertexShader:0x44,pixelShader:0,transforms:[],material:Array(17).fill(0),
    vc:Array.from({length:96},()=>[0,0,0,0]),pc:Array.from({length:8},()=>[0,0,0,0])};
}

async function pipelineFixture(t) {
  const f=await graphicsFixture(t);await f.host.consume(packet([[1,definition]]));
  const renderer=f.host.sessions.get(1n),targets={color:{item:{gpuFormat:'rgba8unorm'}}};
  renderer.pipelineInputMemoEnabled=true;
  return {...f,renderer,targets,run:(state,kind='triangle-list',kinds=Array(8).fill(null),patch=false,output=targets)=>renderer.gpuPipeline(state,kind,output,kinds,patch)};
}

test('normal render passes reuse attachments across state snapshots and close before barriers',async t=>{
  const {host,events}=await graphicsFixture(t);await host.consume(packet([[1,definition]]));
  const renderer=host.sessions.get(1n),state=pipelineState(),calls=[];let active=null,next=0;
  host.device.createCommandEncoder=()=>({beginRenderPass(){
    assert.equal(active,null,'encoder operations cannot nest in an open pass');const id=++next;active=id;calls.push(['begin',id]);
    return Object.fromEntries(['setPipeline','setBindGroup','setViewport','setScissorRect','setStencilReference','setBlendConstant','draw'].map(name=>[name,(...args)=>calls.push([name,id,...args])]).concat([['end',()=>{assert.equal(active,id);active=null;calls.push(['end',id]);}]]));
  },finish(){assert.equal(active,null);calls.push(['finish']);return {};}});
  const color={texture:{},gpuFormat:'rgba8unorm'},depth={texture:{},gpuFormat:'depth24plus-stencil8'};
  let level=0,depthTexture=depth.texture;
  renderer.state=()=>state;renderer.program=()=>({});renderer.resources.packStreams=()=>({buffer:{},temporary:false});
  renderer.renderTargets=()=>({color:{item:color,sub:{info:{width:4,height:4,level,face:0}},view:{}},depth:{item:{...depth,texture:depthTexture},sub:{info:{level:0,face:0}},view:{}}});
  renderer.textureBindings=()=>({kinds:[],entries:[]});renderer.gpuPipeline=async()=>({getBindGroupLayout:()=>({})});
  const draw=async()=>{const words=[4,1,...Array(25).fill(0),0,0];await renderer.draw({u64:()=>1n,targets:()=>({}),u32:()=>words.shift()});assert.equal(words.length,0);};
  const command=(opcode,words)=>{const bytes=new Uint8Array(words.length*4),v=new DataView(bytes.buffer);words.forEach((x,i)=>v.setUint32(i*4,x,true));return {opcode,bytes};};
  assert.equal(renderer.renderPassReuseEnabled,false);await draw();await draw();renderer.flush();
  assert.equal(calls.filter(row=>row[0]==='begin').length,2);assert.equal(renderer.normalPasses,2);
  renderer.configureRenderPassReuse(true);calls.length=0;await draw();
  const snapshot=[2,0,2,174,...state.render,8,...state.stages.flatMap(row=>[29,...row]),266,9,...Array(9+17).fill(0),0,0,0,0,0,0,1,1,0,0x3f800000,0,0,0,96,...Array(384).fill(0),8,...Array(32).fill(0)];
  await renderer.execute(command(6,snapshot));state.viewport.x=1;state.render[57]=7;state.render[60]=0xff112233;await draw();
  assert.equal(calls.filter(row=>row[0]==='begin').length,1);assert.equal(calls.filter(row=>row[0]==='end').length,0);
  assert.deepEqual(calls.filter(row=>row[0]==='setViewport').map(row=>row[2]),[0,1]);
  for(const name of ['setPipeline','setBindGroup','setScissorRect','setStencilReference','setBlendConstant'])assert.equal(calls.filter(row=>row[0]===name).length,2);
  assert.equal(renderer.renderPassReuses,1);assert.throws(()=>renderer.configureRenderPassReuse(false),/idle renderer/);
  depthTexture={};await draw();assert.deepEqual(calls.filter(row=>['begin','end'].includes(row[0])).map(row=>row[0]),['begin','end','begin']);
  level=1;await draw();assert.equal(calls.filter(row=>row[0]==='begin').length,3,'a different mip cannot reuse the attachment');
  await renderer.clear({targets:()=>({}),u32:(()=>{const a=[1,0,0,0];return()=>a.shift();})(),f32:()=>1,count:()=>0});
  assert.equal(active,null,'clear owns a distinct completed pass');
  await draw();const upload=renderer.resources.upload;renderer.resources.upload=()=>{assert.equal(active,null);calls.push(['upload']);};
  await renderer.execute(command(3,[]));renderer.resources.upload=upload;
  assert.equal(active,null);assert.equal(renderer.pendingEncoder,null);assert.equal(calls.at(-1)[0],'upload');
  await draw();const submits=events.filter(x=>x==='submit').length;
  const execute=renderer.execute;renderer.execute=async()=>{throw Error('command failed with open pass');};
  const bad=packet([[1,definition]]),view=new DataView(bad);for(const at of [24,32,40,72])view.setBigUint64(at,2n,true);
  await assert.rejects(renderer.accept(bad),/command failed with open pass/);renderer.execute=execute;
  assert.equal(active,null);assert.equal(renderer.pendingPass,null);assert.equal(renderer.pendingEncoder,null);assert.equal(events.filter(x=>x==='submit').length,submits);
  await draw();host.close();assert.equal(active,null);assert.equal(renderer.pendingPass,null);
});

test('deferred buffer uploads preserve earlier draws until submission and replace same-version packs',async t=>{
  const {host,events}=await graphicsFixture(t);await host.consume(packet([[1,definition]]));
  const renderer=host.sessions.get(1n),device=host.device,ref={key:'1:1:1',version:1n},item={buffer:true,type:6,version:1n,subs:[{bytes:Uint8Array.of(1,2,3,4,5,6,7,8)}]};
  renderer.resources.items.set(ref.key,item);
  device.queue.writeBuffer=(buffer,offset,bytes)=>{buffer.bytes??=new Uint8Array(bytes.byteLength);buffer.bytes.set(bytes,offset);events.push('write-buffer');};
  renderer.state=()=>pipelineState();renderer.renderTargets=()=>({color:{view:{}}});renderer.program=()=>({});renderer.dynamic=()=>{};
  renderer.textureBindings=()=>({kinds:[],entries:[]});renderer.gpuPipeline=async()=>({getBindGroupLayout:()=>({})});
  const command=(opcode,words)=>{const bytes=new Uint8Array(words.length*4),v=new DataView(bytes.buffer);words.forEach((x,i)=>v.setUint32(i*4,x,true));return {opcode,bytes};};
  const upload=value=>command(3,[1,0,1,1,1,0,0,0,8,8,8,1,1,8,value,0x08070605]);
  const draw=async()=>{const values=[4,1,...Array(8).fill(0),1,4,...Array(18).fill(0)];await renderer.draw({u64:()=>1n,targets:()=>({}),version:()=>ref,u32:()=>values.shift()});assert.equal(values.length,0);return [...renderer.resources.packedStreams.values()].at(-1);};
  assert.equal(renderer.deferredBufferUploadsEnabled,false);const baseline=await draw();events.length=0;await renderer.execute(upload(0x0c0b0a09));
  assert.deepEqual(events.filter(x=>x==='submit'||x==='buffer-destroy'),['submit','buffer-destroy']);assert.equal(baseline.buffer.destroyed,true);
  renderer.configureDeferredBufferUploads(true);const old=await draw();events.length=0;
  await renderer.execute(upload(0x100f0e0d));assert.equal(old.buffer.destroyed,false);assert.equal(events.includes('submit'),false);
  const fresh=await draw();assert.notEqual(old.buffer,fresh.buffer);
  assert.deepEqual([...old.buffer.bytes.subarray(128,132)],[9,10,11,12]);assert.deepEqual([...fresh.buffer.bytes.subarray(128,132)],[13,14,15,16]);
  assert.equal(renderer.deferredBufferUploads,1);assert.equal(renderer.deferredBufferRetirements,1);assert.equal(renderer.retiredBufferBytes,136);
  assert.throws(()=>renderer.configureDeferredBufferUploads(false),/idle renderer/);renderer.flush();
  assert.deepEqual(events.filter(x=>x==='submit'||x==='buffer-destroy'),['submit','buffer-destroy']);assert.equal(renderer.retiredBufferBytes,0);assert.equal(fresh.buffer.destroyed,false);
  const bad=packet([[3,[1,0,1,1,1,0,0,0,8,16,4,2,1,8,0x14131211,0x18171615]]]),view=new DataView(bad);for(const at of [24,32,40,72])view.setBigUint64(at,2n,true);
  await assert.rejects(renderer.accept(bad),/upload bounds/);assert.equal(fresh.buffer.destroyed,true);assert.equal(renderer.retiredBufferBytes,0);assert.equal(renderer.resources.packedStreams.size,0);
});

test('deferred buffer retirement is bounded and texture or retained-index uploads keep submission barriers',async t=>{
  const {host,events,buffers}=await graphicsFixture(t);await host.consume(packet([[1,definition]]));const renderer=host.sessions.get(1n);
  renderer.configureDeferredBufferUploads(true);events.length=0;
  const a=host.device.createBuffer({size:32*1024*1024}),b=host.device.createBuffer({size:4});renderer.encoder();
  renderer.retireBuffer(a,32*1024*1024);assert.equal(a.destroyed,false);renderer.retireBuffer(b,4);
  assert.deepEqual(events.filter(x=>x==='submit'||x==='buffer-destroy'),['submit','buffer-destroy']);assert.equal(a.destroyed,true);assert.equal(b.destroyed,false);
  assert.equal(renderer.retiredBufferBytes,4);assert.equal(renderer.deferredBufferUploadFlushes,1);renderer.flush();assert.equal(b.destroyed,true);
  const command=words=>{const bytes=new Uint8Array(words.length*4),v=new DataView(bytes.buffer);words.forEach((x,i)=>v.setUint32(i*4,x,true));return {opcode:3,bytes};};
  renderer.encoder();events.length=0;await renderer.execute(command([1,0,0,1,2,0,0,0,4,4,4,1,1,4,0xff123456]));
  assert.deepEqual(events.filter(x=>x==='submit'||x==='write-texture'),['submit','write-texture']);assert.equal(renderer.deferredBufferUploads,0);
  const ref={key:'1:2:1',version:1n};renderer.resources.items.set(ref.key,{buffer:true,type:7,format:101,version:1n,subs:[{bytes:Uint8Array.of(0,0,1,0)}]});
  const indices=renderer.resources.retainedIndexBuffer(ref,101);renderer.encoder();events.length=0;
  await renderer.execute(command([1,0,2,1,1,0,0,0,4,4,4,1,1,4,0x00020001]));
  assert.deepEqual(events.filter(x=>x==='submit'||x==='write-buffer'),['submit','write-buffer']);assert.equal(renderer.deferredBufferUploads,0);assert.equal(indices.buffer.destroyed,false);
  renderer.close();assert.equal(renderer.retiredBufferBytes,0);assert.equal(buffers.every(x=>x.destroyed),true);
});

test('batched state uploads preserve aligned draw ranges and upload each encoder before submission',async t=>{
  const {host,events}=await graphicsFixture(t);await host.consume(packet([[1,definition]]));
  const renderer=host.sessions.get(1n),device=host.device,writes=[],entries=[],state=pipelineState();
  device.limits.maxStorageBufferBindingSize=37888;
  device.queue.writeBuffer=(buffer,offset,bytes,dataOffset=0,size=bytes.byteLength)=>{events.push('state-upload');writes.push({buffer,offset,bytes:bytes.slice(dataOffset,dataOffset+size)});};
  device.createBindGroup=descriptor=>{entries.push(descriptor.entries[0].resource);return {};};
  renderer.state=()=>state;renderer.renderTargets=()=>({color:{view:{}}});
  renderer.textureBindings=()=>({kinds:[],entries:[]});renderer.gpuPipeline=async()=>({getBindGroupLayout:()=>({})});
  renderer.resources.packStreams=()=>({buffer:{},temporary:false});renderer.program=()=>({});renderer.dynamic=()=>{};
  const draw=async value=>{
    state.vc[0][0]=value;state.material[0]=-value;
    const values=[4,1,...Array(25).fill(0),0,0];
    await renderer.draw({u64:()=>1n,targets:()=>({}),u32:()=>values.shift()});assert.equal(values.length,0);
  };
  assert.equal(renderer.stateUploadBatchEnabled,false);
  renderer.configureStateUploadBatch(true);
  await draw(1);await draw(2);await draw(3);
  assert.equal(writes.length,0,'draw staging must not upload until the shared encoder flushes');
  assert.throws(()=>renderer.configureStateUploadBatch(false),/idle renderer/);
  state.vc[0][0]=99;renderer.flush();
  assert.equal(writes.length,2);assert.deepEqual(entries.map(row=>[row.offset,row.size]),[[0,18768],[18944,18768],[0,18768]]);
  assert.equal(entries[0].buffer,entries[1].buffer);assert.notEqual(entries[0].buffer,entries[2].buffer);
  for(const [i,entry] of entries.entries()){
    const write=writes.find(row=>row.buffer===entry.buffer),view=new DataView(write.bytes.buffer);
    assert.equal(view.getFloat32(entry.offset,true),i+1);
    assert.equal(view.getFloat32(entry.offset+1168*16,true),-(i+1));
    assert.equal(entry.offset%device.limits.minStorageBufferOffsetAlignment,0);
  }
  assert.deepEqual(events.filter(value=>value==='state-upload'||value==='submit'),['state-upload','state-upload','submit']);
  await draw(4);renderer.flush();
  assert.equal(entries[3].buffer,entries[0].buffer,'a later encoder reuses the page after the previous submission');
  assert.equal(new DataView(writes[0].bytes.buffer).getFloat32(0,true),1,'earlier queued upload owns its original byte snapshot');
  assert.equal(new DataView(writes[2].bytes.buffer).getFloat32(0,true),4);
  assert.deepEqual(events.filter(value=>value==='state-upload'||value==='submit'),['state-upload','state-upload','submit','state-upload','submit']);
  assert.deepEqual([renderer.stateUploadBatchUses,renderer.stateUploadBatchUploads,renderer.stateUploadBatchPages],[4,3,2]);
  renderer.configureStateUploadBatch(false);host.close();
  assert.equal(writes.every(row=>row.buffer.destroyed),true);
});

test('failed packets discard unsubmitted state staging and close releases its GPU pages',async t=>{
  const {host,events}=await graphicsFixture(t);await host.consume(packet([[1,definition]]));
  const renderer=host.sessions.get(1n);renderer.configureStateUploadBatch(true);
  renderer.execute=async()=>{renderer.stateBinding(new Uint8Array(18768));renderer.encoder();throw Error('staged command failure');};
  const next=packet([[1,definition]]),view=new DataView(next);
  for(const at of [24,32,40,72])view.setBigUint64(at,2n,true);
  await assert.rejects(host.consume(next),/staged command failure/);
  assert.equal(renderer.pendingEncoder,null);assert.equal(renderer.stateUploadPages.length,0);
  assert.equal(events.includes('submit'),false);assert.equal(events.includes('write-buffer'),false);
  const pages=renderer.stateUploadPool.slice();assert.equal(pages.length,1);assert.equal(renderer.temporary.length,0);
  host.close();assert.equal(pages[0].buffer.destroyed,true);
});

test('state upload pooling keeps eight pages and releases overflow through normal packet cleanup',async t=>{
  const {host}=await graphicsFixture(t);await host.consume(packet([[1,definition]]));
  const renderer=host.sessions.get(1n);renderer.configureStateUploadBatch(true);
  host.device.limits.maxStorageBufferBindingSize=18768;
  const bindings=Array.from({length:9},()=>renderer.stateBinding(new Uint8Array(18768)));
  assert.equal(new Set(bindings.map(row=>row.buffer)).size,9,'one encoder never aliases its own state ranges');
  assert.equal(renderer.stateUploadPool.length,8);assert.equal(renderer.temporary.length,1);
  renderer.encoder();renderer.flush();
  const overflow=bindings[8].buffer,another=definition.slice();another[2]=1;
  const next=packet([[1,another]]),view=new DataView(next);
  for(const at of [24,32,40,72])view.setBigUint64(at,2n,true);
  await host.consume(next);
  assert.equal(overflow.destroyed,true);assert.equal(renderer.temporary.length,0);
  assert.equal(renderer.stateUploadPool.every(page=>!page.buffer.destroyed),true);
  host.close();assert.equal(bindings.every(row=>row.buffer.destroyed),true);
});

test('retained ordinary list indices preserve draw subsets and base vertices while other paths stay transient',async t=>{
  const {host,events}=await graphicsFixture(t);await host.consume(packet([[1,definition]]));
  const renderer=host.sessions.get(1n),ref={key:'1:2:1',version:1n};
  renderer.resources.items.set(ref.key,{buffer:true,type:7,format:101,version:1n,subs:[{bytes:Uint8Array.of(3,0,2,0,1,0,0,0,5,0,4,0)}]});
  renderer.state=()=>pipelineState();renderer.renderTargets=()=>({color:{view:{}}});
  renderer.textureBindings=()=>({kinds:[],entries:[]});renderer.gpuPipeline=async()=>({getBindGroupLayout:()=>({})});
  renderer.resources.packStreams=()=>({buffer:{},temporary:false});renderer.program=()=>({});renderer.dynamic=()=>{};
  const draw=async(primitive,start,base)=>{
    const values=[primitive,1,...Array(24).fill(0),1,101,base>>>0,1,0,6,start];
    await renderer.draw({u64:()=>1n,targets:()=>({}),version:()=>ref,u32:()=>values.shift()});assert.equal(values.length,0);
  };
  assert.equal(renderer.retainedIndicesEnabled,false);await draw(4,1,7);
  assert.equal(renderer.retainedIndexUses,0);assert.equal(renderer.resources.retainedIndexBuilds,0);
  renderer.retainedIndicesEnabled=true;await draw(4,1,7);await draw(2,2,-4);
  const indices=events.filter(row=>row[0]==='index-buffer'),calls=events.filter(row=>row[0]==='indexed');
  assert.notEqual(indices[0][1],indices[1][1]);assert.equal(indices[1][1],indices[2][1]);
  assert.deepEqual(indices.map(row=>row[2]),['uint32','uint16','uint16']);
  assert.deepEqual(calls.map(row=>row.slice(1)),[[3,1,0,7,0],[3,1,1,7,0],[2,1,2,-4,0]]);
  assert.equal(renderer.retainedIndexUses,2);assert.equal(renderer.resources.retainedIndexHits,1);assert.equal(renderer.resources.retainedIndexBuilds,1);
  await draw(3,1,7);await draw(6,1,7);
  assert.equal(renderer.retainedIndexUses,2);assert.equal(renderer.resources.retainedIndexHits,1);
  renderer.resources.packStreams=()=>{
    for(const key of renderer.resources.packedStreams.keys())if(key.startsWith('indices:'))renderer.resources.releaseStreams(key);
    return {buffer:{},temporary:false};
  };
  await draw(4,1,7);
  assert.equal(renderer.retainedIndexUses,2);assert.equal(renderer.resources.retainedIndexHits,1);
  assert.deepEqual(events.filter(row=>row[0]==='indexed').at(-1).slice(1),[3,1,0,7,0],
    'shared-LRU eviction during stream packing uses a fresh transient index buffer');
});

test('pipeline reuse crosses uniform-only snapshots without walking immutable shader IR again',async t=>{
  const {renderer,run}=await pipelineFixture(t),state=pipelineState();let reads=0;
  const elements=[{stream:0,offset:0,register:0,kind:3},{stream:0,offset:16,register:5,kind:4}];
  renderer.shaders.set(0x80000001,{stage:1,program:null,declaration:{transformed:true,get elements(){reads++;return elements;}}});
  state.vertexShader=0x80000001;
  const first=await run(state),initialReads=reads,next=structuredClone(state);
  next.transforms=[[2,0,0,0,0,2,0,0,0,0,2,0,0,0,0,2]];next.vc[0]=[1,2,3,4];next.pc[0]=[5,6,7,8];next.material.fill(9);
  assert.equal(await run(next),first);
  assert.equal(reads,initialReads,'uniform-only snapshots must not regenerate unchanged WGSL');
  assert.deepEqual([renderer.pipelineInputHits,renderer.pipelineInputMisses,renderer.pipelineBuilds],[1,1,1]);
  renderer.pipelineInputMemoEnabled=false;
  assert.equal(await run(next),first);
  assert.deepEqual([renderer.pipelineInputHits,renderer.pipelineInputMisses,renderer.pipelineBuilds],[1,2,2]);
});

test('pipeline input reuse preserves shader replacement, code inputs and descriptor validation',async t=>{
  const {renderer,run,targets}=await pipelineFixture(t),state=pipelineState(),first=await run(state);
  const moved=structuredClone(state);moved.viewport.width=2;
  assert.notEqual(await run(moved),first);
  assert.notEqual(await run(state,'line-list'),first);
  assert.notEqual(await run(state,'triangle-list',Array(8).fill(null),true),first);
  assert.notEqual(await run(state,'triangle-list',Array(8).fill(null),false,{color:{item:{gpuFormat:'bgra8unorm'}}}),first);
  const stage=structuredClone(state);stage.stages[0][1]=2;stage.stages[0][2]=2;
  const kinds=['2d',...Array(7).fill(null)],textured=await run(stage,'triangle-list',kinds);
  const border=structuredClone(stage);border.stages[0][13]=4;border.stages[0][15]=0xff112233;
  assert.notEqual(await run(border,'triangle-list',kinds),textured);
  assert.notEqual(await run(stage,'triangle-list',['cube',...Array(7).fill(null)]),textured);
  const lit=structuredClone(state);lit.vertexShader=0x52;lit.render[137]=1;
  lit.lights=[{index:0,enabled:1,kind:3,direction:[0,0,-1],position:[0,0,0],range:1,falloff:1,attenuation:[1,0,0],theta:0,phi:0,diffuse:[1,1,1,1],specular:[0,0,0,0],ambient:[0,0,0,0]}];
  const lighting=await run(lit),changed=structuredClone(lit);changed.lights[0].diffuse[0]=0.5;
  assert.notEqual(await run(changed),lighting);
  const shader={stage:1,program:null,declaration:{transformed:true,elements:[{stream:0,offset:0,register:0,kind:3}]}};
  state.vertexShader=0x80000001;renderer.shaders.set(state.vertexShader,shader);
  const original=await run(state),replacement=structuredClone(shader);replacement.declaration.elements[0].offset=4;
  renderer.shaders.delete(state.vertexShader);renderer.shaders.set(state.vertexShader,replacement);
  assert.notEqual(await run(state),original,'a reused handle must resolve the new shader object');
  const invalid=structuredClone(state);invalid.render[8]=2;
  await assert.rejects(run(invalid),/fill mode/);
  invalid.render[8]=3;invalid.pixelShader=0x80000002;
  await assert.rejects(run(invalid),/pixel shader unavailable/);
  assert.equal(targets.color.item.gpuFormat,'rgba8unorm');
});

test('pipeline input memo stays bounded and cannot resurrect an evicted canonical GPU pipeline',async t=>{
  const {renderer,run}=await pipelineFixture(t),state=pipelineState();
  for(let i=0;i<300;i++){const variant=structuredClone(state);variant.render[173]=i;await run(variant);}
  assert.equal(renderer.pipelines.size,1,'conservative input differences still use the canonical full key');
  assert.ok(renderer.pipelineInputs.size<=256);
  state.render[15]=1;state.render[25]=2;
  let first;
  for(let i=0;i<129;i++){const variant=structuredClone(state);variant.render[24]=i;const gpu=await run(variant);if(i===0)first=gpu;}
  assert.equal(renderer.pipelines.size,128);
  assert.notEqual(await run(state),first,'stale input memo must miss after canonical LRU eviction');
  assert.ok(renderer.pipelineInputs.size<=256);
});

test('submit ACK returns before Present completion while its fence owns metrics and uniforms',async t=>{
  const {host,fence,presents,buffers}=await graphicsFixture(t);
  assert.equal(host.submissionAck,false);assert.equal(host.configureSubmissionAck(true),true);
  let accepted=false;const work=host.consume(packet([[1,definition],[11,present]])).then(result=>{accepted=true;return result;});
  await turn();assert.equal(accepted,true);assert.equal(presents.length,0);assert.equal(buffers[0].destroyed,false);
  const renderer=host.sessions.get(1n);renderer.draws=90;renderer.resources.bytes=777;
  host.canvas.width=9;host.canvas.height=8;
  assert.equal(host.submissionStatus().pendingPresentFences,1);
  assert.throws(()=>host.configureSubmissionAck(false),/idle|drain/);
  let drained=false;const draining=host.drain().then(()=>{drained=true;});
  await turn();assert.equal(drained,false);fence.resolve();await work;await draining;
  assert.equal(presents.length,1);assert.equal(buffers[0].destroyed,true);
  assert.equal(presents[0].draws,0);assert.equal(presents[0].resourceBytes,4);
  assert.equal(presents[0].width,1);assert.equal(presents[0].height,1);
  assert.equal(host.submissionStatus().pendingPresentFences,0);assert.equal(host.configureSubmissionAck(false),false);
});

test('submit ACK bounds unfinished Presents at two and snapshots every completed frame',async t=>{
  const {host,fence,presents,events}=await graphicsFixture(t);host.configureSubmissionAck(true);
  let accepted=false;const work=host.consume(packet([[1,definition],[11,present],[11,present],[11,present]])).then(()=>{accepted=true;});
  await turn();assert.equal(accepted,false);assert.equal(host.submissionStatus().pendingPresentFences,2);
  assert.equal(events.filter(value=>value==='submit').length,2);assert.equal(presents.length,0);
  fence.resolve();await work;await host.drain();
  assert.deepEqual(presents.map(value=>value.presents),[1,2,3]);
  assert.equal(presents.every(value=>value.width===1&&value.height===1&&value.epoch==='1'),true);
});

test('a frame-probed Present cannot bypass two unfinished submit-ACK Presents',async t=>{
  const {host,fence}=await graphicsFixture(t);host.configureSubmissionAck(true);
  await host.consume(packet([[1,definition],[11,present],[11,present]]));
  const next=packet([[11,present]]),view=new DataView(next);
  view.setBigUint64(24,2n,true);for(const at of [32,40,72])view.setBigUint64(at,4n,true);
  let accepted=false;const work=host.consume(next,{proof_id:'bounded-probe'}).then(()=>{accepted=true;});
  await turn();assert.equal(accepted,false);assert.equal(host.submissionStatus().pendingPresentFences,2);
  fence.resolve();await work;await host.drain();
});

test('concurrent cold device Presents recheck the global cap after async pipeline creation',async t=>{
  const {host,fence}=await graphicsFixture(t);host.configureSubmissionAck(true);
  const works=[1,2,3].map(epoch=>{
    const resource=definition.slice(),frame=present.slice();resource[0]=epoch;frame[6]=epoch;
    const bytes=packet([[1,resource],[11,frame]]);new DataView(bytes).setBigUint64(16,BigInt(epoch),true);
    return host.consume(bytes);
  });
  await turn();assert.equal(host.submissionStatus().pendingPresentFences,2);
  fence.resolve();await Promise.all(works);await host.drain();
});

test('submit ACK fence rejection fails drain without emitting a completed Present',async t=>{
  const {host,fence,presents,buffers,errors}=await graphicsFixture(t);host.configureSubmissionAck(true);
  await host.consume(packet([[1,definition],[11,present]]));
  fence.reject(new Error('late GPU failure'));await turn();
  assert.equal(presents.length,0);assert.equal(buffers[0].destroyed,true);assert.equal(errors.length,1);
  await assert.rejects(host.drain(),/late GPU failure/);
});

test('forced close suppresses late Present diagnostics while retaining fence cleanup',async t=>{
  const {host,fence,presents,buffers}=await graphicsFixture(t);host.configureSubmissionAck(true);
  await host.consume(packet([[1,definition],[11,present]]));host.close();fence.resolve();await turn();
  assert.equal(presents.length,0);assert.equal(buffers[0].destroyed,true);
});

test('device retirement preserves its submitted Present completion while the host stays open',async t=>{
  const {host,fence,presents,buffers}=await graphicsFixture(t);host.configureSubmissionAck(true);
  await host.consume(packet([[1,definition],[11,present],[13,[1,0,3]]]));
  assert.equal(host.sessions.has(1n),false);assert.equal(presents.length,0);
  fence.resolve();await host.drain();await host.drain();
  assert.equal(presents.length,1);assert.equal(presents[0].epoch,'1');assert.equal(presents[0].presents,1);
  assert.equal(buffers[0].destroyed,true);
});

test('non-Present acceptance does not wait for GPU completion but explicit drain does',async t=>{
  const {host,fence,events}=await graphicsFixture(t);
  let settled=false,draining;
  const work=host.consume(packet([[1,definition],[9,clear]])).then(value=>{settled=true;return value;});
  try {
    await turn();
    assert.equal(events.filter(value=>value==='submit').length,1);
    assert.equal(settled,true,'GPU progress must not gate non-Present packet acceptance');
    assert.equal(events.includes('fence'),false);
    assert.deepEqual(await work,{epoch:1n,sequence:1n,readbacks:[]});
    let drained=false;draining=host.drain().then(()=>{drained=true;});
    await turn();assert.equal(events.filter(value=>value==='fence').length,1);assert.equal(drained,false);
    fence.resolve();await draining;assert.equal(drained,true);
  } finally {fence.resolve();await work;await draining;}
});

test('Present completes only after its own GPU fence without a second packet fence',async t=>{
  const {host,fence,events,presents,buffers}=await graphicsFixture(t);
  let settled=false;
  const work=host.consume(packet([[1,definition],[9,clear],[11,present]]),{proof_id:'completion-test'}).then(value=>{settled=true;return value;});
  try {
    await turn();
    assert.equal(settled,false);assert.equal(presents.length,0);
    assert.equal(events.filter(value=>value==='fence').length,1);
    assert.equal(buffers[0].destroyed,false);
    fence.resolve();const result=await work;
    assert.equal(presents.length,1);assert.equal(buffers[0].destroyed,true);
    assert.deepEqual(result.presentCommands,[{epoch:'1',sequence:'1',command_id:'3',chain:'1'}]);
    assert.equal(events.filter(value=>value==='fence').length,1);
  } finally {fence.resolve();await work;}
});

test('readback waits for mapped bytes and preserves its receipt without a packet fence',async t=>{
  const {host,fence,mapping,events}=await graphicsFixture(t);
  host.configureSubmissionAck(true);
  let settled=false;
  const work=host.consume(packet([[1,definition],[9,clear],[14,readback]])).then(value=>{settled=true;return value;});
  try {
    await turn();assert.equal(events.includes('map'),true);assert.equal(settled,false);
    assert.equal(events.includes('fence'),false);
    mapping.resolve();await turn();
    assert.equal(settled,true,'mapped readback must not wait for an additional queue-wide fence');
    const result=await work,receipt=new DataView(result.readbacks[0].receipt.buffer);
    assert.deepEqual(Array.from(result.readbacks[0].bytes),[0,0,0,0]);
    assert.equal(receipt.getBigUint64(16,true),1n);assert.equal(receipt.getBigUint64(24,true),1n);
    assert.equal(receipt.getBigUint64(48,true),2n);assert.equal(events.includes('fence'),false);
  } finally {mapping.resolve();fence.resolve();await work;}
});

test('packet cleanup keeps cached stream storage alive while releasing patch temporaries',async t=>{
  const {host,buffers}=await graphicsFixture(t);
  await host.consume(packet([[1,definition]]));
  const renderer=host.sessions.get(1n),streams=Array(16).fill(null),words=new Uint32Array(256);
  streams[0]={key:'1:0:1',version:1n,stride:4};words[6]=1;words[7]=1;renderer.patchCapture={getBindGroupLayout(){return {};}};
  const packed=renderer.resources.packStreams({streams});
  const patch=await renderer.capturePatch({words,streams});
  assert.equal(renderer.temporary.includes(packed.buffer),false);
  const temporary=renderer.temporary.slice();assert.equal(temporary.length,1);
  const another=definition.slice();another[2]=1;
  const next=packet([[1,another]]),view=new DataView(next);
  for(const at of [24,32,40,72])view.setBigUint64(at,2n,true);
  await host.consume(next);
  assert.equal(temporary.every(buffer=>buffer.destroyed),true);
  assert.equal(packed.buffer.destroyed,false);
  assert.equal(renderer.resources.packStreams({streams}).buffer,packed.buffer);
  patch.controls.destroy();host.close();assert.equal(packed.buffer.destroyed,true);
  assert.equal(buffers.filter(buffer=>buffer===packed.buffer).length,1);
});

function stopFixture(drainError) {
  const draining=deferred(),events=[];
  const session=Object.assign(Object.create(Session.prototype),{
    generation:0,worker:null,capture:{capture:{markIncomplete(){events.push('incomplete');}}},graphicsQueue:Promise.resolve(),failed:false,
    input:{disable(){},close(){}},dialogs:{close(){}},audio:{async close(){events.push('audio-close');}},
    graphics:{async drain(){events.push('drain');await draining.promise;if(drainError)throw drainError;},close(){events.push('graphics-close');}},
    windows:{reset(){}},gdi:{clear(){}},async finishCapture(outcome,error){events.push(['capture',outcome,error]);},
    status(state){events.push(['status',state]);},
  });
  session.worker={postMessage(message){assert.equal(message.type,'stop');queueMicrotask(()=>session.stopped());},terminate(){events.push('worker-terminate');}};
  return {session,draining,events};
}

test('graceful stop drains submitted GPU work before closing or reporting stopped',async()=>{
  const {session,draining,events}=stopFixture();
  let settled=false;const stopping=session.stop().then(()=>{settled=true;});
  try {
    await turn();assert.equal(events.includes('drain'),true);assert.equal(settled,false);
    assert.equal(events.includes('graphics-close'),false);
    assert.equal(events.some(value=>Array.isArray(value)&&value[0]==='status'&&value[1]==='stopped'),false);
    draining.resolve();await stopping;
    assert.equal(events.filter(value=>value==='graphics-close').length,1);
    assert.deepEqual(events.at(-1),['status','stopped']);
  } finally {draining.resolve();await stopping;}
});

test('GPU drain rejection fails graceful stop while still releasing resources',async()=>{
  const failure=new Error('GPU drain failed'),{session,draining,events}=stopFixture(failure);
  const stopping=session.stop();void stopping.catch(()=>{});draining.resolve();
  await assert.rejects(stopping,error=>error===failure);
  assert.equal(events.includes('worker-terminate'),true);assert.equal(events.includes('audio-close'),true);
  assert.equal(events.filter(value=>value==='graphics-close').length,1);
  assert.equal(events.some(value=>Array.isArray(value)&&value[0]==='status'&&value[1]==='stopped'),false);
  assert.equal(events.some(value=>Array.isArray(value)&&value[0]==='capture'&&value[1]==='failed'&&value[2]===failure),true);
});

test('forced dispose closes graphics without requiring graceful GPU drain',async()=>{
  const {session,events}=stopFixture();
  await session.dispose();
  assert.equal(events.includes('drain'),false);
  assert.equal(events.filter(value=>value==='graphics-close').length,1);
  assert.deepEqual(events.at(-1),['capture','cancelled',undefined]);
});
