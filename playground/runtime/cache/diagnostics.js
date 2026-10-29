// Cold-path diagnostics only: no engine calls, timers, or retained module objects.
export class CacheDiagnostics {
  constructor() {
    this.capacityEvictions = 0;
    this.promotionEvictions = 0;
    this.optimizedVictims = 0;
    this.promotionAttempts = 0;
    this.baselineFallbacks = 0;
    this.recentFallbacks = [];
    this.active = null;
  }
  begin(pc) {
    this.promotionAttempts++;
    this.active = { pc, victims: 0, optimizedVictims: 0 };
  }
  evicted(tier, victimTier) {
    if (tier === 1) this.promotionEvictions++;
    else this.capacityEvictions++;
    if (victimTier === 1) this.optimizedVictims++;
    if (this.active) {
      this.active.victims++;
      if (victimTier === 1) this.active.optimizedVictims++;
    }
  }
  finish(unit) {
    if (this.active && unit.tier === 0) {
      this.baselineFallbacks++;
      if (this.recentFallbacks.length === 16) this.recentFallbacks.shift();
      this.recentFallbacks.push({ ...this.active, id: String(unit.id), bytes: unit.bytes });
    }
    this.active = null;
  }
  snapshot() {
    return {
      capacityEvictions: this.capacityEvictions,
      promotionEvictions: this.promotionEvictions,
      optimizedVictims: this.optimizedVictims,
      promotionAttempts: this.promotionAttempts,
      baselineFallbacks: this.baselineFallbacks,
      recentFallbacks: this.recentFallbacks.map(row => ({ ...row })),
    };
  }
}
