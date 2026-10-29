import assert from 'node:assert/strict';
import test from 'node:test';
import {PageTables} from '../runtime/v86/memory.js';
import {CodeValidity} from '../runtime/v86/code-validity.js';
import {BACKING_BASE,DIRECTORY,TABLES,PRIVATE_VA} from '../runtime/v86/layout.js';
import {Engine} from '../runtime/engine.js';

const PAGE=4096, CODE=0x100, DATA=0x200;

function world({journal=false,nativeDirty=false,nativeDirtyEpoch=false,dirtyGateFlags=false,preciseExecutable=false,globalCode=false,dirtyVirtualPages=false,trustedExport=false}={}) {
  const memory=new WebAssembly.Memory({initial:512}),data=new DataView(memory.buffer);
  const capabilities=(nativeDirty?2:0)|(nativeDirtyEpoch?4:0)|(dirtyGateFlags?8:0)|(preciseExecutable?16:0)|(globalCode?32:0)|(dirtyVirtualPages?128:0);
  const info={backing:16*1024*1024,length:16*PAGE,maps:8*1024*1024,permissions:12*1024*1024,
    guestPages:1<<20,capacity:16,mapped:2,mappingVersions:13*1024*1024,
    physicalVersions:21*1024*1024,privateSlots:0,executableViews:27*1024*1024,
    identity:1n,transaction:1n,code:1n,mapping:1n,
    mappingChanges:28*1024*1024,mappingChangeCount:0,mappingChangeCapacity:1<<20,mappingChangeFlags:capabilities};
  if(preciseExecutable)info.executableEpoch=1n;
  const rows=new Map(),pending=new Set(),acks=[],ackAttempts=[],dirtyPublications=[],nativeCollections=[],requests=[],
    hooks={beforeAck:null,collectResult:null};
  let tracking=false;
  function enqueue(page) {
    pending.add(page);info.mappingChangeCount=pending.size;
    let index=0;for(const current of pending)data.setUint32(info.mappingChanges+index++*4,current,true);
  }
  function map(page,slot,permission,version=info.transaction) {
    data.setUint32(info.maps+page*4,slot===null?0:slot+1,true);
    data.setUint8(info.permissions+page,permission);
    data.setBigUint64(info.mappingVersions+page*8,version,true);
    if(slot===null)rows.delete(page);else rows.set(page,{page,slot,permission});
    info.mapped=rows.size;
    if(tracking)enqueue(page);
  }
  function content(slot,version) {data.setBigUint64(info.physicalVersions+slot*16+8,version,true);}
  function changeMapping(page,slot,permission) {
    const executable=(rows.get(page)?.permission&4)||(permission&4);
    info.transaction++;info.code=info.transaction;info.mapping=info.transaction;
    map(page,slot,permission);
    if(preciseExecutable && executable)info.executableEpoch=info.transaction;
  }
  function write(slot) {
    info.transaction++;info.code=info.transaction;content(slot,info.transaction);
    if(preciseExecutable && [...rows.values()].some(row=>row.slot===slot&&(row.permission&4)))
      info.executableEpoch=info.transaction;
  }
  map(CODE,0,5);map(DATA,1,3);content(0,1n);content(1,1n);
  data.setUint32(info.executableViews,1,true);
  const flushes=[],compiled=new Map(),cpu={mem8:{byteOffset:0,length:160*1024*1024},clears:0,tlbClears:0,nonGlobalClears:0,
    jit_clear_cache(){flushes.push('jit');this.clears++;compiled.clear();},full_clear_tlb(){flushes.push('full');this.tlbClears++;},
    clear_tlb(){flushes.push('non-global');this.nonGlobalClears++;}};
  cpu.wasm_memory=memory;cpu.cr=new Int32Array(memory.buffer,580,8);
  cpu.cpl=new Uint8Array(memory.buffer,612,1);cpu.cpl[0]=3;cpu.invalidatedPages=[];
  cpu.wm={exports:{instr16_0F01_7_mem(address){assert.equal(cpu.cpl[0],0);cpu.invalidatedPages.push(address>>>0);if(cpu.invalidateFailure)throw cpu.invalidateFailure;}}};
  const engine={memory,base:0,api:trustedExport?{component_cpu_collect_dirty_trusted(){}}:{},
    ok(operation,...args) {
      if(operation==='component_cpu_attach') {tracking=journal;info.mappingChangeFlags=(journal?1:0)|capabilities;return;}
      if(operation==='component_cpu_release') {tracking=false;info.mappingChangeFlags=capabilities;pending.clear();info.mappingChangeCount=0;return;}
      if(operation==='component_cpu_ack_mappings') {
        const epoch=BigInt(args[2]>>>0)|(BigInt(args[3]>>>0)<<32n),count=args[4];
        ackAttempts.push({epoch,count});hooks.beforeAck?.();
        if(epoch!==info.mapping || count!==pending.size)throw new Error('stale mapping journal acknowledgement');
        acks.push({epoch,count});pending.clear();info.mappingChangeCount=0;return;
      }
      if(operation==='component_cpu_dirty') {
        const slots=Array.from(new Uint32Array(memory.buffer,140,args[2]));
        dirtyPublications.push(slots);info.transaction++;
        for(const slot of slots) {
          content(slot,info.transaction);
          if([...rows.values()].some(row=>row.slot===slot&&(row.permission&4))) {
            info.code=info.transaction;if(preciseExecutable)info.executableEpoch=info.transaction;
          }
        }
        return;
      }
      if(operation==='component_cpu_collect_dirty' || operation==='component_cpu_collect_dirty_trusted') {
        const epoch=BigInt(args[2]>>>0)|(BigInt(args[3]>>>0)<<32n),directory=args[4];
        if(epoch!==info.mapping || pending.size || directory!==DIRECTORY)throw new Error('invalid native dirty source');
        const groups=new Map(),slots=new Set(),accessed=[],dirtyPtes=[];
        for(const row of rows.values()) {
          const group=row.page>>>10;
          if(!groups.has(group))groups.set(group,[]);
          groups.get(group).push(row);
        }
        let scannedPtes=0;
        for(const [group,members] of groups) {
          const at=DIRECTORY+group*4,value=data.getUint32(at,true);
          if(!(value&32))continue;
          accessed.push([at,value&~32]);
          for(const row of members) {
            scannedPtes++;
            const at=TABLES+row.page*4,value=data.getUint32(at,true);
            if(value&64){slots.add(row.slot);dirtyPtes.push([at,value&~64]);}
          }
        }
        const values=[...slots],slotsPtr=27*1024*1024+PAGE,beforeCode=info.code;
        if(values.length) {
          dirtyPublications.push(values);info.transaction++;
          for(const slot of values) {
            content(slot,info.transaction);
            if([...rows.values()].some(row=>row.slot===slot&&(row.permission&4))) {
              info.code=info.transaction;if(preciseExecutable)info.executableEpoch=info.transaction;
            }
          }
        }
        for(const [at,value] of [...accessed,...dirtyPtes])data.setUint32(at,value,true);
        new Uint32Array(memory.buffer,slotsPtr,values.length).set(values);
        const at=140,size=dirtyVirtualPages?96:nativeDirtyEpoch?88:80;new Uint8Array(memory.buffer,at,size).fill(0);
        data.setUint32(at,0x43443352,true);data.setUint16(at+4,1,true);data.setUint16(at+6,1,true);
        data.setUint32(at+8,size,true);
        for(const [offset,value] of [[16,slotsPtr],[20,values.length],[24,accessed.length?1:0],
          [28,accessed.length||values.length?1:0],[32,scannedPtes],[36,groups.size],
          [72,info.capacity],[76,info.physicalVersions]])data.setUint32(at+offset,value,true);
        for(const [offset,value] of [[40,info.mapping],[48,info.transaction],[56,info.code],[64,info.identity]])
          data.setBigUint64(at+offset,value,true);
        if(nativeDirtyEpoch)data.setBigUint64(at+80,beforeCode,true);
        if(dirtyVirtualPages){const pointer=slotsPtr+PAGE;new Uint32Array(memory.buffer,pointer,dirtyPtes.length).set(dirtyPtes.map(([at])=>at-DIRECTORY));data.setUint32(at+88,pointer,true);data.setUint32(at+92,dirtyPtes.length,true);}
        nativeCollections.push({epoch,directory,slots:values,operation});hooks.collectResult?.(data,at);
        return;
      }
      if(operation!=='component_cpu_view')return;
      const at=140;new Uint8Array(memory.buffer,at,128).fill(0);
      data.setUint32(at,0x56433352,true);data.setUint16(at+4,1,true);data.setUint16(at+6,1,true);
      data.setUint32(at+8,128,true);
      for(const [offset,name] of [[16,'backing'],[20,'length'],[24,'maps'],[28,'permissions'],
        [32,'guestPages'],[36,'capacity'],[40,'mapped'],[44,'mappingVersions'],
        [48,'physicalVersions'],[52,'privateSlots'],[56,'executableViews'],
        [96,'mappingChanges'],[100,'mappingChangeCount'],[104,'mappingChangeCapacity'],[108,'mappingChangeFlags']])
        data.setUint32(at+offset,info[name],true);
      for(const [offset,name] of [[64,'identity'],[72,'transaction'],[80,'code'],[88,'mapping']])
        data.setBigUint64(at+offset,info[name],true);
      if(preciseExecutable)data.setBigUint64(at+112,info.executableEpoch,true);
    },
    resultView(length){return new DataView(memory.buffer,140,length);},
    request(bytes){requests.push(bytes);new Uint8Array(memory.buffer,140,bytes.length).set(bytes);},
  };
  return {memory,data,info,rows,cpu,engine,compiled,map,content,changeMapping,write,flushes,
    pending,acks,ackAttempts,dirtyPublications,nativeCollections,requests,hooks,enqueue};
}

