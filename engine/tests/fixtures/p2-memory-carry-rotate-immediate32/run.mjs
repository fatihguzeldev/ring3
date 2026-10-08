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
  COLD = 0x1f00,
  KEEP = 0x3000,
  DATA = 0x4000,
  SECOND = 0x5000,
  WORD = 0x4010,
  TOP = 0xfffff000,
  HIGH = 0xc8411000;
const FLAGS = [2, 0xcd7],
  STYLE_FLAGS = [3, 0xcd6],
  STYLE_RAW = [0, 1, 2, 31],
  VALUES = [0, 0xffffffff, 0x80000000, 0x01234567],
  VALUE_RAW = [0, 1, 2, 31];
const REG = [
  0x1234807f, 0xa1b2c378, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef,
];
const pattern = Buffer.from(Array.from({ length: 4096 }, (_, index) => (index % 251) + 1));
const plan = {
  contexts: 16,
  modules: 62,
  native_modules: 10,
  actual_modules: 52,
  source_instructions: 1180,
  seeds: 800,
  regular_targets: 612,
  native_targets: 140,
  actual_regular_targets: 472,
  producer_targets: 24,
  endpoint_targets: 24,
  repaired_targets: 96,
  code_targets: 12,
  targets: 768,
  q0_targets: 132,
  q1_targets: 116,
  multi_targets: 520,
  setb: 768,
  seto: 768,
  jumps: 768,
  canaries: 112,
  producers: 24,
  prefixes: 128,
  fault_calls: 256,
  read_faults: 160,
  write_faults: 96,
  repairs: 96,
  map_only_repairs: 24,
  permission_only_repairs: 72,
  code_stores: 12,
  same_value_code_stores: 4,
  changing_code_stores: 8,
  stale_calls: 24,
  closed_calls: 62,
  direct_guards: 32,
  generated_calls: 3542,
  retired: 3348,
  maps: 108,
  unmaps: 28,
  protects: 444,
  host_uploads: 848,
  host_uploaded_bytes: 330716,
  operand_reads: 1632,
  host_read32: 833120,
  host_requests: 900,
  cancel_writes: 48,
  host_inputs: 2596,
  host_calls: 3998,
  pages: 812,
  page_bytes: 3325952,
  raw_arenas: 8716,
  raw_arena_bytes: 36920976,
  files: 94,
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
  assert.match(path, /^[a-z0-9][a-z0-9.-]*$/);
  assert.ok(!artifacts.some((row) => row.path === path));
  if (write) writeFileSync(join(output, path), bytes, { flag: "wx" });
  const row = { path, bytes: bytes.length, sha256: hash(bytes) };
  artifacts.push(row);
  return row;
}
const nativeManifestBytes = readFileSync(join(output, "native-manifest.json")),
  nativeManifest = JSON.parse(nativeManifestBytes);
assert.equal(nativeManifest.length, 10);
const expectedNativeFiles = ["left", "right"].flatMap((kind) =>
  Array.from({ length: 5 }, (_, group) => `native-${kind}-g${group}.wasm`));
assert.deepEqual(nativeManifest.map((row) => row.file).sort(), expectedNativeFiles.sort(),
  "exact ten bounded native artifact basenames before any new write");
assert.deepEqual(readdirSync(output).sort(), ["memory-left.x86", "memory-right.x86",
  "native-manifest.json", ...nativeManifest.map((row) => row.file)].sort(), "exclusive native wrapper output roster");
