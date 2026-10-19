import { createHash, randomUUID } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { access, copyFile, mkdir, readFile, readdir, rename, writeFile } from "node:fs/promises";
import { constants } from "node:fs";
import { delimiter, dirname, isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const sdk = fileURLToPath(new URL("../", import.meta.url));
const lock = JSON.parse(await readFile(new URL("../backend.lock.json", import.meta.url), "utf8"));
const options = {};
for (let i = 2; i < process.argv.length; i += 2) {
  const name = process.argv[i];
  if (!["--directory", "--bun", "--bridge"].includes(name) || !process.argv[i + 1] || name in options) {
    throw new Error("usage: prepare-backend.mjs --directory <cache> --bun <executable> [--bridge <patch>]");
  }
  options[name] = process.argv[i + 1];
}
const directory = resolve(options["--directory"] ?? "target/browser-runtime");
const bridge = resolve(options["--bridge"] ?? resolve(sdk, "backend/bridge.patch"));
const bunArgument = options["--bun"] ?? "bun";
const bun = /[/\\]/.test(bunArgument) ? resolve(bunArgument) : bunArgument;
const environment = isAbsolute(bun)
  ? { ...process.env, PATH: `${dirname(bun)}${delimiter}${process.env.PATH ?? ""}` }
  : process.env;
const sha = bytes => createHash("sha256").update(bytes).digest("hex");
const exists = async path => access(path, constants.F_OK).then(() => true, error => {
  if (error.code === "ENOENT") return false;
  throw error;
});
const git = args => execFileSync("git", ["-C", directory, ...args], { encoding: "utf8" }).trim();
const patchedFiles = async () => Object.fromEntries(await Promise.all(patchedPaths.map(async path => [path, sha(await readFile(resolve(directory, path)))])));
async function runtimeArtifacts(root, prefix = "") {
  const files = {};
  for (const entry of await readdir(resolve(root, prefix), { withFileTypes: true })) {
    const path = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) Object.assign(files, await runtimeArtifacts(root, path));
    else if (/\.(?:[cm]?js|wasm)$/.test(path)) files[path] = sha(await readFile(resolve(root, path)));
  }
  return files;
}

const bridgeBytes = await readFile(bridge);
const patchedPaths = [...bridgeBytes.toString("utf8").matchAll(/^\+\+\+ b\/(.+)$/gm)].map(match => match[1]);
if (!patchedPaths.length || patchedPaths.some(path => isAbsolute(path) || path.split(/[\/\\]/).includes(".."))) {
  throw new Error("SDK bridge must contain only checkout-relative paths");
}
if (!await exists(directory)) {
  await mkdir(dirname(directory), { recursive: true });
  execFileSync("git", ["clone", "--filter=blob:none", "--no-checkout", "--depth", "1", lock.source, directory], { stdio: "inherit" });
  if (git(["rev-parse", "HEAD"]) !== lock.revision) {
    git(["fetch", "--depth", "1", "origin", lock.revision]);
  }
  git(["sparse-checkout", "set", "src", "packages", "tools", "deploy", "docs", "public"]);
  git(["checkout", "--detach", lock.revision]);
}
if (git(["rev-parse", "HEAD"]) !== lock.revision) {
  throw new Error("existing backend checkout has another revision; choose a fresh cache directory");
}
for (const path of ["src/app/App.tsx", "deploy/wgb-etag.ts", "bun.lock"]) {
  await access(resolve(directory, path));
}

// Use the original pinned CPU package/artifacts, not the combined CPU experiment.
for (const [path, expected] of Object.entries(lock.cpuFiles)) {
  const destination = resolve(directory, "vendor/v86", path);
  if (!await exists(destination)) {
    const url = `https://raw.githubusercontent.com/jenissimo/v86/${lock.cpuRevision}/${path}`;
    const response = await fetch(url);
    if (!response.ok) throw new Error(`CPU artifact HTTP ${response.status}: ${url}`);
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (sha(bytes) !== expected) throw new Error(`CPU artifact hash mismatch: ${path}`);
    await mkdir(dirname(destination), { recursive: true });
    await writeFile(destination, bytes);
  }
  if (sha(await readFile(destination)) !== expected) throw new Error(`CPU artifact changed: ${path}`);
}
if (sha(await readFile(resolve(directory, "public/v86.wasm"))) !== lock.cpuWasmSha256) {
  throw new Error("backend CPU Wasm does not match its pinned artifact");
}

