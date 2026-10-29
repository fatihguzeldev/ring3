import { BrowserCapture } from "../observability.js";
import { createHttpTraceOutput } from "../../../observability/typescript/dist/output/client.js";

const element = (id) => document.getElementById(id);
const result = element("result"),
  startButton = element("start"),
  stopButton = element("stop"),
  pingButton = element("ping");
const sourceUrls = [
  "/browser/observability/throughput.html",
  "/browser/observability/throughput.js",
  "/browser/observability/worker.js",
  "/browser/observability.js",
  "/runtime/observability.js",
  "/runtime/session/finalization.js",
  "/runtime/engine.js",
  "/runtime/cache.js",
  "/runtime/scheduler/index.js",
  "/observability/typescript/dist/capture/capture.js",
  "/observability/typescript/dist/capture/buffer.js",
];
let current;
function render(value) {
  result.textContent = JSON.stringify(value, null, 2);
}
async function sourceContext() {
  const sources = await Promise.all(
    sourceUrls.map(async (url) => {
      const response = await fetch(url, { cache: "no-store" });
      if (!response.ok) throw new Error(`proof source ${url}: HTTP${response.status}`);
      const bytes = await response.arrayBuffer(),
        digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
      return {
        url,
        bytes: bytes.byteLength,
        sha256: [...digest].map((value) => value.toString(16).padStart(2, "0")).join(""),
      };
    }),
  );
  return {
    url: location.href,
    userAgent: navigator.userAgent,
    hardwareConcurrency: navigator.hardwareConcurrency,
    crossOriginIsolated,
    timeOrigin: performance.timeOrigin,
    sources,
    sourceCount: sources.length,
    scriptCount: sources.filter((source) => source.url.endsWith(".js")).length,
    completeModuleGraph: false,
    semantics:
      "authored compilation/cache operations; no game, no guest instruction execution; listed source snapshots are not a complete import graph; HTTP latency measures response headers, checkpoint.ns is inclusive awaited export latency",
  };
}
function ping() {
  if (!current?.worker || current.finishing) return;
  current.worker.postMessage({ type: "ping", id: current.sentPings++, sentAt: performance.now() });
}
async function finish(run, workerResult) {
  if (run.finishing) return;
  run.finishing = true;
  clearInterval(run.timer);
  stopButton.disabled = pingButton.disabled = true;
  let capture;
  try {
    capture = await run.capture?.finish(workerResult.outcome ?? "failed", workerResult.error);
  } catch (error) {
    run.error = error.message;
  }
  run.worker?.terminate();
  const final = {
    phase: "finished",
    context: run.context,
    worker: workerResult,
    browserCapture: capture ?? null,
    http: run.http,
    pings: {
      sent: run.sentPings,
      acknowledged: run.acknowledgedPings,
      maxRttMs: run.maxPingRttMs,
      outsideWorkLoop: run.outsideWorkLoopPings,
      byPhase: run.pingsByPhase,
    },
    stopAckMs: run.stopAckMs,
    stopToFinishMs: run.stopSentAt === undefined ? undefined : performance.now() - run.stopSentAt,
    trace: run.trace,
    output: run.output?.completion,
    error: run.error,
  };
  render(final);
  window.proofResult = final;
  if (current === run) current = null;
  startButton.disabled = false;
}

startButton.onclick = async () => {
  if (current) return;
  startButton.disabled = true;
  const run = {
    sentPings: 0,
    acknowledgedPings: 0,
    maxPingRttMs: 0,
    outsideWorkLoopPings: 0,
    pingsByPhase: {},
    http: {
      requests: 0,
      bytes: 0,
      active: 0,
      maximumActive: 0,
      elapsedMs: 0,
      maximumMs: 0,
      failed: 0,
      routes: {},
    },
  };
  current = run;
  window.proofResult = null;
  render({ phase: "preparing" });
  try {
    run.context = await sourceContext();
    const level = element("level").value,
      seconds = Number(element("seconds").value);
    run.context.level = level;
    run.context.seconds = seconds;
    if (level !== "off") {
      run.output = await createHttpTraceOutput({
        metadata: { fixture: "authored browser observability throughput", ...run.context },
        fetch: async (url, options) => {
          const started = performance.now(),
            http = run.http,
            route = String(url).split("/").at(-1);
          http.requests++;
          http.active++;
          http.maximumActive = Math.max(http.maximumActive, http.active);
          http.bytes +=
            options.body instanceof Uint8Array
              ? options.body.byteLength
              : new TextEncoder().encode(String(options.body ?? "")).length;
          http.routes[route] = (http.routes[route] ?? 0) + 1;
          try {
            const response = await fetch(url, options);
            if (!response.ok) http.failed++;
            return response;
          } catch (error) {
            http.failed++;
            throw error;
          } finally {
            const elapsed = performance.now() - started;
            http.elapsedMs += elapsed;
            http.maximumMs = Math.max(http.maximumMs, elapsed);
            http.active--;
          }
        },
      });
      run.capture = new BrowserCapture({ level }, run.output, (observation) => {
        run.trace = observation;
      });
    }
    const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
    run.worker = worker;
    run.capture?.workerStarted();
    worker.onerror = (event) => void finish(run, { outcome: "failed", error: event.message });
    worker.onmessage = ({ data }) => {
      if (run.capture?.receive(data, (ack) => worker.postMessage(ack))) return;
      if (data.type === "proof-result") {
        void finish(run, data);
        return;
      }
      if (data.type === "proof-pong") {
        run.acknowledgedPings++;
        run.maxPingRttMs = Math.max(run.maxPingRttMs, performance.now() - data.sentAt);
        if (!data.duringLoop) run.outsideWorkLoopPings++;
        run.pingsByPhase[data.phase] = (run.pingsByPhase[data.phase] ?? 0) + 1;
        return;
      }
      if (data.type === "proof-stop-received") {
        run.stopAckMs = performance.now() - data.sentAt;
        return;
      }
      render({
        phase: data.type,
        context: run.context,
        worker: data,
        http: run.http,
        pings: {
          sent: run.sentPings,
          acknowledged: run.acknowledgedPings,
          maxRttMs: run.maxPingRttMs,
        },
        trace: run.trace,
      });
    };
    worker.postMessage({ type: "start", level, seconds });
    run.timer = setInterval(ping, 250);
    stopButton.disabled = pingButton.disabled = false;
  } catch (error) {
    await finish(run, { outcome: "failed", error: error.message });
  }
};
pingButton.onclick = ping;
stopButton.onclick = () => {
  if (!current?.worker) return;
  current.stopSentAt ??= performance.now();
  current.worker.postMessage({ type: "stop", sentAt: current.stopSentAt });
  stopButton.disabled = true;
};
