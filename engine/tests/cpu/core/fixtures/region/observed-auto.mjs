import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {view} from '../../../../../../sdk/runtime/engine.js';
import {observedFixture,observedLiteral} from './observed.mjs';

const ROOT=0x1000,CALLEE=0x1100,TRAINING=2048;
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');

function integer80(value) {
  let integer=BigInt(value),bits=0;
  assert(integer>0n);
  for(let remaining=integer;remaining;remaining>>=1n)bits++;
  assert(bits<=64);
  const raw=Buffer.alloc(10);
  raw.writeBigUInt64LE(integer<<BigInt(64-bits));raw.writeUInt16LE(0x3fff+bits-1,8);
  return raw.toString('hex');
}

assert.equal(integer80(3),'00000000000000c00040');
assert.equal(integer80(21),'00000000000000a80340');
assert.equal(integer80(4099),'00000000000018800b40');

function registers(before,iterations) {
  const result=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));
  result[0]=iterations;result[1]=0;return result;
}

// Explicit manual trace compatibility has separate observed.mjs controls.
// Ordinary already-owned CALL/RET edges learn connectivity without emitting a
// historical union or changing either fragment's source/ordinal dictionary.
export function runObservedAutoCases(artifact,report,{baseline=false}={}) {
  observedFixture(artifact,report,'observed-production-owned-edges-keep-stable-fragments',{},world=>{
    const {cache,result,root,callee,context}=world;
    const original=[...world.units.values()].map(({unit,module})=>({unit,module,
      run:unit.run,link:unit.link,publish:unit.publish,slot:unit.slot}));
    assert.equal(cache.units.size,2);assert.equal(cache.admissions,2);
    const generated=cache.generatedModuleBytes;
    assert.equal(typeof world.api.core_chain_resolve_owned,'function',
      'the actual stable-fragment resolver is present');

    world.reset(TRAINING);cache.budget=6*TRAINING+1;
    const before=world.snapshot(),selected=world.select(ROOT),run=cache.execute(selected);
    const training=observedLiteral(world,before,run,{pc:0x100c,retired:6*TRAINING+1,
      budget:6*TRAINING+1,registers:registers(before,TRAINING),flags:0x46,
      writes:TRAINING,stackReturn:0x1005,fpValue:integer80(3+2*TRAINING),fpPc:CALLEE});
    assert.equal(run.id,root.id);
    result.trainingSemanticComplete=true;

    for(let replay=0;replay<2;replay++) {
      world.reset(9);cache.budget=54;
      const testBefore=world.snapshot(),resolveBefore=world.probes;
      const test=cache.execute(world.select(ROOT));
      const completed=observedLiteral(world,testBefore,test,{pc:0x100b,retired:54,budget:54,
        registers:registers(testBefore,9),flags:0x46,writes:9,stackReturn:0x1005,
        fpValue:integer80(21),fpPc:CALLEE});
      assert.equal(test.id,root.id,'the actual final caller retains its original dictionary');
      assert.equal(completed.counters.body_entries,test.regions);
      assert.equal(completed.counters.canonical_calls,0);
      assert.equal(completed.counters.wide_snapshot_calls,0);
      assert.equal(completed.counters.normalizer_calls,0);
      assert.equal(world.probes,resolveBefore,'warm fragment transitions require no native ownership resolution');
    }
    result.semanticComplete=true;

    const overlays=[...cache.units.values()].filter(unit=>unit.id!==root.id&&unit.id!==callee.id);
    assert.equal(overlays.length,0,'fully owned automatic captures emit no replacement executor');
    assert.equal(cache.admissions,2);assert.equal(cache.units.size,2);
    assert.equal(cache.generatedModuleBytes,generated);
    assert.equal(cache.transientAdmissions,0);assert.equal(cache.transientRuns,0);
    const owners=world.raw.core_link_owners_ptr();
    for(const saved of original) {
      for(const pc of saved.unit===root?[ROOT,0x1005,0x1008,0x1009]:[CALLEE,0x1102]) {
        assert.equal(world.select(pc),saved.unit,'explicit secondary-port admission retains the immutable fragment');
        assert.equal(world.bigint(context,120),saved.unit.id);
        assert.equal(hash(world.encoded()),saved.module.sha256);
      }
      assert.equal(cache.units.get(saved.unit.id),saved.unit);
      assert.equal(saved.unit.slot,saved.slot);assert.equal(saved.unit.run,saved.run);
      assert.equal(saved.unit.link,saved.link);assert.equal(saved.unit.publish,saved.publish);
      assert.equal(cache.table.get(saved.slot),saved.link);assert.equal(cache.publishers.get(saved.slot),saved.publish);
      assert.equal(world.bigint(owners,saved.slot*8),saved.unit.id);
    }
    assert.equal(cache.admissions,2);assert.equal(cache.generatedModuleBytes,generated);
    result.automatic={trainingIterations:TRAINING,trainingRegions:run.regions,
      admissions:cache.admissions,units:cache.units.size,overlays:[],nativeResolves:world.probes,
      baselineRequested:baseline,trainingCounters:training.counters};
  });
}
