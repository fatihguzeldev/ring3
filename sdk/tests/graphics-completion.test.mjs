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
    GPUBufferUsage:{STORAGE:1,COPY_DST:2,COPY_SRC:4,MAP_READ:8},GPUMapMode:{READ:1}};
  const saved=new Map(Object.keys(globals).map(name=>[name,Object.getOwnPropertyDescriptor(globalThis,name)]));
  for(const [name,value] of Object.entries(globals))Object.defineProperty(globalThis,name,{configurable:true,writable:true,value});
  t.after(()=>{for(const [name,descriptor] of saved)if(descriptor)Object.defineProperty(globalThis,name,descriptor);else delete globalThis[name];});
  const queue={
    submit(){events.push('submit');},writeTexture(){events.push('write-texture');},writeBuffer(){events.push('write-buffer');},
    onSubmittedWorkDone(){events.push('fence');return fence.promise;},
  };
  const device={queue,features:[],limits:{maxTextureDimension2D:4096,maxStorageBufferBindingSize:1024*1024},lost:new Promise(()=>{}),
    addEventListener(){},pushErrorScope(){},async popErrorScope(){return null;},destroy(){events.push('device-destroy');},
    createTexture(){return {createView(){return {};},destroy(){events.push('texture-destroy');}};},
    createBuffer({size}){const buffer={destroyed:false,async mapAsync(){events.push('map');await mapping.promise;},getMappedRange(){return new ArrayBuffer(size);},unmap(){events.push('unmap');},destroy(){this.destroyed=true;events.push('buffer-destroy');}};buffers.push(buffer);return buffer;},
    createCommandEncoder(){return {beginRenderPass(){return {setPipeline(){},setBindGroup(){},setViewport(){},draw(){},end(){events.push('pass-end');}};},beginComputePass(){return {setPipeline(){},setBindGroup(){},dispatchWorkgroups(){},end(){events.push('compute-end');}};},copyTextureToBuffer(){events.push('copy-to-buffer');},finish(){events.push('finish');return {};}};},
    createShaderModule(){return {async getCompilationInfo(){return {messages:[]};}};},
    async createRenderPipelineAsync(){return {getBindGroupLayout(){return {};}};},createBindGroup(){return {};},createSampler(){return {};},
  };
  const adapter={features:[],limits:device.limits,async requestDevice(){return device;}};
  const context={configure(){},unconfigure(){},getCurrentTexture(){return {createView(){return {};}};}};
  const host=await createGraphicsHost({width:1,height:1,getContext(){return context;}},{onDiagnostic:value=>{if(value.type==='present')presents.push(value);if(value.type==='error')errors.push(value);}});
  t.after(()=>{fence.resolve();mapping.resolve();host.close();});
  return {host,fence,mapping,events,presents,buffers,errors};
}

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
