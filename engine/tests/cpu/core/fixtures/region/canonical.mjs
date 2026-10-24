import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {inspectCanonicalSpine} from './structure.mjs';

const THREE='00000000000000c00040',TWO='00000000000000800040';
const FIVE='00000000000000a00140',FULL80='01000000000000c00040';
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const registers=[0,2,0,0x3000,0x4800,0,0x1800,0];
const roundingData=page=>{const d=new DataView(page.buffer);d.setFloat32(0,0,true);d.setFloat32(4,2,true);};

function observe(t,wanted,{canonicalMode,helpers=0,precise=0,entries=1,materializers=helpers+precise,writes=0,fast=false,revoked=false,after},checks) {
  const source=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),revision=t.version();
  const id=new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true);
  const run=t.check(wanted);
  assert.equal(run.activations.length,1,'the original body executes the exact canonical continuation without another preparation');
  assert.equal(t.result.modules.length,1,'canonical service does not replace its source owner');
  assert.equal(run.helperCalls,helpers,'only planned canonical effects call the retained owner ordinal');
  assert.equal(run.preciseCalls,precise,'a rejected or partial span uses the shared exact adapter');
  assert.equal(t.version(),revision+BigInt(writes),'only the literal completed stores advance the transaction sequence');
  assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),revoked?0n:id,
    'a precise code store revokes execution authority while retaining source metadata for stale rejection');
  assert.equal(hash(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0)),hash(source),
    'the same actual encoded owner retains its identity and bytes');
  after?.();
  const spine=inspectCanonicalSpine(source),witness=t.stateWitness();
  t.result.canonicalSpine=spine;t.result.canonicalWitness=witness;t.result.semanticComplete=true;
  checks.push(()=>{
    assert.equal(witness.body_entries,entries,'internal precise continuation retains one bootstrap; external exact ingress uses only the specified wrapper reentry');
    assert.equal(witness.canonical_calls,helpers,'actual Wasm canonical calls match the independent callback oracle');
    assert.equal(typeof canonicalMode,'boolean','each authored program declares whether it contains a planned canonical operation');
    assert.equal(spine.sites.length,Number(canonicalMode),'only owners with a planned canonical operation retain the one shared canonical import callsite');
    assert(spine.sites.every(site=>site.function===spine.body),'the canonical spine belongs to the shared instruction body');
    assert.equal(witness.wide_snapshot_calls,0,'a first planned fallback transports no 40-value state snapshot');
    assert.equal(witness.materializer_calls,materializers,
      'only the specified canonical or precise authority transitions materialize the pool');
    if(fast){
      assert.equal(witness.materializer_calls,0,'successful direct execution gains no canonical publication');
      assert.equal(witness.raw80_stores,0,'successful direct execution leaves the canonical FP bank private');
      assert.equal(witness.afterBankHex,witness.beforeBankHex);
    }
  });
}

