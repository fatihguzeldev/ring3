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
const RAW = [0, 1, 2, 7, 8, 9, 31, 32, 33, 255],
  VALUES = [0, 1, 3, 0x7f, 0x80, 0x81, 0xff], FLAGS = [2, 0xcd7],
  REG = [0x1234807f, 0xa1b2c378, 0x34560f10, 0x4567ff00,
    0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef],
  ECX = [0xa1b2c300, 0xa1b2c301, 0xa1b2c31f, 0xa1b2c320, 0xa1b2c3ff];
const pattern = Buffer.from(Array.from({ length: 4096 }, (_, index) => index % 251 + 1));
const plan = {
  contexts: 42, modules: 114, native_modules: 0, actual_modules: 114,
  source_instructions: 1056, seeds: 1092, arithmetic_targets: 840, ea_targets: 96,
  endpoint_targets: 36, repaired_targets: 72, chain_targets: 24, code_targets: 36,
  targets: 1104, q0_targets: 240, q1_targets: 240, multi_targets: 624,
  chain_pairs: 12, producers: 12, setb: 1092, seto: 1092, jumps: 1092,
  canaries: 36, prefixes: 108, fault_calls: 144, read_faults: 72, write_faults: 72,
  repairs: 72, map_only_repairs: 18, permission_only_repairs: 18, changed_value_repairs: 36,
  code_stores: 36, same_value_code_stores: 24, changing_code_stores: 12,
  count_mutations: 6, stale_calls: 42, closed_calls: 114, direct_guards: 84,
  compiler_refusals: 6, rejection_survivals: 6,
  generated_calls: 4686, retired: 4536, maps: 282, unmaps: 72, protects: 174,
  host_uploads: 1362, host_uploaded_bytes: 1008864, operand_reads: 2280,
  host_read8: 2280, host_read32: 663552, host_requests: 1482, cancel_writes: 138,
  host_inputs: 4074, host_calls: 5106, pages: 648, page_bytes: 2654208,
  raw_arenas: 11652, raw_arena_bytes: 49357872, files: 206,
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
  artifacts.push(row);
  return row;
}
assert.deepEqual(readdirSync(output).sort(), ["sar", "shl", "shr"].map(kind => `memory-${kind}.x86`),
  "exclusive three wrapper-authored x86 bank roster");
