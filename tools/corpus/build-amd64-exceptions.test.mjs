import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildAmd64ExceptionFixtures, contract } from "./build-amd64-exceptions.mjs";
import { sha256 } from "./shared.mjs";

const root = new URL("../../target/amd64-exceptions-corpus-tests/", import.meta.url);
mkdirSync(root, { recursive: true });

test("amd64 exception fixtures repeat exact source, object, DLL, library and inspection bytes", () => {
  const directory = mkdtempSync(new URL("repeat-", root));
  try {
    const first = buildAmd64ExceptionFixtures(join(directory, "first"));
    const second = buildAmd64ExceptionFixtures(join(directory, "second"));
    assert.deepEqual(first, second);
    assert.equal(first.executed, false);
    assert.equal(first.windowsOracle, "not-run");
    for (const name of ["two-functions", "leaf-only"]) {
      for (const [artifact, expected] of Object.entries(contract.configurations[name].artifacts)) {
        const a = readFileSync(join(directory, "first", name, artifact));
        assert.deepEqual(a, readFileSync(join(directory, "second", name, artifact)));
        assert.equal(a.length, expected.bytes);
        assert.equal(sha256(a), expected.sha256);
        if (artifact.endsWith(".obj")) assert.equal(a.readUInt32LE(4), 0);
      }
      for (const artifact of ["probe.s", "inspection.txt"]) {
        assert.deepEqual(readFileSync(join(directory, "first", name, artifact)), readFileSync(join(directory, "second", name, artifact)));
      }
    }
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("amd64 exception producer refuses source, tool, command and expected metadata failures", () => {
  const directory = mkdtempSync(new URL("negative-", root));
  const attempted = [];
  const reject = (name, options, pattern) => {
    attempted.push(name);
    assert.throws(() => buildAmd64ExceptionFixtures(join(directory, name), options), pattern);
  };
  try {
    const source = structuredClone(contract);
    source.sources["two-functions.s"].sha256 = "0".repeat(64);
    reject("source", { contract: source }, /source SHA-256 mismatch/);
    reject("tool", { tools: { clang: process.execPath } }, /clang SHA-256 mismatch/);
    const version = structuredClone(contract);
    version.tools.clang.version = "wrong version";
    reject("version", { contract: version }, /version mismatch/);
    for (const [name, body] of [["compiler", "invalid_assembly_instruction\n"], ["linker", ".text\n.globl frame_small\nframe_small:\n callq missing_symbol\n retq\n.globl frame_saved\nframe_saved:\n retq\n"]]) {
      const sourceDirectory = join(directory, name + "-source");
      mkdirSync(sourceDirectory);
      writeFileSync(join(sourceDirectory, "two-functions.s"), body);
      writeFileSync(join(sourceDirectory, "leaf-only.s"), readFileSync(new URL("../../corpus/pe-amd64-exceptions/leaf-only.s", import.meta.url)));
      const spec = structuredClone(contract);
      spec.sources["two-functions.s"].sha256 = sha256(body);
      reject(name, { sourceDirectory, contract: spec }, /failed:/);
    }
    for (const artifact of ["probe.obj", "Probe.dll", "Probe.lib"]) {
      const spec = structuredClone(contract);
      spec.configurations["two-functions"].artifacts[artifact].sha256 = "0".repeat(64);
      reject(artifact, { contract: spec }, /artifact SHA-256 mismatch/);
    }
    const size = structuredClone(contract);
    size.configurations["leaf-only"].artifacts["Probe.dll"].bytes++;
    reject("size", { contract: size }, /artifact size mismatch/);
    const inspection = structuredClone(contract);
    inspection.configurations["two-functions"].inspection.sha256 = "0".repeat(64);
    reject("inspection", { contract: inspection }, /inspection SHA-256 mismatch/);
    const directoryMetadata = structuredClone(contract);
    directoryMetadata.configurations["two-functions"].metadata.directory.rva++;
    reject("directory", { contract: directoryMetadata }, /raw exception metadata mismatch/);
    const record = structuredClone(contract);
    record.configurations["two-functions"].metadata.entries[0].unwindInfoRva++;
    reject("record", { contract: record }, /raw exception metadata mismatch/);
    for (const name of attempted) assert.equal(existsSync(join(directory, name, "evidence.json")), false);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("amd64 exception producer preserves existing outputs and refuses linked or outside paths", () => {
  const directory = mkdtempSync(new URL("paths-", root));
  const outside = mkdtempSync(join(tmpdir(), "ring3-exceptions-outside-"));
  try {
    writeFileSync(join(directory, "sentinel"), "existing output");
    assert.throws(() => buildAmd64ExceptionFixtures(directory), { code: "EEXIST" });
    assert.equal(readFileSync(join(directory, "sentinel"), "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildAmd64ExceptionFixtures(join(directory, "redirect", "fixture")), /real directories/);
    assert.throws(() => buildAmd64ExceptionFixtures(join(outside, "fixture")), /fresh directory under target/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("amd64 exception CLI rejects extra arguments before output creation", () => {
  const output = new URL("../../target/corpus-amd64-exceptions/", import.meta.url);
  const before = existsSync(output) ? readdirSync(output).sort() : null;
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("./build-amd64-exceptions.mjs", import.meta.url)), "extra"], { encoding: "utf8", timeout: 5000, maxBuffer: 1024 * 1024 });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unsupported corpus arguments/);
  assert.equal(result.stdout, "");
  assert.deepEqual(existsSync(output) ? readdirSync(output).sort() : null, before);
});
