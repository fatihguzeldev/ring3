use iced_x86::{Code, Instruction, MemorySize, Register};
use std::cmp::Ordering;

use super::super::operands::Location;
use super::{Cpu32, GuestMemory, MemoryError, StopReason, rounding};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Stack {
    values: [u64; 8],
    packed: u8,
    top: u8,
    occupied: u8,
    status: u16,
}

impl Stack {
    fn value(self) -> Result<f64, StopReason> {
        self.numeric()?;
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
        self.packed &= !(1 << self.top);
        self.occupied += 1;
        self.status &= !0x0200;
        Ok(())
    }

    fn pop(&mut self) {
        self.top = (self.top + 1) & 7;
        self.occupied -= 1;
    }

    fn numeric(self) -> Result<(), StopReason> {
        if (0..self.occupied).any(|offset| self.packed & (1 << ((self.top + offset) & 7)) != 0) {
            return Err(StopReason::UnsupportedInstruction);
        }
        Ok(())
    }

    pub(in super::super) fn mmx_ready(self, control: u16) -> Result<(), StopReason> {
        if self.status & !control & 0x3f != 0 {
            return Err(StopReason::UnsupportedInstruction);
        }
        Ok(())
    }

    pub(in super::super) fn mmx_read(self, index: usize) -> u64 {
        let bits = self.values[index];
        if self.packed & (1 << index) != 0 {
            return bits;
        }
        let fraction = bits & 0x000f_ffff_ffff_ffff;
        // mmx aliases the extended significand, not the binary64 encoding.
        if bits & 0x7ff0_0000_0000_0000 != 0 {
            (1 << 63) | (fraction << 11)
        } else if fraction != 0 {
            fraction << fraction.leading_zeros()
        } else {
            0
        }
    }

    pub(in super::super) fn mmx_write(&mut self, index: usize, value: u64) {
        self.values[index] = value;
        self.packed |= 1 << index;
    }

    pub(in super::super) fn mmx_enter(&mut self) {
        self.top = 0;
        self.occupied = 8;
    }

    pub(in super::super) fn mmx_empty(&mut self) {
        self.occupied = 0;
    }

    fn rounded(&mut self, inexact: bool, up: bool) {
        self.status = (self.status & !0x0200) | (u16::from(up) << 9);
        if inexact {
            self.status |= 0x20;
        }
    }
}

impl Cpu32 {
    pub(in super::super) fn x87_load_constant(&mut self, value: f64) -> Result<(), StopReason> {
        self.x87_masked()?;
        self.x87_stack.push(value)
    }

    pub(in super::super) fn x87_unary(&mut self, code: Code) -> Result<(), StopReason> {
        match code {
            Code::Fabs | Code::Fchs => self.x87_sign(code),
            Code::Fptan => self.x87_tangent(),
            Code::Fsin | Code::Fcos => self.x87_trigonometric(code),
            _ => Err(StopReason::UnsupportedInstruction),
        }
    }

    fn x87_trigonometric(&mut self, code: Code) -> Result<(), StopReason> {
        self.x87_masked()?;
        if self.x87_control_word & 0x0c00 != 0 {
            return Err(StopReason::UnsupportedInstruction);
        }
        let angle = self.x87_stack.value()?;
        if !(angle == 0.0 || angle.is_normal()) || angle.abs() > f64::from(std::f32::consts::TAU) {
            return Err(StopReason::UnsupportedInstruction);
        }
        let result = if code == Code::Fsin {
            angle.sin()
        } else {
            angle.cos()
        };
        if !result.is_finite() || (result != 0.0 && !result.is_normal()) {
            return Err(StopReason::UnsupportedInstruction);
        }
        self.x87_stack.values[usize::from(self.x87_stack.top)] = result.to_bits();
        self.x87_stack.status &= !0x0400;
        self.x87_stack.rounded(angle != 0.0, false);
        Ok(())
    }

