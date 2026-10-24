import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {view} from '../../../../../../sdk/runtime/engine.js';
import {observedFixture,observedLiteral} from './observed.mjs';

const ROOT=0x1000,CALLEE=0x1100;
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const FIVE='00000000000000a00140',NINE='00000000000000900240',TWENTY_ONE='00000000000000a80340';

function registers(before,{eax,ecx,esp}={}) {
  const result=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));
  if(eax!==undefined)result[0]=eax;
  if(ecx!==undefined)result[1]=ecx;
  if(esp!==undefined)result[4]=esp;
  return result;
}

function recording(world) {
  const pointer=world.raw.core_trace_status_ptr(),header=new DataView(world.memory.buffer,pointer,32);
  assert(pointer>0&&pointer%8===0);const used=header.getUint32(4,true);
  assert.equal(header.getUint32(8,true),129);assert.equal(header.getUint32(12,true),0);assert(used<=129);
  return{active:header.getUint32(0,true),used,events:Array.from({length:used},(_,i)=>{
    const row=new DataView(world.memory.buffer,pointer+32+24*i,24);
    return{kind:row.getUint32(0,true),pc:row.getUint32(4,true),end:row.getUint32(8,true),
      offset:row.getUint32(12,true),owner:String(row.getBigUint64(16,true))};
  })};
}

function retainSources(world,saved) {
  for(const {unit,module,run,link,publish} of saved){
    // Both public roots can now be shadowed by legitimate observed executors.
    // Their other real instruction ports still name the immutable source body.
    const pc=unit.id===world.root.id?0x1005:0x1102;
    assert.equal(world.select(pc).id,unit.id);
    assert.equal(hash(world.encoded()),module.sha256);
    assert.equal(world.cache.units.get(unit.id),unit);
    assert.equal(unit.run,run);assert.equal(unit.link,link);assert.equal(unit.publish,publish);
    assert.equal(world.cache.table.get(unit.slot),link);
    assert.equal(world.cache.publishers.get(unit.slot),publish);
  }
}

