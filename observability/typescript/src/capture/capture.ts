import type { Category } from "../catalog/types.js";
import { requireCategory, matchesCategory } from "../catalog/selection.js";
import { categories } from "../catalog/generated.js";
import type {
  EngineAttachmentOptions,
  EngineObservationPort,
  EngineTraceChunk,
} from "../engine/types.js";
import { clockSnapshot, counterEvent, descriptor, event } from "../perfetto/events.js";
import { TracePacketStream } from "../perfetto/stream.js";
import { isIdentityClockSnapshot } from "../perfetto/clocks.js";
import { BatchWriter } from "./buffer.js";
import { configureCapture, epochNowNs } from "./configuration.js";
import type { CaptureConfiguration } from "./configuration.js";
import { Observer } from "./observer.js";
import type { ScopeToken } from "./observer.js";
import type {
  CaptureOptions,
  CaptureOutcome,
  CaptureResult,
  CaptureStatus,
  ObservationLevel,
  TraceArgs,
} from "./types.js";

let nextProducerId = 1;
const activeProducers = new Set<number>();
const scopeEndReserve = 2048;

function producerId(requested?: number): number {
  if (requested === undefined) {
    while (activeProducers.has(nextProducerId)) nextProducerId++;
  }
  const value = requested ?? nextProducerId++;
  if (!Number.isInteger(value) || value < 1 || value > 0xffff_ffff)
    throw new RangeError("producerId must be a nonzero uint32");
  if (activeProducers.has(value)) throw new Error(`trace producer ${value} is already active`);
  activeProducers.add(value);
  return value;
}

function message(error: unknown): string {
  try {
    return (error instanceof Error ? error.message : String(error)).slice(0, 512);
  } catch {
    return "unprintable error";
  }
}

export class Capture {
  readonly #configuration: CaptureConfiguration;
  readonly #writer: BatchWriter;
  readonly #externalStream: TracePacketStream;
  readonly #engineStream: TracePacketStream;
  readonly #clock: () => bigint;
  readonly #producerId: number;
  readonly #base: bigint;
  readonly #scopes = new Set<ScopeToken>();
  readonly #counters = new Map<string, bigint>();
  readonly #freeAsyncTracks: bigint[] = [];
  readonly #knownTracks = new Set<bigint>();
  #nextTrack = 2;
  #reservedBytes = 0;
  #lastTimestamp = 0n;
  #clockDeclared = false;
  #droppedEvents = 0;
  #droppedChunks = 0;
  #engineDrops = 0;
  #recordingError: string | undefined;
  #state: CaptureStatus["state"] = "recording";
  #engineState: CaptureStatus["engine"];
  #engine: EngineObservationPort | undefined;
  #engineProducerId: number | undefined;
  #finishing: Promise<CaptureResult> | undefined;

