import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync, readdirSync, lstatSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
const SIZE = 4364, FP = 4236, PC = 0x1000;
const rawCounts = [0, 1, 2, 15, 16, 17, 31, 32, 33, 255];
const edges = [0, 1, 0x7fff, 0x8000, 0xffff];
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[0-9a-f]{64}$/);
assert.deepEqual(readdirSync(output).sort(), ["initial-arena.bin", "word-sar.x86"]);
for (const name of readdirSync(output)) {
  const metadata = lstatSync(join(output, name));
  assert.ok(metadata.isFile() && !metadata.isSymbolicLink());
}
const engine = readEngine(enginePath);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const initial = readFileSync(join(output, "initial-arena.bin"));
assert.equal(initial.length, SIZE);
const bank = Buffer.alloc(1024, 0xcc), forms = [], anchors = new Map();
let at = 0;
function instruction(family, alias, raw = 255) {
  const opcode = {one: 0xd1, immediate: 0xc1, cl: 0xd3}[family];
  const bytes = Buffer.from([0x66, opcode, 0xf8 | alias, ...(family === "immediate" ? [raw] : [])]);
  const form = {family, alias, raw, pc: PC + at, bytes: bytes.toString("hex"), length: bytes.length};
  bank.set(bytes, at);
  at += bytes.length;
  return form;
}
for (const family of ["one", "immediate"]) {
  for (let alias = 0; alias < 8; alias++) {
    const form = instruction(family, alias);
    forms.push(form);
    if (family === "immediate" && alias === 0) anchors.set(255, form);
  }
}
for (const raw of rawCounts.slice(0, -1)) anchors.set(raw, instruction("immediate", 0, raw));
for (let alias = 0; alias < 8; alias++) forms.push(instruction("cl", alias));
assert.equal(forms.length, 24);
assert.equal(at, 116);
bank.set([0xeb, 0], at);
const chain = Buffer.from(
  "66b9210066d3f966d3f96683f801f966b9200066d3fc0f92c20f90c6" +
  "66c1ff106683d50066b9000066d3f80f92c166d3f80f90c375020f0beb00", "hex");
assert.equal(chain.length, 58);
bank.set(chain, 512);
assert.deepEqual(bank, readFileSync(join(output, "word-sar.x86")));
const groups = [
  {blocks: [[PC, 118]], instructions: 34},
  {blocks: [[PC + 512, 54], [PC + 568, 2]], instructions: 18},
];
const paths = [
  "engine/src/abi/arena.rs", "engine/src/abi/header.rs", "engine/src/abi/x86/state.rs",
  "engine/src/abi/x86/exit.rs", "engine/src/abi/x86/x87.rs", "engine/src/cpu/x86/ir.rs",
  "engine/src/cpu/x86/decode/decoder.rs", "engine/src/cpu/x86/decode/operands.rs",
  "engine/src/cpu/x86/decode/lower.rs", "engine/src/cpu/x86/decode/profile.rs",
  "engine/src/cpu/x86/decode/integer.rs", "engine/src/cpu/dbt/region.rs",
  "engine/src/cpu/dbt/wasm/integer.rs", "engine/src/cpu/dbt/wasm/emitter.rs",
  "engine/src/cpu/dbt/wasm/abi.rs", "engine/src/cpu/dbt/wasm/locals.rs",
  "engine/src/process/instance.rs", "engine/src/process/resident.rs", "engine/src/process/wasm.rs",
  "engine/tests/cpu_word_sar.rs", "engine/tests/cpu_word_sar_wasm.rs",
  "engine/tests/fixtures/p2-word-sar/run.mjs", "engine/tests/fixtures/support/engine.mjs",
];
const sourcePins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const beforeSources = sourcePins(), frames = [], journal = [], modules = [];
const counts = {contexts: 0, shifts: 0, matrix: 0, anchors: 0, retained: 0,
  generated_calls: 0, retired: 0, controls: 0};
writeFileSync(join(output, "engine.wasm"), engine.bytes, {flag: "wx"});

