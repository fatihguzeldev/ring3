#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct NeedCodeHint32 {
    target: u32,
    score: u32,
}

impl NeedCodeHint32 {
    #[cfg(any(target_arch = "wasm32", test))]
    fn observe(&mut self, target: u32) {
        if self.score == 0 {
            self.target = target;
            self.score = 1;
        } else if self.target == target {
            self.score = self.score.saturating_add(1);
        } else {
            self.score -= 1;
        }
    }

    pub(super) fn target(&self, minimum: u32) -> Option<u32> {
        (self.score != 0 && self.score >= minimum).then_some(self.target)
    }
}

#[cfg(any(target_arch = "wasm32", test))]
use crate::{
    abi::{arena::STATE_OFFSET, header, x86::EIP_OFFSET},
    cpu::dbt::CompilationTier32,
    process::EngineInstance,
};

#[cfg(any(target_arch = "wasm32", test))]
impl EngineInstance {
    pub(in crate::process) fn record_cache_need_code(&mut self, epoch: u64) {
        let Some((active, installed)) = self.execution_frame.and_then(|frame| frame.last) else {
            return;
        };
        if active != epoch
            || self
                .resident_installations
                .get(installed.slot as usize)
                .copied()
                .flatten()
                != Some(installed)
        {
            return;
        }
        let target = header::read_u32(self.arena(), STATE_OFFSET + EIP_OFFSET);
        let Some(row) = self
            .execution_cache
            .as_mut()
            .and_then(|cache| cache.entries.get_mut(installed.slot as usize))
            .and_then(Option::as_mut)
            .filter(|row| row.id == installed.unit_id && row.tier == CompilationTier32::Optimized)
        else {
            return;
        };
        // a validated exit is only a discovery hint; cold admission rechecks its target.
        row.need_code_hint.observe(target);
    }
}

#[cfg(test)]
mod tests;
