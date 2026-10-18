import type { CaptureOptions, ObservationLevel } from "./types.js";
import { resolveCategorySelection } from "../catalog/selection.js";
import type { Category } from "../catalog/types.js";

export interface CaptureConfiguration {
  readonly level: ObservationLevel;
  readonly categories: readonly Category[] | undefined;
  readonly maxBufferedBytes: number;
  readonly batchBytes: number;
}

export function configureCapture(options: CaptureOptions): CaptureConfiguration {
  if (!["off", "basic", "detailed"].includes(options.level))
    throw new TypeError("invalid observation level");
  if (Object.hasOwn(options, "contexts"))
    throw new TypeError("use catalog category selections instead of contexts");
  const categories = resolveCategorySelection(options.categories);
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
  return { level: options.level, categories, maxBufferedBytes, batchBytes };
}

export function createEpochClock(): () => bigint {
  const origin = BigInt(Math.floor(performance.timeOrigin * 1_000_000));
  return () => origin + BigInt(Math.floor(performance.now() * 1_000_000));
}

const defaultClock: () => bigint = createEpochClock();

export function epochNowNs(): bigint {
  return defaultClock();
}
