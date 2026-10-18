import type { Category } from "../catalog/types.js";

export type ObservationLevel = "off" | "basic" | "detailed";
export type CaptureOutcome = "complete" | "cancelled" | "failed";
export type TraceValue = string | number | bigint | boolean;
export type TraceArgs = Readonly<Record<string, TraceValue>>;

export interface TraceOutput {
  write(bytes: Uint8Array): Promise<void>;
  flush(): Promise<void>;
  close?(): Promise<void>;
  abort?(reason?: string): Promise<void>;
}

export interface CaptureOptions {
  readonly level: ObservationLevel;
  readonly output: TraceOutput;
  readonly categories?: readonly Category[] | undefined;
  readonly clock?: () => bigint;
  readonly producerId?: number;
  readonly maxBufferedBytes?: number;
  readonly batchBytes?: number;
}

export interface CaptureStatus {
  readonly level: ObservationLevel;
  readonly state: "recording" | "finishing" | "finished";
  readonly producerId: number;
  readonly droppedEvents: number;
  readonly droppedChunks: number;
  readonly writtenBytes: number;
  readonly bufferedBytes: number;
  readonly pendingPacketBytes: number;
  readonly pendingExportBytes: number;
  readonly queuedInputBytes: number;
  readonly openScopes: number;
  readonly engine: "off" | "host-only" | "recording" | "finished" | "unsupported";
  readonly sinkError?: string;
  readonly recordingError?: string;
}

export interface CaptureResult extends CaptureStatus {
  readonly outcome: CaptureOutcome;
  readonly complete: boolean;
}
