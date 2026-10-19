import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { Fixture, KEY, pair } from "../../cohort/fixtures/support.mjs";
import { ROOTS, STARTS, initial, initialData, initialFp, packet, validate, compareArchitecture } from "./oracle-fp.mjs";

const [artifact, output, root, expectation = "candidate"] = process.argv.slice(2);
assert.ok(artifact && output && root, "engine, fresh output directory, repository root");
assert.ok(["candidate", "no-reoptimization"].includes(expectation));
mkdirSync(output, { recursive: true });
for (const name of ["result.json", "checkpoint.json"])
  assert.ok(!existsSync(join(output, name)), "use a fresh output directory");
const { readEngine } = await import(pathToFileURL(join(root, "engine/tests/support/wasm/engine.mjs")));
const engine = readEngine(artifact), hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const cases = [], transactions = [], modules = [];
const result = {
  engine_sha256: engine.sha256,
  expectation,
  sources: [import.meta.url, new URL("oracle-fp.mjs", import.meta.url).href, new URL("oracle.mjs", import.meta.url).href, new URL("../../cohort/fixtures/support.mjs", import.meta.url).href].map((url) => ({ url, sha256: hash(readFileSync(new URL(url))) })),
  support_before_optional_limits_sha256: "76480aedf97caff24023bf5d8f34a4d5afa157824bef51242ed326be9be22bc8",
  contexts: [], cases, transactions, modules,
};
const hex = (bytes) => Buffer.from(bytes).toString("hex");
const checkpoint = () => writeFileSync(join(output, "checkpoint.json"), JSON.stringify(result, null, 2));

// all external transfers are indirect; no other owner's static root is discoverable.
const sources = [
  [0x1000, [0x83, 0xc0, 1, 0xd9, 0x43, 0x10, 0x85, 0xff, 0x0f, 0x84, 0xf2, 0, 0, 0]],
  [0x100e, [0x83, 0x43, 4, 17, 0xe9, 0xe9, 0, 0, 0]],
  [0x1100, [0xff, 0xe6]],
  [0x1400, [0x83, 0xc5, 2, 0xd8, 0x43, 0x14, 0x83, 0x03, 1, 0x85, 0xff, 0x0f, 0x84, 0xef, 0, 0, 0]],
  [0x1411, [0x83, 0x43, 12, 33, 0xe9, 0xe6, 0, 0, 0]],
  [0x1500, [0xff, 0xe1]],
  [0x1800, [0xd8, 0x4b, 0x18, 0xd9, 0x5b, 0x1c, 0x83, 0x43, 8, 3, 0x85, 0xff, 0x0f, 0x84, 0xee, 0, 0, 0]],
  [0x1812, [0x83, 0x43, 32, 49, 0xe9, 0xe5, 0, 0, 0]],
  [0x1900, [0xff, 0xe2]],
];
const codePage = new Uint8Array(4096);
for (const [pc, bytes] of sources) codePage.set(bytes, pc - 0x1000);
result.guest_code_sha256 = hash(codePage);
result.guest_sources = sources.map(([pc, bytes]) => ({ pc, bytes: hex(bytes) }));

function rawRequest(e, operation, ...args) {
  const request = e.request(operation, ...args), raw = e.bytes(e.base + 140, 64), view = new DataView(raw.buffer);
  assert.equal(Buffer.from(raw.subarray(0, 4)).toString(), "R3CQ");
  assert.equal(view.getUint32(4, true), 0x10001);
  assert.equal(view.getUint32(8, true), 64);
  const present = view.getUint32(12, true);
  assert.ok(present === 0 || present === 1);
  if (present === 0) assert.ok(raw.subarray(12).every((value) => value === 0));
  transactions.push({ context: result.activeContext, operation, raw: hex(raw), present, ...request, serial: request.serial.toString(), id: request.id.toString(), victim: request.victim.toString() });
  return { ...request, present };
}

function saveModule(unit) {
  const imports = WebAssembly.Module.imports(new WebAssembly.Module(unit.material.bytes));
  modules.push({ context: result.activeContext, id: unit.id.toString(), slot: unit.slot, pc: unit.pc, tier: unit.tier,
    sha256: unit.material.sha256, bytes: hex(unit.material.bytes), imports });
}

function install(e, pc, request) {
  const unit = e.install(pc, request);
  assert.equal(unit.pc, pc);
  saveModule(unit);
  return unit;
}

