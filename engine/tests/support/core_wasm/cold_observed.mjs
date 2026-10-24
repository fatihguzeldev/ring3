import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {readEngine} from '../wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../sdk/runtime/cache.js';

const FP=4236,SIMD=4364,YMM=4612;
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=bytes=>Buffer.from(bytes).toString('hex');
const unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const THREE='00000000000000c00040',TWO='00000000000000800040';

// Expected values below are literal x86 arithmetic, not output from a second
// execution backend. The reusable fixture never seeds or prepares guest code.
export function coldFixture(artifact,report,name,options,body) {
  const result={name,complete:false,runs:[],modules:[],helperCalls:[]};report.cases.push(result);
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const raw=Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value]));
  const memory=instance.exports.memory;
  const bytes=(pointer,length)=>new Uint8Array(memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(memory.buffer).getUint32(pointer+offset,true);
  const bigint=(pointer,offset=0)=>new DataView(memory.buffer).getBigUint64(pointer+offset,true);
  const setWord=(pointer,offset,value)=>new DataView(memory.buffer).setUint32(pointer+offset,value,true);
  let prepares=0,resolverCalls=0,cache,engine,context=0,opened=false;
  const numeric=(operation,pointer,pc,ordinal,args)=>{
    const call={operation,ordinal,identity:String(bigint(pointer,120)),pc,contextPc:word(pointer,32)};
    result.helperCalls.push(call);call.returned=raw[operation](...args);
    const bank=word(pointer,92);call.fip=word(bank,8);
    call.opcode=new DataView(memory.buffer).getUint16(bank+6,true);return call.returned;
  };
  const api={...raw,
    core_prepare:(...args)=>{
      prepares++;
      if(options.referencePrepared)return raw.core_prepare(...args);
      throw new Error(`${name}: ordinary cold frontend invoked core_prepare(${args})`);
    },
    core_numeric:(pointer,ordinal)=>numeric('core_numeric',pointer,word(pointer,32),ordinal,[pointer,ordinal]),
    ...(typeof raw.core_numeric_owned==='function'?{core_numeric_owned:(pointer,slot,generation,pc,ordinal)=>
      numeric('core_numeric_owned',pointer,pc,ordinal,[pointer,slot,generation,pc,ordinal])}:{}),
    ...(typeof raw.core_chain_resolve_owned==='function'?{core_chain_resolve_owned:(...args)=>{
      resolverCalls++;return raw.core_chain_resolve_owned(...args);
    }}:{}),
  };
  const moduleDescriptor=Object.getOwnPropertyDescriptor(WebAssembly,'Module'),OriginalModule=WebAssembly.Module;
  const moduleRecords=new Map();
  // Count real constructors, including any hidden guest compilation. The
  // already-loaded engine is outside this window; the shared dispatcher is
  // allowed and distinguished by its actual export contract.
  const ModuleSpy=new Proxy(OriginalModule,{construct(target,args){
    const module=Reflect.construct(target,args,target),exports=OriginalModule.exports(module).map(item=>item.name);
    const source=Uint8Array.from(new Uint8Array(args[0].buffer??args[0],args[0].byteOffset??0,args[0].byteLength));
    const guest=exports.includes('run')&&exports.includes('link')&&exports.includes('publish');
    const support=exports.includes('supportAbi')&&!exports.includes('link');
    assert(guest||support,`${name}: unexpected generated module contract`);
    result.modules.push({kind:guest?'guest':'support',bytes:source.length,sha256:hash(source),exports});
    return module;
  }});
  try {
    Object.defineProperty(WebAssembly,'Module',{...moduleDescriptor,value:ModuleSpy});
    engine=new Engine(api,memory,artifact.sha256);engine.ok('open',16,...KEY);opened=true;
    engine.base=raw.arena_ptr()>>>0;
    const code=new Uint8Array(4096),data=new Uint8Array(4096);
    code.set(typeof options.code==='string'?unhex(options.code):options.code);
    options.data?.(new DataView(data.buffer));
    for(const[address,page]of[[0x1000,code],[0x3000,data],[0x4000,new Uint8Array(4096)]]) {
      engine.ok('map',address,1,address===0x1000?7:3);engine.request(page);engine.ok('upload',address,4096);
    }
    const makeState=({registers=options.registers??[0,0,0,0x3000,0x4800,0x12345678,0x87654321,0x99aabbcc],
      flags=options.flags??2,pc=options.pc??0x1000}={})=>{
      assert.equal(registers.length,8);const state=record('R3ST',56),s=view(state);
      registers.forEach((value,index)=>s.setUint32(16+4*index,value,true));
      s.setUint32(48,pc,true);s.setUint32(52,flags,true);return state;
    };
    new Uint8Array(memory.buffer,engine.base,56).set(makeState());
    const fp=new Uint8Array(memory.buffer,engine.base+FP,128),d=view(fp);
    if(options.fp) {assert.equal(options.fp.length,128);fp.set(options.fp);}
    else {
      for(const[offset,value]of[[16,0x27f],[18,0x200],[20,0xfff0],[22,0x5a5],[32,0x33],[34,0x5b]])d.setUint16(offset,value,true);
      d.setUint32(24,0x11223344,true);d.setUint32(28,0x55667788,true);
      [THREE,TWO,'00000000000000000080','0000000000000040ff3f','00000000000000c0ff7f',
        '000000000000008000c0','01000000000000c00040','00000000000000000000']
        .forEach((value,index)=>fp.set(unhex(value),40+10*index));
    }
    const initialFp=bytes(engine.base+FP,128),initialSimd=bytes(engine.base+SIMD,160),initialYmm=bytes(engine.base+YMM,144);
    assert.equal(raw.core_cold_run(0),2,'zero-budget native initialization executes no guest instruction');
    context=raw.core_context_ptr()>>>0;assert(context>0&&context%8===0);
    assert.equal(bigint(context,120),0n,'cold initialization has no guest module identity');
    assert.equal(raw.core_code_len()>>>0,0,'cold initialization emits no guest module');
    const Cache=artifact.CodeCache??CodeCache;
    cache=new Cache(engine,{budget:options.budget??50000,instructions:128,blocks:32,chain:true});
    cache.retainContext(context);
    const originalEncoded=cache.encoded.bind(cache);
    cache.encoded=(pointerName,lengthName)=>{
      const source=originalEncoded(pointerName,lengthName);
      if(pointerName==='core_code_ptr') {
        const id=bigint(context,120);assert(id>0n);
        moduleRecords.set(id,{id,bytes:source.length,sha256:hash(source),source});
      }
      return source;
    };
    const physical=address=>{
      const slot=word(word(context,56)+(address>>>12)*4);assert(slot>0,'literal write addresses a mapped page');
      return word(context,64)+(slot-1)*4096+(address&4095);
    };
    const world={artifact,result,raw,api,memory,engine,cache,context,bytes,word,bigint,setWord,
      physical,moduleRecords,initialFp,initialSimd,initialYmm,makeState,
      get prepares(){return prepares;},get helperCalls(){return result.helperCalls;},
      get resolverCalls(){return resolverCalls;},
      revision:()=>bigint(word(context,80)),
      snapshot(){return{context:bytes(context,128),state:bytes(engine.base,56),fp:bytes(engine.base+FP,128),
        simd:bytes(engine.base+SIMD,160),ymm:bytes(engine.base+YMM,144),
        ram:bytes(word(context,64),word(context,68)*4096),pages:bytes(word(context,84),word(context,68)*16),
        revision:this.revision(),epoch:bigint(word(context,88)),prepares,helpers:result.helperCalls.length,resolverCalls};},
      reset(changes={}) {
        cache.publish();new Uint8Array(memory.buffer,engine.base,56).set(makeState(changes));
        for(const[offset,image]of[[FP,changes.fp??initialFp],[SIMD,changes.simd??initialSimd],[YMM,changes.ymm??initialYmm]])
          new Uint8Array(memory.buffer,engine.base+offset,image.length).set(image);
        cache.import();
      },
      execute(budget,pc) {
        cache.budget=budget;
        // This explicit reference adapter is CLI-only. It gives the old eager
        // compiler the identical literal program before testing the new cold
        // frontend's zero-emission contract. Ordinary cases never enter it.
        const selected=options.referencePrepared?cache.entry(pc??cache.pc):
          pc===undefined?cache.select():cache.select(pc);
        const run=cache.execute(selected);result.runs.push({...run,id:run.id===null?null:String(run.id)});return run;
      },
      executeNative(budget) {
        // The SDK rejects Infrastructure as an execution error. Verify that
        // native resource boundary through the same public cold ABI, then
        // publish its exact completed prefix without manufacturing a JIT unit.
        const retired=word(context,40),stop=raw.core_cold_run(budget);
        assert(stop>=1&&stop<=10,'native cold ABI returns an architectural stop');
        const work=budget-word(context,44);engine.ok('core_export',stop,work);
        const run={coreReason:stop,pc:word(context,32),retired:word(context,40)-retired,
          work,kind:'cold-native',id:null,regions:0};
        result.runs.push({...run});return run;
      },
      recording() {
        const pointer=raw.core_trace_pointer();assert(pointer>0&&pointer%8===0);
        const h=new DataView(memory.buffer,pointer,32),used=h.getUint32(4,true),limit=h.getUint32(8,true);
        assert.equal(limit,129);assert(used<=limit);
        return{active:h.getUint32(0,true),used,limit,overflow:h.getUint32(12,true),
          events:Array.from({length:used},(_,index)=>{
            const e=new DataView(memory.buffer,pointer+32+index*24,24);
            return{kind:e.getUint32(0,true),pc:e.getUint32(4,true),end:e.getUint32(8,true),
              offset:e.getUint32(12,true),owner:String(e.getBigUint64(16,true))};
          })};
      },
    };
    body(world);
    if(!options.referencePrepared)assert.equal(prepares,0,'normal frontend never calls the baseline compiler');
    result.stats=cache.sampleUsage();result.complete=true;
  } catch(error) {result.failure={message:String(error),stack:error.stack};throw error;}
  finally {
    Object.defineProperty(WebAssembly,'Module',moduleDescriptor);
    if(cache?.open)raw.core_end();if(opened)engine.ok('close');
  }
}

