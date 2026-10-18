import assert from "node:assert/strict";
import test from "node:test";
import { createCapture } from "../../dist/index.js";
import { get, memoryOutput, packets } from "../perfetto/decode.mjs";

test("unsupported detailed engine observation fails clearly; basic labels host-only coverage", async () => {
  const unsupported = {
    supported: false,
    configure() {
      throw Error("unreachable");
    },
  };
  const basic = createCapture({ level: "basic", output: memoryOutput() });
  basic.attachEngine(unsupported, { producerId: 90 });
  assert.equal(basic.status().engine, "unsupported");
  await basic.finish("complete");
  const detailed = createCapture({ level: "detailed", output: memoryOutput() });
  assert.throws(
    () => detailed.attachEngine(unsupported, { producerId: 90 }),
    /observability-enabled engine/,
  );
  assert.equal((await detailed.finish("failed")).complete, false);
});

test("engine configuration owns a separate producer namespace and finish drains original byte chunks", async () => {
  const sink = memoryOutput();
  let configuration;
  let index = 0;
  const chunks = [Uint8Array.of(10, 2, 80, 12), Uint8Array.of(10, 2, 80, 12)];
  const port = {
    supported: true,
    configure(value) {
      configuration = value;
    },
    drain() {
      return { bytes: chunks[index++], finished: index === 2, droppedEvents: 2n };
    },
    finish() {
      return this.drain();
    },
  };
  const capture = createCapture({
    level: "basic",
    contexts: ["files"],
    output: sink,
    producerId: 11,
  });
  capture.attachEngine(port, { producerId: 12 });
  assert.equal(configuration.sequenceId, 12);
  assert.equal(configuration.trackUuid, (12n << 32n) | 1n);
  assert.deepEqual(configuration.contexts, ["files"]);
  const result = await capture.finish("complete");
  assert.equal(result.engine, "finished");
  assert.equal(result.droppedEvents, 2);
  assert.equal(result.complete, false);
  assert.equal(packets(sink.chunks).filter((value) => get(value.packet, 10) === 12n).length, 2);
});

test("external truncation and unfinished async work make a terminal capture incomplete", async () => {
  const capture = createCapture({ level: "basic", output: memoryOutput() });
  let finishWork;
  const pending = capture.observer("process").scopeAsync(
    "worker",
    {},
    () =>
      new Promise((resolve) => {
        finishWork = resolve;
      }),
  );
  capture.markIncomplete("worker terminated before trace flush");
  const result = await capture.finish("cancelled");
  assert.equal(result.complete, false);
  assert.equal(result.openScopes, 1);
  assert.match(result.recordingError, /worker terminated/);
  finishWork();
  await pending;
  assert.equal(capture.status().bufferedBytes, 0);
});

test("terminal engine scope status cannot falsely claim a complete trace", async () => {
  const capture = createCapture({ level: "basic", output: memoryOutput() });
  capture.attachEngine(
    {
      supported: true,
      configure() {},
      finish() {
        return { bytes: new Uint8Array(), finished: true, droppedEvents: 0n, openScopes: 1n };
      },
      drain() {
        throw Error("already terminal");
      },
    },
    { producerId: 500 },
  );
  const result = await capture.finish("failed");
  assert.equal(result.complete, false);
  assert.match(result.recordingError, /1 unfinished scopes/);
});

test("failed engine configuration releases its producer identity", async () => {
  const capture = createCapture({ level: "basic", output: memoryOutput() });
  assert.throws(
    () =>
      capture.attachEngine(
        {
          supported: true,
          configure() {
            throw Error("configuration rejected");
          },
        },
        { producerId: 600 },
      ),
    /configuration rejected/,
  );
  const other = createCapture({ level: "basic", output: memoryOutput(), producerId: 600 });
  assert.equal((await capture.finish("failed")).complete, false);
  await other.finish("complete");
});
