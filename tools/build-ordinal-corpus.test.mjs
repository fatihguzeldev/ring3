import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildOrdinalFixtures, ordinalContract } from "./build-import-corpus.mjs";
import { sha256 } from "./corpus-tools.mjs";

const outputRoot = new URL("../target/ordinal-corpus-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("ordinal PE fixtures preserve pinned identities across independent builds", () => {
  const directory = mkdtempSync(new URL("repeat-", outputRoot));
  try {
    const first = buildOrdinalFixtures(join(directory, "first"));
    const second = buildOrdinalFixtures(join(directory, "second"));
    assert.deepEqual(first, second);
    assert.equal(first.executed, false);
    assert.equal(first.windowsOracle, "not-run");
    for (const architecture of ["i386", "amd64"]) {
      for (const [name, expected] of Object.entries(ordinalContract.architectures[architecture].artifacts)) {
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

test("ordinal definition, linker and byte failures cannot produce success evidence", () => {
  const directory = mkdtempSync(new URL("negative-", outputRoot));
  try {
    const alteredDefinition = structuredClone(ordinalContract);
    alteredDefinition.sources["exports.def"].sha256 = "0".repeat(64);
    assert.throws(() => buildOrdinalFixtures(join(directory, "source"), { contract: alteredDefinition }), /exports.def source SHA-256 mismatch/);
    const sources = join(directory, "sources");
    mkdirSync(sources);
    for (const name of ["probe.c", "imports.c"]) {
      writeFileSync(join(sources, name), readFileSync(new URL(`../corpus/pe-ordinal-imports/${name}`, import.meta.url)));
    }
    const invalid = "EXPORTS\n ring3_probe @invalid NONAME\n";
    writeFileSync(join(sources, "exports.def"), invalid);
    const invalidContract = structuredClone(ordinalContract);
    invalidContract.sources["exports.def"].sha256 = sha256(invalid);
    assert.throws(() => buildOrdinalFixtures(join(directory, "linker"), { sourceDirectory: sources, contract: invalidContract }), /failed:/);
    const wrongBytes = structuredClone(ordinalContract);
    wrongBytes.architectures.amd64.artifacts["Ring3Ordinal.dll"].sha256 = "0".repeat(64);
    assert.throws(() => buildOrdinalFixtures(join(directory, "expectation"), { contract: wrongBytes }), /artifact SHA-256 mismatch/);
    for (const name of ["source", "linker", "expectation"]) {
      assert.equal(existsSync(join(directory, name, "evidence.json")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("ordinal producer preserves existing outputs and refuses linked ancestors", () => {
  const directory = mkdtempSync(new URL("paths-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-ordinal-corpus-outside-"));
  try {
    writeFileSync(join(directory, "sentinel"), "existing output");
    assert.throws(() => buildOrdinalFixtures(directory), { code: "EEXIST" });
    assert.equal(readFileSync(join(directory, "sentinel"), "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildOrdinalFixtures(join(directory, "redirect", "fixture")), /real directories/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("import corpus CLI refuses unknown or extra family arguments", () => {
  for (const args of [["--unknown"], ["--ordinal", "extra"]]) {
    const result = spawnSync(process.execPath, [fileURLToPath(new URL("./build-import-corpus.mjs", import.meta.url)), ...args], { encoding: "utf8" });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /unsupported corpus arguments/);
    assert.equal(result.stdout, "");
  }
});
