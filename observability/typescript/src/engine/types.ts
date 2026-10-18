import type { ObservationLevel } from "../capture/types.js";

export interface EngineTraceConfiguration {
  readonly level: ObservationLevel;
  readonly maxBytes: number;
  readonly sequenceId: number;
  readonly trackUuid: bigint;
  readonly contexts: readonly string[];
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
