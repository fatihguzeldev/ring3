import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildForwarderFixtures, contract } from "./build-forwarders.mjs";
import { sha256 } from "./shared.mjs";

const outputRoot = new URL("../../target/forwarder-corpus-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("sparse forwarder DLL fixtures preserve pinned bytes across independent builds", () => {
  const directory = mkdtempSync(new URL("repeat-", outputRoot));
  try {
    const first = buildForwarderFixtures(join(directory, "first"));
    const second = buildForwarderFixtures(join(directory, "second"));
    assert.deepEqual(first, second);
    assert.equal(first.executed, false);
    assert.equal(first.windowsOracle, "not-run");
    assert.ok(contract.sources["provider.c"]);
    assert.ok(contract.sources["provider.def"]);
    for (const architecture of ["i386", "amd64"]) {
      for (const name of ["OtherModule.dll", "OtherModule.lib", "provider.obj"]) {
        assert.ok(contract.architectures[architecture].artifacts[name]);
      }
      const inspection = readFileSync(join(directory, "first", architecture, "provider-inspection.txt"), "utf8");
      assert.match(inspection, /OtherModule\.dll/);
      assert.match(inspection, /Ordinal base: 32768/);
      assert.match(inspection, /ring3_target/);
    }
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
    for (const name of Object.keys(contract.sources)) {
      writeFileSync(join(sources, name), readFileSync(new URL(`../../corpus/pe-forwarders/${name}`, import.meta.url)));
    }
    const originalAnchor = readFileSync(new URL("../../corpus/pe-forwarders/anchor.c", import.meta.url));
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

test("named provider source, compile, link, artifacts and inspection failures refuse success evidence", () => {
  const directory = mkdtempSync(new URL("provider-negative-", outputRoot));
  const attempted = [];
  const reject = (name, options, pattern) => {
    attempted.push(name);
    assert.throws(() => buildForwarderFixtures(join(directory, name), options), pattern);
  };
  try {
    for (const name of ["provider.c", "provider.def"]) {
      const spec = structuredClone(contract);
      spec.sources[name].sha256 = "0".repeat(64);
      reject(`source-${name}`, { contract: spec }, /source SHA-256 mismatch/);
    }
    const sources = join(directory, "sources");
    mkdirSync(sources);
    for (const name of Object.keys(contract.sources)) {
      writeFileSync(join(sources, name), readFileSync(new URL(`../../corpus/pe-forwarders/${name}`, import.meta.url)));
    }
    const invalidSource = "invalid provider C source\n";
    writeFileSync(join(sources, "provider.c"), invalidSource);
    const compiler = structuredClone(contract);
    compiler.sources["provider.c"].sha256 = sha256(invalidSource);
    reject("compile", { sourceDirectory: sources, contract: compiler }, /failed:/);
    writeFileSync(join(sources, "provider.c"), readFileSync(new URL("../../corpus/pe-forwarders/provider.c", import.meta.url)));
    const invalidDefinition = "EXPORTS\n missing_provider_symbol @32768\n";
    writeFileSync(join(sources, "provider.def"), invalidDefinition);
    const linker = structuredClone(contract);
    linker.sources["provider.def"].sha256 = sha256(invalidDefinition);
    reject("link", { sourceDirectory: sources, contract: linker }, /failed:/);
    for (const name of ["OtherModule.dll", "OtherModule.lib", "provider.obj"]) {
      const spec = structuredClone(contract);
      spec.architectures.amd64.artifacts[name].sha256 = "0".repeat(64);
      reject(`artifact-${name}`, { contract: spec }, /artifact SHA-256 mismatch/);
    }
    const inspection = structuredClone(contract);
    inspection.expectation.provider.symbol = "not_the_provider_export";
    reject("inspection", { contract: inspection }, /LLVM provider mismatch/);
    for (const name of attempted) assert.equal(existsSync(join(directory, name, "evidence.json")), false);
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
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("./build-forwarders.mjs", import.meta.url)), "extra"], { encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unsupported corpus arguments/);
  assert.equal(result.stdout, "");
});
