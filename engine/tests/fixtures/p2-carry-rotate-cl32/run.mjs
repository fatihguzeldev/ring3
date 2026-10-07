import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../support/engine.mjs";
import { ANCHORS, rotate } from "../p2-carry-rotate-immediate32/oracle.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, "expected current engine, output directory and repository root");
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const { bytes: engineBytes, module: engineModule, sha256: engineSha256 } = readEngine(enginePath);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236,
  TRANSFER = 140,
  PC = 0x1000,
  MASTER_COLD = PC + 0xf00,
  LOCAL_COLD = PC + 128;
const OWNERS = ["standalone", "replacement", "resident"],
  KINDS = ["left", "right"],
  FLAGS = [2, 0xcd7];
const RAW = Array.from({ length: 32 }, (_, i) => i),
  EDGE_RAW = [32, 33, 255];
const STYLE_RAW = [0, 1, 8, 9, 10, 18, 19, 27, 28, 31],
  STYLE_FLAGS = [3, 0xcd6],
  VALID_FLAGS = [...FLAGS, ...STYLE_FLAGS];
const REG = [
  0xa1b2c3d4, 0x12345678, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef,
];
const DESTINATIONS = ["EAX", "ECX", "EDX", "EBX", "ESP", "EBP", "ESI", "EDI"];
const BASE = { left: 0x81234567, right: 0x92345678 },
  ECX_BASE = { left: 0x81234500, right: 0x92345600 },
  VALUES = [0, 0xffffffff, 0x80000000, 0x01234567],
  ECX_HIGH = [0, 0xffffff00, 0x80000000, 0x01234500],
  VALUE_RAW = [0, 1, 2, 31];
function putLowByte(registers, parent, value) {
  assert.ok(Number.isInteger(value) && value >= 0 && value < 256);
  registers[parent] = registers[parent] - (registers[parent] % 256) + value;
}
for (const [kind, value, raw, incoming, expected, flags] of ANCHORS) {
  const result = rotate(kind, value, raw, incoming);
  assert.equal(result.value, expected);
  assert.equal(result.flags, flags);
}
const words = (values) => {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4));
  return bytes;
};
const banks = [], currencyBanks = new Map();
function makeBank(name, group, kind = null, currency = null) {
  const bytes = Buffer.alloc(group === "master" ? 4096 : 130, 0xcc),
    instructions = [],
    scans = [],
    groups = [];
  let at = 0;
  const append = (code) => {
    const pc = PC + at;
    bytes.set(code, at);
    instructions.push({ pc, length: code.length, next_pc: pc + code.length });
    at += code.length;
    return pc;
  };
  const tail = (canary) => {
    append([0x0f, 0x92, 0xc0]);
    append([0x0f, 0x90, 0xc2]);
    if (canary) append([0xbb, 0xef, 0xbe, 0xad, 0xde]);
    if (group === "master") {
      const jump = Buffer.alloc(5);
      jump[0] = 0xe9;
      jump.writeInt32LE(MASTER_COLD - (PC + at + 5), 1);
      append(jump);
    } else append([0xeb, 128 - at - 2]);
  };
  if (group === "master")
    for (const [ordinal, direction] of KINDS.entries()) {
      const specs = [], rows = [];
      for (let alias = 0; alias < 8; alias++) {
        const entry = PC + at,
          target = append([0xd3, 0xd0 + (direction === "right" ? 8 : 0) + alias]),
          next = PC + at;
        tail(false);
        specs.push([entry, PC + at - entry]);
        const scan = { kind: direction, alias, ordinal, target, next };
        scans.push(scan);
        rows.push(scan);
      }
      groups.push({ ordinal, kind: direction, specs, scans: rows });
    }
  else
    for (const row of group === "chain"
      ? KINDS.flatMap((direction) => [
          { kind: direction, alias: 1, raw: 1, value: 0x80000001, producer: "clc", opcode: 0xf8 },
          { kind: direction, alias: 4, raw: 0, value: BASE[direction], producer: "stc", opcode: 0xf9 },
          { kind: direction, alias: 5, raw: 17, value: BASE[direction], producer: "cmc", opcode: 0xf5 },
        ])
      : [currency]) {
      const entry = PC + at;
      if (group === "chain") append([row.opcode]);
      const target = append([0xd3, 0xd0 + (row.kind === "right" ? 8 : 0) + row.alias]);
      const second =
          group === "chain"
            ? append([0xd3, 0xd0 + (row.kind === "right" ? 8 : 0) + row.alias])
            : null,
        next = PC + at;
      tail(true);
      scans.push({ ...row, entry, target, second, next });
      groups.push({
        ordinal: groups.length,
        raw: row.raw,
        specs: [[entry, PC + at - entry]],
        scans: [scans.at(-1)],
      });
    }
  const packed = at,
    cold = group === "master" ? MASTER_COLD : LOCAL_COLD;
  assert.equal(packed, group === "master" ? 208 : group === "chain" ? 108 : 15);
  assert.equal(instructions.length, group === "master" ? 64 : group === "chain" ? 42 : 5);
  bytes.set([0x0f, 0x0b], cold - PC);
  assert.deepEqual(
    bytes,
    readFileSync(join(output, `${name}.x86`)),
    "independent Rust/JS authored bank bytes",
  );
  const bank = {
    name,
    kind,
    group,
    bytes,
    packed,
    cold,
    groups,
    scans,
    instructions,
    specs: groups.flatMap((row) => row.specs),
  };
  banks.push(bank);
  return bank;
}
const masterBank = makeBank("master", "master");
const chainBank = makeBank("chain", "chain");
const currencyCases = [
  {
    name: "eax-same-opcode",
    kind: "left",
    alias: 0,
    raw: 10,
    value: 0x80000001,
    mutation_offset: 0,
    original: 0xd3,
    changed: 0xd3,
  },
  {
    name: "ecx-direction",
    kind: "left",
    alias: 1,
    raw: 2,
    value: 0x80000002,
    mutation_offset: 1,
    original: 0xd1,
    changed: 0xd9,
  },
  {
    name: "esp-destination",
    kind: "right",
    alias: 4,
    raw: 10,
    value: 0x80000000,
    mutation_offset: 1,
    original: 0xdc,
    changed: 0xdd,
  },
];
for (const row of currencyCases)
  currencyBanks.set(row.name, makeBank(`currency-${row.name}`, "currency", row.kind, row));
assert.deepEqual(readdirSync(output).sort(), [
  ...banks.map((row) => `${row.name}.x86`),
  "standalone-master-g0.wasm", "standalone-master-g1.wasm", "standalone-chain.wasm",
].sort(), "exclusive initial Rust bank/module output roster");
const plan = {
  regular_per_owner: 2 * 32 * 2 * 2 + 6 * 2 * 4 * 2 + 2 * 2 * 3 * 2 + 2 * 2 * 10 * 2 + 2 * 4 * 4 * 2 * 2,
  regular_targets: 584 * 3,
  chain_pairs: 2 * 3 * 2 * 3,
  chain_targets: 2 * 3 * 2 * 3 * 2,
  currency_targets: 3 * 2,
  contexts: 3 + 3 + 6,
  bound_contexts: 2 + 2 + 6,
  modules: 2 * 3 + 3 + 6 * 2,
  pure_modules: 2 + 1,
  bound_modules: 2 * 2 + 2 + 6 * 2,
  seeds: 584 * 3 + 36 + 6,
  regular_contexts: 3 + 3,
  bound_regular_contexts: 2 + 2,
  managed_page_rosters: 10,
};
plan.targets = plan.regular_targets + plan.chain_targets + plan.currency_targets;
plan.generated_calls =
  plan.regular_targets * 2 +
  plan.chain_pairs * 4 +
  plan.currency_targets * 6 +
  plan.regular_contexts * 4 +
  plan.bound_regular_contexts;
