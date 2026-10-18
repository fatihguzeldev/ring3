import type { ObservationLevel } from "../capture/types.js";
import type { Category } from "../catalog/types.js";

export interface EngineTraceConfiguration {
  readonly level: ObservationLevel;
  readonly maxBytes: number;
  readonly sequenceId: number;
  readonly trackUuid: bigint;
  readonly categories: readonly Category[] | undefined;
}

export interface EngineTraceChunk {
  readonly bytes: Uint8Array;
  readonly finished: boolean;
  readonly droppedEvents: number | bigint;
  readonly openScopes?: number | bigint;
}

export interface EngineObservationPort {
  readonly supported: boolean;
  readonly maxChunkBytes?: number;
  configure(configuration: EngineTraceConfiguration): void;
  drain(): EngineTraceChunk;
  finish(): EngineTraceChunk;
}

export interface EngineAttachmentOptions {
  readonly producerId: number;
  readonly maxBytes?: number;
}

export interface EngineDrainOptions {
  readonly maxChunks?: number;
  readonly maxBytes?: number;
  readonly budgetMs?: number;
}

export interface EngineDrainResult {
  readonly chunks: number;
  readonly bytes: number;
  readonly pending: boolean;
  readonly blocked: boolean;
}
