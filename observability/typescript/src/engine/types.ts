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
  configure(configuration: EngineTraceConfiguration): void;
  drain(): EngineTraceChunk;
  finish(): EngineTraceChunk;
}

export interface EngineAttachmentOptions {
  readonly producerId: number;
  readonly maxBytes?: number;
}
