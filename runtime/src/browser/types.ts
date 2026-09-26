export interface GuestFile {
  path: string;
  size: number;
  role: "executable" | "module" | "deferred" | "data";
}

export interface Manifest {
  files: GuestFile[];
}

export interface GuestWindow {
  hwnd: number;
  parent: number;
  id: number;
  class: number;
  style: number;
  title: string;
  active: boolean;
}

export interface Snapshot {
  state: "ready" | "running" | "waiting" | "file" | "exited" | "stopped";
  reason: string;
  eip: number;
  instructions: string;
  apiCalls: string;
  pending: { id: string; path: string; size: number } | null;
  windows: GuestWindow[];
  frame: { width: number; height: number } | null;
}

export type WorkerInput =
  | { type: "start"; token: string }
  | { type: "pause" | "resume" }
  | { type: "button" | "activate"; hwnd: number }
  | { type: "mouse"; relativeX: number; relativeY: number; buttons: number };

export type WorkerOutput =
  | { type: "status"; snapshot: Snapshot; paused: boolean; note: string }
  | { type: "frame"; width: number; height: number; rgba: ArrayBuffer }
  | { type: "error"; message: string };
