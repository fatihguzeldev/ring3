import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../../../../../../support/wasm/engine.mjs";
import { cache, pair } from "../../../../../../support/wasm/cache.mjs";
import { image } from "../producer/scenario.mjs";
import { verify } from "./oracles.mjs";

const [enginePath, output] = process.argv.slice(2);
mkdirSync(output, { recursive: true });
const artifact = readEngine(enginePath);
const instance = new WebAssembly.Instance(artifact.module, {
  ring3_observability: { now_ns: () => BigInt(Date.now()) * 1000000n },
});
const memory = instance.exports.memory, key = [0x6b657962, 0x686f6f6b];
const api = Object.fromEntries(Object.entries(instance.exports)
  .filter(([name]) => name.startsWith("ring3_abi_v1_"))
  .map(([name, value]) => [name.slice(13), value]));
assert.equal(api.open(128, ...key), 0);
const base = api.arena_ptr() >>> 0;
const get = (offset) => new DataView(memory.buffer).getUint32(base + offset, true);
const take = (length) => Buffer.from(new Uint8Array(memory.buffer).slice(base + 140, base + 140 + length));
const request = (bytes) => new Uint8Array(memory.buffer).set(bytes, base + 140);
const ok = (operation, ...args) => assert.equal(api[operation](...args), 0, operation);
const name = (value) => { const bytes = Buffer.from(value); request(bytes); return bytes.length; };
const read = (address) => {
  ok("read32", address);
  assert.equal(get(116), 0);
  return get(120);
};
ok("configure_cpu_clock", 1000000000, 0, 0, 0);
ok("configure_execution", 64, 8 * 1024 * 1024, 0xffffffff);
ok("configure_module_limits", 8, 1024 * 1024, 8, 65536, 1024 * 1024);
ok("configure_user32", 8, 16, 64, 16, 0x30000000, 16);
ok("register_kernel32_module", 0x70000000);
ok("register_user32_module", 0x72000000);
ok("configure_media", 48000);
const mediaEpoch = take(64).readBigUInt64LE(16);
const guest = image(), a = guest.addresses;
writeFileSync(join(output, "keyboard-hooks-authored.exe"), guest.bytes);
ok("begin_module_input", guest.bytes.length, name("keyboard-hooks-authored.exe"));
for (let offset = 0; offset < guest.bytes.length; offset += 4096) {
  const chunk = guest.bytes.subarray(offset, offset + 4096);
  request(chunk);
  ok("append_module_input", offset, chunk.length);
}
ok("commit_module_input");
ok("start_module_process", name("keyboard-hooks-authored.exe"), 0, 0x7e000000, 4, 0x7f000000);
const returnId = 0xffffe;
let returnPc;
for (let start = 0, total = 1; start < total;) {
  ok("module_gates_page", start, 128);
  const page = take(4096), count = page.readUInt32LE(24);
  total = page.readUInt32LE(16);
  assert.ok(count > 0);
  for (let index = 0; index < count; index++)
    if (page.readUInt32LE(36 + index * 8) === returnId)
      returnPc = page.readUInt32LE(32 + index * 8);
  start += count;
}
assert.ok(returnPc);
const code = cache({ api, memory, key, base, get, take, request, ok, epoch: 0n }, output);
const callSites = new Map(guest.calls.map((call) => [guest.entry + call.end, call]));
const callbacks = [], callbackHistory = [], checkpoints = [];
let inputSequence = 0n, mainCall, waited = false, exited = false;

function input(events) {
  const batch = Buffer.alloc(32 + events.length * 32);
  batch.write("R3IB");
  batch.writeUInt16LE(1, 4);
  batch.writeUInt16LE(32, 6);
  batch.writeBigUInt64LE(mediaEpoch, 8);
  batch.writeBigUInt64LE(++inputSequence, 16);
  batch.writeUInt32LE(events.length, 24);
  const timestamp = inputSequence * 1000000n;
  for (const [index, [kind, first, second = 0]] of events.entries()) {
    const offset = 32 + index * 32;
    batch.writeBigUInt64LE(timestamp, offset);
    batch.writeUInt32LE(kind, offset + 8);
    batch.writeUInt32LE(first, offset + 12);
    batch.writeUInt32LE(second, offset + 16);
  }
  ok("observe_cpu_clock", ...pair(timestamp));
  request(batch);
  ok("submit_windows_input", batch.length);
  ok("sync_windows_input");
}
input([[4, 1]]);

