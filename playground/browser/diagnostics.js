const serialize = (event) => JSON.stringify(event, (_key, value) => typeof value === "bigint" ? String(value) : value);

export function createDiagnostics(write, { fetch: fetcher = globalThis.fetch, timeoutMs = 5000 } = {}) {
  const queue = [];
  let pending = false;
  async function send() {
    if (pending || queue.length === 0) return;
    pending = true;
    const body = queue.shift(), controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), timeoutMs);
    try {
      await fetcher("/api/diagnostics", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body,
        signal: controller.signal,
      });
    } catch {} finally {
      clearTimeout(timeout);
      pending = false;
      void send();
    }
  }
  function report(event) {
    // keep recent errors/status when the local diagnostics server is slow.
    if (queue.length === 16) queue.shift();
    queue.push(serialize(event));
    void send();
  }
  return {
    report,
    log(message, detail) {
      let info = message;
      if (typeof message === "string" && message.startsWith("{")) {
        try { info = JSON.parse(message); } catch {}
      }
      if (info?.type === "present") return;
      write(message, detail);
      report({ type: "diagnostic", message, detail });
    },
  };
}
