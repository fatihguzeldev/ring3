import assert from 'node:assert/strict';
import test from 'node:test';
import {PageTables} from '../runtime/v86/memory.js';
import {CodeValidity} from '../runtime/v86/code-validity.js';

const PAGE=4096, CODE=0x100, DATA=0x200;

function world() {
  const memory=new WebAssembly.Memory({initial:512}),data=new DataView(memory.buffer);
  const info={backing:16*1024*1024,length:16*PAGE,maps:8*1024*1024,permissions:12*1024*1024,
    guestPages:1<<20,capacity:16,mapped:2,mappingVersions:13*1024*1024,
    physicalVersions:21*1024*1024,privateSlots:0,executableViews:27*1024*1024,
    identity:1n,transaction:1n,code:1n,mapping:1n};
  const rows=new Map();
  function map(page,slot,permission,version=info.transaction) {
    data.setUint32(info.maps+page*4,slot===null?0:slot+1,true);
    data.setUint8(info.permissions+page,permission);
    data.setBigUint64(info.mappingVersions+page*8,version,true);
    if(slot===null)rows.delete(page);else rows.set(page,{page,slot,permission});
    info.mapped=rows.size;
  }
  function content(slot,version) {data.setBigUint64(info.physicalVersions+slot*16+8,version,true);}
  function changeMapping(page,slot,permission) {
    info.transaction++;info.code=info.transaction;info.mapping=info.transaction;
    map(page,slot,permission);
  }
  function write(slot) {info.transaction++;info.code=info.transaction;content(slot,info.transaction);}
  map(CODE,0,5);map(DATA,1,3);content(0,1n);content(1,1n);
  data.setUint32(info.executableViews,1,true);
  const compiled=new Map(),cpu={mem8:{byteOffset:0,length:160*1024*1024},clears:0,
    jit_clear_cache(){this.clears++;compiled.clear();},full_clear_tlb(){}};
  const engine={memory,base:0,
    ok(operation) {
      if(operation!=='component_cpu_view')return;
      const at=140;new Uint8Array(memory.buffer,at,128).fill(0);
      data.setUint32(at,0x56433352,true);data.setUint16(at+4,1,true);data.setUint16(at+6,1,true);
      data.setUint32(at+8,128,true);
      for(const [offset,name] of [[16,'backing'],[20,'length'],[24,'maps'],[28,'permissions'],
        [32,'guestPages'],[36,'capacity'],[40,'mapped'],[44,'mappingVersions'],
        [48,'physicalVersions'],[52,'privateSlots'],[56,'executableViews']])
        data.setUint32(at+offset,info[name],true);
      for(const [offset,name] of [[64,'identity'],[72,'transaction'],[80,'code'],[88,'mapping']])
        data.setBigUint64(at+offset,info[name],true);
    },
    resultView(length){return new DataView(memory.buffer,140,length);},
  };
  return {memory,data,info,rows,cpu,engine,compiled,map,content,changeMapping,write};
}

test('data-only mapping retains an already generated v86 code module',()=>{
  const f=world(),tables=new PageTables(f.engine,f.cpu,{selectiveCode:true,incrementalSync:false});
  const generated={id:'retained-code'};f.compiled.set(CODE,generated);
  f.changeMapping(DATA+1,2,3);f.content(2,f.info.transaction);
  tables.refresh();
  assert.equal(f.compiled.get(CODE),generated,'unrelated data mapping must retain generated code');
});

test('data-only protect retains an already generated v86 code module',()=>{
  const f=world(),tables=new PageTables(f.engine,f.cpu,{selectiveCode:true,incrementalSync:false});
  const generated={id:'retained-code'};f.compiled.set(CODE,generated);
  f.changeMapping(DATA,1,1);
  tables.refresh();
  assert.equal(f.compiled.get(CODE),generated,'data permission change must retain generated code');
});

test('code validity seeds without invalidation and unchanged epochs do not scan rows',()=>{
  const f=world(),watch=new CodeValidity(f.memory);
  assert.equal(watch.refresh(f.info,f.rows.values()).invalidate,false);
  const forbidden={[Symbol.iterator](){throw new Error('unchanged epochs scanned mappings');}};
  assert.equal(watch.refresh(f.info,forbidden).invalidate,false);
});

