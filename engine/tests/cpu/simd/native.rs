#[path = "moves/native/scalar.rs"]
mod cpu_simd_scalar_move;

#[path = "integer/native/direct.rs"]
mod simd_direct_integer;

#[path = "integration/native/families.rs"]
mod simd_semantics;

#[path = "integer/native/ssse3.rs"]
mod simd_extensions;

#[path = "arithmetic/native/sse3.rs"]
mod simd_sse3;

#[path = "integer/native/sse4.rs"]
mod simd_sse4;

#[path = "moves/native/sse4.rs"]
mod simd_sse4_moves;

#[path = "arithmetic/native/sse4.rs"]
mod simd_sse4_arithmetic;

#[path = "strings/native/comparison.rs"]
mod packed_strings;

#[path = "crypto/native.rs"]
mod simd_crypto;

#[path = "state/native/xsave.rs"]
mod xstate;

#[path = "state/legacy/native/segments.rs"]
mod cpu_fx_segment_access;

#[path = "state/segments/native.rs"]
mod cpu_xstate_segment_access;

#[path = "state/upper/native.rs"]
mod ymm_upper;

#[path = "state/xsave/upper/native.rs"]
mod xstate_upper;
