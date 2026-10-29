import assert from 'node:assert/strict';
import test from 'node:test';
import {Engine} from '../runtime/engine.js';
import {Session} from '../runtime/session.js';
import {WindowPresentation} from '../runtime/windows.js';
import {CompletedWindowsCalls} from '../runtime/session/diagnostics.js';

test('borrowed results reflect the arena while take preserves an owned snapshot', () => {
  const engine = new Engine({}, new WebAssembly.Memory({initial: 1}), 'fixture');
  engine.base = 256;
  const bytes = new Uint8Array(engine.memory.buffer, engine.base + 140, 64);
  bytes[0] = 17;
  const borrowed = engine.resultView(64), snapshot = engine.take(64);
  assert.equal(borrowed.byteLength, 64);
  assert.equal(borrowed.getUint8(0), 17);
  bytes[0] = 29;
  assert.equal(borrowed.getUint8(0), 29);
  assert.equal(snapshot[0], 17);
});

test('state and result reads refresh after memory growth and arena relocation', () => {
  const engine = new Engine({}, new WebAssembly.Memory({initial: 1}), 'fixture');
  engine.base = 256;
  new DataView(engine.memory.buffer).setUint32(304, 0x12345678, true);
  assert.equal(engine.get(48), 0x12345678);
  engine.memory.grow(1);
  engine.base = 65536;
  const data = new DataView(engine.memory.buffer);
  data.setUint32(engine.base + 48, 0x87654321, true);
  data.setBigUint64(engine.base + 156, 0xfedcba9876543210n, true);
  assert.equal(engine.get(48), 0x87654321);
  assert.equal(engine.resultView(64).getBigUint64(16, true), 0xfedcba9876543210n);
  assert.throws(() => engine.get(65536), RangeError);
  assert.throws(() => engine.resultView(65536), RangeError);
});

test('media values survive later arena reuse without retaining the borrowed result', () => {
  let epoch = 7n;
  const engine = new Engine({fixture() {
    const data = new DataView(engine.memory.buffer, engine.base + 140, 64);
    data.setBigUint64(16, epoch, true);
    for (let i = 0; i < 5; i++) data.setBigUint64(24 + i * 8, epoch + BigInt(i), true);
    return 0;
  }}, new WebAssembly.Memory({initial: 1}), 'fixture');
  const first = engine.media('fixture');
  epoch = 11n;
  const second = engine.media('fixture');
  assert.deepEqual(first, {epoch: 7n, values: [7n, 8n, 9n, 10n, 11n]});
  assert.deepEqual(second, {epoch: 11n, values: [11n, 12n, 13n, 14n, 15n]});
});

test('an absent Windows callback returns its boolean synchronously', () => {
  const session=Object.create(Session.prototype),operations=[];
  session.engine={get:()=>3,ok:operation=>operations.push(operation),
    resultView:()=>new DataView(new ArrayBuffer(96)),
    take(){throw new Error('an absent callback must not copy the request');}};
  assert.equal(session.beginCallback(),false);
  assert.deepEqual(operations,['pending_windows_callback']);
});

