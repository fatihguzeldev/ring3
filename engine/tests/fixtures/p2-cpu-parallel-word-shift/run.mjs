import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[0-9a-f]{64}$/);
const engine = readEngine(enginePath);
const SIZE = 4364, FP = 4236, PC = 0x1000;
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const program = Buffer.from(
  "66b9210066d3e166d3e16683f801f966b9200066d3e40f92c20f90c60f94c3" +
  "66c1ff106683d5000f92c466c1ee106683db0066d1fa0f92c166d3e90f94c2" +
  "66d1e20f94c766d3fd75020f0beb00", "hex");
assert.equal(program.length, 77);
assert.deepEqual(program, readFileSync(join(output, "word-shift.x86")), "independent Rust/JS literal bank");
writeFileSync(join(output, "engine.wasm"), engine.bytes, {flag: "wx"});
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const frames = [], phases = [], modules = [], inputs = [], hosts = [], initialFrames = [];
const paths = [
  "engine/tests/cpu_parallel_word_shift_wasm.rs",
  "engine/tests/fixtures/p2-cpu-parallel-word-shift/run.mjs",
  "engine/tests/fixtures/support/engine.mjs",
];
const sourcePins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const beforeSources = sourcePins();

function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4));
  return bytes;
}
const state = (registers, pc, flags) => record("R3ST", 56, [...registers, pc, flags]);
const exit = (reason, retired) => record("R3EX", 40, [reason, retired, 0, 0, 0, 0]);
function words32(values) {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4));
  return bytes;
}
function fpu(opaque = false) {
  const bytes = record("R3FP", 128);
  bytes.writeUInt16LE(opaque ? 0x27f : 0x37f, 16);
  bytes.writeUInt16LE(opaque ? 0x81a5 : 0, 18);
  bytes.writeUInt16LE(opaque ? 0x55aa : 0xffff, 20);
  bytes.writeUInt16LE(opaque ? 0x7ff : 0, 22);
  bytes.writeUInt32LE(opaque ? 0xf1234567 : 0, 24);
  bytes.writeUInt32LE(opaque ? 0xfedcba98 : 0, 28);
  bytes.writeUInt16LE(opaque ? 0xf135 : 0, 32);
  bytes.writeUInt16LE(opaque ? 0xe246 : 0, 34);
  if (opaque) {
    bytes.set(Array.from({length: 80}, (_, i) => (i * 73 + 29) & 255), 40);
  }
  return bytes;
}
function arena(ctx) {
  return Buffer.from(new Uint8Array(ctx.memory.buffer, ctx.base, SIZE));
}
function check(ctx, label) {
  assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.entries}/${label}`);
}
function input(ctx, offset, bytes, label) {
  assert.equal(ctx.started, false, "no interphase host CPU/RAM input");
  new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + offset);
  ctx.expected.set(bytes, offset);
  check(ctx, label);
  inputs.push({context: ctx.context, kind: "arena", offset, hex: Buffer.from(bytes).toString("hex"), label});
}
function host(ctx, name, ...args) {
  check(ctx, `before ${name}`);
  const status = ctx.api[name](...args);
  assert.equal(status, 0, name);
  check(ctx, name);
  hosts.push({context: ctx.context, name, args, status});
}
function phase(ctx, label, budget, registers, pc, flags, reason, retired = 1) {
  check(ctx, `before ${label}`);
  const beforeFrame = frames.length;
  frames.push(arena(ctx));
  ctx.expected.set(state(registers, pc, flags));
  ctx.expected.set(exit(reason, retired), 56);
  const args = [ctx.base, ctx.base + 56, budget, ctx.base + 96];
  const status = ctx.run(...args);
  assert.equal(status, 0);
  check(ctx, `after ${label}`);
  frames.push(arena(ctx));
  phases.push({context: ctx.context, owner: ctx.owner, entries: ctx.entries, label, args, status,
    budget, before_frame: beforeFrame, after_frame: beforeFrame + 1, retired, reason});
}

for (const owner of ["replacement", "resident"]) {
  for (const entries of [false, true]) {
    const context = modules.length;
    const key = [context + 1, 0x57534846];
    const instance = new WebAssembly.Instance(engine.module, {});
    const ctx = {owner, entries, context, memory: instance.exports.memory, api: {}, started: false};
    for (const name of ["open", "close", "arena_ptr", "map", "protect", "upload", "compile",
      "compile_entries", "compile_resident", "compile_resident_entries", "generation",
      "module_ptr", "module_len", "guard", "guard_resident"]) {
      ctx.api[name] = instance.exports["ring3_abi_v1_" + name];
    }
    const openArgs = [1, ...key];
    const openStatus = ctx.api.open(...openArgs);
    assert.equal(openStatus, 0);
    hosts.push({context, name: "open", args: openArgs, status: openStatus});
    ctx.base = ctx.api.arena_ptr() >>> 0;
    ctx.expected = Buffer.alloc(SIZE);
    ctx.expected.set(state(Array(8).fill(0), 0, 2));
    ctx.expected.set(exit(1, 0), 56);
    ctx.expected.set(record("R3MH", 40, [0, 0, 0, 0, 0, 0]), 100);
    ctx.expected.set(fpu(), FP);
    check(ctx, "initialized full arena");
    host(ctx, "map", PC, 1, 7);
    input(ctx, 140, program, "literal guest bank");
    host(ctx, "upload", PC, program.length);
    inputs.push({context, kind: "upload", address: PC, hex: program.toString("hex"), label: "literal guest bank"});
    host(ctx, "protect", PC, 1, 4);
    input(ctx, 140, words32(entries ? [PC, PC + 75] : [PC, 73, PC + 75, 2]), "two explicit or entry block seeds");
    const compileName = owner === "replacement"
      ? entries ? "compile_entries" : "compile"
      : entries ? "compile_resident_entries" : "compile_resident";
    const compileArgs = entries ? [2, 0] : [2];
    let binding;
    if (owner === "replacement") {
      host(ctx, compileName, ...compileArgs);
      binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
    } else {
      check(ctx, "before resident compile");
      const status = ctx.api[compileName](...compileArgs);
      assert.equal(status, 0);
      hosts.push({context, name: compileName, args: compileArgs, status});
      const receipt = arena(ctx);
      assert.equal(receipt.readUInt32LE(140), 1);
      assert.equal(receipt.readUInt32LE(144), 24);
      binding = Object.fromEntries(["low", "high", "pointer", "length"].map((name, i) => [name, receipt.readUInt32LE(148 + 4 * i)]));
      ctx.expected.set(words32([1, 24, binding.low, binding.high, binding.pointer, binding.length]), 140);
      check(ctx, "resident receipt only");
    }
    const bytes = Buffer.from(new Uint8Array(ctx.memory.buffer, binding.pointer, binding.length));
    const module = new WebAssembly.Module(bytes);
    const guard = owner === "replacement" ? "guard" : "guard_resident";
    assert.deepEqual(WebAssembly.Module.imports(module), [
      {module: "env", name: "memory", kind: "memory"},
      {module: "ring3", name: guard, kind: "function"},
    ]);
    assert.deepEqual(WebAssembly.Module.exports(module), [{name: "run", kind: "function"}]);
    ctx.run = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}).exports.run;
    const file = `${owner}-${entries ? "entry" : "explicit"}.wasm`;
    writeFileSync(join(output, file), bytes, {flag: "wx"});
    modules.push({context, owner, entries, arena_base: ctx.base, key_low: key[0], key_high: key[1], file,
      ...binding, bytes: bytes.length, sha256: hash(bytes)});

    const registers = [0xa53c8000, 0x91b2aa55, 0x34568001, 0x87a9ffff,
      0xc5e60000, 0x5678ffff, 0x6789ffff, 0x789a8001];
    const flags = [0xcd6, 0x402, 0x8d6, 2][context];
    const df = flags & 0x400;
    input(ctx, 0, state(registers, PC, flags), "single initial CPU seed");
    input(ctx, FP, fpu(true), "opaque FP128 seed");
    initialFrames.push({context, frame: frames.length});
    ctx.started = true;
    const inputCount = inputs.length, hostCount = hosts.length;

    registers[1] = 0x91b20042;
    phase(ctx, "MOV CX21 then SHL CX currentCL21 masks to1", 2, registers, PC + 7, 0x6 | df, 1, 2);
    registers[1] = 0x91b20108;
    phase(ctx, "SHL current CX42 by its new CL42 masks to2", 1, registers, PC + 10, 0x2 | df, 1);
    registers[1] = 0x91b20020;
    phase(ctx, "CMP AX8000,1 then STC then MOV CX32 dirties CF OF AF", 3, registers, PC + 19, 0x817 | df, 1, 3);
    phase(ctx, "SHL SP0 with currentCL32 preserves parent and entire dirty flags", 1, registers, PC + 22, 0x817 | df, 1);
    registers[2] = 0x34560101;
    registers[3] = 0x87a9ff00;
    phase(ctx, "SETB DL1 SETO DH1 SETZ BL0 consume q0 flags", 3, registers, PC + 31, 0x817 | df, 1, 3);
    registers[7] = 0x789affff;
    phase(ctx, "SAR DI8001 by16 sign fills and retains sign carry", 1, registers, PC + 35, 0x87 | df, 1);
    registers[5] = 0x56780000;
    registers[0] = 0xa53c0100;
    phase(ctx, "ADC BPFFFF,0 consumes SAR CF1 then SETB AH1", 2, registers, PC + 42, 0x57 | df, 1, 2);
    registers[6] = 0x67890000;
    phase(ctx, "SHR SIFFFF by16 gives zero and declared CF0", 1, registers, PC + 46, 0x46 | df, 1);
    phase(ctx, "SBB current BXFF00,0 consumes SHR CF0", 1, registers, PC + 50, 0x86 | df, 1);
    registers[2] = 0x34560080;
    phase(ctx, "SAR current DX0101 by1 gives80 and CF1", 1, registers, PC + 53, 0x3 | df, 1);
    registers[1] = 0x91b20000;
    phase(ctx, "SETB CL1 then SHR current CX1 by currentCL1 gives0", 2, registers, PC + 59, 0x47 | df, 1, 2);
    registers[2] = 0x34560002;
    phase(ctx, "SETZ DL1 then SHL current DX1 by1 gives2 and ZF0", 2, registers, PC + 65, 0x2 | df, 1, 2);
    registers[3] = 0x87a90000;
    phase(ctx, "SETZ BH0 then SAR BP0 with currentCL0 preserves ZF0", 2, registers, PC + 71, 0x2 | df, 1, 2);
    phase(ctx, "JNZ consumes q0-preserved ZF0 and skips UD2", 1, registers, PC + 75, 0x2 | df, 1);
    phase(ctx, "cold JMP", 2, registers, PC + 77, 0x2 | df, 3);

    assert.equal(inputs.length, inputCount, "retained phases have no host CPU/RAM input");
    assert.equal(hosts.length, hostCount, "no interphase host diagnostics");
    check(ctx, "before close");
    const status = ctx.api.close();
    assert.equal(status, 0);
    hosts.push({context, name: "close", args: [], status});
  }
}

const raw = Buffer.concat(frames);
writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
assert.equal(frames.length, 120);
assert.equal(phases.length, 60);
assert.equal(inputs.length, 20);
assert.equal(hosts.length, 24);
assert.equal(phases.reduce((sum, phase) => sum + phase.retired, 0), 96);
assert.deepEqual(sourcePins(), beforeSources);
const result = {
  status: "ok", engine: {file: "engine.wasm", bytes: engine.bytes.length, sha256: engine.sha256},
  modules, initial_frames: initialFrames, inputs, hosts, phases,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)},
  source_sha256: beforeSources,
  counts: {owners: 2, descriptor_modes: 2, contexts: 4, modules: 4, word_shls: 16, word_shrs: 8,
    word_sars: 12, word_movs: 8, word_cmps: 4, word_adcs: 4, word_sbbs: 4, stc: 4, setcc: 28,
    conditional_jumps: 4, jumps: 4, generated_calls: 60, retired: 96, raw_frames: 120,
    arena_inputs: 16, uploads: 4, host_rows: 24, physical_files: 8, maximum_live_resident_units: 1},
  claim: "retained current-CX/CL alias shifts21->42->108; CMP/STC dirty817 then q0 SHL SP0 preserves flags consumed by SETcc; SAR DI8001,16 sign-fill/CF1 feeds ADC BPFFFF,0 wrap; SHR SIFFFF,16 declaredCF0 feeds SBB currentBX; SAR currentDX0101,1 carry feeds SETBCL then SHR CX1,CL1; SETZDL feeds SHLDX and its ZF0 feeds SETZBH then q0SARBP with newly-produced CL0; JNZ skips unseeded UD2 then cold. Fifteen generated phases and24 retired instructions/context across four bound replacement/resident explicit/entry contexts, two declared blocks, one resident unit/context and no interphase host CPU/RAM repair or diagnostics. Full4364 generated before/after images include current parents/high16/SP/opaqueFP128/helper/transfer. Guard-only imports/ExitV1. Ordered setup host records/live full-arena assertions are not saved host-frame journals, wholeRAM dumps, arbitrary module-body or allocator proof; close preclose assertion/API0 only, no postclose raw run. Literal expected anchors are producer correspondence only; independent model must reconstruct from physical before/input bytes. Declared undefined FLAGS policy is Ring3 deterministic behavior, not hardware equality.",
};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"});
console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
