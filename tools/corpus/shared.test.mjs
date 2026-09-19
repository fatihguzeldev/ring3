import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { root, target } from "./shared.mjs";

const explicit = { clang: "/explicit/clang", lld: "/explicit/lld", objdump: "/explicit/objdump", objcopy: "/explicit/objcopy" };
const rustBin = "/fake/sysroot/lib/rustlib/aarch64-apple-darwin/bin";

function workspace(t, programs = []) {
  mkdirSync(target, { recursive: true });
  const directory = mkdtempSync(join(target, "tool-discovery-test-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const bin = join(directory, "bin");
  const log = join(directory, "calls.jsonl");
  mkdirSync(bin);
  for (const name of programs) {
    const script = join(bin, name);
    writeFileSync(script, `#!${process.execPath}
const { appendFileSync } = require("node:fs");
const args = process.argv.slice(2);
appendFileSync(${JSON.stringify(log)}, JSON.stringify([${JSON.stringify(name)}, ...args]) + "\\n");
if (${JSON.stringify(name)} === "rustc") process.stdout.write("/fake/sysroot\\n");
else process.stdout.write("/fake/" + args[1] + "\\n");
`);
    chmodSync(script, 0o755);
  }
  return { directory, bin, log };
}

function child(directory, source) {
  const result = spawnSync(process.execPath, ["--input-type=module", "-e", source], {
    cwd: root, env: { ...process.env, PATH: directory.bin }, encoding: "utf8", timeout: 5_000,
  });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stderr);
  return result.stdout;
}

function discover(t, overrides, programs = []) {
  const directory = workspace(t, programs);
  const source = `import { locateTools } from "./tools/corpus/shared.mjs";
    const overrides = Object.freeze(${JSON.stringify(overrides)});
    console.log(JSON.stringify(locateTools(overrides)));`;
  const tools = JSON.parse(child(directory, source));
  const calls = existsSync(directory.log) ? readFileSync(directory.log, "utf8").trim().split("\n").map(JSON.parse) : [];
  return { tools, calls };
}

test("complete overrides do not discover unavailable host tools", (t) => {
  assert.deepEqual(discover(t, explicit), { tools: explicit, calls: [] });
});

test("partial overrides discover only missing paths and retain caller values", (t) => {
  assert.deepEqual(discover(t, { clang: explicit.clang, objcopy: explicit.objcopy }, ["rustc", "xcrun"]), {
    tools: { clang: explicit.clang, lld: join(rustBin, "rust-lld"), objdump: "/fake/llvm-objdump", objcopy: explicit.objcopy },
    calls: [["rustc", "+1.97.1", "--print", "sysroot"], ["xcrun", "--find", "llvm-objdump"]],
  });
});

test("supplied Rust tools avoid sysroot discovery", (t) => {
  assert.deepEqual(discover(t, { lld: explicit.lld, objcopy: explicit.objcopy }, ["xcrun"]), {
    tools: { clang: "/fake/clang", lld: explicit.lld, objdump: "/fake/llvm-objdump", objcopy: explicit.objcopy },
    calls: [["xcrun", "--find", "clang"], ["xcrun", "--find", "llvm-objdump"]],
  });
});

test("default discovery uses the pinned sysroot once and the existing xcrun paths", (t) => {
  assert.deepEqual(discover(t, {}, ["rustc", "xcrun"]), {
    tools: { clang: "/fake/clang", lld: join(rustBin, "rust-lld"), objdump: "/fake/llvm-objdump", objcopy: join(rustBin, "rust-objcopy") },
    calls: [["rustc", "+1.97.1", "--print", "sysroot"], ["xcrun", "--find", "clang"], ["xcrun", "--find", "llvm-objdump"]],
  });
});

test("explicit invalid values are retained for builder validation", (t) => {
  const overrides = { ...explicit, clang: null };
  assert.deepEqual(discover(t, overrides), { tools: overrides, calls: [] });
});

test("the load-config producer rejects an overridden tool hash without host discovery", (t) => {
  const directory = workspace(t);
  child(directory, `import assert from "node:assert/strict";
    import { buildLoadConfigFixtures } from "./tools/corpus/build-load-config.mjs";
    const tools = Object.fromEntries(["clang", "lld", "objdump", "objcopy"].map(name => [name, process.execPath]));
    assert.throws(() => buildLoadConfigFixtures(${JSON.stringify(join(directory.directory, "producer"))}, { tools }), /clang SHA-256 mismatch/);`);
  assert.equal(existsSync(directory.log), false);
});
