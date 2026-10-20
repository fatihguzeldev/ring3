import assert from 'node:assert/strict';
import test from 'node:test';
import {PhaseMeasurements} from '../runtime/session/measurement.js';
import {FrameProbe} from '../runtime/session/frame-probe.js';

function session(measurements = null) {
  const events = [];
  return {events, measurements, frameProbe: null, thread: 3,
    engine: {hash: 'engine-hash', get: () => 0x1000},
    code: {sampleUsage() {}, stats: () => ({retired: 7})},
    emit(value) { events.push(value.detail); }};
}

function observed(detailed = false) {
  const events = [], counters = [], scopes = [];
  return {events, counters, scopes, observation: {
    observer: {event: (...args) => events.push(args), counter: (...args) => counters.push(args)},
    capture: {observer: () => ({enabled: detailed,
      scope(name, fields, operation) { scopes.push(name); return operation(); },
      async scopeAsync(name, fields, operation) { scopes.push(name); return await operation(); },
    })},
  }};
}

test('aggregate phases require no observer and close throwing scopes in finally', async t => {
  let now = 0;
  t.mock.method(performance, 'now', () => now);
  const phases = new PhaseMeasurements(null);
  assert.equal(phases.run('entry', () => { now = 2; return 5; }), 5);
  assert.throws(() => phases.run('execution', () => { now = 5; throw new Error('sync'); }), /sync/);
  await assert.rejects(phases.runAsync('providers', async () => { now = 9; throw new Error('async'); }), /async/);
  assert.deepEqual(phases.snapshot(), {
    entry: {elapsedNs: 2_000_000, maximumNs: 2_000_000, count: 1},
    execution: {elapsedNs: 3_000_000, maximumNs: 3_000_000, count: 1},
    providers: {elapsedNs: 4_000_000, maximumNs: 4_000_000, count: 1},
  });
  assert.doesNotThrow(() => phases.report());
});

test('observed phases retain their schema, detailed scopes and counter exports', async t => {
  let now = 0;
  t.mock.method(performance, 'now', () => now);
  const fixture = observed(true), phases = new PhaseMeasurements(fixture.observation);
  phases.run('entry', () => { now = 2; });
  await phases.runAsync('wait', async () => { now = 5; });
  phases.report();
  assert.deepEqual(fixture.scopes, ['entry', 'wait']);
  assert.equal(fixture.events.length, 1);
  assert.equal(fixture.events[0][0], 'measurement.schema');
  assert.deepEqual(fixture.counters, [
    ['entry.ns', 2_000_000], ['entry.count', 1], ['entry.max.ns', 2_000_000],
    ['wait.ns', 3_000_000], ['wait.count', 1], ['wait.max.ns', 3_000_000],
  ]);
});

test('an unobserved probe owns fresh aggregate phases until its ninth completed edge', () => {
  const owner = session(), probe = new FrameProbe('proof', owner);
  owner.frameProbe = probe;
  assert.ok(owner.measurements instanceof PhaseMeasurements);
  assert.equal(probe.measurements, owner.measurements);
  assert.equal(owner.events[0].measurement_enabled, true);
  assert.match(owner.events[0].phase_semantics, /paused-loop waits excluded; in-flight async spans may cross pause/);
  const phases = owner.measurements;
  const receipt = {id: 1, callArgs: [7], pc: 0x1000};
  for (let index = 1; index <= 9; index++) {
    phases.run('completion', () => {});
    const info = probe.batch({requestId: index, epoch: 1, sequence: index});
    probe.present(receipt, 0, 0, 0n, 1n, 1n);
    assert.equal(owner.measurements, phases);
    probe.acknowledged({info, sent: {}}, {}, {}, {}, [{}]);
    if (index < 9) assert.equal(owner.measurements, phases);
  }
  assert.equal(probe.closed, true);
  assert.equal(owner.measurements, null);
  assert.equal(owner.frameProbe, null);
  assert.equal(owner.events.at(-1).reason, 'complete');
  assert.equal(owner.events.filter(event => event.event === 'producer-present').at(-1).phases.completion.count, 9);
  assert.equal(probe.snapshot().phases.completion.count, 9);
  assert.doesNotThrow(() => phases.report());
  assert.equal(owner.events.filter(event => event.event === 'measurement.schema').length, 0);
});

