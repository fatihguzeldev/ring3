import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {readEngine} from '../../../../support/wasm/engine.mjs';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';

const COUNT=4097,BASE=0x10000,STRIDE=4,ITERATIONS=128,WORK=ITERATIONS*2;
const BYTE_BUDGET=128*1024*1024;
const INITIAL_CREDIT=4*1024*1024,CREDIT_WORK=32,PERMIT=16*1024+2*32;
const FUND_WORK=PERMIT*CREDIT_WORK,FUND_ITERATIONS=FUND_WORK/2;
const hash=value=>createHash('sha256').update(value).digest('hex');
const registers=pc=>[0,0,0x11223344,0x3000,0x4800,0x55667788,pc,0x99aabbcc];
const pcAt=index=>BASE+index*STRIDE;

function selectedBytes(world) {
  return world.bytes(world.raw.core_code_ptr()>>>0,world.raw.core_code_len()>>>0);
}

function executeLiteral(world,pc,iterations=ITERATIONS,flags=0x12) {
  world.reset({registers:registers(pc),flags:2,pc});
  const budget=iterations*2,before=world.snapshot(),run=world.execute(budget,pc);
  // the authored INC counts have literal final flags:128→0x12,0x40400→0x16.
  coldLiteral(world,before,run,{pc,retired:budget,budget,
    registers:[iterations,...registers(pc).slice(1)],flags,
    fp:before.fp,writes:[],versionWrites:0,identity:world.bigint(world.context,120)});
  return run;
}

function remember(world,unit) {
  const source=selectedBytes(world);
  return{unit,id:unit.id,slot:unit.slot,run:unit.run,link:unit.link,publish:unit.publish,
    bytes:source.length,sha256:hash(source)};
}

function assertRetained(world,remembered,pc) {
  const unit=world.cache.units.get(remembered.id);
  assert(unit,'the original actual owner remains retained');
  if(remembered.unit)assert.equal(unit,remembered.unit,'the same actual immutable SDK instance remains retained');
  for(const port of[pc,pc+1]) {
    assert.equal(world.selectSource(port),unit,'both exact original source instruction ports retain their owner');
    assert.equal(world.bigint(world.context,120),remembered.id);
    const source=selectedBytes(world);assert.equal(source.length,remembered.bytes);assert.equal(hash(source),remembered.sha256);
  }
  assert.equal(unit.slot,remembered.slot);
  if(remembered.unit) {
    assert.equal(unit.run,remembered.run);assert.equal(unit.link,remembered.link);
    assert.equal(unit.publish,remembered.publish);
  }
  assert.equal(world.cache.table.get(remembered.slot),unit.link);
  assert.equal(world.cache.publishers.get(remembered.slot),unit.publish);
  assert.equal(world.bigint(world.raw.core_link_owners_ptr(),remembered.slot*8),remembered.id);
}

