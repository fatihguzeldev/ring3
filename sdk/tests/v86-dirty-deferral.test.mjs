import assert from 'node:assert/strict';
import test from 'node:test';
import {V86CodeCache} from '../runtime/v86/index.js';
import {importState} from '../runtime/v86/state.js';
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
