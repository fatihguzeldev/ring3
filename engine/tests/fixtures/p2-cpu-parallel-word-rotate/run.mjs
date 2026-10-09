import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[0-9a-f]{64}$/);
const engine = readEngine(enginePath);
const SIZE = 4364, FP = 4236, PC = 0x1000;
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const program = Buffer.from(
  "66b9210066d3c166d3c90f92c20f94c76683f801f966b9200066d3c40f92c60f90c3" +
  "66c1c6100f90c76683d20066c1cf106683dd0066b8008066d1c80f90c5" +
  "66b8008066c1c8110f90c766bb004066d1c30f90c166bb004066c1c31171020f0beb00", "hex");
assert.equal(program.length, 98);
assert.deepEqual(program, readFileSync(join(output, "word-rotate.x86")), "Rust/JS literal bank correspondence");
writeFileSync(join(output, "engine.wasm"), engine.bytes, {flag: "wx"});
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const frames = [], phases = [], modules = [], inputs = [], hosts = [], initialFrames = [];
const paths = [
  "engine/tests/cpu_parallel_word_rotate_wasm.rs",
  "engine/tests/fixtures/p2-cpu-parallel-word-rotate/run.mjs",
  "engine/tests/fixtures/support/engine.mjs",
];
const sourcePins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const beforeSources = sourcePins();

