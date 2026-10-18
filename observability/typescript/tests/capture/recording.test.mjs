import assert from "node:assert/strict";
import test from "node:test";
import { createCapture } from "../../dist/index.js";
import { get, packets } from "../perfetto/decode.mjs";

function output() {
  const chunks = [];
  return {
    chunks,
    async write(bytes) {
      chunks.push(bytes.slice());
    },
    async flush() {},
    async close() {},
  };
}

test("off recording does not read clocks or write output and preserves results", async () => {
  const sink = output();
  const capture = createCapture({
    level: "off",
    output: sink,
    clock() {
      throw Error("clock must not run");
    },
  });
  const observer = capture.observer("cpu.dbt");
  assert.equal(
    observer.scope("decode", {}, () => 42),
    42,
  );
  assert.equal(await observer.scopeAsync("compile", {}, async () => 7), 7);
  observer.event("done");
  observer.counter("units", 3);
  assert.equal((await capture.finish("complete")).complete, true);
  assert.equal(sink.chunks.length, 0);
});

test("application throw and rejected promise retain their exact identity", async () => {
  const capture = createCapture({ level: "detailed", output: output() });
  const observer = capture.observer("files");
  const error = new Error("game failure");
  assert.throws(
    () =>
      observer.scope("read", {}, () => {
        throw error;
      }),
    (value) => value === error,
  );
  await assert.rejects(
    observer.scopeAsync("load", {}, async () => {
      throw error;
    }),
    (value) => value === error,
  );
  assert.equal((await capture.finish("failed", error)).outcome, "failed");
});

test("instrumentation preserves throwing undefined and rejecting non-Error values", async () => {
  const capture = createCapture({ level: "basic", output: output() });
  const observer = capture.observer("process");
  let threw = false;
  try {
    observer.scope("start", {}, () => {
      throw undefined;
    });
  } catch (error) {
    threw = true;
    assert.equal(error, undefined);
  }
  assert.equal(threw, true);
  await assert.rejects(
    observer.scopeAsync("load", {}, async () => {
      throw 0;
    }),
    (error) => error === 0,
  );
  assert.equal((await capture.finish("failed")).complete, true);
});

test("sink failures cannot replace application errors and make capture incomplete", async () => {
  const failure = new Error("disk unavailable");
  const capture = createCapture({
    level: "basic",
    output: {
      async write() {
        throw failure;
      },
      async flush() {},
    },
    batchBytes: 1,
  });
  const gameError = new Error("application error");
  assert.throws(
    () =>
      capture.observer("process").scope("start", {}, () => {
        throw gameError;
      }),
    (value) => value === gameError,
  );
  const result = await capture.finish("failed", gameError);
  assert.equal(result.complete, false);
  assert.match(result.sinkError, /disk unavailable/);
});

test("slow output is bounded and dropped events remain visible", async () => {
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const capture = createCapture({
    level: "basic",
    batchBytes: 256,
    maxBufferedBytes: 2048,
    output: {
      async write() {
        await gate;
      },
      async flush() {},
    },
  });
  const observer = capture.observer("cpu");
  for (let index = 0; index < 1000; index++) observer.event("step", { index });
  assert.ok(capture.status().droppedEvents > 0);
  assert.ok(capture.status().bufferedBytes <= 2048);
  release();
  const result = await capture.finish("complete");
  assert.equal(result.complete, false);
  assert.ok(result.droppedEvents > 0);
});

test("opaque chunk losses are distinct from known event drops and admitted bytes are copied", async () => {
  const sink = output();
  const capture = createCapture({
    level: "basic",
    output: sink,
    maxBufferedBytes: 1024,
    batchBytes: 1024,
  });
  const bytes = Uint8Array.of(10, 2, 80, 9);
  assert.equal(capture.ingest(bytes), true);
  bytes.fill(0);
  assert.equal(capture.ingest(new Uint8Array(2048)), false);
  const result = await capture.finish("cancelled");
  assert.equal(result.complete, false);
  assert.equal(result.droppedChunks, 1);
  assert.equal(result.droppedEvents, 0);
  assert.ok(packets(sink.chunks).some((value) => get(value.packet, 10) === 9n));
});

test("terminal capture is idempotent and cannot write records after output closes", async () => {
  const sink = output();
  let closed = false;
  sink.close = async () => {
    closed = true;
  };
  sink.write = async (bytes) => {
    assert.equal(closed, false);
    sink.chunks.push(bytes.slice());
  };
  sink.flush = async () => {
    assert.equal(closed, false);
  };
  const capture = createCapture({ level: "basic", output: sink });
  const observer = capture.observer("process");
  observer.event("start");
  const first = capture.finish("cancelled");
  const second = capture.finish("failed");
  assert.equal(first, second);
  assert.equal((await first).outcome, "cancelled");
  const count = sink.chunks.length;
  observer.event("late");
  capture.recordEvent("process", "late-direct");
  assert.equal(capture.ingest(Uint8Array.of(1)), false);
  await capture.flush();
  assert.equal(sink.chunks.length, count);
});

test("incomplete records flush their terminal event and abort instead of marking output complete", async () => {
  const sink = output();
  const operations = [];
  sink.flush = async () => {
    operations.push("flush");
  };
  sink.close = async () => {
    operations.push("close");
  };
  sink.abort = async () => {
    operations.push("abort");
  };
  const capture = createCapture({ level: "basic", output: sink });
  capture.markIncomplete("worker terminated");
  const result = await capture.finish("cancelled");
  assert.equal(result.complete, false);
  assert.equal(operations.includes("close"), false);
  assert.equal(operations.at(-1), "abort");
  assert.ok(sink.chunks.length > 0);
});

test("unprintable output exceptions cannot become unhandled rejections or replace game errors", async () => {
  const sinkError = Object.create(null, {
    toString: {
      value() {
        throw new Error("cannot stringify");
      },
    },
  });
  const capture = createCapture({
    level: "basic",
    output: {
      async write() {
        throw sinkError;
      },
      async flush() {},
    },
  });
  const gameError = new Error("game");
  assert.throws(
    () =>
      capture.observer("process").scope("start", {}, () => {
        throw gameError;
      }),
    (error) => error === gameError,
  );
  const result = await capture.finish("failed", gameError);
  assert.equal(result.complete, false);
  assert.equal(result.sinkError, "unprintable output error");
});
