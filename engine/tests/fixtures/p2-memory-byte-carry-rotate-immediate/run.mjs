import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../support/engine.mjs";
import { ANCHORS, rotate } from "./oracle.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, "expected current engine, output directory and repository root");
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const { bytes: engineBytes, module: engineModule, sha256: engineSha256 } = readEngine(enginePath);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236,
  TRANSFER = 140,
  PC = 0x1000,
  COLD = 0x1f00,
  KEEP = 0x3000,
  DATA = 0x4000,
  SECOND = 0x5000,
  BYTE = 0x4010,
  HIGH = 0xc8411000;
const FLAGS = [2, 0xcd7],
  STYLE_FLAGS = [3, 0xcd6],
  STYLE_RAW = [9, 10, 18, 19, 27, 28];
const REG = [
  0x1234807f, 0xa1b2c378, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef,
];
const pattern = Buffer.from(Array.from({ length: 4096 }, (_, index) => (index % 251) + 1));
const plan = {
  contexts: 16,
  modules: 62,
  native_modules: 10,
  actual_modules: 52,
  seeds: 632,
  regular_targets: 500,
  native_targets: 140,
  actual_regular_targets: 360,
  producer_targets: 24,
  endpoint_targets: 24,
  repaired_targets: 72,
  code_targets: 12,
  targets: 632,
  setb: 632,
  seto: 632,
  jumps: 632,
  canaries: 88,
  producers: 24,
  prefixes: 72,
  fault_calls: 144,
  read_faults: 96,
  write_faults: 48,
  repairs: 72,
  map_only_repairs: 24,
  permission_only_repairs: 48,
  code_stores: 12,
  same_value_code_stores: 8,
  changing_code_stores: 4,
  stale_calls: 12,
  closed_calls: 62,
  direct_guards: 32,
  generated_calls: 2910,
  retired: 2712,
  maps: 108,
  unmaps: 28,
  protects: 268,
  host_uploads: 680,
  host_uploaded_bytes: 344660,
  host_read8: 632,
  pages: 348,
  page_bytes: 1425408,
  raw_arenas: 6452,
  raw_arena_bytes: 27330672,
  files: 70,
};
const counts = Object.fromEntries(Object.keys(plan).map((name) => [name, 0]));
const artifacts = [],
  contexts = [],
  modules = [],
  targets = [],
  runs = [],
  faults = [],
  codeStores = [],
  hostInputs = [],
  hostCalls = [],
  arenaRows = [],
  arenaFrames = [],
  pageRows = [],
  pageFrames = [];
let events = 0,
  activeCall = null,
  lastContext = null;
function artifact(path, bytes, write = true) {
  assert.ok(!artifacts.some((row) => row.path === path));
  if (write) writeFileSync(join(output, path), bytes, { flag: "wx" });
  const row = { path, bytes: bytes.length, sha256: hash(bytes) };
  artifacts.push(row);
  return row;
}
artifact("engine.wasm", engineBytes);
const nativeManifestBytes = readFileSync(join(output, "native-manifest.json")),
  nativeManifest = JSON.parse(nativeManifestBytes);
artifact("native-manifest.json", nativeManifestBytes, false);
assert.equal(nativeManifest.length, 10);
const sourcePaths = [
  "engine/src/cpu/x86/ir.rs",
  "engine/src/cpu/x86/decode/integer.rs",
  "engine/src/cpu/x86/decode/profile.rs",
  "engine/src/cpu/dbt/region.rs",
  "engine/src/cpu/dbt/wasm/integer.rs",
  "engine/src/cpu/dbt/wasm/memory.rs",
  "engine/src/cpu/dbt/wasm/memory/byte_store.rs",
  "engine/src/cpu/dbt/wasm/memory/narrow.rs",
  "engine/src/process/instance.rs",
  "engine/src/process/resident.rs",
  "engine/src/abi/arena.rs",
  "engine/src/abi/memory_helper.rs",
  "engine/src/abi/x86/mod.rs",
  "engine/src/abi/x86/state.rs",
  "engine/src/abi/x86/exit.rs",
  "engine/tests/cpu_memory_byte_carry_rotate_immediate.rs",
  "engine/tests/cpu_memory_byte_carry_rotate_immediate_wasm.rs",
  "engine/tests/cpu_memory_byte_carry_rotate_one.rs",
  "engine/tests/cpu_byte_carry_rotate_immediate_one.rs",
  "engine/tests/cpu_memory_byte_rotate.rs",
  "engine/tests/cpu_byte_carry_rotate_immediate.rs",
  "engine/tests/fixtures/p2-byte-carry-rotate-immediate-one/run.mjs",
  "engine/tests/fixtures/p2-memory-byte-carry-rotate-immediate/run.mjs",
  "engine/tests/fixtures/p2-memory-byte-carry-rotate-immediate/oracle.mjs",
  "engine/tests/fixtures/p2-memory-byte-carry-rotate-immediate/oracle.test.mjs",
  "engine/tests/fixtures/support/engine.mjs",
];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
const pinSources = () =>
  Object.fromEntries(
    sourcePaths.map((path) => {
      const bytes = readFileSync(join(root, path));
      return [path, { bytes: bytes.length, sha256: hash(bytes) }];
    }),
  );
