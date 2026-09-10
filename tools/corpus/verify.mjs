import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, writeFileSync } from "node:fs";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { buildFixture } from "./build-pe32.mjs";
import { buildImportFixtures, buildOrdinalFixtures } from "./build-imports.mjs";
import { buildForwarderFixtures } from "./build-forwarders.mjs";
import { buildPe32PlusFixture } from "./build-pe32plus.mjs";
import { buildRelocationFixtures } from "./build-relocations.mjs";
import { buildTlsFixtures } from "./build-tls.mjs";
import { buildDelayImportFixtures } from "./build-delay-imports.mjs";
import { buildResourceFixtures } from "./build-resources.mjs";
import { buildDebugFixtures } from "./build-debug-payloads.mjs";
import { buildManagedFixtures } from "./build-managed.mjs";
import { prepareOutputParents, root, run, sha256, target } from "./shared.mjs";

export const inventory = JSON.parse(readFileSync(join(root, "corpus/real-file-tests.json"), "utf8"));
const binaryName = /^[A-Za-z_][A-Za-z0-9_-]*$/;
const testName = /^[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*$/;
const compare = (a, b) => a < b ? -1 : a > b ? 1 : 0;
const compareCases = (a, b) => compare(a.binary, b.binary) || compare(a.test, b.test);
const lines = (text) => text.split(/\r?\n/).filter((line) => line.length > 0);

export function flattenInventory(spec) {
  assert.equal(spec.schemaVersion, 1, "unsupported test inventory schema");
  const cases = [];
  for (const [binary, names] of Object.entries(spec.tests)) {
    assert.ok(binaryName.test(binary), "invalid test binary name");
    assert.ok(Array.isArray(names) && names.length > 0, "empty test binary inventory");
    assert.equal(new Set(names).size, names.length, "duplicate inventory test");
    for (const name of names) {
      assert.ok(typeof name === "string" && testName.test(name), "invalid test name");
      cases.push({ binary, test: name });
    }
  }
  assert.ok(cases.length > 0, "empty test inventory");
  return cases.sort(compareCases);
}

export function parseTestList(stdout) {
  const output = lines(stdout);
  const summary = /^(\d+) tests?, 0 benchmarks$/.exec(output.pop());
  assert.ok(summary, "invalid ignored-test list summary");
  const names = output.map((line) => {
    assert.ok(line.endsWith(": test"), "invalid ignored-test list entry");
    const name = line.slice(0, -6);
    assert.ok(testName.test(name), "invalid listed test name");
    return name;
  });
  assert.equal(names.length, Number(summary[1]), "ignored-test list count mismatch");
  assert.equal(new Set(names).size, names.length, "duplicate ignored test");
  return names;
}

export function verifyTestResult(result, name) {
  assert.ok(!result.error && !result.signal, "test process did not finish normally");
  assert.equal(result.status, 0, "test process failed");
  const output = lines(result.stdout);
  assert.equal(output.length, 3, "incomplete or ambiguous test output");
  assert.equal(output[0], "running 1 test", "expected exactly one executed test");
  assert.equal(output[1], `test ${name} ... ok`, "expected test did not pass");
  assert.match(output[2], /^test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; \d+ filtered out; finished in \d+(?:\.\d+)?s$/, "invalid test success summary");
}

function sourceHashes() {
  const paths = [".node-version", "rust-toolchain.toml", "Cargo.toml", "Cargo.lock", "core/Cargo.toml"];
  function walk(directory) {
    for (const entry of readdirSync(join(root, directory), { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (/\.(rs|mjs|json|s|c|cs|def)$/.test(entry.name)) {
        assert.ok(entry.isFile(), "source inputs must be regular files");
        paths.push(path);
      }
    }
  }
  for (const directory of ["core/src", "core/tests", "corpus", "tools"]) walk(directory);
  return Object.fromEntries(paths.sort().map((path) => [path, sha256(readFileSync(join(root, path)))]));
}

const families = [
  { name: "pe32", build: buildFixture, fixtures: { RING3_PE32_FIXTURE: "pe32-arithmetic.exe" } },
  { name: "pe32plus", build: buildPe32PlusFixture, fixtures: { RING3_PE32PLUS_FIXTURE: "arithmetic.exe" } },
  { name: "named", build: buildImportFixtures, fixtures: {
    RING3_IMPORT_PE32_FIXTURE: "i386/imports.exe", RING3_IMPORT_PE32PLUS_FIXTURE: "amd64/imports.exe",
    RING3_EXPORT_PE32_NAMED_DLL: "i386/Ring3Probe.dll", RING3_EXPORT_PE32PLUS_NAMED_DLL: "amd64/Ring3Probe.dll",
  } },
  { name: "ordinal", build: buildOrdinalFixtures, fixtures: {
    RING3_ORDINAL_PE32_FIXTURE: "i386/imports.exe", RING3_ORDINAL_PE32PLUS_FIXTURE: "amd64/imports.exe",
    RING3_EXPORT_PE32_ORDINAL_DLL: "i386/Ring3Ordinal.dll", RING3_EXPORT_PE32PLUS_ORDINAL_DLL: "amd64/Ring3Ordinal.dll",
  } },
  { name: "forwarder", build: buildForwarderFixtures, fixtures: {
    RING3_EXPORT_FORWARD_PE32_FIXTURE: "i386/Ring3Forwarders.dll",
    RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE: "amd64/Ring3Forwarders.dll",
  } },
  { name: "relocations", build: buildRelocationFixtures, fixtures: {
    RING3_RELOCATION_PE32_FIXTURE: "i386/relocations.exe",
    RING3_RELOCATION_PE32PLUS_FIXTURE: "amd64/relocations.exe",
  } },
  { name: "tls", build: buildTlsFixtures, fixtures: {
    RING3_TLS_PE32_FIXTURE: "i386/tls.exe",
    RING3_TLS_PE32PLUS_FIXTURE: "amd64/tls.exe",
  } },
  { name: "delay", build: buildDelayImportFixtures, fixtures: {
    RING3_DELAY_PE32_FIXTURE: "i386/delayed.exe",
    RING3_DELAY_PE32PLUS_FIXTURE: "amd64/delayed.exe",
  } },
  { name: "delay-ordinal", build: (directory) => buildDelayImportFixtures(directory, { ordinal: true }), fixtures: {
    RING3_DELAY_ORDINAL_PE32_FIXTURE: "i386/delayed.exe",
    RING3_DELAY_ORDINAL_PE32PLUS_FIXTURE: "amd64/delayed.exe",
  } },
  { name: "debug", build: buildDebugFixtures, fixtures: {
    RING3_DEBUG_PE32_FIXTURE: "i386/debug.exe",
    RING3_DEBUG_PE32PLUS_FIXTURE: "amd64/debug.exe",
  } },
  { name: "managed", build: buildManagedFixtures, fixtures: {
    RING3_CLR_PE32_FIXTURE: "x86/Probe.exe",
    RING3_CLR_PE32PLUS_FIXTURE: "x64/Probe.exe",
  } },
  { name: "resources", build: buildResourceFixtures, fixtures: {
    RING3_RESOURCE_PE32_FIXTURE: "i386/resources.exe",
    RING3_RESOURCE_PE32PLUS_FIXTURE: "amd64/resources.exe",
  } },
];

export function verifyCorpus(outputDirectory, options = {}) {
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const spec = options.inventory ?? inventory;
  const planned = flattenInventory(spec);
  assert.equal(spec.host, "aarch64-apple-darwin", "unsupported native corpus host");
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  mkdirSync(join(output, "logs"));
  const sources = sourceHashes();
  const commands = [];
  function capture(command, args, env, label, timeoutMs = 15_000) {
    const result = spawnSync(command, args, { cwd: root, env, encoding: "utf8", timeout: timeoutMs, maxBuffer: 1024 * 1024 });
    const stdout = result.stdout ?? "";
    const stderr = result.stderr ?? "";
    const logs = {};
    for (const [stream, text] of Object.entries({ stdout, stderr })) {
      const path = `logs/${label}.${stream}.txt`;
      writeFileSync(join(output, path), text);
      logs[stream] = { path, sha256: sha256(text) };
    }
    const record = { command, args, timeoutMs, status: result.status, signal: result.signal, error: result.error?.message ?? null, logs };
    commands.push(record);
    writeFileSync(join(output, "commands.json"), `${JSON.stringify(commands, null, 2)}\n`);
    return { ...result, stdout, stderr };
  }
  function requireSuccess(result, stage) {
    assert.ok(!result.error && !result.signal && result.status === 0, `${stage} failed; see retained child logs`);
  }

  const sysroot = run("rustc", ["+1.97.1", "--print", "sysroot"]).trim();
  const rustTools = {};
  for (const name of ["cargo", "rustc"]) {
    const path = join(sysroot, "bin", name);
    const hash = sha256(readFileSync(path));
    assert.equal(hash, spec.tools[name].sha256, `${name} SHA-256 mismatch`);
    const version = run(path, ["--version"]).trim();
    assert.equal(version, spec.tools[name].version, `${name} version mismatch`);
    rustTools[name] = { path, sha256: hash, version };
  }
  assert.ok(run(rustTools.rustc.path, ["-vV"]).split("\n").includes(`host: ${spec.host}`), "rustc host mismatch");
  const cargoDirectory = join(output, "cargo");
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith("RING3_")));
  Object.assign(env, { LC_ALL: "C", TZ: "UTC", CARGO_TARGET_DIR: cargoDirectory, RUSTC: rustTools.rustc.path });
  const compile = capture(rustTools.cargo.path, ["test", "-p", "ring3-core", "--locked", "--offline", "--tests", "--no-run", "--target", spec.host, "--message-format=json"], env, "compile", 60_000);
  requireSuccess(compile, "native test compilation");
  const messages = lines(compile.stdout).map((line) => JSON.parse(line));
  assert.deepEqual(messages.filter((message) => message.reason === "build-finished"), [{ reason: "build-finished", success: true }], "missing successful Cargo completion");
  const binaries = new Map();
  for (const message of messages.filter((message) => message.reason === "compiler-artifact" && message.target.kind.includes("test"))) {
    assert.deepEqual(message.target.kind, ["test"], "invalid integration artifact kind");
    assert.equal(message.profile.test, true, "invalid test artifact profile");
    const name = message.target.name;
    assert.ok(binaryName.test(name) && !binaries.has(name), "invalid or duplicate test artifact");
    assert.ok(typeof message.executable === "string" && isAbsolute(message.executable), "missing test executable");
    const path = realpathSync(message.executable);
    const child = relative(realpathSync(cargoDirectory), path);
    assert.ok(child && child !== ".." && !child.startsWith(`..${sep}`) && !isAbsolute(child) && lstatSync(path).isFile(), "test executable is outside the fresh Cargo target");
    binaries.set(name, { path, sha256: sha256(readFileSync(path)) });
  }
  const actual = [];
  for (const [binary, executable] of [...binaries].sort(([a], [b]) => compare(a, b))) {
    const result = capture(executable.path, ["--ignored", "--list", "--format", "pretty", "--color", "never"], env, `list-${binary}`);
    requireSuccess(result, "test enumeration");
    for (const name of parseTestList(result.stdout)) actual.push({ binary, test: name });
  }
  assert.deepEqual(actual.sort(compareCases), planned, "compiled ignored-test inventory mismatch");

  const fixtures = {};
  const fixtureHashes = {};
  const familyEvidence = {};
  for (const family of families) {
    const directory = join(output, "fixtures", family.name);
    const first = family.build(join(directory, "first"));
    const second = family.build(join(directory, "second"));
    assert.deepEqual(first, second, `${family.name} evidence repeatability mismatch`);
    familyEvidence[family.name] = {};
    for (const repeat of ["first", "second"]) {
      const path = relative(output, join(directory, repeat, "evidence.json"));
      familyEvidence[family.name][repeat] = { path, sha256: sha256(readFileSync(join(output, path))) };
    }
    for (const [variable, path] of Object.entries(family.fixtures)) {
      const firstPath = join(directory, "first", path);
      const bytes = readFileSync(firstPath);
      assert.deepEqual(bytes, readFileSync(join(directory, "second", path)), `${variable} fixture repeatability mismatch`);
      fixtures[variable] = firstPath;
      fixtureHashes[variable] = { bytes: bytes.length, sha256: sha256(bytes) };
    }
  }
  writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
  Object.assign(env, fixtures);
  const cases = [];
  for (const [index, item] of planned.entries()) {
    const result = capture(binaries.get(item.binary).path, [item.test, "--exact", "--ignored", "--format", "pretty", "--color", "never"], env, `case-${index}`);
    verifyTestResult(result, item.test);
    cases.push({ ...item, passed: true, commandIndex: commands.length - 1 });
  }
  assert.deepEqual(sourceHashes(), sources, "source files changed during corpus verification");
  const report = {
    schemaVersion: 1, verified: true, executedGuestCode: false,
    environment: { node: process.versions.node, host: spec.host, locale: "C", timezone: "UTC" },
    inventory: spec, sources, rustTools, binaries: Object.fromEntries(binaries),
    fixtures, fixtureHashes, familyEvidence, commands, cases, passed: cases.length,
  };
  writeFileSync(join(output, "verification.json"), `${JSON.stringify(report, null, 2)}\n`);
  return report;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    assert.equal(process.argv.length, 2, "unsupported corpus arguments");
    const outputRoot = join(target, "corpus-verification");
    prepareOutputParents(join(outputRoot, "run-"));
    const container = mkdtempSync(join(outputRoot, "run-"));
    const output = join(container, "verification");
    const report = verifyCorpus(output);
    console.log(`Verified ${report.passed} real-file parser tests against ${Object.keys(report.fixtures).length} fresh fixtures.\nEvidence: ${output}`);
  } catch (error) {
    console.error(`[ring3 corpus verification] ${error.message}`);
    process.exitCode = 1;
  }
}
