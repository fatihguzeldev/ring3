import type { IncomingMessage, ServerResponse } from "node:http";
import { CaptureOutputError, requireCaptureId, requireSequence } from "./types.js";
import type { CaptureStore } from "./types.js";

export interface TraceHttpHandlerOptions {
  readonly allowedOrigins: readonly string[];
  readonly prefix?: string;
}

export function createTraceHttpHandler(
  store: CaptureStore,
  options: TraceHttpHandlerOptions,
): (request: IncomingMessage, response: ServerResponse) => Promise<boolean> {
  const prefix = options.prefix ?? "/api/observability/captures";
  const origins = new Set(options.allowedOrigins.map((origin) => new URL(origin).origin));
  if (!prefix.startsWith("/") || prefix.endsWith("/") || origins.size === 0)
    throw new CaptureOutputError("invalid HTTP output configuration");

  function reply(response: ServerResponse, status: number, value: unknown): void {
    response.writeHead(status, {
      "content-type": "application/json",
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
    });
    response.end(JSON.stringify(value));
  }

  async function body(request: IncomingMessage, limit: number): Promise<Uint8Array> {
    const declared = request.headers["content-length"];
    if (declared !== undefined && (!/^\d+$/.test(declared) || Number(declared) > limit))
      throw new CaptureOutputError("request body exceeds output limit", 413);
    const parts: Uint8Array[] = [];
    let size = 0;
    for await (const part of request) {
      const bytes =
        typeof part === "string" ? new TextEncoder().encode(part) : (part as Uint8Array);
      size += bytes.length;
      if (size > limit) throw new CaptureOutputError("request body exceeds output limit", 413);
      parts.push(bytes);
    }
    const result = new Uint8Array(size);
    let offset = 0;
    for (const part of parts) {
      result.set(part, offset);
      offset += part.length;
    }
    return result;
  }

  async function json(request: IncomingMessage): Promise<Record<string, unknown>> {
    if (request.headers["content-type"] !== "application/json")
      throw new CaptureOutputError("expected JSON request", 415);
    let value: unknown;
    try {
      value = JSON.parse(new TextDecoder().decode(await body(request, 66 * 1024)));
    } catch (error) {
      if (error instanceof CaptureOutputError) throw error;
      throw new CaptureOutputError("invalid JSON request");
    }
    if (value === null || typeof value !== "object" || Array.isArray(value))
      throw new CaptureOutputError("invalid request object");
    return value as Record<string, unknown>;
  }

  return async (request, response) => {
    const rawPath = (request.url ?? "").split("?")[0] ?? "";
    if (rawPath !== prefix && !rawPath.startsWith(`${prefix}/`)) return false;
    try {
      const origin = request.headers.origin;
      if (
        origin === undefined ||
        !origins.has(origin) ||
        new URL(origin).host !== request.headers.host ||
        request.headers["sec-fetch-site"] === "cross-site"
      ) {
        throw new CaptureOutputError("capture origin rejected", 403);
      }
      if (request.method !== "POST") throw new CaptureOutputError("capture method rejected", 405);
      if (rawPath === prefix) {
        const value = await json(request);
        if (
          value.metadata === null ||
          typeof value.metadata !== "object" ||
          Array.isArray(value.metadata) ||
          typeof value.requestId !== "string"
        )
          throw new CaptureOutputError("invalid capture metadata");
        reply(
          response,
          201,
          await store.begin(value.metadata as Record<string, unknown>, value.requestId),
        );
        return true;
      }
      const suffix = rawPath.slice(prefix.length + 1).split("/");
      const id = suffix[0] ?? "";
      requireCaptureId(id);
      const action = suffix[1];
      if (suffix.length !== 2 || !["chunks", "flush", "finish", "abort"].includes(action ?? ""))
        throw new CaptureOutputError("capture route rejected", 404);
      if (action === "chunks") {
        if (request.headers["content-type"] !== "application/octet-stream")
          throw new CaptureOutputError("expected trace bytes", 415);
        const sequenceHeader = request.headers["x-trace-sequence"];
        if (typeof sequenceHeader !== "string" || !/^\d+$/.test(sequenceHeader))
          throw new CaptureOutputError("invalid sequence");
        const sequence = Number(sequenceHeader);
        requireSequence(sequence);
        reply(
          response,
          200,
          await store.append(id, sequence, await body(request, store.maxChunkBytes)),
        );
      } else {
        const value = await json(request);
        if (action === "abort") {
          if (value.reason !== undefined && typeof value.reason !== "string")
            throw new CaptureOutputError("invalid abort reason");
          reply(response, 200, await store.abort(id, value.reason as string | undefined));
        } else {
          if (typeof value.sequence !== "number") throw new CaptureOutputError("invalid sequence");
          requireSequence(value.sequence);
          if (action === "finish") reply(response, 200, await store.finish(id, value.sequence));
          else {
            await store.flush(id, value.sequence);
            reply(response, 200, { nextSequence: value.sequence });
          }
        }
      }
    } catch (error) {
      reply(response, error instanceof CaptureOutputError ? error.status : 500, {
        error: error instanceof CaptureOutputError ? error.message : "trace output failed",
      });
    }
    return true;
  };
}