test('data mappings and permissions do not invalidate executable identity or content',()=>{
  const f=world(),watch=new CodeValidity(f.memory);watch.refresh(f.info,f.rows.values());
  f.changeMapping(DATA+1,2,3);f.content(2,f.info.transaction);
  assert.equal(watch.refresh(f.info,f.rows.values()).invalidate,false);
  f.changeMapping(DATA,1,1);
  assert.equal(watch.refresh(f.info,f.rows.values()).invalidate,false);
});

test('executable permission changes and same-slot VA reincarnation invalidate code',()=>{
  const f=world(),watch=new CodeValidity(f.memory);watch.refresh(f.info,f.rows.values());
  f.changeMapping(CODE,0,7);
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'executable-mapping');
  f.changeMapping(CODE,0,7);
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'executable-mapping',
    'a replacement at the same VA and physical slot still has a new mapping incarnation');
});

test('new and removed executable aliases invalidate the complete code set',()=>{
  const f=world(),watch=new CodeValidity(f.memory);watch.refresh(f.info,f.rows.values());
  f.changeMapping(CODE+1,0,5);
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'executable-mapping');
  f.changeMapping(CODE+1,null,0);
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'executable-mapping');
});

test('host writes through a non-executable alias invalidate shared executable content',()=>{
  const f=world(),watch=new CodeValidity(f.memory);watch.refresh(f.info,f.rows.values());
  f.changeMapping(DATA,0,3);
  assert.equal(watch.refresh(f.info,f.rows.values()).invalidate,false);
  f.write(0);
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'host-code-write');
});

test('physical mapping metadata for a data alias is not a code VA incarnation',()=>{
  const f=world(),watch=new CodeValidity(f.memory);f.map(DATA,0,3);
  watch.refresh(f.info,f.rows.values());
  f.changeMapping(DATA,0,1);
  f.data.setBigUint64(f.info.physicalVersions,f.info.transaction,true);
  assert.equal(watch.refresh(f.info,f.rows.values()).invalidate,false,
    'the per-VA mapping version, not shared slot mapping metadata, owns code identity');
});

test('data COW detach preserves code while executable COW detach invalidates without a code epoch change',()=>{
  const f=world(),watch=new CodeValidity(f.memory);f.map(DATA,0,3);
  watch.refresh(f.info,f.rows.values());
  f.changeMapping(DATA,2,3);f.content(2,f.info.transaction);
  assert.equal(watch.refresh(f.info,f.rows.values()).invalidate,false);
  const codeEpoch=f.info.code;
  f.info.transaction++;f.info.mapping=f.info.transaction;
  f.map(CODE,3,5);f.content(3,f.info.transaction);
  assert.equal(f.info.code,codeEpoch);
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'executable-mapping');
});

test('known guest SMC versions are acknowledged without hiding later host writes',()=>{
  const f=world(),watch=new CodeValidity(f.memory);watch.refresh(f.info,f.rows.values());
  f.write(0);watch.acknowledgeGuestWrites(f.info,[0,1]);
  assert.equal(watch.refresh(f.info,f.rows.values()).invalidate,false);
  f.write(0);
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'host-code-write');
});

test('guest dirty acknowledgement cannot skip an unreviewed host write to another executable slot',()=>{
  const f=world(),watch=new CodeValidity(f.memory);
  f.map(CODE+1,2,5);f.content(2,1n);watch.refresh(f.info,f.rows.values());
  f.write(2);f.write(0);
  watch.acknowledgeGuestWrites(f.info,[0]);
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'host-code-write');
});

test('invalid guest acknowledgement is rejected before changing any content baseline',()=>{
  const f=world(),watch=new CodeValidity(f.memory);watch.refresh(f.info,f.rows.values());
  f.write(0);
  assert.throws(()=>watch.acknowledgeGuestWrites(f.info,[0,f.info.capacity]));
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'host-code-write');
});

test('code validity refresh rebuilds views after memory growth and invalidates changed address spaces',()=>{
  const f=world(),watch=new CodeValidity(f.memory);watch.refresh(f.info,f.rows.values());
  f.memory.grow(1);
  const current=new DataView(f.memory.buffer);
  current.setBigUint64(f.info.physicalVersions+8,2n,true);f.info.code=2n;
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'host-code-write');
  f.info.identity++;
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'address-space');
});