artifact("engine.wasm", engineBytes);
const sourcePaths = [
  "engine/Cargo.toml", "engine/src/abi/memory_helper.rs", "engine/src/abi/x86/state.rs",
  "engine/src/cpu/dbt/region.rs", "engine/src/cpu/dbt/wasm/integer.rs",
  "engine/src/cpu/dbt/wasm/locals.rs", "engine/src/cpu/dbt/wasm/memory.rs",
  "engine/src/cpu/dbt/wasm/memory/byte_store.rs", "engine/src/cpu/dbt/wasm/memory/narrow.rs",
  "engine/src/cpu/x86/decode/integer.rs", "engine/src/cpu/x86/decode/profile.rs", "engine/src/cpu/x86/ir.rs",
  "engine/tests/cpu_byte_arithmetic.rs", "engine/tests/cpu_byte_carry.rs", "engine/tests/cpu_byte_shift.rs",
  "engine/tests/cpu_byte_unary.rs", "engine/tests/cpu_memory_byte_arithmetic.rs",
  "engine/tests/cpu_memory_byte_carry.rs", "engine/tests/cpu_memory_byte_logical.rs",
  "engine/tests/cpu_memory_byte_shift.rs", "engine/tests/cpu_memory_byte_shift_immediate_one.rs",
  "engine/tests/cpu_memory_byte_shift_immediate_one_wasm.rs", "engine/tests/cpu_memory_byte_unary.rs",
  "engine/tests/cpu_memory_carry_rotate_cl32.rs", "engine/tests/cpu_memory_carry_rotate_cl_wasm.rs",
  "engine/tests/cpu_memory_set_byte.rs", "engine/tests/cpu_register_memory_byte_arithmetic.rs",
  "engine/tests/cpu_register_memory_byte_carry.rs", "engine/tests/cpu_register_memory_byte_compare.rs",
  "engine/tests/cpu_register_memory_byte_logical.rs", "engine/tests/cpu_shift.rs",
  "engine/tests/dbt_extensions.rs", "engine/tests/dbt_memory.rs", "engine/tests/dbt_memory_extensions.rs",
  "engine/tests/dbt_resident.rs", "engine/tests/dbt_unary.rs",
  "engine/tests/fixtures/p2-byte-shift/run.mjs",
  "engine/tests/fixtures/p2-memory-byte-shift-immediate-one/run.mjs",
  "engine/tests/fixtures/p2-memory-carry-rotate-cl32/run.mjs", "engine/tests/process_resident_control.rs",
  "engine/tests/fixtures/support/engine.mjs", "engine/tests/cpu_memory_byte_shift_immediate.rs",
  "engine/tests/cpu_memory_byte_shift_immediate_wasm.rs",
  "engine/tests/fixtures/p2-memory-byte-shift-immediate/run.mjs",
];
assert.equal(sourcePaths.length, 44);
assert.equal(new Set(sourcePaths).size, 44);
const pinSources = () => Object.fromEntries(sourcePaths.map(path => {
  const bytes = readFileSync(join(root, path));
  return [path, { bytes: bytes.length, sha256: hash(bytes) }];
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
const PRODUCERS = [[0xffffffff, 0, 0x457], [0x7fffffff, 0x80000000, 0xc96]];
function addOne(value, flags) {
  const wide = value + 1, result = wide % 2 ** 32, signed = value < 2 ** 31 ? value : value - 2 ** 32;
  const even = (result % 256).toString(2).replaceAll("0", "").length % 2 === 0;
  return { value: result, flags: 2 + (flags & 0x400) + Number(wide >= 2 ** 32) + Number(even) * 4
    + Number(value % 16 + 1 >= 16) * 0x10 + Number(result === 0) * 0x40 + Number(result >= 2 ** 31) * 0x80
    + Number(signed + 1 > 2 ** 31 - 1) * 0x800 };
}
for (const [before, value, flags] of PRODUCERS) assert.deepEqual(addOne(before, 0xcd7), { value, flags });

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
  const bytes = Buffer.alloc(4096, 0xcc), groups = [[], []], ea = [], fault = [], smc = [];
  let at = 0;
  function append(form, raw, formIndex, prefix = 0, canary = false) {
    const entry = PC + at, target = entry + prefix, next = entry + form.length;
    bytes.set(form, at); at += form.length;
    bytes.set([0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2], at); at += 6;
    if (canary) { bytes.set([0xbb, 0xef, 0xbe, 0xad, 0xde], at); at += 5; }
    bytes[at] = 0xe9; bytes.writeInt32LE(COLD - (PC + at + 5), at + 1); at += 5;
    return { entry, target, next, raw, form: formIndex, canary,
      spec: [entry, PC + at - entry], instructions: 4 + Number(prefix !== 0) + Number(canary) };
  }
  for (const [index, raw] of RAW.entries())
    groups[Math.floor(index / 5)].push(append([0xc0, 5 | extension, 0x10, 0x40, 0, 0, raw], raw, null));
  for (const [index, form] of [
    [0xc0, extension, 0], [0xc0, 0x41 | extension, 0xf0, 1],
    [0xc0, 0x44 | extension, 0x4a, 0x11, 7],
    [0xc0, 0x84 | extension, 0xf3, 0xe0, 0xff, 0xff, 0xff, 8],
    [0xc0, 0x04 | extension, 0x24, 9], [0xc0, 0x45 | extension, 0, 31],
    [0xc0, 0x06 | extension, 32], [0xc0, 0x87 | extension, 0, 1, 0, 0, 255],
  ].entries()) ea.push(append(form, form.at(-1), index));
  for (const raw of [0, 1, 8]) fault.push(append([0x8d, 0x5b, 1, 0xc0, 0x07 | extension, raw], raw, 8, 3));
  const chain = append([0x83, 0xc3, 1, 0xc0, 0x06 | extension, 1, 0xc0, 0x06 | extension, 7], 1, 6, 3);
  chain.second = chain.target + 3; chain.tail = chain.next; chain.next = chain.second;
  chain.instructions = 6;
  assert.equal(at, 377); assert.deepEqual(groups.map(rows => rows.length), [5, 5]);
  for (const raw of [0, 1, 8]) for (const value of [0, 0x81]) {
    const start = 0xe00 + smc.length * 32, entry = PC + start;
    const form = [0xb0, value, 0xc0, 0x07 | extension, raw,
      0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0xbb, 0xef, 0xbe, 0xad, 0xde, 0xe9];
    bytes.set(form, start); bytes.writeInt32LE(COLD - (entry + 21), start + 17);
    smc.push({ entry, target: entry + 2, next: entry + 5, raw, value, form: 8,
      canary: true, spec: [entry, 21], instructions: 6 });
  }
  bytes.set([0x0f, 0x0b], COLD - PC);
  assert.deepEqual(bytes, readFileSync(join(output, `memory-${kind}.x86`)), "independent Rust/JS bank bytes");
  const saved = artifact(`memory-${kind}.x86`, bytes, false);
  return { kind, extension, bytes, groups, ea, fault, chain, smc, packed: at, saved };
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
function addressRegisters(form, address, raw) {
  const registers = [...REG]; registers[1] = ECX[RAW.indexOf(raw) % ECX.length] ?? ECX[0];
  if (form === 0) registers[0] = address;
  if (form === 1) registers[1] = address + 16;
  if (form === 2) { registers[1] = 0x12345603; registers[2] = address - 2 * registers[1] - 17; }
  if (form === 3) { registers[6] = 2; registers[3] = address + 32 - 16; }
  if (form === 4) registers[4] = address;
  if (form === 5) registers[5] = address;
  if (form === 6) registers[6] = address;
  if (form === 7) registers[7] = address - 0x100;
  if (form === 8) registers[7] = address;
  return registers.map(value => value >>> 0);
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
  assert.ok(start >= 0 && start + 3 <= page.length); assert.equal(page[start], 0xc0);
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
  assert.ok(at < page.length); const immediateAddress = PC + at, raw = page[at++];
  return { pc: cpu.pc, address: address >>> 0, raw, immediate_address: immediateAddress,
    old_ecx: cpu.registers[1], raw_source: "physical current encoded imm8; full old ECX belongs only to EA/GPR state",
    next: PC + at, modrm, bytes: page.subarray(start, at).toString("hex"), code_page_sha256: hash(page),
    source_authority: "authored/current code bytes joined to compiler spans and full physical code-page checkpoints" };
}
function observeByte(ctx, unit, address, label) {
  check(ctx, `before ${label}`); const cpuBefore = physicalCPU(ctx);
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
  assert.equal(decoded.address, address); assert.equal(decoded.raw, scan.raw); assert.equal(decoded.next, scan.next);
  const beforeDiagnostic = observeByte(ctx, unit, decoded.address, "physical pre-target Read8 operand"),
    oldByte = beforeDiagnostic.observed_value, arithmetic = shiftArithmetic(ctx.bank.kind, oldByte, decoded.raw, cpuBefore.flags),
    result = { value: arithmetic.result, flags: arithmetic.flags, q: decoded.raw % 32,
      cf: arithmetic.flags & 1, of: arithmetic.flags >>> 11 & 1 };
  patch(ctx, address, Buffer.from([result.value]));
  const frames = run(ctx, unit, 1, "checked byte shift Store8 commits before consumers", registers,
    scan.next, result.flags, reason, 1, storeHelper());
  const afterDiagnostic = observeByte(ctx, unit, address, "physical post-target Read8 operand");
  assert.equal(afterDiagnostic.observed_value, result.value);
  counts.targets++; counts[result.q === 0 ? "q0_targets" : result.q === 1 ? "q1_targets" : "multi_targets"]++;
  counts[`${purpose}_targets`]++;
  const row = { context: ctx.ordinal, owner: ctx.owner, origin: "actual-engine", kind: ctx.bank.kind,
    module: unit.file, purpose, pc: scan.target, next_pc: scan.next, raw: decoded.raw, old_ecx: decoded.old_ecx,
    raw_source: decoded.raw_source, address, decoded, old_byte: oldByte, incoming_flags: flags, expected: result,
    registers: [...registers], observed_byte: afterDiagnostic.observed_value, physical_cpu_before: cpuBefore,
    before_diagnostic: beforeDiagnostic, after_diagnostic: afterDiagnostic, ...frames };
  targets.push(row);
  return result;
}
function consumers(ctx, unit, registers, scan, flags) {
  registers[0] = registers[0] - registers[0] % 256 + flags % 2;
  run(ctx, unit, 1, "SETB live CF", registers, scan.next + 3, flags); counts.setb++;
  registers[2] = registers[2] - registers[2] % 256 + Number(Boolean(flags & 0x800));
  run(ctx, unit, 1, "SETO live OF", registers, scan.next + 6, flags); counts.seto++;
  if (scan.canary) { registers[3] = 0xdeadbeef;
    run(ctx, unit, 1, "current MOV canary", registers, scan.next + 11, flags); counts.canaries++; }
  run(ctx, unit, 1, "following JMP", registers, COLD, flags); counts.jumps++;
}

function countMutation(ctx, unit, registers, scan, result) {
  assert.equal(scan.raw, 0); assert.equal(physicalCPU(ctx).pc, scan.next);
  const countAddress = scan.next - 1;
  assert.equal(modelByte(ctx, countAddress), 0);
  const oldAllocation = Buffer.from(refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.deepEqual(oldAllocation, unit.bytes);
  protect(ctx, PC, 7); upload(ctx, countAddress, Buffer.from([32]), "consumed count0 to32 equivalent masked count");
  pages(ctx, "physical mutated count source page", [PC]);
  cancel(ctx, 1); let afterAllocation;
  const stale = generated(ctx, unit, [0xffffffff, 0xffffffff, 0, 0xffffffff],
    "count-mutated stale owner before malformed pointers cancellation and zero budget", 4, null, () => {
      afterAllocation = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length));
      assert.deepEqual(afterAllocation, oldAllocation, "first synchronous post-stale observation is whole old allocation");
    });
  counts.stale_calls++; cancel(ctx, 0);
  const code = Buffer.from(ctx.pages.get(PC)), savedCurrent = artifact(`count-current-${ctx.ordinal}.x86`, code),
    savedTail = artifact(`count-tail-${ctx.ordinal}.x86`, code.subarray(scan.next - PC, scan.next - PC + 11)),
    tail = { ...scan, entry: scan.next, spec: [scan.next, 11], instructions: 3 },
    current = compile(ctx, [tail], "count-tail", true);
  consumers(ctx, current, registers, scan, result.flags);
  mutations.push({ context: ctx.ordinal, owner: ctx.owner, kind: ctx.bank.kind, address: countAddress,
    old_byte: 0, new_byte: 32, upload_bytes: 1, consumed_target: targets.length - 1, stale,
    old_module_sha256_before: hash(oldAllocation), old_module_sha256_after: hash(afterAllocation),
    saved_current: savedCurrent, saved_tail: savedTail, continuation_module: current.file,
    resume_pc: scan.next, cpu_writes_during_continuation: 0, replayed_memory_instructions: 0,
    allocation_ceiling: "after hash is a receipt of first synchronous live comparison, not separately saved full after bytes" });
  counts.count_mutations++;
  upload(ctx, countAddress, Buffer.from([0]), "restore count0 after completed current tail");
  protect(ctx, PC, 5); pages(ctx, "physical restored count source page", [PC]);
  return compile(ctx, ctx.bank.groups[0], "g0-restored");
}
function numeric(ctx) {
  pages(ctx, "initial declared pages");
  for (const [group, scans] of ctx.bank.groups.entries()) {
    let unit = compile(ctx, scans, `g${group}`);
    for (const scan of scans) for (const value of VALUES) for (const flags of FLAGS) {
      const registers = addressRegisters(null, BYTE, scan.raw);
      upload(ctx, BYTE, Buffer.from([value]), "one initial arithmetic byte operand"); seed(ctx, registers, scan.target, flags);
      const result = target(ctx, unit, registers, scan, BYTE, flags, "arithmetic");
      if (group === 0 && scan.raw === 0 && value === 0xff && flags === 0xcd7)
        unit = countMutation(ctx, unit, registers, scan, result);
      else consumers(ctx, unit, registers, scan, result.flags);
    }
    pages(ctx, `arithmetic group${group} complete data page`, [DATA]);
  }
  const eaUnit = compile(ctx, ctx.bank.ea, "ea");
  for (const scan of ctx.bank.ea) for (const flags of FLAGS) {
    const registers = addressRegisters(scan.form, BYTE, scan.raw);
    upload(ctx, BYTE, Buffer.from([0x81]), "standalone EA initial byte"); seed(ctx, registers, scan.target, flags);
    const result = target(ctx, eaUnit, registers, scan, BYTE, flags, "ea");
    consumers(ctx, eaUnit, registers, scan, result.flags);
  }
  pages(ctx, "standalone EA full data page", [DATA]);
  const chainUnit = compile(ctx, [ctx.bank.chain], "chain");
  for (const [oldEbx, nextEbx, producedFlags] of PRODUCERS) {
    const scan = ctx.bank.chain, registers = addressRegisters(6, BYTE, 1); registers[3] = oldEbx;
    upload(ctx, BYTE, Buffer.from([0x81]), "chain initial byte once"); seed(ctx, registers, scan.entry, 0xcd7);
    registers[3] = nextEbx;
    const producer = run(ctx, chainUnit, 1, "real ADD EBX producer before two memory shifts", registers, scan.target, producedFlags);
    counts.producers++;
    const first = target(ctx, chainUnit, registers, scan, BYTE, producedFlags, "chain"),
      secondScan = { ...scan, target: scan.second, next: scan.tail, raw: 7 },
      second = target(ctx, chainUnit, registers, secondScan, BYTE, first.flags, "chain"),
      firstRow = targets.at(-2), secondRow = targets.at(-1);
    assert.equal(secondRow.old_byte, firstRow.observed_byte);
    assert.equal(secondRow.incoming_flags, first.flags);
    assert.deepEqual(secondRow.physical_cpu_before.registers, firstRow.physical_cpu_before.registers);
    assert.equal(secondRow.old_ecx, firstRow.old_ecx); assert.notEqual(secondRow.raw % 32, secondRow.old_ecx % 32);
    chains.push({ context: ctx.ordinal, owner: ctx.owner, kind: ctx.bank.kind, producer,
      producer_input_ebx: oldEbx, producer_output_ebx: nextEbx, produced_flags: producedFlags,
      first_target: targets.length - 2, second_target: targets.length - 1,
      initial_operand_uploads: 1, cpu_writes_between_targets: 0,
      source: "ADD then q1/q7 from physical immediate bytes; evolved byte/FLAGS, diagnostic helpers only" });
    counts.chain_pairs++; consumers(ctx, chainUnit, registers, secondScan, second.flags);
  }
  pages(ctx, "producer chains full data page", [DATA]);
  const faultUnit = compile(ctx, ctx.bank.fault, "fault");
  for (const scan of ctx.bank.fault) for (const flags of FLAGS) {
    const registers = addressRegisters(8, 0xffffffff, scan.raw);
    upload(ctx, 0xffffffff, Buffer.from([0x81]), "valid final byte operand"); seed(ctx, registers, scan.target, flags);
    const result = target(ctx, faultUnit, registers, scan, 0xffffffff, flags, "endpoint");
    consumers(ctx, faultUnit, registers, scan, result.flags);
  }
  pages(ctx, "valid final byte full top page", [TOP]);
  return faultUnit;
}
const faultShapes = [
  { name: "unmapped-read", address: 0x8000, page: 0x8000, detail: 1, fault: 0x8000, access: 1 },
  { name: "readonly-last-byte-store", address: 0x4fff, page: DATA, detail: 2, fault: 0x4fff, access: 2 },
];
function faultCases(ctx, unit) {
  for (const shape of faultShapes) for (const scan of ctx.bank.fault) for (const flags of FLAGS) {
    protect(ctx, DATA, 3);
    if (shape.access === 1) map(ctx, shape.page);
    upload(ctx, shape.address, Buffer.from([0x81]), "pre-fault byte operand");
    const preRestriction = observeByte(ctx, unit, shape.address, "physical pre-restriction operand"),
      arithmetic = shiftArithmetic(ctx.bank.kind, preRestriction.observed_value, scan.raw, flags);
    if (scan.raw === 1) assert.notEqual(arithmetic.flags, flags, "q1 fault rejects distinct candidate FLAGS");
    pages(ctx, "complete pre-restriction operand page", [shape.page]);
    if (shape.access === 1) unmap(ctx, shape.page); else protect(ctx, shape.page, 1);
    const registers = addressRegisters(8, shape.address, scan.raw);
    seed(ctx, registers, scan.entry, flags); registers[3] = registers[3] + 1 >>> 0;
    const helper = shape.access === 1 ? readHelper(0, shape.detail, shape.fault) : storeHelper(shape.detail, shape.fault);
    const first = run(ctx, unit, 2, "retired LEA then precise byte fault without early FLAGS publication",
      registers, scan.target, flags, 5, 1, helper, shape.detail, shape.fault, shape.access);
    counts.prefixes++;
    const decoded = decodeTarget(ctx, physicalCPU(ctx));
    assert.equal(decoded.raw, scan.raw); assert.equal(decoded.address, shape.address); assert.equal(decoded.next, scan.next);
    pages(ctx, "first byte fault surviving complete data page", [DATA]);
    const retry = run(ctx, unit, 1, "unrepaired byte target retry retires zero", registers,
      scan.target, flags, 5, 0, helper, shape.detail, shape.fault, shape.access);
    counts.fault_calls += 2; counts[shape.access === 1 ? "read_faults" : "write_faults"] += 2;
    pages(ctx, "unrepaired byte retry surviving complete data page", [DATA]);
    if (shape.access === 1) map(ctx, shape.page, null); else protect(ctx, shape.page, 3);
    let repairMode, repairByte;
    if (flags === 0xcd7) {
      repairMode = shape.access === 1 ? "map-only" : "permission-only";
      repairByte = shape.access === 1 ? 0 : 0x81;
      counts[shape.access === 1 ? "map_only_repairs" : "permission_only_repairs"]++;
    } else {
      repairMode = shape.access === 1 ? "map+changed-byte" : "permission+changed-byte"; repairByte = 1;
      upload(ctx, shape.address, Buffer.from([1]), "changed-value fault repair byte"); counts.changed_value_repairs++;
    }
    const result = target(ctx, unit, registers, scan, shape.address, flags, "repaired");
    assert.equal(targets.at(-1).old_byte, repairByte);
    consumers(ctx, unit, registers, scan, result.flags); counts.repairs++;
    pages(ctx, "repaired complete byte operand page and neighbors", [shape.page]);
    faults.push({ context: ctx.ordinal, kind: ctx.bank.kind, owner: ctx.owner, raw: scan.raw, flags, shape,
      helper_version: shape.access === 1 ? 2 : 3, width: 1, physical_fault_target: decoded,
      pre_restriction: preRestriction, rejected_candidate: arithmetic, first, retry,
      repair_mode: repairMode, repaired_input_byte: repairByte, repair_target: targets.length - 1,
      repair_expected: result, cpu_writes_after_first_fault: 0, prefix_replays: 0,
      retention: "CPU and surviving mapped bytes retained; map-only rereads fresh zero backing" });
    if (shape.access === 1) unmap(ctx, shape.page);
  }
  pages(ctx, "complete post-fault declared pages");
}

function ownerChecks(ctx, unit) {
  if (ctx.owner === "resident") {
    assert.equal(unit.high, 0); assert.ok(unit.low >= 1 && unit.low <= 7,
      "fresh selected contexts allocate at most seven resident units before this guard");
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
function strictRefusal(ctx, unit) {
  protect(ctx, KEEP, 7);
  const bytes = Buffer.from([0x66, 0xc0, 5 | ctx.bank.extension, 0x10, 0x40, 0, 0, 8, 0xeb, 0, 0x0f, 0x0b]);
  upload(ctx, KEEP, bytes, "strict66 new immediate neighbor");
  request(ctx, words(ctx.owner === "replacement" ? [KEEP] : [KEEP, bytes.length - 2]), "strict refusal compiler request");
  const beforeArena = arena(ctx), published = [ctx.api.generation(), ctx.api.module_ptr(), ctx.api.module_len()],
    beforeModule = Buffer.from(refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.deepEqual(beforeModule, unit.bytes);
  host(ctx, ctx.owner === "replacement" ? "compile_entries" : "compile_resident",
    ctx.owner === "replacement" ? [1, 0] : [1], 10);
  assert.deepEqual(arena(ctx), beforeArena);
  assert.deepEqual([ctx.api.generation(), ctx.api.module_ptr(), ctx.api.module_len()], published);
  assert.deepEqual(Buffer.from(refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length)), beforeModule);
  const cpu = physicalCPU(ctx), survival = run(ctx, unit, 0, "current published owner survives strict66 compiler refusal",
    cpu.registers, cpu.pc, cpu.flags, 1, 0);
  counts.compiler_refusals++; counts.rejection_survivals++;
  refusals.push({ context: ctx.ordinal, kind: ctx.bank.kind, owner: ctx.owner, bytes: bytes.toString("hex"),
    status: 10, category: "strict66 unsupported opcode", published, module: unit.file,
    module_sha256_before: hash(beforeModule), module_sha256_after: hash(unit.bytes),
    arena_sha256_before: hash(beforeArena), survival });
  upload(ctx, KEEP, pattern.subarray(0, 12), "restore twelve-byte strict probe page"); protect(ctx, KEEP, 3);
  pages(ctx, "restored patterned strict refusal page", [KEEP]);
}
function close(ctx) {
  if (ctx.expected.readUInt32LE(96) !== 1) cancel(ctx, 1);
  host(ctx, "close", []);
  for (const unit of ctx.units) {
    generated(ctx, unit, [0xffffffff, 0xffffffff, 0, 0xffffffff], "closed owner before malformed cancelled zero-budget arguments", 5);
    counts.closed_calls++;
  }
}
function smc(ctx, selected) {
  pages(ctx, "initial SMC declared pages");
  const unit = compile(ctx, [selected], "smc"), address = selected.entry + 1,
    registers = addressRegisters(8, address, selected.raw), flags = 2;
  ownerChecks(ctx, unit); seed(ctx, registers, selected.entry, flags);
  registers[0] = registers[0] - registers[0] % 256 + selected.value;
  const prefix = run(ctx, unit, 1, "real MOV AL before consumed immediate byte code store",
    registers, selected.target, flags);
  counts.prefixes++;
  const oldAllocation = Buffer.from(refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.deepEqual(oldAllocation, unit.bytes);
  const result = target(ctx, unit, registers, selected, address, flags, "code", 6),
    committedCPU = physicalCPU(ctx), committedByte = targets.at(-1).observed_byte,
    replay = shiftArithmetic(ctx.bank.kind, committedByte, selected.raw, committedCPU.flags),
    discriminatingReplay = selected.raw === 1 && selected.value === 0x81;
  if (discriminatingReplay) assert.notEqual(replay.result, committedByte, "q1 old81 repeat changes committed byte");
  else assert.equal(replay.result, committedByte, "selected q0/q8/same-zero repeat is idempotent");
  counts.code_stores++; counts[committedByte === selected.value ? "same_value_code_stores" : "changing_code_stores"]++;
  pages(ctx, "physical committed consumed MOV immediate code page", [PC]);
  cancel(ctx, 1); let afterAllocation;
  const stale = generated(ctx, unit, [0xffffffff, 0xffffffff, 0, 0xffffffff],
    "code-store stale guard before malformed pointers cancellation and zero budget", 4, null, () => {
      afterAllocation = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length));
      assert.deepEqual(afterAllocation, oldAllocation, "first synchronous post-stale observation is whole old allocation");
    });
  counts.stale_calls++; cancel(ctx, 0);
  const currentCode = Buffer.from(ctx.pages.get(PC)), savedCurrent = artifact(`current-${ctx.ordinal}.x86`, currentCode),
    savedTail = artifact(`tail-${ctx.ordinal}.x86`, currentCode.subarray(selected.next - PC, selected.next - PC + 16)),
    tail = { ...selected, entry: selected.next, spec: [selected.next, 16], instructions: 4 },
    current = compile(ctx, [tail], "current-tail", true);
  registers[0] = registers[0] - registers[0] % 256 + result.flags % 2;
  registers[2] = registers[2] - registers[2] % 256 + Number(Boolean(result.flags & 0x800)); registers[3] = 0xdeadbeef;
  const continuation = run(ctx, current, 5, "fresh guard-only sixteen-byte tail without MOV/RMW replay",
    registers, COLD, result.flags, 3, 4);
  counts.setb++; counts.seto++; counts.jumps++; counts.canaries++;
  pages(ctx, "current pure tail keeps full committed code page", [PC]);
  codeStores.push({ context: ctx.ordinal, owner: ctx.owner, kind: ctx.bank.kind, raw: selected.raw,
    address, before: selected.value, after: committedByte, flags_before: flags, flags_after: result.flags,
    prefix, target: targets.length - 1, reason: 6, retired: 1, target_pc: selected.target, next_pc: selected.next,
    same_value: committedByte === selected.value, stale, stale_status: 4,
    old_module_sha256_before: hash(oldAllocation), old_module_sha256_after: hash(afterAllocation),
    replay_counterfactual: replay, discriminating_replay: discriminatingReplay,
    mov_head_replay_counterfactual_al: committedByte, actual_retained_al_before_tail: committedCPU.registers[0] % 256,
    idempotence_ceiling: "q0/q8/same-zero cases use exact current entry/import/body/retirement/retained CPU; scalar repeated byte alone cannot distinguish replay",
    saved_current: savedCurrent, saved_tail: savedTail, continuation_module: current.file, continuation,
    continuation_bytes: 16, continuation_instructions: 4, helper_width: 1, helper_version: 3,
    cpu_writes_during_continuation: 0, replayed_memory_instructions: 0,
    allocation_ceiling: "after hash is producer first-synchronous live comparison receipt, not separately saved full after bytes" });
  close(ctx);
}

for (const selected of banks) for (const owner of ["replacement", "resident"]) {
  const ctx = fresh(owner, selected, "numeric"), unit = numeric(ctx);
  faultCases(ctx, unit); controls(ctx, unit); ownerChecks(ctx, unit); strictRefusal(ctx, unit); close(ctx);
}
for (const selected of banks) for (const owner of ["replacement", "resident"])
  for (const scan of selected.smc) smc(fresh(owner, selected, `smc-${scan.raw}-${scan.value}`), scan);
const rawArenas = artifact("arena-snapshots.bin", Buffer.concat(arenaFrames)),
  rawPages = artifact("page-snapshots.bin", Buffer.concat(pageFrames));
counts.host_inputs = hostInputs.length; counts.host_calls = hostCalls.length; counts.files = artifacts.length + 1;
for (const [name, expected] of Object.entries(plan)) assert.equal(counts[name], expected, `frozen prospective ${name} census`);
assert.deepEqual(pinSources(), sourcePins, "selected source bytes stable during execution");
assert.equal(hash(readFileSync(enginePath)), engineSha256);
const result = {
  status: "ok", profile: "finite flat32 C0 /4,/5,/7 BYTE memory shift by physical encoded immediate",
  command: [process.execPath, process.argv[1], enginePath, output, root],
  environment: { node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch },
  engine: { path: enginePath, bytes: engineBytes.length, sha256: engineSha256 },
  source: { order: "253667-093US", edition: "September 2026", full_text_sha256: hash(fullText),
    chapter_lines: [30072, 30275], saved_chapter: savedChapter, latest_edition_or_hardware_claim: false },
  source_pins: sourcePins, source_pins_after: pinSources(), plan, counts, contexts, modules, targets, runs,
  faults, chains, code_stores: codeStores, mutations, compiler_refusals: refusals,
  host_inputs: hostInputs, host_calls: hostCalls, raw_arena_records: arenaRows, raw_arena_artifact: rawArenas,
  pages: pageRows, raw_page_artifact: rawPages, artifacts,
  corpus: { raw: RAW, values: VALUES, flags: FLAGS, registers: REG, ecx_seeds: ECX,
    literal_anchors: ANCHORS, add_anchors: PRODUCERS, arithmetic_targets: 840,
    ea_pairs: banks[0].ea.map(scan => [scan.form, scan.raw]), fault_raw: [0, 1, 8], fault_shapes: faultShapes,
    endpoint_addresses: [0xffffffff], endpoint_raw: [0, 1, 8], arena_bytes: SIZE, native_origin_targets: 0,
    actual_origin_targets: 1104, limits: { blocks: 8, instructions: 64, wasm_bytes: 65536 },
    patterned_page_sha256: hash(pattern), valid_flags_mask: 0xcd7, fixed_flags_bit: 2 },
  banks: banks.map(({ kind, extension, groups, ea, fault, chain, smc, packed, saved }) =>
    ({ kind, extension, groups, ea, fault, chain, smc, packed, saved })),
  evidence_limits: {
    arithmetic: "unchanged widened arithmetic/literal byte-shift and ADD definitions from selected old fixtures; q0 retains validFLAGS, q>=8 selectedCF and undefinedAF/multiOF clear; independent raw checker owns separate bit-walk",
    origins: "all1104 targets use current actual-engine-compiled replacement/resident modules; no native-produced guest arithmetic or unbound memory backend claim",
    arenas: "all4686 generated calls save full4236B before/after;2208 target pre/postRead8 frames plus72 pre-restriction frames yield11652 saved frames; known arena reconstruction is finite, not a generic certificate",
    pages: "648 complete4096B physical pages with non-target neighbors; every successful target physically reads its old/stored byte;663552 hostRead32 page diagnostics and2280 hostRead8 observations are separate from guest imports",
    lifecycle: "72 true LEA/fault/retry/repair cases retain fullCPU and surviving mapped bytes;18 map-only repairs reread fresh zero,18 permission-only retain81,36 changed-byte repairs reread1; width1 ffffffff is valid, no operand overflow/cross-page fault claim",
    currency: "36 consumed MOV immediate stores include12 q0 same-value invalidations;42 stale4 calls compare complete retained live old allocation first synchronously before helpers/getters/new allocations;36 fresh16B four-instruction tails and6 count-mutation11B three-instruction tails resume currentCPU without head replay; after hashes are producer comparison receipts",
    chains: "12 real ADD prefixes then24 consecutive q1/q7 memory targets; second recaptures physical evolved byte/fullFLAGS/fullGPRs and encoded count, no CPU or operand reseed; Read8 diagnostics legitimately change helper residue",
    scope: "finite10raw/seven-byte/two-validFLAGS arithmetic plus standalone8EA/count pairs; narrow C0/EA parser and Wasm types/imports/locals/guard-prefix checks do not certify a general decoder or remaining Wasm body; no fullRAM/ISA/hardware/MMIO/races/performance/CI/graphics/game claim",
  },
};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), { flag: "wx" });
assert.deepEqual(readdirSync(output).sort(), [...artifacts.map(row => row.path), "result.json"].sort(), "exact physical file census");
const resultBytes = readFileSync(join(output, "result.json"));
console.log(JSON.stringify({ status: "ok", engine_sha256: engineSha256, result_sha256: hash(resultBytes), counts, output }));
