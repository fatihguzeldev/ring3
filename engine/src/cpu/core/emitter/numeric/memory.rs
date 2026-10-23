use super::super::{ADDRESS, CTX, LOGICAL_ADDRESS, Output, mem};
use crate::cpu::core::memory::{authority, cache};
use crate::cpu::x86::{SegmentRegister, ir::EffectiveAddress};

const SLOT: u32 = 33;
const POINTER: u32 = 34;
const BANK: u32 = 37;
const TMP: u32 = 45;
const VERSION: u32 = 46;

pub(super) fn prepare(
    out: &mut Output<'_>,
    address: EffectiveAddress,
    width: u32,
    write: bool,
    aligned: bool,
    destination: u32,
) {
    let segment = address.resolved_segment();
    out.c()
        .local_get(BANK)
        .i32_load16_u(mem(100 + (segment.index() * 2) as u64, 1))
        .i32_const(!3)
        .i32_and()
        .i32_eqz()
        .br_if(0);
    if write && segment == SegmentRegister::Cs {
        out.c().br(0);
    }
    out.address(address);
    out.c()
        .local_get(LOGICAL_ADDRESS)
        .i32_const((u32::MAX - (width - 1)) as i32)
        .i32_gt_u()
        .br_if(0)
        .local_get(ADDRESS)
        .i32_const(4095)
        .i32_and()
        .i32_const((4096 - width) as i32)
        .i32_gt_u()
        .br_if(0);
    if aligned {
        out.c()
            .local_get(ADDRESS)
            .i32_const((width - 1) as i32)
            .i32_and()
            .br_if(0);
    }
    let translate = out.internal(9);
    cache::translate(out.c().raw(), if write { 2 } else { 1 }, translate);
    if write {
        out.c()
            .local_get(CTX)
            .i32_load(mem(72, 2))
            .local_get(SLOT)
            .i32_const(2)
            .i32_shl()
            .i32_add()
            .i32_load(mem(0, 2))
            .br_if(0)
            .block(wasm_encoder::BlockType::Empty)
            .local_get(CTX)
            .i32_load(mem(76, 2))
            .local_tee(TMP)
            .i32_eqz()
            .br_if(0)
            .local_get(TMP)
            .local_get(ADDRESS)
            .i32_const(12)
            .i32_shr_u()
            .i32_const(2)
            .i32_shl()
            .i32_add()
            .i32_load(mem(0, 2))
            .br_if(1)
            .end()
            .local_get(authority::SEQUENCE)
            .local_tee(VERSION)
            .i64_const(-1)
            .i64_eq()
            .br_if(0)
            .local_get(VERSION)
            .i64_const(1)
            .i64_add()
            .local_set(VERSION);
    }
    out.c().local_get(POINTER).local_set(destination);
}

pub(super) fn commit(out: &mut Output<'_>) {
    out.c()
        .local_get(CTX)
        .i32_load(mem(84, 2))
        .local_get(SLOT)
        .i32_const(4)
        .i32_shl()
        .i32_add()
        .local_get(VERSION)
        .i64_store(mem(8, 3))
        .local_get(VERSION)
        .local_set(authority::SEQUENCE);
}
