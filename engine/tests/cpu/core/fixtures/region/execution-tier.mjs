import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';
import {cacheCallbacks} from '../link.mjs';
import {readGeneratedModule} from './structure.mjs';

const ROOT=0x1000,FLAGS=0x403,INITIAL=0x11223344;
const PCS=Array.from({length:7},(_,index)=>ROOT+index*0x20);
const REGS=[INITIAL,0x76543210,0x89abcdef,0x3000,0x4800,0x24681357,0xfedcba98,0x55aa33cc];
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');

function lookupPorts(source) {
  const module=readGeneratedModule(source);
  const named=[...module.names].filter(([,name])=>name==='guest.lookup');
  assert.equal(named.length,1);
  const fn=module.functions[named[0][0]-module.imports.length];assert(fn);
  const pcs=fn.ops.flatMap((op,index)=>op.op===0x41&&fn.ops[index-1]?.op===0x20&&
    fn.ops[index-1].local===0&&fn.ops[index+1]?.op===0x46?[op.value>>>0]:[]).sort((a,b)=>a-b);
  assert.equal(new Set(pcs).size,pcs.length,'the actual lookup contains unique instruction ports');
  return pcs;
}

function remember(world,unit) {
  const record=world.moduleRecords.get(unit.id);assert(record);
  return{unit,id:unit.id,slot:unit.slot,bytes:unit.bytes,sha256:record.sha256,
    run:unit.run,link:unit.link,publish:unit.publish};
}

function unchanged(world,parent) {
  assert.equal(world.cache.units.get(parent.id),parent.unit,'optimization retains the immutable source owner');
  assert.equal(parent.unit.slot,parent.slot);assert.equal(parent.unit.bytes,parent.bytes);
  assert.equal(world.moduleRecords.get(parent.id).sha256,parent.sha256);
  for(const name of ['run','link','publish'])assert.equal(parent.unit[name],parent[name]);
  assert.equal(world.cache.table.get(parent.slot),parent.link);
  assert.equal(world.cache.publishers.get(parent.slot),parent.publish);
  assert.equal(world.bigint(world.raw.core_link_owners_ptr(),parent.slot*8),parent.id);
}

function sumOwners(start,count) {
  const full=Math.floor(count/PCS.length),tail=count%PCS.length;
  let result=full*28;
  for(let index=0;index<tail;index++)result+=(start+index)%PCS.length+1;
  return result;
}

function uploadTargets(world,targets) {
  const bytes=new Uint8Array(4096),view=new DataView(bytes.buffer);
  targets.forEach((pc,index)=>view.setUint32(index*4,pc,true));
  world.cache.publish();world.engine.request(bytes);world.engine.ok('upload',0x3000,4096);
}

function countdown(world) {
  const pointer=world.raw.core_trace_pointer()>>>0;
  const value=world.word(pointer,32+24*129);
  assert(value>=1&&value<=1024,'the existing generated sampler has a bounded positive countdown');
  return value;
}

function completedOwners(world,identities) {
  assert.equal(world.raw.core_continuation_format(),2);
  const header=world.raw.core_continuation_ptr()>>>0;
  assert(header>0&&header%8===0);
  assert.equal(world.word(header),2);assert.equal(world.word(header,4),0);
  const owners=world.word(header,36),length=world.word(header,40),found=new Map();
  assert(owners>0&&owners+48*length<=world.memory.buffer.byteLength);
  for(let index=0;index<length;index++) {
    const address=owners+48*index,id=world.bigint(address);
    if(identities.includes(id))found.set(id,world.bigint(address,40));
  }
  assert.equal(found.size,identities.length);
  return identities.map(id=>found.get(id));
}

