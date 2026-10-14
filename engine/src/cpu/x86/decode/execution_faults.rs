use iced_x86::{Code, CpuidFeature, EncodingKind, Instruction, OpKind, Register};

use crate::cpu::x86::{
    ControlOperation,
    capabilities::{
        HLE_ENABLED, MSR_ENABLED, RDPMC_ENABLED, RDRAND_ENABLED, RDSEED_ENABLED, RTM_ENABLED,
        SEP_ENABLED, SYSCALL_ENABLED, XCR0,
    },
    environment::{UserModeEnvironment, UserPortPermissions},
    ir::AddressSize,
};

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

pub(super) fn for_instruction(
    instruction: &Instruction,
    bytes: &[u8],
    environment: UserModeEnvironment,
) -> Option<ControlOperation> {
    if let Some(address_size) = port_string_address_size(instruction)
        && ports_denied(environment)
        && !instruction.has_repne_prefix()
        && !duplicate_profile_prefix(bytes)
        && bytes
            .iter()
            .take_while(|byte| legacy_prefix(**byte))
            .filter(|byte| **byte == 0x67)
            .count()
            <= 1
    {
        return Some(ControlOperation::DeniedPortString {
            repeat_count: instruction.has_rep_prefix().then_some(address_size),
        });
    }
    if !instruction.has_rep_prefix()
        && !instruction.has_repne_prefix()
        && !duplicate_profile_prefix(bytes)
    {
        if optional::unavailable(instruction)
            || unavailable(instruction)
            || user_privilege_invalid(instruction, environment)
            || state::disabled(instruction, environment)
        {
            return Some(ControlOperation::InvalidOpcode);
        }
        if general_protection(instruction, environment) {
            return Some(ControlOperation::GeneralProtectionZero);
        }
    }
    None
}

// monitor, mwait, pconfig, clac and stac explicitly raise ud at nonzero cpl.
// no implicit operand, monitor or platform backend is reached.
fn user_privilege_invalid(instruction: &Instruction, environment: UserModeEnvironment) -> bool {
    environment.current_privilege() != 0
        && matches!(
            instruction.code(),
            Code::Monitorw | Code::Monitord | Code::Mwait | Code::Pconfig | Code::Clac | Code::Stac
        )
}

