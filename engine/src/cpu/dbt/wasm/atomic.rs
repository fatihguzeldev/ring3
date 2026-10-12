use super::{locals::*, memory};
use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{
        Register32,
        atomic::AtomicKind,
        ir::{AddressSize, AtomicSource, ByteRegister, EffectiveAddress},
    },
};
use wasm_encoder::{InstructionSink, MemArg};

pub(super) fn transfer(offset: usize) -> MemArg {
    MemArg {
        offset: (TRANSFER_OFFSET + offset) as u64,
        align: 2,
        memory_index: 0,
    }
}
pub(super) fn payload(code: &mut InstructionSink<'_>) {
    for (i, r) in [
        Register32::Eax,
        Register32::Edx,
        Register32::Ebx,
        Register32::Ecx,
    ]
    .into_iter()
    .enumerate()
    {
        code.local_get(STATE_PTR)
            .local_get(register(r))
            .i32_store(transfer(8 + i * 4));
    }
    code.local_get(STATE_PTR)
        .local_get(FLAGS)
        .i32_store(transfer(4));
}
pub(super) fn call(
    code: &mut InstructionSink<'_>,
    descriptor: u32,
    width: u32,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let index = imports
        .cpu_integer
        .expect("integer helper")
        .emit_binding(code);
    code.i32_const(descriptor as i32)
        .local_get(ADDRESS)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_result::validate(
        code,
        width,
        if width == 0 { 0 } else { 3 },
        width != 0,
        exit_depth,
    );
    memory::reset_checks(code);
    memory::expect_field(code, 20, 0);
    memory::protocol_if_bad(code);
    memory::exit_if_failed(code, exit_depth);
}
pub(super) fn operation(
    code: &mut InstructionSink<'_>,
    kind: AtomicKind,
    width: u8,
    address: EffectiveAddress,
    source: AtomicSource,
    imports: memory::Imports,
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
pub(super) fn compare8(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: memory::Imports,
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
    super::cpu_result::flags(code, mutable, exit_depth);
}
