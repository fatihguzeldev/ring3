use crate::cpu::dbt::wasm::imports::Imports;
use wasm_encoder::{BlockType, Function, InstructionSink, MemArg, ValType};

use crate::{
    abi::{
        arena::SEGMENTS_OFFSET,
        x86::{SEGMENTS_CACHE_OFFSETS, SEGMENTS_SELECTORS_OFFSET, SEGMENTS_VERSION_2},
    },
    cpu::x86::{SegmentAccess, SegmentRegister},
};

use super::super::locals::{
    ADDRESS, DETAIL, FAULT_ACCESS, FAULT_ADDRESS, FAULT_LENGTH, REASON, STATE_PTR,
};

// the internal function receives only validated context metadata and constant access descriptors.
const CONTEXT: u32 = 0;
const SELECTOR_OFFSET: u32 = 1;
const CACHE_OFFSET: u32 = 2;
const OFFSET: u32 = 3;
const WIDTH: u32 = 4;
const ACCESS: u32 = 5;
const CACHE: u32 = 6;
const LIMIT: u32 = 7;
const END: u32 = 8;

/// one pure Wasm function per module avoids duplicating the cache rules at every memory operand.
/// parameters are context pointer, selector/cache offsets, logical offset, width, access; 1 means fault.
pub(in crate::cpu::dbt::wasm) fn build_function() -> Function {
    let mut function = Function::new([(3, ValType::I32)]);
    let mut code = function.instructions();

    code.local_get(CONTEXT)
        .local_get(SELECTOR_OFFSET)
        .i32_add()
        .i32_load16_u(arg(0, 1));

    code.i32_const(0xfffc).i32_and().i32_eqz();
    return_fault_if(&mut code);
    code.local_get(WIDTH).i32_eqz();
    return_fault_if(&mut code);
    code.local_get(OFFSET)
        .local_get(WIDTH)
        .i32_const(1)
        .i32_sub()
        .i32_add()
        .local_tee(END)
        .local_get(OFFSET)
        .i32_lt_u();
    // the full-4GiB overflow choice is deterministic; Intel permits implementation-specific behavior.
    return_fault_if(&mut code);

    code.local_get(CONTEXT).i32_load16_u(arg(4, 1));

    code.i32_const(i32::from(SEGMENTS_VERSION_2))
        .i32_ge_u()
        .if_(BlockType::Result(ValType::I32));

    code.local_get(CONTEXT)
        .local_get(CACHE_OFFSET)
        .i32_add()
        .i32_load(arg(0, 2));

    code.local_set(CACHE)
        .local_get(CACHE)
        .i32_const(1 << 28)
        .i32_and()
        .i32_eqz();
    return_fault_if(&mut code);
    code.local_get(ACCESS).i32_eqz().if_(BlockType::Empty);
    code.local_get(CACHE)
        .i32_const(8 << 20)
        .i32_and()
        .i32_eqz()
        .i32_eqz()
        .local_get(CACHE)
        .i32_const(2 << 20)
        .i32_and()
        .i32_eqz()
        .i32_and();
    return_fault_if(&mut code);
    code.else_()
        .local_get(ACCESS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    code.local_get(CACHE)
        .i32_const(10 << 20)
        .i32_and()
        .i32_const(2 << 20)
        .i32_ne();
    return_fault_if(&mut code);
    code.else_()
        .local_get(ACCESS)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    code.local_get(CACHE).i32_const(8 << 20).i32_and().i32_eqz();
    return_fault_if(&mut code);
    code.end().end().end();
    code.local_get(CACHE)
        .i32_const(0x000f_ffff)
        .i32_and()
        .local_set(LIMIT)
        .local_get(CACHE)
        .i32_const(1 << 24)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(LIMIT)
        .i32_const(12)
        .i32_shl()
        .i32_const(0xfff)
        .i32_or()
        .local_set(LIMIT)
        .end()
        .local_get(CACHE)
        .i32_const(12 << 20)
        .i32_and()
        .i32_const(4 << 20)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .local_get(OFFSET)
        .local_get(LIMIT)
        .i32_le_u()
        .local_get(END)
        .i32_const(0xffff)
        .i32_gt_u()
        .local_get(CACHE)
        .i32_const(1 << 25)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .i32_or()
        .else_()
        .local_get(END)
        .local_get(LIMIT)
        .i32_gt_u()
        .end()
        .else_();
    // legacy records imply readable CS code and writable data in every other usable segment.
    code.local_get(ACCESS)
        .i32_const(1)
        .i32_eq()
        .local_get(SELECTOR_OFFSET)
        .i32_const((SEGMENTS_SELECTORS_OFFSET + 2 * SegmentRegister::Cs.index()) as i32)
        .i32_eq()
        .i32_and()
        .local_get(ACCESS)
        .i32_const(2)
        .i32_eq()
        .local_get(SELECTOR_OFFSET)
        .i32_const((SEGMENTS_SELECTORS_OFFSET + 2 * SegmentRegister::Cs.index()) as i32)
        .i32_ne()
        .i32_and()
        .i32_or()
        .end()
        .end();
    function
}

/// ADDRESS contains a logical offset; the pure callee leaves every caller local untouched.
pub(in crate::cpu::dbt::wasm) fn check_span(
    code: &mut InstructionSink<'_>,
    segment: SegmentRegister,
    width: u32,
    access: SegmentAccess,
    imports: Imports,
    exit_depth: u32,
) {
    assert!(width != 0, "guest memory spans have a nonzero width");
    code.local_get(STATE_PTR)
        .i32_const(SEGMENTS_OFFSET as i32)
        .i32_add()
        .i32_const((SEGMENTS_SELECTORS_OFFSET + 2 * segment.index()) as i32)
        .i32_const(SEGMENTS_CACHE_OFFSETS[segment.index()] as i32)
        .local_get(ADDRESS)
        .i32_const(width as i32)
        .i32_const(match access {
            SegmentAccess::Read => 0,
            SegmentAccess::Write => 1,
            SegmentAccess::Execute => 2,
            SegmentAccess::CacheFlush => 3,
        })
        .call(
            imports
                .segment_check
                .expect("memory operand has a segment check"),
        )
        .if_(BlockType::Empty)
        .i32_const(if segment == SegmentRegister::Ss {
            15
        } else {
            11
        })
        .local_set(REASON);
    for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.i32_const(0).local_set(local);
    }
    code.br(exit_depth + 1).end();
}

fn return_fault_if(code: &mut InstructionSink<'_>) {
    code.if_(BlockType::Empty).i32_const(1).return_().end();
}

fn arg(offset: usize, align: u32) -> MemArg {
    MemArg {
        offset: offset as u64,
        align,
        memory_index: 0,
    }
}
