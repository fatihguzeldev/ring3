const NAMESPACE = 'ring3.runtime.v1';
const PHASES = new Set(['ready', 'loading', 'running', 'paused', 'stopped', 'error']);
const failureMessage = state => typeof state.error === 'string' ? state.error : state.error?.message ?? 'runtime failed';

export class RuntimeError extends Error {
  constructor(code, message) { super(message); this.name = 'RuntimeError'; this.code = code; }
}

// private first facade: one same-origin iframe and one accepted bundle per session.
export async function createBrowserRuntime({ container, runtimeUrl = '/', timeoutMs = 30000, launchTimeoutMs = 300000 } = {}) {
  if (!(container instanceof HTMLElement) || !container.isConnected) throw new TypeError('container must be an HTMLElement');
  if (!globalThis.isSecureContext || !globalThis.crossOriginIsolated || typeof SharedArrayBuffer !== 'function') {
    throw new RuntimeError('ISOLATION_REQUIRED', 'the host and runtime require a secure, cross-origin isolated origin');
  }
  for (const n of [timeoutMs, launchTimeoutMs]) {
    if (!Number.isInteger(n) || n < 1000 || n > 600000) throw new TypeError('timeout bounds are 1000..600000ms');
  }
  const url = new URL(runtimeUrl, location.href);
  if (url.origin !== location.origin || url.username || url.password) {
    throw new RuntimeError('ORIGIN_MISMATCH', 'runtimeUrl must use the host origin');
  }
  const session = crypto.randomUUID(), generation = 1, origin = url.origin;
  url.searchParams.set('game', 'dev'); url.searchParams.set('ring3Sdk', '1');
  url.searchParams.set('ring3Session', session); url.searchParams.set('ring3Generation', String(generation));
  url.hash = '';
  const frame = document.createElement('iframe');
  frame.title = 'Windows runtime'; frame.tabIndex = 0;
  frame.allow = 'autoplay; fullscreen; gamepad; keyboard-map'; frame.allowFullscreen = true;
  Object.assign(frame.style, { width: '100%', height: '100%', border: '0', display: 'block' });
  let disposed = false, stopped = false, stopping = false, launched = false, readySeen = false, exclusive = null;
  let removed = false;
  let nextId = 1, sequence = 0, snapshot = Object.freeze({ phase: 'ready', connected: false });
  const pending = new Map(), waiters = new Set(), listeners = new Set();
  let resolveReady, rejectReady;
  const ready = new Promise((yes, no) => { resolveReady = yes; rejectReady = no; });
  const readyTimer = setTimeout(() => rejectReady(new RuntimeError('READY_TIMEOUT', 'runtime bridge did not become ready')), timeoutMs);

  const notify = () => {
    for (const listener of listeners) {
      try { listener(snapshot); } catch (error) { console.error('runtime state listener failed', error); }
    }
    for (const waiter of [...waiters]) {
      if (snapshot.phase === 'error') waiter.reject(new RuntimeError('RUNTIME_ERROR', failureMessage(snapshot)));
      else if (waiter.predicate(snapshot)) waiter.resolve(snapshot);
    }
  };
  function updateState(message) {
    if (!Number.isSafeInteger(message.sequence) || message.sequence <= 0) throw new RuntimeError('BAD_STATE', 'invalid runtime state sequence');
    if (message.sequence <= sequence) return false;
    if (!message.state || !PHASES.has(message.state.phase)) throw new RuntimeError('BAD_STATE', 'invalid runtime state');
    sequence = message.sequence;
    snapshot = Object.freeze({ ...message.state, sequence, connected: true }); notify(); return true;
  }
  function receive(event) {
    if (disposed || stopped || event.origin !== origin || event.source !== frame.contentWindow) return;
    const m = event.data;
    if (!m || m.namespace !== NAMESPACE || m.session !== session || m.generation !== generation) return;
    try {
      if (m.type === 'ready') {
        if (!readySeen && m.state?.phase !== 'ready') throw new RuntimeError('BAD_READY', 'runtime ready must report ready phase');
        const fresh = updateState(m);
        if (!readySeen) {
          if (!fresh) throw new RuntimeError('BAD_READY', 'runtime ready must have a fresh sequence');
          readySeen = true; clearTimeout(readyTimer); resolveReady();
        }
      } else if (m.type === 'state') updateState(m);
      else if (m.type === 'reply' && Number.isSafeInteger(m.id) && pending.has(m.id)) {
        const call = pending.get(m.id); pending.delete(m.id); clearTimeout(call.timer);
        if (m.ok === true) call.resolve(m.result);
        else call.reject(new RuntimeError(typeof m.error?.code === 'string' ? m.error.code : 'COMMAND_FAILED',
          typeof m.error?.message === 'string' ? m.error.message : 'runtime command failed'));
      }
    } catch (error) {
      rejectReady(error); rejectPending(error);
      snapshot = Object.freeze({ phase: 'error', connected: readySeen, sequence, error: { code: error.code ?? 'PROTOCOL_ERROR', message: error.message } });
      notify();
    }
  }
  function rejectPending(error) {
    for (const call of pending.values()) { clearTimeout(call.timer); call.reject(error); }
    pending.clear();
    for (const waiter of [...waiters]) waiter.reject(error);
  }
  function removeFrame(phase) {
    if (removed) return;
    removed = true;
    clearTimeout(readyTimer);
    rejectReady(new RuntimeError('SESSION_CLOSED', 'runtime session closed'));
    rejectPending(new RuntimeError('SESSION_CLOSED', 'runtime session closed'));
    window.removeEventListener('message', receive); frame.remove();
    snapshot = Object.freeze({ phase, connected: false, sequence }); notify(); listeners.clear();
  }
  function request(method, params, bound = timeoutMs) {
    if (disposed || stopped || !readySeen) return Promise.reject(new RuntimeError('SESSION_CLOSED', 'runtime session is unavailable'));
    if (pending.size >= 16 || !Number.isSafeInteger(nextId)) return Promise.reject(new RuntimeError('BUSY', 'too many pending commands'));
    const id = nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new RuntimeError('COMMAND_TIMEOUT', `${method} did not acknowledge`)); }, bound);
      pending.set(id, { resolve, reject, timer });
      try { frame.contentWindow.postMessage({ namespace: NAMESPACE, type: 'request', session, generation, id, method, params }, origin); }
      catch (error) { clearTimeout(timer); pending.delete(id); reject(error); }
    });
  }
  function waitFor(predicate, bound) {
    if (predicate(snapshot)) return Promise.resolve(snapshot);
    if (snapshot.phase === 'error') return Promise.reject(new RuntimeError('RUNTIME_ERROR', failureMessage(snapshot)));
    return new Promise((resolve, reject) => {
      const waiter = {
        predicate,
        resolve(value) { clearTimeout(timer); waiters.delete(waiter); resolve(value); },
        reject(error) { clearTimeout(timer); waiters.delete(waiter); reject(error); },
      };
      const timer = setTimeout(() => waiter.reject(new RuntimeError('STATE_TIMEOUT', 'runtime state transition timed out')), bound);
      waiters.add(waiter);
    });
  }
  async function operation(name, action) {
    if (disposed || stopped || stopping) throw new RuntimeError('SESSION_CLOSED', 'runtime session is unavailable');
    if (exclusive) throw new RuntimeError('BUSY', `${exclusive} is in progress`);
    exclusive = name;
    try { return await action(); } finally { exclusive = null; }
  }
  window.addEventListener('message', receive);
  frame.src = url.href; container.append(frame);
  try { await ready; } catch (error) { disposed = true; removeFrame('error'); throw error; }

  return Object.freeze({
    get state() { return snapshot; },
    subscribe(listener) {
      if (typeof listener !== 'function') throw new TypeError('listener must be a function');
      if (disposed || stopped) throw new RuntimeError('SESSION_CLOSED', 'runtime session is unavailable');
      listeners.add(listener); return () => listeners.delete(listener);
    },
    launchOwnedWgb(bundleUrl, options = {}) {
      return operation('launch', async () => {
        if (launched) throw new RuntimeError('ALREADY_LAUNCHED', 'create a fresh session to launch another bundle');
        if (typeof bundleUrl !== 'string' || /[\x00-\x1f]/.test(bundleUrl)) throw new TypeError('bundleUrl must be a URL without control characters');
        const bundle = new URL(bundleUrl, location.href);
        if (bundle.origin !== origin || bundle.username || bundle.password || bundle.hash || !bundle.href.toLowerCase().endsWith('.wgb')) {
          throw new RuntimeError('INVALID_BUNDLE_URL', 'supply an owned same-origin WGB URL');
        }
        if (options.args !== undefined && (typeof options.args !== 'string' || options.args.length > 4096)) throw new TypeError('args must be a bounded string');
        if (options.preload !== undefined && typeof options.preload !== 'boolean') throw new TypeError('preload must be boolean');
        launched = true;
        await request('launch', { url: bundle.href, args: options.args, preload: options.preload === true });
        // loadApp's returned promise only means submitted; the child reports actual first_present.
        return waitFor(s => s.phase === 'running', launchTimeoutMs);
      });
    },
    pause() { return operation('pause', async () => { await request('pause'); return waitFor(s => s.phase === 'paused', timeoutMs); }); },
    resume() { return operation('resume', async () => { await request('resume'); return waitFor(s => s.phase === 'running', timeoutMs); }); },
    status() { return request('status'); },
    focus() {
      if (disposed || stopped || stopping) throw new RuntimeError('SESSION_CLOSED', 'runtime session is unavailable');
      frame.focus(); frame.contentWindow.focus();
    },
    async stop() {
      if (disposed) throw new RuntimeError('SESSION_CLOSED', 'runtime session is unavailable');
      if (stopped) return { graceful: true, alreadyClosed: true };
      if (stopping) throw new RuntimeError('BUSY', 'stop is already in progress');
      stopping = true;
      for (const waiter of [...waiters]) waiter.reject(new RuntimeError('STOPPING', 'runtime is stopping'));
      try {
        await request('stop', undefined, timeoutMs);
        stopped = true; removeFrame('stopped');
        return { graceful: true };
      } finally { stopping = false; }
    },
    dispose() {
      if (disposed) return;
      disposed = true; removeFrame('stopped');
    },
  });
}
