export type RuntimePhase = 'ready' | 'loading' | 'running' | 'paused' | 'stopped' | 'error';
export interface RuntimeState {
  readonly phase: RuntimePhase;
  readonly connected: boolean;
  readonly sequence?: number;
  readonly error?: string | { readonly code?: string; readonly message?: string };
  readonly [fact: string]: unknown;
}
export interface BrowserRuntime {
  readonly state: RuntimeState;
  subscribe(listener: (state: RuntimeState) => void): () => void;
  launchOwnedWgb(url: string, options?: { args?: string; preload?: boolean }): Promise<RuntimeState>;
  pause(): Promise<RuntimeState>;
  resume(): Promise<RuntimeState>;
  status(): Promise<unknown>;
  focus(): void;
  stop(): Promise<{ graceful: true; alreadyClosed?: boolean }>;
  dispose(): void;
}
export class RuntimeError extends Error { readonly code: string; constructor(code: string, message: string); }
export function createBrowserRuntime(options: {
  container: HTMLElement;
  runtimeUrl?: string | URL;
  timeoutMs?: number;
  launchTimeoutMs?: number;
}): Promise<BrowserRuntime>;
