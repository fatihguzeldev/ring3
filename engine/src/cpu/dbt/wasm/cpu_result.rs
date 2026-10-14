use wasm_encoder::{BlockType, InstructionSink};

use super::{
    locals::*,
    memory::{
        add_check, exit_if_failed, expect_field, helper_field, infrastructure, protocol_if_bad,
        reset_checks,
    },
};

pub(super) fn value(code: &mut InstructionSink<'_>) {
    helper_field(code, 20);
}

/// consume returned flags only after validating the instruction's preservation contract.
pub(super) fn flags(
    code: &mut InstructionSink<'_>,
    mutable: u32,
    exit_depth: u32,
    raw_task_context: bool,
) {
    code.local_set(RESULT);
    reset_checks(code);
    code.local_get(RESULT)
        .local_get(FLAGS)
        .i32_xor()
        .i32_const(!mutable as i32)
        .i32_and();
    add_check(code);
    protocol_if_bad(code);
    exit_if_failed(code, exit_depth, raw_task_context);
    code.local_get(RESULT).local_set(FLAGS);
}

pub(super) fn validate(
    code: &mut InstructionSink<'_>,
    length: u32,
    access_mask: u32,
    allow_smc: bool,
    exit_depth: u32,
    raw_task_context: bool,
) {
    validate_span(
        code,
        length,
        access_mask,
        allow_smc,
        Validation::Ordinary { mask: None },
        exit_depth,
        raw_task_context,
    );
}

// gate packets report an effective frame slot, not an ordinary scalar-memory span.
// direct transfers and every other CPU helper continue to require their existing version.
pub(super) fn validate_gate(
    code: &mut InstructionSink<'_>,
    slot: u32,
    allow_smc: bool,
    exit_depth: u32,
    raw_task_context: bool,
) {
    assert!(matches!(slot, 0 | 2 | 4));
    validate_span(
        code,
        slot,
        if slot == 0 { 0 } else { 2 },
        allow_smc,
        Validation::Gate,
        exit_depth,
        raw_task_context,
    );
}

pub(super) fn validate_interrupt(
    code: &mut InstructionSink<'_>,
    slot: u32,
    vector: u8,
    original_flags: u32,
    exit_depth: u32,
    raw_task_context: bool,
) {
    assert!(matches!(slot, 0 | 2 | 4));
    validate_span(
        code,
        slot,
        if slot == 0 { 0 } else { 2 },
        true,
        Validation::Interrupt {
            vector,
            original_flags,
        },
        exit_depth,
        raw_task_context,
    );
}

pub(super) fn validate_masked(
    code: &mut InstructionSink<'_>,
    length: u32,
    mask_local: u32,
    exit_depth: u32,
    raw_task_context: bool,
) {
    assert!((1..=16).contains(&length));
    validate_span(
        code,
        length,
        2,
        true,
        Validation::Ordinary {
            mask: Some(mask_local),
        },
        exit_depth,
        raw_task_context,
    );
}

#[derive(Clone, Copy)]
pub(super) struct AvxValidation {
    pub(super) selected_mask: Option<u32>,
    pub(super) old_mxcsr: Option<u32>,
    pub(super) general_protection: bool,
    pub(super) stack_segment: bool,
}

pub(super) fn validate_avx(
    code: &mut InstructionSink<'_>,
    length: u32,
    access_mask: u32,
    allow_smc: bool,
    validation: AvxValidation,
    exit_depth: u32,
    raw_task_context: bool,
) {
    assert!(length <= 32);
    if validation.selected_mask.is_some() {
        assert!(length == 16 || length == 32);
    }
    validate_span(
        code,
        length,
        access_mask,
        allow_smc,
        Validation::Avx(validation),
        exit_depth,
        raw_task_context,
    );
}

/// selected xstate spans are checked by their owner before this common status/publication path.
pub(super) fn validate_xstate(
    code: &mut InstructionSink<'_>,
    save: bool,
    exit_depth: u32,
    raw_task_context: bool,
) {
    validate_span(
        code,
        832,
        if save { 3 } else { 1 },
        save,
        Validation::XState,
        exit_depth,
        raw_task_context,
    );
}

#[derive(Clone, Copy)]
enum Validation {
    Ordinary { mask: Option<u32> },
    Avx(AvxValidation),
    XState,
    Gate,
    Interrupt { vector: u8, original_flags: u32 },
}