function record(magic, size, fields, version = 1) {
  const bytes = Buffer.alloc(size);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4));
  return bytes;
}
const state = cpu => record("R3ST", 56, [...cpu.registers, cpu.pc, cpu.flags]);
const exit = (reason, retired) => record("R3EX", 40, [reason, retired, 0, 0, 0, 0]);
const words = values => {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
};
function arena(ctx) {
  return Buffer.from(new Uint8Array(ctx.memory.buffer, ctx.base, SIZE));
}
function capture(ctx, label) {
  const bytes = arena(ctx);
  const row = {context: ctx.context, label, offset: frames.length * SIZE, bytes: SIZE, sha256: hash(bytes)};
  frames.push(bytes);
  return row;
}
function input(ctx, offset, bytes, label) {
  const before = capture(ctx, `before input ${label}`), expected = arena(ctx);
  new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + offset);
  expected.set(bytes, offset);
  assert.deepEqual(arena(ctx), expected);
  journal.push({context: ctx.context, name: "input", offset, hex: bytes.toString("hex"), label,
    before, after: capture(ctx, `after input ${label}`)});
}
function host(ctx, name, args) {
  const before = capture(ctx, `before ${name}`), expected = arena(ctx);
  assert.equal(ctx.api[name](...args), 0, name);
  const after = arena(ctx);
  if (name.startsWith("compile_resident")) {
    assert.equal(after.readUInt32LE(140), 1);
    assert.equal(after.readUInt32LE(144), 24);
    after.copy(expected, 140, 140, 164);
  }
  assert.deepEqual(after, expected, `only declared host effect ${name}`);
  journal.push({context: ctx.context, name, args, before, after: capture(ctx, `after ${name}`)});
}
function upload(ctx, address, bytes) {
  input(ctx, 140, bytes, "declared code upload");
  host(ctx, "upload", [address, bytes.length]);
}
function cpu(ctx) {
  const bytes = arena(ctx);
  return {registers: Array.from({length: 8}, (_, index) => bytes.readUInt32LE(16 + 4 * index)),
    pc: bytes.readUInt32LE(48), flags: bytes.readUInt32LE(52)};
}
function seed(ctx, pc, salt, flags, alias, value, raw) {
  const registers = Array.from({length: 8}, (_, index) =>
    (((0xa100 + index + (salt & 1) * 32) * 65536) + edges[(index + salt) % 5]) >>> 0);
  if (raw !== undefined) registers[1] = ((registers[1] & 0xffff0000) | 0xab00 | raw) >>> 0;
  if (alias !== undefined) registers[alias] = ((registers[alias] & 0xffff0000) | value) >>> 0;
  const bytes = Buffer.alloc(100);
  bytes.set(state({registers, pc, flags}));
  bytes.set(exit(3, 0), 56);
  input(ctx, 0, bytes, "explicit CPU and exit seed");
}
function run(ctx, budget, label, wanted, reason = 1, retired = 1, status = 0, customArgs) {
  const before = capture(ctx, `before generated ${label}`), expected = arena(ctx);
  if (status === 0) {
    expected.set(state(wanted));
    expected.set(exit(reason, retired), 56);
  }
  const args = customArgs ?? [ctx.base, ctx.base + 56, budget, ctx.base + 96];
  assert.equal(ctx.run(...args), status, label);
  assert.deepEqual(arena(ctx), expected, `full4364 ${label}`);
  journal.push({context: ctx.context, name: "run", label, budget, status, args,
    before, after: capture(ctx, `after generated ${label}`)});
  counts.generated_calls++;
  if (status === 0) counts.retired += retired;
}
function sarFlags(value, result, q, old) {
  if (q === 0) return old;
  let ones = 0;
  for (let bit = 0; bit < 8; bit++) ones += (result >>> bit) & 1;
  const carry = q < 16 ? (value >>> (q - 1)) & 1 : value >>> 15;
  return (old & 0x402) | carry | (ones % 2 === 0 ? 4 : 0) |
    (result === 0 ? 64 : 0) | (result & 0x8000 ? 128 : 0);
}
function shift(ctx, form, category) {
  const old = cpu(ctx), value = old.registers[form.alias] & 0xffff;
  const raw = form.family === "one" ? 1 : form.family === "cl" ? old.registers[1] & 255 : form.raw;
  const q = raw & 31, signed = (value << 16) >> 16, result = (signed >> q) & 0xffff;
  const registers = [...old.registers];
  if (q !== 0) registers[form.alias] = ((registers[form.alias] & 0xffff0000) | result) >>> 0;
  run(ctx, 1, `${category} ${form.family}/${form.alias}/raw${raw}`, {...old, registers,
    pc: old.pc + form.length, flags: sarFlags(value, result, q, old.flags)});
  counts.shifts++;
  counts[category]++;
}
function compile(ctx, group) {
  input(ctx, 140, words(group.blocks.flatMap(([pc, length]) => ctx.entries ? [pc] : [pc, length])), "block descriptors");
  const method = ctx.owner === "resident"
    ? ctx.entries ? "compile_resident_entries" : "compile_resident"
    : ctx.entries ? "compile_entries" : "compile";
  host(ctx, method, ctx.entries ? [group.blocks.length, 0] : [group.blocks.length]);
  let binding;
  if (ctx.owner === "resident") {
    const bytes = arena(ctx);
    binding = Object.fromEntries(["low", "high", "pointer", "length"].map((name, index) =>
      [name, bytes.readUInt32LE(148 + 4 * index)]));
  } else {
    binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
  }
  const bytes = Buffer.from(new Uint8Array(ctx.memory.buffer, binding.pointer, binding.length));
  const module = new WebAssembly.Module(bytes), guard = ctx.owner === "resident" ? "guard_resident" : "guard";
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module: "env", name: "memory", kind: "memory"}, {module: "ring3", name: guard, kind: "function"},
  ]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: "run", kind: "function"}]);
  ctx.run = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}).exports.run;
  const file = `${ctx.owner}-${ctx.entries ? "entry" : "explicit"}-group${groups.indexOf(group)}.wasm`;
  writeFileSync(join(output, file), bytes, {flag: "wx"});
  modules.push({context: ctx.context, owner: ctx.owner, entries: ctx.entries, group: groups.indexOf(group),
    arena_base: ctx.base, file, ...binding, bytes: bytes.length, sha256: hash(bytes)});
}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const ctx = {owner, entries, context: ++counts.contexts, memory: instance.exports.memory, api: {}};
  for (const name of ["open", "close", "arena_ptr", "map", "upload", "compile", "compile_entries",
    "compile_resident", "compile_resident_entries", "generation", "module_ptr", "module_len", "guard", "guard_resident"]) {
    ctx.api[name] = instance.exports[`ring3_abi_v1_${name}`];
  }
  assert.equal(ctx.api.open(1, ctx.context, 0x57534152), 0);
  ctx.base = ctx.api.arena_ptr() >>> 0;
  input(ctx, 0, initial, "full canonical initialized arena");
  const fp = Buffer.from(initial.subarray(FP));
  fp.writeUInt16LE(0x27f, 16);
  fp.writeUInt16LE(0x81a5, 18);
  fp.writeUInt16LE(0x55aa, 20);
  fp.writeUInt16LE(0x7ff, 22);
  fp.writeUInt32LE(0xf1234567, 24);
  fp.writeUInt32LE(0xfedcba98, 28);
  fp.writeUInt16LE(0xf135, 32);
  fp.writeUInt16LE(0xe246, 34);
  fp.set(Array.from({length: 80}, (_, index) => (index * 73 + 29) & 255), 40);
  input(ctx, FP, fp, "opaque FP128 seed");
  host(ctx, "map", [PC, 1, 7]);
  upload(ctx, PC, bank);
  return ctx;
}
function chainStep(ctx, offset, label, mutate, flags, budget = 1, reason = 1) {
  const old = cpu(ctx), registers = [...old.registers];
  mutate(registers, old);
  run(ctx, budget, label, {...old, registers, pc: PC + 512 + offset, flags}, reason);
}

