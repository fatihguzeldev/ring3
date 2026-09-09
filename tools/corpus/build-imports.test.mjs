import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import { buildImportFixtures, contract } from "./build-imports.mjs";
import { sha256 } from "./shared.mjs";

const outputRoot = new URL("../../target/import-corpus-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("named PE fixtures match pinned bytes across independent builds", () => {
  const directory = mkdtempSync(new URL("repeat-", outputRoot));
  try {
    const first = buildImportFixtures(join(directory, "nested", "first"));
    const second = buildImportFixtures(join(directory, "nested", "second"));
    assert.deepEqual(first, second);
    assert.equal(first.executed, false);
    assert.equal(first.windowsOracle, "not-run");
    for (const architecture of ["i386", "amd64"]) {
      for (const [name, expected] of Object.entries(contract.architectures[architecture].artifacts)) {
        const a = readFileSync(join(directory, "nested", "first", architecture, name));
        const b = readFileSync(join(directory, "nested", "second", architecture, name));
        assert.deepEqual(a, b);
        assert.equal(sha256(a), expected.sha256);
        assert.equal(a.length, expected.bytes);
        if (name.endsWith(".obj")) assert.equal(a.readUInt32LE(4), 0);
      }
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("source, tool, compiler and expected-byte failures never write success evidence", () => {
  const directory = mkdtempSync(new URL("negative-", outputRoot));
  try {
    const alteredSource = structuredClone(contract);
    alteredSource.sources["probe.c"].sha256 = "0".repeat(64);
    assert.throws(() => buildImportFixtures(join(directory, "source"), { contract: alteredSource }), /source SHA-256 mismatch/);
    assert.throws(() => buildImportFixtures(join(directory, "tool"), { tools: { clang: process.execPath } }), /clang SHA-256 mismatch/);
    const sources = join(directory, "sources");
    mkdirSync(sources);
    const invalid = "invalid C source\n";
    writeFileSync(join(sources, "probe.c"), invalid);
    writeFileSync(join(sources, "imports.c"), readFileSync(new URL("../../corpus/pe-named-imports/imports.c", import.meta.url)));
    const invalidContract = structuredClone(contract);
    invalidContract.sources["probe.c"].sha256 = sha256(invalid);
    assert.throws(() => buildImportFixtures(join(directory, "compiler"), { sourceDirectory: sources, contract: invalidContract }), /failed:/);
    const wrongBytes = structuredClone(contract);
    wrongBytes.architectures.amd64.artifacts["imports.exe"].sha256 = "0".repeat(64);
    assert.throws(() => buildImportFixtures(join(directory, "expectation"), { contract: wrongBytes }), /artifact SHA-256 mismatch/);
    for (const name of ["source", "tool", "compiler", "expectation"]) {
      assert.equal(existsSync(join(directory, name, "evidence.json")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("named producer refuses reused outputs and linked ancestors without outside writes", () => {
  const directory = mkdtempSync(new URL("paths-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-named-corpus-outside-"));
  try {
    const sentinel = join(directory, "sentinel");
    writeFileSync(sentinel, "existing output");
    assert.throws(() => buildImportFixtures(directory), { code: "EEXIST" });
    assert.equal(readFileSync(sentinel, "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildImportFixtures(join(directory, "redirect", "fixture")), /real directories/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});
