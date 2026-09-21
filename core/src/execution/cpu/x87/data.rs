use iced_x86::{Code, Instruction, MemorySize, Register};

use super::{Cpu32, GuestMemory, MemoryError, StopReason};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Stack {
    values: [u64; 8],
    top: u8,
    occupied: u8,
}

impl Stack {
    fn value(self) -> Result<f64, StopReason> {
        if self.occupied == 0 {
            return Err(StopReason::UnsupportedInstruction);
        }
        Ok(f64::from_bits(self.values[usize::from(self.top)]))
    }

    fn push(&mut self, value: f64) -> Result<(), StopReason> {
        if self.occupied == 8 {
            return Err(StopReason::UnsupportedInstruction);
        }
        self.top = self.top.wrapping_sub(1) & 7;
        self.values[usize::from(self.top)] = value.to_bits();
        self.occupied += 1;
        Ok(())
    }

    fn pop(&mut self) {
        self.values[usize::from(self.top)] = 0;
        self.top = (self.top + 1) & 7;
        self.occupied -= 1;
    }
}

impl Cpu32 {
    pub(crate) fn pop_x87_truncated_integer(&mut self) -> Option<i64> {
        if self.x87_control_word & 0x3f != 0x3f {
            return None;
        }
        let value = self.x87_stack.value().ok()?;
        // the positive i64 limit rounds to 2^63 in binary64 and must stay exclusive.
        if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value) {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "checked truncation toward zero"
        )]
        let integer = value as i64;
        self.x87_stack.pop();
        Some(integer)
    }

    pub(in super::super) fn x87_arithmetic(
        &mut self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<(), StopReason> {
        self.x87_profile()?;
        let top = self.x87_stack.value()?;
        let result = if instruction.code() == Code::Fsqrt {
            if top < 0.0 {
                return Err(StopReason::UnsupportedInstruction);
            }
            top.sqrt()
        } else {
            let source = self.read_float(instruction, memory)?;
            let multiply = matches!(instruction.code(), Code::Fmul_m32fp | Code::Fmul_m64fp);
            let (result, exact_zero) = if multiply {
                (top * source, top == 0.0 || source == 0.0)
            } else {
                if top == 0.0 {
                    return Err(StopReason::UnsupportedInstruction);
                }
                (source / top, source == 0.0)
            };
            // binary64's bottom binade cannot stand in for x87's extended exponent range.
            if !result.is_finite() || (!exact_zero && result.abs() < 2.0 * f64::MIN_POSITIVE) {
                return Err(StopReason::UnsupportedInstruction);
            }
            result
        };
        self.x87_stack.values[usize::from(self.x87_stack.top)] = result.to_bits();
        Ok(())
    }

    pub(in super::super) fn x87_integer_load(
        &mut self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<(), StopReason> {
        self.x87_profile()?;
        let (address, size) = self.data_address(instruction)?;
        let mut bytes = [0; 8];
        memory
            .read(u64::from(address), &mut bytes[..size])
            .map_err(StopReason::MemoryFault)?;
        let integer = match size {
            2 => i64::from(i16::from_le_bytes(
                bytes[..2].try_into().expect("integer width"),
            )),
            4 => i64::from(i32::from_le_bytes(
                bytes[..4].try_into().expect("integer width"),
            )),
            _ => i64::from_le_bytes(bytes),
        };
        // fild is exact even when the arithmetic precision control requests fewer bits.
        if !(-(1_i64 << 53)..=(1_i64 << 53)).contains(&integer) {
            return Err(StopReason::UnsupportedInstruction);
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "bounded exact integer conversion"
        )]
        let value = integer as f64;
        self.x87_stack.push(value)
    }

    pub(in super::super) fn x87_transfer(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        self.x87_profile()?;
        if matches!(instruction.code(), Code::Fld_m32fp | Code::Fld_m64fp) {
            let value = self.read_float(instruction, memory)?;
            return self.x87_stack.push(value);
        }
        let value = self.x87_stack.value()?;
        let (address, size) = self.data_address(instruction)?;
        let mut bytes = value.to_le_bytes();
        if size == 4 {
            if value != 0.0
                && !(f64::from(f32::MIN_POSITIVE)..=f64::from(f32::MAX)).contains(&value.abs())
            {
                return Err(StopReason::UnsupportedInstruction);
            }
            #[expect(
                clippy::cast_possible_truncation,
                reason = "checked nearest-even narrowing"
            )]
            let narrowed = value as f32;
            bytes[..4].copy_from_slice(&narrowed.to_le_bytes());
        }
        memory
            .write(u64::from(address), &bytes[..size])
            .map_err(StopReason::MemoryFault)?;
        if matches!(instruction.code(), Code::Fstp_m32fp | Code::Fstp_m64fp) {
            self.x87_stack.pop();
        }
        Ok(())
    }

    fn x87_profile(&self) -> Result<(), StopReason> {
        // status and exception delivery remain unsupported; never expose a fabricated status word.
        if self.x87_control_word & 0x0f3f != 0x023f {
            return Err(StopReason::UnsupportedInstruction);
        }
        Ok(())
    }

    fn data_address(&self, instruction: &Instruction) -> Result<(u32, usize), StopReason> {
        let size = match instruction.memory_size() {
            MemorySize::Int16 => 2,
            MemorySize::Float32 | MemorySize::Int32 => 4,
            MemorySize::Float64 | MemorySize::Int64 => 8,
            _ => return Err(StopReason::UnsupportedInstruction),
        };
        let offset = self.effective_address(instruction)?;
        let base = if instruction.memory_segment() == Register::FS {
            self.fs_base
        } else {
            0
        };
        let address = base.wrapping_add(offset);
        address
            .checked_add(u32::try_from(size - 1).expect("x87 operand width"))
            .ok_or(StopReason::MemoryFault(MemoryError::AddressOverflow))?;
        Ok((address, size))
    }

    fn read_float(
        &self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<f64, StopReason> {
        let (address, size) = self.data_address(instruction)?;
        let mut bytes = [0; 8];
        memory
            .read(u64::from(address), &mut bytes[..size])
            .map_err(StopReason::MemoryFault)?;
        let value = if size == 4 {
            let value = f32::from_le_bytes(bytes[..4].try_into().expect("float width"));
            if !value.is_normal() && value != 0.0 {
                return Err(StopReason::UnsupportedInstruction);
            }
            f64::from(value)
        } else {
            f64::from_le_bytes(bytes)
        };
        if !value.is_normal() && value != 0.0 {
            return Err(StopReason::UnsupportedInstruction);
        }
        Ok(value)
    }
}
