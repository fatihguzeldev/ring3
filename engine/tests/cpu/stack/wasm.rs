#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "frame/enter/zero/wasm.rs"]
mod cpu_enter_zero_wasm;

#[path = "frame/wasm/leave.rs"]
mod cpu_leave_wasm;

#[path = "bulk/wasm/pop_all.rs"]
mod cpu_pop_all_wasm;

#[path = "flags/wasm/dword.rs"]
mod cpu_push_flags_wasm;

#[path = "frame/wasm/nesting.rs"]
mod cpu_stack_frames_wasm;

#[path = "integration/frame/wasm/word.rs"]
mod cpu_word_frame_flags_wasm;

#[path = "operands/wasm/word.rs"]
mod cpu_word_stack_wasm;
