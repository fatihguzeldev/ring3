import { categories } from "../../../observability/typescript/dist/index.js";

export class PhaseMeasurements {
  constructor(observation = null) {
    this.observer = observation?.observer ?? null;
    this.detailed = observation?.capture.observer(categories.hostWorker, "detailed") ?? null;
    this.phases = new Map();
    this.exclusiveStack = [];
    this.exclusiveRootMilliseconds = 0;
    this.observer?.event("measurement.schema", {
      unit: "ns",
      semantics: "cumulative inclusive elapsed time; not exclusive CPU time",
      execution: "includes calls completed inside v86/Wasm; JavaScript host completions are measured in completion",
      clock: "performance.now; nanosecond values preserve its actual clock precision",
    });
  }
  add(name, start, classification, end = performance.now()) {
    const elapsed = end - start;
    const phase = this.phases.get(name) ?? { milliseconds: 0, maximumMs: 0, count: 0 };
    phase.milliseconds += elapsed;
    phase.maximumMs = Math.max(phase.maximumMs, elapsed);
    phase.count++;
    phase.lastMilliseconds = elapsed;
    if (name === "execution" && (classification === "cold" || classification === "compiled")) {
      const breakdown = phase.breakdown ??= {};
      const part = breakdown[classification] ?? { milliseconds: 0, maximumMs: 0, count: 0 };
      part.milliseconds += elapsed;
      part.maximumMs = Math.max(part.maximumMs, elapsed);
      part.count++;
      breakdown[classification] = part;
    }
    this.phases.set(name, phase);
  }
  lastMilliseconds(name) {
    return this.phases.get(name)?.lastMilliseconds ?? null;
  }
  runExclusive(name, operation) {
    // returned promises are not awaited; this scope measures the synchronous invocation.
    const start = performance.now(), frame = { name, children: 0 };
    this.exclusiveStack.push(frame);
    try {
      return operation();
    } finally {
      const end = performance.now(), elapsed = end - start;
      this.exclusiveStack.pop();
      this.add(name, start, undefined, end);
      const phase = this.phases.get(name);
      phase.selfMilliseconds = (phase.selfMilliseconds ?? 0) + elapsed - frame.children;
      const parent = this.exclusiveStack.at(-1);
      if (parent) parent.children += elapsed;
      else this.exclusiveRootMilliseconds += elapsed;
    }
  }
  run(name, operation, classification) {
    const start = performance.now();
    try {
      return this.detailed?.enabled ? this.detailed.scope(name, {}, operation) : operation();
    } finally {
      this.add(name, start, classification);
    }
  }
  async runAsync(name, operation) {
    const start = performance.now();
    try {
      return await (this.detailed?.enabled
        ? this.detailed.scopeAsync(name, {}, operation)
        : operation());
    } finally {
      this.add(name, start);
    }
  }
  report() {
    if (!this.observer) return;
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
          ...(phase.selfMilliseconds === undefined ? {} : { selfElapsedNs: Math.round(phase.selfMilliseconds * 1e6) }),
          maximumNs: Math.round(phase.maximumMs * 1e6),
          count: phase.count,
          ...(phase.breakdown ? {
            breakdownSemantics: "included in execution; not additive; entry tier, compiled includes chained cold work",
            breakdown: Object.fromEntries(Object.entries(phase.breakdown).map(([kind, part]) => [
              kind, {
                elapsedNs: Math.round(part.milliseconds * 1e6),
                maximumNs: Math.round(part.maximumMs * 1e6),
                count: part.count,
              },
            ])),
          } : {}),
        },
      ]),
    );
  }
}
