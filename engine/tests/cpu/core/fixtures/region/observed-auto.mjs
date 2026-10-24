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

// Explicit manual compatibility has separate observed.mjs controls. Ordinary
// already-owned CALL/RET cycles must form a bounded executable automatically.
export function runObservedAutoCases(artifact,report,{baseline=false}={}) {
  observedFixture(artifact,report,'automatic-call-ret-cycle-keeps-mixed-state-in-one-window',{automaticResolver:true},world=>{
    const {cache,result,root,callee,context}=world;
    const original=[...world.units.values()].map(({unit,module})=>({unit,module,
      run:unit.run,link:unit.link,publish:unit.publish,slot:unit.slot}));
    assert.equal(cache.units.size,2);assert.equal(cache.admissions,2);
    assert.equal(typeof world.api.core_chain_resolve_owned,'function',
      'the actual stable-fragment resolver is present');

    world.reset(TRAINING);cache.budget=6*TRAINING+1;
    const before=world.snapshot(),selected=world.select(ROOT),run=cache.execute(selected);
    const training=observedLiteral(world,before,run,{pc:0x100c,retired:6*TRAINING+1,
      budget:6*TRAINING+1,registers:registers(before,TRAINING),flags:0x46,
      writes:TRAINING,stackReturn:0x1005,fpValue:integer80(3+2*TRAINING),fpPc:CALLEE,
      identity:([...cache.units.values()].find(unit=>unit.id!==root.id&&unit.id!==callee.id)??root).id});
    assert.equal(run.id,root.id);
    result.trainingSemanticComplete=true;

    const windows=[...cache.units.values()].filter(unit=>unit.id!==root.id&&unit.id!==callee.id);
    assert.equal(windows.length,1,'the normal SDK path installs one bounded hot execution window');
    const window=windows[0];
    assert.equal(world.select(window.pc),window);world.install();
    const generated=cache.generatedModuleBytes,admissions=cache.admissions,traffic=[];

    for(const iterations of[9,129]) {
      world.reset(iterations);cache.budget=6*iterations;
      const testBefore=world.snapshot(),resolveBefore=world.probes;
      const test=cache.execute(world.select(ROOT));
      const completed=observedLiteral(world,testBefore,test,{pc:0x100b,retired:6*iterations,budget:6*iterations,
        registers:registers(testBefore,iterations),flags:0x46,writes:iterations,stackReturn:0x1005,
        fpValue:integer80(3+2*iterations),fpPc:CALLEE});
      assert.equal(test.id,window.id,'the final executed code is the actual automatic window');
      assert.equal(completed.counters.body_entries,test.regions);
      assert(test.regions<=2,'at most the caller prefix and one window execute, independent of loop count');
      assert.equal(completed.counters.canonical_calls,0);
      assert.equal(completed.counters.wide_snapshot_calls,0);
      assert.equal(completed.counters.normalizer_calls,0);
      assert(world.probes-resolveBefore<=1,'only the first caller-to-window route can need native resolution');
      traffic.push(completed);
    }
    result.semanticComplete=true;
    const publicSelections=[ROOT,ROOT+5,CALLEE].map(pc=>({pc,unit:cache.select(pc)}));
    const returnChecks=[];
    for(const target of[0x1800,0x1900]) {
      if(returnChecks.length) {
        // A real completed baseline instruction consumes the previous side
        // exit's one-shot bypass before a different caller invokes the callee.
        world.reset(9,{pc:CALLEE,esp:0x47fc});cache.budget=1;
        const progressBefore=world.snapshot(),progress=cache.execute(cache.select(CALLEE));
        const progressRegisters=Array.from({length:8},(_,i)=>view(progressBefore.state).getUint32(16+i*4,true));
        observedLiteral(world,progressBefore,progress,{pc:CALLEE+2,retired:1,budget:1,
          registers:progressRegisters,flags:2,writes:0,fpValue:integer80(5),fpPc:CALLEE});
      }
      // A different caller supplies its own already-written return word. The
      // observed target misses only after the real RET read/pop has completed.
      world.engine.ok('write32',0x47fc,target);
      world.reset(9,{pc:CALLEE,esp:0x47fc});
      const selected=cache.select(CALLEE),entryBefore=world.snapshot(),test=world.rawExecute(selected,10);
      const actualRegisters=Array.from({length:8},(_,i)=>view(entryBefore.state).getUint32(16+i*4,true));
      actualRegisters[4]=0x4800;
      observedLiteral(world,entryBefore,test,{stop:1,pc:target,retired:2,budget:10,
        registers:actualRegisters,flags:2,writes:0,fpValue:integer80(5),fpPc:CALLEE});
      returnChecks.push(()=>assert.equal(selected.id,window.id,
        'a separately called callee uses the same window before its exact changed-return side exit'));
    }
    result.externalReturnSemanticComplete=true;
    for(const check of returnChecks)check();
    result.externalReturnReuseComplete=true;
    for(const name of['record_reads','record_writes'])
      assert.equal(traffic[1].counters[name],traffic[0].counters[name],
        'more mixed CALL/RET iterations add no FP pool imports or spills');
    assert.equal(cache.admissions,admissions);assert.equal(cache.units.size,3);
    assert.equal(cache.generatedModuleBytes,generated);
    assert.equal(cache.transientAdmissions,0);assert.equal(cache.transientRuns,0);
    const owners=world.raw.core_link_owners_ptr();
    for(const saved of original) {
      for(const pc of saved.unit===root?[ROOT,0x1005,0x1008,0x1009]:[CALLEE,0x1102]) {
        if(pc===window.pc)continue;
        assert.equal(world.select(pc),saved.unit,'nonoverridden secondary ports retain the immutable fragment');
        assert.equal(world.bigint(context,120),saved.unit.id);
        assert.equal(hash(world.encoded()),saved.module.sha256);
      }
      assert.equal(cache.units.get(saved.unit.id),saved.unit);
      assert.equal(saved.unit.slot,saved.slot);assert.equal(saved.unit.run,saved.run);
      assert.equal(saved.unit.link,saved.link);assert.equal(saved.unit.publish,saved.publish);
      assert.equal(cache.table.get(saved.slot),saved.link);assert.equal(cache.publishers.get(saved.slot),saved.publish);
      assert.equal(world.bigint(owners,saved.slot*8),saved.unit.id);
    }
    for(const {unit} of publicSelections)assert.equal(unit,window,
      'ordinary executable selection aliases every complete block to the same window');
    assert.equal(cache.admissions,admissions);assert.equal(cache.generatedModuleBytes,generated);
    result.automatic={trainingIterations:TRAINING,trainingRegions:run.regions,
      admissions:cache.admissions,units:cache.units.size,windows:[String(window.id)],nativeResolves:world.probes,
      baselineRequested:baseline,trainingCounters:training.counters};
  });
}