    fn x87_tangent(&mut self) -> Result<(), StopReason> {
        self.x87_masked()?;
        if self.x87_control_word & 0x0c00 != 0 || self.x87_stack.occupied == 8 {
            return Err(StopReason::UnsupportedInstruction);
        }
        let angle = self.x87_stack.value()?;
        if !(angle == 0.0 || angle.is_normal()) || angle.abs() >= 3.0 * std::f64::consts::PI / 8.0 {
            return Err(StopReason::UnsupportedInstruction);
        }
        let tangent = angle.tan();
        if !tangent.is_finite() {
            return Err(StopReason::UnsupportedInstruction);
        }
        let slot = usize::from(self.x87_stack.top);
        self.x87_stack.values[slot] = tangent.to_bits();
        self.x87_stack.push(1.0)?;
        self.x87_stack.status &= !0x0400;
        self.x87_stack.rounded(angle != 0.0, false);
        Ok(())
    }

    fn x87_sign(&mut self, code: Code) -> Result<(), StopReason> {
        if !matches!(self.x87_control_word & 0x0f3f, 0x003f | 0x023f) {
            return Err(StopReason::UnsupportedInstruction);
        }
        let value = self.x87_stack.value()?;
        if !value.is_finite() {
            return Err(StopReason::UnsupportedInstruction);
        }
        let slot = usize::from(self.x87_stack.top);
        self.x87_stack.values[slot] = if code == Code::Fchs {
            value.to_bits() ^ (1_u64 << 63)
        } else {
            value.to_bits() & !(1_u64 << 63)
        };
        self.x87_stack.rounded(false, false);
        Ok(())
    }

    pub(in super::super) fn x87_binary_pop(
        &mut self,
        instruction: &Instruction,
    ) -> Result<(), StopReason> {
        let single_precision = self.x87_arithmetic_precision(instruction.code())?;
        let index = (instruction.op0_register() as usize)
            .checked_sub(Register::ST0 as usize)
            .ok_or(StopReason::UnsupportedInstruction)?;
        if index == 0 || index >= usize::from(self.x87_stack.occupied) {
            return Err(StopReason::UnsupportedInstruction);
        }
        let top = self.x87_stack.value()?;
        let slot = (usize::from(self.x87_stack.top) + index) & 7;
        let indexed = f64::from_bits(self.x87_stack.values[slot]);
        let multiply = instruction.code() == Code::Fmulp_sti_st0;
        let (left, right) = if instruction.code() == Code::Fdivrp_sti_st0 {
            (top, indexed)
        } else {
            (indexed, top)
        };
        if !multiply && right == 0.0 {
            return Err(StopReason::UnsupportedInstruction);
        }
        let mut result = if multiply { left * right } else { left / right };
        let exact_zero = left == 0.0 || (multiply && right == 0.0);
        if !result.is_finite() || (!exact_zero && result.abs() < 2.0 * f64::MIN_POSITIVE) {
            return Err(StopReason::UnsupportedInstruction);
        }
        if single_precision {
            result = if multiply {
                rounding::single_product(result, left, right)
            } else {
                rounding::single_quotient(result, left, right)
            }
            .ok_or(StopReason::UnsupportedInstruction)?;
        }
        let rounding = if multiply {
            rounding::product_result(result, left, right)
        } else {
            rounding::quotient_result(result, left, right)
        };
        self.x87_stack
            .rounded(rounding != Ordering::Equal, rounding == Ordering::Greater);
        self.x87_stack.values[slot] = result.to_bits();
        self.x87_stack.pop();
        Ok(())
    }