plan.retired = plan.regular_targets * 4 + plan.chain_pairs * 7 + plan.currency_targets * 5;
plan.raw_arenas = plan.generated_calls * 2;
plan.source_spans = 2 * 32 * 3 + 3 * 42 + 6 * 5 + 6 * 4;
plan.host_calls = 10 + 22 + 4 + 18 + 8 + 10;
plan.host_requests = 22 + 18;
plan.cancel_inputs =
  plan.regular_contexts * 2 + plan.bound_regular_contexts + plan.currency_targets * 3;
plan.host_inputs = plan.seeds + plan.host_requests + 22 + plan.cancel_inputs;
plan.arena_checks =
  plan.contexts +
  (plan.host_calls - plan.bound_modules) * 2 +
  plan.bound_modules * 3 +
  plan.pure_modules +
  plan.host_requests +
  plan.seeds +
  plan.cancel_inputs +
  plan.generated_calls * 2 +
  plan.raw_arenas;
plan.files = 1 + 5 + plan.modules + 6 + 6 + 1 + 1 + 1;
assert.deepEqual(
  [
    plan.regular_per_owner,
    plan.targets,
    plan.contexts,
    plan.modules,
    plan.generated_calls,
    plan.retired,
    plan.raw_arenas,
    plan.files,
  ],
  [584, 1830, 12, 21, 3712, 7290, 7424, 42],
);
assert.deepEqual(
  [plan.host_calls, plan.host_inputs, plan.source_spans, plan.arena_checks],
  [72, 1890, 372, 16893],
);
const counts = {
  contexts: 0,
  modules: 0,
  seeds: 0,
  targets: 0,
  regular_targets: 0,
  chain_targets: 0,
  currency_targets: 0,
  left_targets: 0,
  right_targets: 0,
  zero_targets: 0,
  one_targets: 0,
  multi_targets: 0,
  effective_zero_targets: 0,
  generated_calls: 0,
  retired: 0,
  setb: 0,
  seto: 0,
  jumps: 0,
  canaries: 0,
  cancel_calls: 0,
  zero_budget_calls: 0,
  malformed_calls: 0,
  cold_calls: 0,
  direct_guard_controls: 0,
  stale_calls: 0,
  closed_calls: 0,
  arena_checks: 0,
  raw_arenas: 0,
  host_calls: 0,
  producers: 0,
  clc: 0,
  stc: 0,
  cmc: 0,
};
const contexts = [],
  modules = [],
  targets = [],
  runs = [],
  mutations = [],
  hostInputs = [],
  hostCalls = [],
  artifacts = [],
  rawFrames = [],
  rawRecords = [],
  chainRows = [];
const caseCensus = new Set(),
  semanticInputs = new Set(),
  sourceSpans = [],
  managedContexts = [],
  allContexts = [];
function artifact(path, bytes, write = true) {
  assert.ok(!artifacts.some((row) => row.path === path));
  if (write) writeFileSync(join(output, path), bytes, { flag: "wx" });
  const row = { path, bytes: bytes.length, sha256: hash(bytes) };
  artifacts.push(row);
  return row;
}
process.on("uncaughtException", (error) => {
  const retained = [];
  try {
    const held = Buffer.concat(rawFrames);
    writeFileSync(join(output, "failure-arena-snapshots.bin"), held, { flag: "wx" });
    for (const ctx of allContexts) {
      if (!ctx.memory || !ctx.base || ctx.base + SIZE > ctx.memory.buffer.byteLength) continue;
      const bytes = Buffer.from(
          new Uint8Array(ctx.memory.buffer).subarray(ctx.base, ctx.base + SIZE),
        ),
        path = `failure-current-${ctx.ordinal}.bin`;
      writeFileSync(join(output, path), bytes, { flag: "wx" });
      retained.push({
        context: ctx.ordinal,
        owner: ctx.owner,
        path,
        bytes: bytes.length,
        sha256: hash(bytes),
      });
    }
    writeFileSync(
      join(output, "failure.json"),
      JSON.stringify(
        {
          status: "failed",
          error: String(error?.stack ?? error),
          plan,
          counts,
          engine_sha256: engineSha256,
          held_frames: rawRecords,
          held_bytes: held.length,
          held_sha256: hash(held),
          retained,
          runs,
          targets,
          chain_pairs: chainRows,
          mutations,
          host_inputs: hostInputs,
          host_calls: hostCalls,
          authority:
            "already captured physical frames and known live arenas only; no guest/helper/model replay",
        },
        null,
        2,
      ),
      { flag: "wx" },
    );
  } catch (fallbackError) {
    console.error(`failure evidence fallback: ${String(fallbackError)}`);
  }
  console.error(error);
  process.exitCode = 1;
});
artifact("engine.wasm", engineBytes);
for (const selected of banks)
  selected.artifact = artifact(`${selected.name}.x86`, selected.bytes, false);
const source = {
  provenance:
    "physically pinned offline Intel253667-093US September2026; no fresh edition/download claim",
  prerequisite_policy:
    "ignored immutable local PDF/fulltext/extract required before guest; no bootstrap/fullCI claim",
  pdf_path: "target/r3-rotate-scout/intel-253667-093-vol2b.pdf",
  pdf_bytes: 6915682,
  pdf_sha256: "a261998ace8e07f624bf3e2486bb6cfe950fb8be8d0dc9631bcecb4c5ec953e4",
  full_text_path: "target/p2-memory-binary-spec/253667-093-sdm-vol-2b.txt",
  full_text_bytes: 1537522,
  full_text_sha256: "f5e6dc689d41655d64792512bfe8add56fc4ac97203aab5c1377a72bac291a35",
  extract_path: "target/r3-rotate-scout/intel-093-rotate-pages.txt",
  extract_bytes: 11074,
  extract_sha256: "7a0e03302580204f99e8283af389f84ec082701807f1a9615a028af4d6f849d0",
  chapter_lines: [27174, 27413],
  chapter_bytes: 10932,
  chapter_sha256: "f502898d90ce6ea7a6a513d1c8761e507af8ad92fac581e1240ea413b78aa74d",
  distinction:
    "full_text is entire Vol2B; extract is same edition five pages; saved chapter includes complete rotate chapter, excludes successor RCPPS",
};
for (const kind of ["pdf", "full_text", "extract"]) {
  const bytes = readFileSync(join(root, source[`${kind}_path`]));
  assert.equal(bytes.length, source[`${kind}_bytes`]);
  assert.equal(hash(bytes), source[`${kind}_sha256`]);
}
const fullTextLines = readFileSync(join(root, source.full_text_path), "utf8").split("\n");
const chapter = Buffer.from(
  fullTextLines.slice(source.chapter_lines[0] - 1, source.chapter_lines[1]).join("\n") + "\n",
);
assert.equal(chapter.length, source.chapter_bytes);
assert.equal(hash(chapter), source.chapter_sha256);
source.saved_chapter = artifact("intel-rotate.txt", chapter);
const sourcePaths = [
  "engine/src/cpu/x86/ir.rs",
  "engine/src/cpu/x86/decode/integer.rs",
  "engine/src/cpu/dbt/region.rs",
  "engine/src/cpu/dbt/wasm/integer.rs",
  "engine/src/cpu/x86/decode/profile.rs",
  "engine/src/cpu/x86/decode/operands.rs",
  "engine/src/cpu/dbt/wasm/emitter.rs",
  "engine/src/cpu/dbt/wasm/locals.rs",
  "engine/src/cpu/dbt/wasm/memory.rs",
  "engine/Cargo.toml",
  "engine/tests/cpu_carry_rotate_one.rs",
  "engine/tests/cpu_carry_rotate_immediate_one.rs",
  "engine/tests/cpu_byte_carry_rotate_cl.rs",
  "engine/tests/cpu_rotate32.rs",
  "engine/tests/cpu_memory_rotate32.rs",
  "engine/tests/cpu_carry_rotate_immediate32.rs",
  "engine/tests/fixtures/p2-carry-rotate-immediate32/oracle.mjs",
  "engine/tests/fixtures/support/engine.mjs",
  "engine/tests/cpu_carry_rotate_cl32.rs",
  "engine/tests/cpu_carry_rotate_cl_wasm.rs",
  "engine/tests/fixtures/p2-carry-rotate-cl32/run.mjs",
];
assert.equal(new Set(sourcePaths).size, 21);
const sourceIdentities = () =>
  Object.fromEntries(
    sourcePaths.map((path) => {
      const bytes = readFileSync(join(root, path));
      return [path, { bytes: bytes.length, sha256: hash(bytes) }];
    }),
  );
