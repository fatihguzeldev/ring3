import { KEY, pair, view, ownedExecutionBindings } from "./engine.js";
import { appendResidentModuleName } from "./observability/module-name.js";
import { DEFAULT_EXECUTION, MAX_EXECUTION_UNITS } from "./configuration.js";
import { CacheDiagnostics } from "./cache/diagnostics.js";
const X87_CONTROL_OFFSET = 4236 + 16;
export class CodeCache {
  constructor(engine, { slots = DEFAULT_EXECUTION.units, budget = 50000, receiptMode = "checked" } = {}) {
    if (!Number.isInteger(slots) || slots < 1 || slots > MAX_EXECUTION_UNITS)
      throw new Error("invalid execution table capacity");
    if (!Number.isInteger(budget) || budget < 1 || budget > 0xffffffff)
      throw new Error("invalid execution budget");
    this.engine = engine;
    this.native = ownedExecutionBindings(engine);
    this.receiptMode = "checked";
    this.receiptOpen = false;
    this.setReceiptMode(receiptMode);
    this.ownedReceipts = 0;
    this.compiledReceipts = 0;
    this.checkedReceipts = 0;
    this.diagnostics = engine.cacheMeasurementsEnabled ? new CacheDiagnostics() : null;
    this.budget = budget;
    this.slots = new Map();
    this.admissions = 0;
    this.evictions = 0;
    this.runs = 0;
    this.retired = 0;
    this.dispatches = 0;
    this.directRuns = 0;
    this.restoredSeals = 0;
    this.promotions = 0;
    this.promotionDeferrals = 0;
    this.generatedModuleBytes = 0;
    this.profileMetadataBytes = 0;
    this.retainedModuleBytes = 0;
    this.table = new WebAssembly.Table({ element: "anyfunc", initial: slots, maximum: slots });
    engine.ok("dispatcher_module", ...KEY);
    const descriptor = view(engine.take(32));
    if (descriptor.getUint32(0, true) !== 0x50443352 || descriptor.getUint32(8, true) !== 32)
      throw new Error("invalid dispatcher descriptor");
    const pointer = descriptor.getUint32(24, true),
      length = descriptor.getUint32(28, true);
    this.generatedModuleBytes += length;
    const module = new WebAssembly.Module(
      new Uint8Array(engine.memory.buffer).slice(pointer, pointer + length),
    );
    const native = this.native ?? engine.api;
    if (this.native) this.validateImports(module, true);
    this.dispatcher = new WebAssembly.Instance(module, {
      env: { memory: engine.memory, table: this.table },
      ring3: {
        guard_dispatch_entry: native.guard_dispatch_entry,
        find_installed_resident: native.find_installed_resident,
      },
    }).exports.run;
  }
  setReceiptMode(mode) {
    if (this.receiptOpen) throw new Error("cannot change mode while an execution receipt is pinned");
    if (!["checked", "owned", "compiled"].includes(mode)) throw new Error("invalid execution receipt mode");
    if (mode !== "checked" && (!this.native || ownedExecutionBindings(this.engine, true) !== this.native))
      throw new Error("owned execution was not prepared with native bindings");
    if (mode === "compiled" && ["begin_compiled_execution_receipt", "compiled_execution_usage"].some(name => typeof this.native[name] !== "function"))
      throw new Error("compiled execution is unavailable in this engine");
    this.receiptMode = mode;
  }
  validateImports(module, dispatcher = false) {
    for (const item of WebAssembly.Module.imports(module)) {
      if (item.module === "env" && item.name === "memory" && item.kind === "memory") continue;
      if (dispatcher && item.module === "env" && item.name === "table" && item.kind === "table") continue;
      if (item.module === "ring3" && item.kind === "function" && typeof this.native[item.name] === "function") continue;
      throw new Error(`unreviewed generated import ${item.module}.${item.name}`);
    }
  }
  receipt(operation, ...args) {
    const status = this.native[operation](...args);
    if (status !== 0) throw Object.assign(new Error(`${operation} returned${status}`), { operation, status });
  }
  entry(pc) {
    const host = this.engine;
    for (let attempt = 0; attempt < 256; attempt++) {
      host.ok("prepare_cache_entry", ...KEY, pc);
      const packet = view(host.take(64)),
        kind = packet.getUint32(24, true),
        serial = packet.getBigUint64(16, true),
        id = packet.getBigUint64(32, true),
        slot = packet.getUint32(48, true);
      if (kind === 0) {
        const unit = this.slots.get(slot);
        if (!unit || unit.id !== id || this.table.get(slot) !== unit.generated.run)
          throw new Error("engine cache hit has no matching host instance");
        return unit;
      }
      if (kind === 2) {
        this.evict(packet);
        continue;
      }
      if (kind !== 1) throw new Error(`unknown cache admission${kind}`);
      let unit;
      try {
        host.ok("resident_module", ...pair(id));
        const descriptor = view(host.take(24)),
          pointer = descriptor.getUint32(16, true),
          length = descriptor.getUint32(20, true);
        const bytes = new Uint8Array(host.memory.buffer).slice(pointer, pointer + length);
        this.generatedModuleBytes += length;
        const named = host.generatedModuleNames
          ? appendResidentModuleName(bytes, { unitId: id, pc: packet.getUint32(28, true), tier: packet.getUint32(56, true) })
          : { bytes, addedBytes: 0 };
        this.profileMetadataBytes += named.addedBytes;
        const compile = () => new WebAssembly.Module(named.bytes);
        const module = host.observer?.enabled
            ? host.observer.scope("compile", { unit_id: id, bytes: length }, compile)
            : compile(),
          bindings = {};
        if (this.native) this.validateImports(module);
        const native = this.native ?? host.api;
        for (const item of WebAssembly.Module.imports(module)) {
          if (item.module === "ring3" && typeof native[item.name] === "function") {
            bindings[item.name] = native[item.name];
          } else if (item.module !== "env" || item.name !== "memory")
            throw new Error(`generated import ${item.module}.${item.name}`);
        }
        const instantiate = () =>
          new WebAssembly.Instance(module, { env: { memory: host.memory }, ring3: bindings })
            .exports;
        const generated = host.observer?.enabled
          ? host.observer.scope("instantiate", { unit_id: id }, instantiate)
          : instantiate();
        unit = { id, slot, generated, bytes: length, tier: packet.getUint32(56, true) };
        if (slot >= this.table.length || this.slots.has(slot) || this.table.get(slot) !== null)
          throw new Error("cache install slot is not empty");
        this.table.set(slot, generated.run);
        this.slots.set(slot, unit);
        host.ok("install_cached_entry", ...KEY, ...pair(serial), ...pair(id), slot);
        this.admissions++;
        this.retainedModuleBytes += length;
        if (unit.tier === 1) this.promotions++;
        return unit;
      } catch (error) {
        if (unit && this.slots.get(slot) === unit) this.slots.delete(slot);
        if (unit && slot < this.table.length && this.table.get(slot) === unit.generated.run)
          this.table.set(slot, null);
        try {
          host.ok("cancel_cache_request", ...KEY, ...pair(serial));
        } catch (cleanupError) {
          if (error && typeof error === "object" && Object.isExtensible(error))
            error.cleanupError = cleanupError.message;
        }
        throw error;
      }
    }
    throw new Error("cache admission did not converge");
  }
  evict(packet) {
    const host = this.engine;
    const serial = packet.getBigUint64(16, true),
      victim = packet.getBigUint64(40, true),
      slot = packet.getUint32(52, true),
      old = this.slots.get(slot);
    if (!old || old.id !== victim || this.table.get(slot) !== old.generated.run)
      throw new Error("cache eviction identity mismatch");
    this.table.set(slot, null);
    this.slots.delete(slot);
    this.retainedModuleBytes -= old.bytes;
    host.ok("acknowledge_cache_eviction", ...KEY, ...pair(serial), ...pair(victim), slot);
    this.evictions++;
    this.diagnostics?.evicted(packet.getUint32(56, true), old.tier);
  }
  promote() {
    const host = this.engine;
    try {
      host.ok("prepare_cache_promotion", ...KEY);
    } catch (error) {
      if (error.status !== 12) throw error;
      this.promotionDeferrals++;
      return null;
    }
    const packet = view(host.take(64));
    if (!packet.getUint32(12, true)) return null;
    if (packet.getUint32(24, true) !== 2 || packet.getUint32(56, true) !== 1)
      throw new Error("invalid cache promotion request");
    const pc = packet.getUint32(28, true);
    this.diagnostics?.begin(pc);
    try {
      this.evict(packet);
      const unit = this.entry(pc);
      this.diagnostics?.finish(unit);
      return unit;
    } finally {
      if (this.diagnostics) this.diagnostics.active = null;
    }
  }
  execute(unit, { dispatch = true } = {}) {
    const host = this.engine;
    host.epoch = 0n;
    if (this.receiptOpen) throw new Error("an execution receipt is already pinned");
    this.receiptOpen = true;
    const mode = dispatch ? (this.receiptMode ?? "checked") : "checked";
    const nativeReceipt = mode !== "checked";
    if (nativeReceipt) this.receipt(mode === "compiled" ? "begin_compiled_execution_receipt" : "begin_owned_execution_receipt", ...KEY, this.budget, ...pair(unit.id), 1);
    else host.ok("begin_execution_receipt", ...KEY, this.budget, ...pair(unit.id), dispatch ? 1 : 0);
    const frame = view(host.take(32));
    if (
      frame.getUint32(0, true) !== 0x46453352 ||
      frame.getUint32(4, true) !== 0x10001 ||
      frame.getUint32(8, true) !== 32 ||
      frame.getUint32(12, true) !== 0
    )
      throw new Error("invalid execution frame");
    const token = frame.getBigUint64(16, true);
    if (token === 0n) throw new Error("invalid execution frame token");
    // a failed generated call leaves the frame pinned; the session closes that engine.
    const status = (dispatch ? this.dispatcher : unit.generated.run)(
      host.base,
      host.base + 56,
      this.budget,
      host.base + 96,
    );
    if (status !== 0)
      throw Object.assign(new Error(`generated run returned${status}`), {
        status,
        pc: host.get(48),
        owner: unit.id.toString(),
      });
    const reason = host.get(72),
      detail = host.get(80);
    if (nativeReceipt) this.receipt("finish_execution_receipt", ...KEY, ...pair(token));
    else host.ok("finish_execution_receipt", ...KEY, ...pair(token));
    const receipt = view(host.take(64));
    if (
      receipt.getUint32(0, true) !== 0x52453352 ||
      receipt.getUint32(4, true) !== 0x10001 ||
      receipt.getUint32(8, true) !== 64 ||
      receipt.getUint32(12, true) !== 0 ||
      receipt.getUint32(52, true) !== 0 ||
      receipt.getUint32(56, true) !== 0 ||
      receipt.getUint32(60, true) !== 0 ||
      receipt.getUint32(44, true) !== reason
    )
      throw new Error("invalid execution receipt");
    const epoch = receipt.getBigUint64(16, true),
      owner = receipt.getBigUint64(24, true),
      slot = receipt.getUint32(32, true),
      retired = receipt.getUint32(36, true),
      childRetired = receipt.getUint32(40, true),
      kind = receipt.getUint32(48, true);
    if (retired > this.budget || childRetired > retired)
      throw new Error("invalid execution retirement receipt");
    if (kind === 0) {
      if (
        !dispatch ||
        epoch !== 0n ||
        owner !== 0n ||
        slot !== 0xffffffff ||
        retired !== 0 ||
        childRetired !== 0 ||
        (reason !== 1 && reason !== 2)
      )
        throw new Error("invalid empty execution receipt");
    } else {
      if (kind > 3 || epoch === 0n || owner === 0n)
        throw new Error("invalid child execution receipt");
      const last = this.slots.get(slot);
      if (!last || last.id !== owner || this.table.get(slot) !== last.generated.run)
        throw new Error("execution receipt owner is not installed");
      if (kind >= 2 && childRetired !== retired) this.restoredSeals++;
    }
    host.epoch = epoch;
    this.receiptOpen = false;
    this.runs++;
    if (mode === "compiled") this.compiledReceipts = (this.compiledReceipts ?? 0) + 1;
    else if (mode === "owned") this.ownedReceipts = (this.ownedReceipts ?? 0) + 1;
    else this.checkedReceipts = (this.checkedReceipts ?? 0) + 1;
    this.retired += retired;
    if (dispatch) this.dispatches++;
    else this.directRuns++;
    return { pc: host.get(48), reason, detail, retired, owner: kind === 0 ? unit.id : owner };
  }
  sampleUsage() {
    this.engine.ok("cache_usage");
    const usage = view(this.engine.take(80));
    if (usage.getUint32(0, true) !== 0x55433352 || usage.getUint32(8, true) !== 80)
      throw new Error("invalid engine cache usage descriptor");
    this.engineUsage = {
      residentUnits: usage.getUint32(16, true),
      residentWasmBytes: usage.getUint32(20, true),
      engineInstalledUnits: usage.getUint32(24, true),
      engineCacheHits: Number(usage.getBigUint64(32, true)),
      engineCacheMisses: Number(usage.getBigUint64(40, true)),
      engineCacheEvictions: Number(usage.getBigUint64(48, true)),
      engineCachePromotions: Number(usage.getBigUint64(56, true)),
    };
  }
  sampleCompiledUsage() {
    if (typeof this.native?.compiled_execution_usage !== "function") return null;
    if (this.receiptOpen) throw new Error("cannot sample compiled usage while a receipt is pinned");
    const scratch = this.engine.take(4096);
    try {
      this.receipt("compiled_execution_usage", ...KEY);
      const packet = view(this.engine.take(80));
      if (packet.getUint32(0, true) !== 0x55453352 || packet.getUint32(4, true) !== 0x10001 ||
          packet.getUint32(8, true) !== 80 || packet.getUint32(12, true) !== 0)
        throw new Error("invalid compiled execution usage record");
      const fields = ["seedFullNeedCodeSeals", "strictFullNeedCodeSeals", "lightNeedCodeSeals", "fullMaterializations",
        "eligibleChildren", "strictChildren", "continuedX87StepAttempts", "rejectedPublicTransitions"];
      return Object.fromEntries(fields.map((name, i) => [name, String(packet.getBigUint64(16 + i * 8, true))]));
    } finally {
      this.engine.request(scratch);
    }
  }
  stats() {
    return {
      receiptMode: this.receiptMode,
      callbackReceiptMode: "checked",
      ownedReceipts: this.ownedReceipts,
      compiledReceipts: this.compiledReceipts,
      checkedReceipts: this.checkedReceipts,
      admissions: this.admissions,
      evictions: this.evictions,
      runs: this.runs,
      retired: this.retired,
      dispatches: this.dispatches,
      directRuns: this.directRuns,
      restoredSeals: this.restoredSeals,
      promotions: this.promotions,
      promotionDeferrals: this.promotionDeferrals,
      generatedModuleBytes: this.generatedModuleBytes,
      profileMetadataBytes: this.profileMetadataBytes,
      retainedModuleBytes: this.retainedModuleBytes,
      installedUnits: this.slots.size,
      unitCapacity: this.table.length,
      wasmMemoryBytes: this.engine.memory.buffer.byteLength,
      cacheDiagnostics: this.diagnostics?.snapshot(),
      x87ControlWord: this.diagnostics ? this.engine.get(X87_CONTROL_OFFSET) & 0xffff : undefined,
      ...this.engineUsage,
    };
  }
}
