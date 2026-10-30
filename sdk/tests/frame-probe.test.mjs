import assert from 'node:assert/strict';
import test from 'node:test';
import {PhaseMeasurements} from '../runtime/session/measurement.js';
import {FrameProbe} from '../runtime/session/frame-probe.js';

function session(measurements = null) {
  const events = [];
  return {events, measurements, frameProbe: null, thread: 3,
    engine: {hash: 'engine-hash', get: () => 0x1000},
    code: {sampleUsage() {}, stats: () => ({retired: 7})},
    emit(value) { events.push(value.detail); }};
}

function observed(detailed = false) {
  const events = [], counters = [], scopes = [];
  return {events, counters, scopes, observation: {
    observer: {event: (...args) => events.push(args), counter: (...args) => counters.push(args)},
    capture: {observer: () => ({enabled: detailed,
      scope(name, fields, operation) { scopes.push(name); return operation(); },
      async scopeAsync(name, fields, operation) { scopes.push(name); return await operation(); },
    })},
  }};
}

test('chained cold work remains included in its compiled-entry envelope and freezes with the probe', t => {
  let reads = 0;
  t.mock.method(performance, 'now', () => { reads++; return 0; });
  const owner = session(), probe = new FrameProbe('mixed-work', owner), selected = {pc: 0x1000, id: 1n};
  owner.frameProbe = probe;
  const before = reads;
  probe.recordExecution({kind: 'compiled', coreReason: 2, work: 7, retired: 5, regions: 2,
    chainedColdRuns: 2, chainedColdWork: 4, chainedColdRetired: 3}, 0x1000, selected, 3);
  probe.recordExecution({kind: 'compiled', coreReason: 1, work: 2, retired: 2, regions: 1,
    chainedColdRuns: 1, chainedColdWork: 0, chainedColdRetired: 0}, 0x1000, selected, 1);
  assert.equal(reads, before, 'mixed work adds no clock reads');
  const snap = probe.snapshot();
  assert.deepEqual(snap.execution_work.compiled, {count: 2, work: 9, retired: 7});
  assert.deepEqual(snap.execution_work.chained_cold, {runs: 3, work: 4, retired: 3});
  assert.equal(snap.execution_work.cold.count, 0);
  assert.match(snap.execution_work.semantics, /chained_cold included in compiled/);
  for (const row of [snap.execution_attribution.totals, snap.execution_attribution.top32[0]]) {
    assert.deepEqual([row.count, row.work, row.retired, row.regions], [2, 9, 7, 3]);
    assert.deepEqual([row.chainedColdRuns, row.chainedColdWork, row.chainedColdRetired], [3, 4, 3]);
    assert.equal(row.elapsedNs, 4_000_000);
  }
  snap.execution_work.chained_cold.work = 99;
  assert.equal(probe.snapshot().execution_work.chained_cold.work, 4);
  probe.finish('session-ended');
  const frozen = owner.events.at(-1).execution_attribution;
  assert.equal(frozen.totals.chainedColdWork, 4);
  probe.recordExecution({kind: 'compiled', coreReason: 2, work: 8, retired: 8, regions: 1,
    chainedColdRuns: 1, chainedColdWork: 7, chainedColdRetired: 7}, 0x1000, selected, 2);
  assert.deepEqual(probe.snapshot().execution_attribution, frozen);
  const next = new FrameProbe('new-mixed-work', owner);
  assert.deepEqual(next.snapshot().execution_work.chained_cold, {runs: 0, work: 0, retired: 0});
  next.finish('session-ended');
});

test('aggregate phases require no observer and close throwing scopes in finally', async t => {
  let now = 0;
  t.mock.method(performance, 'now', () => now);
  const phases = new PhaseMeasurements(null);
  assert.equal(phases.run('entry', () => { now = 2; return 5; }), 5);
  assert.throws(() => phases.run('execution', () => { now = 5; throw new Error('sync'); }), /sync/);
  await assert.rejects(phases.runAsync('providers', async () => { now = 9; throw new Error('async'); }), /async/);
  assert.deepEqual(phases.snapshot(), {
    entry: {elapsedNs: 2_000_000, maximumNs: 2_000_000, count: 1},
    execution: {elapsedNs: 3_000_000, maximumNs: 3_000_000, count: 1},
    providers: {elapsedNs: 4_000_000, maximumNs: 4_000_000, count: 1},
  });
  assert.doesNotThrow(() => phases.report());
});

test('exclusive phases subtract direct child spans once across three nested levels', t => {
  let now=0,reads=0;
  t.mock.method(performance,'now',()=>{reads++;return now;});
  const phases=new PhaseMeasurements();
  assert.equal(phases.runExclusive('outer',()=>{
    now=2;
    phases.runExclusive('cpu',()=>{
      now=5;
      phases.runExclusive('trap',()=>{now=7;});
      now=11;
    });
    now=13;
    return 42;
  }),42);
  const snapshot=phases.snapshot();
  assert.deepEqual(snapshot,{
    trap:{elapsedNs:2_000_000,selfElapsedNs:2_000_000,maximumNs:2_000_000,count:1},
    cpu:{elapsedNs:9_000_000,selfElapsedNs:7_000_000,maximumNs:9_000_000,count:1},
    outer:{elapsedNs:13_000_000,selfElapsedNs:4_000_000,maximumNs:13_000_000,count:1},
  });
  assert.equal(Object.values(snapshot).reduce((sum,row)=>sum+row.selfElapsedNs,0),13_000_000);
  assert.equal(reads,6,'each synchronous scope uses one clock pair; snapshots add no clock reads');
});

