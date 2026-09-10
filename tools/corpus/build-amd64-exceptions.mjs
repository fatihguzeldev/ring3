import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { locateTools, prepareOutputParents, root, sha256, target } from "./shared.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe-amd64-exceptions/fixture.json"), "utf8"));
const configurations = ["two-functions", "leaf-only"];
const artifacts = ["probe.obj", "Probe.dll", "Probe.lib"];

// this observer reads only fixed fixtures whose complete artifact hashes passed.
function readMetadata(bytes) {
  const pe = bytes.readUInt32LE(60);
  const optional = pe + 24;
  assert.equal(bytes.readUInt16LE(optional), 0x20b, "expected PE32+ fixture");
  const rva = bytes.readUInt32LE(optional + 112 + 24);
  const size = bytes.readUInt32LE(optional + 112 + 28);
  let fileOffset = null;
  const entries = [];
  if (rva !== 0 || size !== 0) {
    const sections = optional + bytes.readUInt16LE(pe + 20);
    for (let index = 0; index < bytes.readUInt16LE(pe + 6); index++) {
      const section = sections + index * 40;
      const start = bytes.readUInt32LE(section + 12);
      const length = Math.min(bytes.readUInt32LE(section + 8), bytes.readUInt32LE(section + 16));
      if (start <= rva && rva + size <= start + length) {
        assert.equal(fileOffset, null, "ambiguous exception fixture section");
        fileOffset = bytes.readUInt32LE(section + 20) + rva - start;
      }
    }
    assert.notEqual(fileOffset, null, "missing exception fixture section");
    assert.equal(size % 12, 0, "invalid exception fixture record size");
    for (let index = 0; index < size / 12; index++) {
      const offset = fileOffset + index * 12;
      entries.push({
        tableIndex: index, entryRva: rva + index * 12, entryFileOffset: offset,
        beginRva: bytes.readUInt32LE(offset), endRva: bytes.readUInt32LE(offset + 4),
        unwindInfoRva: bytes.readUInt32LE(offset + 8),
      });
    }
  }
  return {
    kind: "PE32Plus", machine: bytes.readUInt16LE(pe + 4), timeDateStamp: bytes.readUInt32LE(pe + 8),
    directory: { rva, size, fileOffset }, entries,
  };
}

