const capacity = 8192;
const counters = () => ({count: 0, work: 0, retired: 0, regions: 0, milliseconds: 0, maximumMs: 0});
const elapsed = value => ({elapsedNs: Math.round(value.milliseconds * 1e6), maximumNs: Math.round(value.maximumMs * 1e6)});
const summary = value => ({count: value.count, work: value.work, retired: value.retired, regions: value.regions, ...elapsed(value)});
const order = (a, b) => b.milliseconds - a.milliseconds || b.count - a.count
  || (a.kind === b.kind ? a.pc - b.pc : a.kind < b.kind ? -1 : 1);

function add(value, run, milliseconds) {
  value.count++; value.work += run.work; value.retired += run.retired; value.regions += run.regions;
  value.milliseconds += milliseconds;
  value.maximumMs = Math.max(value.maximumMs, milliseconds);
}

function clone(snapshot) {
  return {...snapshot, totals: {...snapshot.totals}, failed: {...snapshot.failed},
    overflow: {...snapshot.overflow}, top32: snapshot.top32.map(row => ({...row}))};
}

export class ExecutionAttribution {
  #entries = new Map();
  #finished = null;

  constructor() {
    this.totals = counters();
    this.overflow = counters();
    this.failed = {count: 0, milliseconds: 0, maximumMs: 0};
  }
  record(run, pc, selected, milliseconds) {
    if (run === undefined) {
      this.failed.count++; this.failed.milliseconds += milliseconds;
      this.failed.maximumMs = Math.max(this.failed.maximumMs, milliseconds);
      return;
    }
    add(this.totals, run, milliseconds);
    pc >>>= 0;
    const key = pc * 2 + Number(run.kind === "compiled");
    let row = this.#entries.get(key);
    if (!row) {
      if (this.#entries.size === capacity) {
        add(this.overflow, run, milliseconds);
        return;
      }
      row = {kind: run.kind, pc, firstSelectedRoot: selected?.pc ?? null, firstSelectedId: selected?.id ?? null,
        lastSelectedRoot: null, lastSelectedId: null, ...counters()};
      this.#entries.set(key, row);
    }
    row.lastSelectedRoot = selected?.pc ?? null;
    row.lastSelectedId = selected?.id ?? null;
    add(row, run, milliseconds);
  }
  snapshot() {
    if (this.#finished) return clone(this.#finished);
    return {
      semantics: "successful code.execute outer invocation envelopes by tier and actual starting guest PC; includes chained regions and helpers; inclusive host elapsed time, not inner-PC hotspots or CPU time; failed attempts separate; top32 included in totals",
      capacity, distinctEntries: this.#entries.size,
      totals: summary(this.totals), failed: {count: this.failed.count, ...elapsed(this.failed)},
      overflow: summary(this.overflow),
      top32: [...this.#entries.values()].sort(order).slice(0, 32).map(row => ({
        kind: row.kind, pc: row.pc, firstSelectedRoot: row.firstSelectedRoot,
        firstSelectedId: row.firstSelectedId?.toString() ?? null, lastSelectedRoot: row.lastSelectedRoot,
        lastSelectedId: row.lastSelectedId?.toString() ?? null, ...summary(row),
      })),
    };
  }
  finish() {
    if (this.#finished) return;
    this.#finished = this.snapshot();
    this.#entries.clear();
  }
}
