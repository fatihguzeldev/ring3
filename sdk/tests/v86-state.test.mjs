import assert from 'node:assert/strict';
import test from 'node:test';
import * as state from '../runtime/v86/state.js';
import {X87,SIMD,SEGMENTS,YMM} from '../runtime/v86/layout.js';

function fixture() {
  const memory=new WebAssembly.Memory({initial:2}),engine={memory,base:65536};
  let tags=0,updates=0,descriptors=0;
  const data=()=>new DataView(memory.buffer),bytes=()=>new Uint8Array(memory.buffer);
  const cpu={wm:{exports:{
    fpu_load_tag_word(){
      tags++;let value=0;for(let i=0;i<8;i++)if(bytes()[816]&(1<<i))value|=3<<(i*2);return value;
    },
    fpu_load_status_word(){return data().getUint16(1040,true)|((bytes()[1032]&7)<<11);},
    get_eflags(){return data().getUint32(120,true);},
  }},update_state_flags(){updates++;}};
  const traps={setDescriptor(){descriptors++;}};
  const d=data(),b=engine.base;
  d.setUint32(b+48,0x401000,true);d.setUint32(b+52,0x202,true);
  for(let i=0;i<8;i++)d.setUint32(b+16+i*4,0x1000+i,true);
  d.setUint16(b+SEGMENTS+4,1,true);
  for(const [i,selector] of [0x23,0x1b,0x23,0x23,0x33,0].entries())d.setUint16(b+SEGMENTS+16+i*2,selector,true);
  d.setUint32(b+SEGMENTS+32+4*4,0x7000,true);
  d.setUint16(b+X87+16,0x37f,true);d.setUint16(b+X87+20,0xffff,true);
  d.setUint32(b+SIMD+16,0x1f80,true);d.setUint32(b+SIMD+32,0x12345678,true);
  state.importState(engine,cpu,traps);
  const remember=()=>state.rememberStateBanks?.(engine,cpu);
  const publish=trap=>{state.exportState(engine,cpu,trap);remember();};
  return {engine,cpu,traps,data,bytes,remember,publish,
    counts:()=>({tags,updates,descriptors})};
}

test('unchanged exported x87 and SIMD banks retain live state while GPR flags and trap segments still import',()=>{
  const f=fixture(),b=f.engine.base,d=f.data(),v=state.cpuViews(f.engine.memory);
  v.selectors[1]=8;v.selectors[2]=16;v.bytes[612]=0;v.regs[4]=0xff004ff0;
  f.publish({pc:0x401010,flags:0x246,esp:0x8800,cs:0x1b,ss:0x23});
  d.setUint32(b+16,0xabcdef01,true);d.setUint32(b+48,0x401020,true);d.setUint32(b+52,0x202,true);
  const before=f.counts();state.importState(f.engine,f.cpu,f.traps);const after=f.counts();
  assert.equal(after.tags,before.tags,'an unchanged live x87 bank must not be rebuilt or revalidated');
  assert.equal(after.updates,before.updates+1);assert.equal(after.descriptors,before.descriptors+6);
  assert.equal(v.regs[0],0xabcdef01);assert.equal(v.regs[4],0x8800);assert.equal(d.getUint32(556,true),0x401020);
  assert.equal(d.getUint32(120,true),0x202);assert.equal(v.selectors[1],0x1b);assert.equal(v.selectors[2],0x23);
  assert.equal(v.bytes[612],3);
});

test('canonical x87 or SIMD changes force a complete numeric-bank import',()=>{
  for(const offset of [X87+16,SIMD+16,SIMD+32]) {
    const f=fixture(),b=f.engine.base;f.publish();
    const before=f.counts(),value=offset===X87+16?0x27f:offset===SIMD+16?0x1f00:0x76543210;
    if(offset===X87+16)f.data().setUint16(b+offset,value,true);else f.data().setUint32(b+offset,value,true);
    state.importState(f.engine,f.cpu,f.traps);
    assert.equal(f.counts().tags,before.tags+1);
    const live=offset===X87+16?f.data().getUint16(1036,true):f.data().getUint32(offset===SIMD+16?824:832,true);
    assert.equal(live,value);
  }
});

test('a retained numeric-bank proof is consumed once and cannot survive later unexported CPU changes',()=>{
  const f=fixture();f.publish();state.importState(f.engine,f.cpu,f.traps);
  f.data().setUint16(1036,0x27f,true);
  const before=f.counts();state.importState(f.engine,f.cpu,f.traps);
  assert.equal(f.counts().tags,before.tags+1);assert.equal(f.data().getUint16(1036,true),0x37f);
});

test('memory growth or a different canonical owner cannot reuse a retained numeric bank',()=>{
  for(const change of ['grow','owner']) {
    const f=fixture();f.publish();
    const engine=change==='owner'?{...f.engine}:f.engine;
    if(change==='grow')f.engine.memory.grow(1);
    const before=f.counts();state.importState(engine,f.cpu,f.traps);
    assert.equal(f.counts().tags,before.tags+1);
  }
});

test('retained numeric banks do not bypass unsupported YMM SS-shadow or tag validation',()=>{
  for(const change of ['ymm','shadow','tag']) {
    const f=fixture(),b=f.engine.base;f.publish();
    if(change==='ymm')f.bytes()[b+YMM+16]=1;
    else if(change==='shadow')f.data().setUint32(b+12,1,true);
    else f.data().setUint16(b+X87+20,0xfffe,true);
    assert.throws(()=>state.importState(f.engine,f.cpu,f.traps),/does not support|cannot represent/);
  }
});