    pub(in super::super) fn x87_register_add_sub(
        &mut self,
        instruction: &Instruction,
    ) -> Result<(), StopReason> {
        let single_precision = self.x87_arithmetic_precision(instruction.code())?;
        let destination_is_top = instruction.code() == Code::Fadd_st0_sti;
        let subtract = instruction.code() == Code::Fsubp_sti_st0;
        let reverse_subtract = instruction.code() == Code::Fsubrp_sti_st0;
        let pop = matches!(
            instruction.code(),
            Code::Faddp_sti_st0 | Code::Fsubp_sti_st0 | Code::Fsubrp_sti_st0
        );
        let indexed_register = if destination_is_top {
            instruction.op1_register()
        } else {
            instruction.op0_register()
        };
        let index = (indexed_register as usize)
            .checked_sub(Register::ST0 as usize)
            .ok_or(StopReason::UnsupportedInstruction)?;
        if index >= usize::from(self.x87_stack.occupied) || (pop && index == 0) {
            return Err(StopReason::UnsupportedInstruction);
        }
        let top = self.x87_stack.value()?;
        let slot = (usize::from(self.x87_stack.top) + index) & 7;
        let indexed = f64::from_bits(self.x87_stack.values[slot]);
        let (left, right) = if reverse_subtract {
            (top, -indexed)
        } else if subtract {
            (indexed, -top)
        } else {
            (top, indexed)
        };
        let mut result = if reverse_subtract {
            top - indexed
        } else if subtract {
            indexed - top
        } else {
            top + indexed
        };
        if !result.is_finite() || (result != 0.0 && !result.is_normal()) {
            return Err(StopReason::UnsupportedInstruction);
        }
        let truncating = self.x87_control_word & 0x0f3f == 0x0c3f;
        if single_precision {
            result = if truncating {
                rounding::single_sum_toward_zero(result, left, right)
            } else {
                rounding::single_sum(result, left, right)
            }
            .ok_or(StopReason::UnsupportedInstruction)?;
        }
        let rounding = rounding::sum_magnitude_result(result, left, right);
        self.x87_stack.rounded(
            rounding != Ordering::Equal,
            !truncating && rounding == Ordering::Greater,
        );
        let destination = if destination_is_top {
            usize::from(self.x87_stack.top)
        } else {
            slot
        };
        self.x87_stack.values[destination] = result.to_bits();
        if pop {
            self.x87_stack.pop();
        }
        Ok(())
    }

    pub(in super::super) fn x87_register_transfer(
        &mut self,
        instruction: &Instruction,
    ) -> Result<(), StopReason> {
        self.x87_masked()?;
        self.x87_stack.numeric()?;
        let register = if instruction.code() == Code::Fxch_st0_sti {
            instruction.op1_register()
        } else {
            instruction.op0_register()
        };
        let index = (register as usize)
            .checked_sub(Register::ST0 as usize)
            .ok_or(StopReason::UnsupportedInstruction)?;
        if index >= usize::from(self.x87_stack.occupied) {
            return Err(StopReason::UnsupportedInstruction);
        }
        let top = usize::from(self.x87_stack.top);
        let target = (top + index) & 7;
        if instruction.code() == Code::Fld_sti {
            return self
                .x87_stack
                .push(f64::from_bits(self.x87_stack.values[target]));
        }
        if instruction.code() == Code::Fxch_st0_sti {
            self.x87_stack.values.swap(top, target);
        } else {
            self.x87_stack.values[target] = self.x87_stack.values[top];
        }
        self.x87_stack.rounded(false, false);
        if instruction.code() == Code::Fstp_sti {
            self.x87_stack.pop();
        }
        Ok(())
    }