fn validate_span(
    code: &mut InstructionSink<'_>,
    length: u32,
    access_mask: u32,
    allow_smc: bool,
    validation: Validation,
    exit_depth: u32,
    raw_task_context: bool,
) {
    let mask = match validation {
        Validation::Ordinary { mask } => mask,
        Validation::Avx(validation) => validation.selected_mask,
        _ => None,
    };
    let xstate = matches!(validation, Validation::XState);
    let gate = matches!(validation, Validation::Gate);
    let interrupt = if let Validation::Interrupt {
        vector,
        original_flags,
    } = validation
    {
        Some((vector, original_flags))
    } else {
        None
    };
    let control = gate || interrupt.is_some();
    let avx = if let Validation::Avx(value) = validation {
        Some(value)
    } else {
        None
    };
    assert!(if xstate {
        length == 832
    } else {
        length <= crate::abi::cpu_helper::MAX_CPU_SPAN as u32
    });
    assert!(access_mask <= 3);
    code.local_get(HELPER_STATUS);
    if allow_smc {
        code.i32_const(11)
            .i32_ne()
            .local_get(HELPER_STATUS)
            .i32_eqz()
            .i32_eqz()
            .i32_and();
    }
    code.if_(BlockType::Empty);
    infrastructure(code, 3);
    code.else_();
    reset_checks(code);
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (
            4,
            if interrupt.is_some() {
                0x1000b
            } else if gate {
                0x10009
            } else if xstate {
                0x10008
            } else {
                0x10007
            },
        ),
        (8, 40),
        (12, 0),
    ] {
        expect_field(code, offset, expected);
    }
    if interrupt.is_some() && length != 0 {
        helper_field(code, 36);
        code.i32_const(!0x100)
            .i32_and()
            .i32_const(length as i32)
            .i32_ne();
        add_check(code);
    } else {
        expect_field(code, 36, length);
    }
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    helper_field(code, 16);
    code.local_set(RHS);
    code.local_get(RHS).i32_eqz().if_(BlockType::Empty);
    success(code, if xstate || control { 0 } else { length }, mask);
    if interrupt.is_some() && length == 0 {
        infrastructure(code, 2);
    }
    if control && length == 2 {
        reset_checks(code);
        helper_field(code, 20);
        code.i32_const(0xffff).i32_gt_u();
        add_check(code);
        protocol_if_bad(code);
    }
    code.else_();
    code.local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_eq()
        .if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    code.local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    fault(code, length, access_mask, mask, xstate, interrupt.is_some());
    code.else_()
        .local_get(RHS)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    if control && length == 0 {
        infrastructure(code, 2);
    } else {
        rejected(code);
    }
    code.else_()
        .local_get(RHS)
        .i32_const(3)
        .i32_ge_u()
        .local_get(RHS)
        .i32_const(7)
        .i32_le_u()
        .i32_and()
        .if_(BlockType::Empty);
    exception(
        code,
        length,
        avx,
        control,
        interrupt.map(|(vector, _)| vector),
    );
    code.else_();
    if interrupt.is_some() && length == 0 {
        code.local_get(RHS)
            .i32_const(8)
            .i32_eq()
            .if_(BlockType::Empty);
        interrupt_unsupported(code);
        code.else_();
        infrastructure(code, 2);
        code.end();
    } else {
        infrastructure(code, 2);
    }
    code.end().end().end().end().end().end().end();
    if let Some((_, original_flags)) = interrupt {
        code.local_get(REASON)
            .i32_const(4)
            .i32_eq()
            .local_get(DETAIL)
            .i32_const(4)
            .i32_eq()
            .i32_and()
            .if_(BlockType::Empty)
            .local_get(original_flags)
            .local_set(FLAGS)
            .local_get(SNAPSHOT_SHADOW)
            .local_set(DEBUG_SHADOW)
            .end();
    }
    exit_if_failed(code, exit_depth, raw_task_context);
}

fn success(code: &mut InstructionSink<'_>, length: u32, mask: Option<u32>) {
    reset_checks(code);
    for offset in [24, 28, 32] {
        expect_field(code, offset, 0);
    }
    if let Some(mask_local) = mask {
        if length < 32 {
            code.local_get(mask_local)
                .i32_const(!((1u32 << length) - 1) as i32)
                .i32_and();
            add_check(code);
        }
        code.local_get(HELPER_STATUS)
            .i32_const(11)
            .i32_eq()
            .local_get(mask_local)
            .i32_eqz()
            .i32_and();
        add_check(code);
    }
    protocol_if_bad(code);
}

fn fault(
    code: &mut InstructionSink<'_>,
    length: u32,
    access_mask: u32,
    mask: Option<u32>,
    xstate: bool,
    interrupt: bool,
) {
    if length == 0 || access_mask == 0 {
        infrastructure(code, 2);
        return;
    }
    reset_checks(code);
    expect_field(code, 20, 0);
    helper_field(code, 32);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    add_check(code);
    helper_field(code, 32);
    code.i32_const(access_mask as i32).i32_and();
    helper_field(code, 32);
    code.i32_ne();
    add_check(code);
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    add_check(code);
    if !xstate {
        helper_field(code, 28);
        code.local_get(ADDRESS)
            .i32_sub()
            .local_tee(RESULT)
            .i32_const(length as i32 - 1)
            .i32_gt_u();
        add_check(code);
        if let Some(mask_local) = mask {
            code.local_get(mask_local)
                .local_get(RESULT)
                .i32_shr_u()
                .i32_const(1)
                .i32_and()
                .i32_eqz();
            add_check(code);
        }
    }
    if let Some(mask_local) = mask
        && length < 32
    {
        code.local_get(mask_local)
            .i32_const(!((1u32 << length) - 1) as i32)
            .i32_and();
        add_check(code);
    }
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    code.i32_const(5).local_set(REASON);
    for (offset, local) in [(24, DETAIL), (28, FAULT_ADDRESS), (32, FAULT_ACCESS)] {
        helper_field(code, offset);
        code.local_set(local);
    }
    if interrupt {
        code.i32_const(length as i32);
    } else {
        helper_field(code, 36);
    }
    code.local_set(FAULT_LENGTH);
    code.end();
}

