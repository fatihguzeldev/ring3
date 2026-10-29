import assert from "node:assert/strict";
import { test } from "node:test";
import { createGraphicsHost } from "../index.js";

async function graphics(t) {
  const lost = Promise.withResolvers(),
    diagnostics = [],
    listeners = new Map();
  let destroyed = 0;
  const device = {
    lost: lost.promise,
    limits: { maxTextureDimension2D: 8192, maxStorageBufferBindingSize: 134217728 },
    addEventListener: (name, callback) => listeners.set(name, callback),
    queue: { onSubmittedWorkDone: async () => {} },
    pushErrorScope() {},
    popErrorScope: async () => null,
    destroy() {
      destroyed++;
      lost.resolve({ reason: "destroyed", message: "Device was destroyed." });
    },
  };
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: {
      gpu: {
        requestAdapter: async () => ({
          features: new Set(),
          limits: { maxStorageBuffersPerShaderStage: 8, maxStorageBufferBindingSize: 134217728 },
          requestDevice: async () => device,
        }),
        getPreferredCanvasFormat: () => "bgra8unorm",
      },
    },
  });
  t.after(() => {
    if (descriptor) Object.defineProperty(globalThis, "navigator", descriptor);
    else delete globalThis.navigator;
  });
  const host = await createGraphicsHost(
    { getContext: () => ({ unconfigure() {} }) },
    {
      onDiagnostic: (message) => diagnostics.push(message),
    },
  );
  return { host, device, diagnostics, lost, listeners, destroyed: () => destroyed };
}

test("intentional graphics shutdown closes once without a device-loss error", async (t) => {
  const { host, diagnostics, destroyed } = await graphics(t);
  host.close();
  host.close();
  await Promise.resolve();
  assert.equal(destroyed(), 1);
  assert.deepEqual(
    diagnostics.filter((message) => message.type === "error"),
    [],
  );
});

test("unexpected active-device loss remains visible and rejects further packets", async (t) => {
  const { host, diagnostics, lost } = await graphics(t);
  lost.resolve({ reason: "unknown", message: "GPU reset" });
  await Promise.resolve();
  assert.match(diagnostics.at(-1).message, /GPU reset/);
  await assert.rejects(host.consume(new ArrayBuffer(0)), /GPU reset/);
  host.close();
});

test("late validation events after close cannot report a new session failure", async (t) => {
  const { host, diagnostics, listeners } = await graphics(t);
  host.close();
  listeners.get("uncapturederror")({ error: new Error("late validation") });
  await Promise.resolve();
  assert.deepEqual(
    diagnostics.filter((message) => message.type === "error"),
    [],
  );
  await assert.rejects(host.consume(new ArrayBuffer(0)), /closed/);
});

function packet(epoch, sequence, command, opcode, payload) {
  const bytes = new Uint8Array(96 + payload.byteLength),
    data = new DataView(bytes.buffer);
  bytes.set(new TextEncoder().encode("R3D8"));
  data.setUint16(4, 2, true);
  data.setUint16(6, 8, true);
  data.setUint32(8, bytes.length, true);
  data.setUint32(12, 1, true);
  data.setBigUint64(16, BigInt(epoch), true);
  data.setBigUint64(24, BigInt(sequence), true);
  data.setBigUint64(32, BigInt(command), true);
  data.setBigUint64(40, BigInt(command), true);
  data.setUint16(64, opcode, true);
  data.setUint16(66, 3, true);
  data.setUint32(68, 32 + payload.byteLength, true);
  data.setBigUint64(72, BigInt(command), true);
  data.setUint32(84, payload.byteLength, true);
  bytes.set(payload, 96);
  return bytes.buffer;
}
function removed(epoch, sequence = 1, command = 1) {
  const payload = new Uint8Array(12),
    view = new DataView(payload.buffer);
  view.setBigUint64(0, 1n, true);
  view.setUint32(8, 3, true);
  return packet(epoch, sequence, command, 13, payload);
}
function reset(epoch, generation, sequence, command) {
  const payload = new Uint8Array(32),
    view = new DataView(payload.buffer);
  view.setBigUint64(0, BigInt(generation), true);
  [16, 16, 21, 0, 0, 0].forEach((word, i) => view.setUint32(8 + i * 4, word, true));
  return packet(epoch, sequence, command, 12, payload);
}

test("seventeen sequential removed devices do not exhaust the live renderer inventory", async (t) => {
  const { host } = await graphics(t);
  for (let epoch = 1; epoch <= 17; epoch++) {
    await host.consume(removed(epoch));
    assert.equal(host.sessions.size, 0);
  }
  host.close();
});

test("removal waits for GPU completion then destroys each owned resource once", async (t) => {
  const { host, device } = await graphics(t);
  await host.consume(reset(1, 1, 1, 1));
  const session = host.sessions.get(1n),
    calls = [],
    object = (name) => ({ destroy: () => calls.push(name) });
  session.resources.items.set("texture", { texture: object("texture") });
  session.temporary.push(object("scratch"));
  session.patches.set(1n, { controls: object("controls") });
  session.domains.set(1n, { vertices: object("vertices"), indices: object("indices") });
  const submitted = Promise.withResolvers();
  device.queue.onSubmittedWorkDone = () => submitted.promise;
  const completion = host.consume(removed(1, 2, 2));
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(host.sessions.size, 1);
  assert.deepEqual(calls, []);
  submitted.resolve();
  await completion;
  assert.equal(host.sessions.size, 0);
  assert.deepEqual(calls.sort(), ["texture", "scratch", "controls", "vertices", "indices"].sort());
  host.close();
  assert.equal(calls.length, 5);
});

test("Reset reuses its epoch and independent devices may first arrive out of epoch order", async (t) => {
  const { host } = await graphics(t);
  await host.consume(reset(22, 1, 1, 1));
  const original = host.sessions.get(22n);
  await host.consume(reset(11, 1, 1, 1));
  await host.consume(reset(22, 2, 2, 2));
  assert.equal(host.sessions.get(22n), original);
  assert.equal(original.generation, 2n);
  assert.equal(host.sessions.size, 2);
  host.close();
});

test("a retired epoch cannot be recreated by replaying its first packet", async (t) => {
  const { host } = await graphics(t);
  await host.consume(removed(1));
  await assert.rejects(host.consume(reset(1, 1, 1, 1)), /retired.*epoch/i);
  assert.equal(host.sessions.size, 0);
  host.close();
});

test("closing younger epochs does not invent retirement of an unseen older device", async (t) => {
  const { host } = await graphics(t);
  for (let epoch = 2; epoch <= 100; epoch++) await host.consume(removed(epoch));
  await host.consume(reset(1, 1, 1, 1));
  assert.equal(host.sessions.size, 1);
  assert.equal(host.sessions.get(1n).generation, 1n);
  host.close();
});

test("device removal is terminal even when another command follows in the same packet", async (t) => {
  const { host } = await graphics(t);
  const first = new Uint8Array(removed(1)),
    second = new Uint8Array(reset(1, 2, 1, 2));
  const combined = new Uint8Array(first.length + second.length - 64);
  combined.set(first);
  combined.set(second.subarray(64), first.length);
  const view = new DataView(combined.buffer);
  view.setUint32(8, combined.length, true);
  view.setUint32(12, 2, true);
  view.setBigUint64(40, 2n, true);
  await assert.rejects(host.consume(combined.buffer), /command after removal/);
  host.close();
});
