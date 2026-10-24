//! One canonical execution spine, reached only after an exact prefix request.
//! Request sites retain no native callback, publication protocol or status tree.
use super::{X87Transfer, frame, span};
use crate::cpu::core::StopReason;
use crate::cpu::core::emitter::{
    A, CARRY_IN, CTX, EIP, FLAGS, GP_FACTS_VALID, KIND, LEFT, Output, REMAIN, RETIRED, RIGHT,
    SERVICE_NEXT, SERVICE_ORDINAL, SERVICE_PC, SERVICE_RESUME, SERVICE_TRANSFER, VALUE, WIDTH,
    WRITE_STATUS, mem,
};
use wasm_encoder::BlockType;

const CANONICAL: i32 = 2;
const EXACT_SUFFIX: i32 = 3;
const FLAGS_OUTPUT: i32 = 1 << 28;
const GENERIC: i32 = 1 << 29;
const PRESERVES_FP: i32 = 1 << 30;

fn transfer(operation: crate::cpu::x86::ir::Operation) -> u32 {
    let (transfer, generic) = match X87Transfer::for_operation(operation) {
        Some(transfer) => (transfer, false),
        None => (
            X87Transfer {
                inputs: u8::MAX,
                outputs: u8::MAX,
                flags: crate::cpu::core::flags::ARITHMETIC,
                flags_output: true,
            },
            true,
        ),
    };
    u32::from(transfer.inputs)
        | u32::from(transfer.outputs) << 8
        | transfer.flags << 16
        | u32::from(transfer.flags_output) << 28
        | u32::from(generic) << 29
        | u32::from(crate::cpu::core::numeric_preserves_fp(operation)) << 30
}

pub(super) fn request(out: &mut Output<'_>, next: u32, planned: bool) {
    out.address_fact = None;
    let recovery = out.active_recovery.clone();
    let marker = out.fp_entry_marker;
    out.c().i32_const(0).local_set(marker);
    let frame = if planned {
        let before = out
            .before_frame
            .expect("canonical request has an exact input frame");
        crate::cpu::core::emitter::recovery::FrameAction::Select(before)
    } else {
        // A prior canonical instruction owns the actual bank. Its successor
        // cannot publish a predicted fast-success frame or stale aliases.
        crate::cpu::core::emitter::recovery::FrameAction::Clear
    };
    crate::cpu::core::emitter::recovery::request(
        out,
        recovery.as_ref(),
        recovery.as_ref().map(|r| &r.fp),
        frame,
        crate::cpu::core::emitter::recovery::program::KernelMasks {
            gpr: 0,
            flags: false,
        },
        crate::cpu::core::emitter::recovery::ExitAction {
            pc: None,
            retired: out.prefix,
        },
    );
    let ordinal = out
        .native
        .iter()
        .position(|instruction| instruction.pc == out.pc)
        .expect("canonical request retains its immutable owner ordinal");
    let packed = transfer(out.native[ordinal].operation);
    for (local, value) in [
        (SERVICE_ORDINAL, ordinal as u32),
        (SERVICE_PC, out.pc),
        (SERVICE_NEXT, next),
        (SERVICE_TRANSFER, packed),
    ] {
        out.c().i32_const(value as i32).local_set(local);
    }
    // The helper itself does not retire. Consume only the already completed
    // prefix, accounting for an arbitrary port's existing PREFIX_BASE.
    out.c()
        .i32_const(CANONICAL)
        .local_set(SERVICE_RESUME)
        .i32_const(StopReason::NeedCode as i32)
        .return_();
}

pub(super) fn request_exact_suffix(out: &mut Output<'_>, retired: u32) {
    // the failed fast path has not completed its current instruction. retain
    // the same before-state recovery as a public precise exit, then service it
    // inside this body instead of repeating its architectural bootstrap.
    out.address_fact = None;
    let marker = out.fp_entry_marker;
    out.c().i32_const(0).local_set(marker);
    out.publish_exit(Some(out.pc), retired);
    out.c()
        .i32_const(EXACT_SUFFIX)
        .local_set(SERVICE_RESUME)
        .i32_const(StopReason::NeedCode as i32)
        .return_();
}

fn emit_exact_suffix(out: &mut Output<'_>) {
    out.c()
        .local_get(SERVICE_RESUME)
        .i32_const(EXACT_SUFFIX)
        .i32_eq()
        .if_(BlockType::Empty);
    // use the same canonical publishers as the public exact-prefix wrapper.
    // they expose the completed fp pool, all gprs, flags, quota and sequence.
    crate::cpu::core::emitter::link::publish_canonical(out);
    span::forget_view(out);
    let precise = 2 + u32::from(!out.native.is_empty());
    out.c().local_get(CTX).call(precise).local_set(WRITE_STATUS);
    crate::cpu::core::emitter::link::reload_canonical(out);
    let marker = out.fp_entry_marker;
    out.c()
        .i32_const(0)
        .local_set(marker)
        .i32_const(0)
        .local_set(GP_FACTS_VALID)
        .i32_const(0)
        .local_set(frame::SELECTED)
        .i32_const(1)
        .local_set(frame::EXACT_SUFFIX)
        .i32_const(0)
        .local_set(SERVICE_RESUME);
    let hot_entry = crate::cpu::core::emitter::support::owner_defined(8, true);
    out.c()
        .local_get(WRITE_STATUS)
        .i32_const(StopReason::NeedCode as i32)
        .i32_eq()
        .local_get(EIP)
        .local_get(REMAIN)
        .call(hot_entry)
        .i32_and()
        .if_(BlockType::Empty);
    out.dynamic_pc(EIP);
    out.c()
        .i32_const(1)
        .local_set(SERVICE_RESUME)
        .else_()
        .local_get(WRITE_STATUS)
        .return_()
        .end()
        .end();
}

