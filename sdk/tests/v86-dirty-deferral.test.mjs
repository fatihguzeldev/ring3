import assert from 'node:assert/strict';
import test from 'node:test';
import {V86CodeCache,prepareV86} from '../runtime/v86/index.js';
import {Session} from '../runtime/session.js';
import {cpuViews,importState} from '../runtime/v86/state.js';
import {Traps} from '../runtime/v86/traps.js';
import {PARK,SEGMENTS,X87} from '../runtime/v86/layout.js';

function fixture(traps, gate = 0x40232) {
  const memory = new WebAssembly.Memory({initial: 1}), data = new DataView(memory.buffer);
  const events = [], base = 0x4000, result = new DataView(new ArrayBuffer(80));
  let currentGate = gate, currentFlags = 1;
  const cache = Object.assign(Object.create(V86CodeCache.prototype), {
    memory, budget: 1000, imported: true, closed: false, open: false,
    deferDirty: true, dirtyPending: false, deferredDirtyPublications: 0,
    runs: 0, regions: 0, retired: 0, work: 0, gates: 0, faults: 0,
    dirtyPages: 0, cowFaults: 0, traps: {pending: null},
  });
  cache.engine = {
    memory, base,
    get(offset) {return data.getUint32(base + offset, true);},
    resultView() {return result;},
    ok(operation, ...args) {
      if (operation === 'component_cpu_gate') {
        result.setUint32(0, currentGate, true);result.setUint32(4, currentFlags, true);
      }
      else if (operation === 'component_cpu_write_fault') events.push('cow');
      else if (operation === 'component_cpu_publish') {
        events.push('publish');
        data.setUint32(base + 72, args[2] === 7 ? 8 : 0, true);
        data.setUint32(base + 80, args[5], true);
      } else throw new Error('unexpected operation ' + operation);
    },
  };
  cache.tables = {
    dirtyGateFlagsSupported: true,
    calls: 0, released: 0, fail: false,
    publishDirty() {
      events.push('dirty');this.calls++;
      if (this.fail) throw new Error('publication failed');
      return 1;
    },
    isCopyOnWrite: () => true,
    faultReason: () => 2,
    release() {events.push('release');this.released++;},
  };
  cache.cpu = {
    wm: {exports: {
      get_hypercall_page_ptr: () => 0x200,
      get_eflags: () => 2,
      fpu_load_status_word: () => 0,
      fpu_load_tag_word: () => 0xffff,
    }},
    main_loop() {
      events.push('cpu');data.setUint32(664, data.getUint32(664, true) + 3, true);
      cache.traps.pending = traps.shift() ?? null;
    },
  };
  // the fixture supplies the next canonical state without running another CPU batch.
  cache.import = function() {events.push('import');this.traps.pending = null;this.imported = true;};
  return {cache, events, setGate(value, flags = 0) {currentGate = value;currentFlags = flags;}};
}

const gateTrap = () => ({vector: 6, pc: 0x1000, flags: 2, esp: 0, cs: 0, ss: 0});

function budgetFixture() {
  const f=fixture([]),c=f.cache,d=new DataView(c.memory.buffer);
  Object.assign(c,{budgetStateReuse:false,budgetStateReuses:0,pendingBudgetContinuation:false,
    budgetDirtyDeferral:false,budgetDirtyDeferrals:0});
  delete c.import;
  c.tables.refresh=()=>f.events.push('refresh');
  c.traps.setDescriptor=()=>{};c.cpu.update_state_flags=()=>f.events.push('state-import');
  d.setUint16(c.engine.base+SEGMENTS+4,1,true);d.setUint16(c.engine.base+X87+20,0xffff,true);
  for(const [i,selector] of [0x23,0x1b,0x23,0x23,0x33,0].entries())d.setUint16(c.engine.base+SEGMENTS+16+i*2,selector,true);
  c.import();f.events.length=0;
  return f;
}

test('targeted mapping configuration publishes pending evidence before refreshing and switching the paused mode',()=>{
  const f=budgetFixture(),c=f.cache,owner={paused:true,open:false};
  c.tables.configureTargetedMappings=enabled=>{f.events.push('mapping-mode');return enabled;};
  c.dirtyPending=true;
  assert.equal(c.configureTargetedMappings(true,owner),true);
  assert.deepEqual(f.events,['dirty','refresh','mapping-mode']);assert.equal(c.dirtyPending,false);
  for(const invalid of [undefined,{paused:false,open:false},{paused:true,open:true}])
    assert.throws(()=>c.configureTargetedMappings(false,invalid),/paused/);
  c.open=true;assert.throws(()=>c.configureTargetedMappings(false,owner),/paused/);c.open=false;
  c.closed=true;assert.throws(()=>c.configureTargetedMappings(false,owner),/paused/);
});

test('unobserved inner Budget exits defer dirty scans only with explicit owner permission and flush on disable',()=>{
  const f=budgetFixture(),c=f.cache,owner={paused:true,open:false};
  c.inlineOwner={canContinue:()=>true,canBridge:()=>true};
  c.execute();assert.equal(c.tables.calls,1);assert.equal(c.dirtyPending,false);
  c.budgetDirtyDeferral=true;f.events.length=0;
  c.execute();c.select(true);c.execute();
  assert.equal(c.budgetDirtyDeferrals,2);assert.equal(c.dirtyPending,true);assert.equal(c.tables.calls,1);
  assert.equal(f.events.filter(event=>event==='publish').length,2,'native Budget publications remain per batch');
  assert.equal(f.events.filter(event=>event==='state-import').length,2,'normal canonical state import remains');
  assert.equal(c.configureBudgetDirtyDeferral(false,owner),false);assert.equal(c.tables.calls,2);assert.equal(c.dirtyPending,false);
  assert.equal(c.configureBudgetDirtyDeferral(true,owner),true);
  for(const invalid of [undefined,{paused:false,open:false},{paused:true,open:true}])
    assert.throws(()=>c.configureBudgetDirtyDeferral(true,invalid),/paused/);
  c.open=true;assert.throws(()=>c.configureBudgetDirtyDeferral(true,owner),/paused/);c.open=false;
  c.closed=true;assert.throws(()=>c.configureBudgetDirtyDeferral(true,owner),/paused/);
});

test('Budget dirty deferral keeps observer, deadline, non-Budget and native-error publication barriers',()=>{
  for(const barrier of ['missing-owner','deadline','observer','no-bridge','no-dirty-policy','yield','fault','gate']) {
    const f=budgetFixture(),c=f.cache;c.budgetDirtyDeferral=true;
    c.inlineOwner={canContinue:()=>barrier!=='deadline',canBridge:()=>barrier!=='observer'};
    if(barrier==='missing-owner')delete c.inlineOwner;
    if(barrier==='no-bridge')delete c.inlineOwner.canBridge;
    if(barrier==='no-dirty-policy')c.deferDirty=false;
    if(['yield','fault','gate'].includes(barrier)) {
      f.setGate(0x10001,0);
      c.cpu.main_loop=()=>{new DataView(c.memory.buffer).setUint32(664,3,true);
        c.traps.pending={...gateTrap(),cs:0x1b,ss:0x23,
          ...(barrier==='yield'?{budgetYield:true,privateInstructions:0}:barrier==='fault'?{vector:13,error:0}:{})};};
    }
    c.execute();assert.equal(c.tables.calls,1,barrier);assert.equal(c.dirtyPending,false,barrier);
    assert.equal(c.budgetDirtyDeferrals,0,barrier);
  }
  const f=budgetFixture(),c=f.cache;c.budgetDirtyDeferral=true;c.inlineOwner={canContinue:()=>true,canBridge:()=>true};
  const original=c.engine.ok;c.engine.ok=function(operation,...args){if(operation==='component_cpu_publish')throw Error('native Budget failure');return original.call(this,operation,...args);};
  assert.throws(()=>c.execute(),/native Budget failure/);assert.equal(c.tables.calls,1);assert.equal(c.dirtyPending,false);
  assert.equal(c.budgetDirtyDeferrals,0);assert.equal(c.imported,false);
});

