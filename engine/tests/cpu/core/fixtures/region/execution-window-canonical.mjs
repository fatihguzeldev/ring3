import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';
import {readGeneratedModule} from './structure.mjs';

const A=0x1000,B=0x1200,FLAGS=0x403;
const unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const REGS=[0x11223344,0x55667788,0x99aabbcc,0x3000,0x4800,0x12345678,B,A];
const ZEROED=[0,0,0,0,...REGS.slice(4)];
const VALUE={five:'00000000000000a00140',twentyOne:'00000000000000a80340',
  twoFiftyNine:'00000000000080810740',twoTwoZeroThree:'000000000000b0890a40'};

function fp(initial,{value,status=0x20,fip}={}) {
  const result=initial.slice(),d=new DataView(result.buffer,result.byteOffset,result.byteLength);
  d.setUint16(18,status,true);
  if(fip!==undefined){d.setUint16(22,0xc1,true);d.setUint32(24,fip,true);d.setUint16(32,0x1b,true);}
  if(value)result.set(unhex(value),40);
  return result;
}

function program(code,base,root,lastJump) {
  const bytes=unhex(root);code.set(bytes,base-A);
  for(let i=0;i<64;i++)code.set(unhex('90 '+(i===63?lastJump:'eb00')),base-A+bytes.length+i*3);
}

// Parse the complete actual lookup BST, so parent geometry is established from
// emitted instruction ports rather than from the authored byte layout alone.
function lookup(source) {
  const module=readGeneratedModule(source),matches=[...module.names].filter(([,name])=>name==='guest.lookup');
  assert.equal(matches.length,1);const index=matches[0][0],fn=module.functions[index-module.imports.length];
  assert(fn);assert.deepEqual(module.types[module.functionTypes[index-module.imports.length]],
    {parameters:[0x7f],results:[0x7f]});assert.equal(fn.locals.length,0);
  const precise=module.imports.some(item=>item.module==='ring3'&&item.name==='precise'),bits=precise?14:10;
  const entries=[],ops=fn.ops;let at=0;
  const take=op=>{const result=ops[at++];assert.equal(result?.op,op,'exact emitted lookup tree');return result;};
  const constant=value=>{const result=take(0x41);if(value!==undefined)assert.equal(result.value,value);return result.value;};
  const input=()=>assert.equal(take(0x20).local,0);
  function tree(){
    if(ops[at]?.op===0x41&&ops[at].value===-1){constant(-1);return;}
    input();const pc=constant()>>>0;take(0x46);const outer=take(0x04);assert.equal(outer.frame.type,-1);
    const port=constant();assert(Number.isInteger(port)&&port>=0);assert.equal(take(0x05).frame,outer.frame);
    input();assert.equal(constant()>>>0,pc);take(0x49);const inner=take(0x04);assert.equal(inner.frame.type,-1);
    tree();entries.push({pc,block:port&((1<<bits)-1),prefix:port>>>bits});assert.equal(take(0x05).frame,inner.frame);tree();
    assert.equal(take(0x0b).frame,inner.frame);assert.equal(take(0x0b).frame,outer.frame);
  }
  tree();assert.equal(take(0x0b).frame.kind,0xff);assert.equal(at,ops.length);
  for(let i=1;i<entries.length;i++)assert(entries[i-1].pc<entries[i].pc);
  return{entries,blocks:new Set(entries.map(entry=>entry.block)).size,precise};
}

function retain(world,unit,ports) {
  const source=world.moduleRecords.get(unit.id);assert(source);
  return{unit,id:unit.id,pc:unit.pc,slot:unit.slot,bytes:unit.bytes,sha256:source.sha256,
    run:unit.run,link:unit.link,publish:unit.publish,ports};
}

function stable(world,parent,{skip=[]}={}) {
  const unit=world.cache.units.get(parent.id);assert.equal(unit,parent.unit);
  assert.equal(unit.slot,parent.slot);assert.equal(unit.bytes,parent.bytes);
  for(const name of['run','link','publish'])assert.equal(unit[name],parent[name]);
  assert.equal(world.cache.table.get(parent.slot),parent.link);
  assert.equal(world.cache.publishers.get(parent.slot),parent.publish);
  const owners=world.raw.core_link_owners_ptr()>>>0;assert.equal(world.bigint(owners,parent.slot*8),parent.id);
  for(const pc of parent.ports) {
    if(skip.includes(pc))continue;
    assert.equal(world.raw.core_lookup(pc),0);assert.equal(world.bigint(world.context,120),parent.id);
    const bytes=world.bytes(world.raw.core_code_ptr()>>>0,world.raw.core_code_len()>>>0);
    assert.equal(bytes.length,parent.bytes);assert.equal(hash(bytes),parent.sha256);
  }
}

