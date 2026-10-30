import assert from "node:assert/strict";
import test from "node:test";
import { createDiagnostics } from "./diagnostics.js";

const turn = () => new Promise(setImmediate);

test("1000 Present diagnostics leave the main UI and HTTP quiet while errors and status survive", async () => {
  const writes = [], requests = [];
  const diagnostics = createDiagnostics((...args) => writes.push(args), {
    fetch: (_url, request) => { requests.push(JSON.parse(request.body)); return Promise.resolve(); },
  });
  for (let i = 0; i < 1000; i++) diagnostics.log(JSON.stringify({ type: "present", presents: i }));
  diagnostics.log({ type: "present", presents: 1001 });
  assert.equal(writes.length, 0);
  assert.equal(requests.length, 0);
  diagnostics.log("graphics failed", { message: "device lost" });
  diagnostics.report({ type: "status", state: "error" });
  assert.deepEqual(writes, [["graphics failed", { message: "device lost" }]]);
  await turn();
  assert.deepEqual(requests.map(value => value.type), ["diagnostic", "status"]);
});

test("a stalled diagnostics server retains one request and only the latest bounded FIFO", async () => {
  const requests = [], releases = [];
  const diagnostics = createDiagnostics(() => {}, {
    fetch: (_url, request) => {
      requests.push(JSON.parse(request.body));
      return new Promise(resolve => releases.push(resolve));
    },
  });
  for (let i = 0; i < 1000; i++) diagnostics.report({ type: "status", index: i });
  assert.equal(requests.length, 1);
  for (let i = 0; i < 17; i++) { releases[i](); await turn(); }
  assert.deepEqual(requests.map(value => value.index), [0, ...Array.from({ length: 16 }, (_, i) => 984 + i)]);
});

test("timeout aborts a stalled fetch before the next queued request starts", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const signals = [];
  let active = 0, maximum = 0;
  const diagnostics = createDiagnostics(() => {}, {
    timeoutMs: 5000,
    fetch: (_url, { signal }) => new Promise((_resolve, reject) => {
      signals.push(signal); active++; maximum = Math.max(maximum, active);
      signal?.addEventListener("abort", () => { active--; reject(signal.reason); }, { once: true });
    }),
  });
  diagnostics.report({ type: "status", index: 0 });
  diagnostics.report({ type: "status", index: 1 });
  t.mock.timers.tick(5000); await turn();
  assert.equal(signals[0]?.aborted, true);
  assert.equal(signals.length, 2);
  assert.equal(maximum, 1);
  t.mock.timers.tick(5000); await turn();
  assert.equal(signals[1].aborted, true);
  assert.equal(active, 0);
});
