import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { locateTools, prepareOutputParents, root, run, sha256, target } from "./shared.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe-debug-payloads/fixture.json"), "utf8"));
const architectures = ["i386", "amd64"];
const artifacts = ["debug.obj", "debug.exe", "debug.pdb"];

function fingerprint(bytes) {
  let value = 0xcbf29ce484222325n;
  for (const byte of bytes) value = BigInt.asUintN(64, (value ^ BigInt(byte)) * 0x100000001b3n);
  return value.toString(16).padStart(16, "0");
}

// whole artifact identities are checked before these fixture-specific reads.
function readMetadata(bytes) {
  const pe = bytes.readUInt32LE(60);
  const optional = pe + 24;
  const kind = bytes.readUInt16LE(optional) === 0x20b ? 64 : 32;
  const slot = optional + (kind === 64 ? 112 : 96) + 48;
  const rva = bytes.readUInt32LE(slot);
  const size = bytes.readUInt32LE(slot + 4);
  const sections = optional + bytes.readUInt16LE(pe + 20);
  const ranges = [];
  for (let i = 0; i < bytes.readUInt16LE(pe + 6); i++) {
    const section = sections + i * 40;
    const virtualSize = bytes.readUInt32LE(section + 8);
    const virtualAddress = bytes.readUInt32LE(section + 12);
    const rawSize = bytes.readUInt32LE(section + 16);
    const fileOffset = bytes.readUInt32LE(section + 20);
    if (virtualAddress <= rva && rva + size <= virtualAddress + Math.min(virtualSize, rawSize)) {
      ranges.push(fileOffset + rva - virtualAddress);
    }
  }
  assert.equal(ranges.length, 1, "expected one backed debug table section");
  const file = ranges[0];
  assert.equal(size, 56, "expected two debug records");
  const entries = [];
  const payloads = [];
  for (let i = 0; i < 2; i++) {
    const at = file + i * 28;
    const entry = [rva + i * 28, at, bytes.readUInt32LE(at), bytes.readUInt32LE(at + 4),
      bytes.readUInt16LE(at + 8), bytes.readUInt16LE(at + 10), ...[12, 16, 20, 24].map((n) => bytes.readUInt32LE(at + n))];
    entries.push(entry);
    const length = entry[7];
    const offset = entry[9];
    const body = bytes.subarray(offset, offset + length);
    assert.equal(body.length, length, "truncated debug payload");
    payloads.push([i, length === 0 ? null : offset, length, fingerprint(body), body.toString("hex")]);
  }
  return { kind, machine: bytes.readUInt16LE(pe + 4), coffTimestamp: bytes.readUInt32LE(pe + 8),
    directory: [rva, file, size], entries, payloads };
}

export function buildDebugFixtures(outputDirectory, options = {}) {
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  const spec = options.contract ?? contract;
  assert.equal(spec.schemaVersion, 1, "unsupported fixture schema");
  const source = readFileSync(options.sourcePath ?? join(root, spec.source.path));
  assert.equal(sha256(source), spec.source.sha256, "source SHA-256 mismatch");
  const tools = { ...locateTools(), ...options.tools };
  const versions = {};
  for (const name of ["clang", "lld", "objdump"]) {
    assert.equal(sha256(readFileSync(tools[name])), spec.tools[name].sha256, `${name} SHA-256 mismatch`);
    versions[name] = run(tools[name], name === "lld" ? ["-flavor", "link", "--version"] : ["--version"]).trim();
    assert.ok(versions[name].includes(spec.tools[name].version), `${name} version mismatch`);
  }
  const builds = {};
  for (const architecture of architectures) {
    const directory = join(output, architecture);
    mkdirSync(directory);
    writeFileSync(join(directory, "debug.c"), source);
    const expected = spec.architectures[architecture];
    const commands = expected.commands;
    run(tools.clang, commands.clang, directory);
    run(tools.lld, commands.lld, directory);
    const identities = {};
    for (const name of artifacts) {
      const bytes = readFileSync(join(directory, name));
      assert.equal(sha256(bytes), expected.artifacts[name].sha256, `${architecture}/${name} artifact SHA-256 mismatch`);
      assert.equal(bytes.length, expected.artifacts[name].bytes, `${architecture}/${name} artifact size mismatch`);
      if (name === "debug.obj") assert.equal(bytes.readUInt32LE(4), 0, "object COFF timestamp must be zero");
      identities[name] = { bytes: bytes.length, sha256: sha256(bytes) };
    }
    const inspection = run(tools.objdump, commands.objdump, directory);
    assert.equal(sha256(inspection), expected.inspection.sha256, "LLVM inspection SHA-256 mismatch");
    assert.equal(Buffer.byteLength(inspection), expected.inspection.bytes, "LLVM inspection size mismatch");
    assert.deepEqual(readMetadata(readFileSync(join(directory, "debug.exe"))), expected.metadata, "raw debug metadata mismatch");
    writeFileSync(join(directory, "inspection.txt"), inspection);
    builds[architecture] = { commands, artifacts: identities, inspection: { path: `${architecture}/inspection.txt`, sha256: sha256(inspection) } };
  }
  const evidence = { ...spec,
    environment: { platform: process.platform, architecture: process.arch, node: process.versions.node, locale: "C", timezone: "UTC" },
    toolVersions: versions, builds };
  writeFileSync(join(output, "evidence.json"), `${JSON.stringify(evidence, null, 2)}\n`);
  return evidence;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    assert.equal(process.argv.length, 2, "unsupported corpus arguments");
    const outputRoot = join(target, "corpus-debug-payloads");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = buildDebugFixtures(join(output, "first"));
    const second = buildDebugFixtures(join(output, "second"));
    assert.deepEqual(first, second);
    for (const architecture of architectures) {
      for (const name of ["debug.c", ...artifacts, "inspection.txt"]) {
        assert.deepEqual(readFileSync(join(output, "first", architecture, name)), readFileSync(join(output, "second", architecture, name)), `${architecture}/${name} repeatability mismatch`);
      }
    }
    const fixtures = { RING3_DEBUG_PE32_FIXTURE: join(output, "first/i386/debug.exe"), RING3_DEBUG_PE32PLUS_FIXTURE: join(output, "first/amd64/debug.exe") };
    writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json" }, null, 2)}\n`);
    console.log(`debug PE32 and PE32+ fixtures verified.\nEvidence: ${output}\nFixture paths: ${join(output, "fixtures.json")}`);
  } catch (error) {
    console.error(`[ring3 debug corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
