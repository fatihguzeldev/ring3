import assert from "node:assert/strict";
import { test } from "node:test";
import { Engine } from "../engine.js";

async function settle() {
  for (let index = 0; index < 30; index++) await Promise.resolve();
}

test("startup stop settles and closes its own engine before a queued replacement starts", async (t) => {
  const previousSelf = globalThis.self,
    previousPost = globalThis.postMessage;
  const messages = [],
    calls = [];
  globalThis.self = {};
  globalThis.postMessage = (message) => messages.push(message);
  t.after(() => {
    if (previousSelf === undefined) delete globalThis.self;
    else globalThis.self = previousSelf;
    if (previousPost === undefined) delete globalThis.postMessage;
    else globalThis.postMessage = previousPost;
  });
  let release,
    creates = 0;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  t.mock.method(Engine, "create", async () => {
    calls.push(`create.${++creates}`);
    if (creates > 1) throw new Error("replacement startup fixture");
    await gate;
    return { get: () => 0, status: (operation) => calls.push(operation) };
  });
  await import(`../worker.js?settlement=${Date.now()}`);
  const send = (data) => globalThis.self.onmessage({ data });
  send({ type: "start" });
  await settle();
  send({ type: "stop" });
  send({ type: "start" });
  await settle();
  assert.deepEqual(calls, ["create.1"]);
  release();
  await settle();
  assert.deepEqual(calls, ["create.1", "close", "create.2"]);
  assert.equal(messages.filter((message) => message.state === "stopped").length, 1);
  assert.equal(messages.filter((message) => message.state === "error").length, 1);
});