test('pure budget continuation is opt-in and consumes one inner-turn permission without skipping publication',()=>{
  const f=budgetFixture(),c=f.cache;
  c.execute();c.select(true);
  assert.equal(c.budgetStateReuses,0);assert(f.events.includes('refresh'));
  c.budgetStateReuse=true;f.events.length=0;
  c.execute();assert.equal(c.imported,false);assert.equal(c.pendingBudgetContinuation,true);
  assert.deepEqual(f.events,['cpu','dirty','publish']);
  c.select(true);assert.equal(c.budgetStateReuses,1);assert.equal(c.pendingBudgetContinuation,false);
  c.select(true);assert.equal(c.budgetStateReuses,1);
  c.execute();c.select();assert.equal(c.budgetStateReuses,1);
  assert.deepEqual(f.events.slice(-2),['refresh','state-import']);
  c.execute();c.import();assert.equal(c.pendingBudgetContinuation,false);
  c.select(true);assert.equal(c.budgetStateReuses,1,'outer import revokes the previous permission');
});

test('budget continuation cannot survive gate exits, publication errors, active or closed owners',()=>{
  for(const boundary of ['gate','yield','dirty-error','native-error','cpu-error']) {
    const f=budgetFixture(),c=f.cache;c.budgetStateReuse=true;
    if(boundary==='gate')c.cpu.main_loop=()=>{new DataView(c.memory.buffer).setUint32(664,3,true);c.traps.pending={...gateTrap(),cs:0x1b,ss:0x23};};
    if(boundary==='yield')c.cpu.main_loop=()=>{new DataView(c.memory.buffer).setUint32(664,3,true);c.traps.pending={...gateTrap(),cs:0x1b,ss:0x23,budgetYield:true,privateInstructions:0};};
    if(boundary==='dirty-error')c.tables.fail=true;
    if(boundary==='native-error')c.engine.ok=()=>{throw Error('native publish failed');};
    if(boundary==='cpu-error')c.cpu.main_loop=()=>{throw Error('cpu failed');};
    if(boundary.endsWith('error'))assert.throws(()=>c.execute(),/failed/);else c.execute();
    assert.equal(c.pendingBudgetContinuation,false,boundary);assert.equal(c.imported,false,boundary);
  }
  for(const boundary of ['open','closed','inlineFailure']) {
    const f=budgetFixture(),c=f.cache;c.budgetStateReuse=true;c.execute();
    c[boundary]=boundary==='inlineFailure'?Error('poisoned'):true;
    assert.throws(()=>c.select(true),/active|closed|poisoned/);assert.equal(c.pendingBudgetContinuation,false);
  }
  const f=budgetFixture();f.cache.budgetStateReuse=true;f.cache.execute();f.cache.close();
  assert.equal(f.cache.pendingBudgetContinuation,false);assert.throws(()=>f.cache.select(true),/closed/);
});

test('relaxed FPU boot is boolean-only before fetch and strict-compatible when omitted',async()=>{
  const original=globalThis.fetch;let fetches=0;
  globalThis.fetch=async()=>{fetches++;return {ok:false,status:418};};
  try {
    for(const value of [null,0,1,'true',{}])await assert.rejects(prepareV86({relaxedFpu:value}),/must be boolean/);
    assert.equal(fetches,0,'invalid mode options cannot start CPU loading');
    for(const options of [{},{relaxedFpu:false},{relaxedFpu:true}])await assert.rejects(prepareV86(options),/HTTP418/);
    assert.equal(fetches,3,'omitted, strict and explicit experimental modes reach the same CPU loading path');
  } finally {globalThis.fetch=original;}
});

test('fast preset boot requires an explicit boolean and experimental relaxed mode before fetch',async()=>{
  const original=globalThis.fetch;let fetches=0;
  globalThis.fetch=async()=>{fetches++;return {ok:false,status:418};};
  try {
    for(const fastPreset of [null,0,1,'true',{}])await assert.rejects(prepareV86({fastPreset,relaxedFpu:true}),/preset.*boolean/);
    for(const options of [{fastPreset:true},{fastPreset:true,relaxedFpu:false}])await assert.rejects(prepareV86(options),/requires.*relaxed/);
    assert.equal(fetches,0);
    for(const options of [{},{fastPreset:false},{fastPreset:true,relaxedFpu:true}])await assert.rejects(prepareV86(options),/HTTP418/);
    assert.equal(fetches,3);
  } finally {globalThis.fetch=original;}
});

test('fast preset applies seven controls only after fresh supported startup and marks success last',()=>{
  const make=()=>{
    const events=[],code={executionPolicyInitialized:true,runs:0,imported:false,open:false,closed:false,
      relaxedFpu:true,portGates:true,inlineGates:true,directGates:true,directBindings:true,lockStagingReuse:true,
      wasmPortsSupported:true,directStateInputsSupported:true,budget:50000,
      cpu:{wm:{exports:{get_relaxed_fpu:()=>1}}},
      tables:{ownsPin:true,globalCode:true,descriptorReuse:true,targetedDirty:true,targetedDirtySupported:true}};
    for(const [name,field] of [['configureWasmPorts','wasmPorts'],['configureDirectStateInputs','directStateInputs'],
      ['configureBudgetDirtyDeferral','budgetDirtyDeferral'],['configureTargetedMappings','targetedMappings']])
      code[name]=(value,owner)=>{assert.deepEqual(owner,{paused:true,open:false});events.push(name);code[field]=value;};
    const graphicsTransport={deferredSealing:true,configureBorrowedBatches(value){events.push('borrowed');this.borrowedBatches=value;}};
    const session=Object.assign(Object.create(Session.prototype),{backend:'v86',running:false,stopped:false,fastPresetApplied:false,
      externalCpu:{fastPreset:true,relaxedFpu:true},code,media:{graphicsTransport},engine:{api:{component_cpu_try_complete_gate(){}}}});
    return {session,code,graphicsTransport,events};
  };
  const f=make();f.session.initializeFastPreset();
  assert.equal(f.session.fastPresetApplied,true);assert.equal(f.session.fastNativeCalls,true);
  for(const field of ['wasmPorts','directStateInputs','budgetDirtyDeferral','targetedMappings','budgetStateReuse'])assert.equal(f.code[field],true);
  assert.equal(f.graphicsTransport.borrowedBatches,true);assert.equal(f.code.budget,50000);
  assert.deepEqual(f.events,['configureDirectStateInputs','configureBudgetDirtyDeferral','configureTargetedMappings','configureWasmPorts','borrowed']);
  const plain=make();plain.session.externalCpu.fastPreset=false;plain.session.initializeFastPreset();assert.deepEqual(plain.events,[]);
  for(const change of [f=>f.code.wasmPortsSupported=false,f=>f.code.runs=1]) {
    const bad=make();change(bad);assert.throws(()=>bad.session.initializeFastPreset(),/preset/);
    assert.equal(bad.session.fastPresetApplied,false);assert.deepEqual(bad.events,[]);
  }
  const failed=make(),failure=Error('preset configuration failed');failed.code.configureTargetedMappings=()=>{throw failure;};
  assert.throws(()=>failed.session.initializeFastPreset(),error=>error===failure);
  assert.equal(failed.session.fastPresetApplied,false);assert.equal(failed.code.inlineFailure,failure);
});

test('SetRenderState defers dirty publication until the next non-allowlisted gate', () => {
  const f = fixture([gateTrap(), gateTrap()]);
  const first = f.cache.execute();
  assert.equal(first.coreReason, 7);
  assert.equal(f.cache.tables.calls, 0);
  assert.equal(f.cache.dirtyPending, true);
  assert.equal(f.cache.deferredDirtyPublications, 1);
  assert.deepEqual(f.events, ['cpu', 'publish']);
  f.setGate(0x10001, 0);
  f.cache.imported = true;
  f.cache.execute();
  assert.equal(f.cache.tables.calls, 1);
  assert.equal(f.cache.dirtyPending, false);
  assert.equal(f.cache.deferredDirtyPublications, 1);
  assert.deepEqual(f.events.slice(2), ['cpu', 'dirty', 'publish']);
});

test('copy-on-write publishes old dirty evidence before the native split and reimport', () => {
  const cow = {vector: 14, pc: 0x1000, flags: 2, esp: 0, cs: 0, ss: 0, address: 0x7000, error: 2};
  const f = fixture([cow, gateTrap()]);
  f.cache.execute();
  assert.deepEqual(f.events, ['cpu', 'dirty', 'cow', 'import', 'cpu', 'publish']);
  assert.equal(f.cache.cowFaults, 1);
  assert.equal(f.cache.tables.calls, 1);
  assert.equal(f.cache.dirtyPending, true, 'the later allowlisted batch has its own pending evidence');
  assert.equal(f.cache.deferredDirtyPublications, 1);
  f.cache.publish();
  assert.equal(f.cache.tables.calls, 2);
  assert.equal(f.cache.dirtyPending, false);
});

