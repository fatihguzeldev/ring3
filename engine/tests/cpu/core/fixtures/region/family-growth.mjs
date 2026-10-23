import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';

const ROOT=0x1000,B=0x1020,ITERATIONS=128;
const unhex=value=>Uint8Array.from(Buffer.from(value.replaceAll(' ',''),'hex'));
const hash=value=>createHash('sha256').update(value).digest('hex');
const A_PORTS=[0x1000,0x1002,0x1004,0x1005,0x1007,0x1008];
const B_PORTS=[0x1020,0x1023,0x1025,0x1027,0x1028];
const A_ONLY=new Set(A_PORTS.slice(2));
const REGISTERS=[0,ITERATIONS,0,0x3000,0x4800,0x12345678,0x87654321,0x99aabbcc];

function selectedSource(world) {
  return world.bytes(world.raw.core_code_ptr()>>>0,world.raw.core_code_len()>>>0);
}

function retained(world,unit) {
  const source=selectedSource(world);
  return{id:unit.id,slot:unit.slot,run:unit.run,link:unit.link,publish:unit.publish,
    sourceHash:hash(source),sourceBytes:source.length,admissions:world.cache.admissions,
    generatedBytes:world.cache.generatedModuleBytes};
}

function sameFamily(world,unit,saved,ports) {
  for(const pc of ports){
    assert.equal(world.cache.select(pc),unit,'every historical and newly observed port selects one family object');
    assert.equal(world.bigint(world.context,120),saved.id,'the family native dictionary accompanies each port');
    assert.equal(hash(selectedSource(world)),saved.sourceHash);
    assert.equal(selectedSource(world).length,saved.sourceBytes);
    assert.equal(unit.id,saved.id);assert.equal(unit.slot,saved.slot);
    assert.equal(unit.run,saved.run);assert.equal(unit.link,saved.link);assert.equal(unit.publish,saved.publish);
    assert.equal(world.cache.table.get(saved.slot),saved.link);
    assert.equal(world.cache.publishers.get(saved.slot),saved.publish);
  }
  assert.equal(world.cache.admissions,saved.admissions);
  assert.equal(world.cache.generatedModuleBytes,saved.generatedBytes);
  assert.equal(world.cache.units.size,1);
}

