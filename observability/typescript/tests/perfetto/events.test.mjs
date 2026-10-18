import assert from "node:assert/strict";
import test from "node:test";
import { categories, createCapture } from "../../dist/index.js";
import { fields, get, memoryOutput, packets, text } from "./decode.mjs";

test("standard TracePacket fields carry nanoseconds, typed annotations, counters and nested scopes", async () => {
  const sink = memoryOutput();
  let time = 1_700_000_000_000_000_000n;
  const capture = createCapture({ level: "detailed", output: sink, clock: () => time++ });
  const observer = capture.observer(categories.hostFiles);
  observer.scope(
    "open",
    { path: "game.ras", ok: true, offset: 0x1_0000_0001n, delta: -5n, ratio: 0.5 },
    () => {
      observer.scope("read", {}, () => observer.event("read.completed"));
    },
  );
  observer.counter("readBytes", 4096n);
  observer.counter("milliseconds", 0.5);
  await capture.finish("complete");
  const parsed = packets(sink.chunks);
  const snapshot = fields(get(parsed[0].packet, 6));
  assert.equal(get(snapshot, 2), 11n);
  const realtime = fields(get(snapshot, 1));
  assert.equal(get(realtime, 1), 1n);
  assert.equal(get(realtime, 2), 1_700_000_000_000_000_000n);
  const traceFile = fields(snapshot.filter((field) => field.field === 1)[1].value);
  assert.equal(get(traceFile, 1), 11n);
  assert.equal(get(traceFile, 2), get(realtime, 2));
  for (const { packet } of parsed) {
    if (get(packet, 6) === undefined) {
      assert.equal(get(packet, 58), 1n);
      assert.ok(get(packet, 8) >= 1_700_000_000_000_000_000n);
    }
    assert.equal(get(packet, 10), BigInt(capture.status().producerId));
  }
  const events = parsed.flatMap((value) => (value.event ? [value.event] : []));
  assert.deepEqual(
    events.slice(0, 5).map((value) => get(value, 9)),
    [1n, 1n, 3n, 2n, 2n],
  );
  const annotations = events[0]
    .filter((value) => value.field === 4)
    .map((value) => fields(value.value));
  const byName = new Map(annotations.map((value) => [text(get(value, 10)), value]));
  assert.equal(text(get(byName.get("path"), 6)), "game.ras");
  assert.equal(get(byName.get("ok"), 2), 1n);
  assert.equal(get(byName.get("offset"), 3), 0x1_0000_0001n);
  assert.equal(BigInt.asIntN(64, get(byName.get("delta"), 4)), -5n);
  assert.equal(get(byName.get("ratio"), 5), 0.5);
  const counters = events.filter((value) => get(value, 9) === 4n);
  assert.equal(get(counters[0], 30), 4096n);
  assert.equal(get(counters[1], 44), 0.5);
  const descriptors = parsed.flatMap((value) => (value.descriptor ? [value.descriptor] : []));
  for (const counter of counters)
    assert.ok(
      descriptors.some(
        (value) => get(value, 1) === get(counter, 11) && get(value, 8) !== undefined,
      ),
    );
});

test("catalog parent selection includes descendants and detailed observers require detailed level", async () => {
  const sink = memoryOutput();
  const capture = createCapture({
    level: "basic",
    categories: [categories.engineCpu],
    output: sink,
  });
  capture.observer(categories.engineCpu).event("selected");
  capture.observer(categories.engineCpuCache).event("child");
  capture.observer(categories.engineLoader).event("sibling");
  capture.observer(categories.engine).event("parent");
  capture.observer(categories.engineCpu, "detailed").event("detailed-only");
  await capture.finish("complete");
  const names = packets(sink.chunks).flatMap((value) =>
    value.event && get(value.event, 23) ? [text(get(value.event, 23))] : [],
  );
  assert.deepEqual(names, ["selected", "child", "capture.finished"]);
});

test("parallel async operations retain separate tracks across awaits and scoped child calls", async () => {
  const sink = memoryOutput();
  const capture = createCapture({ level: "detailed", output: sink });
  const observer = capture.observer(categories.hostFiles);
  let releaseFirst;
  const firstGate = new Promise((resolve) => {
    releaseFirst = resolve;
  });
  const first = observer.scopeAsync("first", {}, async (scoped) => {
    await firstGate;
    scoped.scope("first-child", {}, () => {});
  });
  await observer.scopeAsync("second", {}, async (scoped) => {
    scoped.scope("second-child", {}, () => {});
    releaseFirst();
  });
  await first;
  assert.equal((await capture.finish("complete")).complete, true);
  const events = packets(sink.chunks).flatMap((value) => (value.event ? [value.event] : []));
  const named = new Map(
    events.filter((value) => get(value, 23)).map((value) => [text(get(value, 23)), value]),
  );
  const firstTrack = get(named.get("first"), 11);
  const secondTrack = get(named.get("second"), 11);
  assert.notEqual(firstTrack, secondTrack);
  assert.equal(get(named.get("first-child"), 11), firstTrack);
  assert.equal(get(named.get("second-child"), 11), secondTrack);
  for (const track of [firstTrack, secondTrack]) {
    assert.deepEqual(
      events.filter((value) => get(value, 11) === track).map((value) => get(value, 9)),
      [1n, 1n, 2n, 2n],
    );
  }
});

test("producer namespaces are disjoint and concurrent explicit reuse is rejected", async () => {
  const first = createCapture({ level: "basic", output: memoryOutput(), producerId: 100 });
  const second = createCapture({ level: "basic", output: memoryOutput() });
  assert.notEqual(first.status().producerId, second.status().producerId);
  assert.throws(
    () => createCapture({ level: "basic", output: memoryOutput(), producerId: 100 }),
    /already active/,
  );
  await first.finish("complete");
  await second.finish("complete");
});