export function coldLiteral(world,before,run,{pc,retired,work=retired,budget,registers,flags=2,stop=2,
  writes=[],versionWrites=writes.length,fp,fpRegisters={},fpPc,fpOpcode=0xc1,fpStatus=0,fpTag=0xfff0,
  fpData,fpDs,identity=run.id??0n,fault=[0,0,0]}) {
  assert.deepEqual([run.coreReason,run.pc,run.retired,run.work],[stop,pc,retired,work],'literal completed execution');
  const expectedContext=before.context.slice(),c=view(expectedContext);
  registers.forEach((value,index)=>c.setUint32(index*4,value,true));
  for(const[offset,value]of[[32,pc],[36,flags],[40,view(before.context).getUint32(40,true)+retired],
    [44,budget-work],[48,fault[0]],[52,fault[1]],[112,0],[116,fault[2]]])c.setUint32(offset,value,true);
  c.setBigUint64(120,BigInt(identity),true);
  // Every begin validates the authority observed before this lease. A store
  // can change that authority afterwards, without changing the expected token.
  c.setBigUint64(96,before.epoch,true);
  assert.deepEqual(world.bytes(world.context,128),expectedContext,'all 128 Context bytes');
  const expectedState=before.state.slice(),s=view(expectedState);s.setUint16(4,1,true);
  registers.forEach((value,index)=>s.setUint32(16+index*4,value,true));s.setUint32(48,pc,true);s.setUint32(52,flags,true);
  assert.deepEqual(world.bytes(world.engine.base,56),expectedState,'entire published canonical scalar packet');
  let expectedFp;
  if(fp) {assert.equal(fp.length,128);expectedFp=fp;}
  else {
    expectedFp=before.fp.slice();const f=view(expectedFp);
    for(const[offset,value]of[[18,fpStatus],[20,fpTag],[22,fpOpcode],[32,0x1b]])f.setUint16(offset,value,true);
    if(fpPc!==undefined)f.setUint32(24,fpPc,true);
    if(fpData!==undefined)f.setUint32(28,fpData,true);
    if(fpDs!==undefined)f.setUint16(34,fpDs,true);
    for(const[index,value]of Object.entries(fpRegisters))expectedFp.set(typeof value==='string'?unhex(value):value,40+10*Number(index));
  }
  assert.deepEqual(world.bytes(world.engine.base+FP,128),expectedFp,'all raw80 payloads, empty ghosts, independent tags and metadata');
  assert.deepEqual(world.bytes(world.engine.base+SIMD,160),before.simd,'whole SIMD packet');
  assert.deepEqual(world.bytes(world.engine.base+YMM,144),before.ymm,'whole YMM packet');
  const expectedRam=before.ram.slice(),expectedPages=before.pages.slice(),backing=world.word(world.context,64);
  const finalWrites=new Map();
  for(const[ordinal,write]of writes.entries()) {
    const payload=typeof write.bytes==='string'?unhex(write.bytes):write.bytes;
    const physical=world.physical(write.address)-backing;expectedRam.set(payload,physical);
    const count=write.count??1,version=write.version??ordinal+1;
    for(let page=physical>>>12;page<=((physical+payload.length-1)>>>12);page++)
      finalWrites.set(page,write.lastVersion??version+count-1);
  }
  for(const[page,ordinal]of finalWrites)view(expectedPages).setBigUint64(page*16+8,before.revision+BigInt(ordinal),true);
  assert.deepEqual(world.bytes(backing,expectedRam.length),expectedRam,'entire backing allocation including code and stack');
  assert.equal(world.revision(),before.revision+BigInt(versionWrites),'one exact logical version per committed write');
  assert.deepEqual(world.bytes(world.word(world.context,84),expectedPages.length),expectedPages,'all physical-page authority stamps');
  world.result.oracle={contextSha256:hash(expectedContext),fpSha256:hash(expectedFp),ramSha256:hash(expectedRam),
    pageVersionsSha256:hash(expectedPages),revision:String(world.revision())};
  const row=world.result.runs.at(-1);
  if(row)Object.assign(row,{semanticComplete:true,oracle:world.result.oracle});
}

