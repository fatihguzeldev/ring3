import type { TraceOutput } from "../capture/types.js";

export interface PersistentTraceOutput extends TraceOutput {
  readonly id: string;
  readonly completion: CaptureManifest | undefined;
  abort(reason?: string): Promise<void>;
}

export interface CaptureManifest {
  readonly version: 1;
  readonly id: string;
  readonly createdAt: string;
  readonly updatedAt: string;
  readonly complete: boolean;
  readonly status: "recording" | "complete" | "aborted";
  readonly bytes: number;
  readonly nextSequence: number;
  readonly metadata: Readonly<Record<string, unknown>>;
  readonly reason?: string;
}

export interface CaptureStore {
  readonly maxChunkBytes: number;
  begin(metadata: Readonly<Record<string, unknown>>, requestId?: string): Promise<{ id: string }>;
  append(id: string, sequence: number, bytes: Uint8Array): Promise<{ nextSequence: number }>;
  flush(id: string, sequence: number): Promise<void>;
  finish(id: string, sequence: number): Promise<CaptureManifest>;
  abort(id: string, reason?: string): Promise<CaptureManifest>;
  get(id: string): Promise<CaptureManifest>;
}

export class CaptureOutputError extends Error {
  readonly status: number;

  constructor(message: string, status = 400) {
    super(message);
    this.name = "CaptureOutputError";
    this.status = status;
  }
}

export const CAPTURE_ID_PATTERN: RegExp =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

export function requireCaptureId(id: string): void {
  if (!CAPTURE_ID_PATTERN.test(id)) throw new CaptureOutputError("invalid capture identifier");
}

export function requireSequence(sequence: number): void {
  if (!Number.isSafeInteger(sequence) || sequence < 0)
    throw new CaptureOutputError("invalid sequence");
}
