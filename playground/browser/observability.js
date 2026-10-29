import {
  createCapture,
  categories,
  resolveCategorySelection,
} from "../../observability/typescript/dist/index.js";
import { createHttpTraceOutput } from "../../observability/typescript/dist/output/index.js";

export class BrowserCapture {
  static async start(options, onUpdate) {
    if (!options?.level || options.level === "off") return null;
    const output = await createHttpTraceOutput({
      metadata: {
        level: options.level,
        ...(options.categories === undefined ? {} : { categories: options.categories }),
        executable: options.executable ?? "",
        host: "browser",
      },
    });
    try {
      return new BrowserCapture(options, output, onUpdate);
    } catch (error) {
      await output.abort("capture initialization failed").catch(() => {});
      throw error;
    }
  }
  constructor(options, output, onUpdate) {
    this.output = output;
    this.onUpdate = onUpdate;
    this.workerPending = false;
    this.workerSequence = 0;
    this.capture = createCapture({
      level: options.level,
      categories: resolveCategorySelection(options.categories),
      output,
      producerId: 1,
    });
    this.observer = this.capture.observer(categories.hostBrowser);
    this.timer = setInterval(() => this.flush(), 1000);
    this.publish();
  }
  publish(result) {
    const status = result ?? this.capture.status();
    const suffix = this.output.completion?.complete ? "trace.pftrace" : "trace.pftrace.partial";
    this.onUpdate({
      id: this.output.id,
      path: `target/observability/${this.output.id}/${suffix}`,
      ...status,
      engine: this.workerEngine ?? status.engine,
    });
  }
  async flush() {
    try {
      await this.capture.flush();
      this.publish();
    } catch (error) {
      this.capture.markIncomplete(error.message);
      this.publish();
    }
  }
  workerStarted() {
    this.workerPending = true;
    this.workerSequence = 0;
  }
  receive(message, acknowledge = () => {}) {
    if (message.type === "observability-error") {
      this.capture.markIncomplete(`worker: ${message.error}`);
      this.publish();
      return true;
    }
    if (message.type === "observability-engine") {
      this.workerEngine = message.engine;
      this.observer.event("engine.instrumentation", {
        engine: message.engine,
        sha256: message.sha256,
      });
      this.publish();
      return true;
    }
    if (message.type === "observability-data") {
      void this.receiveBytes(message, acknowledge);
      return true;
    }
    if (message.type === "observability-result") {
      this.workerPending = false;
      this.workerEngine = message.result.engine;
      if (!message.result.complete)
        this.capture.markIncomplete(
          `worker: ${message.result.sinkError ?? message.result.recordingError ?? "incomplete capture"}`,
        );
      this.observer.event("worker.finished", {
        complete: message.result.complete,
        engine: message.result.engine,
        droppedEvents: message.result.droppedEvents,
      });
      this.resolveWorker?.();
      return true;
    }
    return false;
  }
  async receiveBytes(message, acknowledge) {
    let error;
    let accepted = false;
    try {
      if (this.receiving) throw new Error("worker trace write is already pending");
      if (!Number.isSafeInteger(message.sequence) || message.sequence !== this.workerSequence)
        throw new Error("unexpected worker trace sequence");
      if (!(message.bytes instanceof ArrayBuffer)) throw new Error("invalid worker trace bytes");
      this.receiving = true;
      try {
        accepted = await this.capture.ingestAsync(new Uint8Array(message.bytes));
        const status = accepted ? await this.capture.flush() : this.capture.status();
        error = status.sinkError ?? status.recordingError;
        if (error) accepted = false;
        if (accepted) this.workerSequence++;
        else error ??= "browser rejected worker trace bytes";
      } finally {
        this.receiving = false;
      }
    } catch (failure) {
      accepted = false;
      error = failure instanceof Error ? failure.message : "worker trace ingestion failed";
    }
    if (!accepted) this.capture.markIncomplete(error);
    try {
      acknowledge({
        type: "observability-ack",
        sequence: message.sequence,
        accepted,
        ...(accepted ? {} : { error }),
      });
    } catch {
      this.capture.markIncomplete("worker trace acknowledgement failed");
    }
  }
  finish(outcome, error) {
    if (this.finishing) return this.finishing;
    clearInterval(this.timer);
    this.finishing = (async () => {
      if (this.workerPending)
        await new Promise((resolve) => {
          const timeout = setTimeout(() => {
            this.capture.markIncomplete("worker capture did not finish");
            resolve();
          }, 3000);
          this.resolveWorker = () => {
            clearTimeout(timeout);
            resolve();
          };
        });
      const result = await this.capture.finish(outcome, error);
      this.publish(result);
      return result;
    })();
    return this.finishing;
  }
}
