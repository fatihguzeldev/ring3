import assert from "node:assert/strict";
import { test } from "node:test";
import { BrowserCapture } from "../observability.js";

function outputRequests(t, abortStatus = 200) {
  const requests = [];
  const id = "00000000-0000-4000-8000-000000000001";
  t.mock.method(globalThis, "fetch", async (url, options) => {
    requests.push({ url, body: JSON.parse(options.body) });
    return new Response(JSON.stringify({ id }), {
      status: url.endsWith("/abort") ? abortStatus : 200,
      headers: { "content-type": "application/json" },
    });
  });
  return { requests, id };
}

test("unknown capture categories abort the already opened output", async (t) => {
  const { requests, id } = outputRequests(t);
  await assert.rejects(
    BrowserCapture.start({ level: "basic", categories: ["unknown.category"] }, () => {}),
    /unknown or nonselectable observation category/,
  );
  assert.deepEqual(
    requests.map((request) => request.url),
    ["/api/observability/captures", `/api/observability/captures/${id}/abort`],
  );
});

test("output abort failure preserves the original capture validation error", async (t) => {
  const { requests } = outputRequests(t, 409);
  await assert.rejects(
    BrowserCapture.start({ level: "basic", categories: ["unknown.category"] }, () => {}),
    /unknown or nonselectable observation category/,
  );
  assert.equal(requests.length, 2);
});

test("an explicit empty category selection stays empty while retaining capture lifecycle metadata", async () => {
  const written = [];
  const capture = new BrowserCapture(
    { level: "basic", categories: [] },
    {
      id: "fixture",
      async write(bytes) {
        written.push(bytes);
      },
      async flush() {},
      async close() {},
    },
    () => {},
  );
  assert.equal(capture.observer.enabled, false);
  assert.equal((await capture.finish("cancelled")).complete, true);
  assert.ok(written.length > 0);
});
