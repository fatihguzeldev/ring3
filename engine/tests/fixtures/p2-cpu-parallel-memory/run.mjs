import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[0-9a-f]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, FP = 4236, PC = 0x1000, DIVISOR = 0x5000, SRC = 0x5008, DST = 0x6008;
const program = Buffer.from("dbe2dfe0ba00000000f731aaacfdaafcdfe0eb00", "hex");
assert.deepEqual(program, readFileSync(join(output, "parallel.x86")), "independent Rust/JS program bytes");
assert.equal(0x7f00n / 5n, 6502n); assert.equal(0x7f00n % 5n, 2n);
const hash = bytes => createHash("sha256").update(bytes).digest("hex"), frames = [], phases = [], modules = [];
const raw80 = Buffer.from(Array.from({length: 80}, (_, i) => (i * 73 + 29) & 255));
function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4)); return bytes;
}
const state = (registers, pc, flags) => record("R3ST", 56, [...registers, pc, flags]);
const exit = (reason, retired, version = 5) => record("R3EX", 40, [reason, retired, 0, 0, 0, 0], version);
const helper = (version, value, width) => record("R3MH", 40, [0, value, 0, 0, 0, width], version);
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((v, i) => bytes.writeUInt32LE(v >>> 0, i * 4)); return bytes;};
function fpu(reset, status = 0xffff, payload = raw80) {
  const bytes = record("R3FP", 128);
  bytes.writeUInt16LE(reset ? 0x37f : 0, 16); bytes.writeUInt16LE(reset ? 0 : status, 18);
  bytes.writeUInt16LE(reset ? 0xffff : 0x55aa, 20); bytes.writeUInt16LE(reset ? 0 : 0x7ff, 22);
  bytes.writeUInt32LE(reset ? 0 : 0xf1234567, 24); bytes.writeUInt32LE(reset ? 0 : 0xfedcba98, 28);
  bytes.writeUInt16LE(reset ? 0 : 0xf135, 32); bytes.writeUInt16LE(reset ? 0 : 0xe246, 34);
  bytes.set(payload, 40); return bytes;
}
function arena(ctx) {return Buffer.from(new Uint8Array(ctx.memory.buffer, ctx.base, SIZE));}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${label}`);}
function input(ctx, offset, bytes) {new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + offset); ctx.expected.set(bytes, offset); check(ctx, "declared host input");}
function host(ctx, name, ...args) {check(ctx, `before ${name}`); assert.equal(ctx.api[name](...args), 0, name); check(ctx, name);}
function upload(ctx, address, bytes) {input(ctx, 140, bytes); host(ctx, "upload", address, bytes.length);}
function phase(ctx, budget, registers, pc, reason, retired, nextFP, nextHelper) {
  check(ctx, "before mixed phase"); const beforeFrame = frames.length; frames.push(arena(ctx));
  ctx.expected.set(state(registers, pc, 2)); ctx.expected.set(exit(reason, retired), 56);
  if (nextFP) ctx.expected.set(nextFP, FP); if (nextHelper) ctx.expected.set(nextHelper, 100);
  assert.equal(ctx.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0); check(ctx, "after mixed phase");
  frames.push(arena(ctx)); phases.push({owner: ctx.owner, budget, pc, reason, retired, before_frame: beforeFrame, after_frame: beforeFrame + 1});
}
for (const [index, owner] of ["replacement", "resident"].entries()) {
  const instance = new WebAssembly.Instance(engine.module, {}), ctx = {owner, memory: instance.exports.memory, api: {}};
  for (const name of ["open", "close", "arena_ptr", "map", "upload", "read32", "read8", "compile", "compile_resident", "generation", "module_ptr", "module_len", "guard", "guard_resident", "store8", "store_resident8"])
    ctx.api[name] = instance.exports["ring3_abi_v1_" + name];
  assert.equal(ctx.api.open(3, index + 1, 0x51525354), 0); ctx.base = ctx.api.arena_ptr() >>> 0;
  ctx.expected = Buffer.alloc(SIZE); ctx.expected.set(state(Array(8).fill(0), 0, 2));
  ctx.expected.set(exit(1, 0, 1), 56); ctx.expected.set(helper(1, 0, 0), 100); ctx.expected.set(fpu(true, 0, Buffer.alloc(80)), FP); check(ctx, "initialized full arena");
  for (const address of [PC, DIVISOR & ~0xfff, DST & ~0xfff]) host(ctx, "map", address, 1, address === PC ? 7 : 3);
  upload(ctx, PC, program); upload(ctx, DIVISOR, words([5])); upload(ctx, SRC, Buffer.from([0x81])); upload(ctx, DST, Buffer.from([0x55, 0x56]));
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
  const names = owner === "replacement" ? ["guard", "read32", "read8", "store8"] : ["guard_resident", "read32", "read8", "store_resident8"];
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: "env", name: "memory", kind: "memory"}, ...names.map(name => ({module: "ring3", name, kind: "function"}))]);
  ctx.run = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}).exports.run;
  writeFileSync(join(output, `${owner}.wasm`), bytes, {flag: "wx"}); modules.push({owner, ...binding, bytes: bytes.length, sha256: hash(bytes)});
  const registers = [0, DIVISOR, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, SRC, DST];
  input(ctx, 0, state(registers, PC, 2)); input(ctx, FP, fpu(false));
  registers[0] = 6502; registers[2] = 2;
  phase(ctx, 4, registers, PC + 11, 1, 4, fpu(false, 0x7f00), helper(1, 5, 0));
  registers[0] = 0x1981; registers[6]++; registers[7]++;
  phase(ctx, 2, registers, PC + 13, 1, 2, undefined, helper(2, 0x81, 1));
  registers[7]--;
  phase(ctx, 3, registers, PC + 16, 1, 3, undefined, helper(3, 0, 1));
  registers[0] = 0x7f00;
  phase(ctx, 3, registers, PC + 20, 3, 2);
  for (const [address, literal] of [[DST, 0x66], [DST + 1, 0x81]]) {
    check(ctx, "before destination diagnostic"); assert.equal(ctx.api.read8(address), 0);
    ctx.expected.set(helper(2, literal, 1), 100); check(ctx, "declared destination literal");
  }
  host(ctx, "close");
}
const raw = Buffer.concat(frames); writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
const result = {status: "ok", engine: {bytes: engine.bytes.length, sha256: engine.sha256}, modules, phases,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)},
  source_sha256: Object.fromEntries(["engine/tests/cpu_parallel_memory_wasm.rs", "engine/tests/fixtures/p2-cpu-parallel-memory/run.mjs", "engine/tests/fixtures/support/engine.mjs"].map(path => [path, hash(readFileSync(join(root, path)))])),
  counts: {owners: 2, modules: 2, generated_calls: 8, retired: 22, memory_divisions: 2, loads: 2, stores: 4, fnclex: 2},
  claim: "retained CPU across exact FNCLEX, memory DIV and DF-controlled STOSB/LODSB; full4364-byte arena and literal destination bytes; finite integration, no performance claim"};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"}); console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
