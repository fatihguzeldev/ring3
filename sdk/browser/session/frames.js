export class FrameMeasurements {
  constructor(observer) {
    this.observer = observer;
    this.frames = 0;
    this.last = null;
    this.intervals = [];
  }
  present(now = performance.now()) {
    this.frames++;
    if (this.last !== null) {
      this.intervals.push(now - this.last);
      if (this.intervals.length > 240) this.intervals.shift();
    }
    this.last = now;
  }
  snapshot(now = performance.now()) {
    const sorted = this.intervals.toSorted((a, b) => a - b);
    const mean = sorted.length ? sorted.reduce((sum, value) => sum + value, 0) / sorted.length : null;
    const percentile = (p) => sorted.length ? sorted[Math.ceil(sorted.length * p) - 1] : null;
    const result = {
      semantics: "GPU-completed Present intervals; not compositor refresh or input-to-photon latency",
      frames: this.frames,
      samples: sorted.length,
      recentCapacity: 240,
      meanIntervalMs: mean,
      p50IntervalMs: percentile(0.5),
      p95IntervalMs: percentile(0.95),
      maximumIntervalMs: sorted.at(-1) ?? null,
      sinceLastPresentMs: this.last === null ? null : now - this.last,
      intervalFps: mean > 0 ? 1000 / mean : null,
    };
    this.observer?.counter("present.count", this.frames);
    for (const [name, key] of [["mean-interval", "meanIntervalMs"], ["p95-interval", "p95IntervalMs"], ["maximum-interval", "maximumIntervalMs"], ["since-last", "sinceLastPresentMs"]])
      if (result[key] !== null) this.observer?.counter(`present.${name}.ns`, Math.round(result[key] * 1e6));
    return result;
  }
}
