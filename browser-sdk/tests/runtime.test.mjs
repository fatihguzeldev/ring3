import test from 'node:test';
import assert from 'node:assert/strict';
import { createBrowserRuntime } from '../runtime.mjs';

function fixture() {
  const saved = new Map(), handlers = new Set(), timers = new Map();
  let now = 0, timerId = 0, uuid = 0;
  const origin = 'https://runtime.test';
  class Element {
    isConnected = true; children = []; style = {}; removed = 0; calls = []; focuses = 0;
    contentWindow = { postMessage: (message, targetOrigin) => this.calls.push({ message, targetOrigin }), focus: () => this.focuses++ };
    append(child) { this.children.push(child); child.parent = this; }
    remove() { this.removed++; this.isConnected = false; if (this.parent) this.parent.children = this.parent.children.filter(c => c !== this); }
    focus() { this.focuses++; }
  }
  const container = new Element();
  const values = {
    HTMLElement: Element, isSecureContext: true, crossOriginIsolated: true,
    location: { href: `${origin}/sdk-host.html`, origin },
    crypto: { randomUUID: () => `12345678-1234-4123-8123-${String(++uuid).padStart(12, '0')}` },
    document: { createElement: tag => { assert.equal(tag, 'iframe'); return new Element(); } },
    window: { addEventListener: (name, fn) => { assert.equal(name, 'message'); handlers.add(fn); }, removeEventListener: (name, fn) => handlers.delete(fn) },
    setTimeout: (fn, ms) => { timers.set(++timerId, { fn, at: now + ms }); return timerId; },
    clearTimeout: id => timers.delete(id),
  };
  for (const [key, value] of Object.entries(values)) {
    saved.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  }
  const frame = () => container.children[0];
  const envelope = (target, payload) => {
    const u = new URL(target.src);
    return { namespace: 'ring3.runtime.v1', session: u.searchParams.get('ring3Session'), generation: Number(u.searchParams.get('ring3Generation')), ...payload };
  };
  const emit = (payload, options = {}) => {
    const target = options.frame ?? frame();
    const event = { origin: options.origin ?? origin, source: options.source ?? target.contentWindow, data: envelope(target, payload) };
    for (const fn of [...handlers]) fn(event);
  };
  let sequence = 0;
  const state = (phase, extra = {}) => emit({ type: 'state', sequence: ++sequence, state: { phase }, ...extra });
  const start = (options = {}) => createBrowserRuntime({ container, timeoutMs: 1000, launchTimeoutMs: 1000, ...options });
  const ready = async () => { const creating = start(); emit({ type: 'ready', sequence: ++sequence, state: { phase: 'ready' } }); return creating; };
  const reply = (ok = true, result = {}, error = undefined, target = frame()) => {
    const request = target.calls.at(-1).message;
    emit({ type: 'reply', id: request.id, ok, result, error }, { frame: target });
  };
  return {
    origin, container, frame, emit, state, start, ready, reply, handlers, timers,
    tick(ms) { now += ms; for (const [id, timer] of [...timers]) if (timer.at <= now) { timers.delete(id); timer.fn(); } },
    restore() { for (const [key, descriptor] of saved) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else delete globalThis[key]; } },
  };
}
const code = expected => error => error.code === expected;
const flush = async () => { await Promise.resolve(); await Promise.resolve(); };

