import assert from 'node:assert/strict';
import test from 'node:test';
import {InputHost} from '../browser/input.js';
import {Media} from '../runtime/media.js';
import {Session} from '../runtime/session.js';
import {Session as BrowserSession} from '../browser/session.js';

const ms = value => BigInt(value) * 1_000_000n;

function mediaFixture(clock = {originMs:1000,nowNs:ms(200)}) {
  const operations=[];
  let floor=0n,observed=ms(200),request,afterSubmit;
  const engine={
    media(operation) {
      if(operation==='pending_user32_cursor') return {values:[0n,0n,0n,0n,0n]};
      assert.equal(operation,'windows_input_time');
      operations.push({operation});
      return {epoch:7n,values:[floor,observed,0n,0n,0n]};
    },
    request(bytes) {request=bytes.slice();},
    ok(operation,...args) {
      if(operation==='submit_windows_input') {
        const d=new DataView(request.buffer),events=[];
        assert.equal(d.getBigUint64(8,true),7n);
        assert.equal(d.getUint32(24,true),(request.length-32)/32);
        for(let at=32;at<request.length;at+=32)
          events.push({time:d.getBigUint64(at,true),kind:d.getUint32(at+8,true),a:d.getUint32(at+12,true),b:d.getUint32(at+16,true)});
        for(const event of events) {assert.ok(event.time>=floor);assert.ok(event.time<=observed);floor=event.time;}
        operations.push({operation,sequence:d.getBigUint64(16,true),events});
        if(afterSubmit!==undefined) {floor=afterSubmit;afterSubmit=undefined;}
      } else if(operation==='submit_windows_pointer') {
        const time=BigInt(args[2])+BigInt(args[3])*0x1_0000_0000n;
        assert.ok(time>=floor);assert.ok(time<=observed);floor=time;
        operations.push({operation,time,x:args[4],y:args[5]});
      } else if(operation==='sync_windows_input') operations.push({operation});
      else throw new Error(`unexpected input operation ${operation}`);
    },
    inputMeasurementsEnabled:true,
  };
  const media=new Media(engine,7n,()=>{},null,()=>({...clock}));
  media.fillAudio=media.publishDialogs=media.publishGdi=()=>{};
  return {media,operations,clock,setFloor:value=>{floor=value;},setObserved:value=>{observed=value;},afterSubmit:value=>{afterSubmit=value;}};
}

function browserFixture(t) {
  const previous={window:globalThis.window,document:globalThis.document,requestAnimationFrame:globalThis.requestAnimationFrame,cancelAnimationFrame:globalThis.cancelAnimationFrame};
  const window=new EventTarget(),document=new EventTarget(),canvas=new EventTarget(),packets=[];
  Object.assign(document,{hasFocus:()=>true,hidden:false,activeElement:canvas,pointerLockElement:null});
  Object.assign(canvas,{width:640,height:480,clientHeight:480,focus(){document.activeElement=canvas;},getBoundingClientRect:()=>({left:0,top:0,width:640,height:480})});
  Object.assign(globalThis,{window,document,requestAnimationFrame:()=>1,cancelAnimationFrame(){}});
  let now=100;
  t.mock.method(performance,'now',()=>now);
  const host=new InputHost(canvas,{send:events=>packets.push(structuredClone(events))});
  t.after(()=>{host.close();Object.assign(globalThis,previous);});
  host.enable();
  const key=(type,code,timeStamp)=>{
    const event=new Event(type,{cancelable:true});
    Object.defineProperties(event,{code:{value:code},timeStamp:{value:timeStamp}});
    window.dispatchEvent(event);
  };
  return {host,packets,key,window,document,canvas,set:time=>{now=time;}};
}

test('execution pause releases held inputs without manufacturing guest focus loss',async t=>{
  const b=browserFixture(t),messages=[];
  b.key('keydown','KeyA',100);b.host.buttons.add(0);
  const before=b.packets.length,session=Object.assign(Object.create(BrowserSession.prototype),{
    state:'running',worker:{},generation:1,failed:false,input:b.host,
    async holdAudio(){},audio:{async pause(){}},post:message=>messages.push(message),
  });
  await session.pause();
  assert.deepEqual(b.packets.slice(before).flat().map(({kind,code,button,down})=>({kind,code,button,down})),[
    {kind:'key',code:30,button:undefined,down:false},
    {kind:'button',code:undefined,button:0,down:false},
  ]);
  assert.equal(b.host.enabled,false);assert.equal(b.host.focused,true);
  b.host.enable();
  assert.deepEqual(b.packets.flat().filter(event=>event.kind==='focus').map(event=>event.focused),[true]);
  assert.deepEqual(messages,[{type:'pause'}]);
});

