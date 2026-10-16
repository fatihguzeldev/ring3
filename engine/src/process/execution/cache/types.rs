use crate::cpu::dbt::CompilationTier32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CacheRequestKind32 {
    Hit = 0,
    Install = 1,
    Evict = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheRequest32 {
    pub serial: u64,
    pub kind: CacheRequestKind32,
    pub pc: u32,
    pub unit_id: u64,
    pub victim_id: u64,
    pub slot: u32,
    pub victim_slot: u32,
    pub tier: CompilationTier32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheUsage32 {
    pub units: u32,
    pub wasm_bytes: u32,
    pub installed: u32,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub promotions: u64,
    pub pending_serial: u64,
}
