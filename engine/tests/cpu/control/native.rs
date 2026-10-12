#[path = "../../support/cpu/prefix_admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/word_prefix.rs"]
mod support_word_prefix;

#[path = "cases/native/cpu_alignment_nop.rs"]
mod cpu_alignment_nop;

#[path = "cases/native/cpu_carry_control.rs"]
mod cpu_carry_control;

#[path = "cases/native/cpu_count_branches.rs"]
mod cpu_count_branches;

#[path = "cases/native/cpu_pause.rs"]
mod cpu_pause;

#[path = "cases/native/cpu_prefixed_moffs_near_control.rs"]
mod cpu_prefixed_moffs_near_control;

#[path = "cases/native/cpu_word_indirect_call.rs"]
mod cpu_word_indirect_call;

#[path = "cases/native/cpu_word_indirect_jump.rs"]
mod cpu_word_indirect_jump;

#[path = "cases/native/cpu_word_jump.rs"]
mod cpu_word_jump;

#[path = "cases/native/cpu_word_near_control.rs"]
mod cpu_word_near_control;