// Explicit core_trace_start keeps the legacy root-only executor contract. The
// normal cold/automatic family growth contract is tested in family-growth.mjs.
// A partial caller span is an exact-prefix boundary, so the original three-op
// run below must decline capture. A second run completes the caller span: its
// real final BLOCK confirms FADD/RET without inventing a partial-span event.
// The next capture starts in the baseline caller and must cross that already
// installed executor. No descriptor or recorder event is authored by this test.
function growthCase(artifact,report,name,afterLong=()=>{}) {
  observedFixture(artifact,report,name,{},world=>{
    const {cache,result,root,callee}=world;
    result.manualLegacy=true;
    const sources=[...world.units.values()].map(({unit,module})=>({unit,module,
      run:unit.run,link:unit.link,publish:unit.publish}));

    world.reset(3);let selected=world.select(ROOT);cache.budget=1;
    let before=world.snapshot(),run=cache.execute(selected);
    observedLiteral(world,before,run,{pc:CALLEE,retired:1,budget:1,
      registers:registers(before,{esp:0x47fc}),writes:1,stackReturn:0x1005});

    selected=world.select(CALLEE);world.status('core_trace_start',0,CALLEE);
    cache.budget=3;before=world.snapshot();run=cache.execute(selected);
    observedLiteral(world,before,run,{pc:0x1008,retired:3,budget:3,
      registers:registers(before,{eax:1,esp:0x4800}),fpValue:FIVE,fpPc:CALLEE});
    const exact=typeof world.raw.core_precise==='function';
    result.partialCapture=recording(world);result.shortSemanticComplete=true;
    assert(result.partialCapture.events.some(event=>event.owner===String(callee.id)&&event.pc===CALLEE));
    if(exact){
      assert(!result.partialCapture.events.some(event=>event.owner===String(root.id)&&event.pc===0x1005),
        'the partially executed caller span never fabricates a completed BLOCK witness');
      before=world.snapshot();world.status('core_trace_finish',18,CALLEE,128,32);sameImages(world,before);
      assert.equal(cache.admissions,2);assert.equal(cache.units.size,2);
      primeCall(world,3);selected=world.select(CALLEE);world.status('core_trace_start',0,CALLEE);
      cache.budget=5;before=world.snapshot();run=cache.execute(selected);
      observedLiteral(world,before,run,{pc:ROOT,retired:5,budget:5,
        registers:registers(before,{eax:1,ecx:2,esp:0x4800}),fpValue:FIVE,fpPc:CALLEE});
    }
    result.shortCapture=recording(world);
    assert(result.shortCapture.events.some(event=>event.owner===String(callee.id)&&event.pc===CALLEE));
    assert(result.shortCapture.events.some(event=>event.owner===String(root.id)&&event.pc===0x1005),
      'only actual admission of the complete caller span supplies the confirming witness');
    world.status('core_trace_finish',0,CALLEE,128,32);
    const short=world.select(CALLEE);world.install();
    assert.notEqual(short.id,callee.id);assert.notEqual(short.id,root.id);
    assert.equal(cache.admissions,3);assert.equal(cache.units.size,3);
    const shortModule=world.units.get(short.id).module;
    result.shortExecutor={id:String(short.id),pc:short.pc,slot:short.slot,sha256:shortModule.sha256};

    world.reset(3);selected=world.select(ROOT);world.status('core_trace_start',0,ROOT);
    cache.budget=19;before=world.snapshot();run=cache.execute(selected);
    observedLiteral(world,before,run,{pc:0x100c,retired:19,budget:19,
      registers:registers(before,{eax:3,ecx:0}),flags:0x46,writes:3,
      stackReturn:0x1005,fpValue:NINE,fpPc:CALLEE});
    result.growthCapture=recording(world);result.semanticComplete=true;
    // A pinned old engine ends capture at the installed trace and legitimately
    // declines18. Its whole guest oracle above must pass before this RED.
    const finished=world.raw.core_trace_finish(ROOT,128,32)>>>0;
    result.growthFormation={actual:finished,wanted:0};
    assert.equal(finished,0,'a baseline-root recording crosses the installed short executor and forms its longer observed graph');
    assert(result.growthCapture.events.some(event=>event.owner===String(short.id)),
      'the actual installed short executor contributes its own source-authorized block witness');
    assert(result.growthCapture.events.some(event=>event.owner===String(root.id)));

    const longer=world.select(ROOT);world.install();
    assert.notEqual(longer.id,short.id);assert.notEqual(longer.id,root.id);assert.notEqual(longer.id,callee.id);
    assert.equal(cache.admissions,4);assert.equal(cache.units.size,4);
    result.longExecutor={id:String(longer.id),pc:longer.pc,slot:longer.slot,
      sha256:world.units.get(longer.id).module.sha256};
    assert.equal(world.select(CALLEE).id,short.id,'growing a different root retains the installed short executor');
    assert.equal(hash(world.encoded()),shortModule.sha256);

    world.reset(9);selected=world.select(ROOT);cache.budget=54;
    const dormant=recording(world);assert.equal(dormant.active,0);
    before=world.snapshot();run=cache.execute(selected);
    const completed=observedLiteral(world,before,run,{pc:0x100b,retired:54,budget:54,
      registers:registers(before,{eax:9,ecx:0}),flags:0x46,writes:9,
      stackReturn:0x1005,fpValue:TWENTY_ONE,fpPc:CALLEE});
    result.longSemanticComplete=true;
    assert.equal(run.id,longer.id);assert.equal(run.regions,1);
    assert.equal(completed.counters.body_entries,1);
    assert.equal(completed.probes,0);
    assert.equal(completed.routing.lookup_entries,1);
    assert.equal(completed.routing.frame_commits,1);
    assert.equal(completed.counters.canonical_calls,0);
    assert.equal(completed.counters.wide_snapshot_calls,0);
    assert.equal(completed.counters.normalizer_calls,0);
    assert.equal(world.prepares-before.prepares,0);
    assert.deepEqual(recording(world),dormant,'an inactive recording remains byte-for-byte dormant while the observed loop executes');
    result.inactiveRecording=dormant;
    retainSources(world,sources);
    assert.equal(cache.admissions,4);assert.equal(cache.units.size,4);
    assert.equal(cache.transientAdmissions,0);assert.equal(cache.transientRuns,0);
    afterLong(world,{sources,short,shortModule,longer});
  });
}

export function runObservedGrowthCases(artifact,report) {
  growthCase(artifact,report,'observed-phased-capture-crosses-an-installed-callee-executor');
}

function sameImages(world,before) {
  const after=world.snapshot();
  for(const key of['context','state','fp','simd','ymm','ram','pages'])assert.deepEqual(after[key],before[key],
    `a refused or stale observed capability preserves the complete ${key} image`);
  assert.equal(after.revision,before.revision);assert.equal(after.helpers,before.helpers);
}

function primeCall(world,iterations) {
  world.reset(iterations);const selected=world.select(ROOT);world.cache.budget=1;
  const before=world.snapshot(),run=world.cache.execute(selected);
  observedLiteral(world,before,run,{pc:CALLEE,retired:1,budget:1,
    registers:registers(before,{esp:0x47fc}),writes:1,stackReturn:0x1005});
}