function warm(e) {
  const owners = ROOTS.map((pc) => install(e, pc, rawRequest(e, "prepare_cache_entry", ...KEY, pc)));
  assert.ok(owners.every((unit) => unit.tier === 0));
  for (let index = 0; index < owners.length; index++) {
    for (let hit = 0; hit < 8; hit++) assert.equal(e.find(ROOTS[index]).status, 0);
    const request = rawRequest(e, "prepare_cache_promotion", ...KEY);
    assert.equal(request.present, 1);
    assert.equal(request.kind, 2);
    assert.equal(request.tier, 1);
    assert.equal(request.victim, owners[index].id, "only the selected baseline owner is promoted");
    e.acknowledge(request);
    const next = rawRequest(e, "prepare_cache_entry", ...KEY, ROOTS[index]);
    assert.equal(next.present, 1);
    assert.equal(next.kind, 1, "no other owner's indirect successor can be statically absorbed");
    owners[index] = install(e, ROOTS[index], next);
    assert.equal(owners[index].tier, 1);
  }
  for (let index = 0; index < STARTS.length; index++)
    for (const pc of STARTS[index]) assert.deepEqual(e.find(pc), { status: 0, id: owners[index].id, slot: owners[index].slot });
  return owners;
}

function scratch(e, operation) {
  const before = e.bytes(e.base + 140, 4096);
  try { return operation(); } finally { e.put(before); }
}

function usage(e) {
  return scratch(e, () => {
    e.ok("compiled_execution_usage", ...KEY);
    const bytes = e.bytes(e.base + 140, 80);
    packet(bytes, "R3EU", 80);
    return bytes;
  });
}

function readPages(e) {
  const before = e.bytes(e.base, 4796), pages = [];
  try {
    for (const address of [0x1000, 0x8000]) {
      const bytes = new Uint8Array(4096), view = new DataView(bytes.buffer);
      for (let at = 0; at < 4096; at += 4) {
        e.ok("read32", address + at);
        assert.equal(e.word(116), 0, "mapped diagnostic read");
        view.setUint32(at, e.word(120), true);
      }
      pages.push(bytes);
    }
    assert.deepEqual(e.bytes(e.base, 100), before.subarray(0, 100));
    assert.deepEqual(e.bytes(e.base + 140, 4796 - 140), before.subarray(140));
  } finally {
    new Uint8Array(e.memory.buffer, e.base + 100, 40).set(before.subarray(100, 140));
    assert.deepEqual(e.bytes(e.base, 4796), before);
  }
  return { code: pages[0], data: pages[1] };
}

function census(e, owners, fused) {
  if (typeof e.api.begin_execution_census !== "function") return null;
  return scratch(e, () => {
    e.ok("stop_execution_census", ...KEY);
    e.ok("execution_census_page", ...KEY, 0);
    const raw = e.bytes(e.base + 140, 4096);
    result.active.census = hex(raw); checkpoint();
    const view = packet(raw, "R3CE", 4096);
    assert.equal(view.getUint32(24, true), 2);
    assert.equal(view.getUint32(28, true), 32);
    const rows = view.getUint32(32, true);
    assert.equal(view.getUint32(36, true), 0);
    assert.equal(view.getUint32(40, true), rows);
    assert.equal(view.getUint32(44, true), 80);
    assert.ok(rows <= 32);
    for (const at of [80, 88, 96, 104, 112]) assert.equal(view.getBigUint64(at, true), 0n);
    assert.equal(view.getUint32(120, true), 0);
    assert.equal(view.getUint32(124, true), 0);
    assert.ok(raw.subarray(128 + rows * 80).every((value) => value === 0));
    const actual = [];
    for (let i = 0; i < rows; i++) {
      const at = 128 + i * 80;
      assert.ok(view.getUint32(at + 64, true) > 0);
      assert.equal(view.getUint32(at + 72, true), 1);
      assert.equal(view.getUint32(at + 76, true), 0);
      actual.push([view.getBigUint64(at, true), ...[8, 12, 16, 20].map((off) => view.getUint32(at + off, true)),
        ...[24, 32, 40].map((off) => view.getBigUint64(at + off, true)),
        ...[48, 52, 56, 60, 68].map((off) => view.getUint32(at + off, true))]);
    }
    const expected = fused ? [
      [owners[0].id, 0x1000, 0x1000, 1, 3, 1n, 144n, 144n, 144, 144, 9, 23, 1],
    ] : [
      [owners[0].id, 0x1000, 0x1403, 3, 1, 9n, 45n, 45n, 5, 5, 3, 7, 1],
      [owners[1].id, 0x1403, 0x1800, 3, 1, 9n, 45n, 45n, 5, 5, 3, 8, 1],
      [owners[2].id, 0x1800, 0x1000, 3, 1, 8n, 48n, 48n, 6, 6, 3, 8, 1],
      [owners[2].id, 0x1800, 0x1000, 1, 3, 1n, 6n, 6n, 6, 6, 3, 8, 1],
    ];
    const order = (a, b) => JSON.stringify(a.map(String)).localeCompare(JSON.stringify(b.map(String)));
    assert.deepEqual(actual.sort(order), expected.sort(order), "literal complete child-edge census");
    assert.equal(view.getBigUint64(48, true), fused ? 1n : 27n);
    assert.equal(view.getBigUint64(56, true), fused ? 1n : 27n);
    assert.equal(view.getBigUint64(64, true), 144n);
    assert.equal(view.getBigUint64(72, true), 144n);
    return hex(raw);
  });
}

