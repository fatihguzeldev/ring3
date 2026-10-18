import type { EngineDrainOptions } from "./types.js";

export interface DrainBudget {
  readonly maxChunks: number;
  readonly maxBytes: number;
  readonly budgetMs: number;
}

export function drainBudget(options: EngineDrainOptions = {}): DrainBudget {
  const maxChunks = options.maxChunks ?? 64;
  const maxBytes = options.maxBytes ?? 256 * 1024;
  const budgetMs = options.budgetMs ?? 2;
  if (!Number.isSafeInteger(maxChunks) || maxChunks < 1 || maxChunks > 65_536)
    throw new RangeError("maxChunks must be between 1 and 65536");
  if (!Number.isSafeInteger(maxBytes) || maxBytes < 1 || maxBytes > 64 * 1024 * 1024)
    throw new RangeError("maxBytes must be between 1 and 67108864");
  if (!Number.isFinite(budgetMs) || budgetMs <= 0 || budgetMs > 1000)
    throw new RangeError("budgetMs must be greater than zero and at most 1000");
  return { maxChunks, maxBytes, budgetMs };
}

export const yieldExportTask = (): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, 0));
