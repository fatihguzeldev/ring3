use iced_x86::{Code, Decoder, DecoderError, DecoderOptions, Instruction, Mnemonic, Register};

use super::{GuestMemory, MemoryError};

mod branches;
mod division;
mod flag_stack;
mod identification;
mod operands;
mod register_stack;
mod shifts;
mod stack;
mod strings;
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
    x87_stack: x87::Stack,
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
    /// zero divisor or a quotient outside its signed or unsigned destination range.
    DivideError,
    MemoryFault(MemoryError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunResult {
    pub reason: StopReason,
    /// execution steps; each repeated string element counts as one step.
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
            x87_stack: x87::Stack::default(),
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

    /// sets raw control state; x87 data operations admit only their documented profile.
    pub fn set_x87_control_word(&mut self, value: u16) {
        self.x87_control_word = value;
    }

    pub fn run(&mut self, memory: &mut GuestMemory, instruction_limit: u64) -> RunResult {
        self.run_until(memory, instruction_limit, |_| false)
    }

    /// suspends before fetching an address selected by the caller. interception
    /// leaves cpu state unchanged and consumes no step. a zero budget
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
        if has_unsupported_prefix(instruction) {
            return Err(StopReason::UnsupportedInstruction);
        }
        let mut next = instruction.next_ip32();
        match instruction.code() {
            Code::Cpuid => self.identify(),
            code if is_move(code) => {
                let destination = self.operand(instruction, 0)?;
                let source = self.operand(instruction, 1)?;
                let mut value = self.read_operand(source, memory)?;
                if instruction.mnemonic() == Mnemonic::Movsx {
                    let shift = 32 - source.width as u32 * 8;
                    value = ((value << shift).cast_signed() >> shift).cast_unsigned();
                }
                self.write_operand(destination, value, memory)?;
            }
            Code::Lea_r32_m => {
                let destination = register32(instruction.op0_register())?;
                self.set_register(destination, self.effective_address(instruction)?);
            }
            code if is_binary(code) || is_sbb(code) => self.binary(instruction, memory)?,
            Code::Not_rm8 | Code::Not_rm16 | Code::Not_rm32 => {
                let destination = self.operand(instruction, 0)?;
                let input = self.read_operand(destination, memory)?;
                self.write_operand(destination, !input & destination.width.mask(), memory)?;
            }
            Code::Neg_rm8 | Code::Neg_rm16 | Code::Neg_rm32 => self.negate(instruction, memory)?,
            code if shifts::is_shift(code) => self.shift(instruction, memory)?,
            Code::Imul_r16_rm16
            | Code::Imul_r32_rm32
            | Code::Imul_r16_rm16_imm16
            | Code::Imul_r32_rm32_imm32
            | Code::Imul_r16_rm16_imm8
            | Code::Imul_r32_rm32_imm8 => self.multiply(instruction, memory)?,
            Code::Imul_rm8 | Code::Imul_rm16 | Code::Imul_rm32 => {
                self.multiply_wide(instruction, memory)?;
            }
            Code::Mul_rm32 => self.multiply_unsigned_dword(instruction, memory)?,
            Code::Div_rm8 | Code::Div_rm16 | Code::Div_rm32 => self.divide(instruction, memory)?,
            Code::Idiv_rm32 => self.divide_signed_dword(instruction, memory)?,
            Code::Cdq => self.sign_extend_accumulator(),
            Code::Inc_rm8
            | Code::Inc_rm16
            | Code::Inc_rm32
            | Code::Inc_r16
            | Code::Inc_r32
            | Code::Dec_rm8
            | Code::Dec_rm16
            | Code::Dec_rm32
            | Code::Dec_r16
            | Code::Dec_r32 => self.increment(instruction, memory)?,
            code if stack::is_stack(code) => {
                next = self.stack_instruction(instruction, memory)?;
            }
            Code::Jmp_rel8_32 | Code::Jmp_rel32_32 => next = instruction.near_branch32(),
            Code::Movsb_m8_m8 | Code::Movsw_m16_m16 | Code::Movsd_m32_m32 => {
                next = self.move_string(instruction, memory)?;
            }
            code if strings::is_comparison(code) => {
                next = self.compare_string(instruction, memory)?;
            }
            Code::Stosb_m8_AL | Code::Stosw_m16_AX | Code::Stosd_m32_EAX => {
                next = self.store_string(instruction, memory)?;
            }
            Code::Fldcw_m2byte | Code::Fnstcw_m2byte => self.x87_control(instruction, memory)?,
            Code::Fcom_m32fp | Code::Fcom_m64fp | Code::Fcomp_m32fp | Code::Fcomp_m64fp => {
                self.x87_compare(instruction, memory)?;
            }
            Code::Fnstsw_AX | Code::Fnstsw_m2byte => self.x87_store_status(instruction, memory)?,
            Code::Fldz => self.x87_load_zero()?,
            Code::Fld_m32fp
            | Code::Fld_m64fp
            | Code::Fst_m32fp
            | Code::Fst_m64fp
            | Code::Fstp_m32fp
            | Code::Fstp_m64fp => self.x87_transfer(instruction, memory)?,
            code if x87::is_register_transfer(code) => self.x87_register_transfer(instruction)?,
            code if x87::is_unary(code) => self.x87_unary(code)?,
            code if x87::is_register_add_sub(code) => self.x87_register_add_sub(instruction)?,
            code if x87::is_binary_pop(code) => self.x87_binary_pop(instruction)?,
            Code::Fild_m16int | Code::Fild_m32int | Code::Fild_m64int => {
                self.x87_integer_load(instruction, memory)?;
            }
            Code::Fist_m16int
            | Code::Fist_m32int
            | Code::Fistp_m16int
            | Code::Fistp_m32int
            | Code::Fistp_m64int => {
                self.x87_integer_store(instruction, memory)?;
            }
            code if x87::is_arithmetic(code) => self.x87_arithmetic(instruction, memory)?,
            Code::Wait if self.x87_control_word & 0x3f == 0x3f => {}
            Code::Nopd | Code::Int3 => {}
            _ => next = self.conditional_instruction(instruction, memory)?,
        }
        self.eip = next;
        Ok(instruction.code() == Code::Int3)
    }

    fn sign_extend_accumulator(&mut self) {
        let high = (self.register(Register32::Eax).cast_signed() >> 31).cast_unsigned();
        self.set_register(Register32::Edx, high);
    }

    fn negate(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        let destination = self.operand(instruction, 0)?;
        let input = self.read_operand(destination, memory)?;
        let result = 0_u32.wrapping_sub(input) & destination.width.mask();
        self.write_operand(destination, result, memory)?;
        self.arithmetic_flags(0, input, result, input != 0, true, destination.width);
        Ok(())
    }

    fn binary(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        let destination = self.operand(instruction, 0)?;
        let left = self.read_operand(destination, memory)?;
        let width = destination.width;
        let right = self.read_operand(self.operand(instruction, 1)?, memory)? & width.mask();
        let operation = instruction.mnemonic();
        let subtract = matches!(operation, Mnemonic::Sub | Mnemonic::Cmp | Mnemonic::Sbb);
        let (result, carry) = match operation {
            Mnemonic::Xor => (left ^ right, false),
            Mnemonic::Or => (left | right, false),
            Mnemonic::And | Mnemonic::Test => (left & right, false),
            Mnemonic::Sub | Mnemonic::Cmp => left.overflowing_sub(right),
            Mnemonic::Sbb => {
                let borrow = self.eflags & 1;
                (
                    left.wrapping_sub(right).wrapping_sub(borrow),
                    u64::from(left) < u64::from(right) + u64::from(borrow),
                )
            }
            _ => (
                left.wrapping_add(right),
                u64::from(left) + u64::from(right) > u64::from(width.mask()),
            ),
        };
        let result = result & width.mask();
        if !matches!(operation, Mnemonic::Cmp | Mnemonic::Test) {
            self.write_operand(destination, result, memory)?;
        }
        if matches!(
            operation,
            Mnemonic::Xor | Mnemonic::Or | Mnemonic::And | Mnemonic::Test
        ) {
            self.eflags = (self.eflags & !0x8d5) | result_flags(result, width);
        } else {
            self.arithmetic_flags(left, right, result, carry, subtract, width);
        }
        Ok(())
    }

    fn increment(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        let destination = self.operand(instruction, 0)?;
        let left = self.read_operand(destination, memory)?;
        let subtract = instruction.mnemonic() == Mnemonic::Dec;
        let result = if subtract {
            left.wrapping_sub(1)
        } else {
            left.wrapping_add(1)
        } & destination.width.mask();
        self.write_operand(destination, result, memory)?;
        self.arithmetic_flags(
            left,
            1,
            result,
            self.eflags & 1 != 0,
            subtract,
            destination.width,
        );
        Ok(())
    }

    fn multiply(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        let destination = self.operand(instruction, 0)?;
        let mask = destination.width.mask();
        let shift = 32 - destination.width as u32 * 8;
        let signed = |value: u32| i64::from(((value & mask) << shift).cast_signed() >> shift);
        let first = u32::from(instruction.op_count() == 3);
        let left = signed(self.read_operand(self.operand(instruction, first)?, memory)?);
        let right = signed(self.read_operand(self.operand(instruction, first + 1)?, memory)?);
        let product = left * right;
        let result = u32::try_from(product.cast_unsigned() & u64::from(mask))
            .expect("masked product fits u32");
        self.write_operand(destination, result, memory)?;
        // sf/zf/af/pf are undefined and deterministically preserved.
        self.eflags = (self.eflags & !0x801) | if product == signed(result) { 0 } else { 0x801 };
        Ok(())
    }

    fn multiply_wide(
        &mut self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<(), StopReason> {
        let source = self.operand(instruction, 0)?;
        let bits = source.width as u32 * 8;
        let mask = source.width.mask();
        let signed =
            |value: u32| i64::from(((value & mask) << (32 - bits)).cast_signed() >> (32 - bits));
        let accumulator = self.register(Register32::Eax);
        let product = signed(accumulator) * signed(self.read_operand(source, memory)?);
        let destination_mask = if bits == 8 { 0xffff } else { mask };
        let low = u32::try_from(product.cast_unsigned() & u64::from(destination_mask))
            .expect("masked product fits u32");
        self.set_register(Register32::Eax, (accumulator & !destination_mask) | low);
        if bits != 8 {
            let high = u32::try_from((product.cast_unsigned() >> bits) & u64::from(mask))
                .expect("masked product fits u32");
            self.set_register(
                Register32::Edx,
                (self.register(Register32::Edx) & !mask) | high,
            );
        }
        self.eflags = (self.eflags & !0x801) | if product == signed(low) { 0 } else { 0x801 };
        Ok(())
    }

    fn multiply_unsigned_dword(
        &mut self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<(), StopReason> {
        let source = self.read_operand(self.operand(instruction, 0)?, memory)?;
        let product = u64::from(self.register(Register32::Eax)) * u64::from(source);
        let low = u32::try_from(product & u64::from(u32::MAX)).expect("masked product fits u32");
        let high = u32::try_from(product >> 32).expect("high product fits u32");
        self.set_register(Register32::Eax, low);
        self.set_register(Register32::Edx, high);
        self.eflags = (self.eflags & !0x801) | if high == 0 { 0 } else { 0x801 };
        Ok(())
    }

    fn arithmetic_flags(
        &mut self,
        left: u32,
        right: u32,
        result: u32,
        carry: bool,
        subtract: bool,
        width: operands::Width,
    ) {
        let overflow = if subtract {
            (left ^ right) & (left ^ result)
        } else {
            !(left ^ right) & (left ^ result)
        } & width.sign_bit()
            != 0;
        self.eflags = (self.eflags & !0x8d5)
            | u32::from(carry)
            | result_flags(result, width)
            | ((left ^ right ^ result) & 0x10)
            | (u32::from(overflow) << 11);
    }
}

fn result_flags(result: u32, width: operands::Width) -> u32 {
    (u32::from((result & 0xff).count_ones().is_multiple_of(2)) << 2)
        | (u32::from(result == 0) << 6)
        | (u32::from(result & width.sign_bit() != 0) << 7)
}

fn has_unsupported_prefix(instruction: &Instruction) -> bool {
    instruction.has_lock_prefix()
        || ((instruction.has_rep_prefix() || instruction.has_repne_prefix())
            && !strings::supports_repeat(instruction))
        || (instruction.has_segment_prefix() && instruction.segment_prefix() != Register::FS)
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

fn is_move(code: Code) -> bool {
    matches!(
        code,
        Code::Mov_r8_imm8
            | Code::Mov_r8_rm8
            | Code::Mov_rm8_r8
            | Code::Mov_rm8_imm8
            | Code::Mov_AL_moffs8
            | Code::Mov_moffs8_AL
            | Code::Mov_r16_imm16
            | Code::Mov_r16_rm16
            | Code::Mov_rm16_r16
            | Code::Mov_rm16_imm16
            | Code::Mov_AX_moffs16
            | Code::Mov_moffs16_AX
            | Code::Mov_r32_imm32
            | Code::Mov_r32_rm32
            | Code::Mov_rm32_r32
            | Code::Mov_rm32_imm32
            | Code::Mov_EAX_moffs32
            | Code::Mov_moffs32_EAX
            | Code::Movzx_r16_rm8
            | Code::Movzx_r32_rm8
            | Code::Movzx_r32_rm16
            | Code::Movsx_r16_rm8
            | Code::Movsx_r32_rm8
            | Code::Movsx_r32_rm16
    )
}

fn is_binary(code: Code) -> bool {
    matches!(
        code,
        Code::Add_AL_imm8
            | Code::Add_rm8_imm8
            | Code::Add_rm8_r8
            | Code::Add_r8_rm8
            | Code::Add_AX_imm16
            | Code::Add_rm16_imm16
            | Code::Add_rm16_r16
            | Code::Add_r16_rm16
            | Code::Add_rm16_imm8
            | Code::Add_EAX_imm32
            | Code::Add_rm32_imm32
            | Code::Add_rm32_r32
            | Code::Add_r32_rm32
            | Code::Add_rm32_imm8
            | Code::Sub_AL_imm8
            | Code::Sub_rm8_imm8
            | Code::Sub_rm8_r8
            | Code::Sub_r8_rm8
            | Code::Sub_AX_imm16
            | Code::Sub_rm16_imm16
            | Code::Sub_rm16_r16
            | Code::Sub_r16_rm16
            | Code::Sub_rm16_imm8
            | Code::Sub_EAX_imm32
            | Code::Sub_rm32_imm32
            | Code::Sub_rm32_r32
            | Code::Sub_r32_rm32
            | Code::Sub_rm32_imm8
            | Code::Cmp_AL_imm8
            | Code::Cmp_rm8_imm8
            | Code::Cmp_rm8_r8
            | Code::Cmp_r8_rm8
            | Code::Cmp_AX_imm16
            | Code::Cmp_rm16_imm16
            | Code::Cmp_rm16_r16
            | Code::Cmp_r16_rm16
            | Code::Cmp_rm16_imm8
            | Code::Cmp_EAX_imm32
            | Code::Cmp_rm32_imm32
            | Code::Cmp_rm32_r32
            | Code::Cmp_r32_rm32
            | Code::Cmp_rm32_imm8
            | Code::Xor_AL_imm8
            | Code::Xor_rm8_imm8
            | Code::Xor_rm8_r8
            | Code::Xor_r8_rm8
            | Code::Xor_AX_imm16
            | Code::Xor_rm16_imm16
            | Code::Xor_rm16_r16
            | Code::Xor_r16_rm16
            | Code::Xor_rm16_imm8
            | Code::Xor_EAX_imm32
            | Code::Xor_rm32_imm32
            | Code::Xor_rm32_r32
            | Code::Xor_r32_rm32
            | Code::Xor_rm32_imm8
            | Code::Or_AL_imm8
            | Code::Or_rm8_imm8
            | Code::Or_rm8_r8
            | Code::Or_r8_rm8
            | Code::Or_AX_imm16
            | Code::Or_rm16_imm16
            | Code::Or_rm16_r16
            | Code::Or_r16_rm16
            | Code::Or_rm16_imm8
            | Code::Or_EAX_imm32
            | Code::Or_rm32_imm32
            | Code::Or_rm32_r32
            | Code::Or_r32_rm32
            | Code::Or_rm32_imm8
            | Code::And_AL_imm8
            | Code::And_rm8_imm8
            | Code::And_rm8_r8
            | Code::And_r8_rm8
            | Code::And_AX_imm16
            | Code::And_rm16_imm16
            | Code::And_rm16_r16
            | Code::And_r16_rm16
            | Code::And_rm16_imm8
            | Code::And_EAX_imm32
            | Code::And_rm32_imm32
            | Code::And_rm32_r32
            | Code::And_r32_rm32
            | Code::And_rm32_imm8
            | Code::Test_AL_imm8
            | Code::Test_rm8_imm8
            | Code::Test_rm8_r8
            | Code::Test_AX_imm16
            | Code::Test_rm16_imm16
            | Code::Test_rm16_r16
            | Code::Test_EAX_imm32
            | Code::Test_rm32_imm32
            | Code::Test_rm32_r32
    )
}

fn is_sbb(code: Code) -> bool {
    matches!(
        code,
        Code::Sbb_AL_imm8
            | Code::Sbb_rm8_imm8
            | Code::Sbb_rm8_r8
            | Code::Sbb_r8_rm8
            | Code::Sbb_AX_imm16
            | Code::Sbb_rm16_imm16
            | Code::Sbb_rm16_r16
            | Code::Sbb_r16_rm16
            | Code::Sbb_rm16_imm8
            | Code::Sbb_EAX_imm32
            | Code::Sbb_rm32_imm32
            | Code::Sbb_rm32_r32
            | Code::Sbb_r32_rm32
            | Code::Sbb_rm32_imm8
    )
}
