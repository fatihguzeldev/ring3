use iced_x86::{Code, Instruction, Register};

use crate::cpu::x86::{ControlOperation, environment::UserModeEnvironment};

pub(super) fn invalid(bytes: &[u8]) -> Option<ControlOperation> {
    match bytes.len() {
        1..=14 => Some(ControlOperation::InvalidOpcode),
        15 if bytes.iter().all(|byte| legacy_prefix(*byte)) => {
            Some(ControlOperation::GeneralProtectionZero)
        }
        // the decoder merges malformed and overlong encodings at its 15-byte ceiling.
        _ => None,
    }
}

pub(super) fn privileged(
    instruction: &Instruction,
    bytes: &[u8],
    environment: UserModeEnvironment,
) -> Option<ControlOperation> {
    if general_protection(instruction, environment)
        && !instruction.has_rep_prefix()
        && !instruction.has_repne_prefix()
        && !duplicate_profile_prefix(bytes)
    {
        Some(ControlOperation::GeneralProtectionZero)
    } else {
        None
    }
}

fn general_protection(instruction: &Instruction, environment: UserModeEnvironment) -> bool {
    match instruction.code() {
        Code::Hlt
        | Code::Clts
        | Code::Invlpg_m
        | Code::Lgdt_m1632_16
        | Code::Lgdt_m1632
        | Code::Lidt_m1632_16
        | Code::Lidt_m1632
        | Code::Lldt_rm16
        | Code::Lldt_r32m16
        | Code::Ltr_rm16
        | Code::Ltr_r32m16
        | Code::Lmsw_rm16
        | Code::Lmsw_r32m16
        | Code::Invd
        | Code::Wbinvd
        | Code::Xsetbv => true,
        Code::Mov_r32_cr => control_register(instruction.op1_register()),
        Code::Mov_cr_r32 => control_register(instruction.op0_register()),
        Code::Cli | Code::Sti => {
            environment.current_privilege() > environment.io_privilege()
                && !environment.protected_virtual_interrupts()
        }
        Code::Mov_r32_dr => debug_register_fault(instruction.op1_register(), environment),
        Code::Mov_dr_r32 => debug_register_fault(instruction.op0_register(), environment),
        _ => false,
    }
}

fn debug_register_fault(register: Register, environment: UserModeEnvironment) -> bool {
    environment.current_privilege() != 0
        && !environment.debug_general_detect()
        && !(environment.debug_extensions_enabled()
            && matches!(register, Register::DR4 | Register::DR5))
        && matches!(
            register,
            Register::DR0
                | Register::DR1
                | Register::DR2
                | Register::DR3
                | Register::DR4
                | Register::DR5
                | Register::DR6
                | Register::DR7
        )
}

fn control_register(register: Register) -> bool {
    matches!(
        register,
        Register::CR0 | Register::CR2 | Register::CR3 | Register::CR4
    )
}

fn legacy_prefix(byte: u8) -> bool {
    matches!(
        byte,
        0xf0 | 0xf2 | 0xf3 | 0x66 | 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65
    )
}

fn duplicate_profile_prefix(bytes: &[u8]) -> bool {
    let mut groups = [0; 3];
    for byte in bytes.iter().take_while(|byte| legacy_prefix(**byte)) {
        match byte {
            0x66 => groups[0] += 1,
            0xf0 => groups[1] += 1,
            0xf2 | 0xf3 => groups[2] += 1,
            _ => {}
        }
    }
    groups.iter().any(|count| *count > 1)
}

#[cfg(test)]
#[path = "execution_faults/tests/faults.rs"]
mod tests;

#[cfg(test)]
#[path = "execution_faults/tests/environment.rs"]
mod environment_tests;