test('a known gate without the native safety flag cannot defer publication', () => {
  const f = fixture([gateTrap()]);
  f.setGate(0x40232, 0);
  f.cache.execute();
  assert.equal(f.cache.tables.calls, 1);
  assert.equal(f.cache.dirtyPending, false);
  assert.equal(f.cache.deferredDirtyPublications, 0);
  assert.deepEqual(f.events, ['cpu', 'dirty', 'publish']);
});

test('a legacy host cannot authorize deferral through stale transfer bytes', () => {
  const f = fixture([gateTrap()]);
  f.cache.tables.dirtyGateFlagsSupported = false;
  f.setGate(0x40232, 1);
  f.cache.execute();
  assert.equal(f.cache.tables.calls, 1);
  assert.equal(f.cache.dirtyPending, false);
  assert.equal(f.cache.deferredDirtyPublications, 0);
});

test('mode changes and close preserve pending evidence when publication fails', () => {
  const f = fixture([]);
  f.cache.dirtyPending = true;
  for (const owner of [undefined, {paused: false, open: false}, {paused: true, open: true}])
    assert.throws(() => f.cache.configureDirtyDeferral(false, owner), /paused/);
  assert.equal(f.cache.tables.calls, 0);
  f.cache.tables.fail = true;
  assert.throws(() => f.cache.configureDirtyDeferral(false, {paused: true, open: false}), /publication failed/);
  assert.equal(f.cache.deferDirty, true);
  assert.equal(f.cache.dirtyPending, true);
  assert.throws(() => f.cache.close(), /publication failed/);
  assert.equal(f.cache.closed, false);
  assert.equal(f.cache.tables.released, 0);
  f.cache.tables.fail = false;
  assert.equal(f.cache.configureDirtyDeferral(false, {paused: true, open: false}), false);
  assert.equal(f.cache.dirtyPending, false);
  f.cache.dirtyPending = true;
  f.cache.close();f.cache.close();
  assert.equal(f.cache.tables.released, 1);
  assert.equal(f.cache.closed, true);
  assert.equal(f.cache.dirtyPending, false);
  assert.deepEqual(f.events.slice(-2), ['dirty', 'release']);
});

function inlineFixture({id=0x10001,flags=3,status=0,allow=()=>true,pendingBefore=false,pendingAfter=false,atomic=false,recordId=id}={}) {
  const f=fixture([]),c=f.cache,h=c.engine,data=new DataView(c.memory.buffer),result=new DataView(new ArrayBuffer(112));
  const publications=[];let captured=false,completions=0,checks=0;
  Object.assign(c,{inlineGates:true,portGates:true,inlineCompletions:0,inlineYields:0,inlineFailure:null});
  h.api={component_cpu_try_complete_gate(){},pending_windows_thread_schedule:()=>pendingBefore||captured&&pendingAfter?1:0};
  if(atomic)h.api.component_cpu_try_inline_gate=()=>{};
  h.resultView=()=>result;
  h.ok=(operation,...args)=>{
    if(operation==='component_cpu_gate'){result.setUint32(0,id,true);result.setUint32(4,flags,true);}
    else if(operation==='pending_windows_callback')result.setUint32(12,0,true);
    else if(operation==='component_cpu_publish'){publications.push(args.slice(2));captured=true;}
    else throw new Error('unexpected inline operation '+operation);
  };
  h.status=operation=>{
    assert.equal(operation,atomic?'component_cpu_try_inline_gate':'component_cpu_try_complete_gate');completions++;
    if(atomic&&status===256)return status;
    if(atomic)for(const [offset,value] of [[0,0x46433352],[4,0x10001],[8,112],[16,37],[20,recordId],[32,0x1000]])result.setUint32(offset,value,true);
    data.setUint32(h.base+16,0x89abcdef,true);data.setUint32(h.base+32,0x6100,true);
    data.setUint32(h.base+48,0x2000,true);data.setUint32(h.base+72,3,true);return status;
  };
  c.traps.setDescriptor=()=>{};c.cpu.update_state_flags=()=>{};
  data.setUint16(h.base+SEGMENTS+4,1,true);
  for(const [index,selector] of [0x23,0x1b,0x23,0x23,0x33,0].entries())
    data.setUint16(h.base+SEGMENTS+16+index*2,selector,true);
  data.setUint16(h.base+X87+20,0xffff,true);
  importState(h,c.cpu,c.traps);
  c.inlineOwner={canContinue:()=>allow(++checks),observeClock(){f.events.push('clock');return 123n;}};
  return {...f,publications,completions:()=>completions,
    trap:()=>({...gateTrap(),cs:0x1b,ss:0x23,esp:0x6000,portGate:true,privateInstructions:0})};
}

test('inline completion publishes zero credits and imports the real returned state',()=>{
  const f=inlineFixture();
  assert.equal(f.cache.tryInlineGate(f.trap()),true);
  assert.deepEqual(f.publications,[[7,0,0,0x10001,0,0]]);
  assert.equal(f.completions(),1);assert.equal(f.cache.inlineCompletions,1);
  const data=new DataView(f.cache.memory.buffer);
  assert.equal(data.getUint32(64,true),0x89abcdef);assert.equal(data.getUint32(556,true),0x2000);
  assert.equal(f.cache.retired,0);assert.equal(f.cache.work,0);
});

test('atomic inline completion uses one native boundary without separate publication or lookups',()=>{
  const f=inlineFixture({atomic:true}),trap=f.trap();
  assert.equal(f.cache.tryInlineGate(trap),true);assert.equal(f.completions(),1);
  assert.deepEqual(f.publications,[]);assert.deepEqual(f.events,['clock']);
  assert.equal(trap.observedClock,123n);assert.equal(f.cache.inlineCompletions,1);
});

test('untouched atomic decline reuses exported canonical banks after a pure OUT parks',()=>{
  const f=inlineFixture({atomic:true,status:256,id:0x4023d}),c=f.cache,data=new DataView(c.memory.buffer);
  let exports=0;c.cpu.wm.exports.fpu_load_status_word=()=>{exports++;return 0;};
  c.cpu.main_loop=()=>{
    data.setUint32(64,0x55,true);const trap=f.trap();
    assert.equal(c.tryInlineGate(trap),false);c.traps.pending=trap;
    data.setUint32(556,PARK,true);data.setUint32(620,0xffffffff,true);data.setUint32(664,7,true);
  };
  const run=c.execute();
  assert.equal(exports,1,'parked E6e1 fallback must not export identical FP/GPR banks twice');
  assert.equal(run.work,7);assert.equal(run.retired,7);assert.equal(run.observedClock,123n);
  assert.equal(c.engine.get(16),0x55);assert.equal(c.inlineFailure,null);
  assert.deepEqual(f.publications,[[7,0,0,0x4023d,7,7]]);
});

test('atomic unexpected completion IDs and committed errors remain sticky failures',()=>{
  for(const options of [{status:12},{recordId:0x4023d}]) {
    const f=inlineFixture({atomic:true,...options});let failure;
    assert.throws(()=>f.cache.tryInlineGate(f.trap()),error=>{failure=error;return true;});
    for(const retry of ['import','select','execute'])
      assert.throws(()=>V86CodeCache.prototype[retry].call(f.cache),error=>error===failure);
    assert.equal(f.completions(),1);assert.deepEqual(f.publications,[]);
  }
});

