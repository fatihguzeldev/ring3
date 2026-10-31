use super::*;
use crate::cpu::x86::{X87Format, X87Operation, X87State};
use std::mem::{offset_of, size_of};

pub(super) const STATE: u32 = 34;
pub(super) const TOP: u32 = 35;
pub(super) const PHYS: u32 = 36;
pub(super) const TAG: u32 = 37;
pub(super) const TMP: u32 = 38;
pub(super) const RAW: u32 = 44;
pub(super) const F32_VALUE: u32 = 46;
pub(super) const VALUE: u32 = 49;
const CHECKED_STATE: u32 = 69;
pub(super) const CHECKED_VIEW: u32 = 71;

const CONTROL: u64 = offset_of!(X87State, control) as u64;
const STATUS: u64 = offset_of!(X87State, status) as u64;
const TAG_OFFSET: u64 = offset_of!(X87State, tag) as u64;
const OPCODE: u64 = offset_of!(X87State, opcode) as u64;
const FIP: u64 = offset_of!(X87State, instruction_pointer) as u64;
const FDP: u64 = offset_of!(X87State, data_pointer) as u64;
const CS: u64 = offset_of!(X87State, code_selector) as u64;
const DS: u64 = offset_of!(X87State, data_selector) as u64;
const REGISTERS: i32 = offset_of!(X87State, registers) as i32;

const _: () = {
    assert!(size_of::<X87State>() == 100);
    assert!(CONTROL == 0 && STATUS == 2 && TAG_OFFSET == 4 && OPCODE == 6);
    assert!(FIP == 8 && FDP == 12 && CS == 16 && DS == 18 && REGISTERS == 20);
};

// raw80 cells have a ten-byte stride; two-byte alignment is the common bound
// for their payloads and every halfword field used by this module.
fn mem(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 1,
        memory_index: 0,
    }
}

pub(super) fn supports(operation: &Operation) -> bool {
    match *operation {
        Operation::X87 { operation, .. } => supported(operation),
        Operation::X87StatusToAx
        | Operation::X87StatusToMemory { .. }
        | Operation::X87ControlToMemory { .. } => true,
        _ => false,
    }
}

fn supported(operation: X87Operation) -> bool {
    matches!(
        operation,
        X87Operation::Wait
            | X87Operation::Load(X87Format::Float32 | X87Format::Float64)
            | X87Operation::Store {
                format: X87Format::Float32 | X87Format::Float64,
                truncate: false,
                ..
            }
            | X87Operation::LoadConstant { index: 0 | 6 }
            | X87Operation::LoadRegister { .. }
            | X87Operation::StoreRegister { .. }
            | X87Operation::Exchange { .. }
            | X87Operation::LoadControl
            | X87Operation::StoreControl
            | X87Operation::StoreStatus
            | X87Operation::StatusToAx
    )
}

/// every speculative guard returns with the current instruction unexecuted.
pub(super) fn miss_if(out: &mut Output) {
    out.c().if_(BlockType::Empty);
    out.exit(7);
    out.c().end();
}

fn pointer(out: &mut Output, local: u32, length: i64) {
    out.c()
        .local_get(local)
        .i32_eqz()
        .local_get(local)
        .i32_const(3)
        .i32_and()
        .i32_or()
        .local_get(local)
        .i64_extend_i32_u()
        .i64_const(length)
        .i64_add()
        .memory_size(0)
        .i64_extend_i32_u()
        .i64_const(16)
        .i64_shl()
        .i64_gt_u()
        .i32_or();
    miss_if(out);
}

