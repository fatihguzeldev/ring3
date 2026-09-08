import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildForwarderFixtures, contract } from "./build-forwarder-corpus.mjs";
import { sha256 } from "./corpus-tools.mjs";

const outputRoot = new URL("../target/forwarder-corpus-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("sparse forwarder DLL fixtures preserve pinned bytes across independent builds", () => {
  const directory = mkdtempSync(new URL("repeat-", outputRoot));
  try {
    const first = buildForwarderFixtures(join(directory, "first"));
    const second = buildForwarderFixtures(join(directory, "second"));
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
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("forwarder source, tool, compiler, linker and expectation failures refuse success evidence", () => {
  const directory = mkdtempSync(new URL("negative-", outputRoot));
  try {
    const alteredSource = structuredClone(contract);
    alteredSource.sources["forwarders.def"].sha256 = "0".repeat(64);
    assert.throws(() => buildForwarderFixtures(join(directory, "source"), { contract: alteredSource }), /source SHA-256 mismatch/);
    assert.throws(() => buildForwarderFixtures(join(directory, "tool"), { tools: { clang: process.execPath } }), /clang SHA-256 mismatch/);
    const sources = join(directory, "sources");
    mkdirSync(sources);
    const originalAnchor = readFileSync(new URL("../corpus/pe-forwarders/anchor.c", import.meta.url));
    writeFileSync(join(sources, "forwarders.def"), readFileSync(new URL("../corpus/pe-forwarders/forwarders.def", import.meta.url)));
    const invalidAnchor = "invalid C source\n";
    writeFileSync(join(sources, "anchor.c"), invalidAnchor);
    const invalidCompiler = structuredClone(contract);
    invalidCompiler.sources["anchor.c"].sha256 = sha256(invalidAnchor);
    assert.throws(() => buildForwarderFixtures(join(directory, "compiler"), { sourceDirectory: sources, contract: invalidCompiler }), /failed:/);
    writeFileSync(join(sources, "anchor.c"), originalAnchor);
    const invalidDefinition = "EXPORTS\n by_name=OtherModule.target @invalid\n";
    writeFileSync(join(sources, "forwarders.def"), invalidDefinition);
    const invalidLinker = structuredClone(contract);
    invalidLinker.sources["forwarders.def"].sha256 = sha256(invalidDefinition);
    assert.throws(() => buildForwarderFixtures(join(directory, "linker"), { sourceDirectory: sources, contract: invalidLinker }), /failed:/);
    const wrongBytes = structuredClone(contract);
    wrongBytes.architectures.amd64.artifacts["Ring3Forwarders.dll"].sha256 = "0".repeat(64);
    assert.throws(() => buildForwarderFixtures(join(directory, "expectation"), { contract: wrongBytes }), /artifact SHA-256 mismatch/);
    for (const name of ["source", "tool", "compiler", "linker", "expectation"]) {
      assert.equal(existsSync(join(directory, name, "evidence.json")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("forwarder producer preserves existing outputs and refuses linked ancestors", () => {
  const directory = mkdtempSync(new URL("paths-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-forwarder-corpus-outside-"));
  try {
    writeFileSync(join(directory, "sentinel"), "existing output");
    assert.throws(() => buildForwarderFixtures(directory), { code: "EEXIST" });
    assert.equal(readFileSync(join(directory, "sentinel"), "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildForwarderFixtures(join(directory, "redirect", "fixture")), /real directories/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("forwarder CLI refuses extra arguments", () => {
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("./build-forwarder-corpus.mjs", import.meta.url)), "extra"], { encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unsupported corpus arguments/);
  assert.equal(result.stdout, "");
});