function noGuestCode(world) {
  assert.equal(world.prepares,0);assert.equal(world.cache.units.size,0);assert.equal(world.cache.admissions,0);
  assert.equal(world.cache.generatedModuleBytes,0);assert.equal(world.moduleRecords.size,0);
  assert.equal(world.result.modules.filter(module=>module.kind==='guest').length,0);
  const native=world.cache.sampleUsage().nativeCache;
  assert.equal(native.compiled,'0');assert.equal(native.admittedEncodedBytes,'0');
}

export function runColdBoundaryCases(artifact,report) {
  const source=d=>{[0x11,0x22,0x33].forEach((value,index)=>d.setUint8(0x100+index,value));};
  coldFixture(artifact,report,'cold-rep-partial-quota-and-exact-resume',{
    code:'f3a4 90',registers:[7,3,0,0x3000,0x4800,5,0x3100,0x3200],data:source,
  },world=>{
    let before=world.snapshot(),run=world.execute(2);
    coldLiteral(world,before,run,{pc:0x1000,retired:0,work:2,budget:2,
      registers:[7,1,0,0x3000,0x4800,5,0x3102,0x3202],fp:before.fp,
      writes:[{address:0x3200,bytes:'11'},{address:0x3201,bytes:'22'}],versionWrites:2});
    assert.equal(world.recording().used,0,'partial REP work is never a completed observation');
    before=world.snapshot();run=world.execute(2);
    coldLiteral(world,before,run,{pc:0x1003,retired:2,work:2,budget:2,
      registers:[7,0,0,0x3000,0x4800,5,0x3103,0x3203],fp:before.fp,
      writes:[{address:0x3202,bytes:'33'}],versionWrites:1});
    noGuestCode(world);
  });
  coldFixture(artifact,report,'cold-rep-late-unmapped-write-fault',{
    code:'f3a4 90',registers:[7,3,0,0x3000,0x4800,5,0x3100,0x4fff],data:source,
  },world=>{
    const before=world.snapshot(),run=world.execute(3);
    coldLiteral(world,before,run,{pc:0x1000,retired:0,work:1,budget:3,stop:3,
      registers:[7,2,0,0x3000,0x4800,5,0x3101,0x5000],fp:before.fp,
      writes:[{address:0x4fff,bytes:'11'}],versionWrites:1,fault:[0x5000,0x10201,0x10e]});
    assert.equal(world.recording().used,0,'a faulting partial REP is not recorded as completed');noGuestCode(world);
  });
  coldFixture(artifact,report,'cold-rep-executable-store-partial-and-completed-prefixes',{
    code:'f3a4 90',registers:[7,2,0,0x3000,0x4800,5,0x3100,0x10f0],data:source,
  },world=>{
    let before=world.snapshot(),run=world.execute(8);
    coldLiteral(world,before,run,{pc:0x1000,retired:0,work:1,budget:8,stop:6,
      registers:[7,1,0,0x3000,0x4800,5,0x3101,0x10f1],fp:before.fp,
      writes:[{address:0x10f0,bytes:'11'}],versionWrites:1});
    assert.equal(world.recording().used,0,'partial executable REP is not a completed instruction');
    before=world.snapshot();run=world.execute(8);
    coldLiteral(world,before,run,{pc:0x1002,retired:1,work:1,budget:8,stop:6,
      registers:[7,0,0,0x3000,0x4800,5,0x3102,0x10f2],fp:before.fp,
      writes:[{address:0x10f1,bytes:'22'}],versionWrites:1});
    assert.equal(world.recording().used,0,'SMC completion never creates an obsolete source observation');noGuestCode(world);
  });
  coldFixture(artifact,report,'cold-divide-fault-keeps-completed-prior-prefix',{
    code:'40 f7f1 90',registers:[7,0,0,0x3000,0x4800,5,6,7],
  },world=>{
    const before=world.snapshot(),run=world.execute(3);
    coldLiteral(world,before,run,{pc:0x1001,retired:1,budget:3,stop:9,
      registers:[8,0,0,0x3000,0x4800,5,6,7],flags:2,fp:before.fp,fault:[0,0,0]});
    noGuestCode(world);
  });
  coldFixture(artifact,report,'cold-null-gs-string-fault-before-memory',{
    code:'65a4 90',registers:[7,3,0,0x3000,0x4800,5,0x3100,0x3200],data:source,
  },world=>{
    const before=world.snapshot(),run=world.execute(2);
    coldLiteral(world,before,run,{pc:0x1000,retired:0,budget:2,stop:9,
      registers:[7,3,0,0x3000,0x4800,5,0x3100,0x3200],fp:before.fp,fault:[0,0,13]});
    noGuestCode(world);
  });
  coldFixture(artifact,report,'cold-version-max-keeps-first-rmw-and-rejects-second',{
    code:'830301 830301 90',registers:[7,3,0,0x3000,0x4800,5,6,7],data:d=>d.setUint32(0,5,true),
  },world=>{
    new DataView(world.memory.buffer).setBigUint64(world.word(world.context,80),0xfffffffffffffffen,true);
    const before=world.snapshot(),run=world.executeNative(3);
    coldLiteral(world,before,run,{pc:0x1003,retired:1,budget:3,stop:8,
      registers:[7,3,0,0x3000,0x4800,5,6,7],flags:6,fp:before.fp,
      writes:[{address:0x3000,bytes:'06000000'}],versionWrites:1});
    noGuestCode(world);
  });
  coldFixture(artifact,report,'cold-hot-capture-rejects-source-changed-by-completed-store',{
    code:'49 75fd c6430090 90',registers:[7,96,0,0x1000,0x4800,5,6,7],
  },world=>{
    const before=world.snapshot(),run=world.execute(193);
    coldLiteral(world,before,run,{pc:0x1007,retired:193,budget:193,stop:6,
      registers:[7,0,0,0x1000,0x4800,5,6,7],flags:0x46,fp:before.fp,
      writes:[{address:0x1000,bytes:'90'}],versionWrites:1});
    const recording=world.recording();world.result.recording=recording;
    assert(recording.used>0,'normal heat began a real recording before the source change');
    assert.equal(recording.active,0);assert.equal(recording.overflow,0);
    assert(recording.events.every(event=>event.kind===3&&event.owner==='0'),'recording contains exact completed cold events');
    assert(!recording.events.some(event=>event.pc===0x1003),'SMC store is never fabricated as a stable completed source observation');
    assert.notEqual(world.bigint(world.word(world.context,88)),before.epoch,'actual executable source authority changed');
    assert.equal(world.raw.core_trace_ready(),0,'normal postlease finish consumed and rejected the obsolete source graph');
    noGuestCode(world);
  });
}

