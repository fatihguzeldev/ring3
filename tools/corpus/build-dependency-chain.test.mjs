import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import { buildDependencyChainFixtures, contract } from "./build-dependency-chain.mjs";
import { sha256 } from "./shared.mjs";

const outputRoot = new URL("../../target/dependency-chain-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("linked dependency chains match pinned bytes across independent builds", () => {
  const directory = mkdtempSync(new URL("repeat-", outputRoot));
  try {
    const first = buildDependencyChainFixtures(join(directory, "first"));
    const second = buildDependencyChainFixtures(join(directory, "second"));
    assert.deepEqual(first, second);
    assert.equal(first.executed, false);
    assert.equal(first.windowsOracle, "not-run");
    for (const architecture of ["i386", "amd64"]) {
      for (const [name, expected] of Object.entries(contract.architectures[architecture].artifacts)) {
        const a = readFileSync(join(directory, "first", architecture, name));
        const b = readFileSync(join(directory, "second", architecture, name));
        assert.deepEqual(a, b);
        assert.equal(sha256(a), expected.sha256);
        assert.equal(a.length, expected.bytes);
        if (name.endsWith(".obj")) assert.equal(a.readUInt32LE(4), 0);
      }
      const exe = readFileSync(join(directory, "first", architecture, "Chain.exe.inspection.txt"), "utf8");
      const middle = readFileSync(join(directory, "first", architecture, "Ring3Middle.dll.inspection.txt"), "utf8");
      const leaf = readFileSync(join(directory, "first", architecture, "Ring3Leaf.dll.inspection.txt"), "utf8");
      assert.deepEqual([...exe.matchAll(/DLL Name: (.+)/g)].map((m) => m[1].trim()), ["Ring3Middle.dll"]);
      assert.deepEqual([...middle.matchAll(/DLL Name: (.+)/g)].map((m) => m[1].trim()), ["Ring3Leaf.dll"]);
      assert.equal(leaf.includes("DLL Name:"), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("chain producer refuses identity and compiler failures without success evidence", () => {
  const directory = mkdtempSync(new URL("negative-", outputRoot));
  try {
    const source = structuredClone(contract);
    source.sources["middle.c"].sha256 = "0".repeat(64);
    assert.throws(() => buildDependencyChainFixtures(join(directory, "source"), { contract: source }), /source SHA-256 mismatch/);
    assert.throws(() => buildDependencyChainFixtures(join(directory, "tool"), { tools: { clang: process.execPath } }), /clang SHA-256 mismatch/);
    const sources = join(directory, "sources");
    mkdirSync(sources);
    for (const name of ["chain.c", "middle.c", "leaf.c"]) {
      writeFileSync(join(sources, name), readFileSync(new URL(`../../corpus/pe-dependency-chain/${name}`, import.meta.url)));
    }
    const invalid = "invalid C source\n";
    writeFileSync(join(sources, "middle.c"), invalid);
    const compiler = structuredClone(contract);
    compiler.sources["middle.c"].sha256 = sha256(invalid);
    assert.throws(() => buildDependencyChainFixtures(join(directory, "compiler"), { contract: compiler, sourceDirectory: sources }), /failed:/);
    const expected = structuredClone(contract);
    expected.architectures.amd64.artifacts["Ring3Middle.dll"].sha256 = "0".repeat(64);
    assert.throws(() => buildDependencyChainFixtures(join(directory, "expected"), { contract: expected }), /artifact SHA-256 mismatch/);
    for (const name of ["source", "tool", "compiler", "expected"]) {
      assert.equal(existsSync(join(directory, name, "evidence.json")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("chain producer refuses reused and symlinked output without outside writes", () => {
  const directory = mkdtempSync(new URL("paths-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-chain-outside-"));
  try {
    writeFileSync(join(directory, "sentinel"), "existing output");
    assert.throws(() => buildDependencyChainFixtures(directory), { code: "EEXIST" });
    assert.equal(readFileSync(join(directory, "sentinel"), "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildDependencyChainFixtures(join(directory, "redirect", "fixture")), /real directories/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});