export function buildAmd64ExceptionFixtures(outputDirectory, options = {}) {
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  const spec = options.contract ?? contract;
  assert.equal(spec.schemaVersion, 1);
  const snapshots = Object.fromEntries(configurations.map((name) => {
    const source = `${name}.s`;
    const bytes = readFileSync(join(options.sourceDirectory ?? join(root, "corpus/pe-amd64-exceptions"), source));
    assert.equal(sha256(bytes), spec.sources[source].sha256, `${source} source SHA-256 mismatch`);
    return [name, bytes];
  }));
  const tools = { ...locateTools(), ...options.tools };
  const commands = [];
  function invoke(tool, args, directory, label) {
    assert.equal(sha256(readFileSync(tools[tool])), spec.tools[tool].sha256, `${tool} SHA-256 mismatch`);
    const result = spawnSync(tools[tool], args, {
      cwd: directory, env: { ...process.env, LC_ALL: "C", TZ: "UTC", SOURCE_DATE_EPOCH: "0" },
      encoding: "utf8", timeout: 15_000, maxBuffer: 1024 * 1024,
    });
    const logs = {};
    for (const stream of ["stdout", "stderr"]) {
      const text = result[stream] ?? "";
      const path = join(directory, `${label}.${stream}.txt`);
      writeFileSync(path, text);
      logs[stream] = { path: relative(output, path), sha256: sha256(text) };
    }
    commands.push({ tool, args, directory: relative(output, directory), status: result.status, signal: result.signal, error: result.error?.message ?? null, logs });
    writeFileSync(join(output, "commands.json"), `${JSON.stringify(commands, null, 2)}\n`);
    if (result.error || result.signal || result.status !== 0) {
      throw new Error(`${label} failed: ${result.error?.message ?? result.stderr}`);
    }
    return result.stdout;
  }
  const versions = {};
  for (const tool of ["clang", "lld", "objdump"]) {
    versions[tool] = invoke(tool, tool === "lld" ? ["-flavor", "link", "--version"] : ["--version"], output, `version-${tool}`).trim();
    assert.ok(versions[tool].includes(spec.tools[tool].version), `${tool} version mismatch`);
  }

  const builds = {};
  for (const name of configurations) {
    const directory = join(output, name);
    mkdirSync(directory);
    writeFileSync(join(directory, "probe.s"), snapshots[name]);
    const expected = spec.configurations[name];
    const args = {
      clang: ["--target=x86_64-pc-windows-msvc", "-c", "-mno-incremental-linker-compatible", "probe.s", "-o", "probe.obj"],
      lld: ["-flavor", "link", "/machine:x64", "/nodefaultlib", "/timestamp:0", "/fixed", "/dynamicbase:no", "/nxcompat", "/dll", "/noentry", "/base:0x180000000", "/out:Probe.dll", "/implib:Probe.lib", ...expected.exports.map((symbol) => `/export:${symbol}`), "probe.obj"],
      objdump: ["--private-headers", "--section-headers", "--full-contents", "--unwind-info", "Probe.dll"],
    };
    invoke("clang", args.clang, directory, "compile");
    invoke("lld", args.lld, directory, "link");
    const identities = {};
    for (const artifact of artifacts) {
      const bytes = readFileSync(join(directory, artifact));
      assert.equal(sha256(bytes), expected.artifacts[artifact].sha256, `${name}/${artifact} artifact SHA-256 mismatch`);
      assert.equal(bytes.length, expected.artifacts[artifact].bytes, `${name}/${artifact} artifact size mismatch`);
      identities[artifact] = { bytes: bytes.length, sha256: sha256(bytes) };
      if (artifact.endsWith(".obj")) assert.equal(bytes.readUInt32LE(4), spec.objectTimestamp, "object timestamp mismatch");
    }
    const metadata = readMetadata(readFileSync(join(directory, "Probe.dll")));
    assert.deepEqual(metadata, expected.metadata, "raw exception metadata mismatch");
    const inspection = invoke("objdump", args.objdump, directory, "inspect");
    assert.equal(sha256(inspection), expected.inspection.sha256, "inspection SHA-256 mismatch");
    assert.equal(Buffer.byteLength(inspection), expected.inspection.bytes, "inspection size mismatch");
    writeFileSync(join(directory, "inspection.txt"), inspection);
    builds[name] = { target: "x86_64-pc-windows-msvc", commands: args, artifacts: identities, metadata, inspection: { path: `${name}/inspection.txt`, sha256: sha256(inspection) } };
  }
  const evidence = {
    ...spec,
    environment: { platform: process.platform, architecture: process.arch, node: process.versions.node, locale: "C", timezone: "UTC", sourceDateEpoch: "0" },
    toolVersions: versions, commands, builds,
  };
  writeFileSync(join(output, "evidence.json"), `${JSON.stringify(evidence, null, 2)}\n`);
  return evidence;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    assert.equal(process.argv.length, 2, "unsupported corpus arguments");
    const outputRoot = join(target, "corpus-amd64-exceptions");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = buildAmd64ExceptionFixtures(join(output, "first"));
    const second = buildAmd64ExceptionFixtures(join(output, "second"));
    assert.deepEqual(first, second);
    for (const name of configurations) {
      for (const artifact of ["probe.s", ...artifacts, "inspection.txt"]) {
        assert.deepEqual(readFileSync(join(output, "first", name, artifact)), readFileSync(join(output, "second", name, artifact)), `${name}/${artifact} repeatability mismatch`);
      }
    }
    const fixtures = {
      RING3_EXCEPTION_AMD64_FIXTURE: join(output, "first/two-functions/Probe.dll"),
      RING3_EXCEPTION_AMD64_LEAF_FIXTURE: join(output, "first/leaf-only/Probe.dll"),
    };
    writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json" }, null, 2)}\n`);
    console.log(`AMD64 exception positive and absent fixtures verified.\nEvidence: ${output}\nFixture paths: ${join(output, "fixtures.json")}`);
  } catch (error) {
    console.error(`[ring3 AMD64 exceptions corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
