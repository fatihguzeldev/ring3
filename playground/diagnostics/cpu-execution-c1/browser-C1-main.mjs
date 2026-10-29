const output = document.querySelector("pre"), button = document.querySelector("#run"), save = document.querySelector("#save");
const record = { environment: { userAgent: navigator.userAgent, crossOriginIsolated, url: location.href }, events: [], controls: [] };
const check = (value, message) => { if (!value) throw new Error(message); };
let worker, token, controlToken, failure;
const backlog = [], pending = [];
function receive(message) {
  record.events.push({ receivedAtMs: performance.now(), message });
  output.textContent = JSON.stringify({ state: message.type, mode: message.mode, events: record.events.length,
    retired: message.retired ?? message.publication?.retired }, null, 2);
  if (message.type === "error") {
    failure = new Error(message.message);
    for (const waiter of pending.splice(0)) { clearTimeout(waiter.timeout); waiter.reject(failure); }
    return;
  }
  const index = pending.findIndex(waiter => waiter.type === message.type && waiter.test(message));
  if (index < 0) backlog.push(message);
  else { const waiter = pending.splice(index, 1)[0]; clearTimeout(waiter.timeout); waiter.resolve(message); }
}
function next(type, test = () => true) {
  if (failure) return Promise.reject(failure);
  const index = backlog.findIndex(message => message.type === type && test(message));
  if (index >= 0) return Promise.resolve(backlog.splice(index, 1)[0]);
  return new Promise((resolve, reject) => {
    const waiter = { type, test, resolve, reject };
    waiter.timeout = setTimeout(() => {
      const index = pending.indexOf(waiter); if (index >= 0) pending.splice(index, 1);
      reject(new Error(`bounded browser proof timeout waiting for ${type}`));
    }, 180000);
    pending.push(waiter);
  });
}
const send = (type, extra = {}) => worker.postMessage({ type, token, controlToken, ...extra });
const arm = message => message.token === token && message.controlToken === controlToken;
button.onclick = async () => {
  button.disabled = true;
  try {
    const response = await fetch("./browser-C1-config.json", { cache: "no-store" }); check(response.ok, "exclusive C1 stage config");
    const config = await response.json(); record.config = config; token = crypto.randomUUID(); record.token = token;
    worker = new Worker(new URL("./browser-C1-worker.mjs", import.meta.url), { type: "module" });
    worker.onmessage = ({ data }) => receive(data); worker.onerror = event => receive({ type: "error", message: event.message });
    send("prepare", { artifact: { url: new URL("./C1.wasm", location.href).href, sha256: config.artifact.sha256 },
      sources: [...config.assets.map(asset => ({ url: new URL(`./${asset.name}`, location.href).href, sha256: asset.sha256 })), ...config.hostSources] });
    const ready = await next("ready", message => message.token === token); record.correctness = ready.literalCorrectness; record.sourceProof = ready.sourceProof;
    const identities = [];
    for (const mode of ["checked", "owned"]) {
      controlToken = crypto.randomUUID(); identities.push(controlToken); send("prepare-control", { mode });
      const prepared = await next("control-ready", arm); send("run"); const first = await next("progress", arm);
      send("ping", { requestId: `${mode}-live` }); const pong = await next("pong", message => arm(message) && message.requestId === `${mode}-live`);
      check(pong.retired >= first.retired, "Worker receives ping after bounded native execution and task yield");
      if (mode === "owned") {
        send("ping", { controlToken: identities[0], requestId: "stale-checked-arm" });
        await next("ignored", message => message.incomingControlToken === identities[0] && message.requestId === "stale-checked-arm");
      }
      send("pause"); const paused = await next("paused", arm);
      send("ping", { requestId: `${mode}-paused` }); const frozenPong = await next("pong", message => arm(message) && message.requestId === `${mode}-paused`);
      send("probe-paused"); const probe = await next("paused-probe", arm);
      check(frozenPong.retired === paused.publication.retired && JSON.stringify(probe.publication) === JSON.stringify(paused.publication),
        "pause freezes complete published arena/RAM/receipt state while Worker services messages");
      send("resume"); const resumed = await next("resumed", arm);
      await next("progress", message => arm(message) && message.retired > resumed.retired);
      send("stop"); const stopped = await next("stopped", arm);
      check(stopped.engineClosedAfterLoop && stopped.publication.retired > paused.publication.retired, "resume progresses and stop joins before final publication/close");
      send("ping", { requestId: `${mode}-late` });
      await next("ignored", message => message.incomingControlToken === controlToken && message.requestId === `${mode}-late` && message.state === "stopped");
      record.controls.push({ mode, controlToken, prepared, first, pong, paused, probe, resumed, stopped });
    }
    check(JSON.stringify(record.controls[0].prepared.topology.modules) === JSON.stringify(record.controls[1].prepared.topology.modules), "both live controls use byte-identical generated modules");
    record.status = "PASS"; record.completedAt = new Date().toISOString();
    record.claim = "One immutable C1 artifact, checked versus explicitly owned, actual unwrapped Engine/CodeCache and pristine native imports. Exact dyadic mixed/chain/integer literal oracles and full state/RAM/raw receipt parity under PC24/53/64. Both actual Worker arms service ping, freeze publication while paused, resume progress, join before close and ignore late/stale messages through TaskScheduler. Control cutpoints can differ and are independently checked; no throughput, game Session, GPU/audio/Windows lifecycle or playability claim.";
  } catch (error) { record.status = "FAIL"; record.error = error.stack ?? error.message; }
  finally {
    worker?.terminate(); for (const waiter of pending.splice(0)) clearTimeout(waiter.timeout);
    output.textContent = JSON.stringify(record, null, 2); save.disabled = false;
  }
};
save.onclick = () => {
  const url = URL.createObjectURL(new Blob([JSON.stringify(record, null, 2)], { type: "application/json" }));
  const link = document.createElement("a"); link.href = url; link.download = "cpu-execution-browser-C1-proof.json"; link.click(); URL.revokeObjectURL(url);
};
