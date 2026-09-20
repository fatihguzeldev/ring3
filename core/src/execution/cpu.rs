use iced_x86::{Code, Decoder, DecoderError, DecoderOptions, Instruction, OpKind, Register};

use super::{GuestMemory, MemoryError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register32 {
    Eax,
    Ecx,
    Edx,
    Ebx,
    Esp,
    Ebp,
    Esi,
    Edi,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cpu32 {
    registers: [u32; 8],
    pub eip: u32,
    pub eflags: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StopReason {
    InstructionLimit,
    Breakpoint,
    UnsupportedInstruction,
    InvalidInstruction,
    MemoryFault(MemoryError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunResult {
    pub reason: StopReason,
    pub instructions: u64,
    pub instruction_pointer: u32,
}

impl Cpu32 {
    #[must_use]
    pub fn new(entry_point: u32) -> Self {
        Self {
            registers: [0; 8],
            eip: entry_point,
            eflags: 2,
        }
    }

    #[must_use]
    pub fn register(&self, register: Register32) -> u32 {
        self.registers[register as usize]
    }

    pub fn set_register(&mut self, register: Register32, value: u32) {
        self.registers[register as usize] = value;
    }

    pub fn run(&mut self, memory: &mut GuestMemory, instruction_limit: u64) -> RunResult {
        for executed in 0..instruction_limit {
            let start = self.eip;
            let result = decode(memory, start).and_then(|instruction| self.execute(&instruction));
            match result {
                Ok(false) => {}
                Ok(true) => {
                    return RunResult {
                        reason: StopReason::Breakpoint,
                        instructions: executed + 1,
                        instruction_pointer: start,
                    };
                }
                Err(reason) => {
                    return RunResult {
                        reason,
                        instructions: executed,
                        instruction_pointer: start,
                    };
                }
            }
        }
        RunResult {
            reason: StopReason::InstructionLimit,
            instructions: instruction_limit,
            instruction_pointer: self.eip,
        }
    }

    fn execute(&mut self, instruction: &Instruction) -> Result<bool, StopReason> {
        if instruction.has_lock_prefix()
            || instruction.has_rep_prefix()
            || instruction.has_repne_prefix()
            || instruction.has_segment_prefix()
        {
            return Err(StopReason::UnsupportedInstruction);
        }
        let mut next = instruction.next_ip32();
        match instruction.code() {
            Code::Mov_r32_imm32 => {
                let destination = register32(instruction.op0_register())?;
                self.set_register(destination, instruction.immediate32());
            }
            Code::Mov_r32_rm32 | Code::Mov_rm32_r32
                if instruction.op0_kind() == OpKind::Register
                    && instruction.op1_kind() == OpKind::Register =>
            {
                let destination = register32(instruction.op0_register())?;
                let source = register32(instruction.op1_register())?;
                self.set_register(destination, self.register(source));
            }
            Code::Add_EAX_imm32
            | Code::Add_rm32_imm32
            | Code::Add_rm32_imm8
            | Code::Sub_EAX_imm32
            | Code::Sub_rm32_imm32
            | Code::Sub_rm32_imm8
            | Code::Cmp_EAX_imm32
            | Code::Cmp_rm32_imm32
            | Code::Cmp_rm32_imm8
                if instruction.op0_kind() == OpKind::Register =>
            {
                let destination = register32(instruction.op0_register())?;
                let left = self.register(destination);
                let right = if instruction.op1_kind() == OpKind::Immediate8to32 {
                    instruction.immediate8to32().cast_unsigned()
                } else {
                    instruction.immediate32()
                };
                let subtract = matches!(
                    instruction.code(),
                    Code::Sub_EAX_imm32
                        | Code::Sub_rm32_imm32
                        | Code::Sub_rm32_imm8
                        | Code::Cmp_EAX_imm32
                        | Code::Cmp_rm32_imm32
                        | Code::Cmp_rm32_imm8
                );
                let (result, carry) = if subtract {
                    left.overflowing_sub(right)
                } else {
                    left.overflowing_add(right)
                };
                self.arithmetic_flags(left, right, result, carry, subtract);
                if !matches!(
                    instruction.code(),
                    Code::Cmp_EAX_imm32 | Code::Cmp_rm32_imm32 | Code::Cmp_rm32_imm8
                ) {
                    self.set_register(destination, result);
                }
            }
            Code::Jmp_rel8_32 | Code::Jmp_rel32_32 => next = instruction.near_branch32(),
            Code::Je_rel8_32 | Code::Je_rel32_32 => {
                if self.eflags & 0x40 != 0 {
                    next = instruction.near_branch32();
                }
            }
            Code::Jne_rel8_32 | Code::Jne_rel32_32 => {
                if self.eflags & 0x40 == 0 {
                    next = instruction.near_branch32();
                }
            }
            Code::Nopd | Code::Int3 => {}
            _ => return Err(StopReason::UnsupportedInstruction),
        }
        self.eip = next;
        Ok(instruction.code() == Code::Int3)
    }

    fn arithmetic_flags(
        &mut self,
        left: u32,
        right: u32,
        result: u32,
        carry: bool,
        subtract: bool,
    ) {
        let overflow = if subtract {
            (left ^ right) & (left ^ result)
        } else {
            !(left ^ right) & (left ^ result)
        } & 0x8000_0000
            != 0;
        self.eflags = (self.eflags & !0x8d5)
            | u32::from(carry)
            | (u32::from((result & 0xff).count_ones().is_multiple_of(2)) << 2)
            | ((left ^ right ^ result) & 0x10)
            | (u32::from(result == 0) << 6)
            | ((result >> 24) & 0x80)
            | (u32::from(overflow) << 11);
    }
}

fn register32(register: Register) -> Result<Register32, StopReason> {
    match register {
        Register::EAX => Ok(Register32::Eax),
        Register::ECX => Ok(Register32::Ecx),
        Register::EDX => Ok(Register32::Edx),
        Register::EBX => Ok(Register32::Ebx),
        Register::ESP => Ok(Register32::Esp),
        Register::EBP => Ok(Register32::Ebp),
        Register::ESI => Ok(Register32::Esi),
        Register::EDI => Ok(Register32::Edi),
        _ => Err(StopReason::UnsupportedInstruction),
    }
}

fn decode(memory: &GuestMemory, ip: u32) -> Result<Instruction, StopReason> {
    let mut bytes = [0; 15];
    for length in 1_u8..=15 {
        let index = usize::from(length - 1);
        let address = ip
            .checked_add(u32::from(length - 1))
            .ok_or(StopReason::MemoryFault(MemoryError::AddressOverflow))?;
        memory
            .fetch(u64::from(address), &mut bytes[index..=index])
            .map_err(StopReason::MemoryFault)?;
        let mut decoder = Decoder::with_ip(
            32,
            &bytes[..usize::from(length)],
            u64::from(ip),
            DecoderOptions::NONE,
        );
        let instruction = decoder.decode();
        match decoder.last_error() {
            DecoderError::None => return Ok(instruction),
            DecoderError::NoMoreBytes => {}
            _ => return Err(StopReason::InvalidInstruction),
        }
    }
    Err(StopReason::InvalidInstruction)
}
