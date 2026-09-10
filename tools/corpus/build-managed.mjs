import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { prepareOutputParents, root, sha256, target } from "./shared.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe-managed/fixture.json"), "utf8"));
const architectures = ["x86", "x64"];

function requireSdkRoot(value = process.env.RING3_DOTNET_ROOT) {
  assert.ok(typeof value === "string" && value.trim(), "set RING3_DOTNET_ROOT to the pinned existing SDK root");
  return realpathSync(value);
}

function fileIdentity(path) {
  assert.ok(lstatSync(path).isFile(), "SDK inputs must be regular files");
  const bytes = readFileSync(path);
  return { bytes: bytes.length, sha256: sha256(bytes) };
}

function treeIdentity(directory) {
  const entries = [];
  function walk(path, prefix = "") {
    assert.ok(lstatSync(path).isDirectory(), "SDK trees must be real directories");
    for (const name of readdirSync(path)) {
      const file = join(path, name);
      const relative = prefix + name;
      if (lstatSync(file).isDirectory()) walk(file, relative + "/");
      else entries.push({ path: relative, ...fileIdentity(file) });
    }
  }
  walk(directory);
  entries.sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
  return { files: entries.length, bytes: entries.reduce((sum, entry) => sum + entry.bytes, 0), sha256: sha256(JSON.stringify(entries)) };
}

// whole executable identity is checked before these fixture-specific reads.
function readMetadata(bytes) {
  const pe = bytes.readUInt32LE(60);
  const optional = pe + 24;
  const kind = bytes.readUInt16LE(optional) === 0x20b ? 64 : 32;
  const slot = optional + (kind === 64 ? 112 : 96) + 112;
  const rva = bytes.readUInt32LE(slot);
  const size = bytes.readUInt32LE(slot + 4);
  const sections = optional + bytes.readUInt16LE(pe + 20);
  const ranges = [];
  for (let i = 0; i < bytes.readUInt16LE(pe + 6); i++) {
    const at = sections + i * 40;
    const virtualSize = bytes.readUInt32LE(at + 8);
    const address = bytes.readUInt32LE(at + 12);
    const rawSize = bytes.readUInt32LE(at + 16);
    if (address <= rva && rva + size <= address + Math.min(virtualSize, rawSize)) {
      ranges.push([bytes.readUInt32LE(at + 20) + rva - address, ["section", i]]);
    }
  }
  assert.equal(ranges.length, 1, "expected one backed CLR header section");
  assert.equal(size, 72, "expected fixed CLR directory size");
  const [file, source] = ranges[0];
  const pair = (offset) => [bytes.readUInt32LE(file + offset), bytes.readUInt32LE(file + offset + 4)];
  const flags = bytes.readUInt32LE(file + 16);
  return { kind, directory: [rva, file, size], cb: bytes.readUInt32LE(file),
    version: [bytes.readUInt16LE(file + 4), bytes.readUInt16LE(file + 6)], flags,
    entry: [bytes.readUInt32LE(file + 20), flags & 16 ? "rva" : "token"],
    pairs: [pair(8), ...[24, 32, 40, 48, 56, 64].map(pair)],
    prefixHex: bytes.subarray(file, file + 72).toString("hex"), source,
    fullDirectoryQuery: ["range", file, size, source] };
}