const sourcePins = pinSources();
const fullText = readFileSync(join(root, "target/p2-memory-binary-spec/253667-093-sdm-vol-2b.txt"));
assert.equal(hash(fullText), "f5e6dc689d41655d64792512bfe8add56fc4ac97203aab5c1377a72bac291a35");
const chapter = Buffer.from(
  fullText.toString("utf8").split("\n").slice(27173, 27413).join("\n") + "\n",
);
assert.equal(chapter.length, 10932);
assert.equal(hash(chapter), "f502898d90ce6ea7a6a513d1c8761e507af8ad92fac581e1240ea413b78aa74d");
const savedChapter = artifact("intel-rotate.txt", chapter);
for (const [kind, value, raw, incoming, expected, flags] of ANCHORS) {
  const result = rotate(kind, value, raw, incoming);
  assert.equal(result.value, expected);
  assert.equal(result.flags, flags);
}

function bank(kind, extension) {
  const bytes = Buffer.alloc(4096, 0xcc),
    groups = [],
    ea = [],
    fault = [];
  let at = 0;
  function append(form, raw, prefix = 0, canary = false, formIndex = null) {
    const entry = PC + at;
    bytes.set(form, at);
    const rotatePC = entry + prefix,
      next = entry + form.length,
      rawPC = next - 1;
    at += form.length;
    bytes.set([0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2], at);
    at += 6;
    if (canary) {
      bytes.set([0xbb, 0xef, 0xbe, 0xad, 0xde], at);
      at += 5;
    }
    bytes[at] = 0xe9;
    bytes.writeInt32LE(COLD - (PC + at + 5), at + 1);
    at += 5;
    return {
      entry,
      target: rotatePC,
      next,
      raw_pc: rawPC,
      raw,
      canary,
      form: formIndex,
      spec: [entry, PC + at - entry],
    };
  }
  for (let ordinal = 0; ordinal < 5; ordinal++) {
    const raws =
      ordinal < 4 ? Array.from({ length: 8 }, (_, index) => ordinal * 8 + index) : [32, 33, 255];
    groups.push(raws.map((raw) => append([0xc0, 5 | extension, 0x10, 0x40, 0, 0, raw], raw)));
  }
  const forms = [
    [0xf8, 0xc0, extension, 2],
    [0xf9, 0xc0, 0x41 | extension, 0xf0, 10],
    [0xf5, 0xc0, 0x44 | extension, 0x4a, 0x11, 19],
    [0xc0, 0x84 | extension, 0xf3, 0xe0, 0xff, 0xff, 0xff, 28],
    [0xc0, 0x04 | extension, 0x24, 0],
    [0xc0, 0x45 | extension, 0, 9],
    [0x8d, 0x5b, 1, 0xc0, 0x06 | extension, 18],
    [0xc0, 0x87 | extension, 0, 1, 0, 0, 27],
  ];
  for (const [index, form] of forms.entries())
    ea.push(append(form, form.at(-1), index < 3 ? 1 : index === 6 ? 3 : 0, index === 7, index));
  for (const raw of [0, 9, 10])
    fault.push(append([0x8d, 0x5b, 1, 0xc0, 0x06 | extension, raw], raw, 3, true, 6));
  assert.equal(at, 833);
  bytes.set([0x0f, 0x0b], COLD - PC);
  assert.deepEqual(
    bytes,
    readFileSync(join(output, `memory-${kind}.x86`)),
    "independent Rust/JS bank authoring",
  );
  const saved = artifact(`memory-${kind}.x86`, bytes, false);
  return { kind, extension, bytes, groups, ea, fault, packed: at, saved };
}
const banks = [bank("left", 0x10), bank("right", 0x18)];
process.on("uncaughtException", (error) => {
  const currentContext = activeCall?.ctx ?? lastContext;
  const currentBytes = currentContext === null ? null : arena(currentContext);
  const current =
    currentContext === null
      ? null
      : {
          context: currentContext.ordinal,
          module: activeCall?.unit.file ?? null,
          label: activeCall?.label ?? "outside generated call",
          args: activeCall?.args ?? null,
          actual_status: activeCall?.actualStatus ?? null,
          arena_sha256: hash(currentBytes),
        };
  if (currentBytes !== null)
    writeFileSync(join(output, "failure-current-arena.bin"), currentBytes, { flag: "wx" });
  writeFileSync(join(output, "failure-arena-snapshots.bin"), Buffer.concat(arenaFrames), {
    flag: "wx",
  });
  writeFileSync(join(output, "failure-page-snapshots.bin"), Buffer.concat(pageFrames), {
    flag: "wx",
  });
  writeFileSync(
    join(output, "failure.json"),
    JSON.stringify(
      {
        status: "failed",
        error: String(error.stack ?? error),
        current,
        plan,
        counts,
        source_pins: sourcePins,
        contexts,
        modules,
        targets,
        runs,
        faults,
        code_stores: codeStores,
        host_inputs: hostInputs,
        host_calls: hostCalls,
        raw_arena_records: arenaRows,
        pages: pageRows,
        artifacts,
      },
      null,
      2,
    ),
    { flag: "wx" },
  );
  console.error(error);
  process.exitCode = 1;
});
function record(magic, size, fields, version = 1) {
  const bytes = Buffer.alloc(size);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4));
  return bytes;
}
const state = (registers, pc, flags) => record("R3ST", 56, [...registers, pc, flags]);
const exit = (reason, retired, detail = 0, address = 0, access = 0, version = 2) =>
  record("R3EX", 40, [reason, retired, detail, address, access, access ? 1 : 0], version);
