import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../../../../../support/wasm/engine.mjs";
import { cache, pair } from "../../../../../support/wasm/cache.mjs";
import { image } from "../producer/scenario.mjs";

const [enginePath, output] = process.argv.slice(2);
mkdirSync(output, { recursive: true });
const artifact = readEngine(enginePath),
  instance = new WebAssembly.Instance(artifact.module, {
    ring3_observability: { now_ns: () => BigInt(Date.now()) * 1000000n },
  });
const memory = instance.exports.memory,
  key = [0x61637469, 0x76617465];
const api = Object.fromEntries(
  Object.entries(instance.exports)
    .filter(([name]) => name.startsWith("ring3_abi_v1_"))
    .map(([name, value]) => [name.slice(13), value]),
);
assert.equal(api.open(128, ...key), 0);
const base = api.arena_ptr() >>> 0;
const get = (offset) => new DataView(memory.buffer).getUint32(base + offset, true);
const take = (length) =>
  Buffer.from(new Uint8Array(memory.buffer).slice(base + 140, base + 140 + length));
const request = (bytes) => new Uint8Array(memory.buffer).set(bytes, base + 140);
const ok = (operation, ...args) => assert.equal(api[operation](...args), 0, operation);
const name = (value) => {
  const bytes = Buffer.from(value);
  request(bytes);
  return bytes.length;
};
const read = (address) => {
  ok("read32", address);
  assert.equal(get(116), 0);
  return get(120);
};
ok("configure_cpu_clock", 1000000000, 0, 0, 0);
ok("configure_execution", 32, 8 * 1024 * 1024, 0xffffffff);
ok("configure_module_limits", 8, 1024 * 1024, 8, 65536, 1024 * 1024);
ok("register_kernel32_module", 0x70000000);
ok("configure_user32", 8, 16, 64, 8, 0x30000000, 16);
ok("register_user32_module", 0x72000000);
ok("configure_media", 48000);
const mediaEpoch = take(64).readBigUInt64LE(16);
const guest = image(),
  a = guest.addresses;
writeFileSync(join(output, "activation-authored.exe"), guest.bytes);
ok("begin_module_input", guest.bytes.length, name("activation-authored.exe"));
for (let offset = 0; offset < guest.bytes.length; offset += 4096) {
  const chunk = guest.bytes.subarray(offset, offset + 4096);
  request(chunk);
  ok("append_module_input", offset, chunk.length);
}
ok("commit_module_input");
ok("start_module_process", name("activation-authored.exe"), 0, 0x7e000000, 4, 0x7f000000);
const returnId = 0xffffe;
let returnPc;
for (let start = 0, total = 1; start < total;) {
  ok("module_gates_page", start, 128);
  const page = take(4096),
    count = page.readUInt32LE(24);
  total = page.readUInt32LE(16);
  assert.ok(count > 0);
  for (let index = 0; index < count; index++)
    if (page.readUInt32LE(36 + index * 8) === returnId)
      returnPc = page.readUInt32LE(32 + index * 8);
  start += count;
}
assert.ok(returnPc, "canonical Windows callback return gate");
const code = cache({ api, memory, key, base, get, take, request, ok, epoch: 0n }, output);
const callSites = new Map(guest.calls.map((call) => [guest.entry + call.end, call]));
const callbacks = [],
  checkpoints = [];
