import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, "expected current engine, output directory and repository root");
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const { bytes: engineBytes, module: engineModule, sha256: engineSha256 } = readEngine(enginePath);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = 0x1f00,
  KEEP = 0x3000, DATA = 0x4000, SECOND = 0x5000, BYTE = 0x4010,
  TOP = 0xfffff000, HIGH = 0xc8442000;
const RAW = [0, 1, 2, 7, 8, 9, 31, 32, 33, 255], FLAGS = [2, 0xcd7],
  REG = [0x1234807f, 0xa1b2c378, 0x34560f10, 0x4567ff00,
    0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const pattern = Buffer.from(Array.from({ length: 4096 }, (_, index) => index % 251 + 1));
const plan = {
  contexts: 24, modules: 48, native_modules: 0, actual_modules: 48,
  source_instructions: 474, seeds: 288, arithmetic_targets: 192, ea_targets: 24,
  endpoint_targets: 12, repaired_targets: 30, chain_targets: 24, code_targets: 18,
  targets: 300, q0_targets: 54, q1_targets: 90, multi_targets: 156,
  chain_pairs: 12, movcl_producers: 54, setb: 288, seto: 288, jumps: 288,
  canaries: 18, prefixes: 102, fault_calls: 60, read_faults: 12, write_faults: 48,
  repairs: 30, map_only_repairs: 6, permission_only_repairs: 12,
  changed_value_repairs: 12, cl_input_edits: 6,
  code_stores: 18, same_value_code_stores: 12, changing_code_stores: 6,
  stale_calls: 18, closed_calls: 48, direct_guards: 48,
  compiler_refusals: 6, rejection_survivals: 6,
  generated_calls: 1326, retired: 1284, maps: 132, unmaps: 12, protects: 96,
  host_uploads: 420, host_uploaded_bytes: 516510, operand_reads: 630,
  host_read8: 630, host_read32: 350208, host_requests: 474, cancel_writes: 72,
  host_inputs: 1260, host_calls: 1782, pages: 342, page_bytes: 1400832,
  raw_arenas: 3282, raw_arena_bytes: 13902552, files: 92,
};
const counts = Object.fromEntries(Object.keys(plan).map(name => [name, 0]));
const artifacts = [], contexts = [], modules = [], targets = [], runs = [], faults = [],
  chains = [], codeStores = [], mutations = [], refusals = [], hostInputs = [], hostCalls = [],
  arenaRows = [], arenaFrames = [], pageRows = [], pageFrames = [];
let events = 0, activeCall = null, lastContext = null;
function artifact(path, bytes, write = true) {
  assert.match(path, /^[a-z0-9][a-z0-9.-]*$/);
  assert.ok(!artifacts.some(row => row.path === path));
  if (write) writeFileSync(join(output, path), bytes, { flag: "wx" });
  const row = { path, bytes: bytes.length, sha256: hash(bytes) };
  artifacts.push(row); return row;
}
assert.deepEqual(readdirSync(output).sort(), ["sar", "shl", "shr"].map(kind => "memory-" + kind + ".x86"),
  "exclusive three wrapper-authored x86 bank roster");
artifact("engine.wasm", engineBytes);
const sourcePaths = [
  "Cargo.lock",
  "Cargo.toml",
  "engine/Cargo.toml",
  "engine/src/abi/memory_helper.rs",
  "engine/src/abi/x86/state.rs",
  "engine/src/cpu/dbt/region.rs",
  "engine/src/cpu/dbt/wasm/emitter.rs",
  "engine/src/cpu/dbt/wasm/integer.rs",
  "engine/src/cpu/dbt/wasm/locals.rs",
  "engine/src/cpu/dbt/wasm/memory.rs",
  "engine/src/cpu/dbt/wasm/memory/byte_store.rs",
  "engine/src/cpu/dbt/wasm/memory/narrow.rs",
  "engine/src/cpu/x86/decode/decoder.rs",
  "engine/src/cpu/x86/decode/integer.rs",
  "engine/src/cpu/x86/decode/operands.rs",
  "engine/src/cpu/x86/decode/profile.rs",
  "engine/src/cpu/x86/ir.rs",
  "engine/src/memory/space.rs",
  "engine/src/process/instance.rs",
  "engine/tests/cpu_byte_shift.rs",
  "engine/tests/cpu_memory_byte_arithmetic.rs",
  "engine/tests/cpu_memory_byte_carry.rs",
  "engine/tests/cpu_memory_byte_carry_rotate_cl.rs",
  "engine/tests/cpu_memory_byte_logical.rs",
  "engine/tests/cpu_memory_byte_shift.rs",
  "engine/tests/cpu_memory_byte_shift_immediate.rs",
  "engine/tests/cpu_memory_byte_shift_immediate_one.rs",
  "engine/tests/cpu_memory_byte_shift_immediate_wasm.rs",
  "engine/tests/cpu_memory_set_byte.rs",
  "engine/tests/cpu_register_memory_byte_arithmetic.rs",
  "engine/tests/cpu_register_memory_byte_carry.rs",
  "engine/tests/cpu_register_memory_byte_compare.rs",
  "engine/tests/cpu_register_memory_byte_logical.rs",
  "engine/tests/cpu_shift.rs",
  "engine/tests/fixtures/p2-memory-byte-carry-rotate-cl/run.mjs",
  "engine/tests/fixtures/p2-memory-byte-shift-immediate/run.mjs",
  "engine/tests/fixtures/support/engine.mjs",
  "rust-toolchain.toml",
  "engine/tests/cpu_memory_byte_shift_cl.rs",
  "engine/tests/cpu_memory_byte_shift_cl_wasm.rs",
  "engine/tests/fixtures/p2-memory-byte-shift-cl/run.mjs"
];
assert.equal(sourcePaths.length, 41);
assert.equal(new Set(sourcePaths).size, 41);
const pinSources = () => Object.fromEntries(sourcePaths.map(path => {
  const bytes = readFileSync(join(root, path)); return [path, { bytes: bytes.length, sha256: hash(bytes) }];
}));
const sourcePins = pinSources();
const fullText = readFileSync(join(root, "target/p2-memory-binary-spec/253667-093-sdm-vol-2b.txt"));
assert.equal(hash(fullText), "f5e6dc689d41655d64792512bfe8add56fc4ac97203aab5c1377a72bac291a35");
const chapter = Buffer.from(fullText.toString("utf8").split("\n").slice(30071, 30275).join("\n") + "\n");
assert.equal(chapter.length, 10995);
assert.equal(hash(chapter), "681653cdc3aa1523fc3d58897bad26769a122c39514f2ffa2c007a0a788b588d");
const savedChapter = artifact("intel-shift.txt", chapter);

function shiftArithmetic(kind, operand, rawCount, oldFlags) {
  const count = rawCount % 32;
  if (count === 0) return { result: operand, flags: oldFlags };
  const signed = operand >= 128 ? operand - 256 : operand, factor = 2 ** count;
  const wide = kind === "shl" ? operand * factor : kind === "shr" ? Math.floor(operand / factor) : Math.floor(signed / factor);
  const result = (wide % 256 + 256) % 256;
  const carry = kind === "shl" ? (count < 8 ? Math.floor(operand / 2 ** (8 - count)) % 2 : 0)
    : kind === "shr" ? (count < 8 ? Math.floor(operand / 2 ** (count - 1)) % 2 : 0)
      : Math.floor(operand / 2 ** (Math.min(count, 8) - 1)) % 2;
  const overflow = count === 1 && (kind === "shl" ? Math.floor(result / 128) !== carry : kind === "shr" && operand >= 128);
  let ones = 0;
  for (let value = result; value > 0; value = Math.floor(value / 2)) ones += value % 2;
  const flags = (oldFlags & 0x400) | 2 | carry | (ones % 2 === 0 ? 4 : 0)
    | (result === 0 ? 0x40 : 0) | (result >= 128 ? 0x80 : 0) | (overflow ? 0x800 : 0);
  return { result, flags };
}
const ANCHORS = [
  ["shl", 0x80, 1, 2, 0, 0x847], ["shr", 0x80, 1, 2, 0x40, 0x802], ["sar", 0x80, 1, 2, 0xc0, 0x86],
  ["shl", 0xff, 8, 3, 0, 0x46], ["shr", 0xff, 8, 3, 0, 0x46],
  ["sar", 0x80, 8, 2, 0xff, 0x87], ["sar", 0x80, 31, 2, 0xff, 0x87],
  ["shl", 0x80, 32, 0xcd7, 0x80, 0xcd7], ["shr", 0x80, 33, 2, 0x40, 0x802],
];
for (const [kind, value, raw, flags, result, afterFlags] of ANCHORS)
  assert.deepEqual(shiftArithmetic(kind, value, raw, flags), { result, flags: afterFlags });

function profile(bytes, owner, memory, ctx, binding) {
  let at = 8;
  assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  const uleb = () => {
    let value = 0, scale = 1;
    for (let index = 0; index < 5; index++) {
      assert.ok(at < bytes.length);
      const byte = bytes[at++];
      value += (byte & 127) * scale;
      if (!(byte & 128)) { assert.ok(value <= 0xffffffff); return value; }
      scale *= 128;
    }
    assert.fail("invalid bounded unsigned LEB");
  };
  const sleb = () => {
    let value = 0, scale = 1;
    for (let index = 0; index < 5; index++) {
      assert.ok(at < bytes.length);
      const byte = bytes[at++];
      value += (byte & 127) * scale;
      scale *= 128;
      if (!(byte & 128)) return (byte & 64 ? value - scale : value) >>> 0;
    }
    assert.fail("invalid bounded signed LEB");
  };
  const text = () => {
    const length = uleb();
    assert.ok(at + length <= bytes.length);
    const value = bytes.subarray(at, at + length).toString("utf8"); at += length;
    return value;
  };
  const sections = new Map();
  while (at < bytes.length) {
    const id = bytes[at++], length = uleb(), start = at;
    assert.ok(start + length <= bytes.length && !sections.has(id));
    sections.set(id, [start, start + length]); at += length;
  }
  assert.deepEqual([...sections.keys()], [1, 2, 3, 7, 10]);
  const section = id => { at = sections.get(id)[0]; }, ended = id => assert.equal(at, sections.get(id)[1]);
  section(1);
  const types = [];
  for (let index = 0, length = uleb(); index < length; index++) {
    assert.equal(bytes[at++], 0x60);
    const parameters = [], results = [];
    for (let index = 0, length = uleb(); index < length; index++) parameters.push(bytes[at++]);
    for (let index = 0, length = uleb(); index < length; index++) results.push(bytes[at++]);
    types.push({ parameters, results });
  }
  ended(1);
  const expectedTypes = [{ parameters: Array(4).fill(0x7f), results: [0x7f] },
    { parameters: Array(owner === "replacement" ? 6 : 7).fill(0x7f), results: [0x7f] }];
  if (memory) expectedTypes.push({ parameters: [0x7f], results: [0x7f] },
    { parameters: Array(owner === "replacement" ? 2 : 6).fill(0x7f), results: [0x7f] });
  assert.deepEqual(types, expectedTypes);
  const names = [owner === "replacement" ? "guard" : "guard_resident",
    ...(memory ? ["read8", owner === "replacement" ? "store8" : "store_resident8"] : [])];
  section(2); assert.equal(uleb(), names.length + 1);
  assert.equal(text(), "env"); assert.equal(text(), "memory");
  assert.equal(bytes[at++], 2); assert.equal(uleb(), 0); assert.equal(uleb(), 1);
  for (const [index, name] of names.entries()) {
    assert.equal(text(), "ring3"); assert.equal(text(), name);
    assert.equal(bytes[at++], 0); assert.equal(uleb(), index + 1);
  }
  ended(2); section(3); assert.equal(uleb(), 1); assert.equal(uleb(), 0); ended(3);
  section(7); assert.equal(uleb(), 1); assert.equal(text(), "run");
  assert.equal(bytes[at++], 0); assert.equal(uleb(), names.length); ended(7);
  section(10); assert.equal(uleb(), 1);
  const bodyLength = uleb(), bodyEnd = at + bodyLength;
  assert.equal(bodyEnd, sections.get(10)[1]);
  const locals = [];
  for (let index = 0, length = uleb(); index < length; index++) locals.push([uleb(), bytes[at++]]);
  assert.deepEqual(locals, [[16, 0x7f], [1, 0x7e], ...(memory ? [[6, 0x7f]] : [])]);
  const guardConstants = [ctx.low, ctx.high,
    ...(owner === "replacement" ? [binding.generation] : [binding.low, binding.high])];
  for (const value of guardConstants) { assert.equal(bytes[at++], 0x41); assert.equal(sleb(), value >>> 0); }
  for (const parameter of [0, 1, 3]) { assert.equal(bytes[at++], 0x20); assert.equal(uleb(), parameter); }
  assert.equal(bytes[at++], 0x10); assert.equal(uleb(), 0);
  assert.equal(bytes[bodyEnd - 1], 0x0b);
  return { types, locals, guard_constants: guardConstants, body_bytes: bodyLength,
    sections: [...sections.keys()], body_claim: "types/imports/locals/guard prefix; remaining body validated, not independently certified" };
}

function bank(kind, extension) {
  const bytes = Buffer.alloc(4096, 0xcc), main = [], fault = [], smc = [];
  let at = 0;
  function append(form, formIndex, prefix = 0, prefixInstructions = 0) {
    const entry = PC + at, target = entry + prefix, next = entry + form.length;
    bytes.set(form, at); at += form.length;
    bytes.set([0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2], at); at += 6;
    bytes[at] = 0xe9; bytes.writeInt32LE(COLD - (PC + at + 5), at + 1); at += 5;
    return { entry, target, next, form: formIndex, spec: [entry, PC + at - entry],
      instructions: 4 + prefixInstructions };
  }
  for (const [index, form] of [
    [0xd2, 0x03 | extension], [0xd2, 0x01 | extension],
    [0xd2, 0x04 | extension, 0x89], [0xd2, 0x44 | extension, 0x8f, 0xf0],
    [0xd2, 0x04 | extension, 0x24], [0xd2, 0x07 | extension],
  ].entries()) main.push(append(form, index));
  assert.equal(at, 82);
  for (const raw of [1, 32, 2]) {
    const scan = append([0xb1, raw, 0x8d, 0x5b, 1, 0xd2, 0x03 | extension], 0, 5, 2);
    scan.produced_raw = raw; fault.push(scan);
  }
  const chain = append([0xb1, 1, 0xd2, 0x03 | extension, 0xb1, 7, 0xd2, 0x03 | extension], 0, 2, 3);
  chain.first_next = chain.entry + 4; chain.second_producer = chain.entry + 4;
  chain.second = chain.entry + 6; chain.tail = chain.entry + 8;
  assert.equal(chain.instructions, 7); assert.equal(at, 155);
  for (const [raw, value] of [[32, 0x81], [8, 0], [1, 0x81]]) {
    const start = 0xe00 + smc.length * 32, entry = PC + start;
    const form = [0xb0, value, 0xd2, 0x07 | extension,
      0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0xbb, 0xef, 0xbe, 0xad, 0xde, 0xe9];
    bytes.set(form, start); bytes.writeInt32LE(COLD - (entry + 20), start + 16);
    smc.push({ entry, target: entry + 2, next: entry + 4, raw, value, form: 5,
      canary: true, spec: [entry, 20], instructions: 6 });
  }
  bytes.set([0x0f, 0x0b], COLD - PC);
  assert.deepEqual(bytes, readFileSync(join(output, "memory-" + kind + ".x86")), "independent Rust/JS bank bytes");
  const saved = artifact("memory-" + kind + ".x86", bytes, false);
  return { kind, extension, bytes, main, fault, chain, smc, packed: at, saved };
}
const banks = [bank("shl", 0x20), bank("shr", 0x28), bank("sar", 0x38)];

process.on("uncaughtException", error => {
  const ctx = activeCall?.ctx ?? lastContext;
  let currentBytes = null, authority = null;
  if (ctx?.base !== undefined && ctx.bytes.length >= ctx.base + SIZE) {
    currentBytes = Buffer.from(ctx.bytes.subarray(ctx.base, ctx.base + SIZE));
    authority = "already-held full live arena view; no refresh/getter/helper reentry";
  } else if (ctx !== null) {
    for (let index = arenaRows.length - 1; index >= 0; index--) if (arenaRows[index].context === ctx.ordinal) {
      currentBytes = Buffer.from(arenaFrames[index]);
      authority = { held_frame: arenaRows[index], claim: "last captured full arena, not fresh current bytes" };
      break;
    }
  }
  if (currentBytes !== null) writeFileSync(join(output, "failure-current-arena.bin"), currentBytes, { flag: "wx" });
  writeFileSync(join(output, "failure-arena-snapshots.bin"), Buffer.concat(arenaFrames), { flag: "wx" });
  writeFileSync(join(output, "failure-page-snapshots.bin"), Buffer.concat(pageFrames), { flag: "wx" });
  writeFileSync(join(output, "failure.json"), JSON.stringify({ status: "failed", error: String(error.stack ?? error),
    current: currentBytes === null ? null : { context: ctx.ordinal, label: activeCall?.label ?? "outside generated call",
      args: activeCall?.args ?? null, actual_status: activeCall?.actualStatus ?? null,
      arena_sha256: hash(currentBytes), arena_authority: authority }, plan, counts, source_pins: sourcePins,
    contexts, modules, targets, runs, faults, chains, code_stores: codeStores, mutations, refusals,
    host_inputs: hostInputs, host_calls: hostCalls, raw_arena_records: arenaRows, pages: pageRows, artifacts }, null, 2), { flag: "wx" });
  console.error(error); process.exitCode = 1;
});

function record(magic, size, fields, version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4));
  return bytes;
}
const state = (registers, pc, flags) => record("R3ST", 56, [...registers, pc, flags]);
const exit = (reason, retired, detail = 0, address = 0, access = 0, version = 2) =>
  record("R3EX", 40, [reason, retired, detail, address, access, access ? 1 : 0], version);
