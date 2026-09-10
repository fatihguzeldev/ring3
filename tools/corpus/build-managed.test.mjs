import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildManagedFixtures, contract } from "./build-managed.mjs";
import { sha256 } from "./shared.mjs";

const root = new URL("../../target/managed-corpus-tests/", import.meta.url);
mkdirSync(root, { recursive: true });

test("managed fixtures reproduce pinned unpatched executable bytes across output roots", () => {
  const directory = mkdtempSync(new URL("repeat-", root));
  try {
    const first = buildManagedFixtures(join(directory, "first"));
    const second = buildManagedFixtures(join(directory, "second"));
    assert.deepEqual(first, second);
    assert.equal(first.executed, false);
    assert.equal(first.windowsOracle, "not-run");
    for (const architecture of ["x86", "x64"]) {
      for (const name of ["Probe.cs", "Probe.exe"]) {
        assert.deepEqual(readFileSync(join(directory, "first", architecture, name)), readFileSync(join(directory, "second", architecture, name)));
      }
      const bytes = readFileSync(join(directory, "first", architecture, "Probe.exe"));
      assert.equal(sha256(bytes), contract.architectures[architecture].artifact.sha256);
      assert.equal(bytes.length, contract.architectures[architecture].artifact.bytes);
    }
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("managed producer refuses missing SDK, changed inputs, tools, compiler and output contracts", () => {
  const directory = mkdtempSync(new URL("negative-", root));
  const attempted = [];
  const reject = (name, options, pattern) => {
    attempted.push(name);
    assert.throws(() => buildManagedFixtures(join(directory, name), options), pattern);
  };
  try {
    reject("missing-sdk", { sdkRoot: "" }, /RING3_DOTNET_ROOT/);
    reject("wrong-sdk", { sdkRoot: directory }, /ENOENT|SDK/);
    const source = structuredClone(contract);
    source.source.sha256 = "0".repeat(64);
    reject("source", { contract: source }, /source SHA-256 mismatch/);
    const file = structuredClone(contract);
    file.sdk.files.dotnet.sha256 = "0".repeat(64);
    reject("host", { contract: file }, /SDK file identity mismatch/);
    const tree = structuredClone(contract);
    tree.sdk.trees["sdk/10.0.401/Roslyn/bincore"].sha256 = "0".repeat(64);
    reject("compiler-tree", { contract: tree }, /SDK tree identity mismatch/);
    const version = structuredClone(contract);
    version.sdk.compilerVersion = "wrong version";
    reject("version", { contract: version }, /compiler version mismatch/);
    const badSource = join(directory, "bad.cs");
    writeFileSync(badSource, "invalid csharp source\n");
    const compiler = structuredClone(contract);
    compiler.source.sha256 = sha256(readFileSync(badSource));
    reject("compile", { contract: compiler, sourcePath: badSource }, /compiler failed/);
    const identity = structuredClone(contract);
    identity.architectures.x64.artifact.sha256 = "0".repeat(64);
    reject("identity", { contract: identity }, /artifact SHA-256 mismatch/);
    const size = structuredClone(contract);
    size.architectures.x86.artifact.bytes++;
    reject("size", { contract: size }, /artifact size mismatch/);
    const metadata = structuredClone(contract);
    metadata.architectures.x64.metadata.flags++;
    reject("metadata", { contract: metadata }, /raw CLR metadata mismatch/);
    for (const name of attempted) assert.equal(existsSync(join(directory, name, "evidence.json")), false);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("managed producer preserves existing outputs and refuses linked or outside paths", () => {
  const directory = mkdtempSync(new URL("paths-", root));
  const outside = mkdtempSync(join(tmpdir(), "ring3-managed-outside-"));
  try {
    writeFileSync(join(directory, "sentinel"), "existing output");
    assert.throws(() => buildManagedFixtures(directory), { code: "EEXIST" });
    assert.equal(readFileSync(join(directory, "sentinel"), "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildManagedFixtures(join(directory, "redirect", "fixture")), /real directories/);
    assert.throws(() => buildManagedFixtures(join(outside, "fixture")), /fresh directory under target/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("managed CLI refuses extra arguments and missing SDK without success output", () => {
  const output = new URL("../../target/corpus-managed/", import.meta.url);
  const before = existsSync(output) ? readdirSync(output).sort() : null;
  const command = fileURLToPath(new URL("./build-managed.mjs", import.meta.url));
  for (const args of [["extra"], []]) {
    const env = { ...process.env };
    delete env.RING3_DOTNET_ROOT;
    const result = spawnSync(process.execPath, [command, ...args], { env, encoding: "utf8", timeout: 5000, maxBuffer: 1024 * 1024 });
    assert.equal(result.status, 1);
    assert.match(result.stderr, args.length ? /unsupported corpus arguments/ : /RING3_DOTNET_ROOT/);
    assert.equal(result.stdout, "");
    assert.deepEqual(existsSync(output) ? readdirSync(output).sort() : null, before);
  }
});
