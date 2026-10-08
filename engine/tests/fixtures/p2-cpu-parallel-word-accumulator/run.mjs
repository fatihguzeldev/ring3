import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[0-9a-f]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, FP = 4236, PC = 0x1000, SOURCE = 0x3008, DESTINATION = 0x4008, WITNESS = 0x5008;
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const program = Buffer.from("66ad66ab66ad66ab8903eb00", "hex");
assert.equal(program.length, 12);
assert.deepEqual(program, readFileSync(join(output, "word-accumulator.x86")), "independent Rust/JS literal bank");
writeFileSync(join(output, "engine.wasm"), engine.bytes, {flag: "wx"});
const hash = bytes => createHash("sha256").update(bytes).digest("hex"), frames = [], phases = [], modules = [], ram = [], inputs = [], initialFrames = [];
const paths = ["engine/tests/cpu_parallel_word_accumulator_wasm.rs", "engine/tests/fixtures/p2-cpu-parallel-word-accumulator/run.mjs", "engine/tests/fixtures/support/engine.mjs"];
const sourcePins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))])), beforeSources = sourcePins();
function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4)); return bytes;
}
const state = (registers, pc, flags) => record("R3ST", 56, [...registers, pc, flags]);
const exit = (reason, retired, version = 2) => record("R3EX", 40, [reason, retired, 0, 0, 0, 0], version);
const words32 = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((v, i) => bytes.writeUInt32LE(v >>> 0, i * 4)); return bytes;};
const narrow = value => record("R3MH", 40, [0, value, 0, 0, 0, 2], 2);
const wordStore = () => record("R3MH", 40, [0, 0, 0, 0, 0, 2], 4);
const dwordStore = () => record("R3MH", 40, [0, 0, 0, 0, 0, 0]);
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
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.direction}/${label}`);}
function input(ctx, offset, bytes, label) {
  assert.equal(ctx.started, false, "no interphase host CPU/RAM input");
  new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + offset); ctx.expected.set(bytes, offset); check(ctx, label);
  inputs.push({context: ctx.context, kind: "arena", offset, hex: Buffer.from(bytes).toString("hex"), label});
}
function host(ctx, name, ...args) {check(ctx, `before ${name}`); assert.equal(ctx.api[name](...args), 0, name); check(ctx, name);}
function upload(ctx, address, bytes, label) {
  input(ctx, 140, bytes, label); host(ctx, "upload", address, bytes.length);
  inputs.push({context: ctx.context, kind: "upload", address, hex: Buffer.from(bytes).toString("hex"), label});
}
function phase(ctx, label, budget, registers, pc, flags, reason, retired, helper) {
  check(ctx, `before ${label}`); const beforeFrame = frames.length; frames.push(arena(ctx));
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);
  if (helper !== undefined) ctx.expected.set(helper, 100);
  assert.equal(ctx.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0); check(ctx, `after ${label}`);
  frames.push(arena(ctx)); phases.push({context: ctx.context, owner: ctx.owner, direction: ctx.direction, label, budget, before_frame: beforeFrame, after_frame: beforeFrame + 1, retired, reason});
}
for (const owner of ["replacement", "resident"]) for (const direction of ["forward", "reverse"]) {
  const reverse = direction === "reverse", step = reverse ? -2 : 2, context = modules.length;
  const pointer = DESTINATION + (reverse ? 2 : 0), source = SOURCE + (reverse ? 2 : 0);
  const instance = new WebAssembly.Instance(engine.module, {}), ctx = {owner, direction, context, memory: instance.exports.memory, api: {}, started: false};
  for (const name of ["open", "close", "arena_ptr", "map", "protect", "upload", "compile", "compile_resident", "generation", "module_ptr", "module_len", "guard", "guard_resident", "read8", "read16", "store16", "store_resident16", "store32", "store_resident32"])
    ctx.api[name] = instance.exports["ring3_abi_v1_" + name];
  assert.equal(ctx.api.open(4, context + 1, 0x574f5244), 0); ctx.base = ctx.api.arena_ptr() >>> 0;
  ctx.expected = Buffer.alloc(SIZE); ctx.expected.set(state(Array(8).fill(0), 0, 2));
  ctx.expected.set(exit(1, 0, 1), 56); ctx.expected.set(dwordStore(), 100); ctx.expected.set(fpu(), FP); check(ctx, "initialized full arena");
  for (const address of [PC, SOURCE & ~0xfff, DESTINATION & ~0xfff, WITNESS & ~0xfff]) host(ctx, "map", address, 1, address === PC ? 7 : 3);
  upload(ctx, PC, program, "literal guest bank"); host(ctx, "protect", PC, 1, 4);
  upload(ctx, SOURCE, Buffer.from(reverse ? [0x01, 0x80, 0xff, 0x00] : [0xff, 0x00, 0x01, 0x80]), "distinct source words");
  upload(ctx, DESTINATION - 1, Buffer.from([0x31, 0xde, 0xad, 0xbe, 0xef, 0xd7]), "word destination canaries");
  upload(ctx, WITNESS - 1, Buffer.from([0x63, 0x10, 0x20, 0x30, 0x40, 0xbc]), "full EAX witness canaries");
  input(ctx, 140, words32([PC, program.length]), "one explicit region descriptor"); let binding;
  if (owner === "replacement") {
    host(ctx, "compile", 1); binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
  } else {
    check(ctx, "before resident compile"); assert.equal(ctx.api.compile_resident(1), 0); const receipt = arena(ctx);
    assert.equal(receipt.readUInt32LE(140), 1); assert.equal(receipt.readUInt32LE(144), 24);
    binding = Object.fromEntries(["low", "high", "pointer", "length"].map((name, i) => [name, receipt.readUInt32LE(148 + 4 * i)]));
    ctx.expected.set(words32([1, 24, binding.low, binding.high, binding.pointer, binding.length]), 140); check(ctx, "resident receipt only");
  }
  const bytes = Buffer.from(new Uint8Array(ctx.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const guard = owner === "replacement" ? "guard" : "guard_resident", store32 = owner === "replacement" ? "store32" : "store_resident32", store16 = owner === "replacement" ? "store16" : "store_resident16";
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: "env", name: "memory", kind: "memory"}, ...[guard, store32, "read16", store16].map(name => ({module: "ring3", name, kind: "function"}))]);
  ctx.run = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}).exports.run;
  const file = `${owner}-${direction}.wasm`; writeFileSync(join(output, file), bytes, {flag: "wx"}); modules.push({context, owner, direction, file, ...binding, bytes: bytes.length, sha256: hash(bytes)});
  const registers = [0xa53c5678, 0x91b2c378, 0x34560f10, WITNESS, 0x4567ff00, 0x56789abc, source, pointer], flags = reverse ? 0xcd7 : 0x8d7;
  input(ctx, 0, state(registers, PC, flags), "single initial CPU seed"); input(ctx, FP, fpu(true), "opaque FP128 seed");
  initialFrames.push({context, frame: frames.length}); ctx.started = true; const inputCount = inputs.length;
  registers[0] = 0xa53c00ff; registers[6] += step; phase(ctx, "first LODSW", 1, registers, PC + 2, flags, 1, 1, narrow(0x00ff));
  registers[7] += step; phase(ctx, "first STOSW consumes loaded AX", 1, registers, PC + 4, flags, 1, 1, wordStore());
  registers[0] = 0xa53c8001; registers[6] += step; phase(ctx, "second current LODSW", 1, registers, PC + 6, flags, 1, 1, narrow(0x8001));
  registers[7] += step; phase(ctx, "second STOSW consumes current AX", 1, registers, PC + 8, flags, 1, 1, wordStore());
  phase(ctx, "DWORD witness consumes full preserved EAX", 1, registers, PC + 10, flags, 1, 1, dwordStore());
  phase(ctx, "cold JMP", 2, registers, PC + 12, flags, 3, 1);
  assert.equal(inputs.length, inputCount, "retained phases have no host CPU/RAM input");
  const destination = Buffer.from(reverse ? [0x31, 0x01, 0x80, 0xff, 0x00, 0xd7] : [0x31, 0xff, 0x00, 0x01, 0x80, 0xd7]);
  const witness = Buffer.from([0x63, 0x01, 0x80, 0x3c, 0xa5, 0xbc]);
  for (const [address, expected] of [[DESTINATION - 1, destination], [WITNESS - 1, witness]]) for (let i = 0; i < expected.length; i++) {
    check(ctx, "before end-only live RAM diagnostic"); assert.equal(ctx.api.read8(address + i), 0);
    ctx.expected.set(record("R3MH", 40, [0, expected[i], 0, 0, 0, 1], 2), 100); check(ctx, "literal RAM/canary diagnostic");
    ram.push({context, address: address + i, byte: expected[i]});
  }
  check(ctx, "before close"); assert.equal(ctx.api.close(), 0);
}
const raw = Buffer.concat(frames); writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
assert.equal(frames.length, 48); assert.equal(phases.length, 24); assert.equal(ram.length, 48); assert.equal(phases.reduce((sum, p) => sum + p.retired, 0), 24);
assert.deepEqual(sourcePins(), beforeSources);
const result = {status: "ok", engine: {file: "engine.wasm", bytes: engine.bytes.length, sha256: engine.sha256}, modules, initial_frames: initialFrames, inputs, phases, ram,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)}, source_sha256: beforeSources,
  counts: {owners: 2, directions: 2, contexts: 4, modules: 4, lodsw: 8, stosw: 8, full_eax_stores: 4, jumps: 4, generated_calls: 24, retired: 24, raw_frames: 48, live_ram_bytes: 48, physical_files: 8},
  claim: "two retained LODSW->STOSW pairs/context followed by full EAX DWORD witness/cold; no interphase host CPU/RAM reseed; four explicit owner/direction contexts; first before frames are complete initial4364 images and all48 generated frames preserve opaqueFP; end-only RAM/canary bytes are live Read8 diagnostics, not physical RAM pages or whole host journal"};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"}); console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
