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
assert.deepEqual(readdirSync(output).sort(), ["initial-arena.bin", "word-ror.x86"]);
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
  const bytes = Buffer.from([0x66, opcode, 0xc8 | alias, ...(family === "immediate" ? [raw] : [])]);
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
for (const raw of [0, 1, 15, 16, 17, 31, 32, 33]) anchors.set(raw, instruction("immediate", 0, raw));
for (let alias = 0; alias < 8; alias++) forms.push(instruction("cl", alias));
assert.equal(forms.length, 24);
assert.equal(at, 112);
bank.set([0xeb, 0], at);
const chain = Buffer.from(
  "66b9210066d3c966d3c90f90c20f92c66683f801f966b9200066d3cc0f90c30f92c7" +
  "66c1cf106683d50066b8008066d1c80f90c166b8008066c1c8110f90c171020f0beb00", "hex");
assert.equal(chain.length, 69);
bank.set(chain, 512);
assert.deepEqual(bank, readFileSync(join(output, "word-ror.x86")));
const groups = [
  {blocks: [[PC, 114]], instructions: 33},
  {blocks: [[PC + 512, 65], [PC + 579, 2]], instructions: 21},
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
  "engine/tests/cpu_word_ror.rs", "engine/tests/cpu_word_ror_wasm.rs",
  "engine/tests/fixtures/p2-word-ror/run.mjs", "engine/tests/fixtures/support/engine.mjs",
];
const sourcePins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const beforeSources = sourcePins(), frames = [], journal = [], modules = [];
const counts = {contexts: 0, rotates: 0, matrix: 0, anchors: 0, retained: 0,
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
function rorFlags(result, q, old) {
  if (q === 0) return old;
  const carry = result >>> 15;
  const overflow = q === 1 ? ((result >>> 15) ^ (result >>> 14)) & 1 : 0;
  return (old & ~0x801) | carry | (overflow << 11);
}
function rotate(ctx, form, category) {
  const old = cpu(ctx), value = old.registers[form.alias] & 0xffff;
  const raw = form.family === "one" ? 1 : form.family === "cl" ? old.registers[1] & 255 : form.raw;
  const q = raw & 31, distance = q % 16;
  const result = distance === 0 ? value : ((value >>> distance) | (value << (16 - distance))) & 0xffff;
  const registers = [...old.registers];
  if (q !== 0) registers[form.alias] = ((registers[form.alias] & 0xffff0000) | result) >>> 0;
  run(ctx, 1, `${category} ${form.family}/${form.alias}/raw${raw}`, {...old, registers,
    pc: old.pc + form.length, flags: rorFlags(result, q, old.flags)});
  counts.rotates++;
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
  assert.equal(ctx.api.open(1, ctx.context, 0x57524f52), 0);
  ctx.base = ctx.api.arena_ptr() >>> 0;
  assert.deepEqual(arena(ctx), initial, "current initialized arena equals canonical Rust bytes");
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
      for (let variant = 0; variant < 2; variant++) {
        const raw = rawCounts[(form.alias + variant * 8) % rawCounts.length];
        const value = form.family === "cl" && form.alias === 1
          ? (variant === 0 ? 0x8000 : 0x7f00) | raw
          : edges[(form.alias + variant * 3) % edges.length];
        seed(ctx, form.pc, form.alias + variant, variant === 0 ? 0xcd7 : 2,
          form.alias, value, form.family === "cl" ? raw : undefined);
        rotate(ctx, form, "matrix");
      }
    }
    for (const anchor of [
      {raw: 0, value: 0xffff, flags: 0xcd7, result: 0xffff, afterFlags: 0xcd7},
      {raw: 1, value: 0x8000, flags: 0xcd6, result: 0x4000, afterFlags: 0xcd6},
      {raw: 15, value: 1, flags: 0xcd7, result: 2, afterFlags: 0x4d6},
      {raw: 16, value: 0x8001, flags: 0xcd6, result: 0x8001, afterFlags: 0x4d7},
      {raw: 17, value: 0x8000, flags: 0xcd6, result: 0x4000, afterFlags: 0x4d6},
      {raw: 31, value: 0x7fff, flags: 2, result: 0xfffe, afterFlags: 3},
      {raw: 32, value: 0, flags: 0xcd7, result: 0, afterFlags: 0xcd7},
      {raw: 33, value: 0x8000, flags: 0xcd6, result: 0x4000, afterFlags: 0xcd6},
    ]) {
      const form = anchors.get(anchor.raw);
      seed(ctx, form.pc, anchor.raw, anchor.flags, 0, anchor.value);
      rotate(ctx, form, "anchors");
      assert.equal(cpu(ctx).registers[0] & 0xffff, anchor.result, "literal count/result anchor");
      assert.equal(cpu(ctx).flags, anchor.afterFlags, "literal preservedSZAP/CF/OF anchor");
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
    rotate(ctx, {family: "cl", alias: 1, length: 3}, "retained");
    assert.equal(cpu(ctx).registers[1], 0x91b28010);
    assert.equal(cpu(ctx).flags, flags);
    rotate(ctx, {family: "cl", alias: 1, length: 3}, "retained");
    assert.equal(cpu(ctx).registers[1], 0x91b28010);
    assert.equal(cpu(ctx).flags, flags & ~0x800);
    chainStep(ctx, 13, "SETO DL0 consumes q16 clearedOF", r => {r[2] = 0x34568000;}, flags & ~0x800);
    chainStep(ctx, 16, "SETB DH1 consumes q16 CF", r => {r[2] = 0x34560100;}, flags & ~0x800);
    chainStep(ctx, 20, "CMP AX8000,1", () => {}, 0x816 | df);
    chainStep(ctx, 21, "STC", () => {}, 0x817 | df);
    chainStep(ctx, 25, "MOV CX32", r => {r[1] = 0x91b20020;}, 0x817 | df);
    rotate(ctx, {family: "cl", alias: 4, length: 3}, "retained");
    chainStep(ctx, 31, "SETO BL1 consumes q0 preservedOF", r => {r[3] = 0x87a9ff01;}, 0x817 | df);
    chainStep(ctx, 34, "SETB BH1 consumes q0 preservedCF", r => {r[3] = 0x87a90101;}, 0x817 | df);
    rotate(ctx, {family: "immediate", alias: 7, length: 4, raw: 16}, "retained");
    chainStep(ctx, 42, "ADC BPFFFF,0 consumes ROR CF1", r => {r[5] = 0x56780000;}, 0x57 | df);
    chainStep(ctx, 46, "MOV AX8000", r => {r[0] = 0xa53c8000;}, 0x57 | df);
    rotate(ctx, {family: "one", alias: 0, length: 3}, "retained");
    assert.equal(cpu(ctx).flags, 0x856 | df);
    chainStep(ctx, 52, "SETO CL1 consumes definedOF", r => {r[1] = 0x91b20021;}, 0x856 | df);
    chainStep(ctx, 56, "MOV AX8000", r => {r[0] = 0xa53c8000;}, 0x856 | df);
    rotate(ctx, {family: "immediate", alias: 0, length: 4, raw: 17}, "retained");
    assert.equal(cpu(ctx).flags, 0x56 | df);
    chainStep(ctx, 63, "SETO CL0 consumes q17 undefinedOF policy", r => {r[1] = 0x91b20000;}, 0x56 | df);
    chainStep(ctx, 67, "JNO consumes q17OF0 and skips UD2", () => {}, 0x56 | df);
    chainStep(ctx, 69, "JMP then cold", () => {}, 0x56 | df, 2, 3);
    assert.equal(journal.length - beforeJournal, 21, "only retained generated runs, no host repair");
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
assert.deepEqual(counts, {contexts: 4, rotates: 248, matrix: 192, anchors: 32, retained: 24,
  generated_calls: 340, retired: 308, controls: 32});
const hostCounts = Object.fromEntries(["map", "upload", "compile", "compile_entries",
  "compile_resident", "compile_resident_entries", "close"].map(name =>
  [name, journal.filter(row => row.name === name).length]));
assert.deepEqual(hostCounts, {map: 4, upload: 8, compile: 2, compile_entries: 2,
  compile_resident: 2, compile_resident_entries: 2, close: 4});
assert.equal(modules.length, 8);
assert.equal(journal.filter(row => row.name === "input").length, 264);
assert.equal(journal.length, 628);
assert.equal(frames.length, 1244);
const raw = Buffer.concat(frames);
writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
const result = {status: "ok", engine: {bytes: engine.bytes.length, sha256: engine.sha256,
  caller_sha256: process.env.RING3_ENGINE_SHA256}, source_pins: beforeSources, modules, journal,
  counts: {...counts, modules: modules.length, host_counts: hostCounts, host_inputs: 264,
    journal_records: journal.length, raw_frames: frames.length, maximum_live_resident_units: 2},
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)},
  limits: ["finite exact register WORD ROR D1/C1/D3 across four bound profiles; six native compile profiles separate",
    "224 core rotations: every typedshape twice plus eight literal AX count anchors per profile; no exhaustive value cross-product",
    "current low16/currentCL and coupled CX operand/count; q=raw&31 distinct from distance=q%16; q0 allFLAGS preserved and q16 updatesCF even at distance0",
    "SF/ZF/AF/PF/DF and parenthigh16 preserved; q1 definedOF versus q17 deterministic undefinedOF0, not hardware equality for undefinedOF",
    "full4364 saved input/host/generated pairs plus preclose frames; canonical initial state and opaque FP128 preserved, not x87 arithmetic or fullRAM dumps",
    "code page remains RWX; stale witness has no earlier protect and same-byte consumed upload precedes stale4; closed5 status-only, no postclose allocation read",
    "33-instruction ordinary module and21-instruction two-block retained module; maximumtwo resident units, no release or expired artifact pointer read",
    "open/getters and resident receipt allocation fields are bounded physical correspondence; imports/export/hash do not prove arbitrary body or allocator history",
    "no exhaustive values/FLAGS/fullISA/hardware/performance/CI/browser/playable-game claim"]};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"});
console.log(JSON.stringify({status: result.status, ...result.counts, output}));