function pcZeroAndAccounting(artifact,report,deferred) {
  coldFixture(artifact,report,'legal-pc-zero-and-authenticated-completed-owner-work',{
    code:Uint8Array.from([0x8d,0x40,2,0xff,0x63,4]),registers:REGS,flags:FLAGS,
    data:view=>{view.setUint32(0,0,true);view.setUint32(4,ROOT,true);},
  },world=>{
    const zeroCode=new Uint8Array(4096);zeroCode.set([0x8d,0x40,1,0xff,0x63,0]);
    world.engine.ok('map',0,1,7);world.engine.request(zeroCode);world.engine.ok('upload',0,4096);
    const parents=[];
    for(const pc of [0,ROOT]) {
      world.reset({pc,registers:REGS,flags:FLAGS});
      const before=world.snapshot(),run=world.execute(256,pc),registers=[...REGS];
      registers[0]=(INITIAL+128*(pc===0?1:2))>>>0;
      coldLiteral(world,before,run,{pc,retired:256,budget:256,registers,flags:FLAGS,
        fp:world.initialFp,writes:[],versionWrites:0,identity:world.bigint(world.context,120)});
      const unit=world.cache.select(pc);assert(unit);parents.push(remember(world,unit));
    }
    assert.equal(world.cache.admissions,2);assert.notEqual(parents[0].id,parents[1].id);
    uploadTargets(world,[ROOT,0]);
    const format=world.raw.core_continuation_format();
    assert(format===1||format===2,'only negotiated continuation formats are exercised');
    const identities=parents.map(parent=>parent.id),progress=[];
    for(const fuel of [0,4,0,4]) {
      world.reset({pc:0,registers:REGS,flags:FLAGS});
      const counters=format===2?completedOwners(world,identities):null;
      const before=world.snapshot(),run=world.execute(fuel,0),registers=[...REGS];
      registers[0]=(INITIAL+(fuel?3:0))>>>0;
      coldLiteral(world,before,run,{pc:0,retired:fuel,budget:fuel,registers,flags:FLAGS,
        fp:world.initialFp,writes:[],versionWrites:0,identity:parents[fuel?1:0].id});
      assert.equal(run.regions,fuel?2:1,'PC zero is a real owner entry and exact zero fuel performs no guest work');
      if(counters) {
        const after=completedOwners(world,identities),delta=after.map((value,index)=>value-counters[index]);
        progress.push({fuel,delta:delta.map(String)});
        assert.deepEqual(delta,fuel?[2n,2n]:[0n,0n],
          'only actual completed instructions earn owner work, once across fast routes and final end');
      }
    }
    for(const parent of parents)unchanged(world,parent);
    assert.equal(world.cache.admissions,2);assert.equal(world.helperCalls.length,0);
    world.result.semanticComplete=true;world.result.ownerWork=progress;
    if(format!==2)deferred.push(()=>assert.equal(format,2,
      'v2 supplies authenticated zero/progress owner counters with legal PC-zero origins'));
  });
}

function contextualRows(world,target) {
  const header=world.raw.core_continuation_ptr()>>>0;
  assert.equal(world.word(header),2);
  const pointer=world.word(header,32),length=world.word(header,12)+1,rows=[];
  if(pointer===0)return rows;
  assert(pointer>0&&pointer+88*length<=world.memory.buffer.byteLength);
  for(let index=0;index<length;index++) {
    const row=pointer+88*index;
    if(world.word(row,4)===1&&world.word(row)===target&&world.word(row,12)===1)
      rows.push({origin:world.word(row,8),id:world.bigint(row,16)});
  }
  return rows;
}

