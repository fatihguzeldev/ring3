import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[0-9a-f]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, FP = 4236, PC = 0x1000, DATA = 0x5008, STACK = 0x8008;
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const program = Buffer.from("0fab0b9c580f92c20fbb0b9c5f0f92c20fba33209c5e0f92c2eb00", "hex");
assert.equal(program.length, 27);
assert.deepEqual(program, readFileSync(join(output, "parallel.x86")), "independent Rust/JS literal program");
const hash = bytes => createHash("sha256").update(bytes).digest("hex"), frames = [], phases = [], modules = [], ram = [], inputs = [];
const sourcePaths = ["engine/tests/cpu_parallel_bit_flags_wasm.rs", "engine/tests/fixtures/p2-cpu-parallel-bit-flags/run.mjs", "engine/tests/fixtures/support/engine.mjs"];
const sourcePins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), beforeSources = sourcePins();
function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4)); return bytes;
}
const state = (registers, pc, flags) => record("R3ST", 56, [...registers, pc, flags]);
const exit = (reason, retired, version = 2) => record("R3EX", 40, [reason, retired, 0, 0, 0, 0], version);
const helper = value => record("R3MH", 40, [0, value, 0, 0, 0, 0]);
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((v, i) => bytes.writeUInt32LE(v >>> 0, i * 4)); return bytes;};
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
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${label}`);}
function input(ctx, offset, bytes) {new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + offset); ctx.expected.set(bytes, offset); check(ctx, "declared host input");}
function host(ctx, name, ...args) {check(ctx, `before ${name}`); assert.equal(ctx.api[name](...args), 0, name); check(ctx, name);}
function upload(ctx, address, bytes) {
  input(ctx, 140, bytes); host(ctx, "upload", address, bytes.length);
  inputs.push({owner: ctx.owner, type: "upload", address, hex: Buffer.from(bytes).toString("hex"), before_frame: frames.length});
}
function phase(ctx, label, budget, registers, pc, flags, reason, retired, readValue) {
  check(ctx, "before retained phase"); const beforeFrame = frames.length; frames.push(arena(ctx));
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);
  if (readValue !== undefined) ctx.expected.set(helper(readValue), 100);
  assert.equal(ctx.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0); check(ctx, label);
  frames.push(arena(ctx)); phases.push({owner: ctx.owner, label, budget, pc, flags, reason, retired, before_frame: beforeFrame, after_frame: beforeFrame + 1});
}
function diagnostic(ctx, address, bytes, label) {
  for (const [i, byte] of bytes.entries()) {
    check(ctx, "before live RAM diagnostic"); assert.equal(ctx.api.read8(address + i), 0);
    ctx.expected.set(record("R3MH", 40, [0, byte, 0, 0, 0, 1], 2), 100); check(ctx, label);
    ram.push({owner: ctx.owner, label, address: address + i, byte, before_frame: frames.length});
  }
}
for (const [index, owner] of ["replacement", "resident"].entries()) {
  const instance = new WebAssembly.Instance(engine.module, {}), ctx = {owner, memory: instance.exports.memory, api: {}};
  for (const name of ["open", "close", "arena_ptr", "map", "upload", "compile", "compile_resident", "generation", "module_ptr", "module_len", "guard", "guard_resident", "read8", "read32", "store32", "store_resident32"])
    ctx.api[name] = instance.exports["ring3_abi_v1_" + name];
  assert.equal(ctx.api.open(3, index + 1, 0x42464c47), 0); ctx.base = ctx.api.arena_ptr() >>> 0;
  ctx.expected = Buffer.alloc(SIZE); ctx.expected.set(state(Array(8).fill(0), 0, 2));
  ctx.expected.set(exit(1, 0, 1), 56); ctx.expected.set(helper(0), 100); ctx.expected.set(fpu(), FP); check(ctx, "initialized full arena");
  for (const address of [PC, DATA & ~0xfff, STACK & ~0xfff]) host(ctx, "map", address, 1, address === PC ? 7 : 3);
  upload(ctx, PC, program); upload(ctx, DATA - 2, Buffer.from([0xb1, 0xb2, 5, 0, 0, 0, 0xc3, 0xc4]));
  upload(ctx, STACK - 2, Buffer.from([0xb1, 0xb2, 0x71, 0x82, 0x93, 0xa4, 0xc3, 0xc4]));
  input(ctx, 140, words([PC, program.length])); let binding;
  if (owner === "replacement") {
    host(ctx, "compile", 1); binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
  } else {
    check(ctx, "before resident compile"); assert.equal(ctx.api.compile_resident(1), 0); const receipt = arena(ctx);
    assert.equal(receipt.readUInt32LE(140), 1); assert.equal(receipt.readUInt32LE(144), 24);
    binding = Object.fromEntries(["low", "high", "pointer", "length"].map((name, i) => [name, receipt.readUInt32LE(148 + 4 * i)]));
    ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), 140); check(ctx, "resident receipt only");
  }
  const bytes = Buffer.from(new Uint8Array(ctx.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const guard = owner === "replacement" ? "guard" : "guard_resident", store = owner === "replacement" ? "store32" : "store_resident32";
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: "env", name: "memory", kind: "memory"}, ...[guard, "read32", store].map(name => ({module: "ring3", name, kind: "function"}))]);
  ctx.run = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}).exports.run;
  const file = `${owner}.wasm`; writeFileSync(join(output, file), bytes, {flag: "wx"}); modules.push({owner, file, ...binding, bytes: bytes.length, sha256: hash(bytes)});
  const registers = [0xa53c0011, 31, 0xa53cabcd, DATA, STACK + 4, 0x56789abc, 0xe9876543, 0xf1234567];
  input(ctx, 0, state(registers, PC, 0xcd7)); input(ctx, FP, fpu(true));
  phase(ctx, "BTS old-bit0", 1, registers, PC + 3, 0x442, 1, 1, 0);
  diagnostic(ctx, DATA - 2, [0xb1, 0xb2, 5, 0, 0, 0x80, 0xc3, 0xc4], "BTS selected DWORD");
  registers[4] = STACK; phase(ctx, "PUSHFD currentCF0", 1, registers, PC + 4, 0x442, 1, 1, 0);
  diagnostic(ctx, STACK - 2, [0xb1, 0xb2, 0x42, 4, 0, 0, 0xc3, 0xc4], "PUSHFD literal442");
  registers[0] = 0x442; registers[4] = STACK + 4; registers[2] = 0xa53cab00;
  phase(ctx, "POP EAX and SETB currentCF0", 2, registers, PC + 8, 0x442, 1, 2, 0x442);
  phase(ctx, "BTC old-bit1", 1, registers, PC + 11, 0x443, 1, 1, 0);
  diagnostic(ctx, DATA - 2, [0xb1, 0xb2, 5, 0, 0, 0, 0xc3, 0xc4], "BTC selected DWORD");
  registers[4] = STACK; phase(ctx, "PUSHFD currentCF1", 1, registers, PC + 12, 0x443, 1, 1, 0);
  diagnostic(ctx, STACK - 2, [0xb1, 0xb2, 0x43, 4, 0, 0, 0xc3, 0xc4], "PUSHFD literal443");
  registers[7] = 0x443; registers[4] = STACK + 4; registers[2] = 0xa53cab01;
  phase(ctx, "POP EDI and SETB currentCF1", 2, registers, PC + 16, 0x443, 1, 2, 0x443);
  phase(ctx, "BTR rawimm32 selects bit0", 1, registers, PC + 20, 0x443, 1, 1, 0);
  diagnostic(ctx, DATA - 2, [0xb1, 0xb2, 4, 0, 0, 0, 0xc3, 0xc4], "BTR immediate high bits ignored");
  registers[4] = STACK; phase(ctx, "PUSHFD after current BTR", 1, registers, PC + 21, 0x443, 1, 1, 0);
  diagnostic(ctx, STACK - 2, [0xb1, 0xb2, 0x43, 4, 0, 0, 0xc3, 0xc4], "PUSHFD current BTR flags");
  registers[6] = 0x443; registers[4] = STACK + 4;
  phase(ctx, "POP ESI and SETB currentCF1", 2, registers, PC + 25, 0x443, 1, 2, 0x443);
  phase(ctx, "cold target", 2, registers, PC + 27, 0x443, 3, 1);
  check(ctx, "before close"); assert.equal(ctx.api.close(), 0);
}
const raw = Buffer.concat(frames); writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
writeFileSync(join(output, "engine.wasm"), engine.bytes, {flag: "wx"});
assert.equal(frames.length, 40); assert.equal(phases.length, 20); assert.equal(ram.length, 96); assert.equal(inputs.length, 6);
assert.equal(phases.reduce((sum, p) => sum + p.retired, 0), 26); assert.deepEqual(sourcePins(), beforeSources);
const result = {status: "ok", engine: {file: "engine.wasm", bytes: engine.bytes.length, sha256: engine.sha256}, modules, phases, ram, inputs,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)}, source_sha256: beforeSources,
  counts: {owners: 2, modules: 2, generated_calls: 20, retired: 26, bts: 2, btc: 2, btr: 2, pushfd: 6, pop: 6, setb: 6, jumps: 2, raw_frames: 40, live_ram_bytes: 96, physical_files: 6},
  claim: "same CPU BTS/BTC/BTR→PUSHFD→POP three distinct full registers→SETB old/currentCF0 and1 witnesses, immediate high bits ignored; ten retained phases/owner without CPU reseeding; all40 physical full4364 arenas and opaqueFP unchanged; 96 selected target/stack/sentinel Read8 diagnostics remain live RAM evidence, not saved RAM pages or performance proof"};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"}); console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
