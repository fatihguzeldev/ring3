use iced_x86::{Code, Instruction};

use crate::cpu::x86::environment::UserModeEnvironment;

pub(super) fn disabled(instruction: &Instruction, environment: UserModeEnvironment) -> bool {
    match instruction.code() {
        // cr4.pke=0 makes these instructions unavailable before their ECX/EDX checks.
        Code::Rdpkru | Code::Wrpkru => !environment.protection_keys_enabled(),
        // cr4.cet=0 is sufficient; no shadow-stack enable MSR or SSP value is needed.
        Code::Incsspd_r32 => !environment.control_flow_enforcement_enabled(),
        _ => false,
    }
}

#[cfg(test)]
#[path = "tests/state.rs"]
mod tests;