function revokeContextsAndCheckAba(world,pcs,parents,windows) {
  const obsolete=windows.at(-1).unit,source=world.moduleRecords.get(obsolete.id).source;
  const module=readGeneratedModule(source);
  assert(module.globalImports.some(item=>item.module==='ring3'&&item.name==='recovery.base'),
    'the stale-instance control poisons an actual imported metadata base');
  const clone=new WebAssembly.Instance(new WebAssembly.Module(source),{ring3:{
    memory:world.memory,read:world.api.core_read,write:world.api.core_write,
    ...cacheCallbacks(world.cache),...world.cache.dispatch,'support.abi':world.cache.dispatch.supportAbi,
    'recovery.base':new WebAssembly.Global({value:'i32',mutable:false},-1),
  }}).exports;
  const header=world.raw.core_continuation_ptr()>>>0;
  const oldGeneration=world.bigint(world.word(header,52),obsolete.slot*8);
  world.cache.publish();const beforeWrite=world.snapshot(),expectedRam=beforeWrite.ram.slice();
  const physical=world.physical(ROOT+2)-world.word(world.context,64),expectedPages=beforeWrite.pages.slice();
  expectedRam[physical]=0x11;
  new DataView(expectedPages.buffer).setBigUint64((physical>>>12)*16+8,beforeWrite.revision+1n,true);
  world.engine.ok('write8',ROOT+2,0x11);
  const written=world.snapshot();
  assert.deepEqual(written.ram,expectedRam,'one real executable-byte write changes only the authored LEA immediate');
  assert.deepEqual(written.pages,expectedPages,'the source page receives its exact committed version');
  assert.equal(written.revision,beforeWrite.revision+1n);assert.notEqual(written.epoch,beforeWrite.epoch);
  for(const pc of pcs)assert.equal(world.raw.core_lookup(pc),4,
    'a changed source page revokes every overlapping context and baseline capability');
  const invalidated=world.snapshot();
  for(const name of ['context','state','fp','simd','ymm','ram','pages','revision','epoch'])
    assert.deepEqual(invalidated[name],written[name],'stale lookup has no architectural side effect: '+name);
  assert.equal(contextualRows(world,ROOT).length,0,'source retirement removes both preferred context bindings');
  for(const owner of [...parents.map(parent=>parent.unit),...windows.map(window=>window.unit)])
    assert.equal(world.bigint(world.raw.core_link_owners_ptr(),owner.slot*8),0n);
  uploadTargets(world,pcs);
  world.reset({pc:ROOT,registers:REGS,flags:FLAGS});
  const before=world.snapshot(),run=world.execute(256,ROOT),registers=[...REGS];
  registers[0]=(INITIAL+128*17)>>>0;
  coldLiteral(world,before,run,{pc:ROOT,retired:256,budget:256,registers,flags:FLAGS,
    fp:world.initialFp,writes:[],versionWrites:0,identity:world.bigint(world.context,120)});
  const replacement=world.cache.select(ROOT);assert(replacement);
  assert.notEqual(replacement.id,obsolete.id);
  assert.equal(replacement.slot,obsolete.slot,'real native pruning reuses the last detached SDK slot');
  const generation=world.bigint(world.word(header,52),replacement.slot*8);
  assert(generation>oldGeneration,'slot reuse creates a fresh native generation');
  world.cache.publish();world.setWord(world.context,44,1);
  assert.equal(world.raw.core_begin(),0);
  try {
    const active=world.snapshot(),ticket=world.bytes(header,136);
    assert.equal(world.raw.core_precise_owned(world.context,obsolete.slot,oldGeneration),8,
      'the stale exact callback cannot adopt the new owner occupying its old slot');
    assert.equal(world.raw.core_end_owned(world.context,obsolete.slot,oldGeneration,2),7,
      'the stale end ticket cannot release the current lease');
    // a foreign selected identity is infrastructure rejection before ingress,
    // rather than a guest NeedCode handoff from this obsolete owner.
    assert.equal(clone.run(world.context,1),8,
      'the old instance rejects the new selected identity before touching its poisoned metadata');
    const after=world.snapshot();
    for(const name of ['context','state','fp','simd','ymm','ram','pages','revision','epoch'])
      assert.deepEqual(after[name],active[name],'rejected stale authority is neutral: '+name);
    assert.deepEqual(world.bytes(header,136),ticket,'rejected old authority preserves the current native ticket and work clock');
  } finally {assert.equal(world.raw.core_end(),0);}
  world.result.sourceAndAba={oldId:String(obsolete.id),newId:String(replacement.id),slot:replacement.slot,
    oldGeneration:String(oldGeneration),generation:String(generation),poisonedMetadataRejected:true};
}

