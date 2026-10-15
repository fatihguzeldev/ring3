mod aliases;
mod atomic;
mod authority;
pub(crate) mod avx;
mod bit_manipulation;
mod cache_timestamp;
mod control;
mod decoder;
mod execution_faults;
mod far;
mod fetch;
mod flow;
mod integer;
mod instruction;
mod interrupts;
mod lower;
mod mpx;
mod operands;
mod packed_strings;
mod profile;
mod scalar_bits;
mod segment_queries;
mod segments;
mod simd;
mod stack;
mod strings;
mod system_registers;
mod x87;
mod xstate;

pub(crate) use authority::DecodeAuthority;
pub use decoder::{DecodeError, decode_one};
pub use instruction::DecodedInstruction;
pub(crate) use instruction::DecodedOperation;
pub(crate) use decoder::{DecodePolicy, decode_for_compilation, paging_error};

#[cfg(test)]
pub(crate) use decoder::{decode_bound, decode_for_execution, decode_with_authority};
pub(crate) use fetch::{
    BoundDecode, CapturedCodeFault, CapturedFetchFaultKind32, CodeDecodeKey, CodeFetchContext,
    fetch_unbound,
};

#[cfg(test)]
mod modes;
