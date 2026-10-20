import assert from 'node:assert/strict';
import test from 'node:test';
import {Session} from '../browser/session.js';

function fixture({workerPending = true, audioError} = {}) {
  const events = [];
  let release;
  const graphicsQueue = new Promise(resolve => {release = resolve;});
  const capture = {
    workerPending,
    capture: {markIncomplete(message) {events.push(['incomplete', message]);}},
    async finish(outcome, error) {
      events.push(['capture', outcome, error?.message]);
      assert.equal(this.workerPending, workerPending, 'dispose must not fabricate a Worker result');
      return {complete: !workerPending};
    },
  };
  const session = Object.assign(Object.create(Session.prototype), {
    generation: 0,
    worker: {terminate() {events.push(['terminate']);}},
    capture,
    graphicsQueue,
    input: {close() {events.push(['input']);}},
    dialogs: {close() {events.push(['dialogs']);}},
    audio: {async close() {events.push(['audio']); if (audioError) throw audioError;}},
    graphics: {close() {events.push(['graphics']);}},
    windows: {reset() {events.push(['windows']);}},
    gdi: {clear() {events.push(['gdi']);}},
    status(state) {events.push(['status', state]);},
    onDiagnostic(message) {events.push(['diagnostic', message]);},
  });
  return {session, events, release};
}

test('forced dispose finalizes an incomplete capture after queued graphics settles', async () => {
  const {session, events, release} = fixture();
  const disposing = session.dispose();
  try {
    await Promise.resolve();
    assert.equal(events.filter(([kind]) => kind === 'terminate').length, 1);
    assert.equal(events.filter(([kind]) => kind === 'incomplete').length, 1);
    assert.equal(events.some(([kind]) => kind === 'capture'), false);
  } finally {
    release();
    await disposing;
  }
  assert.deepEqual(events.filter(([kind]) => kind === 'capture').map(([, outcome]) => outcome), ['cancelled']);
  assert.equal(events.filter(([kind]) => kind === 'graphics').length, 1);
  assert.deepEqual(events.at(-1), ['capture', 'cancelled', undefined]);
  assert.equal(session.worker, null);
});

test('dispose does not downgrade an already completed Worker capture', async () => {
  const {session, events, release} = fixture({workerPending: false});
  release();
  await session.dispose();
  assert.equal(events.some(([kind]) => kind === 'incomplete'), false);
  assert.equal(events.filter(([kind]) => kind === 'capture').length, 1);
});

test('capture finalization does not turn a cleanup failure into success', async () => {
  const error = new Error('resource cleanup failed');
  const {session, events, release} = fixture({audioError: error});
  release();
  await assert.rejects(session.dispose(), failure => failure === error);
  assert.equal(events.filter(([kind]) => kind === 'capture').length, 1);
  assert.equal(events.some(([kind, state]) => kind === 'status' && state === 'disposed'), false);
});
