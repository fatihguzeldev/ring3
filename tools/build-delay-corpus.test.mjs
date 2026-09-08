import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildDelayImportFixtures, contract } from "./build-delay-corpus.mjs";
import { sha256 } from "./corpus-tools.mjs";

const outputRoot = new URL("../target/delay-corpus-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("delay fixtures preserve all archived bytes and zero timestamps across builds", () => {
  const directory = mkdtempSync(new URL("repeat-", outputRoot));
  try {
    const first = buildDelayImportFixtures(join(directory, "first"));
    const second = buildDelayImportFixtures(join(directory, "second"));
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
        if (name.endsWith(".dll") || name.endsWith(".exe")) assert.equal(a.readUInt32LE(a.readUInt32LE(60) + 8), 0);
      }
      for (const name of ["probe.c", "delayed.c"]) {
        const a = readFileSync(join(directory, "first", architecture, name));
        assert.equal(sha256(a), contract.sources[name].sha256);
        assert.deepEqual(a, readFileSync(join(directory, "second", architecture, name)));
      }
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("delay source, tool, compiler, linker and artifact mismatches refuse success evidence", () => {
  const directory = mkdtempSync(new URL("negative-", outputRoot));
  try {
    const wrongSource = structuredClone(contract);
    wrongSource.sources["delayed.c"].sha256 = "0".repeat(64);
    assert.throws(() => buildDelayImportFixtures(join(directory, "source"), { contract: wrongSource }), /source SHA-256 mismatch/);
    assert.throws(() => buildDelayImportFixtures(join(directory, "tool"), { tools: { clang: process.execPath } }), /clang SHA-256 mismatch/);
    const sources = join(directory, "sources"); mkdirSync(sources);
    writeFileSync(join(sources, "probe.c"), readFileSync(new URL("../corpus/pe-delay-imports/probe.c", import.meta.url)));
    for (const [name, source] of [
      ["compiler", "this is not valid C;\n"],
      ["linker", "extern int ring3_missing(void); int entry(void) { return ring3_missing(); }\n"],
    ]) {
      writeFileSync(join(sources, "delayed.c"), source);
      const invalid = structuredClone(contract);
      invalid.sources["delayed.c"].sha256 = sha256(source);
      assert.throws(() => buildDelayImportFixtures(join(directory, name), { sourceDirectory: sources, contract: invalid }), /failed:/);
    }
    const wrongBytes = structuredClone(contract);
    wrongBytes.architectures.amd64.artifacts["delayed.exe"].sha256 = "0".repeat(64);
    assert.throws(() => buildDelayImportFixtures(join(directory, "expectation"), { contract: wrongBytes }), /artifact SHA-256 mismatch/);
    const wrongDirectory = structuredClone(contract);
    wrongDirectory.architectures.amd64.directory.size += 1;
    assert.throws(() => buildDelayImportFixtures(join(directory, "directory"), { contract: wrongDirectory }), /LLVM delay directory mismatch/);
    const wrongRaw = structuredClone(contract);
    wrongRaw.architectures.amd64.directory.rawWords[7] = 1;
    assert.throws(() => buildDelayImportFixtures(join(directory, "raw"), { contract: wrongRaw }), /raw delay descriptor mismatch/);
    for (const name of ["source", "tool", "compiler", "linker", "expectation", "directory", "raw"]) {
      assert.equal(existsSync(join(directory, name, "evidence.json")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("delay producer preserves existing outputs and refuses linked ancestors", () => {
  const directory = mkdtempSync(new URL("paths-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-delay-corpus-outside-"));
  try {
    writeFileSync(join(directory, "sentinel"), "existing output");
    assert.throws(() => buildDelayImportFixtures(directory), { code: "EEXIST" });
    assert.equal(readFileSync(join(directory, "sentinel"), "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildDelayImportFixtures(join(directory, "redirect", "fixture")), /real directories/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("delay CLI refuses extra arguments before creating a build directory", () => {
  const root = new URL("../target/corpus-delay/", import.meta.url);
  const before = existsSync(root) ? readdirSync(root).sort() : null;
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("./build-delay-corpus.mjs", import.meta.url)), "extra"], { encoding: "utf8", timeout: 5000, maxBuffer: 1024 * 1024 });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unsupported corpus arguments/);
  assert.equal(result.stdout, "");
  assert.deepEqual(existsSync(root) ? readdirSync(root).sort() : null, before);
});
