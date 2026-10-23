import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {view} from '../../../../../../sdk/runtime/engine.js';
import {observedFixture,observedLiteral} from './observed.mjs';
import {readWitness} from './witness.mjs';

const ROOT=0x1000,CALLEE=0x1100,TRAINING=2048;
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');

// Integer arithmetic is exact here. This constructs the architectural 64-bit
// significand directly; it does not ask the CPU under test to encode its result.
function integer80(value) {
  let integer=BigInt(value),bits=0;
  assert(integer>0n);
  for(let remaining=integer;remaining;remaining>>=1n)bits++;
  assert(bits<=64);
  const raw=Buffer.alloc(10);
  raw.writeBigUInt64LE(integer<<BigInt(64-bits));
  raw.writeUInt16LE(0x3fff+bits-1,8);
  return raw.toString('hex');
}

assert.equal(integer80(3),'00000000000000c00040');
assert.equal(integer80(21),'00000000000000a80340');
assert.equal(integer80(4099),'00000000000018800b40');

function registers(before,iterations) {
  const result=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));
  result[0]=iterations;result[1]=0;return result;
}

function capture(world) {
  const pointer=world.raw.core_trace_status_ptr?.()??world.raw.core_trace_pointer?.();
  assert(Number.isInteger(pointer)&&pointer>0&&pointer%8===0);
  assert(pointer+32+129*24<=world.memory.buffer.byteLength);
  const header=new DataView(world.memory.buffer,pointer,32);
  const used=header.getUint32(4,true);
  assert.equal(header.getUint32(0,true),0);assert.equal(header.getUint32(8,true),129);
  assert.equal(header.getUint32(12,true),0);assert(used>1&&used<=129);
  const events=Array.from({length:used},(_,i)=>{
    const row=new DataView(world.memory.buffer,pointer+32+24*i,24);
    return{kind:row.getUint32(0,true),pc:row.getUint32(4,true),end:row.getUint32(8,true),
      offset:row.getUint32(12,true),owner:String(row.getBigUint64(16,true))};
  });
  return{used,owners:[...new Set(events.map(event=>event.owner))],events};
}