test('preserved pause forwards genuine focus and capture losses and reconciles current focus on resume',t=>{
  const b=browserFixture(t),focuses=()=>b.packets.flat().filter(event=>event.kind==='focus').map(event=>event.focused);
  b.host.disable({preserveFocus:true});
  b.document.hasFocus=()=>false;b.window.dispatchEvent(new Event('blur'));
  assert.deepEqual(focuses(),[true,false]);
  b.host.enable();assert.equal(b.host.focused,false);assert.deepEqual(focuses(),[true,false]);
  b.document.hasFocus=()=>true;b.window.dispatchEvent(new Event('focus'));
  assert.deepEqual(focuses(),[true,false,true]);
  b.host.disable({preserveFocus:true});b.host.setBlocked(true);
  assert.deepEqual(focuses(),[true,false,true,false]);
  b.host.enable();assert.equal(b.host.focused,false);b.host.setBlocked(false);
  assert.deepEqual(focuses(),[true,false,true,false,true]);
  b.document.pointerLockElement=b.canvas;
  b.document.exitPointerLock=()=>{b.document.pointerLockElement=null;b.document.dispatchEvent(new Event('pointerlockchange'));};
  b.host.disable({preserveFocus:true});
  assert.equal(b.packets.flat().at(-1).kind,'capture');assert.equal(b.packets.flat().at(-1).captured,false);
  assert.deepEqual(focuses(),[true,false,true,false,true],'capture release with canvas focus is not a physical blur');
  b.document.hasFocus=()=>false;b.host.enable();
  assert.deepEqual(focuses(),[true,false,true,false,true,false],'resume detects loss even without a browser blur event');
});

test('browser packs distinct source key times before a delayed Worker can receive them',t=>{
  const browser=browserFixture(t),origin=performance.timeOrigin;
  browser.set(120);browser.key('keydown','KeyA',110);
  browser.set(140);browser.key('keyup','KeyA',130);
  assert.deepEqual(browser.packets.slice(-2).flat().map(event=>event.timestampMs),[origin+110,origin+130]);
});

test('one delayed Worker drain retains distinct literal event times in the native input wire',()=>{
  const origin=performance.timeOrigin;
  const f=mediaFixture({originMs:origin,nowNs:ms(170)});
  f.media.enqueue([{kind:'focus',focused:true,timestampMs:origin+100}]);
  f.media.enqueue([{kind:'key',code:30,down:true,timestampMs:origin+110}]);
  f.media.enqueue([{kind:'key',code:30,down:false,timestampMs:origin+130}]);
  f.media.update(ms(200));
  const batches=f.operations.filter(value=>value.operation==='submit_windows_input');
  assert.equal(batches.length,1);
  assert.deepEqual(batches[0].events.map(event=>[event.kind,event.a,event.b,event.time]),
    [[4,1,0,ms(100)],[1,30,1,ms(110)],[1,30,0,ms(130)]]);
  assert.deepEqual(batches[0].events.slice(1).map(event=>Number(event.time/1_000_000n)),[110,130]);
  assert.deepEqual(f.media.inputStatus().history.map(event=>event.timestampNs),[ms(100),ms(110),ms(130)].map(String));
});

test('pointer boundaries and full batches retain source order and recheck native policy time',()=>{
  const f=mediaFixture();
  f.media.enqueue([{kind:'key',code:30,down:true,timestampMs:1010},{kind:'pointer',x:5,y:6,timestampMs:1020},
    {kind:'key',code:30,down:false,timestampMs:1030}]);
  f.afterSubmit(ms(15));
  f.media.update(ms(200));
  assert.deepEqual(f.operations.filter(value=>value.operation!=='sync_windows_input').map(value=>
    value.operation==='submit_windows_input'?[value.operation,value.events.map(event=>event.time)]:
      value.operation==='submit_windows_pointer'?[value.operation,value.time,value.x,value.y]:[value.operation]),
  [['windows_input_time'],['submit_windows_input',[ms(10)]],['windows_input_time'],['submit_windows_pointer',ms(20),5,6],
    ['windows_input_time'],['submit_windows_input',[ms(30)]]]);
  f.operations.length=0;
  f.media.enqueue(Array.from({length:121},(_,i)=>({kind:'relative',dx:1,dy:0,timestampMs:1040+i})));
  f.media.update(ms(200));
  const batches=f.operations.filter(value=>value.operation==='submit_windows_input');
  assert.deepEqual(batches.map(value=>value.events.length),[120,1]);
  assert.deepEqual(batches.flatMap(value=>value.events.map(event=>event.time)),Array.from({length:121},(_,i)=>ms(40+i)));
});

