import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildPe32PlusFixture, contract } from "./build-pe32plus.mjs";
import { sha256 } from "./shared.mjs";

const outputRoot = new URL("../../target/pe32plus-corpus-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("PE32+ header fixture preserves both pinned artifacts across independent builds", () => {
  const directory = mkdtempSync(new URL("repeat-", outputRoot));
  try {
    const first = buildPe32PlusFixture(join(directory, "first"));
    const second = buildPe32PlusFixture(join(directory, "second"));
    assert.deepEqual(first, second);
    assert.equal(first.executed, false);
    assert.equal(first.windowsOracle, "not-run");
    for (const [name, expected] of Object.entries(contract.artifacts)) {
      const a = readFileSync(join(directory, "first", name));
      const b = readFileSync(join(directory, "second", name));
      assert.deepEqual(a, b);
      assert.equal(sha256(a), expected.sha256);
      assert.equal(a.length, expected.bytes);
      if (name.endsWith(".obj")) assert.equal(a.readUInt32LE(4), 0);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("PE32+ source, tool, compile and expectation failures refuse success evidence", () => {
  const directory = mkdtempSync(new URL("negative-", outputRoot));
  try {
    const alteredSource = structuredClone(contract);
    alteredSource.source.sha256 = "0".repeat(64);
    assert.throws(() => buildPe32PlusFixture(join(directory, "source"), { contract: alteredSource }), /source SHA-256 mismatch/);
    assert.throws(() => buildPe32PlusFixture(join(directory, "tool"), { tools: { lld: process.execPath } }), /lld SHA-256 mismatch/);
    const invalid = "invalid instruction\n";
    const source = join(directory, "invalid.s");
    writeFileSync(source, invalid);
    const invalidContract = structuredClone(contract);
    invalidContract.source.sha256 = sha256(invalid);
    assert.throws(() => buildPe32PlusFixture(join(directory, "compiler"), { sourcePath: source, contract: invalidContract }), /failed:/);
    const wrongBytes = structuredClone(contract);
    wrongBytes.artifacts["arithmetic.exe"].sha256 = "0".repeat(64);
    assert.throws(() => buildPe32PlusFixture(join(directory, "expectation"), { contract: wrongBytes }), /artifact SHA-256 mismatch/);
    for (const name of ["source", "tool", "compiler", "expectation"]) {
      assert.equal(existsSync(join(directory, name, "evidence.json")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("PE32+ producer preserves existing outputs and refuses linked ancestors", () => {
  const directory = mkdtempSync(new URL("paths-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-pe32plus-corpus-outside-"));
  try {
    writeFileSync(join(directory, "sentinel"), "existing output");
    assert.throws(() => buildPe32PlusFixture(directory), { code: "EEXIST" });
    assert.equal(readFileSync(join(directory, "sentinel"), "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildPe32PlusFixture(join(directory, "redirect", "fixture")), /real directories/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("PE32+ CLI refuses extra arguments", () => {
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("./build-pe32plus.mjs", import.meta.url)), "extra"], { encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unsupported corpus arguments/);
  assert.equal(result.stdout, "");
});
