import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[0-9a-f]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, FP = 4236, PC = 0x1000, SOURCE = 0x3008, DESTINATION = 0x4008;
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash("sha256").update(bytes).digest("hex"), frames = [], phases = [], modules = [], ram = [];
const paths = ["engine/tests/cpu_parallel_accumulator_string_wasm.rs", "engine/tests/fixtures/p2-cpu-parallel-accumulator-string/run.mjs", "engine/tests/fixtures/support/engine.mjs"];
const sourcePins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))])), beforeSources = sourcePins();
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
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.direction}/${label}`);}
function input(ctx, offset, bytes) {new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + offset); ctx.expected.set(bytes, offset); check(ctx, "declared host input");}
function host(ctx, name, ...args) {check(ctx, `before ${name}`); assert.equal(ctx.api[name](...args), 0, name); check(ctx, name);}
function upload(ctx, address, bytes) {input(ctx, 140, bytes); host(ctx, "upload", address, bytes.length);}
function phase(ctx, budget, registers, pc, flags, reason, retired, readValue) {
  check(ctx, "before phase"); const beforeFrame = frames.length; frames.push(arena(ctx));
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);
  if (readValue !== undefined) ctx.expected.set(helper(readValue), 100);
  assert.equal(ctx.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0); check(ctx, "after phase");
  frames.push(arena(ctx)); phases.push({context: ctx.context, owner: ctx.owner, direction: ctx.direction, budget, pc, flags, reason, retired, before_frame: beforeFrame, after_frame: beforeFrame + 1});
}
for (const owner of ["replacement", "resident"]) for (const direction of ["forward", "reverse"]) {
  const reverse = direction === "reverse", step = reverse ? -4 : 4, context = modules.length;
  const pointer = DESTINATION + (reverse ? 8 : 0), source = SOURCE + (reverse ? 8 : 0);
  const program = Buffer.from(reverse ? "adabbf10400000afa5a7eb00" : "adabbf08400000afa5a7eb00", "hex");
  assert.deepEqual(program, readFileSync(join(output, `${direction}.x86`)), "independent Rust/JS literal program");
  const instance = new WebAssembly.Instance(engine.module, {}), ctx = {owner, direction, context, memory: instance.exports.memory, api: {}};
  for (const name of ["open", "close", "arena_ptr", "map", "upload", "compile", "compile_resident", "generation", "module_ptr", "module_len", "guard", "guard_resident", "read8", "read32", "store32", "store_resident32"])
    ctx.api[name] = instance.exports["ring3_abi_v1_" + name];
  assert.equal(ctx.api.open(3, context + 1, 0x41434353), 0); ctx.base = ctx.api.arena_ptr() >>> 0;
  ctx.expected = Buffer.alloc(SIZE); ctx.expected.set(state(Array(8).fill(0), 0, 2));
  ctx.expected.set(exit(1, 0, 1), 56); ctx.expected.set(helper(0), 100); ctx.expected.set(fpu(), FP); check(ctx, "initialized full arena");
  for (const address of [PC, SOURCE & ~0xfff, DESTINATION & ~0xfff]) host(ctx, "map", address, 1, address === PC ? 7 : 3);
  upload(ctx, PC, program);
  upload(ctx, SOURCE, words(reverse ? [0x80000000, 0xfedcba98, 0x81a573c2] : [0x81a573c2, 0xfedcba98, 0x80000000]));
  const destinationSeed = Buffer.concat([Buffer.from([0x31]), words(reverse ? [0x80000000, 0, 0] : [0, 0, 0x80000000]), Buffer.from([0xd7])]);
  upload(ctx, DESTINATION - 1, destinationSeed);
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
  const file = `${owner}-${direction}.wasm`; writeFileSync(join(output, file), bytes, {flag: "wx"}); modules.push({context, owner, direction, file, ...binding, bytes: bytes.length, sha256: hash(bytes)});
  const registers = [0xa53c0011, 31, 0xa53cabcd, 0x3456789a, 0x4567ff00, 0x56789abc, source, pointer], initialFlags = reverse ? 0xcd7 : 0x8d7, comparisonFlags = reverse ? 0x446 : 0x46;
  input(ctx, 0, state(registers, PC, initialFlags)); input(ctx, FP, fpu(true));
  registers[0] = 0x81a573c2; registers[6] += step; phase(ctx, 1, registers, PC + 1, initialFlags, 1, 1, 0x81a573c2);
  registers[7] += step; phase(ctx, 1, registers, PC + 2, initialFlags, 1, 1, 0);
  registers[7] = pointer + step; phase(ctx, 2, registers, PC + 8, comparisonFlags, 1, 2, 0x81a573c2);
  registers[6] += step; registers[7] += step; phase(ctx, 1, registers, PC + 9, comparisonFlags, 1, 1, 0);
  registers[6] += step; registers[7] += step; phase(ctx, 1, registers, PC + 10, comparisonFlags, 1, 1, 0x80000000);
  phase(ctx, 2, registers, PC + 12, comparisonFlags, 3, 1);
  const destination = Buffer.concat([Buffer.from([0x31]), words(reverse ? [0x80000000, 0xfedcba98, 0x81a573c2] : [0x81a573c2, 0xfedcba98, 0x80000000]), Buffer.from([0xd7])]);
  for (let i = 0; i < destination.length; i++) {
    check(ctx, "before live RAM diagnostic"); assert.equal(ctx.api.read8(DESTINATION - 1 + i), 0);
    ctx.expected.set(record("R3MH", 40, [0, destination[i], 0, 0, 0, 1], 2), 100); check(ctx, "literal destination/sentinel diagnostic");
    ram.push({context, address: DESTINATION - 1 + i, byte: destination[i]});
  }
  check(ctx, "before close"); assert.equal(ctx.api.close(), 0);
}
const raw = Buffer.concat(frames); writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
assert.equal(frames.length, 48); assert.equal(phases.length, 24); assert.equal(ram.length, 56); assert.equal(phases.reduce((sum, p) => sum + p.retired, 0), 28);
assert.deepEqual(sourcePins(), beforeSources);
const result = {status: "ok", engine: {bytes: engine.bytes.length, sha256: engine.sha256}, modules, phases, ram,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)}, source_sha256: beforeSources,
  counts: {owners: 2, directions: 2, contexts: 4, modules: 4, generated_calls: 24, retired: 28, lodsd: 4, stosd: 4, mov_edi: 4, scasd: 4, movsd: 4, cmpsd: 4, jumps: 4, raw_frames: 48, live_ram_bytes: 56, physical_files: 8},
  claim: "full current LODSD EAX→STOSD RAM→guest EDI reset→SCASD Read32 witness→MOVSD→CMPSD→cold, both DF directions/owners; six retained phases/context with no CPU reseed; 48 physical full4364 arenas/opaqueFP; destination/sentinel bytes are live selected Read8 diagnostics, not saved RAM pages or performance proof"};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"}); console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
