import assert from "node:assert/strict";
import test from "node:test";
import { flattenInventory, parseTestList, verifyTestResult } from "./verify.mjs";

const name = "generated_corpus_matches_recorded_header_metadata";
const success = `\nrunning 1 test\ntest ${name} ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out; finished in 0.00s\n\n`;
const result = (stdout) => ({ status: 0, signal: null, stdout });

test("exact test success refuses zero, skipped, wrong, duplicate and partial reports", () => {
  verifyTestResult(result(success), name);
  const zero = "\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out; finished in 0.00s\n\n";
  for (const stdout of [
    zero,
    success.replace("... ok", "... ignored").replace("1 passed", "0 passed").replace("0 ignored", "1 ignored"),
    success.replace(`test ${name}`, "test another_case"),
    success + success,
    success.split("test result:")[0],
    success + "unexplained output\n",
  ]) assert.throws(() => verifyTestResult(result(stdout), name));
});

test("nonzero, signal and timeout results cannot pass even with successful text", () => {
  for (const exit of [
    { status: 101 },
    { status: null, signal: "SIGTERM" },
    { status: 0, signal: "SIGTERM" },
    { status: null, error: new Error("spawn timed out") },
  ]) assert.throws(() => verifyTestResult({ ...result(success), ...exit }, name));
});

test("ignored-test lists require complete unique names and a matching count", () => {
  assert.deepEqual(parseTestList(`${name}: test\n\n1 test, 0 benchmarks\n`), [name]);
  assert.deepEqual(parseTestList("0 tests, 0 benchmarks\n"), []);
  for (const stdout of [
    `${name}: test\n\n2 tests, 0 benchmarks\n`,
    `${name}: test\n${name}: test\n\n2 tests, 0 benchmarks\n`,
    `${name}: test\n`,
    `${name}: benchmark\n\n0 tests, 1 benchmark\n`,
    `${name}: test\nunexpected\n1 test, 0 benchmarks\n`,
  ]) assert.throws(() => parseTestList(stdout));
});

test("inventory refuses empty, duplicate, unsupported and path-like case names", () => {
  const valid = { schemaVersion: 1, tests: { pe_headers: [name] } };
  assert.deepEqual(flattenInventory(valid), [{ binary: "pe_headers", test: name }]);
  for (const inventory of [
    { schemaVersion: 2, tests: valid.tests },
    { schemaVersion: 1, tests: {} },
    { schemaVersion: 1, tests: { pe_headers: [] } },
    { schemaVersion: 1, tests: { pe_headers: [name, name] } },
    { schemaVersion: 1, tests: { "../outside": [name] } },
    { schemaVersion: 1, tests: { pe_headers: ["../outside"] } },
  ]) assert.throws(() => flattenInventory(inventory));
});
