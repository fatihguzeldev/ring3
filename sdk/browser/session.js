import { AudioHost } from "./audio/host.js";
import { InputHost } from "./input.js";
import { DialogHost } from "./dialogs.js";
import { WindowHost } from "./window.js";
import { GdiHost } from "./gdi.js";
import { BrowserCapture } from "./observability.js";
import { categories } from "../../observability/typescript/dist/index.js";
import { FrameMeasurements } from "./session/frames.js";
const frameProbeOrigin = BigInt(Math.floor(performance.timeOrigin * 1e6));
const frameProbeStamp = () => ({ origin_ns: String(frameProbeOrigin),
  timestamp_ns: String(frameProbeOrigin + BigInt(Math.floor(performance.now() * 1e6))) });

export class Session {
  constructor({
    canvas,
    gdiCanvas,
    title,
    dialog,
    onStatus,
    onDiagnostic,
    onAudioStats,
    onCapture,
    onObservation = () => {},
  }) {
    this.canvas = canvas;
    this.onStatus = onStatus;
    this.onDiagnostic = onDiagnostic;
    this.onObservation = onObservation;
    this.state = "idle";
    this.worker = null;
    this.graphics = null;
    this.compiledEngine = undefined;
    this.failed = false;
    this.generation = 0;
    this.audioRequest = 0;
    this.audioHold = null;
    this.graphicsQueue = Promise.resolve();
    this.audio = new AudioHost({ onDiagnostic, onStats: onAudioStats });
    this.input = new InputHost(canvas, {
      send: (events) => this.post({ type: "input", events,
        ...(this.capture?.observer.enabled ? { sentAtMs: performance.timeOrigin + performance.now() } : {}),
      }),
      viewport: () => this.pointerViewport(),
      onDiagnostic,
      onCapture,
    });
    this.windows = new WindowHost(canvas, title, { onDiagnostic });
    this.gdi = new GdiHost(gdiCanvas, this.windows.windows);
    this.dialogs = new DialogHost(dialog, {
      onResult: ({ epoch, id, button, error }) =>
        this.post({ type: "dialog-result", epoch, id, button, error }),
      onHelp: ({ epoch, id }) => this.post({ type: "dialog-help", epoch, id }),
      onVisibility: (visible) => this.input.setBlocked(visible),
    });
  }
  status(state, message, extra = {}) {
    if (this.state !== state && (state === "paused" || (this.state === "paused" && state === "running")))
      this.frames?.resetIntervals();
    this.state = state;
    this.capture?.observer.event("status", { state, message });
    this.onStatus({ state, message, ...extra, browserInput: this.input.snapshot?.(),
      presentation: this.frames?.snapshot(),
    });
  }
  pointerViewport() {
    if (!this.gdi.canvas.hidden)
      return { canvas: this.gdi.canvas, ...(this.gdi.origin ?? { x: 0, y: 0 }) };
    const window = this.windows.windows.get(this.gdi.active);
    return {
      canvas: this.canvas,
      x: window?.clientX ?? 0,
      y: window?.clientY ?? 0,
      width: window?.clientWidth ?? this.canvas.width,
      height: window?.clientHeight ?? this.canvas.height,
    };
  }
  post(message, transfer = []) {
    this.worker?.postMessage(message, transfer);
  }
  armFrameProbe() {
    if (this.state !== "running" || !this.frameProbeEnabled) return;
    this.canvas.focus();
    this.input.focus();
    if (!this.input.active) {
      this.onDiagnostic("frame-probe arm skipped: game display input is inactive", this.input.snapshot());
      return;
    }
    this.post({ type: "frame-probe-arm", id: crypto.randomUUID() });
  }
  async probe(engineUrl = "/engine.wasm") {
    this.status("checking", "checking the engine and browser GPU…");
    if (!navigator.gpu) throw new Error("WebGPU is unavailable in this browser");
    const engine = new Promise((resolve, reject) => {
      const worker = new Worker(new URL("../runtime/worker.js", import.meta.url), {
        type: "module",
      });
      const finish = (error, result) => {
        clearTimeout(timeout);
        worker.terminate();
        error ? reject(error) : resolve(result);
      };
      const timeout = setTimeout(
        () => finish(new Error("engine readiness check timed out")),
        30000,
      );
      worker.onerror = (event) => finish(new Error(event.message || "readiness worker failed"));
      worker.onmessage = ({ data }) => {
        if (data.type === "diagnostic") this.onDiagnostic(data.message, data.detail);
        if (data.type === "probe-result")
          finish(data.ok ? null : new Error(data.message || "engine readiness check failed"), data);
      };
      worker.postMessage({ type: "probe", engineUrl, compiledEngine: this.compiledEngine });
    });
    const result = await engine;
    this.compiledEngine = result.compiledEngine;
    this.onDiagnostic(
      `engine ready: ${result.engineHash}${result.bytes ? ` · ${result.bytes} bytes` : ""}`,
    );
    this.status("checking", "checking the browser graphics host…");
    const { runGraphicsValidation, runGdiValidation } = await import("../graphics/validation.js");
    const graphics = await runGraphicsValidation(document.createElement("canvas"));
    this.onDiagnostic(
      "graphics ready: fixed-function and programmable GPU readbacks passed",
      graphics,
    );
    this.onDiagnostic("GDI ready: frame colors and child composition passed", runGdiValidation());
    return result;
  }
  async start(config) {
    if (this.stopping || this.worker || this.state === "starting")
      throw new Error("stop the current session before starting another");
    const generation = ++this.generation;
    this.frameProbeEnabled = config.frameProbe === true;
    this.failed = false;
    this.status("starting", "preparing browser GPU and audio…");
    this.capture = null;
    let audioReady = Promise.resolve(null);
    try {
      this.capture = await BrowserCapture.start(
        { ...config.observability, executable: config.executable },
        this.onObservation,
      );
      this.input.setDiagnostics(Boolean(this.capture?.observer.enabled));
      this.graphicsObserver = this.capture?.capture.observer(categories.hostGraphics, "detailed");
      const frameObserver = this.capture?.capture.observer(categories.hostGraphics);
      this.frames = new FrameMeasurements(frameObserver?.enabled ? frameObserver : null);
      if (generation !== this.generation) {
        await this.capture?.finish("cancelled");
        return;
      }
      const startAudio = () => this.audio.start();
      audioReady = (
        this.capture
          ? this.capture.observer.scopeAsync("audio.prepare", {}, startAudio)
          : startAudio()
      ).then(
        () => null,
        (error) => error,
      );
      if (!navigator.gpu)
        throw new Error("WebGPU is unavailable. Use a browser with WebGPU enabled on this device.");
      const { createGraphicsHost } = await import("../graphics/index.js");
      let graphics;
      const prepareGraphics = () =>
        createGraphicsHost(this.canvas, {
          onDiagnostic: (info) => {
            if (info?.type === "present" || info?.type === "error") {
              if (generation !== this.generation || !graphics || this.graphics !== graphics) return;
              if (info.type === "error") {
                this.fail(new Error(info.message || "graphics host failed"));
                return;
              }
              this.gdi.presented(info.window);
              this.frames?.present();
            }
            this.onDiagnostic(typeof info === "string" ? info : JSON.stringify(info));
          },
        });
      graphics = await (this.capture
        ? this.capture.observer.scopeAsync("graphics.prepare", {}, prepareGraphics)
        : prepareGraphics());
      if (generation !== this.generation) {
        graphics.close();
        await audioReady;
        return;
      }
      this.graphics = graphics;
      const audioError = await audioReady;
      if (audioError) throw audioError;
      if (generation !== this.generation || this.failed) return;
      const worker = new Worker(new URL("../runtime/worker.js", import.meta.url), {
        type: "module",
      });
      this.worker = worker;
      this.capture?.workerStarted();
      worker.onmessage = (event) => {
        if (this.worker !== worker) return;
        this.message(event.data, worker).catch((error) => {
          if (this.worker === worker && this.generation === generation) this.fail(error);
        });
      };
      worker.onerror = (event) =>
        this.fail(new Error(event.message || "runtime worker failed to load"));
      worker.onmessageerror = () =>
        this.fail(new Error("runtime worker message could not be decoded"));
      const audioPort=this.audio.connectWorker(worker);
      this.post({
        type: "start",
        manifestUrl: "/files/manifest",
        bytesUrl: "/files/bytes",
        ...config,
        compiledEngine: this.compiledEngine,
        audioPort,
      },[audioPort]);
    } catch (error) {
      await audioReady;
      if (generation === this.generation) {
        this.graphics?.close();
        this.graphics = null;
        await this.audio.close();
        this.fail(error);
      }
      throw error;
    }
  }
  async message(message, worker) {
    if (this.capture?.receive(message, (acknowledgement) => worker.postMessage(acknowledgement)))
      return;
    if (message.type === "observability-data") {
      try {
        worker.postMessage({
          type: "observability-ack",
          sequence: message.sequence,
          accepted: false,
          error: "browser trace recorder is unavailable",
        });
      } catch (error) {
        this.onDiagnostic?.(`capture output: ${error.message}`);
      }
      return;
    }
    switch (message.type) {
      case "status":
        if (this.failed && message.state !== "stopped") return;
        const previousState = this.state;
        if (message.state === "running" && previousState !== "running") this.input.enable();
        if (["exited", "stopped", "error"].includes(message.state)) this.input.disable();
        this.status(message.state, message.message, message);
        if (message.state === "exited") this.finishCapture("complete");
        if (message.state === "error") this.finishCapture("failed", new Error(message.message));
        if (message.state === "stopped") this.stopped?.();
        break;
      case "diagnostic":
        this.capture?.observer.event("diagnostic", { message: message.message });
        this.onDiagnostic(message.message, message.detail);
        break;
      case "window":
        this.windows.apply(message);
        this.gdi.changed(message);
        break;
      case "gdi":
        this.gdi.accept(message.frame, message.pixels);
        this.post({ type: "gdi-complete", id: message.frame.id, revision: message.frame.revision });
        break;
      case "cursor":
        this.input.applyCursor(message);
        this.post({ type: "cursor-applied", revision: message.revision });
        break;
      case "dialog":
        this.dialogs.show(message);
        break;
      case "pcm": {
        const generation = this.generation;
        await this.audio.accept(message, ({epoch,sequence,firstFrame,frames,consumedFrames}) => {
          if (this.worker === worker && this.generation === generation && !this.failed)
            this.post({type:"pcm-consumed",epoch,sequence,firstFrame,frames,consumedFrames});
        });
        break;
      }
      case 'audio-stats':this.audio.receive(message.stats);break;
      case 'audio-control-reply':this.audio.receive(message.reply);break;
      case 'audio-closed':this.audio.reportClosed(message.snapshot);break;
      case "audio-held":
        if (this.audioHold?.worker === worker && this.audioHold.request === message.request)
          this.audioHold.resolve();
        break;
      case "graphics": {
        const timeline = message.frameProbe ? { receive: frameProbeStamp() } : null;
        this.graphicsQueue = this.graphicsQueue.then(async () => {
          if (this.worker !== worker) return;
          try {
            if (timeline) timeline.consume_start = frameProbeStamp();
            const consume = () => this.graphics.consume(message.packet, message.frameProbe);
            const result = await (this.graphicsObserver?.enabled
              ? this.graphicsObserver.scopeAsync(
                  "consume",
                  { request_id: message.requestId },
                  consume,
                )
              : consume());
            if (this.worker !== worker) return;
            if (timeline) timeline.consume_end = frameProbeStamp();
            const buffers = [
              ...new Set(
                (result.readbacks ?? []).flatMap(({ receipt, bytes }) => [
                  receipt.buffer,
                  bytes.buffer,
                ]),
              ),
            ];
            if (timeline) timeline.ack_send = frameProbeStamp();
            this.post(
              { type: "graphics-complete", requestId: message.requestId, ...result,
                ...(timeline ? { frameProbeTimeline: timeline } : {}),
              },
              buffers,
            );
          } catch (error) {
            if (this.worker !== worker) return;
            this.post({
              type: "graphics-error",
              requestId: message.requestId,
              error: error.message,
            });
            throw error;
          }
        });
        await this.graphicsQueue;
        break;
      }
      default:
        this.onDiagnostic(`unknown worker message: ${message.type}`);
    }
  }
  holdAudio(worker) {
    if (this.audioHold) return this.audioHold.promise;
    const request = ++this.audioRequest;
    let control;
    const promise = new Promise((resolve,reject) => {
      const settle = (operation,value) => {
        clearTimeout(control.timer);
        if (this.audioHold === control) this.audioHold = null;
        operation(value);
      };
      control = {worker,request,resolve:()=>settle(resolve),reject:error=>settle(reject,error)};
      control.timer = setTimeout(() => control.reject(new Error('audio production hold timed out')),5000);
    });
    control.promise = promise;
    this.audioHold = control;
    try {worker.postMessage({type:'audio-hold',request});}
    catch (error) {control.reject(error);}
    return promise;
  }
  cancelAudioHold(error) {this.audioHold?.reject(error);}
  async pause() {
    if (this.state !== "running") return;
    const worker = this.worker, generation = this.generation;
    this.input.disable({preserveFocus:true});
    try {
      await this.holdAudio(worker);
      if (this.worker !== worker || this.generation !== generation || this.failed) return;
      await this.audio.pause();
      if (this.worker === worker && this.generation === generation && !this.failed)
        this.post({ type: "pause" });
    } catch (error) {
      if (this.worker === worker && this.generation === generation && !this.failed) this.fail(error);
      throw error;
    }
  }
  async resume() {
    if (this.state !== "paused") return;
    const worker = this.worker,
      generation = this.generation;
    await this.audio.resume();
    if (
      this.worker !== worker ||
      this.generation !== generation ||
      this.failed ||
      this.state !== "paused"
    )
      return;
    this.post({ type: "resume" });
  }
  fail(error) {
    if (this.failed) return;
    this.failed = true;
    this.cancelAudioHold(error);
    this.capture?.observer.event("error", {
      message: error.message,
      stack: String(error.stack ?? "").slice(0, 4096),
    });
    this.input.disable();
    this.onDiagnostic(error.message, error.stack);
    this.status("error", error.message);
    this.post({ type: "pause" });
    this.post({ type: "observability-finish", outcome: "failed", error: error.message });
    this.finishCapture("failed", error);
  }
  finishCapture(outcome, error, capture = this.capture, graphicsQueue = this.graphicsQueue) {
    return Promise.resolve(graphicsQueue)
      .catch(() => {})
      .then(() => capture?.finish(outcome, error))
      .catch((problem) => this.onDiagnostic(`capture output: ${problem.message}`));
  }
  stop() {
    if (this.stopping) return this.stopping;
    this.stopping = this.stopCurrent().finally(() => {
      this.stopping = null;
    });
    return this.stopping;
  }
  async stopCurrent() {
    const generation = ++this.generation,
      worker = this.worker,
      capture = this.capture ?? null,
      graphics = this.graphics;
    this.cancelAudioHold(new Error('session stopped'));
    let graphicsQueue = this.graphicsQueue;
    const outcome = this.failed ? "failed" : "cancelled";
    this.input.disable();
    this.dialogs.close();
    if (worker) {
      await new Promise((resolve, reject) => {
        const timeout = setTimeout(() => {
          this.stopped = null;
          reject(new Error('runtime stop acknowledgement timed out'));
        }, 10000);
        this.stopped = () => {
          clearTimeout(timeout);
          resolve();
        };
        worker.postMessage({ type: "stop" });
      });
      if (this.generation === generation && this.worker === worker)
        graphicsQueue = this.graphicsQueue;
    }
    let drainError;
    try {
      await graphicsQueue.catch(() => {});
      await graphics?.drain();
    } catch (error) {
      drainError = error;
      capture?.capture.markIncomplete(`graphics drain failed: ${error.message}`);
    }
    await this.finishCapture(drainError ? "failed" : outcome, drainError, capture, graphicsQueue);
    worker?.terminate();
    if (this.generation !== generation || this.worker !== worker) return;
    this.stopped = null;
    this.worker = null;
    await this.audio.close();
    await this.graphicsQueue.catch(() => {});
    this.graphics?.close();
    this.graphics = null;
    this.graphicsQueue = Promise.resolve();
    this.windows.reset();
    this.gdi.clear();
    this.failed = false;
    if (drainError) throw drainError;
    this.status("stopped", "session stopped");
  }
  async dispose() {
    ++this.generation;
    this.cancelAudioHold(new Error('session disposed'));
    const worker = this.worker,
      capture = this.capture ?? null,
      graphicsQueue = this.graphicsQueue;
    if (capture?.workerPending)
      capture.capture.markIncomplete('session disposed before Worker capture completed');
    this.worker = null;
    worker?.terminate();
    try {
      this.input.close();
      this.dialogs.close();
      await this.audio.close();
      await graphicsQueue.catch(() => {});
      this.graphics?.close();
      this.graphics = null;
      this.windows.reset();
      this.gdi.clear();
      this.status('disposed', 'session disposed');
    } finally {
      await this.finishCapture('cancelled', undefined, capture, graphicsQueue);
    }
  }
}
