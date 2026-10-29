import assert from 'node:assert/strict';
import test from 'node:test';
import {Resources} from '../graphics/resources.js';
import {Reader} from '../graphics/packet.js';

const usage={STORAGE:1,COPY_DST:2,INDEX:4};
function words(values,tail=new Uint8Array()) {
  const bytes=new Uint8Array(values.length*4+tail.length),view=new DataView(bytes.buffer);
  values.forEach((value,index)=>view.setUint32(index*4,value,true));bytes.set(tail,values.length*4);
  return new Reader(bytes);
}
function fixture(t,{limit=128<<20,copy=true}={}) {
  const previous=Object.getOwnPropertyDescriptor(globalThis,'GPUBufferUsage');
  Object.defineProperty(globalThis,'GPUBufferUsage',{configurable:true,value:usage});
  t.after(()=>{if(previous)Object.defineProperty(globalThis,'GPUBufferUsage',previous);else delete globalThis.GPUBufferUsage;});
  const buffers=[],events=[],pending=new Set();let failWrite=false;
  const device={limits:{maxStorageBufferBindingSize:limit},queue:{writeBuffer(buffer,offset,bytes){
    events.push(['write',buffer.id,bytes.byteLength]);
    if(failWrite){failWrite=false;throw new Error('write failed');}
    if(copy)buffer.bytes=new Uint8Array(bytes.buffer,bytes.byteOffset,bytes.byteLength).slice();
  }},createBuffer({size}){
    const buffer={id:buffers.length,size,destroyed:false,destroyCalls:0,destroy(){assert.equal(pending.has(this),false,'submit queued references before destroying a packed buffer');this.destroyCalls++;this.destroyed=true;events.push(['destroy',this.id]);}};
    buffers.push(buffer);return buffer;
  }};
  const resources=new Resources(device,()=>{events.push(['flush']);pending.clear();});
  t.after(()=>resources.close());
  const add=(slot,data,generation=1,type=6,format=0)=>{
    const r=words([1,0,slot,generation,type,format,0,0,data.length,1,1,1,0,0,0,0,1,data.length,1,1,data.length,data.length,data.length,0,0]);
    resources.create(r);r.end();
    const ref={epoch:1n,slot,generation,key:`1:${slot}:${generation}`,version:1n,stride:4};
    upload(ref,data);return ref;
  };
  const upload=(ref,data,offset=0,version=ref.version)=>{
    const r=words([1,0,ref.slot,ref.generation,Number(version&0xffffffffn),Number(version>>32n),0,offset,data.length,data.length,data.length,1,1,data.length],data);
    resources.upload(r);r.end();
  };
  const binding=(...entries)=>{const streams=Array(16).fill(null);for(const [slot,ref] of entries)streams[slot]=ref;return {streams};};
  return {resources,buffers,events,pending,add,upload,binding,failNextWrite(){failWrite=true;}};
}

test('unchanged streams reuse owned GPU storage while headers preserve slot, stride and complete bytes',t=>{
  const {resources,buffers,add,binding}=fixture(t);
  const a=add(1,Uint8Array.of(10,11,12,13,14,15,16,17)),b=add(2,Uint8Array.of(90,91,92));
  const bindings=binding([0,a],[3,{...b,stride:2}]);
  const first=resources.packStreams(bindings),again=resources.packStreams({...bindings,first:4,count:1});
  assert.equal(first.temporary,false);assert.equal(again.buffer,first.buffer);assert.equal(buffers.length,1);
  const view=new DataView(first.buffer.bytes.buffer);
  assert.equal(view.getUint32(0,true),128);assert.equal(view.getUint32(4,true),4);
  assert.equal(view.getUint32(24,true),136);assert.equal(view.getUint32(28,true),2);
  assert.deepEqual([...first.buffer.bytes.subarray(128)],[10,11,12,13,14,15,16,17,90,91,92,0]);
  const stride=resources.packStreams(binding([0,{...a,stride:8}],[3,{...b,stride:2}]));
  const slot=resources.packStreams(binding([1,a],[3,{...b,stride:2}]));
  assert.notEqual(stride.buffer,first.buffer);assert.notEqual(slot.buffer,first.buffer);
  assert.equal(new DataView(stride.buffer.bytes.buffer).getUint32(4,true),8);
  assert.equal(new DataView(slot.buffer.bytes.buffer).getUint32(8,true),128);
});

