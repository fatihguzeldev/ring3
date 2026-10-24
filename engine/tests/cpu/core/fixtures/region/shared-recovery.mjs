import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';

const ROOT=0x1000,OTHER=0x1200,TARGET=0x1800,FLAGS=0x403;
const ONE_HALF='00000000000000c0ff3f',FULL='01000000000000c00040';
const ghosts={0:FULL,1:'00000000000000000080',2:'0000000000000040ff3f',
  3:'00000000000000c0ff7f',4:'7856341200000080ff3f',5:'ffffffffffffffffffff',
  6:'efcdab8967452301807f',7:'ffffffffffffffffffff'};
const registers=[0x7fffffc0,0x11223344,0x55667788,0x3000,0x4800,0x13579bdf,TARGET,0x5000];
const unhex=value=>Uint8Array.from(Buffer.from(value.replaceAll(' ',''),'hex'));
const sha=value=>createHash('sha256').update(value).digest('hex');
const dword=value=>{const b=Buffer.alloc(4);b.writeUInt32LE(value>>>0);return b.toString('hex');};

function fpImage(initial,{control=0x27f,status=0,tag=0xffff,opcode=0x5a5,
  fip=0x11223344,fdp=0x55667788,cs=0x33,ds=0x5b,payloads=ghosts}={}) {
  const fp=initial.slice(),v=new DataView(fp.buffer);
  for(const[offset,value]of[[16,control],[18,status],[20,tag],[22,opcode],[32,cs],[34,ds]])v.setUint16(offset,value,true);
  v.setUint32(24,fip,true);v.setUint32(28,fdp,true);
  for(const[index,value]of Object.entries(payloads))fp.set(unhex(value),40+10*Number(index));
  return fp;
}

function codePage(segments) {
  const page=new Uint8Array(4096);
  for(const[pc,code]of segments)page.set(unhex(code),pc-ROOT);
  return page;
}

// the spy forwards the actual shared wasm function; it supplies no cpu result.
function observeShared(world,body,{beforeCall,afterCall}={}) {
  const descriptor=Object.getOwnPropertyDescriptor(WebAssembly,'Instance'),Original=WebAssembly.Instance;
  const owners=new Map(),calls=[],callbacks=[];
  const Spy=new Proxy(Original,{construct(target,args){
    const[module,imports]=args,bindings=imports?.ring3,fn=bindings?.['support.recover'];
    if(typeof fn!=='function'||!(bindings['recovery.scratch'] instanceof WebAssembly.Global))
      return Reflect.construct(target,args,target);
    const id=world.bigint(world.context,120),items=WebAssembly.Module.imports(module);
    assert(id>0n,'the actual selected immutable owner supplies the imported helper');
    owners.set(id,{id,fn,imports:items,scratch:bindings['recovery.scratch'],blob:bindings['recovery.base']});
    const wrapped=(...input)=>{
      const row={id:String(id),input:input.map(value=>value>>>0)};calls.push(row);
      beforeCall?.({id,fn,input,row});
      row.returned=Reflect.apply(fn,undefined,input)>>>0;
      afterCall?.({id,fn,input,row});return row.returned;
    };
    const guarded={...bindings,'support.recover':wrapped};
    for(const name of['numeric','precise'])if(typeof bindings[name]==='function')guarded[name]=(...input)=>{
      const scratch=bindings['recovery.scratch'].value>>>0;
      assert.equal(world.word(scratch,48),0,'no recovery packet is live across an actual native callback');
      callbacks.push({id:String(id),name});return Reflect.apply(bindings[name],undefined,input);
    };
    return Reflect.construct(target,[module,{...imports,ring3:guarded}],target);
  }});
  try {Object.defineProperty(WebAssembly,'Instance',{...descriptor,value:Spy});return body({owners,calls,callbacks});}
  finally {Object.defineProperty(WebAssembly,'Instance',descriptor);}
}