function overlappingContexts(artifact,report,deferred) {
  const pcs=Array.from({length:11},(_,index)=>ROOT+index*0x20);
  const paths=[[0,1,2,3,4,5,6],[0,1,2,7,8,9,10]];
  const code=new Uint8Array(4096);
  for(const[index,pc]of pcs.entries())code.set([0x8d,0x40,index+1,0xff,0x63,index*4],pc-ROOT);
  coldFixture(artifact,report,'incomparable-overlapping-open-prefixes-retain-actual-origin-preferences',{
    code,registers:REGS,flags:FLAGS,
    data:view=>pcs.forEach((pc,index)=>view.setUint32(index*4,pc,true)),
  },world=>{
    world.cache.instructions=2048;world.cache.blocks=128;
    const parents=[];
    for(const[index,pc]of pcs.entries()) {
      world.reset({pc,registers:REGS,flags:FLAGS});
      const before=world.snapshot(),run=world.execute(256,pc),registers=[...REGS];
      registers[0]=(INITIAL+128*(index+1))>>>0;
      coldLiteral(world,before,run,{pc,retired:256,budget:256,registers,flags:FLAGS,
        fp:world.initialFp,writes:[],versionWrites:0,identity:world.bigint(world.context,120)});
      const unit=world.cache.select(pc);assert(unit);parents.push(remember(world,unit));
      assert.equal(world.cache.admissions,index+1);
    }
    const data=new Uint8Array(4096),view=new DataView(data.buffer);
    for(const[context,path]of paths.entries()) {
      pcs.forEach((pc,index)=>view.setUint32(context*0x100+index*4,pc,true));
      path.forEach((index,at)=>view.setUint32(context*0x100+index*4,pcs[path[(at+1)%path.length]],true));
    }
    world.cache.publish();world.engine.request(data);world.engine.ok('upload',0x3000,4096);
    const windows=[],captures=[];
    for(const[context,path]of paths.entries()) {
      const known=context===1&&windows[0]?Math.min(windows[0].count,3):1;
      const units=context===1&&windows[0]?[path.slice(0,known),...path.slice(known).map(index=>[index])]:path.map(index=>[index]);
      // only validated generated routes decrement the sampler. learn the
      // actual context's edges before using its countdown for alignment.
      assert(countdown(world)>path.length);
      const learnedRegisters=[...REGS];learnedRegisters[3]+=context*0x100;
      world.reset({pc:pcs[0],registers:learnedRegisters,flags:FLAGS});
      const learnedBefore=world.snapshot(),admissionsBefore=world.cache.admissions;
      const learned=world.execute(14,pcs[0]),learnedExpected=[...learnedRegisters];
      learnedExpected[0]=(INITIAL+path.reduce((sum,index)=>sum+index+1,0))>>>0;
      coldLiteral(world,learnedBefore,learned,{pc:pcs[0],retired:14,budget:14,registers:learnedExpected,flags:FLAGS,
        fp:world.initialFp,writes:[],versionWrites:0,identity:parents[path.at(-1)].id});
      assert.equal(world.cache.admissions,admissionsBefore,'a bounded edge-learning lap installs no optimizer output');
      const descriptor=Object.getOwnPropertyDescriptor(world.cache,'installRecordedTrace');
      const original=world.cache.installRecordedTrace,recordings=[];
      Object.defineProperty(world.cache,'installRecordedTrace',{configurable:true,writable:true,value:function(...args) {
        if(world.raw.core_trace_ready()===1)recordings.push({root:world.raw.core_trace_root(),...world.recording()});
        return Reflect.apply(original,this,args);
      }});
      try {
        // the same root consumed 1+4 samples in the first context. its
        // unchanged source heat then needs 16+64 samples in this context.
        for(let pass=0;recordings.length<2&&pass<80;pass++) {
          const distance=countdown(world),start=(units.length-distance%units.length)%units.length;
          const count=distance+units.length-1,executed=[];
          for(let index=0;index<count;index++)executed.push(...units[(start+index)%units.length]);
          const registers=[...REGS];registers[3]+=context*0x100;
          const pc=pcs[units[start][0]],budget=2*executed.length;
          world.reset({pc,registers,flags:FLAGS});
          const before=world.snapshot(),run=world.execute(budget,pc),expected=[...registers];
          expected[0]=(INITIAL+executed.reduce((sum,index)=>sum+index+1,0))>>>0;
          coldLiteral(world,before,run,{pc:pcs[path.at(-1)],retired:budget,budget,registers:expected,flags:FLAGS,
            fp:world.initialFp,writes:[],versionWrites:0,identity:world.bigint(world.context,120)});
          assert.equal(run.id,parents[path.at(-2)].id,'the terminal witness owner completed before installation');
          world.result.runs.at(-1).sampler={context,distance,start,units:count,captures:recordings.length};
        }
      } finally {
        if(descriptor)Object.defineProperty(world.cache,'installRecordedTrace',descriptor);
        else delete world.cache.installRecordedTrace;
      }
      assert.equal(recordings.length,2);
      for(const recording of recordings) {
        assert.equal(recording.root,pcs[0]);assert.equal(recording.active,0);assert.equal(recording.overflow,0);
        assert(recording.used>=2&&recording.used<=6);
        assert(recording.events.every(event=>event.kind===1));
        assert.deepEqual(recording.events.map(event=>event.pc),path.slice(0,recording.used).map(index=>pcs[index]));
        assert.equal(recording.events.filter(event=>event.pc===pcs[0]).length,1);
      }
      captures.push(recordings);
      const newWindows=[...world.cache.units.values()].filter(unit=>!parents.some(parent=>parent.id===unit.id)&&
        !windows.some(previous=>previous?.unit.id===unit.id));
      if(newWindows.length===1) {
        const unit=newWindows[0],record=world.moduleRecords.get(unit.id),ports=lookupPorts(record.source),count=ports.length/2;
        assert(Number.isInteger(count)&&count>=2&&count<=5);
        assert.deepEqual(ports,path.slice(0,count).flatMap(index=>[pcs[index],pcs[index]+3]));
        windows.push({unit,count,ports});
      } else {
        assert.equal(newWindows.length,0,'one capture selection cannot fabricate multiple optimized owners');
        windows.push(null);
      }
    }
    world.result.captureSemanticComplete=true;world.result.captures=captures;
    if(windows.some(window=>!window)) {
      world.result.contextMechanismDeferred=true;
      deferred.push(()=>assert(windows.every(Boolean),
        'two distinct actual caller contexts can each promote a repeated incomparable overlapping prefix'));
      return;
    }
    assert.notEqual(windows[0].unit.id,windows[1].unit.id);
    assert(windows[0].ports.some(pc=>windows[1].ports.includes(pc)),'the immutable owners really overlap');
    assert(windows[0].ports.some(pc=>!windows[1].ports.includes(pc)));
    assert(windows[1].ports.some(pc=>!windows[0].ports.includes(pc)));
    const rows=contextualRows(world,pcs[0]);
    for(const[context,path]of paths.entries())
      assert.deepEqual(rows.filter(row=>row.origin===pcs[path.at(-1)]),
        [{origin:pcs[path.at(-1)],id:windows[context].unit.id}],
        'the actual caller entry selects its own optimized owner without becoming execution authority');
    const admissions=world.cache.admissions,generated=world.cache.generatedModuleBytes;
    for(const[context,path]of paths.entries())for(const laps of [1,9]) {
      const registers=[...REGS];registers[3]+=context*0x100;
      const pc=pcs[path.at(-1)];world.reset({pc,registers,flags:FLAGS});
      const before=world.snapshot(),run=world.execute(14*laps,pc),expected=[...registers];
      expected[0]=(INITIAL+path.reduce((sum,index)=>sum+index+1,0)*laps)>>>0;
      coldLiteral(world,before,run,{pc,retired:14*laps,budget:14*laps,registers:expected,flags:FLAGS,
        fp:world.initialFp,writes:[],versionWrites:0,identity:parents[path.at(-2)].id});
      assert.equal(run.regions,laps*(8-windows[context].count));
      if(laps===9)assert.equal(world.resolverCalls,before.resolverCalls,
        'each learned actual context reuses its own retained owner without native misses');
    }
    for(const parent of parents)unchanged(world,parent);
    for(const window of windows)assert.equal(world.cache.units.get(window.unit.id),window.unit);
    assert.equal(world.cache.admissions,admissions);assert.equal(world.cache.generatedModuleBytes,generated);
    assert.equal(world.helperCalls.length,0);
    revokeContextsAndCheckAba(world,pcs,parents,windows);
    world.result.semanticComplete=true;
    world.result.windows=windows.map(window=>({id:String(window.unit.id),count:window.count,ports:window.ports}));
  });
}

