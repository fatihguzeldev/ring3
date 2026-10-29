import { program, names } from "./compare-programs.mjs";
import { check, fresh, reset, execute, validate, compare, publicResult, modules, hash, hex } from "./compare-core.mjs";
import { Engine, KEY } from "/runtime/engine.js";
import { CodeCache } from "/runtime/cache.js";
import { TaskScheduler } from "/runtime/scheduler/index.js";

const host = { Engine, KEY, CodeCache };
let context, scheduler, loop, lastReturned, lastRawReceipt, running = false, paused = false, retired = 0, chunks = 0, token;
const post = (type, value = {}) => postMessage({ type, token, atMs: performance.now(), ...value });
function current() {
  const engine = context.engine;
  return { retired, calls: chunks, receipts: lastReturned ? [{ ...lastReturned, owner: String(lastReturned.owner), raw: lastRawReceipt }] : [],
    last: lastReturned ? { ...lastReturned, owner: String(lastReturned.owner) } : null, reason: lastReturned?.reason ?? 1, pc: engine.get(48), flags: engine.get(52),
    registers: Array.from({ length: 8 }, (_, i) => engine.get(16 + i * 4)) };
}
async function compute() {
  try {
    while (running) {
      if (paused) { await scheduler.wait(4); continue; }
      const unit = context.owners.get(context.engine.get(48)); check(unit, "compute resumes an installed authored entry");
      context.cache.budget = 512;
      const result = context.cache.execute(unit);
      lastReturned = result; lastRawReceipt = hex(context.engine.take(64));
      check([1, 3].includes(result.reason) && result.retired > 0, "compute has positive bounded progress");
      retired += result.retired; chunks++;
      if (chunks === 1 || chunks % 64 === 0) post("progress", current());
      await scheduler.wait(0);
    }
  } catch (error) { running = false; post("error", { message: error.stack ?? error.message }); }
}
self.onmessage = async ({ data }) => {
  try {
    if (data.type === "prepare") {
      check(!context, "one proof session per Worker"); token = data.token;
      const artifacts = [];
      for (const artifact of data.artifacts) {
        const response = await fetch(artifact.url); check(response.ok, "explicit engine artifact URL");
        const bytes = await response.arrayBuffer(), sha256 = await hash(bytes);
        check(sha256 === artifact.sha256, `browser artifact SHA pin for ${artifact.label}`);
        artifacts.push({ ...artifact, sha256, bytes: bytes.byteLength, module: await WebAssembly.compile(bytes) });
      }
      const correctness = [];
      for (const name of names) {
        const p = program(name), contexts = artifacts.map(a => fresh(a, p, host));
        try {
          const topology = await Promise.all(contexts.map(async c => ({ label: c.artifact.label, modules: await modules(c) })));
          const values = contexts.map(c => { reset(c, 3, { cw: 0x037f }); return validate(c, execute(c), 3, { cw: 0x037f }); });
          compare(...values); correctness.push({ name, topology, values: values.map(publicResult) });
        } finally { for (const c of contexts) c.engine.ok("close"); }
      }
      context = fresh(artifacts[1], program("mixed-call-store"), host);
      reset(context, 1000000, { cw: 0x037f }); scheduler = new TaskScheduler();
      post("ready", { artifacts: artifacts.map(({ module, ...v }) => v), correctness,
        boundary: "actual Engine/CodeCache/table/receipts and Worker TaskScheduler; authored control shim, not the full game Session or graphics/audio lifecycle" });
      return;
    }
    if (!context || data.token !== token) { post("ignored", { incomingToken: data.token }); return; }
    if (data.type === "run") { check(!loop, "run once"); running = true; loop = compute(); }
    else if (data.type === "ping") post("pong", { requestId: data.requestId, ...current() });
    else if (data.type === "pause") {
      paused = true; const proof = validate(context, current(), 1000000, { cw: 0x037f, work: retired });
      post("paused", { publication: publicResult(proof) });
    } else if (data.type === "probe-paused") { check(paused, "probe only while paused"); post("paused-probe", current()); }
    else if (data.type === "resume") { check(paused, "resume follows pause"); paused = false; post("resumed", current()); }
    else if (data.type === "stop") {
      running = false; await loop;
      const publication = publicResult(validate(context, current(), 1000000, { cw: 0x037f, work: retired }));
      const noChurn = { admissions: context.cache.admissions, evictions: context.cache.evictions, promotions: context.cache.promotions };
      check(noChurn.evictions === 0 && noChurn.promotions === 0, "bounded proof cache does not churn");
      context.engine.ok("close"); context = null; scheduler.close();
      post("stopped", { publication, noChurn, engineClosedAfterLoop: true });
    }
  } catch (error) { post("error", { message: error.stack ?? error.message }); }
};
