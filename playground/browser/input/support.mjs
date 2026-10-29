import {InputHost} from '../input.js';

export function fixture(t) {
  const previous = new Map();
  const install = (name, value) => {
    previous.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, {value, configurable:true, writable:true});
  };
  const window = new EventTarget(), document = new EventTarget();
  const frames = new Map();
  let nextFrame = 0;
  document.hidden = false;
  document.focused = true;
  document.hasFocus = () => document.focused;
  document.pointerLockElement = null;
  const canvas = new EventTarget();
  Object.assign(canvas, {width:640,height:480,clientHeight:480,style:{}});
  const event = (target, type, values = {}) => {
    const input = new Event(type, {cancelable:true});
    for (const [name,value] of Object.entries(values)) Object.defineProperty(input, name, {value});
    target.dispatchEvent(input);
    return input;
  };
  canvas.focus = () => {
    if (document.activeElement === canvas) return;
    const old = document.activeElement;
    document.activeElement = canvas;
    if (old) event(old, 'blur');
    event(canvas, 'focus');
  };
  canvas.getBoundingClientRect = () => ({left:0,top:0,width:640,height:480});
  canvas.requestPointerLock = async () => {
    document.pointerLockElement = canvas;
    event(document, 'pointerlockchange');
  };
  document.exitPointerLock = () => {
    if (!document.pointerLockElement) return;
    document.pointerLockElement = null;
    event(document, 'pointerlockchange');
  };
  install('window', window);
  install('document', document);
  install('requestAnimationFrame', callback => {frames.set(++nextFrame, callback);return nextFrame;});
  install('cancelAnimationFrame', frame => frames.delete(frame));
  const events = [], captures = [];
  const host = new InputHost(canvas, {send:batch => events.push(...batch),onCapture:value => captures.push(value)});
  t.after(() => {
    host.close();
    for (const [name, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  });
  return {host, canvas, window, document, events, captures, event,
    blur() {document.focused=false;event(window,'blur');},
    focus() {document.focused=true;event(window,'focus');},
    frame() {const callbacks=[...frames.values()];frames.clear();for (const callback of callbacks) callback();},
  };
}

export function unfocusedEvents(events) {
  let focused = false;
  return events.filter(event => {
    if (event.kind === 'focus') focused = event.focused;
    return !focused && (event.kind === 'relative' ||
      (['key','button'].includes(event.kind) && event.down) ||
      (event.kind === 'capture' && event.captured));
  });
}
