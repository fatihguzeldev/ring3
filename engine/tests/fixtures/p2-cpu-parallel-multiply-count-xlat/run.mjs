import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[0-9a-f]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, FP = 4236, PC = 0x1000, TABLE = 0x5000, SOURCE = 0x5100, DESTINATION = 0x6008;
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const program = Buffer.from("b903000000d7f626e2fbe3020f0baee1020f0beb00", "hex");
assert.deepEqual(program, readFileSync(join(output, "parallel.x86")), "independent Rust/JS program bytes");
const hash = bytes => createHash("sha256").update(bytes).digest("hex"), frames = [], phases = [], modules = [];
function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4)); return bytes;
}
const state = (registers, pc, flags) => record("R3ST", 56, [...registers, pc, flags]);
const exit = (reason, retired, version = 2) => record("R3EX", 40, [reason, retired, 0, 0, 0, 0], version);
const helper = (version, value, width) => record("R3MH", 40, [0, value, 0, 0, 0, width], version);
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
function upload(ctx, address, bytes) {input(ctx, 140, bytes); host(ctx, "upload", address, bytes.length);}
function phase(ctx, budget, registers, pc, flags, reason, retired, readValue) {
  check(ctx, "before mixed phase"); const beforeFrame = frames.length; frames.push(arena(ctx));
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);
  if (readValue !== undefined) ctx.expected.set(helper(2, readValue, 1), 100);
  assert.equal(ctx.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0); check(ctx, "after mixed phase");
  frames.push(arena(ctx)); phases.push({owner: ctx.owner, budget, pc, reason, retired, before_frame: beforeFrame, after_frame: beforeFrame + 1});
}
const table = Buffer.alloc(256, 0x5a); table[0] = 0x7f; table[1] = 0x80; table[254] = 0xff;
for (const [index, owner] of ["replacement", "resident"].entries()) {
  const instance = new WebAssembly.Instance(engine.module, {}), ctx = {owner, memory: instance.exports.memory, api: {}};
  for (const name of ["open", "close", "arena_ptr", "map", "upload", "compile", "compile_resident", "generation", "module_ptr", "module_len", "guard", "guard_resident", "read8"])
    ctx.api[name] = instance.exports["ring3_abi_v1_" + name];
  assert.equal(ctx.api.open(3, index + 1, 0x584c4154), 0); ctx.base = ctx.api.arena_ptr() >>> 0;
  ctx.expected = Buffer.alloc(SIZE); ctx.expected.set(state(Array(8).fill(0), 0, 2));
  ctx.expected.set(exit(1, 0, 1), 56); ctx.expected.set(helper(1, 0, 0), 100); ctx.expected.set(fpu(), FP); check(ctx, "initialized full arena");
  for (const address of [PC, TABLE, DESTINATION & ~0xfff]) host(ctx, "map", address, 1, address === PC ? 7 : 3);
  upload(ctx, PC, program); upload(ctx, TABLE, table); upload(ctx, SOURCE, Buffer.from([2])); upload(ctx, DESTINATION, Buffer.from([0xfe]));
  input(ctx, 140, words([PC, 10, PC + 10, 2, PC + 14, 3, PC + 19, 2])); let binding;
  if (owner === "replacement") {
    host(ctx, "compile", 4); binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
  } else {
    check(ctx, "before resident compile"); assert.equal(ctx.api.compile_resident(4), 0); const receipt = arena(ctx);
    assert.equal(receipt.readUInt32LE(140), 1); assert.equal(receipt.readUInt32LE(144), 24);
    binding = Object.fromEntries(["low", "high", "pointer", "length"].map((name, i) => [name, receipt.readUInt32LE(148 + 4 * i)]));
    ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), 140); check(ctx, "resident receipt only");
  }
  const bytes = Buffer.from(new Uint8Array(ctx.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const guard = owner === "replacement" ? "guard" : "guard_resident";
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: "env", name: "memory", kind: "memory"}, ...[guard, "read8"].map(name => ({module: "ring3", name, kind: "function"}))]);
  ctx.run = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}).exports.run;
  writeFileSync(join(output, `${owner}.wasm`), bytes, {flag: "wx"}); modules.push({owner, ...binding, bytes: bytes.length, sha256: hash(bytes)});
  const registers = [0xa53cab01, 0xdeadbeef, 0x23457f80, TABLE, 0x4567ff00, 0x56789abc, SOURCE, DESTINATION];
  input(ctx, 0, state(registers, PC, 2)); input(ctx, FP, fpu(true));
  registers[1] = 3; phase(ctx, 1, registers, PC + 5, 2, 1, 1);
  registers[0] = 0xa53c0100; phase(ctx, 2, registers, PC + 8, 0x803, 1, 2, 2);
  registers[1] = 2; phase(ctx, 1, registers, PC + 5, 0x803, 1, 1);
  registers[0] = 0xa53c00fe; registers[1] = 1; phase(ctx, 3, registers, PC + 5, 2, 1, 3, 2);
  registers[0] = 0xa53c01fe; registers[1] = 0; phase(ctx, 3, registers, PC + 10, 0x803, 1, 3, 2);
  phase(ctx, 1, registers, PC + 14, 0x803, 1, 1);
  registers[7]++; phase(ctx, 1, registers, PC + 15, 0x46, 1, 1, 0xfe);
  registers[1] = 0xffffffff; phase(ctx, 1, registers, PC + 19, 0x46, 1, 1);
  phase(ctx, 2, registers, PC + 21, 0x46, 3, 1);
  host(ctx, "close");
}
const raw = Buffer.concat(frames); writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
assert.equal(frames.length, 36); assert.equal(phases.reduce((sum, p) => sum + p.retired, 0), 28);
const result = {status: "ok", engine: {bytes: engine.bytes.length, sha256: engine.sha256}, modules, phases,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)},
  source_sha256: Object.fromEntries(["engine/tests/cpu_parallel_multiply_count_xlat_wasm.rs", "engine/tests/fixtures/p2-cpu-parallel-multiply-count-xlat/run.mjs", "engine/tests/fixtures/support/engine.mjs"].map(path => [path, hash(readFileSync(join(root, path)))])),
  counts: {owners: 2, modules: 2, generated_calls: 18, retired: 28, xlatb: 6, byte_products: 6, loop: 6, jecxz: 2, scasb: 2, loope: 2, jumps: 2, moves: 2},
  claim: "current AL XLATB→memory MUL→LOOP interior resume, JECXZ over UD2, SCASB→LOOPE preserved ZF/countwrap and cold JMP; all36 full4364 saved arenas/retained CPU/full opaqueFP, finite two-owner composition, no RAM dump or performance claim"};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"}); console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