test('exclusive sibling and later root scopes retain counts and maxima without counting uncovered gaps', t => {
  let now=0;
  t.mock.method(performance,'now',()=>now);
  const phases=new PhaseMeasurements();
  phases.runExclusive('outer',()=>{
    now=2;phases.runExclusive('child',()=>{now=5;});
    now=6;phases.runExclusive('child',()=>{now=8;});
    now=10;
  });
  now=14;
  phases.runExclusive('outer',()=>{now=19;});
  const snapshot=phases.snapshot();
  assert.deepEqual(snapshot,{
    child:{elapsedNs:5_000_000,selfElapsedNs:5_000_000,maximumNs:3_000_000,count:2},
    outer:{elapsedNs:15_000_000,selfElapsedNs:10_000_000,maximumNs:10_000_000,count:2},
  });
  assert.equal(snapshot.child.selfElapsedNs+snapshot.outer.selfElapsedNs,15_000_000,
    'exclusive rows cover the two roots; the four ms between them remain outside the measured ledger');
});

test('exclusive scopes close in finally and a thrown child cannot retain a parent for later work', t => {
  let now=0;
  t.mock.method(performance,'now',()=>now);
  const phases=new PhaseMeasurements(),failure=new Error('child failed');
  assert.throws(()=>phases.runExclusive('outer',()=>{
    now=2;
    phases.runExclusive('throwing',()=>{now=5;throw failure;});
  }),error=>error===failure);
  now=8;
  phases.runExclusive('after',()=>{now=11;});
  const snapshot=phases.snapshot();
  assert.deepEqual(snapshot,{
    throwing:{elapsedNs:3_000_000,selfElapsedNs:3_000_000,maximumNs:3_000_000,count:1},
    outer:{elapsedNs:5_000_000,selfElapsedNs:2_000_000,maximumNs:5_000_000,count:1},
    after:{elapsedNs:3_000_000,selfElapsedNs:3_000_000,maximumNs:3_000_000,count:1},
  });
  snapshot.outer.selfElapsedNs=99;
  assert.equal(phases.snapshot().outer.selfElapsedNs,2_000_000,'snapshots cannot rewrite retained accounting');
});

function v86MeasurementFixture() {
  const calls=[],result={coreReason:2,reason:1,retired:1,work:1},failure=new Error('guest stop');
  const record=(name,self,args)=>{calls.push({name,self,args});};
  const cache={open:false,
    import(...args){record('import',this,args);return result;},
    select(...args){record('select',this,args);return result;},
    execute(...args){record('execute',this,args);if(args[0]===failure)throw failure;return result;},
    cpu:{
      main_loop(...args){record('cpu',this,args);return result;},
      full_clear_tlb(){},jit_clear_cache(){},
    },
    traps:{capture(...args){record('trap',this,args);return result;}},
    tables:{pages:[],mappingEpoch:0n,codeVersion:0n,refresh(){},publishDirty(){return 0;},
      descriptor(){return {guestPages:1048576};}},
    engine:{ok(...args){record('native',this,args);return result;}},
  };
  return {cache,calls,result,failure};
}

test('v86 aggregate hooks preserve receivers, arguments, return identity and thrown errors', async () => {
  const {instrumentV86}=await import('../runtime/v86/measurement.js');
  const f=v86MeasurementFixture(),argument={value:7};
  const installed=instrumentV86(f.cache,new PhaseMeasurements());
  try {
    for(const [object,name] of [[f.cache,'import'],[f.cache,'select'],[f.cache,'execute'],
      [f.cache.cpu,'main_loop'],[f.cache.traps,'capture']]) {
      assert.equal(object[name](argument,42),f.result);
      const last=f.calls.at(-1);assert.equal(last.self,object);assert.deepEqual(last.args,[argument,42]);
    }
    assert.equal(f.cache.engine.ok('component_cpu_view',argument),f.result);
    assert.equal(f.calls.at(-1).self,f.cache.engine);
    assert.deepEqual(f.calls.at(-1).args,['component_cpu_view',argument]);
    assert.throws(()=>f.cache.execute(f.failure),error=>error===f.failure);
  } finally {installed.restore();}
});

test('v86 aggregate restore reinstates own descriptors and removes wrappers over inherited methods', async () => {
  const {instrumentV86}=await import('../runtime/v86/measurement.js');
  const f=v86MeasurementFixture(),original=f.cache.execute;
  Object.defineProperty(f.cache,'execute',{value:original,writable:false,enumerable:false,configurable:true});
  const prototype={main_loop:f.cache.cpu.main_loop};
  delete f.cache.cpu.main_loop;Object.setPrototypeOf(f.cache.cpu,prototype);
  const own=Object.getOwnPropertyDescriptor(f.cache,'execute');
  const installed=instrumentV86(f.cache,new PhaseMeasurements());
  assert.notEqual(f.cache.execute,original);assert(Object.hasOwn(f.cache.cpu,'main_loop'));
  installed.restore();installed.restore();
  assert.deepEqual(Object.getOwnPropertyDescriptor(f.cache,'execute'),own);
  assert.equal(Object.getOwnPropertyDescriptor(f.cache.cpu,'main_loop'),undefined);
  assert.equal(f.cache.cpu.main_loop,prototype.main_loop);
});

