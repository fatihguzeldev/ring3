use super::{atomic, locals::*, memory};
use crate::cpu::x86::Register32::*;
use wasm_encoder::{BlockType, InstructionSink};

pub(super) fn cpuid(code: &mut InstructionSink<'_>) {
    code.local_get(register(Eax)).local_set(RESULT);
    for r in [Eax, Ebx, Ecx, Edx] {
        code.i32_const(0).local_set(register(r));
    }
    code.local_get(RESULT).i32_eqz().if_(BlockType::Empty);
    for (r, value) in [
        (Eax, 1),
        (Ebx, u32::from_le_bytes(*b"Ring")),
        (Edx, u32::from_le_bytes(*b"3Eng")),
        (Ecx, u32::from_le_bytes(*b"ine ")),
    ] {
        code.i32_const(value as i32).local_set(register(r));
    }
    code.else_()
        .local_get(RESULT)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    // a software cpu profile: fpu, tsc, cx8, cmov, mmx, fxsr, sse and sse2.
    code.i32_const(0x600)
        .local_set(register(Eax))
        .i32_const(1 | 1 << 4 | 1 << 8 | 1 << 15 | 1 << 23 | 1 << 24 | 1 << 25 | 1 << 26)
        .local_set(register(Edx));
    code.end().end();
}
pub(super) fn timestamp(code: &mut InstructionSink<'_>, imports: memory::Imports, exit_depth: u32) {
    atomic::payload(code);
    code.i32_const(0).local_set(ADDRESS);
    atomic::call(code, 0x20000, 0, imports, exit_depth);
    for (offset, r) in [(8, Eax), (12, Edx)] {
        code.local_get(STATE_PTR)
            .i32_load(atomic::transfer(offset))
            .local_set(register(r));
    }
}