fn unavailable(instruction: &Instruction) -> bool {
    // exact optional facilities fault only when their advertised capability is absent.
    match instruction.code() {
        Code::Rdrand_r16 | Code::Rdrand_r32 if !RDRAND_ENABLED => return true,
        Code::Rdseed_r16 | Code::Rdseed_r32 if !RDSEED_ENABLED => return true,
        Code::Xabort_imm8 | Code::Xbegin_rel16 | Code::Xbegin_rel32 | Code::Xend
            if !RTM_ENABLED =>
        {
            return true;
        }
        // XTEST is available with either transaction extension.
        Code::Xtest if !RTM_ENABLED && !HLE_ENABLED => return true,
        _ => {}
    }
    // the virtual feature bit is absent. This precedes EFER and return-privilege
    // questions and does not infer a physical vendor/model or invent an MSR value.
    if !SYSCALL_ENABLED && matches!(instruction.code(), Code::Syscall | Code::Sysretd) {
        return true;
    }
    // SEP0 explicitly makes both instructions unavailable before their entry-MSR
    // or return-privilege rules; no software operating-system state is guessed.
    if !SEP_ENABLED && matches!(instruction.code(), Code::Sysenter | Code::Sysexitd) {
        return true;
    }
    // the fixed protected32 user profile never executes inside smm.
    if instruction.code() == Code::Rsm {
        return true;
    }
    let encoding = instruction.encoding();
    let required_state = match encoding {
        EncodingKind::EVEX => 0xe7,
        EncodingKind::VEX => 0xe3,
        _ => return false,
    };
    if XCR0 & required_state == required_state {
        return false;
    }
    // known evex categories include memory-only vsib and mask-result forms;
    // vex requires an explicit opmask operand before its distinct state rule applies.
    let known_category = instruction.cpuid_features().iter().any(|feature| {
        matches!(
            feature,
            CpuidFeature::AVX512BW
                | CpuidFeature::AVX512CD
                | CpuidFeature::AVX512DQ
                | CpuidFeature::AVX512ER
                | CpuidFeature::AVX512F
                | CpuidFeature::AVX512PF
                | CpuidFeature::AVX512VL
                | CpuidFeature::AVX512_4FMAPS
                | CpuidFeature::AVX512_4VNNIW
                | CpuidFeature::AVX512_BF16
                | CpuidFeature::AVX512_BITALG
                | CpuidFeature::AVX512_FP16
                | CpuidFeature::AVX512_IFMA
                | CpuidFeature::AVX512_VBMI
                | CpuidFeature::AVX512_VBMI2
                | CpuidFeature::AVX512_VNNI
                | CpuidFeature::AVX512_VP2INTERSECT
                | CpuidFeature::AVX512_VPOPCNTDQ
        )
    });
    known_category
        && (encoding == EncodingKind::EVEX
            || (0..instruction.op_count()).any(|index| {
                instruction.op_kind(index) == OpKind::Register
                    && instruction.op_register(index).is_k()
            }))
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
        // absent wbnoinvd falls back to wbinvd, retaining its cpl0 requirement.
        | Code::Wbnoinvd
        | Code::Xsetbv => true,
        Code::Rdmsr | Code::Wrmsr => MSR_ENABLED && environment.current_privilege() != 0,
        Code::Sysexitd => SEP_ENABLED && environment.current_privilege() != 0,
        Code::Rdpmc => {
            RDPMC_ENABLED
                && environment.current_privilege() != 0
                && !environment.performance_counter_access()
        }
        Code::In_AL_imm8
        | Code::In_AX_imm8
        | Code::In_EAX_imm8
        | Code::In_AL_DX
        | Code::In_AX_DX
        | Code::In_EAX_DX
        | Code::Out_imm8_AL
        | Code::Out_imm8_AX
        | Code::Out_imm8_EAX
        | Code::Out_DX_AL
        | Code::Out_DX_AX
        | Code::Out_DX_EAX => ports_denied(environment),
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

fn ports_denied(environment: UserModeEnvironment) -> bool {
    environment.current_privilege() > environment.io_privilege()
        && environment.user_port_permissions() == UserPortPermissions::DenyAll
}

fn port_string_address_size(instruction: &Instruction) -> Option<AddressSize> {
    let operand = match instruction.code() {
        Code::Insb_m8_DX | Code::Insw_m16_DX | Code::Insd_m32_DX => instruction.op0_kind(),
        Code::Outsb_DX_m8 | Code::Outsw_DX_m16 | Code::Outsd_DX_m32 => instruction.op1_kind(),
        _ => return None,
    };
    match operand {
        OpKind::MemoryESDI | OpKind::MemorySegSI => Some(AddressSize::Bits16),
        OpKind::MemoryESEDI | OpKind::MemorySegESI => Some(AddressSize::Bits32),
        _ => None,
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

#[cfg(test)]
#[path = "execution_faults/tests/strings.rs"]
mod strings;

#[cfg(test)]
#[path = "execution_faults/tests/availability/native.rs"]
mod availability;

#[cfg(test)]
#[path = "execution_faults/tests/random.rs"]
mod random;

#[cfg(test)]
#[path = "execution_faults/tests/monitor.rs"]
mod monitor;

#[cfg(test)]
#[path = "execution_faults/tests/transactions.rs"]
mod transactions;

#[cfg(test)]
#[path = "execution_faults/tests/syscall.rs"]
mod syscall;

#[cfg(test)]
#[path = "execution_faults/tests/sep.rs"]
mod sep;

mod optional;

mod state;

#[cfg(test)]
#[path = "execution_faults/tests/system.rs"]
mod system;
