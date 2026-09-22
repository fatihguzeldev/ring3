import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { runWasmSmoke } from "./run-wasm.mjs";

const root = fileURLToPath(new URL("../../", import.meta.url));
const expectedNode = readFileSync(join(root, ".node-version"), "utf8").trim();
assert.equal(process.versions.node, expectedNode, "repository Node version is required");
assert.ok(process.argv.length === 2 || (process.argv.length === 3 && ["--dll-demo", "--windows-demo"].includes(process.argv[2])),
  "usage: verify-wasm.mjs [--dll-demo|--windows-demo]");
const compiledDll = process.argv[2] === "--dll-demo";
const compiledWindows = process.argv[2] === "--windows-demo";

function command(program, args) {
  const result = spawnSync(program, args, { cwd: root, stdio: "inherit" });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${program} failed (${result.signal ?? result.status})`);
}

const harness = join(root, "tools/core/wasm-smoke.rs");
if (compiledDll) command(process.execPath, ["tools/corpus/build-guest-dll.mjs"]);
if (compiledWindows) {
  command(process.execPath, ["tools/corpus/build-windows-api.mjs"]);
  command(process.execPath, ["tools/corpus/build-d3d8-frame.mjs"]);
}
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
  ...(compiledDll ? ["--cfg", "guest_dll_demo"] : []),
  ...(compiledWindows ? ["--cfg", "windows_demo"] : []),
  harness, "-o", output,
]);

const smoke = runWasmSmoke(output);
const sha256 = (value) => createHash("sha256").update(value).digest("hex");
console.log(JSON.stringify({
  ...smoke,
  compiledDll,
  compiledWindows,
  output,
  harnessSha256: sha256(readFileSync(harness)),
  librarySha256: sha256(readFileSync(library)),
}));
