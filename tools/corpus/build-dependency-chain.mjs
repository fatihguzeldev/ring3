import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { locateTools, prepareOutputParents, root, run, sha256, target } from "./shared.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe-dependency-chain/fixture.json"), "utf8"));
export const cycleContract = JSON.parse(readFileSync(join(root, "corpus/pe-dependency-cycle/fixture.json"), "utf8"));
const chainSources = ["leaf.c", "middle.c", "chain.c"];
const chainArtifacts = ["Chain.exe", "Ring3Middle.dll", "Ring3Leaf.dll", "Ring3Middle.lib", "Ring3Leaf.lib", "chain.obj", "middle.obj", "leaf.obj"];
const cycleArtifacts = [...chainArtifacts, "cycle_leaf.obj", "bootstrap/Ring3Leaf.dll", "bootstrap/Ring3Leaf.lib"];
const architectures = [
  ["i386", "i686-pc-windows-msvc", "x86", "0x10000000", "0x20000000", "0x400000", "coff-i386"],
  ["amd64", "x86_64-pc-windows-msvc", "x64", "0x180000000", "0x190000000", "0x140000000", "coff-x86-64"],
];

export function buildDependencyChainFixtures(outputDirectory, options = {}) {
  return buildFixtures(outputDirectory, false, options);
}

export function buildDependencyCycleFixtures(outputDirectory, options = {}) {
  return buildFixtures(outputDirectory, true, options);
}

