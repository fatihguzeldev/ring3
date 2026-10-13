import assert from "node:assert/strict";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";

const [binaryPath, outputPath] = process.argv.slice(2);
assert(binaryPath && outputPath);
const binary = readFileSync(binaryPath);
const module = new WebAssembly.Module(binary);
const backend = new WebAssembly.Instance(module, {}).exports;
const { ring3_avx_probe_read: read, ring3_avx_probe_write: write, ring3_avx_probe_run: run } = backend;
for (const [fn, arity] of [[read, 1], [write, 2], [run, 4]]) assert.equal(fn.length, arity);
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../../../../../../..");
const sourcePaths = [
  "engine/src/cpu/x86/packed_strings.rs",
  "engine/src/cpu/x86/avx/strings.rs",
  "engine/src/cpu/x86/avx/strings/native.rs",
  "engine/src/cpu/x86/avx/strings/fixtures/runtime/run.mjs",
  "engine/src/abi/wasm/avx_proof.rs",
];
const sha = bytes => createHash("sha256").update(bytes).digest("hex");
const sources = sourcePaths.map(path => ({ path, sha256: sha(readFileSync(resolve(root, path))) }));
let calls = 0;
const groups = {};
const modes = [109, 110, 111, 112];
const explicit = id => id < 111;
const indexOutput = id => id % 2 === 1;
const seed = () => {
  const words = Uint32Array.from({ length: 96 }, (_, i) => (0xa5a50000 ^ Math.imul(i, 0x01010101)) >>> 0);
  words[9] = 0x244fd7;
  words[10] = 0xe5af;
  return words;
};
const lower = (words, register, bytes) => {
  for (let lane = 0; lane < 4; lane++) words[12 + register * 4 + lane] = bytes.readUInt32LE(lane * 4);
};
const vector = (words, register) => {
  const bytes = Buffer.alloc(16);
  for (let lane = 0; lane < 4; lane++) bytes.writeUInt32LE(words[12 + register * 4 + lane], lane * 4);
  return bytes;
};
const operand = (words, bytes) => {
  for (let lane = 0; lane < 4; lane++) words[76 + lane] = bytes.readUInt32LE(lane * 4);
};
const memory = words => {
  const bytes = Buffer.alloc(16);
  for (let lane = 0; lane < 4; lane++) bytes.writeUInt32LE(words[76 + lane], lane * 4);
  return bytes;
};
const regs = (a, b, isMemory = false, d = 0) => (d | (a << 8) | (b << 16) | (isMemory ? 1 << 24 : 0)) >>> 0;

// the oracle uses the sdm boolean matrix and validity table, independent of rust helpers.
function compare(leftBytes, rightBytes, leftRaw, rightRaw, id, control) {
  const word = (control & 1) !== 0;
  const signed = (control & 2) !== 0;
  const count = word ? 8 : 16;
  const values = bytes => Array.from({ length: count }, (_, i) => word
    ? (signed ? bytes.readInt16LE(i * 2) : bytes.readUInt16LE(i * 2))
    : (signed ? bytes.readInt8(i) : bytes[i]));
  const left = values(leftBytes), right = values(rightBytes);
  const length = (data, raw) => explicit(id)
    ? Math.min(Math.abs(raw | 0), count)
    : ((data.indexOf(0) + 1) || count + 1) - 1;
  const leftLength = length(left, leftRaw), rightLength = length(right, rightRaw);
  const aggregation = (control >>> 2) & 3;
  const validLeft = Array.from({ length: count }, (_, i) => i < leftLength);
  const validRight = Array.from({ length: count }, (_, i) => i < rightLength);
  const matrix = Array.from({ length: count }, (_, i) => Array.from({ length: count }, (_, j) => {
    if (aggregation === 0) return validLeft[i] && validRight[j] && left[i] === right[j];
    if (aggregation === 2) return (!validLeft[i] && !validRight[j]) || (validLeft[i] && validRight[j] && left[i] === right[j]);
    if (aggregation === 3) return !validLeft[i] || (validRight[j] && left[i] === right[j]);
    return validLeft[i] && validRight[j] && (i % 2 === 0 ? left[i] <= right[j] : right[j] <= left[i]);
  }));
  const positive = Array.from({ length: count }, (_, j) => {
    if (aggregation === 0) return matrix.some(row => row[j]);
    if (aggregation === 1) return matrix.some((row, i) => i % 2 === 0 && matrix[i + 1][j] && row[j]);
    if (aggregation === 2) return matrix[j][j];
    return matrix.slice(0, count - j).every((row, i) => row[j + i]);
  });
  const polarity = (control >>> 4) & 3;
  const selected = positive.map((value, j) => value !== (polarity === 1 || (polarity === 3 && validRight[j])));
  const bits = selected.reduce((result, value, i) => result | (Number(value) << i), 0);
  const mask = Buffer.alloc(16);
  if ((control & 0x40) === 0) mask.writeUInt16LE(bits);
  else for (let i = 0; i < count; i++) if (selected[i]) mask.fill(0xff, i * (word ? 2 : 1), (i + 1) * (word ? 2 : 1));
  const candidates = selected.flatMap((value, i) => value ? [i] : []);
  const index = candidates.length === 0 ? count : ((control & 0x40) === 0 ? candidates[0] : candidates.at(-1));
  const flags = Number(bits !== 0) | (Number(rightLength < count) << 6) | (Number(leftLength < count) << 7) | ((bits & 1) << 11);
  return { bits, mask, index, flags };
}
function check(group, words, id, registers, control, length, status = 0) {
  const expected = Uint32Array.from(words);
  if (status === 0) {
    const a = (registers >>> 8) & 255, b = (registers >>> 16) & 255;
    const result = compare(vector(words, a), (registers & (1 << 24)) ? memory(words) : vector(words, b), words[0], words[2], id, control);
    expected[9] = ((words[9] & ~0x8d5) | result.flags) >>> 0;
    if (indexOutput(id)) expected[1] = result.index;
    else { lower(expected, 0, result.mask); expected.fill(0, 44, 48); }
  }
  for (let i = 0; i < 96; i++) assert.equal(write(i, words[i]), 0);
  const label = `${group}/${id}/${registers.toString(16)}/${control}/${length}`;
  assert.equal(run(id, registers, control, length), status, label);
  for (let i = 0; i < 96; i++) assert.equal(read(i) >>> 0, expected[i], `${label}/word${i}`);
  calls++;
  groups[group] = (groups[group] ?? 0) + 1;
}
function bytes(values, word) {
  const result = Buffer.alloc(16);
  for (let i = 0; i < Math.min(values.length, word ? 8 : 16); i++) {
    if (word) result.writeUInt16LE(values[i] & 0xffff, i * 2);
    else result[i] = values[i];
  }
  return result;
}

