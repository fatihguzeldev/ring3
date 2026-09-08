import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { locateTools, prepareOutputParents, root, run, sha256, target } from "./corpus-tools.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe-forwarders/fixture.json"), "utf8"));
const sources = ["anchor.c", "forwarders.def"];
const artifacts = ["Ring3Forwarders.dll", "Ring3Forwarders.lib", "anchor.obj"];
const architectures = [
  ["i386", "i686-pc-windows-msvc", "x86", "0x10000000", "coff-i386"],
  ["amd64", "x86_64-pc-windows-msvc", "x64", "0x180000000", "coff-x86-64"],
];

export function buildForwarderFixtures(outputDirectory, options = {}) {
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  const spec = options.contract ?? contract;
  assert.equal(spec.schemaVersion, 1);
  const snapshots = Object.fromEntries(sources.map((name) => {
    const bytes = readFileSync(join(options.sourceDirectory ?? join(root, "corpus/pe-forwarders"), name));
    assert.equal(sha256(bytes), spec.sources[name].sha256, `${name} source SHA-256 mismatch`);
    return [name, bytes];
  }));
  const tools = { ...locateTools(), ...options.tools };
  const versions = {};
  for (const name of ["clang", "lld", "objdump"]) {
    assert.equal(sha256(readFileSync(tools[name])), spec.tools[name].sha256, `${name} SHA-256 mismatch`);
    versions[name] = run(tools[name], name === "lld" ? ["-flavor", "link", "--version"] : ["--version"]).trim();
    assert.ok(versions[name].includes(spec.tools[name].version), `${name} version mismatch`);
  }

  const builds = {};
  for (const [architecture, triple, machine, base, format] of architectures) {
    const directory = join(output, architecture);
    mkdirSync(directory);
    for (const name of sources) writeFileSync(join(directory, name), snapshots[name]);
    const compile = [`--target=${triple}`, "-c", "-O1", "-ffreestanding", "-fno-stack-protector", "-fno-ident", "-mno-incremental-linker-compatible", "anchor.c", "-o", "anchor.obj"];
    run(tools.clang, compile, directory);
    const link = ["-flavor", "link", `/machine:${machine}`, "/nodefaultlib", "/timestamp:0", "/fixed", "/dynamicbase:no", "/nxcompat", "/dll", "/noentry", `/base:${base}`, "/out:Ring3Forwarders.dll", "/implib:Ring3Forwarders.lib", "/def:forwarders.def", "anchor.obj"];
    if (architecture === "i386") link.push("/safeseh:no");
    run(tools.lld, link, directory);
    const identities = {};
    for (const name of artifacts) {
      const bytes = readFileSync(join(directory, name));
      const expected = spec.architectures[architecture].artifacts[name];
      assert.equal(sha256(bytes), expected.sha256, `${architecture}/${name} artifact SHA-256 mismatch`);
      assert.equal(bytes.length, expected.bytes, `${architecture}/${name} artifact size mismatch`);
      identities[name] = { bytes: bytes.length, sha256: sha256(bytes) };
    }
    const inspect = ["--private-headers", "--section-headers", "--full-contents", "Ring3Forwarders.dll"];
    const inspection = run(tools.objdump, inspect, directory);
    assert.ok(inspection.includes(`file format ${format}`), "LLVM format mismatch");
    for (const text of spec.expectation.forwarders) assert.ok(inspection.includes(text), "LLVM forwarder mismatch");
    writeFileSync(join(directory, "inspection.txt"), inspection);
    builds[architecture] = {
      target: triple,
      commands: [{ tool: "clang", args: compile }, { tool: "lld", args: link }, { tool: "objdump", args: inspect }],
      artifacts: identities,
      inspection: { path: `${architecture}/inspection.txt`, sha256: sha256(inspection) },
    };
  }
  const evidence = {
    ...spec,
    environment: { platform: process.platform, architecture: process.arch, node: process.versions.node, locale: "C", timezone: "UTC" },
    toolVersions: versions,
    builds,
  };
  writeFileSync(join(output, "evidence.json"), `${JSON.stringify(evidence, null, 2)}\n`);
  return evidence;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    assert.equal(process.argv.length, 2, "unsupported corpus arguments");
    const outputRoot = join(target, "corpus-forwarders");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = buildForwarderFixtures(join(output, "first"));
    const second = buildForwarderFixtures(join(output, "second"));
    assert.deepEqual(first, second);
    for (const [architecture] of architectures) {
      for (const name of artifacts) {
        assert.deepEqual(readFileSync(join(output, "first", architecture, name)), readFileSync(join(output, "second", architecture, name)), `${architecture}/${name} repeatability mismatch`);
      }
    }
    const fixtures = {
      RING3_EXPORT_FORWARD_PE32_FIXTURE: join(output, "first/i386/Ring3Forwarders.dll"),
      RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE: join(output, "first/amd64/Ring3Forwarders.dll"),
    };
    writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json" }, null, 2)}\n`);
    console.log(`Sparse forwarder PE32 and PE32+ fixtures verified.\nEvidence: ${output}\nFixture paths: ${join(output, "fixtures.json")}`);
  } catch (error) {
    console.error(`[ring3 forwarder corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
