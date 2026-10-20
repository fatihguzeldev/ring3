import test from 'node:test';
import assert from 'node:assert/strict';
import {frameRateLabel} from './frame-rate.js';

const presentation = {frames:241,samples:240,intervalFps:60,meanIntervalMs:1000/60,sinceLastPresentMs:12};

test('session states suppress old frame rates while loading, paused or stopped', () => {
  for (const [state,label] of [['idle','not running'],['ready','not running'],['checking','checking'],['starting','starting'],
    ['loading','loading'],['paused','paused'],['stopped','stopped'],['exited','exited'],['error','unavailable']])
    assert.equal(frameRateLabel({state,presentation}),`— FPS · ${label}`);
});

test('a running game displays completed Present cadence and frame time', () => {
  assert.equal(frameRateLabel({state:'running',presentation}),'60.0 FPS · 16.7 ms');
  assert.equal(frameRateLabel({state:'waiting',presentation}),'60.0 FPS · 16.7 ms');
});

test('no frames and a single completed frame do not fabricate a frame rate', () => {
  assert.equal(frameRateLabel({state:'running'}),'— FPS · no completed frames');
  assert.equal(frameRateLabel({state:'running',rafFps:144,presentation:{frames:0}}),'— FPS · no completed frames');
  assert.equal(frameRateLabel({state:'running',presentation:{frames:1,samples:0,sinceLastPresentMs:8}}),
    '— FPS · waiting for frames');
});

test('a stale completed-Present sample stops displaying its old rate', () => {
  assert.equal(frameRateLabel({state:'running',presentation:{...presentation,sinceLastPresentMs:2000}}),
    '— FPS · no recent frames');
  assert.equal(frameRateLabel({state:'running',presentation:{frames:1,samples:0,sinceLastPresentMs:5000}}),
    '— FPS · no recent frames');
});

test('invalid timing values remain an unavailable estimate', () => {
  for (const changed of [{intervalFps:Infinity},{intervalFps:NaN},{intervalFps:-1},
    {meanIntervalMs:0},{meanIntervalMs:'16.7'},{sinceLastPresentMs:-1},{samples:0}])
    assert.equal(frameRateLabel({state:'running',presentation:{...presentation,...changed}}),'— FPS · waiting for frames');
});

test('a slow positive cadence is not rounded into zero FPS', () => {
  assert.equal(frameRateLabel({state:'running',presentation:{...presentation,intervalFps:0.04,meanIntervalMs:25000}}),
    '<0.1 FPS · 25000.0 ms');
});

test('a missing status update stops displaying the old rate without claiming no frames', () => {
  assert.equal(frameRateLabel({state:'running',presentation},2000),'— FPS · waiting for update');
  assert.equal(frameRateLabel({state:'paused',presentation},5000),'— FPS · paused');
});
