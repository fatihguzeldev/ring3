use super::super::context::ExecutionCache32;

const MAX_RETRY_SHIFT: u8 = 6;

#[derive(Default)]
pub(in crate::process::execution::cache) struct PromotionRetry32 {
    after_round: u64,
    shift: u8,
}

impl PromotionRetry32 {
    pub(in crate::process::execution::cache) fn eligible(&self, round: u64) -> bool {
        round >= self.after_round
    }

    pub(in crate::process::execution::cache) fn declined(&mut self, round: u64) {
        // skip 1, 2, ... 64 complete future promotion calls, then reconsider this exact row.
        self.after_round = round.saturating_add(1_u64 << self.shift).saturating_add(1);
        self.shift = self.shift.saturating_add(1).min(MAX_RETRY_SHIFT);
    }

    #[cfg(feature = "observability")]
    pub(in crate::process::execution::cache) fn next_round(&self) -> u64 {
        self.after_round
    }
}

impl ExecutionCache32 {
    pub(in crate::process::execution::cache) fn next_promotion_round(&mut self) -> u64 {
        if let Some(round) = self.promotion_round.checked_add(1) {
            self.promotion_round = round;
        } else {
            // counter rollover discards only policy delays; it cannot suppress a current owner.
            self.promotion_round = 1;
            for row in self.entries.iter_mut().flatten() {
                row.promotion_retry = Default::default();
            }
        }
        self.promotion_round
    }
}