function pendingCallback() {
  ok("pending_windows_callback");
  const pending = take(96);
  if (!pending.readUInt32LE(12)) return;
  assert.equal(pending.readUInt32LE(12), 1);
  const procedure = pending.readUInt32LE(16), count = pending.readUInt32LE(20);
  assert.ok([guest.wndproc, ...guest.callbacks.map((item) => item.entry)].includes(procedure));
  assert.equal(count, procedure === guest.wndproc ? 4 : 3);
  const entry = {
    procedure,
    arguments: Array.from({ length: count }, (_, index) => pending.readUInt32LE(32 + index * 4)),
  };
  const target = code.entry(procedure);
  ok("begin_windows_resident_callback", ...key, ...pair(target.id), returnPc, returnId);
  const record = take(72);
  const callback = { ...entry, home: record.readBigUInt64LE(64), token: record.readUInt32LE(16), active: target.id };
  ok("authorize_resident_callback", ...key, ...pair(callback.home), callback.token);
  callbacks.push(callback);
  callbackHistory.push(entry);
}
function waiting() {
  ok("waiting_user32_message");
  return take(64).readBigUInt64LE(24);
}
function trigger(hook) {
  if (hook === "peek-down") input([[1, 30, 1]]);
  else if (hook === "remove-up") input([[1, 30, 0]]);
  else if (hook === "suppress-noremove") input([[1, 48, 1]]);
  else if (hook === "skip-suppressed") input([[1, 46, 1], [1, 32, 1]]);
  else if (hook === "unhooked") input([[1, 33, 1]]);
}

try {
  for (let iteration = 0; iteration < 4096; iteration++) {
    if (get(72) === 9) { exited = true; break; }
    pendingCallback();
    if (!callbacks.length && mainCall) {
      const wait = waiting();
      if (wait) {
        assert.equal(mainCall.hook, "waiting-get");
        assert.equal(waited, false, "the original GetMessage parks once before real input");
        assert.deepEqual(Array.from({ length: 7 }, (_, index) => read(a.message + index * 4)),
          Array(7).fill(0xa5a5a5a5));
        waited = true;
        input([[1, 18, 1]]);
        ok("resume_user32_wait", ...key, ...pair(wait));
        continue;
      }
      assert.equal(get(48), mainCall.returnPc, "provider returns to the original guest call site");
      const checkpoint = verify(mainCall, { get, read, guest, callbackHistory });
      if (checkpoint) checkpoints.push(checkpoint);
      mainCall = undefined;
    }
    const unit = code.entry(get(48)), callback = callbacks.at(-1);
    if (callback) {
      if (get(72) === 3) {
        ok("select_resident_callback_unit", ...key, ...pair(callback.home), callback.token, ...pair(unit.id));
        callback.active = unit.id;
      } else {
        assert.equal(get(72), 1);
        assert.equal(unit.id, callback.active);
      }
    }
    const run = code.execute(unit);
    if (run.reason === 1 || run.reason === 3) continue;
    assert.equal(run.reason, 8);
    if (callback && run.detail === returnId && run.pc === returnPc) {
      const row = callbackHistory.findLast((item) => item.procedure === callback.procedure && item.result === undefined);
      row.result = get(16);
      ok("finish_windows_resident_callback", ...key, ...pair(callback.home), callback.token);
      callbacks.pop();
      continue;
    }
    if (callback) {
      assert.equal(run.detail, 0x200d3, "only authored CallNextHookEx is called inside a hook");
      ok("capture_active_resident_callback_windows_call", ...key, ...pair(unit.id), callback.token);
      const token = take(112).readUInt32LE(16);
      ok("complete_active_resident_callback_windows_call", ...key, ...pair(unit.id), callback.token, token);
    } else {
      assert.equal(mainCall, undefined);
      ok("capture_resident_windows_call", ...key, ...pair(unit.id));
      const frame = take(112), call = callSites.get(frame.readUInt32LE(40));
      assert.ok(call);
      assert.equal(call.id, run.detail);
      mainCall = { ...call, start: callbackHistory.length, returnPc: frame.readUInt32LE(40) };
      trigger(call.hook);
      ok("complete_resident_windows_call", ...key, ...pair(unit.id), frame.readUInt32LE(16));
    }
  }
  assert.ok(exited, "authored guest reaches ExitProcess");
  assert.ok(waited, "real empty GetMessage wait was resumed with physical input");
  assert.equal(callbacks.length, 0);
  assert.equal(callbackHistory.filter((row) => row.procedure !== guest.wndproc).length, 16);
  const result = {
    engine_sha256: artifact.sha256,
    guest_sha256: createHash("sha256").update(guest.bytes).digest("hex"),
    checkpoints, callbackHistory, ...code.stats(),
    sources: [
      "https://learn.microsoft.com/en-us/windows/win32/winmsg/keyboardproc",
      "https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-callnexthookex",
    ],
    claim: "authored PE32 executes keyboard hooks and nested CallNextHookEx; real input, action/no-remove, suppression, pending GetMessage and posted/unhooked negatives",
  };
  writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
} catch (error) {
  writeFileSync(join(output, "failure.json"), JSON.stringify({
    engine_sha256: artifact.sha256, mainCall, callbackHistory, checkpoints,
    pc: get(48), exit: get(72), eax: get(16), error: error.stack,
  }, null, 2));
  throw error;
}
