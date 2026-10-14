mod aliases;
mod atomic;
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

pub use decoder::{DecodeError, DecodedInstruction, decode_one};
pub(crate) use decoder::{DecodePolicy, decode_with_system_registers};

#[cfg(test)]
pub(crate) use decoder::{decode_bound, decode_for_execution};
pub(crate) use fetch::{BoundDecode, CapturedCodeFault, CodeDecodeKey, CodeFetchContext};

#[cfg(test)]
mod modes;