// numeric view v1 is independent of NumericState's layout.
fn bank(out: &mut Output) {
    // these proofs live only in this function invocation. the owned boxes do
    // not move across trusted memory helpers; mutable x87 state is read below.
    out.c()
        .local_get(CHECKED_STATE)
        .i32_eqz()
        .if_(BlockType::Empty);
    out.c().local_get(0).i32_load(mem(124)).local_set(TMP);
    pointer(out, TMP, 36);
    out.c()
        .local_get(TMP)
        .i32_load(mem(0))
        .i32_const(1)
        .i32_ne()
        .local_get(TMP)
        .i32_load(mem(4))
        .i32_const(36)
        .i32_lt_u()
        .i32_or();
    miss_if(out);
    out.c().local_get(TMP).i32_load(mem(8)).local_set(STATE);
    pointer(out, STATE, size_of::<X87State>() as i64);
    out.c()
        .local_get(STATE)
        .local_set(CHECKED_STATE)
        .local_get(TMP)
        .local_set(CHECKED_VIEW)
        .end()
        .local_get(CHECKED_STATE)
        .local_set(STATE);
}

fn pending(out: &mut Output) {
    out.c()
        .local_get(STATE)
        .i32_load16_u(mem(STATUS))
        .local_get(STATE)
        .i32_load16_u(mem(CONTROL))
        .i32_const(-1)
        .i32_xor()
        .i32_and()
        .i32_const(63)
        .i32_and();
    miss_if(out);
}

/// mutable stack state belongs to each reached x87 computation.
pub(super) fn enter(out: &mut Output, waiting: bool) {
    bank(out);
    if waiting {
        pending(out);
    }
    out.c()
        .local_get(STATE)
        .i32_load16_u(mem(STATUS))
        .i32_const(11)
        .i32_shr_u()
        .i32_const(7)
        .i32_and()
        .local_set(TOP)
        .local_get(STATE)
        .i32_load16_u(mem(TAG_OFFSET))
        .local_set(TAG);
}

fn physical(out: &mut Output, index: u8) {
    out.c()
        .local_get(TOP)
        .i32_const(i32::from(index))
        .i32_add()
        .i32_const(7)
        .i32_and()
        .local_set(PHYS);
}

fn raw_address(out: &mut Output) {
    out.c()
        .local_get(STATE)
        .i32_const(REGISTERS)
        .i32_add()
        .local_get(PHYS)
        .i32_const(10)
        .i32_mul()
        .i32_add();
}

/// load an occupied normal/zero cell, preserving its raw80 bits in RAW/TMP.
pub(super) fn read_raw(out: &mut Output, index: u8) {
    physical(out, index);
    out.c()
        .local_get(TAG)
        .local_get(PHYS)
        .i32_const(1)
        .i32_shl()
        .i32_shr_u()
        .i32_const(3)
        .i32_and()
        .i32_const(3)
        .i32_eq();
    miss_if(out);
    raw_address(out);
    out.c().i64_load(mem(0)).local_set(RAW);
    raw_address(out);
    out.c().i32_load16_u(mem(8)).local_set(TMP);
    out.c()
        .local_get(RAW)
        .i64_eqz()
        .local_get(TMP)
        .i32_const(0x7fff)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty);
    out.c()
        .local_get(TMP)
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffd)
        .i32_gt_u()
        .local_get(RAW)
        .i64_const(i64::MIN)
        .i64_and()
        .i64_eqz()
        .i32_or();
    miss_if(out);
    out.c().end();
}

/// prove an exact normal binary32/zero representation before using Wasm FP.
pub(super) fn read_raw_f32(out: &mut Output, index: u8, destination: u32) {
    read_raw(out, index);
    out.c()
        .local_get(TMP)
        .i32_const(0x7fff)
        .i32_and()
        .if_(BlockType::Result(ValType::I32));
    out.c()
        .local_get(TMP)
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(16257)
        .i32_sub()
        .i32_const(253)
        .i32_gt_u()
        .local_get(RAW)
        .i64_const((1_i64 << 40) - 1)
        .i64_and()
        .i64_eqz()
        .i32_eqz()
        .i32_or();
    miss_if(out);
    out.c()
        .local_get(TMP)
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(16256)
        .i32_sub()
        .i32_const(23)
        .i32_shl()
        .local_get(RAW)
        .i64_const(40)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(0x7fffff)
        .i32_and()
        .i32_or()
        .else_()
        .i32_const(0)
        .end()
        .local_get(TMP)
        .i32_const(0x8000)
        .i32_and()
        .i32_const(16)
        .i32_shl()
        .i32_or()
        .f32_reinterpret_i32()
        .f64_promote_f32()
        .local_set(destination);
}