function execute(world,{pc=ROOT,input=registers,fp,budget,wanted,beforeRun}) {
  world.reset({pc,registers:input,flags:FLAGS,fp});
  world.cache.budget=budget;
  const owner=world.cache.entry(pc);assert(owner,'explicit reference preparation produces the real authored owner');
  const source=world.moduleRecords.get(owner.id);assert(source);
  const before=world.snapshot(),afterRun=beforeRun?.(owner),run=world.execute(budget,pc);
  coldLiteral(world,before,run,{...wanted,budget,identity:owner.id});
  afterRun?.();
  assert.equal(world.cache.units.get(owner.id),owner,'the completed prefix retains the same immutable owner');
  assert.equal(world.moduleRecords.get(owner.id),source);assert.equal(sha(source.source),source.sha256);
  if(run.coreReason===3) {
    const sealed=world.snapshot();
    world.engine.ok('take_cpu_event');
    const consumed=world.snapshot();
    for(const field of['context','state','fp','simd','ymm','ram','pages'])assert.deepEqual(consumed[field],sealed[field],
      `consuming the authentic completed fault event changes no ${field}`);
    assert.equal(consumed.revision,sealed.revision);assert.equal(consumed.epoch,sealed.epoch);
  }
  world.result.semanticComplete=true;
  return{owner,run,before,source};
}

function rounded(count,single) {
  const raw=Buffer.alloc(10),stored=Buffer.alloc(8),n=single?0n:BigInt(count);
  raw.writeBigUInt64LE(0xc000000000000000n+(n<<39n));raw.writeUInt16LE(0x3fff,8);
  stored.writeBigUInt64LE(0x3ff8000000000000n+(n<<28n));
  return{raw:raw.toString('hex'),stored:stored.toString('hex')};
}

function sealedRecoveryState(world,scratch) {
  const header=world.raw.core_continuation_ptr()>>>0,pool=world.word(scratch,132),bank=world.word(scratch,128);
  return{snapshot:world.snapshot(),header,route:world.bytes(header,136),pool,bank,
    poolBytes:pool?world.bytes(pool,384):null,bankBytes:bank?world.bytes(bank,100):null};
}

function assertRecoveryNeutral(world,before) {
  const after=world.snapshot();
  for(const field of['context','state','fp','simd','ymm','ram','pages'])assert.deepEqual(after[field],before.snapshot[field]);
  assert.equal(after.revision,before.snapshot.revision);assert.equal(after.epoch,before.snapshot.epoch);
  assert.deepEqual(world.bytes(before.header,136),before.route,'rejection changes no continuation authority or work');
  if(before.poolBytes)assert.deepEqual(world.bytes(before.pool,384),before.poolBytes,'rejection changes no private physical pool');
  if(before.bankBytes)assert.deepEqual(world.bytes(before.bank,100),before.bankBytes,'rejection changes no canonical numeric prefix');
}

