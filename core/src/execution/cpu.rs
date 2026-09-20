use iced_x86::{Code, Decoder, DecoderError, DecoderOptions, Instruction, Mnemonic, Register};

use super::{GuestMemory, MemoryError};

mod operands;
mod stack;
mod x87;

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
    fs_base: u32,
    x87_control_word: u16,
    pub eip: u32,
    pub eflags: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StopReason {
    InstructionLimit,
    Breakpoint,
    Intercepted,
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
            fs_base: 0,
            x87_control_word: 0x037f,
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

    #[must_use]
    pub fn fs_base(&self) -> u32 {
        self.fs_base
    }

    /// configures the flat 32-bit fs base; selector loading is not emulated.
    pub fn set_fs_base(&mut self, base: u32) {
        self.fs_base = base;
    }

    #[must_use]
    pub fn x87_control_word(&self) -> u16 {
        self.x87_control_word
    }

    /// sets raw control state; floating-point arithmetic is not yet implemented.
    pub fn set_x87_control_word(&mut self, value: u16) {
        self.x87_control_word = value;
    }

    pub fn run(&mut self, memory: &mut GuestMemory, instruction_limit: u64) -> RunResult {
        self.run_until(memory, instruction_limit, |_| false)
    }

    /// suspends before fetching an address selected by the caller. interception
    /// leaves cpu state unchanged and consumes no instruction. a zero budget
    /// returns the instruction limit without consulting the predicate.
    pub fn run_until(
        &mut self,
        memory: &mut GuestMemory,
        instruction_limit: u64,
        mut intercept: impl FnMut(u32) -> bool,
    ) -> RunResult {
        for executed in 0..instruction_limit {
            let start = self.eip;
            if intercept(start) {
                return RunResult {
                    reason: StopReason::Intercepted,
                    instructions: executed,
                    instruction_pointer: start,
                };
            }
            let before = *self;
            let result =
                decode(memory, start).and_then(|instruction| self.execute(&instruction, memory));
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
                    *self = before;
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

    fn execute(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<bool, StopReason> {
        if instruction.has_lock_prefix()
            || instruction.has_rep_prefix()
            || instruction.has_repne_prefix()
            || (instruction.has_segment_prefix() && instruction.segment_prefix() != Register::FS)
        {
            return Err(StopReason::UnsupportedInstruction);
        }
        let mut next = instruction.next_ip32();
        match instruction.code() {
            Code::Mov_r32_imm32
            | Code::Mov_r32_rm32
            | Code::Mov_rm32_r32
            | Code::Mov_rm32_imm32
            | Code::Mov_EAX_moffs32
            | Code::Mov_moffs32_EAX => {
                let destination = self.operand(instruction, 0)?;
                let value = self.read_operand(self.operand(instruction, 1)?, memory)?;
                self.write_operand(destination, value, memory)?;
            }
            Code::Lea_r32_m => {
                let destination = register32(instruction.op0_register())?;
                self.set_register(destination, self.effective_address(instruction)?);
            }
            Code::Add_EAX_imm32
            | Code::Add_rm32_imm32
            | Code::Add_rm32_imm8
            | Code::Add_rm32_r32
            | Code::Add_r32_rm32
            | Code::Sub_EAX_imm32
            | Code::Sub_rm32_imm32
            | Code::Sub_rm32_imm8
            | Code::Sub_rm32_r32
            | Code::Sub_r32_rm32
            | Code::Cmp_EAX_imm32
            | Code::Cmp_rm32_imm32
            | Code::Cmp_rm32_imm8
            | Code::Cmp_rm32_r32
            | Code::Cmp_r32_rm32
            | Code::Xor_EAX_imm32
            | Code::Xor_rm32_imm32
            | Code::Xor_rm32_imm8
            | Code::Xor_rm32_r32
            | Code::Xor_r32_rm32
            | Code::Or_EAX_imm32
            | Code::Or_rm32_imm32
            | Code::Or_rm32_imm8
            | Code::Or_rm32_r32
            | Code::Or_r32_rm32 => self.binary(instruction, memory)?,
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
            | Code::Leaved => {
                next = self.stack_instruction(instruction, memory)?;
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
            Code::Fldcw_m2byte | Code::Fnstcw_m2byte => self.x87_control(instruction, memory)?,
            Code::Nopd | Code::Int3 => {}
            _ => return Err(StopReason::UnsupportedInstruction),
        }
        self.eip = next;
        Ok(instruction.code() == Code::Int3)
    }

    fn binary(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        let destination = self.operand(instruction, 0)?;
        let left = self.read_operand(destination, memory)?;
        let right = self.read_operand(self.operand(instruction, 1)?, memory)?;
        let operation = instruction.mnemonic();
        let subtract = matches!(operation, Mnemonic::Sub | Mnemonic::Cmp);
        let (result, carry) = match operation {
            Mnemonic::Xor => (left ^ right, false),
            Mnemonic::Or => (left | right, false),
            Mnemonic::Sub | Mnemonic::Cmp => left.overflowing_sub(right),
            _ => left.overflowing_add(right),
        };
        if operation != Mnemonic::Cmp {
            self.write_operand(destination, result, memory)?;
        }
        if matches!(operation, Mnemonic::Xor | Mnemonic::Or) {
            self.eflags = (self.eflags & !0x8d5) | result_flags(result);
        } else {
            self.arithmetic_flags(left, right, result, carry, subtract);
        }
        Ok(())
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
            | result_flags(result)
            | ((left ^ right ^ result) & 0x10)
            | (u32::from(overflow) << 11);
    }
}

fn result_flags(result: u32) -> u32 {
    (u32::from((result & 0xff).count_ones().is_multiple_of(2)) << 2)
        | (u32::from(result == 0) << 6)
        | ((result >> 24) & 0x80)
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
