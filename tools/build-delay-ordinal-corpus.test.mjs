import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildDelayImportFixtures, ordinalContract } from "./build-delay-corpus.mjs";
import { sha256 } from "./corpus-tools.mjs";

const outputRoot = new URL("../target/delay-ordinal-corpus-tests/", import.meta.url);
mkdirSync(outputRoot, { recursive: true });

test("ordinal delay fixtures reproduce the archived NONAME and full-width INT bytes", () => {
  const directory = mkdtempSync(new URL("repeat-", outputRoot));
  try {
    const first = buildDelayImportFixtures(join(directory, "first"), { ordinal: true });
    const second = buildDelayImportFixtures(join(directory, "second"), { ordinal: true });
    assert.deepEqual(first, second);
    assert.equal(first.executed, false);
    assert.equal(first.windowsOracle, "not-run");
    let instances = 0;
    for (const architecture of ["i386", "amd64"]) {
      const spec = ordinalContract.architectures[architecture];
      for (const [name, expected] of Object.entries({ ...ordinalContract.sources, ...spec.artifacts })) {
        const a = readFileSync(join(directory, "first", architecture, name));
        assert.deepEqual(a, readFileSync(join(directory, "second", architecture, name)));
        assert.equal(sha256(a), expected.sha256);
        instances += 2;
      }
      const exe = readFileSync(join(directory, "first", architecture, "delayed.exe"));
      const raw = architecture === "i386" ? BigInt(exe.readUInt32LE(1600)) : exe.readBigUInt64LE(1608);
      assert.equal(raw, architecture === "i386" ? 0x80008000n : 0x8000000000008000n);
      assert.equal(raw & 0xffffn, 32768n);
      const dll = readFileSync(join(directory, "first", architecture, "Ring3Delay.dll"));
      assert.deepEqual([dll.readUInt32LE(1552), dll.readUInt32LE(1556), dll.readUInt32LE(1560)], [32768, 1, 0]);
      assert.ok(first.builds[architecture].commands.linkDll.includes("/def:probe.def"));
    }
    assert.equal(instances, 40);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("ordinal definition, raw INT, export and artifact mismatches refuse success evidence", () => {
  const directory = mkdtempSync(new URL("negative-", outputRoot));
  try {
    for (const [name, change, message] of [
      ["definition", spec => { spec.sources["probe.def"].sha256 = "0".repeat(64); }, /source SHA-256 mismatch/],
      ["lookup", spec => { spec.architectures.amd64.lookup.rawValue = "9223372036854808577"; }, /ordinal lookup mismatch/],
      ["export", spec => { spec.architectures.i386.exports.nameCount = 1; }, /ordinal export mismatch/],
      ["export-directory", spec => { spec.architectures.i386.exports.directoryRva += 4; }, /ordinal export directory mismatch/],
      ["export-address", spec => { spec.architectures.amd64.exports.firstAddressRva += 1; }, /ordinal export address mismatch/],
      ["export-table", spec => { spec.architectures.amd64.exports.addressTableRva += 4; }, /ordinal export table mismatch/],
      ["export-file", spec => { spec.architectures.amd64.exports.addressTableFileOffset += 1; }, /ordinal export table offset mismatch/],
      ["artifact", spec => { spec.architectures.amd64.artifacts["Ring3Delay.dll"].sha256 = "0".repeat(64); }, /artifact SHA-256 mismatch/],
    ]) {
      const spec = structuredClone(ordinalContract); change(spec);
      assert.throws(() => buildDelayImportFixtures(join(directory, name), { ordinal: true, contract: spec }), message);
      assert.equal(existsSync(join(directory, name, "evidence.json")), false);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("ordinal producer preserves existing output and refuses linked ancestors", () => {
  const directory = mkdtempSync(new URL("paths-", outputRoot));
  const outside = mkdtempSync(join(tmpdir(), "ring3-ordinal-delay-outside-"));
  try {
    const before = readdirSync(directory);
    assert.throws(() => buildDelayImportFixtures(directory, { ordinal: true }), { code: "EEXIST" });
    assert.deepEqual(readdirSync(directory), before);
    symlinkSync(outside, join(directory, "redirect"), "dir");
    assert.throws(() => buildDelayImportFixtures(join(directory, "redirect", "fixture"), { ordinal: true }), /real directories/);
    assert.deepEqual(readdirSync(outside), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("ordinal CLI rejects extra arguments before writing either output family", () => {
  const outputs = ["../target/corpus-delay/", "../target/corpus-delay-ordinals/"].map(path => new URL(path, import.meta.url));
  const list = path => existsSync(path) ? readdirSync(path).sort() : null;
  const before = outputs.map(list);
  for (const args of [["--ordinal", "extra"], ["--ordinals"], ["--ordinal", "--ordinal"]]) {
    const result = spawnSync(process.execPath, [fileURLToPath(new URL("./build-delay-corpus.mjs", import.meta.url)), ...args], { encoding: "utf8", timeout: 5000, maxBuffer: 1024 * 1024 });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /unsupported corpus arguments/);
    assert.equal(result.stdout, "");
    assert.deepEqual(outputs.map(list), before);
  }
});
