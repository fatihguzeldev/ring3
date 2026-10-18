import { CaptureOutputError, requireCaptureId } from "./types.js";
import type { CaptureManifest, PersistentTraceOutput } from "./types.js";

export interface HttpTraceOutputOptions {
  readonly baseUrl?: string;
  readonly metadata: Readonly<Record<string, unknown>>;
  readonly batchBytes?: number;
  readonly maxChunkBytes?: number;
  readonly maxPendingBytes?: number;
  readonly retries?: number;
  readonly requestTimeoutMs?: number;
  readonly fetch?: typeof globalThis.fetch;
}

export type HttpTraceOutput = PersistentTraceOutput;

export async function createHttpTraceOutput(
  options: HttpTraceOutputOptions,
): Promise<HttpTraceOutput> {
  const baseUrl = options.baseUrl ?? "/api/observability/captures";
  const batchBytes = options.batchBytes ?? 256 * 1024;
  const maxChunkBytes = options.maxChunkBytes ?? 1024 * 1024;
  const maxPendingBytes = options.maxPendingBytes ?? 4 * 1024 * 1024;
  const retries = options.retries ?? 2;
  const timeout = options.requestTimeoutMs ?? 15_000;
  for (const limit of [batchBytes, maxChunkBytes, maxPendingBytes, timeout])
    if (!Number.isSafeInteger(limit) || limit <= 0)
      throw new CaptureOutputError("invalid HTTP output limit");
  if (batchBytes > maxChunkBytes || !Number.isSafeInteger(retries) || retries < 0 || retries > 8)
    throw new CaptureOutputError("invalid HTTP output configuration");
  if (typeof location !== "undefined" && new URL(baseUrl, location.href).origin !== location.origin)
    throw new CaptureOutputError("trace output must be same origin");
  const fetcher = options.fetch ?? globalThis.fetch;
  const requestId = globalThis.crypto.randomUUID();
  let sequence = 0;
  let parts: Uint8Array[] = [];
  let buffered = 0;
  let state: "recording" | "complete" | "aborted" | "failed" = "recording";
  let completion: CaptureManifest | undefined;
  let pending = Promise.resolve();
  let pendingBytes = 0;

  async function request(
    path: string,
    data: string | Uint8Array,
    contentType: string,
    chunkSequence?: number,
  ): Promise<Record<string, unknown>> {
    for (let attempt = 0; ; attempt += 1) {
      try {
        const headers: Record<string, string> = { "content-type": contentType };
        if (chunkSequence !== undefined) headers["x-trace-sequence"] = String(chunkSequence);
        const response = await fetcher(`${baseUrl}${path}`, {
          method: "POST",
          headers,
          body: data as BodyInit,
          credentials: "same-origin",
          redirect: "error",
          signal: AbortSignal.timeout(timeout),
        });
        if (!response.ok) {
          const error = new CaptureOutputError(
            `trace output HTTP ${response.status}`,
            response.status,
          );
          if (response.status < 500 || response.status === 507) throw error;
          if (attempt >= retries) throw error;
        } else {
          const result = (await response.json()) as unknown;
          if (!result || typeof result !== "object" || Array.isArray(result))
            throw new CaptureOutputError("invalid trace acknowledgement", 502);
          return result as Record<string, unknown>;
        }
      } catch (error) {
        if (error instanceof CaptureOutputError || attempt >= retries) throw error;
      }
      await new Promise((resolve) => setTimeout(resolve, 25 * 2 ** attempt));
    }
  }

  const opened = await request(
    "",
    JSON.stringify({ requestId, metadata: options.metadata }),
    "application/json",
  );
  if (typeof opened.id !== "string")
    throw new CaptureOutputError("invalid capture acknowledgement", 502);
  const id = opened.id;
  requireCaptureId(id);

  function ordered(operation: () => Promise<void>): Promise<void> {
    const result = pending.then(operation);
    pending = result.then(
      () => undefined,
      () => {
        state = "failed";
      },
    );
    return result;
  }

  async function drain(): Promise<void> {
    if (buffered === 0) return;
    const bytes = new Uint8Array(buffered);
    let offset = 0;
    for (const part of parts) {
      bytes.set(part, offset);
      offset += part.length;
    }
    const acknowledgement = await request(
      `/${id}/chunks`,
      bytes,
      "application/octet-stream",
      sequence,
    );
    if (acknowledgement.nextSequence !== sequence + 1)
      throw new CaptureOutputError("invalid sequence acknowledgement", 502);
    sequence += 1;
    parts = [];
    buffered = 0;
  }

  return {
    id,
    get completion() {
      return completion;
    },
    write: (bytes) => {
      if (!(bytes instanceof Uint8Array))
        return Promise.reject(new CaptureOutputError("invalid trace bytes"));
      if (pendingBytes + bytes.length > maxPendingBytes)
        return Promise.reject(new CaptureOutputError("trace pending byte limit reached", 507));
      const owned = bytes.slice();
      pendingBytes += owned.length;
      return ordered(async () => {
        try {
          if (state !== "recording") throw new CaptureOutputError("trace output is closed", 409);
          for (let offset = 0; offset < owned.length;) {
            const count = Math.min(owned.length - offset, maxChunkBytes - buffered);
            parts.push(owned.subarray(offset, offset + count));
            buffered += count;
            offset += count;
            if (buffered >= batchBytes) await drain();
          }
        } finally {
          pendingBytes -= owned.length;
        }
      });
    },
    flush: () =>
      ordered(async () => {
        if (state !== "recording") throw new CaptureOutputError("trace output is closed", 409);
        await drain();
        const result = await request(
          `/${id}/flush`,
          JSON.stringify({ sequence }),
          "application/json",
        );
        if (result.nextSequence !== sequence)
          throw new CaptureOutputError("invalid flush acknowledgement", 502);
      }),
    close: () =>
      ordered(async () => {
        if (state === "complete") return;
        if (state !== "recording") throw new CaptureOutputError("trace output is closed", 409);
        await drain();
        const result = await request(
          `/${id}/finish`,
          JSON.stringify({ sequence }),
          "application/json",
        );
        if (result.id !== id || result.complete !== true || result.nextSequence !== sequence)
          throw new CaptureOutputError("invalid finish acknowledgement", 502);
        completion = result as unknown as CaptureManifest;
        state = "complete";
      }),
    abort: (reason = "capture aborted") =>
      ordered(async () => {
        if (state === "complete" || state === "aborted") return;
        parts = [];
        buffered = 0;
        await request(`/${id}/abort`, JSON.stringify({ reason }), "application/json");
        state = "aborted";
      }),
  };
}