test('restored v86 hooks add no clock reads or counter writes and return detached snapshots', async t => {
  const {instrumentV86}=await import('../runtime/v86/measurement.js');
  let reads=0;
  t.mock.method(performance,'now',()=>++reads);
  const f=v86MeasurementFixture(),installed=instrumentV86(f.cache,new PhaseMeasurements());
  f.cache.cpu.main_loop();f.cache.traps.capture();
  assert.equal(installed.snapshot().enabled,true);
  installed.restore();
  const stopped=installed.snapshot(),before=reads;
  assert.equal(stopped.enabled,false);
  f.cache.import();f.cache.select();f.cache.execute();f.cache.cpu.main_loop();
  f.cache.cpu.full_clear_tlb();f.cache.cpu.jit_clear_cache();f.cache.traps.capture();
  f.cache.tables.refresh();f.cache.tables.publishDirty();f.cache.tables.descriptor();
  f.cache.engine.ok('component_cpu_view');
  assert.equal(reads,before,'off methods perform no instrumentation clock reads');
  assert.deepEqual(installed.snapshot(),stopped,'off methods leave retained counters unchanged');
  stopped.counters.cpuRuns=99;
  assert.notEqual(installed.snapshot().counters.cpuRuns,99);
});

test('v86 aggregate installation rejects active or duplicate owners and rolls back failed property replacement', async () => {
  const {instrumentV86}=await import('../runtime/v86/measurement.js');
  const active=v86MeasurementFixture();active.cache.open=true;
  const original=Object.getOwnPropertyDescriptors(active.cache);
  assert.throws(()=>instrumentV86(active.cache,new PhaseMeasurements()));
  assert.deepEqual(Object.getOwnPropertyDescriptors(active.cache),original);
  active.cache.open=false;
  const installed=instrumentV86(active.cache,new PhaseMeasurements());
  assert.throws(()=>instrumentV86(active.cache,new PhaseMeasurements()));
  installed.restore();
  const again=instrumentV86(active.cache,new PhaseMeasurements());again.restore();
  const broken=v86MeasurementFixture();
  Object.defineProperty(broken.cache.tables,'descriptor',{
    value:broken.cache.tables.descriptor,writable:false,enumerable:false,configurable:false,
  });
  const objects=[broken.cache,broken.cache.cpu,broken.cache.traps,broken.cache.tables,broken.cache.engine];
  const descriptors=objects.map(object=>Object.getOwnPropertyDescriptors(object));
  assert.throws(()=>instrumentV86(broken.cache,new PhaseMeasurements()));
  objects.forEach((object,index)=>assert.deepEqual(Object.getOwnPropertyDescriptors(object),descriptors[index]));
});

function sessionMeasurementFixture(methods={}) {
  return {paused:true,pauseStarted:0,pausedDuration:0,started:0,code:{open:false},
    complete(){return true;},async providers(){return false;},async wait(){},
    engine:{status(){return 0;}},...methods};
}

test('session profiling measures fused native work without pretending it has a per-call history',async t=>{
  const {instrumentSession}=await import('../runtime/session/diagnostics/phases.js');
  let now=0;t.mock.method(performance,'now',()=>now);
  const result={completed:true},owner=sessionMeasurementFixture({engine:{status(operation,...args){
    assert.equal(this,owner.engine);assert.equal(operation,'component_cpu_try_complete_gate');
    assert.deepEqual(args,[1,2]);now+=4;return result;
  }}});
  const phases=new PhaseMeasurements(),installed=instrumentSession(owner,phases);
  owner.paused=false;
  assert.equal(owner.engine.status('component_cpu_try_complete_gate',1,2),result);
  const snapshot=installed.snapshot();
  assert.equal(snapshot.sync['windows.fused'].selfElapsedNs,4_000_000);
  assert.equal(snapshot.syncRootNs,4_000_000);assert.deepEqual(snapshot.calls,[]);
  assert.match(snapshot.callCoverage,/fused native time is in windows.fused/);
  owner.paused=true;owner.pauseStarted=now;installed.restore();
  assert.equal(owner.engine.status('component_cpu_try_complete_gate',1,2),result);
  assert.equal(installed.snapshot().syncRootNs,4_000_000,'restored hooks no longer measure');
});

test('session await remainder subtracts nested synchronous roots once and preserves the awaited result', async t => {
  const {instrumentSession}=await import('../runtime/session/diagnostics/phases.js');
  let now=0,resolve;
  t.mock.method(performance,'now',()=>now);
  const pending=new Promise(done=>{resolve=done;}),result={ready:true};
  const owner=sessionMeasurementFixture({providers(){now=2;return pending;}});
  const phases=new PhaseMeasurements(),installed=instrumentSession(owner,phases);
  owner.paused=false;
  const operation=owner.providers();
  assert.equal(installed.snapshot().activeAwaitScopes,1);
  now=4;
  phases.runExclusive('callback',()=>{
    now=5;phases.runExclusive('nested',()=>{now=7;});now=10;
  });
  now=15;resolve(result);
  assert.equal(await operation,result);
  const snapshot=installed.snapshot();
  assert.deepEqual(snapshot.awaits['providers.await'],{selfElapsedNs:7_000_000,maximumNs:7_000_000,count:1});
  assert.equal(snapshot.syncRootNs,8_000_000);
  assert.equal(Object.values(snapshot.sync).reduce((sum,row)=>sum+row.selfElapsedNs,0),8_000_000);
  assert.equal(snapshot.activeSyncScopes,0);assert.equal(snapshot.activeAwaitScopes,0);
  assert.equal(snapshot.syncRootNs+snapshot.awaits['providers.await'].selfElapsedNs,15_000_000);
  owner.paused=true;owner.pauseStarted=now;installed.restore();
});

