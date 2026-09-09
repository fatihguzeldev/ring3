import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildResourceFixtures, contract } from "./build-resources.mjs";
import { sha256 } from "./shared.mjs";

const outputRoot = new URL("../../target/resources-corpus-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("resources fixtures preserve pinned object and executable bytes across builds", () => {
  const directory = mkdtempSync(new URL("repeat-", outputRoot));
  try {
    const first = buildResourceFixtures(join(directory, "first"));
    const second = buildResourceFixtures(join(directory, "second"));
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
      for (const name of ["resources.s", "inspection.txt"]) {
        assert.deepEqual(readFileSync(join(directory, "first", architecture, name)), readFileSync(join(directory, "second", architecture, name)));
      }
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("resources source, tool, assembler, linker and byte mismatches refuse success evidence", () => {
  const directory = mkdtempSync(new URL("negative-", outputRoot));
  try {
    const wrongSource = structuredClone(contract);
    wrongSource.sources["i386.s"].sha256 = "0".repeat(64);
    assert.throws(() => buildResourceFixtures(join(directory, "source"), { contract: wrongSource }), /source SHA-256 mismatch/);
    assert.throws(() => buildResourceFixtures(join(directory, "tool"), { tools: { clang: process.execPath } }), /clang SHA-256 mismatch/);
    const sources = join(directory, "sources"); mkdirSync(sources);
    writeFileSync(join(sources, "amd64.s"), readFileSync(new URL("../../corpus/pe-resources/amd64.s", import.meta.url)));
    for (const [name, source] of [
      ["assembler", "invalid_assembly_instruction\n"],
      ["linker", ".data\n.long ring3_missing_symbol\n"],
    ]) {
      writeFileSync(join(sources, "i386.s"), source);
      const invalid = structuredClone(contract);
      invalid.sources["i386.s"].sha256 = sha256(source);
      assert.throws(() => buildResourceFixtures(join(directory, name), { sourceDirectory: sources, contract: invalid }), /failed:/);
    }
    const wrongBytes = structuredClone(contract);
    wrongBytes.architectures.amd64.artifacts["resources.exe"].sha256 = "0".repeat(64);
    assert.throws(() => buildResourceFixtures(join(directory, "expectation"), { contract: wrongBytes }), /artifact SHA-256 mismatch/);
    const wrongDirectory = structuredClone(contract);
    wrongDirectory.architectures.amd64.directory.size += 1;
    assert.throws(() => buildResourceFixtures(join(directory, "directory"), { contract: wrongDirectory }), /LLVM resource directory mismatch/);
    const wrongVersion = structuredClone(contract);
    wrongVersion.tools.clang.version = "not the pinned compiler version";
    assert.throws(() => buildResourceFixtures(join(directory, "version"), { contract: wrongVersion }), /version mismatch/);
    const wrongName = structuredClone(contract);
    wrongName.architectures.i386.name.utf16le = "000000000000";
    assert.throws(() => buildResourceFixtures(join(directory, "name"), { contract: wrongName }), /raw resource metadata mismatch/);
    const wrongSize = structuredClone(contract);
    wrongSize.architectures.i386.artifacts["resources.exe"].bytes += 1;
    assert.throws(() => buildResourceFixtures(join(directory, "size"), { contract: wrongSize }), /artifact size mismatch/);
    const wrongInspection = structuredClone(contract);
    wrongInspection.architectures.i386.inspection.sha256 = "0".repeat(64);
    assert.throws(() => buildResourceFixtures(join(directory, "inspection"), { contract: wrongInspection }), /inspection SHA-256 mismatch/);
    for (const name of ["source", "tool", "assembler", "linker", "expectation", "directory", "version", "name", "size", "inspection"]) {
      assert.equal(existsSync(join(directory, name, "evidence.json")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("resources producer preserves existing outputs and refuses linked ancestors", () => {
  const directory = mkdtempSync(new URL("paths-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-resources-corpus-outside-"));
  try {
    writeFileSync(join(directory, "sentinel"), "existing output");
    assert.throws(() => buildResourceFixtures(directory), { code: "EEXIST" });
    assert.equal(readFileSync(join(directory, "sentinel"), "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildResourceFixtures(join(directory, "redirect", "fixture")), /real directories/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("resources CLI refuses extra arguments before creating a build directory", () => {
  const root = new URL("../../target/corpus-resources/", import.meta.url);
  const before = existsSync(root) ? readdirSync(root).sort() : null;
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("./build-resources.mjs", import.meta.url)), "extra"], { encoding: "utf8", timeout: 5000, maxBuffer: 1024 * 1024 });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unsupported corpus arguments/);
  assert.equal(result.stdout, "");
  assert.deepEqual(existsSync(root) ? readdirSync(root).sort() : null, before);
});