// No force-recording API is called here. The normal dispatcher chooses a root,
// the normal SDK poll compiles/installs it, and normal selection consumes it.
export function runObservedAutoCases(artifact,report,{baseline=false}={}) {
  observedFixture(artifact,report,'observed-production-auto-record-install-and-consume',{},world=>{
    const {cache,result,root,callee,context}=world;
    const original=[...world.units.values()].map(({unit,module})=>({unit,module,
      run:unit.run,link:unit.link,publish:unit.publish}));
    assert.equal(cache.units.size,2);assert.equal(cache.admissions,2);

    world.reset(TRAINING);cache.budget=6*TRAINING+1;
    const before=world.snapshot(),selected=world.select(ROOT),run=cache.execute(selected);
    const installedIdentity=world.bigint(context,120);
    const training=observedLiteral(world,before,run,{pc:0x100c,retired:6*TRAINING+1,
      budget:6*TRAINING+1,registers:registers(before,TRAINING),flags:0x46,
      writes:TRAINING,stackReturn:0x1005,fpValue:integer80(3+2*TRAINING),fpPc:CALLEE,
      identity:installedIdentity});
    assert.equal(run.id,root.id,'training finishes in the original caller before the SDK poll selects its new unit');
    result.trainingSemanticComplete=true;
    const overlays=[...cache.units.values()].filter(unit=>unit.id!==root.id&&unit.id!==callee.id);
    result.automatic={trainingIterations:TRAINING,trainingRegions:run.regions,
      admissions:cache.admissions,units:cache.units.size,
      installedIdentity:String(installedIdentity),
      overlays:overlays.map(unit=>({pc:unit.pc,id:String(unit.id),slot:unit.slot,bytes:unit.bytes})),
      recordedSelections:result.selections.length,
      trainingCounters:training.counters};

    // The old engine completes exactly the same training and test programs
    // before its missing automatic executor is allowed to fail the mechanism.
    if(baseline){
      world.reset(9);cache.budget=54;
      const testBefore=world.snapshot(),test=cache.execute(world.select(ROOT));
      observedLiteral(world,testBefore,test,{pc:0x100b,retired:54,budget:54,
        registers:registers(testBefore,9),flags:0x46,writes:9,stackReturn:0x1005,
        fpValue:integer80(21),fpPc:CALLEE});
      result.semanticComplete=true;
      assert.equal(overlays.length,1,'the production execution path automatically installs one observed executor');
      return;
    }

    assert.equal(typeof world.raw.core_trace_ready,'function','the candidate exposes its normal automatic poll');
    assert.equal(typeof world.raw.core_trace_root,'function');
    assert.equal(typeof world.raw.core_trace_finish,'function');
    assert.equal(overlays.length,1,'the normal SDK poll installed exactly one bounded overlay without a fixture request');
    const overlay=overlays[0];
    assert.equal(cache.admissions,3);assert.equal(cache.units.size,3);
    assert.equal(installedIdentity,overlay.id,'automatic compilation selected its actual installed native identity');
    assert.equal(cache.table.get(overlay.slot),overlay.link,'the SDK installed the real executable reciprocal slot');
    assert.equal(cache.publishers.get(overlay.slot),overlay.publish);
    assert([ROOT,0x1005,CALLEE].includes(overlay.pc),'the automatic root is an actually executed entry of this literal graph');
    assert.equal(cache.transientAdmissions,0);assert.equal(cache.transientRuns,0);
    result.automatic.capture=capture(world);
    assert.deepEqual(result.automatic.capture.owners.slice().sort(),
      [String(root.id),String(callee.id)].sort(),
      'the production recording contains both real source owners, rather than a one-owner replacement');
    assert(result.automatic.capture.events.every(event=>event.kind===1),
      'this native-only program is recorded at actual block boundaries without helper witnesses');
    assert.equal(result.automatic.capture.events[0].pc,overlay.pc);
    assert.equal(result.automatic.capture.events[0].offset,0);

    // Capture the module already installed by the production SDK. This neither
    // prepares it nor calls private trace control. Instrumentation then observes
    // the actual owner body selected by the ordinary native dispatcher.
    const source=world.encoded(),module={pc:overlay.pc,id:String(overlay.id),slot:overlay.slot,
      bytes:source.length,sha256:hash(source),source};
    assert.equal(source.length,overlay.bytes);
    result.modules.push(module);world.units.set(overlay.id,{unit:overlay,module});
    world.install();
    const executor=world.diagnostics.find(exports=>exports.run===overlay.run);
    assert(executor,'the actual automatic owner has its own executable instrumentation');

    world.reset(9);cache.budget=54;
    const first=world.select(ROOT),testBefore=world.snapshot(),selectionStart=result.selections.length;
    const overlayEntries=readWitness(executor).body_entries;
    const test=cache.execute(first);
    const completed=observedLiteral(world,testBefore,test,{pc:0x100b,retired:54,budget:54,
      registers:registers(testBefore,9),flags:0x46,writes:9,stackReturn:0x1005,
      fpValue:integer80(21),fpPc:CALLEE});
    result.semanticComplete=true;
    const adoptions=result.selections.slice(selectionStart).filter(selection=>
      selection.operation==='select'&&selection.args[1]===overlay.pc&&selection.returned===overlay.slot);
    result.automatic.consumption={initialIdentity:String(first.id),finalIdentity:String(test.id),
      regions:test.regions,bodyEntries:completed.counters.body_entries,adoptions};
    assert(first.id===overlay.id||adoptions.length>0,
      'ordinary native selection really adopted the installed overlay capability');
    assert.equal(test.id,overlay.id,'the observed executor, rather than a helper-only side path, owns the completed loop');
    assert.equal(readWitness(executor).body_entries-overlayEntries,1,
      'exactly one actual automatic overlay body executes the complete observed suffix');
    assert.equal(completed.counters.body_entries,test.regions);
    assert(test.regions>=1&&test.regions<=3,
      'one baseline prefix reaches a single observed CALL/RET body instead of nineteen baseline activations');
    assert.equal(completed.counters.canonical_calls,0);
    assert.equal(completed.counters.wide_snapshot_calls,0);
    assert.equal(completed.counters.normalizer_calls,0);
    assert(completed.routing.lookup_entries<=3);
    assert(completed.routing.frame_commits<=3);
    assert.equal(cache.admissions,3,'consuming the automatic overlay emits no replacement body');
    assert.equal(cache.units.size,3);

    const family={run:overlay.run,link:overlay.link,publish:overlay.publish,slot:overlay.slot};
    for(const pc of[ROOT,0x1005,0x1008,0x1009,CALLEE,0x1102]){
      assert.equal(world.select(pc),overlay,'every actually observed instruction port shares the one family instance');
      assert.equal(world.bigint(context,120),overlay.id,'secondary selection installs the family native dictionary');
      assert.equal(hash(world.encoded()),module.sha256,'secondary ports retain the same emitted body');
      assert.equal(overlay.slot,family.slot);assert.equal(overlay.run,family.run);
      assert.equal(overlay.link,family.link);assert.equal(overlay.publish,family.publish);
      assert.equal(cache.table.get(family.slot),family.link);
      assert.equal(cache.publishers.get(family.slot),family.publish);
    }
    const owners=world.raw.core_link_owners_ptr();
    for(const saved of original){
      // The family shadows validated ports without replacing either immutable
      // source owner or its reciprocal table capability.
      assert.equal(cache.units.get(saved.unit.id),saved.unit);
      assert.equal(saved.unit.run,saved.run);assert.equal(saved.unit.link,saved.link);
      assert.equal(saved.unit.publish,saved.publish);
      assert.equal(cache.table.get(saved.unit.slot),saved.link);
      assert.equal(cache.publishers.get(saved.unit.slot),saved.publish);
      assert.equal(world.bigint(owners,saved.unit.slot*8),saved.unit.id);
    }
    assert.equal(cache.admissions,3);assert.equal(cache.units.size,3);
  });
}
