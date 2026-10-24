import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';

const ROOT=0x1000,INTERIOR=0x1005,TRAIN=1200;
const registers=eax=>[eax,0,0,0x3000,0x4800,0x12345678,INTERIOR,0x99aabbcc];
const digest=bytes=>createHash('sha256').update(bytes).digest('hex');
const source=world=>world.bytes(world.raw.core_code_ptr()>>>0,world.raw.core_code_len()>>>0);

export function runExecutableInteriorCases(artifact,report) {
  coldFixture(artifact,report,'source-owned-interior-acquires-one-real-hot-cut',{
    code:'b807000000 8d4001 8d4002 ffe6',registers:[...registers(0).slice(0,6),ROOT,0x99aabbcc],
  },world=>{
    const {cache,result}=world;
    let before=world.snapshot(),run=world.execute(4*TRAIN,ROOT);
    coldLiteral(world,before,run,{pc:ROOT,retired:4*TRAIN,budget:4*TRAIN,
      registers:[...registers(10).slice(0,6),ROOT,0x99aabbcc],flags:2,fp:before.fp,
      identity:world.bigint(world.context,120)});
    result.originalSemanticComplete=true;
    assert.equal(cache.units.size,1);assert.equal(cache.admissions,1);
    const original=[...cache.units.values()][0],saved={id:original.id,slot:original.slot,
      run:original.run,link:original.link,publish:original.publish,hash:digest(source(world))};
    assert.equal(world.raw.core_lookup(INTERIOR)>>>0,0,'legacy source capability includes its precise interior');
    assert.equal(world.bigint(world.context,120),saved.id);

    world.reset({registers:registers(0x12345678),pc:INTERIOR,flags:2});
    before=world.snapshot();run=world.execute(3*TRAIN,INTERIOR);
    coldLiteral(world,before,run,{pc:INTERIOR,retired:3*TRAIN,budget:3*TRAIN,
      registers:registers(0x12345678+3*TRAIN),flags:2,fp:before.fp,
      identity:world.bigint(world.context,120)});
    result.interiorSemanticComplete=true;
    assert.equal(cache.admissions,2,'owned source without a hot port must form one real executable cut');
    assert.equal(cache.units.size,2);
    const cut=[...cache.units.values()].find(unit=>unit.id!==saved.id);
    assert(cut);assert.equal(cache.select(INTERIOR),cut);
    const admissions=cache.admissions,generated=cache.generatedModuleBytes;
    for(let replay=0;replay<2;replay++) {
      world.reset({registers:registers(0x76543210),pc:INTERIOR,flags:2});
      const precise=cache.preciseRuns,resolvers=world.resolverCalls;
      before=world.snapshot();run=world.execute(27,INTERIOR);
      coldLiteral(world,before,run,{pc:INTERIOR,retired:27,budget:27,
        registers:registers(0x76543210+27),flags:2,fp:before.fp});
      assert.equal(run.id,cut.id);assert.equal(run.regions,1);
      assert.equal(cache.preciseRuns,precise,'warm hot cut needs no exact prefix execution');
      assert.equal(world.resolverCalls,resolvers,'warm hot cut needs no native resolution');
      assert.equal(run.chainedColdWork,0);
    }
    for(const budget of[1,2]) {
      world.reset({registers:registers(17),pc:INTERIOR,flags:2});
      before=world.snapshot();run=world.execute(budget,INTERIOR);
      coldLiteral(world,before,run,{pc:budget===1?0x1008:0x100b,retired:budget,budget,
        registers:registers(budget===1?18:20),flags:2,fp:before.fp});
    }
    assert.equal(cache.admissions,admissions);assert.equal(cache.generatedModuleBytes,generated);
    world.reset({registers:registers(41),pc:INTERIOR,flags:2});
    assert.equal(world.raw.core_lookup(INTERIOR)>>>0,0);
    assert.equal(world.bigint(world.context,120),saved.id,'legacy source lookup retains the original owner');
    cache.budget=3;before=world.snapshot();run=cache.execute(original);result.runs.push({...run,id:String(run.id)});
    coldLiteral(world,before,run,{pc:INTERIOR,retired:3,budget:3,registers:registers(44),
      flags:2,fp:before.fp,identity:saved.id});
    assert.equal(world.raw.core_lookup(ROOT)>>>0,0);
    assert.equal(world.bigint(world.context,120),saved.id);assert.equal(digest(source(world)),saved.hash);
    assert.equal(cache.units.get(saved.id),original);assert.equal(original.slot,saved.slot);
    assert.equal(original.run,saved.run);assert.equal(original.link,saved.link);assert.equal(original.publish,saved.publish);
    assert.equal(cache.table.get(saved.slot),saved.link);assert.equal(cache.publishers.get(saved.slot),saved.publish);
    assert.equal(cache.admissions,admissions);assert.equal(cache.generatedModuleBytes,generated);
    assert.equal(world.helperCalls.length,0);assert.equal(world.prepares,0);result.semanticComplete=true;
  });
}
