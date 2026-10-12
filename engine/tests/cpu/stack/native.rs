#[path = "../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/prefix/word.rs"]
mod support_word_prefix;

#[path = "frame/enter/zero/native.rs"]
mod cpu_enter_zero;

#[path = "frame/native/leave.rs"]
mod cpu_leave;

#[path = "bulk/native/pop_all.rs"]
mod cpu_pop_all;

#[path = "flags/native/dword.rs"]
mod cpu_push_flags;

#[path = "frame/native/nesting.rs"]
mod cpu_stack_frames;

#[path = "integration/frame/native/word.rs"]
mod cpu_word_frame_flags;

#[path = "operands/native/word.rs"]
mod cpu_word_stack;
