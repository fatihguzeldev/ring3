use iced_x86::{Code, Instruction, OpKind};

use super::{Cpu32, GuestMemory, StopReason};

impl Cpu32 {
    pub(super) fn conditional_instruction(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<u32, StopReason> {
        let carry = self.eflags & 1 != 0;
        let parity = self.eflags & 4 != 0;
        let zero = self.eflags & 0x40 != 0;
        let sign = self.eflags & 0x80 != 0;
        let overflow = self.eflags & 0x800 != 0;
        let taken = match instruction.code() {
            Code::Jo_rel8_32 | Code::Jo_rel32_32 | Code::Seto_rm8 => overflow,
            Code::Jno_rel8_32 | Code::Jno_rel32_32 | Code::Setno_rm8 => !overflow,
            Code::Jb_rel8_32 | Code::Jb_rel32_32 | Code::Setb_rm8 => carry,
            Code::Jae_rel8_32 | Code::Jae_rel32_32 | Code::Setae_rm8 => !carry,
            Code::Je_rel8_32 | Code::Je_rel32_32 | Code::Sete_rm8 => zero,
            Code::Jne_rel8_32 | Code::Jne_rel32_32 | Code::Setne_rm8 => !zero,
            Code::Jbe_rel8_32 | Code::Jbe_rel32_32 | Code::Setbe_rm8 => carry || zero,
            Code::Ja_rel8_32 | Code::Ja_rel32_32 | Code::Seta_rm8 => !carry && !zero,
            Code::Js_rel8_32 | Code::Js_rel32_32 | Code::Sets_rm8 => sign,
            Code::Jns_rel8_32 | Code::Jns_rel32_32 | Code::Setns_rm8 => !sign,
            Code::Jp_rel8_32 | Code::Jp_rel32_32 | Code::Setp_rm8 => parity,
            Code::Jnp_rel8_32 | Code::Jnp_rel32_32 | Code::Setnp_rm8 => !parity,
            Code::Jl_rel8_32 | Code::Jl_rel32_32 | Code::Setl_rm8 => sign != overflow,
            Code::Jge_rel8_32 | Code::Jge_rel32_32 | Code::Setge_rm8 => sign == overflow,
            Code::Jle_rel8_32 | Code::Jle_rel32_32 | Code::Setle_rm8 => zero || sign != overflow,
            Code::Jg_rel8_32 | Code::Jg_rel32_32 | Code::Setg_rm8 => !zero && sign == overflow,
            _ => return Err(StopReason::UnsupportedInstruction),
        };
        if matches!(instruction.op0_kind(), OpKind::Register | OpKind::Memory) {
            self.write_operand(self.operand(instruction, 0)?, u32::from(taken), memory)?;
            return Ok(instruction.next_ip32());
        }
        Ok(if taken {
            instruction.near_branch32()
        } else {
            instruction.next_ip32()
        })
    }
}
