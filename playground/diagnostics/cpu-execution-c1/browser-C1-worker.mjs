import { program } from "./compare-programs.mjs";
import { check, reset, execute, validate, compare, publicResult, hash, hex } from "./compare-core.mjs";
import { ownedContext, ownedTopology, equalTopology } from "./compare-owned-host.mjs";
import { Engine, KEY } from "/runtime/engine.js";
import { CodeCache } from "/runtime/cache.js";
import { TaskScheduler } from "/runtime/scheduler/index.js";

const host = { Engine, KEY, CodeCache }, iterations = 1000000, controlWord = 0x003f;
let artifact, context, scheduler, loop, proofToken, controlToken, mode, state = "empty", retired = 0, chunks = 0, last, rawReceipt;
const post = (type, value = {}) => postMessage({ type, token: proofToken, controlToken, mode, state, ...value });
const untimed = proof => { const { elapsedMs, ...value } = publicResult(proof); return value; };
const snapshot = () => ({ retired, calls: chunks, receipts: last ? [{ ...last, owner: String(last.owner), raw: rawReceipt }] : [],
  last: last ? { ...last, owner: String(last.owner) } : null, reason: last?.reason ?? 1, pc: context.engine.get(48), flags: context.engine.get(52),
  registers: Array.from({ length: 8 }, (_, i) => context.engine.get(16 + 4 * i)) });
const publication = () => untimed(validate(context, snapshot(), iterations, { cw: controlWord, work: retired }));
async function sources(pins) {
  const result = [];
  for (const pin of pins) {
    const response = await fetch(pin.url, { cache: "no-store" }); check(response.ok, `source ${pin.url}`);
    const bytes = await response.arrayBuffer(), sha256 = await hash(bytes); check(sha256 === pin.sha256, `source SHA ${pin.url}`);
    result.push({ url: pin.url, sha256, bytes: bytes.byteLength });
  }
  return result;
}
async function correctness() {
  const rows = [];
  for (const name of ["mixed-call-store", "long-fp-chain", "integer-memory"]) for (const cw of [0x003f, 0x023f, 0x037f]) {
    const p = program(name), contexts = [];
    try {
      for (const selected of ["checked", "owned"]) contexts.push(ownedContext(artifact, p, host, selected));
      const topology = await Promise.all(contexts.map(ownedTopology)); equalTopology(...topology);
      const values = contexts.map(c => { reset(c, 3, { cw }); return validate(c, execute(c), 3, { cw }); });
      compare(...values); const frontiers = [];
      for (const work of [...new Set([0, 1, 2, 3, 4, 8, p.loopCount, p.loopCount + 1])]) {
        const cuts = contexts.map(c => { reset(c, 1, { cw }); return validate(c, execute(c, { work }), 1, { cw, work }); });
        compare(...cuts); frontiers.push({ work, values: cuts.map(untimed) });
      }
      rows.push({ name, cw, topology, values: values.map(untimed), frontiers });
    } finally { for (const c of contexts) c.engine.ok("close"); }
  }
  return rows;
}
async function compute() {
  try {
    while (state === "running" || state === "paused") {
      if (state === "paused") { await scheduler.wait(4); continue; }
      const unit = context.owners.get(context.engine.get(48)); check(unit, "control resumes an installed authored frontier");
      context.cache.budget = 512; last = context.cache.execute(unit); rawReceipt = hex(context.engine.take(64));
      check(last.reason === 1 && last.retired === 512, "finite bounded control prefix stops on the exact guest budget");
      retired += last.retired; chunks++;
      if (chunks === 1 || chunks % 16 === 0) post("progress", snapshot());
      await scheduler.wait(0);
    }
  } catch (error) { state = "failed"; post("error", { message: error.stack ?? error.message }); }
}
self.onmessage = async ({ data }) => {
  try {
    if (data.type === "prepare") {
      check(state === "empty", "prepare once per Worker"); proofToken = data.token; state = "preparing";
      const sourceProof = await sources(data.sources);
      const response = await fetch(data.artifact.url, { cache: "no-store" }); check(response.ok, "explicit immutable C1 artifact URL");
      const bytes = await response.arrayBuffer(), sha256 = await hash(bytes); check(sha256 === data.artifact.sha256, "C1 artifact SHA pin");
      artifact = { ...data.artifact, sha256, bytes: bytes.byteLength, module: await WebAssembly.compile(bytes) };
      const literalCorrectness = await correctness(); state = "ready";
      post("ready", { artifact: { url: artifact.url, sha256, bytes: artifact.bytes }, sourceProof, literalCorrectness,
        boundary: "same immutable C1 artifact, pristine direct native helper imports and actual CodeCache/TaskScheduler; authored Worker controls, not game Session or GPU/audio/Windows lifecycle" });
      return;
    }
    if (data.token !== proofToken) { post("ignored", { incomingToken: data.token, incomingControlToken: data.controlToken, requestId: data.requestId }); return; }
    if (data.type === "prepare-control") {
      check(["ready", "stopped"].includes(state) && !context, "previous control engine must be closed before a new arm");
      check(["checked", "owned"].includes(data.mode) && data.controlToken && data.controlToken !== controlToken, "fresh explicit control arm identity");
      state = "preparing-control"; mode = data.mode; controlToken = data.controlToken;
      context = ownedContext(artifact, program("mixed-call-store"), host, mode);
      const topology = await ownedTopology(context); reset(context, iterations, { cw: controlWord });
      scheduler = new TaskScheduler(); loop = null; retired = 0; chunks = 0; last = null; rawReceipt = null; state = "control-ready";
      post("control-ready", { topology, iterations, cw: controlWord, budget: 512 }); return;
    }
    if (!context || data.controlToken !== controlToken || ["stopping", "stopped", "failed"].includes(state)) {
      post("ignored", { incomingControlToken: data.controlToken, requestId: data.requestId }); return;
    }
    if (data.type === "run") { check(state === "control-ready" && !loop, "run each arm once"); state = "running"; loop = compute(); }
    else if (data.type === "ping") post("pong", { requestId: data.requestId, ...snapshot() });
    else if (data.type === "pause") { check(state === "running", "pause only a running arm"); state = "paused"; post("paused", { publication: publication() }); }
    else if (data.type === "probe-paused") { check(state === "paused", "probe a paused arm"); post("paused-probe", { publication: publication() }); }
    else if (data.type === "resume") { check(state === "paused", "resume a paused arm"); state = "running"; post("resumed", snapshot()); }
    else if (data.type === "stop") {
      check(["running", "paused"].includes(state), "stop a live arm"); state = "stopping"; await loop;
      check(state === "stopping", "computation joined without a terminal error");
      const published = publication(), noChurn = { admissions: context.cache.admissions, evictions: context.cache.evictions, promotions: context.cache.promotions };
      check(noChurn.evictions === 0 && noChurn.promotions === 0, "control span has no eviction/promotion");
      context.engine.ok("close"); context = null; scheduler.close(); scheduler = null; loop = null; state = "stopped";
      post("stopped", { publication: published, noChurn, engineClosedAfterLoop: true });
    } else throw new Error(`unknown control ${data.type}`);
  } catch (error) { state = "failed"; post("error", { message: error.stack ?? error.message }); }
};
