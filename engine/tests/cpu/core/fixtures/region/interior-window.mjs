import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';
import {cacheCallbacks} from '../link.mjs';
import {readGeneratedModule} from './structure.mjs';
import {instrumentWitnessModule,readWitness,setWitnessBank} from './witness.mjs';

const A=0x1000,B=0x1100,P=0x1200,FLAGS=0x403,INITIAL=0x11223344;
const REGS=[INITIAL,512,P,0x3000,0x4800,0x12345678,B,A];
const unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
function integer80(value) {
  const n=BigInt(value),bits=n.toString(2).length,b=Buffer.alloc(10);
  b.writeBigUInt64LE(n<<BigInt(64-bits));b.writeUInt16LE(0x3fff+bits-1,8);return b.toString('hex');
}
function fp(initial,mixed,additions=0,last) {
  const result=initial.slice(),d=new DataView(result.buffer);
  d.setUint16(18,mixed&&additions?0x20:0x220,true);
  if(mixed&&additions) {
    result.set(unhex(integer80(3+2*additions)),40);d.setUint16(22,0xc1,true);
    d.setUint32(24,last,true);d.setUint16(32,0x1b,true);
  }
  return result;
}
const scalar=steps=>steps>=32?0:Number((BigInt(INITIAL)<<BigInt(steps))&0xffffffffn);
function keep(world,unit,ports) {
  const record=world.moduleRecords.get(unit.id);assert(record);
  return{unit,id:unit.id,slot:unit.slot,bytes:unit.bytes,sha256:record.sha256,
    run:unit.run,link:unit.link,publish:unit.publish,ports};
}
function stable(world,parent,skip=[]) {
  assert.equal(world.cache.units.get(parent.id),parent.unit);assert.equal(parent.unit.slot,parent.slot);
  assert.equal(parent.unit.bytes,parent.bytes);assert.equal(world.moduleRecords.get(parent.id).sha256,parent.sha256);
  for(const name of ['run','link','publish'])assert.equal(parent.unit[name],parent[name]);
  assert.equal(world.cache.table.get(parent.slot),parent.link);
  assert.equal(world.cache.publishers.get(parent.slot),parent.publish);
  assert.equal(world.bigint(world.raw.core_link_owners_ptr(),parent.slot*8),parent.id);
  for(const pc of parent.ports)if(!skip.includes(pc)) {
    assert.equal(world.raw.core_lookup(pc),0);assert.equal(world.bigint(world.context,120),parent.id);
    const bytes=world.bytes(world.raw.core_code_ptr(),world.raw.core_code_len());
    assert.equal(bytes.length,parent.bytes);assert.equal(hash(bytes),parent.sha256);
  }
}
function ports(source) {
  const module=readGeneratedModule(source),named=[...module.names].filter(([,name])=>name==='guest.lookup');
  assert.equal(named.length,1);const fn=module.functions[named[0][0]-module.imports.length];assert(fn);
  return fn.ops.flatMap((op,index)=>op.op===0x41&&fn.ops[index-1]?.op===0x20&&
    fn.ops[index-1].local===0&&fn.ops[index+1]?.op===0x46?[op.value>>>0]:[]).sort((a,b)=>a-b);
}
function instrument(world,unit) {
  const source=world.moduleRecords.get(unit.id).source;
  const diagnostic=instrumentWitnessModule(source,readGeneratedModule(source));
  const exports=new WebAssembly.Instance(new WebAssembly.Module(diagnostic.bytes),{ring3:{
    memory:world.memory,read:world.api.core_read,write:world.api.core_write,
    ...cacheCallbacks(world.cache),...world.cache.dispatch,'support.abi':world.cache.dispatch.supportAbi,
  }}).exports;
  Object.assign(unit,{run:exports.run,link:exports.link,publish:exports.publish});
  world.cache.table.set(unit.slot,unit.link);world.cache.publishers.set(unit.slot,unit.publish);
  setWitnessBank(exports,world.word(world.context,92));return()=>readWitness(exports);
}