const wordHelper = (value) => record("R3MH", 40, [0, value, 0, 0, 0, 0]);
const readHelper = (value, detail = 0, address = 0) =>
  record(
    "R3MH",
    40,
    [detail ? 1 : 0, detail ? 0 : value, detail, detail ? address : 0, detail ? 1 : 0, 1],
    2,
  );
const storeHelper = (detail = 0, address = 0) =>
  record("R3MH", 40, [detail ? 1 : 0, 0, detail, detail ? address : 0, detail ? 2 : 0, 1], 3);
const words = (values) => {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
};
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
    `${ctx.owner}/${ctx.bank.kind}/${ctx.ordinal}/${label}: complete arena`,
  );
}
function frame(ctx, label, unit) {
  check(ctx, label);
  const bytes = arena(ctx),
    row = {
      context: ctx.ordinal,
      module: unit.file,
      label,
      offset: arenaFrames.length * SIZE,
      bytes: SIZE,
      sha256: hash(bytes),
    };
  arenaFrames.push(bytes);
  arenaRows.push(row);
  counts.raw_arenas++;
  counts.raw_arena_bytes += SIZE;
  return row;
}
function host(ctx, name, args, status = 0) {
  check(ctx, `before host ${name}`);
  assert.equal(ctx.api[name](...args), status, name);
  check(ctx, `host ${name}`);
  hostCalls.push({ event: ++events, context: ctx.ordinal, name, args, status });
}
function request(ctx, bytes, label) {
  refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER);
  ctx.expected.set(bytes, TRANSFER);
  check(ctx, label);
  hostInputs.push({
    event: ++events,
    context: ctx.ordinal,
    label,
    arena_offset: TRANSFER,
    bytes: bytes.length,
    hex: bytes.toString("hex"),
  });
}
function patch(ctx, address, bytes) {
  for (const [index, value] of bytes.entries()) {
    const at = address + index,
      page = Math.floor(at / 4096) * 4096;
    assert.ok(ctx.pages.has(page));
    ctx.pages.get(page)[at % 4096] = value;
  }
}
function upload(ctx, address, bytes, label = "explicit host upload") {
  request(ctx, bytes, label);
  host(ctx, "upload", [address, bytes.length]);
  patch(ctx, address, bytes);
  counts.host_uploads++;
  counts.host_uploaded_bytes += bytes.length;
  hostInputs.push({
    event: ++events,
    context: ctx.ordinal,
    label,
    address,
    bytes: bytes.length,
    hex: bytes.toString("hex"),
  });
}
function map(ctx, address, bytes = pattern) {
  host(ctx, "map", [address, 1, 3]);
  ctx.pages.set(address, Buffer.alloc(4096));
  ctx.permissions.set(address, 3);
  counts.maps++;
  if (bytes !== null) upload(ctx, address, bytes, "patterned page");
}
function protect(ctx, address, permissions) {
  host(ctx, "protect", [address, 1, permissions]);
  ctx.permissions.set(address, permissions);
  counts.protects++;
}
function unmap(ctx, address) {
  host(ctx, "unmap", [address, 1]);
  ctx.pages.delete(address);
  ctx.permissions.delete(address);
  counts.unmaps++;
}
function fresh(owner, selected, role) {
  const ordinal = ++counts.contexts,
    expected = Buffer.alloc(SIZE);
  expected.set(state(Array(8).fill(0), 0, 2));
  expected.set(exit(1, 0, 0, 0, 0, 1), 56);
  expected.set(wordHelper(0), 100);
  const instance = new WebAssembly.Instance(engineModule, {}),
    ctx = {
      owner,
      ordinal,
      bank: selected,
      role,
      low: ordinal,
      high: HIGH,
      expected,
      pages: new Map(),
      permissions: new Map(),
      units: [],
    };
  ctx.memory = instance.exports.memory;
  ctx.api = {};
  const arities = {
    open: 3,
    close: 0,
    arena_ptr: 0,
    map: 3,
    unmap: 2,
    protect: 3,
    upload: 2,
    read32: 1,
    read8: 1,
    compile_entries: 2,
    compile_resident: 1,
    generation: 0,
    module_ptr: 0,
    module_len: 0,
    guard: 6,
    guard_resident: 7,
    store8: 2,
    store_resident8: 6,
  };
  for (const [name, arity] of Object.entries(arities)) {
    ctx.api[name] = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof ctx.api[name], "function", name);
    assert.equal(ctx.api[name].length, arity, name);
  }
  assert.equal(ctx.api.open(7, ctx.low, ctx.high), 0);
  ctx.base = ctx.api.arena_ptr() >>> 0;
  lastContext = ctx;
  check(ctx, "independent initialized arena");
  host(ctx, "map", [PC, 1, 7]);
  ctx.pages.set(PC, Buffer.alloc(4096));
  ctx.permissions.set(PC, 7);
  counts.maps++;
  upload(ctx, PC, selected.bytes, "authored full code bank");
  for (const address of [KEEP, DATA, SECOND, 0xfffff000]) map(ctx, address);
  contexts.push({
    context: ordinal,
    owner,
    kind: selected.kind,
    role,
    key_low: ctx.low,
    key_high: ctx.high,
    page_limit: 7,
  });
  return ctx;
}
function instantiate(ctx, bytes, file, binding, scans, pureTail = false, origin = "actual-engine") {
  const module = new WebAssembly.Module(bytes),
    imports = (pureTail ? ["guard"] : ["guard", "read8", "store8"]).map((name) =>
      ctx.owner === "replacement"
        ? name
        : name === "guard"
          ? "guard_resident"
          : name === "store8"
            ? "store_resident8"
            : name,
    );
  assert.deepEqual(WebAssembly.Module.imports(module), [
    { module: "env", name: "memory", kind: "memory" },
    ...imports.map((name) => ({ module: "ring3", name, kind: "function" })),
  ]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: "run", kind: "function" }]);
  const child = new WebAssembly.Instance(module, { env: { memory: ctx.memory }, ring3: ctx.api });
  assert.equal(child.exports.run.length, 4);
  const unit = {
    ...binding,
    run: child.exports.run,
    bytes,
    file,
    scans,
    origin,
    exitVersion: pureTail ? 1 : 2,
  };
  ctx.units.push(unit);
  counts.modules++;
  counts[origin === "native-rust" ? "native_modules" : "actual_modules"]++;
  modules.push({
    context: ctx.ordinal,
    owner: ctx.owner,
    kind: ctx.bank.kind,
    origin,
    file,
    ...binding,
    imports,
    exit_version: unit.exitVersion,
    scans,
    sha256: hash(bytes),
    bytes: bytes.length,
    key_low: ctx.low,
    key_high: ctx.high,
    code_page_sha256: hash(ctx.pages.get(PC)),
  });
  return unit;
}
function compile(ctx, scans, role, pureTail = false) {
  const specs = scans.map((scan) => scan.spec);
  request(
    ctx,
    words(ctx.owner === "replacement" ? specs.map(([entry]) => entry) : specs.flat()),
    "compiler request",
  );
  let binding;
  if (ctx.owner === "replacement") {
    host(ctx, "compile_entries", [specs.length, 0]);
    binding = {
      generation: ctx.api.generation(),
      pointer: ctx.api.module_ptr() >>> 0,
      length: ctx.api.module_len() >>> 0,
    };
    assert.ok(binding.generation > 0);
  } else {
    check(ctx, "before resident compile");
    assert.equal(ctx.api.compile_resident(specs.length), 0);
    refresh(ctx);
    binding = Object.fromEntries(
      ["low", "high", "pointer", "length"].map((name, index) => [
        name,
        ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true),
      ]),
    );
    assert.ok(binding.low !== 0 || binding.high !== 0);
    ctx.expected.set(
      words([1, 24, binding.low, binding.high, binding.pointer, binding.length]),
      TRANSFER,
    );
    check(ctx, "resident metadata only");
    hostCalls.push({
      event: ++events,
      context: ctx.ordinal,
      name: "compile_resident",
      args: [specs.length],
      status: 0,
      binding,
    });
  }
  assert.ok(binding.length > 8 && binding.length <= 65536);
  const bytes = Buffer.from(
    refresh(ctx).bytes.subarray(binding.pointer, binding.pointer + binding.length),
  );
  const file = `actual-${ctx.ordinal}-${role}.wasm`;
  artifact(file, bytes);
  return instantiate(ctx, bytes, file, binding, scans, pureTail);
}
function native(ctx, actual, group) {
  const row = nativeManifest.find((row) => row.kind === ctx.bank.kind && row.group === group);
  assert.ok(row);
  const expected = {
    key_low: ctx.low,
    key_high: ctx.high,
    generation: actual.generation,
    entries: actual.scans.map((scan) => scan.entry),
  };
  assert.deepEqual(
    {
      key_low: row.key_low,
      key_high: row.key_high,
      generation: row.generation,
      entries: row.entries,
    },
    expected,
    "independently observed actual/native identity join",
  );
  host(ctx, "guard", [
    row.key_low,
    row.key_high,
    row.generation,
    ctx.base,
    ctx.base + 56,
    ctx.base + 96,
  ]);
  const bytes = readFileSync(join(output, row.file));
  artifact(row.file, bytes, false);
  const unit = instantiate(
    ctx,
    bytes,
    row.file,
    {
      generation: row.generation,
      native_identity: row,
      actual_observed_identity: expected,
      matched_guard_status: 0,
    },
    actual.scans,
    false,
    "native-rust",
  );
  return unit;
}
function seed(ctx, registers, pc, flags) {
  ctx.expected.set(state(registers, pc, flags));
  ctx.expected.set(exit(3, 0), 56);
  ctx.expected.writeUInt32LE(0, 96);
  ctx.expected.set(readHelper(0xdecafbad), 100);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 140), ctx.base);
  check(ctx, "explicit initial CPU/helper seed");
  ctx.seed = ++counts.seeds;
  hostInputs.push({
    event: ++events,
    seed: ctx.seed,
    context: ctx.ordinal,
    label: "initial CPU/helper seed",
    arena_offset: 0,
    bytes: 140,
    hex: ctx.expected.subarray(0, 140).toString("hex"),
  });
}
function cancel(ctx, value) {
  ctx.expected.writeUInt32LE(value, 96);
  refresh(ctx).view.setUint32(ctx.base + 96, value, true);
}
function generated(ctx, unit, args, label, status = 0, update = null, afterReturn = null) {
  const before = frame(ctx, `before ${label}`, unit);
  if (update !== null) update();
  activeCall = { ctx, unit, args, label };
  const actualStatus = unit.run(...args);
  activeCall.actualStatus = actualStatus;
  assert.equal(actualStatus, status, label);
  if (afterReturn !== null) afterReturn();
  check(ctx, label);
  const after = frame(ctx, label, unit);
  counts.generated_calls++;
  const row = {
    event: ++events,
    ordinal: runs.length + 1,
    seed: ctx.seed,
    context: ctx.ordinal,
    module: unit.file,
    label,
    operation: label,
    args,
    expected_status: status,
    status: actualStatus,
    before,
    after,
  };
  runs.push(row);
  activeCall = null;
  return { run: row.ordinal, before, after };
}
function run(
  ctx,
  unit,
  budget,
  label,
  registers,
  pc,
  flags,
  reason = 1,
  retired = 1,
  helperBytes,
  detail = 0,
  fault = 0,
  access = 0,
) {
  const frames = generated(
    ctx,
    unit,
    [ctx.base, ctx.base + 56, budget, ctx.base + 96],
    label,
    0,
    () => {
      ctx.expected.set(state(registers, pc, flags));
      ctx.expected.set(exit(reason, retired, detail, fault, access, unit.exitVersion), 56);
      if (helperBytes !== undefined) ctx.expected.set(helperBytes, 100);
    },
  );
  counts.retired += retired;
  return frames;
}
function preflight(ctx, unit, registers, pc, flags) {
  run(ctx, unit, 0, "zero budget before guest access", registers, pc, flags, 1, 0);
  cancel(ctx, 1);
  run(ctx, unit, 1, "cancel before guest access", registers, pc, flags, 2, 0);
  cancel(ctx, 0);
}
function pages(ctx, label, addresses = [...ctx.pages.keys()]) {
  for (const address of addresses) {
    const expected = ctx.pages.get(address),
      permissions = ctx.permissions.get(address);
    assert.ok(expected);
    if (!(permissions & 1)) protect(ctx, address, permissions | 1);
    const actual = Buffer.alloc(4096);
    for (let offset = 0; offset < 4096; offset += 4) {
      check(ctx, "before diagnostic Read32");
      assert.equal(ctx.api.read32(address + offset), 0);
      ctx.expected.set(wordHelper(expected.readUInt32LE(offset)), 100);
      check(ctx, "diagnostic Read32 helper only");
      actual.writeUInt32LE(refresh(ctx).view.getUint32(ctx.base + 120, true), offset);
    }
    assert.deepEqual(actual, expected, "complete physical page and untouched neighbors");
    if (!(permissions & 1)) protect(ctx, address, permissions);
    const row = {
      event: ++events,
      context: ctx.ordinal,
      label,
      address,
      permissions,
      offset: pageFrames.length * 4096,
      bytes: 4096,
      sha256: hash(actual),
    };
    pageFrames.push(actual);
    pageRows.push(row);
    counts.pages++;
    counts.page_bytes += 4096;
  }
}
function addressRegisters(form, address) {
  const registers = [...REG];
  if (form === 0) registers[0] = address;
  if (form === 1) registers[1] = address + 16;
  if (form === 2) {
    registers[1] = 3;
    registers[2] = address - 6 - 17;
  }
  if (form === 3) {
    registers[6] = 2;
    registers[3] = address + 32 - 16;
  }
  if (form === 4) registers[4] = address;
  if (form === 5) registers[5] = address;
  if (form === 6) registers[6] = address;
  if (form === 7) registers[7] = address - 0x100;
  return registers;
}
function target(ctx, unit, registers, scan, address, flags, purpose, reason = 1) {
  const oldByte = ctx.pages.get(Math.floor(address / 4096) * 4096)[address % 4096],
    result = rotate(ctx.bank.kind, oldByte, scan.raw, flags);
  patch(ctx, address, Buffer.from([result.value]));
  const frames = run(
    ctx,
    unit,
    1,
    "checked byte carry rotate commits before consumers",
    registers,
    scan.next,
    result.flags,
    reason,
    1,
    storeHelper(),
  );
  check(ctx, "before separate post-target Read8");
  activeCall = { ctx, unit, args: [address], label: "post-target diagnostic Read8" };
  const readStatus = ctx.api.read8(address);
  activeCall.actualStatus = readStatus;
  assert.equal(readStatus, 0);
  ctx.expected.set(readHelper(result.value), 100);
  check(ctx, "physical post-target byte in Read8 v2 helper");
  const observedByte = refresh(ctx).view.getUint32(ctx.base + 120, true);
  assert.equal(observedByte, result.value);
  const diagnostic = frame(ctx, "separate post-target Read8", unit);
  counts.host_read8++;
  hostCalls.push({
    event: ++events,
    context: ctx.ordinal,
    name: "read8",
    args: [address],
    status: readStatus,
    observed_value: observedByte,
    diagnostic,
  });
  activeCall = null;
  counts.targets++;
  counts[`${purpose}_targets`]++;
  if (purpose === "regular")
    counts[unit.origin === "native-rust" ? "native_targets" : "actual_regular_targets"]++;
  targets.push({
    context: ctx.ordinal,
    owner: ctx.owner,
    origin: unit.origin,
    kind: ctx.bank.kind,
    module: unit.file,
    purpose,
    pc: scan.target,
    next_pc: scan.next,
    raw_pc: scan.raw_pc,
    raw: scan.raw,
    address,
    old_byte: oldByte,
    incoming_flags: flags,
    expected: result,
    registers: [...registers],
    observed_byte: observedByte,
    diagnostic,
    ...frames,
  });
  return result;
}
function consumers(ctx, unit, registers, scan, flags) {
  registers[0] = registers[0] - (registers[0] % 256) + (flags % 2);
  run(ctx, unit, 1, "SETB live CF", registers, scan.next + 3, flags);
  counts.setb++;
  registers[2] = registers[2] - (registers[2] % 256) + Number(Boolean(flags & 0x800));
  run(ctx, unit, 1, "SETO live OF", registers, scan.next + 6, flags);
  counts.seto++;
  if (scan.canary) {
    registers[3] = 0xdeadbeef;
    run(ctx, unit, 1, "current MOV canary", registers, scan.next + 11, flags);
    counts.canaries++;
  }
  run(ctx, unit, 1, "following JMP", registers, COLD, flags);
  counts.jumps++;
}
function numericRow(
  ctx,
  unit,
  scan,
  flags,
  purpose = "regular",
  address = BYTE,
  value = ctx.bank.kind === "left" ? 0x81 : 0x80,
  initialPreflight = false,
) {
  const registers = addressRegisters(scan.form, address);
  upload(ctx, address, Buffer.from([value]), "one initial operand");
  seed(ctx, registers, scan.target, flags);
  if (initialPreflight) preflight(ctx, unit, registers, scan.target, flags);
  const result = target(ctx, unit, registers, scan, address, flags, purpose);
  consumers(ctx, unit, registers, scan, result.flags);
}
function numeric(ctx) {
  pages(ctx, "initial declared pages");
  for (const [group, scans] of ctx.bank.groups.entries()) {
    const actual = compile(ctx, scans, `g${group}`),
      emitted = ctx.owner === "replacement" ? native(ctx, actual, group) : null;
    for (const scan of scans) {
      for (const flags of FLAGS) {
        numericRow(
          ctx,
          actual,
          scan,
          flags,
          "regular",
          BYTE,
          undefined,
          group === 0 && scan.raw === 0 && flags === 2,
        );
        if (emitted !== null) numericRow(ctx, emitted, scan, flags);
      }
      if (STYLE_RAW.includes(scan.raw))
        for (const flags of STYLE_FLAGS) numericRow(ctx, actual, scan, flags);
    }
  }
  const eaUnit = compile(ctx, ctx.bank.ea, "ea");
  for (const scan of ctx.bank.ea) numericRow(ctx, eaUnit, scan, 0xcd7);
  for (const address of [0x4fff, 0x5000, 0xffffffff])
    for (const flags of FLAGS) {
      if (address === 0x4fff && ctx.pages.has(SECOND)) unmap(ctx, SECOND);
      if (address === SECOND && !ctx.pages.has(SECOND)) map(ctx, SECOND);
      numericRow(ctx, eaUnit, ctx.bank.ea[6], flags, "endpoint", address);
    }
  for (const [index, name] of ["clc", "stc", "cmc"].entries())
    for (const flags of FLAGS) {
      const scan = ctx.bank.ea[index],
        registers = addressRegisters(index, BYTE),
        value = ctx.bank.kind === "left" ? 0x81 : 0x80;
      upload(ctx, BYTE, Buffer.from([value]), "producer initial operand");
      seed(ctx, registers, scan.entry, flags);
      const produced =
        name === "clc"
          ? flags - (flags % 2)
          : name === "stc"
            ? flags - (flags % 2) + 1
            : flags - (flags % 2) + 1 - (flags % 2);
      run(ctx, eaUnit, 1, name, registers, scan.target, produced);
      counts.producers++;
      const result = target(ctx, eaUnit, registers, scan, BYTE, produced, "producer");
      consumers(ctx, eaUnit, registers, scan, result.flags);
    }
  pages(ctx, "numeric EA producer and endpoint pages");
  return compile(ctx, ctx.bank.fault, "fault");
}
const faultShapes = [
  { name: "unmapped", address: 0x8000, detail: 1, access: 1, missing: 0x8000 },
  { name: "write-only-last-byte", address: 0x4fff, detail: 2, access: 1, permissions: 2 },
  { name: "read-only-last-byte", address: 0x4fff, detail: 2, access: 2, permissions: 1 },
];
function faultCases(ctx, unit) {
  for (const shape of faultShapes)
    for (const scan of ctx.bank.fault)
      for (const flags of FLAGS) {
        protect(ctx, DATA, 3);
        if (shape.missing === undefined)
          upload(ctx, shape.address, Buffer.from([0x81]), "pre-fault operand");
        if (shape.permissions !== undefined) protect(ctx, DATA, shape.permissions);
        const registers = addressRegisters(6, shape.address);
        seed(ctx, registers, scan.entry, flags);
        registers[3]++;
        const helper =
          shape.access === 1
            ? readHelper(0, shape.detail, shape.address)
            : storeHelper(shape.detail, shape.address);
        const first = run(
          ctx,
          unit,
          2,
          "retired LEA before exact byte fault",
          registers,
          scan.target,
          flags,
          5,
          1,
          helper,
          shape.detail,
          shape.address,
          shape.access,
        );
        counts.prefixes++;
        pages(ctx, "first fault operand and neighbors", [DATA]);
        if (scan.raw === 0 && flags === 2) preflight(ctx, unit, registers, scan.target, flags);
        const retry = run(
          ctx,
          unit,
          1,
          "unrepaired zero-retirement retry",
          registers,
          scan.target,
          flags,
          5,
          0,
          helper,
          shape.detail,
          shape.address,
          shape.access,
        );
        counts.fault_calls += 2;
        counts[shape.access === 1 ? "read_faults" : "write_faults"] += 2;
        pages(ctx, "retry operand and neighbors", [DATA]);
        if (shape.missing !== undefined) {
          map(ctx, shape.missing, null);
          counts.map_only_repairs++;
        } else {
          protect(ctx, DATA, 3);
          counts.permission_only_repairs++;
        }
        const result = target(ctx, unit, registers, scan, shape.address, flags, "repaired");
        consumers(ctx, unit, registers, scan, result.flags);
        counts.repairs++;
        if (shape.missing !== undefined) {
          pages(ctx, "complete map-only repaired page", [shape.missing]);
          unmap(ctx, shape.missing);
        }
        faults.push({
          context: ctx.ordinal,
          kind: ctx.bank.kind,
          owner: ctx.owner,
          raw: scan.raw,
          flags,
          shape,
          helper_version: shape.access === 1 ? 2 : 3,
          width: 1,
          first,
          retry,
          repair_mode: shape.missing !== undefined ? "map-only" : "permission-only",
          repair_expected: result,
          cpu_writes_after_first_fault: 0,
          prefix_replays: 0,
        });
      }
  protect(ctx, DATA, 3);
  pages(ctx, "complete post-fault pages");
}
function ownerChecks(ctx, unit) {
  if (ctx.owner === "resident") {
    assert.equal(unit.high, 0);
    assert.ok(
      unit.low >= 1 && unit.low <= 7,
      "this fresh instance compiled at most seven resident units",
    );
  }
  for (const wrong of ["key", "identity"]) {
    const low = ctx.low ^ Number(wrong === "key");
    const args =
      ctx.owner === "replacement"
        ? [
            low,
            ctx.high,
            unit.generation + Number(wrong === "identity"),
            ctx.base,
            ctx.base + 56,
            ctx.base + 96,
          ]
        : [
            low,
            ctx.high,
            unit.low,
            (unit.high ^ (wrong === "identity" ? 0x80000000 : 0)) >>> 0,
            ctx.base,
            ctx.base + 56,
            ctx.base + 96,
          ];
    host(ctx, ctx.owner === "replacement" ? "guard" : "guard_resident", args, 3);
    counts.direct_guards++;
  }
}
function controls(ctx, unit) {
  const registers = Array.from({ length: 8 }, (_, index) =>
      ctx.expected.readUInt32LE(16 + index * 4),
    ),
    flags = ctx.expected.readUInt32LE(52);
  for (const [cancelled, budget, reason] of [
    [0, 0, 1],
    [1, 1, 2],
    [1, 0, 2],
  ]) {
    cancel(ctx, cancelled);
    run(ctx, unit, budget, "cold preflight", registers, COLD, flags, reason, 0);
  }
  cancel(ctx, 0);
  run(ctx, unit, 1, "cold entry need code", registers, COLD, flags, 3, 0);
  generated(ctx, unit, [0xffffffff, 0xffffffff, 0, 0xffffffff], "malformed pointers", 1);
}
function close(ctx) {
  cancel(ctx, 1);
  host(ctx, "close", []);
  for (const unit of ctx.units) {
    generated(
      ctx,
      unit,
      [0xffffffff, 0xffffffff, 0, 0xffffffff],
      "closed owner before cancelled malformed arguments",
      5,
    );
    counts.closed_calls++;
  }
}
function smc(ctx, raw) {
  const scan = ctx.bank.fault.find((row) => row.raw === raw),
    unit = compile(ctx, ctx.bank.fault, "smc"),
    registers = addressRegisters(6, scan.raw_pc),
    flags = raw === 10 ? 0xcd6 : 0xcd7;
  seed(ctx, registers, scan.target, flags);
  assert.equal(ctx.pages.get(PC)[scan.raw_pc - PC], raw);
  const result = target(ctx, unit, registers, scan, scan.raw_pc, flags, "code", 6);
  counts.code_stores++;
  counts[result.value === raw ? "same_value_code_stores" : "changing_code_stores"]++;
  pages(ctx, "complete committed consumed-immediate code store");
  ownerChecks(ctx, unit);
  const oldAllocation = Buffer.from(
    refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length),
  );
  assert.deepEqual(oldAllocation, unit.bytes);
  let oldAllocationAfter;
  cancel(ctx, 1);
  generated(
    ctx,
    unit,
    [0xffffffff, 0xffffffff, 0, 0xffffffff],
    "stale guard before pointers cancel and budget",
    4,
    null,
    () => {
      oldAllocationAfter = Buffer.from(
        ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length),
      );
      assert.deepEqual(
        oldAllocationAfter,
        oldAllocation,
        "first synchronous post-stale observation is entire old allocation",
      );
    },
  );
  counts.stale_calls++;
  cancel(ctx, 0);
  const tail = { ...scan, entry: scan.next, spec: [scan.next, 16] },
    current = compile(ctx, [tail], "current-tail", true);
  consumers(ctx, current, registers, scan, result.flags);
  pages(ctx, "current pure tail without memory replay");
  codeStores.push({
    context: ctx.ordinal,
    owner: ctx.owner,
    kind: ctx.bank.kind,
    raw,
    address: scan.raw_pc,
    before: raw,
    after: result.value,
    flags_before: flags,
    flags_after: result.flags,
    reason: 6,
    retired: 1,
    target_pc: scan.target,
    next_pc: scan.next,
    stale_status: 4,
    old_module_sha256_before: hash(oldAllocation),
    old_module_sha256_after: hash(oldAllocationAfter),
    same_value: result.value === raw,
    continuation_bytes: 16,
    continuation_instructions: 4,
    helper_width: 1,
    helper_version: 3,
    cpu_writes_during_continuation: 0,
    replayed_memory_instructions: 0,
  });
  close(ctx);
}
for (const selected of banks)
  for (const owner of ["replacement", "resident"]) {
    const ctx = fresh(owner, selected, "numeric");
    const unit = numeric(ctx);
    faultCases(ctx, unit);
    controls(ctx, unit);
    ownerChecks(ctx, unit);
    close(ctx);
  }