  constructor(options: CaptureOptions) {
    this.#configuration = configureCapture(options);
    this.#producerId = producerId(options.producerId);
    this.#base = BigInt(this.#producerId) << 32n;
    this.#clock = options.clock ?? epochNowNs;
    this.#writer = new BatchWriter(
      options.output,
      this.#configuration.maxBufferedBytes,
      this.#configuration.batchBytes,
    );
    this.#externalStream = new TracePacketStream(this.#configuration.maxBufferedBytes);
    this.#engineStream = new TracePacketStream(this.#configuration.maxBufferedBytes);
    this.#engineState = options.level === "off" ? "off" : "host-only";
  }

  observer(category: Category, level: ObservationLevel = "basic"): Observer {
    requireCategory(category);
    return new Observer(this, category, level);
  }

  enabled(category: Category, level: ObservationLevel): boolean {
    requireCategory(category);
    return (
      this.#state === "recording" &&
      this.#configuration.level !== "off" &&
      level !== "off" &&
      (level !== "detailed" || this.#configuration.level === "detailed") &&
      matchesCategory(category, this.#configuration.categories)
    );
  }

  nowNs(): bigint {
    return this.#clock();
  }

  markIncomplete(reason: string): void {
    this.#recordingError ??= reason.slice(0, 512);
  }

  status(): CaptureStatus {
    const status: CaptureStatus = {
      level: this.#configuration.level,
      state: this.#state,
      producerId: this.#producerId,
      droppedEvents: this.#droppedEvents + this.#engineDrops,
      droppedChunks: this.#droppedChunks,
      writtenBytes: this.#writer.writtenBytes,
      bufferedBytes:
        this.#writer.bufferedBytes +
        this.#reservedBytes +
        this.#externalStream.pendingBytes +
        this.#engineStream.pendingBytes,
      pendingPacketBytes: this.#externalStream.pendingBytes + this.#engineStream.pendingBytes,
      openScopes: this.#scopes.size,
      engine: this.#engineState,
      ...(this.#writer.error === undefined ? {} : { sinkError: this.#writer.error }),
      ...(this.#recordingError === undefined ? {} : { recordingError: this.#recordingError }),
    };
    return status;
  }

  ingest(bytes: Uint8Array): boolean {
    if (this.#state !== "recording" || this.#configuration.level === "off") return false;
    if (bytes.byteLength === 0) return true;
    return this.#ingest(this.#externalStream, bytes);
  }

  attachEngine(port: EngineObservationPort, options: EngineAttachmentOptions): void {
    if (this.#state !== "recording" || this.#engine !== undefined)
      throw new Error("engine observation can only attach once while recording");
    if (this.#configuration.level === "off") return;
    if (!port.supported) {
      this.#engineState = "unsupported";
      this.recordEvent(categories.observability, "engine.unsupported", {
        level: this.#configuration.level,
      });
      if (this.#configuration.level === "detailed") {
        const reason = "detailed engine observation requires an observability-enabled engine build";
        this.markIncomplete(reason);
        throw new Error(reason);
      }
      return;
    }
    if (options.producerId === this.#producerId)
      throw new Error("engine and host producers require distinct identities");
    const maxBytes = options.maxBytes ?? 256 * 1024;
    if (!Number.isSafeInteger(maxBytes) || maxBytes < 1024 || maxBytes > 64 * 1024 * 1024)
      throw new RangeError("invalid engine trace byte budget");
    const engineProducer = producerId(options.producerId);
    try {
      port.configure({
        level: this.#configuration.level,
        maxBytes,
        sequenceId: engineProducer,
        trackUuid: (BigInt(engineProducer) << 32n) | 1n,
        categories: this.#configuration.categories,
      });
    } catch (error) {
      activeProducers.delete(engineProducer);
      this.markIncomplete(`engine observation configure: ${message(error)}`);
      throw error;
    }
    this.#engineProducerId = engineProducer;
    this.#engine = port;
    this.#engineState = "recording";
  }

  drainEngine(): void {
    if (this.#engine === undefined || this.#engineState !== "recording") return;
    try {
      this.#consumeEngine(this.#engine.drain());
    } catch (error) {
      this.#recordingError ??= `engine observation: ${message(error)}`;
    }
  }

  recordEvent(category: Category, name: string, args: TraceArgs = {}, track?: bigint): void {
    requireCategory(category);
    if (this.#state !== "recording" || this.#configuration.level === "off") return;
    this.#safe(() => {
      const target = track ?? this.#hostTrack();
      if (target !== undefined)
        this.#append(event(this.#producerId, this.#time(), target, 3, category, name, args));
    });
  }

  recordCounter(category: Category, name: string, value: number | bigint): void {
    requireCategory(category);
    if (this.#state !== "recording" || this.#configuration.level === "off") return;
    this.#safe(() => {
      const key = `${category}:${name}`;
      let track = this.#counters.get(key);
      if (track === undefined) {
        if (this.#counters.size >= 1024) {
          this.#droppedEvents++;
          return;
        }
        const host = this.#hostTrack();
        if (host === undefined) return;
        track = this.#allocateTrack();
        if (!this.#append(descriptor(this.#producerId, this.#time(), track, key, host, true)))
          return;
        this.#counters.set(key, track);
      }
      this.#append(counterEvent(this.#producerId, this.#time(), track, value));
    });
  }

  beginScope(
    category: Category,
    name: string,
    args: TraceArgs,
    existing: bigint | undefined,
    asynchronous: boolean,
  ): ScopeToken | undefined {
    requireCategory(category);
    if (this.#state !== "recording" || this.#configuration.level === "off") return undefined;
    let token: ScopeToken | undefined;
    this.#safe(() => {
      const host = this.#hostTrack();
      if (host === undefined) return;
      let track = existing ?? host;
      let leasedTrack: bigint | undefined;
      try {
        if (asynchronous) {
          track = this.#freeAsyncTracks.pop() ?? this.#allocateTrack();
          leasedTrack = track;
          if (!this.#knownTracks.has(track)) {
            if (
              !this.#append(
                descriptor(this.#producerId, this.#time(), track, "async operation", host),
              )
            )
              return;
            this.#knownTracks.add(track);
          }
        }
        const begin = event(this.#producerId, this.#time(), track, 1, category, name, args);
        this.#reservedBytes += scopeEndReserve;
        if (!this.#append(begin)) {
          this.#reservedBytes -= scopeEndReserve;
          return;
        }
        token = { category, track, asynchronous };
        this.#scopes.add(token);
      } finally {
        if (token === undefined && leasedTrack !== undefined) {
          this.#freeAsyncTracks.push(leasedTrack);
        }
      }
    });
    return token;
  }

  endScope(token: ScopeToken, error?: unknown, failed = false): void {
    if (!this.#scopes.delete(token)) return;
    if (this.#state === "recording") {
      this.#reservedBytes -= scopeEndReserve;
      this.#safe(() => {
        const args = failed ? { error: message(error) } : {};
        this.#append(
          event(this.#producerId, this.#time(), token.track, 2, token.category, "", args),
        );
      });
    }
    if (token.asynchronous) this.#freeAsyncTracks.push(token.track);
  }

  async flush(): Promise<CaptureStatus> {
    if (this.#state === "finished") return this.status();
    if (this.#finishing !== undefined) {
      await this.#finishing;
      return this.status();
    }
    if (this.#configuration.level !== "off") {
      this.drainEngine();
      await this.#writer.flush();
    }
    return this.status();
  }

  finish(outcome: CaptureOutcome = "complete", error?: unknown): Promise<CaptureResult> {
    this.#finishing ??= this.#finish(outcome, error);
    return this.#finishing;
  }

  async #finish(outcome: CaptureOutcome, error?: unknown): Promise<CaptureResult> {
    this.#state = "finishing";
    const unfinishedScopes = this.#scopes.size;
    this.#reservedBytes = 0;
    if (this.#configuration.level !== "off") {
      if (this.#engine !== undefined && this.#engineState === "recording") {
        try {
          let chunk = this.#engine.finish();
          for (let index = 0; index < 65_536; index++) {
            if (
              chunk.bytes.byteLength >
              this.#configuration.maxBufferedBytes -
                this.#writer.bufferedBytes -
                this.#reservedBytes
            ) {
              await this.#writer.flush();
            }
            this.#consumeEngine(chunk);
            if (chunk.finished) break;
            if (chunk.bytes.length === 0) throw new Error("engine trace finish made no progress");
            chunk = this.#engine.drain();
          }
          if (!chunk.finished) throw new Error("engine trace finish did not terminate");
        } catch (failure) {
          this.#recordingError ??= `engine observation: ${message(failure)}`;
        }
      }
      if (this.#externalStream.incomplete)
        this.markIncomplete("external trace stream ended inside a packet");
      if (this.#engineStream.incomplete)
        this.markIncomplete("engine trace stream ended inside a packet");
      await this.#writer.flush();
      this.#safe(() => {
        const host = this.#hostTrack();
        if (host !== undefined)
          this.#append(
            event(
              this.#producerId,
              this.#time(),
              host,
              3,
              categories.observability,
              "capture.finished",
              {
                outcome,
                complete: this.#complete(unfinishedScopes),
                droppedEvents: this.#droppedEvents + this.#engineDrops,
                droppedChunks: this.#droppedChunks,
                openScopes: unfinishedScopes,
                engine: this.#engineState,
                ...(error === undefined ? {} : { error: message(error) }),
                ...(this.#recordingError === undefined
                  ? {}
                  : { recordingError: this.#recordingError }),
              },
            ),
          );
      });
      await this.#writer.close(this.#complete(unfinishedScopes), this.#recordingError);
    }
    this.#state = "finished";
    activeProducers.delete(this.#producerId);
    if (this.#engineProducerId !== undefined) activeProducers.delete(this.#engineProducerId);
    return { ...this.status(), outcome, complete: this.#complete(unfinishedScopes) };
  }

  #consumeEngine(chunk: EngineTraceChunk): void {
    const drops = Number(chunk.droppedEvents);
    if (!Number.isSafeInteger(drops) || drops < this.#engineDrops)
      throw new Error("invalid engine trace drop count");
    this.#engineDrops = drops;
    if (chunk.bytes.length > 0) this.#ingest(this.#engineStream, chunk.bytes);
    if (chunk.finished) {
      this.#engineState = "finished";
      if (chunk.openScopes !== undefined) {
        const openScopes = Number(chunk.openScopes);
        if (!Number.isSafeInteger(openScopes) || openScopes < 0)
          throw new Error("invalid engine open scope count");
        if (openScopes > 0)
          this.markIncomplete(`engine observation ended with ${openScopes} unfinished scopes`);
      }
    }
  }

  #complete(unfinishedScopes: number): boolean {
    return (
      unfinishedScopes === 0 &&
      this.#droppedEvents + this.#engineDrops === 0 &&
      this.#droppedChunks === 0 &&
      !this.#externalStream.incomplete &&
      !this.#engineStream.incomplete &&
      this.#writer.error === undefined &&
      this.#recordingError === undefined &&
      this.#engineState !== "recording"
    );
  }

  #ingest(stream: TracePacketStream, bytes: Uint8Array): boolean {
    try {
      return stream.push(bytes, (packet) => {
        if (isIdentityClockSnapshot(packet)) return true;
        if (this.#hostTrack() === undefined) {
          this.#droppedChunks++;
          return false;
        }
        if (this.#append(packet, false)) return true;
        this.#droppedChunks++;
        return false;
      });
    } catch (error) {
      this.#droppedChunks++;
      this.markIncomplete(`trace framing: ${message(error)}`);
      return false;
    }
  }

  #hostTrack(): bigint | undefined {
    if (!this.#clockDeclared) {
      if (!this.#append(clockSnapshot(this.#producerId, this.#time()))) return undefined;
      this.#clockDeclared = true;
    }
    const track = this.#base | 1n;
    if (!this.#knownTracks.has(track)) {
      if (
        !this.#append(descriptor(this.#producerId, this.#time(), track, `host ${this.#producerId}`))
      )
        return undefined;
      this.#knownTracks.add(track);
    }
    return track;
  }

  #allocateTrack(): bigint {
    if (this.#nextTrack > 0xffff_ffff) throw new Error("trace track namespace exhausted");
    return this.#base | BigInt(this.#nextTrack++);
  }

  #time(): bigint {
    const timestamp = this.#clock();
    this.#lastTimestamp = timestamp > this.#lastTimestamp ? timestamp : this.#lastTimestamp;
    return this.#lastTimestamp;
  }

  #append(bytes: Uint8Array, countEvent = true): boolean {
    if (
      bytes.byteLength >
        this.#configuration.maxBufferedBytes - this.#writer.bufferedBytes - this.#reservedBytes ||
      !this.#writer.append(bytes)
    ) {
      if (countEvent) this.#droppedEvents++;
      return false;
    }
    return true;
  }

  #safe(work: () => void): void {
    try {
      work();
    } catch (error) {
      this.#droppedEvents++;
      this.#recordingError ??= message(error);
    }
  }
}

export function createCapture(options: CaptureOptions): Capture {
  return new Capture(options);
}