test('descriptor reuse compares the whole fresh record and retains journal, malformed and grow barriers',()=>{
  const f=world({journal:true}),tables=new PageTables(f.engine,f.cpu),owner={paused:true,open:false};
  assert.equal(tables.descriptorReuse,true);
  tables.configureDescriptorReuse(false,owner);
  const initialMisses=tables.descriptorReuseMisses,initialHits=tables.descriptorReuseHits;
  for(const invalid of [undefined,{paused:false,open:false},{paused:true,open:true}])
    assert.throws(()=>tables.configureDescriptorReuse(true,invalid),/paused/);
  assert.equal(tables.configureDescriptorReuse(true,owner),true);
  const original=f.engine.ok;let views=0,mutate=null;
  f.engine.ok=function(operation,...args){const result=original.call(this,operation,...args);
    if(operation==='component_cpu_view'){views++;mutate?.(this.resultView(128));}return result;};
  tables.refresh();tables.refresh();
  assert.equal(views,2,'cache hits still request the native descriptor');
  assert.equal(tables.descriptorReuseMisses,initialMisses+1);assert.equal(tables.descriptorReuseHits,initialHits+1);
  assert.notEqual(tables.descriptor(),tables.descriptor(),'public descriptor reads remain fresh owned objects');
  assert.equal(tables.descriptorReuseHits,initialHits+1);
  f.engine.resultView=Engine.prototype.resultView;
  mutate=view=>view.setUint32(60,1,true);tables.refresh();
  assert.equal(tables.descriptorReuseMisses,initialMisses+2,'even an otherwise unused word must prevent reuse');
  mutate=view=>view.setUint32(0,0,true);
  assert.throws(()=>tables.refresh(),/descriptor/);
  mutate=null;
  f.changeMapping(DATA+1,2,3);f.content(2,f.info.transaction);tables.refresh();
  assert.equal(f.pending.size,0);assert.equal(tables.rowsByPage.get(DATA+1).slot,2);
  const misses=tables.descriptorReuseMisses;tables.refresh();
  assert.equal(tables.descriptorReuseMisses,misses+1,'cleared journal count must be observed after ACK');
  tables.descriptor();const snapshot=new Uint8Array(f.memory.buffer,140,128).slice(),beforeGrow=tables.descriptorReuseMisses;
  f.memory.grow(1);
  f.engine.ok=function(operation,...args){if(operation==='component_cpu_view')new Uint8Array(f.memory.buffer,140,128).set(snapshot);else return original.call(this,operation,...args);};
  tables.refresh();assert.equal(tables.descriptorReuseMisses,beforeGrow+1,'equal bytes in a new memory buffer require fresh decoding');
  tables.configureDescriptorReuse(false,owner);assert.equal(tables.descriptorReuse,false);
});

