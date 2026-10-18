import assert from "node:assert/strict";
import { test } from "node:test";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { createCapture, epochNowNs } from "../../typescript/dist/index.js";

const root = resolve(import.meta.dirname, "../../..");
const artifact =
  process.env.RING3_OBSERVABILITY_ENGINE ??
  resolve(root, "target/observability-verification/engine.wasm");
const bytes = await readFile(artifact);
const module = new WebAssembly.Module(bytes);
const key = [0x12345678, 0x76543210];

function engine() {
  const instance = new WebAssembly.Instance(module, {
    ring3_observability: { now_ns: epochNowNs },
  });
  const api = Object.fromEntries(
    Object.entries(instance.exports)
      .filter(([name]) => name.startsWith("ring3_abi_v1_"))
      .map(([name, value]) => [name.slice(13), value]),
  );
  const call = (name, ...args) => assert.equal(api[name](...args), 0, name);
  call("open", 16, ...key);
  const base = api.arena_ptr() >>> 0;
  const transfer = () => new Uint8Array(instance.exports.memory.buffer, base + 140, 4096);
  const chunk = (operation) => {
    call(operation);
    const packet = transfer().slice(),
      header = new DataView(packet.buffer),
      size = header.getUint32(0, true);
    assert.ok(size <= 4080);
    call("observability_status");
    const status = new DataView(transfer().buffer, transfer().byteOffset, 40);
    return {
      bytes: packet.slice(16, 16 + size),
      finished: header.getUint32(4, true) !== 0,
      droppedEvents: header.getBigUint64(8, true),
      openScopes: status.getBigUint64(32, true),
    };
  };
  return {
    api,
    call,
    transfer,
    port: {
      supported: typeof api.configure_observability === "function",
      configure(config) {
        const context = new TextEncoder().encode(config.contexts.join("\n"));
        transfer().set(context);
        call(
          "configure_observability",
          ["off", "basic", "detailed"].indexOf(config.level),
          config.maxBytes,
          config.sequenceId,
          Number(config.trackUuid & 0xffffffffn),
          Number(config.trackUuid >> 32n),
          context.length,
        );
      },
      drain: () => chunk("observability_drain"),
      finish: () => chunk("observability_finish"),
    },
  };
}

test("real Wasm codegen and host scopes share a complete standard capture without a game", async () => {
  const parts = [],
    capture = createCapture({
      level: "detailed",
      producerId: 31,
      output: {
        async write(bytes) {
          parts.push(bytes.slice());
        },
        async flush() {},
      },
    });
  const e = engine();
  capture.attachEngine(e.port, { producerId: 32, maxBytes: 128 * 1024 });
  e.call("map", 0x1000, 1, 7);
  e.transfer().set([0xb8, 1, 0, 0, 0]);
  e.call("upload", 0x1000, 5);
  const observer = capture.observer("host.test");
  for (let i = 0; i < 48; i++) {
    const descriptor = new DataView(e.transfer().buffer, e.transfer().byteOffset, 8);
    descriptor.setUint32(0, 0x1000, true);
    descriptor.setUint32(4, 5, true);
    observer.scope("compile-request", { iteration: i }, () => e.call("compile", 1));
  }
  const result = await capture.finish();
  assert.equal(result.complete, true, JSON.stringify(result));
  assert.equal(result.engine, "finished");
  assert.equal(result.droppedEvents, 0);
  assert.equal(result.droppedChunks, 0);
  const output = Buffer.concat(parts);
  assert.ok(output.length > 4096, "must drain beyond the first transfer chunk");
  const directory = resolve(root, "target/observability-verification");
  await mkdir(directory, { recursive: true });
  await writeFile(resolve(directory, "engine-host.pftrace"), output);
  e.call("close");
});

test("feature Wasm capture configuration validates without starting a guest", () => {
  const e = engine();
  assert.notEqual(e.api.configure_observability(3, 65536, 40, 1, 40, 0), 0);
  e.call("configure_observability", 1, 65536, 40, 1, 40, 0);
  assert.notEqual(
    e.api.configure_observability(1, 65536, 40, 1, 40, 0),
    0,
    "active capture cannot be discarded",
  );
  let chunk = e.port.finish();
  while (!chunk.finished) chunk = e.port.drain();
  e.call("configure_observability", 0, 65536, 41, 1, 41, 0);
  e.call("close");
});
