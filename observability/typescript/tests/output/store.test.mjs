import assert from "node:assert/strict";
import { mkdtemp, readFile, readdir, mkdir, symlink, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import fs from "node:fs/promises";
import { constants } from "node:fs";
import { createNodeCaptureStore, createNodeTraceOutput } from "../../dist/output/node.js";
import { categories, createCapture } from "../../dist/index.js";

async function fixture(t, limits = {}) {
  const rootDirectory = await mkdtemp(join(tmpdir(), "ring3-observation-"));
  t.after(() => rm(rootDirectory, { recursive: true, force: true }));
  return { rootDirectory, store: await createNodeCaptureStore({ rootDirectory, ...limits }) };
}

test("persists ordered chunks, retries identical acknowledgements and atomically completes", async (t) => {
  const { rootDirectory, store } = await fixture(t);
  const capture = await store.begin({ engineHash: "author-written-fixture" }, "request-1");
  assert.deepEqual(
    await store.begin({ engineHash: "author-written-fixture" }, "request-1"),
    capture,
  );
  await assert.rejects(store.begin({ other: true }, "request-1"), /metadata/i);
  assert.deepEqual(await store.append(capture.id, 0, Uint8Array.of(1, 2)), { nextSequence: 1 });
  assert.deepEqual(await store.append(capture.id, 0, Uint8Array.of(1, 2)), { nextSequence: 1 });
  await assert.rejects(store.append(capture.id, 0, Uint8Array.of(9)), /retry/i);
  await assert.rejects(store.append(capture.id, 2, Uint8Array.of(3)), /sequence/i);
  await store.append(capture.id, 1, Uint8Array.of(3));
  await store.flush(capture.id, 2);
  const directory = join(rootDirectory, capture.id);
  assert.equal(JSON.parse(await readFile(join(directory, "capture.json"), "utf8")).complete, false);
  const completed = await store.finish(capture.id, 2);
  assert.equal(completed.complete, true);
  assert.equal(completed.bytes, 3);
  assert.deepEqual(await store.finish(capture.id, 2), completed);
  assert.deepEqual([...(await readFile(join(directory, "trace.pftrace")))], [1, 2, 3]);
  assert.equal((await readdir(directory)).includes("trace.pftrace.partial"), false);
  await assert.rejects(store.append(capture.id, 2, Uint8Array.of(4)), /closed/i);
});

test("keeps interrupted and aborted streams explicitly incomplete", async (t) => {
  const { rootDirectory, store } = await fixture(t);
  const capture = await store.begin({});
  await store.append(capture.id, 0, Uint8Array.of(10, 0));
  await store.flush(capture.id, 1);
  const restarted = await createNodeCaptureStore({ rootDirectory });
  const manifest = await restarted.get(capture.id);
  assert.equal(manifest.complete, false);
  assert.equal(manifest.bytes, 2);
  await assert.rejects(restarted.append(capture.id, 1, Uint8Array.of(2)), /active/i);
  const aborted = await store.abort(capture.id, "capture stopped");
  assert.equal(aborted.complete, false);
  assert.equal(aborted.status, "aborted");
  assert.equal(
    (await readdir(join(rootDirectory, capture.id))).includes("trace.pftrace.partial"),
    true,
  );
});

test("retries a transient manifest failure after the trace was renamed without losing data", async (t) => {
  const { rootDirectory, store } = await fixture(t);
  const { id } = await store.begin({});
  await store.append(id, 0, Uint8Array.of(1, 2, 3));
  const rename = fs.rename;
  let manifestWrites = 0;
  const fault = t.mock.method(fs, "rename", async (source, destination) => {
    if (destination.endsWith("capture.json") && ++manifestWrites === 2) {
      throw new Error("transient manifest write failure");
    }
    return rename(source, destination);
  });
  await assert.rejects(store.finish(id, 1), /transient manifest/);
  fault.mock.restore();
  assert.equal((await store.get(id)).complete, false);
  assert.deepEqual([...(await readFile(join(rootDirectory, id, "trace.pftrace")))], [1, 2, 3]);
  assert.equal((await store.finish(id, 1)).complete, true);
  assert.equal((await store.get(id)).complete, true);
});

test("a file close failure aborts the stream and a retry cannot append duplicate bytes", async (t) => {
  const { rootDirectory, store } = await fixture(t);
  const { id } = await store.begin({});
  const open = fs.open;
  const fault = t.mock.method(fs, "open", async (...args) => {
    const handle = await open(...args);
    if (typeof args[1] !== "number" || !(args[1] & constants.O_APPEND)) return handle;
    return {
      write: handle.write.bind(handle),
      async close() {
        await handle.close();
        throw new Error("trace close failure");
      },
    };
  });
  await assert.rejects(store.append(id, 0, Uint8Array.of(1, 2, 3)), /close failure/);
  fault.mock.restore();
  await assert.rejects(store.append(id, 0, Uint8Array.of(1, 2, 3)), /closed/);
  assert.equal((await store.get(id)).complete, false);
  assert.equal((await store.get(id)).status, "aborted");
  assert.deepEqual(
    [...(await readFile(join(rootDirectory, id, "trace.pftrace.partial")))],
    [1, 2, 3],
  );
});

test("a capture can write directly to the Node output without an HTTP server", async (t) => {
  const { rootDirectory, store } = await fixture(t, { maxChunkBytes: 64 });
  const output = await createNodeTraceOutput(store, { source: "direct Node capture" });
  const capture = createCapture({ level: "basic", output });
  capture.observer(categories.engineLoader).event("fixture.ready", { bytes: 128 });
  const result = await capture.finish("cancelled");
  assert.equal(result.complete, true);
  assert.equal(result.outcome, "cancelled");
  assert.equal(output.completion.complete, true);
  const trace = await readFile(join(rootDirectory, output.id, "trace.pftrace"));
  assert.ok(trace.length > 64);
  await assert.rejects(output.write(Uint8Array.of(1)), /closed/i);
});

test("bounds chunk, capture, total disk, metadata and capture count", async (t) => {
  const { store } = await fixture(t, {
    maxChunkBytes: 3,
    maxCaptureBytes: 4,
    maxTotalBytes: 5,
    maxCaptures: 2,
  });
  const first = await store.begin({});
  await assert.rejects(store.append(first.id, 0, Uint8Array.of(1, 2, 3, 4)), /chunk/i);
  await store.append(first.id, 0, Uint8Array.of(1, 2, 3));
  await assert.rejects(store.append(first.id, 1, Uint8Array.of(4, 5)), /capture byte/i);
  const second = await store.begin({});
  await assert.rejects(store.append(second.id, 0, Uint8Array.of(1, 2, 3)), /total byte/i);
  await assert.rejects(store.begin({}), /capture count/i);
  await assert.rejects(store.begin({ text: "x".repeat(70_000) }), /metadata/i);
  await assert.rejects(store.begin({ toJSON: () => undefined }), /metadata/i);
});

test("rejects path input, symlink capture directories and manifest output failures", async (t) => {
  const { rootDirectory, store } = await fixture(t);
  await assert.rejects(store.get("../escape"), /identifier/i);
  await assert.rejects(store.get("%2e%2e"), /identifier/i);
  const capture = await store.begin({});
  const directory = join(rootDirectory, capture.id);
  await rm(join(directory, "capture.json"));
  await mkdir(join(directory, "capture.json"));
  await assert.rejects(store.flush(capture.id, 0));
  const external = await mkdtemp(join(tmpdir(), "ring3-observation-outside-"));
  t.after(() => rm(external, { recursive: true, force: true }));
  await rm(directory, { recursive: true });
  await symlink(external, directory);
  await assert.rejects(store.append(capture.id, 0, Uint8Array.of(1)), /directory/i);
  assert.deepEqual(await readdir(external), []);
});