export function runObservedGrowthReplacementCases(artifact,report) {
  growthCase(artifact,report,'observed-strict-same-root-growth-recycles-old-slot-and-declines-equal-graph',
    (world,{sources,short,shortModule,longer})=>{
      const {cache,result}=world;
      primeCall(world,3);let selected=world.select(CALLEE);assert.equal(selected.id,short.id);
      world.status('core_trace_start',0,CALLEE);
      cache.budget=18;let before=world.snapshot(),run=cache.execute(selected);
      observedLiteral(world,before,run,{pc:0x100c,retired:18,budget:18,
        registers:registers(before,{eax:3,ecx:0,esp:0x4800}),flags:0x46,writes:2,
        stackReturn:0x1005,fpValue:NINE,fpPc:CALLEE});
      result.sameRootCapture=recording(world);result.sameRootSemanticComplete=true;
      assert(result.sameRootCapture.events.some(event=>event.owner===String(short.id)));
      // RET selects the baseline caller's continuation1005. Its known JNZ
      // backedge and CALL1000 then stay inside that source owner, rather than
      // reselecting the independently retained executor at public root1000.
      assert(result.sameRootCapture.events.some(event=>event.owner===String(world.root.id)));
      world.status('core_trace_finish',0,CALLEE,128,32);
      const upgraded=world.select(CALLEE);world.install();
      assert.notEqual(upgraded.id,short.id);
      assert.equal(upgraded.slot,short.slot,'the new identity actually reuses the retired short executor slot');
      assert.equal(cache.units.has(short.id),false);
      assert.equal(cache.units.get(longer.id),longer,'strict same-root growth retains the executor at the other root');
      assert.equal(cache.admissions,5);assert.equal(cache.units.size,4);
      result.replacement={oldIdentity:String(short.id),newIdentity:String(upgraded.id),slot:upgraded.slot,
        oldSha256:shortModule.sha256,newSha256:world.units.get(upgraded.id).module.sha256};
      const owners=world.raw.core_link_owners_ptr();
      assert.equal(world.bigint(owners,upgraded.slot*8),upgraded.id);
      before=world.snapshot();
      world.status('core_unlink',4,short.slot,Number(short.id&0xffffffffn),Number(short.id>>32n));
      sameImages(world,before);
      assert.equal(world.bigint(owners,upgraded.slot*8),upgraded.id,'stale unlink does not revoke the replacement slot owner');
      assert.equal(cache.table.get(upgraded.slot),upgraded.link);
      assert.equal(cache.publishers.get(upgraded.slot),upgraded.publish);

      // The old actual function is also rejected inside a valid lease for the
      // replacement. Instrumentation can count this attempted call, but none of
      // the guest/context/backing bytes or canonical callbacks may change.
      world.status('core_begin',0);
      try{
        before=world.snapshot();const stopped=short.run(world.context,1)>>>0;
        result.staleFunctionReason=stopped;assert.equal(stopped,8);sameImages(world,before);
      }finally{world.status('core_end',0);}

      primeCall(world,9);selected=world.select(CALLEE);assert.equal(selected.id,upgraded.id);
      cache.budget=53;before=world.snapshot();run=cache.execute(selected);
      const completed=observedLiteral(world,before,run,{pc:0x100b,retired:53,budget:53,
        registers:registers(before,{eax:9,ecx:0,esp:0x4800}),flags:0x46,writes:8,
        stackReturn:0x1005,fpValue:TWENTY_ONE,fpPc:CALLEE});
      result.upgradedSemanticComplete=true;
      assert.equal(run.id,upgraded.id);assert.equal(run.regions,1);
      assert.equal(completed.counters.body_entries,1);assert.equal(completed.probes,0);
      assert.equal(completed.routing.lookup_entries,1);assert.equal(completed.routing.frame_commits,1);
      assert.equal(completed.counters.wide_snapshot_calls,0);assert.equal(completed.counters.normalizer_calls,0);
      assert.equal(world.prepares-before.prepares,0);

      primeCall(world,3);selected=world.select(CALLEE);world.status('core_trace_start',0,CALLEE);
      cache.budget=18;before=world.snapshot();run=cache.execute(selected);
      observedLiteral(world,before,run,{pc:0x100c,retired:18,budget:18,
        registers:registers(before,{eax:3,ecx:0,esp:0x4800}),flags:0x46,writes:2,
        stackReturn:0x1005,fpValue:NINE,fpPc:CALLEE});
      result.equalCapture=recording(world);result.equalSemanticComplete=true;
      before=world.snapshot();const stats=cache.stats();
      world.status('core_trace_finish',18,CALLEE,128,32);sameImages(world,before);
      assert.deepEqual(cache.stats(),stats,'equal-score refusal admits no owner, byte, slot, or SDK instance');
      assert.equal(recording(world).active,0);
      assert.equal(world.select(CALLEE).id,upgraded.id);
      assert.equal(hash(world.encoded()),result.replacement.newSha256);
      retainSources(world,sources);
      assert.equal(cache.units.get(longer.id),longer);assert.equal(cache.units.get(upgraded.id),upgraded);
      assert.equal(cache.admissions,5);assert.equal(cache.units.size,4);
    });
}
