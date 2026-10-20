import { categories } from "../../../observability/typescript/dist/index.js";

export class PhaseMeasurements {
  constructor(observation) {
    this.observer = observation.observer;
    this.detailed = observation.capture.observer(categories.hostWorker, "detailed");
    this.phases = new Map();
    this.observer.event("measurement.schema", {
      unit: "ns",
      semantics: "cumulative inclusive elapsed time; not exclusive CPU time",
      clock: "performance.now; nanosecond values preserve its actual clock precision",
    });
  }
  add(name, start) {
    const elapsed = performance.now() - start;
    const phase = this.phases.get(name) ?? { milliseconds: 0, maximumMs: 0, count: 0 };
    phase.milliseconds += elapsed;
    phase.maximumMs = Math.max(phase.maximumMs, elapsed);
    phase.count++;
    this.phases.set(name, phase);
  }
  run(name, operation) {
    const start = performance.now();
    try {
      return this.detailed.enabled ? this.detailed.scope(name, {}, operation) : operation();
    } finally {
      this.add(name, start);
    }
  }
  async runAsync(name, operation) {
    const start = performance.now();
    try {
      return await (this.detailed.enabled
        ? this.detailed.scopeAsync(name, {}, operation)
        : operation());
    } finally {
      this.add(name, start);
    }
  }
  report() {
    for (const [name, phase] of this.phases) {
      this.observer.counter(`${name}.ns`, Math.round(phase.milliseconds * 1e6));
      this.observer.counter(`${name}.count`, phase.count);
      this.observer.counter(`${name}.max.ns`, Math.round(phase.maximumMs * 1e6));
    }
  }
  snapshot() {
    return Object.fromEntries(
      [...this.phases].map(([name, phase]) => [
        name,
        {
          elapsedNs: Math.round(phase.milliseconds * 1e6),
          maximumNs: Math.round(phase.maximumMs * 1e6),
          count: phase.count,
        },
      ]),
    );
  }
}
