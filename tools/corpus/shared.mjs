import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { lstatSync, mkdirSync } from "node:fs";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const root = fileURLToPath(new URL("../../", import.meta.url));
export const target = join(root, "target");
export const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

export function run(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    env: { ...process.env, LC_ALL: "C", TZ: "UTC" },
    encoding: "utf8",
    timeout: 15_000,
    maxBuffer: 1024 * 1024,
  });
  if (result.error || result.status !== 0) {
    throw new Error(`${command} failed: ${result.error?.message ?? result.stderr}`);
  }
  return result.stdout;
}

export function locateTools(overrides = {}) {
  const tools = { ...overrides };
  const missing = (name) => !Object.hasOwn(tools, name);
  if (missing("lld") || missing("objcopy")) {
    const sysroot = run("rustc", ["+1.97.1", "--print", "sysroot"]).trim();
    const rustBin = join(sysroot, "lib/rustlib/aarch64-apple-darwin/bin");
    if (missing("lld")) tools.lld = join(rustBin, "rust-lld");
    if (missing("objcopy")) tools.objcopy = join(rustBin, "rust-objcopy");
  }
  if (missing("clang")) tools.clang = run("xcrun", ["--find", "clang"]).trim();
  if (missing("objdump")) tools.objdump = run("xcrun", ["--find", "llvm-objdump"]).trim();
  return tools;
}

function ensureRealDirectory(directory) {
  try {
    mkdirSync(directory);
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
  }
  assert.ok(lstatSync(directory).isDirectory(), "output path components must be real directories");
}

// this checks existing paths; it is not a concurrent filesystem sandbox.
export function prepareOutputParents(outputDirectory) {
  const output = resolve(outputDirectory);
  const child = relative(target, output);
  assert.ok(child && child !== ".." && !child.startsWith(`..${sep}`) && !isAbsolute(child),
    "output must be a fresh directory under target");
  let parent = target;
  ensureRealDirectory(parent);
  for (const component of child.split(sep).slice(0, -1)) {
    parent = join(parent, component);
    ensureRealDirectory(parent);
  }
  return output;
}
