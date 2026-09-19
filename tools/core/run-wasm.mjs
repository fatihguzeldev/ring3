import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

export function runWasmSmoke(file, timeoutMs = 15_000) {
  assert.ok(Number.isSafeInteger(timeoutMs) && timeoutMs > 0, "timeout must be a positive safe integer");
  const result = spawnSync(process.execPath, [fileURLToPath(import.meta.url), file], {
    encoding: "utf8",
    timeout: timeoutMs,
    killSignal: "SIGKILL",
    maxBuffer: 1024 * 1024,
  });
  if (result.error || result.signal || result.status !== 0) {
    const message = result.error?.code === "ETIMEDOUT"
      ? `Wasm smoke timed out after ${timeoutMs} ms`
      : `Wasm smoke failed (${result.error?.code ?? result.signal ?? result.status}): ${result.stderr?.trim() ?? ""}`;
    throw new Error(message, { cause: result });
  }
  const report = JSON.parse(result.stdout);
  assert.deepEqual(report, {
    node: process.versions.node,
    imports: 0,
    calls: 2,
    sentinel: "0x52330001",
    wasmSha256: sha256(readFileSync(file)),
  }, "Wasm smoke report must match the checked module");
  return report;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const bytes = readFileSync(process.argv[2]);
  const module = await WebAssembly.compile(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [], "smoke must have no host imports");
  const instance = await WebAssembly.instantiate(module);
  assert.equal(typeof instance.exports.run, "function", "run export must be a function");
  for (let call = 0; call < 2; call += 1) {
    assert.equal(instance.exports.run(), 0x52330001, "all core smoke assertions must finish");
  }
  console.log(JSON.stringify({
    node: process.versions.node,
    imports: 0,
    calls: 2,
    sentinel: "0x52330001",
    wasmSha256: sha256(bytes),
  }));
}
