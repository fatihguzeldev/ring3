import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildRelocationFixtures, contract } from "./build-relocation-corpus.mjs";
import { sha256 } from "./corpus-tools.mjs";

const outputRoot = new URL("../target/relocation-corpus-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("base-relocation fixtures preserve pinned object and executable bytes across builds", () => {
  const directory = mkdtempSync(new URL("repeat-", outputRoot));
  try {
    const first = buildRelocationFixtures(join(directory, "first"));
    const second = buildRelocationFixtures(join(directory, "second"));
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
      for (const name of ["relocations.s", "inspection.txt"]) {
        assert.deepEqual(readFileSync(join(directory, "first", architecture, name)), readFileSync(join(directory, "second", architecture, name)));
      }
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("relocation source, tool, assembler, linker and byte mismatches refuse success evidence", () => {
  const directory = mkdtempSync(new URL("negative-", outputRoot));
  try {
    const wrongSource = structuredClone(contract);
    wrongSource.sources["i386.s"].sha256 = "0".repeat(64);
    assert.throws(() => buildRelocationFixtures(join(directory, "source"), { contract: wrongSource }), /source SHA-256 mismatch/);
    assert.throws(() => buildRelocationFixtures(join(directory, "tool"), { tools: { clang: process.execPath } }), /clang SHA-256 mismatch/);
    const sources = join(directory, "sources"); mkdirSync(sources);
    writeFileSync(join(sources, "amd64.s"), readFileSync(new URL("../corpus/pe-base-relocations/amd64.s", import.meta.url)));
    for (const [name, source] of [
      ["assembler", "invalid_assembly_instruction\n"],
      ["linker", ".data\n.long ring3_missing_symbol\n"],
    ]) {
      writeFileSync(join(sources, "i386.s"), source);
      const invalid = structuredClone(contract);
      invalid.sources["i386.s"].sha256 = sha256(source);
      assert.throws(() => buildRelocationFixtures(join(directory, name), { sourceDirectory: sources, contract: invalid }), /failed:/);
    }
    const wrongBytes = structuredClone(contract);
    wrongBytes.architectures.amd64.artifacts["relocations.exe"].sha256 = "0".repeat(64);
    assert.throws(() => buildRelocationFixtures(join(directory, "expectation"), { contract: wrongBytes }), /artifact SHA-256 mismatch/);
    const wrongDirectory = structuredClone(contract);
    wrongDirectory.expectation.directorySize += 1;
    assert.throws(() => buildRelocationFixtures(join(directory, "directory"), { contract: wrongDirectory }), /LLVM base-relocation directory mismatch/);
    for (const name of ["source", "tool", "assembler", "linker", "expectation", "directory"]) {
      assert.equal(existsSync(join(directory, name, "evidence.json")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("relocation producer preserves existing outputs and refuses linked ancestors", () => {
  const directory = mkdtempSync(new URL("paths-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-relocation-corpus-outside-"));
  try {
    writeFileSync(join(directory, "sentinel"), "existing output");
    assert.throws(() => buildRelocationFixtures(directory), { code: "EEXIST" });
    assert.equal(readFileSync(join(directory, "sentinel"), "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildRelocationFixtures(join(directory, "redirect", "fixture")), /real directories/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("relocation CLI refuses extra arguments before creating a build directory", () => {
  const root = new URL("../target/corpus-relocations/", import.meta.url);
  const before = existsSync(root) ? readdirSync(root).sort() : null;
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("./build-relocation-corpus.mjs", import.meta.url)), "extra"], { encoding: "utf8", timeout: 5000, maxBuffer: 1024 * 1024 });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unsupported corpus arguments/);
  assert.equal(result.stdout, "");
  assert.deepEqual(existsSync(root) ? readdirSync(root).sort() : null, before);
});
