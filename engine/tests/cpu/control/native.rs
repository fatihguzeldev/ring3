#[path = "../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/prefix/word.rs"]
mod support_word_prefix;

#[path = "hints/native/nop.rs"]
mod cpu_alignment_nop;

#[path = "flags/native/carry.rs"]
mod cpu_carry_control;

#[path = "branch/count/native/dword.rs"]
mod cpu_count_branches;

#[path = "exceptions/native/flags.rs"]
mod cpu_flags_exceptions;

#[path = "hints/native/pause.rs"]
mod cpu_pause;

#[path = "integration/native/prefixes.rs"]
mod cpu_prefixed_moffs_near_control;

#[path = "call/indirect/native/word.rs"]
mod cpu_word_indirect_call;

#[path = "branch/indirect/native/word.rs"]
mod cpu_word_indirect_jump;

#[path = "branch/direct/native/word.rs"]
mod cpu_word_jump;

#[path = "near/native/word.rs"]
mod cpu_word_near_control;