let inputSequence = 0n;
function focus(focused) {
  const batch = Buffer.alloc(64);
  batch.write("R3IB");
  batch.writeUInt16LE(1, 4);
  batch.writeUInt16LE(32, 6);
  batch.writeBigUInt64LE(mediaEpoch, 8);
  batch.writeBigUInt64LE(++inputSequence, 16);
  batch.writeUInt32LE(1, 24);
  batch.writeBigUInt64LE(inputSequence * 1000000n, 32);
  batch.writeUInt32LE(4, 40);
  batch.writeUInt32LE(Number(focused), 44);
  ok("observe_cpu_clock", ...pair(inputSequence * 1000000n));
  request(batch);
  ok("submit_windows_input", batch.length);
  ok("sync_windows_input");
}
focus(true);
function finishCallbacks() {
  for (let pendingCount = 0; pendingCount < 64; pendingCount++) {
    ok("pending_windows_callback");
    const pending = take(96);
    if (!pending.readUInt32LE(12)) return;
    assert.equal(pending.readUInt32LE(12), 1);
    assert.equal(pending.readUInt32LE(16), guest.callbacks[0].entry);
    const args = Array.from({ length: 4 }, (_, index) => pending.readUInt32LE(32 + index * 4));
    const target = code.entry(guest.callbacks[0].entry);
    ok("begin_windows_resident_callback", ...key, ...pair(target.id), returnPc, returnId);
    const record = take(72),
      token = record.readUInt32LE(16),
      home = record.readBigUInt64LE(64);
    ok("authorize_resident_callback", ...key, ...pair(home), token);
    let finished = false;
    for (let step = 0; step < 64; step++) {
      const unit = code.entry(get(48));
      ok("select_resident_callback_unit", ...key, ...pair(home), token, ...pair(unit.id));
      const run = code.execute(unit);
      if (run.reason === 1 || run.reason === 3) continue;
      assert.equal(run.reason, 8);
      assert.equal(run.detail, returnId);
      ok("finish_windows_resident_callback", ...key, ...pair(home), token);
      finished = true;
      break;
    }
    assert.ok(finished, "bounded real WndProc return");
    assert.deepEqual(
      Array.from({ length: 4 }, (_, index) => read(a.windowArguments + index * 4)),
      args,
    );
    callbacks.push(args);
  }
  assert.fail("bounded pending callback inventory");
}
function verify(hook) {
  const app = callbacks.filter((args) => args[1] === 0x1c);
  if (hook === "first-shown" || hook === "same-app") {
    assert.equal(
      app.length,
      1,
      "same GUI application and child focus cannot duplicate app activation",
    );
    assert.deepEqual(app[0], [read(a.first), 0x1c, 1, 0]);
    assert.equal(read(a.active), 1);
  } else if (
    hook === "physical-blur" ||
    hook === "physical-restore" ||
    hook === "duplicate-restore"
  ) {
    const restored = hook !== "physical-blur",
      expectedCount = restored ? 5 : 3;
    assert.equal(app.length, expectedCount);
    const expected = [read(a.first), read(a.second)].sort((a, b) => a - b);
    const tail = app.slice(restored ? 3 : 1, restored ? 5 : 3);
    assert.deepEqual(
      tail.map((args) => args[0]).sort((a, b) => a - b),
      expected,
    );
    for (const args of tail) assert.deepEqual(args.slice(1), [0x1c, Number(restored), 0]);
    assert.equal(read(a.active), Number(restored));
    assert.equal(
      app.some((args) => args[0] === read(a.child)),
      false,
    );
  }
  assert.equal(read(a.activationCount), app.length);
  if (hook === "no-posted-app") assert.equal(app.length, 5);
  checkpoints.push({ hook, active: read(a.active), applicationCallbacks: app.length });
}
let exited = false;
for (let iteration = 0; iteration < 4096; iteration++) {
  if (get(72) === 9) {
    exited = true;
    break;
  }
  const unit = code.entry(get(48)),
    run = code.execute(unit);
  if (run.reason === 1 || run.reason === 3) continue;
  assert.equal(run.reason, 8);
  ok("capture_resident_windows_call", ...key, ...pair(unit.id));
  const frame = take(112),
    token = frame.readUInt32LE(16),
    call = callSites.get(frame.readUInt32LE(40));
  assert.ok(call);
  assert.equal(call.id, run.detail);
  if (call.hook === "physical-blur") focus(false);
  else if (call.hook === "physical-restore" || call.hook === "duplicate-restore") focus(true);
  let completed = false;
  for (let retry = 0; retry < 64; retry++) {
    const result = api.complete_resident_windows_call(...key, ...pair(unit.id), token);
    assert.ok([0, 12].includes(result), `${call.name} completion returned ${result}`);
    if (get(72) === 9) {
      completed = true;
      break;
    }
    finishCallbacks();
    if (result === 0) {
      completed = true;
      break;
    }
  }
  assert.ok(completed, "original captured call resumes after sent activation callbacks");
  if (call.expected === "pointer") assert.ok(get(16) !== 0 && get(16) !== 0xffffffff);
  else if (call.expected === "atom") assert.ok(get(16) >= 0xc000 && get(16) <= 0xffff);
  else if (call.expected?.ref) assert.equal(get(16), read(a[call.expected.ref]), call.name);
  else if (typeof call.expected === "number") assert.equal(get(16), call.expected, call.name);
  if (call.name === "PeekMessageA")
    assert.equal(get(16), 0, "sent activation bypasses posted-message filter");
  if (call.hook) {
    try {
      verify(call.hook);
    } catch (error) {
      writeFileSync(
        join(output, "failure.json"),
        JSON.stringify(
          {
            engine_sha256: artifact.sha256,
            hook: call.hook,
            active: read(a.active),
            activationCount: read(a.activationCount),
            callbacks,
            error: error.message,
          },
          null,
          2,
        ),
      );
      throw error;
    }
  }
}
assert.ok(exited);
assert.equal(read(a.activationCount), 5);
const result = {
  engine_sha256: artifact.sha256,
  guest_sha256: createHash("sha256").update(guest.bytes).digest("hex"),
  checkpoints,
  applicationCallbacks: callbacks.filter((args) => args[1] === 0x1c),
  generatedWndProcCalls: callbacks.length,
  ...code.stats(),
  claim:
    "authored PE32 real WndProc; normal ShowWindow and physical focus, same-app/child negatives; no posted activation injection or game bytes",
};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2));
console.log(JSON.stringify(result));