const wordHelper = (value = 0) => record("R3MH", 40, [0, value, 0, 0, 0, 0]);
const readHelper = (value, detail = 0, address = 0) =>
  record("R3MH", 40, [detail ? 1 : 0, detail ? 0 : value, detail, detail ? address : 0, detail ? 1 : 0, 1], 2);
const storeHelper = (detail = 0, address = 0) =>
  record("R3MH", 40, [detail ? 1 : 0, 0, detail, detail ? address : 0, detail ? 2 : 0, 1], 3);
const words = values => { const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes; };
function refresh(ctx) { ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx; }
function arena(ctx) { return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE)); }
function check(ctx, label) { assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.bank.kind}/${ctx.ordinal}/${label}: full arena`); }
function frame(ctx, label, unit) {
  check(ctx, label);
  const bytes = arena(ctx), row = { file: "arena-snapshots.bin", context: ctx.ordinal, module: unit.file, label,
    offset: arenaFrames.length * SIZE, bytes: SIZE, sha256: hash(bytes) };
  arenaFrames.push(bytes); arenaRows.push(row); counts.raw_arenas++; counts.raw_arena_bytes += SIZE;
  return row;
}
function host(ctx, name, args, status = 0) {
  check(ctx, `before host ${name}`); assert.equal(ctx.api[name](...args), status, name); check(ctx, `host ${name}`);
  hostCalls.push({ event: ++events, context: ctx.ordinal, name, args, status });
}
function request(ctx, bytes, label) {
  assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER);
  ctx.expected.set(bytes, TRANSFER); check(ctx, label);
  hostInputs.push({ event: ++events, context: ctx.ordinal, label, arena_offset: TRANSFER, bytes: bytes.length, hex: bytes.toString("hex") });
  counts.host_requests++;
}
function patch(ctx, address, bytes) {
  assert.ok(address >= 0 && address + bytes.length <= 4294967296);
  for (const [index, value] of bytes.entries()) {
    const at = address + index, page = Math.floor(at / 4096) * 4096;
    assert.ok(ctx.pages.has(page)); ctx.pages.get(page)[at % 4096] = value;
  }
}
function upload(ctx, address, bytes, label = "explicit host upload") {
  request(ctx, bytes, label); host(ctx, "upload", [address, bytes.length]); patch(ctx, address, bytes);
  counts.host_uploads++; counts.host_uploaded_bytes += bytes.length;
  hostInputs.push({ event: ++events, context: ctx.ordinal, label, address, bytes: bytes.length, hex: bytes.toString("hex") });
}
function map(ctx, address, bytes = pattern) {
  host(ctx, "map", [address, 1, 3]); ctx.pages.set(address, Buffer.alloc(4096)); ctx.permissions.set(address, 3); counts.maps++;
  if (bytes !== null) upload(ctx, address, bytes, "patterned page");
}
function protect(ctx, address, permissions) {
  host(ctx, "protect", [address, 1, permissions]); ctx.permissions.set(address, permissions); counts.protects++;
}
function unmap(ctx, address) { host(ctx, "unmap", [address, 1]); ctx.pages.delete(address); ctx.permissions.delete(address); counts.unmaps++; }

function fresh(owner, selected, role) {
  const ordinal = ++counts.contexts, expected = Buffer.alloc(SIZE);
  expected.set(state(Array(8).fill(0), 0, 2)); expected.set(exit(1, 0, 0, 0, 0, 1), 56); expected.set(wordHelper(0), 100);
  const instance = new WebAssembly.Instance(engineModule, {}),
    ctx = { owner, ordinal, bank: selected, role, low: ordinal, high: HIGH, expected,
      pages: new Map(), permissions: new Map(), units: [], seed: null };
  lastContext = ctx;
  ctx.memory = instance.exports.memory; assert.ok(ctx.memory instanceof WebAssembly.Memory); refresh(ctx); ctx.api = {};
  const arities = { open: 3, close: 0, arena_ptr: 0, map: 3, unmap: 2, protect: 3, upload: 2,
    read8: 1, read32: 1, compile_entries: 2, compile_resident: 1, generation: 0,
    module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store8: 2, store_resident8: 6 };
  for (const [name, arity] of Object.entries(arities)) {
    ctx.api[name] = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof ctx.api[name], "function", name); assert.equal(ctx.api[name].length, arity, name);
  }
  assert.equal(ctx.api.open(7, ctx.low, ctx.high), 0); ctx.base = ctx.api.arena_ptr() >>> 0;
  refresh(ctx); assert.ok(ctx.base > 0 && ctx.base + SIZE <= ctx.bytes.length); check(ctx, "independent initialized arena");
  hostCalls.push({ event: ++events, context: ordinal, name: "open", args: [7, ctx.low, ctx.high], status: 0,
    arena_pointer: ctx.base, initialized_arena_sha256: hash(arena(ctx)) });
  host(ctx, "map", [PC, 1, 7]); ctx.pages.set(PC, Buffer.alloc(4096)); ctx.permissions.set(PC, 7); counts.maps++;
  upload(ctx, PC, selected.bytes, "authored full code bank");
  for (const address of [KEEP, DATA, SECOND, TOP]) map(ctx, address);
  if (role === "numeric") protect(ctx, PC, 5);
  contexts.push({ context: ordinal, owner, kind: selected.kind, role, key_low: ctx.low, key_high: ctx.high,
    page_limit: 7, arena_pointer: ctx.base, code_permissions: role === "numeric" ? 5 : 7 });
  return ctx;
}
function instantiate(ctx, bytes, file, binding, scans, pureTail = false) {
  assert.ok(bytes.length > 8 && bytes.length <= 65536);
  const module = new WebAssembly.Module(bytes),
    imports = (pureTail ? ["guard"] : ["guard", "read8", "store8"]).map(name =>
      ctx.owner === "replacement" ? name : name === "guard" ? "guard_resident" : name === "store8" ? "store_resident8" : name);
  assert.deepEqual(WebAssembly.Module.imports(module), [{ module: "env", name: "memory", kind: "memory" },
    ...imports.map(name => ({ module: "ring3", name, kind: "function" }))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: "run", kind: "function" }]);
  assert.ok(WebAssembly.validate(bytes));
  const abi = profile(bytes, ctx.owner, !pureTail, ctx, binding),
    child = new WebAssembly.Instance(module, { env: { memory: ctx.memory }, ring3: ctx.api });
  assert.equal(child.exports.run.length, 4);
  const unit = { ...binding, run: child.exports.run, bytes, file, scans, profile: abi,
    source_spans: scans.map(({ spec: [entry, length] }) => ({ entry, bytes: length,
      hex: ctx.pages.get(PC).subarray(entry - PC, entry - PC + length).toString("hex") })),
    origin: "actual-engine", exitVersion: pureTail ? 1 : 2 };
  ctx.units.push(unit); counts.modules++; counts.actual_modules++;
  const sourceInstructions = scans.reduce((sum, scan) => sum + scan.instructions, 0);
  counts.source_instructions += sourceInstructions;
  modules.push({ context: ctx.ordinal, owner: ctx.owner, kind: ctx.bank.kind, origin: "actual-engine", file,
    ...binding, imports, exit_version: unit.exitVersion, scans, sha256: hash(bytes), profile: abi,
    source_spans: unit.source_spans, source_instructions: sourceInstructions, bytes: bytes.length,
    key_low: ctx.low, key_high: ctx.high, code_page_sha256: hash(ctx.pages.get(PC)) });
  return unit;
}
function compile(ctx, scans, role, pureTail = false) {
  assert.ok(scans.length >= 1 && scans.length <= 8);
  const specs = scans.map(scan => scan.spec);
  assert.ok(scans.reduce((sum, scan) => sum + scan.instructions, 0) <= 64);
  request(ctx, words(ctx.owner === "replacement" ? specs.map(([entry]) => entry) : specs.flat()), "compiler request");
  let binding;
  if (ctx.owner === "replacement") {
    host(ctx, "compile_entries", [specs.length, 0]);
    binding = { generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0 };
    assert.ok(binding.generation > 0);
  } else {
    check(ctx, "before resident compile"); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
    binding = Object.fromEntries(["low", "high", "pointer", "length"].map((name, index) =>
      [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
    assert.ok(binding.low !== 0 || binding.high !== 0);
    ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER);
    check(ctx, "resident metadata only");
    hostCalls.push({ event: ++events, context: ctx.ordinal, name: "compile_resident", args: [specs.length], status: 0, binding });
  }
  refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
  const bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)), file = `actual-${ctx.ordinal}-${role}.wasm`;
  artifact(file, bytes); return instantiate(ctx, bytes, file, binding, scans, pureTail);
}
function seed(ctx, registers, pc, flags) {
  assert.ok((flags & 2) !== 0 && (flags & ~0xcd7) === 0);
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96);
  ctx.expected.set(wordHelper(0xdecafbad), 100); refresh(ctx).bytes.set(ctx.expected.subarray(0, 140), ctx.base);
  check(ctx, "explicit initial CPU/helper seed"); ctx.seed = ++counts.seeds;
  hostInputs.push({ event: ++events, seed: ctx.seed, context: ctx.ordinal, label: "initial CPU/helper seed",
    arena_offset: 0, bytes: 140, hex: ctx.expected.subarray(0, 140).toString("hex") });
}
function cancel(ctx, value) {
  ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);
  check(ctx, "explicit cancel field input"); counts.cancel_writes++;
  hostInputs.push({ event: ++events, context: ctx.ordinal, label: "explicit cancellation field",
    arena_offset: 96, bytes: 4, hex: words([value]).toString("hex") });
}
function generated(ctx, unit, args, label, status = 0, update = null, afterReturn = null) {
  const before = frame(ctx, `before ${label}`, unit);
  if (update !== null) update();
  activeCall = { ctx, unit, args, label };
  const actualStatus = unit.run(...args); activeCall.actualStatus = actualStatus;
  assert.equal(actualStatus, status, label);
  if (afterReturn !== null) afterReturn();
  check(ctx, label); const after = frame(ctx, label, unit); counts.generated_calls++;
  const row = { event: ++events, ordinal: runs.length + 1, seed: ctx.seed, context: ctx.ordinal,
    module: unit.file, label, operation: label, args, expected_status: status, status: actualStatus, before, after };
  runs.push(row); activeCall = null;
  return { run: row.ordinal, before, after };
}
function run(ctx, unit, budget, label, registers, pc, flags, reason = 1, retired = 1, helperBytes,
  detail = 0, fault = 0, access = 0) {
  const frames = generated(ctx, unit, [ctx.base, ctx.base + 56, budget, ctx.base + 96], label, 0, () => {
    ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired, detail, fault, access, unit.exitVersion), 56);
    if (helperBytes !== undefined) ctx.expected.set(helperBytes, 100);
  });
  counts.retired += retired;
  return frames;
}
function pages(ctx, label, addresses = [...ctx.pages.keys()]) {
  for (const address of addresses) {
    const expected = ctx.pages.get(address), permissions = ctx.permissions.get(address); assert.ok(expected);
    if (!(permissions & 1)) protect(ctx, address, permissions | 1);
    const actual = Buffer.alloc(4096);
    for (let offset = 0; offset < 4096; offset += 4) {
      check(ctx, "before diagnostic Read32"); assert.equal(ctx.api.read32(address + offset), 0);
      ctx.expected.set(wordHelper(expected.readUInt32LE(offset)), 100); check(ctx, "diagnostic Read32 helper only");
      actual.writeUInt32LE(refresh(ctx).view.getUint32(ctx.base + 120, true), offset); counts.host_read32++;
    }
    assert.deepEqual(actual, expected, "complete physical page and non-target neighbors");
    if (!(permissions & 1)) protect(ctx, address, permissions);
    const row = { file: "page-snapshots.bin", event: ++events, context: ctx.ordinal, label, address, permissions,
      offset: pageFrames.length * 4096, bytes: 4096, sha256: hash(actual) };
    pageFrames.push(actual); pageRows.push(row); counts.pages++; counts.page_bytes += 4096;
    hostCalls.push({ event: ++events, context: ctx.ordinal, name: "read32-page", args: [address, 1024], status: 0,
      page: row, final_helper_hex: ctx.expected.subarray(100, 140).toString("hex") });
  }
}
function addressRegisters(form, raw) {
  const registers = [...REG]; registers[1] = 0xa1b2c300 + raw; registers[3] = BYTE;
  if (form === 1) registers[1] = 0x4010;
  if (form === 2) registers[1] = 0x33334009;
  if (form === 3) { registers[1] = 0x40001009; registers[7] = 0xfffffffc; }
  if (form === 4) { registers[4] = BYTE; registers[1] = 0xa1b2c321; }
  return registers;
}
function physicalCPU(ctx) {
  const bytes = arena(ctx);
  return { registers: Array.from({ length: 8 }, (_, index) => bytes.readUInt32LE(16 + index * 4)),
    pc: bytes.readUInt32LE(48), flags: bytes.readUInt32LE(52) };
}
function modelByte(ctx, address) {
  assert.ok(address >= 0 && address < 4294967296);
  return ctx.pages.get(Math.floor(address / 4096) * 4096)[address % 4096];
}
function decodeTarget(ctx, cpu) {
  const page = ctx.pages.get(PC), start = cpu.pc - PC;
  assert.ok(start >= 0 && start + 2 <= page.length); assert.equal(page[start], 0xd2);
  const modrm = page[start + 1], mod = modrm >>> 6, operation = modrm >>> 3 & 7, rm = modrm & 7;
  assert.ok(mod !== 3 && [4, 5, 7].includes(operation)); assert.equal(operation, ctx.bank.extension >>> 3);
  let at = start + 2, address = 0;
  if (rm === 4) {
    const sib = page[at++], scale = 2 ** (sib >>> 6), index = sib >>> 3 & 7, base = sib & 7;
    if (index !== 4) address += cpu.registers[index] * scale;
    if (mod === 0 && base === 5) { address += page.readInt32LE(at); at += 4; }
    else address += cpu.registers[base];
  } else if (mod === 0 && rm === 5) { address += page.readInt32LE(at); at += 4; }
  else address += cpu.registers[rm];
  if (mod === 1) address += page.readInt8(at++);
  else if (mod === 2) { address += page.readInt32LE(at); at += 4; }
  const oldEcx = cpu.registers[1], raw = oldEcx % 256;
  return { pc: cpu.pc, address: address >>> 0, raw, old_ecx: oldEcx,
    raw_source: "physical current before-state full ECX low8; full old ECX independently participates in EA",
    next: PC + at, modrm, bytes: page.subarray(start, at).toString("hex"), code_page_sha256: hash(page),
    source_authority: "authored/current D2 bytes and physical fullGPRs joined to compiler spans and physical code pages" };
}
function observeByte(ctx, unit, address, label) {
  check(ctx, "before " + label); const cpuBefore = physicalCPU(ctx);
  activeCall = { ctx, unit, args: [address], label };
  const status = ctx.api.read8(address); activeCall.actualStatus = status; assert.equal(status, 0, label);
  const value = refresh(ctx).view.getUint32(ctx.base + 120, true);
  assert.equal(value, modelByte(ctx, address), "physical byte vs successful page/host-input history");
  ctx.expected.set(readHelper(value), 100); check(ctx, "diagnostic Read8 publishes helper only");
  assert.deepEqual(physicalCPU(ctx), cpuBefore, "diagnostic preserves complete CPU");
  const diagnostic = frame(ctx, label, unit), row = { event: ++events, context: ctx.ordinal, name: "read8",
    args: [address], status, observed_value: value, diagnostic, cpu_before: cpuBefore, helper_version: 2, width: 1 };
  hostCalls.push(row); counts.operand_reads++; counts.host_read8++; activeCall = null;
  return row;
}
function target(ctx, unit, registers, scan, address, flags, purpose, reason = 1) {
  const cpuBefore = physicalCPU(ctx), decoded = decodeTarget(ctx, cpuBefore);
  assert.deepEqual(cpuBefore, { registers: [...registers], pc: scan.target, flags });
  assert.equal(decoded.address, address); assert.equal(decoded.next, scan.next);
  const beforeDiagnostic = observeByte(ctx, unit, decoded.address, "physical pre-target Read8 operand"),
    oldByte = beforeDiagnostic.observed_value, arithmetic = shiftArithmetic(ctx.bank.kind, oldByte, decoded.raw, cpuBefore.flags),
    result = { value: arithmetic.result, flags: arithmetic.flags, q: decoded.raw % 32,
      cf: arithmetic.flags & 1, of: arithmetic.flags >>> 11 & 1 };
  patch(ctx, address, Buffer.from([result.value]));
  const frames = run(ctx, unit, 1, "runtime CL checked Store8 commits before consumers", registers,
    scan.next, result.flags, reason, 1, storeHelper());
  const afterDiagnostic = observeByte(ctx, unit, address, "physical post-target Read8 operand");
  assert.equal(afterDiagnostic.observed_value, result.value);
  counts.targets++; counts[result.q === 0 ? "q0_targets" : result.q === 1 ? "q1_targets" : "multi_targets"]++;
  counts[purpose + "_targets"]++;
  const row = { ordinal: targets.length + 1, context: ctx.ordinal, owner: ctx.owner, origin: "actual-engine",
    kind: ctx.bank.kind, module: unit.file, purpose, pc: scan.target, next_pc: scan.next,
    raw: decoded.raw, old_ecx: decoded.old_ecx, raw_source: decoded.raw_source, address, decoded,
    old_byte: oldByte, incoming_flags: flags, expected: result, registers: [...registers],
    observed_byte: afterDiagnostic.observed_value, physical_cpu_before: cpuBefore,
    before_diagnostic: beforeDiagnostic, after_diagnostic: afterDiagnostic, ...frames };
  targets.push(row); return result;
}
function consumers(ctx, unit, registers, scan, flags) {
  registers[0] = registers[0] - registers[0] % 256 + flags % 2;
  run(ctx, unit, 1, "SETB live CF", registers, scan.next + 3, flags); counts.setb++;
  registers[2] = registers[2] - registers[2] % 256 + Number(Boolean(flags & 0x800));
  run(ctx, unit, 1, "SETO live OF", registers, scan.next + 6, flags); counts.seto++;
  run(ctx, unit, 1, "following JMP", registers, COLD, flags); counts.jumps++;
}
function numeric(ctx) {
  pages(ctx, "initial declared pages");
  const unit = compile(ctx, ctx.bank.main, "same-module");
  const scan = ctx.bank.main[0];
  const arithmeticSets = [[0x81, RAW], [0x80, [1, 8, 31]], [0, [0, 1, 8]]];
  for (const [value, selectedRaw] of arithmeticSets) for (const raw of selectedRaw) for (const flags of FLAGS) {
    const registers = addressRegisters(0, raw);
    upload(ctx, BYTE, Buffer.from([value]), "one arithmetic byte input after unchanged compilation");
    seed(ctx, registers, scan.target, flags);
    const result = target(ctx, unit, registers, scan, BYTE, flags, "arithmetic");
    assert.equal(targets.at(-1).raw, raw);
    consumers(ctx, unit, registers, scan, result.flags);
  }
  pages(ctx, "same-module variable CL arithmetic full data page", [DATA]);
  for (const [index, address, raw] of [[1, 0x4010, 16], [2, 0x402d, 9], [3, 0x4010, 9], [4, BYTE, 33]]) {
    const selected = ctx.bank.main[index], registers = addressRegisters(index, raw);
    upload(ctx, address, Buffer.from([0x81]), "coupled full ECX EA byte input");
    seed(ctx, registers, selected.target, 0xcd7);
    const result = target(ctx, unit, registers, selected, address, 0xcd7, "ea");
    assert.equal(targets.at(-1).raw, raw); consumers(ctx, unit, registers, selected, result.flags);
  }
  pages(ctx, "coupled ECX EA complete data page", [DATA]);
  const endpoint = ctx.bank.main[5];
  for (const [address, raw] of [[0x4fff, 32], [0xffffffff, 1]]) {
    const registers = addressRegisters(0, raw); registers[7] = address;
    upload(ctx, address, Buffer.from([0x81]), "valid last byte operand");
    seed(ctx, registers, endpoint.target, 0xcd7);
    const result = target(ctx, unit, registers, endpoint, address, 0xcd7, "endpoint");
    consumers(ctx, unit, registers, endpoint, result.flags);
  }
  pages(ctx, "valid end-of-page and final address physical pages", [DATA, TOP]);
  const special = compile(ctx, [...ctx.bank.fault, ctx.bank.chain], "fault-chain"), chain = ctx.bank.chain;
  for (const flags of FLAGS) {
    const registers = addressRegisters(0, 32);
    upload(ctx, BYTE, Buffer.from([0x81]), "one initial chain operand");
    seed(ctx, registers, chain.entry, flags); registers[1] = registers[1] - registers[1] % 256 + 1;
    const firstProducer = run(ctx, special, 1, "real MOV CL1 before first shift", registers, chain.target, flags);
    counts.prefixes++; counts.movcl_producers++;
    const firstIndex = targets.length,
      firstScan = { ...chain, next: chain.first_next },
      first = target(ctx, special, registers, firstScan, BYTE, flags, "chain");
    registers[1] = registers[1] - registers[1] % 256 + 7;
    const secondProducer = run(ctx, special, 1, "real MOV CL7 between memory shifts", registers, chain.second, first.flags);
    counts.prefixes++; counts.movcl_producers++;
    const secondScan = { ...chain, target: chain.second, next: chain.tail },
      secondIndex = targets.length, second = target(ctx, special, registers, secondScan, BYTE, first.flags, "chain"),
      firstRow = targets[firstIndex], secondRow = targets[secondIndex];
    assert.equal(firstRow.raw, 1); assert.equal(secondRow.raw, 7);
    assert.equal(secondRow.old_byte, firstRow.observed_byte); assert.equal(secondRow.incoming_flags, first.flags);
    assert.equal(secondRow.old_ecx - secondRow.raw, firstRow.old_ecx - firstRow.raw);
    for (const index of [0, 2, 3, 4, 5, 6, 7])
      assert.equal(secondRow.physical_cpu_before.registers[index], firstRow.physical_cpu_before.registers[index]);
    chains.push({ context: ctx.ordinal, owner: ctx.owner, kind: ctx.bank.kind,
      first_producer: firstProducer, second_producer: secondProducer, first_target: firstIndex, second_target: secondIndex,
      initial_operand_uploads: 1, host_cpu_writes_between_targets: 0, guest_movcl_retired: 2,
      source: "real MOVCL1 / shift / MOVCL7 / shift; evolved physical byte/FLAGS, helper diagnostics only" });
    counts.chain_pairs++; consumers(ctx, special, registers, secondScan, second.flags);
  }
  pages(ctx, "real MOVCL chains complete data page", [DATA]);
  return special;
}
const faultShapes = {
  unmapped: { name: "unmapped-read", address: 0x8000, page: 0x8000, detail: 1, access: 1 },
  readonly: { name: "readonly-last-byte-store", address: 0x4fff, page: DATA, detail: 2, access: 2 },
};
function editCl(ctx, raw) {
  const before = physicalCPU(ctx), old = before.registers[1], current = old - old % 256 + raw;
  ctx.expected.writeUInt32LE(current, 20); refresh(ctx).view.setUint32(ctx.base + 20, current, true);
  check(ctx, "explicit legal input edit of CL only, not wholesale CPU repair");
  const after = physicalCPU(ctx), expected = { ...before, registers: [...before.registers] }; expected.registers[1] = current;
  assert.deepEqual(after, expected);
  const row = { event: ++events, context: ctx.ordinal, label: "explicit lowCL-only resume input",
    arena_offset: 20, bytes: 4, hex: words([current]).toString("hex"), old_ecx: old, current_ecx: current };
  hostInputs.push(row); counts.cl_input_edits++; return row;
}
function faultCases(ctx, unit) {
  const cases = [
    { shape: faultShapes.readonly, scan: ctx.bank.fault[0], flags: 2, mode: "changed-value" },
    { shape: faultShapes.readonly, scan: ctx.bank.fault[0], flags: 0xcd7, mode: "permission-only" },
    { shape: faultShapes.readonly, scan: ctx.bank.fault[1], flags: 0xcd7, mode: "permission-only" },
    { shape: faultShapes.unmapped, scan: ctx.bank.fault[2], flags: 0xcd7, mode: "map-only" },
    { shape: faultShapes.readonly, scan: ctx.bank.fault[0], flags: 0xcd7, mode: "CL-and-operand-input-edit" },
  ];
  for (const { shape, scan, flags, mode } of cases) {
    protect(ctx, DATA, 3);
    if (shape.access === 1) map(ctx, shape.page);
    upload(ctx, shape.address, Buffer.from([0x81]), "pre-fault byte operand");
    const preRestriction = observeByte(ctx, unit, shape.address, "physical pre-restriction operand"),
      rejected = shiftArithmetic(ctx.bank.kind, preRestriction.observed_value, scan.produced_raw, flags);
    if (scan.produced_raw === 1) assert.notEqual(rejected.flags, flags, "rejected Store8 candidate FLAGS are distinct");
    pages(ctx, "complete pre-restriction operand page", [shape.page]);
    if (shape.access === 1) unmap(ctx, shape.page); else protect(ctx, shape.page, 1);
    const registers = addressRegisters(0, 9); registers[3] = shape.address - 1;
    seed(ctx, registers, scan.entry, flags);
    registers[1] = registers[1] - registers[1] % 256 + scan.produced_raw;
    registers[3] = shape.address;
    const helper = shape.access === 1 ? readHelper(0, shape.detail, shape.address) : storeHelper(shape.detail, shape.address);
    const first = run(ctx, unit, 3, "real MOVCL+LEA then precise target fault with FLAGS retained",
      registers, scan.target, flags, 5, 2, helper, shape.detail, shape.address, shape.access);
    counts.prefixes += 2; counts.movcl_producers++;
    const decoded = decodeTarget(ctx, physicalCPU(ctx));
    assert.equal(decoded.raw, scan.produced_raw); assert.equal(decoded.address, shape.address);
    pages(ctx, "first target fault surviving complete data page", [DATA]);
    const retry = run(ctx, unit, 1, "unrepaired currentCL target retry retires zero", registers,
      scan.target, flags, 5, 0, helper, shape.detail, shape.address, shape.access);
    counts.fault_calls += 2; counts[shape.access === 1 ? "read_faults" : "write_faults"] += 2;
    pages(ctx, "retained target retry surviving complete data page", [DATA]);
    if (shape.access === 1) map(ctx, shape.page, null); else protect(ctx, shape.page, 3);
    let repairedByte = shape.access === 1 ? 0 : 0x81, inputEdit = null;
    if (mode === "changed-value") {
      repairedByte = 1; upload(ctx, shape.address, Buffer.from([repairedByte]), "changed-value input after permission repair");
      counts.changed_value_repairs++;
    } else if (mode === "CL-and-operand-input-edit") {
      repairedByte = 0x40; upload(ctx, shape.address, Buffer.from([repairedByte]), "explicit changed operand input");
      inputEdit = editCl(ctx, 2); registers[1] = inputEdit.current_ecx; counts.changed_value_repairs++;
      const current = shiftArithmetic(ctx.bank.kind, 0x40, 2, flags);
      for (const [staleByte, staleRaw] of [[0x81, 1], [0x40, 1], [0x81, 2]])
        assert.notEqual(current.result, shiftArithmetic(ctx.bank.kind, staleByte, staleRaw, flags).result,
          "current byte40/CL2 distinguishes stale operand and stale CL for every kind");
    } else counts[mode === "map-only" ? "map_only_repairs" : "permission_only_repairs"]++;
    const result = target(ctx, unit, registers, scan, shape.address, flags, "repaired");
    assert.equal(targets.at(-1).old_byte, repairedByte);
    assert.equal(targets.at(-1).raw, inputEdit === null ? scan.produced_raw : 2);
    consumers(ctx, unit, registers, scan, result.flags); counts.repairs++;
    pages(ctx, "repaired complete operand page and neighbors", [shape.page]);
    faults.push({ context: ctx.ordinal, owner: ctx.owner, kind: ctx.bank.kind, shape, flags,
      width: 1, helper_version: shape.access === 1 ? 2 : 3, pre_restriction: preRestriction,
      first, retry, physical_fault_target: decoded, rejected_candidate: rejected,
      repair_mode: mode, repaired_input_byte: repairedByte, input_edit: inputEdit,
      repair_target: targets.length - 1, repair_expected: result,
      cpu_writes_after_first_fault: inputEdit === null ? 0 : 1, prefix_replays: 0,
      retention: inputEdit === null ? "faulted fullCPU retained; mapped bytes survive, map-only rereads fresh zero"
        : "explicit lowCL-only input; highECX/EIP/otherGPR/FLAGS/helper retained; operand40 is recorded input" });
    if (shape.access === 1) unmap(ctx, shape.page);
  }
  pages(ctx, "complete post-repair declared pages");
}


function ownerChecks(ctx, unit) {
  if (ctx.owner === "resident") {
    assert.equal(unit.high, 0); assert.ok(unit.low >= 1 && unit.low <= 2,
      "fresh selected contexts allocate at most two resident units before this guard");
  }
  for (const wrong of ["key", "identity"]) {
    const low = ctx.low ^ Number(wrong === "key"),
      args = ctx.owner === "replacement"
        ? [low, ctx.high, unit.generation + Number(wrong === "identity"), ctx.base, ctx.base + 56, ctx.base + 96]
        : [low, ctx.high, unit.low, wrong === "identity" ? 0x80000000 : unit.high, ctx.base, ctx.base + 56, ctx.base + 96];
    host(ctx, ctx.owner === "replacement" ? "guard" : "guard_resident", args, 3); counts.direct_guards++;
  }
}
function controls(ctx, unit) {
  const cpu = physicalCPU(ctx); assert.equal(cpu.pc, COLD); assert.equal(ctx.expected.readUInt32LE(96), 0);
  run(ctx, unit, 0, "zero budget before cold guest access", cpu.registers, COLD, cpu.flags, 1, 0);
  cancel(ctx, 1); run(ctx, unit, 1, "cancel before cold guest access", cpu.registers, COLD, cpu.flags, 2, 0);
  generated(ctx, unit, [0xffffffff, 0xffffffff, 0, 0xffffffff], "malformed pointers before cancel and budget", 1);
  cancel(ctx, 0); run(ctx, unit, 1, "cold entry need code", cpu.registers, COLD, cpu.flags, 3, 0);
}


function strictNeighbor(ctx, unit) {
  protect(ctx, KEEP, 7);
  const probe = Buffer.from([0x66, 0xd2, 0x05 | ctx.bank.extension, 0x10, 0x40, 0, 0, 0xeb, 0, 0x0f, 0x0b]);
  upload(ctx, KEEP, probe, "strict operand-size-prefix neighbor of new memory CL");
  request(ctx, words(ctx.owner === "replacement" ? [KEEP] : [KEEP, 9]), "strict compiler request");
  const before = arena(ctx), generation = ctx.api.generation(),
    pointer = ctx.api.module_ptr() >>> 0, length = ctx.api.module_len() >>> 0,
    priorBytes = Buffer.from(refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length));
  host(ctx, ctx.owner === "replacement" ? "compile_entries" : "compile_resident",
    ctx.owner === "replacement" ? [1, 0] : [1], 10);
  assert.deepEqual(arena(ctx), before, "failed compile retains whole current arena");
  assert.equal(ctx.api.generation(), generation); assert.equal(ctx.api.module_ptr() >>> 0, pointer);
  assert.equal(ctx.api.module_len() >>> 0, length);
  assert.deepEqual(Buffer.from(refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length)), priorBytes,
    "failed compile retains current live allocation");
  counts.compiler_refusals++;
  const cpu = physicalCPU(ctx);
  const survival = run(ctx, unit, 0, "strict failed compile preserves prior owner publication",
    cpu.registers, cpu.pc, cpu.flags, 1, 0);
  counts.rejection_survivals++;
  refusals.push({ context: ctx.ordinal, owner: ctx.owner, module: unit.file, kind: ctx.bank.kind,
    probe_hex: probe.toString("hex"), status: 10, prior_generation: generation,
    prior_pointer: pointer, prior_length: length, prior_module_sha256: hash(priorBytes), survival });
  upload(ctx, KEEP, pattern.subarray(0, probe.length), "restore exact strict-prefix probe extent");
  protect(ctx, KEEP, 3); pages(ctx, "strict code page restored", [KEEP]);
}

function close(ctx) {
  if (ctx.expected.readUInt32LE(96) !== 1) cancel(ctx, 1);
  host(ctx, "close", []);
  for (const unit of ctx.units) {
    generated(ctx, unit, [0xffffffff, 0xffffffff, 0, 0xffffffff], "closed owner before malformed cancelled zero-budget arguments", 5);
    counts.closed_calls++;
  }
}


function smcCase(owner, selected, scan) {
  const ctx = fresh(owner, selected, "smc");
  pages(ctx, "initial declared SMC pages");
  const unit = compile(ctx, [scan], "smc"), address = scan.entry + 1,
    registers = addressRegisters(0, scan.raw), flags = 2;
  registers[7] = address; seed(ctx, registers, scan.entry, flags); ownerChecks(ctx, unit);
  registers[0] = registers[0] - registers[0] % 256 + scan.value;
  const prefix = run(ctx, unit, 1, "MOV AL before consumed-byte Store8", registers, scan.target, flags);
  counts.prefixes++;
  const sourceBefore = Buffer.from(ctx.pages.get(PC)), allocationBefore = Buffer.from(refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.equal(hash(allocationBefore), hash(unit.bytes), "current retained allocation matches original physical module");
  const arithmetic = target(ctx, unit, registers, scan, address, flags, "code", 6),
    row = targets.at(-1), committed = physicalCPU(ctx), once = arithmetic.value,
    twice = shiftArithmetic(selected.kind, once, row.raw, arithmetic.flags).result;
  assert.equal(committed.pc, scan.next); assert.equal(committed.flags, arithmetic.flags);
  assert.deepEqual(committed.registers, registers);
  const same = once === scan.value;
  counts.code_stores++; counts[same ? "same_value_code_stores" : "changing_code_stores"]++;
  if (row.raw === 1) assert.notEqual(twice, once, "selected changing case makes replay observable");
  pages(ctx, "physical consumed source page after checked Store8", [PC]);
  cancel(ctx, 1);
  let retainedAfterHash = null;
  const stale = generated(ctx, unit, [0xffffffff, 0xffffffff, 0, 0xffffffff],
    "stale owner before malformed cancelled zero-budget arguments", 4, null, () => {
      const after = Buffer.from(refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length));
      retainedAfterHash = hash(after);
      assert.deepEqual(after, allocationBefore,
        "immediate post-stale live old allocation equality before arena snapshot/getter/compile");
    });
  counts.stale_calls++; assert.deepEqual(physicalCPU(ctx), committed);
  cancel(ctx, 0);
  const current = Buffer.from(ctx.pages.get(PC));
  artifact("current-" + ctx.ordinal + ".x86", current);
  const tailBytes = current.subarray(scan.next - PC, scan.next - PC + 16);
  artifact("tail-" + ctx.ordinal + ".x86", tailBytes);
  const tailScan = { entry: scan.next, target: scan.next, next: COLD, instructions: 4, spec: [scan.next, 16] },
    tail = compile(ctx, [tailScan], "tail", true);
  assert.deepEqual(physicalCPU(ctx), committed, "compile changes transfer/receipt only");
  assert.equal(ctx.expected.readUInt32LE(96), 0);
  assert.equal(modelByte(ctx, address), once);
  registers[0] = registers[0] - registers[0] % 256 + arithmetic.flags % 2;
  registers[2] = registers[2] - registers[2] % 256 + Number(Boolean(arithmetic.flags & 0x800));
  registers[3] = 0xdeadbeef;
  const continuation = run(ctx, tail, 5, "fresh guard-only current tail with no shift replay",
    registers, COLD, arithmetic.flags, 3, 4);
  counts.setb++; counts.seto++; counts.canaries++; counts.jumps++;
  assert.equal(modelByte(ctx, address), once);
  pages(ctx, "physical source page after guard-only continuation", [PC]);
  codeStores.push({ context: ctx.ordinal, owner, kind: selected.kind, address,
    raw: row.raw, q: row.raw % 32, old_byte: scan.value, committed_byte: once,
    same_value_store: same, prefix, target: row.ordinal - 1, stale, continuation,
    old_module: unit.file, tail_module: tail.file, code_before_sha256: hash(sourceBefore),
    code_after_sha256: hash(current), original_allocation_sha256: hash(allocationBefore),
    retained_allocation_after_sha256: retainedAfterHash, current_source_artifact: "current-" + ctx.ordinal + ".x86",
    tail_artifact: "tail-" + ctx.ordinal + ".x86", tail_entry: scan.next, tail_bytes: 16,
    exit_reason: 6, retired_at_store: 1, no_replay_byte: once, replay_counterfactual_byte: twice,
    cpu_reseeds_after_commit: 0, original_shift_replays: 0,
    retention_authority: "producer compares live full allocation immediately after stale call; checker can join original module and receipt hashes, no independently saved after-allocation bytes",
    continuation_authority: "physical committed fullCPU/ECX/FLAGS retained through compiler request/resident receipt; fresh guard-only 16-byte SETB/SETO/MOV/JMP tail",
  });
  close(ctx);
}


for (const selected of banks) for (const owner of ["replacement", "resident"]) {
  const ctx = fresh(owner, selected, "numeric"), unit = numeric(ctx);
  faultCases(ctx, unit); ownerChecks(ctx, unit); controls(ctx, unit);
  strictNeighbor(ctx, unit); close(ctx);
  for (const scan of selected.smc) smcCase(owner, selected, scan);
}
artifact("arena-snapshots.bin", Buffer.concat(arenaFrames));
artifact("page-snapshots.bin", Buffer.concat(pageFrames));
counts.host_inputs = hostInputs.length; counts.host_calls = hostCalls.length;
counts.files = artifacts.length + 1;
assert.deepEqual(counts, plan, "literal source-derived finite schedule census");
assert.equal(targets.length, 300); assert.equal(runs.length, 1326); assert.equal(faults.length, 30);
assert.equal(chains.length, 12); assert.equal(codeStores.length, 18);
assert.equal(arenaRows.length, 3282); assert.equal(pageRows.length, 342);
assert.deepEqual(readdirSync(output).sort(), artifacts.map(row => row.path).sort(), "exact pre-result physical inventory");
const sourcePinsAfter = pinSources(); assert.deepEqual(sourcePinsAfter, sourcePins, "all selected 41 source pins unchanged during actual");
assert.equal(hash(readFileSync(enginePath)), engineSha256, "same current engine before and after");
const result = {
  profile: "prefix-free flat32 memory byte D2 /4,/5,/7 runtime CL shifts, finite current-value/count and checked-store campaign",
  command: { argv: process.argv, node: process.version, platform: process.platform, architecture: process.arch,
    ring3_engine_wasm: process.env.RING3_ENGINE_WASM ?? null },
  engine: { path: enginePath, bytes: engineBytes.length, sha256: engineSha256, before_sha256: engineSha256,
    after_sha256: engineSha256, saved: "engine.wasm", caller_sha256: process.env.RING3_ENGINE_WASM_SHA256 ?? null },
  primary: { source: "target/p2-memory-binary-spec/253667-093-sdm-vol-2b.txt", full_sha256: hash(fullText),
    lines: [30072, 30275], chapter: savedChapter },
  source_pins: sourcePins, source_pins_after: sourcePinsAfter,
  plan, counts, contexts, modules, targets, runs, faults, chains, code_stores: codeStores,
  mutations, refusals, host_inputs: hostInputs, host_calls: hostCalls,
  raw_arena_records: arenaRows, page_records: pageRows, artifacts,
  banks: banks.map(({ kind, extension, main, fault, chain, smc, packed, saved: physical }) =>
    ({ kind, extension, main, fault, chain, smc, packed, artifact: physical })),
  scalar_anchors: ANCHORS,
  corpus: { target_executions: 300, actual_engine_targets: 300, native_guest_targets: 0,
    arithmetic: { same_module_per_kind_owner: true, primary_byte: 0x81, primary_raw: RAW,
      opposite_byte: 0x80, opposite_raw: [1, 8, 31], zero_byte: 0, zero_raw: [0, 1, 8], flags: FLAGS,
      rows: "6 kind-owner contexts times (10+3+3) raw-byte recipes times 2 FLAGS =192" },
    ea: { rows: 24, complete_old_ecx_inputs: [0x4010, 0x33334009, 0x40001009, 0xa1b2c321],
      effective_addresses: [0x4010, 0x402d, 0x4010, 0x4010], count_sources: [16, 9, 9, 33] },
    endpoints: { rows: 12, pairs: [[0x4fff, 32], [0xffffffff, 1]], width: 1 },
    repairs: { rows: 30, failed_attempts: 60, retained_cpu_repairs: 24, explicit_cl_operand_repairs: 6,
      pre_fault_byte: 0x81, changed_value_byte: 1, explicit_input_byte: 0x40, explicit_input_cl: 2,
      clean_readonly_changed_value: 6, dirty_permission_only: 12, dirty_map_only: 6,
      cpu_seed_per_fault_case: 1, prefix_replays_after_fault: 0 },
    chains: { pairs: 12, targets: 24, initial_byte: 0x81, real_guest_cl_inputs: [1, 7],
      cpu_seeds_per_pair: 1, host_cpu_input_writes_between_targets: 0 },
    smc: { rows: 18, pairs: [[32, 0x81], [8, 0], [1, 0x81]], same_value: 12, changing: 6,
      consumed_byte: "MOV AL immediate before D2 memory target", fresh_tail_bytes: 16 },
    valid_flags_inputs: FLAGS, pure_memory_module_admission: false,
    q_partitions: { q0: 54, q1: 90, multi: 156 },
  },
  caps: { page_limit: 7, blocks_per_module_maximum: 6, instructions_per_module_maximum: 25,
    emitted_module_bytes_maximum: 65536, compile_entries_resident_maximum: 8 },
  evidence_limits: {
    arithmetic: "finite300 runtime target executions only; scalar anchors exercise selected policy, not every raw byte/FLAGS or every ISA profile",
    count_and_ea: "all target count/opcode/EA metadata derives from physical full before-state and current D2 source; ordinary arithmetic inputs explicitly seed CPU after same-module compile",
    repair: "map-only repairs reread fresh zero; permission-only retains CPU/operand; clean changed-value upload and six lowCL-only/byte40 edits are explicit host inputs, not untouched CPU claims",
    journal: "every generated callback has one physical before/after full4236 arena; each Read8 has an additional full frame; summarized page Read32 calls are not individually framed; binding/getter nonmutating calls are source-described, not individually journaled",
    pages: "all declared physical 4096-byte snapshots and mapped neighbors only; no full physical RAM, MMIO, crossing-byte or byte-overflow campaign",
    module: "profile/import/type/local/guard-prefix and physical hash checks; remaining compiled body is not independently semantically certified",
    stale: "live producer old allocation equality happens immediately post-stale before snapshot/getter/compile; independently saved after-allocation bytes absent; no dangling allocation dereference after successful replacement or close",
    continuation: "fresh current 16-byte guard-only tail preserves committed state, legal transfer/resident receipt changes, no CPU seed or original target replay",
    ownership: "actual engine-Wasm compiler and generated modules for replacement/resident only; native tests validate modules without guest arithmetic execution",
    scope: "no browser/SDK/game/full CI/performance/manual-latest/download/clean-bootstrap claim; current pinned Intel093 chapter reused",
  },
};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2) + "\n", { flag: "wx" });
assert.deepEqual(readdirSync(output).sort(), [...artifacts.map(row => row.path), "result.json"].sort());
console.log(JSON.stringify({ counts, engine_sha256: engineSha256, result: join(output, "result.json") }));
