import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {readEngine} from '../../../../support/wasm/engine.mjs';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';

const COUNT=4097,BASE=0x10000,STRIDE=4,ITERATIONS=128,WORK=ITERATIONS*2;
const BYTE_BUDGET=128*1024*1024;
const hash=value=>createHash('sha256').update(value).digest('hex');
const registers=pc=>[0,0,0x11223344,0x3000,0x4800,0x55667788,pc,0x99aabbcc];
const pcAt=index=>BASE+index*STRIDE;

function selectedBytes(world) {
  return world.bytes(world.raw.core_code_ptr()>>>0,world.raw.core_code_len()>>>0);
}

function executeLiteral(world,pc) {
  world.reset({registers:registers(pc),flags:2,pc});
  const before=world.snapshot(),run=world.execute(WORK,pc);
  // INC from127 to128 sets AF, clears PF/ZF/SF/OF and preserves incomingCF0.
  coldLiteral(world,before,run,{pc,retired:WORK,budget:WORK,
    registers:[ITERATIONS,...registers(pc).slice(1)],flags:0x12,
    fp:before.fp,writes:[],versionWrites:0,identity:world.bigint(world.context,120)});
  return run;
}

function remember(world,unit) {
  const source=selectedBytes(world);
  return{unit,id:unit.id,slot:unit.slot,run:unit.run,link:unit.link,publish:unit.publish,
    bytes:source.length,sha256:hash(source)};
}

function assertRetained(world,remembered,pc) {
  assert.equal(world.cache.units.get(remembered.id),remembered.unit,'the same actual immutable SDK instance remains retained');
  for(const port of[pc,pc+1]) {
    assert.equal(world.selectSource(port),remembered.unit,'both exact original source instruction ports retain their owner');
    assert.equal(world.bigint(world.context,120),remembered.id);
    const source=selectedBytes(world);assert.equal(source.length,remembered.bytes);assert.equal(hash(source),remembered.sha256);
  }
  assert.equal(remembered.unit.slot,remembered.slot);
  assert.equal(remembered.unit.run,remembered.run);assert.equal(remembered.unit.link,remembered.link);
  assert.equal(remembered.unit.publish,remembered.publish);
  assert.equal(world.cache.table.get(remembered.slot),remembered.link);
  assert.equal(world.cache.publishers.get(remembered.slot),remembered.publish);
  assert.equal(world.bigint(world.raw.core_link_owners_ptr(),remembered.slot*8),remembered.id);
}