test('a probe preserves observed phases and snapshots its original object', () => {
  const fixture = observed(), phases = new PhaseMeasurements(fixture.observation);
  phases.run('entry', () => {});
  const owner = session(phases), probe = new FrameProbe('proof', owner);
  owner.frameProbe = probe;
  assert.equal(probe.measurements, phases);
  probe.finish('session-ended');
  assert.equal(owner.measurements, phases);
  owner.measurements = {snapshot: () => ({replacement: true})};
  assert.equal(probe.snapshot().phases.entry.count, 1);
  assert.equal(probe.snapshot().phases.replacement, undefined);
});

test('finishing owned phases does not clear a replacement measurement object', () => {
  const owner = session(), probe = new FrameProbe('proof', owner);
  const replacement = {snapshot: () => ({replacement: true})};
  owner.measurements = replacement;
  probe.finish('session-ended');
  assert.equal(owner.measurements, replacement);
  assert.deepEqual(probe.snapshot().phases, {});
});

for (const reason of ['attempt-overflow', 'batch-overflow', 'producer-overflow', 'batch-identity-mismatch']) {
  test(`${reason} releases the probe-owned phases`, () => {
    const owner = session(), probe = new FrameProbe('proof', owner);
    owner.frameProbe = probe;
    assert.ok(owner.measurements instanceof PhaseMeasurements);
    if (reason === 'attempt-overflow') {
      probe.attempts = 4096;
      probe.present(null, 0, 0, 0n, 1n, 1n);
    } else if (reason === 'batch-overflow') {
      probe.batches = 2048;
      assert.equal(probe.batch(null), null);
    } else if (reason === 'producer-overflow') {
      probe.accepted = 64;
      probe.present({id: 1, callArgs: [7], pc: 0x1000}, 0, 0, 0n, 1n, 1n);
    } else {
      probe.acknowledged(null, {}, {}, {}, []);
    }
    assert.equal(probe.closed, true);
    assert.equal(owner.measurements, null);
    assert.equal(owner.frameProbe, null);
    assert.equal(owner.events.at(-1).reason, reason);
  });
}

test('a failing finished diagnostic still releases exact owned state once', () => {
  const owner = session(), probe = new FrameProbe('proof', owner);
  owner.frameProbe = probe;
  owner.emit = () => { throw new Error('diagnostic failed'); };
  assert.throws(() => probe.finish('session-ended'), /diagnostic failed/);
  assert.equal(owner.measurements, null);
  assert.equal(owner.frameProbe, null);
  assert.doesNotThrow(() => probe.finish('session-ended'));
});

test('a failed arm does not leave aggregate phases attached', () => {
  const owner = session();
  owner.emit = () => { throw new Error('arm failed'); };
  assert.throws(() => new FrameProbe('proof', owner), /arm failed/);
  assert.equal(owner.measurements, null);
});

test('an async scope finishing after probe release cannot contaminate a later probe', async t => {
  let now = 0, release;
  t.mock.method(performance, 'now', () => now);
  const owner = session(), first = new FrameProbe('first', owner);
  const phases = owner.measurements;
  const operation = phases.runAsync('wait', () => new Promise(resolve => { release = resolve; }));
  now = 2;
  first.finish('session-ended');
  const second = new FrameProbe('second', owner);
  second.measurements.run('entry', () => { now = 3; });
  now = 7;
  release();
  await operation;
  assert.deepEqual(first.snapshot().phases.wait, {elapsedNs: 7_000_000, maximumNs: 7_000_000, count: 1});
  assert.deepEqual(second.snapshot().phases, {entry: {elapsedNs: 1_000_000, maximumNs: 1_000_000, count: 1}});
  assert.equal(owner.measurements, second.measurements);
  second.finish('session-ended');
});