function reset(e, template, options) {
  e.put(initialData(options)); e.ok("upload", 0x8000, 4096);
  new Uint8Array(e.memory.buffer, e.base, 4796).set(template);
  new Uint8Array(e.memory.buffer, e.base + 4236, 128).set(initialFp(template, options));
  e.set(4, 0x00010003); e.set(8, 56); e.set(12, 0);
  const state = initial(options);
  state.gpr.forEach((value, index) => e.set(16 + index * 4, value));
  e.set(48, state.pc); e.set(52, state.flags); e.set(96, Number(Boolean(options.cancel)));
}

function run(e, template, owners, fused, label, budget, options = {}, mode = "compiled", capture = false) {
  result.active = { label, budget, options, mode, fused }; checkpoint();
  reset(e, template, options);
  const before = usage(e);
  const canCensus = capture && typeof e.api.begin_execution_census === "function";
  if (canCensus) e.ok("begin_execution_census", ...KEY, 32);
  const pc = options.pc ?? 0x1000, installed = e.find(pc);
  assert.equal(installed.status, 0);
  const begin = mode === "compiled" ? "begin_compiled_execution_receipt" : "begin_execution_receipt";
  e.ok(begin, ...KEY, budget, ...pair(installed.id), 1);
  const frame = e.bytes(e.base + 140, 32), f = packet(frame, "R3EF", 32);
  const token = f.getBigUint64(16, true);
  assert.equal(e.dispatch(e.base, e.base + 56, budget, e.base + 96), 0);
  const outerExit = e.bytes(e.base + 56, 40), work = e.bytes(e.base + 4588, 24);
  e.ok("finish_execution_receipt", ...KEY, ...pair(token));
  const receipt = e.bytes(e.base + 140, 64), arena = e.bytes(e.base, 4796);
  Object.assign(result.active, { frame: hex(frame), outerExit: hex(outerExit), work: hex(work), receipt: hex(receipt), arena: hex(arena) });
  checkpoint();
  const nativeCensus = canCensus ? census(e, owners, fused) : null;
  const after = usage(e), pages = readPages(e);
  const record = { budget, mode, frame, outerExit, work, receipt, arena, usageBefore: before, usageAfter: after, ...pages };
  const retained = { label, options, fused, budget, mode,
    frame: hex(frame), outerExit: hex(outerExit), work: hex(work), receipt: hex(receipt), arena: hex(arena),
    code: hex(pages.code), data: hex(pages.data), usageBefore: hex(before), usageAfter: hex(after), census: nativeCensus };
  cases.push(retained); checkpoint();
  const summary = validate(record, template, codePage, owners, fused, options);
  Object.assign(retained, summary);
  if (summary.reason === 23) e.ok("take_cpu_event");
  delete result.active; checkpoint();
  return record;
}