/// exact binary64 values occupy 53 significand bits and a bounded exponent range.
pub(super) fn read_raw_f64(out: &mut Output, index: u8, destination: u32) {
    read_raw(out, index);
    out.c()
        .local_get(TMP)
        .i32_const(0x7fff)
        .i32_and()
        .if_(BlockType::Result(ValType::I64));
    out.c()
        .local_get(TMP)
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(15361)
        .i32_sub()
        .i32_const(2045)
        .i32_gt_u()
        .local_get(RAW)
        .i64_const(0x7ff)
        .i64_and()
        .i64_eqz()
        .i32_eqz()
        .i32_or();
    miss_if(out);
    out.c()
        .local_get(TMP)
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(15360)
        .i32_sub()
        .i64_extend_i32_u()
        .i64_const(52)
        .i64_shl()
        .local_get(RAW)
        .i64_const(11)
        .i64_shr_u()
        .i64_const(0x000f_ffff_ffff_ffff)
        .i64_and()
        .i64_or()
        .else_()
        .i64_const(0)
        .end()
        .local_get(TMP)
        .i32_const(0x8000)
        .i32_and()
        .i64_extend_i32_u()
        .i64_const(48)
        .i64_shl()
        .i64_or()
        .f64_reinterpret_i64()
        .local_set(destination);
}

pub(super) fn raw_from_f64_bits(out: &mut Output) {
    out.c()
        .local_get(RAW)
        .i64_const(52)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(0x7ff)
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .local_get(RAW)
        .i64_const(52)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(0x7ff)
        .i32_and()
        .i32_const(15360)
        .i32_add()
        .else_()
        .i32_const(0)
        .end()
        .local_get(RAW)
        .i64_const(48)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(0x8000)
        .i32_and()
        .i32_or()
        .local_set(TMP)
        .local_get(TMP)
        .i32_const(0x7fff)
        .i32_and()
        .if_(BlockType::Result(ValType::I64))
        .local_get(RAW)
        .i64_const(0x000f_ffff_ffff_ffff)
        .i64_and()
        .i64_const(0x0010_0000_0000_0000)
        .i64_or()
        .i64_const(11)
        .i64_shl()
        .else_()
        .i64_const(0)
        .end()
        .local_set(RAW);
}

pub(super) fn raw_from_f32_bits(out: &mut Output, bits: u32) {
    out.c()
        .local_get(bits)
        .i32_const(23)
        .i32_shr_u()
        .i32_const(255)
        .i32_and()
        .if_(BlockType::Result(ValType::I64))
        .local_get(bits)
        .i32_const(0x7fffff)
        .i32_and()
        .i32_const(0x800000)
        .i32_or()
        .i64_extend_i32_u()
        .i64_const(40)
        .i64_shl()
        .else_()
        .i64_const(0)
        .end()
        .local_set(RAW)
        .local_get(bits)
        .i32_const(23)
        .i32_shr_u()
        .i32_const(255)
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .local_get(bits)
        .i32_const(23)
        .i32_shr_u()
        .i32_const(255)
        .i32_and()
        .i32_const(16256)
        .i32_add()
        .else_()
        .i32_const(0)
        .end()
        .local_get(bits)
        .i32_const(16)
        .i32_shr_u()
        .i32_const(0x8000)
        .i32_and()
        .i32_or()
        .local_set(TMP);
}