assert.equal(new Set(nativeManifest.map((row) => row.file)).size, 10);
artifact("engine.wasm", engineBytes);
artifact("native-manifest.json", nativeManifestBytes, false);
const sourcePaths = [
  "engine/src/cpu/x86/ir.rs",
  "engine/src/cpu/x86/decode/integer.rs",
  "engine/src/cpu/dbt/region.rs",
  "engine/src/cpu/dbt/wasm/integer.rs",
  "engine/src/cpu/dbt/wasm/memory.rs",
  "engine/tests/cpu_carry_rotate_immediate_one.rs",
  "engine/tests/cpu_memory_carry_rotate_one.rs",
  "engine/tests/cpu_memory_byte_carry_rotate_immediate.rs",
  "engine/tests/cpu_memory_byte_carry_rotate_cl.rs",
  "engine/tests/cpu_carry_rotate_immediate32.rs",
  "engine/tests/cpu_carry_rotate_cl32.rs",
  "engine/tests/cpu_rotate32.rs",
  "engine/tests/cpu_memory_rotate32.rs",
  "engine/tests/cpu_memory_carry_rotate_immediate32.rs",
  "engine/tests/cpu_memory_carry_rotate_immediate_wasm.rs",
  "engine/tests/fixtures/p2-memory-carry-rotate-immediate32/run.mjs",
  "engine/tests/fixtures/p2-carry-rotate-immediate-one/run.mjs",
  "engine/tests/cpu_carry_rotate_immediate_one_wasm.rs",
  "engine/tests/fixtures/p2-carry-rotate-immediate32/oracle.mjs",
  "engine/tests/fixtures/support/engine.mjs",
  "engine/src/cpu/x86/decode/profile.rs",
  "engine/src/cpu/dbt/wasm/emitter.rs",
  "engine/src/cpu/dbt/wasm/locals.rs",
  "engine/src/memory/space.rs",
  "engine/src/process/instance.rs",
  "engine/src/process/resident.rs",
  "engine/src/abi/arena.rs",
  "engine/src/abi/memory_helper.rs",
  "engine/src/abi/x86/state.rs",
  "engine/src/abi/x86/exit.rs",
  "engine/src/abi/wasm/exports.rs",
];
assert.equal(sourcePaths.length, 31);
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
    const value = bytes.subarray(at, at + length).toString("utf8");
    at += length;
    return value;
  };
  const sections = new Map();
  while (at < bytes.length) {
    const id = bytes[at++], length = uleb(), start = at;
    assert.ok(start + length <= bytes.length && !sections.has(id));
    sections.set(id, [start, start + length]);
    at += length;
  }
  assert.deepEqual([...sections.keys()], [1, 2, 3, 7, 10]);
  const section = (id) => { at = sections.get(id)[0]; },
    ended = (id) => assert.equal(at, sections.get(id)[1]);
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
    ...(memory ? ["read32", owner === "replacement" ? "store32" : "store_resident32"] : [])];
  section(2);
  assert.equal(uleb(), names.length + 1);
  assert.equal(text(), "env"); assert.equal(text(), "memory");
  assert.equal(bytes[at++], 2); assert.equal(uleb(), 0); assert.equal(uleb(), 1);
  for (const [index, name] of names.entries()) {
    assert.equal(text(), "ring3"); assert.equal(text(), name);
    assert.equal(bytes[at++], 0); assert.equal(uleb(), index + 1);
  }
  ended(2);
  section(3); assert.equal(uleb(), 1); assert.equal(uleb(), 0); ended(3);
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
    sections: [...sections.keys()], body_claim: "guard prefix, types and locals; remaining body not independently certified" };
}

