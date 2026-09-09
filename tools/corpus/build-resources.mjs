import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { locateTools, prepareOutputParents, root, run, sha256, target } from "./shared.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe-resources/fixture.json"), "utf8"));
const artifacts = ["resources.obj", "resources.exe"];
const architectures = [
  ["i386", "i686-pc-windows-msvc", "x86", "0x400000", "coff-i386"],
  ["amd64", "x86_64-pc-windows-msvc", "x64", "0x140000000", "coff-x86-64"],
];

// artifact hashes are checked before these fixture-specific reads.
function readMetadata(bytes) {
  const pe = bytes.readUInt32LE(60);
  const optional = pe + 24;
  const plus = bytes.readUInt16LE(optional) === 0x20b;
  const slot = optional + (plus ? 112 : 96) + 16;
  const rva = bytes.readUInt32LE(slot);
  const size = bytes.readUInt32LE(slot + 4);
  const sections = optional + bytes.readUInt16LE(pe + 20);
  let offset;
  for (let i = 0; i < bytes.readUInt16LE(pe + 6); i++) {
    const record = sections + i * 40;
    if (bytes.subarray(record, record + 8).equals(Buffer.from(".rsrc\0\0\0"))) {
      assert.equal(bytes.readUInt32LE(record + 12), rva);
      assert.equal(bytes.readUInt32LE(record + 8), size);
      offset = bytes.readUInt32LE(record + 20);
    }
  }
  assert.notEqual(offset, undefined, "missing resource section");
  const rootHeader = [bytes.readUInt32LE(offset), bytes.readUInt32LE(offset + 4),
    ...[8, 10, 12, 14].map((at) => bytes.readUInt16LE(offset + at))];
  assert.deepEqual(rootHeader.slice(4), [1, 1], "unexpected resource root counts");
  const entries = [0, 1].map((i) => ({
    rva: rva + 16 + i * 8, fileOffset: offset + 16 + i * 8,
    rawNameOrId: bytes.readUInt32LE(offset + 16 + i * 8),
    rawDataOrSubdirectory: bytes.readUInt32LE(offset + 20 + i * 8),
  }));
  const nameOffset = entries[0].rawNameOrId & 0x7fffffff;
  const nameFile = offset + nameOffset;
  const codeUnits = bytes.readUInt16LE(nameFile);
  const nameBytes = bytes.subarray(nameFile + 2, nameFile + 2 + codeUnits * 2);
  assert.equal(nameBytes.length, codeUnits * 2, "truncated resource name");
  return {
    kind: plus ? "PE32+" : "PE32", machine: bytes.readUInt16LE(pe + 4),
    directory: { rva, fileOffset: offset, size }, rootHeader, entries,
    name: { offset: nameOffset, rva: rva + nameOffset, fileOffset: nameFile, codeUnits, utf16le: nameBytes.toString("hex") },
  };
}

export function buildResourceFixtures(outputDirectory, options = {}) {
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  const spec = options.contract ?? contract;
  assert.equal(spec.schemaVersion, 1);
  const snapshots = Object.fromEntries(architectures.map(([architecture]) => {
    const name = `${architecture}.s`;
    const bytes = readFileSync(join(options.sourceDirectory ?? join(root, "corpus/pe-resources"), name));
    assert.equal(sha256(bytes), spec.sources[name].sha256, `${name} source SHA-256 mismatch`);
    return [architecture, bytes];
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
    writeFileSync(join(directory, "resources.s"), snapshots[architecture]);
    const commands = {
      clang: [`--target=${triple}`, "-c", "resources.s", "-o", "resources.obj"],
      lld: ["-flavor", "link", `/machine:${machine}`, "/nodefaultlib", "/timestamp:0", "/fixed", "/dynamicbase:no", "/nxcompat", `/base:${base}`, "/entry:entry", "/subsystem:console", "/out:resources.exe", "resources.obj"],
      objdump: ["--private-headers", "--section-headers", "--full-contents", "resources.exe"],
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
    assert.ok(inspection.includes(`Entry 2 ${address} ${size} Resource Directory [.rsrc]`), "LLVM resource directory mismatch");
    const expected = spec.architectures[architecture];
    assert.equal(sha256(inspection), expected.inspection.sha256, "LLVM inspection SHA-256 mismatch");
    assert.equal(Buffer.byteLength(inspection), expected.inspection.bytes, "LLVM inspection size mismatch");
    const metadata = readMetadata(readFileSync(join(directory, "resources.exe")));
    const { kind, machine: expectedMachine, directory: resourceDirectory, rootHeader, entries, name } = expected;
    assert.deepEqual(metadata, { kind, machine: expectedMachine, directory: resourceDirectory, rootHeader, entries, name }, "raw resource metadata mismatch");
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
    const outputRoot = join(target, "corpus-resources");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = buildResourceFixtures(join(output, "first"));
    const second = buildResourceFixtures(join(output, "second"));
    assert.deepEqual(first, second);
    for (const [architecture] of architectures) {
      for (const name of ["resources.s", ...artifacts, "inspection.txt"]) {
        assert.deepEqual(readFileSync(join(output, "first", architecture, name)), readFileSync(join(output, "second", architecture, name)), `${architecture}/${name} repeatability mismatch`);
      }
    }
    const fixtures = {
      RING3_RESOURCE_PE32_FIXTURE: join(output, "first/i386/resources.exe"),
      RING3_RESOURCE_PE32PLUS_FIXTURE: join(output, "first/amd64/resources.exe"),
    };
    writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json" }, null, 2)}\n`);
    console.log(`resource PE32 and PE32+ fixtures verified.\nEvidence: ${output}\nFixture paths: ${join(output, "fixtures.json")}`);
  } catch (error) {
    console.error(`[ring3 resources corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
