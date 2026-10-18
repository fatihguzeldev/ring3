import type { Category } from "../catalog/types.js";
import { requireCategory } from "../catalog/selection.js";
import type { ObservationLevel, TraceArgs } from "./types.js";

export interface ScopeToken {
  readonly category: Category;
  readonly track: bigint;
  readonly asynchronous: boolean;
}

export interface ObservationTarget {
  enabled(category: Category, level: ObservationLevel): boolean;
  beginScope(
    category: Category,
    name: string,
    args: TraceArgs,
    track: bigint | undefined,
    asynchronous: boolean,
  ): ScopeToken | undefined;
  endScope(token: ScopeToken, error?: unknown, failed?: boolean): void;
  recordEvent(category: Category, name: string, args: TraceArgs, track?: bigint): void;
  recordCounter(category: Category, name: string, value: number | bigint): void;
}

export class Observer {
  readonly #target: ObservationTarget;
  readonly #category: Category;
  readonly #level: ObservationLevel;
  readonly #track: bigint | undefined;

  constructor(
    target: ObservationTarget,
    category: Category,
    level: ObservationLevel = "basic",
    track?: bigint,
  ) {
    requireCategory(category);
    this.#target = target;
    this.#category = category;
    this.#level = level;
    this.#track = track;
  }

  get enabled(): boolean {
    return this.#target.enabled(this.#category, this.#level);
  }

  event(name: string, args: TraceArgs = {}): void {
    if (this.enabled) this.#target.recordEvent(this.#category, name, args, this.#track);
  }

  counter(name: string, value: number | bigint): void {
    if (this.enabled) this.#target.recordCounter(this.#category, name, value);
  }

  scope<T>(name: string, args: TraceArgs, work: () => T): T {
    if (!this.enabled) return work();
    const token = this.#target.beginScope(this.#category, name, args, this.#track, false);
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
    const token = this.#target.beginScope(this.#category, name, args, this.#track, true);
    const scoped =
      token === undefined
        ? this
        : new Observer(this.#target, this.#category, this.#level, token.track);
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
