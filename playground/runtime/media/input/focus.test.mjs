import assert from "node:assert/strict";
import { test } from "node:test";
import { Media } from "../../media.js";

function fixture() {
  let failure;
  const submissions = [];
  const engine = {
    request: (bytes) => submissions.push(bytes.slice()),
    ok(operation) {
      if (operation === "submit_windows_input" && failure) throw failure;
    },
    media: () => ({ values: [0n] }),
  };
  const media = new Media(engine, 7n, () => {});
  media.audio = {};
  media.publishDialogs = () => {};
  media.publishGdi = () => {};
  return {
    media,
    submissions,
    fail: (error) => {
      failure = error;
    },
  };
}

test("the latest successfully submitted physical focus has its real batch sequence and time", () => {
  const { media, submissions } = fixture();
  media.enqueue([{ kind: "focus", focused: true }]);
  media.update(12n);
  assert.deepEqual(media.lastPhysicalFocus, { focused: true, sequence: "1", timestampNs: "12" });
  assert.equal(new DataView(submissions[0].buffer).getBigUint64(16, true), 1n);
  media.enqueue([
    { kind: "focus", focused: true },
    { kind: "focus", focused: false },
  ]);
  media.update(19n);
  assert.deepEqual(media.lastPhysicalFocus, { focused: false, sequence: "2", timestampNs: "19" });
});

test("a rejected physical focus batch never replaces the last accepted focus diagnostic", () => {
  const f = fixture();
  f.media.enqueue([{ kind: "focus", focused: true }]);
  f.media.update(12n);
  const previous = f.media.lastPhysicalFocus;
  const failure = new Error("input submission rejected");
  f.fail(failure);
  f.media.enqueue([{ kind: "focus", focused: false }]);
  assert.throws(
    () => f.media.update(19n),
    (error) => error === failure,
  );
  assert.equal(f.media.lastPhysicalFocus, previous);
});

test("keyboard and pointer batches do not invent physical focus", () => {
  const { media } = fixture();
  media.enqueue([
    { kind: "key", code: 30, down: true },
    { kind: "pointer", x: 3, y: 4 },
  ]);
  media.update(12n);
  assert.equal(media.lastPhysicalFocus, undefined);
});
