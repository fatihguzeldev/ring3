//! completed typed blocks publish directly into the existing physical fp pool.
use super::{FpPayload, FpRecovery, Output, frame, mem, pool};
use crate::cpu::core::{
    emitter::{FP_PAYLOAD_TOKENS, FP_TAG_TOKENS},
    fp_pool as p,
    region::values::{FrameStateId, executable::ExecValueId},
};
use wasm_encoder::BlockType;

// these scratch locals are otherwise used only by the common inline commit.
const DIRTY: u32 = 158;
const TAG: u32 = 159;
const TOP: u32 = 160;
const SHIFT: u32 = 161;
const FIP: u32 = 163;
const OPCODE: u32 = 164;
const CS: u32 = 165;
const DS: u32 = 166;

fn get(out: &mut Output<'_>, value: ExecValueId) {
    let local =
        out.local_layout.values[value.0].expect("a completed recovery root has typed storage");
    out.c().local_get(local);
}

fn stage(out: &mut Output<'_>, value: ExecValueId) {
    out.demand(value);
    out.c().drop();
}

fn selected(out: &mut Output<'_>, aliases: u8, written: u8) -> bool {
    if aliases & written != 0 {
        false
    } else {
        out.c()
            .local_get(DIRTY)
            .i32_const(i32::from(aliases))
            .i32_and()
            .if_(BlockType::Empty);
        true
    }
}

fn stage_payload(out: &mut Output<'_>, payload: FpPayload) {
    match payload {
        FpPayload::Native { value, .. } => stage(out, value),
        FpPayload::Opaque { low, high } => {
            stage(out, low);
            stage(out, high);
        }
        FpPayload::Tagged {
            low,
            high,
            native,
            valid,
            certificate,
        } => {
            stage(out, valid);
            let before = out.available.clone();
            get(out, valid);
            out.c().if_(BlockType::Empty);
            stage(out, native);
            stage(out, certificate);
            out.c().else_();
            out.available = before.clone();
            stage(out, low);
            stage(out, high);
            out.c().end();
            out.available = before;
        }
    }
}

fn physical(out: &mut Output<'_>, logical: u8) {
    out.c()
        .local_get(TOP)
        .i32_const(i32::from(logical))
        .i32_add()
        .i32_const(7)
        .i32_and();
}

fn write_payload(out: &mut Output<'_>, payload: FpPayload, logical: u8, control: ExecValueId) {
    let native_writer = out.internal(12);
    let raw_writer = out.internal(13);
    match payload {
        FpPayload::Native { value, domain, .. } => {
            out.c().local_get(pool::POINTER);
            physical(out, logical);
            get(out, value);
            out.c().i32_const(domain.certificate_bits() as i32);
            get(out, control);
            out.c().call(native_writer).drop();
        }
        FpPayload::Opaque { low, high } => {
            out.c().local_get(pool::POINTER);
            physical(out, logical);
            get(out, low);
            get(out, high);
            out.c().call(raw_writer).drop();
        }
        FpPayload::Tagged {
            low,
            high,
            native,
            valid,
            certificate,
        } => {
            get(out, valid);
            out.c().if_(BlockType::Empty).local_get(pool::POINTER);
            physical(out, logical);
            get(out, native);
            get(out, certificate);
            get(out, control);
            out.c()
                .call(native_writer)
                .drop()
                .else_()
                .local_get(pool::POINTER);
            physical(out, logical);
            get(out, low);
            get(out, high);
            out.c().call(raw_writer).drop().end();
        }
    }
}

