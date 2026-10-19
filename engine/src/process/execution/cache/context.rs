use super::types::CacheRequest32;
use crate::cpu::dbt::{
    CompileLimits, PreparedResidentReplacement, RegistryError, RegistryLimits,
    validate_compile_limits,
};
use crate::cpu::{dbt::CompilationTier32, x86::decode::CodeDecodeKey};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionConfig32 {
    pub units: u32,
    pub wasm_bytes: u32,
    pub promotion_hits: u32,
}
impl Default for ExecutionConfig32 {
    fn default() -> Self {
        Self {
            units: 256,
            wasm_bytes: 16 * 1024 * 1024,
            promotion_hits: 256,
        }
    }
}
pub(in crate::process) struct ExecutionCache32 {
    pub(super) config: ExecutionConfig32,
    pub(super) compile_limits: CompileLimits,
    pub(super) entries: Vec<Option<CacheEntry32>>,
    pub(super) free_slots: super::slots::FreeSlots,
    pub(super) victims: super::victims::VictimIndex,
    pub(super) request: Option<PendingCache32>,
    pub(super) promotion: Option<PromotionTarget32>,
    pub(super) cohort: Option<super::cohort::PromotionCohort32>,
    pub(super) replacement: Option<PreparedResidentReplacement>,
    pub(super) promotion_round: u64,
    #[cfg(test)]
    pub(super) promotion_attempts: AtomicU64,
    pub(super) serial: u64,
    pub(super) revision: u64,
    pub(super) clock: AtomicU64,
    pub(super) hits: AtomicU64,
    pub(super) misses: u64,
    pub(super) evictions: u64,
    pub(super) promotions: u64,
}
pub(super) struct CacheEntry32 {
    pub id: u64,
    pub entry: u32,
    pub bytes: usize,
    pub tier: CompilationTier32,
    pub hits: AtomicU64,
    pub last: AtomicU64,
    pub need_code_hint: super::edges::NeedCodeHint32,
    pub promotion_retry: super::promotion::PromotionRetry32,
}
#[derive(Clone, Copy)]
pub(super) struct PromotionTarget32 {
    pub pc: u32,
    pub version: u64,
    pub decode_key: CodeDecodeKey,
    pub coverage: Option<crate::cpu::dbt::ResidentCohort>,
}
#[derive(Clone, Copy)]
pub(super) struct PendingCache32 {
    pub request: CacheRequest32,
    pub version: u64,
    pub decode_key: CodeDecodeKey,
    pub revision: u64,
    pub owned_unit: bool,
}
pub(super) fn increment(counter: &AtomicU64) -> u64 {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(value.saturating_add(1))
        })
        .unwrap()
        .saturating_add(1)
}
impl ExecutionCache32 {
    pub(super) fn new(
        config: ExecutionConfig32,
        compile_limits: CompileLimits,
    ) -> Result<Self, RegistryError> {
        if !(1..=RegistryLimits::MAX_UNITS as u32).contains(&config.units)
            || !(65_536..=RegistryLimits::MAX_WASM_BYTES as u32).contains(&config.wasm_bytes)
            || config.promotion_hits == 0
        {
            return Err(RegistryError::InvalidLimits);
        }
        if validate_compile_limits(compile_limits).is_err()
            || compile_limits.blocks != CompileLimits::default().blocks
            || compile_limits.wasm_bytes > config.wasm_bytes as usize
        {
            return Err(RegistryError::InvalidLimits);
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(config.units as usize)
            .map_err(|_| RegistryError::Allocation)?;
        entries.resize_with(config.units as usize, || None);
        let free_slots = super::slots::FreeSlots::new(config.units as usize)?;
        let victims = super::victims::VictimIndex::new(config.units as usize)?;
        Ok(Self {
            config,
            compile_limits,
            entries,
            free_slots,
            victims,
            request: None,
            promotion: None,
            cohort: None,
            replacement: None,
            promotion_round: 0,
            #[cfg(test)]
            promotion_attempts: AtomicU64::new(0),
            serial: 1,
            revision: 0,
            clock: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: 0,
            evictions: 0,
            promotions: 0,
        })
    }
    pub(super) fn registry_limits(&self) -> RegistryLimits {
        RegistryLimits {
            units: self.config.units as usize,
            wasm_bytes: self.config.wasm_bytes as usize,
        }
    }
}