test('descriptor reuse adopts only validated native publication stamps and still observes executable changes',()=>{
  const f=world({journal:true,nativeDirty:true,nativeDirtyEpoch:true,preciseExecutable:true});
  const tables=new PageTables(f.engine,f.cpu);tables.configureDescriptorReuse(true,{paused:true,open:false});tables.refresh();
  const dirty=page=>{f.data.setUint32(tables.pte(page),f.data.getUint32(tables.pte(page),true)|64,true);
    f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);};
  dirty(DATA);assert.equal(tables.publishDirty(),1);
  const misses=tables.descriptorReuseMisses,hits=tables.descriptorReuseHits;tables.refresh();
  assert.equal(tables.descriptorReuseMisses,misses);assert.equal(tables.descriptorReuseHits,hits+1);
  assert.equal(tables.descriptor().transaction,f.info.transaction);
  const generated={id:'descriptor-guest-code'};f.compiled.set(CODE,generated);
  dirty(CODE);assert.equal(tables.publishDirty(),1);tables.refresh();
  assert.equal(tables.descriptorReuseMisses,misses+1,'executable epoch must come from the next full native descriptor');
  assert.equal(f.compiled.get(CODE),generated);
  f.hooks.collectResult=(data,at)=>data.setUint32(at,0,true);dirty(DATA);
  assert.throws(()=>tables.publishDirty(),/header/);
  const malformedMisses=tables.descriptorReuseMisses;tables.refresh();
  assert.equal(tables.descriptorReuseMisses,malformedMisses+1,'malformed publication cannot patch the descriptor baseline');
  f.hooks.collectResult=()=>f.write(0);dirty(DATA);assert.equal(tables.publishDirty(),1);tables.refresh();
  assert.equal(f.compiled.has(CODE),false,'a host write after collection must still invalidate executable aliases');
});

test('data-only mapping retains an already generated v86 code module',()=>{
  const f=world(),tables=new PageTables(f.engine,f.cpu,{selectiveCode:true,incrementalSync:false});
  const generated={id:'retained-code'};f.compiled.set(CODE,generated);
  f.changeMapping(DATA+1,2,3);f.content(2,f.info.transaction);
  tables.refresh();
  assert.equal(f.compiled.get(CODE),generated,'unrelated data mapping must retain generated code');
});

test('global code is guarded opt-in and marks only canonical RX pages without COW',()=>{
  const f=world({journal:true,nativeDirty:true,globalCode:true});
  f.info.privateSlots=23*1024*1024;f.map(CODE+1,2,5);f.data.setUint32(f.info.privateSlots+(CODE+1)*4,1,true);
  f.map(CODE+2,3,7);f.map(CODE+3,4,1);
  const tables=new PageTables(f.engine,f.cpu),owner={paused:true,open:false};
  assert.equal(tables.globalCode,false);assert.equal(f.data.getUint32(596,true),0x600);
  for(const invalid of [undefined,{paused:false,open:false},{paused:true,open:true}])
    assert.throws(()=>tables.configureGlobalCode(true,invalid),/paused/);
  const before=f.cpu.tlbClears;
  assert.equal(tables.configureGlobalCode(true,owner),true);
  assert.equal(f.cpu.tlbClears,before+2);assert.equal(f.data.getUint32(596,true),0x680);
  for(const page of [CODE,CODE+1,CODE+2,CODE+3,DATA])
    assert.equal(!!(f.data.getUint32(tables.pte(page),true)&256),page===CODE);
  f.changeMapping(DATA+1,5,5);tables.refresh();assert(f.data.getUint32(tables.pte(DATA+1),true)&256);
  f.changeMapping(CODE,0,7);tables.refresh();assert.equal(f.data.getUint32(tables.pte(CODE),true)&256,0);
  assert.equal(tables.configureGlobalCode(false,owner),false);assert.equal(f.data.getUint32(596,true),0x600);
  assert(tables.pages.every(row=>!(f.data.getUint32(row.pte,true)&256)));
  const old=world(),legacy=new PageTables(old.engine,old.cpu);
  assert.throws(()=>legacy.configureGlobalCode(true,owner),/unsupported/);
});