/// only a caller that has completed the whole executable block may use this.
/// scalar state and retirement are already committed by that caller.
pub(in crate::cpu::core::emitter) fn commit_completed(
    out: &mut Output<'_>,
    recovery: &FpRecovery,
    frame: FrameStateId,
) {
    if out.native.is_empty() {
        return;
    }
    let block = out
        .current_block
        .expect("a completed handoff has a source block");
    assert_eq!(frame, out.plan.block(block).exit);
    if !out.executable.fp_parameters(block).header {
        return;
    }
    let before = out.available.clone();
    let metadata = out.plan.metadata(frame).clone();
    let shape = out.plan.shape(frame).clone();
    let delta = i32::from(recovery.top_delta).rem_euclid(8);
    let possible = out
        .executable
        .fp_parameters(block)
        .incoming_dirty
        .rotate_right(delta as u32)
        | recovery.written;
    out.c()
        .local_get(84)
        .local_get(frame::EXACT_SUFFIX)
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Empty);

    // all lazy reads finish before any destination record or header changes.
    for value in [
        recovery.top,
        recovery.control,
        recovery.status.bits,
        recovery.status.pending,
        recovery.status.left,
        recovery.status.right,
        recovery.status.result,
        recovery.metadata.fip,
        recovery.metadata.fdp,
        recovery.metadata.opcode,
        recovery.metadata.cs,
        recovery.metadata.ds,
    ] {
        stage(out, value);
    }
    get(out, recovery.top);
    out.c()
        .i32_const(delta)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .local_set(TOP);
    out.c().local_get(86);
    if delta != 0 {
        out.c()
            .i32_const(delta)
            .i32_shr_u()
            .local_get(86)
            .i32_const(8 - delta)
            .i32_shl()
            .i32_or();
    }
    out.c()
        .i32_const(i32::from(recovery.written))
        .i32_or()
        .i32_const(255)
        .i32_and()
        .local_set(DIRTY)
        .local_get(89)
        .local_set(TAG);

    let mut seen = 0u8;
    for logical in 0..8 {
        let cell = shape.cells[logical].0;
        if seen & (1 << cell) != 0 {
            continue;
        }
        seen |= 1 << cell;
        let aliases = (0..8)
            .filter(|&index| shape.cells[index].0 == cell)
            .fold(0u8, |mask, index| mask | (1 << index));
        if aliases & possible == 0 {
            continue;
        }
        assert!(
            (0..8)
                .filter(|&index| aliases & (1 << index) != 0)
                .all(|index| recovery.payload[index] == recovery.payload[logical])
        );
        if let FpPayload::Native { epoch, .. } = recovery.payload[logical] {
            assert_eq!(
                epoch, recovery.epoch,
                "native proof belongs to the current CW epoch"
            );
        }
        let available = out.available.clone();
        let conditional = selected(out, aliases & possible, recovery.written);
        stage_payload(out, recovery.payload[logical]);
        if conditional {
            out.c().end();
        }
        out.available = available;
    }

    for logical in 0..8u8 {
        if recovery.tag_written & (1 << logical) == 0 {
            continue;
        }
        stage(out, recovery.tags[usize::from(logical)]);
        physical(out, logical);
        out.c()
            .i32_const(1)
            .i32_shl()
            .local_set(SHIFT)
            .local_get(TAG)
            .i32_const(3)
            .local_get(SHIFT)
            .i32_shl()
            .i32_const(-1)
            .i32_xor()
            .i32_and();
        get(out, recovery.tags[usize::from(logical)]);
        out.c().local_get(SHIFT).i32_shl().i32_or().local_set(TAG);
    }

    // frame metadata may supply the last successful legacy instruction's stamp.
    if let Some((pc, opcode)) = metadata.instruction {
        out.c()
            .i32_const(pc as i32)
            .local_set(FIP)
            .i32_const(i32::from(opcode))
            .local_set(OPCODE)
            .local_get(96)
            .i32_load16_u(mem(102, 1))
            .local_set(CS);
    }
    if let Some(data) = metadata.data.as_ref() {
        out.c()
            .local_get(96)
            .i32_load16_u(mem(
                100 + data.address.resolved_segment().index() as u64 * 2,
                1,
            ))
            .local_set(DS);
    }

    for logical in 0..8u8 {
        if possible & (1 << logical) == 0 {
            continue;
        }
        let conditional = selected(out, 1 << logical, recovery.written);
        write_payload(
            out,
            recovery.payload[usize::from(logical)],
            logical,
            recovery.control,
        );
        if conditional {
            out.c().end();
        }
    }
    out.c()
        .local_get(pool::POINTER)
        .local_get(pool::POINTER)
        .i32_load(mem(p::CHANGED, 2))
        .local_get(DIRTY)
        .local_get(TOP)
        .i32_shl()
        .local_get(DIRTY)
        .i32_const(8)
        .local_get(TOP)
        .i32_sub()
        .i32_shr_u()
        .i32_or()
        .i32_const(255)
        .i32_and()
        .i32_or()
        .i32_store(mem(p::CHANGED, 2));
    for (offset, value, override_local) in [
        (p::CONTROL, recovery.control, None),
        (p::STATUS, recovery.status.bits, None),
        (
            p::FIP,
            recovery.metadata.fip,
            metadata.instruction.map(|_| FIP),
        ),
        (p::FDP, recovery.metadata.fdp, None),
        (
            p::OPCODE,
            recovery.metadata.opcode,
            metadata.instruction.map(|_| OPCODE),
        ),
        (
            p::CS,
            recovery.metadata.cs,
            metadata.instruction.map(|_| CS),
        ),
        (
            p::DS,
            recovery.metadata.ds,
            metadata.data.as_ref().map(|_| DS),
        ),
        (p::PENDING, recovery.status.pending, None),
    ] {
        out.c().local_get(pool::POINTER);
        if let Some(local) = override_local {
            out.c().local_get(local);
        } else {
            get(out, value);
        }
        if offset == p::STATUS {
            out.c().i32_const(!0x3800).i32_and();
        }
        out.c().i32_store(mem(offset, 2));
    }
    for (offset, local) in [(p::TAG, TAG), (p::TOP, TOP)] {
        out.c()
            .local_get(pool::POINTER)
            .local_get(local)
            .i32_store(mem(offset, 2));
    }
    for (offset, value) in [
        (p::PENDING_LEFT, recovery.status.left),
        (p::PENDING_RIGHT, recovery.status.right),
        (p::PENDING_RESULT, recovery.status.result),
    ] {
        out.c().local_get(pool::POINTER);
        get(out, value);
        out.c().f64_store(mem(offset, 3));
    }
    for local in [
        84,
        85,
        86,
        pool::LOADED,
        pool::ADMITTED,
        frame::SELECTED,
        FP_TAG_TOKENS,
        out.fp_entry_marker,
    ] {
        out.c().i32_const(0).local_set(local);
    }
    out.c().i64_const(0).local_set(FP_PAYLOAD_TOKENS).end();
    out.available = before;
}