test('a present callback retains its owned request across native result reuse and memory growth', () => {
  const memory = new WebAssembly.Memory({initial: 1}), calls = [], stack = [];
  const engine = new Engine({
    pending_windows_callback() {
      const request = new DataView(memory.buffer, 140, 96);
      for (const [offset, value] of [[12, 1], [16, 0x12345678], [40, 0xabcdef]])
        request.setUint32(offset, value, true);
      return 0;
    },
    core_begin_windows_callback(returnPc, returnId) {
      calls.push(['begin', returnPc, returnId]);
      memory.grow(1);
      new Uint8Array(memory.buffer, 140, 96).fill(0xcc);
      new Uint8Array(memory.buffer, 140, 64).fill(0);
      const callback = new DataView(memory.buffer, 140, 64);
      for (const [offset, value] of [[0, 0x42433352], [4, 0x10002], [8, 64],
        [16, 2], [20, 1], [24, 1], [32, 0x5000], [36, 0x8000], [40, returnPc], [44, returnId]])
        callback.setUint32(offset, value, true);
      return 0;
    },
  }, memory, 'fixture');
  new DataView(memory.buffer).setUint32(72, 3, true);
  const session = Object.assign(Object.create(Session.prototype), {
    engine, thread: 9, stack: () => stack,
    returnGate() {
      calls.push('returnGate');
      new Uint8Array(memory.buffer, 140, 96).fill(0xdd);
      return 0x7000;
    },
    callDiagnostics: {activation: {begin(request, thread) {
      calls.push(['activation', request.getUint32(16, true), request.getUint32(40, true), thread]);
    }}},
  });
  assert.equal(session.beginCallback(), true);
  assert.deepEqual(calls, ['returnGate', ['begin', 0x7000, 0xffffe],
    ['activation', 0x12345678, 0xabcdef, 9]]);
  assert.equal(stack.length, 1);
});

test('call capture consumes borrowed fields before native completion overwrites the arena', async () => {
  const memory = new WebAssembly.Memory({initial: 1});
  const api = {
    pending_windows_thread_schedule: () => 0,
    core_capture_windows_call() {
      const data = new DataView(memory.buffer, 140, 112);
      for (const [offset, value] of [[0, 0x46433352], [4, 0x10001], [8, 112], [16, 7],
        [20, 0x4010f], [24, 1], [28, 2], [36, 0x8000], [40, 0x1234], [48, 0xaaaa], [52, 0xbbbb]])
        data.setUint32(offset, value, true);
      return 0;
    },
    core_complete_windows_call() {
      new Uint8Array(memory.buffer, 140, 112).fill(0xcc);
      return 0;
    },
  };
  const engine = new Engine(api, memory, 'fixture');
  new DataView(memory.buffer).setUint32(72, 3, true);
  const windows = new WindowPresentation(engine, () => {}), diagnostics = new CompletedWindowsCalls(engine);
  let calls = 0;
  const session = Object.assign(Object.create(Session.prototype), {
    running: true, paused: false, stopped: false, lastSchedule: Infinity, lastStatus: Infinity,
    recent: [], thread: 1, blocked: new Map(), exits: new Map(), engine, windows,
    callDiagnostics: diagnostics, media: {update() {}, async graphicsOutput() {}, graphicsTransport: {}},
    code: {
      publish() {}, import() {}, select: () => null,
      execute() {
        if (++calls === 1) return {coreReason: 4, reason: 8, pc: 0x1000, detail: 0x4010f};
        session.running = false;
        return {coreReason: 2};
      },
    },
    async providers() { return false; }, observeClock: () => 0n, initializer: () => null,
    stack: () => [], async wait() {}, beginCallback() { return false; }, settleWindows() {},
  });
  await session.loop();
  assert.equal(calls, 2);
  assert.deepEqual(session.lastCall.args, [0xaaaa, 0xbbbb]);
  assert.deepEqual(windows.pending[0].receipt.callArgs, [0xaaaa, 0xbbbb]);
  assert.equal(windows.pending[0].returnPc, 0x1234);
  assert.deepEqual(diagnostics.pending[0].args, [0xaaaa, 0xbbbb]);
  assert.equal(diagnostics.pending[0].token, 7);
  assert.equal(diagnostics.pending[0].returnEsp, 0x8004);
  assert.equal(engine.resultView(112).getUint32(0, true), 0xcccccccc);
});