test('session active await time excludes a pause and paused native calls remain unmeasured', async t => {
  const {activeClock,instrumentSession}=await import('../runtime/session/diagnostics/phases.js');
  let now=0,resolve;
  t.mock.method(performance,'now',()=>now);
  const pending=new Promise(done=>{resolve=done;});
  const owner=sessionMeasurementFixture({wait(){return pending;}});
  const phases=new PhaseMeasurements(),installed=instrumentSession(owner,phases);
  owner.paused=false;
  const operation=owner.wait(4);
  now=5;owner.paused=true;owner.pauseStarted=now;
  now=105;
  assert.equal(activeClock(owner),5);
  owner.engine.status('paused-query');
  assert.equal(installed.snapshot().sync['host.native'],undefined);
  owner.pausedDuration=100;owner.paused=false;
  now=112;resolve();await operation;
  const snapshot=installed.snapshot();
  assert.equal(activeClock(owner),12);
  assert.deepEqual(snapshot.awaits['scheduler.await'],{selfElapsedNs:12_000_000,maximumNs:12_000_000,count:1});
  assert.equal(snapshot.syncRootNs,0);assert.equal(snapshot.activeAwaitScopes,0);
  owner.paused=true;owner.pauseStarted=now;installed.restore();
});

test('session instrumentation rejects overlapping waits and active restore, then unwinds rejected awaits', async t => {
  const {instrumentSession}=await import('../runtime/session/diagnostics/phases.js');
  let now=0,reject;
  t.mock.method(performance,'now',()=>now);
  const pending=new Promise((_resolve,fail)=>{reject=fail;}),failure=new Error('provider failed');
  const owner=sessionMeasurementFixture({providers(){now=2;return pending;}}),original=owner.providers;
  const phases=new PhaseMeasurements(),installed=instrumentSession(owner,phases);
  owner.paused=false;
  const operation=owner.providers();
  await assert.rejects(owner.wait(1),/overlapping/);
  now=4;owner.paused=true;owner.pauseStarted=now;
  assert.throws(()=>installed.restore(),/active/);
  assert.notEqual(owner.providers,original);
  now=104;reject(failure);
  await assert.rejects(operation,error=>error===failure);
  const snapshot=installed.snapshot();
  assert.equal(snapshot.activeAwaitScopes,0);assert.equal(snapshot.activeSyncScopes,0);
  assert.deepEqual(snapshot.awaits['providers.await'],{selfElapsedNs:2_000_000,maximumNs:2_000_000,count:1});
  assert.equal(snapshot.awaits['scheduler.await'],undefined);
  phases.runExclusive('manual',()=>assert.throws(()=>installed.restore(),/active/));
  installed.restore();installed.restore();
  assert.equal(owner.providers,original);
  const stopped=installed.snapshot();owner.engine.status('restored-query');
  assert.deepEqual(installed.snapshot(),stopped);
});

test('observed phases retain their schema, detailed scopes and counter exports', async t => {
  let now = 0;
  t.mock.method(performance, 'now', () => now);
  const fixture = observed(true), phases = new PhaseMeasurements(fixture.observation);
  phases.run('entry', () => { now = 2; });
  await phases.runAsync('wait', async () => { now = 5; });
  phases.report();
  assert.deepEqual(fixture.scopes, ['entry', 'wait']);
  assert.equal(fixture.events.length, 1);
  assert.equal(fixture.events[0][0], 'measurement.schema');
  assert.deepEqual(fixture.counters, [
    ['entry.ns', 2_000_000], ['entry.count', 1], ['entry.max.ns', 2_000_000],
    ['wait.ns', 3_000_000], ['wait.count', 1], ['wait.max.ns', 3_000_000],
  ]);
});

test('execution breakdown reuses one clock pair and includes throwing runs in aggregate totals', t => {
  let now = 0, reads = 0;
  t.mock.method(performance, 'now', () => { reads++; return now; });
  const fixture = observed(true), phases = new PhaseMeasurements(fixture.observation);
  assert.equal(phases.run('execution', () => { now = 2; return 7; }, 'cold'), 7);
  assert.equal(reads, 2);
  const failure = new Error('compiled failure');
  assert.throws(() => phases.run('execution', () => { now = 7; throw failure; }, 'compiled'),
    error => error === failure);
  assert.equal(reads, 4);
  phases.run('execution', () => { now = 8; }, 'cold');
  assert.equal(reads, 6);
  const snapshot = phases.snapshot();
  assert.deepEqual(snapshot, {
    execution: {
      elapsedNs: 8_000_000, maximumNs: 5_000_000, count: 3,
      breakdownSemantics: 'included in execution; not additive; entry tier, compiled includes chained cold work',
      breakdown: {
        cold: {elapsedNs: 3_000_000, maximumNs: 2_000_000, count: 2},
        compiled: {elapsedNs: 5_000_000, maximumNs: 5_000_000, count: 1},
      },
    },
  });
  const {execution} = snapshot;
  assert.equal(execution.count, execution.breakdown.cold.count + execution.breakdown.compiled.count);
  assert.equal(execution.elapsedNs, execution.breakdown.cold.elapsedNs + execution.breakdown.compiled.elapsedNs);
  snapshot.execution.breakdown.cold.count = 100;
  assert.equal(phases.snapshot().execution.breakdown.cold.count, 2);
  phases.report();
  assert.equal(reads, 6, 'snapshots and aggregate reporting do not sample the clock again');
  assert.deepEqual(fixture.scopes, ['execution', 'execution', 'execution']);
  assert.equal(fixture.events.length, 1);
  assert.equal(fixture.events[0][0], 'measurement.schema');
  assert.deepEqual(fixture.counters, [
    ['execution.ns', 8_000_000], ['execution.count', 3], ['execution.max.ns', 5_000_000],
  ]);
});

