import assert from "node:assert/strict";
import { test } from "node:test";
import { loadEngineModule } from "./module.js";

const emptyModule = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]);

test("an explicitly retained module is reused after fresh content validation", async (t) => {
  let reads = 0;
  t.mock.method(globalThis, "fetch", async () => {
    reads++;
    return new Response(emptyModule);
  });
  const first = await loadEngineModule("/engine.wasm");
  const second = await loadEngineModule("/engine.wasm", undefined, first);
  assert.equal(reads, 2);
  assert.equal(second.module, first.module);
  assert.equal(second.hash, first.hash);
});

test("updated engine bytes invalidate the retained module", async (t) => {
  let content = emptyModule;
  t.mock.method(globalThis, "fetch", async () => new Response(content));
  const first = await loadEngineModule("/engine.wasm");
  content = new Uint8Array([...emptyModule, 0, 2, 1, 120]);
  const second = await loadEngineModule("/engine.wasm", undefined, first);
  assert.notEqual(second.hash, first.hash);
  assert.notEqual(second.module, first.module);
});

test("cancellation after fetching prevents module publication", async (t) => {
  const abort = new AbortController();
  const reason = new Error("cancel engine loading");
  t.mock.method(globalThis, "fetch", async () => {
    abort.abort(reason);
    return new Response(emptyModule);
  });
  await assert.rejects(loadEngineModule("/engine.wasm", abort.signal), (error) => error === reason);
});

test("HTTP and invalid compiled-artifact failures are explicit", async (t) => {
  t.mock.method(globalThis, "fetch", async () => new Response("unavailable", { status: 503 }));
  await assert.rejects(loadEngineModule("/engine.wasm"), /engine HTTP 503/);
  await assert.rejects(
    loadEngineModule("/engine.wasm", undefined, { module: {}, hash: "0".repeat(64) }),
    /invalid compiled engine/,
  );
});