// every source page is uploaded before any recording. automatic policy may defer
// an optional owner; completed guest execution and source authority remain exact.
export function runFamilyResidencyCases(artifact,report) {
  coldFixture(artifact,report,'earned-family-admission-preserves-4097-exact-cold-loops-and-retained-owners',
    {code:'90',registers:registers(BASE)},world=>{
      const pageCount=Math.ceil(COUNT*STRIDE/4096),pages=Array.from({length:pageCount},()=>new Uint8Array(4096));
      for(let index=0;index<COUNT;index++)pages[Math.floor(index*STRIDE/4096)].set([0x40,0xff,0xe6],index*STRIDE%4096);
      for(let index=0;index<pages.length;index++) {
        const address=BASE+index*4096;
        world.engine.ok('map',address,1,7);world.engine.request(pages[index]);world.engine.ok('upload',address,4096);
      }
      const epoch=world.bigint(world.word(world.context,88)),revision=world.revision();
      const initialRam=world.snapshot().ram,initialRamHash=hash(initialRam),rows=[],owners=new Map();
      world.result.families=rows;
      let first,firstDeferred,lastAdmittedId,exhaustedReplay=false;
      const recordOwner=(pc,unit)=>{
        const source=selectedBytes(world);
        const row={pc,id:unit.id,slot:unit.slot,bytes:source.length,sha256:hash(source)};
        const existing=owners.get(unit.id);
        if(existing)assert.deepEqual(row,existing,'the same immutable owner keeps exact source bytes and slot');
        else {
          if(lastAdmittedId!==undefined)assert.equal(unit.id,lastAdmittedId+1n,'deferred leases consume no hidden guest identity');
          lastAdmittedId=unit.id;owners.set(unit.id,row);
        }
        const record=world.moduleRecords.get(unit.id);if(record)delete record.source;
        return row;
      };
      const assertBound=usage=>{
        const earned=Math.floor(usage.retired/CREDIT_WORK),ceiling=Math.floor((INITIAL_CREDIT+earned)/PERMIT);
        assert(usage.admissions<=ceiling,'emitted automatic owners cannot exceed genuine completed-work credit');
        assert.equal(BigInt(usage.nativeCache.compiled),BigInt(usage.admissions),'every completed native compilation installed its actual immutable owner');
        assert.equal(BigInt(usage.nativeCache.admittedEncodedBytes),BigInt(usage.generatedModuleBytes));
        assert.equal(usage.admissions,usage.installedUnits);assert.equal(usage.installedUnits,owners.size);
        assert.equal(BigInt(usage.nativeCache.retainedUnits),BigInt(owners.size));
        for(const field of['countEvictions','byteEvictions','sourceInvalidations'])assert.equal(BigInt(usage.nativeCache[field]),0n);
        assert.equal(BigInt(usage.nativeCache.residentBudget),BigInt(BYTE_BUDGET));
        assert(BigInt(usage.nativeCache.residentCharge)<=BigInt(BYTE_BUDGET),'all storage stays within the unchanged resident budget');
        assert(BigInt(usage.nativeCache.retainedEncodedBytes)<BigInt(BYTE_BUDGET));
        assert.equal(usage.evictions,0);assert.equal(usage.transientAdmissions,0);
        assert.equal(world.prepares,0);assert.equal(world.bigint(world.word(world.context,88)),epoch);
        assert.equal(world.revision(),revision);
      };
      const assertNoEmission=(before,after,modules)=>{
        assert.equal(after.admissions,before.admissions);assert.equal(after.generatedModuleBytes,before.generatedModuleBytes);
        assert.equal(after.nativeCache.compiled,before.nativeCache.compiled);
        assert.equal(after.nativeCache.admittedEncodedBytes,before.nativeCache.admittedEncodedBytes);
        assert.equal(world.result.modules.filter(module=>module.kind==='guest').length,modules);
      };
      for(let index=0;index<COUNT;index++) {
        const pc=pcAt(index),run=executeLiteral(world,pc);
        const id=world.bigint(world.context,120),unit=world.cache.units.get(id);
        if(unit) {
          const row=recordOwner(pc,unit);rows.push({index,...row,id:String(id),admitted:true,kind:run.kind});
          if(!first)first=remember(world,unit);
        } else {
          assert.equal(id,0n,'a deferred exact cold loop has no manufactured owner identity');
          assert.equal(run.kind,'cold');assert.equal(run.regions,0);
          rows.push({index,pc,id:'0',slot:null,bytes:0,sha256:null,admitted:false,kind:run.kind});
          if(firstDeferred===undefined) {
            firstDeferred=pc;
            const before=world.cache.sampleUsage(),modules=world.result.modules.filter(module=>module.kind==='guest').length;
            const replay=executeLiteral(world,pc),after=world.cache.sampleUsage();
            assert.equal(replay.kind,'cold');assert.equal(replay.regions,0);assert.equal(world.bigint(world.context,120),0n);
            assertNoEmission(before,after,modules);assertBound(after);
            world.result.exhaustedReplay={pc,retired:replay.retired,work:replay.work,identity:'0',lastAdmittedIdentity:String(lastAdmittedId),before,after};
            exhaustedReplay=true;
          }
        }
        assert.equal(world.bigint(world.word(world.context,88)),epoch,'all loops keep their unchanged original source authority');
        assert.equal(world.revision(),revision,'compilation and exact execution perform no guest writes');
      }
      const training=world.cache.sampleUsage();world.result.afterTraining=training;
      assert(first&&exhaustedReplay&&firstDeferred!==undefined,'the bounded initial credit admits real owners and then defers');
      assert.equal(rows.length,COUNT);assert(rows.some(row=>!row.admitted));assert(owners.size<COUNT);
      assert.equal(training.retired,(COUNT+1)*WORK,'all4097 roots and the exhausted replay completed their full literal work');
      assertBound(training);assert.equal(hash(world.bytes(world.word(world.context,64),initialRam.length)),initialRamHash);

      // retain the original last-root and first-root architectural replays.
      const lastReplay=executeLiteral(world,pcAt(COUNT-1)),lastId=world.bigint(world.context,120);
      if(lastId)recordOwner(pcAt(COUNT-1),world.cache.units.get(lastId));
      const firstReplay=executeLiteral(world,BASE);
      assert.equal(firstReplay.kind,'compiled');assert.equal(firstReplay.id,first.id);
      assert.equal(firstReplay.regions,1);assert.equal(firstReplay.chainedColdWork,0);assertRetained(world,first,BASE);
      world.result.replaySemanticComplete=true;
      world.result.lastReplay={kind:lastReplay.kind,id:lastReplay.id===null?null:String(lastReplay.id)};

      assert(![...world.cache.units.values()].some(unit=>unit.pc===firstDeferred),'the exhausted root is still genuinely uncompiled');
      const beforeFund=world.cache.sampleUsage(),modulesBeforeFund=world.result.modules.filter(module=>module.kind==='guest').length;
      const fund=executeLiteral(world,BASE,FUND_ITERATIONS,0x16),afterFund=world.cache.sampleUsage();
      assert.equal(fund.kind,'compiled');assert.equal(fund.id,first.id);assert.equal(fund.regions,1);assert.equal(fund.chainedColdWork,0);
      assertNoEmission(beforeFund,afterFund,modulesBeforeFund);assertBound(afterFund);
      assert.equal(afterFund.retired-beforeFund.retired,FUND_WORK,'real successfully completed instructions earn a complete fresh permit');
      world.result.earnedWork={retired:FUND_WORK,iterations:FUND_ITERATIONS,flags:0x16,before:beforeFund,after:afterFund};

      const retry=executeLiteral(world,firstDeferred),retryId=world.bigint(world.context,120),retryUnit=world.cache.units.get(retryId);
      assert(retryUnit&&retryId>0n,'the same source-current deferred root can earn an actual owner later');
      recordOwner(firstDeferred,retryUnit);
      const rememberedRetry=remember(world,retryUnit),afterRetry=world.cache.sampleUsage();
      assert.equal(afterRetry.admissions,afterFund.admissions+1);assert.equal(afterRetry.installedUnits,afterFund.installedUnits+1);
      assert.equal(BigInt(afterRetry.nativeCache.compiled),BigInt(afterFund.nativeCache.compiled)+1n);
      assert.equal(world.result.modules.filter(module=>module.kind==='guest').length,modulesBeforeFund+1);
      const warm=executeLiteral(world,firstDeferred),after=world.cache.sampleUsage();
      assert.equal(warm.kind,'compiled');assert.equal(warm.id,retryUnit.id);assert.equal(warm.regions,1);assert.equal(warm.chainedColdWork,0);
      assertNoEmission(afterRetry,after,modulesBeforeFund+1);assertBound(after);
      assertRetained(world,rememberedRetry,firstDeferred);assertRetained(world,first,BASE);
      for(const owner of owners.values())assertRetained(world,owner,owner.pc);
      assert.equal(world.cache.publishers.length,world.cache.table.length,'both typed tables retain the same bounded usable slots');
      const nativeOwners=world.raw.core_link_owners_ptr(),slots=new Set();
      for(const unit of world.cache.units.values()) {
        assert(unit.slot>0&&unit.slot<world.cache.table.length);assert(!slots.has(unit.slot));slots.add(unit.slot);
        assert.equal(world.bigint(nativeOwners,unit.slot*8),unit.id,'every actual table identity has reciprocal native authority');
        assert.equal(world.cache.table.get(unit.slot),unit.link);assert.equal(world.cache.publishers.get(unit.slot),unit.publish);
      }
      assert.equal(world.result.modules.filter(module=>module.kind==='guest').length,owners.size);
      assert.equal(hash(world.bytes(world.word(world.context,64),initialRam.length)),initialRamHash);
      assert.equal(world.bigint(world.word(world.context,88)),epoch);assert.equal(world.revision(),revision);
      world.result.first={id:String(first.id),slot:first.slot,bytes:first.bytes,sha256:first.sha256};
      world.result.retry={pc:firstDeferred,id:String(retryUnit.id),slot:retryUnit.slot,kind:retry.kind};
      world.result.owners=[...owners.values()].map(owner=>({...owner,id:String(owner.id)}));
      world.result.afterReplay=after;world.result.semanticComplete=true;world.result.residencyComplete=true;
    });
}

if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href) {
  const[enginePath,reportPath]=process.argv.slice(2);
  assert(enginePath&&reportPath,'usage: family-residency.mjs engine.wasm fresh-report.json');
  const artifact=readEngine(enginePath),sources=['./family-residency.mjs','../../../../support/core_wasm/cold_observed.mjs',
    '../../../../../../sdk/runtime/cache.js'];
  const report={schema:1,complete:false,engineSha256:artifact.sha256,
    sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),
    scope:'4097 exact source-current loops under earned automatic admission; deferred cold progress, genuine earned retry and all retained owner reciprocity; no timing or V8 native-memory claim',cases:[]};
  closeSync(openSync(reportPath,'wx'));
  try {runFamilyResidencyCases(artifact,report);report.complete=true;}
  catch(error){report.failure={message:String(error),stack:error.stack};throw error;}
  finally{writeFileSync(reportPath,JSON.stringify(report,null,2));}
  console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,cases:report.cases.length}));
}
