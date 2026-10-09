import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[0-9a-f]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, FP = 4236, PC = 0x1000;
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const program = Buffer.from("f96611c86619ca6611d46613f10f90c1661bf90f90c30f92c06619ed0f94c274020f0beb00", "hex");
assert.equal(program.length, 37);
assert.deepEqual(program, readFileSync(join(output, "word-carry.x86")), "independent Rust/JS literal bank");
writeFileSync(join(output, "engine.wasm"), engine.bytes, {flag: "wx"});
const hash = bytes => createHash("sha256").update(bytes).digest("hex"), frames = [], phases = [], modules = [], inputs = [], hosts = [], initialFrames = [];
const paths = ["engine/tests/cpu_parallel_word_carry_wasm.rs", "engine/tests/fixtures/p2-cpu-parallel-word-carry/run.mjs", "engine/tests/fixtures/support/engine.mjs"];
const sourcePins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))])), beforeSources = sourcePins();
function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4)); return bytes;
}
const state = (registers, pc, flags) => record("R3ST", 56, [...registers, pc, flags]);
const exit = (reason, retired) => record("R3EX", 40, [reason, retired, 0, 0, 0, 0]);
const words32 = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4)); return bytes;};
function fpu(opaque = false) {
  const bytes = record("R3FP", 128); bytes.writeUInt16LE(opaque ? 0x27f : 0x37f, 16);
  bytes.writeUInt16LE(opaque ? 0x81a5 : 0, 18); bytes.writeUInt16LE(opaque ? 0x55aa : 0xffff, 20);
  bytes.writeUInt16LE(opaque ? 0x7ff : 0, 22); bytes.writeUInt32LE(opaque ? 0xf1234567 : 0, 24);
  bytes.writeUInt32LE(opaque ? 0xfedcba98 : 0, 28); bytes.writeUInt16LE(opaque ? 0xf135 : 0, 32);
  bytes.writeUInt16LE(opaque ? 0xe246 : 0, 34);
  if (opaque) bytes.set(Array.from({length: 80}, (_, i) => (i * 73 + 29) & 255), 40);
  return bytes;
}
function arena(ctx) {return Buffer.from(new Uint8Array(ctx.memory.buffer, ctx.base, SIZE));}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.entries}/${label}`);}
function input(ctx, offset, bytes, label) {
  assert.equal(ctx.started, false, "no interphase host CPU/RAM input");
  new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + offset); ctx.expected.set(bytes, offset); check(ctx, label);
  inputs.push({context: ctx.context, kind: "arena", offset, hex: Buffer.from(bytes).toString("hex"), label});
}
function host(ctx, name, ...args) {
  check(ctx, `before ${name}`); const status = ctx.api[name](...args); assert.equal(status, 0, name); check(ctx, name);
  hosts.push({context: ctx.context, name, args, status});
}
function phase(ctx, label, budget, registers, pc, flags, reason) {
  check(ctx, `before ${label}`); const beforeFrame = frames.length; frames.push(arena(ctx));
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, 1), 56);
  const args = [ctx.base, ctx.base + 56, budget, ctx.base + 96], status = ctx.run(...args); assert.equal(status, 0); check(ctx, `after ${label}`);
  frames.push(arena(ctx)); phases.push({context: ctx.context, owner: ctx.owner, entries: ctx.entries, label, args, status, budget, before_frame: beforeFrame, after_frame: beforeFrame + 1, retired: 1, reason});
}
for (const owner of ["replacement", "resident"]) for (const entries of [false, true]) {
  const context = modules.length, key = [context + 1, 0x57434152];
  const instance = new WebAssembly.Instance(engine.module, {}), ctx = {owner, entries, context, memory: instance.exports.memory, api: {}, started: false};
  for (const name of ["open", "close", "arena_ptr", "map", "protect", "upload", "compile", "compile_entries", "compile_resident", "compile_resident_entries", "generation", "module_ptr", "module_len", "guard", "guard_resident"])
    ctx.api[name] = instance.exports["ring3_abi_v1_" + name];
  const openArgs = [1, ...key], openStatus = ctx.api.open(...openArgs); assert.equal(openStatus, 0);
  hosts.push({context, name: "open", args: openArgs, status: openStatus}); ctx.base = ctx.api.arena_ptr() >>> 0;
  ctx.expected = Buffer.alloc(SIZE); ctx.expected.set(state(Array(8).fill(0), 0, 2));
  ctx.expected.set(exit(1, 0), 56); ctx.expected.set(record("R3MH", 40, [0, 0, 0, 0, 0, 0]), 100); ctx.expected.set(fpu(), FP); check(ctx, "initialized full arena");
  host(ctx, "map", PC, 1, 7);
  input(ctx, 140, program, "literal guest bank"); host(ctx, "upload", PC, program.length);
  inputs.push({context, kind: "upload", address: PC, hex: program.toString("hex"), label: "literal guest bank"});
  host(ctx, "protect", PC, 1, 4);
  input(ctx, 140, words32(entries ? [PC, PC + 35] : [PC, 33, PC + 35, 2]), "two explicit or entry block seeds"); let binding;
  const compileName = owner === "replacement" ? entries ? "compile_entries" : "compile" : entries ? "compile_resident_entries" : "compile_resident";
  const compileArgs = entries ? [2, 0] : [2];
  if (owner === "replacement") {
    host(ctx, compileName, ...compileArgs); binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
  } else {
    check(ctx, "before resident compile"); const status = ctx.api[compileName](...compileArgs); assert.equal(status, 0);
    hosts.push({context, name: compileName, args: compileArgs, status}); const receipt = arena(ctx);
    assert.equal(receipt.readUInt32LE(140), 1); assert.equal(receipt.readUInt32LE(144), 24);
    binding = Object.fromEntries(["low", "high", "pointer", "length"].map((name, i) => [name, receipt.readUInt32LE(148 + 4 * i)]));
    ctx.expected.set(words32([1, 24, binding.low, binding.high, binding.pointer, binding.length]), 140); check(ctx, "resident receipt only");
  }
  const bytes = Buffer.from(new Uint8Array(ctx.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const guard = owner === "replacement" ? "guard" : "guard_resident";
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: "env", name: "memory", kind: "memory"}, {module: "ring3", name: guard, kind: "function"}]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: "run", kind: "function"}]);
  ctx.run = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}).exports.run;
  const file = `${owner}-${entries ? "entry" : "explicit"}.wasm`; writeFileSync(join(output, file), bytes, {flag: "wx"});
  modules.push({context, owner, entries, arena_base: ctx.base, key_low: key[0], key_high: key[1], file, ...binding, bytes: bytes.length, sha256: hash(bytes)});
  const registers = [0xa53cffff, 0x91b20000, 0x34560000, 0x87a97654, 0xc5e60000, 0x5678ffff, 0x67897fff, 0x789a8000];
  const flags = [0xcd6, 0x402, 0x8d6, 2][context];
  input(ctx, 0, state(registers, PC, flags), "single initial CPU seed"); input(ctx, FP, fpu(true), "opaque FP128 seed");
  initialFrames.push({context, frame: frames.length}); ctx.started = true; const inputCount = inputs.length, hostCount = hosts.length;
  const df = flags & 0x400;
  phase(ctx, "STC sets carry from a CF0 initial state", 1, registers, PC + 1, flags | 1, 1);
  registers[0] = 0xa53c0000; phase(ctx, "ADC AX,CX wraps FFFF plus carry", 1, registers, PC + 4, 0x57 | df, 1);
  registers[2] = 0x3456ffff; phase(ctx, "SBB DX,CX consumes current borrow and yields FFFF", 1, registers, PC + 7, 0x97 | df, 1);
  registers[4] = 0xc5e60000; phase(ctx, "ADC SP,current DX uses SBB-written FFFF and equality carry", 1, registers, PC + 10, 0x57 | df, 1);
  registers[6] = 0x67898000; phase(ctx, "ADC SI,CX sign15 overflow clears carry", 1, registers, PC + 13, 0x896 | df, 1);
  registers[1] = 0x91b20001; phase(ctx, "SETO CL updates the following SBB source", 1, registers, PC + 16, 0x896 | df, 1);
  registers[7] = 0x789a7fff; phase(ctx, "SBB DI,current CX consumes updated1 and current CF0", 1, registers, PC + 19, 0x816 | df, 1);
  registers[3] = 0x87a97601; phase(ctx, "SETO BL consumes SBB overflow", 1, registers, PC + 22, 0x816 | df, 1);
  registers[0] = 0xa53c0000; phase(ctx, "SETB AL consumes SBB cleared borrow", 1, registers, PC + 25, 0x816 | df, 1);
  registers[5] = 0x56780000; phase(ctx, "self SBB BP,BP consumes current CF0 and yields zero", 1, registers, PC + 28, 0x46 | df, 1);
  registers[2] = 0x3456ff01; phase(ctx, "SETZ DL consumes self-SBB zero and preserves DH", 1, registers, PC + 31, 0x46 | df, 1);
  phase(ctx, "JZ consumes current equality and skips UD2", 1, registers, PC + 35, 0x46 | df, 1);
  phase(ctx, "cold JMP", 2, registers, PC + 37, 0x46 | df, 3);
  assert.equal(inputs.length, inputCount, "retained phases have no host CPU/RAM input"); assert.equal(hosts.length, hostCount, "no interphase host diagnostics");
  check(ctx, "before close"); const status = ctx.api.close(); assert.equal(status, 0); hosts.push({context, name: "close", args: [], status});
}
const raw = Buffer.concat(frames); writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
assert.equal(frames.length, 104); assert.equal(phases.length, 52); assert.equal(inputs.length, 20); assert.equal(hosts.length, 24);
assert.equal(phases.reduce((sum, phase) => sum + phase.retired, 0), 52); assert.deepEqual(sourcePins(), beforeSources);
const result = {status: "ok", engine: {file: "engine.wasm", bytes: engine.bytes.length, sha256: engine.sha256}, modules, initial_frames: initialFrames, inputs, hosts, phases,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)}, source_sha256: beforeSources,
  counts: {owners: 2, descriptor_modes: 2, contexts: 4, modules: 4, word_adcs: 12, word_sbbs: 12, stc: 4, setcc: 16, conditional_jumps: 4, jumps: 4, generated_calls: 52, retired: 52, raw_frames: 104, arena_inputs: 16, uploads: 4, host_rows: 24, physical_files: 8, maximum_live_resident_units: 1},
  claim: "retained STC->ADC AXFFFF+CX0+CF1=0/57->SBB DX0-CX0-CF1=FFFF/97->ADC SP0+currentDXFFFF+CF1=0/57->ADC SI7FFF+CX0+CF1=8000/896->SETO CL1->SBB DI8000-currentCX1-CF0=7FFF/816->SETO BL1/SETB AL0->self SBB BP,BP with currentCF0=0/46->SETZ DL1->JZ skips unseeded UD2->cold. Thirteen single-instruction phases/context across four replacement/resident explicit/entry contexts, two declared blocks, one resident unit/context and no interphase host CPU/RAM repair or diagnostics; current source/carry/self/SP/high16 poison and each full4364 generated before/after image including opaqueFP128/helper/transfer. Pure guard-only imports/ExitV1. Ordered host records/live full-arena assertions are not saved host-frame journals, wholeRAM dumps, arbitrary module-body or allocator proof; close preclose assertion/API0 only, no postclose raw run. Arithmetic expected anchors are literal fixture correspondence only, independent model must reconstruct from physical before/input bytes"};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"}); console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
