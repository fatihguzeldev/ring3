import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildDebugFixtures, contract } from "./build-debug-payloads.mjs";
import { sha256 } from "./shared.mjs";

const root = new URL("../../target/debug-payloads-corpus-tests/", import.meta.url);
mkdirSync(root, { recursive: true });

test("debug fixtures repeat exact object, executable and PDB bytes across output roots", () => {
  const directory = mkdtempSync(new URL("repeat-", root));
  try {
    const first = buildDebugFixtures(join(directory, "first"));
    const second = buildDebugFixtures(join(directory, "second"));
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
      for (const name of ["debug.c", "inspection.txt"]) {
        assert.deepEqual(readFileSync(join(directory, "first", architecture, name)), readFileSync(join(directory, "second", architecture, name)));
      }
    }
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("debug source, tool, compiler, linker and identity mismatches refuse success evidence", () => {
  const directory = mkdtempSync(new URL("negative-", root));
  const attempted = [];
  const reject = (name, options, pattern) => {
    attempted.push(name);
    assert.throws(() => buildDebugFixtures(join(directory, name), options), pattern);
  };
  try {
    const source = structuredClone(contract);
    source.source.sha256 = "0".repeat(64);
    reject("source", { contract: source }, /source SHA-256 mismatch/);
    reject("tool", { tools: { clang: process.execPath } }, /clang SHA-256 mismatch/);
    for (const [name, body] of [["compiler", "invalid c source\n"], ["linker", "extern int missing(void); int entry(void){return missing();}\n"]]) {
      const path = join(directory, name + ".c");
      writeFileSync(path, body);
      const spec = structuredClone(contract);
      spec.source.sha256 = sha256(body);
      reject(name, { sourcePath: path, contract: spec }, /failed:/);
    }
    const version = structuredClone(contract);
    version.tools.clang.version = "wrong version";
    reject("version", { contract: version }, /version mismatch/);
    for (const artifact of ["debug.obj", "debug.exe", "debug.pdb"]) {
      const spec = structuredClone(contract);
      spec.architectures.amd64.artifacts[artifact].sha256 = "0".repeat(64);
      reject(artifact, { contract: spec }, /artifact SHA-256 mismatch/);
    }
    const size = structuredClone(contract);
    size.architectures.i386.artifacts["debug.exe"].bytes++;
    reject("size", { contract: size }, /artifact size mismatch/);
    const inspection = structuredClone(contract);
    inspection.architectures.i386.inspection.sha256 = "0".repeat(64);
    reject("inspection", { contract: inspection }, /inspection SHA-256 mismatch/);
    const metadata = structuredClone(contract);
    metadata.architectures.amd64.metadata.entries[0][3]++;
    reject("metadata", { contract: metadata }, /raw debug metadata mismatch/);
    const payload = structuredClone(contract);
    payload.architectures.i386.metadata.payloads[0][4] = "00";
    reject("payload", { contract: payload }, /raw debug metadata mismatch/);
    for (const name of attempted) assert.equal(existsSync(join(directory, name, "evidence.json")), false);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("debug producer preserves existing outputs and refuses linked or outside paths", () => {
  const directory = mkdtempSync(new URL("paths-", root));
  const outside = mkdtempSync(join(tmpdir(), "ring3-debug-outside-"));
  try {
    writeFileSync(join(directory, "sentinel"), "existing output");
    assert.throws(() => buildDebugFixtures(directory), { code: "EEXIST" });
    assert.equal(readFileSync(join(directory, "sentinel"), "utf8"), "existing output");
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildDebugFixtures(join(directory, "redirect", "fixture")), /real directories/);
    assert.deepEqual(readdirSync(outside), []);
    assert.throws(() => buildDebugFixtures(join(outside, "fixture")), /fresh directory under target/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("debug CLI refuses extra arguments before output creation", () => {
  const output = new URL("../../target/corpus-debug-payloads/", import.meta.url);
  const before = existsSync(output) ? readdirSync(output).sort() : null;
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("./build-debug-payloads.mjs", import.meta.url)), "extra"], { encoding: "utf8", timeout: 5000, maxBuffer: 1024 * 1024 });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unsupported corpus arguments/);
  assert.equal(result.stdout, "");
  assert.deepEqual(existsSync(output) ? readdirSync(output).sort() : null, before);
});
