import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { buildFixture, contract, verifyTextPermissions } from "./build-corpus.mjs";

const outputRoot = new URL("../target/corpus-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("separate builds have identical PE bytes and evidence", () => {
  const directory = mkdtempSync(new URL("run-", outputRoot));
  try {
    const first = buildFixture(join(directory, "first"));
    const second = buildFixture(join(directory, "second"));
    assert.deepEqual(first, second);
    assert.deepEqual(
      readFileSync(join(directory, "first/pe32-arithmetic.exe")),
      readFileSync(join(directory, "second/pe32-arithmetic.exe")),
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
  const source = readFileSync(new URL("../corpus/pe32-arithmetic.s", import.meta.url), "utf8");
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
