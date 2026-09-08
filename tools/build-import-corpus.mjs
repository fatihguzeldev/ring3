import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { locateTools, prepareOutputParents, root, run, sha256, target } from "./corpus-tools.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe-named-imports.json"), "utf8"));
const sources = ["probe.c", "imports.c"];
const artifacts = ["imports.exe", "Ring3Probe.dll", "Ring3Probe.lib", "imports.obj", "probe.obj"];
const architectures = [
  ["i386", "i686-pc-windows-msvc", "x86", "0x10000000", "0x400000", "coff-i386"],
  ["amd64", "x86_64-pc-windows-msvc", "x64", "0x180000000", "0x140000000", "coff-x86-64"],
];

export function buildImportFixtures(outputDirectory, options = {}) {
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  const spec = options.contract ?? contract;
  assert.equal(spec.schemaVersion, 1);
  const snapshots = Object.fromEntries(sources.map((name) => {
    const bytes = readFileSync(join(options.sourceDirectory ?? join(root, "corpus/pe-named-imports"), name));
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
  for (const [architecture, triple, machine, dllBase, exeBase, format] of architectures) {
    const directory = join(output, architecture);
    mkdirSync(directory);
    for (const name of sources) writeFileSync(join(directory, name), snapshots[name]);
    const commands = [];
    for (const source of ["probe", "imports"]) {
      const args = [`--target=${triple}`, "-c", "-O1", "-ffreestanding", "-fno-stack-protector", "-fno-ident", "-mno-incremental-linker-compatible", `${source}.c`, "-o", `${source}.obj`];
      run(tools.clang, args, directory);
      commands.push({ tool: "clang", args });
    }
    const common = ["-flavor", "link", `/machine:${machine}`, "/nodefaultlib", "/timestamp:0", "/fixed", "/dynamicbase:no", "/nxcompat"];
    if (architecture === "i386") common.push("/safeseh:no");
    const dll = [...common, "/dll", "/noentry", `/base:${dllBase}`, "/out:Ring3Probe.dll", "/implib:Ring3Probe.lib", "probe.obj"];
    run(tools.lld, dll, directory);
    commands.push({ tool: "lld", args: dll });
    const exe = [...common, "/entry:entry", "/subsystem:console", `/base:${exeBase}`, "/out:imports.exe", "imports.obj", "Ring3Probe.lib"];
    run(tools.lld, exe, directory);
    commands.push({ tool: "lld", args: exe });

    const identities = {};
    for (const name of artifacts) {
      const bytes = readFileSync(join(directory, name));
      const expected = spec.architectures[architecture].artifacts[name];
      assert.equal(sha256(bytes), expected.sha256, `${architecture}/${name} artifact SHA-256 mismatch`);
      assert.equal(bytes.length, expected.bytes, `${architecture}/${name} artifact size mismatch`);
      identities[name] = { bytes: bytes.length, sha256: sha256(bytes) };
    }
    const inspections = {};
    for (const name of ["imports.exe", "Ring3Probe.dll"]) {
      const args = ["--private-headers", "--section-headers", "--disassemble", name];
      const inspection = run(tools.objdump, args, directory);
      assert.ok(inspection.includes(`file format ${format}`), `${name} LLVM format mismatch`);
      assert.ok(inspection.includes(spec.expectation.symbol), `${name} LLVM symbol mismatch`);
      if (name === "imports.exe") assert.ok(inspection.includes(`DLL Name: ${spec.expectation.dllName}`), "LLVM DLL name mismatch");
      writeFileSync(join(directory, `${name}.inspection.txt`), inspection);
      inspections[name] = { path: `${architecture}/${name}.inspection.txt`, sha256: sha256(inspection) };
      commands.push({ tool: "objdump", args });
    }
    builds[architecture] = { target: triple, commands, artifacts: identities, inspections };
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
    const outputRoot = join(target, "corpus-imports");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = buildImportFixtures(join(output, "first"));
    const second = buildImportFixtures(join(output, "second"));
    assert.deepEqual(first, second);
    for (const [architecture] of architectures) {
      for (const name of artifacts) {
        assert.deepEqual(readFileSync(join(output, "first", architecture, name)), readFileSync(join(output, "second", architecture, name)), `${architecture}/${name} repeatability mismatch`);
      }
    }
    const fixtures = {
      RING3_IMPORT_PE32_FIXTURE: join(output, "first/i386/imports.exe"),
      RING3_IMPORT_PE32PLUS_FIXTURE: join(output, "first/amd64/imports.exe"),
      RING3_EXPORT_PE32_NAMED_DLL: join(output, "first/i386/Ring3Probe.dll"),
      RING3_EXPORT_PE32PLUS_NAMED_DLL: join(output, "first/amd64/Ring3Probe.dll"),
    };
    writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json" }, null, 2)}\n`);
    console.log(`Named PE32 and PE32+ fixtures verified.\nEvidence: ${output}\nFixture paths: ${join(output, "fixtures.json")}`);
  } catch (error) {
    console.error(`[ring3 import corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