export function buildManagedFixtures(outputDirectory, options = {}) {
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  assert.equal(process.platform, "darwin", "unsupported managed fixture SDK host");
  assert.equal(process.arch, "arm64", "unsupported managed fixture SDK architecture");
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);
  const spec = options.contract ?? contract;
  assert.equal(spec.schemaVersion, 1, "unsupported fixture schema");
  const source = readFileSync(options.sourcePath ?? join(root, spec.source.path));
  assert.equal(sha256(source), spec.source.sha256, "source SHA-256 mismatch");
  const sdk = requireSdkRoot(options.sdkRoot);
  assert.deepEqual(readdirSync(join(sdk, "host/fxr")).sort(), [spec.sdk.runtimeVersion], "SDK hostfxr selection mismatch");
  for (const [path, identity] of Object.entries(spec.sdk.files)) {
    assert.deepEqual(fileIdentity(join(sdk, path)), identity, `SDK file identity mismatch: ${path}`);
  }
  for (const [path, identity] of Object.entries(spec.sdk.trees)) {
    assert.deepEqual(treeIdentity(join(sdk, path)), identity, `SDK tree identity mismatch: ${path}`);
  }
  for (const name of ["cli-home", "tmp"]) mkdirSync(join(output, name));
  const overrides = { DOTNET_ROOT: sdk, DOTNET_CLI_HOME: join(output, "cli-home"), DOTNET_CLI_TELEMETRY_OPTOUT: "1",
    DOTNET_SKIP_FIRST_TIME_EXPERIENCE: "1", DOTNET_NOLOGO: "1", DOTNET_MULTILEVEL_LOOKUP: "0", DOTNET_EnableDiagnostics: "0",
    TMPDIR: join(output, "tmp"), LANG: "en_US.UTF-8", LC_ALL: "en_US.UTF-8", TZ: "UTC" };
  const removedPrefixes = ["DOTNET_", "COREHOST_", "COMPlus_"];
  const removedKeys = ["LIBPATH", "LIB", "MSBuildSDKsPath", "MSBUILD_EXE_PATH"];
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !removedPrefixes.some((prefix) => name.startsWith(prefix)) && !removedKeys.includes(name)));
  Object.assign(env, overrides);
  const host = join(sdk, "dotnet");
  const baseArgs = ["exec", "--fx-version", spec.sdk.runtimeVersion, "--roll-forward", "Disable", join(sdk, spec.sdk.compilerPath)];
  const commands = [];
  function compile(args, directory, label) {
    const result = spawnSync(host, [...baseArgs, ...args], { cwd: directory, env, encoding: "utf8", timeout: 30_000, maxBuffer: 1024 * 1024 });
    for (const [name, text] of Object.entries({ stdout: result.stdout ?? "", stderr: result.stderr ?? "" })) {
      writeFileSync(join(output, `${label}.${name}.txt`), text);
    }
    commands.push({ command: host, args: [...baseArgs, ...args], cwd: label === "version" ? "." : label, status: result.status, signal: result.signal, error: result.error?.message ?? null });
    writeFileSync(join(output, "commands.json"), `${JSON.stringify(commands, null, 2)}\n`);
    assert.ok(!result.error && !result.signal && result.status === 0, "compiler failed; see retained command logs");
    return result.stdout.trim();
  }
  const version = compile(["-version"], output, "version");
  assert.equal(version, spec.sdk.compilerVersion, "compiler version mismatch");
  const builds = {};
  for (const architecture of architectures) {
    const directory = join(output, architecture);
    mkdirSync(directory);
    writeFileSync(join(directory, "Probe.cs"), source);
    compile([...spec.compilerArguments, `-platform:${architecture}`, "-out:Probe.exe", `-reference:${join(sdk, spec.sdk.referencePath)}`, "Probe.cs"], directory, architecture);
    const bytes = readFileSync(join(directory, "Probe.exe"));
    const expected = spec.architectures[architecture];
    assert.equal(sha256(bytes), expected.artifact.sha256, `${architecture} artifact SHA-256 mismatch`);
    assert.equal(bytes.length, expected.artifact.bytes, `${architecture} artifact size mismatch`);
    assert.deepEqual(readMetadata(bytes), expected.metadata, "raw CLR metadata mismatch");
    assert.deepEqual(readFileSync(join(directory, "Probe.cs")), source, "compiler source changed");
    builds[architecture] = { artifact: fileIdentity(join(directory, "Probe.exe")), metadata: expected.metadata };
  }
  const evidence = { ...spec, sdkRoot: sdk, compilerVersion: version, commands,
    environment: { platform: process.platform, architecture: process.arch, node: process.versions.node,
      overrides: { ...overrides, DOTNET_CLI_HOME: "<output>/cli-home", TMPDIR: "<output>/tmp" }, removedPrefixes, removedKeys }, builds };
  writeFileSync(join(output, "evidence.json"), `${JSON.stringify(evidence, null, 2)}\n`);
  return evidence;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    assert.equal(process.argv.length, 2, "unsupported corpus arguments");
    requireSdkRoot();
    const outputRoot = join(target, "corpus-managed");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = buildManagedFixtures(join(output, "first"));
    const second = buildManagedFixtures(join(output, "second"));
    assert.deepEqual(first, second);
    for (const architecture of architectures) {
      for (const name of ["Probe.cs", "Probe.exe"]) {
        assert.deepEqual(readFileSync(join(output, "first", architecture, name)), readFileSync(join(output, "second", architecture, name)), `${architecture}/${name} repeatability mismatch`);
      }
    }
    const fixtures = { RING3_CLR_PE32_FIXTURE: join(output, "first/x86/Probe.exe"), RING3_CLR_PE32PLUS_FIXTURE: join(output, "first/x64/Probe.exe") };
    writeFileSync(join(output, "fixtures.json"), `${JSON.stringify(fixtures, null, 2)}\n`);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json" }, null, 2)}\n`);
    console.log(`managed PE32 and PE32+ fixtures verified.\nEvidence: ${output}\nFixture paths: ${join(output, "fixtures.json")}`);
  } catch (error) {
    console.error(`[ring3 managed corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