function directFixture({status=0,recordId=0x10001,grow=false,allow=()=>true,masked=false,bindings=false,guestReturns=false,stateInputs=false,...options}={}) {
  const f=inlineFixture({atomic:true,allow,...options}),c=f.cache,h=c.engine;
  Object.assign(c,{directGates:true,directCompletions:0,directDeclines:0,directBindings:bindings,directBindingCompletions:0});
  Object.assign(c,{guestReturns,guestReturnCompletions:0,portGateMode:guestReturns?2:1,portGatePatchedCount:12});
  Object.assign(c,{directStateInputs:stateInputs,directStateInputCompletions:0});c.tables.directStateInputsSupported=true;
  h.api.component_cpu_try_direct_gate=()=>{};
  if(masked)h.api.component_cpu_try_direct_gate_mask=()=>{};
  const previous=h.status,calls=[];
  h.status=(operation,...args)=>{
    if(operation!==(masked?'component_cpu_try_direct_gate_mask':'component_cpu_try_direct_gate'))return previous(operation,...args);
    assert.equal(c.dirtyPending,true,'guest evidence must survive a native dispatch failure');
    calls.push(args.slice(masked?-4:-3));
    if(grow)c.memory.grow(1);
    if(status!==0)return status;
    const result=h.resultView(32);
    for(const [offset,value] of [[0,0x46493352],[4,0x10001],[8,32],[12,0],[16,recordId],[20,0x89abcdef],[24,0x2000],[28,0x6100]])
      result.setUint32(offset,value,true);
    return 0;
  };
  return {...f,directCalls:calls};
}

test('experimental relaxed FPU transitions publish the live owner and import canonical state without changing credits or CW',()=>{
  const f=inlineFixture(),c=f.cache,d=new DataView(c.memory.buffer),v=cpuViews(c.memory),b=c.engine.base;
  c.relaxedFpu=false;let mode=0,clears=0;
  c.cpu.wm.exports.get_relaxed_fpu=()=>mode;
  c.cpu.wm.exports.set_relaxed_fpu=value=>{f.events.push('mode'+value);mode=value;};
  c.cpu.jit_clear_cache=()=>{clears++;};
  d.setUint16(b+4,3,true);d.setUint32(b+52,2,true);d.setUint16(b+X87+16,0x7f,true);d.setUint16(1036,0x7f,true);
  v.regs[0]=0x12345678;v.regs[4]=0x6000;d.setUint32(556,0x401000,true);d.setUint32(664,123,true);c.dirtyPending=true;
  assert.equal(c.configureRelaxedFpu(true,{paused:true,open:false}),true);
  assert.equal(c.engine.get(16),0x12345678);assert.equal(d.getUint32(556,true),0x401000);
  assert.equal(d.getUint16(b+X87+16,true),0x7f);assert.equal(d.getUint16(1036,true),0x7f);
  assert(f.events.indexOf('dirty')<f.events.indexOf('mode1'));assert.equal(c.relaxedFpu,true);
  c.imported=false;v.regs[0]=0xdeadbeef;d.setUint32(b+16,0x87654321,true);
  const canonical=v.bytes.slice(b,b+8192);
  assert.equal(c.configureRelaxedFpu(false,{paused:true,open:false}),false);
  assert.equal(v.regs[0],0x87654321,'a non-imported CPU cannot overwrite newer canonical state');
  assert.deepEqual(v.bytes.slice(b,b+8192),canonical);assert.equal(d.getUint16(1036,true),0x7f);
  assert.equal(d.getUint32(664,true),123);assert.equal(c.work,0);assert.equal(c.retired,0);assert.equal(clears,2);
});

test('relaxed FPU rejects open owners and unsupported modes and poisons failed transitions',()=>{
  const f=wasmPortFixture(),c=f.cache;c.relaxedFpu=false;
  for(const owner of [undefined,{paused:false,open:false},{paused:true,open:true}])
    assert.throws(()=>c.configureRelaxedFpu(true,owner),/paused/);
  assert.throws(()=>c.configureRelaxedFpu(true,{paused:true,open:false}),/unsupported/);
  c.wasmPorts=true;assert.equal(c.canUseWasmPorts(),true);c.relaxedFpu=true;assert.equal(c.canUseWasmPorts(),true);c.relaxedFpu=false;
  c.cpu.wm.exports.get_relaxed_fpu=()=>0;c.cpu.wm.exports.set_relaxed_fpu=()=>{};c.cpu.jit_clear_cache=()=>{};
  let failure;assert.throws(()=>c.configureRelaxedFpu(true,{paused:true,open:false}),error=>{failure=error;return /readback/.test(error.message);});
  assert.equal(c.inlineFailure,failure);
  for(const method of ['import','select','execute'])assert.throws(()=>V86CodeCache.prototype[method].call(c),error=>error===failure);
});

test('direct gates change only live return registers across native growth without exporting banks or observing clocks',()=>{
  for(const grow of [false,true]) {
    const f=directFixture({grow}),c=f.cache,v=cpuViews(c.memory),b=c.engine.base;
    v.regs[0]=0x55;v.regs[4]=0x6000;v.data.setUint32(b+72,2,true);
    v.data.setUint32(120,0x246,true);v.bytes.fill(0x5a,832,960);v.bytes.fill(0x3c,1152,1280);
    const canonical=v.bytes.slice(b,b+8192),numeric=v.bytes.slice(816,1280);
    c.cpu.wm.exports.fpu_load_status_word=()=>{throw new Error('unexpected full-bank export');};
    c.cpu.wm.exports.fpu_load_tag_word=()=>{throw new Error('unexpected full-bank import');};
    c.cpu.update_state_flags=()=>{throw new Error('unexpected full-state import');};
    assert.equal(c.tryInlineGate(f.trap()),true);
    const current=cpuViews(c.memory);
    assert.deepEqual(f.directCalls,[[0x1000,0x6000,0x55]]);
    assert.deepEqual(current.bytes.slice(b,b+8192),canonical);assert.deepEqual(current.bytes.slice(816,1280),numeric);
    assert.equal(current.regs[0],0x89abcdef);assert.equal(current.regs[4],0x6100);
    assert.equal(current.data.getUint32(556,true),0x2000);assert.equal(current.data.getUint32(560,true),0x2000);
    assert.equal(current.data.getUint32(620,true),0xffffffff);assert.equal(current.data.getUint32(120,true),0x246);
    assert.equal(c.directCompletions,1);assert.equal(c.inlineCompletions,1);assert.equal(c.directDeclines,0);
    assert.deepEqual(f.events,[]);assert.deepEqual(f.publications,[]);assert.equal(f.completions(),0);
  }
});

test('direct decline uses one ordinary full-state fallback with no atomic retry or adapter clock',()=>{
  const f=directFixture({status:256,id:0x4023d}),c=f.cache,data=new DataView(c.memory.buffer);
  let exports=0;c.cpu.wm.exports.fpu_load_status_word=()=>{exports++;return 0;};
  c.cpu.main_loop=()=>{
    data.setUint32(64,0x55,true);data.setUint32(80,0x6000,true);const trap=f.trap();
    assert.equal(c.tryInlineGate(trap),false);c.traps.pending=trap;
    data.setUint32(556,PARK,true);data.setUint32(620,0xffffffff,true);data.setUint32(664,7,true);
  };
  const run=c.execute();
  assert.equal(exports,1);assert.equal(f.completions(),0);assert.equal(f.directCalls.length,1);
  assert.equal(c.directDeclines,1);assert.equal(c.directCompletions,0);assert.equal(run.observedClock,undefined);
  assert.equal(run.work,7);assert.equal(run.retired,7);assert.equal(c.engine.get(16),0x55);
  assert.deepEqual(f.events,[]);assert.deepEqual(f.publications,[[7,0,0,0x4023d,7,7]]);
});

test('direct deadline yields the live returned state while fatal or segment failures cannot replay',()=>{
  const yielded=directFixture({allow:check=>check<2});
  assert.equal(yielded.cache.tryInlineGate(yielded.trap()),'yield');
  assert.equal(yielded.cache.inlineYields,1);assert.equal(cpuViews(yielded.cache.memory).regs[0],0x89abcdef);
  for(const options of [{status:12},{recordId:0x4023d},{badSegment:true}]) {
    const f=directFixture(options);let failure;
    if(options.badSegment)cpuViews(f.cache.memory).selectors[4]=0x3b;
    assert.throws(()=>f.cache.tryInlineGate(f.trap()),error=>{failure=error;return true;});
    assert.equal(f.cache.dirtyPending,true);
    for(const retry of ['import','select','execute'])
      assert.throws(()=>V86CodeCache.prototype[retry].call(f.cache),error=>error===failure);
    assert.equal(f.directCalls.length,options.badSegment?0:1);assert.equal(f.completions(),0);
  }
});