function buildFixtures(outputDirectory, cycle, options) {
  const sources = cycle ? [...chainSources, "cycle_leaf.c"] : chainSources;
  const artifacts = cycle ? cycleArtifacts : chainArtifacts;
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  const spec = options.contract ?? (cycle ? cycleContract : contract);
  assert.equal(spec.schemaVersion, 1);
  const snapshots = Object.fromEntries(sources.map((name) => {
    const sourceDirectory = options.sourceDirectory ?? join(root, name === "cycle_leaf.c" ? "corpus/pe-dependency-cycle" : "corpus/pe-dependency-chain");
    const bytes = readFileSync(join(sourceDirectory, name));
    assert.equal(sha256(bytes), spec.sources[name].sha256, `${name} source SHA-256 mismatch`);
    return [name, bytes];
  }));
  const tools = locateTools(options.tools);
  const versions = {};
  for (const name of ["clang", "lld", "objdump"]) {
    assert.equal(sha256(readFileSync(tools[name])), spec.tools[name].sha256, `${name} SHA-256 mismatch`);
    versions[name] = run(tools[name], name === "lld" ? ["-flavor", "link", "--version"] : ["--version"]).trim();
    assert.ok(versions[name].includes(spec.tools[name].version), `${name} version mismatch`);
  }
  const builds = {};
  for (const [architecture, triple, machine, leafBase, middleBase, exeBase, format] of architectures) {
    const directory = join(output, architecture);
    mkdirSync(directory);
    if (cycle) mkdirSync(join(directory, "bootstrap"));
    for (const name of sources) writeFileSync(join(directory, name), snapshots[name]);
    const commands = [];
    function invoke(tool, args, cwd = directory) {
      const result = run(tools[tool], args, cwd);
      commands.push({ tool, args, ...(cwd === directory ? {} : { cwd: relative(directory, cwd) }) });
      return result;
    }
    for (const name of sources) {
      invoke("clang", [`--target=${triple}`, "-c", "-O1", "-ffreestanding", "-fno-stack-protector", "-fno-ident", "-mno-incremental-linker-compatible", name, "-o", name.replace(".c", ".obj")]);
    }
    const common = ["-flavor", "link", `/machine:${machine}`, "/nodefaultlib", "/timestamp:0", "/fixed", "/dynamicbase:no", "/nxcompat"];
    if (architecture === "i386") common.push("/safeseh:no");
    invoke("lld", [...common, "/dll", "/noentry", `/base:${leafBase}`, "/out:Ring3Leaf.dll", "/implib:Ring3Leaf.lib", cycle ? "../leaf.obj" : "leaf.obj"], cycle ? join(directory, "bootstrap") : directory);
    invoke("lld", [...common, "/dll", "/noentry", `/base:${middleBase}`, "/out:Ring3Middle.dll", "/implib:Ring3Middle.lib", "middle.obj", cycle ? "bootstrap/Ring3Leaf.lib" : "Ring3Leaf.lib"]);
    if (cycle) {
      invoke("lld", [...common, "/dll", "/noentry", `/base:${leafBase}`, "/out:Ring3Leaf.dll", "/implib:Ring3Leaf.lib", "cycle_leaf.obj", "Ring3Middle.lib"]);
    }
    invoke("lld", [...common, "/entry:entry", "/subsystem:console", `/base:${exeBase}`, "/out:Chain.exe", "chain.obj", "Ring3Middle.lib"]);
    const identities = {};
    for (const name of artifacts) {
      const bytes = readFileSync(join(directory, name));
      const expected = spec.architectures[architecture].artifacts[name];
      assert.equal(sha256(bytes), expected.sha256, `${architecture}/${name} artifact SHA-256 mismatch`);
      assert.equal(bytes.length, expected.bytes, `${architecture}/${name} artifact size mismatch`);
      identities[name] = { bytes: bytes.length, sha256: sha256(bytes) };
    }
    const inspections = {};
    const images = ["Chain.exe", "Ring3Middle.dll", "Ring3Leaf.dll", ...(cycle ? ["bootstrap/Ring3Leaf.dll"] : [])];
    for (const name of images) {
      const inspection = invoke("objdump", ["--private-headers", "--section-headers", "--disassemble", name]);
      assert.ok(inspection.includes(`file format ${format}`), `${name} LLVM format mismatch`);
      const expected = spec.expectation[name];
      const dlls = [...inspection.matchAll(/DLL Name: (.+)/g)].map((match) => match[1].trim());
      assert.deepEqual(dlls, expected.importedDlls, `${name} LLVM dependency mismatch`);
      for (const symbol of expected.symbols) assert.ok(inspection.includes(symbol), `${name} LLVM symbol mismatch`);
      writeFileSync(join(directory, `${name}.inspection.txt`), inspection);
      inspections[name] = { path: `${architecture}/${name}.inspection.txt`, sha256: sha256(inspection) };
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
    assert.ok(process.argv.length <= 3 && [undefined, "--cycle"].includes(process.argv[2]), "unsupported corpus arguments");
    const cycle = process.argv[2] === "--cycle";
    const build = cycle ? buildDependencyCycleFixtures : buildDependencyChainFixtures;
    const artifacts = cycle ? cycleArtifacts : chainArtifacts;
    const outputRoot = join(target, cycle ? "corpus-dependency-cycle" : "corpus-dependency-chain");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = build(join(output, "first"));
    const second = build(join(output, "second"));
    assert.deepEqual(first, second);
    const fixtures = {};
    for (const [architecture] of architectures) {
      for (const name of artifacts) {
        assert.deepEqual(readFileSync(join(output, "first", architecture, name)), readFileSync(join(output, "second", architecture, name)), `${architecture}/${name} repeatability mismatch`);
      }
      const family = cycle ? "CYCLE" : "CHAIN";
      const prefix = `RING3_${family}_${architecture === "i386" ? "PE32" : "PE32PLUS"}`;
      for (const [suffix, name] of [["EXE", "Chain.exe"], ["MIDDLE", "Ring3Middle.dll"], ["LEAF", "Ring3Leaf.dll"]]) {
        fixtures[`${prefix}_${suffix}`] = join(output, "first", architecture, name);
      }
    }
    writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json" }, null, 2)}\n`);
    console.log(`PE dependency ${cycle ? "cycles" : "chains"} verified.\nEvidence: ${output}\nFixture paths: ${join(output, "fixtures.json")}`);
  } catch (error) {
    console.error(`[ring3 dependency chain corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
