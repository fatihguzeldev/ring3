use iced_x86::{Code, Instruction};

use crate::cpu::x86::capabilities::CPU_ID_LEAVES;

mod amd;
mod modern;

pub(super) fn unavailable(instruction: &Instruction) -> bool {
    let (function, subfunction, register, bit) = match instruction.code() {
        Code::Adcx_r32_rm32 | Code::Adox_r32_rm32 => (7, 0, 1, 19),
        Code::VEX_Vaesenc_ymm_ymm_ymmm256
        | Code::VEX_Vaesenclast_ymm_ymm_ymmm256
        | Code::VEX_Vaesdec_ymm_ymm_ymmm256
        | Code::VEX_Vaesdeclast_ymm_ymm_ymmm256 => (7, 0, 2, 9),
        Code::Aesencwide128kl_m384
        | Code::Aesdecwide128kl_m384
        | Code::Aesencwide256kl_m512
        | Code::Aesdecwide256kl_m512
        | Code::Aesenc128kl_xmm_m384
        | Code::Aesdec128kl_xmm_m384
        | Code::Aesenc256kl_xmm_m512
        | Code::Aesdec256kl_xmm_m512
        | Code::Encodekey128_r32_r32
        | Code::Encodekey256_r32_r32
        | Code::Loadiwkey_xmm_xmm => (7, 0, 2, 23),
        Code::VEX_Vpclmulqdq_ymm_ymm_ymmm256_imm8 => (7, 0, 2, 10),
        Code::Invpcid_r32_m128 => (7, 0, 1, 10),
        // explicit software-profile priority: absent opcode before enabled-state work.
        Code::Enqcmd_r16_m512
        | Code::Enqcmd_r32_m512
        | Code::Enqcmds_r16_m512
        | Code::Enqcmds_r32_m512 => (7, 0, 2, 29),
        Code::Hreset_imm8 => (7, 1, 0, 22),
        // revision-qualified unavailable-opcode priority; no enabled msr bank.
        Code::Wrmsrns => (7, 1, 0, 19),
        Code::Clflushopt_m8 => (7, 0, 1, 23),
        Code::Clwb_m8 => (7, 0, 1, 24),
        Code::Sha1nexte_xmm_xmmm128
        | Code::Sha1msg1_xmm_xmmm128
        | Code::Sha1msg2_xmm_xmmm128
        | Code::Sha256rnds2_xmm_xmmm128
        | Code::Sha256msg1_xmm_xmmm128
        | Code::Sha256msg2_xmm_xmmm128
        | Code::Sha1rnds4_xmm_xmmm128_imm8 => (7, 0, 1, 29),
        Code::Tpause_r32 | Code::Umonitor_r16 | Code::Umonitor_r32 | Code::Umwait_r32 => {
            (7, 0, 2, 5)
        }
        Code::Gf2p8mulb_xmm_xmmm128
        | Code::VEX_Vgf2p8mulb_xmm_xmm_xmmm128
        | Code::VEX_Vgf2p8mulb_ymm_ymm_ymmm256
        | Code::Gf2p8affineqb_xmm_xmmm128_imm8
        | Code::VEX_Vgf2p8affineqb_xmm_xmm_xmmm128_imm8
        | Code::VEX_Vgf2p8affineqb_ymm_ymm_ymmm256_imm8
        | Code::Gf2p8affineinvqb_xmm_xmmm128_imm8
        | Code::VEX_Vgf2p8affineinvqb_xmm_xmm_xmmm128_imm8
        | Code::VEX_Vgf2p8affineinvqb_ymm_ymm_ymmm256_imm8 => (7, 0, 2, 8),
        Code::Movdiri_m32_r32 => (7, 0, 2, 27),
        Code::Movdir64b_r16_m512 | Code::Movdir64b_r32_m512 => (7, 0, 2, 28),
        Code::Serialize => (7, 0, 3, 14),
        Code::Xsusldtrk | Code::Xresldtrk => (7, 0, 3, 16),
        Code::Xsaveopt_mem => (13, 1, 0, 0),
        Code::Xsavec_mem => (13, 1, 0, 1),
        Code::Xrstors_mem | Code::Xsaves_mem => (13, 1, 0, 3),
        Code::Ptwrite_rm32 => (20, 0, 1, 4),
        _ => {
            return instruction.cpuid_features().iter().any(|&feature| {
                amd::required(feature)
                    .or_else(|| modern::required(feature))
                    .is_some_and(|required| !available(required))
            });
        }
    };
    !available((function, subfunction, register, bit))
}

fn available((function, subfunction, register, bit): (u32, u32, usize, u32)) -> bool {
    CPU_ID_LEAVES.iter().any(|leaf| {
        leaf.function == function
            && leaf.subfunction.is_none_or(|value| value == subfunction)
            && leaf.registers[register] & (1 << bit) != 0
    })
}

#[cfg(test)]
#[path = "tests/optional.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/tsxldtrk.rs"]
mod tsxldtrk;

#[cfg(test)]
#[path = "tests/extended.rs"]
mod extended;

#[cfg(test)]
#[path = "tests/keys.rs"]
mod keys;

#[cfg(test)]
#[path = "tests/pclmul.rs"]
mod pclmul;

#[cfg(test)]
#[path = "tests/invpcid.rs"]
mod invpcid;

#[cfg(test)]
#[path = "tests/enqueue.rs"]
mod enqueue;

#[cfg(test)]
#[path = "tests/history.rs"]
mod history;

#[cfg(test)]
#[path = "tests/msr.rs"]
mod msr;