function callbacks(calls,identity,ordinal,count) {
  assert.equal(calls.length,count,'literal canonical callback count');
  for(const call of calls) {
    assert.equal(BigInt(call.identity),identity,'canonical callback binds the actual executing owner');
    assert.equal(call.ordinal,ordinal,'canonical callback uses that owner\'s native dictionary');
  }
}

export function runExecutionWindowCanonicalCases(artifact,report) {
  const code=new Uint8Array(4096);
  program(code,A,'d8c1 ffe6','ffe7');program(code,B,'b8ffffffff 0fa2 ffe7','ffe6');
  coldFixture(artifact,report,'automatic-window-canonical-ordinal-without-parent-absorption',{
    code,registers:REGS,flags:FLAGS,
  },world=>{
    world.cache.instructions=2048;world.cache.blocks=128;
    const initial=fp(world.initialFp,{status:0x220}),parents=[];
    for(const base of[A,B]) {
      const registers=[...REGS];registers[6]=base===A?A+4:B;registers[7]=base===A?A:B+9;
      world.reset({pc:base,registers,flags:FLAGS,fp:initial});
      const budget=base===A?16640:16768,before=world.snapshot(),run=world.execute(budget,base);
      const expected=base===A?registers:[0,0,0,0,...registers.slice(4)];
      coldLiteral(world,before,run,{pc:base,retired:budget,budget,registers:expected,flags:FLAGS,
        fp:base===A?fp(initial,{value:VALUE.twoFiftyNine,fip:A}):initial,writes:[],versionWrites:0,
        identity:world.bigint(world.context,120)});
      assert.equal(run.kind,'cold');
      const unit=world.cache.select(base);assert(unit);
      const ports=base===A?[A,A+2,...Array.from({length:64},(_,i)=>A+4+i*3)
        .flatMap((pc,i)=>i===63?[pc]:[pc,pc+1])]:
        [B,B+5,B+7,...Array.from({length:63},(_,i)=>[B+9+i*3,B+10+i*3]).flat()];
      const decoded=lookup(world.moduleRecords.get(unit.id).source);
      assert.deepEqual(decoded.entries.map(entry=>entry.pc),ports);assert.equal(decoded.blocks,base===A||decoded.precise?65:64);
      parents.push(retain(world,unit,ports));
    }
    const[parentA,parentB]=parents;
    assert.notEqual(parentA.id,parentB.id);assert.equal(world.cache.admissions,2);assert.equal(world.cache.units.size,2);
    // The authored branch-target source lower bounds still total129>128.
    // New hot spans additionally split the canonical continuation in B.
    world.result.parentHistories={sourceBlockLowerBounds:[65,64],
      executableSpans:parents.map(parent=>lookup(world.moduleRecords.get(parent.id).source).blocks),
      instructions:[129,129],limitBlocks:128,
      parents:parents.map(parent=>({id:String(parent.id),pc:parent.pc,slot:parent.slot,bytes:parent.bytes,sha256:parent.sha256}))};

    // training still executes all callbacks in the original dictionary;
    // automatic installation occurs only after that lease completes.
    world.reset({pc:B,registers:REGS,flags:FLAGS,fp:initial});
    const before=world.snapshot(),helpers=world.helperCalls.length,formed=world.execute(5500,B);
    coldLiteral(world,before,formed,{pc:B,retired:5500,budget:5500,registers:ZEROED,flags:FLAGS,
      fp:fp(initial,{value:VALUE.twoTwoZeroThree,fip:A}),writes:[],versionWrites:0,
      identity:world.bigint(world.context,120)});
    assert.equal(formed.id,parentA.id);assert.equal(formed.regions,2200);
    assert.equal(formed.chainedColdWork,0);assert.equal(formed.chainedColdRetired,0);
    callbacks(world.helperCalls.slice(helpers),parentB.id,0,1100);
    world.result.trainingSemanticComplete=true;
    const windows=[...world.cache.units.values()].filter(unit=>!parents.some(parent=>parent.id===unit.id));
    assert.equal(windows.length,1,'a canonical boundary does not prevent automatic hot-window formation');
    const window=windows[0],admissions=world.cache.admissions,generated=world.cache.generatedModuleBytes;
    assert.equal(admissions,3);assert.equal(world.cache.units.size,3);
    for(let replay=0;replay<2;replay++) {
      world.reset({pc:A,registers:REGS,flags:FLAGS,fp:initial});
      const warmBefore=world.snapshot(),warmHelpers=world.helperCalls.length,warm=world.execute(45,A);
      coldLiteral(world,warmBefore,warm,{pc:A,retired:45,budget:45,registers:ZEROED,flags:FLAGS,
        fp:fp(initial,{value:VALUE.twentyOne,fip:A}),writes:[],versionWrites:0});
      assert.equal(warm.id,window.id);assert(warm.regions<=2);
      assert.equal(warm.chainedColdWork,0);assert.equal(warm.chainedColdRetired,0);
      callbacks(world.helperCalls.slice(warmHelpers),window.id,1,9);
      assert(world.resolverCalls-warmBefore.resolverCalls<=1,
        'only the initial caller-to-window edge can need native resolution');
    }
    world.result.semanticComplete=true;
    for(const budget of[3,4]) {
      world.reset({pc:A,registers:REGS,flags:FLAGS,fp:initial});
      const prefixBefore=world.snapshot(),prefixHelpers=world.helperCalls.length,run=world.execute(budget,A);
      coldLiteral(world,prefixBefore,run,{pc:B+(budget===3?5:7),retired:budget,budget,
        registers:budget===3?[0xffffffff,...REGS.slice(1)]:ZEROED,flags:FLAGS,
        fp:fp(initial,{value:VALUE.five,fip:A}),writes:[],versionWrites:0});
      assert(run.regions<=2);
      if(budget===4)callbacks(world.helperCalls.slice(prefixHelpers),run.id,run.id===window.id?1:0,1);
      else assert.equal(world.helperCalls.length,prefixHelpers);
    }
    const publicEntries=[A,B,B+5,B+7];
    for(const pc of publicEntries) {
      assert.equal(world.raw.core_lookup(pc),0);
      assert.equal(world.bigint(world.context,120),window.id,
        'each complete canonical-boundary block selects the same window');
    }
    stable(world,parentA,{skip:publicEntries});stable(world,parentB,{skip:publicEntries});
    assert.equal(world.cache.admissions,admissions);assert.equal(world.cache.generatedModuleBytes,generated);
    assert.equal(world.cache.units.size,3);world.result.windowSemanticComplete=true;
  });

  coldFixture(artifact,report,'manual-root-only-canonical-trace-rebases-its-own-ordinal-and-exact-prefixes',{
    code,registers:REGS,flags:FLAGS,referencePrepared:true,
  },world=>{
    const initial=fp(world.initialFp,{status:0x220});
    const parents=[A,B].map(base=>{
      const unit=world.cache.entry(base);world.cache.entry(base+(base===A?2:7));
      return retain(world,unit,[base,base+(base===A?2:7)]);
    });
    world.reset({pc:A,registers:REGS,flags:FLAGS,fp:initial});world.cache.select(A);
    assert.equal(world.raw.core_trace_start(A),0);
    const before=world.snapshot(),helpers=world.helperCalls.length,train=world.execute(5500,A);
    coldLiteral(world,before,train,{pc:A,retired:5500,budget:5500,registers:ZEROED,flags:FLAGS,
      fp:fp(initial,{value:VALUE.twoTwoZeroThree,fip:A}),writes:[],versionWrites:0});
    callbacks(world.helperCalls.slice(helpers),parents[1].id,0,1100);
    assert.equal(world.raw.core_trace_finish(A,128,32),0);
    const trace=world.cache.select(A);assert(trace&&!parents.some(parent=>parent.id===trace.id));
    const decoded=lookup(world.moduleRecords.get(trace.id).source);
    assert.deepEqual(decoded.entries,[{pc:A,block:0,prefix:0},{pc:A+2,block:0,prefix:1},
      {pc:B,block:1,prefix:0},{pc:B+5,block:2,prefix:0},{pc:B+7,block:3,prefix:0}]);
    assert.equal(decoded.blocks,4);
    const admissions=world.cache.admissions,generated=world.cache.generatedModuleBytes;
    for(const budget of[3,4,45]) {
      world.reset({pc:A,registers:REGS,flags:FLAGS,fp:initial});
      const prefixBefore=world.snapshot(),prefixHelpers=world.helperCalls.length,run=world.execute(budget,A);
      coldLiteral(world,prefixBefore,run,{pc:budget===45?A:B+(budget===3?5:7),retired:budget,budget,
        registers:budget===3?[0xffffffff,...REGS.slice(1)]:ZEROED,flags:FLAGS,
        fp:fp(initial,{value:budget===45?VALUE.twentyOne:VALUE.five,fip:A}),writes:[],versionWrites:0});
      assert.equal(run.id,trace.id);assert.equal(run.regions,1);
      callbacks(world.helperCalls.slice(prefixHelpers),trace.id,1,budget===3?0:budget===4?1:9);
    }
    stable(world,parents[0],{skip:[A]});stable(world,parents[1]);
    assert.equal(world.cache.admissions,admissions);assert.equal(world.cache.generatedModuleBytes,generated);
    assert.equal(world.cache.units.size,3);world.result.semanticComplete=true;
  });
}
