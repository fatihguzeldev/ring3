import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { locateTools, prepareOutputParents, root, run, sha256, target } from "./shared.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe-named-imports/fixture.json"), "utf8"));
export const ordinalContract = JSON.parse(readFileSync(join(root, "corpus/pe-ordinal-imports/fixture.json"), "utf8"));
const artifactNames = (library) => ["imports.exe", `${library}.dll`, `${library}.lib`, "imports.obj", "probe.obj"];
const architectures = [
  ["i386", "i686-pc-windows-msvc", "x86", "0x10000000", "0x400000", "coff-i386"],
  ["amd64", "x86_64-pc-windows-msvc", "x64", "0x180000000", "0x140000000", "coff-x86-64"],
];

export function buildImportFixtures(outputDirectory, options = {}) {
  return buildFixtures(outputDirectory, "named", options);
}

export function buildOrdinalFixtures(outputDirectory, options = {}) {
  return buildFixtures(outputDirectory, "ordinal", options);
}

function buildFixtures(outputDirectory, family, options) {
  const ordinal = family === "ordinal";
  const sources = ordinal ? ["probe.c", "imports.c", "exports.def"] : ["probe.c", "imports.c"];
  const library = ordinal ? "Ring3Ordinal" : "Ring3Probe";
  const artifacts = artifactNames(library);
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  const spec = options.contract ?? (ordinal ? ordinalContract : contract);
  assert.equal(spec.schemaVersion, 1);
  const snapshots = Object.fromEntries(sources.map((name) => {
    const bytes = readFileSync(join(options.sourceDirectory ?? join(root, `corpus/pe-${family}-imports`), name));
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
    const dll = [...common, "/dll", "/noentry", `/base:${dllBase}`, `/out:${library}.dll`, `/implib:${library}.lib`];
    if (ordinal) dll.push("/def:exports.def");
    dll.push("probe.obj");
    run(tools.lld, dll, directory);
    commands.push({ tool: "lld", args: dll });
    const exe = [...common, "/entry:entry", "/subsystem:console", `/base:${exeBase}`, "/out:imports.exe", "imports.obj", `${library}.lib`];
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
    for (const name of ["imports.exe", `${library}.dll`]) {
      const args = ordinal && name.endsWith(".dll")
        ? ["--section-headers", "--full-contents", name]
        : ["--private-headers", "--section-headers", "--disassemble", name];
      const inspection = run(tools.objdump, args, directory);
      assert.ok(inspection.includes(`file format ${format}`), `${name} LLVM format mismatch`);
      if (!ordinal) assert.ok(inspection.includes(spec.expectation.symbol), `${name} LLVM symbol mismatch`);
      if (ordinal && name === "imports.exe") assert.ok(inspection.includes(String(spec.expectation.ordinal)), "LLVM import ordinal mismatch");
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
    assert.ok(process.argv.length <= 3 && [undefined, "--ordinal"].includes(process.argv[2]), "unsupported corpus arguments");
    const ordinal = process.argv[2] === "--ordinal";
    const build = ordinal ? buildOrdinalFixtures : buildImportFixtures;
    const outputRoot = join(target, ordinal ? "corpus-ordinals" : "corpus-imports");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = build(join(output, "first"));
    const second = build(join(output, "second"));
    assert.deepEqual(first, second);
    for (const [architecture] of architectures) {
      for (const name of artifactNames(ordinal ? "Ring3Ordinal" : "Ring3Probe")) {
        assert.deepEqual(readFileSync(join(output, "first", architecture, name)), readFileSync(join(output, "second", architecture, name)), `${architecture}/${name} repeatability mismatch`);
      }
    }
    const fixtures = ordinal ? {
      RING3_ORDINAL_PE32_FIXTURE: join(output, "first/i386/imports.exe"),
      RING3_ORDINAL_PE32PLUS_FIXTURE: join(output, "first/amd64/imports.exe"),
      RING3_EXPORT_PE32_ORDINAL_DLL: join(output, "first/i386/Ring3Ordinal.dll"),
      RING3_EXPORT_PE32PLUS_ORDINAL_DLL: join(output, "first/amd64/Ring3Ordinal.dll"),
    } : {
      RING3_IMPORT_PE32_FIXTURE: join(output, "first/i386/imports.exe"),
      RING3_IMPORT_PE32PLUS_FIXTURE: join(output, "first/amd64/imports.exe"),
      RING3_EXPORT_PE32_NAMED_DLL: join(output, "first/i386/Ring3Probe.dll"),
      RING3_EXPORT_PE32PLUS_NAMED_DLL: join(output, "first/amd64/Ring3Probe.dll"),
    };
    writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json" }, null, 2)}\n`);
    console.log(`${ordinal ? "Ordinal" : "Named"} PE32 and PE32+ fixtures verified.\nEvidence: ${output}\nFixture paths: ${join(output, "fixtures.json")}`);
  } catch (error) {
    console.error(`[ring3 import corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
