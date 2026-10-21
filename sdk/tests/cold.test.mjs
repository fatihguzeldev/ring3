import assert from 'node:assert/strict';
import {test} from 'node:test';
import {Session} from '../runtime/session.js';
import {PhaseMeasurements} from '../runtime/session/measurement.js';

for(const measured of [false,true]) {
  test(`session selects the cold tier without eager entry${measured?' with phase measurements':''}`,async()=>{
    const calls=[],phases=[];
    const session=Object.assign(Object.create(Session.prototype),{
      running:true,paused:false,stopped:false,lastSchedule:Infinity,lastStatus:Infinity,
      recent:[],thread:1,blocked:new Map(),exits:new Map(),
      engine:{get:()=>3,api:{pending_windows_thread_schedule:()=>0}},
      code:{
        publish(){calls.push('publish');},import(){calls.push('import');},
        entry(){throw new Error('eager compilation bypassed cold selection');},
        select(){calls.push('select');return null;},
        execute(selected){
          assert.equal(selected,null);calls.push('execute');session.running=false;
          return {kind:'cold',id:null,pc:0x1004,coreReason:2,reason:1,detail:0,retired:1,work:1,regions:0};
        },
      },
      media:{update(){},async graphicsOutput(){},graphicsTransport:{}},
      async providers(){return false;},observeClock:()=>0n,initializer:()=>null,stack:()=>[],
      async wait(){},
      measurements:measured ? {
        run(name,operation){phases.push(name);return operation();},
        async runAsync(name,operation){phases.push(name);return operation();},
      } : null,
    });
    await session.loop();
    assert.deepEqual(calls,['publish','import','select','execute','publish']);
    assert.equal(session.recent.length,1);assert.equal(session.recent[0].kind,'cold');
    assert.equal(session.recent[0].id,null);assert.equal(session.recent[0].regions,0);
    assert.deepEqual(phases,measured ? ['providers','entry','execution'] : []);
  });
}

for (const mode of ['active-probe', 'observed-without-probe', 'mismatched-probe', 'unmeasured']) {
  test(`session classifies execution only for its active probe measurements: ${mode}`, async t => {
    t.mock.method(performance, 'now', () => 0);
    const calls = [], phases = [], recorded = [], unit = Object.freeze({id: 7n});
    let selected = 0, executed = 0;
    const measurements = mode === 'unmeasured' ? null : {
      run(name, operation, classification) {
        phases.push([name, classification]);
        return operation();
      },
      async runAsync(name, operation) {
        phases.push([name, undefined]);
        return await operation();
      },
    };
    const session = Object.assign(Object.create(Session.prototype), {
      running: true, paused: false, stopped: false, lastSchedule: Infinity, lastStatus: Infinity,
      recent: [], thread: 1, blocked: new Map(), exits: new Map(), measurements,
      frameProbe: mode === 'active-probe' ? {measurements,recordExecution(run){recorded.push(run);}}
        : mode === 'mismatched-probe' ? {measurements: {},recordExecution(){}} : null,
      engine: {get: () => 3, api: {pending_windows_thread_schedule: () => 0}},
      code: {
        publish() { calls.push('publish'); }, import() { calls.push('import'); },
        entry() { throw new Error('classification must not bypass cold selection'); },
        select() { calls.push('select'); return selected++ === 0 ? null : unit; },
        execute(value) {
          const cold = executed++ === 0;
          assert.equal(value, cold ? null : unit);
          calls.push('execute');
          if (!cold) session.running = false;
          return {kind: cold ? 'cold' : 'compiled', id: cold ? null : unit.id,
            pc: 0x1000, coreReason: cold ? 1 : 2, reason: cold ? null : 1,
            detail: cold ? null : 0, retired: cold ? 0 : 1, work: cold ? 0 : 1, regions: cold ? 0 : 1};
        },
      },
      media: {update() {}, async graphicsOutput() {}, graphicsTransport: {}},
      async providers() { return false; }, observeClock: () => 0n,
      initializer: () => null, stack: () => [], async wait() {},
    });
    await session.loop();
    assert.deepEqual(calls, ['publish', 'import', 'select', 'execute', 'select', 'execute', 'publish']);
    assert.deepEqual(session.recent.map(run => [run.kind, run.id]), [['cold', null], ['compiled', 7n]]);
    assert.deepEqual(recorded,mode==='active-probe'?session.recent:[]);
    assert.deepEqual(phases, mode === 'unmeasured' ? [] : [
      ['providers', undefined], ['entry', undefined],
      ['execution', mode === 'active-probe' ? 'cold' : undefined],
      ['entry', undefined], ['execution', mode === 'active-probe' ? 'compiled' : undefined],
    ]);
  });
}

for (const mode of ['active', 'foreign', 'unmeasured']) {
  test(`session records the actual start PC and existing elapsed only for the captured probe: ${mode}`, async t => {
    let now = 0, selections = 0, pcReads = 0;
    t.mock.method(performance, 'now', () => now);
    const measurements = mode === 'unmeasured' ? null : new PhaseMeasurements();
    const selected = Object.freeze({pc: 0x9000, id: 7n}), recorded = [], failure = new Error('execute failed');
    const first = {kind: 'cold', id: null, pc: 0x7777, coreReason: 1, work: 2, retired: 2, regions: 0};
    const probe = {measurements: mode === 'active' ? measurements : new PhaseMeasurements(),
      recordExecution(...args) { recorded.push(args); }};
    const session = Object.assign(Object.create(Session.prototype), {
      running: true, paused: false, stopped: false, lastSchedule: Infinity, lastStatus: Infinity,
      recent: [], thread: 1, blocked: new Map(), exits: new Map(), measurements, frameProbe: probe,
      engine: {get: () => 3, api: {pending_windows_thread_schedule: () => 0}},
      code: {
        publish() {}, import() {}, select() { return ++selections === 1 ? null : selected; },
        get pc() { pcReads++; return selections === 1 ? 0x1234 : 0x2345; },
        execute(unit) {
          if (selections === 1) {assert.equal(unit, null); now = 2; return first;}
          assert.equal(unit, selected); now = 7; throw failure;
        },
      },
      media: {update() {}, async graphicsOutput() {}, graphicsTransport: {}},
      async providers() {return false;}, observeClock: () => 0n,
      initializer: () => null, stack: () => [], async wait() {},
    });
    await assert.rejects(session.loop(), error => error === failure);
    assert.deepEqual(recorded, mode === 'active' ? [[first, 0x1234, null, 2], [undefined, 0x2345, selected, 5]] : []);
    assert.equal(pcReads, mode === 'active' ? 2 : 0, 'normal and foreign-observer paths add no start-PC query');
    if (measurements) assert.equal(measurements.snapshot().execution.elapsedNs, 7_000_000);
  });
}