test('unclassified execution and other phase names keep the original snapshot and report shape', async t => {
  let now = 0, reads = 0;
  t.mock.method(performance, 'now', () => { reads++; return now; });
  const fixture = observed(), phases = new PhaseMeasurements(fixture.observation);
  phases.run('entry', () => { now = 1; }, 'cold');
  phases.run('execution', () => { now = 3; }, 'other');
  phases.run('execution', () => { now = 5; });
  await phases.runAsync('execution', async () => { now = 7; });
  assert.equal(reads, 8);
  assert.deepEqual(phases.snapshot(), {
    entry: {elapsedNs: 1_000_000, maximumNs: 1_000_000, count: 1},
    execution: {elapsedNs: 6_000_000, maximumNs: 2_000_000, count: 3},
  });
  phases.report();
  assert.equal(reads, 8);
  assert.deepEqual(fixture.counters, [
    ['entry.ns', 1_000_000], ['entry.count', 1], ['entry.max.ns', 1_000_000],
    ['execution.ns', 6_000_000], ['execution.count', 3], ['execution.max.ns', 2_000_000],
  ]);
});

test('probe work counters separate zero-work NeedCode from gates and positive prefixes without per-run clocks or events', t => {
  let reads = 0;
  t.mock.method(performance, 'now', () => { reads++; return 0; });
  const owner = session(), probe = new FrameProbe('work-proof', owner);
  owner.frameProbe = probe;
  const beforeReads = reads, beforeEvents = owner.events.length;
  const results = [
    {kind: 'cold', id: null, pc: 0x1000, coreReason: 1, reason: null, detail: null, work: 0, retired: 0, regions: 0},
    {kind: 'cold', id: null, pc: 0x7000, coreReason: 7, reason: 8, detail: 0x10001, work: 0, retired: 0, regions: 0},
    {kind: 'cold', id: null, pc: 0x1000, coreReason: 2, reason: 1, detail: 0, work: 0, retired: 0, regions: 0},
    {kind: 'cold', id: null, pc: 0x1002, coreReason: 1, reason: null, detail: null, work: 2, retired: 2, regions: 0},
    {kind: 'cold', id: null, pc: 0x1004, coreReason: 3, reason: 5, detail: 0x5000, work: 4, retired: 4, regions: 0},
    {kind: 'compiled', id: 7n, pc: 0x100a, coreReason: 2, reason: 1, detail: 0, work: 5, retired: 3, regions: 1},
    {kind: 'compiled', id: 9n, pc: 0x7000, coreReason: 7, reason: 8, detail: 0x10001, work: 0, retired: 0, regions: 1},
  ];
  for (const result of results) probe.recordExecution(Object.freeze(result));
  assert.equal(reads, beforeReads);
  assert.equal(owner.events.length, beforeEvents);
  const expected = {
    semantics: 'successful code.execute results by entry tier; chained_cold included in compiled; counts are not additive or time',
    cold: {
      count: 5, work: 6, retired: 6, zeroWorkCount: 3, positiveWorkCount: 2, zeroWorkNeedCodeCount: 1,
      reasons: {needCode: 2, budget: 1, memoryFault: 1, unsupported: 0, debug: 0,
        codeInvalidated: 0, host: 1, infrastructure: 0, cpuFault: 0, systemEntry: 0},
    },
    compiled: {count: 2, work: 5, retired: 3},
    chained_cold: {runs: 0, work: 0, retired: 0},
  };
  const snapshot = probe.snapshot().execution_work;
  assert.deepEqual(snapshot, expected);
  assert.equal(snapshot.cold.count, snapshot.cold.zeroWorkCount + snapshot.cold.positiveWorkCount);
  assert.equal(snapshot.cold.count, Object.values(snapshot.cold.reasons).reduce((sum, count) => sum + count, 0));
  snapshot.cold.reasons.needCode = 100;
  snapshot.compiled.work = 100;
  assert.deepEqual(probe.snapshot().execution_work, expected);
  probe.present({id: 1, callArgs: [7], pc: 0x1000}, 0, 0, 0n, 1n, 1n);
  const event = owner.events.find(value => value.event === 'producer-present');
  assert.deepEqual(event.execution_work, expected);
  assert.deepEqual(JSON.parse(JSON.stringify(event)).execution_work, expected);
  probe.finish('session-ended');
});