const sourcePins = sourceIdentities();
function profile(bytes, owner) {
  let at = 8;
  assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  const uleb = () => {
    let value = 0,
      scale = 1;
    for (let i = 0; i < 5; i++) {
      assert.ok(at < bytes.length);
      const byte = bytes[at++];
      value += (byte & 127) * scale;
      if (!(byte & 128)) {
        assert.ok(value <= 0xffffffff);
        return value;
      }
      scale *= 128;
    }
    assert.fail("invalid bounded unsigned LEB");
  };
  const text = () => {
    const n = uleb();
    assert.ok(at + n <= bytes.length);
    const value = bytes.subarray(at, at + n).toString("utf8");
    at += n;
    return value;
  };
  const sections = new Map();
  while (at < bytes.length) {
    const id = bytes[at++],
      n = uleb();
    assert.ok(at + n <= bytes.length && !sections.has(id));
    sections.set(id, [at, at + n]);
    at += n;
  }
  assert.deepEqual([...sections.keys()], [1, 2, 3, 7, 10]);
  const section = (id) => {
      at = sections.get(id)[0];
    },
    ended = (id) => assert.equal(at, sections.get(id)[1]);
  section(1);
  const types = [];
  for (let i = 0, n = uleb(); i < n; i++) {
    assert.equal(bytes[at++], 0x60);
    const parameters = [],
      results = [];
    for (let j = 0, n = uleb(); j < n; j++) parameters.push(bytes[at++]);
    for (let j = 0, n = uleb(); j < n; j++) results.push(bytes[at++]);
    types.push({ parameters, results });
  }
  ended(1);
  const standalone = owner === "standalone";
  assert.deepEqual(types, [
    { parameters: Array(4).fill(0x7f), results: [0x7f] },
    ...(standalone
      ? []
      : [{ parameters: Array(owner === "replacement" ? 6 : 7).fill(0x7f), results: [0x7f] }]),
  ]);
  section(2);
  assert.equal(uleb(), standalone ? 1 : 2);
  assert.equal(text(), "env");
  assert.equal(text(), "memory");
  assert.equal(bytes[at++], 2);
  assert.equal(uleb(), 0);
  assert.equal(uleb(), 1);
  if (!standalone) {
    assert.equal(text(), "ring3");
    assert.equal(text(), owner === "replacement" ? "guard" : "guard_resident");
    assert.equal(bytes[at++], 0);
    assert.equal(uleb(), 1);
  }
  ended(2);
  section(3);
  assert.equal(uleb(), 1);
  assert.equal(uleb(), 0);
  ended(3);
  section(7);
  assert.equal(uleb(), 1);
  assert.equal(text(), "run");
  assert.equal(bytes[at++], 0);
  assert.equal(uleb(), standalone ? 0 : 1);
  ended(7);
  section(10);
  assert.equal(uleb(), 1);
  const bodyBytes = uleb(),
    end = at + bodyBytes;
  assert.equal(end, sections.get(10)[1]);
  const locals = [];
  for (let i = 0, n = uleb(); i < n; i++) locals.push([uleb(), bytes[at++]]);
  assert.deepEqual(locals, [
    [16, 0x7f],
    [1, 0x7e],
  ]);
  assert.ok(at < end);
  assert.equal(bytes[end - 1], 0x0b);
  return { types, locals, body_bytes: bodyBytes, sections: [...sections.keys()] };
}
function record(magic, size, fields) {
  const bytes = Buffer.alloc(size);
  bytes.write(magic);
  bytes.writeUInt16LE(1, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4));
  return bytes;
}
const state = (registers, pc, flags) => record("R3ST", 56, [...registers, pc, flags]);
const exit = (reason, retired) => record("R3EX", 40, [reason, retired, 0, 0, 0, 0]);
function refresh(ctx) {
  ctx.bytes = new Uint8Array(ctx.memory.buffer);
  ctx.view = new DataView(ctx.memory.buffer);
  return ctx;
}
function arena(ctx) {
  return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));
}
function check(ctx, label) {
  assert.deepEqual(
    arena(ctx),
    ctx.expected,
    `${ctx.owner}/${ctx.ordinal}/${label}: complete live arena`,
  );
  counts.arena_checks++;
}
function snapshot(ctx) {
  const bytes = arena(ctx);
  return {
    state_hex: bytes.subarray(0, 56).toString("hex"),
    exit_hex: bytes.subarray(56, 96).toString("hex"),
    helper_hex: bytes.subarray(100, 140).toString("hex"),
    whole_arena_sha256: hash(bytes),
  };
}
function cpu(ctx) {
  return {
    registers: Array.from({ length: 8 }, (_, i) => ctx.expected.readUInt32LE(16 + i * 4)),
    pc: ctx.expected.readUInt32LE(48),
    flags: ctx.expected.readUInt32LE(52),
  };
}
function physicalCpu(ctx) {
  const bytes = arena(ctx),
    captured = {
      registers: Array.from({ length: 8 }, (_, i) => bytes.readUInt32LE(16 + i * 4)),
      pc: bytes.readUInt32LE(48),
      flags: bytes.readUInt32LE(52),
    };
  assert.deepEqual(captured, cpu(ctx), "physical full before CPU agrees known arena");
  return captured;
}
function saveArena(ctx, label, unit) {
  check(ctx, label);
  const bytes = arena(ctx),
    row = {
      context: ctx.ordinal,
      module: unit.file,
      label,
      path: "arena-snapshots.bin",
      offset: rawFrames.length * SIZE,
      bytes: SIZE,
      sha256: hash(bytes),
    };
  rawFrames.push(bytes);
  rawRecords.push(row);
  counts.raw_arenas++;
  return row;
}
function hostInput(ctx, kind, label, offset, bytes, extra = {}) {
  hostInputs.push({
    ordinal: hostInputs.length + 1,
    context: ctx.ordinal,
    kind,
    label,
    arena_offset: offset,
    bytes: bytes.length,
    hex: bytes.toString("hex"),
    sha256: hash(bytes),
    ...extra,
  });
}
function pure(ctx, name, args, status = 0) {
  check(ctx, `before host ${name}`);
  const before = snapshot(ctx);
  assert.equal(ctx.api[name](...args), status, name);
  check(ctx, `host ${name}`);
  if (name === "close" && status === 0) ctx.guestPagesRetired = true;
  counts.host_calls++;
  hostCalls.push({
    ordinal: counts.host_calls,
    context: ctx.ordinal,
    name,
    args,
    status,
    before,
    after: snapshot(ctx),
  });
}
function request(ctx, bytes, label) {
  assert.ok(bytes.length <= 4096);
  refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER);
  ctx.expected.set(bytes, TRANSFER);
  hostInput(ctx, "request", label, TRANSFER, bytes);
  check(ctx, "explicit host request");
}
function upload(ctx, address, bytes, label) {
  request(ctx, bytes, label);
  pure(ctx, "upload", [address, bytes.length]);
  assert.ok(address >= PC && address + bytes.length <= PC + ctx.code.length);
  ctx.code.set(bytes, address - PC);
  hostInput(ctx, "upload", label, null, bytes, { address });
}
function fresh(owner, selected, label, writable = false) {
  const ordinal = ++counts.contexts,
    ctx = {
      owner,
      ordinal,
      selected,
      label,
      low: ordinal,
      high: 0x4243494d,
      expected: Buffer.alloc(SIZE),
      code: Buffer.from(selected.bytes),
      units: [],
    };
  allContexts.push(ctx);
  ctx.expected.set(state(Array(8).fill(0), 0, 2));
  ctx.expected.set(exit(1, 0), 56);
  ctx.expected.set(record("R3MH", 40, [0, 0, 0, 0, 0, 0]), 100);
  if (owner === "standalone") {
    ctx.memory = new WebAssembly.Memory({ initial: 1 });
    ctx.base = 128;
    refresh(ctx).bytes.set(ctx.expected, ctx.base);
    check(ctx, "independent standalone initialized arena");
  } else {
    const instance = new WebAssembly.Instance(engineModule, {}),
      api = {};
    ctx.memory = instance.exports.memory;
    ctx.api = api;
    const arities = {
      open: 3,
      close: 0,
      arena_ptr: 0,
      map: 3,
      protect: 3,
      upload: 2,
      compile_entries: 2,
      compile_resident: 1,
      generation: 0,
      module_ptr: 0,
      module_len: 0,
      guard: 6,
      guard_resident: 7,
    };
    for (const [name, arity] of Object.entries(arities)) {
      api[name] = instance.exports[`ring3_abi_v1_${name}`];
      assert.equal(typeof api[name], "function", name);
      assert.equal(api[name].length, arity, name);
    }
    assert.equal(api.open(1, ctx.low, ctx.high), 0);
    ctx.base = api.arena_ptr() >>> 0;
    refresh(ctx);
    assert.ok(ctx.base > 0 && ctx.base + SIZE <= ctx.bytes.length);
    check(ctx, "independent arena after open/getter group");
    pure(ctx, "map", [PC, 1, 7]);
    upload(ctx, PC, selected.bytes, "authored bank");
    if (!writable) pure(ctx, "protect", [PC, 1, 5]);
    ctx.codePermissions = writable ? 7 : 5;
    managedContexts.push(ctx);
  }
  contexts.push({
    context: ordinal,
    owner,
    bank: selected.name,
    label,
    key: owner === "standalone" ? null : [ctx.low, ctx.high],
    base: ctx.base,
    page_limit: owner === "standalone" ? null : 1,
    code_permissions: owner === "standalone" ? null : writable ? 7 : 5,
  });
  return ctx;
}
function physical(ctx, unit) {
  refresh(ctx);
  assert.ok(unit.pointer > 0 && unit.pointer + unit.length <= ctx.bytes.length);
  const bytes = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.deepEqual(bytes, unit.bytes, "complete physical live module allocation retained");
  return { pointer: unit.pointer, bytes: unit.length, sha256: hash(bytes) };
}
function compile(ctx, phase, specs = ctx.selected.specs, scan = null) {
  let binding = {},
    bytes,
    file;
  assert.ok(specs.length > 0 && specs.length <= 8);
  if (ctx.owner === "standalone") {
    file = `standalone-${ctx.selected.name}${ctx.selected.group === "master" ? `-${phase}` : ""}.wasm`;
    bytes = readFileSync(join(output, file));
  } else {
    request(
      ctx,
      words(ctx.owner === "replacement" ? specs.map(([entry]) => entry) : specs.flat()),
      `compile ${phase}`,
    );
    check(ctx, "before compiler group");
    const before = snapshot(ctx);
    const name = ctx.owner === "replacement" ? "compile_entries" : "compile_resident",
      args = ctx.owner === "replacement" ? [specs.length, 0] : [specs.length];
    assert.equal(ctx.api[name](...args), 0);
    if (ctx.owner === "replacement") {
      binding = {
        generation: ctx.api.generation(),
        pointer: ctx.api.module_ptr() >>> 0,
        length: ctx.api.module_len() >>> 0,
      };
      assert.ok(binding.generation > 0);
    } else {
      refresh(ctx);
      binding = Object.fromEntries(
        ["low", "high", "pointer", "length"].map((name, i) => [
          name,
          ctx.view.getUint32(ctx.base + TRANSFER + 8 + i * 4, true),
        ]),
      );
      assert.ok(binding.low || binding.high);
      ctx.expected.set(
        words([1, 24, binding.low, binding.high, binding.pointer, binding.length]),
        TRANSFER,
      );
      assert.equal(ctx.api.generation(), 0);
      assert.equal(ctx.api.module_ptr(), 0);
      assert.equal(ctx.api.module_len(), 0);
    }
    check(ctx, "compiler receipt/getter group");
    counts.host_calls++;
    hostCalls.push({
      ordinal: counts.host_calls,
      context: ctx.ordinal,
      name,
      args,
      status: 0,
      before,
      after: snapshot(ctx),
      binding,
    });
    assert.ok(
      binding.pointer > 0 &&
        binding.length > 8 &&
        binding.length <= 65536 &&
        binding.pointer + binding.length <= ctx.bytes.length,
    );
    bytes = Buffer.from(
      refresh(ctx).bytes.subarray(binding.pointer, binding.pointer + binding.length),
    );
    file = `${ctx.owner}-${ctx.ordinal}-${phase}.wasm`;
  }
  assert.ok(bytes.length > 8 && bytes.length <= 65536 && WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes),
    guard = ctx.owner === "replacement" ? "guard" : "guard_resident";
  const imports = [
    { module: "env", name: "memory", kind: "memory" },
    ...(ctx.owner === "standalone" ? [] : [{ module: "ring3", name: guard, kind: "function" }]),
  ];
  assert.deepEqual(WebAssembly.Module.imports(module), imports);
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: "run", kind: "function" }]);
  const abi = profile(bytes, ctx.owner);
  const child = new WebAssembly.Instance(module, {
    env: { memory: ctx.memory },
    ...(ctx.api ? { ring3: { [guard]: ctx.api[guard] } } : {}),
  });
  assert.equal(child.exports.run.length, 4);
  const saved = artifact(file, bytes, ctx.owner !== "standalone"),
    unit = { ...binding, bytes, file, run: child.exports.run };
  ctx.units.push(unit);
  counts.modules++;
  assert.ok(
    ctx.owner !== "resident" || ctx.units.length <= 8,
    "resident unit budget is per fresh engine context",
  );
  const codeArtifact = scan ? ctx.currentCodeArtifact : ctx.selected.artifact,
    spans = ctx.selected.instructions
      .filter((row) => specs.some(([entry, length]) => row.pc >= entry && row.pc < entry + length))
      .map((row) => {
        const bytes = ctx.code.subarray(row.pc - PC, row.pc - PC + row.length),
          span = {
            ...row,
            hex: bytes.toString("hex"),
            sha256: hash(bytes),
            file: codeArtifact.path,
            offset: row.pc - PC,
          };
        sourceSpans.push(span);
        return span;
      });
  modules.push({
    context: ctx.ordinal,
    owner: ctx.owner,
    bank: ctx.selected.name,
    phase,
    file,
    ...binding,
    specs,
    instructions: spans.length,
    imports,
    exports: [{ name: "run", kind: "function" }],
    run_arity: 4,
    exit_version: 1,
    profile: abi,
    artifact: saved,
    code_artifact: codeArtifact,
    tail_artifact: scan ? ctx.tailArtifact : null,
    source_spans: spans,
  });
  check(ctx, "module validation/instantiation leaves arena unchanged");
  return unit;
}
function seed(ctx, registers, pc, flags) {
  assert.ok(VALID_FLAGS.includes(flags));
  ctx.expected.set(state(registers, pc, flags));
  ctx.expected.set(exit(3, 0), 56);
  ctx.expected.writeUInt32LE(0, 96);
  ctx.expected.fill(0x5a, 100, 140);
  const bytes = Buffer.from(ctx.expected.subarray(0, 140));
  refresh(ctx).bytes.set(bytes, ctx.base);
  hostInput(ctx, "seed", "independent case seed", 0, bytes);
  check(ctx, "one independent case seed");
  counts.seeds++;
}
function cancel(ctx, value) {
  const bytes = words([value]);
  ctx.expected.set(bytes, 96);
  refresh(ctx).bytes.set(bytes, ctx.base + 96);
  hostInput(ctx, "cancel", "explicit cancellation field", 96, bytes);
  check(ctx, "cancellation field only");
}
function run(ctx, unit, budget, label, outcome = {}, status = 0, malformed = false, retention) {
  check(ctx, `before ${label}`);
  const beforeArena = saveArena(ctx, `before ${label}`, unit),
    before = snapshot(ctx),
    current = cpu(ctx),
    {
      registers = current.registers,
      pc = current.pc,
      flags = current.flags,
      reason = 1,
      retired = 0,
    } = outcome;
  if (status === 0) {
    ctx.expected.set(state(registers, pc, flags));
    ctx.expected.set(exit(reason, retired), 56);
  }
  const pointers = malformed
    ? [0xffffffff, ctx.base + 56, budget, ctx.base + 96]
    : [ctx.base, ctx.base + 56, budget, ctx.base + 96];
  const observedStatus = unit.run(...pointers);
  assert.equal(observedStatus, status, label);
  if (retention) retention();
  check(ctx, label);
  const afterArena = saveArena(ctx, `after ${label}`, unit);
  counts.generated_calls++;
  counts.retired += status ? 0 : retired;
  const row = {
    ordinal: counts.generated_calls,
    context: ctx.ordinal,
    module: unit.file,
    label,
    budget,
    status: observedStatus,
    expected_status: status,
    pointers,
    reason: status ? null : reason,
    retired: status ? 0 : retired,
    before,
    after: snapshot(ctx),
    before_arena: beforeArena,
    after_arena: afterArena,
  };
  runs.push(row);
  return row;
}
function target(ctx, unit, scan, pc, group, caseKey) {
  const original = physicalCpu(ctx),
    offset = pc - PC,
    encoded = ctx.code.subarray(offset, offset + 2);
  assert.equal(original.pc, pc);
  assert.equal(encoded.length, 2);
  assert.equal(encoded[0], 0xd3);
  assert.equal(encoded[1], 0xd0 + (scan.kind === "right" ? 8 : 0) + scan.alias);
  const raw = original.registers[1] % 256,
    oldValue = original.registers[scan.alias];
  const result = rotate(scan.kind, oldValue, raw, original.flags),
    registers = [...original.registers];
  registers[scan.alias] = result.value;
  const observed = run(ctx, unit, 1, "complete target before any consumer", {
    registers,
    pc: pc + 2,
    flags: result.flags,
    retired: 1,
  });
  const beforeArena = observed.before_arena,
    afterArena = observed.after_arena;
  counts.targets++;
  counts[group]++;
  counts[`${scan.kind}_targets`]++;
  counts[result.q === 0 ? "zero_targets" : result.q === 1 ? "one_targets" : "multi_targets"]++;
  if (result.r === 0) counts.effective_zero_targets++;
  const key = `${ctx.owner}/${caseKey}`;
  assert.ok(!caseCensus.has(key));
  caseCensus.add(key);
  semanticInputs.add([scan.kind, scan.alias, oldValue, raw, original.flags].join("/"));
  const row = {
    context: ctx.ordinal,
    owner: ctx.owner,
    module: unit.file,
    bank: ctx.selected.name,
    group,
    case_key: caseKey,
    kind: scan.kind,
    alias: scan.alias,
    encoded_target_hex: encoded.toString("hex"),
    raw_count: raw,
    raw_count_source: "physical before full ECX low8",
    masked_count: result.q,
    distance: result.r,
    old_cl: original.registers[1] % 256,
    old_ecx: original.registers[1],
    old_value: oldValue,
    old_cf: original.flags % 2,
    target_pc: pc,
    next_pc: pc + 2,
    input_registers: original.registers,
    input_flags: original.flags,
    expected: result,
    registers_after: registers,
    before: observed.before,
    after: observed.after,
    run_ordinal: observed.ordinal,
    before_arena: beforeArena,
    after_arena: afterArena,
    snapshot_phase:
      "complete physical before/after D3 DWORD target before SETcc; count from this target's old full ECX; consecutive chain has no intervening reseed or consumer",
  };
  targets.push(row);
  return row;
}
function consumers(ctx, unit, scan, canary = false) {
  const current = cpu(ctx),
    registers = [...current.registers];
  putLowByte(registers, 0, current.flags % 2);
  putLowByte(registers, 2, Math.floor(current.flags / 0x800) % 2);
  if (canary) registers[3] = 0xdeadbeef;
  const row = run(
    ctx,
    unit,
    canary ? 5 : 4,
    canary ? "current SETB/SETO/MOV/JMP tail only" : "SETB/SETO/nearJMP consume current FLAGS",
    { registers, pc: ctx.selected.cold, reason: 3, retired: canary ? 4 : 3 },
  );
  counts.setb++;
  counts.seto++;
  counts.jumps++;
  if (canary) counts.canaries++;
  return row;
}
function initial(alias, value, raw) {
  assert.ok(Number.isInteger(value) && value >= 0 && value <= 0xffffffff);
  assert.ok(Number.isInteger(raw) && raw >= 0 && raw <= 255);
  const registers = [...REG];
  registers[1] = 0x12345600 + raw;
  registers[alias] = value;
  assert.equal(registers[1] % 256, raw, "ECX value/count seeds are physically coupled");
  return registers;
}
function regular(ctx, unit, scan, raw, value, flags, group) {
  seed(ctx, initial(scan.alias, value, raw), scan.target, flags);
  target(
    ctx,
    unit,
    scan,
    scan.target,
    "regular_targets",
    `${group}/${scan.kind}/g${scan.ordinal}/a${scan.alias}/raw${raw}/v${value}/f${flags}`,
  );
  consumers(ctx, unit, scan);
}
function closed(ctx, unit, label) {
  const before = arena(ctx);
  let retained;
  const observed = run(ctx, unit, 0, label, {}, 5, true, () => {
    const after = arena(ctx);
    assert.deepEqual(
      after,
      before,
      "known arena allocation retained immediately after closed guard",
    );
    retained = {
      pointer: ctx.base,
      bytes: SIZE,
      sha256: hash(after),
      note: "retained EngineInstance arena from known pointer; guest page owners/code publication were closed",
    };
  });
  counts.closed_calls++;
  return { run_ordinal: observed.ordinal, retained_arena: retained };
}
function controls(ctx, unit) {
  run(ctx, unit, 0, "valid current module zero budget");
  counts.zero_budget_calls++;
  cancel(ctx, 1);
  run(ctx, unit, 0, "pending cancel wins over zero budget", { reason: 2 });
  counts.cancel_calls++;
  run(ctx, unit, 0, "invalid State pointer range wins over cancel/budget", {}, 1, true);
  counts.malformed_calls++;
  cancel(ctx, 0);
  run(ctx, unit, 1, "valid current cold EIP returns NeedCode", { reason: 3 });
  counts.cold_calls++;
  if (ctx.api) {
    for (const role of ["key", "identity"]) {
      const low = ctx.low ^ Number(role === "key");
      const name = ctx.owner === "replacement" ? "guard" : "guard_resident",
        args =
          ctx.owner === "replacement"
            ? [
                low,
                ctx.high,
                unit.generation + Number(role === "identity"),
                ctx.base,
                ctx.base + 56,
                ctx.base + 96,
              ]
            : [
                low,
                ctx.high,
                unit.low ^ Number(role === "identity"),
                unit.high,
                ctx.base,
                ctx.base + 56,
                ctx.base + 96,
              ];
      pure(ctx, name, args, 3);
      counts.direct_guard_controls++;
    }
    pure(ctx, "close", []);
    cancel(ctx, 1);
    ctx.closed = closed(ctx, unit, "closed current child before pointers/cancel/budget");
  }
}
for (const owner of OWNERS) {
    const ctx = fresh(owner, masterBank, "584-case owner regular corpus/both directions");
    let last;
    for (const group of masterBank.groups) {
      const unit = compile(ctx, `g${group.ordinal}`, group.specs);
      last = unit;
      for (const scan of group.scans) {
        const base = (raw) => scan.alias === 1 ? ECX_BASE[scan.kind] + raw : BASE[scan.kind];
        for (const raw of scan.alias < 2 ? RAW : VALUE_RAW)
          for (const flags of FLAGS)
            regular(ctx, unit, scan, raw, base(raw), flags,
              scan.alias < 2 ? "eax-ecx-all32q" : "other-gpr-selected-count");
        if (scan.alias < 2) {
          for (const raw of EDGE_RAW)
            for (const flags of FLAGS)
              regular(ctx, unit, scan, raw, base(raw), flags, "eax-ecx-high-raw");
          for (const raw of STYLE_RAW)
          for (const flags of STYLE_FLAGS)
            regular(ctx, unit, scan, raw, base(raw), flags, "eax-ecx-carry-style-split");
          for (const raw of VALUE_RAW)
          for (const high of scan.alias === 1 ? ECX_HIGH : VALUES)
            for (const flags of FLAGS)
              regular(ctx, unit, scan, raw, scan.alias === 1 ? high + raw : high,
                flags, "eax-ecx-value-partition");
        }
      }
    }
    controls(ctx, last);
  }
