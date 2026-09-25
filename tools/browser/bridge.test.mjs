import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { Bridge } from "../../runtime/dist/browser/bridge.js";

const fixture = spawnSync("cargo", ["run", "--quiet", "-p", "ring3-browser", "--example", "browser_fixture"], { encoding: "utf8", timeout: 120_000 });
assert.equal(fixture.status, 0, fixture.stderr);
const executable = Uint8Array.from(JSON.parse(fixture.stdout));
const wasm = readFileSync(new URL("../../target/wasm32-unknown-unknown/release/ring3_browser.wasm", import.meta.url));
const module = await WebAssembly.compile(wasm);

test("actual browser Wasm resumes exact file requests across budgets and memory growth", async () => {
  assert.deepEqual(WebAssembly.Module.imports(module), []);
  let expected;
  for (const budget of [1, 5, 100_000]) {
    const instance = await WebAssembly.instantiate(module, {});
    const bridge = new Bridge(instance);
    assert.throws(() => bridge.command(99), /unknown operation/);
    bridge.command(0, 0, { files: [
      { path: "C:\\sample.exe", size: executable.length, role: "executable" },
      { path: "C:\\a.bin", size: 4, role: "data" },
    ] });
    assert.throws(() => bridge.command(1, 0, new Uint8Array(1)), /invalid module/);
    bridge.command(1, 0, executable);
    bridge.command(2);
    const oldBuffer = instance.exports.memory.buffer;
    instance.exports.memory.grow(1);
    assert.equal(oldBuffer.byteLength, 0);
    let snapshot;
    for (let index = 0; index < 200; index++) {
      snapshot = bridge.command(3, budget, { elapsedMs: 100 });
      if (snapshot.state === "file") break;
      assert.equal(snapshot.state, "running");
    }
    assert.equal(snapshot.pending.path, "C:\\a.bin");
    const input = new Uint8Array(12);
    new DataView(input.buffer).setBigUint64(0, BigInt(snapshot.pending.id) + 1n, true);
    input.set(new TextEncoder().encode("data"), 8);
    assert.throws(() => bridge.command(4, 0, input), /StaleRequest/);
    assert.deepEqual(bridge.command(6), snapshot);
    new DataView(input.buffer).setBigUint64(0, BigInt(snapshot.pending.id), true);
    assert.throws(() => bridge.command(4, 0, input.slice(0, 8)), /SizeMismatch/);
    bridge.command(4, 0, input);
    for (let index = 0; index < 200; index++) {
      snapshot = bridge.command(3, budget, { elapsedMs: 100 });
      if (snapshot.state !== "running") break;
    }
    assert.equal(snapshot.reason, "Some(Exited(42))");
    assert.equal(bridge.frame(snapshot), null);
    if (expected) assert.deepEqual(snapshot, expected);
    expected = snapshot;
  }
});
