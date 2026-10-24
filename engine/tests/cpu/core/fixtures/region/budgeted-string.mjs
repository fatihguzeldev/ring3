import assert from 'node:assert/strict';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';

const ROOT=0x1000,SOURCE=0x3100,DESTINATION=0x3200,TRAIN=128;
const registers=n=>[0x12345678,0x87654321,n,0x3000,0x4800,0x89abcdef,SOURCE,DESTINATION];
// Exact positive integers encoded directly as binary80, independently of either
// guest execution backend and without host floating-point rounding.
function integer80(value) {
  const n=BigInt(value),exponent=n.toString(2).length-1,bytes=Buffer.alloc(10);
  bytes.writeBigUInt64LE(n<<BigInt(63-exponent));bytes.writeUInt16LE(0x3fff+exponent,8);
  return bytes;
}

function fpAfter(initial,mixed,iterations) {
  if(!mixed)return initial;
  const fp=initial.slice(),d=new DataView(fp.buffer);
  d.setUint16(18,0,true);d.setUint16(22,0xc1,true);d.setUint32(24,0x100f,true);
  d.setUint16(32,0x1b,true);fp.set(integer80(3+2*iterations),40);
  return fp;
}

export function runBudgetedStringCases(artifact,report) {
  const checks=[];
  for(const {name,width,count,opcode}of[
    {name:'movsd',width:4,count:13,opcode:'f3a5'},
    {name:'movsb',width:1,count:34,opcode:'f3a4'},
  ])for(const mixed of[false,true]) {
    const countHex=Buffer.alloc(4);countHex.writeUInt32LE(count);
    const repPc=mixed?0x1011:0x100f,next=repPc+2,done=next+3;
    const code=`b9${countHex.toString('hex')} be00310000 bf00320000 ${mixed?'d8c1 ':''}${opcode} 4a 75${mixed?'ea':'ec'} 90 0f0b`;
    const payload=Uint8Array.from({length:width*count},(_,index)=>(index*37+11)&255);
    coldFixture(artifact,report,`budgeted-${name}-${mixed?'mixed-fp':'scalar'}-normal-cold-to-hot`,{
      code,registers:registers(TRAIN),flags:2,
      data:data=>payload.forEach((value,index)=>data.setUint8(0x100+index,value)),
    },world=>{
      const retiredPerIteration=6+Number(mixed),workPerIteration=count+5+Number(mixed);
      const finishedRegisters=()=>[...registers(0).slice(0,6),SOURCE+width*count,DESTINATION+width*count]
        .map((value,index)=>index===1?0:value);
      const writes=iterations=>[{address:DESTINATION,bytes:payload,count:count*iterations,lastVersion:count*iterations}];
      const trainBefore=world.snapshot(),trainBudget=TRAIN*workPerIteration+1,train=world.execute(trainBudget);
      coldLiteral(world,trainBefore,train,{pc:done+1,retired:TRAIN*retiredPerIteration+1,work:trainBudget,
        budget:trainBudget,registers:finishedRegisters(),flags:0x46,
        fp:fpAfter(world.initialFp,mixed,TRAIN),writes:writes(TRAIN),versionWrites:count*TRAIN,
        identity:world.bigint(world.context,120)});
      const unit=world.cache.select(ROOT),record=unit&&world.moduleRecords.get(unit.id);
      const admitted=world.cache.admissions,generated=world.cache.generatedModuleBytes;
      const warm=[];
      for(const iterations of[1,9]) {
        world.reset({pc:ROOT,registers:registers(iterations),flags:2});
        const before=world.snapshot(),preciseBefore=world.cache.preciseRuns;
        const budget=iterations*workPerIteration,run=world.execute(budget);
        coldLiteral(world,before,run,{pc:done,retired:iterations*retiredPerIteration,work:budget,budget,
          registers:finishedRegisters(),flags:0x46,fp:fpAfter(world.initialFp,mixed,iterations),
          writes:writes(iterations),versionWrites:count*iterations,identity:world.bigint(world.context,120)});
        warm.push({run,before,precise:world.cache.preciseRuns-preciseBefore,
          admissions:world.cache.admissions,generated:world.cache.generatedModuleBytes});
      }
      // Direct entry into the naturally captured REP also tests the zero-count
      // rule: one work/retirement, no memory access, and no skipped FP execution.
      const zeroRegisters=registers(1);zeroRegisters[1]=0;zeroRegisters[6]=0x6000;zeroRegisters[7]=0x7000;
      world.reset({pc:repPc,registers:zeroRegisters,flags:2});
      const zeroBefore=world.snapshot(),zeroPreciseBefore=world.cache.preciseRuns,zero=world.execute(1,repPc);
      const zeroPrecise=world.cache.preciseRuns-zeroPreciseBefore;
      coldLiteral(world,zeroBefore,zero,{pc:next,retired:1,work:1,budget:1,registers:zeroRegisters,
        flags:2,fp:world.initialFp,writes:[],versionWrites:0,identity:world.bigint(world.context,120)});
      const boundaries=[];
      if(width===1&&mixed) {
        // the hot FP prefix is already dirty when quota interrupts REP. resume
        // must carry the two completed bytes and must not repeat that FADD.
        world.reset({pc:ROOT,registers:registers(1),flags:2});
        const before=world.snapshot(),precise=world.cache.preciseRuns,partial=world.execute(6);
        const partialRegisters=registers(1);partialRegisters[1]=count-2;
        partialRegisters[6]=SOURCE+2;partialRegisters[7]=DESTINATION+2;
        coldLiteral(world,before,partial,{pc:repPc,retired:4,work:6,budget:6,registers:partialRegisters,
          flags:2,fp:fpAfter(world.initialFp,true,1),writes:[{address:DESTINATION,bytes:payload.slice(0,2),count:2}],
          versionWrites:2,identity:world.bigint(world.context,120)});
        boundaries.push({name:'partial-dirty-fp',precise:world.cache.preciseRuns-precise});
        const resumeBefore=world.snapshot(),resumePrecise=world.cache.preciseRuns,resume=world.execute(count);
        coldLiteral(world,resumeBefore,resume,{pc:done,retired:3,work:count,budget:count,
          registers:finishedRegisters(),flags:0x46,fp:fpAfter(world.initialFp,true,1),
          writes:[{address:DESTINATION+2,bytes:payload.slice(2),count:count-2}],versionWrites:count-2,
          identity:world.bigint(world.context,120)});
        boundaries.push({name:'resume-dirty-fp',precise:world.cache.preciseRuns-resumePrecise});
        // backwards overlap must observe each preceding store. a bulk memmove
        // would incorrectly retain the original bytes at3100/3101.
        const overlapRegisters=registers(1);overlapRegisters[1]=3;
        overlapRegisters[6]=SOURCE+2;overlapRegisters[7]=SOURCE+1;
        world.reset({pc:repPc,registers:overlapRegisters,flags:0x402});
        const overlapBefore=world.snapshot(),overlapPrecise=world.cache.preciseRuns,overlap=world.execute(3,repPc);
        const overlapAfter=[...overlapRegisters];overlapAfter[1]=0;overlapAfter[6]=SOURCE-1;overlapAfter[7]=SOURCE-2;
        coldLiteral(world,overlapBefore,overlap,{pc:next,retired:1,work:3,budget:3,
          registers:overlapAfter,flags:0x402,fp:world.initialFp,
          writes:[SOURCE+1,SOURCE,SOURCE-1].map(address=>({address,bytes:Uint8Array.of(payload[2])})),
          identity:world.bigint(world.context,120)});
        boundaries.push({name:'backwards-overlap',precise:world.cache.preciseRuns-overlapPrecise});
      }
      world.result.semanticComplete=true;
      world.result.budgetedString={width,count,repPc,owner:unit?String(unit.id):null,
        ownerBytes:record?.bytes,ownerSha256:record?.sha256,trainingRetired:train.retired,trainingWork:train.work,
        warm:warm.map(({run,precise})=>({retired:run.retired,work:run.work,preciseCalls:precise,regions:run.regions})),
        zero:{preciseCalls:zeroPrecise,regions:zero.regions},boundaries};
      checks.push(()=>{
        assert(unit&&record,'normal completed cold heat installs an immutable executable owner');
        assert.equal(train.kind,'cold');assert.equal(admitted,1);
        for(const {run,precise,admissions,generated:emitted}of warm) {
          assert.equal(run.kind,'compiled');assert.equal(run.id,unit.id);
          assert.equal(precise,0,'positive-fuel REP executes its element loop without the native exact adapter');
          assert.equal(run.regions,1,'REP and its ordinary continuation stay in the same body');
          assert.equal(run.chainedColdWork,0);assert.equal(run.chainedColdRetired,0);
          assert.equal(admissions,admitted);assert.equal(emitted,generated);
        }
        assert.equal(zeroPrecise,0,'zero-count REP is an admitted one-work instruction without memory access');
        assert.equal(zero.kind,'compiled');assert.equal(zero.id,unit.id);
        for(const boundary of boundaries)assert.equal(boundary.precise,0,`${boundary.name}: admitted kernel retains its exact partial progress`);
        assert.equal(world.cache.units.get(unit.id),unit);assert.equal(world.cache.units.size,1);
        assert.equal(world.moduleRecords.get(unit.id),record);
        assert.equal(world.cache.admissions,admitted);assert.equal(world.cache.generatedModuleBytes,generated);
        assert.equal(world.helperCalls.length,0);assert.equal(world.prepares,0);
      });
    });
  }
  for(const sourceFault of[true,false])coldFixture(artifact,report,
    `budgeted-rep-late-${sourceFault?'source':'destination'}-fault-preserves-one-element`,{
      code:'b903000000 be00310000 bf00320000 f3a4 4a 75ec 90 0f0b',registers:registers(TRAIN),flags:2,
      data:data=>[0x11,0x22,0x33].forEach((byte,index)=>data.setUint8(0x100+index,byte)),
    },world=>{
      const before=world.snapshot(),train=world.execute(TRAIN*8+1);
      coldLiteral(world,before,train,{pc:0x1015,retired:TRAIN*6+1,work:TRAIN*8+1,budget:TRAIN*8+1,
        registers:[0x12345678,0,0,0x3000,0x4800,0x89abcdef,SOURCE+3,DESTINATION+3],flags:0x46,fp:world.initialFp,
        writes:[{address:DESTINATION,bytes:'112233',count:TRAIN*3}],versionWrites:TRAIN*3,identity:world.bigint(world.context,120)});
      const input=registers(1);input[1]=3;input[6]=sourceFault?0x4fff:SOURCE;input[7]=sourceFault?0x3300:0x4fff;
      world.reset({pc:0x100f,registers:input,flags:2});
      const faultBefore=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(8,0x100f);
      const output=[...input];output[1]=2;output[6]++;output[7]++;
      coldLiteral(world,faultBefore,run,{pc:0x100f,retired:0,work:1,budget:8,stop:3,
        registers:output,flags:2,fp:world.initialFp,fault:[0x5000,sourceFault?0x10101:0x10201,0x10e],
        writes:[{address:input[7],bytes:Uint8Array.of(sourceFault?0:0x11)}],identity:world.bigint(world.context,120)});
      const preciseCalls=world.cache.preciseRuns-precise;world.result.semanticComplete=true;
      checks.push(()=>assert.equal(preciseCalls,0,'the admitted kernel directly reports its literal partial page fault'));
    });
  coldFixture(artifact,report,'budgeted-rep-executable-write-stops-after-one-element',{
    code:'b903000000 be00310000 bf00320000 f3a4 4a 75ec 90 0f0b',registers:registers(TRAIN),flags:2,
    data:data=>[0x11,0x22,0x33].forEach((byte,index)=>data.setUint8(0x100+index,byte)),
  },world=>{
    const before=world.snapshot(),train=world.execute(TRAIN*8+1);
    coldLiteral(world,before,train,{pc:0x1015,retired:TRAIN*6+1,work:TRAIN*8+1,budget:TRAIN*8+1,
      registers:[0x12345678,0,0,0x3000,0x4800,0x89abcdef,SOURCE+3,DESTINATION+3],flags:0x46,fp:world.initialFp,
      writes:[{address:DESTINATION,bytes:'112233',count:TRAIN*3}],versionWrites:TRAIN*3,identity:world.bigint(world.context,120)});
    const owner=world.cache.select(ROOT);
    const input=registers(1);input[1]=3;input[7]=0x10f0;
    world.reset({pc:0x100f,registers:input,flags:2});
    const smcBefore=world.snapshot(),precise=world.cache.preciseRuns,smc=world.execute(8,0x100f);
    const output=[...input];output[1]=2;output[6]++;output[7]++;
    coldLiteral(world,smcBefore,smc,{pc:0x100f,retired:0,work:1,budget:8,stop:6,registers:output,
      flags:2,fp:world.initialFp,writes:[{address:0x10f0,bytes:'11'}],identity:owner?.id??0n});
    // generated SMC retains stale source metadata, but grants no new lease.
    // the precise executor instead clears its identity while executing natively.
    const sealed=world.snapshot();
    assert.equal(world.raw.core_begin(),4,'the invalidated compiled owner cannot reenter');
    const rejected=world.snapshot();
    for(const field of['context','state','fp','simd','ymm','ram','pages'])
      assert.deepEqual(rejected[field],sealed[field],`stale begin changes no ${field} or completed store`);
    assert.equal(rejected.revision,sealed.revision);
    const preciseCalls=world.cache.preciseRuns-precise;world.result.semanticComplete=true;
    checks.push(()=>{assert(owner);assert.equal(preciseCalls,0,'the compiled string kernel reports its committed SMC element without replay');});
  });
  // Every literal Context/FP/vector/RAM/version oracle precedes the first
  // architecture assertion, including on the unchanged old engine.
  for(const check of checks)check();
}
