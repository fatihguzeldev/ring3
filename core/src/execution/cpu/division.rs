use super::{Cpu32, GuestMemory, Instruction, Register32, StopReason};

impl Cpu32 {
    pub(super) fn divide_signed_dword(
        &mut self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<(), StopReason> {
        let source = self.operand(instruction, 0)?;
        let divisor = i64::from(self.read_operand(source, memory)?.cast_signed());
        let dividend = ((u64::from(self.register(Register32::Edx)) << 32)
            | u64::from(self.register(Register32::Eax)))
        .cast_signed();
        let quotient = dividend
            .checked_div(divisor)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or(StopReason::DivideError)?;
        let remainder = i32::try_from(dividend % divisor).expect("remainder fits signed divisor");
        self.set_register(Register32::Eax, quotient.cast_unsigned());
        self.set_register(Register32::Edx, remainder.cast_unsigned());
        // arithmetic flags are undefined and deterministically preserved.
        Ok(())
    }

    pub(super) fn divide(
        &mut self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<(), StopReason> {
        let source = self.operand(instruction, 0)?;
        let divisor = u64::from(self.read_operand(source, memory)?);
        let bits = source.width as u32 * 8;
        let mask = source.width.mask();
        let low = self.register(Register32::Eax);
        let high = self.register(Register32::Edx);
        let dividend = if bits == 8 {
            u64::from(low & 0xffff)
        } else {
            (u64::from(high & mask) << bits) | u64::from(low & mask)
        };
        let quotient = dividend
            .checked_div(divisor)
            .ok_or(StopReason::DivideError)?;
        if quotient > u64::from(mask) {
            return Err(StopReason::DivideError);
        }
        let remainder = u32::try_from(dividend % divisor).expect("remainder fits source width");
        let quotient = u32::try_from(quotient).expect("validated quotient width");
        if bits == 8 {
            self.set_register(
                Register32::Eax,
                (low & !0xffff) | (remainder << 8) | quotient,
            );
        } else {
            self.set_register(Register32::Eax, (low & !mask) | quotient);
            self.set_register(Register32::Edx, (high & !mask) | remainder);
        }
        // arithmetic flags are undefined and deterministically preserved.
        Ok(())
    }
}
