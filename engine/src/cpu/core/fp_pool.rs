//! CPU-private x87 storage. Architectural physical slots remain stable across owners.
//!
//! The canonical x87/SIMD prefix of NumericBank is unchanged. This frame is never
//! guest memory and its writes do not participate in guest memory revisions.

pub(crate) const BANK_OFFSET: u64 = 512;
pub(crate) const ACTIVE: u64 = 0;
pub(crate) const CONTROL: u64 = 4;
pub(crate) const STATUS: u64 = 8;
pub(crate) const TAG: u64 = 12;
pub(crate) const TOP: u64 = 16;
pub(crate) const FIP: u64 = 20;
pub(crate) const FDP: u64 = 24;
pub(crate) const OPCODE: u64 = 28;
pub(crate) const CS: u64 = 32;
pub(crate) const DS: u64 = 36;
pub(crate) const PENDING: u64 = 40;
pub(crate) const PENDING_LEFT: u64 = 48;
pub(crate) const PENDING_RIGHT: u64 = 56;
pub(crate) const PENDING_RESULT: u64 = 64;
pub(crate) const CHANGED: u64 = 76;
pub(crate) const RECORDS: u64 = 128;
pub(crate) const RECORD_SIZE: u64 = 32;
pub(crate) const NATIVE: u64 = 0;
pub(crate) const LOW: u64 = 8;
pub(crate) const HIGH: u64 = 16;
pub(crate) const VALID: u64 = 20;
pub(crate) const PROOFS: u64 = 24;
pub(crate) const PROOF_CONTROL: u64 = 28;

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct FpLeaseRecord {
    pub native: f64,
    pub low: u64,
    pub high: u32,
    pub valid: u32,
    pub proofs: u32,
    pub control: u32,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct FpLeasePool {
    pub active: u32,
    pub control: u32,
    pub status: u32,
    pub tag: u32,
    pub top: u32,
    pub fip: u32,
    pub fdp: u32,
    pub opcode: u32,
    pub cs: u32,
    pub ds: u32,
    pub pending: u32,
    reserved0: u32,
    pub pending_left: f64,
    pub pending_right: f64,
    pub pending_result: f64,
    reserved_epoch: u32,
    pub changed: u32,
    reserved: [u32; 12],
    pub records: [FpLeaseRecord; 8],
}

const _: () = {
    assert!(core::mem::size_of::<FpLeaseRecord>() == RECORD_SIZE as usize);
    assert!(core::mem::offset_of!(FpLeaseRecord, low) == LOW as usize);
    assert!(core::mem::offset_of!(FpLeaseRecord, valid) == VALID as usize);
    assert!(core::mem::offset_of!(FpLeasePool, pending_left) == PENDING_LEFT as usize);
    assert!(core::mem::offset_of!(FpLeasePool, records) == RECORDS as usize);
    assert!(core::mem::size_of::<FpLeasePool>() == 384);
};
