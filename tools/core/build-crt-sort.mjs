import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { locateTools, root, run, sha256, target } from "../corpus/shared.mjs";

assert.ok(process.argv.length === 2 || (process.argv.length === 3 && process.argv[2] === "--write"));
const source = join(root, "core/src/execution/windows/crt/sorting.s");
const generated = join(root, "core/src/execution/windows/crt/sorting_code.rs");
const output = mkdtempSync(join(target, "crt-sort-"));
const tools = locateTools();
run(tools.clang, ["--target=i686-pc-windows-msvc", "-c", source, "-o", "sort.obj"], output);
const object = readFileSync(join(output, "sort.obj"));
assert.equal(object.readUInt16LE(0), 0x14c);
assert.equal(object.readUInt16LE(16), 0);
let bytes;
for (let i = 0; i < object.readUInt16LE(2); i++) {
  const section = 20 + i * 40;
  if (object.toString("ascii", section, section + 8).replaceAll("\0", "") !== ".text") continue;
  assert.equal(object.readUInt16LE(section + 32), 0, "helper must have no relocations");
  assert.equal(bytes, undefined, "one text section required");
  const length = object.readUInt32LE(section + 16);
  const start = object.readUInt32LE(section + 20);
  assert.ok(start + length <= object.length);
  bytes = object.subarray(start, start + length);
}
assert.ok(bytes);
assert.ok(bytes.length > 0 && bytes.length <= 0xf00);
const code = `// generated from sorting.s by tools/core/build-crt-sort.mjs.\npub(super) const CODE: &[u8] = &[${[...bytes].map(b => `0x${b.toString(16).padStart(2, "0")}`).join(", ")}];\n`;
const formatted = spawnSync("rustfmt", ["--edition", "2024", "--emit", "stdout"], { input: code, encoding: "utf8" });
if (formatted.error) throw formatted.error;
assert.equal(formatted.status, 0);
if (process.argv[2] === "--write") writeFileSync(generated, formatted.stdout);
assert.equal(readFileSync(generated, "utf8"), formatted.stdout, "generated helper differs; run with --write");
console.log(JSON.stringify({ bytes: bytes.length, sourceSha256: sha256(readFileSync(source)), codeSha256: sha256(bytes), generatorSha256: sha256(readFileSync(new URL(import.meta.url))), clangSha256: sha256(readFileSync(tools.clang)), clang: run(tools.clang, ["--version"]).split("\n")[0] }));
