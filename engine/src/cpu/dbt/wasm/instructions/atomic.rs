use crate::cpu::dbt::wasm::helpers::integer::{call, payload, transfer};
use crate::cpu::dbt::wasm::imports::Imports;
use crate::cpu::dbt::wasm::instructions::memory;
use crate::cpu::dbt::wasm::locals::*;
use crate::cpu::x86::Register32;
use crate::cpu::x86::atomic::AtomicKind;
use crate::cpu::x86::ir::AddressSize;
use crate::cpu::x86::ir::AtomicSource;
use crate::cpu::x86::ir::ByteRegister;
use crate::cpu::x86::ir::EffectiveAddress;
use wasm_encoder::InstructionSink;

pub(in crate::cpu::dbt::wasm) fn operation(
    code: &mut InstructionSink<'_>,
    kind: AtomicKind,
    width: u8,
    address: EffectiveAddress,
    source: AtomicSource,
    imports: Imports,
    exit_depth: u32,
) {
    match source {
        AtomicSource::Immediate(value) => {
            code.i32_const(value as i32);
        }
        AtomicSource::Register(r) => {
            code.local_get(register(r));
        }
        AtomicSource::ByteRegister(r) => {
            let (parent, shift) = match r {
                ByteRegister::Al => (Register32::Eax, 0),
                ByteRegister::Cl => (Register32::Ecx, 0),
                ByteRegister::Dl => (Register32::Edx, 0),
                ByteRegister::Bl => (Register32::Ebx, 0),
                ByteRegister::Ah => (Register32::Eax, 8),
                ByteRegister::Ch => (Register32::Ecx, 8),
                ByteRegister::Dh => (Register32::Edx, 8),
                ByteRegister::Bh => (Register32::Ebx, 8),
            };
            code.local_get(register(parent))
                .i32_const(shift)
                .i32_shr_u()
                .i32_const(255)
                .i32_and();
        }
    }
    code.local_set(RESULT);
    memory::offset_value(code, address);
    if matches!(
        kind,
        AtomicKind::BitSet | AtomicKind::BitReset | AtomicKind::BitComplement
    ) && matches!(source, AtomicSource::Register(_))
    {
        code.local_get(RESULT);
        if width == 2 {
            code.i32_const(16).i32_shl().i32_const(16).i32_shr_s();
        }
        code.i32_const(if width == 2 { 4 } else { 5 })
            .i32_shr_s()
            .i32_const(i32::from(width))
            .i32_mul()
            .i32_add();
        if address.address_size == AddressSize::Bits16 {
            code.i32_const(65535).i32_and();
        }
    }
    memory::checked_offset(
        code,
        address.resolved_segment(),
        u32::from(width),
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    payload(code);
    code.local_get(STATE_PTR)
        .local_get(RESULT)
        .i32_store(transfer(0));
    call(
        code,
        (kind as u32) | (u32::from(width) << 8),
        u32::from(width),
        imports,
        exit_depth,
    );
    flags(
        code,
        match kind {
            AtomicKind::Not => 0,
            AtomicKind::Increment | AtomicKind::Decrement => 0x8d4,
            AtomicKind::BitSet | AtomicKind::BitReset | AtomicKind::BitComplement => 1,
            _ => 0x8d5,
        },
        exit_depth,
    );
}
pub(in crate::cpu::dbt::wasm) fn compare8(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    memory::checked_address(
        code,
        address,
        8,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    payload(code);
    call(code, 0x10000, 8, imports, exit_depth);
    flags(code, 0x40, exit_depth);
    for (offset, r) in [(8, Register32::Eax), (12, Register32::Edx)] {
        code.local_get(STATE_PTR)
            .i32_load(transfer(offset))
            .local_set(register(r));
    }
}

fn flags(code: &mut InstructionSink<'_>, mutable: u32, exit_depth: u32) {
    code.local_get(STATE_PTR).i32_load(transfer(4));
    crate::cpu::dbt::wasm::helpers::result::flags(code, mutable, exit_depth);
}
