import assert from 'node:assert/strict';
import {test} from 'node:test';
import {Session} from '../session.js';

function pausedSession() {
  let finishAudio;
  const messages = [], input = [];
  const session = Object.assign(Object.create(Session.prototype), {
    state:'paused', generation:1, failed:false,
    worker:{postMessage:message => messages.push(message)},
    audio:{resume:() => new Promise(resolve => { finishAudio = resolve; })},
    input:{enable:() => input.push('enable'),disable:() => input.push('disable')},
    onStatus:() => {},
  });
  return {session,messages,input,finishAudio:() => finishAudio()};
}

test('resume waits for the worker running acknowledgement before enabling input',async () => {
  const {session,messages,input,finishAudio} = pausedSession();
  const resumed = session.resume();
  assert.deepEqual(messages,[]);
  finishAudio();
  await resumed;
  assert.deepEqual(messages,[{type:'resume'}]);
  assert.deepEqual(input,[]);
  await session.message({type:'status',state:'running',message:'resumed'},session.worker);
  assert.deepEqual(input,['enable']);
});

test('a terminal error during audio resume cannot restart execution or input',async () => {
  const {session,messages,input,finishAudio} = pausedSession();
  const resumed = session.resume();
  session.failed = true;
  session.state = 'error';
  finishAudio();
  await resumed;
  assert.deepEqual(messages,[]);
  assert.deepEqual(input,[]);
});

test('an old audio resume cannot send commands to a replacement worker',async () => {
  const {session,messages,input,finishAudio} = pausedSession();
  const resumed = session.resume();
  session.generation++;
  session.worker = {postMessage:message => messages.push(message)};
  finishAudio();
  await resumed;
  assert.deepEqual(messages,[]);
  assert.deepEqual(input,[]);
});