test('global code keeps dirty resets non-global but mapping and host code changes fully invalidate before JIT',()=>{
  const f=world({journal:true,nativeDirty:true,nativeDirtyEpoch:true,preciseExecutable:true,globalCode:true});
  const tables=new PageTables(f.engine,f.cpu);tables.configureGlobalCode(true,{paused:true,open:false});f.flushes.length=0;
  f.data.setUint32(DIRECTORY, f.data.getUint32(DIRECTORY,true)|32,true);
  assert.equal(tables.publishDirty(),0);assert.deepEqual(f.flushes,['non-global']);
  assert.equal(f.data.getUint32(DIRECTORY,true)&32,0);
  f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
  f.data.setUint32(tables.pte(DATA),f.data.getUint32(tables.pte(DATA),true)|64,true);
  assert.equal(tables.publishDirty(),1);assert.equal(f.data.getUint32(tables.pte(DATA),true)&64,0);
  assert.equal(f.data.getUint32(tables.pte(CODE),true)&256,256);assert.deepEqual(f.flushes,['non-global','non-global']);
  f.flushes.length=0;f.write(0);tables.refresh();assert.deepEqual(f.flushes,['full','jit']);
  f.flushes.length=0;f.changeMapping(DATA,1,1);tables.refresh();assert.equal(f.flushes[0],'full');
  f.flushes.length=0;f.hooks.collectResult=(data,at)=>data.setUint32(at,0,true);
  assert.throws(()=>tables.publishDirty(),/header/);assert.deepEqual(f.flushes,['full']);
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

test('incremental PageTables applies final deduplicated map/protect/unmap/remap rows and PTEs',()=>{
  const f=world({journal:true}),tables=new PageTables(f.engine,f.cpu);
  assert.equal(tables.lastMappingScanCount,1<<20);assert.equal(tables.lastMappingFullRebuild,true);
  const generated={id:'retained-code'};f.compiled.set(CODE,generated);
  f.info.transaction=(1n<<32n)+5n;
  f.changeMapping(DATA+1,2,3);f.content(2,f.info.transaction);f.changeMapping(DATA,1,1);
  const firstEpoch=f.info.mapping;tables.refresh();
  assert.equal(tables.lastMappingScanCount,2);assert.equal(tables.lastMappingFullRebuild,false);
  assert.deepEqual(f.acks.at(-1),{epoch:firstEpoch,count:2});assert.equal(f.pending.size,0);
  assert.equal(f.data.getUint32(tables.pte(DATA),true),BACKING_BASE+PAGE+5);
  assert.equal(f.data.getUint32(tables.pte(DATA+1),true),BACKING_BASE+2*PAGE+7);
  f.changeMapping(DATA+1,2,1);f.changeMapping(DATA+1,null,0);
  f.changeMapping(DATA+1,3,3);f.content(3,f.info.transaction);f.changeMapping(DATA,null,0);
  tables.refresh();
  assert.equal(tables.lastMappingScanCount,2,'repeated changes to one VA are one final journal entry');
  assert.equal(f.data.getUint32(tables.pte(DATA),true),0);
  assert.equal(f.data.getUint32(tables.pte(DATA+1),true),BACKING_BASE+3*PAGE+7);
  assert.equal(tables.rowsByPage.has(DATA),false);assert.equal(tables.rowsByPage.get(DATA+1).slot,3);
  assert.deepEqual(tables.pages.map(row=>row.page).sort((a,b)=>a-b),[CODE,DATA+1]);
  for(const [index,row] of tables.pages.entries())assert.equal(row.index,index);
  assert.equal(f.compiled.get(CODE),generated);assert.equal(f.pending.size,0);
});

test('invalid and colliding mapping journals are not acknowledged and stale ACKs retain pending changes',()=>{
  for(const page of [1<<20,PRIVATE_VA>>>12]) {
    const f=world({journal:true}),tables=new PageTables(f.engine,f.cpu);
    f.info.transaction++;f.info.code=f.info.transaction;f.info.mapping=f.info.transaction;
    if(page<1<<20)f.map(page,2,3);else f.enqueue(page);
    assert.throws(()=>tables.refresh());
    assert.equal(f.ackAttempts.length,0);assert.equal(f.acks.length,0);assert.equal(f.pending.size,1);
  }
  const f=world({journal:true}),tables=new PageTables(f.engine,f.cpu);
  f.changeMapping(DATA+1,2,3);
  f.hooks.beforeAck=()=>{f.info.transaction++;f.info.mapping=f.info.transaction;f.hooks.beforeAck=null;};
  assert.throws(()=>tables.refresh(),/stale mapping journal acknowledgement/);
  assert.equal(f.ackAttempts.length,1);assert.equal(f.acks.length,0);assert.equal(f.pending.size,1);
  tables.refresh();assert.equal(f.acks.length,1);assert.equal(f.pending.size,0);
  assert.equal(tables.rowsByPage.get(DATA+1).slot,2);
});

test('policy switches flush pending writes and preserve mappings in legacy, JIT-only, sync-only and combined modes',()=>{
  const f=world({journal:true}),tables=new PageTables(f.engine,f.cpu);
  for(const policy of [{selectiveCode:false,incrementalSync:false},{selectiveCode:true,incrementalSync:false},
    {selectiveCode:false,incrementalSync:true},{selectiveCode:true,incrementalSync:true}]) {
    const pte=tables.pte(DATA),directory=DIRECTORY+(DATA>>>10)*4;
    f.data.setUint32(pte,f.data.getUint32(pte,true)|64,true);
    f.data.setUint32(directory,f.data.getUint32(directory,true)|32,true);
    const published=f.dirtyPublications.length,clears=f.cpu.clears;
    assert.deepEqual(tables.configure(policy),policy);
    assert.deepEqual(tables.policy,policy);assert(f.cpu.clears>clears);
    assert.deepEqual(f.dirtyPublications.slice(published),[[1]],'pending old-mode writes publish before switching');
    assert.equal(f.data.getUint32(pte,true)&64,0);assert.equal(tables.pages.find(row=>row.page===DATA).slot,1);
    if(!policy.incrementalSync) {
      assert.equal(tables.rowsByPage.size,0,'legacy full-sync does not populate a per-VA index');
      assert.equal(tables.dirty.groups.size,0,'legacy dirty collection does not populate directory groups');
    }
    assert.equal(tables.lastMappingFullRebuild,true);assert.equal(tables.lastMappingScanCount,1<<20);
    const generated={policy};f.compiled.set(CODE,generated);
    f.changeMapping(DATA+1,2,3);f.content(2,f.info.transaction);tables.refresh();
    assert.equal(tables.lastMappingScanCount,policy.incrementalSync?1:1<<20);
    assert.equal(f.compiled.get(CODE),policy.selectiveCode?generated:undefined);
    assert.equal(f.pending.size,0);assert.equal(tables.pages.find(row=>row.page===DATA+1).slot,2);
    if(!policy.incrementalSync) {
      assert.equal(tables.rowsByPage.size,0);assert.equal(tables.dirty.groups.size,0);
    }
  }
  const before=f.cpu.clears;
  assert.deepEqual(tables.configure({selectiveCode:true,incrementalSync:true}),tables.policy);
  assert.equal(f.cpu.clears,before,'reapplying the same policy is a no-op');
});

test('native dirty publication borrows deduplicated slots and acknowledges guest SMC without a JS request',()=>{
  const f=world({journal:true,nativeDirty:true});
  f.map(CODE,0,7);f.map(DATA,0,3);
  const tables=new PageTables(f.engine,f.cpu),generated={id:'guest-smc'};
  f.compiled.set(CODE,generated);
  const acknowledge=tables.codeValidity.acknowledgeGuestWrites;
  let borrowed=false;
  tables.codeValidity.acknowledgeGuestWrites=function(info,slots) {
    borrowed=slots instanceof Uint32Array && slots.buffer===f.memory.buffer && slots.byteOffset===27*1024*1024+PAGE;
    return acknowledge.call(this,info,slots);
  };
  for(const page of [CODE,DATA])f.data.setUint32(tables.pte(page),f.data.getUint32(tables.pte(page),true)|64,true);
  const directory=DIRECTORY+(DATA>>>10)*4;
  f.data.setUint32(directory,f.data.getUint32(directory,true)|32,true);
  const flushes=f.cpu.tlbClears;
  assert.equal(tables.publishDirty(),1);
  assert.equal(f.nativeCollections.length,1,'capable combined mode must call the bulk native collector');
  assert.deepEqual(f.nativeCollections[0].slots,[0]);assert.deepEqual(f.dirtyPublications,[[0]]);
  assert.equal(f.requests.length,0,'native-owned slot results must not be copied through the request buffer');
  assert.equal(borrowed,true,'code validity consumes the native-owned shared-memory result directly');
  assert.equal(f.cpu.tlbClears,flushes+1);assert.equal(tables.lastDirtyScanCount,2);
  assert.equal(tables.lastDirtyDirectoryCount,1);
  tables.refresh();assert.equal(f.compiled.get(CODE),generated);
  f.write(0);tables.refresh();assert.equal(f.compiled.has(CODE),false,'later host alias writes still invalidate');
});

test('native read-only directory access resets require a TLB flush even without dirty slots',()=>{
  const f=world({journal:true,nativeDirty:true}),tables=new PageTables(f.engine,f.cpu);
  f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
  const flushes=f.cpu.tlbClears,transaction=f.info.transaction;
  assert.equal(tables.publishDirty(),0);
  assert.equal(f.nativeCollections.length,1);
  assert.equal(f.cpu.tlbClears,flushes+1);
  assert.equal(f.data.getUint32(DIRECTORY,true)&32,0);
  assert.equal(f.info.transaction,transaction);assert.equal(f.requests.length,0);
});

test('targeted mapping refresh invalidates every changed VA after final PTE writes and before acknowledgement',()=>{
  const f=world({journal:true,nativeDirtyEpoch:true,dirtyVirtualPages:true,preciseExecutable:true});
  f.map(DATA+2,4,3);const t=new PageTables(f.engine,f.cpu),owner={paused:true,open:false};
  assert.equal(t.targetedMappings,false);assert.equal(t.configureTargetedMappings(true,owner),true);
  f.flushes.length=0;
  f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
  f.info.privateSlots=26*1024*1024;f.data.setUint32(f.info.privateSlots+DATA*4,1,true);
  f.changeMapping(DATA,2,3);f.changeMapping(DATA+1,3,1);f.changeMapping(DATA+2,null,0);
  f.hooks.beforeAck=()=>{
    assert.deepEqual(f.cpu.invalidatedPages,[DATA*PAGE,(DATA+1)*PAGE,(DATA+2)*PAGE]);
    assert.equal(f.data.getUint32(t.pte(DATA),true)&2,0,'COW keeps the new PTE read-only');
    assert.equal(f.data.getUint32(t.pte(DATA+2),true),0);
  };
  t.refresh();assert.equal(f.pending.size,0);assert.deepEqual(f.flushes,[]);
  assert.equal(f.cpu.cpl[0],3);assert(f.data.getUint32(DIRECTORY,true)&32,'mapping refresh must retain PDE access evidence');
  assert.deepEqual([t.targetedMappingRefreshes,t.targetedMappingPages,t.targetedMappingFallbacks],[1,3,0]);
});

test('targeted mappings retain full flushes for executable changes, rebuilds and unavailable helpers',()=>{
  for(const kind of ['code','rebuild','helper']){
    const f=world({journal:true,nativeDirtyEpoch:true,dirtyVirtualPages:true,preciseExecutable:true,globalCode:true});
    const t=new PageTables(f.engine,f.cpu);t.configureGlobalCode(true,{paused:true,open:false});
    t.configureTargetedMappings(true,{paused:true,open:false});f.flushes.length=0;
    if(kind==='code')f.changeMapping(CODE,2,5);else f.changeMapping(DATA,2,3);
    if(kind==='rebuild')t.mappingEpoch=null;
    if(kind==='helper')t.invalidateDirtyPage=null;
    t.refresh();assert.equal(f.cpu.invalidatedPages.length,0,kind);assert.equal(f.flushes[0],'full',kind);
    assert.equal(f.flushes.includes('jit'),kind==='code',kind);assert.equal(t.targetedMappingFallbacks,1,kind);
  }
});

test('targeted mapping failure restores CPL and full-flushes before preserving the original error',()=>{
  const f=world({journal:true,nativeDirtyEpoch:true,dirtyVirtualPages:true}),t=new PageTables(f.engine,f.cpu);
  t.configureTargetedMappings(true,{paused:true,open:false});f.flushes.length=0;
  f.changeMapping(DATA,2,3);f.changeMapping(DATA+1,3,3);
  const original=Error('INVLPG failed');let count=0;
  t.invalidateDirtyPage=f.cpu.wm.exports.instr16_0F01_7_mem=()=>{assert.equal(f.cpu.cpl[0],0);if(++count===2)throw original;};
  assert.throws(()=>t.refresh(),error=>error===original);
  assert.equal(count,2);assert.equal(f.cpu.cpl[0],3);assert.deepEqual(f.flushes,['full']);
  assert.equal(f.ackAttempts.length,0);assert.equal(f.pending.size,2);
});

test('targeted dirty rearm invalidates every dirty virtual alias and preserves read-only translations',()=>{
  const f=world({journal:true,nativeDirty:true,nativeDirtyEpoch:true,dirtyVirtualPages:true});
  f.cpu.cpl=new Proxy({},{get(_target,key){const view=new Uint8Array(f.memory.buffer,612,1);return Reflect.get(view,key,view);},
    set(_target,key,value){new Uint8Array(f.memory.buffer,612,1)[key]=value;return true;}});
  assert.equal(f.cpu.cpl instanceof Uint8Array,false);
  f.map(DATA+1024,1,3);const t=new PageTables(f.engine,f.cpu),owner={paused:true,open:false};
  assert.equal(t.configureTargetedDirty(true,owner),true);f.flushes.length=0;
  for(const page of [DATA,DATA+1024]){f.data.setUint32(t.pte(page),f.data.getUint32(t.pte(page),true)|64,true);const at=DIRECTORY+(page>>>10)*4;f.data.setUint32(at,f.data.getUint32(at,true)|32,true);}
  assert.equal(t.publishDirty(),1);assert.deepEqual(f.cpu.invalidatedPages,[DATA*PAGE,(DATA+1024)*PAGE]);
  assert.equal(f.cpu.cpl[0],3);assert.deepEqual(f.flushes,[]);
  f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
  assert.equal(t.publishDirty(),0);assert.deepEqual(f.flushes,[]);assert.equal(f.cpu.invalidatedPages.length,2);
  f.data.setUint32(t.pte(DATA),f.data.getUint32(t.pte(DATA),true)|64,true);f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
  assert.equal(t.publishDirty(),1);assert.equal(f.cpu.invalidatedPages.length,3);
});

test('targeted rearm restores privilege and flushes on helper failure, malformed aliases or executable writes',()=>{
  for(const kind of ['helper','malformed','executable','alias']){
    const f=world({journal:true,nativeDirty:true,nativeDirtyEpoch:true,dirtyVirtualPages:true});
    if(kind==='executable')f.map(DATA,1,7);
    if(kind==='alias')f.map(DATA,0,3);
    const t=new PageTables(f.engine,f.cpu);t.configureTargetedDirty(true,{paused:true,open:false});f.flushes.length=0;
    f.data.setUint32(t.pte(DATA),f.data.getUint32(t.pte(DATA),true)|64,true);f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
    if(kind==='helper')f.cpu.invalidateFailure=new Error('invlpg failed');
    if(kind==='malformed')f.hooks.collectResult=(d,at)=>d.setUint32(d.getUint32(at+88,true),0,true);
    if(kind==='executable'||kind==='alias'){assert.equal(t.publishDirty(),1);assert.deepEqual(f.cpu.invalidatedPages,[]);}
    else assert.throws(()=>t.publishDirty(),kind==='helper'?/invlpg failed/:/native dirty/);
    assert.equal(f.cpu.cpl[0],3);assert.deepEqual(f.flushes,['full']);
  }
});

test('malformed native dirty results fail closed after a safety TLB flush and before guest acknowledgement',()=>{
  const changes=[
    (d,at)=>d.setUint32(at,0,true),
    (d,at)=>d.setUint16(at+4,2,true),
    (d,at)=>d.setUint16(at+6,2,true),
    (d,at)=>d.setUint32(at+8,79,true),
    (d,at)=>d.setUint32(at+12,1,true),
    (d,at)=>d.setUint32(at+16,0,true),
    (d,at)=>d.setUint32(at+16,3,true),
    (d,at)=>d.setUint32(at+16,d.byteLength,true),
    (d,at)=>d.setUint32(at+20,17,true),
    (d,at)=>d.setUint32(at+24,2,true),
    (d,at)=>d.setUint32(at+28,0,true),
    (d,at)=>d.setUint32(at+32,(1<<20)+1,true),
    (d,at)=>d.setUint32(at+36,1025,true),
    (d,at)=>d.setBigUint64(at+40,99n,true),
    (d,at)=>d.setBigUint64(at+64,99n,true),
    (d,at)=>d.setUint32(at+72,17,true),
    (d,at)=>d.setUint32(at+76,0,true),
  ];
  for(const corrupt of changes) {
    const f=world({journal:true,nativeDirty:true});f.map(CODE,0,7);
    const tables=new PageTables(f.engine,f.cpu);
    f.data.setUint32(tables.pte(CODE),f.data.getUint32(tables.pte(CODE),true)|64,true);
    f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
    f.hooks.collectResult=corrupt;
    const flushes=f.cpu.tlbClears,baseline=tables.codeValidity.content.get(0);
    assert.throws(()=>tables.publishDirty(),/native dirty/);
    assert.equal(f.cpu.tlbClears,flushes+1,'a successful native call may already have reset cached A/D state');
    assert.equal(tables.codeValidity.content.get(0),baseline,'malformed results cannot acknowledge guest code');
    assert.equal(f.requests.length,0,'malformed native results cannot fall back to JS publication');
  }
});

test('native dirty collector errors preserve the JS view and do not flush or silently fall back',()=>{
  const f=world({journal:true,nativeDirty:true}),tables=new PageTables(f.engine,f.cpu);
  f.changeMapping(DATA+1,2,3);
  const flushes=f.cpu.tlbClears;
  assert.throws(()=>tables.publishDirty(),/invalid native dirty source/);
  assert.equal(f.cpu.tlbClears,flushes);assert.equal(f.requests.length,0);
  assert.equal(f.nativeCollections.length,0);assert.equal(f.dirtyPublications.length,0);
});

test('an untouched native dirty result does not flush or advance publication',()=>{
  const f=world({journal:true,nativeDirty:true}),tables=new PageTables(f.engine,f.cpu);
  const flushes=f.cpu.tlbClears,transaction=f.info.transaction;
  assert.equal(tables.publishDirty(),0);assert.equal(f.nativeCollections.length,1);
  assert.equal(f.cpu.tlbClears,flushes);assert.equal(f.info.transaction,transaction);
  assert.equal(tables.lastDirtyScanCount,0);assert.equal(tables.lastDirtyDirectoryCount,1);
});

test('dirty collector switching requires a closed paused owner and publishes the old mode before switching',()=>{
  const f=world({journal:true,nativeDirty:true}),tables=new PageTables(f.engine,f.cpu);
  const generated={id:'mode-retains-code'};f.compiled.set(CODE,generated);
  const mapping=tables.mappingEpoch,clears=f.cpu.clears;
  for(const owner of [undefined,{paused:false,open:false},{paused:true,open:true}])
    assert.throws(()=>tables.configureDirty(false,owner),/paused/);
  for(const mode of [false,true]) {
    f.data.setUint32(tables.pte(DATA),f.data.getUint32(tables.pte(DATA),true)|64,true);
    f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
    const calls=f.nativeCollections.length,requests=f.requests.length;
    assert.equal(tables.configureDirty(mode,{paused:true,open:false}),mode);
    assert.equal(tables.nativeDirty,mode);
    assert.equal(f.nativeCollections.length-calls,mode?0:1,'the outgoing collector publishes pending writes');
    assert.equal(f.requests.length-requests,mode?1:0);
    assert.equal(tables.mappingEpoch,mapping);assert.equal(f.cpu.clears,clears);
    assert.equal(f.compiled.get(CODE),generated);
  }
  const legacy=world(),legacyTables=new PageTables(legacy.engine,legacy.cpu);
  assert.equal(legacyTables.nativeDirty,false);
  assert.throws(()=>legacyTables.configureDirty(true,{paused:true,open:false}),/unsupported/);
});

test('native guest acknowledgement preserves an unreviewed host write to another executable slot',()=>{
  const f=world({journal:true,nativeDirty:true});
  f.map(CODE,0,7);f.map(CODE+1,2,5);f.content(2,1n);
  const tables=new PageTables(f.engine,f.cpu),generated={id:'unreviewed-host-write'};
  f.compiled.set(CODE,generated);f.write(2);
  f.data.setUint32(tables.pte(CODE),f.data.getUint32(tables.pte(CODE),true)|64,true);
  f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
  assert.equal(tables.publishDirty(),1);
  tables.refresh();assert.equal(f.compiled.has(CODE),false);
});

test('native collection uses the acknowledged COW/remap slot and retains the former executable alias',()=>{
  const f=world({journal:true,nativeDirty:true});f.map(DATA,0,3);
  const tables=new PageTables(f.engine,f.cpu),generated={id:'cow-keeps-former-alias'};
  f.compiled.set(CODE,generated);
  f.changeMapping(DATA,2,3);f.content(2,f.info.transaction);tables.refresh();
  f.data.setUint32(tables.pte(DATA),f.data.getUint32(tables.pte(DATA),true)|64,true);
  f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
  const original=f.data.getBigUint64(f.info.physicalVersions+8,true);
  assert.equal(tables.publishDirty(),1);assert.deepEqual(f.nativeCollections.at(-1).slots,[2]);
  assert.equal(f.data.getBigUint64(f.info.physicalVersions+8,true),original);
  tables.refresh();assert.equal(f.compiled.get(CODE),generated);
});

test('native 88-byte publication adopts a proven guest code stamp without rebuilding executable rows',()=>{
  const f=world({journal:true,nativeDirty:true,nativeDirtyEpoch:true,dirtyGateFlags:true,preciseExecutable:true});f.map(CODE,0,7);
  const tables=new PageTables(f.engine,f.cpu);
  assert.equal(tables.nativeDirtyEpochSupported,true);assert.equal(tables.dirtyGateFlagsSupported,true);
  f.data.setUint32(tables.pte(CODE),f.data.getUint32(tables.pte(CODE),true)|64,true);
  f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
  assert.equal(tables.publishDirty(),1);assert.equal(tables.codeValidity.code,f.info.code);
  assert.equal(tables.codeValidity.executableEpoch,f.info.executableEpoch);
  const forbidden={[Symbol.iterator](){throw new Error('proven guest stamp rebuilt executable rows');}};
  assert.equal(tables.codeValidity.refresh(f.info,forbidden).invalidate,false);
  assert.equal(f.nativeCollections.length,1);assert.equal(f.requests.length,0);
});

test('native 88-byte publication cannot acknowledge a same-slot host write unseen before collection',()=>{
  const f=world({journal:true,nativeDirty:true,nativeDirtyEpoch:true});f.map(CODE,0,7);
  const tables=new PageTables(f.engine,f.cpu),generated={id:'unseen-before-collect'};
  f.compiled.set(CODE,generated);f.write(0);
  f.data.setUint32(tables.pte(CODE),f.data.getUint32(tables.pte(CODE),true)|64,true);
  f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
  assert.equal(tables.publishDirty(),1);
  assert.equal(tables.codeValidity.code,1n);assert.equal(tables.codeValidity.content.get(0),1n);
  tables.refresh();assert.equal(f.compiled.has(CODE),false);
});

test('native 88-byte publication cannot absorb a same-slot host write after collection',()=>{
  const f=world({journal:true,nativeDirty:true,nativeDirtyEpoch:true});f.map(CODE,0,7);
  const tables=new PageTables(f.engine,f.cpu),generated={id:'host-after-collect'};
  f.compiled.set(CODE,generated);
  f.data.setUint32(tables.pte(CODE),f.data.getUint32(tables.pte(CODE),true)|64,true);
  f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
  f.hooks.collectResult=()=>f.write(0);
  assert.equal(tables.publishDirty(),1);
  assert.equal(tables.codeValidity.code,2n);assert.equal(tables.codeValidity.content.get(0),2n);
  assert.equal(f.data.getBigUint64(f.info.physicalVersions+8,true),3n);
  tables.refresh();assert.equal(f.compiled.has(CODE),false);
});

test('precise executable epoch skips executable rows for data-only mapping and protection updates',()=>{
  const f=world({journal:true,preciseExecutable:true}),tables=new PageTables(f.engine,f.cpu);
  assert.equal(tables.descriptor().executableEpoch,1n);
  const watch=tables.codeValidity,pages=watch.pages;
  const forbidden={[Symbol.iterator](){throw new Error('data-only change walked executable rows');}};
  f.changeMapping(DATA+1,2,3);f.content(2,f.info.transaction);
  assert.equal(watch.refresh(f.info,forbidden).invalidate,false);
  f.changeMapping(DATA,1,1);
  assert.equal(watch.refresh(f.info,forbidden).invalidate,false);
  assert.equal(watch.pages,pages);assert.equal(watch.code,f.info.code);assert.equal(watch.mapping,f.info.mapping);
});

test('changed precise executable epoch still validates executable identity and physical content',()=>{
  const f=world({preciseExecutable:true}),watch=new CodeValidity(f.memory);
  watch.refresh(f.info,f.rows.values());
  f.changeMapping(CODE,0,7);
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'executable-mapping');
  f.write(0);
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'host-code-write');
  f.content(0,f.info.transaction+1n);f.info.executableEpoch++;
  assert.equal(watch.refresh(f.info,f.rows.values()).reason,'host-code-write',
    'a precise epoch change cannot be skipped even when broad markers are unchanged');
});