async function fusedGateCase(fusedStatus, backend = 'v86', observedClock, id = 0x40232) {
  const memory = new WebAssembly.Memory({initial: 1}), calls = [], token = 37;
  const state = new DataView(memory.buffer);
  state.setUint32(72, 3, true);
  function capture() {
    const record = new DataView(memory.buffer, 140, 112);
    for (const [offset, value] of [[16, token], [28, 1], [48, 0xaaaa]])
      record.setUint32(offset, value, true);
  }
  function completed() {
    state.setUint32(16, 0x88776655, true);
    state.setUint32(32, 0x8008, true);
    state.setUint32(48, 0x1234, true);
  }
  const engine = new Engine({
    pending_windows_thread_schedule() { calls.push('schedule'); return 0; },
    component_cpu_try_complete_gate() {
      calls.push('fused');
      if (fusedStatus !== 256) capture();
      if (fusedStatus === 0) completed();
      return fusedStatus;
    },
    core_capture_windows_call() { calls.push('capture'); capture(); return 0; },
    core_complete_windows_call(actualToken) {
      calls.push(['complete', actualToken]);
      completed();
      return 0;
    },
  }, memory, 'fixture');
  let executions = 0, clocks = 0, callbacks = 0, settled = 0, flushes = 0;
  const session = Object.assign(Object.create(Session.prototype), {
    running: true, paused: false, stopped: false, lastSchedule: Infinity, lastStatus: Infinity,
    recent: [], thread: 1, blocked: new Map(), exits: new Map(), engine,
    backend, fastNativeCalls: true, fastNativeCompletions: 0,
    windows: {capture(receipt) { calls.push(['window', receipt.args[0]]); }},
    media: {update() {}, async graphicsOutput() {}, graphicsTransport: {requestFlush(){flushes++;}}},
    code: {
      publish() {}, import() {}, select: () => null,
      execute() {
        if (++executions === 1)
          return {coreReason: 4, reason: 8, fastGate: false, pc: 0x1000, detail: id,observedClock};
        session.running = false;
        return {coreReason: 2};
      },
    },
    async providers() { return false; }, observeClock() { clocks++; return 123n; },
    initializer: () => null, stack: () => [], async wait() { session.running = false; },
    beginCallback() { callbacks++; return false; }, settleWindows() { settled++; },
  });
  await session.loop();
  return {session, engine, calls, clocks, callbacks, settled, token, flushes};
}

test('successful Present and D3D Busy completions demand a tail flush without widening other APIs',()=>{
  for(const [id,status,expected] of [[0x4020f,0,1],[0x40c03,0,1],[0x4024e,0,1],[0x4025a,0,1],[0x40232,12,1],
      [0x50001,12,1],[0x40232,0,0],[0x10001,12,0],[0x4020f,7,0]]) {
    let flushes=0;
    const session=Object.assign(Object.create(Session.prototype),{observeClock:()=>0n,settleWindows(){},
      engine:{status:()=>status},media:{graphicsTransport:{requestFlush(){flushes++;}}}});
    const complete=()=>session.complete({id,operation:'core_complete_windows_call',args:[]});
    if(status===7)assert.throws(complete);else assert.equal(complete(),status===0);
    assert.equal(flushes,expected,`id ${id.toString(16)} status ${status}`);
  }
});

test('fused Present and D3D Busy use the same flush demand without recapturing calls',async()=>{
  for(const [id,status] of [[0x4020f,0],[0x40c03,0],[0x40232,12],[0x50001,12]]) {
    const result=await fusedGateCase(status,'v86',undefined,id);
    assert.equal(result.flushes,1);assert(!result.calls.includes('capture'));
  }
});

test('pause publishes and drains the native tail before reporting paused or allowing resume',async()=>{
  const events=[],controller=new AbortController();let release;
  const draining=new Promise(resolve=>{release=resolve;});
  const session=Object.assign(Object.create(Session.prototype),{paused:false,pausedDuration:0,running:true,
    abort:controller,code:{publish(){events.push('publish');}},
    media:{pauseAudio(){events.push('hold');},resumeAudio(){events.push('refill');},
      graphicsTransport:{drain(){events.push('drain');return draining;}}},status:state=>events.push(state)});
  const pausing=session.pause(),resuming=session.resume();
  try {
    assert.equal(session.paused,true);assert.deepEqual(events,['hold','publish','drain']);
    assert.equal(controller.signal.aborted,false);release();await pausing;await resuming;
    assert.deepEqual(events,['hold','publish','drain','paused','refill','running']);assert.equal(session.paused,false);
  } finally {release();await pausing;await resuming;}
});