pub(super) fn write_raw(out: &mut Output, index: u8) {
    physical(out, index);
    raw_address(out);
    out.c().local_get(RAW).i64_store(mem(0));
    raw_address(out);
    out.c().local_get(TMP).i32_store16(mem(8));
    out.c()
        .local_get(TAG)
        .i32_const(3)
        .local_get(PHYS)
        .i32_const(1)
        .i32_shl()
        .i32_shl()
        .i32_const(-1)
        .i32_xor()
        .i32_and()
        .local_get(RAW)
        .i64_eqz()
        .local_get(PHYS)
        .i32_const(1)
        .i32_shl()
        .i32_shl()
        .i32_or()
        .local_set(TAG)
        .local_get(STATE)
        .local_get(TAG)
        .i32_store16(mem(TAG_OFFSET));
}

/// the caller proves finite normal/zero binary32 exactness before this commit.
pub(super) fn commit_raw_f32(out: &mut Output, index: u8, value: u32) {
    out.c()
        .local_get(value)
        .f32_demote_f64()
        .local_tee(F32_VALUE)
        .i32_reinterpret_f32()
        .local_set(R);
    raw_from_f32_bits(out, R);
    write_raw(out, index);
}

/// the caller proves a finite normal/zero result before committing binary64.
pub(super) fn commit_raw_f64(out: &mut Output, index: u8, value: u32) {
    out.c()
        .local_get(value)
        .i64_reinterpret_f64()
        .local_set(RAW);
    raw_from_f64_bits(out);
    write_raw(out, index);
}

pub(super) fn clear_c1(out: &mut Output) {
    out.c()
        .local_get(STATE)
        .local_get(STATE)
        .i32_load16_u(mem(STATUS))
        .i32_const(!0x200)
        .i32_and()
        .i32_store16(mem(STATUS));
}

fn top(out: &mut Output, displacement: i32) {
    out.c()
        .local_get(TOP)
        .i32_const(displacement)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .local_set(TOP)
        .local_get(STATE)
        .local_get(STATE)
        .i32_load16_u(mem(STATUS))
        .i32_const(!0x3800)
        .i32_and()
        .local_get(TOP)
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .i32_store16(mem(STATUS));
}

pub(super) fn pop(out: &mut Output) {
    out.c()
        .local_get(TAG)
        .i32_const(3)
        .local_get(TOP)
        .i32_const(1)
        .i32_shl()
        .i32_shl()
        .i32_or()
        .local_set(TAG)
        .local_get(STATE)
        .local_get(TAG)
        .i32_store16(mem(TAG_OFFSET));
    top(out, 1);
}

fn push_space(out: &mut Output) {
    physical(out, 7);
    out.c()
        .local_get(TAG)
        .local_get(PHYS)
        .i32_const(1)
        .i32_shl()
        .i32_shr_u()
        .i32_const(3)
        .i32_and()
        .i32_const(3)
        .i32_ne();
    miss_if(out);
}