function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4));
  return bytes;
}
const state = (registers, pc, flags) => record("R3ST", 56, [...registers, pc, flags]);
const exit = (reason, retired) => record("R3EX", 40, [reason, retired, 0, 0, 0, 0]);
function words32(values) {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4));
  return bytes;
}
function fpu(opaque = false) {
  const bytes = record("R3FP", 128);
  bytes.writeUInt16LE(opaque ? 0x27f : 0x37f, 16);
  bytes.writeUInt16LE(opaque ? 0x81a5 : 0, 18);
  bytes.writeUInt16LE(opaque ? 0x55aa : 0xffff, 20);
  bytes.writeUInt16LE(opaque ? 0x7ff : 0, 22);
  bytes.writeUInt32LE(opaque ? 0xf1234567 : 0, 24);
  bytes.writeUInt32LE(opaque ? 0xfedcba98 : 0, 28);
  bytes.writeUInt16LE(opaque ? 0xf135 : 0, 32);
  bytes.writeUInt16LE(opaque ? 0xe246 : 0, 34);
  if (opaque) {
    bytes.set(Array.from({length: 80}, (_, i) => (i * 73 + 29) & 255), 40);
  }
  return bytes;
}
function arena(ctx) {
  return Buffer.from(new Uint8Array(ctx.memory.buffer, ctx.base, SIZE));
}
function check(ctx, label) {
  assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.entries}/${label}`);
}
function input(ctx, offset, bytes, label) {
  assert.equal(ctx.started, false, "no interphase host CPU/RAM input");
  new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + offset);
  ctx.expected.set(bytes, offset);
  check(ctx, label);
  inputs.push({context: ctx.context, kind: "arena", offset, hex: Buffer.from(bytes).toString("hex"), label});
}
function host(ctx, name, ...args) {
  check(ctx, `before ${name}`);
  const status = ctx.api[name](...args);
  assert.equal(status, 0, name);
  check(ctx, name);
  hosts.push({context: ctx.context, name, args, status});
}
function phase(ctx, label, budget, registers, pc, flags, reason, retired = 1) {
  check(ctx, `before ${label}`);
  const beforeFrame = frames.length;
  frames.push(arena(ctx));
  ctx.expected.set(state(registers, pc, flags));
  ctx.expected.set(exit(reason, retired), 56);
  const args = [ctx.base, ctx.base + 56, budget, ctx.base + 96];
  const status = ctx.run(...args);
  assert.equal(status, 0);
  check(ctx, `after ${label}`);
  frames.push(arena(ctx));
  phases.push({context: ctx.context, owner: ctx.owner, entries: ctx.entries, label, args, status,
    budget, before_frame: beforeFrame, after_frame: beforeFrame + 1, retired, reason});
}

for (const owner of ["replacement", "resident"]) {
  for (const entries of [false, true]) {
    const context = modules.length;
    const key = [context + 1, 0x57524f54];
    const instance = new WebAssembly.Instance(engine.module, {});
    const ctx = {owner, entries, context, memory: instance.exports.memory, api: {}, started: false};
    for (const name of ["open", "close", "arena_ptr", "map", "protect", "upload", "compile",
      "compile_entries", "compile_resident", "compile_resident_entries", "generation",
      "module_ptr", "module_len", "guard", "guard_resident"]) {
      ctx.api[name] = instance.exports["ring3_abi_v1_" + name];
    }
    const openArgs = [1, ...key];
    const openStatus = ctx.api.open(...openArgs);
    assert.equal(openStatus, 0);
    hosts.push({context, name: "open", args: openArgs, status: openStatus});
    ctx.base = ctx.api.arena_ptr() >>> 0;
    ctx.expected = Buffer.alloc(SIZE);
    ctx.expected.set(state(Array(8).fill(0), 0, 2));
    ctx.expected.set(exit(1, 0), 56);
    ctx.expected.set(record("R3MH", 40, [0, 0, 0, 0, 0, 0]), 100);
    ctx.expected.set(fpu(), FP);
    check(ctx, "initialized full arena");
    host(ctx, "map", PC, 1, 7);
    input(ctx, 140, program, "literal guest bank");
    host(ctx, "upload", PC, program.length);
    inputs.push({context, kind: "upload", address: PC, hex: program.toString("hex"), label: "literal guest bank"});
    host(ctx, "protect", PC, 1, 4);
    input(ctx, 140, words32(entries ? [PC, PC + 96] : [PC, 94, PC + 96, 2]), "two explicit or entry block seeds");
    const compileName = owner === "replacement"
      ? entries ? "compile_entries" : "compile"
      : entries ? "compile_resident_entries" : "compile_resident";
    const compileArgs = entries ? [2, 0] : [2];
    let binding;
    if (owner === "replacement") {
      host(ctx, compileName, ...compileArgs);
      binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
    } else {
      check(ctx, "before resident compile");
      const status = ctx.api[compileName](...compileArgs);
      assert.equal(status, 0);
      hosts.push({context, name: compileName, args: compileArgs, status});
      const receipt = arena(ctx);
      assert.equal(receipt.readUInt32LE(140), 1);
      assert.equal(receipt.readUInt32LE(144), 24);
      binding = Object.fromEntries(["low", "high", "pointer", "length"].map((name, i) => [name, receipt.readUInt32LE(148 + 4 * i)]));
      ctx.expected.set(words32([1, 24, binding.low, binding.high, binding.pointer, binding.length]), 140);
      check(ctx, "resident receipt only");
    }
    const bytes = Buffer.from(new Uint8Array(ctx.memory.buffer, binding.pointer, binding.length));
    const module = new WebAssembly.Module(bytes);
    const guard = owner === "replacement" ? "guard" : "guard_resident";
    assert.deepEqual(WebAssembly.Module.imports(module), [
      {module: "env", name: "memory", kind: "memory"},
      {module: "ring3", name: guard, kind: "function"},
    ]);
    assert.deepEqual(WebAssembly.Module.exports(module), [{name: "run", kind: "function"}]);
    ctx.run = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}).exports.run;
    const file = `${owner}-${entries ? "entry" : "explicit"}.wasm`;
    writeFileSync(join(output, file), bytes, {flag: "wx"});
    modules.push({context, owner, entries, arena_base: ctx.base, key_low: key[0], key_high: key[1], file,
      ...binding, bytes: bytes.length, sha256: hash(bytes)});

    const registers = [0xa53c8000, 0x91b2aa55, 0x345680a5, 0x87a9f0a5,
      0xc5e68001, 0x56780000, 0x67898001, 0x789a8001];
    const flags = [0xcd7, 0x8d6, 0x497, 2][context];
    const df = flags & 0x400;
    input(ctx, 0, state(registers, PC, flags), "single initial CPU seed");
    input(ctx, FP, fpu(true), "opaque FP128 seed");
    initialFrames.push({context, frame: frames.length});
    ctx.started = true;
    const inputCount = inputs.length, hostCount = hosts.length;

    registers[1] = 0x91b20042;
    phase(ctx, "MOV CX21 then ROL currentCX by currentCL21 masked1", 2,
      registers, PC + 7, flags & ~0x801, 1, 2);
    registers[1] = 0x91b28010;
    phase(ctx, "ROR currentCX42 by newly-produced CL42 masked2", 1,
      registers, PC + 10, (flags & ~0x801) | 1, 1);
    registers[2] = 0x34568001;
    registers[3] = 0x87a900a5 | ((flags & 0x40) ? 0x100 : 0);
    registers[1] = 0x91b20020;
    phase(ctx, "SETB DL1 SETZ BH preservedZF then CMP AX8000,1 STC MOV CX32 dirtyCF OF AF", 5,
      registers, PC + 25, 0x817 | df, 1, 5);
    phase(ctx, "ROL SP8001 by currentCL32 masked0 preserves fullFLAGS and parent", 1,
      registers, PC + 28, 0x817 | df, 1);
    registers[2] = 0x34560101;
    registers[3] = ((registers[3] & 0xffffff00) | 1) >>> 0;
    phase(ctx, "SETB DH1 SETO BL1 consume masked0 preserved dirtyFLAGS", 2,
      registers, PC + 34, 0x817 | df, 1, 2);
    phase(ctx, "ROL SI8001 by16 keeps value but updatesCF1 and clears undefinedOF", 1,
      registers, PC + 38, 0x17 | df, 1);
    registers[3] = 0x87a90001;
    registers[2] = 0x34560102;
    phase(ctx, "SETO BH0 then ADC currentDX0101,0 consumes q16CF1", 2,
      registers, PC + 45, 0x2 | df, 1, 2);
    phase(ctx, "ROR DI8001 by16 keeps value but changes oldCF0 to1", 1,
      registers, PC + 49, 0x3 | df, 1);
    registers[5] = 0x5678ffff;
    phase(ctx, "SBB BP0,0 consumes q16CF1 toFFFF with high16 preserved", 1,
      registers, PC + 53, 0x97 | df, 1);
    registers[0] = 0xa53c4000;
    registers[1] = 0x91b20120;
    phase(ctx, "MOV AX8000 ROR AX1 SETO CH1 keeps SZAP and captures definedOF", 3,
      registers, PC + 63, 0x896 | df, 1, 3);
    phase(ctx, "MOV AX8000 ROR AX17 SETO BH0 same distance1 but undefinedOF0", 3,
      registers, PC + 74, 0x96 | df, 1, 3);
    registers[3] = 0x87a98000;
    registers[1] = 0x91b20101;
    phase(ctx, "MOV BX4000 ROL BX1 SETO CL1 captures definedOF in currentCX", 3,
      registers, PC + 84, 0x896 | df, 1, 3);
    phase(ctx, "MOV BX4000 ROL BX17 same distance1 clears undefinedOF", 2,
      registers, PC + 92, 0x96 | df, 1, 2);
    phase(ctx, "JNO consumes q17OF0 and skips unseeded UD2", 1,
      registers, PC + 96, 0x96 | df, 1);
    phase(ctx, "cold JMP", 2, registers, PC + 98, 0x96 | df, 3);

    assert.equal(inputs.length, inputCount, "retained phases have no host CPU/RAM input");
    assert.equal(hosts.length, hostCount, "no interphase host diagnostics");
    check(ctx, "before close");
    const status = ctx.api.close();
    assert.equal(status, 0);
    hosts.push({context, name: "close", args: [], status});
  }
}

const raw = Buffer.concat(frames);
writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
assert.equal(frames.length, 120);
assert.equal(phases.length, 60);
assert.equal(inputs.length, 20);
assert.equal(hosts.length, 24);
assert.equal(phases.reduce((sum, phase) => sum + phase.retired, 0), 116);
assert.deepEqual(sourcePins(), beforeSources);
const result = {
  status: "ok", engine: {file: "engine.wasm", bytes: engine.bytes.length, sha256: engine.sha256},
  modules, initial_frames: initialFrames, inputs, hosts, phases,
  raw: {file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw)},
  source_sha256: beforeSources,
  counts: {owners: 2, descriptor_modes: 2, contexts: 4, modules: 4,
    word_rols: 20, word_rors: 16, word_movs: 24, word_cmps: 4, word_adcs: 4, word_sbbs: 4,
    stc: 4, setcc: 32, conditional_jumps: 4, jumps: 4, generated_calls: 60, retired: 116,
    raw_frames: 120, arena_inputs: 16, uploads: 4, host_rows: 24, physical_files: 8,
    maximum_live_resident_units: 1},
  claim: "retained currentCX/CL alias ROL21->42 then ROR42 with newCL42->8010, CF1 consumed by SETBDL and preserved initialZF consumed by SETZBH; CMP/STC dirty817 then currentCL32 masked0 ROLSP preserves fullFLAGS/SP for SETB/SETO. ROLSI8001,16 preserves value but clears dirtyOF and supplies CF1 to ADCDX; RORDI8001,16 changes oldCF0 to1 for SBBBP0->FFFF. Both directions compare value-equal distance1 q1/q17 but definedOF1 versus declared undefinedOF0, captured by SETcc/currentCX and JNO skipping UD2 before cold. Fifteen generated phases and29 retired instructions/context across four bound replacement/resident explicit/entry contexts, two declared blocks and one resident unit/context; no interphase host CPU/RAM input or diagnostics. Full4364 generated before/after images include high16, all parents, ordinarySP, opaqueFP128, helper and transfer. Ordered setup host records/live full-arena assertions are not saved host-frame journals or wholeRAM/module-body/allocator proof. Close has preclose assertion/API0 only, no postclose arena/module access or raw5. Literal producer anchors require independent reconstruction from physical before/input bytes. OF0 for nonzero multi-count is Ring3 deterministic undefined-bit policy, not hardware equivalence.",
};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), {flag: "wx"});
console.log(JSON.stringify({status: result.status, counts: result.counts, output}));