for (const owner of OWNERS) {
  const ctx = fresh(
      owner,
      chainBank,
      "real carry producer then two consecutive targets capture their own live CL",
    ),
    unit = compile(ctx, "initial");
  for (const scan of chainBank.scans)
    for (const flags of FLAGS) {
      const registers = initial(scan.alias, scan.value, scan.raw);
      seed(ctx, registers, scan.entry, flags);
      const key = `chain/${scan.kind}/${scan.producer}/a${scan.alias}/f${flags}`;
      const carry = scan.producer === "clc" ? 0 : scan.producer === "stc" ? 1 : 1 - (flags % 2),
        producedFlags = flags - (flags % 2) + carry;
      assert.equal(ctx.code[scan.entry - PC], scan.opcode);
      const producer = run(ctx, unit, 1, `real ${scan.producer} before consecutive targets`, {
        registers,
        pc: scan.target,
        flags: producedFlags,
        retired: 1,
      });
      counts.producers++;
      counts[scan.producer]++;
      const first = target(ctx, unit, scan, scan.target, "chain_targets", `${key}/first`),
        second = target(ctx, unit, scan, scan.second, "chain_targets", `${key}/second`);
      assert.deepEqual(first.before, producer.after);
      assert.equal(first.before_arena.sha256, producer.after_arena.sha256);
      assert.deepEqual(second.before, first.after);
      assert.equal(second.before_arena.sha256, first.after_arena.sha256);
      assert.equal(first.raw_count, scan.raw);
      let wrongFirstCount = null;
      if (scan.alias === 1) {
        assert.equal(second.raw_count, scan.kind === "left" ? 2 : 0);
        assert.deepEqual([first.expected.value, second.expected.value],
          scan.kind === "left" ? [2, 10] : [0x40000000, 0x40000000]);
        wrongFirstCount = rotate(
          scan.kind,
          first.expected.value,
          first.raw_count,
          first.expected.flags,
        );
        assert.notEqual(wrongFirstCount.value, second.expected.value);
        assert.equal(wrongFirstCount.value, scan.kind === "left" ? 5 : 0xa0000000);
      } else assert.equal(second.raw_count, first.raw_count);
      const tail = consumers(ctx, unit, scan, true);
      chainRows.push({
        context: ctx.ordinal,
        alias: scan.alias,
        kind: scan.kind,
        input_flags: flags,
        producer: scan.producer,
        producer_run: producer.ordinal,
        produced_flags: producedFlags,
        first_case: first.case_key,
        second_case: second.case_key,
        first_run: first.run_ordinal,
        second_run: second.run_ordinal,
        tail_run: tail.ordinal,
        first_raw_count: first.raw_count,
        second_raw_count: second.raw_count,
        second_before_ecx: second.old_ecx,
        wrong_first_count: wrongFirstCount,
        no_intervening_write:
          "real carry producer then two consecutive budget1 D3 DWORD calls; each target captures current physical ECX; no consumer/CPU/helper/EIP reseed between them",
      });
    }
  controls(ctx, unit);
}
function headReplay(code, committed) {
  assert.equal(code.length, 2);
  assert.equal(code[0], 0xd3);
  const operand = code[1], extension = Math.floor(operand / 8) % 8;
  assert.equal(Math.floor(operand / 64), 3);
  assert.ok(extension === 2 || extension === 3);
  const kind = extension === 3 ? "right" : "left", alias = operand % 8,
    raw = committed.registers[1] % 256,
    result = rotate(kind, committed.registers[alias], raw, committed.flags),
    registers = [...committed.registers];
  registers[alias] = result.value;
  return {...result, kind, alias, raw_count: raw, old_ecx: committed.registers[1],
    old_value: committed.registers[alias], input_flags: committed.flags,
    registers_after: registers};
}
for (const owner of ["replacement", "resident"])
  for (const mutation of currencyCases) {
    const selected = currencyBanks.get(mutation.name),
      ctx = fresh(owner, selected, `host-${mutation.name}`, true),
      old = compile(ctx, "initial"),
      scan = selected.scans[0];
    seed(ctx, initial(mutation.alias, mutation.value, mutation.raw), scan.target, 2);
    const row = target(
      ctx,
      old,
      scan,
      scan.target,
      "currency_targets",
      `currency/${mutation.name}`,
    );
    const committedCpu = physicalCpu(ctx), originalHead = Buffer.from(ctx.code.subarray(0, 2)),
      beforeOld = physical(ctx, old);
    assert.equal(ctx.code[mutation.mutation_offset], mutation.original);
    upload(
      ctx,
      PC + mutation.mutation_offset,
      Buffer.from([mutation.changed]),
      `sole consumed target upload/${mutation.name}`,
    );
    cancel(ctx, 1);
    let retainedOld;
    const staleOld = run(
      ctx,
      old,
      0,
      "stale original before invalid pointers/cancel/budget",
      {},
      4,
      true,
      () => {
        retainedOld = physical(ctx, old);
        assert.deepEqual(retainedOld, beforeOld);
      },
    );
    counts.stale_calls++;
    const oldStaleArena = staleOld.after_arena;
    cancel(ctx, 0);
    const committed = snapshot(ctx);
    ctx.currentCodeArtifact = artifact(`current-${ctx.ordinal}.x86`, ctx.code);
    ctx.tailArtifact = artifact(`tail-${ctx.ordinal}.x86`, ctx.code.subarray(2, 15));
    const current = compile(ctx, "current-tail", [[scan.next, 13]], scan),
      published = snapshot(ctx);
    for (const field of ["state_hex", "exit_hex", "helper_hex"])
      assert.equal(published[field], committed[field], `fresh compile preserves ${field}`);
    if (owner === "replacement") assert.notEqual(current.generation, old.generation);
    else assert.notDeepEqual([current.low, current.high], [old.low, old.high]);
    assert.deepEqual(physicalCpu(ctx), committedCpu, "fresh tail compiler preserves physical committed CPU");
    const originalReplay = headReplay(originalHead, committedCpu),
      currentReplay = headReplay(Buffer.from(ctx.code.subarray(0, 2)), committedCpu);
    for (const replay of [originalReplay, currentReplay]) {
      assert.equal(replay.old_ecx, committedCpu.registers[1]);
      assert.equal(replay.raw_count, committedCpu.registers[1] % 256);
      assert.notDeepEqual([replay.registers_after, replay.flags], [committedCpu.registers, committedCpu.flags]);
    }
    assert.equal(row.expected.value, mutation.name === "esp-destination" ? 0x200000 : mutation.name === "ecx-direction" ? 9 : 0x500);
    assert.equal(row.expected.flags, 2);
    assert.equal(originalReplay.value, mutation.name === "esp-destination" ? 0x800 : mutation.name === "ecx-direction" ? 0x1200 : 0x140000);
    assert.equal(
      currentReplay.value,
      mutation.name === "eax-same-opcode"
        ? 0x140000
        : mutation.name === "ecx-direction"
          ? 0x09000000
          : 0xe699e26a,
    );
    assert.equal(currentReplay.alias, mutation.name === "esp-destination" ? 5 : mutation.alias);
    if (mutation.name === "esp-destination") {
      assert.equal(currentReplay.flags, 3);
      assert.equal(currentReplay.registers_after[4], committedCpu.registers[4]);
    }
    const tail = consumers(ctx, current, scan, true),
      completedArena = tail.after_arena;
    const expectedRegisters = [...row.registers_after];
    putLowByte(expectedRegisters, 0, row.expected.cf);
    putLowByte(expectedRegisters, 2, row.expected.of);
    expectedRegisters[3] = 0xdeadbeef;
    assert.deepEqual(cpu(ctx).registers, expectedRegisters);
    assert.equal(snapshot(ctx).helper_hex, Buffer.alloc(40, 0x5a).toString("hex"));
    const beforeTail = physical(ctx, current);
    assert.equal(ctx.code[2], 0x0f);
    upload(ctx, scan.next, Buffer.from([0x0f]), "same consumed tail byte");
    cancel(ctx, 1);
    let retainedTail;
    const staleTail = run(
      ctx,
      current,
      0,
      "stale current tail before invalid pointers/cancel/budget",
      {},
      4,
      true,
      () => {
        retainedTail = physical(ctx, current);
        assert.deepEqual(retainedTail, beforeTail);
      },
    );
    counts.stale_calls++;
    const tailStaleArena = staleTail.after_arena;
    pure(ctx, "close", []);
    const closedRows = [];
    for (const unit of [old, current]) {
      const close = closed(ctx, unit, "closed retained child before stale/pointer/cancel/budget");
      close.arena = runs[close.run_ordinal - 1].after_arena;
      closedRows.push(close);
    }
    mutations.push({
      context: ctx.ordinal,
      case: mutation.name,
      original_module: old.file,
      current_module: current.file,
      address: PC + mutation.mutation_offset,
      offset: mutation.mutation_offset,
      original: mutation.original,
      changed: mutation.changed,
      causation:
        "one same/changing consumed target host upload before first stale; no permission mutation",
      target_run_ordinal: row.run_ordinal,
      stale_old_run_ordinal: staleOld.ordinal,
      old_stale_arena: oldStaleArena,
      retained_old_after_stale: retainedOld,
      current_tail_run_ordinal: tail.ordinal,
      completed_tail_arena: completedArena,
      stale_tail_run_ordinal: staleTail.ordinal,
      tail_stale_arena: tailStaleArena,
      retained_tail_after_stale: retainedTail,
      closed: closedRows,
      current_code_artifact: ctx.currentCodeArtifact,
      tail_artifact: ctx.tailArtifact,
      tail_entry: scan.next,
      tail_bytes: 13,
      no_replay: {
        committed_registers: committedCpu.registers,
        committed_ecx: committedCpu.registers[1],
        committed_flags: committedCpu.flags,
        original_target: originalReplay,
        current_head_target: currentReplay,
        authority:
          "mathematical counterfactuals from physical committed full GPR/ECX/live CL plus decoded current destination; no replay executed; two-byte D3 head has no earlier MOV prefix",
      },
    });
  }
