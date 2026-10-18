import { join } from "../perfetto/writer.js";
import type { TraceOutput } from "./types.js";

export class BatchWriter {
  readonly #output: TraceOutput;
  readonly #limit: number;
  readonly #batchBytes: number;
  #chunks: Uint8Array[] = [];
  #queuedBytes = 0;
  #bufferedBytes = 0;
  #writtenBytes = 0;
  #pending: Promise<void> | undefined;
  #error: string | undefined;

  constructor(output: TraceOutput, limit: number, batchBytes: number) {
    this.#output = output;
    this.#limit = limit;
    this.#batchBytes = batchBytes;
  }

  get bufferedBytes(): number {
    return this.#bufferedBytes;
  }
  get writtenBytes(): number {
    return this.#writtenBytes;
  }
  get error(): string | undefined {
    return this.#error;
  }

  append(data: Uint8Array): boolean {
    if (this.#error !== undefined || data.byteLength > this.#limit - this.#bufferedBytes)
      return false;
    this.#chunks.push(data);
    this.#queuedBytes += data.byteLength;
    this.#bufferedBytes += data.byteLength;
    if (this.#queuedBytes >= this.#batchBytes) this.#start();
    return true;
  }

  async flush(): Promise<void> {
    for (;;) {
      this.#start();
      const pending = this.#pending;
      if (pending === undefined) break;
      await pending;
    }
    if (this.#error === undefined) {
      try {
        await this.#output.flush();
      } catch (error) {
        this.#fail(error);
      }
    }
  }

  async close(complete = true, reason = "trace recording is incomplete"): Promise<void> {
    await this.flush();
    if (this.#error === undefined && complete) {
      try {
        await this.#output.close?.();
      } catch (error) {
        this.#fail(error);
      }
    }
    if (this.#error !== undefined || !complete) {
      try {
        await this.#output.abort?.(this.#error ?? reason);
      } catch (error) {
        this.#fail(error);
      }
    }
  }

  #fail(error: unknown): void {
    if (this.#error === undefined) {
      try {
        this.#error = error instanceof Error ? error.message : String(error);
      } catch {
        this.#error = "unprintable output error";
      }
    }
    this.#bufferedBytes -= this.#queuedBytes;
    this.#queuedBytes = 0;
    this.#chunks = [];
  }

  #start(): void {
    if (this.#pending !== undefined || this.#queuedBytes === 0 || this.#error !== undefined) return;
    this.#pending = this.#write().finally(() => {
      this.#pending = undefined;
    });
  }

  async #write(): Promise<void> {
    while (this.#chunks.length > 0 && this.#error === undefined) {
      const chunks = this.#chunks;
      const size = this.#queuedBytes;
      this.#chunks = [];
      this.#queuedBytes = 0;
      try {
        await this.#output.write(join(chunks));
        this.#writtenBytes += size;
      } catch (error) {
        this.#fail(error);
      } finally {
        this.#bufferedBytes -= size;
      }
    }
  }
}
