import { categories, createCapture } from "/observability/typescript/dist/index.js";
import { createHttpTraceOutput } from "/observability/typescript/dist/output/index.js";

const button = document.getElementById("record"),
  result = document.getElementById("result");
button.onclick = async () => {
  button.disabled = true;
  let capture, worker;
  try {
    const level = document.getElementById("level").value;
    if (level === "off") {
      result.textContent = "recording off: no output created";
      return;
    }
    const output = await createHttpTraceOutput({
      metadata: { fixture: "browser CPU compile", level },
    });
    capture = createCapture({ level, producerId: 1, output });
    const observer = capture.observer(categories.hostBrowser);
    observer.event("fixture.started");
    worker = new Worker("./worker.js", { type: "module" });
    const completed = await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("capture fixture timed out")), 15000);
      worker.onerror = (event) => {
        clearTimeout(timeout);
        reject(new Error(event.message));
      };
      worker.onmessage = ({ data }) => {
        if (data.type === "bytes") capture.ingest(new Uint8Array(data.bytes));
        if (data.type === "complete") {
          clearTimeout(timeout);
          resolve(data);
        }
      };
      worker.postMessage({ level });
    });
    if (!completed.result.complete)
      capture.markIncomplete(completed.error ?? "worker capture incomplete");
    const status = await capture.finish(completed.error ? "failed" : "complete");
    result.textContent = JSON.stringify(
      {
        id: output.id,
        path: `target/observability/${output.id}/${output.completion?.complete ? "trace.pftrace" : "trace.pftrace.partial"}`,
        worker: completed,
        ...status,
      },
      null,
      2,
    );
  } catch (error) {
    capture?.markIncomplete(error.message);
    await capture?.finish("failed", error);
    result.textContent = `error: ${error.message}`;
  } finally {
    worker?.terminate();
    button.disabled = false;
  }
};