export function runInteriorWindowCases(artifact,report) {
  const formationChecks=[];
  for(const mixed of [false,true]) {
    const code=new Uint8Array(4096),operation=mixed?'d8c1':'01c0';
    code.set(unhex(operation+' ffe6'),A-A);
    code.set(unhex(operation+' 49 0f44fa ffe7'),B-A);
    code.set(unhex('b900100000 bf00100000 ffe6'),P-A);
    coldFixture(artifact,report,'automatic-interior-cycle-after-one-shot-prefix-'+(mixed?'mixed-fp':'scalar'),{
      code,registers:REGS,flags:FLAGS,
    },world=>{
      world.cache.instructions=2048;world.cache.blocks=128;
      const initial=fp(world.initialFp,mixed),parents=[];
      for(const base of [A,B,P]) {
        const registers=[...REGS];registers[1]=128;
        if(base===A)registers[6]=A;
        if(base===B){registers[2]=B;registers[7]=B;}
        if(base===P)registers[6]=P;
        world.reset({pc:base,registers,flags:FLAGS,fp:initial});const before=world.snapshot();
        const budget=(base===A?2:base===B?4:3)*128,run=world.execute(budget,base);
        const expected=[...registers];
        if(base===B)expected[1]=0;
        if(base===P){expected[1]=4096;expected[7]=A;}
        if(!mixed&&base!==P)expected[0]=0;
        coldLiteral(world,before,run,{pc:base,retired:budget,budget,registers:expected,
          flags:base===P?FLAGS:mixed?(base===A?FLAGS:0x447):0x446,
          fp:fp(initial,mixed,base===P?0:128,base),writes:[],versionWrites:0,
          identity:world.bigint(world.context,120)});
        const unit=world.cache.select(base);assert(unit);assert.equal(world.cache.admissions,parents.length+1);
        const entrypoints=base===A?[A,A+2]:base===B?[B,B+2,B+3,B+6]:[P,P+5,P+10];
        parents.push(keep(world,unit,entrypoints));
      }
      const [parentA,parentB,parentP]=parents;assert.equal(world.cache.units.size,3);
      // Learn only the existing edges. No trace API or counter write is used.
      let registers=[...REGS];registers[1]=1024;
      world.reset({pc:A,registers,flags:FLAGS,fp:initial});let before=world.snapshot(),run=world.execute(54,A);
      let expected=[...registers];expected[1]=1015;if(!mixed)expected[0]=0xcd100000;
      coldLiteral(world,before,run,{pc:A,retired:54,budget:54,registers:expected,flags:mixed?FLAGS:0x402,
        fp:fp(initial,mixed,18,B),writes:[],versionWrites:0,identity:parentB.id});
      registers=[...REGS];registers[1]=1;
      world.reset({pc:B,registers,flags:FLAGS,fp:initial});before=world.snapshot();run=world.execute(7,B);
      expected=[...registers];expected[1]=4096;expected[7]=A;if(!mixed)expected[0]=0x22446688;
      coldLiteral(world,before,run,{pc:B,retired:7,budget:7,registers:expected,flags:mixed?0x447:0x446,
        fp:fp(initial,mixed,1,B),writes:[],versionWrites:0,identity:parentP.id});
      assert.equal(world.cache.admissions,3);
      const pointer=world.raw.core_trace_pointer()>>>0,countdown=world.word(pointer,32+24*129);
      assert(countdown>=1&&countdown<=1024);
      const start=countdown%2===0?A:B,visits=Math.ceil(countdown/2),odd=Number(start===B);
      registers=[...REGS];registers[1]=visits;
      const prefixWork=6*visits-2*odd,budget=prefixWork+3+120;
      const descriptor=Object.getOwnPropertyDescriptor(world.cache,'installRecordedTrace');
      const original=world.cache.installRecordedTrace,attempts=[];
      Object.defineProperty(world.cache,'installRecordedTrace',{configurable:true,writable:true,value:function(...args) {
        if(world.raw.core_trace_ready()===1)attempts.push({root:world.raw.core_trace_root(),...world.recording()});
        return Reflect.apply(original,this,args);
      }});
      try {
        world.reset({pc:start,registers,flags:FLAGS,fp:initial});before=world.snapshot();run=world.execute(budget,start);
      } finally {
        if(descriptor)Object.defineProperty(world.cache,'installRecordedTrace',descriptor);
        else delete world.cache.installRecordedTrace;
      }
      expected=[...registers];expected[1]=4076;expected[7]=A;if(!mixed)expected[0]=0;
      coldLiteral(world,before,run,{pc:B,retired:budget,budget,registers:expected,flags:mixed?FLAGS:0x446,
        fp:fp(initial,mixed,2*visits-odd+40,A),writes:[],versionWrites:0,
        identity:world.bigint(world.context,120)});
      assert.equal(run.id,parentA.id);assert.equal(run.regions,2*visits-odd+41);
      world.result.trainingSemanticComplete=true;
      assert.equal(attempts.length,1);const capture=attempts[0];
      assert.equal(capture.root,P,'the actual generated sampler selects the one-shot prefix');
      assert.equal(capture.active,0);assert.equal(capture.overflow,0);assert(capture.used<=129);
      assert.equal(capture.events.filter(event=>event.pc===P).length,1,'the selected capture root never returns');
      assert(capture.events.every(event=>event.kind===1));
      assert.deepEqual(capture.events.slice(1,6).map(event=>event.pc),[B,A,B,A,B],
        'two completed cross-owner cycles are present inside the same bounded timeline');
      world.result.capture={countdown,start,visits,root:P,used:capture.used,events:capture.events};
      const windows=[...world.cache.units.values()].filter(unit=>!parents.some(parent=>parent.id===unit.id));
      world.result.windowCandidateCount=windows.length;
      if(windows.length!==1) {
        // OLD semantic evidence must cover both scalar and mixed formation
        // before reporting the missing optimization. New-only replay needs it.
        world.result.windowMechanismDeferred=true;
        formationChecks.push(()=>assert.equal(windows.length,1,
          'the already-hot interior cycle gains one automatic window despite the one-shot selected root'));
        return;
      }
      const window=windows[0],record=world.moduleRecords.get(window.id);
      assert.equal(window.pc,B,'the earliest completed interior cycle is selected deterministically');
      assert.deepEqual(ports(record.source),[A,A+2,B,B+2,B+3,B+6]);
      const counter=instrument(world,window),traffic=[];
      const admissions=world.cache.admissions,generated=world.cache.generatedModuleBytes;
      assert.equal(admissions,4);assert.equal(world.cache.units.size,4);
      for(const count of [9,129]) {
        registers=[...REGS];registers[1]=4096;
        world.reset({pc:B,registers,flags:FLAGS,fp:initial});const testBefore=world.snapshot(),old=counter();
        const warm=world.execute(6*count,B),after=counter();
        expected=[...registers];expected[1]=4096-count;if(!mixed)expected[0]=scalar(2*count);
        coldLiteral(world,testBefore,warm,{pc:B,retired:6*count,budget:6*count,registers:expected,
          flags:mixed?(count===9?FLAGS:0x413):count===9?0xc86:0x446,
          fp:fp(initial,mixed,2*count,A),writes:[],versionWrites:0,identity:window.id});
        assert.equal(warm.id,window.id);assert.equal(warm.regions,1);assert.equal(after.body_entries-old.body_entries,1);
        assert.equal(world.resolverCalls,testBefore.resolverCalls);assert.equal(warm.chainedColdWork,0);
        traffic.push(Object.fromEntries(['record_reads','record_writes'].map(name=>[name,after[name]-old[name]])));
      }
      assert.deepEqual(traffic[0],traffic[1],'more loop iterations add no fragment FP imports or spills');
      for(const fuel of [0,1]) {
        registers=[...REGS];registers[1]=4096;
        world.reset({pc:B,registers,flags:FLAGS,fp:initial});const prefixBefore=world.snapshot();
        const prefix=world.execute(fuel,B);expected=[...registers];if(!mixed&&fuel)expected[0]=0x22446688;
        coldLiteral(world,prefixBefore,prefix,{pc:B+(fuel?2:0),retired:fuel,budget:fuel,registers:expected,
          flags:!mixed&&fuel?0x406:FLAGS,fp:fp(initial,mixed,fuel,B),writes:[],versionWrites:0,identity:window.id});
      }
      for(const parent of parents)stable(world,parent,[B]);
      registers=[...REGS];registers[1]=4096;
      world.reset({pc:A,registers,flags:FLAGS,fp:initial});const selected=world.cache.select(B);
      assert.equal(selected,window);const wrongBefore=world.snapshot();world.cache.budget=3;
      const wrong=world.cache.execute(selected);world.result.runs.push({...wrong,id:String(wrong.id)});
      coldLiteral(world,wrongBefore,wrong,{pc:A,retired:0,work:0,budget:3,stop:1,registers,flags:FLAGS,
        fp:initial,writes:[],versionWrites:0,identity:window.id});
      assert.equal(world.raw.core_lookup(B),0);assert.equal(world.bigint(world.context,120),parentB.id);
      assert.equal(world.raw.core_lookup(P),0);assert.equal(world.bigint(world.context,120),parentP.id);
      world.reset({pc:P,registers,flags:FLAGS,fp:initial});before=world.snapshot();run=world.executeSource(1,P);
      expected=[...registers];expected[1]=4096;
      coldLiteral(world,before,run,{pc:P+5,retired:1,budget:1,registers:expected,flags:FLAGS,
        fp:initial,writes:[],versionWrites:0,identity:parentP.id});
      const staleBefore=world.snapshot(),wantedRam=staleBefore.ram.slice();
      wantedRam[world.physical(A)-world.word(world.context,64)]=0x90;
      world.engine.ok('write8',A,0x90);assert.deepEqual(world.snapshot().ram,wantedRam);
      assert.equal(world.revision(),staleBefore.revision+1n);
      // CodeSnapshot authority is page-granular. All three original owners
      // and the window depend on this same page, even though their PCs differ.
      assert.equal(A>>>12,B>>>12);assert.equal(A>>>12,P>>>12);
      const invalidated=world.snapshot();
      for(const pc of [...new Set(parents.flatMap(parent=>parent.ports))])
        assert.equal(world.raw.core_lookup(pc),4,'every alias on the changed source page is revoked');
      const afterInvalidation=world.snapshot();
      for(const field of ['context','state','fp','simd','ymm','ram','pages','revision','epoch'])
        assert.deepEqual(afterInvalidation[field],invalidated[field],
          'stale lookup performs no guest work or architectural publication: '+field);
      const owners=world.raw.core_link_owners_ptr();
      for(const owner of [window,...parents])assert.equal(world.bigint(owners,owner.slot*8),0n);
      world.result.sourceInvalidation={granularity:'page',page:A>>>12,
        identities:[window,...parents].map(owner=>String(owner.id)),aliases:parents.flatMap(parent=>parent.ports)};
      assert.equal(world.cache.admissions,admissions);assert.equal(world.cache.generatedModuleBytes,generated);
      assert.equal(world.helperCalls.length,0);world.result.semanticComplete=true;
      world.result.window={id:String(window.id),root:B,bytes:window.bytes,sha256:record.sha256,traffic};
    });
  }
  for(const check of formationChecks)check();
}