assert.equal(caseCensus.size, plan.targets);
for (const group of ["regular_targets", "chain_targets", "currency_targets"])
  assert.equal(counts[group], plan[group]);
for (const name of [
  "targets",
  "contexts",
  "modules",
  "seeds",
  "generated_calls",
  "retired",
  "raw_arenas",
  "host_calls",
  "arena_checks",
])
  assert.equal(counts[name], plan[name], `source-derived ${name}`);
assert.deepEqual(
  [
    counts.left_targets,
    counts.right_targets,
    counts.zero_targets,
    counts.one_targets,
    counts.multi_targets,
    counts.effective_zero_targets,
  ],
  [916, 914, 270, 252, 1308, 270],
);
assert.deepEqual([counts.setb, counts.seto, counts.jumps, counts.canaries], [1794, 1794, 1794, 42]);
assert.deepEqual([counts.producers, counts.clc, counts.stc, counts.cmc], [36, 12, 12, 12]);
assert.deepEqual(
  [
    counts.cancel_calls,
    counts.zero_budget_calls,
    counts.malformed_calls,
    counts.cold_calls,
    counts.direct_guard_controls,
    counts.stale_calls,
    counts.closed_calls,
  ],
  [6, 6, 6, 6, 8, 12, 16],
);
assert.deepEqual([hostInputs.length, sourceSpans.length], [plan.host_inputs, plan.source_spans]);
const savedArenas = artifact("arena-snapshots.bin", Buffer.concat(rawFrames));
assert.equal(savedArenas.bytes, plan.raw_arenas * SIZE);
const sourcePinsAfter = sourceIdentities();
assert.deepEqual(sourcePinsAfter, sourcePins);
assert.equal(hash(readFileSync(enginePath)), engineSha256);
const ownerCounts = Object.fromEntries(
  OWNERS.map((owner) => [owner, targets.filter((row) => row.owner === owner).length]),
);
assert.deepEqual(ownerCounts, { standalone: 608, replacement: 611, resident: 611 });
const managedPageRosters = managedContexts.map((ctx) => {
  assert.equal(ctx.guestPagesRetired, true, "all managed guest page owners were closed");
  const modeledPage = Buffer.alloc(4096);
  modeledPage.set(ctx.code);
  return {
    context: ctx.ordinal,
    owner: ctx.owner,
    page_limit: 1,
    final_state: "retired-by-successful-close",
    current_mapped_pages: [],
    retired_pages: [
      {
        address: PC,
        bytes: 4096,
        permissions_before_close: ctx.codePermissions,
        modeled_image: {
          bytes: modeledPage.length,
          hex: modeledPage.toString("hex"),
          sha256: hash(modeledPage),
        },
        known_uploaded_extent: ctx.code.length,
        upload_input_ordinals: hostInputs
          .filter((row) => row.context === ctx.ordinal && row.kind === "upload")
          .map((row) => row.ordinal),
      },
    ],
    authority:
      "model from successful zero-initialized map plus exact successful host uploads; no physical guest-RAM readback; retained EngineInstance arena is separate",
  };
});
assert.equal(managedPageRosters.length, plan.managed_page_rosters);
const result = {
  status: "ok",
  profile:
    "finite D3 register-DWORD carry rotates by captured CL: native-Rust standalone Node plus actual-engine-Wasm replacement/resident",
  command: [process.execPath, process.argv[1], enginePath, output, root],
  environment: {
    node: process.version,
    v8: process.versions.v8,
    platform: process.platform,
    arch: process.arch,
  },
  engine: {
    path: enginePath,
    bytes: engineBytes.length,
    sha256: engineSha256,
    caller_sha256: process.env.RING3_ENGINE_SHA256 ?? null,
  },
  source,
  source_pins: sourcePins,
  source_pins_after: sourcePinsAfter,
  selected_input_census: { repo_paths: 21, engine: 1, total: 22 },
  plan,
  counts,
  corpus: {
    regular_cases_per_owner: 584,
    owner_target_counts: ownerCounts,
    pure_native_node_targets: 608,
    actual_engine_compiled_bound_targets: 1222,
    case_keys: caseCensus.size,
    semantic_input_keys: semanticInputs.size,
    semantic_key:
      "direction/destination/oldDWORD/physical oldECX low8/FLAGS, owner omitted; finite coupled partitions and evolved chains, no prospective distinct-key count",
    raw_counts: RAW,
    basis_values: BASE,
    ecx_basis_high24: ECX_BASE,
    flags: VALID_FLAGS,
    count_source: "each target's physical before full ECX low8; high24 retained for non-ECX destinations",
    regular_groups_per_owner: {
      "eax-ecx-all32q": 256,
      "other-gpr-selected-count": 96,
      "eax-ecx-high-raw": 24,
      "eax-ecx-carry-style-split": 80,
      "eax-ecx-value-partition": 128,
    },
    other_gpr_raw: [0, 1, 2, 31],
    eax_ecx_high_raw: EDGE_RAW,
    style_raw: STYLE_RAW,
    style_flags: STYLE_FLAGS,
    extra_values: VALUES,
    ecx_value_high24: ECX_HIGH,
    value_raw: VALUE_RAW,
    registers: REG,
    destinations: DESTINATIONS,
    literal_anchors: ANCHORS,
    currency_seeds: currencyCases,
  },
  banks: banks.map(
    ({ name, kind, group, bytes, packed, cold, instructions, specs, scans, groups, artifact }) => ({
      name,
      kind,
      group,
      bytes: bytes.length,
      packed,
      cold,
      instructions,
      specs,
      scans,
      groups,
      artifact,
    }),
  ),
  contexts,
  managed_page_rosters: managedPageRosters,
  modules,
  targets,
  runs,
  chain_pairs: chainRows,
  mutations,
  host_inputs: hostInputs,
  host_calls: hostCalls,
  raw_arena_records: rawRecords,
  raw_arena_artifact: savedArenas,
  artifacts,
  evidence_limits: {
    oracle:
      "unchanged sibling33-character oldCF||oldDWORD ring oracle and22 literal anchors; independent checker uses integer carry walk with physical D3/ModRM and beforeECX decode",
    owners:
      "three standalone modules are native Rust wrapper output executed by Node in two independent memory arenas;1222 managed targets are compiled by frozen engine Wasm",
    flags:
      "FLAGS2/3/cd6/cd7 finite seeds; no128FLAGSdomain claim; q0 preserves all flags; q1 definedOF; q2..31 OF0 is undefined-flag policy, not hardware measurement",
    chains:
      "real CLC/STC/CMC then two consecutive budget1 D3 DWORD targets with separate complete frames; consumers only after second; second count is recaptured from changed physical ECX when ECX is destination",
    raw_frames:
      "all3712 generated calls save one physical before/after full4236B pair including targets/producers/consumers/preflight/stale/closed; target records reference those frames; full hash joins are not all-body independent semantic modelling",
    temporal:
      "stale full live module callbacks and closed known retained-arena callbacks directly follow status assertion before post-call check/snapshot; saved files do not certify timing",
    code: "five authored banks/current code/tails and host inputs are physical files;10 managed4096B page rosters/images are map/upload models retired by close, no physical Read32 RAM dump",
    host_schedule:
      "72 recorded map/upload/protect/compiler/guard/close calls;10 opens and getter groups covered by initialization checks; two pure initializations separate",
    scope:
      "register-DWORD D3/2,3 only;16 encoded direction/destination identities with finite runtime CL/value/FLAGS partitions, no raw256Cartesian; no memory/general memoryC1/word/x64/store/exit6/helper/local/cap/FLAGSvalidation expansion, wholeWasmbodycertificate, fullCI/bootstrap/SDK/browser/graphics/game/performance claim",
  },
  pre_execution_findings: [
    {
      label: "REUSED-ORACLE",
      note: "the sibling R438 arithmetic body/22anchors are unchanged; its historical q2-anchor correction is reused authority, not a fresh D3 oracle defect or campaign",
    },
    {
      label: "ECX-COUPLING",
      note: "physical full before ECX owns CL even when ECX is destination; seeds reject impossible independent value/count; second ECX count is newCL2 or0 and differs from first-count-reuse counterfactual",
    },
    {
      label: "FRAME-SCOPE",
      note: "new runner captures every generated call rather than inheriting target-only raw scope; target/currency records reuse run frames, not duplicate captures",
    },
    {
      label: "OWNER-ORIGIN",
      note: "608 native-Rust-emitted/Node pure and1222 engine-compiled bound targets; no all1830-engine-origin claim",
    },
    {
      label: "CLOSED-ARENA",
      note: "close retires guest memory/publication but keeps known EngineInstance arena; full retained-arena equality does not assert guest page survival",
    },
  ],
};
assert.equal(artifacts.length + 1, plan.files);
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), { flag: "wx" });
assert.deepEqual(
  readdirSync(output).sort(),
  [...artifacts.map((row) => row.path), "result.json"].sort(),
  "exact physical output census",
);
const resultBytes = readFileSync(join(output, "result.json"));
console.log(
  JSON.stringify({
    status: "ok",
    engine_sha256: engineSha256,
    result_sha256: hash(resultBytes),
    targets: counts.targets,
    owner_targets: ownerCounts,
    contexts: counts.contexts,
    modules: counts.modules,
    generated_calls: counts.generated_calls,
    retired: counts.retired,
    raw_arenas: counts.raw_arenas,
    files: plan.files,
    output,
  }),
);
