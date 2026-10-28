const installations = new WeakSet();

export function activeClock(session) {
  return (session.paused ? session.pauseStarted : performance.now())
    - session.started - (session.pausedDuration ?? 0);
}

export function instrumentSession(session, measurements) {
  if (!session.paused || session.code.open || installations.has(session))
    throw new Error('session phase measurement requires a paused, uninstrumented session');
  const restorations = [], awaits = new Map(), pending = new Set(), calls = new Map();
  let restored = false;
  const inside = prefix => measurements.exclusiveStack.some(frame => frame.name.startsWith(prefix));
  function patch(object, key, build) {
    const descriptor = Object.getOwnPropertyDescriptor(object, key), original = object[key];
    if (typeof original !== 'function') throw new Error(`missing session measurement method ${key}`);
    const wrapper = build(original);
    Object.defineProperty(object, key, {configurable: true, writable: true, value: wrapper});
    restorations.push({
      verify() {
        if (object[key] !== wrapper) throw new Error(`session measurement method replaced: ${key}`);
      },
      restore() {
        if (descriptor) Object.defineProperty(object, key, descriptor);
        else delete object[key];
      },
    });
  }
  const sync = (object, key, name) => patch(object, key, original => function (...args) {
    return session.paused ? Reflect.apply(original, this, args)
      : measurements.runExclusive(name, () => Reflect.apply(original, this, args));
  });
  function awaiting(key, name) {
    patch(session, key, original => async function (...args) {
      if (session.paused) return Reflect.apply(original, this, args);
      if (pending.size) throw new Error('overlapping session await envelopes');
      const frame = {}, start = activeClock(session), syncStart = measurements.exclusiveRootMilliseconds;
      pending.add(frame);
      try {
        return await measurements.runExclusive(`${name}.request`, () => Reflect.apply(original, this, args));
      } finally {
        const elapsed = activeClock(session) - start;
        const synchronous = measurements.exclusiveRootMilliseconds - syncStart;
        pending.delete(frame);
        const self = elapsed - synchronous;
        if (self < -0.001) throw new Error('session await accounting overlaps synchronous work');
        const row = awaits.get(name) ?? {milliseconds: 0, maximumMs: 0, count: 0};
        row.milliseconds += Math.max(0, self);
        row.maximumMs = Math.max(row.maximumMs, self);
        row.count++;
        awaits.set(name, row);
      }
    });
  }
  try {
    patch(session, 'complete', original => function (receipt, ...args) {
      if (session.paused) return Reflect.apply(original, this, [receipt, ...args]);
      const group = receipt.id >>> 16 === 4 ? 'windows.graphics' : 'windows.other';
      const id = receipt.id >>> 0, row = calls.get(id) ?? {id, count: 0, milliseconds: 0};
      const before = measurements.phases.get(group)?.milliseconds ?? 0;
      try {return measurements.runExclusive(group, () => Reflect.apply(original, this, [receipt, ...args]));}
      finally {
        row.count++;
        row.milliseconds += (measurements.phases.get(group)?.milliseconds ?? before) - before;
        calls.set(id, row);
      }
    });
    patch(session.engine, 'status', original => function (operation, ...args) {
      if (session.paused || operation.startsWith('component_cpu_') || inside('windows.') || inside('graphics.'))
        return Reflect.apply(original, this, [operation, ...args]);
      return measurements.runExclusive('host.native', () => Reflect.apply(original, this, [operation, ...args]));
    });
    awaiting('providers', 'providers.await');
    awaiting('wait', 'scheduler.await');
    const transport = session.media?.graphicsTransport;
    if (transport) {
      sync(transport, 'output', 'graphics.transport');
      sync(transport, 'complete', 'graphics.acknowledge');
    }
    if (session.media?.consumedAudio) sync(session.media, 'consumedAudio', 'audio.acknowledge');
    if (globalThis.constructor.name === 'DedicatedWorkerGlobalScope' && typeof globalThis.onmessage === 'function') {
      patch(globalThis, 'onmessage', original => function (...args) {
        const type = args[0]?.data?.type;
        return session.paused || type === 'pause' || type === 'resume'
          ? Reflect.apply(original, this, args)
          : measurements.runExclusive('worker.events', () => Reflect.apply(original, this, args));
      });
    }
    installations.add(session);
  } catch (error) {
    for (const item of [...restorations].reverse()) item.restore();
    throw error;
  }
  return {
    snapshot() {
      return {
        semantics: 'exclusive synchronous worker elapsed plus active await remainder; await remainder can contain unmeasured continuation JavaScript; GPU work can overlap',
        activeSyncScopes: measurements.exclusiveStack.length,
        activeAwaitScopes: pending.size,
        syncRootNs: Math.round(measurements.exclusiveRootMilliseconds * 1e6),
        sync: measurements.snapshot(),
        awaits: Object.fromEntries([...awaits].map(([name, row]) => [name, {
          selfElapsedNs: Math.round(row.milliseconds * 1e6),
          maximumNs: Math.round(row.maximumMs * 1e6), count: row.count,
        }])),
        calls: [...calls.values()].map(row => ({id: row.id, count: row.count,
          elapsedNs: Math.round(row.milliseconds * 1e6)})).sort((a,b) => b.elapsedNs - a.elapsedNs),
      };
    },
    restore() {
      if (restored) return;
      if (!session.paused || measurements.exclusiveStack.length || pending.size)
        throw new Error('cannot restore an active session phase measurement');
      for (const item of restorations) item.verify();
      for (const item of [...restorations].reverse()) item.restore();
      installations.delete(session);
      restored = true;
    },
  };
}