// literal cases also check the oracle itself before testing the backend.
for (let format = 0; format < 4; format++) {
  const word = (format & 1) !== 0, full = word ? 0xff : 0xffff;
  const a = bytes([word ? 0x8000 : 0x80, word ? 0x7fff : 0x7f, 0x20, 0x30], word);
  const b = bytes([word ? 0x7fff : 0x7f, word ? 0x8000 : 0x80, 0x20, 0x30, 0x25, word ? 0xffff : 0xff], word);
  for (let aggregation = 0; aggregation < 4; aggregation++) for (let polarity = 0; polarity < 4; polarity++) {
    const positive = [0xf, (format & 2) ? 0x3f : 0x1c, (full & ~0x3f) | 0xc, 0][aggregation];
    const literal = polarity === 1 ? positive ^ full : polarity === 3 ? positive ^ 0x3f : positive;
    const control = format | (aggregation << 2) | (polarity << 4);
    assert.equal(compare(a, b, 4, 6, 109, control).bits, literal);
    assert.equal(compare(a, b, 0x80000000, 0xffffffff, 111, control).bits, literal);
  }
}
{
  const a = Buffer.alloc(16, 9), b = Buffer.alloc(16, 9);
  a.set([1, 2, 3, 4]); b.set([1, 2], 14);
  assert.equal(compare(a, b, 4, 16, 109, 0xc).bits, 0x4000);
  for (const [control, literal] of [[0xc, 0xffff], [0x1c, 0], [0x2c, 0xffff], [0x3c, 0xfffe]]) assert.equal(compare(a, b, 0, 1, 109, control).bits, literal);
  assert.equal(compare(bytes([10, 20, 30], false), bytes([10, 20, 30, 40], false), 3, 4, 109, 4).bits, 3);
}