function probeLiveRequest(world,fn,input) {
  const scratch=input[3]>>>0,header=world.raw.core_continuation_ptr()>>>0,v=new DataView(world.memory.buffer),rows=[];
  const probe=(name,args=input,change)=>{
    const restore=change?.();
    try {
      const sealed=sealedRecoveryState(world,scratch),returned=Reflect.apply(fn,undefined,args)>>>0;
      assert.equal(returned,3,`${name} rejects rather than trapping or producing an output`);
      assertRecoveryNeutral(world,sealed);rows.push({name,returned});
    } finally {restore?.();}
  };
  const word=(pointer,offset,value)=>()=>{const old=v.getUint32(pointer+offset,true);v.setUint32(pointer+offset,value>>>0,true);
    return()=>v.setUint32(pointer+offset,old,true);};
  const wide=(pointer,offset)=>()=>{const old=v.getBigUint64(pointer+offset,true);v.setBigUint64(pointer+offset,old^1n,true);
    return()=>v.setBigUint64(pointer+offset,old,true);};
  for(const[name,index,value]of[['wrong-context',0,0],['poisoned-blob',1,0xfffffff0],
    ['absent-site',2,0],['minimum-signed-site',2,0x80000000],['poisoned-scratch',3,0xfffffff0]]) {
    const args=input.slice();args[index]=value;probe(name,args);
  }
  probe('wrong-packet-owner',input,wide(scratch,56));
  probe('wrong-packet-slot',input,word(scratch,64,world.word(scratch,64)^1));
  probe('wrong-packet-generation',input,wide(scratch,72));
  probe('wrong-packet-nonce',input,wide(scratch,80));
  probe('inactive-continuation',input,word(header,4,0));
  probe('stale-continuation-nonce',input,wide(header,104));
  const unaligned=input.slice();unaligned[2]=Math.sign(input[2])*(Math.abs(input[2])+1);
  probe('non-descriptor-site',unaligned,word(scratch,92,unaligned[2]));
  const outside=input.slice();outside[2]=0x7fffffff;
  probe('outside-descriptor-directory',outside,word(scratch,92,outside[2]));
  const poisoned=input.slice();poisoned[1]=0xfffffff0;
  probe('matching-poisoned-blob',poisoned,word(scratch,88,poisoned[1]));
  return rows;
}

function poisonUninitializedCells(world,scratch) {
  const v=new DataView(world.memory.buffer),epoch=v.getUint32(scratch+52,true);let poisoned=0;
  for(let color=0;color<8192;color++) {
    const at=scratch+1024+color*16;
    if(v.getUint32(at+12,true)!==epoch||v.getInt32(at+8,true)===0) {
      v.setBigUint64(at,0x7ff8123456780000n|BigInt(color),true);poisoned++;
    }
  }
  return poisoned;
}

