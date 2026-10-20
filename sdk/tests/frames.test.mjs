import test from 'node:test';
import assert from 'node:assert/strict';
import {FrameMeasurements} from '../browser/session/frames.js';
import {Session} from '../browser/session.js';

test('completed Present intervals stay bounded and remain independent of display refresh', () => {
  const frames=new FrameMeasurements();
  for(let index=0;index<243;index++)frames.present(1000+index*20);
  const value=frames.snapshot(5850);
  assert.equal(value.frames,243); assert.equal(value.samples,240);
  assert.equal(value.intervalFps,50); assert.equal(value.meanIntervalMs,20);
  assert.equal(value.sinceLastPresentMs,10);
});

test('resetting an interval window preserves lifetime frames and excludes the paused gap', () => {
  const frames=new FrameMeasurements();
  frames.present(0); frames.present(20); frames.present(40);
  frames.resetIntervals();
  const paused=frames.snapshot(5000);
  assert.equal(paused.frames,3); assert.equal(paused.samples,0);
  assert.equal(paused.intervalFps,null); assert.equal(paused.sinceLastPresentMs,null);
  frames.present(5020);
  assert.equal(frames.snapshot(5020).samples,0);
  frames.present(5040);
  const resumed=frames.snapshot(5040);
  assert.equal(resumed.frames,5); assert.equal(resumed.samples,1);
  assert.equal(resumed.meanIntervalMs,20); assert.equal(resumed.intervalFps,50);
});

test('browser pause and resume clear interval continuity without resetting normal status updates', () => {
  const frames=new FrameMeasurements(),updates=[];
  const session={state:'running',frames,input:{snapshot:()=>({})},onStatus:value=>updates.push(value)};
  frames.present(0); frames.present(20);
  Session.prototype.status.call(session,'paused','paused');
  assert.equal(updates.at(-1).presentation.frames,2);
  assert.equal(updates.at(-1).presentation.samples,0);
  frames.present(50); frames.present(70);
  Session.prototype.status.call(session,'running','running');
  assert.equal(updates.at(-1).presentation.frames,4);
  assert.equal(updates.at(-1).presentation.samples,0);
  frames.present(100); frames.present(120);
  Session.prototype.status.call(session,'running','running');
  Session.prototype.status.call(session,'running','running');
  assert.equal(updates.at(-1).presentation.frames,6);
  assert.equal(updates.at(-1).presentation.samples,1);
  assert.equal(updates.at(-1).presentation.intervalFps,50);
});

test('Worker status messages preserve the transition needed to reset presentation intervals', async () => {
  const frames=new FrameMeasurements(),updates=[];
  let enabled=0;
  const session={state:'running',frames,status:Session.prototype.status,
    input:{snapshot:()=>({}),enable:()=>enabled++,disable:()=>{}},onStatus:value=>updates.push(value)};
  const worker={};
  frames.present(0); frames.present(20);
  await Session.prototype.message.call(session,{type:'status',state:'paused',message:'paused'},worker);
  assert.equal(updates.at(-1).presentation.frames,2);
  assert.equal(updates.at(-1).presentation.samples,0);
  frames.present(50); frames.present(70);
  await Session.prototype.message.call(session,{type:'status',state:'running',message:'running'},worker);
  assert.equal(updates.at(-1).presentation.frames,4);
  assert.equal(updates.at(-1).presentation.samples,0);
  assert.equal(enabled,1);
  frames.present(100); frames.present(120);
  await Session.prototype.message.call(session,{type:'status',state:'running',message:'running'},worker);
  assert.equal(updates.at(-1).presentation.samples,1);
  assert.equal(updates.at(-1).presentation.intervalFps,50);
  assert.equal(enabled,1);
});
