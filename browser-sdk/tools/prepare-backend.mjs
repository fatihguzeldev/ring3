import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { access, copyFile, lstat, mkdir, readFile, readdir, rename, rm, writeFile } from "node:fs/promises";
import { constants } from "node:fs";
import { delimiter, dirname, isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function applyBridge(directory, patch) {
  const missing = [];
  for (const section of patch.toString().split(/(?=^diff --git )/m).filter(value => value.trim())) {
    const check = reverse => spawnSync("git", ["-C", directory, "apply", ...(reverse ? ["--reverse"] : []), "--check", "-"], { input: section, encoding: "utf8" });
    if (check(false).status === 0) missing.push(section);
    else if (check(true).status !== 0) throw new Error("SDK bridge does not fit the pinned source; no reset performed");
  }
  if (missing.length) execFileSync("git", ["-C", directory, "apply", "-"], { input: missing.join("") });
}

export async function prepareDistribution(directory, build, validate = async () => {}) {
  const distribution = resolve(directory, "dist");
  const previous = resolve(directory, "dist.previous"), staging = resolve(directory, "dist.staging");
  const entry = async path => lstat(path).catch(error => {
    if (error.code === "ENOENT") return null;
    throw error;
  });
  const generated = async path => {
    const info = await entry(path);
    if (!info) return false;
    if (!info.isDirectory() || info.isSymbolicLink()) throw new Error(`unrecognized distribution: ${path}`);
    let metadata;
    try { metadata = JSON.parse(await readFile(resolve(path, "ring3-runtime-build.json"), "utf8")); }
    catch { throw new Error(`unrecognized distribution: ${path}`); }
    if (metadata?.backend !== "bottleship-v86" || metadata.cpuBuild !== "original pinned upstream artifact"
      || !metadata.artifacts || typeof metadata.artifacts !== "object") {
      throw new Error(`unrecognized distribution: ${path}`);
    }
    return true;
  };
  if (await entry(staging)) throw new Error(`staging already exists; no files removed: ${staging}`);
  const hadCurrent = await generated(distribution), hadPrevious = await generated(previous);
  await mkdir(staging);
  let ownsStaging = true;
  try {
    await build(staging);
    await validate(staging);
    if (!await generated(staging)) throw new Error("build did not produce a distribution");
    if (hadPrevious) await rm(previous, { recursive: true });
    if (hadCurrent) await rename(distribution, previous);
    try { await rename(staging, distribution); }
    catch (error) {
      if (hadCurrent) await rename(previous, distribution);
      throw error;
    }
    ownsStaging = false;
    return distribution;
  } finally {
    if (ownsStaging) await rm(staging, { recursive: true, force: true });
  }
}

async function main() {
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
  const directory = resolve(options["--directory"] ?? ".cache/browser-runtime");
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

  // use the original pinned CPU package/artifacts, not the combined CPU experiment.
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

  applyBridge(directory, bridgeBytes);

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
  await prepareDistribution(directory, async distribution => {
    execFileSync(bun, ["run", "build", "--outDir", distribution], { cwd: directory, env: environment, stdio: "inherit" });
    metadata.sourceDiffSha256 = sha(git(["diff", "--binary"]));
    metadata.patchedFiles = await patchedFiles();
    metadata.artifacts = await runtimeArtifacts(distribution);
    if (metadata.artifacts["v86.wasm"] !== lock.cpuWasmSha256) {
      throw new Error("emitted CPU Wasm does not match its pinned artifact");
    }
    await writeFile(resolve(directory, "public/ring3-runtime-build.json"), JSON.stringify(metadata, null, 2) + "\n");
    await copyFile(resolve(directory, "public/ring3-runtime-build.json"), resolve(distribution, "ring3-runtime-build.json"));
    const licenses = resolve(distribution, "licenses");
    await mkdir(licenses, { recursive: true });
    for (const [source, name] of [["LICENSE", "bottleship-LICENSE"], ["THIRD-PARTY-NOTICES.md", "bottleship-THIRD-PARTY-NOTICES.md"], ["vendor/v86/LICENSE", "v86-LICENSE"]]) {
      await copyFile(resolve(directory, source), resolve(licenses, name));
    }
  });
  console.log(JSON.stringify({ ...metadata, sourceDirectory: directory, distributionDirectory: resolve(directory, "dist") }, null, 2));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
