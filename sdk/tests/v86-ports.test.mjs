import assert from 'node:assert/strict';
import test from 'node:test';
import {createPortBridge} from '../runtime/v86/ports.js';
import {KEY} from '../runtime/engine.js';
import {SEGMENTS} from '../runtime/v86/layout.js';

function fixture({id=0x40228,status=0,badPacket=false,grow=false}={}) {
  const memory=new WebAssembly.Memory({initial:1}),base=0x4000,calls=[];
  const cache={memory,engine:{memory,base,api:{}},cpu:{wasm_memory:memory},inlineFailure:null};
  const selectors=[0x23,0x1b,0x23,0x23,0x3b,0];
  let response=status,growth=grow;
  const data=()=>new DataView(memory.buffer);
  for(let i=0;i<6;i++) {
    data().setUint16(668+i*2,selectors[i],true);
    data().setUint16(base+SEGMENTS+16+i*2,selectors[i],true);
  }
  new Uint8Array(memory.buffer,832,128).fill(9);
  new Uint8Array(memory.buffer,1152,128).fill(7);
  data().setUint32(120,0x246,true);data().setUint32(664,40001,true);
  function prepare() {
    new Uint8Array(memory.buffer)[612]=3;
    data().setUint32(64,0xabcdef01,true);data().setUint32(80,0x3000,true);
    data().setUint32(556,0x2002,true);data().setUint32(560,0x2000,true);
    data().setUint32(620,0x12345678,true);
  }
  cache.engine.api.component_cpu_try_direct_gate_mask=(...args)=>{
    calls.push(args);assert.deepEqual(args.slice(0,2),KEY);
    assert.deepEqual(args.slice(2,5).map(value=>value>>>0),[0x2000,0x3000,0xabcdef01]);
    if(growth){memory.grow(1);growth=false;}
    if(response!==0)return response;
    const out=new DataView(memory.buffer,base+140,32);
    for(const [at,value] of [[0,badPacket?0:0x46493352],[4,0x10001],[8,32],[12,0],
      [16,id],[20,0xf1234567],[24,0x4000],[28,0x3010]])out.setUint32(at,value,true);
    return 0;
  };
  prepare();
  return {cache,memory,base,calls,prepare,data,setStatus:value=>{response=value;}};
}

test('port bridge late binding preserves fallback and checks JS at most eight calls apart',()=>{
  const f=fixture(),fallback=[];
  const bridge=createPortBridge((...args)=>fallback.push(args));
  bridge.write8(0xe1,0x12);assert.deepEqual(fallback,[[0xe1,0x12]]);
  assert.equal(bridge.bind({...f.cache,engine:{...f.cache.engine,api:{}}}),false);
  assert.equal(bridge.bind(f.cache),true);assert.equal(bridge.arm(0x1fff),true);
  let before;
  for(let i=0;i<18;i++) {
    f.prepare();if(i===17)before=new Uint8Array(f.memory.buffer,0,8192).slice();
    bridge.write8(0xe1,0x12);
  }
  assert.equal(fallback.length,4);assert.equal(f.calls.length,15);
  assert.ok(f.calls.every(args=>args[5]===0x1fff));
  const expected=new DataView(before.buffer);
  for(const [at,value] of [[64,0xf1234567],[80,0x3010],[556,0x4000],[560,0x4000],[620,0xffffffff]])
    expected.setUint32(at,value,true);
  assert.deepEqual(new Uint8Array(f.memory.buffer,0,8192),before,'FP, flags, segments and instruction credits stay exact');
  assert.deepEqual(bridge.harvest(),{completed:15,declined:0,bindings:0,stateInputs:15,checkpoints:3,declinedPc:null});
  bridge.arm(0x1fff);f.prepare();bridge.write8(0xe1,0);f.prepare();bridge.write8(0xe1,0);
  assert.equal(bridge.disarm(),false);assert.equal(bridge.harvest().completed,1,'disarm must retain unharvested calls');
  bridge.close();f.prepare();bridge.write8(0xe1,0);assert.equal(f.calls.length,16);
});

test('disabled, CPL, selector, instruction-shape and other-port cases stay on the original handler',()=>{
  const f=fixture(),fallback=[];const bridge=createPortBridge((...args)=>fallback.push(args));
  bridge.bind(f.cache);bridge.write8(0xe1,1);assert.equal(f.calls.length,0);
  bridge.arm(0x1fff);f.prepare();bridge.write8(0xe1,1);
  f.prepare();new Uint8Array(f.memory.buffer)[612]=0;bridge.write8(0xe1,1);
  for(let i=0;i<6;i++) {
    f.prepare();const old=f.data().getUint16(668+i*2,true);
    f.data().setUint16(668+i*2,old^8,true);bridge.write8(0xe1,1);
    f.data().setUint16(668+i*2,old,true);
  }
  f.prepare();f.data().setUint32(556,0x2001,true);bridge.write8(0xe1,1);
  f.prepare();bridge.write8(0xe0,1);assert.equal(f.calls.length,0);
  f.prepare();bridge.write8(0xe1,1);assert.equal(f.calls.length,1,'a skipped profile must not poison the owner');
  assert.equal(fallback.length,11);bridge.close();
});

test('unexpected native status and malformed success poison the owner without fallback or replay',()=>{
  for(const options of [{status:12},{status:7},{badPacket:true},{id:0x40247}]) {
    const f=fixture(options),fallback=[];const bridge=createPortBridge((...args)=>fallback.push(args));
    bridge.bind(f.cache);bridge.arm(0x1fff);f.prepare();bridge.write8(0xe1,0);
    f.prepare();assert.throws(()=>bridge.write8(0xe1,0));
    const failure=f.cache.inlineFailure;assert.ok(failure instanceof Error);
    assert.equal(f.calls.length,1);assert.equal(fallback.length,1);
    f.prepare();assert.throws(()=>bridge.write8(0xe1,0),error=>error===failure);
    assert.throws(()=>bridge.arm(0x1fff),error=>error===failure);
    assert.equal(f.calls.length,1);assert.equal(fallback.length,1);bridge.close();
  }
});

test('native memory growth remains valid and neutral decline is reported once to its JS fallback',()=>{
  const f=fixture({grow:true}),fallback=[];let bridge;
  bridge=createPortBridge((...args)=>fallback.push({args,...bridge.harvest()}));
  bridge.bind(f.cache);bridge.arm(0x1fff);f.prepare();bridge.write8(0xe1,0);
  const old=f.memory.buffer;f.prepare();bridge.write8(0xe1,0);
  assert.notEqual(f.memory.buffer,old);assert.equal(f.data().getUint32(64,true),0xf1234567);
  assert.equal(f.data().getUint32(556,true),0x4000);
  assert.equal(bridge.harvest().completed,1);
  f.setStatus(256);f.prepare();const packet=new Uint8Array(f.memory.buffer,f.base+140,32).slice();
  bridge.write8(0xe1,0);
  assert.equal(f.calls.length,2);assert.equal(fallback.length,2);
  assert.equal(fallback[1].declined,1);assert.equal(fallback[1].declinedPc,0x2000);
  assert.equal(bridge.harvest().declinedPc,null,'the fallback marker is one-shot');
  assert.deepEqual(new Uint8Array(f.memory.buffer,f.base+140,32),packet);
  assert.equal(f.data().getUint32(556,true),0x2002);bridge.close();
});
