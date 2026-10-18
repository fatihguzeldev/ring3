import assert from "node:assert/strict";
import test from "node:test";
import { createCapture } from "../../dist/index.js";
import { clockSnapshot, event } from "../../dist/perfetto/events.js";
import { fields, get, memoryOutput, packets, text } from "./decode.mjs";

test("split external packets cannot interleave with host events", async () => {
  const sink = memoryOutput();
  const capture = createCapture({ level: "basic", output: sink });
  const packet = event(900, 1n, 900n << 32n, 3, "worker", "split", { payload: "x".repeat(500) });
  assert.equal(capture.ingest(packet.subarray(0, 3)), true);
  assert.ok(capture.status().pendingPacketBytes > 0);
  assert.ok(capture.status().bufferedBytes >= capture.status().pendingPacketBytes);
  capture.observer("host").event("between");
  assert.equal(capture.ingest(packet.subarray(3)), true);
  assert.equal((await capture.finish("complete")).complete, true);
  const names = packets(sink.chunks).flatMap((value) =>
    value.event && get(value.event, 23) ? [text(get(value.event, 23))] : [],
  );
  assert.deepEqual(names, ["between", "split", "capture.finished"]);
});

test("terminal partial packets remain incomplete and never poison the saved trace", async () => {
  const sink = memoryOutput();
  let closed = false;
  let aborted = false;
  sink.close = async () => {
    closed = true;
  };
  sink.abort = async () => {
    aborted = true;
  };
  const capture = createCapture({ level: "basic", output: sink });
  const packet = event(901, 1n, 901n << 32n, 3, "worker", "truncated");
  capture.ingest(packet.subarray(0, 3));
  const result = await capture.finish("failed");
  assert.equal(result.complete, false);
  assert.match(result.recordingError, /inside a packet/);
  assert.equal(closed, false);
  assert.equal(aborted, true);
  assert.ok(packets(sink.chunks).length > 0);
});

test("invalid async scope arguments reuse the abandoned track lease", async () => {
  const sink = memoryOutput();
  const capture = createCapture({ level: "detailed", output: sink });
  const observer = capture.observer("files");
  for (let index = 0; index < 100; index++)
    await observer.scopeAsync("x".repeat(257), {}, async () => {});
  await observer.scopeAsync("valid", {}, async () => {});
  const result = await capture.finish("complete");
  assert.equal(result.droppedEvents, 100);
  const tracks = packets(sink.chunks).filter((value) => value.descriptor);
  assert.equal(tracks.length, 2);
  assert.equal(result.openScopes, 0);
});

test("aggregation keeps one clock anchor despite late arrivals from older producers", async () => {
  const childOutput = memoryOutput();
  const child = createCapture({ level: "basic", output: childOutput, clock: () => 10n });
  child.observer("worker").event("child");
  await child.finish("complete");
  const sink = memoryOutput();
  const capture = createCapture({ level: "basic", output: sink, clock: () => 100n });
  capture.observer("host").event("parent");
  for (const chunk of childOutput.chunks) assert.equal(capture.ingest(chunk), true);
  assert.equal((await capture.finish("complete")).complete, true);
  const parsed = packets(sink.chunks);
  assert.equal(parsed.filter((value) => get(value.packet, 6)).length, 1);
  const names = parsed.flatMap((value) =>
    value.event && get(value.event, 23) ? [text(get(value.event, 23))] : [],
  );
  assert.ok(names.includes("parent"));
  assert.ok(names.includes("child"));
});

test("incompatible imported clock mappings fail without rewriting or persisting them", async () => {
  const sink = memoryOutput();
  const capture = createCapture({ level: "basic", output: sink });
  const incompatible = clockSnapshot(999, 50n);
  incompatible[incompatible.length - 1] = 1;
  assert.equal(capture.ingest(incompatible), false);
  const result = await capture.finish("failed");
  assert.equal(result.complete, false);
  assert.match(result.recordingError, /incompatible imported trace clock snapshot/);
  assert.equal(packets(sink.chunks).filter((value) => get(value.packet, 6)).length, 1);
});