test('direct configuration is opt-in and requires a supported native export and a closed paused owner',()=>{
  const f=directFixture(),c=f.cache;c.directGates=false;
  for(const owner of [undefined,{paused:false,open:false},{paused:true,open:true}])
    assert.throws(()=>c.configureDirectGates(true,owner),/paused/);
  const owner={paused:true,open:false};c.dirtyPending=true;
  assert.equal(c.configureDirectGates(true,owner),true);assert.equal(c.tables.calls,1);assert.equal(c.dirtyPending,false);
  assert.equal(c.configureDirectGates(false,owner),false);
  delete c.engine.api.component_cpu_try_direct_gate;
  assert.throws(()=>c.configureDirectGates(true,owner),/unsupported/);assert.equal(c.directGates,false);
});

test('masked direct binding requests admit only the selected three binding IDs and count them separately',()=>{
  for(const recordId of [0x4023d,0x40253,0x40255]) {
    const f=directFixture({masked:true,bindings:true,recordId});
    assert.equal(f.cache.tryInlineGate(f.trap()),true);
    assert.deepEqual(f.directCalls,[[0x1000,0,0,0x7ff]]);
    assert.equal(f.cache.directBindingCompletions,1);assert.equal(f.cache.directCompletions,1);
    assert.equal(f.cache.inlineCompletions,1);assert.deepEqual(f.events,[]);
  }
});

test('masked direct8 requests cannot accept binding results and unsupported binding hosts cannot fall back',()=>{
  const oldHost=directFixture({bindings:true});let failure;
  assert.throws(()=>oldHost.cache.tryInlineGate(oldHost.trap()),error=>{failure=error;return /unsupported/.test(error.message);});
  assert.equal(oldHost.directCalls.length,0);assert.equal(oldHost.cache.inlineFailure,failure);
  for(const recordId of [0x4023d,0x40253,0x40255,0x40246]) {
    const f=directFixture({masked:true,recordId});
    assert.throws(()=>f.cache.tryInlineGate(f.trap()),/invalid v86 direct completion record/);
    assert.deepEqual(f.directCalls,[[0x1000,0,0,0xff]]);
    assert.equal(f.cache.directBindingCompletions,0);assert.equal(f.cache.directCompletions,0);
  }
  const getter=directFixture({masked:true});assert.equal(getter.cache.tryInlineGate(getter.trap()),true);
  assert.equal(getter.directCalls[0][3],0xff);assert.equal(getter.cache.directBindingCompletions,0);
});

test('binding opt-in requires a masked host and publishes pending evidence without requiring active direct mode',()=>{
  const f=directFixture({masked:true}),c=f.cache;c.directGates=false;
  for(const owner of [undefined,{paused:false,open:false},{paused:true,open:true}])
    assert.throws(()=>c.configureDirectBindings(true,owner),/paused/);
  const owner={paused:true,open:false};c.dirtyPending=true;
  assert.equal(c.configureDirectBindings(true,owner),true);assert.equal(c.tables.calls,1);
  assert.equal(c.configureDirectBindings(false,owner),false);
  delete c.engine.api.component_cpu_try_direct_gate_mask;
  assert.throws(()=>c.configureDirectBindings(true,owner),/unsupported/);assert.equal(c.directBindings,false);
});

test('direct state input masks admit only the two requested IDs without publishing or copying CPU banks',()=>{
  for(const bindings of [false,true])for(const recordId of [0x40228,0x4022c]) {
    const f=directFixture({masked:true,stateInputs:true,bindings,recordId}),c=f.cache,v=cpuViews(c.memory);
    const numeric=v.bytes.slice(816,1280);c.cpu.wm.exports.fpu_load_status_word=()=>{throw new Error('unexpected bank export');};
    assert.equal(c.tryInlineGate(f.trap()),true);assert.equal(f.directCalls[0][3],bindings?0x1fff:0x18ff);
    assert.equal(c.directStateInputCompletions,1);assert.equal(c.directBindingCompletions,0);assert.equal(c.directCompletions,1);
    assert.deepEqual(cpuViews(c.memory).bytes.slice(816,1280),numeric);assert.deepEqual(f.events,[]);assert.deepEqual(f.publications,[]);
  }
  for(const recordId of [0x40228,0x4022c]) {
    const f=directFixture({masked:true,bindings:true,recordId});
    assert.throws(()=>f.cache.tryInlineGate(f.trap()),/invalid v86 direct completion record/);
    assert.equal(f.directCalls[0][3],0x7ff);assert.equal(f.cache.directStateInputCompletions,0);
  }
});

test('direct state input opt-in requires capability256 and masked native support before publishing',()=>{
  const f=directFixture({masked:true}),c=f.cache,owner={paused:true,open:false};c.directGates=false;
  for(const invalid of [undefined,{paused:false,open:false},{paused:true,open:true}])
    assert.throws(()=>c.configureDirectStateInputs(true,invalid),/paused/);
  c.dirtyPending=true;assert.equal(c.configureDirectStateInputs(true,owner),true);assert.equal(c.tables.calls,1);
  assert.equal(c.configureDirectStateInputs(false,owner),false);c.tables.directStateInputsSupported=false;
  assert.throws(()=>c.configureDirectStateInputs(true,owner),/unsupported/);assert.equal(c.directStateInputs,false);
  c.tables.directStateInputsSupported=true;delete c.engine.api.component_cpu_try_direct_gate_mask;
  assert.throws(()=>c.configureDirectStateInputs(true,owner),/unsupported/);assert.equal(c.directStateInputs,false);
});

test('a declined state input call publishes old guest writes before normal fallback without widening generic flags',()=>{
  const f=directFixture({masked:true,stateInputs:true,status:256,id:0x40228,flags:2}),c=f.cache,data=new DataView(c.memory.buffer);
  c.cpu.main_loop=()=>{const trap=f.trap();assert.equal(c.tryInlineGate(trap),false);c.traps.pending=trap;
    data.setUint32(556,PARK,true);data.setUint32(620,0xffffffff,true);data.setUint32(664,7,true);};
  const run=c.execute();assert.equal(run.coreReason,7);assert.deepEqual(f.events,['dirty']);
  assert.deepEqual(f.publications,[[7,0,0,0x40228,7,7]]);assert.equal(c.dirtyPending,false);
  assert.equal(c.directDeclines,1);assert.equal(c.directStateInputCompletions,0);assert.equal(f.completions(),0);
});

function wasmPortFixture() {
  const f=directFixture({masked:true,bindings:true}),c=f.cache,bridgeEvents=[];
  let counts={completed:0,declined:0,bindings:0,stateInputs:0,checkpoints:0,declinedPc:null};
  const bridge={arm(mask){bridgeEvents.push(['arm',mask]);return true;},
    harvest(){bridgeEvents.push('harvest');const result=counts;counts={completed:0,declined:0,bindings:0,stateInputs:0,checkpoints:0,declinedPc:null};return result;},
    disarm(){bridgeEvents.push('disarm');return false;},close(){bridgeEvents.push('close');}};
  Object.assign(c,{portBridge:bridge,portBridgeBound:true,wasmPorts:false,wasmPortsActive:false,
    wasmPortCompletions:0,wasmPortDeclines:0,wasmPortCheckpoints:0,portBridgeDeclinedPc:null,portGateCalls:0,relaxedFpu:false});
  c.cpu.wm.exports.get_relaxed_fpu=()=>c.relaxedFpu?1:0;
  return {...f,bridgeEvents,counts(value){Object.assign(counts,value);}};
}

