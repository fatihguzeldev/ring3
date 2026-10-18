import assert from "node:assert/strict";
import test from "node:test";
import {
  categories,
  categoryCatalog,
  catalogVersion,
  createCapture,
  encodeCategorySelection,
  resolveCategorySelection,
} from "../../dist/index.js";
import { get, memoryOutput, packets, text } from "../perfetto/decode.mjs";

test("catalog references and descriptors are immutable and retain registered metadata", () => {
  assert.equal(catalogVersion, 1);
  const descriptor = categoryCatalog.find((entry) => entry.id === categories.engineCpuDecode);
  assert.equal(descriptor.parent, categories.engineCpu);
  assert.ok(descriptor.label.length > 0);
  assert.ok(descriptor.description.length > 0);
  assert.throws(() => {
    categories.engineCpu = "forged";
  }, TypeError);
  assert.throws(() => {
    descriptor.label = "changed";
  }, TypeError);
});

test("wire selection distinguishes all, none and registered subsets", () => {
  assert.equal(encodeCategorySelection(undefined), "");
  assert.equal(encodeCategorySelection([]), "-");
  assert.equal(
    encodeCategorySelection([categories.engineCpu, categories.hostFiles]),
    "engine.cpu\nhost.files",
  );
  assert.deepEqual(resolveCategorySelection(["engine.cpu"]), [categories.engineCpu]);
});

test("forged, duplicate and internal category selections fail explicitly", () => {
  const output = memoryOutput();
  for (const selection of [["engine.cpus"], ["observability"], ["engine", "engine"]]) {
    assert.throws(() => resolveCategorySelection(selection), /category/);
    assert.throws(
      () => createCapture({ level: "basic", categories: selection, output }),
      /category/,
    );
  }
  const capture = createCapture({ level: "basic", output });
  assert.throws(() => capture.observer("engine.cpus"), /unregistered/);
  assert.throws(() => capture.recordEvent("engine.cpus", "forged"), /unregistered/);
  assert.throws(() => capture.recordCounter("engine.cpus", "forged", 1), /unregistered/);
  assert.throws(() => createCapture({ level: "basic", contexts: [], output }), /catalog/);
  return capture.finish("complete");
});

test("none records control metadata only while all includes component events", async () => {
  for (const selection of [undefined, []]) {
    const output = memoryOutput();
    const capture = createCapture({ level: "basic", categories: selection, output });
    capture.observer(categories.engineCpuDecode).event("decoded");
    capture.observer(categories.hostFiles).event("read");
    await capture.finish("complete");
    const names = packets(output.chunks).flatMap((value) =>
      value.event && get(value.event, 23) ? [text(get(value.event, 23))] : [],
    );
    assert.deepEqual(
      names,
      selection === undefined ? ["decoded", "read", "capture.finished"] : ["capture.finished"],
    );
  }
});
