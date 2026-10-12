use super::{atomic, locals::*, memory};
use crate::cpu::x86::{Register32::*, capabilities::CPU_ID_LEAVES};
use wasm_encoder::{BlockType, InstructionSink};

pub(super) fn cpuid(code: &mut InstructionSink<'_>) {
    // capture both query inputs before publishing any output register.
    code.local_get(register(Eax))
        .local_set(RESULT)
        .local_get(register(Ecx))
        .local_set(RHS);
    for r in [Eax, Ebx, Ecx, Edx] {
        code.i32_const(0).local_set(register(r));
    }
    // unknown leaves and structured subleaves are zero by software profile policy.
    for leaf in CPU_ID_LEAVES {
        code.local_get(RESULT)
            .i32_const(leaf.function as i32)
            .i32_eq();
        if let Some(subfunction) = leaf.subfunction {
            code.local_get(RHS);
            if subfunction == 0 {
                code.i32_eqz();
            } else {
                code.i32_const(subfunction as i32).i32_eq();
            }
            code.i32_and();
        }
        code.if_(BlockType::Empty);
        for (r, value) in [Eax, Ebx, Ecx, Edx].into_iter().zip(leaf.registers) {
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
