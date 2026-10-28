import assert from 'node:assert/strict';
import test from 'node:test';
import {Engine} from '../runtime/engine.js';
import {Session} from '../runtime/session.js';
import {WindowPresentation} from '../runtime/windows.js';
import {CompletedWindowsCalls} from '../runtime/session/diagnostics.js';

test('borrowed results reflect the arena while take preserves an owned snapshot', () => {
  const engine = new Engine({}, new WebAssembly.Memory({initial: 1}), 'fixture');
  engine.base = 256;
  const bytes = new Uint8Array(engine.memory.buffer, engine.base + 140, 64);
  bytes[0] = 17;
  const borrowed = engine.resultView(64), snapshot = engine.take(64);
  assert.equal(borrowed.byteLength, 64);
  assert.equal(borrowed.getUint8(0), 17);
  bytes[0] = 29;
  assert.equal(borrowed.getUint8(0), 29);
  assert.equal(snapshot[0], 17);
});

test('state and result reads refresh after memory growth and arena relocation', () => {
  const engine = new Engine({}, new WebAssembly.Memory({initial: 1}), 'fixture');
  engine.base = 256;
  new DataView(engine.memory.buffer).setUint32(304, 0x12345678, true);
  assert.equal(engine.get(48), 0x12345678);
  engine.memory.grow(1);
  engine.base = 65536;
  const data = new DataView(engine.memory.buffer);
  data.setUint32(engine.base + 48, 0x87654321, true);
  data.setBigUint64(engine.base + 156, 0xfedcba9876543210n, true);
  assert.equal(engine.get(48), 0x87654321);
  assert.equal(engine.resultView(64).getBigUint64(16, true), 0xfedcba9876543210n);
  assert.throws(() => engine.get(65536), RangeError);
  assert.throws(() => engine.resultView(65536), RangeError);
});

test('media values survive later arena reuse without retaining the borrowed result', () => {
  let epoch = 7n;
  const engine = new Engine({fixture() {
    const data = new DataView(engine.memory.buffer, engine.base + 140, 64);
    data.setBigUint64(16, epoch, true);
    for (let i = 0; i < 5; i++) data.setBigUint64(24 + i * 8, epoch + BigInt(i), true);
    return 0;
  }}, new WebAssembly.Memory({initial: 1}), 'fixture');
  const first = engine.media('fixture');
  epoch = 11n;
  const second = engine.media('fixture');
  assert.deepEqual(first, {epoch: 7n, values: [7n, 8n, 9n, 10n, 11n]});
  assert.deepEqual(second, {epoch: 11n, values: [11n, 12n, 13n, 14n, 15n]});
});

test('an absent Windows callback returns its boolean synchronously', () => {
  const session=Object.create(Session.prototype),operations=[];
  session.engine={get:()=>3,ok:operation=>operations.push(operation),take:()=>new Uint8Array(96)};
  assert.equal(session.beginCallback(),false);
  assert.deepEqual(operations,['pending_windows_callback']);
});

test('call capture consumes borrowed fields before native completion overwrites the arena', async () => {
  const memory = new WebAssembly.Memory({initial: 1});
  const api = {
    pending_windows_thread_schedule: () => 0,
    core_capture_windows_call() {
      const data = new DataView(memory.buffer, 140, 112);
      for (const [offset, value] of [[0, 0x46433352], [4, 0x10001], [8, 112], [16, 7],
        [20, 0x4010f], [24, 1], [28, 2], [36, 0x8000], [40, 0x1234], [48, 0xaaaa], [52, 0xbbbb]])
        data.setUint32(offset, value, true);
      return 0;
    },
    core_complete_windows_call() {
      new Uint8Array(memory.buffer, 140, 112).fill(0xcc);
      return 0;
    },
  };
  const engine = new Engine(api, memory, 'fixture');
  new DataView(memory.buffer).setUint32(72, 3, true);
  const windows = new WindowPresentation(engine, () => {}), diagnostics = new CompletedWindowsCalls(engine);
  let calls = 0;
  const session = Object.assign(Object.create(Session.prototype), {
    running: true, paused: false, stopped: false, lastSchedule: Infinity, lastStatus: Infinity,
    recent: [], thread: 1, blocked: new Map(), exits: new Map(), engine, windows,
    callDiagnostics: diagnostics, media: {update() {}, async graphicsOutput() {}, graphicsTransport: {}},
    code: {
      publish() {}, import() {}, select: () => null,
      execute() {
        if (++calls === 1) return {coreReason: 4, reason: 8, pc: 0x1000, detail: 0x4010f};
        session.running = false;
        return {coreReason: 2};
      },
    },
    async providers() { return false; }, observeClock: () => 0n, initializer: () => null,
    stack: () => [], async wait() {}, beginCallback() { return false; }, settleWindows() {},
  });
  await session.loop();
  assert.equal(calls, 2);
  assert.deepEqual(session.lastCall.args, [0xaaaa, 0xbbbb]);
  assert.deepEqual(windows.pending[0].receipt.callArgs, [0xaaaa, 0xbbbb]);
  assert.equal(windows.pending[0].returnPc, 0x1234);
  assert.deepEqual(diagnostics.pending[0].args, [0xaaaa, 0xbbbb]);
  assert.equal(diagnostics.pending[0].token, 7);
  assert.equal(diagnostics.pending[0].returnEsp, 0x8004);
  assert.equal(engine.resultView(112).getUint32(0, true), 0xcccccccc);
});