// Every full architectural/RAM oracle runs before the first new-format assertion.
// The pinned old compiler must fail on its real duplicated protocol, not on a
// renamed helper or on an unvalidated semantic expectation.
export function runCanonicalCases(fixture) {
  const checks=[];
  fixture('canonical-service-successful-direct-path-preserves-existing-private-traffic',{
    segments:[[0x1000,'d903 d84304 d95b08 ffe6']],registers,stateWitness:true,
  },t=>{
    t.writeExpected(0x3008,'0000a040');
    observe(t,{pc:0x1800,retired:4,budget:4,
      fp:t.fp({opcode:0x15b,fip:0x1005,fdp:0x3008,registers:{7:FIVE}})},
      {canonicalMode:false,helpers:0,writes:1,fast:true},checks);
  });

  // 3+2^-62 is an exact normal binary80 value. At PC53, adding zero rounds
  // downward to3, sets PE, and clears the initial C1. The next addition is5.
  // The same literal prefixes exercise quota before, at, and after cold entry.
  for(const budget of[0,1,2,4])fixture(`canonical-service-first-planned-failure-exact-prefix-${budget}`,{
    segments:[[0x1000,'d803 d84304 db7b10 ffe6']],registers,stateWitness:true,data:roundingData,
    initialFp:{status:0x200,tag:0xfffc,registers:{0:FULL80}},
  },t=>{
    if(budget===4)t.writeExpected(0x3010,FIVE);
    const expected=budget===0?t.startingFp:t.fp(budget===1?
      {status:0x20,tag:0xfffc,opcode:3,fip:0x1000,fdp:0x3000,registers:{0:THREE}}:
      budget===2?{status:0x20,tag:0xfffc,opcode:0x43,fip:0x1002,fdp:0x3004,registers:{0:FIVE}}:
      {status:0x820,opcode:0x37b,fip:0x1005,fdp:0x3010,registers:{0:FIVE}});
    observe(t,{pc:[0x1000,0x1002,0x1005,0,0x1800][budget],retired:budget,budget,fp:expected},
      {canonicalMode:true,precise:Number(budget!==0),writes:Number(budget===4)},checks);
  });

  fixture('canonical-service-arbitrary-port-imports-full80-top-and-unexecuted-history',{
    segments:[[0x1000,'d903 d84304 db7b10 ffe6']],registers,stateWitness:true,control:0x37f,
    initialFp:{status:0x1000,tag:0xffcf,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,
      registers:{2:FULL80,7:'00000000000000000080'}},
    data:page=>{const d=new DataView(page.buffer);d.setFloat32(0,3,true);d.setFloat32(4,0,true);},
  },t=>{
    new DataView(t.engine.memory.buffer).setUint32(t.context()+32,0x1002,true);
    t.writeExpected(0x3010,FULL80);
    observe(t,{pc:0x1800,retired:3,budget:3,
      fp:t.fp({status:0x1800,opcode:0x37b,fip:0x1005,fdp:0x3010,
        registers:{2:FULL80,7:'00000000000000000080'}})}, {canonicalMode:true,precise:1,entries:2,writes:1},checks);
  });

  for(const codeStore of[false,true])fixture(`canonical-service-${codeStore?'smc-commits-current-once':'late-fault-preserves-completed-prefix'}`,{
    segments:[[0x1000,'d803 db3f ffe6']],registers:[...registers.slice(0,7),codeStore?0x1100:0x6000],
    stateWitness:true,data:roundingData,initialFp:{status:0x200,tag:0xfffc,registers:{0:FULL80}},
  },t=>{
    if(codeStore)t.writeExpected(0x1100,THREE);
    observe(t,{pc:codeStore?0x1004:0x1002,retired:codeStore?2:1,budget:3,
      work:codeStore?2:1,remaining:codeStore?1:2,stop:codeStore?6:3,
      fault:codeStore?[0,0,0]:[0x6000,0x1020a,0xa0e],
      fp:t.fp(codeStore?{status:0x820,opcode:0x33f,fip:0x1002,fdp:0x1100,registers:{0:THREE}}:
        {status:0x20,tag:0xfffc,opcode:3,fip:0x1000,fdp:0x3000,registers:{0:THREE}})},
      {canonicalMode:true,precise:1,revoked:codeStore,writes:Number(codeStore),after:()=>{if(codeStore)t.status('core_begin',4);}},checks);
  });

  fixture('canonical-service-planned-fault-publishes-pending-c1-and-empty-ghost',{
    segments:[[0x1000,'d903 d9c9 d84304 ddd8 db3f ffe6']],
    registers:[...registers.slice(0,7),0x6000],stateWitness:true,
    initialFp:{status:0x20,tag:0xfffc,registers:{0:'0008000000000080ff3f'}},
    data:page=>page.set(Buffer.from('0000604000000025','hex')),
  },t=>{
    // The odd PC53 midpoint rounds upward to1+2^-51. Register pop preserves
    // its pending C1 and empty ghost before the unsupported raw80 store faults.
    observe(t,{pc:0x1009,retired:4,budget:6,work:4,remaining:2,stop:3,
      fault:[0x6000,0x1020a,0xa0e],fp:t.fp({status:0x220,tag:0xfffc,
        opcode:0x5d8,fip:0x1007,fdp:0x3004,
        registers:{0:'00000000000000e00040',7:'0010000000000080ff3f'}})}, {canonicalMode:true,helpers:1},checks);
  });

  // Legal authored repetition supplies a scalable static discriminator. Native
  // success stays hot; rejected full80 executes one decoded precise suffix.
  for(const canonical of[false,true])fixture(`canonical-service-160-fp-sites-${canonical?'exact-full80-suffix':'successful-native'}`,{
    segments:[[0x1000,'d8c1'.repeat(160)+'ffe6']],registers,stateWitness:true,compileBlocks:1,
    control:canonical?0x37f:0x27f,
    initialFp:{tag:0xfff0,registers:{0:canonical?FULL80:THREE,1:canonical?'00000000000000000000':TWO}},
  },t=>{
    observe(t,{pc:0x1800,retired:161,budget:161,
      fp:t.fp({tag:0xfff0,opcode:0xc1,fip:0x113e,
        registers:{0:canonical?FULL80:'00000000000080a10740',1:canonical?'00000000000000000000':TWO}})},
      {canonicalMode:false,precise:canonical?1:0,fast:!canonical},checks);
  });
  for(const check of checks)check();
}

export function runCanonicalStatusCases(fixture) {
  const checks=[];
  for(const {name,code,vector}of[
    {name:'sysenter',code:'0f34',vector:0x100},
    {name:'int2e',code:'cd2e',vector:0x2e},
  ])fixture(`canonical-service-${name}-retires-current-once-before-system-entry`,{
    segments:[[0x1000,`90 ${code}`]],registers,stateWitness:true,initialFlags:0x403,
  },t=>{
    // processor.rs returns SystemEntry without modifying scalar or numeric
    // values; the generated service alone owns retirement and the next PC.
    observe(t,{pc:0x1003,retired:2,budget:4,work:2,remaining:2,stop:10,flags:0x403,
      fault:[0x1001,0,vector],fp:t.startingFp},{canonicalMode:true,helpers:1},checks);
  });
  fixture('canonical-service-unconfigured-rdtsc-rejects-current-without-retirement',{
    segments:[[0x1000,'90 0f31']],registers,stateWitness:true,initialFlags:0x403,
  },t=>{
    // EngineInstance constructs CpuClock::default() with state=None; this
    // harness never configures it. timestamp() therefore fails Infrastructure
    // before processor.rs writes the tentative EAX/EDX state back to Context.
    observe(t,{pc:0x1001,retired:1,budget:4,work:1,remaining:3,stop:8,flags:0x403,
      fp:t.startingFp},{canonicalMode:true,helpers:1,materializers:0},checks);
  });
  for(const check of checks)check();
}
