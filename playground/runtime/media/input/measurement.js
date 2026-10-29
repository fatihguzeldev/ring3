export class InputMeasurements {
  constructor() {
    this.enqueuedEvents = 0;
    this.submittedEvents = 0;
    this.submittedBatches = 0;
    this.pointerSubmissions = 0;
    this.history = [];
    this.pending = [];
    this.pendingHead = 0;
    this.transport = { count: 0, sumNs: 0, maximumNs: 0 };
    this.queue = { count: 0, sumNs: 0, maximumNs: 0 };
  }
  enqueue(count, sentAtMs) {
    this.enqueuedEvents += count;
    if (!count) return;
    const received = performance.now();
    this.pending.push({ remaining: count, received });
    if (Number.isFinite(sentAtMs))
      this.add(this.transport, performance.timeOrigin + received - sentAtMs);
  }
  add(metric, milliseconds) {
    const ns = Math.round(Math.max(0, milliseconds) * 1e6);
    metric.count++;
    metric.sumNs += ns;
    metric.maximumNs = Math.max(metric.maximumNs, ns);
  }
  submitted(events, sequence, now) {
    this.submittedBatches++;
    const identity = { sequence: sequence.toString(), timestampNs: now.toString() };
    const accepted = performance.now();
    for (const event of events) this.record(event, identity, "submit_windows_input", accepted);
  }
  pointer(x, y, now) {
    this.pointerSubmissions++;
    this.record({ kind: "pointer", x, y }, { timestampNs: now.toString() }, "submit_windows_pointer", performance.now());
  }
  record(event, identity, operation, accepted) {
    const pending = this.pending[this.pendingHead];
    if (pending) {
      this.add(this.queue, accepted - pending.received);
      if (--pending.remaining === 0) this.pendingHead++;
      if (this.pendingHead === this.pending.length) {
        this.pending.length = 0;
        this.pendingHead = 0;
      } else if (this.pendingHead >= 256) {
        this.pending.splice(0, this.pendingHead);
        this.pendingHead = 0;
      }
    }
    const row = { operation, kind: event.kind, ...identity };
    for (const field of ["code", "down", "button", "focused", "captured", "dx", "dy", "wheel", "x", "y"])
      if (event[field] !== undefined) row[field] = event[field];
    this.submittedEvents++;
    this.history.push(row);
    if (this.history.length > 8) this.history.shift();
  }
  snapshot(queuedEvents) {
    return {
      enqueuedEvents: this.enqueuedEvents,
      submittedEvents: this.submittedEvents,
      submittedBatches: this.submittedBatches,
      pointerSubmissions: this.pointerSubmissions,
      queuedEvents,
      latency: {
        semantics: "browser-post to Worker receive, then Worker receive to successful engine submission; not guest consumption",
        transport: { ...this.transport },
        workerQueue: { ...this.queue },
      },
      history: this.history.map((row) => ({ ...row })),
    };
  }
  report(observer) {
    for (const [name, metric] of [["transport", this.transport], ["worker-queue", this.queue]]) {
      observer.counter(`input.${name}.count`, metric.count);
      observer.counter(`input.${name}.ns`, metric.sumNs);
      observer.counter(`input.${name}.max.ns`, metric.maximumNs);
    }
  }
}