for (const owner of ["replacement", "resident"]) {
  for (const entries of [false, true]) {
    const ctx = open(owner, entries), flags = ctx.context & 1 ? 0xcd7 : 0x8d7;
    compile(ctx, groups[0]);
    for (const form of forms) {
      const raw = rawCounts[form.alias % rawCounts.length];
      const value = form.family === "cl" && form.alias === 1 ? 0x8000 | raw : edges[form.alias % 5];
      seed(ctx, form.pc, form.alias, flags, form.alias, value, form.family === "cl" ? raw : undefined);
      shift(ctx, form, "matrix");
    }
    for (const raw of rawCounts) {
      for (const value of edges) {
        const form = anchors.get(raw);
        seed(ctx, form.pc, raw + value, flags, 0, value);
        shift(ctx, form, "anchors");
      }
    }
    const cl = forms.find(form => form.family === "cl" && form.alias === 0);
    for (const raw of rawCounts) {
      for (const value of [0x7fff, 0x8000]) {
        seed(ctx, cl.pc, raw, flags, 0, value, raw);
        shift(ctx, cl, "anchors");
      }
    }
    const one = forms[0];
    for (const value of edges) {
      seed(ctx, one.pc, value, flags, 0, value);
      shift(ctx, one, "anchors");
    }
    compile(ctx, groups[1]);
    const registers = [0xa53c8000, 0x91b2aa55, 0x34568001, 0x87a9ffff,
      0xc5e68000, 0x5678ffff, 0x6789ffff, 0x789a8001];
    const chainSeed = Buffer.alloc(100);
    chainSeed.set(state({registers, pc: PC + 512, flags}));
    chainSeed.set(exit(3, 0), 56);
    input(ctx, 0, chainSeed, "single retained CPU seed");
    const df = flags & 0x400, beforeJournal = journal.length;
    chainStep(ctx, 4, "MOV CX21", r => {r[1] = 0x91b20021;}, flags);
    shift(ctx, {family: "cl", alias: 1, length: 3}, "retained");
    assert.equal(cpu(ctx).registers[1], 0x91b20010);
    shift(ctx, {family: "cl", alias: 1, length: 3}, "retained");
    assert.equal(cpu(ctx).registers[1], 0x91b20000);
    chainStep(ctx, 14, "CMP AX8000,1", () => {}, 0x816 | df);
    chainStep(ctx, 15, "STC", () => {}, 0x817 | df);
    chainStep(ctx, 19, "MOV CX32", r => {r[1] = 0x91b20020;}, 0x817 | df);
    shift(ctx, {family: "cl", alias: 4, length: 3}, "retained");
    chainStep(ctx, 25, "SETB DL1", r => {r[2] = 0x34568001;}, 0x817 | df);
    chainStep(ctx, 28, "SETO DH1", r => {r[2] = 0x34560101;}, 0x817 | df);
    shift(ctx, {family: "immediate", alias: 7, length: 4, raw: 16}, "retained");
    chainStep(ctx, 36, "ADC BPFFFF,0 consumes SAR CF1", r => {r[5] = 0x56780000;}, 0x57 | df);
    chainStep(ctx, 40, "MOV CX0", r => {r[1] = 0x91b20000;}, 0x57 | df);
    shift(ctx, {family: "cl", alias: 0, length: 3}, "retained");
    chainStep(ctx, 46, "SETB CL1 consumes q0-preserved CF", r => {r[1] = 0x91b20001;}, 0x57 | df);
    shift(ctx, {family: "cl", alias: 0, length: 3}, "retained");
    chainStep(ctx, 52, "SETO BL0 consumes SAR-cleared OF", r => {r[3] = 0x87a9ff00;}, 0x86 | df);
    chainStep(ctx, 56, "JNZ skips UD2", () => {}, 0x86 | df);
    chainStep(ctx, 58, "JMP then cold", () => {}, 0x86 | df, 2, 3);
    assert.equal(journal.length - beforeJournal, 18, "only retained generated runs, no host repair");
    assert.ok(journal.slice(beforeJournal).every(row => row.name === "run"));

    seed(ctx, PC + 512, 6, flags);
    run(ctx, 0, "zero budget", cpu(ctx), 1, 0);
    counts.controls++;
    input(ctx, 96, words([1]), "set cancellation");
    run(ctx, 1, "cancellation", cpu(ctx), 2, 0);
    counts.controls++;
    input(ctx, 96, words([0]), "clear cancellation");
    for (const args of [[ctx.base + 1, ctx.base + 56, 1, ctx.base + 96],
      [ctx.base, ctx.base + 57, 1, ctx.base + 96], [ctx.base, ctx.base + 16, 1, ctx.base + 96],
      [ctx.base, ctx.base + 56, 1, ctx.base + 16]]) {
      run(ctx, 1, "invalid or overlapping pointers", cpu(ctx), 1, 0, 1, args);
      counts.controls++;
    }
    upload(ctx, PC + 512, Buffer.from([0x66]));
    run(ctx, 1, "same-byte consumed code upload stale owner", cpu(ctx), 1, 0, 4);
    counts.controls++;
    journal.push({context: ctx.context, name: "close", before: capture(ctx, "before close"), args: []});
    assert.equal(ctx.api.close(), 0);
    assert.equal(ctx.run(ctx.base, ctx.base + 56, 1, ctx.base + 96), 5, "closed status only");
    journal.push({context: ctx.context, name: "closed-run", status: 5});
    counts.generated_calls++;
    counts.controls++;
  }
}