pub(super) fn metadata(out: &mut Output, opcode: u16, memory: bool) {
    out.c()
        .local_get(STATE)
        .local_get(PC)
        .i32_store(mem(FIP))
        .local_get(STATE)
        .i32_const(i32::from(opcode))
        .i32_store16(mem(OPCODE))
        .local_get(STATE)
        .local_get(CHECKED_VIEW)
        .i32_load(mem(12 + SegmentRegister::Cs.index() as u64 * 4))
        .i32_store16(mem(CS));
    if memory {
        out.c()
            .local_get(STATE)
            .local_get(OFFSET)
            .i32_store(mem(FDP))
            .local_get(STATE)
            .local_get(CHECKED_VIEW)
            .local_get(SEGMENT)
            .i32_const(2)
            .i32_shl()
            .i32_add()
            .i32_load(mem(12))
            .i32_store16(mem(DS));
    }
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    let (operation, address, waiting, opcode) = match operation {
        Operation::X87 {
            operation,
            address,
            waiting,
            opcode,
        } if supported(operation) => (operation, address, waiting, opcode),
        Operation::X87StatusToAx => (X87Operation::StatusToAx, None, false, 0),
        Operation::X87StatusToMemory { address } => {
            (X87Operation::StoreStatus, Some(address), false, 0)
        }
        Operation::X87ControlToMemory { address } => {
            (X87Operation::StoreControl, Some(address), false, 0)
        }
        _ => return false,
    };
    if operation == X87Operation::Wait {
        bank(out);
        pending(out);
        return true;
    }
    enter(out, waiting || !operation.ignores_pending_exception());
    match operation {
        X87Operation::LoadConstant { index } => {
            push_space(out);
            out.c()
                .i64_const(if index == 0 { i64::MIN } else { 0 })
                .local_set(RAW)
                .i32_const(if index == 0 { 0x3fff } else { 0 })
                .local_set(TMP);
            write_raw(out, 7);
            clear_c1(out);
            top(out, -1);
        }
        X87Operation::Load(X87Format::Float32) => {
            out.address(address.expect("decoded memory load"));
            out.read(4, R);
            out.c()
                .local_get(R)
                .i32_const(0x7f800000)
                .i32_and()
                .i32_const(0x7f800000)
                .i32_eq()
                .local_get(R)
                .i32_const(0x7fffffff)
                .i32_and()
                .i32_const(1)
                .i32_sub()
                .i32_const(0x7ffffe)
                .i32_le_u()
                .i32_or();
            miss_if(out);
            push_space(out);
            raw_from_f32_bits(out, R);
            write_raw(out, 7);
            clear_c1(out);
            top(out, -1);
        }
        X87Operation::Load(X87Format::Float64) => {
            out.address(address.expect("decoded binary64 load"));
            memory::read_words(out, &[A, B], 1);
            out.c()
                .local_get(A)
                .i64_extend_i32_u()
                .local_get(B)
                .i64_extend_i32_u()
                .i64_const(32)
                .i64_shl()
                .i64_or()
                .local_set(RAW)
                .local_get(RAW)
                .i64_const(0x7ff0_0000_0000_0000)
                .i64_and()
                .i64_const(0x7ff0_0000_0000_0000)
                .i64_eq()
                .local_get(RAW)
                .i64_const(0x7fff_ffff_ffff_ffff)
                .i64_and()
                .i64_const(1)
                .i64_sub()
                .i64_const(0x000f_ffff_ffff_fffe)
                .i64_le_u()
                .i32_or();
            miss_if(out);
            out.c()
                .local_get(RAW)
                .f64_reinterpret_i64()
                .local_set(VALUE);
            push_space(out);
            commit_raw_f64(out, 7, VALUE);
            clear_c1(out);
            top(out, -1);
        }
        X87Operation::Store {
            format: X87Format::Float32,
            pop: popping,
            ..
        } => {
            let inexact = 41;
            let incremented = 42;
            let result = 53;
            read_raw_f64(out, 0, VALUE);
            // this ordinary range excludes ieee underflow and overflow, even
            // when a value just outside it would round to a normal endpoint.
            out.c()
                .local_get(VALUE)
                .f64_abs()
                .f64_const(f64::from(f32::MIN_POSITIVE).into())
                .f64_lt()
                .local_get(VALUE)
                .f64_const(0.0.into())
                .f64_ne()
                .i32_and()
                .local_get(VALUE)
                .f64_abs()
                .f64_const(f64::from(f32::MAX).into())
                .f64_gt()
                .i32_or();
            miss_if(out);
            out.c()
                .local_get(VALUE)
                .f32_demote_f64()
                .local_tee(F32_VALUE)
                .f64_promote_f32()
                .local_tee(result)
                .local_get(VALUE)
                .f64_ne()
                .local_set(inexact)
                .local_get(inexact)
                .if_(BlockType::Empty)
                .local_get(STATE)
                .i32_load16_u(mem(CONTROL))
                .i32_const(0x0c3f)
                .i32_and()
                .i32_const(0x003f)
                .i32_ne();
            // exact transport keeps its old control word independence. only the new
            // inexact arm requires nearest rounding and masked exceptions.
            miss_if(out);
            out.c()
                .end()
                .local_get(result)
                .f64_abs()
                .local_get(VALUE)
                .f64_abs()
                .f64_gt()
                .local_set(incremented)
                .local_get(F32_VALUE)
                .i32_reinterpret_f32()
                .local_set(R);
            out.address(address.expect("decoded memory store"));
            out.write(4, R);
            // the checked store precedes every fp effect. preserve the source
            // payload and sticky status; c1 records a magnitude increment.
            out.c()
                .local_get(STATE)
                .local_get(STATE)
                .i32_load16_u(mem(STATUS))
                .i32_const(!0x200)
                .i32_and()
                .local_get(inexact)
                .i32_const(5)
                .i32_shl()
                .i32_or()
                .local_get(incremented)
                .i32_const(9)
                .i32_shl()
                .i32_or()
                .i32_store16(mem(STATUS));
            if popping {
                pop(out);
            }
        }
        X87Operation::Store {
            format: X87Format::Float64,
            pop: popping,
            ..
        } => {
            read_raw_f64(out, 0, VALUE);
            out.c()
                .local_get(VALUE)
                .i64_reinterpret_f64()
                .local_tee(RAW)
                .i32_wrap_i64()
                .local_set(A)
                .local_get(RAW)
                .i64_const(32)
                .i64_shr_u()
                .i32_wrap_i64()
                .local_set(B);
            out.address(address.expect("decoded binary64 store"));
            memory::write_words(out, &[A, B], 1);
            clear_c1(out);
            if popping {
                pop(out);
            }
        }
        X87Operation::LoadRegister { index } => {
            read_raw(out, index);
            push_space(out);
            write_raw(out, 7);
            top(out, -1);
        }
        X87Operation::StoreRegister {
            index,
            pop: popping,
        } => {
            read_raw(out, 0);
            write_raw(out, index);
            if popping {
                pop(out);
            }
        }
        X87Operation::Exchange { index } => {
            read_raw(out, index);
            out.c()
                .local_get(RAW)
                .i32_wrap_i64()
                .local_set(A)
                .local_get(RAW)
                .i64_const(32)
                .i64_shr_u()
                .i32_wrap_i64()
                .local_set(B)
                .local_get(TMP)
                .local_set(R);
            read_raw(out, 0);
            write_raw(out, index);
            out.c()
                .local_get(A)
                .i64_extend_i32_u()
                .local_get(B)
                .i64_extend_i32_u()
                .i64_const(32)
                .i64_shl()
                .i64_or()
                .local_set(RAW)
                .local_get(R)
                .local_set(TMP);
            write_raw(out, 0);
            clear_c1(out);
        }
        X87Operation::StatusToAx => {
            out.c()
                .local_get(STATE)
                .i32_load16_u(mem(STATUS))
                .local_set(R);
            out.store_word(Register32::Eax, R);
        }
        X87Operation::StoreControl | X87Operation::StoreStatus => {
            out.c()
                .local_get(STATE)
                .i32_load16_u(mem(if operation == X87Operation::StoreControl {
                    CONTROL
                } else {
                    STATUS
                }))
                .local_set(R);
            out.address(address.expect("decoded state word store"));
            out.write(2, R);
        }
        X87Operation::LoadControl => {
            out.address(address.expect("decoded control load"));
            out.read(2, R);
            out.c()
                .local_get(STATE)
                .local_get(R)
                .i32_store16(mem(CONTROL))
                .local_get(STATE)
                .local_get(STATE)
                .i32_load16_u(mem(STATUS))
                .i32_const(!0x8080)
                .i32_and()
                .local_get(STATE)
                .i32_load16_u(mem(STATUS))
                .local_get(R)
                .i32_const(-1)
                .i32_xor()
                .i32_and()
                .i32_const(63)
                .i32_and()
                .if_(BlockType::Result(ValType::I32))
                .i32_const(0x8080)
                .else_()
                .i32_const(0)
                .end()
                .i32_or()
                .i32_store16(mem(STATUS));
        }
        _ => unreachable!("supported x87 transport"),
    }
    if operation.is_computation() {
        metadata(out, opcode, address.is_some());
    }
    true
}
