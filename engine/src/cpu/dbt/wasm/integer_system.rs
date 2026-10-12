use super::{atomic, locals::*, memory};
use crate::cpu::x86::Register32::*;
use wasm_encoder::{BlockType, InstructionSink};

pub(super) fn cpuid(code: &mut InstructionSink<'_>) {
    const LEAVES: [(u32, [u32; 4]); 5] = [
        (
            0,
            [
                7,
                u32::from_le_bytes(*b"Ring"),
                u32::from_le_bytes(*b"ine "),
                u32::from_le_bytes(*b"3Eng"),
            ],
        ),
        (
            1,
            [
                0x600,
                0,
                1 | 1 << 9 | 1 << 22 | 1 << 23,
                1 | 1 << 4 | 1 << 8 | 1 << 15 | 1 << 23 | 1 << 24 | 1 << 25 | 1 << 26,
            ],
        ),
        (7, [0, 1 << 3 | 1 << 8, 0, 0]),
        (0x8000_0000, [0x8000_0001, 0, 0, 0]),
        (0x8000_0001, [0, 0, 1 << 5, 0]),
    ];
    // capture both query inputs before publishing any output register.
    code.local_get(register(Eax))
        .local_set(RESULT)
        .local_get(register(Ecx))
        .local_set(RHS);
    for r in [Eax, Ebx, Ecx, Edx] {
        code.i32_const(0).local_set(register(r));
    }
    // unknown leaves and structured subleaves are zero by software profile policy.
    for (leaf, values) in LEAVES {
        code.local_get(RESULT).i32_const(leaf as i32).i32_eq();
        if leaf == 7 {
            code.local_get(RHS).i32_eqz().i32_and();
        }
        code.if_(BlockType::Empty);
        for (r, value) in [Eax, Ebx, Ecx, Edx].into_iter().zip(values) {
            code.i32_const(value as i32).local_set(register(r));
        }
        code.end();
    }
}
pub(super) fn timestamp(
    code: &mut InstructionSink<'_>,
    auxiliary: bool,
    imports: memory::Imports,
    exit_depth: u32,
) {
    atomic::payload(code);
    code.i32_const(0).local_set(ADDRESS);
    atomic::call(code, 0x20000 | u32::from(auxiliary), 0, imports, exit_depth);
    for (offset, r) in [(8, Eax), (12, Edx)] {
        code.local_get(STATE_PTR)
            .i32_load(atomic::transfer(offset))
            .local_set(register(r));
    }
    if auxiliary {
        code.local_get(STATE_PTR)
            .i32_load(atomic::transfer(20))
            .local_set(register(Ecx));
    }
}