test('Wasm ports admit relaxed banks only after a cold mode-owner check without adding hot getter calls',()=>{
  const f=wasmPortFixture(),c=f.cache,d=new DataView(c.memory.buffer);let reads=0,mode=1;
  c.relaxedFpu=true;c.cpu.wm.exports.get_relaxed_fpu=()=>{reads++;return mode;};
  assert.equal(c.configureWasmPorts(true,{paused:true,open:false}),true);
  c.cpu.main_loop=()=>{c.traps.pending=f.trap();d.setUint32(664,d.getUint32(664,true)+7,true);};
  for(let n=0;n<2;n++){assert.equal(c.canUseWasmPorts(),true);c.execute();}
  assert.equal(reads,1);assert.equal(c.wasmPortsActive,false);
  c.relaxedFpu=false;mode=0;c.execute();assert.equal(reads,2,'a changed mode is rechecked once before arming');
  const invalid=wasmPortFixture(),bad=invalid.cache;bad.relaxedFpu=true;bad.dirtyPending=true;
  bad.cpu.wm.exports.get_relaxed_fpu=()=>0;
  assert.throws(()=>bad.configureWasmPorts(true,{paused:true,open:false}),/FPU mode owner changed/);
  assert.equal(bad.wasmPorts,false);assert.equal(bad.tables.calls,0);assert(bad.inlineFailure);
  const initial=wasmPortFixture(),first=initial.cache;first.wasmPorts=true;first.relaxedFpu=true;
  first.cpu.wm.exports.get_relaxed_fpu=()=>2;
  assert.throws(()=>first.execute(),/FPU mode owner changed/);
  assert.equal(initial.events.includes('cpu'),false);assert.equal(initial.bridgeEvents.length,0);
});

test('a runtime without the optional Wasm port bridge executes and closes through its ordinary owner',()=>{
  const f=wasmPortFixture(),c=f.cache;c.portBridge=undefined;c.portBridgeBound=false;
  assert.equal(c.wasmPortsSupported,false);
  assert.throws(()=>c.configureWasmPorts(true,{paused:true,open:false}),/unsupported/);
  const d=new DataView(c.memory.buffer);
  c.cpu.main_loop=()=>{c.traps.pending=f.trap();d.setUint32(664,7,true);};
  assert.equal(c.execute().work,7);c.portGates=false;c.close();
  assert.equal(c.closed,true);assert.equal(c.tables.released,1);assert.deepEqual(f.bridgeEvents,[]);
});

test('Wasm port configuration is closed-owner opt-in and disabled policy leaves the original IO path untouched',()=>{
  const f=wasmPortFixture(),c=f.cache,owner={paused:true,open:false};
  for(const invalid of [undefined,{paused:false,open:false},{paused:true,open:true}])
    assert.throws(()=>c.configureWasmPorts(true,invalid),/paused/);
  let original=0;assert.equal(c.forwardPortIO(()=>++original,0xe1,0),1);assert.deepEqual(f.bridgeEvents,[]);
  c.dirtyPending=true;assert.equal(c.configureWasmPorts(true,owner),true);assert.equal(c.tables.calls,1);
  assert.equal(c.canUseWasmPorts(),true);c.inlineOwner.canBridge=()=>false;assert.equal(c.canUseWasmPorts(),false);
  c.inlineOwner.canBridge=()=>true;c.guestReturns=true;assert.equal(c.canUseWasmPorts(),false);
  c.guestReturns=false;assert.equal(c.configureWasmPorts(false,owner),false);c.portBridgeBound=false;
  assert.throws(()=>c.configureWasmPorts(true,owner),/unsupported/);assert.equal(c.wasmPorts,false);
});

test('Wasm port fallback harvests once before JS policy and suppresses only its same-PC native retry',()=>{
  const f=wasmPortFixture(),c=f.cache;c.wasmPorts=true;c.wasmPortsActive=true;
  f.counts({completed:7,bindings:2,declined:1,checkpoints:1,declinedPc:0x1000});
  const result=c.forwardPortIO(()=>{
    assert.equal(c.directCompletions,7);assert.equal(c.inlineCompletions,7);assert.equal(c.directDeclines,1);
    assert.equal(c.tryInlineGate(f.trap()),false);assert.equal(f.directCalls.length,0);return 19;
  },0xe1,0);
  assert.equal(result,19);assert.equal(c.portBridgeDeclinedPc,null);c.harvestPortBridge();
  assert.equal(c.wasmPortCompletions,7);assert.equal(c.wasmPortDeclines,1);assert.equal(c.wasmPortCheckpoints,1);
  assert.equal(c.directBindingCompletions,2);assert.equal(c.gates,7);assert.equal(c.portGateCalls,7);
});

test('Wasm port execution arms after marking dirty evidence and harvests final progress without double credits',()=>{
  const f=wasmPortFixture(),c=f.cache,d=new DataView(c.memory.buffer);c.wasmPorts=true;
  c.cpu.main_loop=()=>{assert.equal(c.dirtyPending,true);assert.equal(c.wasmPortsActive,true);
    f.counts({completed:3,bindings:1});c.traps.pending=f.trap();d.setUint32(664,11,true);};
  const run=c.execute();assert.equal(run.work,11);assert.equal(c.work,11);assert.equal(c.runs,1);
  assert.equal(c.wasmPortCompletions,3);assert.equal(c.directCompletions,3);assert.equal(c.gates,4);
  assert.equal(c.wasmPortsActive,false);assert.deepEqual(f.bridgeEvents,[['arm',0x7ff],'harvest','disarm']);
});

test('unexpected Wasm port traps poison retries and restore the fallback table before backing release',()=>{
  const f=wasmPortFixture(),c=f.cache,failure=new WebAssembly.RuntimeError('native gate trap');c.wasmPorts=true;
  c.cpu.main_loop=()=>{f.counts({completed:2});throw failure;};
  assert.throws(()=>c.execute(),error=>error===failure);assert.equal(c.inlineFailure,failure);
  assert.equal(c.dirtyPending,true);assert.equal(c.wasmPortCompletions,2);assert.equal(c.open,false);
  for(const operation of ['import','select','execute'])assert.throws(()=>V86CodeCache.prototype[operation].call(c),error=>error===failure);
  c.portGates=false;c.tables.release=()=>f.bridgeEvents.push('release');c.close();
  assert(f.bridgeEvents.indexOf('close')<f.bridgeEvents.indexOf('release'));assert.equal(c.closed,true);
});

test('owned guest RET continuations change EAX only for the exact nine neutral APIs',()=>{
  for(const recordId of [0x10001,0x40225,0x4022a,0x4022e,0x40232,0x4023f,0x4023d,0x40253,0x40255]) {
    const f=directFixture({masked:true,bindings:true,guestReturns:true,recordId}),v=cpuViews(f.cache.memory);
    v.regs[4]=0x6000;v.data.setUint32(556,0x1002,true);v.data.setUint32(560,0x1000,true);v.data.setUint32(620,0x1234,true);
    assert.equal(f.cache.tryInlineGate(f.trap()),true);assert.equal(v.regs[0],0x89abcdef);
    assert.equal(v.regs[4],0x6000);assert.equal(v.data.getUint32(556,true),0x1002);
    assert.equal(v.data.getUint32(560,true),0x1000);assert.equal(v.data.getUint32(620,true),0x1234);
    assert.equal(f.cache.guestReturnCompletions,1);
  }
});

test('TEB writers and completed deadline yields keep manual returns even in guest RET mode',()=>{
  for(const options of [{recordId:0x10002},{recordId:0xf000b},{recordId:0x10001,allow:check=>check<2}]) {
    const f=directFixture({guestReturns:true,...options}),v=cpuViews(f.cache.memory);
    assert.equal(f.cache.tryInlineGate(f.trap()),options.allow?'yield':true);
    assert.equal(v.regs[4],0x6100);assert.equal(v.data.getUint32(556,true),0x2000);
    assert.equal(f.cache.guestReturnCompletions,0);
  }
});

test('guest RET mode changes commit SDK state only after successful native ownership validation',()=>{
  const f=wasmPortFixture(),c=f.cache;c.guestReturns=false;c.tables.guestReturnsSupported=true;
  c.tables.refresh=()=>{};c.cpu.full_clear_tlb=()=>{};c.traps.configurePortGates=()=>{};
  c.engine.api.component_cpu_set_gate_mode=()=>{};
  c.engine.api.component_cpu_gate_mode=()=>{};
  const ordinary=c.engine.ok;let busy=false,nativeMode=1;
  c.engine.ok=(operation,...args)=>{
    if(operation==='component_cpu_gate_mode') {
      const result=c.engine.resultView(16);[nativeMode,12,nativeMode===2?9:0,busy?1:0].forEach((value,i)=>result.setUint32(i*4,value,true));return;
    }
    if(operation!=='component_cpu_set_gate_mode')return ordinary(operation,...args);
    if(busy)throw Object.assign(new Error('pending guest RET'),{status:12});
    nativeMode=args[2];
    const result=c.engine.resultView(8);result.setUint32(0,args[2],true);result.setUint32(4,args[2]?12:0,true);
  };
  const owner={paused:true,open:false};
  assert.throws(()=>c.configureGuestReturns(true,{paused:false,open:false}),/paused/);
  assert.equal(c.configureGuestReturns(true,owner),true);assert.equal(c.portGateMode,2);
  busy=true;
  assert.throws(()=>c.configureGuestReturns(false,owner),error=>error.status===12);
  assert.equal(c.guestReturns,true);assert.equal(c.portGateMode,2);assert.equal(c.portGatePatchedCount,12);
  assert.throws(()=>c.close(),error=>error.status===12);
  assert.equal(f.bridgeEvents.includes('close'),false);assert.equal(c.portBridgeBound,true);assert.equal(c.closed,false);
  busy=false;assert.equal(c.configureGuestReturns(false,owner),false);assert.equal(c.portGateMode,1);
  c.tables.guestReturnsSupported=false;assert.throws(()=>c.configureGuestReturns(true,owner),/unsupported/);
  c.close();assert.equal(f.bridgeEvents.filter(value=>value==='close').length,1);assert.equal(c.portBridgeBound,false);
});