test('native source floor clamps historical events without moving later events backwards',()=>{
  const f=mediaFixture();f.setFloor(ms(35));
  f.media.enqueue([{kind:'key',code:30,down:true,timestampMs:1010},{kind:'key',code:30,down:false,timestampMs:1030},
    {kind:'relative',dx:1,dy:0,timestampMs:1040}]);
  f.media.update(ms(200));
  assert.deepEqual(f.operations.find(value=>value.events).events.map(event=>event.time),[ms(35),ms(35),ms(40)]);
  f.operations.length=0;f.setFloor(ms(250));f.setObserved(ms(250));
  f.media.enqueue([{kind:'relative',dx:1,dy:0,timestampMs:1045}]);
  f.media.update(ms(200));
  assert.equal(f.operations.find(value=>value.events).events[0].time,ms(250));
});

test('missing, malformed, future and backwards source timestamps preserve monotone compatibility',()=>{
  const f=mediaFixture();
  f.media.enqueue([
    {kind:'relative',dx:1,dy:0,timestampMs:1050},
    {kind:'relative',dx:2,dy:0,timestampMs:1040},
    {kind:'relative',dx:3,dy:0,timestampMs:Infinity},
    {kind:'relative',dx:4,dy:0,timestampMs:999999},
    {kind:'relative',dx:5,dy:0},
    {kind:'relative',dx:6,dy:0,timestampMs:1n},
  ]);
  f.media.update(ms(200));
  assert.deepEqual(f.operations.find(value=>value.events).events.map(event=>event.time),[ms(50),ms(50),ms(200),ms(200),ms(200),ms(200)]);
  f.operations.length=0;f.media.update(ms(200));
  assert.equal(f.operations.some(value=>value.operation==='windows_input_time'),false);
});

test('pause offset is applied on enqueue while queued pre-pause events and new sessions retain their own time',()=>{
  const f=mediaFixture({originMs:1000,nowNs:ms(50)});
  f.media.enqueue([{kind:'key',code:30,down:true,timestampMs:1010},{kind:'key',code:30,down:false,timestampMs:1030}]);
  f.clock.originMs=2000;f.clock.nowNs=ms(60);
  f.media.enqueue([{kind:'relative',dx:1,dy:0,timestampMs:2055}]);
  f.media.update(ms(60));
  assert.deepEqual(f.operations.find(value=>value.events).events.map(event=>event.time),[ms(10),ms(30),ms(55)]);
  const next=mediaFixture({originMs:3000,nowNs:ms(5)});
  next.media.enqueue([{kind:'relative',dx:1,dy:0,timestampMs:3001}]);next.media.update(ms(5));
  const submitted=next.operations.find(value=>value.events);
  assert.equal(submitted.sequence,1n);assert.equal(submitted.events[0].time,ms(1));
});

test('session clock adapter excludes completed and open manual pauses without observing the native clock',t=>{
  let now=150; t.mock.method(performance,'now',()=>now);
  const session=Object.assign(Object.create(Session.prototype),{started:10,pausedDuration:20,paused:false,
    engine:{ok(){throw new Error('input clock mapping must not observe the native clock');}}});
  assert.deepEqual(session.inputClock(),{originMs:performance.timeOrigin+30,nowNs:ms(120)});
  session.paused=true;session.pauseStarted=100;now=500;
  assert.deepEqual(session.inputClock(),{originMs:performance.timeOrigin+30,nowNs:ms(70)});
});

test('input received before the runtime start point maps safely to guest time zero',t=>{
  t.mock.method(performance,'now',()=>50);
  const session=Object.assign(Object.create(Session.prototype),{paused:false});
  assert.deepEqual(session.inputClock(),{originMs:performance.timeOrigin+50,nowNs:0n});
  const f=mediaFixture(session.inputClock());
  f.media.enqueue([{kind:'focus',focused:true,timestampMs:performance.timeOrigin+40}]);
  f.media.update(ms(5));
  assert.equal(f.operations.find(value=>value.events).events[0].time,0n);
});

test('zero, legacy epoch and future browser timestamps fall back to dispatch time',t=>{
  const browser=browserFixture(t),origin=performance.timeOrigin;
  browser.set(120);browser.key('keydown','KeyA',0);
  browser.set(140);browser.key('keyup','KeyA',Date.now());
  browser.set(160);browser.key('keydown','KeyB',99999);
  assert.deepEqual(browser.packets.slice(-3).flat().map(event=>event.timestampMs),[origin+120,origin+140,origin+160]);
});