test('trusted clean PTE collection is explicit and retains native read/write rearming',()=>{
  const f=world({journal:true,nativeDirty:true,nativeDirtyEpoch:true,dirtyVirtualPages:true,trustedExport:true}),t=new PageTables(f.engine,f.cpu),owner={paused:true,open:false};
  assert.equal(t.trustedCleanPtes,false);assert.equal(t.trustedCleanPtesSupported,true);
  for(const invalid of [undefined,{paused:false,open:false},{paused:true,open:true}])assert.throws(()=>t.configureTrustedCleanPtes(true,invalid),/paused/);
  t.configureTrustedCleanPtes(true,owner);assert.equal(f.nativeCollections.at(-1).operation,'component_cpu_collect_dirty');
  for(const dirty of [false,true,false,true]) {
    f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
    if(dirty)f.data.setUint32(t.pte(DATA),f.data.getUint32(t.pte(DATA),true)|64,true);
    const before=f.info.transaction,clears=f.cpu.tlbClears;
    assert.equal(t.publishDirty(),Number(dirty));assert.equal(f.nativeCollections.at(-1).operation,'component_cpu_collect_dirty_trusted');
    assert.equal(f.cpu.tlbClears,clears+1);assert.equal(f.info.transaction>before,dirty);
  }
  assert.throws(()=>t.configureDirty(false,owner),/requires/);
  t.configureTrustedCleanPtes(false,owner);assert.equal(f.nativeCollections.at(-1).operation,'component_cpu_collect_dirty_trusted');
  t.publishDirty();assert.equal(f.nativeCollections.at(-1).operation,'component_cpu_collect_dirty');
});

