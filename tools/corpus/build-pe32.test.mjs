import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, mkdirSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildFixture, contract, verifyTextPermissions } from "./build-pe32.mjs";

const outputRoot = new URL("../../target/corpus-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("separate builds have identical PE bytes and evidence", () => {
  const directory = mkdtempSync(new URL("run-", outputRoot));
  try {
    const first = buildFixture(join(directory, "nested", "first"));
    const second = buildFixture(join(directory, "nested", "second"));
    assert.deepEqual(first, second);
    assert.deepEqual(
      readFileSync(join(directory, "nested/first/pe32-arithmetic.exe")),
      readFileSync(join(directory, "nested/second/pe32-arithmetic.exe")),
    );
    assert.equal(first.expectation.eaxBeforeInt3, 42);
    assert.equal(first.windowsOracle, "not-run");
    assert.equal(first.licenseStatus, "pending");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("missing and mismatched tools fail without success evidence", () => {
  const directory = mkdtempSync(new URL("tool-controls-", outputRoot));
  try {
    for (const [name, clang, error] of [
      ["missing", join(directory, "missing-clang"), /ENOENT/],
      ["mismatched", process.execPath, /clang SHA-256 mismatch/],
    ]) {
      const output = join(directory, name);
      assert.throws(() => buildFixture(output, { tools: { clang } }), error);
      assert.equal(existsSync(join(output, "evidence.json")), false);
      assert.equal(existsSync(join(output, "pe32-arithmetic.exe")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("changed source, invalid assembly and altered expectation fail closed", () => {
  const directory = mkdtempSync(new URL("source-controls-", outputRoot));
  const sourcePath = join(directory, "changed.s");
  const source = readFileSync(new URL("../../corpus/pe32-arithmetic/pe32-arithmetic.s", import.meta.url), "utf8");
  try {
    writeFileSync(sourcePath, source.replace("$7", "$8"));
    assert.throws(() => buildFixture(join(directory, "identity"), { sourcePath }), /source SHA-256 mismatch/);

    const changedExpectation = structuredClone(contract);
    changedExpectation.expectation.entryBytes = "00";
    assert.throws(() => buildFixture(join(directory, "expectation"), { contract: changedExpectation }), /entry bytes mismatch/);

    const invalidSource = "invalid instruction\n";
    writeFileSync(sourcePath, invalidSource);
    const invalidContract = structuredClone(contract);
    invalidContract.source.sha256 = createHash("sha256").update(invalidSource).digest("hex");
    assert.throws(() => buildFixture(join(directory, "assembly"), { sourcePath, contract: invalidContract }), /clang failed:/);

    for (const name of ["identity", "expectation", "assembly"]) {
      assert.equal(existsSync(join(directory, name, "evidence.json")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("LLVM permission check rejects writable text and restores the original bytes", () => {
  const directory = mkdtempSync(new URL("permission-control-", outputRoot));
  try {
    const original = join(directory, "original");
    buildFixture(original);
    const sysroot = spawnSync("rustc", ["+1.97.1", "--print", "sysroot"], { encoding: "utf8" });
    assert.equal(sysroot.status, 0);
    const objcopy = join(sysroot.stdout.trim(), "lib/rustlib/aarch64-apple-darwin/bin/rust-objcopy");
    const writable = join(directory, "writable");
    mkdirSync(writable);
    const result = spawnSync(objcopy, ["--set-section-flags", ".text=alloc,load,code,data", join(original, "pe32-arithmetic.exe"), join(writable, "pe32-arithmetic.exe")], { encoding: "utf8", timeout: 15_000 });
    assert.equal(result.status, 0, result.stderr);
    assert.throws(() => verifyTextPermissions(writable, objcopy), /already be read-only and executable/);
    assert.deepEqual(readFileSync(join(writable, "rx-check.exe")), readFileSync(join(original, "pe32-arithmetic.exe")));
    assert.equal(existsSync(join(writable, "evidence.json")), false);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("output parents reject symbolic links before creating outside directories", () => {
  const directory = mkdtempSync(new URL("path-control-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-corpus-outside-"));
  try {
    symlinkSync(outside, join(directory, "redirect"), "dir");
    const invalidContract = structuredClone(contract);
    invalidContract.source.sha256 = "0".repeat(64);
    let failure;
    try {
      buildFixture(join(directory, "redirect", "nested", "fixture"), { contract: invalidContract });
    } catch (error) {
      failure = error;
    }
    assert.deepEqual(readdirSync(outside), []);
    assert.match(failure?.message ?? "build did not refuse", /real directories/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("CLI rejects linked target roots and corpus prefixes before creating a run", () => {
  const directory = mkdtempSync(new URL("cli-path-control-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-corpus-cli-outside-"));
  try {
    for (const linkedPath of ["target", "target/corpus0"]) {
      const workspace = join(directory, linkedPath.replaceAll("/", "-"));
      mkdirSync(join(workspace, "tools/corpus"), { recursive: true });
      mkdirSync(join(workspace, "corpus/pe32-arithmetic"), { recursive: true });
      writeFileSync(join(workspace, "tools/corpus/build-pe32.mjs"), readFileSync(new URL("./build-pe32.mjs", import.meta.url)));
      writeFileSync(join(workspace, "tools/corpus/shared.mjs"), readFileSync(new URL("./shared.mjs", import.meta.url)));
      writeFileSync(join(workspace, "corpus/pe32-arithmetic/fixture.json"), JSON.stringify(contract));
      writeFileSync(join(workspace, ".node-version"), process.versions.node);
      if (linkedPath !== "target") mkdirSync(join(workspace, "target"));
      symlinkSync(outside, join(workspace, linkedPath), "dir");
      const result = spawnSync(process.execPath, [join(workspace, "tools/corpus/build-pe32.mjs")], { encoding: "utf8", timeout: 5_000 });
      assert.deepEqual(readdirSync(outside), []);
      assert.equal(result.status, 1);
      assert.match(result.stderr, /real directories/);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("existing output directories are preserved when reuse is refused", () => {
  const directory = mkdtempSync(new URL("reuse-control-", outputRoot));
  try {
    const sentinel = join(directory, "existing.txt");
    writeFileSync(sentinel, "preserve this output\n");
    assert.throws(() => buildFixture(directory), { code: "EEXIST" });
    assert.equal(readFileSync(sentinel, "utf8"), "preserve this output\n");
    assert.deepEqual(readdirSync(directory), ["existing.txt"]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("unsupported PE32 fixture schemas cannot produce success evidence", () => {
  const directory = mkdtempSync(new URL("schema-control-", outputRoot));
  const output = join(directory, "fixture");
  try {
    const spec = { ...contract, schemaVersion: 2 };
    assert.throws(() => buildFixture(output, { contract: spec }), /unsupported fixture schema/);
    assert.equal(existsSync(join(output, "evidence.json")), false);
    assert.equal(existsSync(join(output, "pe32-arithmetic.exe")), false);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("PE32 CLI refuses extra arguments before creating a build directory", () => {
  const root = new URL("../../target/corpus0/", import.meta.url);
  const before = existsSync(root) ? readdirSync(root).sort() : null;
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("./build-pe32.mjs", import.meta.url)), "--ordinal"], { encoding: "utf8", timeout: 5000, maxBuffer: 1024 * 1024 });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unsupported corpus arguments/);
  assert.equal(result.stdout, "");
  assert.deepEqual(existsSync(root) ? readdirSync(root).sort() : null, before);
});