test('fused gate completion settles and checks scheduling without a second capture or completion', async () => {
  const {session, engine, calls, clocks, callbacks, settled, token} = await fusedGateCase(0);
  assert.deepEqual(calls, ['schedule', 'fused', ['window', token], 'schedule']);
  assert.equal(clocks, 2);
  assert.equal(callbacks, 1);
  assert.equal(settled, 1);
  assert.equal(session.fastNativeCompletions, 1);
  assert.deepEqual(session.lastCall, {id: 0x40232, pc: 0x1000, args: [0xaaaa]});
  assert.equal(engine.get(16), 0x88776655);
  assert.equal(engine.get(32), 0x8008);
  assert.equal(engine.get(48), 0x1234);
});

test('declined fused gate captures once and reuses its already observed completion clock', async () => {
  const {session, calls, clocks, callbacks, settled, token} = await fusedGateCase(256);
  assert.deepEqual(calls, ['schedule', 'fused', 'capture', ['window', token], ['complete', token], 'schedule']);
  assert.equal(clocks, 2);
  assert.equal(callbacks, 1);
  assert.equal(settled, 1);
  assert.equal(session.fastNativeCompletions, 0);
});

test('an inline-declined gate reuses its observed clock through ordinary capture and completion',async()=>{
  const {calls,clocks,token}=await fusedGateCase(256,'v86',0n);
  assert.equal(clocks,1,'only the outer host-loop observation remains');
  assert.deepEqual(calls,['schedule','fused','capture',['window',token],['complete',token],'schedule']);
});

test('busy fused gate retains the returned capture token for the existing blocked-call retry', async () => {
  const {session, calls, clocks, callbacks, settled, token} = await fusedGateCase(12);
  assert.deepEqual(calls, ['schedule', 'fused', ['window', token]]);
  assert.equal(clocks, 2);
  assert.equal(callbacks, 0);
  assert.equal(settled, 0);
  assert.equal(session.needsSchedule, true);
  assert.equal(session.fastNativeCompletions, 0);
  assert.deepEqual(session.blocked.get(1), {
    pc: 0x1000, id: 0x40232, callArgs: [0xaaaa],
    operation: 'core_complete_windows_call', args: [token],
  });
});

test('the core backend keeps the ordinary capture and completion path', async () => {
  const {session, calls, clocks, token} = await fusedGateCase(0, 'core');
  assert.deepEqual(calls, ['schedule', 'capture', ['window', token], ['complete', token], 'schedule']);
  assert.equal(clocks, 2);
  assert.equal(session.fastNativeCompletions, 0);
});

test('an inline completion yield returns to the host before another CPU entry or gate capture', async () => {
  let executions=0,published=0,waits=0;
  const session=Object.assign(Object.create(Session.prototype),{
    running:true,paused:false,stopped:false,lastSchedule:Infinity,lastStatus:Infinity,
    recent:[],thread:1,blocked:new Map(),exits:new Map(),
    engine:{get:()=>3,api:{pending_windows_thread_schedule:()=>0},
      ok(){throw new Error('a completed inline yield must not recapture its gate');}},
    media:{update(){},async graphicsOutput(){},graphicsTransport:{}},
    code:{publish(){published++;},import(){},select:()=>null,execute(){
      assert.equal(++executions,1,'the host must see the completed yield before another CPU entry');
      return {coreReason:2,inlineYield:true};
    }},
    async providers(){return false;},observeClock:()=>0n,initializer:()=>null,stack:()=>[],
    async wait(){waits++;session.running=false;},
  });
  await session.loop();
  assert.equal(executions,1);assert.equal(published,2);assert.equal(waits,1);
});
