import type { CaptureOptions, ObservationLevel } from "./types.js";

export interface CaptureConfiguration {
  readonly level: ObservationLevel;
  readonly contexts: readonly string[];
  readonly maxBufferedBytes: number;
  readonly batchBytes: number;
}

export function configureCapture(options: CaptureOptions): CaptureConfiguration {
  if (!["off", "basic", "detailed"].includes(options.level))
    throw new TypeError("invalid observation level");
  const contexts = [...(options.contexts ?? [])];
  if (contexts.length > 64) throw new RangeError("at most 64 context prefixes are supported");
  for (const context of contexts) validateContext(context);
  const maxBufferedBytes = options.maxBufferedBytes ?? 1024 * 1024;
  const batchBytes = options.batchBytes ?? Math.min(64 * 1024, maxBufferedBytes);
  if (
    !Number.isSafeInteger(maxBufferedBytes) ||
    maxBufferedBytes < 1024 ||
    maxBufferedBytes > 64 * 1024 * 1024
  ) {
    throw new RangeError("maxBufferedBytes must be between 1024 and 67108864");
  }
  if (!Number.isSafeInteger(batchBytes) || batchBytes < 1 || batchBytes > maxBufferedBytes) {
    throw new RangeError("batchBytes must fit within maxBufferedBytes");
  }
  return { level: options.level, contexts, maxBufferedBytes, batchBytes };
}

export function validateContext(context: string): void {
  if (context.length > 128 || !/^[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*)*$/.test(context)) {
    throw new TypeError(`invalid observation context: ${context}`);
  }
}

export function matchesContext(context: string, prefixes: readonly string[]): boolean {
  return (
    prefixes.length === 0 ||
    prefixes.some((prefix) => context === prefix || context.startsWith(`${prefix}.`))
  );
}

export function createEpochClock(): () => bigint {
  const origin = BigInt(Math.floor(performance.timeOrigin * 1_000_000));
  return () => origin + BigInt(Math.floor(performance.now() * 1_000_000));
}

const defaultClock: () => bigint = createEpochClock();

export function epochNowNs(): bigint {
  return defaultClock();
}
