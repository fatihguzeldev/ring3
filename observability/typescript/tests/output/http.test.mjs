import assert from "node:assert/strict";
import { createServer, request } from "node:http";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { createNodeCaptureStore } from "../../dist/output/node.js";
import { createTraceHttpHandler } from "../../dist/output/http.js";
import { createHttpTraceOutput } from "../../dist/output/client.js";

async function fixture(t, options = {}) {
  const rootDirectory = await mkdtemp(join(tmpdir(), "ring3-http-observation-"));
  t.after(() => rm(rootDirectory, { recursive: true, force: true }));
  const store = await createNodeCaptureStore({ rootDirectory, maxChunkBytes: 8, ...options });
  let handler;
  const server = createServer(async (request, response) => {
    if (!(await handler(request, response))) {
      response.writeHead(404);
      response.end();
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  const origin = `http://127.0.0.1:${server.address().port}`;
  handler = createTraceHttpHandler(store, { allowedOrigins: [origin] });
  const fetcher = (url, init = {}) => fetch(url, { ...init, headers: { ...init.headers, origin } });
  return {
    rootDirectory,
    store,
    origin,
    fetcher,
    server,
    baseUrl: `${origin}/api/observability/captures`,
  };
}

test("browser output batches writes and retries lost chunk and finish acknowledgements without duplicating bytes", async (t) => {
  const { rootDirectory, fetcher, baseUrl } = await fixture(t);
  let lostChunk = false;
  let lostFinish = false;
  let lostBegin = false;
  let chunks = 0;
  const output = await createHttpTraceOutput({
    baseUrl,
    metadata: { source: "fixture" },
    batchBytes: 4,
    maxChunkBytes: 8,
    fetch: async (url, init) => {
      const response = await fetcher(url, init);
      if (url === baseUrl && !lostBegin) {
        lostBegin = true;
        await response.arrayBuffer();
        throw new TypeError("connection lost after begin");
      }
      if (url.endsWith("/chunks")) {
        chunks += 1;
        if (!lostChunk) {
          lostChunk = true;
          await response.arrayBuffer();
          throw new TypeError("connection lost after append");
        }
      }
      if (url.endsWith("/finish") && !lostFinish) {
        lostFinish = true;
        await response.arrayBuffer();
        throw new TypeError("connection lost after finish");
      }
      return response;
    },
  });
  await output.write(Uint8Array.of(1));
  await output.write(Uint8Array.of(2));
  assert.equal(chunks, 0);
  await output.write(Uint8Array.of(3, 4));
  assert.equal(chunks, 2);
  await output.write(Uint8Array.of(5));
  await output.flush();
  await output.close();
  await output.close();
  assert.equal(output.completion.complete, true);
  assert.deepEqual(
    [...(await readFile(join(rootDirectory, output.id, "trace.pftrace")))],
    [1, 2, 3, 4, 5],
  );
  await assert.rejects(output.write(Uint8Array.of(6)), /closed/i);
});

test("bounds pending client bytes while preserving the caller-owned buffer", async (t) => {
  const { rootDirectory, fetcher, baseUrl } = await fixture(t);
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const output = await createHttpTraceOutput({
    baseUrl,
    metadata: {},
    batchBytes: 4,
    maxChunkBytes: 8,
    maxPendingBytes: 4,
    fetch: async (url, init) => {
      if (url.endsWith("/chunks")) await gate;
      return fetcher(url, init);
    },
  });
  const bytes = Uint8Array.of(1, 2, 3, 4);
  const first = output.write(bytes);
  bytes.fill(9);
  await assert.rejects(output.write(Uint8Array.of(5)), /pending byte/i);
  release();
  await first;
  await output.close();
  assert.deepEqual(
    [...(await readFile(join(rootDirectory, output.id, "trace.pftrace")))],
    [1, 2, 3, 4],
  );
});

test("HTTP output rejects foreign origins, missing origin, traversal, body limits and sequence gaps", async (t) => {
  const { baseUrl, fetcher } = await fixture(t);
  const open = {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ metadata: {}, requestId: "request-1" }),
  };
  assert.equal((await fetch(baseUrl, open)).status, 403);
  assert.equal(
    (
      await fetch(baseUrl, {
        ...open,
        headers: { ...open.headers, origin: "https://other.example" },
      })
    ).status,
    403,
  );
  const response = await fetcher(baseUrl, open);
  const { id } = await response.json();
  const chunk = {
    method: "POST",
    headers: { "content-type": "application/octet-stream", "x-trace-sequence": "1" },
    body: Uint8Array.of(1),
  };
  assert.equal((await fetcher(`${baseUrl}/${id}/chunks`, chunk)).status, 409);
  assert.equal((await fetcher(`${baseUrl}/%2e%2e%2fescape/chunks`, chunk)).status, 400);
  assert.equal(
    (await fetcher(`${baseUrl}/${id}/chunks`, { ...chunk, body: new Uint8Array(9) })).status,
    413,
  );
  assert.equal(
    (
      await fetcher(`${baseUrl}/${id}/chunks`, {
        ...chunk,
        headers: { ...chunk.headers, "x-trace-sequence": "9007199254740992" },
      })
    ).status,
    400,
  );
});

test("disk budget failure terminates output without retrying permanent rejection or marking complete", async (t) => {
  const { rootDirectory, fetcher, baseUrl } = await fixture(t, { maxCaptureBytes: 2 });
  let chunks = 0;
  const output = await createHttpTraceOutput({
    baseUrl,
    metadata: {},
    batchBytes: 4,
    maxChunkBytes: 8,
    fetch: async (url, init) => {
      if (url.endsWith("/chunks")) chunks += 1;
      return fetcher(url, init);
    },
  });
  await assert.rejects(output.write(Uint8Array.of(1, 2, 3, 4)), /507/);
  assert.equal(chunks, 1);
  await assert.rejects(output.close(), /closed/);
  await output.abort("output quota reached");
  const manifest = JSON.parse(
    await readFile(join(rootDirectory, output.id, "capture.json"), "utf8"),
  );
  assert.equal(manifest.complete, false);
  assert.equal(manifest.status, "aborted");
});

test(
  "a disconnected partial upload leaves the sequence untouched and can be retried",
  { timeout: 3000 },
  async (t) => {
    const { rootDirectory, store, server, origin, baseUrl } = await fixture(t);
    const { id } = await store.begin({});
    const received = new Promise((resolve) =>
      server.once("request", (incoming) => incoming.once("data", resolve)),
    );
    const upload = request(`${baseUrl}/${id}/chunks`, {
      method: "POST",
      headers: {
        origin,
        "content-type": "application/octet-stream",
        "content-length": "8",
        "x-trace-sequence": "0",
      },
    });
    upload.on("error", () => undefined);
    upload.write(Uint8Array.of(1, 2, 3, 4));
    await received;
    const closed = new Promise((resolve) => upload.once("close", resolve));
    upload.destroy();
    await closed;
    assert.equal((await store.get(id)).nextSequence, 0);
    const retry = await fetch(`${baseUrl}/${id}/chunks`, {
      method: "POST",
      headers: { origin, "content-type": "application/octet-stream", "x-trace-sequence": "0" },
      body: Uint8Array.of(1, 2, 3, 4, 5, 6, 7, 8),
    });
    assert.equal(retry.status, 200);
    await store.finish(id, 1);
    assert.deepEqual(
      [...(await readFile(join(rootDirectory, id, "trace.pftrace")))],
      [1, 2, 3, 4, 5, 6, 7, 8],
    );
  },
);
