export class TaskScheduler {
  constructor(signal) {
    this.signal = signal;
    this.onAbort = () => this.close(signal.reason);
    if (signal?.aborted) this.close(signal.reason);
    else signal?.addEventListener("abort", this.onAbort, { once: true });
  }
  yield() {
    return this.schedule("task", () => {
      if (!this.channel) {
        this.channel = new MessageChannel();
        this.channel.port1.onmessage = () => {
          if (this.pending?.kind === "task") this.settle();
        };
      }
      this.channel.port2.postMessage(null);
    });
  }
  wait(milliseconds) {
    if (!Number.isFinite(milliseconds) || milliseconds < 0)
      return Promise.reject(new RangeError("invalid task wait"));
    if (milliseconds === 0) return this.yield();
    return this.schedule("timer", () => {
      const timer = setTimeout(() => this.settle(), milliseconds);
      this.pending.cancel = () => clearTimeout(timer);
    });
  }
  schedule(kind, start) {
    if (this.closed) return Promise.reject(this.reason);
    if (this.pending) return Promise.reject(new Error("task already pending"));
    return new Promise((resolve, reject) => {
      this.pending = { kind, resolve, reject };
      try {
        start();
      } catch (error) {
        this.settle(error);
      }
    });
  }
  settle(error) {
    const pending = this.pending;
    if (!pending) return;
    this.pending = null;
    pending.cancel?.();
    if (error === undefined) pending.resolve();
    else pending.reject(error);
  }
  close(reason = new DOMException("task scheduler closed", "AbortError")) {
    if (this.closed) return;
    this.closed = true;
    this.reason = reason;
    this.signal?.removeEventListener("abort", this.onAbort);
    this.channel?.port1.close();
    this.channel?.port2.close();
    this.settle(reason);
  }
}
