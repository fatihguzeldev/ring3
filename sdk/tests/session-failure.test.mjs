import assert from 'node:assert/strict';
import test from 'node:test';
import {Session} from '../runtime/session.js';

function fixture(sampleUsage,stats=()=>({regions:17})) {
  const events=[],calls=[];
  const session=new Session(message=>events.push(message));
  session.running=true;
  session.engine={get(offset){assert.equal(offset,48);return 0x70000af0;}};
  session.code={publish(){calls.push('publish');},sampleUsage,stats};
  session.lastCall={id:0x100015,pc:0x70001270,args:[337227312]};
  session.recent=[{pc:0x1000,id:3n}];
  return {session,events,calls};
}

test('a trapped CPU retains its original stack and emits error status when native cache sampling fails',()=>{
  const unavailable=new Error('native cache descriptor unavailable after trap');
  const f=fixture(()=>{throw unavailable;});
  const trap=new WebAssembly.RuntimeError('unreachable');
  trap.stack='RuntimeError: unreachable\n    at wasm-function[4319]:0x1234\n    at originalNativeCall';
  assert.doesNotThrow(()=>f.session.fail(trap));
  assert.equal(f.session.running,false);assert.equal(f.session.scheduler.closed,true);
  assert.deepEqual(f.calls,[],'terminal failure never republishes native CPU state');
  assert.equal(f.events.length,2);
  const [diagnostic,status]=f.events;
  assert.equal(diagnostic.type,'diagnostic');assert.equal(diagnostic.message,'unreachable');
  assert.equal(diagnostic.detail.name,'RuntimeError');assert.equal(diagnostic.detail.stack,trap.stack);
  assert.deepEqual(diagnostic.detail.call,f.session.lastCall);
  assert.deepEqual(diagnostic.detail.recent,[{pc:0x1000,id:'3'}]);
  assert.deepEqual([status.type,status.state,status.message,status.pc],
    ['status','error','unreachable',0x70000af0]);
  assert.deepEqual(status.stats,{regions:17});
  assert.deepEqual(status.unavailable,[{field:'cache.sampleUsage',name:'Error',message:unavailable.message}]);
  f.session.fail(new Error('later cleanup failure'));
  assert.equal(f.events.length,2,'a later failure cannot replace the first error');
});

test('unavailable sampling and stats cannot suppress the original terminal error',()=>{
  const f=fixture(()=>undefined,()=>{throw new Error('native stats unavailable');});
  const original=new Error('first failure');original.stack='the original complete stack';
  assert.doesNotThrow(()=>f.session.fail(original));
  assert.equal(f.events[0].detail.stack,original.stack);
  assert.equal(f.events[0].message,'first failure');
  const terminal=f.events[1];
  assert.equal(terminal.state,'error');assert.equal(terminal.stats,undefined);
  assert.deepEqual(terminal.unavailable,[
    {field:'cache.sampleUsage',message:'unavailable'},
    {field:'cache.stats',name:'Error',message:'native stats unavailable'},
  ]);
});

test('unavailable PC and instruction diagnostics cannot replace or suppress the first failure',()=>{
  const f=fixture(()=>({regions:17})),readFailure=new Error('CPU packet is unavailable');
  // An unavailable engine allocation also makes unsupportedInstruction's
  // initial memory snapshot fail, before its own optional byte-read guards.
  f.session.engine={get(){throw readFailure;}};
  const original=new Error('original unsupported CPU stop');
  original.reason=4;original.stack='the first CPU failure stack';
  assert.doesNotThrow(()=>f.session.fail(original));
  assert.equal(f.events.length,2);
  const [diagnostic,status]=f.events;
  assert.equal(diagnostic.message,original.message);
  assert.equal(diagnostic.detail.stack,original.stack);
  assert.equal(diagnostic.detail.pc,undefined);assert.equal(diagnostic.detail.instruction,undefined);
  assert.deepEqual(diagnostic.detail.unavailable.map(item=>item.field),['pc','instruction']);
  assert.equal(diagnostic.detail.unavailable[0].message,readFailure.message);
  assert.equal(status.state,'error');assert.equal(status.message,original.message);
  assert.equal(status.pc,undefined);
  assert.deepEqual(status.unavailable,[{field:'pc',name:'Error',message:readFailure.message}]);
});

test('ordinary running status still reports sampling failures to its caller',()=>{
  const failure=new Error('ordinary status failure'),f=fixture(()=>{throw failure;});
  assert.throws(()=>f.session.status('running','running'),error=>error===failure);
  assert.deepEqual(f.calls,['publish']);assert.deepEqual(f.events,[]);
  f.session.scheduler.close();
});
