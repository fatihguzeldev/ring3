import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { runWasmSmoke } from "./run-wasm.mjs";

const root = fileURLToPath(new URL("../../", import.meta.url));
const expectedNode = readFileSync(join(root, ".node-version"), "utf8").trim();
assert.equal(process.versions.node, expectedNode, "repository Node version is required");
assert.ok(
  process.argv.length === 2 ||
    (process.argv.length === 3 && ["--dll-demo", "--windows-demo"].includes(process.argv[2])),
  "usage: verify-wasm.mjs [--dll-demo|--windows-demo]",
);
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
  command(process.execPath, ["tools/corpus/build-crt-rtti.mjs"]);
}
command("rustfmt", ["--edition", "2024", "--check", harness]);
const features = [
  "smoke",
  ...(compiledDll ? ["guest-dll-demo"] : []),
  ...(compiledWindows ? ["windows-demo"] : []),
];
const build = spawnSync(
  "cargo",
  [
    "build",
    "-p",
    "ring3-smoke",
    "--example",
    "core-smoke",
    "--features",
    features.join(","),
    "--locked",
    "--release",
    "--target",
    "wasm32-unknown-unknown",
    "--message-format=json-render-diagnostics",
  ],
  {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
    maxBuffer: 16 * 1024 * 1024,
  },
);
if (build.error) throw build.error;
assert.equal(build.status, 0, `cargo failed (${build.signal ?? build.status})`);
const artifacts = build.stdout
  .split(/\r?\n/)
  .filter(Boolean)
  .map((line) => JSON.parse(line))
  .filter((message) => message.reason === "compiler-artifact");
const smokeArtifact = artifacts.find((message) => message.target.name === "core-smoke");
const coreArtifact = artifacts.find((message) => message.target.name === "ring3_core");
assert.ok(smokeArtifact && coreArtifact, "Cargo must report smoke and core artifacts");
const output = smokeArtifact.filenames.find((path) => path.endsWith(".wasm"));
const library = coreArtifact.filenames.find((path) => path.endsWith(".rlib"));
assert.ok(output && library, "Cargo must produce the Wasm harness and core library");

const smoke = runWasmSmoke(output);
const sha256 = (value) => createHash("sha256").update(value).digest("hex");
console.log(
  JSON.stringify({
    ...smoke,
    compiledDll,
    compiledWindows,
    reused: smokeArtifact.fresh,
    output,
    harnessSha256: sha256(readFileSync(harness)),
    librarySha256: sha256(readFileSync(library)),
  }),
);
