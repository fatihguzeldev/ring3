use iced_x86::{Code, Instruction, OpKind};

use super::operands::{read_dword, write_dword};
use super::{Cpu32, GuestMemory, Register32, StopReason, register32};

pub(super) fn is_stack(code: Code) -> bool {
    matches!(
        code,
        Code::Push_r32
            | Code::Pushd_imm32
            | Code::Pushd_imm8
            | Code::Push_rm32
            | Code::Pop_r32
            | Code::Pop_rm32
            | Code::Call_rel32_32
            | Code::Call_rm32
            | Code::Jmp_rm32
            | Code::Retnd
            | Code::Retnd_imm16
            | Code::Leaved
            | Code::Pushaw
            | Code::Pushad
            | Code::Popaw
            | Code::Popad
    )
}

impl Cpu32 {
    pub(super) fn stack_instruction(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<u32, StopReason> {
        let next = instruction.next_ip32();
        match instruction.code() {
            Code::Pushaw | Code::Pushad | Code::Popaw | Code::Popad => {
                self.register_stack(instruction.code(), memory)?;
            }
            Code::Push_r32 | Code::Pushd_imm32 | Code::Pushd_imm8 | Code::Push_rm32 => {
                let value = self.read_operand(self.operand(instruction, 0)?, memory)?;
                self.push(memory, value)?;
            }
            Code::Pop_r32 | Code::Pop_rm32 if instruction.op0_kind() == OpKind::Register => {
                let register = register32(instruction.op0_register())?;
                let value = self.pop(memory)?;
                self.set_register(register, value);
            }
            Code::Call_rel32_32 | Code::Call_rm32 => {
                let target = if instruction.code() == Code::Call_rel32_32 {
                    instruction.near_branch32()
                } else {
                    self.read_operand(self.operand(instruction, 0)?, memory)?
                };
                self.push(memory, next)?;
                return Ok(target);
            }
            Code::Jmp_rm32 => return self.read_operand(self.operand(instruction, 0)?, memory),
            Code::Retnd | Code::Retnd_imm16 => {
                let target = self.pop(memory)?;
                if instruction.code() == Code::Retnd_imm16 {
                    self.set_register(
                        Register32::Esp,
                        self.register(Register32::Esp)
                            .wrapping_add(u32::from(instruction.immediate16())),
                    );
                }
                return Ok(target);
            }
            Code::Leaved => {
                let frame = self.register(Register32::Ebp);
                let saved = read_dword(memory, frame)?;
                self.set_register(Register32::Esp, frame.wrapping_add(4));
                self.set_register(Register32::Ebp, saved);
            }
            _ => return Err(StopReason::UnsupportedInstruction),
        }
        Ok(next)
    }

    fn push(&mut self, memory: &mut GuestMemory, value: u32) -> Result<(), StopReason> {
        let address = self.register(Register32::Esp).wrapping_sub(4);
        write_dword(memory, address, value)?;
        self.set_register(Register32::Esp, address);
        Ok(())
    }

    fn pop(&mut self, memory: &GuestMemory) -> Result<u32, StopReason> {
        let address = self.register(Register32::Esp);
        let value = read_dword(memory, address)?;
        self.set_register(Register32::Esp, address.wrapping_add(4));
        Ok(value)
    }
}
