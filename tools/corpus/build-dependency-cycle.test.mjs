import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import { buildDependencyCycleFixtures, cycleContract } from "./build-dependency-chain.mjs";
import { root, sha256 } from "./shared.mjs";

const outputRoot = new URL("../../target/dependency-cycle-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("linked cycles preserve every pinned bootstrap and final artifact", () => {
  const directory = mkdtempSync(new URL("repeat-", outputRoot));
  try {
    const first = buildDependencyCycleFixtures(join(directory, "first"));
    const second = buildDependencyCycleFixtures(join(directory, "second"));
    assert.deepEqual(first, second);
    assert.equal(first.executed, false);
    assert.equal(first.windowsOracle, "not-run");
    assert.equal(first.bootstrap.includedInFinalGraph, false);
    for (const architecture of ["i386", "amd64"]) {
      for (const [name, expected] of Object.entries(cycleContract.architectures[architecture].artifacts)) {
        const a = readFileSync(join(directory, "first", architecture, name));
        assert.deepEqual(a, readFileSync(join(directory, "second", architecture, name)));
        assert.equal(sha256(a), expected.sha256);
        assert.equal(a.length, expected.bytes);
        if (name.endsWith(".obj")) assert.equal(a.readUInt32LE(4), 0);
      }
      const inspect = (name) => readFileSync(join(directory, "first", architecture, `${name}.inspection.txt`), "utf8");
      const imports = (text) => [...text.matchAll(/DLL Name: (.+)/g)].map((match) => match[1].trim());
      assert.deepEqual(imports(inspect("Chain.exe")), ["Ring3Middle.dll"]);
      assert.deepEqual(imports(inspect("Ring3Middle.dll")), ["Ring3Leaf.dll"]);
      assert.deepEqual(imports(inspect("Ring3Leaf.dll")), ["Ring3Middle.dll"]);
      assert.deepEqual(imports(inspect("bootstrap/Ring3Leaf.dll")), []);
      assert.notDeepEqual(
        readFileSync(join(directory, "first", architecture, "Ring3Leaf.dll")),
        readFileSync(join(directory, "first", architecture, "bootstrap/Ring3Leaf.dll")),
      );
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("cycle producer rejects identity and compiler failures without success evidence", () => {
  const directory = mkdtempSync(new URL("negative-", outputRoot));
  try {
    const source = structuredClone(cycleContract);
    source.sources["cycle_leaf.c"].sha256 = "0".repeat(64);
    assert.throws(() => buildDependencyCycleFixtures(join(directory, "source"), { contract: source }), /source SHA-256 mismatch/);
    assert.throws(() => buildDependencyCycleFixtures(join(directory, "tool"), { tools: { clang: process.execPath } }), /clang SHA-256 mismatch/);
    const sources = join(directory, "sources");
    mkdirSync(sources);
    for (const [name, spec] of Object.entries(cycleContract.sources)) {
      writeFileSync(join(sources, name), readFileSync(join(root, spec.path)));
    }
    const invalid = "invalid C source\n";
    writeFileSync(join(sources, "cycle_leaf.c"), invalid);
    const compiler = structuredClone(cycleContract);
    compiler.sources["cycle_leaf.c"].sha256 = sha256(invalid);
    assert.throws(() => buildDependencyCycleFixtures(join(directory, "compiler"), { contract: compiler, sourceDirectory: sources }), /failed:/);
    for (const [label, artifact] of [["bootstrap", "bootstrap/Ring3Leaf.dll"], ["final", "Ring3Leaf.dll"]]) {
      const expected = structuredClone(cycleContract);
      expected.architectures.amd64.artifacts[artifact].sha256 = "0".repeat(64);
      assert.throws(() => buildDependencyCycleFixtures(join(directory, label), { contract: expected }), /artifact SHA-256 mismatch/);
    }
    for (const name of ["source", "tool", "compiler", "bootstrap", "final"]) {
      assert.equal(existsSync(join(directory, name, "evidence.json")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("cycle producer refuses reused and symlinked output without outside writes", () => {
  const directory = mkdtempSync(new URL("paths-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-cycle-outside-"));
  try {
    writeFileSync(join(directory, "sentinel"), "existing output");
    assert.throws(() => buildDependencyCycleFixtures(directory), { code: "EEXIST" });
    assert.equal(readFileSync(join(directory, "sentinel"), "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildDependencyCycleFixtures(join(directory, "redirect", "fixture")), /real directories/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});
