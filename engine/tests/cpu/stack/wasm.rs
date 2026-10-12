#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "cases/wasm/cpu_enter_zero_wasm.rs"]
mod cpu_enter_zero_wasm;

#[path = "cases/wasm/cpu_leave_wasm.rs"]
mod cpu_leave_wasm;

#[path = "cases/wasm/cpu_pop_all_wasm.rs"]
mod cpu_pop_all_wasm;

#[path = "cases/wasm/cpu_push_flags_wasm.rs"]
mod cpu_push_flags_wasm;

#[path = "cases/wasm/cpu_stack_frames_wasm.rs"]
mod cpu_stack_frames_wasm;

#[path = "cases/wasm/cpu_word_frame_flags_wasm.rs"]
mod cpu_word_frame_flags_wasm;

#[path = "cases/wasm/cpu_word_stack_wasm.rs"]
mod cpu_word_stack_wasm;