export function runSharedRecoveryCases(artifact,report) {
  const checks=[];
  const diamonds=count=>`d903 ${'85c0 7405 d84304 eb03 d86304 '.repeat(count)}dd1f ffe6`;
  coldFixture(artifact,report,'shared-recovery-two-real-owners-and-completed-control',{
    code:codePage([[ROOT,diamonds(16)],[OTHER,diamonds(32)]]),referencePrepared:true,
    data:v=>{v.setUint32(0,0x3fc00000,true);v.setUint32(4,0x33800000,true);},
  },world=>observeShared(world,({owners,calls})=>{
    world.cache.instructions=2048;world.cache.blocks=512;
    const retained=[];
    for(const[pc,count,single]of[[ROOT,16,true],[OTHER,32,false]]) {
      const initial=fpImage(world.initialFp,{control:single?0x7f:0x27f}),value=rounded(count,single),
        input=registers.slice(),store=pc+2+12*count,lastAdd=pc+6+12*(count-1),completed=4*count+1;
      const after=fpImage(initial,{control:single?0x7f:0x27f,status:0x3800+(single?0x20:0),tag:0x3fff,
        opcode:0x43,fip:lastAdd,fdp:0x3004,cs:0x1b,ds:0x23,payloads:{7:value.raw}});
      const first=calls.length,result=execute(world,{pc,input,fp:initial,budget:completed+2,wanted:{
        pc:store,retired:completed,work:completed,stop:3,registers:input,flags:0x406,fp:after,
        fault:[0x5000,0x10208,0x80e]}});
      retained.push({...result,pc,count,single,calls:calls.length-first});
    }
    assert.notEqual(retained[0].owner.id,retained[1].owner.id,'two independent actual owner identities');
    const first=retained[0],value=rounded(16,true),input=registers.slice();input[7]=0x3008;
    const beforeCalls=calls.length,initial=fpImage(world.initialFp,{control:0x7f});
    let normalCells=false;
    const normal=execute(world,{input,fp:initial,budget:67,beforeRun:owner=>{
      const scratch=owners.get(owner.id)?.scratch;if(scratch===undefined)return;
      assert(scratch instanceof WebAssembly.Global,'the actual immutable scratch import supplies the private packet');
      const pointer=scratch.value>>>0;assert(pointer>0&&pointer%8===0&&pointer+132096<=world.memory.buffer.byteLength);
      const captured=world.bytes(pointer+1024,8192*16);
      return()=>{assert.deepEqual(world.bytes(pointer+1024,captured.length),captured,
        'ordinary completed execution never writes capture cells');normalCells=true;};
    },wanted:{pc:TARGET,retired:67,work:67,
      stop:2,registers:input,flags:0x406,writes:[{address:0x3008,bytes:value.stored}],
      fp:fpImage(initial,{control:0x7f,status:0x20,opcode:0x51f,fip:0x10c2,fdp:0x3008,
        cs:0x1b,ds:0x23,payloads:{7:value.raw}})}});
    assert.equal(normal.owner.id,first.owner.id,'the ordinary completed control reuses the very same owner');
    world.result.sharedRecovery={owners:retained.map(row=>({id:String(row.owner.id),pc:row.pc,
      bytes:row.source.bytes,sha256:row.source.sha256,calls:row.calls})),normalCalls:calls.length-beforeCalls,normalCells,calls};
    checks.push(()=>{
      for(const row of retained) {
        const binding=owners.get(row.owner.id);
        assert(binding,'each authored costly owner imports the shared recovery executor');
        assert.deepEqual(binding.imports.filter(item=>item.name==='support.recover'),
          [{module:'ring3',name:'support.recover',kind:'function'}]);
        assert.deepEqual(binding.imports.filter(item=>item.name==='recovery.scratch'),
          [{module:'ring3',name:'recovery.scratch',kind:'global'}]);
        assert(row.calls>0,'the actual generic fault recovery reaches that shared helper');
      }
      assert.equal(owners.get(retained[0].owner.id).fn,owners.get(retained[1].owner.id).fn,
        'both real owners call the identical per-CPU wasm executor');
      assert.equal(calls.length-beforeCalls,0,'ordinary completed handoff performs no shared recovery or capture');
      assert(normalCells,'the completed control checked every actual private capture cell');
      assert(calls.every(call=>call.returned<=2),'every actual shared call completes or requests one of the two capture phases');
    });
  }));

  const scalar='83f801 ba00000000 '+Array.from({length:31},(_,i)=>`8d4001 8b8b${dword(i*4)}`).join(' ')+' 8d4001 8b0f ffe6';
  coldFixture(artifact,report,'shared-recovery-old-flags-and-colored-ordered-read-lifetimes',{
    code:scalar,referencePrepared:true,data:v=>{for(let i=0;i<1024;i++)v.setUint32(i*4,0x89abcd00+i,true);},
  },world=>{
    const initial=fpImage(world.initialFp,{status:0x1220,tag:0xff2f}),output=registers.slice();
    output[0]=0x7fffffe0;output[1]=0x89abcd1e;output[2]=0;
    execute(world,{fp:initial,budget:67,wanted:{pc:0x1122,retired:65,work:65,stop:3,
      registers:output,flags:0x412,fp:initial,fault:[0x5000,0x10104,14]}});
  });

  coldFixture(artifact,report,'shared-recovery-raw80-valid-zero-alias-and-empty-ghosts',{
    code:'d9c0 d9e0 d9c9 8b17 ffe6',referencePrepared:true,
  },world=>{
    const initial=fpImage(world.initialFp,{status:0x200,tag:0xfff4});
    execute(world,{fp:initial,budget:5,wanted:{pc:0x1006,retired:3,work:3,stop:3,
      registers,flags:FLAGS,fault:[0x5000,0x10104,14],
      fp:fpImage(initial,{status:0x3800,tag:0x3ff4,opcode:0x1c9,fip:0x1004,
        cs:0x1b,payloads:{0:'01000000000000c000c0',7:FULL}})}});
  });

  coldFixture(artifact,report,'shared-recovery-pending-c1-and-current-cw-before-fault',{
    code:`d903 ${'d84304 '.repeat(16)}d96b08 8b17 ffe6`,referencePrepared:true,
    data:v=>{v.setUint32(0,0x3fc00000,true);v.setUint32(4,0x24800000,true);v.setUint16(8,0x7f,true);},
  },world=>{
    const initial=fpImage(world.initialFp);
    execute(world,{fp:initial,budget:20,wanted:{pc:0x1035,retired:18,work:18,stop:3,
      registers,flags:FLAGS,fault:[0x5000,0x10104,14],
      fp:fpImage(initial,{control:0x7f,status:0x3820,tag:0x3fff,opcode:0x43,
        fip:0x102f,fdp:0x3004,cs:0x1b,ds:0x23,payloads:{7:ONE_HALF}})}});
  });

  coldFixture(artifact,report,'shared-recovery-dirty-fp-and-partial-rep-store-fault',{
    code:'d903 f3a4 ffe6',referencePrepared:true,
    data:v=>{v.setUint32(0,0x3fc00000,true);v.setUint8(0x100,0x11);v.setUint8(0x101,0x22);},
  },world=>{
    const input=registers.slice();input[1]=3;input[6]=0x3100;input[7]=0x3000;
    const output=input.slice();output[1]=2;output[6]--;output[7]--;
    const initial=fpImage(world.initialFp);
    execute(world,{input,fp:initial,budget:5,wanted:{pc:0x1002,retired:1,work:2,stop:3,
      registers:output,flags:FLAGS,fault:[0x2fff,0x10201,0x10e],writes:[{address:0x3000,bytes:'11'}],
      fp:fpImage(initial,{status:0x3800,tag:0x3fff,opcode:0x103,fip:ROOT,fdp:0x3000,
        cs:0x1b,ds:0x23,payloads:{7:ONE_HALF}})}});
  });

  coldFixture(artifact,report,'shared-recovery-completed-smc-keeps-held-owner-prefix',{
    code:'d903 c60790 ffe6',referencePrepared:true,data:v=>v.setUint32(0,0x3fc00000,true),
  },world=>{
    const input=registers.slice();input[7]=0x10f0;const initial=fpImage(world.initialFp);
    execute(world,{input,fp:initial,budget:3,wanted:{pc:0x1005,retired:2,work:2,stop:6,
      registers:input,flags:FLAGS,writes:[{address:0x10f0,bytes:'90'}],
      fp:fpImage(initial,{status:0x3800,tag:0x3fff,opcode:0x103,fip:ROOT,fdp:0x3000,
        cs:0x1b,ds:0x23,payloads:{7:ONE_HALF}})}});
    const sealed=world.snapshot();assert.equal(world.raw.core_begin(),4,'completed stale source cannot grant a fresh lease');
    const rejected=world.snapshot();
    for(const field of['context','state','fp','simd','ymm','ram','pages'])assert.deepEqual(rejected[field],sealed[field]);
    assert.equal(rejected.revision,sealed.revision);
  });

  // all original full-state controls finish before the old representation fails.
  for(const check of checks)check();

  coldFixture(artifact,report,'shared-recovery-live-ticket-rejections-and-poisoned-unused-cells',{
    code:diamonds(16),referencePrepared:true,
    data:v=>{v.setUint32(0,0x3fc00000,true);v.setUint32(4,0x33800000,true);},
  },world=>{
    let probed=false,last,poisoned=0;const probes=[];
    observeShared(world,({owners,calls,callbacks})=>{
      world.cache.instructions=2048;world.cache.blocks=512;
      const initial=fpImage(world.initialFp,{control:0x7f});
      const result=execute(world,{fp:initial,budget:67,wanted:{pc:0x10c2,retired:65,work:65,stop:3,
        registers,flags:0x406,fault:[0x5000,0x10208,0x80e],
        fp:fpImage(initial,{control:0x7f,status:0x3820,tag:0x3fff,opcode:0x43,
          fip:0x10ba,fdp:0x3004,cs:0x1b,ds:0x23,payloads:{7:ONE_HALF}})}});
      assert(probed&&calls.length>0&&poisoned>0,'the actual shared request consumed the adversarial controls');
      assert(owners.has(result.owner.id));assert(callbacks.some(row=>row.name==='precise'));
      const inactive=sealedRecoveryState(world,last.input[3]>>>0),returned=Reflect.apply(last.fn,undefined,last.input)>>>0;
      assert.equal(returned,3,'the ended lease rejects the identical former request');assertRecoveryNeutral(world,inactive);
      probes.push({name:'ended-lease',returned});world.result.adversarialRecovery={probes,poisoned,calls,callbacks};
    },{beforeCall:({fn,input})=>{
      last={fn,input:input.slice()};
      if(!probed){probes.push(...probeLiveRequest(world,fn,input));probed=true;}
      poisoned+=poisonUninitializedCells(world,input[3]>>>0);
    }});
  });

  coldFixture(artifact,report,'shared-recovery-idle-before-canonical-and-precise-services',{
    code:`d903 ${'85c0 7405 d84304 eb03 d86304 '.repeat(16)}dbe2 8b0f ffe6`,referencePrepared:true,
    data:v=>{v.setUint32(0,0x3fc00000,true);v.setUint32(4,0x33800000,true);},
  },world=>observeShared(world,({calls,callbacks})=>{
    world.cache.instructions=2048;world.cache.blocks=512;const initial=fpImage(world.initialFp,{control:0x7f});
    execute(world,{fp:initial,budget:68,wanted:{pc:0x10c4,retired:66,work:66,stop:3,
      registers,flags:0x406,fault:[0x5000,0x10104,14],
      fp:fpImage(initial,{control:0x7f,status:0x3800,tag:0x3fff,opcode:0x43,
        fip:0x10ba,fdp:0x3004,cs:0x1b,ds:0x23,payloads:{7:ONE_HALF}})}});
    assert(calls.length>0);assert.equal(callbacks.filter(row=>row.name==='numeric').length,1);
    world.result.callbackRecovery={calls,callbacks};
  }));

  coldFixture(artifact,report,'shared-recovery-import-throw-releases-packet-and-next-lease',{
    code:diamonds(16),referencePrepared:true,
    data:v=>{v.setUint32(0,0x3fc00000,true);v.setUint32(4,0x33800000,true);},
  },world=>{
    let injected=false,scratch=0;
    observeShared(world,({calls})=>{
      world.cache.instructions=2048;world.cache.blocks=512;const initial=fpImage(world.initialFp,{control:0x7f});
      world.reset({registers,flags:FLAGS,fp:initial});world.cache.budget=67;assert(world.cache.entry(ROOT));
      assert.throws(()=>world.execute(67,ROOT),error=>error?.message==='authored-recovery-import-throw');
      assert(injected);assert.equal(world.cache.open,false,'SDK finally closes the failed actual lease');
      assert.equal(world.word(scratch,48),0);assert.equal(world.bigint(scratch,32),0n);assert.equal(world.bigint(scratch,152),0n);
      const valid=execute(world,{fp:initial,budget:67,wanted:{pc:0x10c2,retired:65,work:65,stop:3,
        registers,flags:0x406,fault:[0x5000,0x10208,0x80e],
        fp:fpImage(initial,{control:0x7f,status:0x3820,tag:0x3fff,opcode:0x43,
          fip:0x10ba,fdp:0x3004,cs:0x1b,ds:0x23,payloads:{7:ONE_HALF}})}});
      assert(valid.run.retired===65);world.result.throwCleanup={injected,calls,packetIdle:world.word(scratch,48)===0};
    },{afterCall:({input,row})=>{
      if(!injected&&row.returned===1){injected=true;scratch=input[3]>>>0;throw new Error('authored-recovery-import-throw');}
    }});
  });
}