export function runExecutionTierCases(artifact,report) {
  const deferred=[];
  const code=new Uint8Array(4096);
  for(const[index,pc]of PCS.entries())
    code.set([0x8d,0x40,index+1,0xff,0x63,index*4],pc-ROOT);
  coldFixture(artifact,report,'repeated-open-compiled-prefix-without-a-closed-word',{
    code,registers:REGS,flags:FLAGS,
    data:view=>PCS.forEach((pc,index)=>view.setUint32(index*4,pc,true)),
  },world=>{
    world.cache.instructions=2048;world.cache.blocks=128;
    const parents=[];
    for(const[index,pc]of PCS.entries()) {
      world.reset({pc,registers:REGS,flags:FLAGS});
      const before=world.snapshot(),run=world.execute(256,pc),registers=[...REGS];
      registers[0]=(INITIAL+128*(index+1))>>>0;
      coldLiteral(world,before,run,{pc,retired:256,budget:256,registers,flags:FLAGS,
        fp:world.initialFp,writes:[],versionWrites:0,identity:world.bigint(world.context,120)});
      const unit=world.cache.select(pc);assert(unit);
      assert.equal(world.cache.admissions,index+1,'each independent cold loop has one naturally formed owner');
      parents.push(remember(world,unit));
    }
    assert.equal(new Set(parents.map(parent=>parent.id)).size,7);
    uploadTargets(world,PCS.map((_,index)=>PCS[(index+1)%PCS.length]));
    const descriptor=Object.getOwnPropertyDescriptor(world.cache,'installRecordedTrace');
    const original=world.cache.installRecordedTrace,captures=[];
    Object.defineProperty(world.cache,'installRecordedTrace',{configurable:true,writable:true,value:function(...args) {
      if(world.raw.core_trace_ready()===1)
        captures.push({root:world.raw.core_trace_root(),...world.recording()});
      return Reflect.apply(original,this,args);
    }});
    try {
      for(let pass=0;captures.length<2&&pass<16;pass++) {
        const distance=countdown(world),start=(PCS.length-distance%PCS.length)%PCS.length;
        const ownerCount=distance+6,budget=2*ownerCount,pc=PCS[start];
        world.reset({pc,registers:REGS,flags:FLAGS});
        const before=world.snapshot(),run=world.execute(budget,pc),registers=[...REGS];
        registers[0]=(INITIAL+sumOwners(start,ownerCount))>>>0;
        const end=PCS[(start+ownerCount)%PCS.length];assert.equal(end,PCS[6]);
        coldLiteral(world,before,run,{pc:end,retired:budget,budget,registers,flags:FLAGS,
          fp:world.initialFp,writes:[],versionWrites:0,identity:world.bigint(world.context,120)});
        assert.equal(run.id,parents[5].id,'the last actually executed owner is F, before any postlease installation');
        world.result.runs.at(-1).sampler={distance,start,ownerCount,captures:captures.length};
      }
    } finally {
      if(descriptor)Object.defineProperty(world.cache,'installRecordedTrace',descriptor);
      else delete world.cache.installRecordedTrace;
    }
    assert.equal(captures.length,2,'two distinct real completed captures establish repeated open-prefix evidence');
    for(const capture of captures) {
      assert.equal(capture.root,PCS[0]);assert.equal(capture.active,0);assert.equal(capture.overflow,0);
      assert(capture.used>=2&&capture.used<=6);
      assert(capture.events.every(event=>event.kind===1),'only actual compiled entries form this candidate');
      assert.deepEqual(capture.events.map(event=>event.pc),PCS.slice(0,capture.used));
      assert.equal(capture.events.filter(event=>event.pc===PCS[0]).length,1,
        'the capture contains no repeated closed word at its selected root');
    }
    world.result.captureSemanticComplete=true;
    world.result.captures=captures;
    const windows=[...world.cache.units.values()].filter(unit=>!parents.some(parent=>parent.id===unit.id));
    world.result.windowCandidateCount=windows.length;
    if(windows.length!==1) {
      world.result.openPrefixMechanismDeferred=true;
      deferred.push(()=>assert.equal(windows.length,1,
        'two repeated open compiled prefixes install one automatic execution window without a closed cycle'));
      return;
    }
    const window=windows[0],record=world.moduleRecords.get(window.id);assert(record);
    const actualPorts=lookupPorts(record.source),spanCount=actualPorts.length/2;
    assert(Number.isInteger(spanCount)&&spanCount>=2&&spanCount<=5);
    assert.deepEqual(actualPorts,PCS.slice(0,spanCount).flatMap(pc=>[pc,pc+3]),
      'the owner contains exactly a completed contiguous captured prefix');
    assert.equal(window.pc,PCS[0]);
    const admissions=world.cache.admissions,generated=world.cache.generatedModuleBytes;
    for(const laps of [1,9]) {
      // the actual G entry supplies the recorded origin for its A target.
      // public selection of A alone must not invent that caller context.
      world.reset({pc:PCS[6],registers:REGS,flags:FLAGS});
      const before=world.snapshot(),run=world.execute(14*laps,PCS[6]),registers=[...REGS];
      registers[0]=(INITIAL+28*laps)>>>0;
      coldLiteral(world,before,run,{pc:PCS[6],retired:14*laps,budget:14*laps,registers,flags:FLAGS,
        fp:world.initialFp,writes:[],versionWrites:0,identity:parents[5].id});
      assert.equal(run.regions,laps*(8-spanCount),'the repeated open prefix executes inside one body');
      if(laps===9)assert.equal(world.resolverCalls,before.resolverCalls,'a learned repeated path has no native routing misses');
    }
    for(const fuel of [0,1,2,3]) {
      world.reset({pc:PCS[0],registers:REGS,flags:FLAGS});
      const selected=world.cache.select(PCS[0]);assert.equal(selected,window);
      const before=world.snapshot(),run=world.execute(fuel,PCS[0]),registers=[...REGS];
      const whole=Math.floor(fuel/2),partial=fuel%2;
      registers[0]=(INITIAL+sumOwners(0,whole)+(partial?whole+1:0))>>>0;
      coldLiteral(world,before,run,{pc:PCS[whole]+3*partial,retired:fuel,budget:fuel,registers,flags:FLAGS,
        fp:world.initialFp,writes:[],versionWrites:0,identity:window.id});
    }
    for(const parent of parents)unchanged(world,parent);
    assert.equal(world.cache.admissions,admissions);assert.equal(world.cache.generatedModuleBytes,generated);
    assert.equal(world.helperCalls.length,0);
    const old=world.snapshot(),expectedRam=old.ram.slice();
    expectedRam[world.physical(PCS[0])-world.word(world.context,64)]=0x90;
    world.engine.ok('write8',PCS[0],0x90);
    assert.deepEqual(world.snapshot().ram,expectedRam);assert.equal(world.revision(),old.revision+1n);
    const changed=world.snapshot();
    for(const pc of PCS)assert.equal(world.raw.core_lookup(pc),4,
      'source-page mutation revokes both overlapping window aliases and immutable baseline capabilities');
    const after=world.snapshot();
    for(const name of ['context','state','fp','simd','ymm','ram','pages','revision','epoch'])
      assert.deepEqual(after[name],changed[name],'stale lookup has no guest or publication side effect: '+name);
    for(const owner of [window,...parents])
      assert.equal(world.bigint(world.raw.core_link_owners_ptr(),owner.slot*8),0n);
    world.result.window={id:String(window.id),root:window.pc,spans:spanCount,
      bytes:record.bytes,sha256:hash(record.source),ports:actualPorts};
    world.result.semanticComplete=true;
  });
  pcZeroAndAccounting(artifact,report,deferred);
  overlappingContexts(artifact,report,deferred);
  for(const check of deferred)check();
}