for (const selected of banks)
  for (const owner of ["replacement", "resident"])
    for (const raw of [0, 9, 10]) smc(fresh(owner, selected, `smc-${raw}`), raw);
const rawArenas = artifact("arena-snapshots.bin", Buffer.concat(arenaFrames)),
  rawPages = artifact("page-snapshots.bin", Buffer.concat(pageFrames));
counts.files = artifacts.length + 1;
for (const [name, expected] of Object.entries(plan))
  assert.equal(counts[name], expected, `frozen prospective ${name} census`);
assert.deepEqual(pinSources(), sourcePins, "selected sources byte-stable during execution");
assert.equal(hash(readFileSync(enginePath)), engineSha256);
const result = {
  status: "ok",
  profile: "finite flat32 C0 /2,/3 memory-byte immediate carry rotate",
  command: [process.execPath, process.argv[1], enginePath, output, root],
  environment: {
    node: process.version,
    v8: process.versions.v8,
    platform: process.platform,
    arch: process.arch,
  },
  engine: { path: enginePath, bytes: engineBytes.length, sha256: engineSha256 },
  source: {
    order: "253667-093US",
    edition: "September 2026",
    full_text_sha256: hash(fullText),
    chapter_lines: [27174, 27413],
    saved_chapter: savedChapter,
    latest_edition_or_hardware_claim: false,
  },
  source_pins: sourcePins,
  source_pins_after: pinSources(),
  plan,
  counts,
  native_manifest: nativeManifest,
  contexts,
  modules,
  targets,
  runs,
  faults,
  code_stores: codeStores,
  host_inputs: hostInputs,
  host_calls: hostCalls,
  raw_arena_records: arenaRows,
  raw_arena_artifact: rawArenas,
  pages: pageRows,
  raw_page_artifact: rawPages,
  artifacts,
  corpus: {
    native_admission_raw_domain: "all256 raw counts separately in native decoding/admission target",
    actual_arithmetic_raw: [...Array.from({ length: 32 }, (_, index) => index), 32, 33, 255],
    base_flags: FLAGS,
    style_flags: STYLE_FLAGS,
    style_raw: STYLE_RAW,
    numeric_basis: { left: 0x81, right: 0x80 },
    ea_count_pairings: banks[0].ea.map((row) => ({ form: row.form, raw: row.raw })),
    literal_anchors: ANCHORS,
    registers: REG,
    fault_raw: [0, 9, 10],
    fault_shapes: faultShapes,
    endpoint_addresses: [0x4fff, 0x5000, 0xffffffff],
    arena_bytes: SIZE,
    native_origin_targets: 140,
    actual_origin_targets: 492,
    limits: { blocks: 8, instructions: 64, wasm_bytes: 65536 },
    patterned_page_sha256: hash(pattern),
  },
  banks: banks.map(({ kind, extension, groups, ea, fault, packed, saved }) => ({
    kind,
    extension,
    groups,
    ea,
    fault,
    packed,
    saved,
  })),
  evidence_limits: {
    arithmetic:
      "nine-character oldCF:byte repeated rotation with20 literal anchors; q1 definedOF, q>1 nonzero-r clearsOF by deterministic profile; r0 retains allFLAGS",
    origins:
      "140 native-Rust bound replacement targets execute with independently matched actual key/generation+guard and real current memory helpers;492 targets use actual-engine-compiled replacement/resident modules; no native-resident binding patch",
    arenas:
      "all2910 generated calls save full4236B before/after;632 separate immediate post-target Read8 diagnostics save another fullarena and physically verify each storedbyte; complete physical frames alone are not a complete independent arena oracle",
    pages:
      "348 complete physical4096B pages, neighbors included; expected pages from successful map/upload and independent byte target model; per-target old RAM is modeled from explicit one-byte host uploads or prior successful map, not physically read before every store; diagnostic Read32/Read8 is separate from guest Read1/Store1",
    lifecycle:
      "firstfault retains CPU/EIP/FLAGS/byte after realLEA, retry0, permission-only/map-only repair without CPU seed; code stores consume their own immediate and even same-value q0/r0 writes invalidate",
    currency:
      "stale4 beforecancel/malformed/zerobudget preserves complete known arena and entire retained old allocation before newunit compilation; fresh guard-only puretail continues retainedCPU withoutreplay",
    scope:
      "finite35raw arithmetic basis and8EA count pairings, not fullbyte×FLAGS×count×EA; no D2 memory/CL/DWORD/word/prefix/LOCK/races/MMIO/newABI/caps/fullCI/performance/browser/graphics/game claim",
  },
};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), { flag: "wx" });
assert.deepEqual(
  readdirSync(output).sort(),
  [...artifacts.map((row) => row.path), "result.json"].sort(),
  "exact physical file census",
);
const resultBytes = readFileSync(join(output, "result.json"));
console.log(
  JSON.stringify({
    status: "ok",
    engine_sha256: engineSha256,
    result_sha256: hash(resultBytes),
    counts,
    output,
  }),
);