test('trusted clean PTE capability rejects old hosts and malformed output retains safety flush',()=>{
  const owner={paused:true,open:false},old=world({journal:true,nativeDirty:true}),legacy=new PageTables(old.engine,old.cpu);
  assert.throws(()=>legacy.configureTrustedCleanPtes(true,owner),/unsupported/);assert.equal(old.nativeCollections.length,0);
  for(const change of [f=>f.cpu.cr[0]&=~0x10000,f=>f.cpu.wasm_memory=new WebAssembly.Memory({initial:1})]) {
    const f=world({journal:true,nativeDirty:true,nativeDirtyEpoch:true,dirtyVirtualPages:true,trustedExport:true}),t=new PageTables(f.engine,f.cpu);change(f);
    assert.throws(()=>t.configureTrustedCleanPtes(true,owner),/pinned.*write protection/);assert.equal(f.nativeCollections.length,0);assert.equal(t.trustedCleanPtes,false);
  }
  const f=world({journal:true,nativeDirty:true,nativeDirtyEpoch:true,dirtyVirtualPages:true,trustedExport:true}),t=new PageTables(f.engine,f.cpu);
  t.configureTrustedCleanPtes(true,owner);f.data.setUint32(DIRECTORY,f.data.getUint32(DIRECTORY,true)|32,true);
  f.hooks.collectResult=(d,at)=>d.setUint32(at,0,true);const clears=f.cpu.tlbClears;
  assert.throws(()=>t.publishDirty(),/invalid.*header/);assert.equal(f.cpu.tlbClears,clears+1);
  assert.equal(f.nativeCollections.at(-1).operation,'component_cpu_collect_dirty_trusted');
});
