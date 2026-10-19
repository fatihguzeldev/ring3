// browser-only acceptance driver; visual observations must be supplied by the operator.
export async function createVerticalAcceptance({ createRuntime, container, runtimeUrl, bundleUrl, options = {} }) {
  const assert = (ok, label) => { if (!ok) throw new Error(`acceptance: ${label}`); };
  assert(typeof createRuntime === 'function', 'pass the actual facade factory');
  assert(container?.querySelectorAll('iframe').length === 0, 'use an empty dedicated container');
  const events = [], observations = [];
  const record = (type, detail) => events.push({ index: events.length, at_ms: performance.now(), type, detail: structuredClone(detail) });
  const runtime = await createRuntime({ container, runtimeUrl, ...options });
  record('ready', runtime.state);
  assert(runtime.state.connected === true && runtime.state.phase === 'ready', 'actual bridge ready');
  assert(container.querySelectorAll('iframe').length === 1, 'one frame at readiness');
  const firstFrame = container.querySelector('iframe');
  const unsubscribe = runtime.subscribe(state => record('state', state));
  let closed = false;
  const action = async (name, call, expectedPhase) => {
    record(`${name}-requested`, runtime.state);
    try {
      const result = await call();
      record(`${name}-acknowledged`, { state: runtime.state, result });
      assert(runtime.state.phase === expectedPhase, `${name} phase=${expectedPhase}`);
      if (expectedPhase !== 'stopped') assert(container.querySelector('iframe') === firstFrame, 'same live iframe');
      return result;
    } catch (error) {
      record(`${name}-rejected`, { code: error.code ?? null, message: error.message, state: runtime.state });
      throw error;
    }
  };
  return Object.freeze({
    get runtime() { return runtime; },
    async launch() { return action('launch', () => runtime.launchOwnedWgb(bundleUrl), 'running'); },
    async pause() { return action('pause', () => runtime.pause(), 'paused'); },
    async resume() { return action('resume', () => runtime.resume(), 'running'); },
    async status() { const result = await runtime.status(); record('status', result); return result; },
    focus() { runtime.focus(); record('focused', runtime.state); },
    observe(stage, detail) {
      assert(typeof stage === 'string' && stage.length > 0 && detail && typeof detail === 'object', 'explicit operator observation');
      observations.push({ stage, at_ms: performance.now(), attribution: 'operator-supplied; not inferred by driver', detail });
    },
    async stop() {
      const result = await action('stop', () => runtime.stop(), 'stopped');
      assert(container.querySelectorAll('iframe').length === 0, 'successful stop removed frame');
      closed = true;
      return result;
    },
    async assertClosed() {
      assert(closed && runtime.state.phase === 'stopped', 'session closed');
      for (const method of ['pause', 'resume', 'status']) {
        let rejected = false;
        try { await runtime[method](); } catch (error) { rejected = true; record('closed-call-rejected', { method, code: error.code, message: error.message }); }
        assert(rejected, `closed ${method} rejects`);
      }
    },
    dispose() {
      runtime.dispose(); runtime.dispose();
      assert(container.querySelectorAll('iframe').length === 0, 'dispose removed frame');
      assert(runtime.state.phase === 'stopped', 'dispose terminal state');
      closed = true; unsubscribe(); record('disposed', runtime.state);
    },
    evidence() { return { schema: 1, driver: 'vertical-acceptance-v1', runtimeUrl, bundleUrl, events: structuredClone(events), observations: structuredClone(observations) }; },
  });
}
