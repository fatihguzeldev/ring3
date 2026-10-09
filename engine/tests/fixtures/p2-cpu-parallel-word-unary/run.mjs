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
const program = Buffer.from("f966400f90c10f92c266f7d40f94c36683d500664e0f92c666f7df0f90c566f7dd0f92c40f94c774020f0beb00", "hex");
assert.equal(program.length, 45);
assert.deepEqual(program, readFileSync(join(output, "word-unary.x86")), "independent Rust/JS literal bank");
writeFileSync(join(output, "engine.wasm"), engine.bytes, {flag: "wx"});
const hash = bytes => createHash("sha256").update(bytes).digest("hex"), frames = [], phases = [], modules = [], inputs = [], hosts = [], initialFrames = [];
const paths = ["engine/tests/cpu_parallel_word_unary_wasm.rs", "engine/tests/fixtures/p2-cpu-parallel-word-unary/run.mjs", "engine/tests/fixtures/support/engine.mjs"];
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
function phase(ctx, label, budget, registers, pc, flags, reason, retired = 1) {
  check(ctx, `before ${label}`); const beforeFrame = frames.length; frames.push(arena(ctx));
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);
  const args = [ctx.base, ctx.base + 56, budget, ctx.base + 96], status = ctx.run(...args); assert.equal(status, 0); check(ctx, `after ${label}`);
  frames.push(arena(ctx)); phases.push({context: ctx.context, owner: ctx.owner, entries: ctx.entries, label, args, status, budget, before_frame: beforeFrame, after_frame: beforeFrame + 1, retired, reason});
}
for (const owner of ["replacement", "resident"]) for (const entries of [false, true]) {
  const context = modules.length, key = [context + 1, 0x57554e52];
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
  input(ctx, 140, words32(entries ? [PC, PC + 43] : [PC, 41, PC + 43, 2]), "two explicit or entry block seeds"); let binding;
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
  const registers = [0xa53c7fff, 0x91b2aa55, 0x34560000, 0x87a97654, 0xc5e6ffff, 0x5678ffff, 0x67890000, 0x789a8000];
  const flags = [0xcd6, 0x402, 0x8d6, 2][context];
  input(ctx, 0, state(registers, PC, flags), "single initial CPU seed"); input(ctx, FP, fpu(true), "opaque FP128 seed");
  initialFrames.push({context, frame: frames.length}); ctx.started = true; const inputCount = inputs.length, hostCount = hosts.length;
  const df = flags & 0x400;
  phase(ctx, "STC makes old CF1", 1, registers, PC + 1, flags | 1, 1);
  registers[0] = 0xa53c8000; phase(ctx, "INC AX creates sign15 OF while preserving CF1", 1, registers, PC + 3, 0x897 | df, 1);
  registers[1] = 0x91b2aa01; registers[2] = 0x34560001; phase(ctx, "SETO CL and SETB DL consume INC flags", 2, registers, PC + 9, 0x897 | df, 1, 2);
  registers[4] = 0xc5e60000; phase(ctx, "NOT SP makes zero without changing dirty CF OF AF or ZF0", 1, registers, PC + 12, 0x897 | df, 1);
  registers[3] = 0x87a97600; phase(ctx, "SETZ BL observes preserved ZF0 despite NOT zero result", 1, registers, PC + 15, 0x897 | df, 1);
  registers[5] = 0x56780000; phase(ctx, "ADC BP 0 consumes NOT-preserved CF1 and wraps current FFFF", 1, registers, PC + 19, 0x57 | df, 1);
  registers[6] = 0x6789ffff; phase(ctx, "DEC SI wraps zero and preserves live ADC CF1", 1, registers, PC + 21, 0x97 | df, 1);
  registers[2] = 0x34560101; phase(ctx, "SETB DH consumes DEC-preserved CF1", 1, registers, PC + 24, 0x97 | df, 1);
  registers[7] = 0x789a8000; registers[1] = 0x91b20101; phase(ctx, "NEG DI8000 and SETO CH expose width16 overflow", 2, registers, PC + 30, 0x887 | df, 1, 2);
  phase(ctx, "NEG current BP0 clears prior NEG CF1 and sets ZF1", 1, registers, PC + 33, 0x46 | df, 1);
  registers[0] = 0xa53c0000; registers[3] = 0x87a90100; phase(ctx, "SETB AH0 and SETZ BH1 consume current zero NEG", 2, registers, PC + 39, 0x46 | df, 1, 2);
  phase(ctx, "JZ consumes NEG zero and skips UD2", 1, registers, PC + 43, 0x46 | df, 1);
  phase(ctx, "cold JMP", 2, registers, PC + 45, 0x46 | df, 3);
  assert.equal(inputs.length, inputCount, "retained phases have no host CPU/RAM input"); assert.equal(hosts.length, hostCount, "no interphase host diagnostics");
  check(ctx, "before close"); const status = ctx.api.close(); assert.equal(status, 0); hosts.push({context, name: "close", args: [], status});
}
const raw = Buffer.concat(frames); writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
assert.equal(frames.length, 104); assert.equal(phases.length, 52); assert.equal(inputs.length, 20); assert.equal(hosts.length, 24);
assert.equal(phases.reduce((sum, phase) => sum + phase.retired, 0), 64); assert.deepEqual(sourcePins(), beforeSources);
const result = {status: "ok", engine: {file: "engine.wasm", bytes: engine.bytes.length, sha256: engine.sha256}, modules, initial_frames: initialFrames, inputs, hosts, phases,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)}, source_sha256: beforeSources,
  counts: {owners: 2, descriptor_modes: 2, contexts: 4, modules: 4, word_incs: 4, word_decs: 4, word_nots: 4, word_negs: 8, word_adcs: 4, stc: 4, setcc: 28, conditional_jumps: 4, jumps: 4, generated_calls: 52, retired: 64, raw_frames: 104, arena_inputs: 16, uploads: 4, host_rows: 24, physical_files: 8, maximum_live_resident_units: 1},
  claim: "retained STC->INC AX7FFF=8000/897 preservingCF1->SETOCL1/SETBDL1->NOT SPFFFF=0 with dirtyCF/OF/AF and ZF0 preserved->SETZBL0->ADC BPFFFF,0/currentCF1=0/57->DEC SI0=FFFF/97 preservingCF1->SETBDH1->NEG DI8000=8000/887->SETOCH1->NEG currentBP0=0/46 clearingCF1->SETBAH0/SETZBH1->JZ skips unseeded UD2->cold. Thirteen generated phases and sixteen retired instructions/context across four bound replacement/resident explicit/entry contexts, two declared blocks, one resident unit/context and no interphase host CPU/RAM repair or diagnostics. Current flags/parents/SP/high16 poison and full4364 generated before/after images including opaqueFP128/helper/transfer. Pure guard-only imports/ExitV1. Ordered setup host records/live full-arena assertions are not saved host-frame journals, wholeRAM dumps, arbitrary module-body or allocator proof; close preclose assertion/API0 only, no postclose raw run. Literal expected anchors are producer correspondence only; independent model must reconstruct from physical before/input bytes"};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"}); console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
