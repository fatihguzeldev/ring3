import assert from "node:assert/strict";
import { test } from "node:test";
import { categories } from "../../../observability/typescript/dist/index.js";
import { Engine } from "../engine.js";
import { WorkerCapture } from "../observability.js";

test("the worker rejects unknown category IDs before allocating a capture", () => {
  assert.throws(
    () => new WorkerCapture({ level: "basic", categories: ["unknown.category"] }, () => {}),
    /unknown or nonselectable observation category/,
  );
});

test("the engine adapter distinguishes all, none and selected category wire values", () => {
  const configured = [];
  const engine = new Engine(
    {
      configure_observability: (...args) => {
        configured.push(args);
        return 0;
      },
    },
    new WebAssembly.Memory({ initial: 1 }),
    "fixture",
  );
  const port = engine.observabilityPort();
  for (const selection of [undefined, [], [categories.engineCpu]]) {
    port.configure({
      level: "basic",
      maxBytes: 4096,
      sequenceId: 3,
      trackUuid: 0x3_0000_0001n,
      categories: selection,
    });
    const length = configured.at(-1).at(-1);
    const text = new TextDecoder().decode(new Uint8Array(engine.memory.buffer, 140, length));
    assert.equal(text, selection === undefined ? "" : selection.length === 0 ? "-" : "engine.cpu");
  }
  assert.throws(
    () =>
      port.configure({
        level: "basic",
        maxBytes: 4096,
        sequenceId: 3,
        trackUuid: 1n,
        categories: ["unknown.category"],
      }),
    /unknown or nonselectable observation category/,
  );
});
