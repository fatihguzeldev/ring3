import type { ObservationLevel, TraceArgs } from "./types.js";

export interface ScopeToken {
  readonly context: string;
  readonly track: bigint;
  readonly asynchronous: boolean;
}

export interface ObservationTarget {
  enabled(context: string, level: ObservationLevel): boolean;
  beginScope(
    context: string,
    name: string,
    args: TraceArgs,
    track: bigint | undefined,
    asynchronous: boolean,
  ): ScopeToken | undefined;
  endScope(token: ScopeToken, error?: unknown, failed?: boolean): void;
  recordEvent(context: string, name: string, args: TraceArgs, track?: bigint): void;
  recordCounter(context: string, name: string, value: number | bigint): void;
}

export class Observer {
  readonly #target: ObservationTarget;
  readonly #context: string;
  readonly #level: ObservationLevel;
  readonly #track: bigint | undefined;

  constructor(
    target: ObservationTarget,
    context: string,
    level: ObservationLevel = "basic",
    track?: bigint,
  ) {
    this.#target = target;
    this.#context = context;
    this.#level = level;
    this.#track = track;
  }

  get enabled(): boolean {
    return this.#target.enabled(this.#context, this.#level);
  }

  event(name: string, args: TraceArgs = {}): void {
    if (this.enabled) this.#target.recordEvent(this.#context, name, args, this.#track);
  }

  counter(name: string, value: number | bigint): void {
    if (this.enabled) this.#target.recordCounter(this.#context, name, value);
  }

  scope<T>(name: string, args: TraceArgs, work: () => T): T {
    if (!this.enabled) return work();
    const token = this.#target.beginScope(this.#context, name, args, this.#track, false);
    try {
      const result = work();
      if (token !== undefined) this.#target.endScope(token);
      return result;
    } catch (error) {
      if (token !== undefined) this.#target.endScope(token, error, true);
      throw error;
    }
  }

  async scopeAsync<T>(
    name: string,
    args: TraceArgs,
    work: (observer: Observer) => Promise<T>,
  ): Promise<T> {
    if (!this.enabled) return work(this);
    const token = this.#target.beginScope(this.#context, name, args, this.#track, true);
    const scoped =
      token === undefined
        ? this
        : new Observer(this.#target, this.#context, this.#level, token.track);
    try {
      const result = await work(scoped);
      if (token !== undefined) this.#target.endScope(token);
      return result;
    } catch (error) {
      if (token !== undefined) this.#target.endScope(token, error, true);
      throw error;
    }
  }
}
