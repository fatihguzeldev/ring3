import test from 'node:test';
import assert from 'node:assert/strict';
import {VisibilityController} from '../visibility.js';

const flush = async () => { for (let i=0;i<8;i++) await Promise.resolve(); };
function fixture(state='running') {
  let hidden=false;
  const calls=[],errors=[],diagnostics=[];
  let current = state;
  const session={status:()=>({state:current}),
    pause:async()=>{calls.push('pause');},resume:async()=>{calls.push('resume');}};
  const controller=new VisibilityController(session,{hidden:()=>hidden,onError:error=>errors.push(error),onDiagnostic:message=>diagnostics.push(message)});
  return {session,controller,calls,errors,diagnostics,
    visibility(value) {hidden=value;controller.changed();},
    status(value) {current=value;controller.status({state:value});}};
}

test('returning to the tab resumes only the pause requested by visibility',async()=>{
  const f=fixture();
  f.visibility(true);await flush();
  assert.deepEqual(f.calls,['pause']);
  f.status('paused');await flush();
  assert.deepEqual(f.calls,['pause']);
  f.visibility(false);await flush();
  assert.deepEqual(f.calls,['pause','resume']);
  assert.deepEqual(f.diagnostics,['tab visibility requested pause','tab visibility requested resume']);
  f.status('running');f.visibility(false);await flush();
  assert.deepEqual(f.calls,['pause','resume']);
});

test('a manual pause survives hiding and returning to the tab',async()=>{
  const f=fixture();
  f.controller.manualToggle();await flush();f.status('paused');await flush();
  f.visibility(true);f.visibility(false);await flush();
  assert.deepEqual(f.calls,['pause']);
  assert.deepEqual(f.diagnostics,[]);
  f.controller.manualToggle();await flush();
  assert.deepEqual(f.calls,['pause','resume']);
});

test('show before pause acknowledgement waits for both acknowledgement and async completion',async()=>{
  const f=fixture();
  let completePause;
  f.session.pause=()=>{f.calls.push('pause');return new Promise(resolve=>{completePause=resolve;});};
  f.visibility(true);await flush();f.visibility(false);f.status('paused');await flush();
  assert.deepEqual(f.calls,['pause']);
  completePause();await flush();
  assert.deepEqual(f.calls,['pause','resume']);
  f.status('running');await flush();
  assert.deepEqual(f.calls,['pause','resume']);
});

test('manual pause takes ownership while an automatic pause is awaiting acknowledgement',async()=>{
  const f=fixture();
  f.visibility(true);await flush();f.visibility(false);
  f.controller.manualToggle();f.status('paused');await flush();
  assert.deepEqual(f.calls,['pause']);
});

test('a hidden tab pauses when loading transitions to running',async()=>{
  const f=fixture('loading');
  f.visibility(true);await flush();assert.deepEqual(f.calls,[]);
  f.status('running');await flush();assert.deepEqual(f.calls,['pause']);
});

test('terminal status cannot be revived by a pending visibility pause',async()=>{
  for(const state of ['error','exited','stopped']) {
    const f=fixture();let completePause;
    f.session.pause=()=>{f.calls.push('pause');return new Promise(resolve=>{completePause=resolve;});};
    f.visibility(true);await flush();f.status(state);f.visibility(false);
    completePause();await flush();
    assert.deepEqual(f.calls,['pause'],state);
  }
});

test('a new public launch discards old visibility ownership and async completion',async()=>{
  const f=fixture();let completePause;
  f.session.pause=()=>{f.calls.push('pause');return new Promise(resolve=>{completePause=resolve;});};
  f.visibility(true);await flush();f.visibility(false);
  f.controller.reset();f.status('loading');
  completePause();await flush();f.status('running');await flush();
  assert.deepEqual(f.calls,['pause']);
});

test('hiding again during resume queues the next pause after running acknowledgement',async()=>{
  const f=fixture();
  f.visibility(true);await flush();f.status('paused');await flush();
  f.visibility(false);await flush();f.visibility(true);await flush();
  assert.deepEqual(f.calls,['pause','resume']);
  f.status('running');await flush();
  assert.deepEqual(f.calls,['pause','resume','pause']);
});