const checkPatch = reverse => spawnSync("git", ["-C", directory, "apply", ...(reverse ? ["--reverse"] : []), "--check", bridge], { encoding: "utf8" });
if (checkPatch(false).status === 0) git(["apply", bridge]);
else if (checkPatch(true).status !== 0) throw new Error("SDK bridge does not fit the pinned source; no reset performed");

const client = resolve(directory, "public/ring3-sdk");
await mkdir(client, { recursive: true });
for (const path of ["runtime.mjs", "runtime.d.mts", "tests/acceptance.html", "tests/acceptance.mjs", "tests/vertical-acceptance.mjs"]) {
  await copyFile(resolve(sdk, path), resolve(client, path.split("/").at(-1)));
}

const metadata = {
  backend: "bottleship-v86",
  upstreamRevision: lock.revision,
  cpuRevision: lock.cpuRevision,
  cpuJavaScriptSha256: lock.cpuFiles["build/libv86.mjs"],
  servedCpuWasmSha256: lock.cpuWasmSha256,
  bridgeSha256: sha(bridgeBytes),
  sourceDiffSha256: sha(git(["diff", "--binary"])),
  patchedFiles: await patchedFiles(),
  sourceIdentityScope: "pinned base, tracked local diff, patched files and emitted JavaScript/Wasm hashes; local edits are not reset",
  cpuBuild: "original pinned upstream artifact",
  buildTools: { node: process.version, bun: execFileSync(bun, ["--version"], { env: environment, encoding: "utf8" }).trim() },
};
await writeFile(resolve(directory, "public/ring3-runtime-build.json"), JSON.stringify(metadata, null, 2) + "\n");
execFileSync(bun, ["install", "--frozen-lockfile", "--ignore-scripts"], { cwd: directory, env: environment, stdio: "inherit" });
if (sha(await readFile(resolve(directory, "node_modules/v86/build/libv86.mjs"))) !== lock.cpuFiles["build/libv86.mjs"]) {
  throw new Error("installed CPU JavaScript does not match its pinned artifact");
}
// Upstream retains dist contents. Preserve any old output instead of mixing it
// with this build or deleting potentially valuable local files.
if (await exists(resolve(directory, "dist"))) {
  const previous = `dist.previous-${randomUUID()}`;
  await rename(resolve(directory, "dist"), resolve(directory, previous));
  metadata.previousDistribution = previous;
}
execFileSync(bun, ["run", "build"], { cwd: directory, env: environment, stdio: "inherit" });
metadata.sourceDiffSha256 = sha(git(["diff", "--binary"]));
metadata.patchedFiles = await patchedFiles();
metadata.artifacts = await runtimeArtifacts(resolve(directory, "dist"));
if (metadata.artifacts["v86.wasm"] !== lock.cpuWasmSha256) {
  throw new Error("emitted CPU Wasm does not match its pinned artifact");
}
await writeFile(resolve(directory, "public/ring3-runtime-build.json"), JSON.stringify(metadata, null, 2) + "\n");
await copyFile(resolve(directory, "public/ring3-runtime-build.json"), resolve(directory, "dist/ring3-runtime-build.json"));
const licenses = resolve(directory, "dist/licenses");
await mkdir(licenses, { recursive: true });
for (const [source, name] of [["LICENSE", "bottleship-LICENSE"], ["THIRD-PARTY-NOTICES.md", "bottleship-THIRD-PARTY-NOTICES.md"], ["vendor/v86/LICENSE", "v86-LICENSE"]]) {
  await copyFile(resolve(directory, source), resolve(licenses, name));
}
console.log(JSON.stringify({ ...metadata, sourceDirectory: directory, distributionDirectory: resolve(directory, "dist") }, null, 2));
