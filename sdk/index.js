import {Session as BrowserSession} from './browser/session.js';
export {selectedFolder} from './browser/folder.js';

export function createSession({container, engineUrl = '/engine.wasm', onStatus = () => {}, onDiagnostic = () => {},
  onAudioStats = () => {}, onCapture = () => {}, onObservation = () => {}}) {
  if (!container?.ownerDocument || typeof container.append !== 'function') throw new TypeError('a session container is required');
  const document = container.ownerDocument;
  const host = document.createElement('section'), title = document.createElement('div'), display = document.createElement('div');
  title.textContent = 'game display'; display.style.cssText = 'position:relative;max-width:100%;width:640px;aspect-ratio:4/3;background:#000';
  const canvas = document.createElement('canvas'), gdiCanvas = document.createElement('canvas');
  for (const surface of [canvas,gdiCanvas]) {
    surface.width = 640; surface.height = 480; surface.tabIndex = 0;
    surface.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;object-fit:contain';
    surface.setAttribute('aria-label','game display');
  }
  gdiCanvas.hidden = true; gdiCanvas.tabIndex = -1; gdiCanvas.style.pointerEvents = 'none'; display.append(canvas,gdiCanvas);
  const dialog = document.createElement('dialog');
  for (const name of ['caption','text','buttons']) {
    const element = document.createElement('div'); element.setAttribute(`data-dialog-${name}`,''); dialog.append(element);
  }
  host.append(title,display,dialog); container.append(host);
  let state = Object.freeze({state:'idle',backend:'core'}), selectedBackend = 'core',
    disposed = false, pending = Promise.resolve(), stopping = null, generation = 0;
  const waiters = new Set();
  const runtime = new BrowserSession({canvas,gdiCanvas,title,dialog,onDiagnostic,onAudioStats,onCapture,onObservation,
    onStatus(value) {
      state = Object.freeze({...value,backend:value.backend ?? value.stats?.backend ?? selectedBackend});
      for (const item of [...waiters]) {
        if (value.state === 'error') item.reject(new Error(value.message));
        else if (item.states.includes(value.state)) item.resolve(state);
      }
      onStatus(state);
    },
  });
  function operation(run) {
    if (disposed) return Promise.reject(new Error('session is disposed'));
    const identity = generation, barrier = stopping;
    const task = pending.then(async () => {
      if (barrier) await barrier;
      if (disposed) throw new Error('session is disposed');
      if (identity !== generation) throw new Error('session stopped');
      return run();
    });
    pending = task.catch(() => {}); return task;
  }
  function changed(states, milliseconds) {
    let item;
    const promise = new Promise((resolve,reject) => {
      const timer = setTimeout(() => item.reject(new Error('runtime state acknowledgement timed out')),milliseconds);
      item = {states, resolve(value) {clearTimeout(timer); waiters.delete(item); resolve(value);},
        reject(error) {clearTimeout(timer); waiters.delete(item); reject(error);}};
      waiters.add(item);
    });
    void promise.catch(() => {});
    return {promise, cancel: error => item.reject(error)};
  }
  return Object.freeze({
    launch(options) {return operation(async () => {
      if (!['idle','stopped'].includes(state.state)) throw new Error('stop the current session before launching another');
      const backend = options?.backend ?? 'core';
      if (!['core','independent','v86'].includes(backend)) throw new TypeError('unknown CPU backend');
      selectedBackend = backend;
      const ready = changed(['running','exited'],120000);
      try {await runtime.start({...options,engineUrl:options?.engineUrl ?? engineUrl}); return await ready.promise;}
      catch (error) {ready.cancel(error); await ready.promise.catch(() => {}); throw error;}
    });},
    pause() {return operation(async () => {
      if (state.state === 'paused') return state;
      if (state.state !== 'running') throw new Error('session is not running');
      const paused = changed(['paused'],10000);
      try {await runtime.pause(); return await paused.promise;}
      catch (error) {paused.cancel(error); await paused.promise.catch(() => {}); throw error;}
    });},
    resume() {return operation(async () => {
      if (state.state === 'running') return state;
      if (state.state !== 'paused') throw new Error('session is not paused');
      const resumed = changed(['running'],10000);
      try {await runtime.resume(); return await resumed.promise;}
      catch (error) {resumed.cancel(error); await resumed.promise.catch(() => {}); throw error;}
    });},
    stop() {
      if (disposed) return Promise.reject(new Error('session is disposed'));
      if (stopping) return stopping;
      generation++;
      for (const item of [...waiters]) item.reject(new Error('session stopped'));
      const previous = pending;
      stopping = Promise.all([Promise.resolve().then(() => runtime.stop()),previous])
        .then(() => state).finally(() => {stopping = null;});
      return stopping;
    },
    status() {if (disposed) throw new Error('session is disposed'); return state;},
    focus() {if (disposed) throw new Error('session is disposed'); canvas.focus(); runtime.input.focus();},
    captureMouse() {if (disposed) return Promise.reject(new Error('session is disposed')); return runtime.input.capture();},
    setVolume(value,muted = false) {if (disposed) throw new Error('session is disposed'); runtime.audio.setVolume(value,muted);},
    armFrameProbe() {if (disposed) throw new Error('session is disposed'); runtime.armFrameProbe();},
    async dispose() {
      if (disposed) return;
      disposed = true;
      for (const item of [...waiters]) item.reject(new Error('session disposed'));
      try {await runtime.dispose();} finally {host.remove();}
    },
  });
}
