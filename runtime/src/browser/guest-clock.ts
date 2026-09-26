export class GuestClock {
  readonly #origin: number;
  #excluded = 0;
  #stoppedAt: number | null = null;

  constructor(now: number) {
    this.#origin = now;
  }

  elapsed(now: number): number {
    return Math.floor((this.#stoppedAt ?? now) - this.#origin - this.#excluded);
  }

  stop(now: number): void {
    this.#stoppedAt ??= now;
  }

  resume(now: number): void {
    if (this.#stoppedAt === null) return;
    this.#excluded += now - this.#stoppedAt;
    this.#stoppedAt = null;
  }
}
