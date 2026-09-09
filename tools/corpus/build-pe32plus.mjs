import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { locateTools, prepareOutputParents, root, run, sha256, target } from "./shared.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe32plus-arithmetic/fixture.json"), "utf8"));
const artifacts = ["arithmetic.exe", "arithmetic.obj"];

export function buildPe32PlusFixture(outputDirectory, options = {}) {
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  const spec = options.contract ?? contract;
  assert.equal(spec.schemaVersion, 1);
  const source = readFileSync(options.sourcePath ?? join(root, "corpus/pe32plus-arithmetic/pe32plus-arithmetic.s"));
  assert.equal(sha256(source), spec.source.sha256, "source SHA-256 mismatch");
  const tools = { ...locateTools(), ...options.tools };
  const versions = {};
  for (const name of ["clang", "lld", "objdump"]) {
    assert.equal(sha256(readFileSync(tools[name])), spec.tools[name].sha256, `${name} SHA-256 mismatch`);
    versions[name] = run(tools[name], name === "lld" ? ["-flavor", "link", "--version"] : ["--version"]).trim();
    assert.ok(versions[name].includes(spec.tools[name].version), `${name} version mismatch`);
  }

  writeFileSync(join(output, "arithmetic.s"), source);
  const commands = {
    clang: ["--target=x86_64-pc-windows-msvc", "-c", "arithmetic.s", "-o", "arithmetic.obj"],
    lld: ["-flavor", "link", "/entry:entry", "/subsystem:console", "/machine:x64", "/nodefaultlib", "/base:0x140000000", "/fixed", "/dynamicbase:no", "/nxcompat", "/timestamp:0", "/stack:4294975488,4294971392", "/heap:8589946880,8589938688", "/out:arithmetic.exe", "arithmetic.obj"],
    objdump: ["--private-headers", "--section-headers", "--disassemble", "arithmetic.exe"],
  };
  run(tools.clang, commands.clang, output);
  run(tools.lld, commands.lld, output);
  const identities = {};
  for (const name of artifacts) {
    const bytes = readFileSync(join(output, name));
    assert.equal(sha256(bytes), spec.artifacts[name].sha256, `${name} artifact SHA-256 mismatch`);
    assert.equal(bytes.length, spec.artifacts[name].bytes, `${name} artifact size mismatch`);
    identities[name] = { bytes: bytes.length, sha256: sha256(bytes) };
  }
  const inspection = run(tools.objdump, commands.objdump, output);
  assert.ok(inspection.includes(`file format ${spec.expectation.format}`), "LLVM format mismatch");
  const normalized = inspection.replace(/[\t ]+/g, " ");
  for (const [name, value] of Object.entries(spec.expectation.headerFields)) {
    assert.ok(normalized.includes(`${name} ${value}`), `LLVM ${name} mismatch`);
  }
  writeFileSync(join(output, "inspection.txt"), inspection);
  const evidence = {
    ...spec,
    environment: { platform: process.platform, architecture: process.arch, node: process.versions.node, locale: "C", timezone: "UTC" },
    toolVersions: versions,
    commands,
    artifacts: identities,
    inspection: { path: "inspection.txt", sha256: sha256(inspection) },
  };
  writeFileSync(join(output, "evidence.json"), `${JSON.stringify(evidence, null, 2)}\n`);
  return evidence;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    assert.equal(process.argv.length, 2, "unsupported corpus arguments");
    const outputRoot = join(target, "corpus-pe32plus");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = buildPe32PlusFixture(join(output, "first"));
    const second = buildPe32PlusFixture(join(output, "second"));
    assert.deepEqual(first, second);
    for (const name of artifacts) {
      assert.deepEqual(readFileSync(join(output, "first", name)), readFileSync(join(output, "second", name)), `${name} repeatability mismatch`);
    }
    const fixtures = { RING3_PE32PLUS_FIXTURE: join(output, "first/arithmetic.exe") };
    writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json" }, null, 2)}\n`);
    console.log(`PE32+ header fixture verified.\nEvidence: ${output}\nFixture paths: ${join(output, "fixtures.json")}`);
  } catch (error) {
    console.error(`[ring3 PE32+ corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