function overlapFixture() {
  const f=fixture([]),c=f.cache,h=c.engine,result=h.resultView();
  c.graphicsOverlap=false;let enabled=false,completed=(1n<<63n)+7n,busy=false,corrupt=null;
  h.api={component_cpu_set_graphics_overlap(){},component_cpu_graphics_overlap(){}};
  const ordinary=h.ok;
  h.ok=(operation,...args)=>{
    if(operation!=='component_cpu_set_graphics_overlap' && operation!=='component_cpu_graphics_overlap')return ordinary(operation,...args);
    f.events.push(operation==='component_cpu_graphics_overlap'?'query':'set');
    if(operation==='component_cpu_set_graphics_overlap') {
      if(busy)throw Object.assign(new Error('overlap Busy'),{status:12});
      enabled=!!args[2];
    }
    result.setUint32(0,enabled?1:0,true);result.setUint32(4,0,true);result.setBigUint64(8,completed,true);corrupt?.(result);
  };
  return {...f,setBusy(value){busy=value;},setNative(value,count){enabled=value;completed=count;},corrupt(fn){corrupt=fn;}};
}

function startupFixture() {
  const f=fixture([]),c=f.cache,result=c.engine.resultView(),ordinary=c.engine.ok;
  Object.assign(c,{imported:false,portGates:false,portGateMode:0,inlineGates:false,directGates:false,
    directBindings:false,directStateInputs:false,guestReturns:false,graphicsOverlap:false,inlineFailure:null});
  c.inlineOwner={canContinue:()=>false,observeClock:()=>0n};
  c.engine.api={component_cpu_set_gate_mode(){},component_cpu_try_inline_gate(){},
    component_cpu_try_complete_gate(){},component_cpu_try_direct_gate(){},component_cpu_try_direct_gate_mask(){}};
  c.engine.ok=(operation,...args)=>{
    if(operation!=='component_cpu_set_gate_mode')return ordinary(operation,...args);
    f.events.push('mode:'+args[2]);result.setUint32(0,args[2],true);result.setUint32(4,12,true);
  };
  c.tables.ownsPin=true;c.tables.globalCodeSupported=true;c.tables.globalCode=false;
  c.tables.refresh=()=>f.events.push('refresh');
  c.tables.configureGlobalCode=function(enabled,owner){
    assert.deepEqual(owner,{paused:true,open:false});f.events.push('global:'+enabled);this.globalCode=enabled;
  };
  c.cpu.full_clear_tlb=()=>f.events.push('tlb');c.traps.configurePortGates=enabled=>f.events.push('io:'+enabled);
  return f;
}

test('the fresh v86 backend initializes the measured bridge policy before importing or running guest code',()=>{
  const f=startupFixture(),c=f.cache;c.deferDirty=false;
  Object.assign(c.tables,{targetedDirtySupported:true,nativeDirty:true,policy:{incrementalSync:true},targetedDirty:false,
    configureTargetedDirty(enabled,owner){assert.deepEqual(owner,{paused:true,open:false});this.targetedDirty=enabled;f.events.push('targeted:'+enabled);}});
  c.initializeExecutionPolicy();
  assert.equal(c.deferDirty,true);assert.equal(c.portGates,true);assert.equal(c.portGateMode,1);
  assert.equal(c.inlineGates,true);assert.equal(c.directGates,true);assert.equal(c.directBindings,true);
  assert.equal(c.directStateInputs,false);
  assert.equal(c.tables.globalCode,true);assert.equal(c.guestReturns,false);assert.equal(c.graphicsOverlap,false);
  assert.equal(c.tables.targetedDirty,true);
  assert.equal(c.imported,false);assert.equal(c.runs,0);assert.equal(c.open,false);
  assert.deepEqual(f.events.filter(event=>event.startsWith('mode:')),['mode:1']);
  assert(f.events.indexOf('global:true')>f.events.indexOf('mode:1'));
  assert(!f.events.includes('cpu'));assert(!f.events.includes('import'));
  const before=f.events.slice();assert.throws(()=>c.initializeExecutionPolicy(),/initial|startup|once/);
  assert.deepEqual(f.events,before,'the one-shot initializer cannot patch or flush twice');
});

test('startup keeps unsupported old hosts on compatible execution without pretending masked support',()=>{
  const f=startupFixture(),c=f.cache;c.engine.api={};c.tables.dirtyGateFlagsSupported=false;c.tables.globalCodeSupported=false;
  c.initializeExecutionPolicy();assert.deepEqual(f.events,[]);
  assert.equal(c.deferDirty,true);assert.equal(c.portGates,false);assert.equal(c.inlineGates,false);
  assert.equal(c.directGates,false);assert.equal(c.directBindings,false);assert.equal(c.tables.globalCode,false);
  const old=startupFixture();delete old.cache.engine.api.component_cpu_try_inline_gate;
  delete old.cache.engine.api.component_cpu_try_direct_gate_mask;old.cache.tables.globalCodeSupported=false;
  old.cache.initializeExecutionPolicy();assert.equal(old.cache.inlineGates,true);assert.equal(old.cache.directGates,true);
  assert.equal(old.cache.directBindings,false);assert.equal(old.cache.tables.globalCode,false);
});

test('startup policy admission rejects started or unowned caches before any configuration mutation',()=>{
  for(const change of [c=>c.runs=1,c=>c.imported=true,c=>c.open=true,c=>c.closed=true,
      c=>c.tables.ownsPin=false,c=>delete c.inlineOwner.observeClock]) {
    const f=startupFixture();change(f.cache);
    assert.throws(()=>f.cache.initializeExecutionPolicy(),/initial|startup/);assert.deepEqual(f.events,[]);
    assert.equal(f.cache.portGates,false);assert.equal(f.cache.directGates,false);
  }
});

test('a claimed startup feature failure remains terminal after native configuration has begun',()=>{
  const f=startupFixture(),c=f.cache,failure=new Error('native startup global configuration failed');
  c.tables.configureGlobalCode=()=>{throw failure;};
  assert.throws(()=>c.initializeExecutionPolicy(),error=>error===failure);
  assert.equal(c.portGates,true);assert.equal(c.inlineFailure,failure);
  const before=f.events.slice();
  for(const operation of ['initializeExecutionPolicy','import','select','execute'])
    assert.throws(()=>V86CodeCache.prototype[operation].call(c),error=>error===failure);
  assert.deepEqual(f.events,before,'terminal startup failure cannot retry native mutation or guest execution');
});

function stagingFixture() {
  const f=startupFixture(),c=f.cache,ordinary=c.engine.ok,result=new DataView(new ArrayBuffer(16)),ordinaryView=c.engine.resultView;
  let enabled=false,retainedBytes=0,busy=false,corrupt=null;
  c.lockStagingReuse=false;c.runtime={hash:'cpu',jitConfig:[],counters:()=>({})};
  c.engine.api.component_cpu_set_lock_staging_reuse=()=>{};c.engine.api.component_cpu_lock_staging_reuse=()=>{};
  c.engine.resultView=size=>size===16?result:ordinaryView(size);
  c.engine.ok=(operation,...args)=>{
    if(!['component_cpu_set_lock_staging_reuse','component_cpu_lock_staging_reuse'].includes(operation))return ordinary(operation,...args);
    f.events.push(operation==='component_cpu_set_lock_staging_reuse'?'staging:set':'staging:get');
    if(operation==='component_cpu_set_lock_staging_reuse') {
      if(busy)throw Object.assign(new Error('staging Busy'),{status:12});enabled=!!args[2];if(!enabled)retainedBytes=0;
    }
    result.setUint32(0,Number(enabled),true);result.setUint32(4,retainedBytes,true);result.setBigUint64(8,(1n<<63n)+9n,true);corrupt?.(result);
  };
  return {...f,setBusy(value){busy=value;},corrupt(fn){corrupt=fn;}};
}