test('probe execution work stops with its exact measurement owner and starts fresh on the next probe', () => {
  const owner = session(), first = new FrameProbe('first-work', owner);
  owner.frameProbe = first;
  const cold = {kind: 'cold', id: null, pc: 0x1000, coreReason: 1, reason: null,
    detail: null, work: 0, retired: 0, regions: 0};
  const compiled = {kind: 'compiled', id: 1n, pc: 0x1001, coreReason: 2, reason: 1,
    detail: 0, work: 1, retired: 1, regions: 1};
  first.recordExecution(cold);
  const before = first.snapshot().execution_work;
  owner.measurements = {snapshot: () => ({replacement: true})};
  first.recordExecution(compiled);
  assert.deepEqual(first.snapshot().execution_work, before);
  owner.measurements = first.measurements;
  first.recordExecution(compiled);
  const recorded = first.snapshot().execution_work;
  assert.equal(recorded.cold.count, 1);
  assert.equal(recorded.compiled.count, 1);
  first.finish('session-ended');
  first.recordExecution(cold);
  first.recordExecution(compiled);
  assert.deepEqual(first.snapshot().execution_work, recorded);
  const second = new FrameProbe('second-work', owner);
  owner.frameProbe = second;
  assert.equal(second.snapshot().execution_work.cold.count, 0);
  assert.equal(second.snapshot().execution_work.compiled.count, 0);
  assert.deepEqual(first.snapshot().execution_work, recorded);
  second.finish('session-ended');
});

test('an unobserved probe owns fresh aggregate phases until its ninth completed edge', () => {
  const owner = session(), probe = new FrameProbe('proof', owner);
  owner.frameProbe = probe;
  assert.ok(owner.measurements instanceof PhaseMeasurements);
  assert.equal(probe.measurements, owner.measurements);
  assert.equal(owner.events[0].measurement_enabled, true);
  assert.match(owner.events[0].phase_semantics, /paused-loop waits excluded; in-flight async spans may cross pause/);
  const phases = owner.measurements;
  const receipt = {id: 1, callArgs: [7], pc: 0x1000};
  for (let index = 1; index <= 9; index++) {
    phases.run('completion', () => {});
    const info = probe.batch({requestId: index, epoch: 1, sequence: index});
    probe.present(receipt, 0, 0, 0n, 1n, 1n);
    assert.equal(owner.measurements, phases);
    probe.acknowledged({info, sent: {}}, {}, {}, {}, [{}]);
    if (index < 9) assert.equal(owner.measurements, phases);
  }
  assert.equal(probe.closed, true);
  assert.equal(owner.measurements, null);
  assert.equal(owner.frameProbe, null);
  assert.equal(owner.events.at(-1).reason, 'complete');
  assert.equal(owner.events.filter(event => event.event === 'producer-present').at(-1).phases.completion.count, 9);
  assert.equal(probe.snapshot().phases.completion.count, 9);
  assert.doesNotThrow(() => phases.report());
  assert.equal(owner.events.filter(event => event.event === 'measurement.schema').length, 0);
});

test('a probe preserves observed phases and snapshots its original object', () => {
  const fixture = observed(), phases = new PhaseMeasurements(fixture.observation);
  phases.run('entry', () => {});
  const owner = session(phases), probe = new FrameProbe('proof', owner);
  owner.frameProbe = probe;
  assert.equal(probe.measurements, phases);
  probe.finish('session-ended');
  assert.equal(owner.measurements, phases);
  owner.measurements = {snapshot: () => ({replacement: true})};
  assert.equal(probe.snapshot().phases.entry.count, 1);
  assert.equal(probe.snapshot().phases.replacement, undefined);
});

test('finishing owned phases does not clear a replacement measurement object', () => {
  const owner = session(), probe = new FrameProbe('proof', owner);
  const replacement = {snapshot: () => ({replacement: true})};
  owner.measurements = replacement;
  probe.finish('session-ended');
  assert.equal(owner.measurements, replacement);
  assert.deepEqual(probe.snapshot().phases, {});
});

for (const reason of ['attempt-overflow', 'batch-overflow', 'producer-overflow', 'batch-identity-mismatch']) {
  test(`${reason} releases the probe-owned phases`, () => {
    const owner = session(), probe = new FrameProbe('proof', owner);
    owner.frameProbe = probe;
    assert.ok(owner.measurements instanceof PhaseMeasurements);
    if (reason === 'attempt-overflow') {
      probe.attempts = 4096;
      probe.present(null, 0, 0, 0n, 1n, 1n);
    } else if (reason === 'batch-overflow') {
      probe.batches = 2048;
      assert.equal(probe.batch(null), null);
    } else if (reason === 'producer-overflow') {
      probe.accepted = 64;
      probe.present({id: 1, callArgs: [7], pc: 0x1000}, 0, 0, 0n, 1n, 1n);
    } else {
      probe.acknowledged(null, {}, {}, {}, []);
    }
    assert.equal(probe.closed, true);
    assert.equal(owner.measurements, null);
    assert.equal(owner.frameProbe, null);
    assert.equal(owner.events.at(-1).reason, reason);
  });
}

test('a failing finished diagnostic still releases exact owned state once', () => {
  const owner = session(), probe = new FrameProbe('proof', owner);
  owner.frameProbe = probe;
  owner.emit = () => { throw new Error('diagnostic failed'); };
  assert.throws(() => probe.finish('session-ended'), /diagnostic failed/);
  assert.equal(owner.measurements, null);
  assert.equal(owner.frameProbe, null);
  assert.doesNotThrow(() => probe.finish('session-ended'));
});