assert.deepEqual(sourcePins(), beforeSources);
assert.deepEqual(counts, {contexts: 4, shifts: 420, matrix: 96, anchors: 300, retained: 24,
  generated_calls: 500, retired: 468, controls: 32});
const hostCounts = Object.fromEntries(["map", "upload", "compile", "compile_entries",
  "compile_resident", "compile_resident_entries", "close"].map(name =>
  [name, journal.filter(row => row.name === name).length]));
assert.deepEqual(hostCounts, {map: 4, upload: 8, compile: 2, compile_entries: 2,
  compile_resident: 2, compile_resident_entries: 2, close: 4});
assert.equal(modules.length, 8);
assert.equal(journal.filter(row => row.name === "input").length, 436);
assert.equal(journal.length, 960);
assert.equal(frames.length, 1908);
const raw = Buffer.concat(frames);
writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
const result = {status: "ok", engine: {bytes: engine.bytes.length, sha256: engine.sha256,
  caller_sha256: process.env.RING3_ENGINE_SHA256}, source_pins: beforeSources, modules, journal,
  counts: {...counts, modules: modules.length, host_counts: hostCounts, host_inputs: 436,
    journal_records: journal.length, raw_frames: frames.length, maximum_live_resident_units: 2},
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)},
  limits: ["finite exact register WORD SAR D1/C1/D3 across four bound profiles; six native compile profiles separate",
    "current low16/currentCL and coupled CX operand/count; q=raw&31, q0 fullFLAGS preservation; deterministic undefined AF/OF policy, not hardware equality",
    "full4364 saved input/host/generated pairs plus preclose frames; first canonical state and opaque FP128 preserved, not x87 arithmetic or full RAM dumps",
    "code page remains RWX so stale witness has no earlier protect; same-byte consumed upload precedes stale4, closed5 is status-only with no postclose allocation dereference",
    "34-instruction ordinary module and18-instruction two-block retained module; two live resident units maximum, no release/expired pointer use",
    "open/getters and resident receipt allocation fields are bounded physical correspondence; module imports/export/hash do not prove arbitrary body or allocator history",
    "no exhaustive values/FLAGS/fullISA/hardware/performance/CI/browser/playable-game claim"]};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"});
console.log(JSON.stringify({status: result.status, ...result.counts, output}));
