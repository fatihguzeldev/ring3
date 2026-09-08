import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { locateTools, prepareOutputParents, root, run, sha256, target } from "./corpus-tools.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe-delay-imports.json"), "utf8"));
export const ordinalContract = JSON.parse(readFileSync(join(root, "corpus/pe-delay-ordinals.json"), "utf8"));
const sources = ["probe.c", "delayed.c"];
const ordinalSources = [...sources, "probe.def"];
const artifacts = ["probe.obj", "delayed.obj", "Ring3Delay.dll", "Ring3Delay.lib", "delayed.exe", "Ring3Delay.dll.inspection.txt", "delayed.exe.inspection.txt"];
const architectures = [
  ["i386", "i686-pc-windows-msvc", "x86", "0x10000000", "0x400000", "coff-i386"],
  ["amd64", "x86_64-pc-windows-msvc", "x64", "0x180000000", "0x140000000", "coff-x86-64"],
];

export function buildDelayImportFixtures(outputDirectory, options = {}) {
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  const ordinal = options.ordinal === true;
  const spec = options.contract ?? (ordinal ? ordinalContract : contract);
  const sourceNames = ordinal ? ordinalSources : sources;
  const sourceDirectory = options.sourceDirectory ?? join(root, ordinal ? "corpus/pe-delay-ordinals" : "corpus/pe-delay-imports");
  assert.equal(spec.schemaVersion, 1);
  const snapshots = Object.fromEntries(sourceNames.map(name => {
    const bytes = readFileSync(join(sourceDirectory, name));
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
    for (const name of sourceNames) writeFileSync(join(directory, name), snapshots[name]);
    const compile = ["--target=" + triple, "-c", "-O1", "-ffreestanding", "-fno-stack-protector", "-fno-ident", "-mno-incremental-linker-compatible"];
    const common = ["-flavor", "link", `/machine:${machine}`, "/nodefaultlib", "/timestamp:0", "/fixed", "/dynamicbase:no", "/nxcompat"];
    if (architecture === "i386") common.push("/safeseh:no");
    const commands = {
      compileProbe: [...compile, "probe.c", "-o", "probe.obj"],
      compileDelayed: [...compile, "delayed.c", "-o", "delayed.obj"],
      linkDll: [...common, "/dll", "/noentry", `/base:${dllBase}`, ...(ordinal ? ["/def:probe.def"] : []), "/out:Ring3Delay.dll", "/implib:Ring3Delay.lib", "probe.obj"],
      linkExe: [...common, "/entry:entry", "/subsystem:console", `/base:${exeBase}`, "/delayload:Ring3Delay.dll", "/out:delayed.exe", "delayed.obj", "Ring3Delay.lib"],
      inspectDll: ["--private-headers", "--section-headers", "--full-contents", "Ring3Delay.dll"],
      inspectExe: ["--private-headers", "--section-headers", "--full-contents", "delayed.exe"],
    };
    run(tools.clang, commands.compileProbe, directory);
    run(tools.clang, commands.compileDelayed, directory);
    run(tools.lld, commands.linkDll, directory);
    run(tools.lld, commands.linkExe, directory);
    for (const [name, args] of [["Ring3Delay.dll", commands.inspectDll], ["delayed.exe", commands.inspectExe]]) {
      const inspection = run(tools.objdump, args, directory);
      assert.ok(inspection.includes(`file format ${format}`), "LLVM format mismatch");
      writeFileSync(join(directory, `${name}.inspection.txt`), inspection);
    }
    const expectedDirectory = spec.architectures[architecture].directory;
    const address = expectedDirectory.rva.toString(16).padStart(architecture === "amd64" ? 16 : 8, "0");
    const size = expectedDirectory.size.toString(16).padStart(8, "0");
    const inspection = readFileSync(join(directory, "delayed.exe.inspection.txt"), "utf8");
    assert.ok(inspection.includes(`Entry d ${address} ${size} Delay Import Directory`), "LLVM delay directory mismatch");
    const exe = readFileSync(join(directory, "delayed.exe"));
    assert.deepEqual(Array.from({ length: 8 }, (_, index) => exe.readUInt32LE(expectedDirectory.fileOffset + index * 4)), expectedDirectory.rawWords, "raw delay descriptor mismatch");
    assert.equal(expectedDirectory.terminatorRva, expectedDirectory.rva + 32);
    assert.equal(expectedDirectory.terminatorFileOffset, expectedDirectory.fileOffset + 32);
    assert.deepEqual(exe.subarray(expectedDirectory.terminatorFileOffset, expectedDirectory.terminatorFileOffset + 32), Buffer.alloc(32), "delay terminator mismatch");
    if (ordinal) {
      const expected = spec.architectures[architecture];
      const lookup = expected.lookup;
      const width = architecture === "i386" ? 4 : 8;
      assert.equal(lookup.width, width);
      assert.equal(lookup.rva, expectedDirectory.rawWords[4]);
      const raw = width === 4 ? BigInt(exe.readUInt32LE(lookup.fileOffset)) : exe.readBigUInt64LE(lookup.fileOffset);
      assert.equal(raw, BigInt(lookup.rawValue), "ordinal lookup mismatch");
      assert.equal(raw & 0xffffn, BigInt(lookup.ordinal), "ordinal value mismatch");
      assert.equal(lookup.terminatorRva, lookup.rva + width);
      assert.equal(lookup.terminatorFileOffset, lookup.fileOffset + width);
      assert.deepEqual(exe.subarray(lookup.terminatorFileOffset, lookup.terminatorFileOffset + width), Buffer.alloc(width), "ordinal lookup terminator mismatch");
      const dll = readFileSync(join(directory, "Ring3Delay.dll"));
      const exports = expected.exports;
      const optional = dll.readUInt32LE(60) + 24;
      assert.equal(dll.readUInt32LE(optional + (architecture === "amd64" ? 112 : 96)), exports.directoryRva, "ordinal export directory mismatch");
      assert.equal(dll.readUInt32LE(exports.directoryFileOffset + 28), exports.addressTableRva, "ordinal export table mismatch");
      assert.equal(exports.addressTableFileOffset - exports.directoryFileOffset, exports.addressTableRva - exports.directoryRva, "ordinal export table offset mismatch");
      assert.equal(dll.readUInt32LE(exports.addressTableFileOffset), exports.firstAddressRva, "ordinal export address mismatch");
      assert.deepEqual([16, 20, 24].map(offset => dll.readUInt32LE(exports.directoryFileOffset + offset)), [exports.ordinalBase, exports.functionCount, exports.nameCount], "ordinal export mismatch");
    }
    const identities = {};
    for (const name of artifacts) {
      const bytes = readFileSync(join(directory, name));
      const expected = spec.architectures[architecture].artifacts[name];
      assert.equal(sha256(bytes), expected.sha256, `${architecture}/${name} artifact SHA-256 mismatch`);
      assert.equal(bytes.length, expected.bytes, `${architecture}/${name} artifact size mismatch`);
      if (name.endsWith(".obj")) assert.equal(bytes.readUInt32LE(4), 0);
      if (name.endsWith(".exe") || name.endsWith(".dll")) assert.equal(bytes.readUInt32LE(bytes.readUInt32LE(60) + 8), 0);
      identities[name] = { bytes: bytes.length, sha256: sha256(bytes) };
    }
    builds[architecture] = { target: triple, commands, artifacts: identities };
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
    assert.ok(process.argv.length === 2 || (process.argv.length === 3 && process.argv[2] === "--ordinal"), "unsupported corpus arguments");
    const ordinal = process.argv[2] === "--ordinal";
    const sourceNames = ordinal ? ordinalSources : sources;
    const outputRoot = join(target, ordinal ? "corpus-delay-ordinals" : "corpus-delay");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = buildDelayImportFixtures(join(output, "first"), { ordinal });
    const second = buildDelayImportFixtures(join(output, "second"), { ordinal });
    assert.deepEqual(first, second);
    for (const [architecture] of architectures) {
      for (const name of [...sourceNames, ...artifacts]) {
        assert.deepEqual(readFileSync(join(output, "first", architecture, name)), readFileSync(join(output, "second", architecture, name)), `${architecture}/${name} repeatability mismatch`);
      }
    }
    const fixtures = {
      [ordinal ? "RING3_DELAY_ORDINAL_PE32_FIXTURE" : "RING3_DELAY_PE32_FIXTURE"]: join(output, "first/i386/delayed.exe"),
      [ordinal ? "RING3_DELAY_ORDINAL_PE32PLUS_FIXTURE" : "RING3_DELAY_PE32PLUS_FIXTURE"]: join(output, "first/amd64/delayed.exe"),
    };
    writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json" }, null, 2)}\n`);
    console.log(`${ordinal ? "Ordinal delay" : "Delay"} PE32 and PE32+ fixtures verified.\nEvidence: ${output}\nFixture paths: ${join(output, "fixtures.json")}`);
  } catch (error) {
    console.error(`[ring3 delay corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