export function runColdObserved(artifact,report,{baselineOnepass=false}={}) {
  coldFixture(artifact,report,'one-pass-total-cold-without-guest-wasm',{
    referencePrepared:baselineOnepass,
    code:'b807000000 c1e001 31d2 b903000000 f7f1 0fabc8 0fbaf003 b903000000 f3a4 d8c1 dd5b08 31c9 90 0f0b',
    registers:[0,0,0,0x3000,0x4800,0x12345678,0x3100,0x3200],flags:2,
    data:d=>{[0x11,0x22,0x33].forEach((value,index)=>d.setUint8(0x100+index,value));},
  },world=>{
    const before=world.snapshot(),run=world.execute(15);
    coldLiteral(world,before,run,{pc:0x1027,retired:13,work:15,budget:15,
      registers:[4,0,2,0x3000,0x4800,0x12345678,0x3103,0x3203],flags:0x46,
      writes:[{address:0x3200,bytes:'11'},{address:0x3201,bytes:'22'},{address:0x3202,bytes:'33'},
        {address:0x3008,bytes:'0000000000001440'}],versionWrites:4,
      fpRegisters:{0:'00000000000000a00140'},fpPc:0x1021,fpOpcode:0x55b,fpStatus:0x800,fpTag:0xfff3,fpData:0x3008,fpDs:0x23});
    if(baselineOnepass) {
      world.result.reference={adapter:'explicit cache.entry/core_prepare',prepares:world.prepares,
        guestAdmissions:world.cache.admissions,guestBytes:world.cache.generatedModuleBytes};
      assert.equal(world.cache.admissions,0,'zero guest admissions after the complete literal one-pass oracle');
      assert.equal(world.cache.generatedModuleBytes,0,'zero guest bytes after the complete literal one-pass oracle');
    }
    assert.equal(run.kind,'cold');assert.equal(world.cache.units.size,0);assert.equal(world.cache.admissions,0);
    assert.equal(world.cache.generatedModuleBytes,0);assert.equal(world.moduleRecords.size,0);
    assert.equal(world.result.modules.filter(module=>module.kind==='guest').length,0);
    assert.equal(world.helperCalls.length,0,'cold numeric instructions use their exact decoded owner directly');
    noGuestCode(world);
  });
  if(baselineOnepass)return;
  runColdBoundaryCases(artifact,report);
  const code=new Uint8Array(4096);code.set(unhex('e8fb000000 83c001 49 75f5 90 0f0b'));
  code.set(unhex('d8c1 c3'),0x100);
  coldFixture(artifact,report,'automatic-cold-call-fp-return-one-observed-owner',{
    code,registers:[0,96,0x1005,0x3000,0x4800,0x12345678,0x87654321,0x99aabbcc],flags:2,
  },world=>{
    const before=world.snapshot(),train=world.execute(577),units=[...world.cache.units.values()];
    const unit=units[0];
    coldLiteral(world,before,train,{pc:0x100c,retired:577,budget:577,
      registers:[96,0,0x1005,0x3000,0x4800,0x12345678,0x87654321,0x99aabbcc],flags:0x46,
      writes:[{address:0x47fc,bytes:'05100000',count:96,lastVersion:96}],versionWrites:96,
      fpRegisters:{0:'00000000000000c30640'},fpPc:0x1100,identity:unit?.id??0n});
    assert.equal(units.length,1,'normal cold heat installs one optimized owner without a baseline seed');
    assert(unit.id>0n);assert.equal(world.cache.admissions,1);
    assert.equal(train.kind,'cold','optimization happens after the completed cold lease');
    assert.equal(world.moduleRecords.size,1);assert.equal(world.result.modules.filter(module=>module.kind==='guest').length,1);
    const module=world.moduleRecords.get(unit.id);assert(module);
    world.reset({registers:[0,9,0x1005,0x3000,0x4800,0x12345678,0x87654321,0x99aabbcc]});
    const warmBefore=world.snapshot(),warm=world.execute(54);
    coldLiteral(world,warmBefore,warm,{pc:0x100b,retired:54,budget:54,
      registers:[9,0,0x1005,0x3000,0x4800,0x12345678,0x87654321,0x99aabbcc],flags:0x46,
      writes:[{address:0x47fc,bytes:'05100000',count:9,lastVersion:9}],versionWrites:9,
      fpRegisters:{0:'00000000000000a80340'},fpPc:0x1100,identity:unit.id});
    assert.equal(warm.kind,'compiled');assert.equal(warm.id,unit.id);assert.equal(warm.regions,1,'the actual one body executes every CALL and RET');
    assert.equal(warm.chainedColdWork,0);assert.equal(world.cache.units.size,1);assert.equal(world.cache.admissions,1);
    assert.equal(world.cache.units.get(unit.id),unit,'automatic execution retains the same immutable instance');
    assert.equal(world.moduleRecords.get(unit.id).sha256,module.sha256);assert.equal(world.moduleRecords.size,1);
    assert.equal(world.helperCalls.length,0,'the PC53 loop uses native FP in its registered observed owner');
    const native=world.cache.sampleUsage().nativeCache;
    assert.equal(native.compiled,'1');assert.equal(native.admittedEncodedBytes,String(module.bytes));
    world.result.owner={id:String(unit.id),root:unit.pc,bytes:module.bytes,sha256:module.sha256};
  });
}