fn test(out: &mut Output<'_>, mask: i32) {
    out.c()
        .local_get(SERVICE_TRANSFER)
        .i32_const(mask)
        .i32_and();
}

fn publish_inputs(out: &mut Output<'_>) {
    out.c()
        .local_get(SERVICE_TRANSFER)
        .i32_const(16)
        .i32_shr_u()
        .i32_const(0xfff)
        .i32_and()
        .local_tee(A)
        .if_(BlockType::Empty);
    // The common service has no instruction-specific compile-time FrameState.
    // Materialize its actual runtime tuple with the literal transfer mask.
    for local in [FLAGS, KIND, WIDTH, LEFT, RIGHT, VALUE, CARRY_IN] {
        out.c().local_get(local);
    }
    let flags = out.internal(2);
    out.c()
        .local_get(A)
        .call(flags)
        .local_set(FLAGS)
        .local_get(CTX)
        .local_get(FLAGS)
        .i32_store(mem(36, 2))
        .end();
    test(out, FLAGS_OUTPUT);
    out.c()
        .if_(BlockType::Empty)
        .i32_const(0)
        .local_set(KIND)
        .end();
    for register in 0..8u32 {
        test(out, 1 << register);
        out.c()
            .if_(BlockType::Empty)
            .local_get(CTX)
            .local_get(register + 2)
            .i32_store(mem(u64::from(register * 4), 2))
            .end();
    }
    // Specialized x87 callbacks intentionally leave private Context32 alone.
    // Generic processor/vector/stack callbacks require the actual faulting PC.
    test(out, GENERIC);
    out.c()
        .if_(BlockType::Empty)
        .local_get(CTX)
        .local_get(SERVICE_PC)
        .i32_store(mem(32, 2))
        .end();
}

fn reload_outputs(out: &mut Output<'_>) {
    for register in 0..8u32 {
        test(out, 1 << (8 + register));
        out.c()
            .if_(BlockType::Empty)
            .local_get(CTX)
            .i32_load(mem(u64::from(register * 4), 2))
            .local_set(register + 2)
            .end();
    }
    test(out, FLAGS_OUTPUT);
    out.c()
        .if_(BlockType::Empty)
        .local_get(CTX)
        .i32_load(mem(36, 2))
        .local_set(FLAGS)
        .i32_const(0)
        .local_set(KIND)
        .end();
}

/// Called inside the existing nonzero service-kind arm, after selective commit.
/// Kind 1 is the unchanged computed restart; ordinary terminal exits add no test.
pub(in crate::cpu::core::emitter) fn emit(out: &mut Output<'_>) {
    if out.native.is_empty() {
        return;
    }
    emit_exact_suffix(out);
    out.c()
        .local_get(SERVICE_RESUME)
        .i32_const(CANONICAL)
        .i32_eq()
        .if_(BlockType::Empty);
    test(out, PRESERVES_FP);
    out.c().if_(BlockType::Empty);
    span::forget_view(out);
    out.c().else_();
    span::invalidate(out);
    out.c().end();
    out.c()
        .i32_const(1)
        .local_set(frame::EXACT_SUFFIX)
        .i32_const(0)
        .local_set(frame::SELECTED);
    publish_inputs(out);
    crate::cpu::core::memory::authority::flush(out.c().raw());
    out.c()
        .local_get(CTX)
        .local_get(SERVICE_PC)
        .local_get(SERVICE_ORDINAL)
        .call(2)
        .local_set(WRITE_STATUS);
    crate::cpu::core::memory::authority::load(out.c().raw());
    crate::cpu::core::memory::cache::invalidate(out.c().raw());
    out.c()
        .local_get(WRITE_STATUS)
        .i32_const(StopReason::SystemEntry as i32)
        .i32_gt_u()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Infrastructure as i32)
        .local_set(WRITE_STATUS)
        .end()
        .local_get(WRITE_STATUS)
        .i32_eqz()
        .local_get(WRITE_STATUS)
        .i32_const(StopReason::CodeInvalidated as i32)
        .i32_eq()
        .i32_or()
        .local_get(WRITE_STATUS)
        .i32_const(StopReason::SystemEntry as i32)
        .i32_eq()
        .i32_or()
        .if_(BlockType::Empty);
    reload_outputs(out);
    out.c()
        .local_get(REMAIN)
        .i32_const(1)
        .i32_sub()
        .local_set(REMAIN)
        .local_get(RETIRED)
        .i32_const(1)
        .i32_add()
        .local_set(RETIRED)
        .local_get(SERVICE_NEXT)
        .local_set(EIP)
        .local_get(WRITE_STATUS)
        .if_(BlockType::Empty)
        .i32_const(0)
        .local_set(SERVICE_RESUME)
        .local_get(WRITE_STATUS)
        .return_()
        .end();
    out.dynamic_pc(SERVICE_NEXT);
    out.c()
        .i32_const(0)
        .local_set(GP_FACTS_VALID)
        .i32_const(1)
        .local_set(SERVICE_RESUME)
        .else_()
        .local_get(SERVICE_PC)
        .local_set(EIP)
        .i32_const(0)
        .local_set(SERVICE_RESUME)
        .local_get(WRITE_STATUS)
        .return_()
        .end()
        .end();
}
