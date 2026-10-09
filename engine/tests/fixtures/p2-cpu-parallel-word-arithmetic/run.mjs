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
const program = Buffer.from("6601c80f92c26629c86683f8ff74020f0b6683c6010f90c36683ec016683e9ff6603c96683f904eb00", "hex");
assert.equal(program.length, 41);
assert.deepEqual(program, readFileSync(join(output, "word-arithmetic.x86")), "independent Rust/JS literal bank");
writeFileSync(join(output, "engine.wasm"), engine.bytes, {flag: "wx"});
const hash = bytes => createHash("sha256").update(bytes).digest("hex"), frames = [], phases = [], modules = [], inputs = [], hosts = [], initialFrames = [];
const paths = ["engine/tests/cpu_parallel_word_arithmetic_wasm.rs", "engine/tests/fixtures/p2-cpu-parallel-word-arithmetic/run.mjs", "engine/tests/fixtures/support/engine.mjs"];
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
  const context = modules.length, key = [context + 1, 0x57505244];
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
  input(ctx, 140, words32(entries ? [PC, PC + 17] : [PC, 15, PC + 17, 24]), "two explicit or entry block seeds"); let binding;
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
  const registers = [0xa53cffff, 0x91b20001, 0x34560f10, 0x87a97654, 0xc5e68000, 0x56789abc, 0x67897fff, 0x789a2468];
  const flags = [0xcd7, 0x402, 0x8d7, 2][context];
  input(ctx, 0, state(registers, PC, flags), "single initial CPU seed"); input(ctx, FP, fpu(true), "opaque FP128 seed");
  initialFrames.push({context, frame: frames.length}); ctx.started = true; const inputCount = inputs.length, hostCount = hosts.length;
  const df = flags & 0x400;
  registers[0] = 0xa53c0000; phase(ctx, "ADD AX,CX wrap carry AF parity zero", 1, registers, PC + 3, 0x57 | df, 1);
  registers[2] = 0x34560f01; phase(ctx, "SETB DL consumes ADD carry", 1, registers, PC + 6, 0x57 | df, 1);
  registers[0] = 0xa53cffff; phase(ctx, "SUB current AX,CX borrow sign parity AF", 1, registers, PC + 9, 0x97 | df, 1);
  phase(ctx, "CMP current AX sign-extended rawFF equality", 1, registers, PC + 13, 0x46 | df, 1);
  phase(ctx, "JZ consumes current arithmetic equality and skips UD2", 1, registers, PC + 17, 0x46 | df, 1);
  registers[6] = 0x67898000; phase(ctx, "ADD SI1 sign15 overflow AF parity", 1, registers, PC + 21, 0x896 | df, 1);
  registers[3] = 0x87a97601; phase(ctx, "SETO BL consumes ADD overflow", 1, registers, PC + 24, 0x896 | df, 1);
  registers[4] = 0xc5e67fff; phase(ctx, "SUB SP1 sign15 overflow ordinary parent", 1, registers, PC + 28, 0x816 | df, 1);
  registers[1] = 0x91b20002; phase(ctx, "SUB CX signed rawFF borrow AF odd parity", 1, registers, PC + 32, 0x13 | df, 1);
  registers[1] = 0x91b20004; phase(ctx, "self ADD CX,CX consumes current low16", 1, registers, PC + 35, 2 | df, 1);
  phase(ctx, "CMP current CX4 proves signed SUB and self ADD", 1, registers, PC + 39, 0x46 | df, 1);
  phase(ctx, "cold JMP", 2, registers, PC + 41, 0x46 | df, 3);
  assert.equal(inputs.length, inputCount, "retained phases have no host CPU/RAM input"); assert.equal(hosts.length, hostCount, "no interphase host diagnostics");
  check(ctx, "before close"); const status = ctx.api.close(); assert.equal(status, 0); hosts.push({context, name: "close", args: [], status});
}
const raw = Buffer.concat(frames); writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
assert.equal(frames.length, 96); assert.equal(phases.length, 48); assert.equal(inputs.length, 20); assert.equal(hosts.length, 24);
assert.equal(phases.reduce((sum, phase) => sum + phase.retired, 0), 48); assert.deepEqual(sourcePins(), beforeSources);
const result = {status: "ok", engine: {file: "engine.wasm", bytes: engine.bytes.length, sha256: engine.sha256}, modules, initial_frames: initialFrames, inputs, hosts, phases,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)}, source_sha256: beforeSources,
  counts: {owners: 2, descriptor_modes: 2, contexts: 4, modules: 4, word_adds: 12, word_subs: 12, word_compares: 8, setcc: 8, conditional_jumps: 4, jumps: 4, generated_calls: 48, retired: 48, raw_frames: 96, arena_inputs: 16, uploads: 4, host_rows: 24, physical_files: 8},
  claim: "retained ADD AX,CX FFFF+1 wrap/carry/AF/PF/ZF->SETB DL->SUB current AX,CX borrow/sign/AF/PF->CMP AX signed83 rawFF equality->JZ skips unseeded UD2; ADD SI1 7FFF->8000 sign15 overflow->SETO BL; SUB SP1 8000->7FFF overflow/ordinary upper16 parent; SUB CX rawFF 1->2 signed16 borrow/AF/odd parity->self ADD CX,CX current2->4->CMP CX4->cold; twelve single-instruction phases/context in four replacement/resident explicit/entry contexts with two declared blocks; every generated before/after image is complete4364 including high16/all other GPRs, opaqueFP/helper/transfer; one initial CPU/FP seed percontext, no interphase host CPU/RAM repair or diagnostics; pure guard-only imports and ExitV1 only; ordered host records/live full-arena assertions are not saved host-frame journals, guestRAM dumps or allocator/generated-body proof; close has preclose assertion/APIstatus0 only, no postclose raw run"};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"}); console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