test('same-version offset uploads invalidate dependent packs before mutation but preserve unrelated packs',t=>{
  const {resources,events,pending,add,upload,binding}=fixture(t);
  const a=add(1,Uint8Array.of(0,1,2,3,4,5,6,7)),b=add(2,Uint8Array.of(40,41,42,43));
  const old=resources.packStreams(binding([0,a])),unrelated=resources.packStreams(binding([0,b]));
  pending.add(old.buffer);events.length=0;
  upload(a,Uint8Array.of(99,98),3,1n);
  assert.deepEqual(events.slice(0,2),[['flush'],['destroy',old.buffer.id]]);
  assert.equal(old.buffer.destroyed,true);assert.equal(unrelated.buffer.destroyed,false);
  const current=resources.packStreams(binding([0,a]));
  assert.notEqual(current.buffer,old.buffer);
  assert.deepEqual([...current.buffer.bytes.subarray(128)],[0,1,2,99,98,5,6,7]);
  assert.equal(resources.packStreams(binding([0,b])).buffer,unrelated.buffer);
  upload(a,Uint8Array.of(77),0,2n);
  assert.equal(current.buffer.destroyed,true);
  assert.equal(resources.packStreams(binding([0,{...a,version:2n}])).buffer.bytes[128],77);
});

test('retained index expansion reuses storage and revokes it before same-version and partial upload mutations',t=>{
  const {resources,events,pending,add,upload}=fixture(t),a=add(1,Uint8Array.of(0,0,1,0,255,255,3,0),1,7,101);
  const first=resources.retainedIndexBuffer(a,101),again=resources.retainedIndexBuffer(a,101);
  assert.equal(again.buffer,first.buffer);assert.equal(resources.retainedIndexBuilds,1);assert.equal(again.cacheHit,true);
  assert.deepEqual([...new Uint32Array(first.buffer.bytes.buffer)],[0,1,65535,3]);
  pending.add(first.buffer);events.length=0;upload(a,Uint8Array.of(9,0),2,1n);
  assert.deepEqual(events.slice(0,2),[['flush'],['destroy',first.buffer.id]]);
  const changed=resources.retainedIndexBuffer(a,101);assert.deepEqual([...new Uint32Array(changed.buffer.bytes.buffer)],[0,9,65535,3]);
  pending.add(changed.buffer);
  const invalid=words([1,0,1,1,1,0,0,0,8,16,2,2,1,4],Uint8Array.of(7,0,8,0));
  assert.throws(()=>resources.upload(invalid),/upload bounds/);assert.equal(changed.buffer.destroyed,true);
  const partial=resources.retainedIndexBuffer(a,101);assert.deepEqual([...new Uint32Array(partial.buffer.bytes.buffer)],[7,9,65535,3]);
  resources.destroy(a);assert.equal(partial.buffer.destroyed,true);
  const replacement=add(1,Uint8Array.of(4,0,5,0),2,7,101),fresh=resources.retainedIndexBuffer(replacement,101);
  assert.notEqual(fresh.buffer,partial.buffer);assert.deepEqual([...new Uint32Array(fresh.buffer.bytes.buffer)],[4,5]);
});

test('destroy and resource-generation reuse cannot resurrect a stale packed buffer',t=>{
  const {resources,pending,add,binding}=fixture(t);
  const a=add(1,Uint8Array.of(1,2,3,4)),old=resources.packStreams(binding([0,a]));
  pending.add(old.buffer);resources.destroy(a);assert.equal(old.buffer.destroyed,true);
  assert.throws(()=>resources.packStreams(binding([0,a])),/stale resource/);
  const replacement=add(1,Uint8Array.of(5,6,7,8),2),fresh=resources.packStreams(binding([0,replacement]));
  assert.notEqual(fresh.buffer,old.buffer);assert.deepEqual([...fresh.buffer.bytes.subarray(128)],[5,6,7,8]);
});

test('UP override packs remain temporary and never reuse equal lengths with different content',t=>{
  const {resources,buffers,add,binding}=fixture(t);
  const a=add(1,Uint8Array.of(1,2,3,4)),bindings=binding([0,a]);
  const cached=resources.packStreams(bindings);
  const one=resources.packStreams(bindings,{bytes:Uint8Array.of(7,8,9),stride:3});
  const two=resources.packStreams(bindings,{bytes:Uint8Array.of(4,5,6),stride:3});
  assert.equal(one.temporary,true);assert.equal(two.temporary,true);assert.notEqual(one.buffer,two.buffer);
  assert.deepEqual([...one.buffer.bytes.subarray(128)],[7,8,9,0]);assert.deepEqual([...two.buffer.bytes.subarray(128)],[4,5,6,0]);
  one.buffer.destroy();two.buffer.destroy();
  assert.equal(resources.packStreams(bindings).buffer,cached.buffer);assert.equal(buffers.length,3);
});