    pub(in super::super) fn x87_compare(
        &mut self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<(), StopReason> {
        self.x87_masked()?;
        let left = self.x87_stack.value()?;
        let right = if matches!(instruction.code(), Code::Fcomp_st0_sti | Code::Fcompp) {
            let source = if instruction.code() == Code::Fcompp {
                Register::ST1
            } else {
                instruction.op1_register()
            };
            let right = self.x87_register_value(source)?;
            if !(left.is_normal() || left == 0.0) || !(right.is_normal() || right == 0.0) {
                return Err(StopReason::UnsupportedInstruction);
            }
            right
        } else {
            self.read_float(instruction, memory)?
        };
        let condition = match left
            .partial_cmp(&right)
            .ok_or(StopReason::UnsupportedInstruction)?
        {
            Ordering::Less => 0x0100,
            Ordering::Equal => 0x4000,
            Ordering::Greater => 0,
        };
        self.x87_stack.status = (self.x87_stack.status & !0x4700) | condition;
        if matches!(
            instruction.code(),
            Code::Fcomp_m32fp | Code::Fcomp_m64fp | Code::Fcomp_st0_sti | Code::Fcompp
        ) {
            self.x87_stack.pop();
        }
        if instruction.code() == Code::Fcompp {
            self.x87_stack.pop();
        }
        Ok(())
    }

    pub(in super::super) fn x87_store_status(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        self.x87_masked()?;
        let destination = self.operand(instruction, 0)?;
        if let Location::Memory(address) = destination.location {
            address
                .checked_add(1)
                .ok_or(StopReason::MemoryFault(MemoryError::AddressOverflow))?;
        }
        let status = self.x87_stack.status | (u16::from(self.x87_stack.top) << 11);
        self.write_operand(destination, u32::from(status), memory)
    }

    pub(crate) fn push_x87_double(&mut self, value: f64) -> Option<()> {
        self.x87_masked().ok()?;
        if value != 0.0 && !value.is_normal() {
            return None;
        }
        self.x87_stack.push(value).ok()
    }

    pub(crate) fn x87_inline_fmod(&mut self) -> Option<()> {
        self.x87_inline_binary(|numerator, divisor| {
            if divisor == 0.0 {
                return None;
            }
            Some(numerator % divisor)
        })
    }

    pub(crate) fn x87_inline_inverse_trig(&mut self, acos: bool) -> Option<()> {
        self.x87_masked().ok()?;
        self.x87_stack.numeric().ok()?;
        let input = self.x87_stack.value().ok()?;
        if !input.is_finite() || !(-1.0..=1.0).contains(&input) {
            return None;
        }
        let result = if acos { input.acos() } else { input.asin() };
        if result != 0.0 && !result.is_normal() {
            return None;
        }
        self.x87_stack.values[usize::from(self.x87_stack.top)] = result.to_bits();
        self.x87_stack.status &= !0x0200;
        Some(())
    }

    pub(crate) fn x87_inline_pow(&mut self) -> Option<()> {
        self.x87_inline_binary(|base, exponent| {
            if (base < 0.0 && exponent.fract() != 0.0) || (base == 0.0 && exponent < 0.0) {
                return None;
            }
            let result = base.powf(exponent);
            if base != 0.0 && result == 0.0 {
                return None;
            }
            Some(result)
        })
    }

    fn x87_inline_binary(&mut self, evaluate: impl FnOnce(f64, f64) -> Option<f64>) -> Option<()> {
        self.x87_masked().ok()?;
        self.x87_stack.numeric().ok()?;
        if self.x87_stack.occupied < 2 {
            return None;
        }
        let right = self.x87_stack.value().ok()?;
        let left_slot = (usize::from(self.x87_stack.top) + 1) & 7;
        let left = f64::from_bits(self.x87_stack.values[left_slot]);
        if !left.is_finite() || !right.is_finite() {
            return None;
        }
        let result = evaluate(left, right)?;
        if !result.is_finite() || (result != 0.0 && !result.is_normal()) {
            return None;
        }
        self.x87_stack.values[left_slot] = result.to_bits();
        self.x87_stack.pop();
        self.x87_stack.status &= !0x0200;
        Some(())
    }

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
        self.x87_stack
            .rounded(value.to_bits() != value.trunc().to_bits(), false);
        self.x87_stack.pop();
        Some(integer)
    }