function fuse(e, owners) {
  for (let i = 0; i < 64; i++) assert.equal(e.find(0x1000).status, 0);
  let request = rawRequest(e, "prepare_cache_promotion", ...KEY);
  if (expectation === "no-reoptimization") {
    assert.equal(request.present, 0, "old engine cannot select an optimized second-pass root");
    return null;
  }
  assert.equal(request.present, 1, "mature real need-code hints enable second-pass promotion");
  const victims = [];
  while (request.kind === 2) {
    assert.ok(victims.length < 3);
    assert.ok(owners.some((owner) => owner.id === request.victim));
    assert.ok(!victims.includes(request.victim));
    victims.push(request.victim);
    e.acknowledge(request);
    request = rawRequest(e, "prepare_cache_entry", ...KEY, 0x1000);
  }
  assert.equal(request.kind, 1);
  assert.equal(victims.length, 3, "complete nine-block closure retires all three whole owners");
  const formed = install(e, 0x1000, request);
  assert.equal(formed.tier, 1);
  for (const pc of STARTS.flat()) assert.deepEqual(e.find(pc), { status: 0, id: formed.id, slot: formed.slot });
  result.replacement = { id: formed.id.toString(), slot: formed.slot, victims: victims.map(String) };
  return formed;
}

const controls = [
  ...[0, 1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 15, 16, 17, 32, 272].map((budget) => ({ label: `fp-prefix-${budget}`, budget })),
  { label: "cold-roots", budget: 22, options: { edi: 1 } },
  ...STARTS.flat().map((pc) => ({ label: `entry-${pc.toString(16)}`, budget: 1,
    options: { pc, ...(pc === 0x1403 ? { fpSeed: "onePointFive" } : pc === 0x1800 ? { fpSeed: "four" } : pc === 0x1803 ? { fpSeed: "eight" } : {}) } })),
  { label: "incoming-zf-false", budget: 1, options: { pc: 0x1008, edi: 0, flags: 2 } },
  { label: "incoming-zf-true", budget: 1, options: { pc: 0x1008, edi: 1, flags: 0x46 } },
  { label: "pre-cancel", budget: 32, options: { cancel: true } },
  { label: "fld-unmapped", budget: 32, options: { ebx: 0xdead0000 } },
  { label: "fadd-after-fld-fault", budget: 32, options: { ebx: 0x8fec } },
  { label: "fstp-after-fmul-fault", budget: 32, options: { ebx: 0x8fe4 } },
];

for (const cw of [0x007f, 0x027f, 0x037f]) for (const mode of ["checked", "compiled"]) {
  const prefix = `cw${cw}-${mode}`, context = { cw, mode };
  result.activeContext = prefix;
  result.contexts.push(context);
  const e = new Fixture(engine.module, sources, { instructions: 256, wasmBytes: 512 * 1024 });
  try {
    const owners = warm(e), template = e.bytes(e.base, 4796);
    context.initialOwners = owners.map((owner) => ({ id: owner.id.toString(), slot: owner.slot, pc: owner.pc, tier: owner.tier }));
    context.initialArena = hex(template);
    const training = run(e, template, owners, false, `${prefix}-real-dispatch-training-nine-cycles`, 144, { cw }, mode, true);
    const before = controls.map((control) => run(e, template, owners, false, `${prefix}-before-${control.label}`, control.budget, { cw, ...control.options }, mode));
    const formed = fuse(e, owners), fused = formed !== null, afterOwners = fused ? [formed] : owners;
    context.replacement = result.replacement ?? null;
    delete result.replacement;
    const afterTraining = run(e, template, afterOwners, fused, `${prefix}-after-nine-cycles`, 144, { cw }, mode, true);
    compareArchitecture(training, afterTraining);
    for (let i = 0; i < controls.length; i++) {
      const control = controls[i], after = run(e, template, afterOwners, fused, `${prefix}-after-${control.label}`, control.budget, { cw, ...control.options }, mode);
      compareArchitecture(before[i], after);
    }
    for (let owner = 0; owner < STARTS.length; owner++) for (const pc of STARTS[owner]) assert.deepEqual(e.find(pc), {
      status: 0, id: (formed ?? owners[owner]).id, slot: (formed ?? owners[owner]).slot,
    });
    context.censusAvailable = typeof e.api.begin_execution_census === "function";
    context.status = "PASS"; checkpoint();
  } catch (error) {
    result.status = "FAIL"; context.status = "FAIL";
    result.error = { message: error.message, stack: error.stack }; checkpoint(); throw error;
  } finally { e.close(); }
}
delete result.activeContext;
result.status = "PASS"; checkpoint();
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2));
console.log(JSON.stringify({ engine_sha256: engine.sha256, expectation, cases: cases.length, contexts: result.contexts.length, output, status: result.status }));
