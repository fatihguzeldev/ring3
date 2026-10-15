use wasm_encoder::{BlockType, InstructionSink, ValType};

use super::{control, locals::*, memory, x87};
use crate::cpu::x86::{
    Register32,
    decode::DecodedOperation,
    ir::{
        AsciiAdjustKind, BinaryKind, BitIndex, BitScanKind, BitTestKind, BranchTarget,
        ByteArithmeticKind, ByteLogicalKind, BytePredicateKind, ByteReadArithmeticKind,
        ByteRegister, ByteValue, CarryKind, Condition, CountBranchKind, DecimalAdjustKind,
        DivideKind, DoubleShiftKind, EffectiveAddress, ExtensionKind, Location32,
        MemoryByteArithmeticKind, MultiplyKind, Operation, RotateKind, ShiftCount, ShiftKind,
        SmallSource, SmallWidth, UnaryKind, Value32, WordArithmeticKind, WordLogicalKind,
        WordMemoryArithmeticKind, WordReadArithmeticKind, WordValue,
    },
};

#[derive(Clone, Copy)]
enum CarryFlag {
    Calculate,
    Preserve,
}

mod arithmetic;
mod bit_manipulation;
mod bits;
mod carry_rotates;
mod decimal;
mod dispatch;
mod flags;
mod multiply_divide;
mod operands;
mod rotates;
mod scalar_bits;
mod shifts;

use arithmetic::{
    arithmetic_memory_byte, binary_memory_store, logical_memory_byte, unary, unary_byte,
    unary_memory, unary_memory_byte, unary_memory_word, unary_word,
};
use bits::{
    bit_scan, bit_scan_word, bit_test, bit_test_word, memory_bit_mutation, memory_bit_test_word,
    trailing_zero_count32,
};
use carry_rotates::{
    byte_rotate_through_carry, byte_rotate_through_carry_cl, byte_rotate_through_carry_memory_cl,
    byte_rotate_through_carry_memory_immediate, byte_rotate_through_carry_one,
    byte_rotate_through_carry_one_flags, byte_rotate_through_carry_one_value,
    memory_rotate_through_carry_cl, memory_rotate_through_carry_immediate, rotate_through_carry_cl,
    rotate_through_carry_immediate, rotate_through_carry_memory_word, rotate_through_carry_one,
    rotate_through_carry_one_flags, rotate_through_carry_one_value, rotate_through_carry_word,
};
use decimal::{
    ascii_adjust, ascii_adjust_after_multiplication, ascii_adjust_before_division, decimal_adjust,
};
pub(super) use dispatch::instruction;
use flags::{arithmetic_flags, logical_flags, parity_flag, zero_sign_flags};
use multiply_divide::{
    accumulator_divide, accumulator_multiply, byte_accumulator_divide,
    byte_accumulator_divide_value, byte_accumulator_multiply, byte_accumulator_multiply_value,
    divide_error_if, signed_multiply, signed_multiply_word, word_accumulator_divide,
    word_accumulator_multiply,
};
use operands::{
    byte_value, extend_byte_to_word, extend_value, indirect_target, insert_byte, value, word_value,
};
use rotates::{
    byte_rotate_cl, byte_rotate_immediate, byte_rotate_memory, byte_rotate_one,
    byte_rotate_one_flags, byte_rotate_one_value, rotate, rotate_memory, rotate_memory_word,
    rotate_one, rotate_one_flags, rotate_one_value, rotate_word,
};
use shifts::{
    double_shift, double_shift_memory, double_shift_memory_word, double_shift_word, shift,
    shift_byte, shift_count, shift_memory, shift_memory_byte, shift_memory_byte_cl,
    shift_memory_byte_immediate, shift_memory_word, shift_word,
};