test('startup enables lock staging only when both native exports are present',()=>{
  const f=stagingFixture(),c=f.cache;c.initializeExecutionPolicy();
  assert.equal(c.lockStagingReuse,true);assert.equal(c.executionPolicyInitialized,true);assert.equal(c.runs,0);assert.equal(c.imported,false);
  assert(f.events.indexOf('staging:set')>f.events.indexOf('global:true'));
  const before=f.events.slice();assert.equal(c.stats().lockStagingReuse,true);assert.deepEqual(f.events,before,'stats does not query native staging state');
  for(const name of ['component_cpu_set_lock_staging_reuse','component_cpu_lock_staging_reuse']) {
    const old=stagingFixture();delete old.cache.engine.api[name];old.cache.initializeExecutionPolicy();
    assert.equal(old.cache.lockStagingReuse,false);assert(!old.events.includes('staging:set'));
  }
});

test('lock staging mode publishes and refreshes mappings while Busy and malformed results cannot commit',()=>{
  const owner={paused:true,open:false},f=stagingFixture(),c=f.cache;
  for(const invalid of [undefined,{paused:false,open:false},{paused:true,open:true}])assert.throws(()=>c.configureLockStagingReuse(true,invalid),/paused/);
  assert.deepEqual(f.events,[]);c.dirtyPending=true;c.configureLockStagingReuse(true,owner);
  assert.deepEqual(f.events,['dirty','refresh','staging:set','refresh']);
  assert.deepEqual(c.getLockStagingReuse(),{enabled:true,retainedBytes:0,reuses:(1n<<63n)+9n});
  c.configureLockStagingReuse(false,owner);assert.equal(c.lockStagingReuse,false);
  const busy=stagingFixture();busy.setBusy(true);assert.throws(()=>busy.cache.configureLockStagingReuse(true,owner),error=>error.status===12);
  assert.equal(busy.cache.lockStagingReuse,false);assert.equal(busy.cache.inlineFailure,null);
  for(const corrupt of [d=>d.setUint32(0,2,true),d=>d.setUint32(4,1,true),d=>d.setUint32(4,262145,true),d=>d.setUint32(0,0,true)]) {
    const bad=stagingFixture();bad.corrupt(corrupt);assert.throws(()=>bad.cache.configureLockStagingReuse(true,owner),/invalid/);
    assert.equal(bad.cache.lockStagingReuse,false);
  }
});

test('graphics overlap publishes before guarded mode changes and queries the exact fresh u64 counter',()=>{
  const f=overlapFixture(),c=f.cache,owner={paused:true,open:false};
  for(const invalid of [undefined,{paused:false,open:false},{paused:true,open:true}])
    assert.throws(()=>c.configureGraphicsOverlap(true,invalid),/paused/);
  assert.deepEqual(f.events,[]);c.dirtyPending=true;
  assert.equal(c.configureGraphicsOverlap(true,owner),true);assert.equal(c.graphicsOverlap,true);
  assert.deepEqual(f.events,['dirty','set']);
  assert.deepEqual(c.getGraphicsOverlap(),{enabled:true,completed:(1n<<63n)+7n});
  f.setNative(false,(1n<<63n)+11n);
  assert.deepEqual(c.getGraphicsOverlap(),{enabled:false,completed:(1n<<63n)+11n});
  assert.equal(c.graphicsOverlap,true,'a read-only query does not silently rewrite the cached configured mode');
});

test('graphics overlap Busy unsupported hosts and malformed native results cannot commit cached mode',()=>{
  const owner={paused:true,open:false},busy=overlapFixture();busy.setBusy(true);busy.cache.dirtyPending=true;
  assert.throws(()=>busy.cache.configureGraphicsOverlap(true,owner),error=>error.status===12);
  assert.equal(busy.cache.graphicsOverlap,false);assert.deepEqual(busy.events,['dirty','set']);
  const unsupported=overlapFixture();delete unsupported.cache.engine.api.component_cpu_graphics_overlap;
  assert.throws(()=>unsupported.cache.configureGraphicsOverlap(true,owner),/unsupported/);
  assert.equal(unsupported.cache.graphicsOverlap,false);assert.deepEqual(unsupported.events,[]);
  for(const corrupt of [d=>d.setUint32(0,2,true),d=>d.setUint32(4,1,true),d=>d.setUint32(0,0,true)]) {
    const f=overlapFixture();f.corrupt(corrupt);
    assert.throws(()=>f.cache.configureGraphicsOverlap(true,owner),/invalid/);
    assert.equal(f.cache.graphicsOverlap,false);
  }
  const f=overlapFixture();f.corrupt(d=>d.setUint32(4,1,true));
  assert.throws(()=>f.cache.getGraphicsOverlap(),/invalid/);
});

test('inline eligibility does not expand with future native safe flags or stale ownership',()=>{
  for(const options of [{id:0x4023d},{flags:1},{flags:2},{pendingBefore:true},{allow:()=>false},{allow:check=>check<2}]) {
    const f=inlineFixture(options);
    assert.equal(f.cache.tryInlineGate(f.trap()),false);
    assert.equal(f.completions(),0);assert.deepEqual(f.publications,[]);
  }
});

test('an inline deadline after completion yields only after importing the returned state',()=>{
  const f=inlineFixture({allow:check=>check<3});
  assert.equal(f.cache.tryInlineGate(f.trap()),'yield');
  assert.equal(f.cache.inlineYields,1);assert.equal(f.completions(),1);
  assert.equal(new DataView(f.cache.memory.buffer).getUint32(556,true),0x2000);
});

test('captured Busy decline and unexpected native pending work poison inline retries',()=>{
  for(const options of [{status:12},{status:256},{pendingAfter:true}]) {
    const f=inlineFixture(options);let failure;
    assert.throws(()=>f.cache.tryInlineGate(f.trap()),error=>{failure=error;return /inline gate completion/.test(error.message);});
    for(const retry of ['import','select','execute'])
      assert.throws(()=>V86CodeCache.prototype[retry].call(f.cache),error=>error===failure);
    assert.equal(f.completions(),1);assert.equal(f.publications.length,1);
  }
});

test('port fallback and completed yield park through fresh views after continuation grows memory',()=>{
  for(const continuation of [false,'yield']) {
    const memory=new WebAssembly.Memory({initial:1}),data=()=>new DataView(memory.buffer);
    const trap=Object.assign(Object.create(Traps.prototype),{
      memory,pending:null,portGates:true,
      cpu:{wm:{exports:{get_eflags:()=>data().getUint32(120,true)}}},
      readCode:address=>address===0x1000?0xe6:address===0x1001?0xe1:null,
      continueGate(){
        memory.grow(1);
        if(continuation==='yield') {
          data().setUint32(556,0x2000,true);data().setUint32(80,0x6100,true);
          data().setUint32(120,0x247,true);
        }
        return continuation;
      },
    });
    data().setUint8(612,3);data().setUint16(670,0x1b,true);data().setUint16(672,0x23,true);
    data().setUint32(80,0x6000,true);data().setUint32(120,0x246,true);
    data().setUint32(556,0x1002,true);data().setUint32(560,0x1000,true);
    trap.capturePort(true);
    assert.equal(data().getUint32(556,true),PARK);assert.equal(data().getUint32(620,true),0xffffffff);
    assert.equal(trap.pending.pc,continuation==='yield'?0x2000:0x1000);
    assert.equal(trap.pending.esp,continuation==='yield'?0x6100:0x6000);
    assert.equal(trap.pending.flags,continuation==='yield'?0x247:0x246);
    assert.equal(trap.pending.budgetYield===true,continuation==='yield');
    assert.equal(trap.pending.privateInstructions,0);
  }
});