test('parent facade protocol and lifetime', { concurrency: false }, async t => {
  await t.test('requires an isolated, connected same-origin host', async () => {
    const f = fixture();
    try {
      await assert.rejects(f.start({ runtimeUrl: 'https://other.test/' }), code('ORIGIN_MISMATCH'));
      globalThis.crossOriginIsolated = false;
      await assert.rejects(f.start(), code('ISOLATION_REQUIRED'));
      assert.equal(f.container.children.length, 0);
    } finally { f.restore(); }
  });
  await t.test('forged source, origin and session cannot satisfy ready', async () => {
    const f = fixture();
    try {
      const creating = f.start(), child = f.frame();
      f.emit({ type: 'ready', sequence: 1, state: { phase: 'ready' } }, { origin: 'https://other.test' });
      f.emit({ type: 'ready', sequence: 1, state: { phase: 'ready' } }, { source: {} });
      f.emit({ type: 'ready', session: 'other', sequence: 1, state: { phase: 'ready' } });
      f.emit({ type: 'ready', generation: 2, sequence: 1, state: { phase: 'ready' } });
      assert.equal(child.removed, 0);
      f.emit({ type: 'ready', sequence: 1, state: { phase: 'ready' } });
      const runtime = await creating; assert.equal(runtime.state.connected, true);
      const url = new URL(child.src); assert.equal(url.searchParams.get('game'), 'dev'); assert.equal(url.searchParams.get('ring3Sdk'), '1');
      runtime.focus(); assert.equal(child.focuses, 2); runtime.dispose();
    } finally { f.restore(); }
  });
  await t.test('nonpositive, wrong-phase and stale initial ready reject and remove', async () => {
    for (const kind of ['zero', 'error', 'stale']) {
      const f = fixture();
      try {
        const creating = f.start(), child = f.frame();
        if (kind === 'stale') f.emit({ type: 'state', sequence: 2, state: { phase: 'ready' } });
        f.emit({ type: 'ready', sequence: kind === 'zero' ? 0 : 1, state: { phase: kind === 'error' ? 'error' : 'ready' } });
        await assert.rejects(creating, error => ['BAD_READY', 'BAD_STATE'].includes(error.code));
        assert.equal(child.removed, 1); assert.equal(f.handlers.size, 0); assert.equal(f.timers.size, 0);
      } finally { f.restore(); }
    }
  });
  await t.test('launch acceptance waits for first presentation and operations serialize', async () => {
    const f = fixture();
    try {
      const runtime = await f.ready(); let launched = false;
      const launch = runtime.launchOwnedWgb('/owned.wgb').then(value => { launched = true; return value; });
      assert.equal(f.frame().calls.at(-1).message.method, 'launch');
      f.reply(); await flush(); assert.equal(launched, false);
      await assert.rejects(runtime.pause(), code('BUSY'));
      f.state('running'); await launch; assert.equal(launched, true);
      const paused = runtime.pause(); await assert.rejects(runtime.resume(), code('BUSY'));
      f.state('paused'); f.reply(); assert.equal((await paused).phase, 'paused');
      const resumed = runtime.resume(); f.state('running'); f.reply(); assert.equal((await resumed).phase, 'running');
      await assert.rejects(runtime.launchOwnedWgb('/another.wgb'), code('ALREADY_LAUNCHED'));
      runtime.dispose();
    } finally { f.restore(); }
  });
  await t.test('stale state and reply IDs cannot complete or roll back current work', async () => {
    const f = fixture();
    try {
      const runtime = await f.ready(); f.state('running');
      f.emit({ type: 'state', sequence: 1, state: { phase: 'ready' } }); assert.equal(runtime.state.phase, 'running');
      let done = false; const status = runtime.status().then(value => { done = true; return value; });
      const id = f.frame().calls.at(-1).message.id;
      f.emit({ type: 'reply', id: id + 1, ok: true, result: 'wrong' }); await flush(); assert.equal(done, false);
      f.reply(true, { phase: 'running' }); assert.deepEqual(await status, { phase: 'running' });
      f.emit({ type: 'reply', id, ok: true, result: 'late' }); assert.equal(runtime.state.phase, 'running'); runtime.dispose();
    } finally { f.restore(); }
  });
  await t.test('fresh re-exec ready may report loading without repeating launch', async () => {
    const f = fixture();
    try {
      const runtime = await f.ready(), child = f.frame();
      const launch = runtime.launchOwnedWgb('/owned.wgb'); f.reply(); await flush();
      f.emit({ type: 'ready', sequence: 2, state: { phase: 'loading', restarting: true } });
      assert.equal(child.removed, 0); assert.equal(runtime.state.phase, 'loading');
      f.emit({ type: 'state', sequence: 3, state: { phase: 'running', firstPresent: true } });
      await launch; assert.equal(child.calls.filter(c => c.message.method === 'launch').length, 1);
      runtime.dispose();
    } finally { f.restore(); }
  });
  await t.test('failed graceful stop retains frame for retry; success is terminal', async () => {
    const f = fixture();
    try {
      const runtime = await f.ready(), child = f.frame(), first = runtime.stop();
      f.reply(false, undefined, { code: 'DRAIN_FAILED', message: 'retained for retry' });
      await assert.rejects(first, code('DRAIN_FAILED')); assert.equal(child.removed, 0); assert.equal(f.handlers.size, 1);
      const retry = runtime.stop(); f.reply(); assert.deepEqual(await retry, { graceful: true });
      assert.equal(child.removed, 1); assert.equal(f.handlers.size, 0);
      assert.deepEqual(await runtime.stop(), { graceful: true, alreadyClosed: true });
      f.emit({ type: 'state', sequence: 100, state: { phase: 'running' } }, { frame: child });
      assert.equal(runtime.state.phase, 'stopped'); await assert.rejects(runtime.resume(), code('SESSION_CLOSED'));
    } finally { f.restore(); }
  });
  await t.test('stop timeout retains ownership; explicit disposal settles pending once', async () => {
    const f = fixture();
    try {
      const runtime = await f.ready(), child = f.frame(), stop = runtime.stop();
      f.tick(1000); await assert.rejects(stop, code('COMMAND_TIMEOUT')); assert.equal(child.removed, 0);
      const pending = runtime.status(); runtime.dispose(); await assert.rejects(pending, code('SESSION_CLOSED'));
      runtime.dispose(); assert.equal(child.removed, 1); assert.equal(f.handlers.size, 0); assert.equal(f.timers.size, 0);
      await assert.rejects(runtime.stop(), code('SESSION_CLOSED'));
    } finally { f.restore(); }
  });
  await t.test('stop interrupts launch wait without claiming launch or discarding a failed drain', async () => {
    const f = fixture();
    try {
      const runtime = await f.ready(), launch = runtime.launchOwnedWgb('/owned.wgb');
      f.reply(); await flush(); const stop = runtime.stop();
      await assert.rejects(launch, code('STOPPING'));
      f.reply(false, undefined, { code: 'DRAIN_FAILED', message: 'retry' }); await assert.rejects(stop, code('DRAIN_FAILED'));
      assert.equal(f.frame().removed, 0); runtime.dispose();
    } finally { f.restore(); }
  });
  await t.test('malformed active state rejects pending work but preserves frame for recovery', async () => {
    const f = fixture();
    try {
      const runtime = await f.ready(), child = f.frame(), pending = runtime.status();
      f.emit({ type: 'state', sequence: 2, state: { phase: 'unknown' } });
      await assert.rejects(pending, code('BAD_STATE')); assert.equal(child.removed, 0); assert.equal(runtime.state.phase, 'error');
      runtime.dispose();
    } finally { f.restore(); }
  });
  await t.test('ready timeout releases the unlaunched frame and listener', async () => {
    const f = fixture();
    try {
      const creating = f.start(), child = f.frame(); f.tick(1000);
      await assert.rejects(creating, code('READY_TIMEOUT')); assert.equal(child.removed, 1); assert.equal(f.handlers.size, 0);
    } finally { f.restore(); }
  });
});
