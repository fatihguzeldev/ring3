import { Engine, KEY, pair, view, RESIDENT_PAGES } from "./engine.js";
import { CodeCache } from "./cache.js";
import { callbackRecord } from "./callback.js";
import { Files, launch } from "./files.js";
import { configure, DEFAULT_EXECUTION } from "./configuration.js";
import { Media } from "./media.js";
import { WindowPresentation } from "./windows.js";
import { TaskScheduler } from "./scheduler/index.js";
import { PhaseMeasurements } from "./session/measurement.js";
import { CompletedWindowsCalls } from "./session/diagnostics.js";
import { unsupportedInstruction } from "./session/diagnostics/instruction.js";
import { FrameProbe, frameProbeNow } from "./session/frame-probe.js";
const faults = new Set([5, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24]);
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
    this.fastNativeCalls = false;
    this.fastNativeCompletions = 0;
  }
  status(state, message, extra = {}) {
    const unavailable = [];
    const read = (field, operation) => {
      if (state !== "error") return operation();
      try {
        return operation();
      } catch (error) {
        unavailable.push({ field, name: error?.name ?? "Error", message: error?.message ?? String(error) });
        return undefined;
      }
    };
    if (state !== 'error' && !this.stopped) this.code?.publish();
    if (!this.stopped) {
      const usage = read("cache.sampleUsage", () => this.code?.sampleUsage());
      if (state === "error" && this.code && usage == null &&
          !unavailable.some(item => item.field === "cache.sampleUsage"))
        unavailable.push({ field: "cache.sampleUsage", message: "unavailable" });
    }
    read("windowsCalls.sampleInput", () => this.callDiagnostics?.sampleInput({ closed: this.stopped }));
    const snapshot = {
      type: "status",
      state,
      message,
      backend: this.backend ?? "core",
      cpuHash: this.externalCpu?.hash,
      pc: read("pc", () => this.engine?.get(48)),
      stats: read("cache.stats", () => this.code?.stats()),
      windowsCalls: read("windowsCalls.snapshot", () => this.callDiagnostics?.snapshot()),
      physicalInputFocus: this.media?.lastPhysicalFocus,
      inputDelivery: read("inputDelivery", () => this.media?.inputStatus?.()),
      graphicsInventory: read("graphicsInventory", () => this.media?.graphicsStatus?.()),
      fileReads: read("fileReads", () => this.files?.snapshot?.()),
      manualPauseMs: (this.pausedDuration ?? 0) + (this.paused ? performance.now() - this.pauseStarted : 0),
      ...(this.measurements ? {waitSemantics: 'host scheduler task/timer elapsed including event-loop delay; paused-loop waits excluded; not guest wait API duration'} : {}),
      ...extra,
    };
    if (unavailable.length) snapshot.unavailable = unavailable;
    this.emit(snapshot);
  }
  start(options) {
    this.initializing ??= this.initialize(options);
    return this.initializing;
  }
  async initialize(options) {
    if (options.receiptMode !== undefined) throw new Error("execution receipt modes are not supported by the core SDK");
    this.backend = options.backend ?? "core";
    if (!["core", "v86"].includes(this.backend)) throw new TypeError("unknown CPU backend");
    if (this.backend === "v86" && options.frameProbe) throw new Error("frame probing is unavailable for the v86 backend");
    this.observation = options.observation;
    this.frameProbeEnabled = options.frameProbe === true;
    if (options.frameProbeCensus) throw new Error("resident execution census is not supported by the core SDK");
    this.frameProbe = null;
    this.status("loading", "loading engine and local file manifest");
    if (this.backend === "v86") {
      const { prepareV86 } = await import("./v86/index.js");
      this.externalCpu = await prepareV86(options.v86, { signal: this.abort.signal });
      this.abort.signal.throwIfAborted();
    }
    const loadEngine = () =>
      Engine.create(
        options.engineUrl ?? "/engine.wasm",
        RESIDENT_PAGES,
        this.abort.signal,
        options.compiledEngine,
        this.externalCpu ? { memory: this.externalCpu.memory, minimumMemoryBytes: 805306368 } : undefined,
      );
    this.engine = await (this.observation
      ? this.observation.observer.scopeAsync("engine.load", {}, loadEngine)
      : loadEngine());
    this.abort.signal.throwIfAborted();
    const h = this.engine;
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
    this.media = new Media(h, configure(h), this.emit, this.windows, () => this.inputClock());
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
    this.code = this.externalCpu ? await this.externalCpu.attach(h, execution) : new CodeCache(h, execution);
    if(this.backend==='v86') {
      this.code.inlineOwner={
        canContinue:()=>this.running && !this.paused && !this.stopped && !this.measurements &&
          !this.callDiagnostics && !this.frameProbe && !globalThis.__r3PhaseWindow &&
          !this.stack().length && !this.needsSchedule && performance.now()<this.inlineQuantumDeadline,
        observeClock:()=>this.observeClock(),
        canBridge:()=>!this.measurements && !this.callDiagnostics && !this.frameProbe &&
          !globalThis.__r3PhaseWindow && !globalThis.__r3GateCensus && !globalThis.__r3BindingGatePolicy,
      };
      if(typeof this.code.initializeExecutionPolicy==='function')this.code.initializeExecutionPolicy();
    }
    this.abort.signal.throwIfAborted();
    this.observation?.observer.event("execution.backend", { backend: this.backend, callback_backend: this.backend,
      engine_sha256: h.hash, cpu_sha256: this.externalCpu?.hash });
    this.started = performance.now();
    this.lastSchedule = this.started;
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
  beginCallback() {
    const h = this.engine;
    if (h.get(72) === 9) return false;
    h.ok("pending_windows_callback");
    if (!h.resultView(96).getUint32(12, true)) return false;
    const request = view(h.take(96));
    const returnPc = this.returnGate();
    h.ok("core_begin_windows_callback", returnPc, 0xffffe);
    const callback = callbackRecord(h.take(64), returnPc, 0xffffe);
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
    this.beginCallback();
    return pending;
  }
  observeClock() {
    const now = BigInt(Math.floor((performance.now() - this.started - this.pausedDuration) * 1e6));
    this.engine.ok("observe_cpu_clock", ...pair(now));
    return now;
  }
  inputClock() {
    const current=this.paused?this.pauseStarted:performance.now();
    if(!Number.isFinite(this.started))return {originMs:performance.timeOrigin+current,nowNs:0n};
    const paused=this.pausedDuration??0;
    return {
      originMs:performance.timeOrigin+this.started+paused,
      nowNs:BigInt(Math.max(0,Math.floor((current-this.started-paused)*1e6))),
    };
  }
  armFrameProbe(id) {
    if (!this.frameProbeEnabled || !this.running || this.paused || this.frameProbe || this.requestedFrameProbe) return;
    if (typeof id !== "string" || !/^[a-zA-Z0-9-]{1,80}$/.test(id)) return;
    this.media?.graphicsTransport?.requestFlush?.();
    this.requestedFrameProbe = id;
  }
  complete(receipt, observedClock) {
    const guestClock = observedClock ?? this.observeClock();
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
    this.requestGraphicsFlush(receipt.id,status);
    if (status === 0) this.settleWindows();
    return status === 0;
  }
  requestGraphicsFlush(id,status) {
    if ((status===0 && (id===0x4020f || id===0x40c03 || id===0x4024e || id===0x4025a)) ||
        (status===12 && ((id&0xffff0000)===0x40000 || id===0x50001)))
      this.media?.graphicsTransport?.requestFlush?.();
  }
  settleWindows() {
    this.windows.settle(this.thread);
    this.callDiagnostics?.settle(this.thread);
  }
  async loop() {
    const h = this.engine;
    while (this.running) {
      this.code.publish();
      if (this.paused) {
        await this.scheduler.wait(20);
        continue;
      }
      const checkpoint = this.observation?.checkpoint();
      if (checkpoint) await checkpoint;
      if (!this.running || this.stopped) break;
      if (this.paused) continue;
      if (h.get(72) === 9) {
        this.status("exited", `process exited with code${h.get(80)}`, { exitCode: h.get(80) });
        this.running = false;
        break;
      }
      const now = this.observeClock();
      this.media.update(now);
      await this.media.graphicsOutput();
      if (this.paused) continue;
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
      if (this.paused) continue;
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
            this.beginCallback();
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
          this.beginCallback();
        }
      }
      if (h.get(72) === 9) continue;
      const init = this.initializer();
      this.code.import();
      const start = performance.now();
      this.inlineQuantumDeadline=start+6;
      do {
        const allowBudgetContinuation=this.backend==='v86' && !this.measurements && !this.callDiagnostics &&
          !this.frameProbe && !globalThis.__r3PhaseWindow;
        const unit = this.measurements
            ? this.measurements.run("entry", () => this.code.select(false))
            : this.code.select(allowBudgetContinuation),
          callback = this.stack().at(-1);
        const measurements = this.measurements,
          probe = this.frameProbe?.measurements === measurements ? this.frameProbe : null,
          startPc = probe ? this.code.pc : undefined;
        let run;
        try {
          run = measurements
            ? measurements.run("execution", () => this.code.execute(unit),
                probe ? (unit === null ? "cold" : "compiled") : undefined)
            : this.code.execute(unit);
        } finally {
          if (probe) probe.recordExecution(run, startPc, unit, measurements.lastMilliseconds?.("execution"));
        }
        this.recent.push(run);
        if (this.recent.length > 48) this.recent.shift();
        if(run.inlineYield)break;
        if (run.coreReason === 1 || run.coreReason === 2 || run.coreReason === 6) continue;
        if (run.reason === 9) break;
        if (faults.has(run.reason)) {
          h.ok("core_dispatch_exception");
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
          callback &&
          run.pc === callback.returnPc &&
          h.get(32) === callback.returnEsp
        ) {
          h.ok("core_finish_windows_callback", callback.token);
          this.callDiagnostics?.activation.complete(callback.activation);
          if (callback.activation)
            this.observation.observer.event("windows.activation.end", callback.activation);
          this.stack().pop();
          this.settleWindows();
          break;
        }
        let fused = 256, observedClock=run.observedClock;
        if (this.backend === "v86" && this.fastNativeCalls && !this.measurements && !this.callDiagnostics &&
            !this.frameProbe && !globalThis.__r3PhaseWindow &&
            typeof h.api.component_cpu_try_complete_gate === "function") {
          observedClock ??= this.observeClock();
          fused = h.status("component_cpu_try_complete_gate", ...KEY);
          if (fused !== 0 && fused !== 12 && fused !== 256)
            throw Object.assign(new Error(`component_cpu_try_complete_gate returned${fused}`), {
              operation: "component_cpu_try_complete_gate", status: fused, pc: run.pc, id: run.detail,
            });
        }
        if (fused === 256) h.ok("core_capture_windows_call");
        const captured = h.resultView(112),
          token = captured.getUint32(16, true),
          args = Array.from({ length: captured.getUint32(28, true) }, (_, i) =>
            captured.getUint32(48 + i * 4, true),
          );
        this.lastCall = { id: run.detail, pc: run.pc, args };
        const receipt = {
          pc: run.pc,
          id: run.detail,
          callArgs: args,
          operation: "core_complete_windows_call",
          args: [token],
        };
        this.windows.capture(receipt, captured, this.thread);
        this.callDiagnostics?.capture(receipt, captured, this.thread);
        if(fused!==256)this.requestGraphicsFlush(receipt.id,fused);
        if ([0x10003, 0xfffff, 0xf0003].includes(run.detail)) this.exits.set(this.thread, receipt);
        if (fused === 0) {
          this.fastNativeCompletions++;
          this.settleWindows();
        } else if (fused === 12 || !this.complete(receipt, observedClock)) {
          this.blocked.set(this.thread, receipt);
          this.needsSchedule = true;
          break;
        }
        const callbackStarted = this.beginCallback();
        this.needsSchedule = run.detail === 0xffffe || !!h.api.pending_windows_thread_schedule();
        if (
          run.detail === 0x1001e ||
          run.detail === 0x1001f ||
          callbackStarted ||
          this.needsSchedule ||
          h.get(72) !== 3
        )
          break;
        this.code.import();
      } while (this.running && !this.paused && performance.now() - start < 6);
      this.code.publish();
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
      h.ok("core_finish_exception");
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
    if (this.paused) return this.pausing;
    this.media?.pauseAudio?.();
    this.paused = true;
    this.pauseStarted = performance.now();
    this.pausing=(async()=>{
      await this.drainGraphics();
      if(!this.stopped && !this.abort.signal.aborted)this.status("paused", "paused");
    })().finally(()=>{this.pausing=null;});
    return this.pausing;
  }
  resume() {
    if(this.finalizing || this.stopped || this.abort?.signal.aborted)return;
    if(this.pausing)return this.pausing.then(()=>this.resume());
    if (!this.paused) return;
    this.pausedDuration += performance.now() - this.pauseStarted;
    this.media?.resumeAudio?.();
    this.paused = false;
    this.status("running", "resumed");
  }
  drainGraphics() {
    this.code?.publish();
    return this.media?.graphicsTransport?.drain?.();
  }
  prepareFinalization() {
    this.running=false;
    this.media?.pauseAudio?.();
    this.scheduler?.wake();
    if(this.executing && !this.failureReported && !this.abort.signal.aborted)return this.drainGraphics();
  }
  stop(notify = true) {
    if (this.stopped) return;
    this.cancel();
    this.code?.close?.();
    if (this.engine) this.engine.ok("close");
    this.externalCpu?.dispose();
    if (this.paused) {
      this.pausedDuration = (this.pausedDuration ?? 0) + performance.now() - this.pauseStarted;
      this.paused = false;
    }
    this.stopped = true;
    if (notify) this.status("stopped", "stopped");
  }
  cancel() {
    this.running = false;
    this.media?.pauseAudio?.();
    this.abort.abort();
  }
  async settled() {
    await this.initializing?.catch(() => {});
    await this.executing?.catch(() => {});
    this.frameProbe?.finish("session-ended");
  }
  fail(error) {
    if (this.stopped || this.failureReported) return;
    // A native trap can leave later diagnostic exports unavailable. Preserve
    // the original exception before attempting any cleanup or status reads.
    const original = { name: error.name, message: error.message, stack: error.stack };
    this.failureReported = true;
    this.scheduler.close();
    this.running = false;
    const unavailable = [];
    const inspect = (field, operation) => {
      try {
        return operation();
      } catch (error) {
        unavailable.push({ field, name: error?.name ?? "Error", message: error?.message ?? String(error) });
        return undefined;
      }
    };
    const pc = error.pc ?? inspect("pc", () => this.engine?.get(48));
    const instruction = error.reason === 4 && this.engine
      ? inspect("instruction", () => unsupportedInstruction(this.engine,this.files?.assets,error.pc ?? this.code?.pc ?? pc))
      : undefined;
    this.emit({
      type: "diagnostic",
      message: original.message,
      detail: {
        name: original.name,
        stack: original.stack,
        operation: error.operation,
        status: error.status,
        pc,
        reason: error.reason,
        detail: error.detail,
        exception: error.exception,
        call: this.lastCall,
        recent: this.recent.map(run => ({...run, id: run.id === null ? null : run.id.toString()})),
        ...(instruction ? {instruction} : {}),
        ...(unavailable.length ? {unavailable} : {}),
      },
    });
    this.status("error", original.message);
  }
}
