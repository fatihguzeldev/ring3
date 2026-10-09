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
const program = Buffer.from("66b800806689c10fbfd166c7c4fe80668bdc0fb7eb0f92c20f90c3eb00", "hex");
assert.equal(program.length, 29);
assert.deepEqual(program, readFileSync(join(output, "word-register.x86")), "independent Rust/JS literal bank");
writeFileSync(join(output, "engine.wasm"), engine.bytes, {flag: "wx"});
const hash = bytes => createHash("sha256").update(bytes).digest("hex"), frames = [], phases = [], modules = [], inputs = [], hosts = [], initialFrames = [];
const paths = ["engine/tests/cpu_parallel_word_register_wasm.rs", "engine/tests/fixtures/p2-cpu-parallel-word-register/run.mjs", "engine/tests/fixtures/support/engine.mjs"];
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
  const context = modules.length, key = [context + 1, 0x57524d56];
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
  input(ctx, 140, words32(entries ? [PC] : [PC, program.length]), "one explicit or entry descriptor"); let binding;
  const compileName = owner === "replacement" ? entries ? "compile_entries" : "compile" : entries ? "compile_resident_entries" : "compile_resident";
  const compileArgs = entries ? [1, 0] : [1];
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
  const registers = [0xa53c1357, 0x91b22468, 0x34560f10, 0x87a97654, 0xc5e6f780, 0x56789abc, 0x6789def0, 0x789a2468];
  const flags = [0xcd7, 0x402, 0x8d7, 2][context];
  input(ctx, 0, state(registers, PC, flags), "single initial CPU seed"); input(ctx, FP, fpu(true), "opaque FP128 seed");
  initialFrames.push({context, frame: frames.length}); ctx.started = true; const inputCount = inputs.length, hostCount = hosts.length;
  registers[0] = 0xa53c8000; phase(ctx, "word opcode immediate AX8000", 1, registers, PC + 4, flags, 1);
  registers[1] = 0x91b28000; phase(ctx, "word89 current CX from AX", 1, registers, PC + 7, flags, 1);
  registers[2] = 0xffff8000; phase(ctx, "MOVSX current CX sign witness", 1, registers, PC + 10, flags, 1);
  registers[4] = 0xc5e680fe; phase(ctx, "word ModRM immediate ordinary SP80fe", 1, registers, PC + 15, flags, 1);
  registers[3] = 0x87a980fe; phase(ctx, "word8B current BX from SP", 1, registers, PC + 18, flags, 1);
  registers[5] = 0x80fe; phase(ctx, "MOVZX current BX unsigned witness", 1, registers, PC + 21, flags, 1);
  registers[2] = flags & 1 ? 0xffff8001 : 0xffff8000; phase(ctx, "SETB consumes retained carry", 1, registers, PC + 24, flags, 1);
  registers[3] = flags & 0x800 ? 0x87a98001 : 0x87a98000; phase(ctx, "SETO consumes retained overflow", 1, registers, PC + 27, flags, 1);
  phase(ctx, "cold JMP", 2, registers, PC + 29, flags, 3);
  assert.equal(inputs.length, inputCount, "retained phases have no host CPU/RAM input"); assert.equal(hosts.length, hostCount, "no interphase host diagnostics");
  check(ctx, "before close"); const status = ctx.api.close(); assert.equal(status, 0); hosts.push({context, name: "close", args: [], status});
}
const raw = Buffer.concat(frames); writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
assert.equal(frames.length, 72); assert.equal(phases.length, 36); assert.equal(inputs.length, 20); assert.equal(hosts.length, 24);
assert.equal(phases.reduce((sum, phase) => sum + phase.retired, 0), 36); assert.deepEqual(sourcePins(), beforeSources);
const result = {status: "ok", engine: {file: "engine.wasm", bytes: engine.bytes.length, sha256: engine.sha256}, modules, initial_frames: initialFrames, inputs, hosts, phases,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)}, source_sha256: beforeSources,
  counts: {owners: 2, descriptor_modes: 2, contexts: 4, modules: 4, word_immediates: 8, word_transfers: 8, extensions: 8, setcc: 8, jumps: 4, generated_calls: 36, retired: 36, raw_frames: 72, arena_inputs: 16, uploads: 4, host_rows: 24, physical_files: 8},
  claim: "retained word immediate AX->CX->MOVSX and ordinary SP->BX->MOVZX plus unchanged FLAGS->SETB/SETO->cold; nine single-instruction phases/context; four replacement/resident explicit/entry contexts; every generated before/after image is complete4364 including preserved opaqueFP/helper/transfer; one initial CPU/FP seed per context, no interphase host CPU/RAM repair or diagnostics; pure imports and ExitV1 only; host rows are ordered records/live full-arena assertions, not saved full host journals, guest RAM dump or allocator proof; close has preclose/rawstatus only"};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"}); console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
