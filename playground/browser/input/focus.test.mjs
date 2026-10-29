import assert from 'node:assert/strict';
import {test} from 'node:test';
import {fixture,unfocusedEvents} from './support.mjs';

test('window blur rejects trailing wheel and key input although canvas stays the active element', t => {
  const f=fixture(t);f.host.enable();f.blur();
  assert.equal(f.document.activeElement,f.canvas);
  f.event(f.canvas,'wheel',{deltaY:2,deltaMode:0});
  f.event(f.window,'keydown',{code:'KeyW'});f.frame();
  assert.deepEqual(unfocusedEvents(f.events),[],'physical presses/motion must not follow focus=false');
  assert.equal(f.host.active,false);
});

test('returning to the focused window restores guest focus before the next key without a second canvas focus event', t => {
  const f=fixture(t);f.host.enable();f.blur();f.focus();
  f.event(f.window,'keydown',{code:'ArrowDown'});
  assert.deepEqual(unfocusedEvents(f.events),[]);
  assert.deepEqual(f.events.slice(-2),[{kind:'focus',focused:true},{kind:'key',code:0xd0,down:true}]);
});

test('hidden or unfocused documents cannot publish focus on enable or a visibility event', t => {
  const f=fixture(t);f.document.hidden=true;f.document.focused=false;f.host.enable();
  f.event(f.document,'visibilitychange');
  assert.equal(f.events.some(event=>event.kind==='focus'&&event.focused),false);
  f.document.hidden=false;f.event(f.document,'visibilitychange');
  assert.equal(f.events.some(event=>event.kind==='focus'&&event.focused),false);
  f.canvas.focus();f.focus();
  assert.equal(f.events.at(-1).focused,true);
});

test('modal blocking releases pressed keys and restores input only after real canvas focus returns', t => {
  const f=fixture(t);f.host.enable();f.event(f.window,'keydown',{code:'KeyW'});
  f.host.setBlocked(true);
  assert.deepEqual(f.events.slice(-2),[{kind:'key',code:0x11,down:false},{kind:'focus',focused:false}]);
  f.host.setBlocked(false);f.event(f.window,'keydown',{code:'KeyS'});
  assert.deepEqual(unfocusedEvents(f.events),[]);
  assert.deepEqual(f.events.slice(-2),[{kind:'focus',focused:true},{kind:'key',code:0x1f,down:true}]);
  f.host.setBlocked(true);f.document.activeElement=new EventTarget();
  const count=f.events.length;f.host.setBlocked(false);
  assert.equal(f.events.length,count,'unblocking does not steal focus from another control');
});

test('disable and hidden modal dismissal cannot restore guest focus', t => {
  const f=fixture(t);f.host.enable();f.host.disable();f.focus();
  assert.equal(f.events.at(-1).focused,false);
  f.host.enable();f.host.setBlocked(true);f.document.hidden=true;
  const count=f.events.length;f.host.setBlocked(false);f.focus();
  assert.equal(f.events.length,count);
});
