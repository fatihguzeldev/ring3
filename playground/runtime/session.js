import { Engine, KEY, pair, view, RESIDENT_PAGES, ownedExecutionBindings } from "./engine.js";
import { CodeCache } from "./cache.js";
import { Files, launch } from "./files.js";
import { configure, DEFAULT_EXECUTION } from "./configuration.js";
import { Media } from "./media.js";
import { WindowPresentation } from "./windows.js";
import { TaskScheduler } from "./scheduler/index.js";
import { PhaseMeasurements } from "./session/measurement.js";
import { CompletedWindowsCalls } from "./session/diagnostics.js";
import { FrameProbe, frameProbeNow, frameProbeStamp } from "./session/frame-probe.js";
const faults = new Set([10, 11, 12, 13, 16, 17, 18, 19, 20, 21, 22, 23]);
export class Session {
  constructor(emit) {
    this.emit = emit;
    this.running = false;
    this.paused = false;
    this.stopped = false;
    this.abort = new AbortController();
    this.scheduler = new TaskScheduler(this.abort.signal);
    this.thread = 1;
    this.callbacks = new Map();
    this.exits = new Map();
    this.blocked = new Map();
    this.recent = [];
    this.lastStatus = 0;
    this.needsSchedule = false;
  }
  status(state, message, extra = {}) {
    if (!this.stopped) this.code?.sampleUsage();
    this.callDiagnostics?.sampleInput({ closed: this.stopped });
    this.emit({
      type: "status",
      state,
      message,
      pc: this.engine?.get(48),
      stats: this.code?.stats(),
      windowsCalls: this.callDiagnostics?.snapshot(),
      physicalInputFocus: this.media?.lastPhysicalFocus,
      inputDelivery: this.media?.inputStatus?.(),
      graphicsInventory: this.media?.graphicsStatus?.(),
      fileReads: this.files?.snapshot?.(),
      ...extra,
    });
  }
  start(options) {
    this.initializing ??= this.initialize(options);
    return this.initializing;
  }
  async initialize(options) {
    this.receiptMode = options.receiptMode ?? "checked";
    if (!["checked", "owned", "compiled"].includes(this.receiptMode)) throw new Error("invalid execution receipt mode");
    this.observation = options.observation;
    this.frameProbeEnabled = options.frameProbe === true;
    this.frameProbeCensusEnabled = this.frameProbeEnabled && options.frameProbeCensus === true;
    this.frameProbeMathEnabled = this.frameProbeEnabled && options.frameProbeMath === true;
    this.frameProbe = null;
    this.receiptModeControl = options.receiptModeControl === true;
    this.status("loading", "loading engine and local file manifest");
    const loadEngine = () =>
      Engine.create(
        options.engineUrl ?? "/engine.wasm",
        RESIDENT_PAGES,
        this.abort.signal,
        options.compiledEngine,
      );
    this.engine = await (this.observation
      ? this.observation.observer.scopeAsync("engine.load", {}, loadEngine)
      : loadEngine());
    this.abort.signal.throwIfAborted();
    const h = this.engine;
    if (this.receiptMode !== "checked") ownedExecutionBindings(h, true);
    this.observation?.bind(h);
    this.measurements = this.observation?.observer.enabled
      ? new PhaseMeasurements(this.observation)
      : null;
    let manifest = options.manifest;
    if (manifest) this.engine.localFiles = options.localFiles;
    else {
      const response = await fetch(options.manifestUrl ?? "/files/manifest", {
        signal: this.abort.signal,
      });
      if (!response.ok) throw new Error(`manifest HTTP${response.status}`);
      manifest = await response.json();
    }
    this.abort.signal.throwIfAborted();
    if (!Array.isArray(manifest.files)) throw new Error("file manifest lacks files");
    const executable = options.executable ?? manifest.executable;
    if (!executable) throw new Error("choose an executable");
    const slash = executable.lastIndexOf("\\"),
      directory = slash < 3 ? "C:\\" : executable.slice(0, slash);
    const execution = options.execution ?? DEFAULT_EXECUTION;
    this.observation?.observer.event("execution.config", execution);
    this.windows = new WindowPresentation(h, this.emit);
    this.media = new Media(h, configure(h, execution), this.emit, this.windows);
    this.media.graphicsTransport.probe = () => this.frameProbe;
    this.callDiagnostics = this.observation?.observer.enabled ? new CompletedWindowsCalls(h) : null;
    this.files = new Files(h, manifest, options.bytesUrl ?? "/files/bytes");
    this.files.configure();
    this.status("loading", `mounting ${manifest.files.length} local file entries`);
    for (const file of manifest.files) this.files.mount(file);
    this.files.environment(directory);
    launch(h, executable, options.commandLine, directory);
    this.status("loading", `loading ${executable}`);
    const stage = () => this.files.stageExecutable(executable);
    await (this.observation
      ? this.observation.observer.scopeAsync("executable.stage", {}, stage)
      : stage());
    this.abort.signal.throwIfAborted();
    h.ok("start_module_process", h.name(executable), 0, 0x7e000000, 256, 0x7f000000);
    while (await this.files.sourceProgress()) this.abort.signal.throwIfAborted();
    this.abort.signal.throwIfAborted();
    this.code = new CodeCache(h, { slots: execution.units, receiptMode: this.receiptMode });
    this.observation?.observer.event("execution.receipt-mode", { mode: this.code.receiptMode, callback_mode: "checked", engine_sha256: h.hash });
    this.started = performance.now();
    this.lastSchedule = this.started;
    this.lastPromotion = this.started;
    this.pausedDuration = 0;
    this.running = true;
    this.status("running", "guest execution started", { engineHash: h.hash });
    this.executing = this.loop()
      .catch((error) => {
        if (this.abort.signal.aborted && error === this.abort.signal.reason) return;
        this.fail(error);
      })
      .finally(() => this.scheduler.close());
  }
  stack() {
    if (!this.callbacks.has(this.thread)) this.callbacks.set(this.thread, []);
    return this.callbacks.get(this.thread);
  }
  initializer() {
    this.engine.ok("module_initializer");
    const d = view(this.engine.take(72));
    return d.getUint32(16, true) === 1
      ? {
          token: d.getBigUint64(24, true),
          returnPc: d.getUint32(44, true),
          returnId: d.getUint32(48, true),
        }
      : null;
  }
  returnGate() {
    if (this.returnPc) return this.returnPc;
    const h = this.engine;
    for (let start = 0, total = 1; start < total;) {
      h.ok("module_gates_page", start, 128);
      const d = view(h.take(4096)),
        count = d.getUint32(24, true);
      total = d.getUint32(16, true);
      if (!count) throw new Error("empty module gate page");
      for (let i = 0; i < count; i++)
        if (d.getUint32(36 + i * 8, true) === 0xffffe)
          this.returnPc = d.getUint32(32 + i * 8, true);
      start += count;
    }
    if (!this.returnPc) throw new Error("canonical callback return gate absent");
    return this.returnPc;
  }
  async beginCallback() {
    const h = this.engine;
    if (h.get(72) === 9) return false;
    h.ok("pending_windows_callback");
    const request = view(h.take(96));
    if (!request.getUint32(12, true)) return false;
    const unit = this.code.entry(request.getUint32(16, true)),
      returnPc = this.returnGate();
    h.ok("begin_windows_resident_callback", ...KEY, ...pair(unit.id), returnPc, 0xffffe);
    const d = view(h.take(72)),
      callback = {
        home: d.getBigUint64(64, true),
        token: d.getUint32(16, true),
        returnPc,
        returnId: 0xffffe,
        returnEsp: (d.getUint32(36, true) + 4 + d.getUint32(48, true) * 4) >>> 0,
      };
    h.ok("authorize_resident_callback", ...KEY, ...pair(callback.home), callback.token);
    callback.activation = this.callDiagnostics?.activation.begin(request, this.thread);
    if (callback.activation)
      this.observation.observer.event("windows.activation.begin", callback.activation);
    this.stack().push(callback);
    return true;
  }
  async providers() {
    const h = this.engine;
    let pending = false;
    if (await this.files.supplyPending()) pending = true;
    if (await this.files.sourceProgress()) pending = true;
    h.ok("progress_file_io", ...pair(this.files.epoch));
    for (const [query, resume] of [
      ["pending_file_call", "resume_file_call"],
      ["pending_windows_wait", "resume_windows_wait"],
      ["pending_activation_call", "resume_activation_call"],
    ]) {
      const data = h.media(query),
        token = Number(data.values[0]);
      if (!data.epoch || !token) continue;
      pending = true;
      const status = h.status(resume, ...KEY, token);
      if (status !== 0 && status !== 12)
        throw Object.assign(new Error(`${resume} returned ${status}`), {
          operation: resume,
          status,
        });
    }
    h.ok("pending_registry_call");
    const registry = view(h.take(32));
    if (registry.getBigUint64(16, true) && registry.getUint32(28, true)) {
      h.ok("resume_registry_call", ...KEY, registry.getUint32(24, true));
      pending = true;
    }
    const user = h.media("waiting_user32_message");
    if (user.values[0]) {
      const status = h.status("resume_user32_wait", ...KEY, ...pair(user.values[0]));
      if (status !== 0 && status !== 12) throw new Error(`resume_user32_wait returned${status}`);
      pending = true;
    }
    this.settleWindows();
    await this.beginCallback();
    return pending;
  }
  observeClock() {
    const now = BigInt(Math.floor((performance.now() - this.started - this.pausedDuration) * 1e6));
    this.engine.ok("observe_cpu_clock", ...pair(now));
    return now;
  }
  setReceiptMode(mode) {
    if (!this.frameProbeEnabled || !this.receiptModeControl || !this.running || this.frameProbe || this.requestedFrameProbe || this.censusDrain) return;
    const previousMode = this.code.receiptMode;
    this.code.setReceiptMode(mode);
    this.emit({ type: "diagnostic", message: "execution-receipt-mode", detail: {
      ...frameProbeStamp(), previous_mode: previousMode, stats: this.code.stats(),
      mode: this.code.receiptMode, callback_mode: "checked", engine_sha256: this.engine.hash,
    } });
  }
  armFrameProbe(id) {
    if (!this.frameProbeEnabled || !this.running || this.paused || this.frameProbe || this.requestedFrameProbe || this.censusDrain) return;
    if (typeof id !== "string" || !/^[a-zA-Z0-9-]{1,80}$/.test(id)) return;
    this.requestedFrameProbe = id;
  }
  complete(receipt) {
    const guestClock = this.observeClock();
    const probe = this.frameProbe;
    const isPresent = probe && (receipt.id === 0x4020f || receipt.id === 0x40c03);
    const before = isPresent ? frameProbeNow() : null;
    const status = this.measurements
      ? this.measurements.run("completion", () => this.engine.status(receipt.operation, ...receipt.args))
      : this.engine.status(receipt.operation, ...receipt.args);
    if (isPresent) probe.present(receipt, status, this.engine.get(16), before, frameProbeNow(), guestClock);
    if (status !== 0 && status !== 12)
      throw Object.assign(new Error(`${receipt.operation} returned ${status}`), {
        operation: receipt.operation,
        status,
      });
    if (status === 0) this.settleWindows();
    return status === 0;
  }
  settleWindows() {
    this.windows.settle(this.thread);
    this.callDiagnostics?.settle(this.thread);
  }
  async loop() {
    const h = this.engine;
    while (this.running) {
      if (this.paused) {
        await this.wait(20);
        continue;
      }
      const checkpoint = this.observation?.checkpoint();
      if (checkpoint) await checkpoint;
      if (!this.running || this.stopped) break;
      if (h.get(72) === 9) {
        this.status("exited", `process exited with code${h.get(80)}`, { exitCode: h.get(80) });
        this.running = false;
        break;
      }
      const now = this.observeClock();
      this.media.update(now);
      await this.media.graphicsOutput();
      if (this.running && !this.stopped && this.requestedFrameProbe && !this.media.graphicsTransport.pending &&
          ![...this.blocked.values()].some((call) => call.id === 0x4020f || call.id === 0x40c03)) {
        this.frameProbe = new FrameProbe(this.requestedFrameProbe, this);
        this.requestedFrameProbe = null;
      }
      if (!this.running || this.stopped) break;
      const waiting = await (this.measurements
        ? this.measurements.runAsync("providers", () => this.providers())
        : this.providers());
      if (!this.running || this.stopped) break;
      // yield at a bounded host boundary even when a guest only polls or computes.
      if (
        this.needsSchedule ||
        h.api.pending_windows_thread_schedule() ||
        waiting ||
        performance.now() - this.lastSchedule >= 8
      ) {
        const result = h.status("schedule_windows_thread");
        if (result !== 0 && result !== 12)
          throw new Error(`schedule_windows_thread returned${result}`);
        if (result === 0) {
          this.lastSchedule = performance.now();
          const d = view(h.take(40)),
            state = d.getUint32(12, true),
            thread = d.getUint32(16, true);
          if (thread) this.thread = thread;
          this.settleWindows();
          if (state === 3) {
            this.running = false;
            this.status("exited", `process exited with code${d.getUint32(20, true)}`);
            break;
          }
          if (state === 2) {
            await this.wait(4);
            continue;
          }
          if (state === 4) {
            await this.beginCallback();
            continue;
          }
          if (state === 5) {
            this.handleException();
            continue;
          }
          this.needsSchedule = false;
        } else if (h.api.pending_windows_thread_schedule()) {
          await this.wait(4);
          continue;
        }
      }
      // scheduling consumes completed thread returns before any retained exit retry.
      const retained = this.exits.get(this.thread);
      if (
        retained &&
        !this.blocked.has(this.thread) &&
        h.get(48) === retained.pc &&
        h.get(72) === 8
      ) {
        this.complete(retained);
        this.needsSchedule = true;
        continue;
      }
      const blocked = this.blocked.get(this.thread);
      if (blocked) {
        if (h.get(48) !== blocked.pc) this.blocked.delete(this.thread);
        else if (!this.complete(blocked)) {
          this.needsSchedule = true;
          await this.wait(1);
          continue;
        } else {
          this.blocked.delete(this.thread);
          await this.beginCallback();
        }
      }
      if (h.get(72) === 9) continue;
      const start = performance.now();
      do {
        const init = this.initializer(),
          unit = this.measurements
            ? this.measurements.run("entry", () => this.code.entry(h.get(48)))
            : this.code.entry(h.get(48)),
          callback = this.stack().at(-1);
        let active = false;
        if (callback && !init)
          active =
            h.status(
              "select_resident_callback_unit",
              ...KEY,
              ...pair(callback.home),
              callback.token,
              ...pair(unit.id),
            ) === 0;
        const run = this.measurements
          ? this.measurements.run("execution", () =>
              this.code.execute(unit, { dispatch: !callback }),
            )
          : this.code.execute(unit, { dispatch: !callback });
        this.recent.push({ ...run, owner: run.owner.toString() });
        if (this.recent.length > 48) this.recent.shift();
        if (run.reason === 1 || run.reason === 3) continue;
        if (run.reason === 9) break;
        if (faults.has(run.reason)) {
          h.ok("dispatch_resident_windows_exception", ...KEY, ...pair(run.owner), ...pair(h.epoch));
          this.handleException();
          break;
        }
        if (run.reason !== 8) throw new Error(`unhandled CPU stop${run.reason}/${run.detail}`);
        if (init && run.pc === init.returnPc && run.detail === init.returnId) {
          h.ok("finish_module_initializer", ...pair(init.token));
          break;
        }
        if (run.detail === 0x121000) {
          this.finishException();
          break;
        }
        if (
          active &&
          callback &&
          run.pc === callback.returnPc &&
          h.get(32) === callback.returnEsp
        ) {
          h.ok("finish_windows_resident_callback", ...KEY, ...pair(callback.home), callback.token);
          this.callDiagnostics?.activation.complete(callback.activation);
          if (callback.activation)
            this.observation.observer.event("windows.activation.end", callback.activation);
          this.stack().pop();
          this.settleWindows();
          break;
        }
        if (active)
          h.ok(
            "capture_active_resident_callback_windows_call",
            ...KEY,
            ...pair(run.owner),
            callback.token,
          );
        else h.ok("capture_resident_windows_call", ...KEY, ...pair(run.owner));
        const captured = view(h.take(112)),
          token = captured.getUint32(16, true),
          args = Array.from({ length: captured.getUint32(28, true) }, (_, i) =>
            captured.getUint32(48 + i * 4, true),
          );
        this.lastCall = { id: run.detail, pc: run.pc, args };
        const receipt = {
          pc: run.pc,
          id: run.detail,
          callArgs: args,
          operation: active
            ? "complete_active_resident_callback_windows_call"
            : "complete_resident_windows_call",
          args: active
            ? [...KEY, ...pair(run.owner), callback.token, token]
            : [...KEY, ...pair(run.owner), token],
        };
        this.windows.capture(receipt, captured, this.thread);
        this.callDiagnostics?.capture(receipt, captured, this.thread);
        if ([0x10003, 0xfffff, 0xf0003].includes(run.detail)) this.exits.set(this.thread, receipt);
        if (!this.complete(receipt)) {
          this.blocked.set(this.thread, receipt);
          this.needsSchedule = true;
          break;
        }
        const callbackStarted = await this.beginCallback();
        this.needsSchedule = run.detail === 0xffffe || !!h.api.pending_windows_thread_schedule();
        if (
          run.detail === 0x1001e ||
          run.detail === 0x1001f ||
          callbackStarted ||
          this.needsSchedule ||
          h.get(72) !== 3
        )
          break;
      } while (this.running && performance.now() - start < 6);
      if (performance.now() - this.lastPromotion >= 100) {
        this.lastPromotion = performance.now();
        if (this.measurements) this.measurements.run("promotion", () => this.code.promote());
        else this.code.promote();
      }
      if (performance.now() - this.lastStatus > 1000) {
        this.lastStatus = performance.now();
        this.status("running", "running", {
          bytesFetched: this.files.fetched,
          thread: this.thread,
          elapsedMs: performance.now() - this.started - this.pausedDuration,
          lastCall: this.lastCall,
          phases: this.measurements?.snapshot(),
        });
        this.observation?.counters(this.code.stats(), this.files.fetched);
        this.measurements?.report();
        this.media.inputMeasurements?.report(this.observation.observer);
      }
      await this.wait(0);
    }
  }
  wait(milliseconds) {
    if (!this.measurements) return this.scheduler.wait(milliseconds);
    return this.measurements.runAsync("wait", () => this.scheduler.wait(milliseconds));
  }
  finishException() {
    const h = this.engine;
    h.ok("pending_windows_exception");
    const d = view(h.take(64)),
      id = d.getBigUint64(16, true);
    const snapshot = {
      present: !!d.getUint32(12, true),
      id: id.toString(),
      thread: d.getUint32(24, true),
      phase: d.getUint32(28, true),
      procedure: d.getUint32(32, true),
      record: d.getUint32(36, true),
      context: d.getUint32(40, true),
      returnPc: d.getUint32(44, true),
      code: d.getUint32(52, true),
      pc: h.get(48),
      esp: h.get(32),
      eax: h.get(16),
    };
    try {
      h.ok("finish_windows_exception_handler", ...KEY, ...pair(id));
    } catch (error) {
      error.exception = snapshot;
      throw error;
    }
  }
  handleException() {
    const h = this.engine;
    h.ok("pending_windows_exception");
    const d = view(h.take(64));
    if (d.getUint32(12, true) && d.getUint32(28, true) === 6)
      throw Object.assign(
        new Error(
          `unhandled guest exception0x${d.getUint32(52, true).toString(16)} at0x${h.get(48).toString(16)}`,
        ),
        { exception: Array.from(new Uint32Array(h.take(64).buffer)) },
      );
  }
  pause() {
    if (this.paused) return;
    this.paused = true;
    this.pauseStarted = performance.now();
    this.status("paused", "paused");
  }
  resume() {
    if (!this.paused) return;
    this.pausedDuration += performance.now() - this.pauseStarted;
    this.paused = false;
    this.status("running", "resumed");
  }
  stop(notify = true) {
    if (this.stopped) return;
    this.stopped = true;
    this.cancel();
    if (this.engine) this.engine.status("close");
    if (notify) this.status("stopped", "stopped");
  }
  cancel() {
    this.running = false;
    this.abort.abort();
  }
  async settled() {
    await this.initializing?.catch(() => {});
    await this.executing?.catch(() => {});
    this.frameProbe?.finish("session-ended");
    await this.censusDrain?.catch(() => {});
  }
  fail(error) {
    this.scheduler.close();
    if (this.stopped) return;
    this.running = false;
    this.emit({
      type: "diagnostic",
      message: error.message,
      detail: {
        operation: error.operation,
        status: error.status,
        pc: error.pc ?? this.engine?.get(48),
        reason: error.reason,
        detail: error.detail,
        exception: error.exception,
        call: this.lastCall,
        recent: this.recent,
      },
    });
    this.status("error", error.message);
  }
}