    pub(in super::super) fn x87_arithmetic(
        &mut self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<(), StopReason> {
        let single_precision = self.x87_arithmetic_precision(instruction.code())?;
        let top = self.x87_stack.value()?;
        let (result, rounding) = if instruction.code() == Code::Fsqrt {
            if top < 0.0 {
                return Err(StopReason::UnsupportedInstruction);
            }
            let mut result = top.sqrt();
            if single_precision {
                result = rounding::single_square_root(result, top)
                    .ok_or(StopReason::UnsupportedInstruction)?;
            }
            (result, rounding::square_root_result(result, top))
        } else if matches!(
            instruction.code(),
            Code::Fadd_m32fp
                | Code::Fadd_m64fp
                | Code::Fiadd_m32int
                | Code::Fsub_m32fp
                | Code::Fsub_m64fp
                | Code::Fsub_st0_sti
                | Code::Fsubr_st0_sti
                | Code::Fsubr_m32fp
                | Code::Fsubr_m64fp
        ) {
            let (left, right, add) = self.x87_add_sub_operands(instruction, memory, top)?;
            let mut result = if add { left + right } else { left - right };
            if !result.is_finite() || (result != 0.0 && !result.is_normal()) {
                return Err(StopReason::UnsupportedInstruction);
            }
            let signed_right = if add { right } else { -right };
            if single_precision {
                result = if self.x87_control_word & 0x0f3f == 0x0c3f {
                    rounding::single_sum_toward_zero(result, left, signed_right)
                } else {
                    rounding::single_sum(result, left, signed_right)
                }
                .ok_or(StopReason::UnsupportedInstruction)?;
            }
            (
                result,
                rounding::sum_magnitude_result(result, left, signed_right),
            )
        } else {
            let source = if matches!(instruction.code(), Code::Fmul_st0_sti | Code::Fdiv_st0_sti) {
                self.x87_register_value(instruction.op1_register())?
            } else if matches!(instruction.code(), Code::Fimul_m32int | Code::Fidiv_m32int) {
                self.read_integer_m32(instruction, memory)?
            } else {
                self.read_float(instruction, memory)?
            };
            let multiply = matches!(
                instruction.code(),
                Code::Fmul_st0_sti | Code::Fmul_m32fp | Code::Fmul_m64fp | Code::Fimul_m32int
            );
            let (left, right) =
                if matches!(instruction.code(), Code::Fdivr_m32fp | Code::Fdivr_m64fp) {
                    (source, top)
                } else {
                    (top, source)
                };
            let (mut result, exact_zero) = if multiply {
                (left * right, left == 0.0 || right == 0.0)
            } else {
                if right == 0.0 {
                    return Err(StopReason::UnsupportedInstruction);
                }
                (left / right, left == 0.0)
            };
            // binary64's bottom binade cannot stand in for x87's extended exponent range.
            if !result.is_finite() || (!exact_zero && result.abs() < 2.0 * f64::MIN_POSITIVE) {
                return Err(StopReason::UnsupportedInstruction);
            }
            if single_precision {
                result = if !multiply {
                    rounding::single_quotient(result, left, right)
                } else if self.x87_control_word & 0x0f3f == 0x0c3f {
                    rounding::single_product_toward_zero(result, left, right)
                } else {
                    rounding::single_product(result, left, right)
                }
                .ok_or(StopReason::UnsupportedInstruction)?;
            }
            let rounding = if multiply {
                rounding::product_result(result, top, source)
            } else {
                rounding::quotient_result(result, left, right)
            };
            (result, rounding)
        };
        self.x87_stack.rounded(
            rounding != Ordering::Equal,
            self.x87_control_word & 0x0f3f != 0x0c3f && rounding == Ordering::Greater,
        );
        self.x87_stack.values[usize::from(self.x87_stack.top)] = result.to_bits();
        Ok(())
    }

    fn x87_add_sub_operands(
        &mut self,
        instruction: &Instruction,
        memory: &GuestMemory,
        top: f64,
    ) -> Result<(f64, f64, bool), StopReason> {
        let code = instruction.code();
        let source = if matches!(code, Code::Fsub_st0_sti | Code::Fsubr_st0_sti) {
            self.x87_register_value(instruction.op1_register())?
        } else if code == Code::Fiadd_m32int {
            self.read_integer_m32(instruction, memory)?
        } else {
            self.read_float(instruction, memory)?
        };
        let reverse = matches!(
            code,
            Code::Fsubr_st0_sti | Code::Fsubr_m32fp | Code::Fsubr_m64fp
        );
        let (left, right) = if reverse {
            (source, top)
        } else {
            (top, source)
        };
        let add = matches!(
            code,
            Code::Fadd_m32fp | Code::Fadd_m64fp | Code::Fiadd_m32int
        );
        Ok((left, right, add))
    }

    pub(in super::super) fn x87_integer_load(
        &mut self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<(), StopReason> {
        self.x87_masked()?;
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

    pub(in super::super) fn x87_integer_store(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        self.x87_masked()?;
        let value = self.x87_stack.value()?;
        let rounded = match (self.x87_control_word >> 10) & 3 {
            0 => value.round_ties_even(),
            1 => value.floor(),
            2 => value.ceil(),
            _ => value.trunc(),
        };
        let (address, size) = self.data_address(instruction)?;
        let limit = match size {
            2 => 32_768.0,
            4 => 2_147_483_648.0,
            _ => 9_223_372_036_854_775_808.0,
        };
        if !(-limit..limit).contains(&rounded) {
            return Err(StopReason::UnsupportedInstruction);
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "rounded and range-checked integer"
        )]
        let integer = rounded as i64;
        memory
            .write(u64::from(address), &integer.to_le_bytes()[..size])
            .map_err(StopReason::MemoryFault)?;
        self.x87_stack.rounded(
            rounded.to_bits() != value.to_bits(),
            rounded.abs() > value.abs(),
        );
        if matches!(
            instruction.code(),
            Code::Fistp_m16int | Code::Fistp_m32int | Code::Fistp_m64int
        ) {
            self.x87_stack.pop();
        }
        Ok(())
    }

    pub(in super::super) fn x87_transfer(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        if matches!(instruction.code(), Code::Fld_m32fp | Code::Fld_m64fp) {
            self.x87_masked()?;
            if instruction.code() == Code::Fld_m64fp {
                let (address, _) = self.data_address(instruction)?;
                let mut bytes = [0; 8];
                memory
                    .read(u64::from(address), &mut bytes)
                    .map_err(StopReason::MemoryFault)?;
                let bits = u64::from_le_bytes(bytes);
                if bits & 0x7ff0_0000_0000_0000 == 0x7ff0_0000_0000_0000
                    && bits & 0x000f_ffff_ffff_ffff != 0
                {
                    let signaling = bits & (1 << 51) == 0;
                    self.x87_stack.push(f64::from_bits(bits | (1 << 51)))?;
                    if signaling {
                        self.x87_stack.status |= 1;
                    }
                    return Ok(());
                }
                let value = f64::from_bits(bits);
                if value.is_subnormal() {
                    self.x87_stack.push(value)?;
                    self.x87_stack.status |= 2;
                    return Ok(());
                }
                if !value.is_normal() && value != 0.0 {
                    return Err(StopReason::UnsupportedInstruction);
                }
                return self.x87_stack.push(value);
            }
            let value = self.read_float(instruction, memory)?;
            return self.x87_stack.push(value);
        }
        let profile = self.x87_control_word & 0x0f3f;
        if !matches!(
            instruction.code(),
            Code::Fst_m32fp | Code::Fstp_m32fp | Code::Fst_m64fp | Code::Fstp_m64fp
        ) || !matches!(profile, 0x003f | 0x0c3f)
        {
            self.x87_profile()?;
        }
        let value = self.x87_stack.value()?;
        let (address, size) = self.data_address(instruction)?;
        let mut bytes = value.to_le_bytes();
        let mut rounded = value;
        let mut tiny = false;
        if size == 4 {
            if !value.is_finite() || value.abs() > f64::from(f32::MAX) {
                return Err(StopReason::UnsupportedInstruction);
            }
            // tininess uses p24 rounding with an unbounded exponent, before denormalization.
            let normal = f64::from(f32::MIN_POSITIVE);
            let threshold = if profile == 0x0c3f {
                normal
            } else {
                normal - f64::from(f32::from_bits(1)) * 0.25
            };
            tiny = value.abs() < threshold;
            #[expect(clippy::cast_possible_truncation, reason = "checked f32 range")]
            let mut narrowed = value as f32;
            if profile == 0x0c3f && f64::from(narrowed).abs() > value.abs() {
                narrowed = if value.is_sign_negative() {
                    narrowed.next_up()
                } else {
                    narrowed.next_down()
                };
            }
            rounded = f64::from(narrowed);
            bytes[..4].copy_from_slice(&narrowed.to_le_bytes());
        }
        memory
            .write(u64::from(address), &bytes[..size])
            .map_err(StopReason::MemoryFault)?;
        if tiny && rounded.to_bits() != value.to_bits() {
            self.x87_stack.status |= 0x10;
        }
        self.x87_stack.rounded(
            rounded.to_bits() != value.to_bits(),
            rounded.abs() > value.abs(),
        );
        if matches!(instruction.code(), Code::Fstp_m32fp | Code::Fstp_m64fp) {
            self.x87_stack.pop();
        }
        Ok(())
    }

    fn x87_profile(&self) -> Result<(), StopReason> {
        if self.x87_control_word & 0x0f3f != 0x023f {
            return Err(StopReason::UnsupportedInstruction);
        }
        Ok(())
    }

    fn x87_arithmetic_precision(&self, code: Code) -> Result<bool, StopReason> {
        match self.x87_control_word & 0x0f3f {
            0x023f => Ok(false),
            0x003f
                if matches!(
                    code,
                    Code::Fsqrt
                        | Code::Fdivp_sti_st0
                        | Code::Fdiv_st0_sti
                        | Code::Fmulp_sti_st0
                        | Code::Fsubp_sti_st0
                        | Code::Fsubrp_sti_st0
                        | Code::Fdiv_m32fp
                        | Code::Fidiv_m32int
                        | Code::Fdivr_m32fp
                        | Code::Fdivr_m64fp
                        | Code::Fsubr_st0_sti
                        | Code::Fsub_st0_sti
                        | Code::Fadd_st0_sti
                        | Code::Faddp_sti_st0
                        | Code::Fmul_st0_sti
                        | Code::Fmul_m32fp
                        | Code::Fmul_m64fp
                        | Code::Fimul_m32int
                        | Code::Fiadd_m32int
                        | Code::Fadd_m32fp
                        | Code::Fsub_m32fp
                        | Code::Fsubr_m32fp
                ) =>
            {
                Ok(true)
            }
            0x0c3f
                if matches!(
                    code,
                    Code::Fmul_m32fp | Code::Fadd_st0_sti | Code::Faddp_sti_st0 | Code::Fadd_m32fp
                ) =>
            {
                Ok(true)
            }
            _ => Err(StopReason::UnsupportedInstruction),
        }
    }

    fn x87_masked(&self) -> Result<(), StopReason> {
        if self.x87_control_word & 0x3f != 0x3f {
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
        &mut self,
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
            if !value.is_finite() {
                return Err(StopReason::UnsupportedInstruction);
            }
            if value.is_subnormal() {
                self.x87_stack.status |= 2;
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

    fn x87_register_value(&self, register: Register) -> Result<f64, StopReason> {
        let index = (register as usize)
            .checked_sub(Register::ST0 as usize)
            .ok_or(StopReason::UnsupportedInstruction)?;
        if index >= usize::from(self.x87_stack.occupied) {
            return Err(StopReason::UnsupportedInstruction);
        }
        let slot = (usize::from(self.x87_stack.top) + index) & 7;
        Ok(f64::from_bits(self.x87_stack.values[slot]))
    }

    fn read_integer_m32(
        &self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<f64, StopReason> {
        let (address, size) = self.data_address(instruction)?;
        if size != 4 {
            return Err(StopReason::UnsupportedInstruction);
        }
        let mut bytes = [0; 4];
        memory
            .read(u64::from(address), &mut bytes)
            .map_err(StopReason::MemoryFault)?;
        Ok(f64::from(i32::from_le_bytes(bytes)))
    }
}
