import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { locateTools, prepareOutputParents, root, run, sha256, target } from "./corpus-tools.mjs";

export const contract = JSON.parse(readFileSync(join(root, "corpus/pe32-arithmetic.json"), "utf8"));

export function verifyTextPermissions(output, objcopy) {
  const args = ["--set-section-flags", ".text=alloc,load,readonly,code", "pe32-arithmetic.exe", "rx-check.exe"];
  run(objcopy, args, output);
  // llvm must leave the original bytes unchanged when requesting rx section flags.
  assert.deepEqual(readFileSync(join(output, "rx-check.exe")), readFileSync(join(output, "pe32-arithmetic.exe")), ".text must already be read-only and executable");
  return args;
}

export function buildFixture(outputDirectory, options = {}) {
  assert.equal(process.versions.node, readFileSync(join(root, ".node-version"), "utf8").trim());
  const output = prepareOutputParents(outputDirectory);
  mkdirSync(output);

  const spec = options.contract ?? contract;
  const source = readFileSync(options.sourcePath ?? join(root, spec.source.path));
  assert.equal(sha256(source), spec.source.sha256, "source SHA-256 mismatch");
  const tools = { ...locateTools(), ...options.tools };
  for (const name of ["clang", "lld", "objdump", "objcopy"]) {
    const expected = spec.tools[name];
    assert.equal(sha256(readFileSync(tools[name])), expected.sha256, `${name} SHA-256 mismatch`);
    const args = name === "lld" ? ["-flavor", "link", "--version"] : ["--version"];
    assert.ok(run(tools[name], args).includes(expected.version), `${name} version mismatch`);
  }

  writeFileSync(join(output, "pe32-arithmetic.s"), source);
  const commands = {
    clang: ["--target=i686-pc-windows-msvc", "-c", "pe32-arithmetic.s", "-o", "pe32-arithmetic.obj"],
    lld: ["-flavor", "link", "/entry:entry", "/subsystem:console", "/machine:x86", "/nodefaultlib",
      "/base:0x400000", "/fixed", "/dynamicbase:no", "/nxcompat", "/safeseh:no", "/timestamp:0",
      "/out:pe32-arithmetic.exe", "pe32-arithmetic.obj"],
    objdump: ["--private-headers", "--section-headers", "--disassemble", "pe32-arithmetic.exe"],
  };
  run(tools.clang, commands.clang, output);
  run(tools.lld, commands.lld, output);
  const inspection = run(tools.objdump, commands.objdump, output);
  writeFileSync(join(output, "inspection.txt"), inspection);
  const expected = spec.expectation;
  const normalized = inspection.replace(/[\t ]+/g, " ");
  for (const fragment of [
    `file format ${expected.machine}`, `Magic ${expected.optionalHeader}`,
    `AddressOfEntryPoint ${expected.entryRva}`, `ImageBase ${expected.imageBase}`,
    "Time/Date Thu Jan 1 00:00:00 1970", "relocations stripped", "NX_COMPAT",
    "Subsystem 00000003 (Windows CUI)", "Entry 1 00000000 00000000 Import Directory",
    "Entry c 00000000 00000000 Import Address Table Directory",
    "Entry d 00000000 00000000 Delay Import Directory",
  ]) assert.ok(normalized.includes(fragment), `unexpected PE evidence: ${fragment}`);
  assert.ok(!inspection.includes("DYNAMIC_BASE"), "ASLR must be disabled");
  const sections = [...inspection.matchAll(/^\s+\d+\s+(\.\S+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+(\S+)$/gm)];
  assert.deepEqual(sections.map((section) => section.slice(1)), [[".text", "0000000b", "00401000", "TEXT"]]);
  const instructions = [...inspection.matchAll(/^\s+([0-9a-f]+):\s+((?:[0-9a-f]{2}\s+)+)(\S.*)$/gm)];
  const bytes = instructions.map((instruction) => instruction[2].replace(/\s/g, "")).join("");
  assert.equal(bytes, expected.entryBytes, "entry bytes mismatch");
  assert.deepEqual(instructions.map((instruction) => instruction[3].replace(/\s+/g, " ").trim()), expected.instructions);
  assert.equal(Number.parseInt(instructions[0][1], 16), 0x401000);
  assert.equal(Number.parseInt(instructions[2][1], 16) - 0x401000, expected.int3Offset);

  const binary = readFileSync(join(output, "pe32-arithmetic.exe"));
  commands.objcopy = verifyTextPermissions(output, tools.objcopy);
  const evidence = {
    ...spec,
    commands,
    environment: { platform: process.platform, architecture: process.arch, node: process.versions.node, locale: "C", timezone: "UTC" },
    artifact: { path: "pe32-arithmetic.exe", size: binary.length, sha256: sha256(binary) },
    inspection: { path: "inspection.txt", sha256: sha256(inspection), rxFlagsConfirmedByUnchangedLlvmCopy: true },
  };
  writeFileSync(join(output, "evidence.json"), `${JSON.stringify(evidence, null, 2)}\n`);
  return evidence;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const outputRoot = join(target, "corpus0");
    prepareOutputParents(join(outputRoot, "run-"));
    const output = mkdtempSync(join(outputRoot, "run-"));
    const first = buildFixture(join(output, "first"));
    const second = buildFixture(join(output, "second"));
    assert.deepEqual(readFileSync(join(output, "first/pe32-arithmetic.exe")), readFileSync(join(output, "second/pe32-arithmetic.exe")), "binary repeatability mismatch");
    assert.equal(first.artifact.sha256, second.artifact.sha256);
    writeFileSync(join(output, "repeatability.json"), `${JSON.stringify({ verified: true, first: "first/evidence.json", second: "second/evidence.json", sha256: first.artifact.sha256 }, null, 2)}\n`);
    console.log(`PE32 fixture verified: ${first.artifact.size} bytes, SHA-256 ${first.artifact.sha256}\nEvidence: ${output}`);
  } catch (error) {
    console.error(`[ring3 corpus] ${error.message}`);
    process.exitCode = 1;
  }
}