// Every source page is uploaded before any family is recorded. No native
// insert/test-only owner, eager baseline, source mutation or forged history is
// involved: completed cold INC/JMP instructions train each ordinary family.
export function runFamilyResidencyCases(artifact,report) {
  coldFixture(artifact,report,'byte-current-4097-families-outlive-the-old-executable-slot-ceiling',
    {code:'90',registers:registers(BASE)},world=>{
      const pageCount=Math.ceil(COUNT*STRIDE/4096),pages=Array.from({length:pageCount},()=>new Uint8Array(4096));
      for(let index=0;index<COUNT;index++)pages[Math.floor(index*STRIDE/4096)].set([0x40,0xff,0xe6],index*STRIDE%4096);
      for(let index=0;index<pages.length;index++) {
        const address=BASE+index*4096;
        world.engine.ok('map',address,1,7);world.engine.request(pages[index]);world.engine.ok('upload',address,4096);
      }
      const epoch=world.bigint(world.word(world.context,88)),revision=world.revision();
      const initialRam=world.snapshot().ram,initialRamHash=hash(initialRam),rows=[];
      let first,last;
      for(let index=0;index<COUNT;index++) {
        const pc=pcAt(index),run=executeLiteral(world,pc);
        const id=world.bigint(world.context,120),unit=world.cache.units.get(id);
        assert(unit,'each completed hot loop installs a real source-owned observed family');
        const source=selectedBytes(world);
        rows.push({index,pc,id:String(id),slot:unit.slot,bytes:source.length,sha256:hash(source),kind:run.kind});
        if(index===0)first=remember(world,unit);
        if(index===COUNT-1)last=remember(world,unit);
        assert.equal(world.bigint(world.word(world.context,88)),epoch,'all families share the unchanged original source authority');
        assert.equal(world.revision(),revision,'recording/compilation never performs a guest write');
        // Retain only byte fingerprints in the test oracle. The fixture must
        // not act as a second encoded-code cache outside the production SDK.
        const record=world.moduleRecords.get(id);if(record)delete record.source;
      }
      world.result.families=rows;
      const training=world.cache.sampleUsage();world.result.afterTraining=training;
      assert.equal(hash(world.bytes(world.word(world.context,64),initialRam.length)),initialRamHash);

      // Complete both replays before testing residency. The old implementation
      // still computes every architectural byte correctly, but reconstructs
      // the first graph after count eviction despite unchanged source/bytes.
      const lastReplay=executeLiteral(world,pcAt(COUNT-1));
      const firstReplay=executeLiteral(world,BASE);
      world.result.replaySemanticComplete=true;
      world.result.first={id:String(first.id),slot:first.slot,bytes:first.bytes,sha256:first.sha256};
      world.result.last={id:String(last.id),slot:last.slot,bytes:last.bytes,sha256:last.sha256};
      world.result.semanticComplete=true;
      const after=world.cache.sampleUsage();world.result.afterReplay=after;

      assert.equal(BigInt(training.nativeCache.countEvictions),0n,
        'executable slot count must not discard byte-current graph history below the byte budget');
      assert.equal(BigInt(training.nativeCache.byteEvictions),0n);
      assert.equal(BigInt(training.nativeCache.sourceInvalidations),0n);
      assert(BigInt(training.nativeCache.retainedEncodedBytes)<BigInt(BYTE_BUDGET),
        'this count-boundary witness does not exhaust the encoded-byte budget');
      assert.equal(BigInt(training.nativeCache.residentBudget),BigInt(BYTE_BUDGET));
      assert(BigInt(training.nativeCache.residentCharge)<=BigInt(training.nativeCache.residentBudget),
        'code, immutable history, indexes and slot storage share the real resident budget');
      assert.equal(training.admissions,COUNT);assert.equal(training.installedUnits,COUNT);
      assert.equal(BigInt(training.nativeCache.retainedUnits),BigInt(COUNT));
      assert.equal(lastReplay.kind,'compiled');assert.equal(lastReplay.id,last.id);
      assert.equal(lastReplay.regions,1);assert.equal(lastReplay.chainedColdWork,0);
      assert.equal(firstReplay.kind,'compiled');assert.equal(firstReplay.id,first.id);
      assert.equal(firstReplay.regions,1);assert.equal(firstReplay.chainedColdWork,0);
      assertRetained(world,first,BASE);assertRetained(world,last,pcAt(COUNT-1));
      assert.equal(after.admissions,training.admissions);assert.equal(after.generatedModuleBytes,training.generatedModuleBytes);
      assert.equal(after.nativeCache.compiled,training.nativeCache.compiled);
      assert.equal(after.nativeCache.admittedEncodedBytes,training.nativeCache.admittedEncodedBytes);
      assert.equal(after.nativeCache.countEvictions,'0');assert.equal(after.nativeCache.byteEvictions,'0');
      assert.equal(after.nativeCache.sourceInvalidations,'0');
      assert.equal(after.nativeCache.residentBudget,training.nativeCache.residentBudget);
      assert(BigInt(after.nativeCache.residentCharge)<=BigInt(after.nativeCache.residentBudget));

      assert(world.cache.table.length>4097,'the actual executable table has a usable slot beyond4096');
      assert.equal(world.cache.publishers.length,world.cache.table.length,'both typed tables grow together');
      assert(last.slot>4096,'the final real owner executes through a new reciprocal high slot');
      const owners=world.raw.core_link_owners_ptr(),slots=new Set();
      for(const unit of world.cache.units.values()) {
        assert(unit.slot>0&&unit.slot<world.cache.table.length);assert(!slots.has(unit.slot));slots.add(unit.slot);
        assert.equal(world.bigint(owners,unit.slot*8),unit.id,'native directory agrees with every actual table identity');
        assert.equal(world.cache.table.get(unit.slot),unit.link);assert.equal(world.cache.publishers.get(unit.slot),unit.publish);
      }
      assert.equal(world.result.modules.filter(module=>module.kind==='guest').length,COUNT,
        'byte-current replays construct no replacement Wasm module');
      assert.equal(world.prepares,0);assert.equal(world.cache.transientAdmissions,0);
      assert.equal(world.bigint(world.word(world.context,88)),epoch);assert.equal(world.revision(),revision);
      world.result.residencyComplete=true;
    });
}

if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href) {
  const[enginePath,reportPath]=process.argv.slice(2);
  assert(enginePath&&reportPath,'usage: family-residency.mjs engine.wasm fresh-report.json');
  const artifact=readEngine(enginePath),sources=['./family-residency.mjs','../../../../support/core_wasm/cold_observed.mjs',
    '../../../../../../sdk/runtime/cache.js'];
  const report={schema:1,complete:false,engineSha256:artifact.sha256,
    sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),
    scope:'actual source-current owner residency across the former4096-slot boundary; no timing or V8 native-memory claim',cases:[]};
  closeSync(openSync(reportPath,'wx'));
  try {runFamilyResidencyCases(artifact,report);report.complete=true;}
  catch(error){report.failure={message:String(error),stack:error.stack};throw error;}
  finally{writeFileSync(reportPath,JSON.stringify(report,null,2));}
  console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,cases:report.cases.length}));
}
