import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const expectedNode = readFileSync(join(root, ".node-version"), "utf8").trim();
assert.equal(process.versions.node, expectedNode, "repository Node version is required");

function command(program, args) {
  const result = spawnSync(program, args, { cwd: root, stdio: "inherit" });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${program} failed (${result.signal ?? result.status})`);
}

const harness = join(root, "tools/core/wasm-smoke.rs");
const target = join(root, "target");
command("rustfmt", ["--edition", "2024", "--check", harness]);
command("cargo", [
  "build", "--workspace", "--locked", "--release", "--target", "wasm32-unknown-unknown",
  "--target-dir", target,
]);
const release = join(target, "wasm32-unknown-unknown/release");
const library = join(release, "libring3_core.rlib");
const output = join(mkdtempSync(join(target, "wasm-smoke-")), "smoke.wasm");
command("rustc", [
  "--edition", "2024", "--crate-type", "cdylib", "--target", "wasm32-unknown-unknown",
  "-C", "opt-level=3", "-C", "panic=abort", "-D", "warnings",
  "--extern", `ring3_core=${library}`, "-L", `dependency=${join(release, "deps")}`,
  harness, "-o", output,
]);

const bytes = readFileSync(output);
const module = await WebAssembly.compile(bytes);
assert.deepEqual(WebAssembly.Module.imports(module), [], "smoke must have no host imports");
const instance = await WebAssembly.instantiate(module);
assert.equal(typeof instance.exports.run, "function");
for (let call = 0; call < 2; call += 1) {
  assert.equal(instance.exports.run(), 0x52330001, "all core smoke assertions must finish");
}
const sha256 = (value) => createHash("sha256").update(value).digest("hex");
console.log(JSON.stringify({
  node: process.versions.node,
  imports: 0,
  calls: 2,
  sentinel: "0x52330001",
  output,
  wasmSha256: sha256(bytes),
  harnessSha256: sha256(readFileSync(harness)),
  librarySha256: sha256(readFileSync(library)),
}));