for (let control = 0; control < 256; control++) {
  const word = (control & 1) !== 0, count = word ? 8 : 16, high = word ? 0x8000 : 0x80, max = word ? 0xffff : 0xff;
  const orderedA = Array(count).fill(9), orderedB = Array(count).fill(9);
  orderedA.splice(0, 4, 1, 2, 3, 4); orderedB.splice(count - 2, 2, 1, 2);
  const scenarios = [
    [[high, high - 1, 0x20, 0x30], [high - 1, high, 0x20, 0x30, 0x25, max], 4, 6],
    [Array(count).fill(7), Array(count).fill(7), count, count],
    [Array(count).fill(7), Array(count).fill(8), count, count],
    [orderedA, orderedB, 4, count],
    [[0, 7, max], [0, max, 7], 0, 0],
    [[10, 20, 30], [10, 20, 30, 40], 3, 4],
    [[high, max, 0, 1], [high, max, 0, 1], 0x80000000, 0x7fffffff],
    [[1, 0, 2, 0, max], [1, 2, 0, max], 0xffffffff, 0xfffffffe],
  ];
  for (const [a, b, la, lb] of scenarios) for (const id of modes) for (const isMemory of [false, true]) {
    const words = seed(), left = bytes(a, word), right = bytes(b, word);
    lower(words, 0, left); lower(words, 1, right); operand(words, right);
    words[0] = la; words[2] = lb;
    check("all-controls-formats-aggregation-polarity", words, id, regs(0, isMemory ? 0 : 1, isMemory), control, isMemory ? 16 : 0);
  }
}
const lengths = [[0, 0], [1, 1], [-1, -1], [8, 16], [-8, -16], [17, 17], [-17, -17], [0x7fffffff, 0x80000000], [0x80000000, 0x7fffffff], [0xffffffff, 16], [16, 0], [0, 16]];
for (let control = 0; control < 256; control++) for (const [la, lb] of lengths) for (const id of [109, 110]) for (const isMemory of [false, true]) {
  const words = seed(), value = bytes(Array((control & 1) ? 8 : 16).fill(7), (control & 1) !== 0);
  lower(words, 1, value); lower(words, 2, value); operand(words, value); words[0] = la; words[2] = lb;
  check("signed-absolute-lengths-including-min-max", words, id, regs(1, isMemory ? 0 : 2, isMemory), control, isMemory ? 16 : 0);
}
for (let a = 0; a < 8; a++) for (let b = 0; b < 8; b++) for (const id of modes) for (const control of [0, 1, 0xc, 0x38, 0x4c, 0x55, 0x80, 0xff]) {
  const words = seed(); words[0] = 4; words[2] = 6;
  lower(words, a, bytes([1, 2, 3, 4, 0, 7, 8, 9], (control & 1) !== 0));
  lower(words, b, bytes([1, 9, 3, 4, 2, 0, 8, 9], (control & 1) !== 0));
  check("all-input-aliases-including-XMM0", words, id, regs(a, b), control, 0);
}
for (const word of [false, true]) for (let stop = 0; stop <= (word ? 8 : 16); stop++) for (const id of [111, 112]) for (const aggregation of [0, 1, 2, 3]) for (const isMemory of [false, true]) {
  const count = word ? 8 : 16, a = Array(count).fill(7), b = Array(count).fill(7);
  if (stop < count) a[stop] = 0;
  if (count - stop < count) b[count - stop] = 0;
  const words = seed(), left = bytes(a, word), right = bytes(b, word);
  lower(words, 0, left); lower(words, 1, right); operand(words, right);
  words[0] = 0x80000000; words[2] = 0x7fffffff;
  check("implicit-null-at-every-position", words, id, regs(0, isMemory ? 0 : 1, isMemory), Number(word) | (aggregation << 2), isMemory ? 16 : 0);
}
for (const id of modes) {
  for (const length of [0, 1, 15, 17, 32, 33]) {
    const words = seed(); words[0] = words[2] = 0; lower(words, 0, Buffer.alloc(16));
    check("zero-length-still-exact-memory-read16", words, id, regs(0, 0, true), 0, length, 1);
  }
  check("register-rejects-memory-payload", seed(), id, regs(0, 1), 0, 16, 1);
  const invalid = seed(); invalid[10] = 0x10000;
  check("invalid-MXCSR-state-neutral", invalid, id, regs(0, 1), 0, 0, 2);
  for (const registers of [regs(8, 1), regs(0, 8), regs(0, 1, false, 8), (regs(0, 1) | 0x80000000) >>> 0]) check("malformed-register-neutral", seed(), id, registers, 0, 0, 1);
  check("immediate-range-neutral", seed(), id, regs(0, 1), 256, 0, 1);
}
check("unknown-operation-neutral", seed(), 0xffffffff, regs(0, 1), 0, 0, 1);
assert.equal(calls, 31189);
const receipt = {
  calls, groups, engine_sha256: sha(binary), sources,
  oracle: "independent SDM boolean validity matrix, reductions, polarity and literal legacy truth vectors; no calls into Rust to derive expected values",
  coverage: "all256 controls/four forms/register-memory; signed absolute MIN/MAX; every null position; truncated physical-end order; odd ranges; XMM0 aliases; all384B staged state",
  scope: "real Rust-Wasm private kernel; payload length contract only, future guest paging/admission not activated",
  publicAVX: false,
};
mkdirSync(resolve(outputPath), { recursive: true });
writeFileSync(resolve(outputPath, "result.json"), JSON.stringify(receipt, null, 2) + "\n");
console.log(JSON.stringify(receipt));
