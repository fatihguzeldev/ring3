const output = document.querySelector("pre"), button = document.querySelector("#run"), save = document.querySelector("#save");
const record = { environment: { userAgent: navigator.userAgent, crossOriginIsolated, url: location.href }, events: [] };
let worker, token;
const check = (value, message) => { if (!value) throw new Error(message); };
const backlog = [], pending = [];
function receive(message) {
  record.events.push({ receivedAtMs: performance.now(), message });
  output.textContent = JSON.stringify({ state: message.type, events: record.events.length, retired: message.retired ?? message.publication?.retired }, null, 2);
  if (message.type === "error") { for (const waiter of pending.splice(0)) waiter.reject(new Error(message.message)); return; }
  const index = pending.findIndex(waiter => waiter.type === message.type && waiter.test(message));
  if (index >= 0) pending.splice(index, 1)[0].resolve(message); else backlog.push(message);
}
function next(type, test = () => true) {
  const found = backlog.findIndex(message => message.type === type && test(message));
  if (found >= 0) return Promise.resolve(backlog.splice(found, 1)[0]);
  return new Promise((resolve, reject) => pending.push({ type, test, resolve, reject }));
}
const send = (type, extra = {}) => worker.postMessage({ type, token, ...extra });
button.onclick = async () => {
  button.disabled = true;
  try {
    const response = await fetch("./browser-config.json"); check(response.ok, "stage config exists"); const config = await response.json();
    token = crypto.randomUUID(); record.config = config; record.token = token;
    worker = new Worker(new URL("./browser-worker.mjs", import.meta.url), { type: "module" });
    worker.onmessage = ({ data }) => receive(data); worker.onerror = event => receive({ type: "error", message: event.message });
    const artifacts = ["baseline", "candidate"].map(label => ({ label, url: new URL(`./${label}.wasm`, location.href).href, sha256: config[label].sha256 }));
    send("prepare", { artifacts }); await next("ready");
    send("run"); const first = await next("progress");
    const pingAt = performance.now(); send("ping", { requestId: 1 }); const pong = await next("pong", message => message.requestId === 1);
    check(pong.retired >= first.retired, "Worker receives a real main-thread ping after bounded execution and task yield");
    record.pingRoundTripMs = performance.now() - pingAt;
    send("pause"); const paused = await next("paused");
    send("probe-paused"); const probe = await next("paused-probe");
    check(probe.retired === paused.publication.retired, "pause freezes guest retirement while Worker still responds");
    send("resume"); const resumed = await next("resumed");
    await next("progress", value => value.retired > resumed.retired);
    send("stop"); const stopped = await next("stopped");
    check(stopped.engineClosedAfterLoop && stopped.publication.retired > paused.publication.retired, "resume progresses and stop joins publication before close");
    send("ping", { requestId: 2 }); await next("ignored");
    record.status = "PASS"; record.completedAt = new Date().toISOString();
    record.claim = "Authored real-browser Worker execution/publication/control seam using actual Engine, CodeCache and TaskScheduler; baseline/candidate SHA-pinned full-state/RAM/receipt parity and literal workload oracles. Actual game Session, Windows callbacks, GPU/audio and desktop-like playability remain separate evidence.";
    output.textContent = JSON.stringify(record, null, 2); save.disabled = false;
  } catch (error) { record.status = "FAIL"; record.error = error.stack ?? error.message; output.textContent = JSON.stringify(record, null, 2); save.disabled = false; }
  finally { worker?.terminate(); }
};
save.onclick = () => {
  const url = URL.createObjectURL(new Blob([JSON.stringify(record, null, 2)], { type: "application/json" }));
  const link = document.createElement("a"); link.href = url; link.download = "cpu-execution-browser-proof.json"; link.click(); URL.revokeObjectURL(url);
};