// Phase A supplies one real cold history. Phase B never executes A's private
// path: retaining it therefore requires the previous family's actual graph,
// rather than reconstructing a union from the latest capture alone.
export function runFamilyGrowthCases(artifact,report) {
  const code=new Uint8Array(4096);
  code.set(unhex('85d2 751c 40 d8c1 49 75f6 eb34'));
  code.set(unhex('83c003 d8c1 d8c1 49 75d6 eb14'),0x20);
  code.set(unhex('90 0f0b'),0x40);
  coldFixture(artifact,report,'automatic-cold-family-growth-retains-unvisited-historical-ports-and-edges',
    {code,registers:REGISTERS},world=>{
      const {cache,result}=world,epoch=world.bigint(world.word(world.context,88));
      let before=world.snapshot(),run=world.execute(6*ITERATIONS+2,ROOT);
      coldLiteral(world,before,run,{pc:0x1041,retired:770,budget:770,
        registers:[128,0,...REGISTERS.slice(2)],flags:0x46,writes:[],versionWrites:0,
        fpRegisters:{0:'00000000000080810740'},fpPc:0x1005,
        identity:world.bigint(world.context,120)});
      result.phaseASemanticComplete=true;
      assert.equal(cache.admissions,1);assert.equal(cache.units.size,1);
      const first=[...cache.units.values()][0];
      const original=retained(world,first);
      sameFamily(world,first,original,A_PORTS);
      assert.equal(world.raw.core_lookup(B)>>>0,4,'unexecuted B is absent before its real cold history');
      assert.equal(world.moduleRecords.size,1);
      assert.equal(result.modules.filter(module=>module.kind==='guest').length,1);
      result.firstFamily={id:String(first.id),slot:first.slot,sha256:original.sourceHash,bytes:original.sourceBytes};

      world.reset({registers:[0,ITERATIONS,1,...REGISTERS.slice(3)],flags:2,pc:ROOT});
      before=world.snapshot();run=world.execute(7*ITERATIONS+2,ROOT);
      coldLiteral(world,before,run,{pc:0x1041,retired:898,budget:898,
        registers:[384,0,1,...REGISTERS.slice(3)],flags:0x46,writes:[],versionWrites:0,
        fpRegisters:{0:'000000000000c0800840'},fpPc:0x1025,
        identity:world.bigint(world.context,120)});
      result.phaseBSemanticComplete=true;
      const capture=world.recording();result.growthCapture=capture;
      assert.equal(capture.active,0);assert.equal(capture.overflow,0);assert(capture.used>1);
      assert(capture.events.some(event=>event.kind===3&&event.owner==='0'&&event.pc===B),
        'the second capture contains an actually completed new cold B instruction');
      assert(capture.events.some(event=>event.kind===1&&event.owner===String(first.id)),
        'cold-to-family resumption retains the actual old owner as source authority');
      assert(capture.events.every(event=>!A_ONLY.has(event.pc)),
        'the new capture never supplies A-only instructions or their outgoing edges');
      assert.equal(world.bigint(world.word(world.context,88)),epoch,
        'both phases use unchanged executable-source authority');
      assert.equal(cache.admissions,2,'new executable nodes and internal edges replace the one family exactly once');
      assert.equal(cache.units.size,1,'historical growth retains one owner rather than a rotated-root clone');
      assert.equal(cache.units.has(first.id),false);
      const grown=[...cache.units.values()][0],saved=retained(world,grown);
      assert.notEqual(grown.id,first.id);
      assert.equal(world.moduleRecords.size,2);
      assert.equal(result.modules.filter(module=>module.kind==='guest').length,2);
      result.grownFamily={id:String(grown.id),slot:grown.slot,selectedPort:grown.pc,
        sha256:saved.sourceHash,bytes:saved.sourceBytes};
      sameFamily(world,grown,saved,[...A_PORTS,...B_PORTS]);

      for(const test of[
        {name:'historical-A-loop',mode:0,budget:54,pc:0x100a,eax:9,fp:'00000000000000a80340',fip:0x1005},
        {name:'new-B-loop',mode:1,budget:63,pc:0x102a,eax:27,fp:'000000000000009c0440',fip:0x1025},
      ]){
        world.reset({registers:[0,9,test.mode,...REGISTERS.slice(3)],flags:2,pc:ROOT});
        before=world.snapshot();run=world.execute(test.budget,ROOT);
        coldLiteral(world,before,run,{pc:test.pc,retired:test.budget,budget:test.budget,
          registers:[test.eax,0,test.mode,...REGISTERS.slice(3)],flags:0x46,
          writes:[],versionWrites:0,fpRegisters:{0:test.fp},fpPc:test.fip});
        result.runs.at(-1).phase=test.name;
        assert.equal(run.id,grown.id);assert.equal(run.regions,1,
          'the retained old/new executable edges stay inside the same actual body');
        assert.equal(run.chainedColdWork,0);assert.equal(world.helperCalls.length,before.helpers);
      }
      for(const pc of[...A_PORTS,...B_PORTS]){
        world.reset({registers:REGISTERS,flags:2,pc});before=world.snapshot();run=world.execute(0,pc);
        coldLiteral(world,before,run,{pc,retired:0,budget:0,registers:REGISTERS,flags:2,
          fp:before.fp,writes:[],versionWrites:0});
        assert.equal(run.id,grown.id,'zero-quota historical and new ports use the same validated owner');
      }
      sameFamily(world,grown,saved,[...A_PORTS,...B_PORTS]);
      assert.equal(cache.transientAdmissions,0);assert.equal(cache.transientRuns,0);
      assert.equal(world.prepares,0);result.semanticComplete=true;
    });
}