test('entry eviction submits pending references and releases only the least recently used pack',t=>{
  const {resources,buffers,pending,add,binding}=fixture(t);
  const first=add(1,Uint8Array.of(1)),second=add(2,Uint8Array.of(2));
  const a=resources.packStreams(binding([0,first])),b=resources.packStreams(binding([0,second]));
  pending.add(b.buffer);resources.packStreams(binding([0,first]));
  for(let slot=3;slot<=1025;slot++)resources.packStreams(binding([0,add(slot,Uint8Array.of(slot&255))]));
  assert.equal(b.buffer.destroyed,true);assert.equal(a.buffer.destroyed,false);
  assert.equal(buffers.filter(buffer=>!buffer.destroyed).length,1024);
  assert.equal(resources.packStreams(binding([0,first])).buffer,a.buffer);
});

test('failed uploads of a fresh packed GPU buffer leave no cache entry or leaked buffer',t=>{
  const {resources,buffers,add,binding,failNextWrite}=fixture(t);
  const a=add(1,Uint8Array.of(1,2,3,4));failNextWrite();
  assert.throws(()=>resources.packStreams(binding([0,a])),/write failed/);
  assert.equal(buffers[0].destroyed,true);
  const good=resources.packStreams(binding([0,a]));assert.equal(good.buffer,buffers[1]);
  assert.equal(resources.packStreams(binding([0,a])).buffer,good.buffer);
});

test('a late invalid upload cannot leave a cached copy of the partially changed CPU resource',t=>{
  const {resources,pending,add,binding}=fixture(t);
  const a=add(1,Uint8Array.of(0,1,2,3,4,5,6,7)),old=resources.packStreams(binding([0,a]));
  pending.add(old.buffer);
  const r=words([1,0,1,1,1,0,0,0,8,16,4,2,1,8],Uint8Array.of(9,8,7,6,5,4,3,2));
  assert.throws(()=>resources.upload(r),/upload bounds/);assert.equal(old.buffer.destroyed,true);
  const fresh=resources.packStreams(binding([0,a]));
  assert.notEqual(fresh.buffer,old.buffer);assert.deepEqual([...fresh.buffer.bytes.subarray(128)],[9,8,7,6,4,5,6,7]);
});

test('the byte budget evicts before the entry limit while retaining the new complete pack',t=>{
  const {resources,buffers,pending,add,binding}=fixture(t,{copy:false});
  const a=resources.packStreams(binding([0,add(1,new Uint8Array(16<<20))]));pending.add(a.buffer);
  const b=resources.packStreams(binding([0,add(2,new Uint8Array(16<<20))]));
  assert.equal(a.buffer.destroyed,true);assert.equal(b.buffer.destroyed,false);
  assert.equal(buffers.filter(buffer=>!buffer.destroyed).length,1);
});

test('oversized packs bypass the byte-bounded cache and remain caller-owned',t=>{
  const {resources,buffers,add,binding}=fixture(t,{copy:false});
  const small=add(1,Uint8Array.of(1)),cached=resources.packStreams(binding([0,small]));
  const large=add(2,new Uint8Array(32<<20)),big=resources.packStreams(binding([0,large]));
  assert.equal(big.temporary,true);assert.equal(big.buffer.size,(32<<20)+128);
  assert.equal(cached.buffer.destroyed,false);big.buffer.destroy();
  assert.equal(resources.packStreams(binding([0,small])).buffer,cached.buffer);assert.equal(buffers.length,2);
});

test('closing destroys every cached packed buffer after pending work is submitted and is idempotent',t=>{
  const {resources,buffers,pending,add,binding}=fixture(t);
  for(let slot=1;slot<=3;slot++)pending.add(resources.packStreams(binding([0,add(slot,Uint8Array.of(slot))])).buffer);
  resources.close();assert.equal(buffers.every(buffer=>buffer.destroyed),true);
  resources.close();assert.equal(buffers.every(buffer=>buffer.destroyCalls===1),true);
});