test('a failed arm does not leave aggregate phases attached', () => {
  const owner = session();
  owner.emit = () => { throw new Error('arm failed'); };
  assert.throws(() => new FrameProbe('proof', owner), /arm failed/);
  assert.equal(owner.measurements, null);
});

test('an async scope finishing after probe release cannot contaminate a later probe', async t => {
  let now = 0, release;
  t.mock.method(performance, 'now', () => now);
  const owner = session(), first = new FrameProbe('first', owner);
  const phases = owner.measurements;
  const operation = phases.runAsync('wait', () => new Promise(resolve => { release = resolve; }));
  now = 2;
  first.finish('session-ended');
  const second = new FrameProbe('second', owner);
  second.measurements.run('entry', () => { now = 3; });
  now = 7;
  release();
  await operation;
  assert.deepEqual(first.snapshot().phases.wait, {elapsedNs: 7_000_000, maximumNs: 7_000_000, count: 1});
  assert.deepEqual(second.snapshot().phases, {entry: {elapsedNs: 1_000_000, maximumNs: 1_000_000, count: 1}});
  assert.equal(owner.measurements, second.measurements);
  second.finish('session-ended');
});

test('outer execution attribution reuses measured elapsed time and separates failed attempts', t => {
  let now = 0, reads = 0;
  t.mock.method(performance, 'now', () => { reads++; return now; });
  const owner = session(), probe = new FrameProbe('outer-clock', owner);
  owner.frameProbe = probe;
  const phases = probe.measurements, beforeReads = reads, beforeEvents = owner.events.length;
  assert.equal(phases.lastMilliseconds('execution'), null);
  const cold = {kind: 'cold', id: null, pc: 0x9999, coreReason: 2, work: 2, retired: 2, regions: 0};
  const first = phases.run('execution', () => { now = 2; return cold; }, 'cold');
  probe.recordExecution(first, 0x1000, null, phases.lastMilliseconds('execution'));
  const selected = {pc: 0x3000, id: 7n}, failure = new Error('execute failed');
  assert.throws(() => phases.run('execution', () => { now = 7; throw failure; }, 'compiled'),
    error => error === failure);
  probe.recordExecution(undefined, 0x1004, selected, phases.lastMilliseconds('execution'));
  const compiled = {kind: 'compiled', id: 99n, pc: 0x8888, coreReason: 2, work: 5, retired: 3, regions: 4};
  const last = phases.run('execution', () => { now = 8; return compiled; }, 'compiled');
  probe.recordExecution(last, 0x1004, selected, phases.lastMilliseconds('execution'));
  assert.equal(reads - beforeReads, 6, 'three attempts use only the existing three clock pairs');
  assert.equal(owner.events.length, beforeEvents, 'attribution emits no per-attempt events');
  const snapshot = probe.snapshot(), attribution = snapshot.execution_attribution;
  assert.deepEqual(attribution.totals, {count: 2, work: 7, retired: 5, regions: 4,
    elapsedNs: 3_000_000, maximumNs: 2_000_000});
  assert.deepEqual(attribution.failed, {count: 1, elapsedNs: 5_000_000, maximumNs: 5_000_000});
  assert.equal(snapshot.phases.execution.count, attribution.totals.count + attribution.failed.count);
  assert.equal(snapshot.phases.execution.elapsedNs, attribution.totals.elapsedNs + attribution.failed.elapsedNs);
  assert.equal(snapshot.execution_work.cold.count, 1);
  assert.equal(snapshot.execution_work.compiled.count, 1, 'a throwing attempt has no returned guest work');
  assert.equal(attribution.top32.length, 2);
  assert.equal(attribution.top32[0].pc, 0x1000, 'grouping uses the actual start, not the returned PC');
  assert.equal(phases.lastMilliseconds('missing'), null);
  probe.finish('session-ended');
});

