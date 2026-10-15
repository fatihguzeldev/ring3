#[path = "moves/native/scalar.rs"]
mod cpu_simd_scalar_move;

#[path = "integer/native/direct.rs"]
mod simd_direct_integer;

#[path = "integration/native/families.rs"]
mod simd_semantics;

#[path = "descriptor/native.rs"]
mod simd_descriptor;

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

#[path = "avx/scalar/mask/capability/native.rs"]
mod movemask_capability;

#[path = "avx/moves/stream/capability/native.rs"]
mod stream_capability;

#[path = "avx/gather/capability/native.rs"]
mod gather_capability;

#[path = "avx/logical/native.rs"]
mod avx2_logical;

#[path = "avx/integer/native.rs"]
mod avx2_integer;

#[path = "avx/blocks/native.rs"]
mod avx2_blocks;

#[path = "avx/broadcast/native.rs"]
mod avx2_broadcast;

#[path = "avx/masked/native.rs"]
mod avx2_masked;

#[path = "avx/shift/uniform/native.rs"]
mod avx2_shift_uniform;

#[path = "avx/shift/variable/native.rs"]
mod avx2_shift_variable;

#[path = "avx/rearrange/widen/native.rs"]
mod avx2_rearrange_widen;

#[path = "avx/rearrange/lanes/native.rs"]
mod avx2_rearrange_lanes;

#[path = "avx/multiply/native.rs"]
mod avx2_multiply;

#[path = "avx/multiply/sad/native.rs"]
mod avx2_sad;

#[path = "avx/permute/whole/native.rs"]
mod avx2_whole;

#[path = "avx/permute/blend/native.rs"]
mod avx2_blend;

#[path = "avx/scalar/mask/native.rs"]
mod avx2_movemask;

#[path = "avx/moves/stream/native.rs"]
mod avx2_stream;

#[path = "avx/gather/native.rs"]
mod avx2_gather;
