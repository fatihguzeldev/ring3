import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { locateTools, prepareOutputParents, root, run, sha256, target } from "./shared.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe-load-config/fixture.json"), "utf8"));
const artifacts = ["load-config.obj", "load-config.exe"];
const architectures = [
  ["i386", "i686-pc-windows-msvc", "x86", "0x400000", "coff-i386"],
  ["amd64", "x86_64-pc-windows-msvc", "x64", "0x140000000", "coff-x86-64"],
];

export function buildLoadConfigFixtures(outputDirectory, options = {}) {
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  const spec = options.contract ?? contract;
  assert.equal(spec.schemaVersion, 1);
  const snapshots = Object.fromEntries(architectures.map(([architecture]) => {
    const name = `${architecture}.s`;
    const bytes = readFileSync(join(options.sourceDirectory ?? join(root, "corpus/pe-load-config"), name));
    assert.equal(sha256(bytes), spec.sources[name].sha256, `${name} source SHA-256 mismatch`);
    return [architecture, bytes];
  }));
  const tools = locateTools(options.tools);
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
    writeFileSync(join(directory, "load-config.s"), snapshots[architecture]);
    const commands = {
      clang: [`--target=${triple}`, "-c", "load-config.s", "-o", "load-config.obj"],
      lld: ["-flavor", "link", `/machine:${machine}`, "/nodefaultlib", "/timestamp:0", "/fixed:no", "/dynamicbase", "/nxcompat", `/base:${base}`, "/entry:entry", "/subsystem:console", "/out:load-config.exe", "load-config.obj"],
      objdump: ["--private-headers", "--section-headers", "--full-contents", "load-config.exe"],
    };
    if (architecture === "i386") commands.lld.push("/safeseh:no");
    run(tools.clang, commands.clang, directory);
    run(tools.lld, commands.lld, directory);
    const identities = {};
    for (const name of artifacts) {
      const bytes = readFileSync(join(directory, name));
      const expected = spec.architectures[architecture].artifacts[name];
      assert.equal(sha256(bytes), expected.sha256, `${architecture}/${name} artifact SHA-256 mismatch`);
      assert.equal(bytes.length, expected.bytes, `${architecture}/${name} artifact size mismatch`);
      identities[name] = { bytes: bytes.length, sha256: sha256(bytes) };
    }
    const inspection = run(tools.objdump, commands.objdump, directory);
    assert.ok(inspection.includes(`file format ${format}`), "LLVM format mismatch");
    const address = spec.architectures[architecture].directory.rva.toString(16).padStart(architecture === "amd64" ? 16 : 8, "0");
    const size = spec.architectures[architecture].directory.size.toString(16).padStart(8, "0");
    assert.ok(inspection.includes(`Entry a ${address} ${size} Load Configuration Directory`), "LLVM load-config directory mismatch");
    const expected = spec.architectures[architecture].directory;
    const prefix = Buffer.alloc(24);
    prefix.writeUInt32LE(expected.structureSize, 0);
    prefix.writeUInt32LE(expected.timeDateStamp, 4);
    prefix.writeUInt16LE(expected.majorVersion, 8);
    prefix.writeUInt16LE(expected.minorVersion, 10);
    prefix.writeUInt32LE(expected.globalFlagsClear, 12);
    prefix.writeUInt32LE(expected.globalFlagsSet, 16);
    prefix.writeUInt32LE(expected.criticalSectionDefaultTimeout, 20);
    const executable = readFileSync(join(directory, "load-config.exe"));
    assert.deepEqual(executable.subarray(expected.fileOffset, expected.fileOffset + 24), prefix, "load-config prefix mismatch");
    assert.deepEqual(executable.subarray(expected.fileOffset + 24, expected.fileOffset + expected.size), Buffer.alloc(expected.size - 24), "load-config tail mismatch");
    writeFileSync(join(directory, "inspection.txt"), inspection);
    builds[architecture] = {
      target: triple,
      commands,
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
    const outputRoot = join(target, "corpus-load-config");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = buildLoadConfigFixtures(join(output, "first"));
    const second = buildLoadConfigFixtures(join(output, "second"));
    assert.deepEqual(first, second);
    for (const [architecture] of architectures) {
      for (const name of ["load-config.s", ...artifacts, "inspection.txt"]) {
        assert.deepEqual(readFileSync(join(output, "first", architecture, name)), readFileSync(join(output, "second", architecture, name)), `${architecture}/${name} repeatability mismatch`);
      }
    }
    const fixtures = {
      RING3_LOAD_CONFIG_PE32_FIXTURE: join(output, "first/i386/load-config.exe"),
      RING3_LOAD_CONFIG_PE32PLUS_FIXTURE: join(output, "first/amd64/load-config.exe"),
    };
    writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json" }, null, 2)}\n`);
    console.log(`Load-config PE32 and PE32+ fixtures verified.\nEvidence: ${output}\nFixture paths: ${join(output, "fixtures.json")}`);
  } catch (error) {
    console.error(`[ring3 load-config corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