if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href) {
  const[enginePath,reportPath,...flags]=process.argv.slice(2),baselineOnepass=flags.includes('--baseline-onepass');
  assert(enginePath&&reportPath&&flags.every(flag=>flag==='--baseline-onepass'),
    'usage: cold_observed.mjs engine.wasm fresh-report.json [--baseline-onepass]');
  const artifact=readEngine(enginePath),sources=['./cold_observed.mjs','../wasm/engine.mjs',
    '../../../../sdk/runtime/engine.js','../../../../sdk/runtime/cache.js'];
  const report={schema:1,complete:false,engineSha256:artifact.sha256,
    mode:baselineOnepass?'baseline-onepass-reference':'normal-cold-frontend',
    sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),
    scope:baselineOnepass?'explicit eager-compiler semantic reference followed by the zero-emission architecture assertion; no performance claim':
      'exact cold-only execution and normal automatic observed compilation; no baseline producer or performance claim',cases:[]};
  closeSync(openSync(reportPath,'wx'));
  try {runColdObserved(artifact,report,{baselineOnepass});report.complete=true;}
  catch(error){report.failure={message:String(error),stack:error.stack};throw error;}
  finally{writeFileSync(reportPath,JSON.stringify(report,null,2));}
  console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,cases:report.cases.length}));
}
