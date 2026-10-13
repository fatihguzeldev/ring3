#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "hints/wasm/nop.rs"]
mod cpu_alignment_nop_wasm;

#[path = "flags/wasm/carry.rs"]
mod cpu_carry_control_wasm;

#[path = "branch/count/wasm/dword.rs"]
mod cpu_count_branches_wasm;

#[path = "hints/wasm/pause.rs"]
mod cpu_pause_wasm;

#[path = "branch/conditional/wasm/word.rs"]
mod cpu_word_conditional_jump_wasm;

#[path = "call/indirect/wasm/word.rs"]
mod cpu_word_indirect_call_wasm;

#[path = "branch/indirect/wasm/word.rs"]
mod cpu_word_indirect_jump_wasm;

#[path = "branch/direct/wasm/word.rs"]
mod cpu_word_jump_wasm;

#[path = "exceptions/wasm/flags.rs"]
mod cpu_flags_exceptions_wasm;
#[path = "near/wasm/word.rs"]
mod cpu_word_near_control_wasm;

#[path = "exceptions/wasm/execution.rs"]
mod cpu_execution_faults_wasm;

#[path = "exceptions/wasm/privileged.rs"]
mod cpu_privileged_faults_wasm;

#[path = "far/jump/wasm.rs"]
mod cpu_far_jump_wasm;

#[path = "far/call/wasm.rs"]
mod cpu_far_call;

#[path = "far/return/wasm.rs"]
mod cpu_far_return;

#[path = "exceptions/undefined/wasm.rs"]
mod cpu_undefined;

#[path = "exceptions/privileged/msr/wasm.rs"]
mod cpu_msr;

#[path = "exceptions/environment/counters/wasm.rs"]
mod cpu_counters;

#[path = "exceptions/environment/io/wasm.rs"]
mod cpu_port_io;
