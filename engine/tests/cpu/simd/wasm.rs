#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "moves/wasm/scalar.rs"]
mod cpu_simd_scalar_move_wasm;

#[path = "integration/wasm/families.rs"]
mod cpu_simd_wasm;

#[path = "state/wasm/xsave.rs"]
mod cpu_xstate_wasm;

#[path = "state/legacy/wasm/segments.rs"]
mod cpu_fx_segment_access_wasm;

#[path = "state/segments/wasm.rs"]
mod cpu_xstate_segment_access_wasm;

#[path = "state/upper/wasm.rs"]
mod ymm_upper;

#[path = "state/xsave/upper/wasm.rs"]
mod xstate_upper;

#[path = "avx/helper/wasm.rs"]
mod avx_helper;

#[path = "avx/execution/wasm.rs"]
mod cpu_avx_execution_wasm;

#[path = "avx/protocol/wasm.rs"]
mod cpu_avx_protocol_wasm;