fn interrupt_unsupported(code: &mut InstructionSink<'_>) {
    reset_checks(code);
    for offset in [20, 28, 32] {
        expect_field(code, offset, 0);
    }
    expect_field(code, 24, 4);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_()
        .i32_const(4)
        .local_set(REASON)
        .i32_const(4)
        .local_set(DETAIL)
        .end();
}

fn rejected(code: &mut InstructionSink<'_>) {
    reset_checks(code);
    for offset in [20, 28, 32] {
        expect_field(code, offset, 0);
    }
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    helper_field(code, 24);
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    infrastructure(code, 1);
    code.else_();
    infrastructure(code, 3);
    code.end().end();
}

fn exception(
    code: &mut InstructionSink<'_>,
    length: u32,
    avx: Option<AvxValidation>,
    gate: bool,
    interrupt: Option<u8>,
) {
    reset_checks(code);
    for offset in [20, 28, 32] {
        expect_field(code, offset, 0);
    }
    if let Some(vector) = interrupt.filter(|_| length == 0) {
        code.local_get(RHS)
            .i32_const(3)
            .i32_ne()
            .local_get(RHS)
            .i32_const(6)
            .i32_ne()
            .i32_and();
        add_check(code);
        expect_field(code, 24, (u32::from(vector) << 3) | 2);
    } else if gate {
        code.local_get(RHS)
            .i32_const(4)
            .i32_eq()
            .local_get(RHS)
            .i32_const(5)
            .i32_eq()
            .i32_or();
        add_check(code);
        helper_field(code, 24);
        code.i32_const(0xfffc).i32_gt_u();
        helper_field(code, 24);
        code.i32_const(3).i32_and().i32_or();
        add_check(code);
        code.local_get(RHS)
            .i32_const(6)
            .i32_eq()
            .if_(BlockType::Empty);
        helper_field(code, 24);
        code.i32_eqz();
        add_check(code);
        code.end();
        code.local_get(RHS)
            .i32_const(7)
            .i32_eq()
            .if_(BlockType::Empty);
        expect_field(code, 24, 0);
        if length == 0 {
            code.i32_const(1);
            add_check(code);
        }
        code.end();
    }
    if let Some(validation) = avx {
        code.local_get(RHS)
            .i32_const(3)
            .i32_ne()
            .local_get(RHS)
            .i32_const(5)
            .i32_ne()
            .i32_and()
            .local_get(RHS)
            .i32_const(7)
            .i32_ne()
            .i32_and();
        add_check(code);
        for (status, permitted) in [
            (3, validation.general_protection),
            (7, validation.stack_segment),
        ] {
            if !permitted {
                code.local_get(RHS).i32_const(status).i32_eq();
                add_check(code);
            }
        }
        code.local_get(RHS)
            .i32_const(5)
            .i32_eq()
            .if_(BlockType::Empty);
        if let Some(old_mxcsr) = validation.old_mxcsr {
            helper_field(code, 24);
            code.local_get(old_mxcsr)
                .i32_xor()
                .i32_const(!0x3f)
                .i32_and();
            add_check(code);
            helper_field(code, 24);
            code.local_get(old_mxcsr)
                .i32_and()
                .i32_const(0x3f)
                .i32_and()
                .local_get(old_mxcsr)
                .i32_const(0x3f)
                .i32_and()
                .i32_ne();
            add_check(code);
            helper_field(code, 24);
            code.local_get(STATE_PTR)
                .i32_load(wasm_encoder::MemArg {
                    offset: (crate::abi::arena::SIMD_OFFSET + crate::abi::x86::SIMD_MXCSR_OFFSET)
                        as u64,
                    align: 2,
                    memory_index: 0,
                })
                .i32_ne();
            add_check(code);
            helper_field(code, 24);
            helper_field(code, 24);
            code.i32_const(7)
                .i32_shr_u()
                .i32_const(-1)
                .i32_xor()
                .i32_and()
                .i32_const(0x3f)
                .i32_and()
                .i32_eqz();
            add_check(code);
        } else {
            code.i32_const(1);
            add_check(code);
        }
        code.else_();
        expect_field(code, 24, 0);
        if length == 0 {
            code.i32_const(1);
            add_check(code);
        }
        if let Some(mask) = validation.selected_mask {
            code.local_get(mask).i32_eqz();
            add_check(code);
        }
        code.end();
    }
    code.local_get(RHS)
        .i32_const(4)
        .i32_eq()
        .local_get(RHS)
        .i32_const(5)
        .i32_eq()
        .i32_or()
        .if_(BlockType::Empty);
    helper_field(code, 24);
    code.i32_const(0xffff).i32_gt_u();
    add_check(code);
    code.end();
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    code.local_get(RHS).i32_const(8).i32_add().local_set(REASON);
    helper_field(code, 24);
    code.local_set(DETAIL);
    code.end();
}
