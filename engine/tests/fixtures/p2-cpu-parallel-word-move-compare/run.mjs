import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[0-9a-f]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, FP = 4236, PC = 0x1000, TABLE = 0x4008;
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const program = Buffer.from("66a566a70f90c166af0f92c2b800803ca566af0f94c3eb00", "hex");
assert.equal(program.length, 24);
assert.deepEqual(program, readFileSync(join(output, "word-move-compare.x86")), "independent Rust/JS literal bank");
writeFileSync(join(output, "engine.wasm"), engine.bytes, {flag: "wx"});
const hash = bytes => createHash("sha256").update(bytes).digest("hex"), frames = [], phases = [], modules = [], ram = [], inputs = [], hosts = [], initialFrames = [];
const paths = ["engine/tests/cpu_parallel_word_move_compare_wasm.rs", "engine/tests/fixtures/p2-cpu-parallel-word-move-compare/run.mjs", "engine/tests/fixtures/support/engine.mjs"];
const sourcePins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))])), beforeSources = sourcePins();
function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4)); return bytes;
}
const state = (registers, pc, flags) => record("R3ST", 56, [...registers, pc, flags]);
const exit = (reason, retired, version = 2) => record("R3EX", 40, [reason, retired, 0, 0, 0, 0], version);
const words32 = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((v, i) => bytes.writeUInt32LE(v >>> 0, i * 4)); return bytes;};
const narrow = (value, width = 2) => record("R3MH", 40, [0, value, 0, 0, 0, width], 2);
const wordStore = () => record("R3MH", 40, [0, 0, 0, 0, 0, 2], 4);
const freshHelper = () => record("R3MH", 40, [0, 0, 0, 0, 0, 0]);
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
function host(ctx, name, ...args) {
  check(ctx, `before ${name}`); const status = ctx.api[name](...args); assert.equal(status, 0, name); check(ctx, name);
  hosts.push({context: ctx.context, name, args, status});
}
function upload(ctx, address, bytes, label) {
  input(ctx, 140, bytes, label); host(ctx, "upload", address, bytes.length);
  inputs.push({context: ctx.context, kind: "upload", address, hex: Buffer.from(bytes).toString("hex"), label});
}
function phase(ctx, label, budget, registers, pc, flags, reason, retired, helper) {
  check(ctx, `before ${label}`); const beforeFrame = frames.length; frames.push(arena(ctx));
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);
  if (helper !== undefined) ctx.expected.set(helper, 100);
  const args = [ctx.base, ctx.base + 56, budget, ctx.base + 96], status = ctx.run(...args); assert.equal(status, 0); check(ctx, `after ${label}`);
  frames.push(arena(ctx)); phases.push({context: ctx.context, owner: ctx.owner, direction: ctx.direction, label, args, status, budget, before_frame: beforeFrame, after_frame: beforeFrame + 1, retired, reason});
}
for (const owner of ["replacement", "resident"]) for (const direction of ["forward", "reverse"]) {
  const reverse = direction === "reverse", step = reverse ? -2 : 2, context = modules.length;
  const pointer = TABLE + (reverse ? 6 : 2), source = pointer - step;
  const instance = new WebAssembly.Instance(engine.module, {}), ctx = {owner, direction, context, memory: instance.exports.memory, api: {}, started: false};
  for (const name of ["open", "close", "arena_ptr", "map", "protect", "upload", "compile", "compile_resident", "generation", "module_ptr", "module_len", "guard", "guard_resident", "read8", "read16", "store16", "store_resident16"])
    ctx.api[name] = instance.exports["ring3_abi_v1_" + name];
  assert.equal(ctx.api.open(2, context + 1, 0x574d4353), 0); ctx.base = ctx.api.arena_ptr() >>> 0;
  ctx.expected = Buffer.alloc(SIZE); ctx.expected.set(state(Array(8).fill(0), 0, 2));
  ctx.expected.set(exit(1, 0, 1), 56); ctx.expected.set(freshHelper(), 100); ctx.expected.set(fpu(), FP); check(ctx, "initialized full arena");
  for (const address of [PC, TABLE & ~0xfff]) host(ctx, "map", address, 1, address === PC ? 7 : 3);
  upload(ctx, PC, program, "literal guest bank"); host(ctx, "protect", PC, 1, 4);
  upload(ctx, TABLE - 1, Buffer.from(reverse ? [0x63, 0x00, 0x80, 0x00, 0x80, 0x01, 0x00, 0xaa, 0x55, 0x00, 0x80, 0xbc] : [0x63, 0x00, 0x80, 0xaa, 0x55, 0x01, 0x00, 0x00, 0x80, 0x00, 0x80, 0xbc]), "five current words and canaries");
  input(ctx, 140, words32([PC, program.length]), "one explicit region descriptor"); let binding;
  if (owner === "replacement") {
    host(ctx, "compile", 1); binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
  } else {
    check(ctx, "before resident compile"); const status = ctx.api.compile_resident(1); assert.equal(status, 0);
    hosts.push({context, name: "compile_resident", args: [1], status}); const receipt = arena(ctx);
    assert.equal(receipt.readUInt32LE(140), 1); assert.equal(receipt.readUInt32LE(144), 24);
    binding = Object.fromEntries(["low", "high", "pointer", "length"].map((name, i) => [name, receipt.readUInt32LE(148 + 4 * i)]));
    ctx.expected.set(words32([1, 24, binding.low, binding.high, binding.pointer, binding.length]), 140); check(ctx, "resident receipt only");
  }
  const bytes = Buffer.from(new Uint8Array(ctx.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const guard = owner === "replacement" ? "guard" : "guard_resident", store16 = owner === "replacement" ? "store16" : "store_resident16";
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: "env", name: "memory", kind: "memory"}, ...[guard, "read16", store16].map(name => ({module: "ring3", name, kind: "function"}))]);
  ctx.run = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}).exports.run;
  const file = `${owner}-${direction}.wasm`; writeFileSync(join(output, file), bytes, {flag: "wx"}); modules.push({context, owner, direction, arena_base: ctx.base, key_low: context + 1, key_high: 0x574d4353, file, ...binding, bytes: bytes.length, sha256: hash(bytes)});
  const registers = [0xa53c7fff, 0x91b2c378, 0x34560f10, 0x82345678, 0x4567ff00, 0x56789abc, source, pointer], df = reverse ? 0x400 : 0;
  input(ctx, 0, state(registers, PC, 0x8d7 | df), "single initial CPU seed"); input(ctx, FP, fpu(true), "opaque FP128 seed");
  initialFrames.push({context, frame: frames.length}); ctx.started = true; const inputCount = inputs.length, hostCount = hosts.length;
  registers[6] += step; registers[7] += step; phase(ctx, "MOVSW current RAM transfer", 1, registers, PC + 2, 0x8d7 | df, 1, 1, wordStore());
  registers[6] += step; registers[7] += step; phase(ctx, "CMPSW consumes MOVSW-written 8000 minus 0001", 1, registers, PC + 4, 0x816 | df, 1, 1, narrow(1));
  registers[1] = 0x91b2c301; phase(ctx, "SETO CL consumes word overflow", 1, registers, PC + 7, 0x816 | df, 1, 1);
  registers[7] += step; phase(ctx, "SCASW 7fff minus 8000", 1, registers, PC + 9, 0x887 | df, 1, 1, narrow(0x8000));
  registers[2] = 0x34560f01; phase(ctx, "SETB DL consumes word borrow", 1, registers, PC + 12, 0x887 | df, 1, 1);
  registers[0] = 0xa53c8000; phase(ctx, "guest MOV EAX changes current AX", 1, registers, PC + 17, 0x887 | df, 1, 1);
  registers[7] += step; phase(ctx, "SCASW current AX equality", 1, registers, PC + 19, 0x46 | df, 1, 1, narrow(0x8000));
  registers[3] = 0x82345601; phase(ctx, "SETZ BL consumes word equality", 1, registers, PC + 22, 0x46 | df, 1, 1);
  phase(ctx, "cold JMP", 2, registers, PC + 24, 0x46 | df, 3, 1);
  assert.equal(inputs.length, inputCount, "retained phases have no host CPU/RAM input"); assert.equal(hosts.length, hostCount, "no interphase host diagnostics");
  const expected = Buffer.from([0x63, 0x00, 0x80, 0x00, 0x80, 0x01, 0x00, 0x00, 0x80, 0x00, 0x80, 0xbc]);
  for (let i = 0; i < expected.length; i++) {
    const address = TABLE - 1 + i; check(ctx, "before end-only live RAM diagnostic"); const status = ctx.api.read8(address); assert.equal(status, 0);
    ctx.expected.set(narrow(expected[i], 1), 100); check(ctx, "literal RAM/canary diagnostic");
    const helper = arena(ctx).subarray(100, 140); ram.push({context, address, byte: helper.readUInt32LE(20), helper_hex: helper.toString("hex")});
    hosts.push({context, name: "read8", args: [address], status});
  }
  check(ctx, "before close"); const status = ctx.api.close(); assert.equal(status, 0); hosts.push({context, name: "close", args: [], status});
}
const raw = Buffer.concat(frames); writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
assert.equal(frames.length, 72); assert.equal(phases.length, 36); assert.equal(ram.length, 48); assert.equal(inputs.length, 28); assert.equal(hosts.length, 76);
assert.equal(phases.reduce((sum, p) => sum + p.retired, 0), 36); assert.deepEqual(sourcePins(), beforeSources);
const result = {status: "ok", engine: {file: "engine.wasm", bytes: engine.bytes.length, sha256: engine.sha256}, modules, initial_frames: initialFrames, inputs, hosts, phases, ram,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)}, source_sha256: beforeSources,
  counts: {owners: 2, directions: 2, contexts: 4, modules: 4, movsw: 4, cmpsw: 4, scasw: 8, setcc: 12, mov_eax: 4, jumps: 4, generated_calls: 36, retired: 36, raw_frames: 72, arena_inputs: 20, uploads: 8, host_rows: 76, live_ram_bytes: 48, physical_files: 8},
  claim: "retained MOVSW-written word->CMPSW/SETO->SCASW/SETB->guest currentAX MOV->SCASW/SETZ->cold; nine single-instruction phases/context; no interphase host CPU/RAM reseed/diagnostic; four explicit owner/direction contexts; every generated before/after image is complete4364 with opaqueFP preserved; end-only RAM/canaries are observed live Read8 helper diagnostics and ordered host records, not a physical RAM dump or full host-arena journal"};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"}); console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