test('outer execution rows group tier and start PC while preserving selected-owner variation', t => {
  let reads = 0;
  t.mock.method(performance, 'now', () => { reads++; return 0; });
  const owner = session(), probe = new FrameProbe('outer-groups', owner);
  owner.frameProbe = probe;
  const old = {pc: 0x9000, id: 7n}, fresh = {pc: 0x8000, id: 8n};
  const beforeReads = reads, beforeEvents = owner.events.length;
  probe.recordExecution({kind: 'cold', id: null, pc: 0x2000, coreReason: 1, work: 0, retired: 0, regions: 0}, 0x1000, null, 1);
  probe.recordExecution({kind: 'compiled', id: 99n, pc: 0x3000, coreReason: 2, work: 5, retired: 3, regions: 4}, 0x1000, old, 2);
  probe.recordExecution({kind: 'compiled', id: 100n, pc: 0x4000, coreReason: 2, work: 7, retired: 7, regions: 2}, 0x1000, fresh, 3);
  probe.recordExecution({kind: 'compiled', id: 101n, pc: 0x5000, coreReason: 2, work: 1, retired: 1, regions: 1}, 0x1001, fresh, 5);
  old.pc = 0xdead; old.id = 55n; fresh.pc = 0xbeef; fresh.id = 66n;
  assert.equal(reads, beforeReads);
  assert.equal(owner.events.length, beforeEvents);
  const expected = [
    {kind: 'compiled', pc: 0x1000, firstSelectedRoot: 0x9000, firstSelectedId: '7', lastSelectedRoot: 0x8000,
      lastSelectedId: '8', count: 2, work: 12, retired: 10, regions: 6, elapsedNs: 5_000_000, maximumNs: 3_000_000},
    {kind: 'compiled', pc: 0x1001, firstSelectedRoot: 0x8000, firstSelectedId: '8', lastSelectedRoot: 0x8000,
      lastSelectedId: '8', count: 1, work: 1, retired: 1, regions: 1, elapsedNs: 5_000_000, maximumNs: 5_000_000},
    {kind: 'cold', pc: 0x1000, firstSelectedRoot: null, firstSelectedId: null, lastSelectedRoot: null,
      lastSelectedId: null, count: 1, work: 0, retired: 0, regions: 0, elapsedNs: 1_000_000, maximumNs: 1_000_000},
  ];
  const snapshot = probe.snapshot().execution_attribution;
  assert.equal(snapshot.capacity, 8192);
  assert.equal(snapshot.distinctEntries, 3);
  assert.deepEqual(snapshot.top32, expected);
  assert.deepEqual(snapshot.totals, {count: 4, work: 13, retired: 11, regions: 7,
    elapsedNs: 11_000_000, maximumNs: 5_000_000});
  assert.deepEqual(snapshot.overflow, {count: 0, work: 0, retired: 0, regions: 0, elapsedNs: 0, maximumNs: 0});
  snapshot.top32[0].count = 100;
  snapshot.totals.elapsedNs = 0;
  assert.deepEqual(probe.snapshot().execution_attribution.top32, expected, 'snapshots cannot rewrite retained rows');
  assert.equal(probe.snapshot().execution_attribution.totals.elapsedNs, 11_000_000);
  probe.finish('session-ended');
});

test('outer execution attribution bounds distinct PCs while retaining exact overflow and existing rows', () => {
  const owner = session(), probe = new FrameProbe('outer-cap', owner);
  owner.frameProbe = probe;
  const run = {kind: 'compiled', id: 5n, pc: 0xffffffff, coreReason: 2, work: 2, retired: 1, regions: 3};
  const selected = {pc: 0x9000, id: 3n};
  for (let index = 0; index < 8192; index++) probe.recordExecution(run, 0x1000 + index, selected, 1);
  probe.recordExecution(run, 0x80000000, selected, 7);
  probe.recordExecution(run, 0x1000, selected, 3);
  const snapshot = probe.snapshot().execution_attribution;
  assert.equal(snapshot.distinctEntries, 8192);
  assert.equal(snapshot.top32.length, 32);
  assert.deepEqual(snapshot.totals, {count: 8194, work: 16388, retired: 8194, regions: 24582,
    elapsedNs: 8_202_000_000, maximumNs: 7_000_000});
  assert.deepEqual(snapshot.overflow, {count: 1, work: 2, retired: 1, regions: 3,
    elapsedNs: 7_000_000, maximumNs: 7_000_000});
  assert.equal(snapshot.top32[0].pc, 0x1000);
  assert.equal(snapshot.top32[0].count, 2);
  assert.equal(snapshot.top32[0].elapsedNs, 4_000_000);
  assert.deepEqual(snapshot.top32.slice(1).map(row => row.pc), Array.from({length: 31}, (_, index) => 0x1001 + index));
  assert(!snapshot.top32.some(row => row.pc === 0x80000000), 'overflow does not displace retained identities');
  assert.equal(snapshot.totals.elapsedNs - snapshot.overflow.elapsedNs - snapshot.top32.reduce((sum, row) => sum + row.elapsedNs, 0),
    8_160_000_000, 'the omitted retained rows remain accounted outside top32');
  const again = probe.snapshot().execution_attribution;
  assert.deepEqual(again, snapshot, 'snapshot ordering does not mutate retention or overflow');
  probe.finish('session-ended');
});

test('outer execution attribution ends with its exact owner and begins empty for a new probe', () => {
  const owner = session(), first = new FrameProbe('outer-first', owner);
  owner.frameProbe = first;
  const run = {kind: 'cold', id: null, pc: 0x1001, coreReason: 2, work: 1, retired: 1, regions: 0};
  first.recordExecution(run, 0x1000, null, 2);
  const expected = first.snapshot().execution_attribution;
  owner.measurements = new PhaseMeasurements();
  first.recordExecution(run, 0x2000, null, 3);
  first.recordExecution(undefined, 0x2000, null, 4);
  assert.deepEqual(first.snapshot().execution_attribution, expected);
  owner.measurements = first.measurements;
  first.finish('session-ended');
  assert.deepEqual(owner.events.find(event => event.event === 'finished').execution_attribution, expected,
    'the final event retains the complete attributed window, including incomplete probes');
  first.recordExecution(run, 0x3000, null, 5);
  assert.deepEqual(first.snapshot().execution_attribution, expected);
  const next = new FrameProbe('outer-next', owner);
  owner.frameProbe = next;
  assert.equal(next.snapshot().execution_attribution.distinctEntries, 0);
  assert.equal(next.snapshot().execution_attribution.totals.count, 0);
  assert.equal(next.snapshot().execution_attribution.failed.count, 0);
  assert.deepEqual(first.snapshot().execution_attribution, expected);
  next.finish('session-ended');
});
