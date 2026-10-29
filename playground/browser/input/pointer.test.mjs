import assert from 'node:assert/strict';
import {test} from 'node:test';
import {fixture,unfocusedEvents} from './support.mjs';

test('pointer lock arriving after blur cannot publish capture=true or stale relative movement', t => {
  const f=fixture(t);f.host.enable();f.blur();
  f.document.pointerLockElement=f.canvas;
  f.event(f.document,'mousemove',{target:f.canvas,movementX:8,movementY:-3});
  f.event(f.document,'pointerlockchange');f.frame();
  assert.deepEqual(unfocusedEvents(f.events),[]);
  assert.equal(f.document.pointerLockElement,null);
  assert.equal(f.captures.at(-1),false);
});

test('focus loss discards scheduled motion and modal block prevents pointer presses', t => {
  const f=fixture(t);f.host.enable();
  f.document.pointerLockElement=f.canvas;f.event(f.document,'pointerlockchange');
  f.event(f.document,'mousemove',{movementX:17,movementY:4});
  const count=f.events.length;f.blur();f.frame();
  assert.equal(f.events.slice(count).some(event=>event.kind==='relative'),false);
  f.focus();f.host.setBlocked(true);
  f.event(f.canvas,'mousedown',{button:0,clientX:10,clientY:10});
  assert.deepEqual(unfocusedEvents(f.events),[]);
});

test('focused canvas preserves pointer mapping and propagates real send errors', t => {
  const f=fixture(t);f.host.enable();
  f.event(f.canvas,'mousedown',{button:0,clientX:20,clientY:30});
  assert.deepEqual(f.events.slice(-2),[{kind:'pointer',x:20,y:30},{kind:'button',button:0,down:true}]);
  const failure=new Error('real engine transport failure');
  f.host.send=()=>{throw failure;};
  assert.throws(()=>f.host.key({code:'KeyW',preventDefault(){}},true),error=>error===failure);
  f.host.send=()=>{};
});

test('unlocked physical motion preserves relative deltas before the final absolute correction', t => {
  const f=fixture(t);f.host.enable();
  f.event(f.document,'mousemove',{target:f.canvas,clientX:20,clientY:30,movementX:7,movementY:-2});
  f.event(f.document,'mousemove',{target:f.canvas,clientX:24,clientY:35,movementX:4,movementY:5});
  f.frame();
  assert.deepEqual(f.events.slice(-2),[
    {kind:'relative',dx:11,dy:3,wheel:0},
    {kind:'pointer',x:24,y:35},
  ]);
});

test('unlocked outside and letterbox motion cannot publish physical or absolute movement', t => {
  const f=fixture(t);f.host.enable();
  f.canvas.getBoundingClientRect=()=>({left:0,top:0,width:640,height:640});
  const before=f.events.length;
  f.event(f.document,'mousemove',{target:f.canvas,clientX:20,clientY:20,movementX:7,movementY:5});
  f.event(f.document,'mousemove',{target:new EventTarget(),clientX:20,clientY:100,movementX:7,movementY:5});
  f.frame();
  assert.equal(f.events.length,before);
});

test('locked movement remains relative-only and focus loss clears pending unlocked deltas', t => {
  const f=fixture(t);f.host.enable();
  f.document.pointerLockElement=f.canvas;
  f.event(f.document,'mousemove',{movementX:7,movementY:3});f.frame();
  assert.deepEqual(f.events.at(-1),{kind:'relative',dx:7,dy:3,wheel:0});
  assert.equal(f.events.some(event=>event.kind==='pointer'),false);
  f.document.pointerLockElement=null;
  f.event(f.document,'mousemove',{target:f.canvas,clientX:20,clientY:30,movementX:4,movementY:5});
  f.blur();const afterBlur=f.events.length;f.frame();
  assert.equal(f.events.length,afterBlur);
});
