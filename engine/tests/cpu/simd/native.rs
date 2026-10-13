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

#[path = "avx/helper/native.rs"]
mod avx_helper;

#[path = "avx/f16c/native.rs"]
mod f16c;
#[path = "avx/fma/native.rs"]
mod fma;

#[path = "avx/logical/capability/native.rs"]
mod logical_capability;

#[path = "avx/integer/wrapping/capability/native.rs"]
mod wrapping_capability;

#[path = "avx/blocks/capability/native.rs"]
mod blocks_capability;

#[path = "avx/integer/compare/capability/native.rs"]
mod comparison_capability;

#[path = "avx/integer/capability/native.rs"]
mod integer_capability;

#[path = "avx/masked/capability/native.rs"]
mod masked_capability;

#[path = "avx/shift/uniform/capability/native.rs"]
mod uniform_shift_capability;

#[path = "avx/rearrange/widen/capability/native.rs"]
mod avx2_widen_capability;

#[path = "avx/shift/variable/capability/native.rs"]
mod variable_shift_capability;


#[path = "avx/broadcast/capability/native.rs"]
mod broadcast_capability;

#[path = "avx/rearrange/lanes/capability/native.rs"]
mod lane_capability;

#[path = "avx/multiply/capability/native.rs"]
mod multiply_capability;

#[path = "avx/permute/whole/capability/native.rs"]
mod whole_permute_capability;

#[path = "avx/permute/blend/capability/native.rs"]
mod blend_capability;