function bank(kind, extension) {
  const bytes = Buffer.alloc(4096, 0xcc),
    groups = [],
    ea = [],
    fault = [],
    smc = [];
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
      instructions: 4 + Number(prefix !== 0) + Number(canary),
    };
  }
  for (let ordinal = 0; ordinal < 5; ordinal++) {
    const raws =
      ordinal < 4 ? Array.from({ length: 8 }, (_, index) => ordinal * 8 + index) : [32, 33, 255];
    groups.push(raws.map((raw) => append([0xc1, 5 | extension, 0x10, 0x40, 0, 0, raw], raw)));
  }
  const forms = [
    [0xf8, 0xc1, extension, 2],
    [0xf9, 0xc1, 0x41 | extension, 0xf0, 1],
    [0xf5, 0xc1, 0x44 | extension, 0x4a, 0x11, 31],
    [0xc1, 0x84 | extension, 0xf3, 0xe0, 0xff, 0xff, 0xff, 2],
    [0xc1, 0x04 | extension, 0x24, 0],
    [0xc1, 0x45 | extension, 0, 1],
    [0x8d, 0x5b, 1, 0xc1, 0x06 | extension, 0],
    [0xc1, 0x87 | extension, 0, 1, 0, 0, 31],
  ];
  for (const [index, form] of forms.entries())
    ea.push(append(form, form.at(-1), index < 3 ? 1 : index === 6 ? 3 : 0, index === 7, index));
  for (const raw of [0, 1, 2])
    fault.push(append([0x8d, 0x5b, 1, 0xc1, 0x07 | extension, raw], raw, 3, true, 8));
  assert.equal(at, 833);
  for (const [slot, raw] of [0, 1, 2].entries()) {
    const start = 0xe80 + slot * 21,
      entry = PC + start,
      bytesHere = [0xbe, 1, 0, 0, 0x80, 0xc1, 0x07 | extension, raw,
        0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0xbb, 0xef, 0xbe, 0xad, 0xde,
        0xeb, COLD - (entry + 21)];
    bytes.set(bytesHere, start);
    smc.push({ entry, target: entry + 5, next: entry + 8, raw_pc: entry + 7,
      raw, canary: true, form: 8, spec: [entry, 21], instructions: 6 });
  }
  bytes.set([0x0f, 0x0b], COLD - PC);
  assert.deepEqual(
    bytes,
    readFileSync(join(output, `memory-${kind}.x86`)),
    "independent Rust/JS bank authoring",
  );
  const saved = artifact(`memory-${kind}.x86`, bytes, false);
  return { kind, extension, bytes, groups, ea, fault, smc, packed: at, saved };
}
const banks = [bank("left", 0x10), bank("right", 0x18)];
process.on("uncaughtException", (error) => {
  const currentContext = activeCall?.ctx ?? lastContext;
  let currentBytes = null, currentAuthority = null;
  if (currentContext?.base !== undefined && currentContext.bytes.length >= currentContext.base + SIZE) {
    currentBytes = Buffer.from(currentContext.bytes.subarray(currentContext.base, currentContext.base + SIZE));
    currentAuthority = "already-held valid full live arena view; no refresh/getter/helper";
  } else if (currentContext !== null) {
    for (let index = arenaRows.length - 1; index >= 0; index--) {
      if (arenaRows[index].context === currentContext.ordinal) {
        currentBytes = Buffer.from(arenaFrames[index]);
        currentAuthority = { held_frame: arenaRows[index], claim: "last already-captured full arena, not fresh current bytes" };
        break;
      }
    }
  }
  const current =
    currentBytes === null
      ? null
      : {
          context: currentContext.ordinal,
          module: activeCall?.unit.file ?? null,
          label: activeCall?.label ?? "outside generated call",
          args: activeCall?.args ?? null,
          actual_status: activeCall?.actualStatus ?? null,
          arena_sha256: hash(currentBytes),
          arena_authority: currentAuthority,
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
  record("R3EX", 40, [reason, retired, detail, address, access, access ? 4 : 0], version);
const wordHelper = (value = 0, detail = 0, address = 0, access = 0) =>
  record("R3MH", 40, [access ? 1 : 0, value, detail, address, access, access ? 4 : 0]);
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
  assert.ok(bytes.length <= 4096);
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
  counts.host_requests++;
}
function patch(ctx, address, bytes) {
  assert.ok(address >= 0 && address + bytes.length <= 4294967296);
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
      seed: null,
    };
  lastContext = ctx;
  ctx.memory = instance.exports.memory;
  assert.ok(ctx.memory instanceof WebAssembly.Memory);
  refresh(ctx);
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
    compile_entries: 2,
    compile_resident: 1,
    generation: 0,
    module_ptr: 0,
    module_len: 0,
    guard: 6,
    guard_resident: 7,
    store32: 2,
    store_resident32: 6,
  };
  for (const [name, arity] of Object.entries(arities)) {
    ctx.api[name] = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof ctx.api[name], "function", name);
    assert.equal(ctx.api[name].length, arity, name);
  }
  assert.equal(ctx.api.open(7, ctx.low, ctx.high), 0);
  ctx.base = ctx.api.arena_ptr() >>> 0;
  refresh(ctx);
  assert.ok(ctx.base > 0 && ctx.base + SIZE <= ctx.bytes.length);
  check(ctx, "independent initialized arena");
  hostCalls.push({ event: ++events, context: ordinal, name: "open", args: [7, ctx.low, ctx.high], status: 0,
    arena_pointer: ctx.base, initialized_arena_sha256: hash(arena(ctx)) });
  host(ctx, "map", [PC, 1, 7]);
  ctx.pages.set(PC, Buffer.alloc(4096));
  ctx.permissions.set(PC, 7);
  counts.maps++;
  upload(ctx, PC, selected.bytes, "authored full code bank");
  for (const address of [KEEP, DATA, SECOND, TOP]) map(ctx, address);
  if (role === "numeric") protect(ctx, PC, 5);
  contexts.push({
    context: ordinal,
    owner,
    kind: selected.kind,
    role,
    key_low: ctx.low,
    key_high: ctx.high,
    page_limit: 7,
    arena_pointer: ctx.base,
    code_permissions: role === "numeric" ? 5 : 7,
  });
  return ctx;
}
function instantiate(ctx, bytes, file, binding, scans, pureTail = false, origin = "actual-engine") {
  assert.ok(bytes.length > 8 && bytes.length <= 65536);
  const module = new WebAssembly.Module(bytes),
    imports = (pureTail ? ["guard"] : ["guard", "read32", "store32"]).map((name) =>
      ctx.owner === "replacement"
        ? name
        : name === "guard"
          ? "guard_resident"
          : name === "store32"
            ? "store_resident32"
            : name,
    );
  assert.deepEqual(WebAssembly.Module.imports(module), [
    { module: "env", name: "memory", kind: "memory" },
    ...imports.map((name) => ({ module: "ring3", name, kind: "function" })),
  ]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: "run", kind: "function" }]);
  assert.ok(WebAssembly.validate(bytes));
  const abi = profile(bytes, ctx.owner, !pureTail, ctx, binding);
  const child = new WebAssembly.Instance(module, { env: { memory: ctx.memory }, ring3: ctx.api });
  assert.equal(child.exports.run.length, 4);
  const unit = {
    ...binding,
    run: child.exports.run,
    bytes,
    file,
    scans,
    profile: abi,
    source_spans: scans.map(({ spec: [entry, length] }) => ({ entry, bytes: length,
      hex: ctx.pages.get(PC).subarray(entry - PC, entry - PC + length).toString("hex") })),
    origin,
    exitVersion: pureTail ? 1 : 2,
  };
  ctx.units.push(unit);
  counts.modules++;
  counts[origin === "native-rust" ? "native_modules" : "actual_modules"]++;
  counts.source_instructions += scans.reduce((sum, scan) => sum + scan.instructions, 0);
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
    profile: abi,
    source_spans: unit.source_spans,
    source_instructions: scans.reduce((sum, scan) => sum + scan.instructions, 0),
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
  refresh(ctx);
  assert.ok(binding.pointer > 0 && binding.pointer + binding.length <= ctx.bytes.length);
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
  assert.equal(row.file, `native-${ctx.bank.kind}-g${group}.wasm`);
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
  ctx.expected.set(wordHelper(0xdecafbad), 100);
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
  check(ctx, "explicit cancel field input");
  counts.cancel_writes++;
  hostInputs.push({ event: ++events, context: ctx.ordinal, label: "explicit cancellation field",
    arena_offset: 96, bytes: 4, hex: words([value]).toString("hex") });
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
      counts.host_read32++;
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
    hostCalls.push({ event: ++events, context: ctx.ordinal, name: "read32-page", args: [address, 1024],
      status: 0, page: row, final_helper_hex: ctx.expected.subarray(100, 140).toString("hex") });
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
  if (form === 8) registers[7] = address;
  return registers.map((value) => value >>> 0);
}
function physicalCPU(ctx) {
  const bytes = arena(ctx),
    registers = Array.from({ length: 8 }, (_, index) => bytes.readUInt32LE(16 + index * 4));
  return { registers, pc: bytes.readUInt32LE(48), flags: bytes.readUInt32LE(52) };
}
function modelWord(ctx, address) {
  assert.ok(address >= 0 && address + 4 <= 4294967296);
  const bytes = Buffer.alloc(4);
  for (let index = 0; index < 4; index++) {
    const at = address + index;
    bytes[index] = ctx.pages.get(Math.floor(at / 4096) * 4096)[at % 4096];
  }
  return bytes.readUInt32LE();
}
function decodeTarget(ctx, cpu) {
  const page = ctx.pages.get(PC), start = cpu.pc - PC;
  assert.ok(start >= 0 && start + 2 < page.length);
  assert.equal(page[start], 0xc1);
  const modrm = page[start + 1], mod = modrm >>> 6, operation = (modrm >>> 3) & 7, rm = modrm & 7;
  assert.ok(mod !== 3 && (operation === 2 || operation === 3));
  assert.equal(operation, ctx.bank.kind === "left" ? 2 : 3);
  let at = start + 2, address = 0;
  if (rm === 4) {
    const sib = page[at++], scale = 2 ** (sib >>> 6), index = (sib >>> 3) & 7, base = sib & 7;
    if (index !== 4) address += cpu.registers[index] * scale;
    if (mod === 0 && base === 5) { address += page.readInt32LE(at); at += 4; }
    else address += cpu.registers[base];
  } else if (mod === 0 && rm === 5) { address += page.readInt32LE(at); at += 4; }
  else address += cpu.registers[rm];
  if (mod === 1) address += page.readInt8(at++);
  else if (mod === 2) { address += page.readInt32LE(at); at += 4; }
  const raw = page[at++];
  return { pc: cpu.pc, address: address >>> 0, raw, raw_pc: PC + at - 1, next: PC + at,
    modrm, bytes: page.subarray(start, at).toString("hex"), code_page_sha256: hash(page),
    source_authority: "authored/current code bytes joined to compiler spans and physical full-page checkpoints" };
}
function observeWord(ctx, unit, address, label) {
  check(ctx, `before ${label}`);
  const cpuBefore = physicalCPU(ctx);
  activeCall = { ctx, unit, args: [address], label };
  const status = ctx.api.read32(address);
  activeCall.actualStatus = status;
  assert.equal(status, 0, label);
  const value = refresh(ctx).view.getUint32(ctx.base + 120, true);
  assert.equal(value, modelWord(ctx, address), "physical operand vs successful map/upload/store page history");
  ctx.expected.set(wordHelper(value), 100);
  check(ctx, "Read32 publishes diagnostic helper only");
  assert.deepEqual(physicalCPU(ctx), cpuBefore, "host diagnostic preserves full architectural state");
  const diagnostic = frame(ctx, label, unit),
    row = { event: ++events, context: ctx.ordinal, name: "read32", args: [address], status,
      observed_value: value, diagnostic, cpu_before: cpuBefore, helper_version: 1, width: 4 };
  hostCalls.push(row);
  counts.operand_reads++;
  counts.host_read32++;
  activeCall = null;
  return row;
}
function target(ctx, unit, registers, scan, address, flags, purpose, reason = 1) {
  const cpuBefore = physicalCPU(ctx), decoded = decodeTarget(ctx, cpuBefore);
  assert.deepEqual(cpuBefore, { registers: [...registers], pc: scan.target, flags });
  assert.equal(decoded.address, address);
  assert.equal(decoded.raw, scan.raw);
  assert.equal(decoded.next, scan.next);
  assert.equal(decoded.raw_pc, scan.raw_pc);
  const beforeDiagnostic = observeWord(ctx, unit, decoded.address, "physical pre-target Read32 operand"),
    oldWord = beforeDiagnostic.observed_value,
    result = rotate(ctx.bank.kind, oldWord, decoded.raw, cpuBefore.flags);
  patch(ctx, address, words([result.value]));
  const frames = run(
    ctx,
    unit,
    1,
    "checked dword carry rotate Store4 commits before consumers",
    registers,
    scan.next,
    result.flags,
    reason,
    1,
    wordHelper(),
  );
  const afterDiagnostic = observeWord(ctx, unit, address, "physical post-target Read32 operand");
  assert.equal(afterDiagnostic.observed_value, result.value);
  counts.targets++;
  counts[result.q === 0 ? "q0_targets" : result.q === 1 ? "q1_targets" : "multi_targets"]++;
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
    decoded,
    old_word: oldWord,
    incoming_flags: flags,
    expected: result,
    registers: [...registers],
    observed_word: afterDiagnostic.observed_value,
    physical_cpu_before: cpuBefore,
    before_diagnostic: beforeDiagnostic,
    after_diagnostic: afterDiagnostic,
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
  address = WORD,
  value = ctx.bank.kind === "left" ? 0x80000001 : 0x92345678,
) {
  const registers = addressRegisters(scan.form, address);
  upload(ctx, address, words([value]), "one initial DWORD operand");
  seed(ctx, registers, scan.target, flags);
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
        numericRow(ctx, actual, scan, flags);
        if (emitted !== null) numericRow(ctx, emitted, scan, flags);
      }
      if (STYLE_RAW.includes(scan.raw))
        for (const flags of STYLE_FLAGS) numericRow(ctx, actual, scan, flags);
      if (VALUE_RAW.includes(scan.raw))
        for (const value of VALUES)
          for (const flags of FLAGS) numericRow(ctx, actual, scan, flags, "regular", WORD, value);
    }
  }
  const eaUnit = compile(ctx, ctx.bank.ea, "ea");
  for (const scan of ctx.bank.ea) numericRow(ctx, eaUnit, scan, 0xcd7);
  for (const address of [0x4ffc, 0x4ffe, 0xfffffffc])
    for (const flags of FLAGS) {
      if (address === 0x4ffc && ctx.pages.has(SECOND)) unmap(ctx, SECOND);
      if (address === 0x4ffe && !ctx.pages.has(SECOND)) map(ctx, SECOND, null);
      numericRow(ctx, eaUnit, ctx.bank.ea[4], flags, "endpoint", address);
    }
  for (const [index, name] of ["clc", "stc", "cmc"].entries())
    for (const flags of FLAGS) {
      const scan = ctx.bank.ea[index],
        registers = addressRegisters(index, WORD),
        value = ctx.bank.kind === "left" ? 0x80000001 : 0x92345678;
      upload(ctx, WORD, words([value]), "producer initial DWORD operand");
      seed(ctx, registers, scan.entry, flags);
      const produced =
        name === "clc"
          ? flags - (flags % 2)
          : name === "stc"
            ? flags - (flags % 2) + 1
            : flags - (flags % 2) + 1 - (flags % 2);
      run(ctx, eaUnit, 1, name, registers, scan.target, produced);
      counts.producers++;
      const result = target(ctx, eaUnit, registers, scan, WORD, produced, "producer");
      consumers(ctx, eaUnit, registers, scan, result.flags);
    }
  controls(ctx, eaUnit);
  ownerChecks(ctx, eaUnit);
  return compile(ctx, ctx.bank.fault, "fault");
}
const faultShapes = [
  { name: "second-unmapped", address: 0x4ffe, detail: 1, fault: SECOND, access: 1, missing: SECOND },
  { name: "second-write-only", address: 0x4ffe, detail: 2, fault: SECOND, access: 1, denied: SECOND, permissions: 2 },
  { name: "first-read-only", address: 0x4ffe, detail: 2, fault: 0x4ffe, access: 2, denied: DATA, permissions: 1 },
  { name: "second-read-only", address: 0x4ffe, detail: 2, fault: SECOND, access: 2, denied: SECOND, permissions: 1 },
];
function faultCases(ctx, unit) {
  for (const shape of faultShapes)
    for (const scan of ctx.bank.fault)
      for (const flags of FLAGS) {
        protect(ctx, DATA, 3);
        protect(ctx, SECOND, 3);
        const oldWord = ctx.bank.kind === "left" ? 0x40000000 : 0x80000001;
        upload(ctx, shape.address, words([oldWord]), "pre-fault full DWORD operand");
        const preRestriction = observeWord(ctx, unit, shape.address, "physical pre-restriction operand");
        assert.equal(preRestriction.observed_value, oldWord);
        const candidate = rotate(ctx.bank.kind, preRestriction.observed_value, scan.raw, flags);
        if (scan.raw === 1) assert.notEqual(candidate.flags, flags, "q1 write fault must reject distinct candidate FLAGS");
        if (shape.missing !== undefined) unmap(ctx, shape.missing);
        else protect(ctx, shape.denied, shape.permissions);
        const registers = addressRegisters(8, shape.address);
        seed(ctx, registers, scan.entry, flags);
        registers[3] = (registers[3] + 1) >>> 0;
        const helper = wordHelper(0, shape.detail, shape.fault, shape.access),
          touched = shape.missing === undefined ? [DATA, SECOND] : [DATA];
        const first = run(
          ctx,
          unit,
          2,
          "retired LEA before exact DWORD fault without early FLAGS publication",
          registers,
          scan.target,
          flags,
          5,
          1,
          helper,
          shape.detail,
          shape.fault,
          shape.access,
        );
        counts.prefixes++;
        pages(ctx, "first fault atomic touched pages", touched);
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
          shape.fault,
          shape.access,
        );
        counts.fault_calls += 2;
        counts[shape.access === 1 ? "read_faults" : "write_faults"] += 2;
        pages(ctx, "unrepaired retry atomic touched pages", touched);
        if (shape.missing !== undefined) {
          map(ctx, shape.missing, null);
          counts.map_only_repairs++;
        } else {
          protect(ctx, shape.denied, 3);
          counts.permission_only_repairs++;
        }
        const result = target(ctx, unit, registers, scan, shape.address, flags, "repaired");
        consumers(ctx, unit, registers, scan, result.flags);
        counts.repairs++;
        pages(ctx, "complete repaired cross-page operand and neighbors", [DATA, SECOND]);
        faults.push({
          context: ctx.ordinal,
          kind: ctx.bank.kind,
          owner: ctx.owner,
          raw: scan.raw,
          flags,
          shape,
          helper_version: 1,
          width: 4,
          pre_restriction: preRestriction,
          rejected_candidate: candidate,
          touched_pages: touched,
          first,
          retry,
          repair_mode: shape.missing !== undefined ? "map-only" : "permission-only",
          repair_expected: result,
          cpu_writes_after_first_fault: 0,
          prefix_replays: 0,
        });
      }
  for (const address of [0xfffffffd, 0xffffffff])
    for (const raw of [0, 2])
      for (const flags of FLAGS) {
        const scan = ctx.bank.fault.find((row) => row.raw === raw),
          registers = addressRegisters(8, address),
          helper = wordHelper(0, 3, address, 1);
        seed(ctx, registers, scan.entry, flags);
        registers[3] = (registers[3] + 1) >>> 0;
        const first = run(ctx, unit, 2, "LEA then DWORD address overflow before RAM access", registers,
          scan.target, flags, 5, 1, helper, 3, address, 1);
        counts.prefixes++;
        pages(ctx, "first overflow top page unchanged", [TOP]);
        const retry = run(ctx, unit, 1, "overflow retry retires zero", registers,
          scan.target, flags, 5, 0, helper, 3, address, 1);
        counts.fault_calls += 2;
        counts.read_faults += 2;
        pages(ctx, "overflow retry top page unchanged", [TOP]);
        faults.push({ context: ctx.ordinal, kind: ctx.bank.kind, owner: ctx.owner, raw, flags,
          shape: { name: "address-overflow", address, fault: address, detail: 3, access: 1 },
          helper_version: 1, width: 4, first, retry, repair_mode: null,
          cpu_writes_after_first_fault: 0, prefix_replays: 0 });
      }
  protect(ctx, DATA, 3);
  protect(ctx, SECOND, 3);
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
  assert.equal(ctx.expected.readUInt32LE(96), 0);
  run(ctx, unit, 0, "zero budget before cold guest access", registers, COLD, flags, 1, 0);
  cancel(ctx, 1);
  run(ctx, unit, 1, "cancel before cold guest access", registers, COLD, flags, 2, 0);
  generated(ctx, unit, [0xffffffff, 0xffffffff, 0, 0xffffffff], "malformed pointers before cancel/budget", 1);
  cancel(ctx, 0);
  run(ctx, unit, 1, "cold entry need code", registers, COLD, flags, 3, 0);
}
function close(ctx) {
  if (ctx.expected.readUInt32LE(96) !== 1) cancel(ctx, 1);
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
  pages(ctx, "initial SMC declared pages");
  const scan = ctx.bank.smc.find((row) => row.raw === raw),
    unit = compile(ctx, [scan], "smc"),
    address = scan.entry + 1,
    registers = addressRegisters(8, address),
    flags = 2;
  ownerChecks(ctx, unit);
  seed(ctx, registers, scan.entry, flags);
  registers[6] = 0x80000001;
  run(ctx, unit, 1, "MOV ESI before consumed immediate DWORD code store", registers, scan.target, flags);
  const oldAllocation = Buffer.from(refresh(ctx).bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.deepEqual(oldAllocation, unit.bytes);
  const result = target(ctx, unit, registers, scan, address, flags, "code", 6);
  const replayCounterfactual = rotate(ctx.bank.kind, result.value, raw, result.flags);
  if (raw !== 0) assert.notEqual(replayCounterfactual.value, result.value,
    "nonzero retained-state RMW replay counterfactual changes the committed word");
  counts.code_stores++;
  counts[result.value === 0x80000001 ? "same_value_code_stores" : "changing_code_stores"]++;
  pages(ctx, "complete committed consumed MOV immediate DWORD code store");
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
  const currentCode = Buffer.from(ctx.pages.get(PC)),
    savedCurrent = artifact(`current-${ctx.ordinal}.x86`, currentCode),
    savedTail = artifact(`tail-${ctx.ordinal}.x86`, currentCode.subarray(scan.next - PC, scan.next - PC + 13)),
    tail = { ...scan, entry: scan.next, spec: [scan.next, 13], instructions: 4 },
    current = compile(ctx, [tail], "current-tail", true);
  registers[0] = registers[0] - (registers[0] % 256) + (result.flags % 2);
  registers[2] = registers[2] - (registers[2] % 256) + Number(Boolean(result.flags & 0x800));
  registers[3] = 0xdeadbeef;
  const continuation = run(ctx, current, 5, "fresh guard-only thirteen-byte tail without RMW replay", registers,
    COLD, result.flags, 3, 4);
  counts.setb++; counts.seto++; counts.canaries++; counts.jumps++;
  pages(ctx, "current pure tail without memory replay");
  const retainedTail = Buffer.from(refresh(ctx).bytes.subarray(current.pointer, current.pointer + current.length));
  assert.deepEqual(retainedTail, current.bytes);
  const lastByteAddress = scan.next + 12,
    sameByte = ctx.pages.get(PC)[lastByteAddress - PC];
  upload(ctx, lastByteAddress, Buffer.from([sameByte]), "same-value consumed tail byte upload invalidates currency");
  cancel(ctx, 1);
  let retainedTailAfter;
  const staleTail = generated(ctx, current, [0xffffffff, 0xffffffff, 0, 0xffffffff],
    "stale current tail before malformed arguments cancellation and zero budget", 4, null, () => {
      retainedTailAfter = Buffer.from(ctx.bytes.subarray(current.pointer, current.pointer + current.length));
      assert.deepEqual(retainedTailAfter, retainedTail,
        "first synchronous post-stale observation is complete current allocation");
    });
  counts.stale_calls++;
  codeStores.push({
    context: ctx.ordinal,
    owner: ctx.owner,
    kind: ctx.bank.kind,
    raw,
    address,
    before: 0x80000001,
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
    same_value: result.value === 0x80000001,
    replay_counterfactual: replayCounterfactual,
    head_replay_counterfactual_esi: result.value,
    actual_retained_esi: registers[6],
    q0_replay_ceiling: "q0 is idempotent; exact entry/body/retirement/retained GPR ownership proves continuation scope",
    saved_current: savedCurrent,
    saved_tail: savedTail,
    continuation,
    continuation_bytes: 13,
    continuation_instructions: 4,
    helper_width: 4,
    helper_version: 1,
    stale_tail: staleTail,
    tail_allocation_sha256_before: hash(retainedTail),
    tail_allocation_sha256_after: hash(retainedTailAfter),
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
    close(ctx);
  }
for (const selected of banks)
  for (const owner of ["replacement", "resident"])
    for (const raw of [0, 1, 2]) smc(fresh(owner, selected, `smc-${raw}`), raw);
const rawArenas = artifact("arena-snapshots.bin", Buffer.concat(arenaFrames)),
  rawPages = artifact("page-snapshots.bin", Buffer.concat(pageFrames));
counts.host_inputs = hostInputs.length;
counts.host_calls = hostCalls.length;
counts.files = artifacts.length + 1;
for (const [name, expected] of Object.entries(plan))
  assert.equal(counts[name], expected, `frozen prospective ${name} census`);
assert.deepEqual(pinSources(), sourcePins, "selected sources byte-stable during execution");
assert.equal(hash(readFileSync(enginePath)), engineSha256);
const result = {
  status: "ok",
  profile: "finite flat32 C1 /2,/3 DWORD memory immediate carry rotate",
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
    numeric_basis: { left: 0x80000001, right: 0x92345678 },
    asymmetric_values: VALUES,
    value_raw: VALUE_RAW,
    ea_count_pairings: banks[0].ea.map((row) => ({ form: row.form, raw: row.raw })),
    literal_anchors: ANCHORS,
    registers: REG,
    fault_raw: [0, 1, 2],
    fault_shapes: faultShapes,
    endpoint_addresses: [0x4ffc, 0x4ffe, 0xfffffffc],
    overflow_addresses: [0xfffffffd, 0xffffffff],
    overflow_raw: [0, 2],
    arena_bytes: SIZE,
    native_origin_targets: 140,
    actual_origin_targets: 628,
    limits: { blocks: 8, instructions: 64, wasm_bytes: 65536 },
    patterned_page_sha256: hash(pattern),
  },
  banks: banks.map(({ kind, extension, groups, ea, fault, smc, packed, saved }) => ({
    kind,
    extension,
    groups,
    ea,
    fault,
    smc,
    packed,
    saved,
  })),
  evidence_limits: {
    arithmetic:
      "unchanged sibling33-character oldCF:DWORD repeated rotation with22 literal anchors; q1 definedOF, q2..31 clearsOF by deterministic profile, q0 retains allFLAGS; arithmetic input is physical pre-target Read32 and complete live CPU/FLAGS",
    origins:
      "140 native-Rust bound replacement targets use independently matched actual key/generation+guard and current memory helpers;628 targets use actual-engine-compiled replacement/resident modules; native artifacts have no separate live engine-allocation proof",
    arenas:
      "all3542 generated calls save full4236B before/after;768 targets save separate physical pre/post Read32 frames and96 pre-restriction operands save full frames;8716 physical frames join the declared full known arena model, not an independently certified generic arena model",
    pages:
      "812 complete physical4096B pages, neighbors included; page expectations derive successful map/upload and scalar target model; every successful target physically reads its old and stored DWORD, including post-map repaired words;833120 host Read32 calls are diagnostics separate from guest Read4/Store4",
    lifecycle:
      "96 repair cases retain CPU/EIP/FLAGS/operand after realLEA and zero-retirement retry, repair only map/perms with no CPU or data rewrite;32 overflow cases have no repair; q0 still Read4 plus same-value Store4, including readonly Write faults and consumed MOV immediate code invalidation",
    currency:
      "24 stale4 calls precede cancelled malformed zero-budget arguments and preserve known arenas; first synchronous post-stale callback compares whole retained live actual allocation before getters/arena helpers/fresh compilation;12 fresh guard-only13B tails retire4 without head replay; saved after-allocation hashes are live comparison evidence rather than independently saved full after-byte artifacts",
    scope:
      "finite35raw basis, selected FLAGS/value partitions and8EA/count pairs; no full DWORD×FLAGS×count×EA Cartesian proof, new D3/CL/memory-byte/word/prefix/LOCK/races/MMIO/helperABI/locals/caps/fullCI/performance/browser/graphics/game claim; module parser certifies only types/imports/locals/guard prefix and validates the remaining Wasm with the runtime validator",
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
